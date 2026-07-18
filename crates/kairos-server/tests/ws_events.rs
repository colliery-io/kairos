//! Integration test for the KAIROS-T-0022 `/ws/events` channel (contract
//! per KAIROS-A-0005 §5 / KAIROS-S-0005 "Event Push"): NOTIFY emission
//! from the mutating services → the server's LISTEN task → real WebSocket
//! clients, against the LIVE compose stack (real Postgres, real Dex
//! tokens).
//!
//! The subscribed event stream runs through the typed
//! `kairos_client::EventStream` helper (KAIROS-T-0024): connect with the
//! client's token/tenant, `subscribe_board`/`subscribe_all`, typed
//! `ThinEvent`s (raw frames kept side-by-side for the wire-shape
//! assertions, e.g. "the tenant tag never reaches a client"). Raw
//! tokio-tungstenite stays ONLY for the protocol-level upgrade probes
//! (missing/garbage tokens, non-member, unknown tenant, the
//! `?access_token=` browser fallback) that the helper deliberately cannot
//! express.
//!
//! For isolation the test owns the uniquely named scratch database
//! `kairos_ws_events_t0022_test` (shared-services discipline); the server
//! is bound to an ephemeral local port because WebSocket upgrades need a
//! real connection.
//!
//! Covered: upgrade auth semantics (401 missing/garbage token, 403
//! non-member, 404 unknown tenant), the `?access_token=` browser fallback,
//! event shape for create/update/transition/delete/relationship/metadata,
//! two-tenant isolation (acme socket never sees widgets events), the
//! `board_id` subscribe filter (set and cleared), and clean
//! disconnect/reconnect — every await timeout-bounded.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types_events::ThinEvent;
use kairos_client::types_meta::CreateMetadataDefinitionRequest;
use kairos_client::{EntityKind, Error, EventStream};
use kairos_core::short_code::ItemType;
use kairos_db::items::{ContentUpdate, CreateStrategy, CreateTask};
use kairos_db::models::{
    BoardLevel, NewOrganizationMember, NewUser, OrgRole, RelationshipType, TaskType, User,
};
use kairos_db::schema::{board_transitions, boards, organizations, users};
use kairos_db::{TenantPool, create_board, graph, items, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_ws_events_t0022_test";

/// Bound on every socket receive; generous because CI shares the compose
/// Postgres with concurrent work.
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

type WsClient = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A fresh sync connection pinned to `schema` (same mechanism as the
/// pools) — the direct write path the services run on.
fn tenant_connection(scratch_url: &str, schema: &str) -> PgConnection {
    let mut conn = PgConnection::establish(scratch_url).expect("connecting to scratch database");
    sql_query(format!("SET search_path TO {schema}, public"))
        .execute(&mut conn)
        .expect("pinning search_path");
    conn
}

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The tenant board of a level in the CURRENT search_path schema.
fn board_of_level(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

/// A column reachable from `from` per the board's transition graph.
fn transition_target(conn: &mut PgConnection, board: Uuid, from: Uuid) -> Uuid {
    board_transitions::table
        .filter(board_transitions::board_id.eq(board))
        .filter(board_transitions::from_column_id.eq(from))
        .select(board_transitions::to_column_id)
        .first(conn)
        .expect("default boards allow a move out of the first column")
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// Open a RAW WebSocket to the test server (protocol-level upgrade probes
/// only — the happy path goes through `KairosClient::connect_events`).
/// `auth`: `Some(token)` sets the Authorization header; the caller
/// controls everything else through `path` (query string) and `tenant`
/// (X-Tenant header).
async fn ws_connect(
    addr: std::net::SocketAddr,
    path: &str,
    auth: Option<&str>,
    tenant: Option<&str>,
) -> Result<WsClient, tokio_tungstenite::tungstenite::Error> {
    let mut req = format!("ws://{addr}{path}")
        .into_client_request()
        .expect("client request");
    if let Some(token) = auth {
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).expect("header value"),
        );
    }
    if let Some(tenant) = tenant {
        req.headers_mut()
            .insert("x-tenant", HeaderValue::from_str(tenant).expect("header"));
    }
    connect_async(req).await.map(|(socket, _)| socket)
}

/// The HTTP status a rejected raw upgrade came back with.
fn upgrade_status(err: tokio_tungstenite::tungstenite::Error) -> StatusCode {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            StatusCode::from_u16(response.status().as_u16()).expect("status")
        }
        other => panic!("expected an HTTP rejection, got: {other:?}"),
    }
}

