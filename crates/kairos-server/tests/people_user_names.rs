//! KAIROS-T-0665 — the REST routes that the web reads for people give the
//! user name: the name to write after `@` for a mention. The team members,
//! the board members, the claim of a task and the author of a note each
//! have it, and it is the user name that MCP `list_members` gives
//! (KAIROS-T-0556).
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_people_user_names_t0665_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::EntityKind;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_people_user_names_t0665_test";

/// The id and the user name of the user with `email`.
fn user(conn: &mut PgConnection, email: &str) -> (Uuid, String) {
    users::table
        .filter(users::email.eq(email))
        .select((users::id, users::user_name))
        .first(conn)
        .expect("user row")
}

/// A GET: the status and the JSON body.
async fn get(http: &reqwest::Client, url: String, token: &str) -> (u16, Value) {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header("X-Tenant", "acme")
        .send()
        .await
        .expect("request");
    let status = response.status().as_u16();
    (
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
    )
}

/// One tool call over MCP: `(is_error, text)`. It opens a session first.
async fn mcp_call(base_url: &str, token: &str, tool: &str, arguments: Value) -> (bool, String) {
    let http = reqwest::Client::new();
    let url = format!("{base_url}/mcp");
    let post = |body: Value| {
        http.post(&url)
            .bearer_auth(token)
            .header("X-Tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body)
            .send()
    };
    let response = post(json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "kairos-t0665-test", "version": "0.0.0"},
        },
    }))
    .await
    .expect("initialize");
    assert_eq!(response.status(), 200, "initialize");
    let response = post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await
        .expect("initialized");
    assert!(response.status().is_success(), "initialized");
    let response = post(json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }))
    .await
    .expect("tools/call");
    assert_eq!(response.status(), 200, "tools/call");
    let body = response.text().await.expect("body");
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .filter(|v| v.get("jsonrpc").is_some())
        .or_else(|| {
            body.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                .find(|v| v.get("jsonrpc").is_some())
        })
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"));
    let result = &message["result"];
    (
        result["isError"].as_bool().unwrap_or(false),
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text: {message}"))
            .to_string(),
    )
}

/// The user names of a REST member list, by user id.
fn names_by_id(list: &Value) -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> = list
        .as_array()
        .unwrap_or_else(|| panic!("a list: {list}"))
        .iter()
        .map(|member| {
            (
                member["user_id"].as_str().expect("user_id").to_string(),
                member["user_name"]
                    .as_str()
                    .unwrap_or_else(|| panic!("user_name on {member}"))
                    .to_string(),
            )
        })
        .collect();
    names.sort();
    names
}

/// The user names of the lines of an MCP `list_members` text
/// (`- <user name> — <display name> <email> · ...`).
fn mcp_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .filter_map(|line| line.split(" — ").next())
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_people_routes_give_the_user_name_of_list_members_against_live_stack() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let carol_token = user_token(&http, "carol").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let base = server.base_url.clone();
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    for token in [&svc_token, &alice_token, &bob_token, &carol_token] {
        let _ = server.client(token, "acme").whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
    ] {
        let (user_id, _) = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("membership");
    }
    let (alice_id, alice_name) = user(&mut conn, "alice@kairos.test");
    let (bob_id, bob_name) = user(&mut conn, "bob@kairos.test");
    let (carol_id, carol_name) = user(&mut conn, "carol@kairos.test");
    for name in [&alice_name, &bob_name, &carol_name] {
        assert!(!name.is_empty(), "each user has a user name");
    }

    // alice and bob are in the team; carol has a grant on the board only.
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    for user_id in [alice_id, bob_id] {
        admin
            .add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let board = team.delivery_board_id.clone().expect("delivery board");
    let granted = admin
        .add_board_member(
            &board,
            &AddBoardMemberRequest {
                user_id: carol_id.to_string(),
                capabilities: vec!["manage_tasks".into()],
            },
        )
        .await
        .expect("grant");
    assert_eq!(granted.user_name, carol_name, "the add answers with it");

    // --- the team members -----------------------------------------------
    let (status, members) = get(&http, format!("{base}/api/teams/web/members"), &bob_token).await;
    assert_eq!(status, 200, "{members}");
    let mut want = vec![
        (alice_id.to_string(), alice_name.clone()),
        (bob_id.to_string(), bob_name.clone()),
    ];
    want.sort();
    assert_eq!(names_by_id(&members), want, "{members}");
    let (is_error, text) =
        mcp_call(&base, &bob_token, "list_members", json!({"team": "web"})).await;
    assert!(!is_error, "{text}");
    let mut rest: Vec<String> = want.iter().map(|(_, name)| name.clone()).collect();
    rest.sort();
    assert_eq!(mcp_names(&text), rest, "REST and MCP agree: {text}");

    // --- the board members (the grants) -----------------------------------
    let (status, members) = get(
        &http,
        format!("{base}/api/boards/web-delivery/members"),
        &bob_token,
    )
    .await;
    assert_eq!(status, 200, "{members}");
    assert_eq!(
        names_by_id(&members),
        vec![(carol_id.to_string(), carol_name.clone())],
        "{members}"
    );
    let (is_error, text) = mcp_call(
        &base,
        &bob_token,
        "list_members",
        json!({"board": "web-delivery"}),
    )
    .await;
    assert!(!is_error, "{text}");
    // MCP adds the team of the board to the grants; each REST name is there.
    let mcp = mcp_names(&text);
    for name in [&alice_name, &bob_name, &carol_name] {
        assert!(mcp.contains(name), "{name} in {text}");
    }

    // --- the claim and the author of a note -------------------------------
    let detail = admin.get_board(&board).await.expect("board");
    let column = |name: &str| {
        detail
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name}"))
            .id
            .clone()
    };
    let task = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(board.clone()),
            column_id: None,
            title: "Mention".into(),
            content: "one".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("task");
    let code = task.short_code.clone();
    alice
        .transition_task(&code, &column("Todo"))
        .await
        .expect("to Todo");
    alice
        .transition_task(&code, &column("Active"))
        .await
        .expect("to Active");
    alice
        .append_note(EntityKind::Task, &code, &format!("Ask @{bob_name}."))
        .await
        .expect("note");
    let (status, item) = get(&http, format!("{base}/api/tasks/{code}"), &bob_token).await;
    assert_eq!(status, 200, "{item}");
    assert_eq!(item["claim"]["user_id"], json!(alice_id.to_string()));
    assert_eq!(item["claim"]["user_name"], json!(alice_name), "{item}");
    assert_eq!(
        item["notes"][0]["author_user_name"],
        json!(alice_name),
        "{item}"
    );
    // The items of the board give the claim with the user name too.
    let (status, items) = get(
        &http,
        format!("{base}/api/boards/{board}/items"),
        &bob_token,
    )
    .await;
    assert_eq!(status, 200, "{items}");
    let listed = find_code(&items, &code).unwrap_or_else(|| panic!("{code} in {items}"));
    assert_eq!(listed["claim"]["user_name"], json!(alice_name), "{listed}");
}

/// The object with `short_code` = `code` anywhere in `value`.
fn find_code<'a>(value: &'a Value, code: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) if map.get("short_code") == Some(&json!(code)) => Some(value),
        Value::Object(map) => map.values().find_map(|v| find_code(v, code)),
        Value::Array(list) => list.iter().find_map(|v| find_code(v, code)),
        _ => None,
    }
}
