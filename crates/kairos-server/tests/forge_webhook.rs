//! Integration test for the KAIROS-T-0099 webhook endpoint (design in
//! KAIROS-I-0009): signed deliveries in, `item_links` out.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_forge_hook_t0099_test`.
//!
//! The properties that matter here are the ones that are easy to get
//! wrong and invisible when wrong:
//! - **ordering safety**: a redelivered "opened" must not un-merge a PR;
//! - **uniform rejection**: bad signature / unknown connection / unknown
//!   tenant are indistinguishable;
//! - **silent tolerance**: unmatched codes and unconsumed event types are
//!   successful no-ops, because a non-2xx makes forges retry forever.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types_forge::CreateForgeConnectionRequest;
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, TaskType, WorkClass};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, items, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::forge::auth::github_signature;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_forge_hook_t0099_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// One `item_links` row's state, by item + PR number.
fn link_state(conn: &mut PgConnection, item_id: Uuid, external_id: &str) -> Option<String> {
    use kairos_db::schema::item_links::dsl;
    dsl::item_links
        .filter(dsl::item_id.eq(item_id))
        .filter(dsl::external_id.eq(external_id))
        .select(dsl::state)
        .first::<String>(conn)
        .optional()
        .expect("querying item_links")
}

fn link_count(conn: &mut PgConnection) -> i64 {
    use kairos_db::schema::item_links::dsl;
    dsl::item_links
        .count()
        .get_result(conn)
        .expect("counting item_links")
}

/// A GitHub `pull_request` payload for PR `number` naming `code`.
fn github_pr_n(number: i64, code: &str, state: &str, merged: bool, updated_at: &str) -> String {
    json!({
        "action": if merged { "closed" } else { state },
        "repository": {
            "full_name": "acme/payments-api",
            "html_url": "https://github.com/acme/payments-api"
        },
        "sender": { "login": "dylan" },
        "pull_request": {
            "number": number,
            "state": state,
            "merged": merged,
            "draft": false,
            "title": format!("Work on {code}"),
            "body": "",
            "html_url": format!("https://github.com/acme/payments-api/pull/{number}"),
            "updated_at": updated_at,
            "user": { "login": "dylan" },
            "head": { "ref": format!("dylan/{code}-branch") }
        }
    })
    .to_string()
}

/// The common case: PR #42.
fn github_pr(code: &str, state: &str, merged: bool, updated_at: &str) -> String {
    github_pr_n(42, code, state, merged, updated_at)
}