/// Next event from the typed helper stream, bounded by [`RECV_TIMEOUT`]:
/// the typed `ThinEvent` plus the raw frame (for wire-shape assertions).
async fn recv_event(stream: &mut EventStream) -> (ThinEvent, Value) {
    let raw = tokio::time::timeout(RECV_TIMEOUT, stream.next_event_raw())
        .await
        .expect("timed out waiting for an event")
        .expect("event frame");
    let event: ThinEvent =
        serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("thin event {raw}: {e}"));
    (event, raw)
}

/// Next Text frame from a RAW socket as JSON (the browser-fallback probe
/// socket), bounded by [`RECV_TIMEOUT`].
async fn recv_raw_ws(socket: &mut WsClient) -> Value {
    tokio::time::timeout(RECV_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str::<Value>(&text)
                        .unwrap_or_else(|e| panic!("non-JSON event {text:?}: {e}"));
                }
                Some(Ok(_)) => continue, // ping/pong
                other => panic!("socket ended while awaiting an event: {other:?}"),
            }
        }
    })
    .await
    .expect("timed out waiting for an event")
}

/// Assert the S-0005 thin-event shape (typed fields + the raw-frame
/// invariants the DTO cannot see).
fn assert_event_shape(event: &ThinEvent, raw: &Value, kind: &str, entity_type: &str) {
    assert_eq!(event.event, kind, "event kind in {raw}");
    assert_eq!(event.entity_type, entity_type, "entity_type in {raw}");
    assert!(!event.short_code.is_empty(), "short_code in {raw}");
    assert!(!event.actor.is_empty(), "actor in {raw}");
    assert!(!event.occurred_at.is_empty(), "occurred_at in {raw}");
    assert!(
        raw.get("tenant").is_none(),
        "the tenant tag must never reach a client: {raw}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_events_against_live_stack() {
    // --- scratch database + two tenants ------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    provision_tenant(&mut conn, "widgets", "Widgets Co").expect("provisioning widgets");
    let acme_org: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // --- real server + live tokens + typed clients ---------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
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
    let addr = server.addr;
    let alice = server.client(&alice_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // --- JIT-provision users, grant memberships ------------------------------
    for client in [&alice, &svc] {
        let err = rejection(client.whoami().await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user, role) in [(alice_id, OrgRole::Member), (svc_id, OrgRole::Admin)] {
        diesel::insert_into(kairos_db::schema::organization_members::table)
            .values(NewOrganizationMember {
                organization_id: acme_org,
                user_id: user,
                role,
            })
            .execute(&mut conn)
            .expect("granting acme membership");
    }
    // The widgets-side actor never authenticates; a directory row is enough.
    let widget_actor: Uuid = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "ws-widgets-actor".into(),
            email: "widgets-actor@kairos.test".into(),
            display_name: "Widgets Actor".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("inserting widgets actor")
        .id;

    // --- tenant boards -------------------------------------------------------
    let mut acme = tenant_connection(&scratch_url, "org_acme");
    create_board(
        &mut acme,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("acme delivery board");
    let acme_delivery = board_of_level(&mut acme, BoardLevel::Delivery);
    let acme_strategy_board = board_of_level(&mut acme, BoardLevel::Strategy);

    let mut widgets = tenant_connection(&scratch_url, "org_widgets");
    create_board(
        &mut widgets,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("widgets delivery board");
    let widgets_delivery = board_of_level(&mut widgets, BoardLevel::Delivery);

    // =========================================================================
    // Upgrade auth: the standard middleware semantics apply pre-upgrade
    // (raw handshake probes — protocol-level by design)
    // =========================================================================
    let err = ws_connect(addr, "/ws/events", None, Some("acme"))
        .await
        .expect_err("unauthenticated upgrade must be rejected");
    assert_eq!(upgrade_status(err), StatusCode::UNAUTHORIZED);

    let err = ws_connect(addr, "/ws/events", Some("not.a.jwt"), Some("acme"))
        .await
        .expect_err("garbage token must be rejected");
    assert_eq!(upgrade_status(err), StatusCode::UNAUTHORIZED);

    let err = ws_connect(addr, "/ws/events", Some(&alice_token), Some("widgets"))
        .await
        .expect_err("non-member upgrade must be rejected");
    assert_eq!(upgrade_status(err), StatusCode::FORBIDDEN);

    let err = ws_connect(addr, "/ws/events", Some(&alice_token), Some("ghost"))
        .await
        .expect_err("unknown tenant must be rejected");
    assert_eq!(upgrade_status(err), StatusCode::NOT_FOUND);

    // The client helper maps the same rejections onto the typed errors.
    let err = rejection(
        server
            .client(&alice_token, "widgets")
            .connect_events()
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // =========================================================================
    // Event shape: create → transition → update → delete
    // =========================================================================
    let mut stream = alice
        .connect_events()
        .await
        .expect("member upgrade through the client helper succeeds");

    let task = items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "Ship the event channel",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating acme task");

    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "item_created", "task");
    assert_eq!(event.short_code, task.short_code.as_str());
    assert_eq!(
        event.board_id.as_deref(),
        Some(acme_delivery.to_string().as_str())
    );
    assert_eq!(
        event.column_id.as_deref(),
        Some(task.column_id.to_string().as_str())
    );
    assert_eq!(event.actor, alice_id.to_string());

    let target = transition_target(&mut acme, acme_delivery, task.column_id);
    kairos_db::transition_task(&mut acme, task.id, target, alice_id).expect("transitioning task");
    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "item_transitioned", "task");
    assert_eq!(event.short_code, task.short_code.as_str());
    assert_eq!(
        event.column_id.as_deref(),
        Some(target.to_string().as_str()),
        "the NEW column"
    );

    items::update_item_content(
        &mut acme,
        ItemType::Task,
        task.id,
        ContentUpdate {
            new_title: None,
            new_content: "updated",
            expected_version: 1,
        },
        alice_id,
    )
    .expect("updating task content");
    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "item_updated", "task");
    assert_eq!(event.short_code, task.short_code.as_str());

    // =========================================================================
    // relationship_changed: one event per endpoint
    // =========================================================================
    let task2 = items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "Blocked work",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating second acme task");
    let (event, _) = recv_event(&mut stream).await;
    assert_eq!(event.event, "item_created");

    graph::link_items(
        &mut acme,
        task.id,
        task2.id,
        RelationshipType::Blocks,
        alice_id,
    )
    .expect("linking tasks");
    let (first, first_raw) = recv_event(&mut stream).await;
    let (second, second_raw) = recv_event(&mut stream).await;
    for (event, raw) in [(&first, &first_raw), (&second, &second_raw)] {
        assert_event_shape(event, raw, "relationship_changed", "task");
    }
    let codes: Vec<&str> = [&first, &second]
        .iter()
        .map(|e| e.short_code.as_str())
        .collect();
    assert!(codes.contains(&task.short_code.as_str()), "{codes:?}");
    assert!(codes.contains(&task2.short_code.as_str()), "{codes:?}");

    // =========================================================================
    // metadata_changed: through the typed REST client (svc is org admin)
    // =========================================================================
    svc.create_metadata_definition(&CreateMetadataDefinitionRequest {
        name: "WS Priority".into(),
        slug: "ws_priority".into(),
        field_type: "string".into(),
        enum_options: vec![],
    })
    .await
    .expect("creating ws_priority definition");
    svc.update_metadata(
        EntityKind::Task,
        &task.short_code,
        &kairos_client::types_meta::UpdateMetadataRequest {
            values: std::collections::BTreeMap::from([(
                "ws_priority".to_string(),
                Some("high".to_string()),
            )]),
        },
    )
    .await
    .expect("setting ws_priority");
    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "metadata_changed", "task");
    assert_eq!(event.short_code, task.short_code.as_str());
    assert_eq!(event.actor, svc_id.to_string());

    // =========================================================================
    // Two-tenant isolation: widgets mutations never reach the acme socket
    // =========================================================================
    items::create_task(
        &mut widgets,
        CreateTask {
            board_id: widgets_delivery,
            column_id: None,
            title: "Widgets-only work",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        widget_actor,
    )
    .expect("creating widgets task");
    // Deterministic ordering probe: the widgets event was emitted BEFORE
    // this acme delete, so if it were (wrongly) delivered it would arrive
    // first. The next frame must be the acme delete.
    items::soft_delete_item(&mut acme, ItemType::Task, task2.id, alice_id)
        .expect("deleting second acme task");
    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "item_deleted", "task");
    assert_eq!(
        event.short_code,
        task2.short_code.as_str(),
        "the widgets event must never be delivered to an acme socket"
    );

    // =========================================================================
    // board_id subscribe filter: set, honored, cleared
    // =========================================================================
    stream
        .subscribe_board(&acme_strategy_board.to_string())
        .await
        .expect("sending subscribe");
    // Give the server a beat to apply the filter before mutating.
    tokio::time::sleep(Duration::from_millis(300)).await;

    items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "Filtered out",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating filtered-out task");
    let strategy = items::create_strategy(
        &mut acme,
        CreateStrategy {
            board_id: acme_strategy_board,
            column_id: None,
            title: "Strategy behind the filter",
            content: "…",
            hypothesis: None,
        },
        alice_id,
    )
    .expect("creating strategy");
    let (event, raw) = recv_event(&mut stream).await;
    assert_event_shape(&event, &raw, "item_created", "strategy");
    assert_eq!(
        event.short_code,
        strategy.short_code.as_str(),
        "the delivery-board event must be filtered out"
    );
    assert_eq!(
        event.board_id.as_deref(),
        Some(acme_strategy_board.to_string().as_str())
    );

    stream.subscribe_all().await.expect("clearing the filter");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let task3 = items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "Filter cleared",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating post-clear task");
    let (event, _) = recv_event(&mut stream).await;
    assert_eq!(event.event, "item_created");
    assert_eq!(event.short_code, task3.short_code.as_str());

    // =========================================================================
    // Browser fallback: ?access_token= with no Authorization header (raw
    // socket — the client helper always authenticates via the header)
    // =========================================================================
    let mut browser_socket = ws_connect(
        addr,
        &format!("/ws/events?access_token={alice_token}"),
        None,
        Some("acme"),
    )
    .await
    .expect("query-param token upgrade succeeds");

    let task4 = items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "Fan-out to both sockets",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating fan-out task");
    let (event, _) = recv_event(&mut stream).await;
    assert_eq!(event.event, "item_created");
    assert_eq!(event.short_code, task4.short_code.as_str());
    let raw = recv_raw_ws(&mut browser_socket).await;
    assert_eq!(raw["event"], "item_created");
    assert_eq!(raw["short_code"], task4.short_code.as_str());

    // =========================================================================
    // Clean disconnect / reconnect (timeout-bounded)
    // =========================================================================
    stream.close().await.expect("closing the helper stream");
    futures_util::SinkExt::close(&mut browser_socket)
        .await
        .expect("closing browser socket");
    drop(browser_socket);

    let mut stream = tokio::time::timeout(RECV_TIMEOUT, alice.connect_events())
        .await
        .expect("reconnect within the timeout")
        .expect("reconnect upgrade succeeds");
    let task5 = items::create_task(
        &mut acme,
        CreateTask {
            board_id: acme_delivery,
            column_id: None,
            title: "After reconnect",
            content: "…",
            task_type: TaskType::Task,
            team_id: None,
        },
        alice_id,
    )
    .expect("creating post-reconnect task");
    let (event, _) = recv_event(&mut stream).await;
    assert_eq!(event.event, "item_created");
    assert_eq!(
        event.short_code,
        task5.short_code.as_str(),
        "no replay: only events emitted after the reconnect arrive"
    );
    stream.close().await.expect("final close");

    // --- teardown ------------------------------------------------------------
    drop(conn);
    drop(acme);
    drop(widgets);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
