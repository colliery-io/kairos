//! Integration test for the KAIROS-T-0051 pre-delete cascade preview
//! (`GET /api/{entity_type}/{short_code}/cascade-preview`), through the
//! typed `kairos_client::KairosClient` against the booted production router.
//!
//! THE contract this proves: the preview reports EXACTLY the descendant set
//! a subsequent soft-delete cascades to, on a 3-level
//! strategy → initiative → task tree, and computes it WITHOUT mutating
//! anything (the tree is still live after the preview). It shares the
//! kairos-core `cascade_descendants` BFS with the delete path, so the two
//! cannot disagree — this test is the end-to-end witness of that.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres + real Dex. Owns the uniquely named scratch database
//! `kairos_cascade_preview_t0051_test`; the shared `kairos` database is
//! never touched (shared-services discipline).

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
use kairos_client::types::{CreateInitiativeRequest, CreateStrategyRequest, CreateTaskRequest};
use kairos_client::{EntityKind, Error};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, RelationshipType};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_cascade_preview_t0051_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn board_id(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

/// Parse a DTO id string to a Uuid.
fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("DTO id is a UUID")
}

#[tokio::test]
async fn cascade_preview_matches_actual_cascade() {
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

    // --- live server + a single org-admin client (svc bypasses ABAC) -------
    let http = reqwest::Client::new();
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
    let svc = server.client(&svc_token, "acme");

    // JIT-provision svc, then grant the org-admin role (bypasses per-board
    // capability checks for the creates below).
    let _ = svc
        .list_tasks(kairos_client::types::Pagination::default())
        .await;
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting svc admin membership");

    // --- tenant-schema seeding (delivery board for tasks) ------------------
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning seeding connection to the tenant schema");
    kairos_db::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("creating the delivery board");

    let strategy_board = board_id(&mut conn, BoardLevel::Strategy);
    let initiative_board = board_id(&mut conn, BoardLevel::Initiative);
    let delivery_board = board_id(&mut conn, BoardLevel::Delivery);

    // --- build the 3-level tree: strategy -> initiative -> task ------------
    let strategy = svc
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.to_string(),
            column_id: None,
            title: "Grow ARR".into(),
            content: "# Strategy".into(),
            hypothesis: None,
        })
        .await
        .expect("creating strategy");
    let initiative = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.to_string(),
            column_id: None,
            title: "Enterprise tier".into(),
            content: "# Initiative".into(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("creating initiative");
    let task = svc
        .create_task(&CreateTaskRequest {
            board_id: delivery_board.to_string(),
            column_id: None,
            title: "SSO login".into(),
            content: "# Task".into(),
            task_type: None,
            team_id: None,
        })
        .await
        .expect("creating task");

    // parent edges (A-0001): strategy -> initiative -> task.
    graph::link_items(
        &mut conn,
        uuid(&strategy.id),
        uuid(&initiative.id),
        RelationshipType::Parent,
        svc_id,
    )
    .expect("linking strategy -> initiative");
    graph::link_items(
        &mut conn,
        uuid(&initiative.id),
        uuid(&task.id),
        RelationshipType::Parent,
        svc_id,
    )
    .expect("linking initiative -> task");

    // --- previews at each level (root excluded, sorted) -------------------
    let leaf = svc
        .cascade_preview(EntityKind::Task, &task.short_code)
        .await
        .expect("preview of the leaf task");
    assert_eq!(leaf.short_code, task.short_code);
    assert_eq!(leaf.cascade_count, 0);
    assert!(leaf.cascaded_short_codes.is_empty(), "{leaf:?}");

    let mid = svc
        .cascade_preview(EntityKind::Initiative, &initiative.short_code)
        .await
        .expect("preview of the mid initiative");
    assert_eq!(mid.cascade_count, 1);
    assert_eq!(mid.cascaded_short_codes, vec![task.short_code.clone()]);

    let root = svc
        .cascade_preview(EntityKind::Strategy, &strategy.short_code)
        .await
        .expect("preview of the root strategy");
    assert_eq!(root.short_code, strategy.short_code);
    assert_eq!(root.cascade_count, 2);
    let mut expected = vec![initiative.short_code.clone(), task.short_code.clone()];
    expected.sort();
    assert_eq!(root.cascaded_short_codes, expected);

    // --- the preview is READ-ONLY: the whole tree is still live -----------
    svc.get_strategy(&strategy.short_code)
        .await
        .expect("strategy still live after preview");
    svc.get_initiative(&initiative.short_code)
        .await
        .expect("initiative still live after preview");
    svc.get_task(&task.short_code)
        .await
        .expect("task still live after preview");

    // --- THE contract: preview == actual cascade --------------------------
    // A second preview (idempotent) captured immediately before the delete,
    // then the delete's own report — the two must be identical.
    let preview_before_delete = svc
        .cascade_preview(EntityKind::Strategy, &strategy.short_code)
        .await
        .expect("re-preview before delete");
    let deleted = svc
        .delete_strategy(&strategy.short_code)
        .await
        .expect("deleting the strategy root");

    assert_eq!(
        deleted.short_code, preview_before_delete.short_code,
        "root short code matches"
    );
    assert_eq!(
        deleted.cascade_count, preview_before_delete.cascade_count,
        "cascade counts match"
    );
    assert_eq!(
        deleted.cascaded_short_codes, preview_before_delete.cascaded_short_codes,
        "the preview named EXACTLY the descendants the delete cascaded to"
    );

    // The cascade actually removed the descendants (they now 404).
    for (kind, code) in [
        (EntityKind::Initiative, &initiative.short_code),
        (EntityKind::Task, &task.short_code),
    ] {
        let err = match kind {
            EntityKind::Initiative => svc.get_initiative(code).await.err(),
            EntityKind::Task => svc.get_task(code).await.err(),
            _ => unreachable!(),
        }
        .expect("cascaded descendant is gone");
        assert!(matches!(err, Error::NotFound { .. }), "{err}");
    }

    // Previewing a soft-deleted item is a 404 (no live root).
    let err = svc
        .cascade_preview(EntityKind::Strategy, &strategy.short_code)
        .await
        .expect_err("preview of a deleted item 404s");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
