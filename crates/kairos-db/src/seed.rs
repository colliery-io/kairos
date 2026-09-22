//! The `seed-demo` fixture (KAIROS-T-0035, per the KAIROS-A-0012 fixtures
//! decision): provision the `demo` tenant with users, teams, boards, and a
//! representative strategy → initiatives → tasks tree so "a running Kairos
//! with known data" is one command — `angreal db seed`, which invokes the
//! `kairos-server seed-demo` subcommand, which calls [`seed_demo`].
//! Integration tests and skills verification consume the same fixture.
//!
//! # What gets seeded
//!
//! - tenant `demo` ("Demo Org") via the real [`crate::provision_tenant`]
//!   path (schema, tenant migrations, default boards, templates, metadata)
//! - users matching the `.angreal/dex/config.yaml` static passwords —
//!   alice (org admin), bob, carol (members) — as JIT-shaped `public.users`
//!   rows whose `external_id` is the REAL Dex `sub` each user presents at
//!   login (see [`DEMO_USERS`]), so first login upserts onto the seeded row
//!   instead of duplicating it
//! - teams `platform` and `web`, each with its per-team delivery board
//!   (`{slug}-delivery`, seeded from `system_board_defaults`), plus team
//!   memberships (alice+bob → platform, carol → web)
//! - delivery stream `customer-portal` linked to both teams
//! - one strategy → two initiatives (`parent` edges); the first initiative
//!   carries a PRD document created from the `prd` system template and
//!   attached with a `supports` edge (initiative → document `parent` is
//!   disallowed by the kairos-core rule matrix)
//! - standing `bugs` and `tech-debt` bucket initiatives
//! - three team-owned repositories (KAIROS-T-0103, A-0019): platform owns
//!   `payments-api` and `platform-infra`, web owns `portal-web`
//! - eleven tasks spread across both delivery boards and several columns,
//!   with two `blocks` edges, including one bug and one tech-debt item
//!   parented to the matching bucket, most bound to a repository, and one
//!   filed CROSS-TEAM by carol (web) into platform's Backlog against
//!   `payments-api`
//! - `priority` metadata stamps on four tasks
//! - two ADRs on the ADR board, the newer superseding the older
//!   (`supersedes` edge; the old one sits in the Superseded column)
//! - team landing content (KAIROS-T-0087): real charter text for both
//!   teams (content saves over the scaffold, so history has a v2), one
//!   announcement per team (platform's pinned), a how-to page under
//!   platform's Documentation, and a runbook document supporting a
//!   platform task (the work-documents panel's demo-visible row)
//!
//! # Idempotency contract (recorded in KAIROS-T-0035)
//!
//! `seed_demo(conn, force=false)` against an existing `demo` tenant returns
//! the typed [`SeedError::AlreadySeeded`] and CHANGES NOTHING — the CLI maps
//! it to a polite exit-0 no-op naming `--force`. With `force=true` the demo
//! tenant is dropped (its `organization_members` rows first — no FK
//! cascade, mirroring the admin delete-tenant handler) and reseeded. The
//! whole run (drop included) is ONE transaction: a failed seed leaves no
//! partial state. Only the `demo` tenant and the three fixture users are
//! ever touched; other tenants and users are invisible to this module.

use chrono::{NaiveDate, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Bool, Text};
use uuid::Uuid;

use crate::boards::{BoardError, create_board};
use crate::graph::{GraphError, link_items};
use crate::items::{
    CreateAdr, CreateDocument, CreateInitiative, CreateStrategy, CreateTask, ItemError, create_adr,
    create_document, create_initiative, create_strategy, create_task,
};
use crate::models::enums::{
    BoardLevel, BucketType, Complexity, OrgRole, RelationshipType, TaskType, TeamPageKind,
    TeamType, WorkClass,
};
use crate::models::public::{NewOrganizationMember, NewUser, User};
use crate::models::team_pages::TeamPage;
use crate::models::teams::{NewDeliveryStream, NewTeam, NewTeamMember, Team};
use crate::models::templates::NewItemMetadata;
use crate::tenant::{TenantError, drop_tenant, provision_tenant};

/// The demo tenant slug (schema `org_demo`, short-code prefix `DEMO`).
pub const DEMO_SLUG: &str = "demo";

/// The demo tenant display name.
pub const DEMO_NAME: &str = "Demo Org";

