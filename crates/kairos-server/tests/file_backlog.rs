//! Integration test for the COMPUTED `file_backlog` capability
//! (KAIROS-T-0105, A-0019 §4 amending A-0006): any tenant member may create
//! a task against another team's repository, landing in that team's
//! delivery-board Backlog — and NOTHING else opens up. The negative suite
//! is the contract; it walks the whole write surface rather than sampling.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_file_backlog_t0105_test`.
//!
//! Cast:
//! - `svc`   — org admin: sets the world up,
//! - `bob`   — member of team `platform` (owns `payments-api`),
//! - `alice` — member of team `web` only: the cross-team filer,
//! - `carol` — org member on NO team: also a filer (membership is enough),
//! - a service account with a key: also a filer (A-0017 principals count),
//! - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.

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
use kairos_client::types::{CreateTaskRequest, UpdateContentRequest};
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{Error, KairosClient};
use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::enums::Forge;
use kairos_db::models::public::{NewServiceAccountUser, User};
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, repositories, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::service_accounts::auth::{generate_key, hash_key};

const SCRATCH_DB: &str = "kairos_file_backlog_t0105_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn org_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    organizations::table
        .filter(organizations::slug.eq(slug))
        .select(organizations::id)
        .first(conn)
        .expect("org row")
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

fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

fn forbidden<T: std::fmt::Debug>(result: Result<T, Error>, what: &str) {
    let err = rejection(result);
    assert!(matches!(err, Error::Forbidden { .. }), "{what}: {err}");
}

/// Minimal MCP session over HTTP (initialize → initialized → tools/call).
struct McpSession {
    http: reqwest::Client,
    url: String,
    token: String,
    session_id: String,
    next_id: u64,
}

