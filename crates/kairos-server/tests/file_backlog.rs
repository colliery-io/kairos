//! Integration test for the COMPUTED `file_backlog` capability
//! (KAIROS-T-0105, amended by COLLIERY-T-0218 for COLLIERY-A-0023
//! decisions 7 and 8): any tenant member may send a REQUEST to any team, a
//! task in the entry column of its delivery board, in the support lane —
//! and NOTHING else opens up on the BOARD. The negative suite is the
//! contract; it walks the whole write surface rather than sampling.
//!
//! COLLIERY-T-0228: the person who sent a request created it, so she can
//! edit it, link it and archive it (the edit rule and the link rule;
//! `tests/edit_rule.rs` is their contract). She cannot MOVE it: transition,
//! work class and board are as they were. The assertions here that said
//! she cannot edit are inverted, and no other.
//!
//! The rule, for a caller who does not manage the target board:
//!
//! 1. The task goes to the entry column. A different column is refused.
//! 2. The work class is `support`. `planned` is refused.
//! 3. The repository is optional. It is not part of the condition.
//!
//! Until COLLIERY-T-0218 the condition was the repository: a filing had to
//! link to one, and its lane was whatever the caller sent. This file held
//! "filing without a repository" as a NEGATIVE case, over REST and over
//! MCP. Both are positive cases now, and both are still here.
//!
//! COLLIERY-T-0217 (COLLIERY-A-0023): a repository no longer chooses the
//! board, so every filing here names the board.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_file_backlog_t0105_test`.
//!
//! Cast:
//! - `svc`   — org admin: sets the world up,
//! - `bob`   — member of team `platform` (owns `payments-api`),
//! - `alice` — member of team `web` only: she sends the requests,
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

    // The filer names the board (COLLIERY-T-0217). Until then the repository
    // alone was enough: it routed to its owner's board. This request carries
    // a repository as well; the one below it carries none.
    let filing = CreateTaskRequest {
        board_id: Some(platform_board.clone()),
        column_id: None,
        title: "Bulk invoice export".into(),
        content: "Filed from portal-web".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: Some("payments-api".into()),
    };

    // =======================================================================
    // whoami advertises the computed capability
    // =======================================================================
    let me = alice.whoami().await.expect("whoami");
    assert_eq!(me.implicit, vec!["file_backlog".to_string()]);

    // =======================================================================
    // The positive case: alice (web) sends a request to platform
    // =======================================================================
    let filed = alice
        .create_task(&filing)
        .await
        .expect("a member of another team sends a request to platform");
    assert_eq!(filed.board_id, platform_board);
    assert_eq!(filed.column_id, backlog, "lands in the entry column");
    // COLLIERY-T-0218: she sent no work class and the type is `task`, which
    // was `planned` until now. A request is `support` whatever its type.
    assert_eq!(filed.task_type, "task");
    assert_eq!(filed.work_class, "support", "a request is support work");
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

    // No repository: the create succeeds (COLLIERY-T-0218). Until then this
    // was the 403 "filing without a repository": a plain board create was
    // `manage_tasks` only.
    let no_repository = alice
        .create_task(&CreateTaskRequest {
            repository: None,
            title: "Request with no repository".into(),
            ..filing.clone()
        })
        .await
        .expect("a request needs no repository");
    assert_eq!(no_repository.board_id, platform_board);
    assert_eq!(no_repository.column_id, backlog);
    assert_eq!(no_repository.work_class, "support");
    assert!(no_repository.repository.is_none());
    assert_eq!(
        no_repository.team_id.as_deref(),
        Some(platform.id.as_str()),
        "the board decides the team of a request too"
    );

    // Naming `support` is the same as naming nothing.
    let explicit_support = alice
        .create_task(&CreateTaskRequest {
            work_class: Some("support".into()),
            ..filing.clone()
        })
        .await
        .expect("explicit support");
    assert_eq!(explicit_support.work_class, "support");

    // The type does not choose the lane of a request: a bug is support work.
    let bug = alice
        .create_task(&CreateTaskRequest {
            task_type: Some("bug".into()),
            repository: None,
            ..filing.clone()
        })
        .await
        .expect("a bug as a request");
    assert_eq!(bug.task_type, "bug");
    assert_eq!(bug.work_class, "support");

    // On the board she manages nothing changes: the KAIROS-T-0077 default,
    // any work class, any column.
    let web_board = web.delivery_board_id.clone().expect("web board");
    let own_default = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            repository: None,
            ..filing.clone()
        })
        .await
        .expect("her own board, no work class");
    assert_eq!(
        own_default.work_class, "planned",
        "a manager keeps the default"
    );
    let web_second_column = svc
        .list_columns(&web_board)
        .await
        .expect("web columns")
        .iter()
        .find(|c| c.position == 1)
        .expect("second column")
        .id
        .clone();
    let own_planned = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            column_id: Some(web_second_column.clone()),
            work_class: Some("planned".into()),
            ..filing.clone()
        })
        .await
        .expect("her own board, planned, second column");
    assert_eq!(own_planned.work_class, "planned");
    assert_eq!(own_planned.column_id, web_second_column);

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
        "a request into a column that is not the entry column",
    );
    // The same with no repository: the repository never was what let the
    // column through.
    forbidden(
        alice
            .create_task(&CreateTaskRequest {
                column_id: Some(todo.clone()),
                repository: None,
                ..filing.clone()
            })
            .await,
        "a request with no repository into a column that is not the entry column",
    );
    // Not into the planned lane — 403, never silently changed to support.
    // The refusal has the shape of the column refusal: what she lacks is
    // `manage_tasks` on platform's board.
    for repository in [Some("payments-api".to_string()), None] {
        let planned_before = svc
            .list_tasks(kairos_client::types::Pagination {
                limit: Some(200),
                offset: None,
            })
            .await
            .expect("count")
            .total;
        let err = rejection(
            alice
                .create_task(&CreateTaskRequest {
                    work_class: Some("planned".into()),
                    repository: repository.clone(),
                    ..filing.clone()
                })
                .await,
        );
        match &err {
            Error::Forbidden {
                code,
                message,
                capability,
                ..
            } => {
                assert_eq!(code, "FORBIDDEN");
                assert_eq!(capability.as_deref(), Some("manage_tasks"));
                assert!(
                    message.contains("goes to the support lane") && message.contains("`planned`"),
                    "the refusal says which lane a request uses: {message}"
                );
            }
            other => panic!("a request for the planned lane is a 403: {other}"),
        }
        let planned_after = svc
            .list_tasks(kairos_client::types::Pagination {
                limit: Some(200),
                offset: None,
            })
            .await
            .expect("count")
            .total;
        assert_eq!(
            planned_before, planned_after,
            "a refused request leaves no task"
        );
    }
    // A work class that does not exist is a 422, as for everyone.
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                work_class: Some("urgent".into()),
                ..filing.clone()
            })
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Not with a repository alone: it does not choose a board
    // (COLLIERY-T-0217). A 422 from routing, before any capability question.
    // Until then this place held a different 422: web's board with
    // platform's repository. That request is valid now; task_repositories.rs
    // has it as a positive case.
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                board_id: None,
                ..filing.clone()
            })
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // She cannot MOVE what she filed: transition, work class, board. A
    // team controls its own plan, and creation grants nothing here
    // (COLLIERY-T-0228).
    forbidden(
        alice.transition_task(&filed.short_code, &todo).await,
        "transitioning what she filed",
    );
    forbidden(
        alice
            .set_task_work_class(&filed.short_code, "support")
            .await,
        "re-laning what she filed",
    );
    forbidden(
        alice
            .set_task_work_class(&filed.short_code, "planned")
            .await,
        "planning what she filed",
    );
    forbidden(
        alice
            .move_task(&filed.short_code, web.delivery_board_id.as_deref().unwrap())
            .await,
        "moving what she filed to her own board",
    );
    // She CAN edit it: she created it (the edit rule, COLLIERY-T-0228).
    // Until then these four were refused, with `manage_tasks` as the reason.
    let edited = alice
        .update_task(
            &filed.short_code,
            &UpdateContentRequest {
                title: None,
                content: "edited".into(),
                version: filed.version,
            },
        )
        .await
        .expect("the creator edits what she filed");
    assert_eq!(edited.content, "edited");
    alice
        .set_task_repository(&filed.short_code, None)
        .await
        .expect("the creator clears the repository of what she filed");
    alice
        .set_task_repository(&filed.short_code, Some("payments-api"))
        .await
        .expect("the creator sets the repository of what she filed");
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
        .await
        .expect("the creator sets metadata on what she filed");
    alice
        .delete_task(&filed.short_code)
        .await
        .expect("the creator archives what she filed");
    alice
        .restore_task(&filed.short_code)
        .await
        .expect("the creator restores what she filed");
    // A person who did not create it, and holds nothing on the board, can
    // do none of that. carol is one.
    forbidden(
        carol
            .update_task(
                &filed.short_code,
                &UpdateContentRequest {
                    title: None,
                    content: "by carol".into(),
                    version: edited.version,
                },
            )
            .await,
        "editing what a different person filed",
    );
    forbidden(
        carol.delete_task(&filed.short_code).await,
        "deleting what a different person filed",
    );
    forbidden(
        carol.set_task_repository(&filed.short_code, None).await,
        "unbinding what a different person filed",
    );
    forbidden(
        carol
            .update_metadata(
                kairos_client::EntityKind::Task,
                &filed.short_code,
                &kairos_client::types_meta::UpdateMetadataRequest {
                    values: [("priority".to_string(), Some("low".to_string()))]
                        .into_iter()
                        .collect(),
                },
            )
            .await,
        "setting metadata on what a different person filed",
    );
    // No edit and no refusal moved the request.
    let unchanged = svc.get_task(&filed.short_code).await.expect("the request");
    assert_eq!(unchanged.column_id, backlog, "still in the entry column");
    assert_eq!(unchanged.work_class, "support");
    assert_eq!(unchanged.board_id, platform_board, "still on the board");
    assert_eq!(unchanged.content, "edited", "the edit of its creator only");
    assert!(unchanged.archived_at.is_none());
    // The receiving team CAN. bob moves the request to the planned lane with
    // the work-class endpoint: the team plans its own work (COLLIERY-A-0023).
    let planned = bob
        .set_task_work_class(&filed.short_code, "planned")
        .await
        .expect("the receiving team changes the work class");
    assert_eq!(planned.work_class, "planned");
    assert_eq!(
        planned.column_id, backlog,
        "a lane change is not a column change"
    );
    // And bob transitions it out of the entry column.
    let triaged = bob
        .transition_task(&filed.short_code, &todo)
        .await
        .expect("the receiving team triages it");
    assert_eq!(triaged.column_id, todo);
    // The initiative board is not a delivery board: no request goes there.
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                board_id: Some("initiatives".into()),
                repository: None,
                ..filing.clone()
            })
            .await,
    );
    assert!(
        matches!(err, Error::Forbidden { .. } | Error::Validation { .. }),
        "a request goes to a delivery board only: {err}"
    );
    // Another tenant's member cannot reach into acme at all.
    // alice IS a globex member, so the tenant middleware admits her there and
    // every lookup runs in globex's schema. The filing names acme's board,
    // which does not exist there: a 404, never a leak.
    let err = rejection(globex_alice.create_task(&filing).await);
    assert!(
        matches!(err, Error::NotFound { .. }),
        "another tenant never sees acme's boards: {err}"
    );
    // With a board that globex does have, the repository lookup is reached,
    // and the slug does not exist in globex: a 422 from routing, never a
    // leak (KAIROS-T-0116 pinned it).
    let err = rejection(
        globex_alice
            .create_task(&CreateTaskRequest {
                board_id: Some("initiatives".into()),
                ..filing.clone()
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "another tenant never sees acme's repositories: {err}"
    );

    // =======================================================================
    // The link rule (COLLIERY-T-0228, which replaced KAIROS-T-0111): a
    // person links what she can edit, and she can edit what she created
    // =======================================================================
    // Two initiatives on the initiative board: one alice authored (she is
    // its creator; the board is admin-managed so she holds no grant there),
    // one bob's, standing in for "the other team's".
    let initiative_board: String = {
        use kairos_db::schema::boards::dsl;
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");
        let id: Uuid = dsl::boards
            .filter(dsl::board_level.eq(kairos_db::models::enums::BoardLevel::Initiative))
            .filter(dsl::deleted_at.is_null())
            .select(dsl::id)
            .first(&mut conn)
            .expect("initiative board");
        id.to_string()
    };
    // Members cannot create initiatives on the admin-managed board; svc
    // creates one and alice's authorship is established by a task she
    // creates on her own team's board (web) instead.
    let initiative_board_id = initiative_board.clone();
    let foreign_initiative = svc
        .create_initiative(&kairos_client::types::CreateInitiativeRequest {
            board_id: initiative_board,
            column_id: None,
            title: "Platform's initiative".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("svc creates an initiative");
    let alice_own = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            column_id: None,
            title: "Alice's own item".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .expect("alice creates on her own team's board");
    let edge = |source: &str, target: &str, relationship: &str| {
        kairos_client::types_meta::CreateRelationshipRequest {
            source_short_code: source.into(),
            target_short_code: target.into(),
            relationship: relationship.into(),
        }
    };
    // blocks: filed task (source, alice created it) -> her own item: allowed.
    let blocks = alice
        .create_relationship(&edge(&filed.short_code, &alice_own.short_code, "blocks"))
        .await
        .expect("the filer marks what her filed task blocks");
    // …and she may remove it again (same gate).
    alice
        .delete_relationship(&blocks.id)
        .await
        .expect("the filer removes her own edge");
    // blocks the other way: her own item (she manages web) -> filed task: allowed.
    alice
        .create_relationship(&edge(&alice_own.short_code, &filed.short_code, "blocks"))
        .await
        .expect("manage on the source's board suffices");
    // parent under the foreign initiative. She cannot edit the initiative.
    // She can edit the TARGET, her request, and one end is sufficient
    // (COLLIERY-T-0228). Until then this was a 403: the rule looked at the
    // creator of the source only. tests/edit_rule.rs has the negative case
    // that replaces it: a person who can edit NEITHER end.
    let under_foreign = alice
        .create_relationship(&edge(
            &foreign_initiative.short_code,
            &filed.short_code,
            "parent",
        ))
        .await
        .expect("she can edit the target: her request");
    // She removes it again, so the request has no parent for what follows.
    alice
        .delete_relationship(&under_foreign.id)
        .await
        .expect("the same rule for the removal");
    // Two foreign items: 403.
    forbidden(
        alice
            .create_relationship(&edge(
                &foreign_initiative.short_code,
                &by_carol.short_code,
                "blocks",
            ))
            .await,
        "linking two items she neither manages nor authored",
    );
    // No relationship type needs the admin role (COLLIERY-T-0228). She can
    // edit the two tasks, so the refusal of `supersedes` is the TYPE rule
    // (adr -> adr only), and it is a 422. Until then it was a 403 that
    // said "admin only", and the type rule was never reached.
    let err = rejection(
        alice
            .create_relationship(&edge(
                &alice_own.short_code,
                &filed.short_code,
                "supersedes",
            ))
            .await,
    );
    assert!(
        !matches!(err, Error::Forbidden { .. }) && err.to_string().contains("RELATIONSHIP_RULE"),
        "supersedes between two tasks is refused by the type rule: {err}"
    );
    // For a person who can edit neither end, each type is a 403: carol
    // created `by_carol` only.
    for relationship in ["parent", "blocks", "supports", "informs", "supersedes"] {
        forbidden(
            carol
                .create_relationship(&edge(
                    &foreign_initiative.short_code,
                    &filed.short_code,
                    relationship,
                ))
                .await,
            "an edge between two items that she cannot edit",
        );
    }
    // The same rule over MCP create_item's `parent`. She creates the task,
    // so she can link it to a parent that she cannot edit
    // (COLLIERY-T-0228). Until then the foreign initiative was refused. A
    // parent that the TYPE rules refuse still leaves no task.
    let tasks_before = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("count")
        .total;
    let mut mcp = McpSession::open(&server.base_url, &alice_token).await;
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Parented under a foreign initiative",
                "board": platform_board,
                "repository": "payments-api",
                "parent": foreign_initiative.short_code,
            }),
        )
        .await;
    assert!(!is_error, "she creates the target of the edge: {text}");
    assert!(text.contains("parent edge created"), "{text}");
    let tasks_before = tasks_before + 1;
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Parented under a task",
                "board": platform_board,
                "parent": alice_own.short_code,
            }),
        )
        .await;
    assert!(
        is_error && text.contains("VALIDATION"),
        "a task cannot be the parent of a task: {text}"
    );
    let tasks_after = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("count")
        .total;
    assert_eq!(
        tasks_before, tasks_after,
        "a refused parent leaves no orphan task"
    );
    // An initiative alice CREATED (svc grants her manage_initiatives on the
    // initiative board so she can create one; the link rule then admits her
    // as its creator even without manage on the filed task's board).
    svc.add_board_member(
        &initiative_board_id,
        &AddBoardMemberRequest {
            user_id: alice_id.to_string(),
            capabilities: vec!["manage_initiatives".into()],
        },
    )
    .await
    .expect("grant manage_initiatives to alice");
    let alice_initiative = alice
        .create_initiative(&kairos_client::types::CreateInitiativeRequest {
            board_id: initiative_board_id.clone(),
            column_id: None,
            title: "Alice's initiative".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("alice creates her initiative");
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Parented under her own initiative",
                "board": platform_board,
                "repository": "payments-api",
                "parent": alice_initiative.short_code,
            }),
        )
        .await;
    assert!(!is_error, "a parent that she created: {text}");
    assert!(text.contains("parent edge created"), "{text}");
    let (is_error, text) = mcp
        .call(
            "link_items",
            json!({
                "source": by_carol.short_code,
                "target": alice_own.short_code,
                "relationship": "blocks",
            }),
        )
        .await;
    // by_carol is carol's, but alice manages the TARGET's board (web).
    assert!(
        !is_error,
        "MCP link_items blocks via manage on the target: {text}"
    );
    // get_item now shows the repository.
    let (_, text) = mcp
        .call("get_item", json!({ "short_code": filed.short_code }))
        .await;
    assert!(
        text.contains("repository: payments-api (owner: platform)"),
        "{text}"
    );
    drop(mcp);

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
    // Same rule over MCP: create_item with `board` from a caller who does
    // not manage that board
    // =======================================================================
    let mut mcp = McpSession::open(&server.base_url, &alice_token).await;
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Filed over MCP",
                "board": platform_board,
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
        "an MCP request lands in the entry column too"
    );
    assert_eq!(over_mcp.created_by, alice_id.to_string());
    assert_eq!(over_mcp.board_id, platform_board);
    assert_eq!(
        over_mcp.work_class, "support",
        "an MCP request is support work too"
    );
    // With no `board`, acme has two delivery boards and so no default. The
    // repository does not supply one (COLLIERY-T-0217).
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "No board named",
                "repository": "payments-api",
            }),
        )
        .await;
    assert!(
        is_error && text.contains("pass `board`"),
        "a repository alone does not choose the board: {text}"
    );
    // No repository: the request is created (COLLIERY-T-0218). Until then
    // this call was refused, under the title "Not allowed".
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "task_type": "bug",
                "title": "MCP request with no repository",
                "board": platform_board,
            }),
        )
        .await;
    assert!(!is_error, "an MCP request needs no repository: {text}");
    let code_start = text.find("ACME-T-").expect("short code in the reply");
    let code: String = text[code_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let bare = svc.get_task(&code).await.expect("the MCP request");
    assert_eq!(bare.column_id, backlog);
    assert_eq!(bare.work_class, "support");
    assert_eq!(bare.task_type, "bug");
    assert!(bare.repository.is_none());
    // `planned` is refused over MCP by the same function, with the same
    // text, and no task is made.
    let tasks_before = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("count")
        .total;
    for arguments in [
        json!({
            "item_type": "task",
            "title": "Planned over MCP",
            "board": platform_board,
            "work_class": "planned",
        }),
        json!({
            "item_type": "task",
            "title": "Planned over MCP",
            "board": platform_board,
            "repository": "payments-api",
            "work_class": "planned",
        }),
    ] {
        let (is_error, text) = mcp.call("create_item", arguments).await;
        assert!(
            is_error
                && text.contains("FORBIDDEN")
                && text.contains("goes to the support lane")
                && text.contains("manage_tasks"),
            "an MCP request for the planned lane is refused: {text}"
        );
    }
    let tasks_after = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("count")
        .total;
    assert_eq!(
        tasks_before, tasks_after,
        "a refused MCP request leaves no task"
    );
    // On her own board the MCP defaults are the old ones.
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Her own planned work over MCP",
                "board": web_board,
            }),
        )
        .await;
    assert!(!is_error, "{text}");
    let code_start = text.find("ACME-T-").expect("short code in the reply");
    let code: String = text[code_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    assert_eq!(
        svc.get_task(&code).await.expect("her task").work_class,
        "planned"
    );
    // She cannot move a request out of the entry column over MCP, and the
    // refusal tells her the request rule: she filed it, and it is there.
    let (is_error, text) = mcp
        .call(
            "transition_item",
            json!({"short_code": over_mcp.short_code, "to_column": "Todo"}),
        )
        .await;
    assert!(
        is_error
            && text.contains("FORBIDDEN")
            && text.contains("is a request in the entry column of the board of team platform")
            && text.contains("file_backlog"),
        "{text}"
    );
    // The explanation says what is true since COLLIERY-T-0228: she can
    // edit, link and archive the request, and she cannot move it.
    assert!(
        text.contains("so you can edit it, link it and archive it")
            && text.contains("To move it, you need \"transition_items\"")
            && !text.contains("To move, edit or delete it"),
        "{text}"
    );
    // `filed` is hers too, but bob moved it out of the entry column above:
    // the plain refusal, which does not name the entry column. Until
    // COLLIERY-T-0228 the refused write here was `update_item`. That is an
    // edit, and she created the task, so it is allowed now; a move is what
    // she is still refused.
    let (is_error, text) = mcp
        .call(
            "transition_item",
            json!({"short_code": filed.short_code, "to_column": "Active"}),
        )
        .await;
    assert!(
        is_error && text.contains("FORBIDDEN") && text.contains("transition_items"),
        "{text}"
    );
    assert!(
        !text.contains("entry column") && !text.contains("file_backlog"),
        "a card that left the entry column is not described as in it: {text}"
    );
    let filed_now = svc.get_task(&filed.short_code).await.expect("the request");
    let (is_error, text) = mcp
        .call(
            "update_item",
            json!({"short_code": filed.short_code, "content": "edited over MCP",
                   "version": filed_now.version}),
        )
        .await;
    assert!(!is_error, "the creator edits over MCP: {text}");
    assert_eq!(
        svc.get_task(&filed.short_code)
            .await
            .expect("the request")
            .column_id,
        todo,
        "an edit does not move the card"
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