/// The demo users: `(external_id, email, display_name, org_role)`.
///
/// `external_id` is the exact `sub` claim Dex mints for the static
/// `.angreal/dex/config.yaml` users through the `local` password connector:
/// `base64url(protobuf {1: userID, 2: "local"})`, no padding. Verified
/// against live tokens in KAIROS-T-0035 — so a seeded user's first real
/// login JIT-upserts onto this row (`ON CONFLICT (external_id)`) instead of
/// creating a duplicate identity.
pub const DEMO_USERS: [(&str, &str, &str, OrgRole); 3] = [
    (
        // Dex userID 08a8684b-db88-4b73-90a9-3cd1661f5466
        "CiQwOGE4Njg0Yi1kYjg4LTRiNzMtOTBhOS0zY2QxNjYxZjU0NjYSBWxvY2Fs",
        "alice@kairos.test",
        "alice",
        OrgRole::Admin,
    ),
    (
        // Dex userID 41331323-6f44-45e6-b3b9-2c4b60c02be5
        "CiQ0MTMzMTMyMy02ZjQ0LTQ1ZTYtYjNiOS0yYzRiNjBjMDJiZTUSBWxvY2Fs",
        "bob@kairos.test",
        "bob",
        OrgRole::Member,
    ),
    (
        // Dex userID 7f38a8d0-4c65-4c8d-95a6-2f7f5e2a9c11
        "CiQ3ZjM4YThkMC00YzY1LTRjOGQtOTVhNi0yZjdmNWUyYTljMTESBWxvY2Fs",
        "carol@kairos.test",
        "carol",
        OrgRole::Member,
    ),
];

