//! The notification inbox of a person (KAIROS-T-0381, COLLIERY-I-0614).
//! The acceptance criteria:
//!
//! 1. Each of the 5 events makes one notification for the right person:
//!    a hand-off, a blocker that is done or archived, a cancel, a mention,
//!    and a P0 bug on the board of the team (a priority change, and a move).
//! 2. None for the actor of the change, and none for a kind that the person
//!    turned off (`/api/me/notification-settings`).
//! 3. A mention of an unknown name, or in code, makes nothing; an update
//!    tells only the new mentions.
//! 4. An event `notification_created` goes to the socket of the person
//!    only.
//! 5. The retention pass deletes the read notifications after the setting.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_notifications_t0381_server_test`.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateTaskRequest, Task, UpdateContentRequest};
use kairos_client::types_meta::{CreateRelationshipRequest, UpdateMetadataRequest};
use kairos_client::types_notifications::UpdateNotificationSettingsRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{EntityKind, Error, EventStream, KairosClient};
use kairos_core::retention::RetentionConfig;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_notifications_t0381_server_test";
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(QueryableByName, Debug)]
struct Row {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Text)]
    text: String,
}

/// Each notification of the tenant, oldest first.
fn all(conn: &mut PgConnection) -> Vec<Row> {
    diesel::sql_query(
        "SELECT user_id, kind, text FROM org_acme.notifications ORDER BY created_at, id",
    )
    .load(conn)
    .expect("notifications")
}

/// The notifications made after the first `seen`, as (person, kind, text).
fn since(conn: &mut PgConnection, seen: &mut usize) -> Vec<Row> {
    let rows = all(conn);
    let new: Vec<Row> = rows.into_iter().skip(*seen).collect();
    *seen += new.len();
    new
}

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn new_task(board: &str, title: &str, content: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        board_id: Some(board.to_string()),
        column_id: None,
        title: title.into(),
        content: content.into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
        from_team: None,
        parent: None,
    }
}

async fn to_active(client: &KairosClient, task: &Task, to: &impl Fn(&str) -> String) {
    client
        .transition_task(&task.short_code, &to("Todo"))
        .await
        .expect("to Todo");
    client
        .transition_task(&task.short_code, &to("Active"))
        .await
        .expect("to Active");
}

async fn blocks(client: &KairosClient, blocker: &Task, blocked: &Task) {
    client
        .create_relationship(&CreateRelationshipRequest {
            source_short_code: blocker.short_code.clone(),
            target_short_code: blocked.short_code.clone(),
            relationship: "blocks".into(),
        })
        .await
        .expect("blocks edge");
}

async fn priority(client: &KairosClient, task: &Task, value: &str) {
    client
        .update_metadata(
            EntityKind::Task,
            &task.short_code,
            &UpdateMetadataRequest {
                values: BTreeMap::from([("priority".to_string(), Some(value.to_string()))]),
            },
        )
        .await
        .expect("priority");
}

async fn next_frame(stream: &mut EventStream) -> Value {
    tokio::time::timeout(RECV_TIMEOUT, stream.next_event_raw())
        .await
        .expect("timed out waiting for an event")
        .expect("event frame")
}

