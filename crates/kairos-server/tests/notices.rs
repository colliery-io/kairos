//! Notices (KAIROS-T-0737, COLLIERY-I-0664 decision 9). The acceptance
//! criteria:
//!
//! 1. A person sends a notice to a person, and to each member of a team,
//!    with an item link (REST `POST /api/notices`).
//! 2. A notice from an agent key (MCP `send_notice`) names the key and its
//!    person; `to: me` tells the person of the key.
//! 3. A rate limit for each key: over the limit, 429 NOTICE_RATE_LIMITED
//!    and nothing is sent. Another key, and the person, keep their own
//!    count. A rotated key keeps the count.
//! 4. A recipient mutes a key: no notification from that key, nor from the
//!    key that replaces it by a rotation. Unmute: notifications again.
//! 5. The refusals: both or neither target, an empty text, an unknown
//!    item, a service account.
//!
//! Runs against the dev Postgres and Dex. Owns the scratch database
//! `kairos_notices_t0737_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_notifications::{
    Notification, SendNoticeRequest, UpdateNotificationSettingsRequest,
};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_service_accounts::{CreateApiKeyRequest, CreateServiceAccountRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_notices_t0737_test";

/// The limit of this test: 3 notices in one hour for each key.
const LIMIT: u32 = 3;

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn key_request(name: &str) -> CreateApiKeyRequest {
    CreateApiKeyRequest {
        name: name.into(),
        expires_at: None,
        no_expiry: false,
        scope: None,
    }
}

fn to(person: &str, text: &str) -> SendNoticeRequest {
    SendNoticeRequest {
        to: Some(person.into()),
        team: None,
        text: text.into(),
        short_code: None,
    }
}

/// One JSON-RPC message of an MCP response: plain JSON or SSE `data:`.
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|value| value.get("jsonrpc").is_some())
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
}

/// A minimal MCP session over HTTP.
struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    session: Option<String>,
    next_id: i64,
}

impl Mcp {
    async fn post(&mut self, body: Value) -> (reqwest::StatusCode, String) {
        let mut request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("x-tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.expect("MCP request");
        let status = response.status();
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().expect("session id").to_string());
        }
        (status, response.text().await.expect("MCP body"))
    }

    async fn connect(base_url: &str, token: &str) -> Mcp {
        let mut mcp = Mcp {
            http: reqwest::Client::new(),
            url: format!("{base_url}/mcp"),
            token: token.to_string(),
            session: None,
            next_id: 0,
        };
        let (status, body) = mcp
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0737-notices", "version": "0.0.0"},
                },
            }))
            .await;
        assert!(status.is_success(), "initialize: {body}");
        let (status, body) = mcp
            .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert!(status.is_success(), "initialized: {body}");
        mcp
    }

    /// The first text block of a tool call (the answer), and whether it is
    /// an error.
    async fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        self.next_id += 1;
        let (status, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await;
        assert!(status.is_success(), "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string();
        (text, result["isError"].as_bool().unwrap_or(false))
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }
}

/// The notices of a person, newest first.
async fn notices_of(client: &kairos_client::KairosClient) -> Vec<Notification> {
    client
        .notifications()
        .await
        .expect("notifications")
        .items
        .into_iter()
        .filter(|n| n.kind == "notice")
        .collect()
}

