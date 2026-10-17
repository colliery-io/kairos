//! A cancelled blocker is unresolved, not done (KAIROS-T-0567,
//! COLLIERY-I-0641):
//!
//! 1. A cancel of a blocker fires no `unblocked` rule and sends no
//!    `blocker_cleared`. The claimant and the creator of the dependant get
//!    `blocker_cancelled`, with the reason.
//! 2. The `blocks` edge stays: REST marks the blocker `cancelled`, MCP
//!    `get_item` marks it `[cancelled]`, and the card still counts it in
//!    "blocked by".
//! 3. While the cancelled blocker stays, a second blocker that is done
//!    fires nothing. An unlink of the cancelled blocker then fires the rule.
//! 4. A reopen of a cancelled blocker sends no `blocked_again`.
//! 5. Children progress counts done, cancelled and open apart.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_cancelled_blocker_t0567_test`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateInitiativeRequest, CreateTaskRequest};
use kairos_client::types_dispatch::{CreateDispatchRuleRequest, CreateWebhookTargetRequest};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_org::{
    AddTeamMemberRequest, BoardItemsQuery, CreateTeamRequest, CreateTransitionRequest,
};
use kairos_client::{EntityKind, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;

const SCRATCH_DB: &str = "kairos_cancelled_blocker_t0567_test";

#[derive(QueryableByName, Debug)]
struct Note {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Text)]
    text: String,
}

#[derive(QueryableByName, Debug)]
struct Sent {
    #[diesel(sql_type = Text)]
    event: String,
    #[diesel(sql_type = Text)]
    body: String,
}

/// The notifications after the first `seen`, as (person, kind, text),
/// sorted.
fn new_notes(conn: &mut PgConnection, seen: &mut usize) -> Vec<(Uuid, String, String)> {
    let rows: Vec<Note> = diesel::sql_query(
        "SELECT user_id, kind, text FROM org_acme.notifications ORDER BY created_at, id",
    )
    .load(conn)
    .expect("notifications");
    let mut new: Vec<(Uuid, String, String)> = rows
        .into_iter()
        .skip(*seen)
        .map(|n| (n.user_id, n.kind, n.text))
        .collect();
    *seen += new.len();
    new.sort();
    new
}

