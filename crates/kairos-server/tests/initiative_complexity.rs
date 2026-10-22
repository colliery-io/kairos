//! The complexity of an initiative changes after its creation: REST
//! `PUT /api/initiatives/{short_code}/complexity` and MCP `set_complexity`,
//! which call one function (`api::initiatives::change_complexity`).
//!
//! Complexity is not content: a change writes one activity row and does not
//! bump the version. `null` and `""` clear it. The edit rule applies.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_initiative_complexity_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types::{CreateInitiativeRequest, CreateTaskRequest};
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_initiative_complexity_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn add_member(conn: &mut PgConnection, org: Uuid, user: Uuid, role: OrgRole) {
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org,
            user_id: user,
            role,
        })
        .execute(conn)
        .expect("granting membership");
}

fn board_of_level(conn: &mut PgConnection, level: BoardLevel) -> String {
    use kairos_db::schema::boards::dsl;
    let id: Uuid = dsl::boards
        .filter(dsl::board_level.eq(level))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level:?} board: {e}"));
    id.to_string()
}

/// The details of the `complexity` activity rows of one item, oldest first.
fn complexity_rows(conn: &mut PgConnection, item: &str) -> Vec<String> {
    use kairos_db::schema::activity_log::dsl;
    let item: Uuid = item.parse().expect("item id");
    dsl::activity_log
        .filter(dsl::entity_id.eq(item))
        .filter(dsl::action.eq("complexity"))
        .order(dsl::occurred_at.asc())
        .select(dsl::details)
        .load(conn)
        .expect("reading the activity")
}

/// Minimal MCP session over HTTP (initialize → initialized → tools/call).
struct McpSession {
    http: reqwest::Client,
    url: String,
    token: String,
    tenant: String,
    next_id: u64,
}

impl McpSession {
    async fn open(base_url: &str, token: &str, tenant: &str) -> Self {
        let http = reqwest::Client::new();
        let url = format!("{base_url}/mcp");
        let response = http
            .post(&url)
            .bearer_auth(token)
            .header("X-Tenant", tenant)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "kairos-complexity-test", "version": "0.0.0"},
                },
            }))
            .send()
            .await
            .expect("initialize");
        assert_eq!(response.status(), 200, "initialize failed");
        let session = Self {
            http,
            url,
            token: token.to_string(),
            tenant: tenant.to_string(),
            next_id: 0,
        };
        let response = session
            .http
            .post(&session.url)
            .bearer_auth(&session.token)
            .header("X-Tenant", &session.tenant)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .send()
            .await
            .expect("initialized");
        assert_eq!(response.status(), 202, "initialized notify");
        session
    }

    /// Call a tool; returns `(is_error, text)`.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        self.next_id += 1;
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("X-Tenant", &self.tenant)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .send()
            .await
            .expect("tools/call");
        assert_eq!(response.status(), 200, "tools/call HTTP status");
        let body = response.text().await.expect("body");
        let message: Value = serde_json::from_str::<Value>(&body)
            .ok()
            .filter(|v| v.get("jsonrpc").is_some())
            .or_else(|| {
                body.lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .find(|v| v.get("jsonrpc").is_some())
            })
            .unwrap_or_else(|| panic!("no JSON-RPC message in response body: {body:?}"));
        let result = &message["result"];
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {message}"))
            .to_string();
        (is_error, text)
    }
}

