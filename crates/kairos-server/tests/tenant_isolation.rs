//! KAIROS-T-0024: two-tenant isolation at the HTTP level, proven entirely
//! through `kairos_client::KairosClient` instances (KAIROS-T-0016 covered
//! the service level; this is the API-surface counterpart per A-0001/
//! A-0005): two tenants with SAME-SHAPED data, cross-invisible through
//! every read surface the API offers.
//!
//! Cast:
//! - `alice` — org admin of `acme` only,
//! - `bob`   — org admin of `widgets` only,
//! - `svc`   — org admin of BOTH: the same bearer token behind two clients
//!   differing only in `X-Tenant` must see two disjoint worlds.
//!
//! Runs against the LIVE compose stack; owns the uniquely named scratch
//! database `kairos_tenant_isolation_m2_test` (shared-services
//! discipline).

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{CreateStrategyRequest, CreateTaskRequest, Pagination};
use kairos_client::types_org::CreateBoardRequest;
use kairos_client::types_search::SearchRequest;
use kairos_client::{Error, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_tenant_isolation_m2_test";

/// The distinctive text seeded IDENTICALLY into both tenants.
const SHARED_TITLE: &str = "Migrate the billing pipeline";

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// `public.organizations.id` by slug.
fn org_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    organizations::table
        .filter(organizations::slug.eq(slug))
        .select(organizations::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {slug} org row: {e}"))
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// Seed one tenant's same-shaped fixture THROUGH THE API: a delivery
/// board, a task on it, and a strategy on the default strategy board.
/// Returns (task short code, strategy short code, delivery board id).
async fn seed_tenant(admin: &KairosClient) -> (String, String, String) {
    let board = admin
        .create_board(&CreateBoardRequest {
            name: "Delivery".into(),
            slug: "delivery".into(),
            board_level: "delivery".into(),
            team_id: None,
        })
        .await
        .expect("creating the delivery board");
    let board_id = board.board.id.clone();
    let task = admin
        .create_task(&CreateTaskRequest {
            board_id: board_id.clone(),
            column_id: None,
            title: SHARED_TITLE.into(),
            content: "billing cutover runbook".into(),
            task_type: None,
            work_class: None,
            team_id: None,
        })
        .await
        .expect("creating the task");
    let strategy_board = admin
        .list_boards(Pagination::default())
        .await
        .expect("listing boards")
        .items
        .into_iter()
        .find(|b| b.board_level == "strategy")
        .expect("default strategy board");
    let strategy = admin
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.id,
            column_id: None,
            title: SHARED_TITLE.into(),
            content: "same-shaped strategy in both tenants".into(),
            hypothesis: None,
        })
        .await
        .expect("creating the strategy");
    (task.short_code, strategy.short_code, board_id)
}