impl McpSession {
    async fn open(base_url: &str, token: &str) -> Self {
        let http = reqwest::Client::new();
        let url = format!("{base_url}/mcp");
        let response = http
            .post(&url)
            .bearer_auth(token)
            .header("X-Tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "kairos-t0105-test", "version": "0.0.0"},
                },
            }))
            .send()
            .await
            .expect("initialize");
        assert_eq!(response.status(), 200, "initialize failed");
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("session id")
            .to_string();
        let session = Self {
            http,
            url,
            token: token.to_string(),
            session_id,
            next_id: 0,
        };
        let response = session
            .http
            .post(&session.url)
            .bearer_auth(&session.token)
            .header("X-Tenant", "acme")
            .header("mcp-session-id", &session.session_id)
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
            .header("X-Tenant", "acme")
            .header("mcp-session-id", &self.session_id)
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
        // The server may answer as JSON or as SSE frames; take the first
        // frame that parses as a JSON-RPC message (mirrors tests/mcp.rs).
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
async fn file_backlog_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    provision_tenant(&mut conn, "globex", "Globex").expect("provisioning globex");
    let acme = org_id(&mut conn, "acme");
    let globex = org_id(&mut conn, "globex");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
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
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");
    let globex_alice = server.client(&alice_token, "globex");

    // JIT-provision everyone, then memberships.
    for client in [&svc, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let carol_id = user_id(&mut conn, "carol@kairos.test");
    add_member(&mut conn, acme, svc_id, OrgRole::Admin);
    add_member(&mut conn, acme, alice_id, OrgRole::Member);
    add_member(&mut conn, acme, bob_id, OrgRole::Member);
    add_member(&mut conn, acme, carol_id, OrgRole::Member);
    add_member(&mut conn, globex, alice_id, OrgRole::Member);

    // Teams: platform (bob) owns payments-api; web (alice) owns portal-web.
    let platform = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await
        .expect("platform team");
    let web = svc
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            team_type: None,
        })
        .await
        .expect("web team");
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    svc.add_team_member(
        &platform.id,
        &AddTeamMemberRequest {
            user_id: bob_id.to_string(),
        },
    )
    .await
    .expect("bob → platform");
    svc.add_team_member(
        &web.id,
        &AddTeamMemberRequest {
            user_id: alice_id.to_string(),
        },
    )
    .await
    .expect("alice → web");
    let platform_id: Uuid = platform.id.parse().expect("uuid");

    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let payments = repositories::create(
        &mut conn,
        NewRepository {
            slug: "payments-api".into(),
            forge: Forge::Github,
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: "main".into(),
            team_id: platform_id,
            description: String::new(),
            created_by: svc_id,
            updated_by: svc_id,
        },
    )
    .expect("payments repo");
    let columns = svc.list_columns(&platform_board).await.expect("columns");
    let backlog = columns
        .iter()
        .find(|c| c.position == 0)
        .expect("Backlog column")
        .id
        .clone();
    let todo = columns
        .iter()
        .find(|c| c.position == 1)
        .expect("second column")
        .id
        .clone();

    let filing = CreateTaskRequest {
        board_id: None,
        column_id: None,
        title: "Bulk invoice export".into(),
        content: "Filed from portal-web".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository_id: Some("payments-api".into()),
    };

    // =======================================================================
    // whoami advertises the computed capability
    // =======================================================================
    let me = alice.whoami().await.expect("whoami");
    assert_eq!(me.implicit, vec!["file_backlog".to_string()]);

    // =======================================================================
    // The positive case: alice (web) files into platform's Backlog
    // =======================================================================
    let filed = alice
        .create_task(&filing)
        .await
        .expect("a member of another team files against platform's repo");
    assert_eq!(filed.board_id, platform_board);
    assert_eq!(filed.column_id, backlog, "lands in Backlog");
    assert_eq!(filed.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(filed.created_by, alice_id.to_string());
    assert_eq!(
        filed.repository.as_ref().map(|r| r.slug.as_str()),
        Some("payments-api")
    );

    // Membership alone is enough — carol is on no team at all.
    let by_carol = carol
        .create_task(&filing)
        .await
        .expect("a team-less member files too");
    assert_eq!(by_carol.column_id, backlog);

    // A service-account key (A-0017) is a member like any other.
    let sa: User = diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            "svc:filer".to_string(),
            "filer@svc.acme.kairos".to_string(),
            "filer",
        ))
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("service account");
    add_member(&mut conn, acme, sa.id, OrgRole::Member);
    let raw_key = generate_key("acme");
    api_keys::create_key(
        &mut conn,
        NewApiKey {
            user_id: sa.id,
            name: "filer".into(),
            token_hash: hash_key(&raw_key),
            prefix: "filer".into(),
            created_by: svc_id,
            expires_at: None,
        },
    )
    .expect("key");
    let bot = KairosClient::with_static_token(&server.base_url, &raw_key).with_tenant("acme");
    let by_bot = bot
        .create_task(&filing)
        .await
        .expect("a service account files too");
    assert_eq!(by_bot.column_id, backlog);
    assert_eq!(by_bot.created_by, sa.id.to_string());

    // Explicitly naming the Backlog column is the same thing.
    let explicit_backlog = alice
        .create_task(&CreateTaskRequest {
            column_id: Some(backlog.clone()),
            ..filing.clone()
        })
        .await
        .expect("explicit Backlog column");
    assert_eq!(explicit_backlog.column_id, backlog);

    // =======================================================================
    // The negative suite: nothing else opens up
    // =======================================================================
    // Not into any other column — 403, never silently re-routed.
    forbidden(
        alice
            .create_task(&CreateTaskRequest {
                column_id: Some(todo.clone()),
                ..filing.clone()
            })
            .await,
        "filing into a non-Backlog column",
    );
    // Not without a repository (a plain board create is still manage_tasks).
    forbidden(
        alice
            .create_task(&CreateTaskRequest {
                board_id: Some(platform_board.clone()),
                repository_id: None,
                ..filing.clone()
            })
            .await,
        "filing without a repository",
    );
    // Not with a repository owned by a different team than the board
    // (422 from the routing rule, before any capability question).
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                board_id: Some(web.delivery_board_id.clone().expect("web board")),
                ..filing.clone()
            })
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Not on what she filed: transition, edit, delete, work-class,
    // repository, metadata.
    forbidden(
        alice.transition_task(&filed.short_code, &todo).await,
        "transitioning what she filed",
    );
    forbidden(
        alice
            .update_task(
                &filed.short_code,
                &UpdateContentRequest {
                    title: None,
                    content: "edited".into(),
                    version: filed.version,
                },
            )
            .await,
        "editing what she filed",
    );
    forbidden(
        alice.delete_task(&filed.short_code).await,
        "deleting what she filed",
    );
    forbidden(
        alice
            .set_task_work_class(&filed.short_code, "support")
            .await,
        "re-laning what she filed",
    );
    forbidden(
        alice.set_task_repository(&filed.short_code, None).await,
        "unbinding what she filed",
    );
    forbidden(
        alice
            .update_metadata(
                kairos_client::EntityKind::Task,
                &filed.short_code,
                &kairos_client::types_meta::UpdateMetadataRequest {
                    values: [("priority".to_string(), Some("high".to_string()))]
                        .into_iter()
                        .collect(),
                },
            )
            .await,
        "setting metadata on what she filed",
    );
    // The owning team CAN: bob transitions it out of Backlog.
    bob.transition_task(&filed.short_code, &todo)
        .await
        .expect("the owning team triages it");
    // Another tenant's member cannot reach into acme at all.
    let err = rejection(globex_alice.create_task(&filing).await);
    assert!(
        matches!(
            err,
            Error::Validation { .. } | Error::Forbidden { .. } | Error::NotFound { .. }
        ),
        "another tenant never sees acme's repositories: {err}"
    );

    // =======================================================================
    // file_backlog is computed: never grantable, never revocable
    // =======================================================================
    let err = rejection(
        svc.add_board_member(
            &platform_board,
            &AddBoardMemberRequest {
                user_id: alice_id.to_string(),
                capabilities: vec!["file_backlog".into()],
            },
        )
        .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "granting file_backlog is refused: {err}"
    );

    // =======================================================================
    // Same rule over MCP: create_item with `repository` from a non-member
    // =======================================================================
    let mut mcp = McpSession::open(&server.base_url, &alice_token).await;
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Filed over MCP",
                "repository": "payments-api",
            }),
        )
        .await;
    assert!(!is_error, "MCP cross-team filing: {text}");
    let code_start = text.find("ACME-T-").expect("short code in the reply");
    let code: String = text[code_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let over_mcp = svc.get_task(&code).await.expect("the MCP-filed task");
    assert_eq!(
        over_mcp.column_id, backlog,
        "MCP filing lands in Backlog too"
    );
    assert_eq!(over_mcp.created_by, alice_id.to_string());
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Not allowed",
                "board": platform_board,
            }),
        )
        .await;
    assert!(
        is_error,
        "MCP repo-less create on another team's board is refused: {text}"
    );
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "initiative",
                "title": "Wrong type",
                "repository": "payments-api",
            }),
        )
        .await;
    assert!(is_error, "repository is tasks-only: {text}");
    let (_, text) = mcp.call("whoami", json!({})).await;
    assert!(
        text.contains("file_backlog"),
        "MCP whoami names the implicit capability: {text}"
    );

    drop(mcp);
    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    let _ = payments;
}
