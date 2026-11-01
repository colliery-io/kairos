//! Integration test for the ABAC capability service (KAIROS-T-0011,
//! contract per KAIROS-A-0006, layering per KAIROS-A-0009).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration`); the database is never mocked
//! (KAIROS-A-0012). For isolation the test drops and recreates a dedicated
//! scratch database (`kairos_abac_test`) on the same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - `check_capability` truth table against the real SQL: exact match,
//!   `manage_*`, `*`, non-matches, whitelist default-deny, and stored
//!   capabilities containing literal `%`/`_` NOT acting as SQL wildcards
//! - grant/revoke flows: `activity_log` rows
//!   (`capability_grant`/`capability_revoke`), the composite-PK UNIQUE
//!   constraint surfacing as typed `AlreadyGranted`, revoke of a missing
//!   grant as typed `GrantNotFound`, and revoke actually removing access
//! - org-admin bypass via `authorize` (admin passes with zero grants;
//!   plain member denied)
//! - document authorization resolving through the `supports` edge to the
//!   parent initiative's board; board items resolving to their own board;
//!   off-board ADRs, orphan documents, and unknown ids resolving to None

#[path = "common/scratch.rs"]
mod scratch;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::abac as rules;
use kairos_core::short_code::ItemType;
use kairos_db::abac::{self, AbacError};
use kairos_db::models::{
    ActivityAction, NewAdr, NewDocument, NewInitiative, NewItemRelationship, NewOrganizationMember,
    NewStrategy, NewUser, OrgRole, RelationshipType, User,
};
use kairos_db::{provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_abac_test");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

fn insert_user(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid {
    diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: external_id.into(),
            user_name: external_id.into(),
            email: email.into(),
            display_name: name.into(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

/// The board with this slug in the current tenant schema.
fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::boards::table
        .filter(schema::boards::slug.eq(slug))
        .select(schema::boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

/// The first column of a board (position order) — items need a placement.
fn first_column(conn: &mut PgConnection, board: Uuid) -> Uuid {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .order(schema::board_columns::position.asc())
        .select(schema::board_columns::id)
        .first(conn)
        .expect("board has columns")
}

/// All `activity_log.details` values for (action, entity_id), in order.
fn activity_details(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq(action.as_str()))
        .filter(schema::activity_log::entity_id.eq(entity))
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading activity_log")
}

/// Assert the SQL check agrees with the pure core matcher for one stored
/// grant — the two layers implement ONE contract (KAIROS-A-0006).
///
/// The matcher is about grants, so this asks `check_granted_capability`:
/// on the strategy and initiative boards, `check_capability` also counts
/// membership of the board (KAIROS-T-0410), which the
/// `board_membership_gives_edit_and_move_on_strategy_and_initiative_boards`
/// test covers.
fn assert_check(
    conn: &mut PgConnection,
    board: Uuid,
    user: Uuid,
    granted: &str,
    required: &str,
    expected: bool,
) {
    let sql = abac::check_granted_capability(conn, board, user, required).expect("check");
    assert_eq!(
        sql, expected,
        "SQL check: grant {granted:?}, required {required:?}"
    );
    let pure = rules::is_authorized(&[granted.to_string()], required);
    assert_eq!(
        pure, expected,
        "core matcher disagrees with SQL for grant {granted:?}, required {required:?}"
    );
}

/// The number of queries that `f` sends on `conn` (KAIROS-T-0625): diesel
/// instrumentation counts each `StartQuery`.
fn count_queries<T>(conn: &mut PgConnection, f: impl FnOnce(&mut PgConnection) -> T) -> (T, usize) {
    use diesel::connection::InstrumentationEvent;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    conn.set_instrumentation(move |event: InstrumentationEvent<'_>| {
        if matches!(event, InstrumentationEvent::StartQuery { .. }) {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    let out = f(conn);
    conn.set_instrumentation(|_: InstrumentationEvent<'_>| {});
    (out, count.load(Ordering::SeqCst))
}

#[test]
fn abac_capability_lifecycle() {
    let admin_url = admin_database_url();

    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);

    let scratch_url = with_database(&admin_url, &SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    // Fresh tenant, then work inside its schema (same mechanism as the
    // pool: unqualified tenant tables resolve via search_path).
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // ---- seed users + org membership ---------------------------------------
    let org_id: Uuid = schema::organizations::table
        .filter(schema::organizations::slug.eq("acme"))
        .select(schema::organizations::id)
        .first(&mut conn)
        .expect("provisioned org row");

    let org_admin = insert_user(&mut conn, "dex|admin", "admin@acme.test", "Org Admin");
    let manager = insert_user(&mut conn, "dex|manager", "manager@acme.test", "Manager");
    let helper = insert_user(&mut conn, "dex|helper", "helper@acme.test", "Helper");
    let superuser = insert_user(&mut conn, "dex|super", "super@acme.test", "Board Admin");
    let hostile = insert_user(&mut conn, "dex|hostile", "hostile@acme.test", "Hostile");

    diesel::insert_into(schema::organization_members::table)
        .values(&[
            NewOrganizationMember {
                organization_id: org_id,
                user_id: org_admin,
                role: OrgRole::Admin,
            },
            NewOrganizationMember {
                organization_id: org_id,
                user_id: manager,
                role: OrgRole::Member,
            },
        ])
        .execute(&mut conn)
        .expect("inserting org members");

    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let strategy_board = board_id_by_slug(&mut conn, "strategy");

    // ---- whitelist default: no grants, no access ----------------------------
    assert!(
        !abac::check_capability(&mut conn, initiative_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "whitelist: a user with no grants has no write access"
    );

    // ---- grant flow: activity row + composite-PK UNIQUE ----------------------
    abac::grant_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect("granting manage_tasks");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::CapabilityGrant, initiative_board),
        [format!("capability:manage_tasks user:{manager}")],
        "grant writes a capability_grant activity row"
    );

    // Duplicate grant: composite PK honored, typed error, no extra audit row.
    let err = abac::grant_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect_err("duplicate grant must fail");
    assert!(
        matches!(
            &err,
            AbacError::AlreadyGranted { board_id, user_id, capability }
                if *board_id == initiative_board && *user_id == manager && capability == "manage_tasks"
        ),
        "expected AlreadyGranted, got {err:?}"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::CapabilityGrant, initiative_board).len(),
        1,
        "the rejected duplicate grant wrote no activity row"
    );

    // Empty capability is rejected before touching the database.
    let err = abac::grant_capability(&mut conn, initiative_board, manager, "", org_admin)
        .expect_err("empty capability must fail");
    assert!(
        matches!(err, AbacError::EmptyCapability),
        "expected EmptyCapability, got {err:?}"
    );

    // ---- check_capability truth table against the real SQL -------------------
    // Exact grant: matches itself, nothing else, and is board-scoped.
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::MANAGE_TASKS,
        true,
    );
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::MANAGE_DOCUMENTS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::TRANSITION_ITEMS,
        false,
    );
    // '_' in the STORED grant is escaped in the SQL: it does not act as the
    // LIKE single-char wildcard ('managextasks' must NOT match).
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        "managextasks",
        false,
    );
    assert!(
        !abac::check_capability(&mut conn, strategy_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "grants are board-scoped: no access on another board"
    );

    // manage_* glob: all manage capabilities, not workflow/config ones.
    abac::grant_capability(
        &mut conn,
        initiative_board,
        helper,
        rules::GLOB_MANAGE,
        org_admin,
    )
    .expect("granting manage_*");
    for required in [
        rules::MANAGE_STRATEGIES,
        rules::MANAGE_INITIATIVES,
        rules::MANAGE_TASKS,
        rules::MANAGE_DOCUMENTS,
        rules::MANAGE_ADRS,
    ] {
        assert_check(
            &mut conn,
            initiative_board,
            helper,
            "manage_*",
            required,
            true,
        );
    }
    // KAIROS-T-0183: board administration is NOT in the manage_* family, and
    // this is the assertion that matters most — it checks the SQL and the pure
    // matcher agree about it. While the capability was called `manage_members`
    // this loop expected `true`, in both layers, which is why the accidental
    // privilege looked deliberate: it was consistently implemented and
    // consistently tested.
    assert_check(
        &mut conn,
        initiative_board,
        helper,
        "manage_*",
        rules::ADMINISTER_MEMBERS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        helper,
        "manage_*",
        rules::TRANSITION_ITEMS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        helper,
        "manage_*",
        rules::CONFIGURE_BOARDS,
        false,
    );

    // '*' grant: everything on the board.
    abac::grant_capability(
        &mut conn,
        initiative_board,
        superuser,
        rules::GLOB_ALL,
        org_admin,
    )
    .expect("granting *");
    for required in rules::CAPABILITIES {
        assert_check(&mut conn, initiative_board, superuser, "*", required, true);
    }

    // A STORED capability containing a literal '%' must NOT wildcard in SQL.
    abac::grant_capability(&mut conn, initiative_board, hostile, "manage%", org_admin)
        .expect("granting hostile literal-% capability");
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        rules::MANAGE_TASKS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        "manageX",
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        "manage%",
        true,
    );

    // ---- org-admin bypass -----------------------------------------------------
    assert!(
        abac::is_org_admin(&mut conn, "acme", org_admin).expect("is_org_admin"),
        "role='admin' member is an org admin"
    );
    assert!(
        !abac::is_org_admin(&mut conn, "acme", manager).expect("is_org_admin"),
        "role='member' is not an org admin"
    );
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            strategy_board,
            org_admin,
            rules::CONFIGURE_BOARDS
        )
        .expect("authorize"),
        "org admin bypasses the whitelist with zero grants"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            strategy_board,
            manager,
            rules::CONFIGURE_BOARDS
        )
        .expect("authorize"),
        "plain member without grants is denied on this board"
    );
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            initiative_board,
            manager,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "authorize falls through to the capability check for non-admins"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "nonexistent",
            initiative_board,
            org_admin,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "an unknown org slug grants nothing"
    );

    // ---- a document authorizes against its owner board (COLLIERY-T-3109) -------
    let discovery = first_column(&mut conn, initiative_board);
    let initiative_id: Uuid = diesel::insert_into(schema::initiatives::table)
        .values(NewInitiative {
            short_code: "I-0001".into(),
            title: "ABAC".into(),
            content: "".into(),
            board_id: initiative_board,
            column_id: discovery,
            complexity: None,
            is_bucket: false,
            bucket_type: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::initiatives::id)
        .get_result(&mut conn)
        .expect("inserting initiative");

    let document_id: Uuid = diesel::insert_into(schema::documents::table)
        .values(NewDocument {
            short_code: "D-0001".into(),
            title: "PRD".into(),
            content: "".into(),
            template_id: None,
            board_id: initiative_board,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::documents::id)
        .get_result(&mut conn)
        .expect("inserting document");

    // With no edge, the document resolves to its owner board.
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, document_id).expect("resolve"),
        Some(initiative_board),
        "a document with no supports edge resolves to its owner board"
    );

    // S-0004 edge semantics: target supports source (document supports
    // initiative) — source = initiative (parent), target = document.
    diesel::insert_into(schema::item_relationships::table)
        .values(NewItemRelationship {
            source_id: initiative_id,
            target_id: document_id,
            relationship: RelationshipType::Supports,
        })
        .execute(&mut conn)
        .expect("inserting supports edge");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, document_id).expect("resolve"),
        Some(initiative_board),
        "a supports edge does not change the owner board of a document"
    );
    // End-to-end per A-0006: editing the document requires manage_documents
    // on its owner board — helper's manage_* covers it, manager's
    // manage_tasks does not.
    let doc_board = abac::resolve_authorization_board(&mut conn, document_id)
        .expect("resolve")
        .expect("document has a board");
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            doc_board,
            helper,
            rules::MANAGE_DOCUMENTS
        )
        .expect("authorize"),
        "helper's manage_* on the parent board authorizes editing the document"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            doc_board,
            manager,
            rules::MANAGE_DOCUMENTS
        )
        .expect("authorize"),
        "manager's manage_tasks does not authorize editing the document"
    );

    // Board items resolve to their own board.
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, initiative_id).expect("resolve"),
        Some(initiative_board)
    );
    let strategy_col = first_column(&mut conn, strategy_board);
    let strategy_id: Uuid = diesel::insert_into(schema::strategies::table)
        .values(NewStrategy {
            short_code: "S-0001".into(),
            title: "Strategy".into(),
            content: "".into(),
            board_id: strategy_board,
            column_id: strategy_col,
            hypothesis: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::strategies::id)
        .get_result(&mut conn)
        .expect("inserting strategy");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, strategy_id).expect("resolve"),
        Some(strategy_board)
    );

    // An ADR not placed on a board has no board context; unknown ids too.
    let off_board_adr: Uuid = diesel::insert_into(schema::adrs::table)
        .values(NewAdr {
            short_code: "A-0001".into(),
            title: "Off board".into(),
            content: "".into(),
            board_id: None,
            column_id: None,
            decision_maker: None,
            decision_date: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::adrs::id)
        .get_result(&mut conn)
        .expect("inserting off-board adr");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, off_board_adr).expect("resolve"),
        None
    );
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, Uuid::new_v4()).expect("resolve"),
        None
    );

    // KAIROS-T-0625: the one query of the edit rule gives what the probes
    // of each table gave, for each type, and nothing for an id of a
    // different type or an unknown id.
    for (item_type, id) in [
        (ItemType::Document, document_id),
        (ItemType::Initiative, initiative_id),
        (ItemType::Strategy, strategy_id),
        (ItemType::Adr, off_board_adr),
    ] {
        let creator = item_created_by(&mut conn, id).expect("created_by");
        let board = abac::resolve_authorization_board(&mut conn, id).expect("resolve");
        assert_eq!(
            abac::creator_and_board(&mut conn, item_type, id).expect("creator and board"),
            creator.map(|creator| (creator, board)),
            "{item_type:?}: one query gives the creator and the board"
        );
    }
    assert_eq!(
        abac::creator_and_board(&mut conn, ItemType::Task, document_id).expect("wrong type"),
        None,
        "an id of a different type is not found"
    );
    assert_eq!(
        abac::creator_and_board(&mut conn, ItemType::Document, Uuid::new_v4()).expect("unknown"),
        None
    );

    // ---- revoke flow: removes access, writes activity, typed on missing -------
    abac::revoke_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect("revoking manage_tasks");
    assert!(
        !abac::check_capability(&mut conn, initiative_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "revoke removes access"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            initiative_board,
            manager,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "authorize denies after revoke"
    );
    assert_eq!(
        activity_details(
            &mut conn,
            ActivityAction::CapabilityRevoke,
            initiative_board
        ),
        [format!("capability:manage_tasks user:{manager}")],
        "revoke writes a capability_revoke activity row"
    );

    let err = abac::revoke_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect_err("revoking a missing grant must fail");
    assert!(
        matches!(
            &err,
            AbacError::GrantNotFound { board_id, user_id, capability }
                if *board_id == initiative_board && *user_id == manager && capability == "manage_tasks"
        ),
        "expected GrantNotFound, got {err:?}"
    );
    assert_eq!(
        activity_details(
            &mut conn,
            ActivityAction::CapabilityRevoke,
            initiative_board
        )
        .len(),
        1,
        "the rejected revoke wrote no activity row"
    );

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}