/// Errors from [`seed_demo`].
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The `demo` tenant already exists and `force` was false. Nothing was
    /// changed; pass `force = true` (CLI: `--force`) to drop and reseed.
    #[error(
        "tenant {DEMO_SLUG:?} is already seeded; pass --force to drop and \
         recreate it (destructive for the demo tenant only)"
    )]
    AlreadySeeded,
    /// Tenant provisioning / teardown failed.
    #[error(transparent)]
    Tenant(#[from] TenantError),
    /// Creating a demo item failed.
    #[error(transparent)]
    Item(#[from] ItemError),
    /// Creating a demo relationship edge failed.
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// Creating a delivery board failed.
    #[error(transparent)]
    Board(#[from] BoardError),
    /// Seeding demo team-page content failed (KAIROS-T-0087).
    #[error(transparent)]
    TeamPage(#[from] crate::team_pages::TeamPageError),
    /// Seeding demo forge connections/links failed (KAIROS-T-0102).
    #[error(transparent)]
    Forge(#[from] crate::forge::ForgeError),
    /// Seeding demo repositories failed (KAIROS-T-0103).
    #[error(transparent)]
    Repository(#[from] crate::repositories::RepositoryError),
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// What [`seed_demo`] created.
#[derive(Debug)]
pub struct SeedDemoReport {
    /// The tenant slug (`demo`).
    pub slug: String,
    /// The tenant schema (`org_demo`).
    pub schema: String,
    /// Whether an existing demo tenant was dropped first (`force`).
    pub recreated: bool,
    /// Users upserted into `public.users` (alice, bob, carol).
    pub users: usize,
    /// Teams created (each with its delivery board).
    pub teams: usize,
    /// Boards that exist after seeding (3 from provisioning + 2 delivery).
    pub boards: usize,
    /// Delivery streams created.
    pub streams: usize,
    /// Strategies created.
    pub strategies: usize,
    /// Initiatives created (2 real + 2 buckets).
    pub initiatives: usize,
    /// Repositories created (KAIROS-T-0103): two for platform, one for web.
    pub repositories: usize,
    /// Tasks created (incl. the bug and tech-debt items and the cross-team
    /// filed one).
    pub tasks: usize,
    /// Documents created (the PRD + the platform runbook).
    pub documents: usize,
    /// ADRs created.
    pub adrs: usize,
    /// Relationship edges created (parent/supports/blocks/supersedes).
    pub edges: usize,
    /// `item_metadata` priority stamps.
    pub metadata_stamps: usize,
    /// Short codes of the seeded strategy, initiatives, tasks, document,
    /// and ADRs, in creation order — handy for demos and assertions.
    pub short_codes: Vec<String>,
}

#[derive(QueryableByName)]
struct BoolRow {
    #[diesel(sql_type = Bool)]
    present: bool,
}

/// Whether the demo tenant exists (org row or schema).
pub fn demo_tenant_exists(conn: &mut PgConnection) -> Result<bool, SeedError> {
    let row: BoolRow = sql_query(
        "SELECT EXISTS (SELECT 1 FROM public.organizations WHERE slug = $1)
             OR EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'org_' || $1) AS present",
    )
    .bind::<Text, _>(DEMO_SLUG)
    .get_result(conn)?;
    Ok(row.present)
}

/// A board column's id by board + name (seeded default columns).
fn column_id(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Result<Uuid, SeedError> {
    use crate::schema::board_columns;
    Ok(board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)?)
}

/// The tenant's board id for a level (unique for strategy/initiative/adr
/// right after provisioning).
fn board_id_of(conn: &mut PgConnection, level: BoardLevel) -> Result<Uuid, SeedError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)?)
}

/// The tenant's metadata definition id by slug (copied from system
/// defaults at provision time).
fn metadata_definition_id(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError> {
    use crate::schema::metadata_definitions;
    Ok(metadata_definitions::table
        .filter(metadata_definitions::slug.eq(slug))
        .select(metadata_definitions::id)
        .first(conn)?)
}

/// The tenant's template id by slug (copied from system defaults).
fn template_id(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError> {
    use crate::schema::templates;
    Ok(templates::table
        .filter(templates::slug.eq(slug))
        .select(templates::id)
        .first(conn)?)
}

/// Upsert the three demo users (keyed on `external_id`, exactly like JIT
/// provisioning) and return their ids in [`DEMO_USERS`] order.
fn upsert_demo_users(conn: &mut PgConnection) -> Result<Vec<Uuid>, SeedError> {
    use crate::schema::users;
    use diesel::upsert::excluded;

    let mut ids = Vec::with_capacity(DEMO_USERS.len());
    for (external_id, email, display_name, _) in DEMO_USERS {
        let user: User = diesel::insert_into(users::table)
            .values(NewUser {
                external_id: external_id.to_string(),
                email: email.to_string(),
                display_name: display_name.to_string(),
            })
            .on_conflict(users::external_id)
            .do_update()
            .set((
                users::email.eq(excluded(users::email)),
                users::display_name.eq(excluded(users::display_name)),
                users::updated_at.eq(diesel::dsl::now),
            ))
            .returning(User::as_returning())
            .get_result(conn)?;
        ids.push(user.id);
    }
    Ok(ids)
}

/// Create a team + its delivery board (the same shape as the API's team
/// creation) and enroll `member_ids`. Returns `(team, delivery_board_id)`.
fn seed_team(
    conn: &mut PgConnection,
    name: &str,
    slug: &str,
    team_type: TeamType,
    member_ids: &[Uuid],
    actor: Uuid,
) -> Result<(Team, Uuid), SeedError> {
    use crate::schema::team_members;

    let team: Team = diesel::insert_into(crate::schema::teams::table)
        .values(NewTeam {
            name: name.to_string(),
            slug: slug.to_string(),
            team_type,
        })
        .returning(Team::as_returning())
        .get_result(conn)?;
    let board = create_board(
        conn,
        BoardLevel::Delivery,
        &format!("{name} Delivery"),
        &format!("{slug}-delivery"),
        Some(team.id),
        Some(actor),
    )?;
    // KAIROS-T-0082: seeded teams get the page scaffold exactly like
    // API-created ones (a team is never born bare).
    crate::team_pages::seed_team_scaffold(conn, team.id, actor)?;
    for user_id in member_ids {
        diesel::insert_into(team_members::table)
            .values(NewTeamMember {
                team_id: team.id,
                user_id: *user_id,
            })
            .execute(conn)?;
    }
    Ok((team, board.id))
}

/// Stamp one `priority` metadata value on an item.
fn stamp_priority(
    conn: &mut PgConnection,
    priority_def: Uuid,
    item_id: Uuid,
    value: &str,
) -> Result<(), SeedError> {
    diesel::insert_into(crate::schema::item_metadata::table)
        .values(NewItemMetadata {
            item_id,
            metadata_definition_id: priority_def,
            value: value.to_string(),
        })
        .execute(conn)?;
    Ok(())
}

/// Seed the demo tenant (see module docs for the full inventory and the
/// idempotency contract). One transaction, including the `force` teardown.
pub fn seed_demo(conn: &mut PgConnection, force: bool) -> Result<SeedDemoReport, SeedError> {
    conn.transaction::<_, SeedError, _>(|conn| {
        use crate::schema::{organization_members, organizations, team_delivery_streams};

        let mut recreated = false;
        if demo_tenant_exists(conn)? {
            if !force {
                return Err(SeedError::AlreadySeeded);
            }
            // No FK cascade from organization_members → organizations:
            // clear memberships first (same order as the admin
            // delete-tenant handler), then drop schema + org row.
            diesel::delete(
                organization_members::table.filter(
                    organization_members::organization_id.eq_any(
                        organizations::table
                            .filter(organizations::slug.eq(DEMO_SLUG))
                            .select(organizations::id),
                    ),
                ),
            )
            .execute(conn)?;
            drop_tenant(conn, DEMO_SLUG, true)?;
            recreated = true;
        }

        // --- tenant + users + memberships (public schema) -------------------
        let report = provision_tenant(conn, DEMO_SLUG, DEMO_NAME)?;
        let user_ids = upsert_demo_users(conn)?;
        let (alice, bob, carol) = (user_ids[0], user_ids[1], user_ids[2]);

        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq(DEMO_SLUG))
            .select(organizations::id)
            .first(conn)?;
        for (user_id, (_, _, _, role)) in user_ids.iter().zip(DEMO_USERS) {
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id: *user_id,
                    role,
                })
                .execute(conn)?;
        }

        // Everything below is tenant-schema work. SET LOCAL reverts when
        // this transaction ends, exactly like provisioning.
        sql_query(format!(
            "SET LOCAL search_path TO \"{}\", public",
            report.schema
        ))
        .execute(conn)?;

        // --- teams + delivery boards + stream --------------------------------
        let (platform, platform_board) = seed_team(
            conn,
            "Platform",
            "platform",
            TeamType::Platform,
            &[alice, bob],
            alice,
        )?;
        let (web, web_board) =
            seed_team(conn, "Web", "web", TeamType::StreamAligned, &[carol], alice)?;

        let stream_id: Uuid = diesel::insert_into(crate::schema::delivery_streams::table)
            .values(NewDeliveryStream {
                name: "Customer Portal".to_string(),
                slug: "customer-portal".to_string(),
                description: Some(
                    "Everything a customer touches between sign-up and invoice.".to_string(),
                ),
            })
            .returning(crate::schema::delivery_streams::id)
            .get_result(conn)?;
        for team_id in [platform.id, web.id] {
            diesel::insert_into(team_delivery_streams::table)
                .values((
                    team_delivery_streams::team_id.eq(team_id),
                    team_delivery_streams::delivery_stream_id.eq(stream_id),
                ))
                .execute(conn)?;
        }

        // --- boards + columns -------------------------------------------------
        let strategy_board = board_id_of(conn, BoardLevel::Strategy)?;
        let initiative_board = board_id_of(conn, BoardLevel::Initiative)?;
        let adr_board = board_id_of(conn, BoardLevel::Adr)?;

        let strategy_active = column_id(conn, strategy_board, "Active")?;
        let initiative_active = column_id(conn, initiative_board, "Active")?;
        let initiative_design = column_id(conn, initiative_board, "Design")?;
        let adr_decided = column_id(conn, adr_board, "Decided")?;
        let adr_superseded = column_id(conn, adr_board, "Superseded")?;

        let mut codes: Vec<String> = Vec::new();
        let mut edges = 0usize;

        // --- strategy → initiatives (+ buckets) -------------------------------
        let strategy = create_strategy(
            conn,
            CreateStrategy {
                board_id: strategy_board,
                column_id: Some(strategy_active),
                title: "Self-serve customer onboarding",
                content: "## Why\n\nEvery new customer today needs a hand-held setup call. \
                          Self-serve onboarding removes the call from the critical path.\n",
                hypothesis: Some(
                    "If a customer can sign up, provision, and see value without talking \
                     to us, activation doubles within two quarters.",
                ),
            },
            alice,
        )?;
        codes.push(strategy.short_code.clone());

        let signup = create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: Some(initiative_active),
                title: "Portal sign-up flow",
                content: "End-to-end self-serve sign-up: form, auth, tenant provisioning, \
                          and the welcome path.",
                complexity: Some(Complexity::M),
                bucket_type: None,
            },
            alice,
        )?;
        codes.push(signup.short_code.clone());

        let billing = create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: Some(initiative_design),
                title: "Billing provider integration",
                content: "Meter usage and invoice through the billing provider; webhooks \
                          drive the invoice lifecycle.",
                complexity: Some(Complexity::L),
                bucket_type: None,
            },
            alice,
        )?;
        codes.push(billing.short_code.clone());

        let bug_bucket = create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: Some(initiative_active),
                title: "Bugs",
                content: "Standing bucket for defect work (KAIROS-A-0001 buckets).",
                complexity: None,
                bucket_type: Some(BucketType::Bug),
            },
            alice,
        )?;
        codes.push(bug_bucket.short_code.clone());

        let debt_bucket = create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: Some(initiative_active),
                title: "Tech Debt",
                content: "Standing bucket for tech-debt work, weighed against its ~20% \
                          allocation at triage.",
                complexity: None,
                bucket_type: Some(BucketType::TechDebt),
            },
            alice,
        )?;
        codes.push(debt_bucket.short_code.clone());

        for child in [signup.id, billing.id] {
            link_items(conn, strategy.id, child, RelationshipType::Parent, alice)?;
            edges += 1;
        }

        // --- the PRD document (from the prd template, supports edge) ----------
        let prd_template = template_id(conn, "prd")?;
        let prd = create_document(
            conn,
            CreateDocument {
                title: "PRD: Portal sign-up flow",
                content: None, // copy the template skeleton
                template_id: Some(prd_template),
            },
            alice,
        )?;
        codes.push(prd.short_code.clone());
        link_items(conn, signup.id, prd.id, RelationshipType::Supports, alice)?;
        edges += 1;

        // --- repositories (KAIROS-T-0103, A-0019) ----------------------------
        // Team-owned, created BEFORE tasks so tasks can bind to them: two for
        // platform (one with a webhook, one plain), one for web. Fictional —
        // nothing here reaches the network.
        use crate::models::enums::Forge;
        use crate::models::repositories::NewRepository;
        let new_repo = |slug: &str, forge: Forge, name: &str, url: &str, team: Uuid, desc: &str| {
            NewRepository {
                slug: slug.to_string(),
                forge,
                repo_full_name: name.to_string(),
                repo_url: url.to_string(),
                default_branch: "main".to_string(),
                team_id: team,
                description: desc.to_string(),
                created_by: alice,
                updated_by: alice,
            }
        };
        let payments_repo = crate::repositories::create(
            conn,
            new_repo(
                "payments-api",
                Forge::Github,
                "acme/payments-api",
                "https://github.com/acme/payments-api",
                platform.id,
                "Rust/axum service. `cargo test` before every PR; migrations via diesel.",
            ),
        )?;
        let infra_repo = crate::repositories::create(
            conn,
            new_repo(
                "platform-infra",
                Forge::Github,
                "acme/platform-infra",
                "https://github.com/acme/platform-infra",
                platform.id,
                "Terraform + Helm. Plan output in every PR description.",
            ),
        )?;
        let portal_repo = crate::repositories::create(
            conn,
            new_repo(
                "portal-web",
                Forge::Gitlab,
                "acme/portal-web",
                "https://gitlab.com/acme/portal-web",
                web.id,
                "Leptos front end. `trunk build` must pass; no hardcoded colors.",
            ),
        )?;
        let repositories = 3usize;

        // --- tasks across both delivery boards --------------------------------
        struct SeedTask<'a> {
            board: Uuid,
            column: &'a str,
            title: &'a str,
            content: &'a str,
            task_type: TaskType,
            /// KAIROS-T-0077 lane axis: planned work vs unplanned intake.
            work_class: WorkClass,
            team: Uuid,
            /// KAIROS-T-0103 (A-0019): the repository the task is issued against.
            repo: Option<Uuid>,
            parent: Uuid,
            actor: Uuid,
            priority: Option<&'a str>,
        }
        let plan = [
            SeedTask {
                board: web_board,
                column: "Completed",
                title: "Sign-up form UI skeleton",
                content: "Static form with client-side validation; no backend wiring yet.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: web.id,
                repo: Some(portal_repo.id),
                parent: signup.id,
                actor: carol,
                priority: None,
            },
            SeedTask {
                board: platform_board,
                column: "Active",
                title: "Password-less email auth",
                content: "Magic-link issue + verify endpoints behind the OIDC issuer.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: platform.id,
                repo: Some(payments_repo.id),
                parent: signup.id,
                actor: alice,
                priority: Some("high"),
            },
            SeedTask {
                board: platform_board,
                column: "Todo",
                title: "Provision tenant on first login",
                content: "First verified login provisions the org schema and seeds defaults.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: platform.id,
                repo: Some(payments_repo.id),
                parent: signup.id,
                actor: alice,
                priority: None,
            },
            SeedTask {
                board: web_board,
                column: "Backlog",
                title: "Welcome-email trigger",
                content: "Send the welcome sequence once provisioning completes.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: web.id,
                repo: Some(portal_repo.id),
                parent: signup.id,
                actor: bob,
                priority: Some("low"),
            },
            SeedTask {
                board: platform_board,
                column: "Todo",
                title: "Billing provider spike",
                content: "Prototype the metering API against a sandbox account.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: platform.id,
                repo: Some(infra_repo.id),
                parent: billing.id,
                actor: bob,
                priority: Some("medium"),
            },
            SeedTask {
                board: platform_board,
                column: "Backlog",
                title: "Invoice webhook handler",
                content: "Consume invoice.created/paid webhooks; reconcile invoice state.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: platform.id,
                repo: Some(payments_repo.id),
                parent: billing.id,
                actor: bob,
                priority: None,
            },
            SeedTask {
                board: web_board,
                column: "Todo",
                title: "Sign-up form drops UTF-8 names",
                content: "Names with combining characters are truncated at submit. \
                          Repro: sign up as \"Ana Müller-Sørensen\".",
                task_type: TaskType::Bug,
                work_class: WorkClass::Planned,
                team: web.id,
                repo: Some(portal_repo.id),
                parent: bug_bucket.id,
                actor: carol,
                priority: Some("critical"),
            },
            SeedTask {
                board: web_board,
                column: "Backlog",
                title: "Extract shared form-validation helpers",
                content: "Sign-up and settings forms duplicate validation; extract one module.",
                task_type: TaskType::TechDebt,
                work_class: WorkClass::Planned,
                team: web.id,
                repo: None,
                parent: debt_bucket.id,
                actor: carol,
                priority: None,
            },
            // KAIROS-T-0077: the Support lane in action — a support
            // request (type AND lane 'support') …
            SeedTask {
                board: platform_board,
                column: "Active",
                title: "Customer cannot reset password",
                content: "Support intake: reset emails never arrive for one tenant. \
                          Working the incident with the customer.",
                task_type: TaskType::Support,
                work_class: WorkClass::Support,
                team: platform.id,
                repo: None,
                parent: signup.id,
                actor: alice,
                priority: Some("critical"),
            },
            // … and the recorded no-bug-lane decision: an UNPLANNED bug
            // keeps task_type 'bug' and sits in the Support lane.
            SeedTask {
                board: platform_board,
                column: "Todo",
                title: "Login page 500s on expired trials",
                content: "Unplanned: expired-trial orgs hit a 500 on login instead of \
                          the renewal prompt.",
                task_type: TaskType::Bug,
                work_class: WorkClass::Support,
                team: platform.id,
                repo: Some(payments_repo.id),
                parent: bug_bucket.id,
                actor: bob,
                priority: Some("high"),
            },
            // KAIROS-T-0103 (A-0019 §4): the cross-team fixture. Carol (web,
            // NOT a platform member) files work against platform's
            // payments-api; it lands in platform's Backlog behind their
            // triage gate. The e2e (KAIROS-T-0110) asserts on this row.
            SeedTask {
                board: platform_board,
                column: "Backlog",
                title: "Portal needs a bulk invoice export endpoint",
                content: "Filed from portal-web: the billing page wants \
                          GET /invoices/export?from=&to= returning CSV. \
                          Blocks the portal export button.",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team: platform.id,
                repo: Some(payments_repo.id),
                parent: billing.id,
                actor: carol,
                priority: None,
            },
        ];

        let priority_def = metadata_definition_id(conn, "priority")?;
        let mut task_ids: Vec<Uuid> = Vec::with_capacity(plan.len());
        let mut metadata_stamps = 0usize;
        for spec in &plan {
            let task_column = column_id(conn, spec.board, spec.column)?;
            let task = create_task(
                conn,
                CreateTask {
                    board_id: spec.board,
                    column_id: Some(task_column),
                    title: spec.title,
                    content: spec.content,
                    task_type: spec.task_type,
                    work_class: spec.work_class,
                    team_id: Some(spec.team),
                    repository_id: spec.repo,
                },
                spec.actor,
            )?;
            link_items(conn, spec.parent, task.id, RelationshipType::Parent, alice)?;
            edges += 1;
            if let Some(priority) = spec.priority {
                stamp_priority(conn, priority_def, task.id, priority)?;
                metadata_stamps += 1;
            }
            codes.push(task.short_code.clone());
            task_ids.push(task.id);
        }
        // Blocks edges: auth blocks provisioning; the spike blocks the
        // webhook handler.
        link_items(
            conn,
            task_ids[1],
            task_ids[2],
            RelationshipType::Blocks,
            alice,
        )?;
        link_items(
            conn,
            task_ids[4],
            task_ids[5],
            RelationshipType::Blocks,
            alice,
        )?;
        edges += 2;

        // --- ADRs (supersedes chain) ------------------------------------------
        let old_adr = create_adr(
            conn,
            CreateAdr {
                board_id: Some(adr_board),
                column_id: Some(adr_superseded),
                title: "Session storage: server-side sessions",
                content: "## Decision\n\nStore sessions server-side in Postgres.\n\n\
                          Superseded: stateless tokens won once the portal went multi-node.",
                decision_maker: Some("alice"),
                decision_date: NaiveDate::from_ymd_opt(2026, 5, 12),
            },
            alice,
        )?;
        codes.push(old_adr.short_code.clone());
        let new_adr = create_adr(
            conn,
            CreateAdr {
                board_id: Some(adr_board),
                column_id: Some(adr_decided),
                title: "Session storage: short-lived JWT access tokens",
                content: "## Decision\n\nIssue short-lived JWTs with refresh at the issuer; \
                          no server-side session table.",
                decision_maker: Some("alice"),
                decision_date: NaiveDate::from_ymd_opt(2026, 6, 30),
            },
            alice,
        )?;
        codes.push(new_adr.short_code.clone());
        link_items(
            conn,
            new_adr.id,
            old_adr.id,
            RelationshipType::Supersedes,
            alice,
        )?;
        edges += 1;

        // --- team landing content (KAIROS-T-0087) -----------------------------
        // Real charters (a content save over the scaffold skeleton, so the
        // history chain looks lived-in), one announcement per team (the
        // platform one pinned), a how-to page under platform's
        // Documentation, and a runbook document supporting a platform TASK
        // so the work-documents panel has a demo-visible row (the PRD
        // supports the org-level sign-up initiative and correctly does not
        // appear there).
        let charter_of = |conn: &mut PgConnection, team: Uuid| -> Result<TeamPage, SeedError> {
            use crate::schema::team_pages as tp;
            Ok(tp::table
                .filter(tp::team_id.eq(team))
                .filter(tp::parent_id.is_null())
                .filter(tp::slug.eq("charter"))
                .select(TeamPage::as_select())
                .first(conn)?)
        };
        let platform_charter = charter_of(conn, platform.id)?;
        crate::team_pages::update_page_content(
            conn,
            platform.id,
            platform_charter.id,
            None,
            "# Team Charter\n\n## Mission\n\nOwn the paved road: tenancy, auth, and the \
             delivery pipeline every other team builds on.\n\n## Scope\n\n- Tenant \
             provisioning and isolation\n- Sign-in and session tokens\n- CI/CD and the \
             deploy runway\n\n## Working agreements\n\n- Support intake before new \
             work\n- Every incident gets a runbook page",
            platform_charter.version,
            alice,
        )?;
        let web_charter = charter_of(conn, web.id)?;
        crate::team_pages::update_page_content(
            conn,
            web.id,
            web_charter.id,
            None,
            "# Team Charter\n\n## Mission\n\nShip the customer-facing portal: every \
             screen between sign-up and invoice.\n\n## Scope\n\n- Sign-up and onboarding \
             flows\n- Portal UI and accessibility\n\n## Working agreements\n\n- Demo \
             every Friday\n- Design review before build",
            web_charter.version,
            carol,
        )?;

        let how_to_folder: Uuid = {
            use crate::schema::team_pages as tp;
            let docs_folder: Uuid = tp::table
                .filter(tp::team_id.eq(platform.id))
                .filter(tp::parent_id.is_null())
                .filter(tp::slug.eq("documentation"))
                .select(tp::id)
                .first(conn)?;
            tp::table
                .filter(tp::team_id.eq(platform.id))
                .filter(tp::parent_id.eq(docs_folder))
                .filter(tp::slug.eq("how-to-guides"))
                .select(tp::id)
                .first(conn)?
        };
        crate::team_pages::create_page(
            conn,
            platform.id,
            crate::team_pages::CreatePage {
                parent_id: Some(how_to_folder),
                kind: TeamPageKind::Page,
                slug: "deploy-kairos",
                title: "Deploy Kairos",
                content: "# Deploy Kairos\n\n1. `angreal services up`\n2. `angreal db \
                          migrate && angreal db seed --force`\n3. Start the server and \
                          smoke `/api/whoami`\n\nRollbacks: redeploy the previous tag; \
                          migrations are guarded and idempotent.",
                position: 0,
            },
            bob,
        )?;

        use crate::models::team_pages::NewTeamAnnouncement;
        diesel::insert_into(crate::schema::team_announcements::table)
            .values(NewTeamAnnouncement {
                team_id: platform.id,
                body: "Welcome to the Platform team space — start with the charter, \
                       and file support intake before planned work."
                    .to_string(),
                pinned: true,
                created_by: alice,
            })
            .execute(conn)?;
        diesel::insert_into(crate::schema::team_announcements::table)
            .values(NewTeamAnnouncement {
                team_id: web.id,
                body: "Sprint demo Friday 14:00 — portal sign-up flow, end to end.".to_string(),
                pinned: false,
                created_by: carol,
            })
            .execute(conn)?;

        let runbook = create_document(
            conn,
            CreateDocument {
                title: "Runbook: password-less auth rollout",
                content: Some(
                    "## Purpose\n\nOperating notes for the password-less email auth \
                     rollout.\n\n## Checks\n\n- Magic-link emails delivered < 30s\n- \
                     Token TTL 15 minutes\n\n## Rollback\n\nFlip `AUTH_MAGIC_LINKS` \
                     off; password auth remains available.",
                ),
                template_id: None,
            },
            alice,
        )?;
        codes.push(runbook.short_code.clone());
        link_items(
            conn,
            task_ids[1],
            runbook.id,
            RelationshipType::Supports,
            alice,
        )?;
        edges += 1;

        // --- forge connections + links (KAIROS-T-0102) -----------------------
        // Webhook wiring on one repository per team (platform-infra stays
        // unconnected on purpose: a repo without webhooks is a valid state),
        // and links covering every state the panels render: an open PR, a
        // draft, a merged one, and a branch.
        use crate::models::enums::{LinkKind, LinkState};
        use crate::models::forge::{NewForgeConnection, NewItemLink};
        let platform_repo = crate::forge::create_connection(
            conn,
            NewForgeConnection {
                forge: Forge::Github,
                repository_id: payments_repo.id,
                created_by: alice,
            },
        )?;
        let web_repo = crate::forge::create_connection(
            conn,
            NewForgeConnection {
                forge: Forge::Gitlab,
                repository_id: portal_repo.id,
                created_by: alice,
            },
        )?;

        // (connection, item, kind, external id, title, state, author)
        let seeded_links: [(Uuid, Uuid, LinkKind, &str, &str, LinkState, &str); 4] = [
            (
                platform_repo.id,
                task_ids[1],
                LinkKind::PullRequest,
                "42",
                "Password-less email auth",
                LinkState::Open,
                "bob",
            ),
            (
                platform_repo.id,
                task_ids[1],
                LinkKind::Branch,
                "bob/passwordless-auth",
                "bob/passwordless-auth",
                LinkState::Open,
                "bob",
            ),
            (
                platform_repo.id,
                task_ids[2],
                LinkKind::PullRequest,
                "43",
                "Provision tenant on first login",
                LinkState::Merged,
                "alice",
            ),
            (
                web_repo.id,
                task_ids[0],
                LinkKind::PullRequest,
                "7",
                "Draft: sign-up form skeleton",
                LinkState::Draft,
                "carol",
            ),
        ];
        let mut links = 0usize;
        for (connection_id, item_id, kind, external_id, title, state, author) in seeded_links {
            let repo = if connection_id == platform_repo.id {
                ("github.com", "acme/payments-api", "pull")
            } else {
                ("gitlab.com", "acme/portal-web", "-/merge_requests")
            };
            let url = match kind {
                LinkKind::PullRequest => {
                    format!("https://{}/{}/{}/{external_id}", repo.0, repo.1, repo.2)
                }
                LinkKind::Branch => {
                    format!("https://{}/{}/tree/{external_id}", repo.0, repo.1)
                }
            };
            crate::forge::upsert_link(
                conn,
                NewItemLink {
                    item_id,
                    connection_id,
                    kind,
                    external_id: external_id.to_string(),
                    title: title.to_string(),
                    url,
                    state,
                    author: author.to_string(),
                    forge_updated_at: Utc::now(),
                },
            )?;
            links += 1;
        }
        debug_assert_eq!(links, 4);

        Ok(SeedDemoReport {
            slug: DEMO_SLUG.to_string(),
            schema: report.schema,
            recreated,
            users: user_ids.len(),
            teams: 2,
            boards: report.boards_created.len() + 2,
            streams: 1,
            strategies: 1,
            initiatives: 4,
            repositories,
            tasks: plan.len(),
            documents: 2,
            adrs: 2,
            edges,
            metadata_stamps,
            short_codes: codes,
        })
    })
}