/// `(code, blocker)` of each `task.unblocked` delivery after the first
/// `seen` deliveries.
fn new_unblocked(conn: &mut PgConnection, seen: &mut usize) -> Vec<(String, String)> {
    let rows: Vec<Sent> = diesel::sql_query(
        "SELECT event, body FROM org_acme.webhook_deliveries ORDER BY created_at, id",
    )
    .load(conn)
    .expect("deliveries");
    let new: Vec<Sent> = rows.into_iter().skip(*seen).collect();
    *seen += new.len();
    new.into_iter()
        .filter(|d| d.event == "task.unblocked")
        .map(|d| {
            let body: Value = serde_json::from_str(&d.body).expect("body");
            (
                body["data"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                body["data"]["blocker"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

async fn task(client: &KairosClient, board: &str, title: &str) -> String {
    client
        .create_task(&CreateTaskRequest {
            board_id: Some(board.to_string()),
            column_id: None,
            title: title.into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
        })
        .await
        .expect("task")
        .short_code
}

async fn link(client: &KairosClient, source: &str, target: &str, relationship: &str) -> String {
    client
        .create_relationship(&CreateRelationshipRequest {
            source_short_code: source.into(),
            target_short_code: target.into(),
            relationship: relationship.into(),
        })
        .await
        .expect("edge")
        .id
}

/// Move a task through the columns, one hop each.
async fn moves(
    client: &KairosClient,
    code: &str,
    columns: &HashMap<String, String>,
    path: &[&str],
) {
    for name in path {
        client
            .transition_task(code, &columns[*name])
            .await
            .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
    }
}

/// The text of the MCP tool `get_item` for `code`.
async fn get_item(base_url: &str, token: &str, code: &str) -> String {
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
            "clientInfo": {"name": "kairos-t0567-test", "version": "0.0.0"},
        },
    }))
    .await
    .expect("initialize");
    assert_eq!(response.status(), 200, "initialize");
    let response = post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await
        .expect("initialized");
    assert_eq!(response.status(), 202, "initialized");
    let response = post(json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "get_item", "arguments": {"short_code": code}},
    }))
    .await
    .expect("tools/call");
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
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"));
    message["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text: {message}"))
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancelled_blocker_still_blocks() {
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
    let mut config = base_config(&scratch_url);
    config.secrets_key = Some(SecretsKey::from_bytes([9u8; 32]));
    config.webhooks.allow_private = true;
    let state = app::state_with(config, pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");
    for client in [&admin, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
    ] {
        let user_id = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let alice_id = user(&mut conn, "alice@kairos.test");
    let bob_id = user(&mut conn, "bob@kairos.test");
    let carol_id = user(&mut conn, "carol@kairos.test");

    let web = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team web");
    for user_id in [alice_id, bob_id, carol_id] {
        admin
            .add_team_member(
                &web.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let board = web.delivery_board_id.clone().expect("web board");
    // KAIROS-T-0641: the tasks walk to Active under an initiative that
    // stays in Discovery.
    common::parent_gate_off(&admin, &board).await;
    let detail = admin.get_board(&board).await.expect("board");
    let slug = detail.board.slug.clone();
    let columns: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    admin
        .add_transition(
            &board,
            &CreateTransitionRequest {
                from_column_id: columns["Completed"].clone(),
                to_column_id: columns["Todo"].clone(),
            },
        )
        .await
        .expect("Completed -> Todo");
    let receiver = common::hook_http::serve().await;
    let target = admin
        .create_webhook_target(
            &slug,
            &CreateWebhookTargetRequest {
                name: "Runner".into(),
                url: receiver.url.clone(),
            },
        )
        .await
        .expect("target")
        .target
        .id;
    admin
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("unblocked".into()),
                column: None,
                target_id: target,
                task_type: None,
                metadata_slug: None,
                metadata_value: None,
                repository: None,
            },
        )
        .await
        .expect("unblocked rule");
    let mut notes = 0usize;
    let mut sent = 0usize;

    // --- 1. The cancel ------------------------------------------------------
    // carol made the dependant, alice has the claim, bob cancels a blocker.
    let cancelled = task(&bob, &board, "Cancelled blocker").await;
    let second = task(&bob, &board, "Second blocker").await;
    let waiting = task(&carol, &board, "Waits for two").await;
    let edge = link(&bob, &cancelled, &waiting, "blocks").await;
    link(&bob, &second, &waiting, "blocks").await;
    moves(&alice, &waiting, &columns, &["Todo", "Active"]).await;
    let _ = new_notes(&mut conn, &mut notes);
    let _ = new_unblocked(&mut conn, &mut sent);
    bob.cancel_task(&cancelled, "The customer dropped it")
        .await
        .expect("cancel");
    assert!(new_unblocked(&mut conn, &mut sent).is_empty());
    let text = format!(
        "{cancelled} is cancelled, and it still blocks {waiting}. Remove the blocks link, or \
         reopen {cancelled}. The reason: The customer dropped it"
    );
    let mut want = vec![
        (alice_id, "blocker_cancelled".to_string(), text.clone()),
        (carol_id, "blocker_cancelled".to_string(), text.clone()),
    ];
    want.sort();
    assert_eq!(new_notes(&mut conn, &mut notes), want);

    // --- 2. The edge stays, marked cancelled ----------------------------------
    let relationships = alice
        .relationships(EntityKind::Task, &waiting)
        .await
        .expect("relationships");
    let blockers: Vec<(String, bool, bool)> = relationships
        .incoming
        .iter()
        .filter(|g| g.relationship == "blocks")
        .flat_map(|g| g.items.iter())
        .map(|i| (i.short_code.clone(), i.done, i.cancelled))
        .collect();
    assert!(
        blockers.contains(&(cancelled.clone(), true, true)),
        "{blockers:?}"
    );
    let text = get_item(&server.base_url, &alice_token, &waiting).await;
    assert!(
        text.contains(&format!("{cancelled} — Cancelled blocker [cancelled]")),
        "{text}"
    );
    assert!(!text.contains("[done]"), "{text}");
    let items = alice
        .board_items(&board, &BoardItemsQuery::default())
        .await
        .expect("board items");
    assert_eq!(
        items.blocks_summary.get(&waiting).map(|c| c.blocked_by),
        Some(2),
        "the cancelled blocker still counts: {:?}",
        items.blocks_summary
    );

    // --- 3. The other blocker is done: nothing fires -------------------------
    moves(&bob, &second, &columns, &["Todo", "Active", "Completed"]).await;
    assert!(new_unblocked(&mut conn, &mut sent).is_empty());
    let told = new_notes(&mut conn, &mut notes);
    assert_eq!(
        told,
        vec![(
            alice_id,
            "blocker_cleared".to_string(),
            format!("{second} does not block {waiting} now: it is done.")
        )]
    );
    // A person removes the cancelled blocker: now it fires.
    bob.delete_relationship(&edge).await.expect("unlink");
    assert_eq!(
        new_unblocked(&mut conn, &mut sent),
        vec![(waiting.clone(), cancelled.clone())]
    );

    // --- 4. A reopen of a cancelled blocker ---------------------------------
    let reopened = task(&bob, &board, "Cancelled, then reopened").await;
    let other = task(&carol, &board, "Waits for the reopen").await;
    link(&bob, &reopened, &other, "blocks").await;
    bob.cancel_task(&reopened, "Not now").await.expect("cancel");
    let _ = new_notes(&mut conn, &mut notes);
    moves(&bob, &reopened, &columns, &["Todo"]).await;
    assert!(
        new_notes(&mut conn, &mut notes).is_empty(),
        "it never stopped blocking"
    );
    assert!(new_unblocked(&mut conn, &mut sent).is_empty());

    // --- 5. Children progress ------------------------------------------------
    let initiative = admin
        .create_initiative(&CreateInitiativeRequest {
            board_id: "initiatives".into(),
            column_id: None,
            title: "Five children".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("initiative")
        .short_code;
    let mut children = Vec::new();
    for n in 0..5 {
        let child = task(&bob, &board, &format!("Child {n}")).await;
        link(&admin, &initiative, &child, "parent").await;
        children.push(child);
    }
    for child in &children[..3] {
        moves(&bob, child, &columns, &["Todo", "Active", "Completed"]).await;
    }
    bob.cancel_task(&children[3], "Out of scope")
        .await
        .expect("cancel");
    let progress = alice
        .children_progress(EntityKind::Initiative, &initiative)
        .await
        .expect("progress");
    assert_eq!(
        (
            progress.done,
            progress.cancelled,
            progress.open,
            progress.total
        ),
        (3, 1, 1, 5)
    );
    let done_column: i64 = progress
        .by_column
        .iter()
        .filter(|c| c.is_done)
        .map(|c| c.cancelled)
        .sum();
    assert_eq!(done_column, 1, "{:?}", progress.by_column);
    let items = alice
        .board_items("initiatives", &BoardItemsQuery::default())
        .await
        .expect("initiative board items");
    let counts = items
        .children_progress
        .get(&initiative)
        .expect("progress of the initiative");
    assert_eq!(
        (counts.done, counts.cancelled, counts.open, counts.total),
        (3, 1, 1, 5)
    );
}
