//! Integration test for the KAIROS-T-0083 team-pages + announcements API
//! (design in KAIROS-I-0007), through the typed `kairos_client` against
//! the booted production router on a real port.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. Owns the uniquely named scratch
//! database `kairos_team_pages_t0083_test` (shared-services discipline).
//!
//! Cast (the T-0083 permission matrix):
//! - `svc`   — org ADMIN, NOT a team member (admin override path),
//! - `alice` — org member AND team member (the normal author),
//! - `bob`   — org member, NOT a team member (403s name team membership).

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types::Pagination;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_team_pages::{
    CreateTeamAnnouncementRequest, CreateTeamPageRequest, TeamPage, UpdateTeamPageRequest,
};
use kairos_db::models::{
    BoardLevel, NewOrganizationMember, OrgRole, RelationshipType, TaskType, WorkClass,
};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, items, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_team_pages_t0083_test";

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The tenant's provisioned initiative board.
fn initiative_board(conn: &mut PgConnection) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(BoardLevel::Initiative))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .expect("provisioned initiative board")
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// The scaffold node with this slug (panics when absent).
fn by_slug<'a>(pages: &'a [TeamPage], slug: &str) -> &'a TeamPage {
    pages
        .iter()
        .find(|p| p.slug == slug)
        .unwrap_or_else(|| panic!("no page {slug:?} in {pages:?}"))
}

/// A content-only PATCH body.
fn content_edit(content: &str, version: i32) -> UpdateTeamPageRequest {
    UpdateTeamPageRequest {
        title: None,
        content: Some(content.into()),
        version: Some(version),
        slug: None,
        parent_id: None,
        move_to_root: false,
        position: None,
    }
}

/// A structure-only PATCH body (rename/move).
fn structure_edit(
    slug: Option<&str>,
    parent_id: Option<&str>,
    move_to_root: bool,
) -> UpdateTeamPageRequest {
    UpdateTeamPageRequest {
        title: None,
        content: None,
        version: None,
        slug: slug.map(Into::into),
        parent_id: parent_id.map(Into::into),
        move_to_root,
        position: None,
    }
}