/// KAIROS-T-0072 (A-0006 amendment): membership of a board's owning team
/// implies the delivery capability set — and ONLY that set, ONLY on that
/// team's boards. Own scratch database: cargo runs test functions in
/// parallel threads and the lifecycle test drops/recreates its own.
#[test]
fn team_membership_implies_delivery_capabilities() {
    use kairos_db::models::{BoardLevel, NewBoard, NewTeam, NewTeamMember, TeamType};

    static TEAM_SCRATCH_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_abac_team_test");

    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {TEAM_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {TEAM_SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&TEAM_SCRATCH_DB);

    let scratch_url = with_database(&admin_url, &TEAM_SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "teamco", "Teamco").expect("provisioning teamco");
    sql_query("SET search_path TO org_teamco, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let member = insert_user(&mut conn, "dex|member", "member@teamco.test", "Member");
    let outsider = insert_user(
        &mut conn,
        "dex|outsider",
        "outsider@teamco.test",
        "Outsider",
    );

    // A team, its member, and its delivery board.
    let team_id: Uuid = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: TeamType::Platform,
        })
        .returning(schema::teams::id)
        .get_result(&mut conn)
        .expect("inserting team");
    diesel::insert_into(schema::team_members::table)
        .values(NewTeamMember {
            team_id,
            user_id: member,
        })
        .execute(&mut conn)
        .expect("inserting team member");
    let team_board: Uuid = diesel::insert_into(schema::boards::table)
        .values(NewBoard {
            name: "Platform Delivery".into(),
            slug: "platform-delivery".into(),
            board_level: BoardLevel::Delivery,
            team_id: Some(team_id),
            // A prefix of its own (COLLIERY-T-3099).
            code_prefix: format!("B{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
                .to_uppercase(),
        })
        .returning(schema::boards::id)
        .get_result(&mut conn)
        .expect("inserting team board");
    // A board no team owns (provisioned default).
    let orgwide_board = board_id_by_slug(&mut conn, "strategy");

    let check = |conn: &mut PgConnection, board, user, required: &str| {
        abac::check_capability(conn, board, user, required).expect("check_capability")
    };

    // The implied set, on the team's own board.
    for implied in rules::TEAM_IMPLIED_CAPABILITIES {
        assert!(
            check(&mut conn, team_board, member, implied),
            "team member should hold implied {implied:?} on the team board"
        );
        assert!(
            rules::team_implies(implied),
            "core team_implies must agree for {implied:?}"
        );
    }

    // Nothing configuration- or membership-shaped is implied.
    for withheld in [
        rules::CONFIGURE_BOARDS,
        rules::ADMINISTER_MEMBERS,
        rules::MANAGE_STRATEGIES,
        rules::MANAGE_INITIATIVES,
        rules::MANAGE_ADRS,
    ] {
        assert!(
            !check(&mut conn, team_board, member, withheld),
            "team membership must NOT imply {withheld:?}"
        );
    }

    // Non-members get nothing from the team arm.
    assert!(
        !check(&mut conn, team_board, outsider, rules::TRANSITION_ITEMS),
        "an outsider has no implied capability on the team board"
    );
    // Team boards only: no implication on org-wide (teamless) boards.
    assert!(
        !check(&mut conn, orgwide_board, member, rules::MANAGE_TASKS),
        "team membership implies nothing on boards without an owning team"
    );

    // Explicit grants keep working, independent of teams.
    abac::grant_capability(
        &mut conn,
        team_board,
        outsider,
        rules::TRANSITION_ITEMS,
        member,
    )
    .expect("granting explicit capability");
    assert!(
        check(&mut conn, team_board, outsider, rules::TRANSITION_ITEMS),
        "explicit grants still authorize non-members"
    );

    drop(conn);
    sql_query(format!(
        "DROP DATABASE IF EXISTS {TEAM_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database after test");
}

/// KAIROS-T-0153 / [KAIROS-A-0020]: archiving is a visibility default, not a
/// permission boundary, so an archived item must resolve to exactly the
/// capabilities it had while live.
///
/// The failure this guards against is quiet and looks like a config problem:
/// if `resolve_authorization_board` returns `None` for an archived item, the
/// caller falls back to the tenant-wide org-admin policy, and archived work
/// becomes *more* restricted than live work.
#[test]
fn archived_items_resolve_the_same_capabilities_as_live_ones() {
    use kairos_db::models::{
        BoardLevel, NewBoard, NewBoardColumn, NewTask, NewTeam, NewTeamMember, TaskType, TeamType,
        WorkClass,
    };

    static ARCHIVE_SCRATCH_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_abac_archive_test");

    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {ARCHIVE_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {ARCHIVE_SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&ARCHIVE_SCRATCH_DB);

    let scratch_url = with_database(&admin_url, &ARCHIVE_SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "archco", "Archco").expect("provisioning archco");
    sql_query("SET search_path TO org_archco, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let member = insert_user(&mut conn, "dex|arch-member", "m@archco.test", "Member");
    let outsider = insert_user(&mut conn, "dex|arch-outsider", "o@archco.test", "Outsider");

    let team_id: Uuid = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: TeamType::Platform,
        })
        .returning(schema::teams::id)
        .get_result(&mut conn)
        .expect("inserting team");
    diesel::insert_into(schema::team_members::table)
        .values(NewTeamMember {
            team_id,
            user_id: member,
        })
        .execute(&mut conn)
        .expect("inserting team member");
    let team_board: Uuid = diesel::insert_into(schema::boards::table)
        .values(NewBoard {
            name: "Platform Delivery".into(),
            slug: "platform-delivery".into(),
            board_level: BoardLevel::Delivery,
            team_id: Some(team_id),
            // A prefix of its own (COLLIERY-T-3099).
            code_prefix: format!("B{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
                .to_uppercase(),
        })
        .returning(schema::boards::id)
        .get_result(&mut conn)
        .expect("inserting team board");
    // A hand-made board has no columns; the task needs somewhere to sit.
    let column: Uuid = diesel::insert_into(schema::board_columns::table)
        .values(NewBoardColumn {
            board_id: team_board,
            name: "Todo".into(),
            position: 0,
            is_done: false,
            claims: false,
            keeps_claims: false,
        })
        .returning(schema::board_columns::id)
        .get_result(&mut conn)
        .expect("inserting column");

    let task_id: Uuid = diesel::insert_into(schema::tasks::table)
        .values(NewTask {
            short_code: "T-0001".into(),
            title: "Put away later".into(),
            content: "".into(),
            board_id: team_board,
            column_id: column,
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            team_id: Some(team_id),
            repository_id: None,
            created_by: member,
            updated_by: member,
        })
        .returning(schema::tasks::id)
        .get_result(&mut conn)
        .expect("inserting task");

    // Baseline, while live.
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, task_id).expect("resolve"),
        Some(team_board),
        "a live task resolves to its own board"
    );
    let live_caps: Vec<&str> = rules::TEAM_IMPLIED_CAPABILITIES
        .iter()
        .copied()
        .filter(|cap| {
            abac::check_capability(&mut conn, team_board, member, cap).expect("check_capability")
        })
        .collect();
    assert!(
        !live_caps.is_empty(),
        "the team member must hold something while the task is live, or this test proves nothing"
    );
    assert_eq!(
        item_created_by(&mut conn, task_id).expect("created_by"),
        Some(member)
    );

    // KAIROS-T-0625: the queries of one edit of a task. Before, the facts
    // probed the five entity tables for the creator, re-joined the org for
    // the admin role, and probed the tables again for the board.
    let (_, before) = count_queries(&mut conn, |conn| {
        let created = item_created_by(conn, task_id).expect("created_by") == Some(member);
        let admin = abac::is_org_admin(conn, "archco", member).expect("is_org_admin");
        let board = abac::resolve_authorization_board(conn, task_id)
            .expect("resolve")
            .expect("board");
        let manage =
            abac::check_capability(conn, board, member, rules::MANAGE_TASKS).expect("check");
        (created, admin, manage)
    });
    let ((facts, board), after) = count_queries(&mut conn, |conn| {
        abac::edit_facts(
            conn,
            member,
            false,
            ItemType::Task,
            task_id,
            rules::EditUse::Edit,
        )
        .expect("edit facts")
    });
    eprintln!("KAIROS-T-0625: one edit of a task: {before} queries before, {after} after");
    assert_eq!(before, 8, "the old facts of one edit of a task");
    assert_eq!(
        after, 2,
        "one edit: the row of the item, and the capability"
    );
    assert_eq!(board, Some(team_board));
    assert!(facts.created_item && facts.holds_manage && !facts.is_org_admin);
    // A refusal reads the claim too: one query more.
    let ((facts, _), refused) = count_queries(&mut conn, |conn| {
        abac::edit_facts(
            conn,
            outsider,
            false,
            ItemType::Task,
            task_id,
            rules::EditUse::Edit,
        )
        .expect("edit facts")
    });
    assert_eq!(refused, 3, "a refused edit of a task also reads the claim");
    assert!(!rules::may_edit_item(facts));
    // The admin role is the caller's: it is not read again.
    let ((facts, _), admin) = count_queries(&mut conn, |conn| {
        abac::edit_facts(
            conn,
            outsider,
            true,
            ItemType::Task,
            task_id,
            rules::EditUse::Edit,
        )
        .expect("edit facts")
    });
    assert_eq!(admin, 2);
    assert!(rules::may_edit_item(facts));

    // Put it away.
    diesel::update(schema::tasks::table.filter(schema::tasks::id.eq(task_id)))
        .set(schema::tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("archiving the task");

    assert_eq!(
        abac::resolve_authorization_board(&mut conn, task_id).expect("resolve"),
        Some(team_board),
        "an archived task still resolves to its board — otherwise the caller \
         falls back to the org-admin-only policy and archived work becomes \
         more restricted than live work (KAIROS-A-0020 rule 4)"
    );
    let archived_caps: Vec<&str> = rules::TEAM_IMPLIED_CAPABILITIES
        .iter()
        .copied()
        .filter(|cap| {
            abac::check_capability(&mut conn, team_board, member, cap).expect("check_capability")
        })
        .collect();
    assert_eq!(
        live_caps, archived_caps,
        "archiving must not change what the team member may do"
    );
    assert_eq!(
        item_created_by(&mut conn, task_id).expect("created_by"),
        Some(member),
        "who created a thing is a fact about the row, not about its visibility"
    );

    // …and archiving must not WIDEN access either.
    for cap in rules::TEAM_IMPLIED_CAPABILITIES {
        assert!(
            !abac::check_capability(&mut conn, team_board, outsider, cap)
                .expect("check_capability"),
            "an outsider must hold no capability on an archived item's board either ({cap:?})"
        );
    }

    drop(conn);
    sql_query(format!(
        "DROP DATABASE IF EXISTS {ARCHIVE_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database after test");
}

/// KAIROS-T-0410 (KAIROS-A-0025 decisions 1 and 8): on a strategy or
/// initiative board, a principal with the `member` grant is a member, and
/// membership satisfies `manage_<type>` of the board and
/// `transition_items`. Not `configure_boards`, not `administer_members`,
/// not archive (the edit rule for an archive reads grants only), and not on
/// delivery or ADR boards. The revoke of the member grant removes it.
///
/// KAIROS-T-0541 changed this test. Until then ANY grant made a member, and
/// the test showed `configure_boards` and `administer_members` alone give
/// the edit and the move. That was the bug. Now `configure_boards`,
/// `pages` and `administer_members` alone give neither, and
/// `transition_items` alone moves and does not edit.
#[test]
fn board_membership_gives_edit_and_move_on_strategy_and_initiative_boards() {
    use kairos_core::abac::EditUse;
    use kairos_db::models::{BoardLevel, NewBoard};

    static MEMBER_SCRATCH_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_abac_member_test");

    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {MEMBER_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {MEMBER_SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&MEMBER_SCRATCH_DB);

    let scratch_url = with_database(&admin_url, &MEMBER_SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "memberco", "Memberco").expect("provisioning memberco");
    sql_query("SET search_path TO org_memberco, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let org_id: Uuid = schema::organizations::table
        .filter(schema::organizations::slug.eq("memberco"))
        .select(schema::organizations::id)
        .first(&mut conn)
        .expect("provisioned org row");
    let author = insert_user(&mut conn, "dex|author", "author@memberco.test", "Author");
    let member = insert_user(&mut conn, "dex|member", "member@memberco.test", "Member");
    let outsider = insert_user(&mut conn, "dex|outsider", "out@memberco.test", "Outsider");
    for user_id in [author, member, outsider] {
        diesel::insert_into(schema::organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role: OrgRole::Member,
            })
            .execute(&mut conn)
            .expect("inserting org member");
    }

    let initiatives = board_id_by_slug(&mut conn, "initiatives");
    let strategy = board_id_by_slug(&mut conn, "strategy");
    let new_board = |conn: &mut PgConnection, slug: &str, level: BoardLevel| -> Uuid {
        diesel::insert_into(schema::boards::table)
            .values(NewBoard {
                name: slug.into(),
                slug: slug.into(),
                board_level: level,
                team_id: None,
                code_prefix: format!("B{}", &Uuid::new_v4().simple().to_string()[..8])
                    .to_uppercase(),
            })
            .returning(schema::boards::id)
            .get_result(conn)
            .expect("inserting board")
    };
    let delivery = new_board(&mut conn, "member-delivery", BoardLevel::Delivery);
    let adr_board = new_board(&mut conn, "member-adrs", BoardLevel::Adr);

    let check = |conn: &mut PgConnection, board, user, required: &str| {
        abac::check_capability(conn, board, user, required).expect("check_capability")
    };
    let granted = |conn: &mut PgConnection, board, user, required: &str| {
        abac::check_granted_capability(conn, board, user, required).expect("check")
    };

    // An initiative that the member did not create.
    let discovery = first_column(&mut conn, initiatives);
    let initiative: Uuid = diesel::insert_into(schema::initiatives::table)
        .values(NewInitiative {
            short_code: "I-0001".into(),
            title: "Shared".into(),
            content: "".into(),
            board_id: initiatives,
            column_id: discovery,
            complexity: None,
            is_bucket: false,
            bucket_type: None,
            created_by: author,
            updated_by: author,
        })
        .returning(schema::initiatives::id)
        .get_result(&mut conn)
        .expect("inserting initiative");

    // Before any grant: the member is not a member.
    assert!(!check(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        member,
        rules::TRANSITION_ITEMS
    ));

    // KAIROS-T-0541: a grant other than `member` makes no member. Until
    // then each of these grants also gave the edit and the move.
    for (other, moves) in [
        (rules::CONFIGURE_BOARDS, false),
        (rules::PAGES, false),
        (rules::ADMINISTER_MEMBERS, false),
        (rules::TRANSITION_ITEMS, true),
    ] {
        abac::grant_capability(&mut conn, initiatives, member, other, author)
            .expect("granting a grant that is not member");
        assert!(
            !check(&mut conn, initiatives, member, rules::MANAGE_INITIATIVES),
            "{other:?} alone must not edit"
        );
        assert_eq!(
            check(&mut conn, initiatives, member, rules::TRANSITION_ITEMS),
            moves,
            "{other:?} alone moves only when it is transition_items"
        );
        let (edit, _) = abac::edit_facts(
            &mut conn,
            member,
            false,
            ItemType::Initiative,
            initiative,
            EditUse::Edit,
        )
        .expect("edit facts");
        assert!(!rules::may_edit_item(edit), "{other:?} alone must not edit");
        abac::revoke_capability(&mut conn, initiatives, member, other, author)
            .expect("revoking the grant");
    }

    // The member grant on the initiative board: a member of it. The member
    // keeps `configure_boards` too, so the revoke below shows that only
    // the member grant counts.
    abac::grant_capability(&mut conn, initiatives, member, rules::MEMBER, author)
        .expect("granting member");
    abac::grant_capability(
        &mut conn,
        initiatives,
        member,
        rules::CONFIGURE_BOARDS,
        author,
    )
    .expect("granting configure_boards");
    assert!(check(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES
    ));
    assert!(check(
        &mut conn,
        initiatives,
        member,
        rules::TRANSITION_ITEMS
    ));
    assert!(check(
        &mut conn,
        initiatives,
        member,
        rules::CONFIGURE_BOARDS
    ));
    for not_implied in [
        rules::ADMINISTER_MEMBERS,
        rules::MANAGE_STRATEGIES,
        rules::MANAGE_TASKS,
        rules::MANAGE_DOCUMENTS,
        rules::MANAGE_ADRS,
        rules::PAGES,
    ] {
        assert!(
            !check(&mut conn, initiatives, member, not_implied),
            "membership must not give {not_implied:?}"
        );
    }
    // The archive check reads grants only.
    assert!(!granted(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!granted(
        &mut conn,
        initiatives,
        member,
        rules::TRANSITION_ITEMS
    ));
    // Through the edit rule: an edit passes, an archive does not.
    let (edit, board) = abac::edit_facts(
        &mut conn,
        member,
        false,
        ItemType::Initiative,
        initiative,
        EditUse::Edit,
    )
    .expect("edit facts");
    assert_eq!(board, Some(initiatives));
    assert!(rules::may_edit_item(edit));
    let (archive, _) = abac::edit_facts(
        &mut conn,
        member,
        false,
        ItemType::Initiative,
        initiative,
        EditUse::Archive,
    )
    .expect("archive facts");
    assert!(!rules::may_edit_item(archive));
    // The archive cascade reads grants only too.
    let facts = abac::edit_facts_of_items(&mut conn, "memberco", member, &[initiative])
        .expect("facts of items");
    assert!(!rules::may_edit_item(facts[&initiative].facts));
    // A grant of `manage_initiatives` gives the archive.
    abac::grant_capability(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES,
        author,
    )
    .expect("granting manage_initiatives");
    assert!(granted(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES
    ));
    abac::revoke_capability(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES,
        author,
    )
    .expect("revoking manage_initiatives");

    // A non-member is refused, on the same board.
    assert!(!check(
        &mut conn,
        initiatives,
        outsider,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        outsider,
        rules::TRANSITION_ITEMS
    ));

    // Membership is per board: nothing on the strategy board yet.
    assert!(!check(
        &mut conn,
        strategy,
        member,
        rules::MANAGE_STRATEGIES
    ));
    assert!(!check(&mut conn, strategy, member, rules::TRANSITION_ITEMS));
    // KAIROS-T-0541: `administer_members` makes no member of the strategy
    // board.
    abac::grant_capability(
        &mut conn,
        strategy,
        member,
        rules::ADMINISTER_MEMBERS,
        author,
    )
    .expect("granting administer_members");
    assert!(!check(
        &mut conn,
        strategy,
        member,
        rules::MANAGE_STRATEGIES
    ));
    assert!(!check(&mut conn, strategy, member, rules::TRANSITION_ITEMS));
    abac::grant_capability(&mut conn, strategy, member, rules::MEMBER, author)
        .expect("granting member");
    assert!(check(&mut conn, strategy, member, rules::MANAGE_STRATEGIES));
    assert!(check(&mut conn, strategy, member, rules::TRANSITION_ITEMS));
    assert!(!check(
        &mut conn,
        strategy,
        member,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!check(&mut conn, strategy, member, rules::CONFIGURE_BOARDS));

    // Delivery and ADR boards are unchanged: a grant is not membership
    // there, and the member grant is not either.
    for (board, manage) in [
        (delivery, rules::MANAGE_TASKS),
        (adr_board, rules::MANAGE_ADRS),
    ] {
        for grant in [rules::CONFIGURE_BOARDS, rules::MEMBER] {
            abac::grant_capability(&mut conn, board, member, grant, author)
                .expect("granting on a delivery or ADR board");
        }
        assert!(!check(&mut conn, board, member, manage));
        assert!(!check(&mut conn, board, member, rules::TRANSITION_ITEMS));
    }

    // The revoke of the member grant removes the rights, though the
    // member keeps `configure_boards` on the board.
    abac::revoke_capability(&mut conn, initiatives, member, rules::MEMBER, author)
        .expect("revoking member");
    assert!(check(
        &mut conn,
        initiatives,
        member,
        rules::CONFIGURE_BOARDS
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        member,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        member,
        rules::TRANSITION_ITEMS
    ));
    let (edit, _) = abac::edit_facts(
        &mut conn,
        member,
        false,
        ItemType::Initiative,
        initiative,
        EditUse::Edit,
    )
    .expect("edit facts");
    assert!(!rules::may_edit_item(edit));

    // KAIROS-T-0440 (KAIROS-A-0026): the board has its own team. A member of
    // that team gets what board membership gives, with no grant: the edit
    // and the move, not configuration, not archive, and nothing on the
    // strategy board, which has a team of its own.
    let team: Option<Uuid> = schema::boards::table
        .find(initiatives)
        .select(schema::boards::team_id)
        .first(&mut conn)
        .expect("the board");
    let team = team.expect("an initiative board has a team");
    diesel::insert_into(schema::team_members::table)
        .values(kairos_db::models::teams::NewTeamMember {
            team_id: team,
            user_id: outsider,
        })
        .execute(&mut conn)
        .expect("the outsider joins the team of the board");
    assert!(check(
        &mut conn,
        initiatives,
        outsider,
        rules::MANAGE_INITIATIVES
    ));
    assert!(check(
        &mut conn,
        initiatives,
        outsider,
        rules::TRANSITION_ITEMS
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        outsider,
        rules::CONFIGURE_BOARDS
    ));
    assert!(!check(
        &mut conn,
        initiatives,
        outsider,
        rules::ADMINISTER_MEMBERS
    ));
    assert!(!granted(
        &mut conn,
        initiatives,
        outsider,
        rules::MANAGE_INITIATIVES
    ));
    assert!(!check(
        &mut conn,
        strategy,
        outsider,
        rules::MANAGE_STRATEGIES
    ));

    drop(conn);
    sql_query(format!(
        "DROP DATABASE IF EXISTS {MEMBER_SCRATCH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database after test");
}

/// Who created a workflow item or document: a probe of the five entity
/// tables, which span one UUID space. The old first fact of the edit rule
/// (KAIROS-T-0111), kept here as the oracle of `creator_and_board`.
fn item_created_by(conn: &mut PgConnection, item_id: Uuid) -> QueryResult<Option<Uuid>> {
    use kairos_db::schema::{adrs, documents, initiatives, strategies, tasks};
    macro_rules! try_table {
        ($table:ident) => {
            if let Some(creator) = $table::table
                .filter($table::id.eq(item_id))
                .select($table::created_by)
                .first::<Uuid>(conn)
                .optional()?
            {
                return Ok(Some(creator));
            }
        };
    }
    try_table!(strategies);
    try_table!(initiatives);
    try_table!(tasks);
    try_table!(documents);
    try_table!(adrs);
    Ok(None)
}