#[tokio::test]
async fn the_five_events_reach_the_right_person() {
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
    // The user names of a mention.
    for name in ["alice", "bob", "carol"] {
        diesel::update(users::table.filter(users::email.eq(format!("{name}@kairos.test"))))
            .set(users::user_name.eq(name))
            .execute(&mut conn)
            .expect("user name");
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
    let ops = admin
        .create_team(&CreateTeamRequest {
            name: "Ops".into(),
            slug: "ops".into(),
            code_prefix: "OPS".into(),
            team_type: None,
        })
        .await
        .expect("team ops");
    for (team, members) in [(&web, vec![alice_id, bob_id]), (&ops, vec![carol_id])] {
        for user_id in members {
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
    }
    let board = web.delivery_board_id.clone().expect("web board");
    let ops_board = ops.delivery_board_id.clone().expect("ops board");
    let columns: HashMap<String, String> = admin
        .get_board(&board)
        .await
        .expect("board")
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let to = |name: &str| columns[name].clone();
    let mut seen = 0usize;

    // Sockets of alice and bob, to see where `notification_created` goes.
    let mut alice_events = alice.connect_events().await.expect("alice socket");
    let mut bob_events = bob.connect_events().await.expect("bob socket");

    // --- 1. A hand-off -------------------------------------------------------
    let claimed = alice
        .create_task(&new_task(&board, "Claimed", "one"))
        .await
        .expect("task");
    to_active(&alice, &claimed, &to).await;
    alice
        .hand_off_task(&claimed.short_code, "bob@kairos.test")
        .await
        .expect("alice hands off to bob");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!((new[0].user_id, new[0].kind.as_str()), (bob_id, "hand_off"));
    assert!(new[0].text.contains(&claimed.short_code), "{:?}", new[0]);
    admin
        .hand_off_task(&claimed.short_code, "alice@kairos.test")
        .await
        .expect("svc hands off to alice");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(
        (new[0].user_id, new[0].kind.as_str()),
        (alice_id, "hand_off")
    );

    // The event of the person: alice's socket gets it, bob's does not.
    let mut got = false;
    for _ in 0..20 {
        let frame = next_frame(&mut alice_events).await;
        if frame["event"] == "notification_created" {
            assert_eq!(frame["short_code"], json!(claimed.short_code));
            assert_eq!(frame["entity_type"], "notification");
            assert!(frame.get("recipient").is_none(), "{frame}");
            assert!(frame.get("tenant").is_none(), "{frame}");
            got = true;
            break;
        }
    }
    assert!(got, "alice's socket gets her notification");
    // A last event of the tenant: bob reads up to it, and no event of a
    // different person comes before it.
    let sentinel = admin
        .create_task(&new_task(&board, "Sentinel", ""))
        .await
        .expect("sentinel");
    loop {
        let frame = next_frame(&mut bob_events).await;
        if frame["event"] == "notification_created" {
            // Only bob's own: the hand-off from alice.
            assert_eq!(frame["short_code"], json!(claimed.short_code), "{frame}");
            assert_eq!(frame["actor"], json!(alice_id.to_string()), "{frame}");
        }
        if frame["event"] == "item_created" && frame["short_code"] == json!(sentinel.short_code) {
            break;
        }
    }

    // --- 2. A blocker that is done, or archived -----------------------------
    let blocker = alice
        .create_task(&new_task(&board, "Blocker", ""))
        .await
        .expect("blocker");
    let archived = alice
        .create_task(&new_task(&board, "Archived blocker", ""))
        .await
        .expect("blocker 2");
    let done_then_archived = alice
        .create_task(&new_task(&board, "Done blocker", ""))
        .await
        .expect("blocker 3");
    for b in [&blocker, &archived, &done_then_archived] {
        blocks(&admin, b, &claimed).await;
    }
    to_active(&bob, &blocker, &to).await;
    bob.transition_task(&blocker.short_code, &to("Completed"))
        .await
        .expect("blocker done");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(
        (new[0].user_id, new[0].kind.as_str()),
        (alice_id, "blocker_cleared")
    );
    assert!(
        new[0].text.contains(&blocker.short_code) && new[0].text.contains("done"),
        "{:?}",
        new[0]
    );
    admin
        .delete_task(&archived.short_code)
        .await
        .expect("archive");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(
        (new[0].user_id, new[0].kind.as_str()),
        (alice_id, "blocker_cleared")
    );
    assert!(new[0].text.contains("archived"), "{:?}", new[0]);
    to_active(&bob, &done_then_archived, &to).await;
    bob.transition_task(&done_then_archived.short_code, &to("Completed"))
        .await
        .expect("done");
    assert_eq!(since(&mut conn, &mut seen).len(), 1);
    admin
        .delete_task(&done_then_archived.short_code)
        .await
        .expect("archive a done blocker");
    assert!(
        since(&mut conn, &mut seen).is_empty(),
        "a done blocker blocked nothing"
    );

    // --- 3. A cancel ----------------------------------------------------------
    let cancelled = alice
        .create_task(&new_task(&board, "To cancel", ""))
        .await
        .expect("task");
    bob.cancel_task(&cancelled.short_code, "A copy of a different task")
        .await
        .expect("bob cancels");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(
        (new[0].user_id, new[0].kind.as_str()),
        (alice_id, "cancelled")
    );
    assert!(
        new[0].text.contains("A copy of a different task"),
        "{:?}",
        new[0]
    );
    let own = alice
        .create_task(&new_task(&board, "Own", ""))
        .await
        .expect("task");
    alice
        .cancel_task(&own.short_code, "Not needed")
        .await
        .expect("alice cancels her task");
    assert!(since(&mut conn, &mut seen).is_empty(), "not for the actor");

    // --- 4. Mentions ------------------------------------------------------------
    bob.append_note(
        EntityKind::Task,
        &claimed.short_code,
        "Ask @alice and @Carol. Not @nobody, not robin@example.com, not `@bob`.",
    )
    .await
    .expect("note");
    let mut new: Vec<(Uuid, String)> = since(&mut conn, &mut seen)
        .into_iter()
        .map(|r| (r.user_id, r.kind))
        .collect();
    new.sort();
    let mut want = vec![
        (alice_id, "mention".to_string()),
        (carol_id, "mention".to_string()),
    ];
    want.sort();
    assert_eq!(new, want);
    // An update with the same mentions tells nobody.
    let current = bob.get_task(&claimed.short_code).await.expect("task");
    bob.update_task(
        &claimed.short_code,
        &UpdateContentRequest {
            title: None,
            content: format!("{}\n\nMore text.", current.content),
            version: current.version,
        },
    )
    .await
    .expect("update");
    assert!(since(&mut conn, &mut seen).is_empty(), "no new mention");
    // A mention in code, and of yourself.
    bob.append_note(
        EntityKind::Task,
        &claimed.short_code,
        "```\n@alice\n```\nand @bob",
    )
    .await
    .expect("note");
    assert!(since(&mut conn, &mut seen).is_empty(), "code and the actor");
    // A new item with a mention.
    bob.create_task(&new_task(&board, "Hello", "For @carol"))
        .await
        .expect("task with a mention");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(
        (new[0].user_id, new[0].kind.as_str()),
        (carol_id, "mention")
    );

    // --- 5. A P0 bug on the board of the team -------------------------------
    let bug = bob
        .create_task(&CreateTaskRequest {
            task_type: Some("bug".into()),
            ..new_task(&board, "Crash", "")
        })
        .await
        .expect("bug");
    priority(&bob, &bug, "high").await;
    assert!(since(&mut conn, &mut seen).is_empty(), "high is not P0");
    priority(&bob, &bug, "critical").await;
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!((new[0].user_id, new[0].kind.as_str()), (alice_id, "p0_bug"));
    assert!(new[0].text.contains(&bug.short_code), "{:?}", new[0]);
    priority(&bob, &bug, "critical").await;
    assert!(since(&mut conn, &mut seen).is_empty(), "P0 already");
    admin
        .move_task(&bug.short_code, &ops_board)
        .await
        .expect("move to ops");
    let new = since(&mut conn, &mut seen);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!((new[0].user_id, new[0].kind.as_str()), (carol_id, "p0_bug"));
    // A P0 task that is not a bug tells nobody.
    let task = bob
        .create_task(&new_task(&board, "Not a bug", ""))
        .await
        .expect("task");
    priority(&bob, &task, "critical").await;
    assert!(since(&mut conn, &mut seen).is_empty(), "not a bug");

    // --- Settings ------------------------------------------------------------
    let settings = alice.notification_settings().await.expect("settings");
    assert!(settings.mention && settings.hand_off && settings.p0_bug);
    let settings = alice
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            mention: Some(false),
            ..Default::default()
        })
        .await
        .expect("turn mentions off");
    assert!(!settings.mention && settings.hand_off);
    assert!(
        !alice
            .notification_settings()
            .await
            .expect("settings")
            .mention
    );
    bob.append_note(EntityKind::Task, &claimed.short_code, "@alice again")
        .await
        .expect("note");
    assert!(
        since(&mut conn, &mut seen).is_empty(),
        "alice turned mentions off"
    );
    alice
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            mention: Some(true),
            ..Default::default()
        })
        .await
        .expect("turn mentions on");
    let err = alice
        .update_notification_settings(&UpdateNotificationSettingsRequest::default())
        .await
        .expect_err("no kind");
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    let response = reqwest::Client::new()
        .put(format!("{}/api/me/notification-settings", server.base_url))
        .bearer_auth(&alice_token)
        .header("x-tenant", "acme")
        .json(&json!({"email": false}))
        .send()
        .await
        .expect("PUT");
    assert!(
        response.status().is_client_error(),
        "an unknown kind is refused: {}",
        response.status()
    );
    // A service account has no notifications.
    let account = admin
        .create_service_account(
            &kairos_client::types_service_accounts::CreateServiceAccountRequest {
                name: "bot".into(),
            },
        )
        .await
        .expect("service account");
    let key = admin
        .create_api_key(
            &account.id,
            &kairos_client::types_service_accounts::CreateApiKeyRequest {
                name: "k".into(),
                expires_at: None,
                no_expiry: false,
                scope: None,
            },
        )
        .await
        .expect("key");
    let bot = server.client(&key.key, "acme");
    let err = bot
        .notification_settings()
        .await
        .expect_err("a service account");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // --- Retention -----------------------------------------------------------
    let total = all(&mut conn).len();
    diesel::sql_query(
        "UPDATE org_acme.notifications SET read_at = now() - interval '40 days' \
          WHERE id = (SELECT id FROM org_acme.notifications ORDER BY created_at LIMIT 1)",
    )
    .execute(&mut conn)
    .expect("one read 40 days ago");
    diesel::sql_query(
        "UPDATE org_acme.notifications SET read_at = now() - interval '1 day' \
          WHERE id = (SELECT id FROM org_acme.notifications ORDER BY created_at DESC LIMIT 1)",
    )
    .execute(&mut conn)
    .expect("one read yesterday");
    let keep = RetentionConfig {
        notifications_read_days: 0,
        code_index_pool_days: 30,
        ..RetentionConfig::default()
    };
    assert_eq!(
        read_notifications_deleted(&mut conn, &keep),
        0,
        "0 keeps read notifications"
    );
    let deleted = read_notifications_deleted(&mut conn, &RetentionConfig::default());
    assert_eq!(deleted, 1);
    assert_eq!(all(&mut conn).len(), total - 1);
}

/// The read notifications that one retention pass deletes now. The pass
/// sweeps no history and keeps each code index.
fn read_notifications_deleted(conn: &mut PgConnection, config: &RetentionConfig) -> u64 {
    let config = RetentionConfig {
        mode: kairos_core::retention::RetentionMode::Off,
        code_index_keep: 0,
        ..config.clone()
    };
    kairos_db::retention::run_retention_pass(conn, &config, Utc::now(), &mut || {})
        .expect("pass")
        .read_notifications_deleted
}