#[tokio::test]
async fn notices_go_to_a_person_or_a_team_with_a_limit_and_a_mute() {
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
    config.notice_limit_per_hour = LIMIT;
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
    diesel::update(users::table.filter(users::email.eq("bob@kairos.test")))
        .set(users::user_name.eq("bob"))
        .execute(&mut conn)
        .expect("user name");
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
        .expect("team");
    for member in [bob_id, carol_id] {
        admin
            .add_team_member(
                &web.id,
                &AddTeamMemberRequest {
                    user_id: member.to_string(),
                },
            )
            .await
            .expect("member of web");
    }
    let task = admin
        .create_task(&CreateTaskRequest {
            board_id: web.delivery_board_id.clone(),
            column_id: None,
            title: "Ship the page".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("task");

    // --- 1. A person to a person, and to a team with an item -----------------
    let sent = alice
        .send_notice(&to("bob", "The review is ready."))
        .await
        .expect("notice to bob");
    assert_eq!(sent.told, vec![bob_id.to_string()]);
    assert_eq!(sent.remaining, Some(i64::from(LIMIT) - 1));
    let got = notices_of(&bob).await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0]
            .text
            .ends_with("sent you a notice: The review is ready."),
        "{}",
        got[0].text
    );
    assert_eq!(got[0].actor_id, Some(alice_id.to_string()));
    assert_eq!(got[0].agent_key_id, None, "a person: no key");
    assert_eq!(got[0].read_at, None);

    let sent = alice
        .send_notice(&SendNoticeRequest {
            to: None,
            team: Some("web".into()),
            text: "Look at this task.".into(),
            short_code: Some(task.short_code.to_lowercase()),
        })
        .await
        .expect("notice to web");
    let mut told = sent.told.clone();
    told.sort();
    let mut members = vec![bob_id.to_string(), carol_id.to_string()];
    members.sort();
    assert_eq!(told, members, "each member of the team");
    assert_eq!(sent.remaining, Some(i64::from(LIMIT) - 2));
    for client in [&bob, &carol] {
        let newest = notices_of(client).await.remove(0);
        assert_eq!(newest.short_code.as_deref(), Some(task.short_code.as_str()));
        assert!(
            newest
                .text
                .ends_with("sent a notice to the team Web: Look at this task."),
            "{}",
            newest.text
        );
    }
    assert!(notices_of(&alice).await.is_empty(), "not the sender");

    // --- 2. An agent key: the label, and `to: me` ----------------------------
    let laptop = alice
        .create_agent_key(&key_request("laptop"))
        .await
        .expect("agent key");
    let mut agent = Mcp::connect(&server.base_url, &laptop.key).await;
    let text = agent
        .call_ok(
            "send_notice",
            json!({"to": "bob", "text": "Build 12 is green.", "short_code": task.short_code}),
        )
        .await;
    assert!(text.starts_with("Sent the notice "), "{text}");
    assert!(text.contains("to the person "), "{text}");
    assert!(text.contains("1 person got a notification."), "{text}");
    assert!(
        text.contains(&format!("You can send {} more notice(s)", LIMIT - 1)),
        "the key counts by itself: {text}"
    );
    let newest = notices_of(&bob).await.remove(0);
    assert_eq!(newest.agent_key_id.as_deref(), Some(laptop.id.as_str()));
    assert_eq!(newest.agent_key_name.as_deref(), Some("laptop"));
    assert_eq!(newest.actor_id, Some(alice_id.to_string()));
    assert!(
        newest.text.starts_with("The agent key laptop of ")
            && newest
                .text
                .ends_with("sent you a notice: Build 12 is green."),
        "{}",
        newest.text
    );
    let text = agent
        .call_ok(
            "send_notice",
            json!({"to": "me", "text": "The task is done."}),
        )
        .await;
    assert!(text.contains("1 person got a notification."), "{text}");
    let own = notices_of(&alice).await;
    assert_eq!(own.len(), 1, "an agent tells its own person: {own:?}");
    assert_eq!(own[0].agent_key_id.as_deref(), Some(laptop.id.as_str()));

    // --- 3. The rate limit ---------------------------------------------------
    let text = agent
        .call_ok("send_notice", json!({"to": "bob", "text": "Third."}))
        .await;
    assert!(text.contains("You can send 0 more notice(s)"), "{text}");
    let bob_before = notices_of(&bob).await.len();
    let (text, error) = agent
        .call("send_notice", json!({"to": "bob", "text": "Fourth."}))
        .await;
    assert!(error, "{text}");
    assert!(text.starts_with("NOTICE_RATE_LIMITED:"), "{text}");
    assert!(
        text.contains(&format!("You sent {LIMIT} notices")),
        "{text}"
    );
    assert_eq!(notices_of(&bob).await.len(), bob_before, "nothing sent");
    // The same refusal over REST with the key, with the details.
    let laptop_rest = server.client(&laptop.key, "acme");
    let err = laptop_rest
        .send_notice(&to("bob", "Fourth again."))
        .await
        .expect_err("over the limit");
    assert_eq!(err.status(), Some(429), "{err}");
    assert_eq!(err.code(), Some("NOTICE_RATE_LIMITED"), "{err}");
    // A rotated key keeps the count of the key that it replaced.
    let rotated = alice
        .rotate_agent_key(&laptop.id)
        .await
        .expect("rotate the key");
    let mut rotated_agent = Mcp::connect(&server.base_url, &rotated.key).await;
    let (text, error) = rotated_agent
        .call(
            "send_notice",
            json!({"to": "bob", "text": "After the rotation."}),
        )
        .await;
    assert!(error && text.starts_with("NOTICE_RATE_LIMITED:"), "{text}");
    // Another key has its own count; the person with no key too.
    let desk = alice
        .create_agent_key(&key_request("desk"))
        .await
        .expect("second key");
    let mut desk_agent = Mcp::connect(&server.base_url, &desk.key).await;
    desk_agent
        .call_ok(
            "send_notice",
            json!({"to": "bob", "text": "From the desk."}),
        )
        .await;
    let sent = alice
        .send_notice(&to("bob", "From the person."))
        .await
        .expect("the person has a free place");
    assert_eq!(sent.remaining, Some(0));
    let err = alice
        .send_notice(&to("bob", "One more."))
        .await
        .expect_err("the person is at the limit");
    assert_eq!(err.code(), Some("NOTICE_RATE_LIMITED"), "{err}");

    // --- 4. A mute -----------------------------------------------------------
    let settings = bob
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            mute_keys: Some(vec![desk.id.clone()]),
            ..Default::default()
        })
        .await
        .expect("mute");
    assert_eq!(settings.muted_keys.len(), 1, "{settings:?}");
    assert_eq!(settings.muted_keys[0].id, desk.id);
    assert_eq!(settings.muted_keys[0].name, "desk");
    assert!(settings.notice, "the kind stays on");
    let text = desk_agent
        .call_ok(
            "send_notice",
            json!({"team": "web", "text": "Muted for bob.", "request_key": "notice-mute-1"}),
        )
        .await;
    assert!(text.contains("1 person got a notification."), "{text}");
    let bob_notices = notices_of(&bob).await;
    assert!(
        bob_notices
            .iter()
            .all(|n| !n.text.ends_with("Muted for bob.")),
        "bob muted the key: {bob_notices:?}"
    );
    assert!(
        notices_of(&carol).await[0].text.ends_with("Muted for bob."),
        "carol did not mute it"
    );
    // A retry with the same request_key sends nothing more.
    let replay = desk_agent
        .call_ok(
            "send_notice",
            json!({"team": "web", "text": "Muted for bob.", "request_key": "notice-mute-1"}),
        )
        .await;
    assert!(replay.contains("Sent the notice "), "{replay}");
    assert_eq!(
        notices_of(&carol)
            .await
            .iter()
            .filter(|n| n.text.ends_with("Muted for bob."))
            .count(),
        1,
        "a replay sends nothing"
    );
    // A mute of the old key holds for the key that replaced it.
    bob.update_notification_settings(&UpdateNotificationSettingsRequest {
        mute_keys: Some(vec![laptop.id.clone()]),
        ..Default::default()
    })
    .await
    .expect("mute the old key");
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("search path");
    diesel::sql_query("DELETE FROM notices")
        .execute(&mut conn)
        .expect("a new hour");
    let text = rotated_agent
        .call_ok(
            "send_notice",
            json!({"to": "bob", "text": "Rotated, muted."}),
        )
        .await;
    assert!(text.contains("Nobody got a notification"), "{text}");
    // Unmute: the notices come again.
    let settings = bob
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            unmute_keys: Some(vec![desk.id.clone(), laptop.id.clone()]),
            ..Default::default()
        })
        .await
        .expect("unmute");
    assert!(settings.muted_keys.is_empty(), "{settings:?}");
    desk_agent
        .call_ok("send_notice", json!({"to": "bob", "text": "Unmuted."}))
        .await;
    assert!(notices_of(&bob).await[0].text.ends_with("Unmuted."));
    // The kind off: nothing.
    carol
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            notice: Some(false),
            ..Default::default()
        })
        .await
        .expect("notices off");
    let sent = alice
        .send_notice(&to("carol@kairos.test", "Off."))
        .await
        .expect("sent");
    assert!(sent.told.is_empty(), "carol turned notices off");
    // A mute of an id that is not an agent key is refused and named.
    let unknown = Uuid::new_v4().to_string();
    let err = bob
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            mute_keys: Some(vec![unknown.clone()]),
            ..Default::default()
        })
        .await
        .expect_err("not a key");
    assert_eq!(err.status(), Some(404), "{err}");
    assert!(err.to_string().contains(&unknown), "{err}");

    // --- 5. The refusals -----------------------------------------------------
    let both = SendNoticeRequest {
        to: Some("bob".into()),
        team: Some("web".into()),
        text: "x".into(),
        short_code: None,
    };
    let err = bob.send_notice(&both).await.expect_err("both");
    assert_eq!(err.status(), Some(422), "{err}");
    let neither = SendNoticeRequest {
        text: "x".into(),
        ..Default::default()
    };
    let err = bob.send_notice(&neither).await.expect_err("neither");
    assert_eq!(err.status(), Some(422), "{err}");
    let err = bob.send_notice(&to("me", "  ")).await.expect_err("empty");
    assert_eq!(err.status(), Some(422), "{err}");
    let err = bob
        .send_notice(&to("me", &"x".repeat(2001)))
        .await
        .expect_err("long");
    assert_eq!(err.status(), Some(422), "{err}");
    let err = bob
        .send_notice(&SendNoticeRequest {
            short_code: Some("WEB-T-9999".into()),
            ..to("me", "x")
        })
        .await
        .expect_err("unknown item");
    assert_eq!(err.status(), Some(404), "{err}");
    let err = bob
        .send_notice(&SendNoticeRequest {
            to: None,
            team: Some("nope".into()),
            text: "x".into(),
            short_code: None,
        })
        .await
        .expect_err("unknown team");
    assert_eq!(err.status(), Some(404), "{err}");
    let err = bob
        .send_notice(&to("nobody", "x"))
        .await
        .expect_err("unknown person");
    assert_eq!(err.status(), Some(422), "{err}");
    let account = admin
        .create_service_account(&CreateServiceAccountRequest { name: "bot".into() })
        .await
        .expect("service account");
    let bot_key = admin
        .create_api_key(&account.id, &key_request("k"))
        .await
        .expect("key");
    let bot = server.client(&bot_key.key, "acme");
    let err = bot
        .send_notice(&to("bob", "From a bot."))
        .await
        .expect_err("a service account");
    assert_eq!(err.status(), Some(403), "{err}");
    let err = alice
        .send_notice(&to("bot", "To a bot."))
        .await
        .expect_err("to a service account");
    assert_eq!(err.status(), Some(422), "{err}");
}