#[tokio::test]
async fn forge_webhook_ingestion_against_live_stack() {
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
        auth.clone(),
    ));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");

    // JIT-provision svc, then make it an org admin.
    let _ = svc.list_forge_connections().await;
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting admin");

    // A task to link against, created directly in the tenant schema.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let delivery = kairos_db::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(svc_id),
    )
    .expect("delivery board")
    .id;
    let task = items::create_task(
        &mut conn,
        items::CreateTask {
            board_id: delivery,
            column_id: None,
            title: "Password-less auth",
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            team_id: None,
        },
        svc_id,
    )
    .expect("task");
    let code = task.short_code.clone();

    let created = svc
        .create_forge_connection(&CreateForgeConnectionRequest {
            forge: "github".into(),
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            team_id: None,
        })
        .await
        .expect("connecting the repo");
    let secret = created.webhook_secret.clone();
    let hook_path = format!(
        "/webhooks/github/acme/{}",
        created.connection.id
    );
    let deliver = |path: String, body: String, signature: Option<String>, event: &str| {
        let url = format!("{}{path}", server.base_url);
        let http = http.clone();
        let event = event.to_string();
        async move {
            let mut request = http
                .post(url)
                .header("x-github-event", event)
                .header("content-type", "application/json");
            if let Some(signature) = signature {
                request = request.header("x-hub-signature-256", signature);
            }
            request.body(body).send().await.expect("delivery")
        }
    };

    // =======================================================================
    // A correctly signed delivery creates the link
    // =======================================================================
    let opened = github_pr(&code, "open", false, "2026-09-01T10:00:00Z");
    let response = deliver(
        hook_path.clone(),
        opened.clone(),
        Some(github_signature(&secret, opened.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(response.status(), 200, "{:?}", response.text().await);
    assert_eq!(link_state(&mut conn, task.id, "42").as_deref(), Some("open"));

    // =======================================================================
    // ORDERING SAFETY: merge, then replay the earlier "opened"
    // =======================================================================
    let merged = github_pr(&code, "closed", true, "2026-09-01T12:00:00Z");
    let response = deliver(
        hook_path.clone(),
        merged.clone(),
        Some(github_signature(&secret, merged.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        link_state(&mut conn, task.id, "42").as_deref(),
        Some("merged")
    );

    // Forges retry and do not guarantee order. Replaying the older event
    // must NOT regress the row — the whole reason the upsert is guarded.
    let response = deliver(
        hook_path.clone(),
        opened.clone(),
        Some(github_signature(&secret, opened.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        link_state(&mut conn, task.id, "42").as_deref(),
        Some("merged"),
        "a redelivered 'opened' must not un-merge the pull request"
    );

    // =======================================================================
    // Authenticity failures are uniform, and change nothing
    // =======================================================================
    let before = link_count(&mut conn);
    let bad_signature = deliver(
        hook_path.clone(),
        opened.clone(),
        Some(github_signature("wrong-secret", opened.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(bad_signature.status(), 401);
    let unsigned = deliver(hook_path.clone(), opened.clone(), None, "pull_request").await;
    assert_eq!(unsigned.status(), 401);
    let unknown_connection = deliver(
        format!("/webhooks/github/acme/{}", Uuid::new_v4()),
        opened.clone(),
        Some(github_signature(&secret, opened.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(
        unknown_connection.status(),
        401,
        "an unknown connection must be indistinguishable from a bad signature"
    );
    let unknown_tenant = deliver(
        format!("/webhooks/github/nosuchtenant/{}", created.connection.id),
        opened.clone(),
        Some(github_signature(&secret, opened.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(unknown_tenant.status(), 401);
    assert_eq!(link_count(&mut conn), before, "rejections write nothing");

    // =======================================================================
    // Understood-but-inactionable deliveries are 2xx no-ops
    // =======================================================================
    // An event type we do not consume.
    let ping = json!({ "zen": "…", "repository": { "full_name": "acme/payments-api" } })
        .to_string();
    let response = deliver(
        hook_path.clone(),
        ping.clone(),
        Some(github_signature(&secret, ping.as_bytes())),
        "ping",
    )
    .await;
    assert_eq!(
        response.status(),
        200,
        "an unconsumed event type must not make the forge retry"
    );

    // A DIFFERENT pull request naming a short code that does not exist
    // here — a branch may legitimately reference another deployment's
    // code. (Its own PR number, so it cannot disturb PR 42's link.)
    let foreign = github_pr_n(99, "OTHER-T-9999", "open", false, "2026-09-01T13:00:00Z");
    let response = deliver(
        hook_path.clone(),
        foreign.clone(),
        Some(github_signature(&secret, foreign.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        link_count(&mut conn),
        before,
        "a code from another deployment is ignored, not an error"
    );

    // =======================================================================
    // Re-editing a PR to drop the code removes the stale link
    // =======================================================================
    // PR 42 previously named the task; this revision does not.
    let dropped = github_pr("OTHER-T-9999", "open", false, "2026-09-01T14:00:00Z");
    let response = deliver(
        hook_path.clone(),
        dropped.clone(),
        Some(github_signature(&secret, dropped.as_bytes())),
        "pull_request",
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        link_state(&mut conn, task.id, "42"),
        None,
        "a PR edited to no longer mention the code drops its link"
    );

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