#[tokio::test]
async fn team_pages_endpoints_against_live_stack() {
    // --- scratch database + tenant ----------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // --- live server + typed clients (X-Tenant acme) ------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let svc_token = user_token(&http, "svc").await;

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth,
    ));
    let server = spawn_server(router).await;
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // --- JIT-provision the three users, then grant membership ---------------
    for client in [&alice, &bob, &svc] {
        let err = rejection(client.list_teams(Pagination::default()).await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user_id, role) in [
        (alice_id, OrgRole::Member),
        (bob_id, OrgRole::Member),
        (svc_id, OrgRole::Admin),
    ] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    // Later db assertions read the tenant schema.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning assertion connection to the tenant schema");

    // =======================================================================
    // Team creation seeds the scaffold; by-slug resolves it
    // =======================================================================
    let team = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await
        .expect("org admin creates the team");
    svc.add_team_member(
        &team.id,
        &AddTeamMemberRequest {
            user_id: alice_id.to_string(),
        },
    )
    .await
    .expect("alice joins the team");

    let resolved = svc
        .get_team_by_slug("platform")
        .await
        .expect("by-slug resolves the team");
    assert_eq!(resolved.id, team.id);
    let err = rejection(svc.get_team_by_slug("nope").await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // Reads are open tenant-wide: bob (not a team member) sees the scaffold.
    let pages = bob
        .list_team_pages(&team.id)
        .await
        .expect("open tenant-wide read");
    assert_eq!(pages.len(), 10, "scaffold shape: {pages:?}");
    let charter = by_slug(&pages, "charter").clone();
    assert!(charter.is_protected);
    assert_eq!(charter.kind, "page");
    assert_eq!(charter.version, 1);
    let docs = by_slug(&pages, "documentation").clone();
    assert_eq!(docs.kind, "folder");
    let tutorials = by_slug(&pages, "tutorials").clone();
    assert_eq!(tutorials.parent_id.as_deref(), Some(docs.id.as_str()));

    // Unknown team id → 404 on every family.
    let ghost = Uuid::new_v4().to_string();
    let err = rejection(bob.list_team_pages(&ghost).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(bob.list_team_announcements(&ghost).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Permission matrix: writes need team membership (or org admin)
    // =======================================================================
    let new_page = CreateTeamPageRequest {
        parent_id: Some(tutorials.id.clone()),
        kind: "page".into(),
        slug: "getting-started".into(),
        title: "Getting Started".into(),
        content: "# Getting Started".into(),
        position: 0,
    };
    // bob: org member but NOT team member → 403 naming team membership.
    let err = rejection(bob.create_team_page(&team.id, &new_page).await);
    match &err {
        Error::Forbidden { message, .. } => assert!(
            message.contains("team membership"),
            "403 must name team membership: {message}"
        ),
        other => panic!("expected 403, got {other}"),
    }
    let err = rejection(
        bob.update_team_page(&team.id, &charter.id, &content_edit("# Coup", 1))
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let err = rejection(bob.delete_team_page(&team.id, &tutorials.id).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // alice: team member → 201.
    let started = alice
        .create_team_page(&team.id, &new_page)
        .await
        .expect("team member creates a page");
    assert_eq!(started.version, 1);
    // svc: org admin, NOT a team member → also allowed.
    svc.create_team_page(
        &team.id,
        &CreateTeamPageRequest {
            parent_id: Some(tutorials.id.clone()),
            kind: "page".into(),
            slug: "advanced".into(),
            title: "Advanced".into(),
            content: String::new(),
            position: 1,
        },
    )
    .await
    .expect("org admin override");

    // =======================================================================
    // Validation: kind, parent, sibling slugs, size cap, mixed edits
    // =======================================================================
    let err = rejection(
        alice
            .create_team_page(
                &team.id,
                &CreateTeamPageRequest {
                    kind: "scroll".into(),
                    ..new_page.clone()
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Parent must be a live FOLDER of this team — a page is refused.
    let err = rejection(
        alice
            .create_team_page(
                &team.id,
                &CreateTeamPageRequest {
                    parent_id: Some(charter.id.clone()),
                    slug: "under-a-page".into(),
                    ..new_page.clone()
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Sibling slug conflict → 422 SLUG_CONFLICT.
    let err = rejection(alice.create_team_page(&team.id, &new_page).await);
    assert_eq!(err.code(), Some("SLUG_CONFLICT"), "{err}");
    // Size cap: content above MAX_CONTENT_BYTES → 422.
    let err = rejection(
        alice
            .update_team_page(
                &team.id,
                &started.id,
                &content_edit(&"x".repeat(256 * 1024 + 1), 1),
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Mixing a content edit with a rename in one call → 422.
    let err = rejection(
        alice
            .update_team_page(
                &team.id,
                &started.id,
                &UpdateTeamPageRequest {
                    content: Some("# Both".into()),
                    version: Some(1),
                    slug: Some("both".into()),
                    ..content_edit("# Both", 1)
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Empty PATCH and content-without-version → 422.
    let err = rejection(
        alice
            .update_team_page(&team.id, &started.id, &structure_edit(None, None, false))
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    let err = rejection(
        alice
            .update_team_page(
                &team.id,
                &started.id,
                &UpdateTeamPageRequest {
                    version: None,
                    ..content_edit("# No version", 1)
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");

    // =======================================================================
    // Version-checked content saves: 409 carries details.current; history
    // is append-only
    // =======================================================================
    let saved = alice
        .update_team_page(&team.id, &charter.id, &content_edit("# Charter v2", 1))
        .await
        .expect("fresh version saves");
    assert_eq!(saved.version, 2);
    assert_eq!(saved.content, "# Charter v2");
    // Replaying the SAME stale version → 409 with the current page inline.
    let err = rejection(
        alice
            .update_team_page(&team.id, &charter.id, &content_edit("# Stale", 1))
            .await,
    );
    match &err {
        Error::Conflict { current, .. } => {
            assert_eq!(current["version"], 2, "{current}");
            assert_eq!(current["content"], "# Charter v2", "{current}");
        }
        other => panic!("expected 409 with details.current, got {other}"),
    }
    // Every content save wrote a history row (baseline v1 + the v2 save).
    let charter_uuid: Uuid = charter.id.parse().unwrap();
    let history: i64 = kairos_db::schema::team_page_history::table
        .filter(kairos_db::schema::team_page_history::page_id.eq(charter_uuid))
        .count()
        .get_result(&mut conn)
        .expect("history rows");
    assert_eq!(history, 2, "append-only history: baseline + one save");

    // =======================================================================
    // Charter protection + folder deletion rules
    // =======================================================================
    let err = rejection(
        alice
            .update_team_page(
                &team.id,
                &charter.id,
                &structure_edit(Some("renamed"), None, false),
            )
            .await,
    );
    assert_eq!(err.code(), Some("PROTECTED_PAGE"), "{err}");
    let err = rejection(alice.delete_team_page(&team.id, &charter.id).await);
    assert_eq!(err.code(), Some("PROTECTED_PAGE"), "{err}");
    // Folder with live children → 422 FOLDER_NOT_EMPTY naming the count.
    let err = rejection(alice.delete_team_page(&team.id, &tutorials.id).await);
    match &err {
        Error::Other { code, message, .. } => {
            assert_eq!(code, "FOLDER_NOT_EMPTY");
            assert!(message.contains('2'), "count in message: {message}");
        }
        other => panic!("expected FOLDER_NOT_EMPTY, got {other}"),
    }
    // Rename/move a normal page works; then empty the folder and delete it.
    let moved = alice
        .update_team_page(&team.id, &started.id, &structure_edit(None, None, true))
        .await
        .expect("move to root");
    assert_eq!(moved.parent_id, None);
    let deleted = alice
        .delete_team_page(&team.id, &moved.id)
        .await
        .expect("soft-delete the moved page");
    assert!(deleted.deleted);
    let err = rejection(alice.get_team_page(&team.id, &moved.id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let live = svc.list_team_pages(&team.id).await.unwrap();
    let advanced_id = by_slug(&live, "advanced").id.clone();
    svc.delete_team_page(&team.id, &advanced_id)
        .await
        .expect("emptying tutorials");
    alice
        .delete_team_page(&team.id, &tutorials.id)
        .await
        .expect("deleting the now-empty folder");

    // =======================================================================
    // Announcements: append-only, pinned-first, author-or-admin delete
    // =======================================================================
    let err = rejection(
        bob.create_team_announcement(
            &team.id,
            &CreateTeamAnnouncementRequest {
                body: "intruder".into(),
                pinned: false,
            },
        )
        .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    let first = alice
        .create_team_announcement(
            &team.id,
            &CreateTeamAnnouncementRequest {
                body: "Sprint review Friday".into(),
                pinned: false,
            },
        )
        .await
        .expect("team member posts");
    let second = alice
        .create_team_announcement(
            &team.id,
            &CreateTeamAnnouncementRequest {
                body: "Deploy freeze next week".into(),
                pinned: false,
            },
        )
        .await
        .expect("second post");
    let pinned = svc
        .create_team_announcement(
            &team.id,
            &CreateTeamAnnouncementRequest {
                body: "Read the charter".into(),
                pinned: true,
            },
        )
        .await
        .expect("org admin pins");
    // Pinned first, then newest-first.
    let feed = bob
        .list_team_announcements(&team.id)
        .await
        .expect("open read");
    assert_eq!(
        feed.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec![pinned.id.as_str(), second.id.as_str(), first.id.as_str()],
        "pinned-first then newest: {feed:?}"
    );
    // Body size cap applies to announcements too.
    let err = rejection(
        alice
            .create_team_announcement(
                &team.id,
                &CreateTeamAnnouncementRequest {
                    body: "x".repeat(256 * 1024 + 1),
                    pinned: false,
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");

    // Deletion: author or org admin only. (bob is a member of NOTHING and
    // still 403s by authorship even if he were a team member — the check
    // is author-or-admin.)
    let err = rejection(bob.delete_team_announcement(&team.id, &first.id).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    alice
        .delete_team_announcement(&team.id, &first.id)
        .await
        .expect("author deletes own");
    svc.delete_team_announcement(&team.id, &second.id)
        .await
        .expect("org admin deletes any");
    let err = rejection(svc.delete_team_announcement(&team.id, &first.id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // Lifecycle events landed in the activity log (content edits are NOT
    // logged — team_page_history is their record, asserted above).
    use kairos_db::schema::activity_log;
    let by_type = |conn: &mut PgConnection, entity: &str, action: &str| -> i64 {
        activity_log::table
            .filter(activity_log::entity_type.eq(entity))
            .filter(activity_log::action.eq(action))
            .count()
            .get_result(conn)
            .expect("activity rows")
    };
    assert_eq!(by_type(&mut conn, "team_page", "create"), 2, "alice + svc");
    assert_eq!(
        by_type(&mut conn, "team_page", "delete"),
        3,
        "moved page + advanced + tutorials"
    );
    assert_eq!(by_type(&mut conn, "team_announcement", "create"), 3);

    // =======================================================================
    // Derived work-documents (KAIROS-T-0084): supports-parent is a team
    // task (team_id) OR an item on the team's delivery board
    // =======================================================================
    let web_team = svc
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            team_type: None,
        })
        .await
        .expect("second team");
    let platform_board: Uuid = team
        .delivery_board_id
        .as_deref()
        .expect("platform delivery board")
        .parse()
        .unwrap();
    let web_board: Uuid = web_team
        .delivery_board_id
        .as_deref()
        .expect("web delivery board")
        .parse()
        .unwrap();
    let platform_uuid: Uuid = team.id.parse().unwrap();

    let mk_task = |conn: &mut PgConnection, board: Uuid, team: Option<Uuid>, title: &str| {
        items::create_task(
            conn,
            items::CreateTask {
                board_id: board,
                column_id: None,
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team_id: team,
            },
            alice_id,
        )
        .expect("fixture task")
    };
    let mk_doc = |conn: &mut PgConnection, title: &str, parent: Uuid| {
        let doc = items::create_document(
            conn,
            items::CreateDocument {
                title,
                content: Some(""),
                template_id: None,
            },
            alice_id,
        )
        .expect("fixture document");
        kairos_db::graph::link_items(conn, parent, doc.id, RelationshipType::Supports, alice_id)
            .expect("supports edge");
        doc
    };

    // Membership paths: (a) team_id, (b) delivery-board placement.
    let t1 = mk_task(&mut conn, platform_board, Some(platform_uuid), "T1 both");
    let t2 = mk_task(&mut conn, platform_board, None, "T2 board only");
    let t3 = mk_task(&mut conn, web_board, Some(platform_uuid), "T3 team only");
    let org_board = initiative_board(&mut conn);
    let org_initiative = items::create_initiative(
        &mut conn,
        items::CreateInitiative {
            board_id: org_board,
            column_id: None,
            title: "Org-level initiative",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        alice_id,
    )
    .expect("org-level initiative");

    let doc_a = mk_doc(&mut conn, "Doc A (team task)", t1.id);
    let doc_b = mk_doc(&mut conn, "Doc B (board item)", t2.id);
    let doc_c = mk_doc(&mut conn, "Doc C (team task, foreign board)", t3.id);
    let _doc_d = mk_doc(&mut conn, "Doc D (org-level, absent)", org_initiative.id);
    // Dedup: one doc supporting TWO team items appears once.
    let doc_e = mk_doc(&mut conn, "Doc E (two parents)", t1.id);
    kairos_db::graph::link_items(&mut conn, t2.id, doc_e.id, RelationshipType::Supports, alice_id)
        .expect("second supports edge");
    // Soft-deleted document and soft-deleted parent are both excluded.
    let doc_f = mk_doc(&mut conn, "Doc F (deleted doc)", t1.id);
    diesel::update(kairos_db::schema::documents::table.find(doc_f.id))
        .set(kairos_db::schema::documents::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("soft-deleting doc F");
    let t4 = mk_task(&mut conn, platform_board, Some(platform_uuid), "T4 doomed");
    let _doc_g = mk_doc(&mut conn, "Doc G (deleted parent)", t4.id);
    diesel::update(kairos_db::schema::tasks::table.find(t4.id))
        .set(kairos_db::schema::tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("soft-deleting task T4");

    // The derived set, DISTINCT, ordered by document short code; parent
    // attribution is deterministic (lexicographically first parent).
    let work_docs = bob
        .list_team_work_documents(&team.id)
        .await
        .expect("open tenant-wide read");
    let mut expected = vec![
        (doc_a.short_code.clone(), t1.short_code.clone()),
        (doc_b.short_code.clone(), t2.short_code.clone()),
        (doc_c.short_code.clone(), t3.short_code.clone()),
        (doc_e.short_code.clone(), t1.short_code.clone()),
    ];
    expected.sort();
    assert_eq!(
        work_docs
            .iter()
            .map(|d| (d.short_code.clone(), d.parent_short_code.clone()))
            .collect::<Vec<_>>(),
        expected,
        "derived set: {work_docs:?}"
    );
    let a_row = work_docs
        .iter()
        .find(|d| d.short_code == doc_a.short_code)
        .unwrap();
    assert_eq!(a_row.lifecycle, "draft");
    assert_eq!(a_row.parent_title, "T1 both");
    assert_eq!(a_row.parent_type, "task");
    // Web's panel sees only the doc whose parent sits on ITS board.
    let web_docs = bob
        .list_team_work_documents(&web_team.id)
        .await
        .expect("web team's derived set");
    assert_eq!(
        web_docs
            .iter()
            .map(|d| d.short_code.as_str())
            .collect::<Vec<_>>(),
        vec![doc_c.short_code.as_str()],
        "{web_docs:?}"
    );
    // Unknown team → 404.
    let err = rejection(bob.list_team_work_documents(&ghost).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