#[tokio::test]
async fn http_level_two_tenant_isolation() {
    // --- scratch database + BOTH tenants --------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    provision_tenant(&mut conn, "widgets", "Widgets Co").expect("provisioning widgets");
    let acme_org = org_id(&mut conn, "acme");
    let widgets_org = org_id(&mut conn, "widgets");

    // --- live server + per-tenant clients ---------------------------------------
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
    let bob = server.client(&bob_token, "widgets");
    // The SAME svc token behind two clients differing only in X-Tenant.
    let svc_acme = server.client(&svc_token, "acme");
    let svc_widgets = server.client(&svc_token, "widgets");

    // --- JIT-provision, then memberships: alice→acme, bob→widgets, svc→both ----
    for client in [&alice, &bob, &svc_acme] {
        let err = rejection(client.whoami().await);
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (org, user) in [
        (acme_org, alice_id),
        (widgets_org, bob_id),
        (acme_org, svc_id),
        (widgets_org, svc_id),
    ] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org,
                user_id: user,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }

    // --- membership isolation: each single-tenant user is a stranger in the
    // --- other tenant (same valid token, wrong X-Tenant) ------------------------
    let err = rejection(server.client(&alice_token, "widgets").whoami().await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    let err = rejection(server.client(&bob_token, "acme").whoami().await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");

    // --- same-shaped data in BOTH tenants, created through the API --------------
    let (acme_task, acme_strategy, acme_board) = seed_tenant(&alice).await;
    let (widgets_task, widgets_strategy, widgets_board) = seed_tenant(&bob).await;
    assert!(acme_task.starts_with("ACME-T-"), "{acme_task}");
    assert!(widgets_task.starts_with("WIDGETS-T-"), "{widgets_task}");
    assert_ne!(acme_board, widgets_board);

    // --- list surfaces: each tenant sees exactly its own rows -------------------
    let body = alice
        .list_tasks(Pagination::default())
        .await
        .expect("acme tasks");
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].short_code, acme_task);
    let body = bob
        .list_tasks(Pagination::default())
        .await
        .expect("widgets tasks");
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].short_code, widgets_task);

    // --- direct reads by the OTHER tenant's identifiers 404 ---------------------
    let err = rejection(alice.get_task(&widgets_task).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(bob.get_task(&acme_task).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(alice.get_strategy(&widgets_strategy).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(bob.get_strategy(&acme_strategy).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    // Board UUIDs from the other tenant's schema resolve to nothing.
    let err = rejection(alice.get_board(&widgets_board).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(bob.get_board(&acme_board).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // --- search: identical queries return only the caller's tenant --------------
    let request = SearchRequest {
        q: Some("billing".into()),
        ..SearchRequest::default()
    };
    let body = alice.search(&request).await.expect("acme search");
    assert_eq!(body.total, 2, "{body:?}");
    assert_eq!(body.results.tasks[0].short_code, acme_task);
    assert_eq!(body.results.strategies[0].short_code, acme_strategy);
    let body = bob.search(&request).await.expect("widgets search");
    assert_eq!(body.total, 2, "{body:?}");
    assert_eq!(body.results.tasks[0].short_code, widgets_task);
    assert_eq!(body.results.strategies[0].short_code, widgets_strategy);

    // --- the SAME token sees two disjoint worlds via X-Tenant alone -------------
    let acme_view = svc_acme
        .list_tasks(Pagination::default())
        .await
        .expect("svc/acme");
    let widgets_view = svc_widgets
        .list_tasks(Pagination::default())
        .await
        .expect("svc/widgets");
    assert_eq!(acme_view.total, 1);
    assert_eq!(widgets_view.total, 1);
    assert_eq!(acme_view.items[0].short_code, acme_task);
    assert_eq!(widgets_view.items[0].short_code, widgets_task);
    let acme_boards: Vec<String> = svc_acme
        .list_boards(Pagination::default())
        .await
        .expect("svc/acme boards")
        .items
        .into_iter()
        .map(|b| b.id)
        .collect();
    let widgets_boards: Vec<String> = svc_widgets
        .list_boards(Pagination::default())
        .await
        .expect("svc/widgets boards")
        .items
        .into_iter()
        .map(|b| b.id)
        .collect();
    assert!(
        acme_boards.iter().all(|id| !widgets_boards.contains(id)),
        "board id sets must be disjoint: {acme_boards:?} vs {widgets_boards:?}"
    );

    // --- org membership rosters are tenant-scoped --------------------------------
    let acme_members = alice
        .list_org_members(Pagination::default())
        .await
        .expect("acme members");
    let emails: Vec<&str> = acme_members
        .items
        .iter()
        .map(|m| m.email.as_str())
        .collect();
    assert!(emails.contains(&"alice@kairos.test"), "{emails:?}");
    assert!(!emails.contains(&"bob@kairos.test"), "{emails:?}");
    let widgets_members = bob
        .list_org_members(Pagination::default())
        .await
        .expect("widgets members");
    let emails: Vec<&str> = widgets_members
        .items
        .iter()
        .map(|m| m.email.as_str())
        .collect();
    assert!(emails.contains(&"bob@kairos.test"), "{emails:?}");
    assert!(!emails.contains(&"alice@kairos.test"), "{emails:?}");

    // --- activity/history surfaces: the other tenant's short codes are unknown ---
    let err = rejection(
        alice
            .history(kairos_client::EntityKind::Task, &widgets_task, None, None)
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(
        alice
            .relationships(kairos_client::EntityKind::Task, &widgets_task)
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // --- teardown ------------------------------------------------------------------
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
