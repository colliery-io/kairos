//! KAIROS-T-0506 — `PUT /api/tasks/{short_code}/state`: a task has one
//! live state (branch, commit, pushed, PR, next step), separate from its
//! notes. A write is a patch (KAIROS-T-0533): a field that is not sent is
//! kept, `null` clears it, a stale `expected_updated_at` is a CONFLICT.
//! It makes no new version. The task endpoints and the board items carry
//! it. The edit rule applies.
//! The MCP tool `set_state` is in `tests/mcp.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_state_t0506_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_state_t0506_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The id of the column `name` of the board `board_slug`.
fn column_id(conn: &mut PgConnection, board_slug: &str, name: &str) -> String {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
    }
    let row: Row = diesel::sql_query(
        "SELECT c.id FROM org_acme.board_columns c \
         JOIN org_acme.boards b ON b.id = c.board_id \
         WHERE b.slug = $1 AND c.name = $2",
    )
    .bind::<diesel::sql_types::Text, _>(board_slug)
    .bind::<diesel::sql_types::Text, _>(name)
    .get_result(conn)
    .expect("column");
    row.id.to_string()
}

#[tokio::test]
async fn a_task_has_one_live_state_that_a_write_patches_against_live_stack() {
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

    let http = reqwest::Client::new();
    let token = user_token(&http, "svc").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 8).await.expect("pool");
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
    for (user, user_token, role) in [
        ("svc", &token, OrgRole::Admin),
        ("bob", &bob_token, OrgRole::Member),
    ] {
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(user_token),
            &TENANT,
            None,
        )
        .await;
        let user_id: Uuid = users::table
            .filter(users::email.eq(format!("{user}@kairos.test")))
            .select(users::id)
            .first(&mut conn)
            .expect("user provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let svc_name: String = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::display_name)
        .first(&mut conn)
        .expect("svc display name");

    let send = |method: Method, uri: String, token: String, body: Option<Value>| {
        let router = router.clone();
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    let (status, team) = send(
        Method::POST,
        "/api/teams".into(),
        token.clone(),
        Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
    )
    .await;
    assert!(status.is_success(), "{status} {team}");
    let (status, task) = send(
        Method::POST,
        "/api/tasks".into(),
        token.clone(),
        Some(json!({
            "board_id": "payments-delivery",
            "title": "Pay",
            "content": "# Pay\n\nThe body.\n",
        })),
    )
    .await;
    assert!(status.is_success(), "{status} {task}");
    let code = task["short_code"].as_str().expect("short code").to_string();
    assert!(
        task.get("state").is_none(),
        "a new task has no state: {task}"
    );
    let state_uri = format!("/api/tasks/{code}/state");

    // --- set: each field, no new version --------------------------------------
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({
            "branch": "pay-1",
            "commit": "abc1234",
            "pushed": true,
            "pr_url": "https://example.com/pr/12",
            "next_step": "Wait for CI"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["short_code"], code.as_str());
    // KAIROS-T-0792: version 1 is the template, version 2 the text.
    assert_eq!(answer["version"], 2, "the state is not content: {answer}");
    let state = &answer["state"];
    assert_eq!(state["branch"], "pay-1", "{answer}");
    assert_eq!(state["commit"], "abc1234");
    assert_eq!(state["pushed"], true);
    assert_eq!(state["pr_url"], "https://example.com/pr/12");
    assert_eq!(state["next_step"], "Wait for CI");
    assert_eq!(state["updated_by_name"], svc_name.as_str());
    assert!(state["updated_at"].as_str().is_some_and(|at| at.len() > 10));

    // --- read: the task, the list and the board items carry it -----------------
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(got["state"]["branch"], "pay-1", "{got}");
    let (_, list) = send(Method::GET, "/api/tasks".into(), token.clone(), None).await;
    let listed = list["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|t| t["short_code"] == code.as_str())
        .expect("the task is listed");
    assert_eq!(listed["state"]["next_step"], "Wait for CI", "{list}");
    let (status, board) = send(
        Method::GET,
        "/api/boards/payments-delivery/items".into(),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{board}");
    let board_text = board.to_string();
    assert!(
        board_text.contains("\"branch\":\"pay-1\""),
        "the board items carry the state: {board_text}"
    );

    // --- patch (KAIROS-T-0533): a field that is not sent is kept ---------------
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({"next_step": " Rebase on main "})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let state = &answer["state"];
    assert_eq!(state["branch"], "pay-1", "kept: {answer}");
    assert_eq!(state["next_step"], "Rebase on main", "trimmed: {answer}");
    let stamp = state["updated_at"]
        .as_str()
        .expect("updated_at")
        .to_string();

    // null clears one field and keeps the others.
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({"pr_url": null, "expected_updated_at": stamp})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["state"]["pr_url"].is_null(), "cleared: {answer}");
    assert_eq!(answer["state"]["branch"], "pay-1", "kept: {answer}");
    assert_eq!(
        answer["state"]["next_step"], "Rebase on main",
        "kept: {answer}"
    );

    // A stale expected_updated_at is a CONFLICT with the current state.
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({"next_step": "Over an old read", "expected_updated_at": stamp})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(answer["error"]["code"], "CONFLICT", "{answer}");
    assert_eq!(
        answer["error"]["details"]["state"]["next_step"], "Rebase on main",
        "the current state rides along: {answer}"
    );
    // An empty patch is refused.
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["code"], "VALIDATION", "{answer}");

    // A blank text clears the field, like null (KAIROS-T-0533).
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({"commit": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["state"]["commit"].is_null(), "cleared: {answer}");
    assert_eq!(answer["state"]["branch"], "pay-1", "kept: {answer}");

    // --- refusals: an unknown field, the edit rule, an unknown code -----------
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        token.clone(),
        Some(json!({"branch": "x", "sha": "abc"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("sha")),
        "the refusal names the field: {answer}"
    );
    let (status, answer) = send(
        Method::PUT,
        state_uri.clone(),
        bob_token.clone(),
        Some(json!({"next_step": "Not mine."})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    let (status, answer) = send(
        Method::PUT,
        "/api/tasks/PAYMENTS-T-9999/state".into(),
        token.clone(),
        Some(json!({"next_step": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(
        got["state"]["next_step"], "Rebase on main",
        "a refused write changes nothing: {got}"
    );

    // --- the state stays when the task reaches a done column; a change of
    // the claim clears the next step (KAIROS-T-0533) ---------------------------
    for column in ["Todo", "Active"] {
        let to_column_id = column_id(&mut conn, "payments-delivery", column);
        let (status, moved) = send(
            Method::POST,
            format!("/api/tasks/{code}/transition"),
            token.clone(),
            Some(json!({"to_column_id": to_column_id})),
        )
        .await;
        assert!(status.is_success(), "{column}: {status} {moved}");
    }
    // In Active with the claim: a release clears the next step, the rest stays.
    let (status, released) = send(
        Method::POST,
        format!("/api/tasks/{code}/release"),
        token.clone(),
        None,
    )
    .await;
    assert!(status.is_success(), "{status} {released}");
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert!(
        got["state"]["next_step"].is_null(),
        "cleared by the release: {got}"
    );
    assert_eq!(got["state"]["branch"], "pay-1", "the facts stay: {got}");
    let to_column_id = column_id(&mut conn, "payments-delivery", "Completed");
    let (status, moved) = send(
        Method::POST,
        format!("/api/tasks/{code}/transition"),
        token.clone(),
        Some(json!({"to_column_id": to_column_id})),
    )
    .await;
    assert!(status.is_success(), "Completed: {status} {moved}");
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(
        got["state"]["branch"], "pay-1",
        "the state stays when done: {got}"
    );

    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
