//! Integration test for COLLIERY-T-0233: the graph says which nodes are in
//! a done column, and a board hears when a blocker on a different board is
//! completed.
//!
//! Two defects, one rule (COLLIERY-T-0214: a `blocks` edge counts only
//! while no end of the edge is in a done column):
//!
//! - `GET /api/{family}/{code}/graph` gave a node no flag for a done
//!   column, so the graph view drew each `blocks` arrow as an open blocker;
//! - a `/ws/events` socket filtered to one board did not get the event of a
//!   blocker on a different board, so the blocked-by count on the card
//!   stayed stale until a reload.
//!
//! The test reads the two surfaces over the wire: the REST graph, and a
//! real WebSocket filtered to the board of the blocked card. After each
//! delivered event it reads `GET /api/boards/{id}/items` again, as the
//! board view does, and checks the count.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_blocks_across_boards_t0233_test`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types_events::ThinEvent;
use kairos_client::types_graph::GraphResponse;
use kairos_client::{EntityKind, EventStream};
use kairos_core::short_code::ItemType;
use kairos_db::items::{self, ContentUpdate, CreateTask};
use kairos_db::models::{
    BoardLevel, NewOrganizationMember, OrgRole, RelationshipType, TaskType, WorkClass,
};
use kairos_db::schema::{board_columns, organization_members, organizations, users};
use kairos_db::{TenantPool, boards, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_blocks_across_boards_t0233_test";

/// Bound on every socket receive; generous because CI shares the compose
/// Postgres with concurrent work.
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

fn column_id(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    board_columns::table
        .filter(board_columns::board_id.eq(board))
        .filter(board_columns::name.eq(name))
        .filter(board_columns::deleted_at.is_null())
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name:?} not found: {e}"))
}

/// Next event from the stream, bounded by [`RECV_TIMEOUT`], with the raw
/// frame for the wire-shape assertion.
async fn recv_event(stream: &mut EventStream) -> (ThinEvent, serde_json::Value) {
    let raw = tokio::time::timeout(RECV_TIMEOUT, stream.next_event_raw())
        .await
        .expect("timed out waiting for an event")
        .expect("event frame");
    let event: ThinEvent =
        serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("thin event {raw}: {e}"));
    (event, raw)
}