#[tokio::test]
async fn the_complexity_of_an_initiative_changes_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let carol_token = user_token(&http, "carol").await;
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
    let carol = server.client(&carol_token, "acme");
    for client in [&svc, &carol] {
        let _ = client.whoami().await;
    }
    // svc is an org admin; carol is a member with no team, no grant and no
    // board membership: the edit rule refuses her.
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let carol_id = user_id(&mut conn, "carol@kairos.test");
    add_member(&mut conn, acme, svc_id, OrgRole::Admin);
    add_member(&mut conn, acme, carol_id, OrgRole::Member);

    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let initiatives_board = board_of_level(&mut conn, BoardLevel::Initiative);
    let created = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiatives_board,
            column_id: None,
            title: "Sized at creation".into(),
            content: String::new(),
            complexity: Some("m".into()),
            bucket_type: None,
            parent: None,
        })
        .await
        .expect("creating the initiative");
    let code = created.short_code.clone();
    assert_eq!(created.complexity.as_deref(), Some("m"));

    // --- set: m -> l, one activity row, no version bump ------------------------
    let set = svc
        .set_initiative_complexity(&code, Some("l"))
        .await
        .expect("setting l");
    assert_eq!(set.complexity.as_deref(), Some("l"));
    assert_eq!(set.version, created.version, "complexity is not content");
    assert_eq!(complexity_rows(&mut conn, &created.id), ["complexity:m->l"]);

    // --- the same value: nothing changes, no row --------------------------------
    let same = svc
        .set_initiative_complexity(&code, Some("l"))
        .await
        .expect("setting l again");
    assert_eq!(same.complexity.as_deref(), Some("l"));
    assert_eq!(complexity_rows(&mut conn, &created.id).len(), 1);

    // --- clear: null, then "" ---------------------------------------------------
    let cleared = svc
        .set_initiative_complexity(&code, None)
        .await
        .expect("clearing with null");
    assert_eq!(cleared.complexity, None);
    svc.set_initiative_complexity(&code, Some("xs"))
        .await
        .expect("setting xs");
    let cleared = svc
        .set_initiative_complexity(&code, Some(""))
        .await
        .expect("clearing with an empty string");
    assert_eq!(cleared.complexity, None);
    assert_eq!(
        complexity_rows(&mut conn, &created.id),
        [
            "complexity:m->l",
            "complexity:l->none",
            "complexity:none->xs",
            "complexity:xs->none",
        ]
    );

    // --- a bad value: 422 that lists the sizes ----------------------------------
    let err = svc
        .set_initiative_complexity(&code, Some("xxl"))
        .await
        .expect_err("xxl is not a size");
    match &err {
        Error::Validation { message, .. } => {
            assert!(
                message.contains("xxl") && message.contains("xl"),
                "{message}"
            );
        }
        other => panic!("expected a 422 validation error, got {other}"),
    }

    // --- the edit rule: carol is refused, and nothing changes -------------------
    let err = carol
        .set_initiative_complexity(&code, Some("s"))
        .await
        .expect_err("carol may not edit the initiative");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let still = svc.get_initiative(&code).await.expect("reading");
    assert_eq!(still.complexity, None, "a refused write changes nothing");

    // --- MCP: the same function, and a task is refused ---------------------------
    let mut mcp = McpSession::open(&server.base_url, &svc_token, "acme").await;
    let (is_error, text) = mcp
        .call(
            "set_complexity",
            json!({"short_code": code, "complexity": "s"}),
        )
        .await;
    assert!(!is_error, "{text}");
    assert_eq!(
        text,
        format!("Set the complexity of {code} to s (was none).")
    );
    let (is_error, text) = mcp
        .call(
            "set_complexity",
            json!({"short_code": code, "complexity": "s"}),
        )
        .await;
    assert!(!is_error, "{text}");
    assert_eq!(
        text,
        format!("{code} already has the complexity s: nothing changed.")
    );
    let (is_error, text) = mcp
        .call(
            "set_complexity",
            json!({"short_code": code, "complexity": null}),
        )
        .await;
    assert!(!is_error, "{text}");
    assert_eq!(text, format!("Cleared the complexity of {code} (was s)."));

    let team = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            code_prefix: kairos_core::short_code::prefix_from_slug("platform"),
            team_type: None,
        })
        .await
        .expect("platform");
    let task = svc
        .create_task(&CreateTaskRequest {
            board_id: team.delivery_board_id.clone(),
            column_id: None,
            title: "A task has no complexity".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("a task");
    let (is_error, text) = mcp
        .call(
            "set_complexity",
            json!({"short_code": task.short_code, "complexity": "s"}),
        )
        .await;
    assert!(is_error, "a task is refused: {text}");
    assert!(
        text.contains("set_complexity applies to initiatives only"),
        "{text}"
    );

    // --- archived: a write is refused with 404 -----------------------------------
    svc.delete_initiative(&code).await.expect("archiving");
    let err = svc
        .set_initiative_complexity(&code, Some("m"))
        .await
        .expect_err("an archived initiative is refused");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