/// `done` of the node `code` in a graph response.
fn node_done(graph: &GraphResponse, code: &str) -> bool {
    graph
        .nodes
        .iter()
        .find(|node| node.short_code == code)
        .unwrap_or_else(|| panic!("no node {code} in {graph:?}"))
        .done
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_board_hears_a_blocker_on_a_different_board() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("org row");

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
    let _ = svc.whoami().await;
    let svc_id: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: acme,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting membership");

    // --- two delivery boards -------------------------------------------------
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let board = |conn: &mut PgConnection, name: &str, slug: &str| {
        // COLLIERY-T-0230: a delivery board always has a team.
        let team = common::seed_team(conn, &format!("{name} Team"), &format!("{slug}-team"));
        boards::create_board(conn, BoardLevel::Delivery, name, slug, Some(team), None)
            .expect("creating a delivery board")
            .id
    };
    // `watched` holds the cards under test. `elsewhere` holds the work at
    // the other end of their `blocks` edges.
    let watched = board(&mut conn, "Watched", "watched");
    let elsewhere = board(&mut conn, "Elsewhere", "elsewhere");
    let task = |conn: &mut PgConnection, title: &str, board: Uuid, column: &str| {
        let column = column_id(conn, board, column);
        items::create_task(
            conn,
            CreateTask {
                board_id: board,
                column_id: Some(column),
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                repository_id: None,
            },
            svc_id,
        )
        .expect("task")
    };
    let link = |conn: &mut PgConnection, from: Uuid, to: Uuid| {
        graph::link_items(conn, from, to, RelationshipType::Blocks, svc_id).expect("linking");
    };

    // On `watched`: a card with two blockers on `elsewhere`, and a card
    // that blocks work on `elsewhere`. On `elsewhere`, one task has no
    // edge to `watched`.
    let waiting = task(&mut conn, "Waiting", watched, "Todo");
    let upstream = task(&mut conn, "Upstream", watched, "Active");
    let blocker = task(&mut conn, "Blocker", elsewhere, "Active");
    let second_blocker = task(&mut conn, "Second blocker", elsewhere, "Active");
    let downstream = task(&mut conn, "Downstream", elsewhere, "Active");
    let unrelated = task(&mut conn, "Unrelated", elsewhere, "Active");
    link(&mut conn, blocker.id, waiting.id);
    link(&mut conn, second_blocker.id, waiting.id);
    link(&mut conn, upstream.id, downstream.id);
    let completed_elsewhere = column_id(&mut conn, elsewhere, "Completed");

    let counts = |code: String| {
        let svc = svc.clone();
        async move {
            svc.board_items(&watched.to_string(), &Default::default())
                .await
                .expect("board items")
                .blocks_summary
                .get(&code)
                .map_or((0, 0), |c| (c.blocked_by, c.blocks))
        }
    };

    // =======================================================================
    // The graph: a node says if its item is in a done column
    // =======================================================================
    let before = svc
        .get_item_graph(EntityKind::Task, &waiting.short_code, Some(1))
        .await
        .expect("graph of the waiting task");
    for code in [&waiting.short_code, &blocker.short_code] {
        assert!(
            !node_done(&before, code),
            "{code} is in an open column: {before:?}"
        );
    }
    let raw: serde_json::Value = serde_json::to_value(&before).expect("graph as JSON");
    assert!(
        raw["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .all(|node| node["done"] == false),
        "each node carries `done`, also when it is false: {raw}"
    );

    // =======================================================================
    // The events: a socket filtered to `watched`
    // =======================================================================
    let mut stream = svc.connect_events().await.expect("event socket");
    stream
        .subscribe_board(&watched.to_string())
        .await
        .expect("sending subscribe");
    // Give the server a beat to apply the filter before mutating.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(counts(waiting.short_code.clone()).await, (2, 0));
    assert_eq!(counts(upstream.short_code.clone()).await, (0, 1));

    // Ordering probe: the unrelated task moves FIRST, so if its event were
    // (wrongly) delivered it would arrive first. The content edit of the
    // blocker cannot change a count, so it is not delivered either. The
    // next frame must be the transition of the blocker.
    kairos_db::transition_task(&mut conn, unrelated.id, completed_elsewhere, svc_id)
        .expect("completing the unrelated task");
    items::update_item_content(
        &mut conn,
        ItemType::Task,
        blocker.id,
        ContentUpdate {
            new_title: None,
            new_content: "edited",
            expected_version: 1,
        },
        svc_id,
    )
    .expect("editing the blocker");
    kairos_db::transition_task(&mut conn, blocker.id, completed_elsewhere, svc_id)
        .expect("completing the blocker");
    let (event, raw) = recv_event(&mut stream).await;
    assert_eq!(
        (event.event.as_str(), event.short_code.as_str()),
        ("item_transitioned", blocker.short_code.as_str()),
        "the completed blocker on the other board is the first event that \
         reaches the filtered socket: {raw}"
    );
    assert_eq!(
        event.board_id.as_deref(),
        Some(elsewhere.to_string().as_str()),
        "the event names the board of the blocker, not the subscribed board"
    );
    assert!(
        raw.get("blocks_boards").is_none() && raw.get("tenant").is_none(),
        "routing metadata must never reach a client: {raw}"
    );
    assert_eq!(
        counts(waiting.short_code.clone()).await,
        (1, 0),
        "the re-fetch that the event triggers shows one open blocker"
    );

    // The graph agrees about the same edge.
    let after = svc
        .get_item_graph(EntityKind::Task, &waiting.short_code, Some(1))
        .await
        .expect("graph of the waiting task");
    assert!(node_done(&after, &blocker.short_code), "{after:?}");
    assert!(!node_done(&after, &second_blocker.short_code), "{after:?}");
    assert!(!node_done(&after, &waiting.short_code), "{after:?}");
    assert!(
        after.edges.iter().any(|edge| {
            edge.relationship == "blocks"
                && edge.source_id == blocker.id.to_string()
                && edge.target_id == waiting.id.to_string()
        }),
        "the edge stays in the graph as history: {after:?}"
    );

    // The other direction: the blocked item on the other board is
    // completed, and the `blocks` count of the card goes.
    kairos_db::transition_task(&mut conn, downstream.id, completed_elsewhere, svc_id)
        .expect("completing the blocked item");
    let (event, raw) = recv_event(&mut stream).await;
    assert_eq!(
        (event.event.as_str(), event.short_code.as_str()),
        ("item_transitioned", downstream.short_code.as_str()),
        "{raw}"
    );
    assert_eq!(counts(upstream.short_code.clone()).await, (0, 0));

    // Archived work does not count either, and the board hears that too.
    items::soft_delete_item(&mut conn, ItemType::Task, second_blocker.id, svc_id)
        .expect("archiving the second blocker");
    let (event, raw) = recv_event(&mut stream).await;
    assert_eq!(
        (event.event.as_str(), event.short_code.as_str()),
        ("item_deleted", second_blocker.short_code.as_str()),
        "{raw}"
    );
    assert_eq!(counts(waiting.short_code.clone()).await, (0, 0));

    // The filter still holds for the rest: an event of `watched` itself is
    // the next frame, with nothing from `elsewhere` before it.
    task(&mut conn, "No edge", elsewhere, "Todo");
    let local = task(&mut conn, "Local", watched, "Todo");
    let (event, raw) = recv_event(&mut stream).await;
    assert_eq!(
        (event.event.as_str(), event.short_code.as_str()),
        ("item_created", local.short_code.as_str()),
        "a create on the other board is filtered out: {raw}"
    );

    stream.close().await.expect("closing the stream");
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
