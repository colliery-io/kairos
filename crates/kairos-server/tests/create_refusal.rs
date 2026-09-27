//! Integration test for COLLIERY-T-0227: a refused create leaves NOTHING in
//! the tenant.
//!
//! The defect: `create_item` wrote the item, then wrote the edge that
//! `parent` asks for, in two transactions. When the edge was refused the
//! caller got `VALIDATION` and no short code, but the item stayed, in the
//! entry column, with its content. The caller could not know that, and
//! created the item a second time.
//!
//! "Nothing" is checked where it is stored, not where it is shown: the five
//! item tables (archived rows included), `item_history`, `activity_log`,
//! `item_relationships`, and the `kairos_events` channel. `search` is
//! checked as well, because that is where the caller looks.
//!
//! Two kinds of refusal are covered, because they prove different things:
//!
//! 1. A relationship that the type rules do not allow. The types are known
//!    before the write, so the refusal comes before any work is done.
//! 2. A failure that nothing can know before the write. A trigger on
//!    `item_relationships` makes the edge insert fail. That proves the
//!    transaction: the item was written, and the rollback removed it.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_create_refusal_t0227_test`.
//!
//! Cast:
//! - `svc`   — org admin: passes each capability gate, so that a refusal
//!   here is a refusal of the RELATIONSHIP and not of the caller,
//! - `alice` — org member: the capability path of an ADR with a parent.

mod common;

use std::sync::Arc;
use std::time::Duration;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text as SqlText};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::CreateDocumentRequest;
use kairos_client::types_search::SearchRequest;
use kairos_client::{Error, KairosClient};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, abac, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_create_refusal_t0227_test";

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

/// The tenant board of a level (`search_path` must be pinned).
fn board_id_of(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

/// Minimal MCP session over HTTP (initialize → initialized → tools/call);
/// the same shape as the one in tests/file_backlog.rs.
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
                    "clientInfo": {"name": "colliery-t0227-test", "version": "0.0.0"},
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

    /// Create an item that must succeed; returns its short code.
    async fn create(&mut self, arguments: Value, prefix: &str) -> String {
        let (is_error, text) = self.call("create_item", arguments).await;
        assert!(!is_error, "create_item unexpectedly errored: {text}");
        let start = text
            .find(prefix)
            .unwrap_or_else(|| panic!("no {prefix} short code in {text:?}"));
        text[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect()
    }
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn count_by(conn: &mut PgConnection, sql: &str, value: &str) -> i64 {
    sql_query(sql)
        .bind::<SqlText, _>(value)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

/// Every row that a create writes, counted straight from the tables. No
/// `deleted_at` filter anywhere: an archived orphan is still an orphan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Footprint {
    items: i64,
    history: i64,
    activity: i64,
    edges: i64,
}

fn footprint(conn: &mut PgConnection) -> Footprint {
    Footprint {
        items: count(
            conn,
            "SELECT (SELECT COUNT(*) FROM org_acme.strategies) \
                  + (SELECT COUNT(*) FROM org_acme.initiatives) \
                  + (SELECT COUNT(*) FROM org_acme.tasks) \
                  + (SELECT COUNT(*) FROM org_acme.documents) \
                  + (SELECT COUNT(*) FROM org_acme.adrs) AS n",
        ),
        history: count(conn, "SELECT COUNT(*) AS n FROM org_acme.item_history"),
        activity: count(conn, "SELECT COUNT(*) AS n FROM org_acme.activity_log"),
        edges: count(
            conn,
            "SELECT COUNT(*) AS n FROM org_acme.item_relationships",
        ),
    }
}

/// Item rows with this title, in any of the five tables, archived or not.
fn rows_titled(conn: &mut PgConnection, title: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM ( \
             SELECT title FROM org_acme.strategies \
             UNION ALL SELECT title FROM org_acme.initiatives \
             UNION ALL SELECT title FROM org_acme.tasks \
             UNION ALL SELECT title FROM org_acme.documents \
             UNION ALL SELECT title FROM org_acme.adrs) t \
         WHERE t.title = $1",
        title,
    )
}

/// `supports` edges that point at the ADR with this title.
fn supports_edges_to_adr(conn: &mut PgConnection, title: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM org_acme.item_relationships r \
         JOIN org_acme.adrs a ON a.id = r.target_id \
         WHERE r.relationship = 'supports' AND a.title = $1",
        title,
    )
}

/// The events that arrived since the last call. PostgreSQL delivers a
/// `NOTIFY` when its transaction commits, so the wait gives a committed
/// event the time to arrive; an event that was rolled back never does.
async fn drain_events(listener: &mut PgConnection) -> Vec<String> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    listener
        .notifications_iter()
        .map(|n| n.expect("notification").payload)
        .collect()
}

/// The whole of "nothing was written" for one refused create.
async fn assert_nothing_written(
    conn: &mut PgConnection,
    listener: &mut PgConnection,
    mcp: &mut McpSession,
    svc: &KairosClient,
    title: &str,
    before: Footprint,
    what: &str,
) {
    assert_eq!(
        rows_titled(conn, title),
        0,
        "{what}: an item row with the title {title:?} exists"
    );
    assert_eq!(
        count_by(
            conn,
            "SELECT COUNT(*) AS n FROM org_acme.item_history WHERE title = $1",
            title
        ),
        0,
        "{what}: a history row with the title {title:?} exists"
    );
    assert_eq!(
        footprint(conn),
        before,
        "{what}: the refused create changed the row counts"
    );
    let events = drain_events(listener).await;
    assert!(events.is_empty(), "{what}: events were sent: {events:?}");

    let (is_error, text) = mcp.call("search", json!({ "q": title })).await;
    assert!(!is_error, "{what}: search: {text}");
    assert!(
        text.starts_with("0 match(es)"),
        "{what}: MCP search finds the item: {text}"
    );
    let found = svc
        .search(&SearchRequest {
            q: Some(title.to_string()),
            filter: None,
            traverse: None,
            sort: None,
            limit: None,
            offset: None,
        })
        .await
        .expect("REST search");
    assert_eq!(found.total, 0, "{what}: REST search finds the item");
}

#[tokio::test]
async fn a_refused_create_leaves_nothing_in_the_tenant() {
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
    let alice_token = user_token(&http, "alice").await;
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
    for client in [&svc, &alice] {
        let _ = client.whoami().await;
    }
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    add_member(&mut conn, acme, svc_id, OrgRole::Admin);
    add_member(&mut conn, acme, alice_id, OrgRole::Member);

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        None,
        None,
    )
    .expect("creating the delivery board");
    let initiative_board = board_id_of(&mut conn, BoardLevel::Initiative);
    let adr_board = board_id_of(&mut conn, BoardLevel::Adr);
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");

    let mut listener = PgConnection::establish(&scratch_url).expect("listener connection");
    listener
        .batch_execute("LISTEN kairos_events")
        .expect("LISTEN");

    let mut mcp = McpSession::open(&server.base_url, &svc_token).await;

    // =======================================================================
    // The items that the cases below name as a parent
    // =======================================================================
    let strategy = mcp
        .create(
            json!({"item_type": "strategy", "title": "Seed strategy"}),
            "ACME-S-",
        )
        .await;
    let initiative = mcp
        .create(
            json!({"item_type": "initiative", "title": "Seed initiative", "parent": strategy}),
            "ACME-I-",
        )
        .await;
    let task = mcp
        .create(
            json!({"item_type": "task", "title": "Seed task", "parent": initiative}),
            "ACME-T-",
        )
        .await;
    let document = mcp
        .create(
            json!({"item_type": "document", "title": "Seed document", "parent": initiative}),
            "ACME-D-",
        )
        .await;
    let adr = mcp
        .create(json!({"item_type": "adr", "title": "Seed ADR"}), "ACME-A-")
        .await;
    let archived = mcp
        .create(
            json!({"item_type": "initiative", "title": "Seed archived initiative"}),
            "ACME-I-",
        )
        .await;
    let (is_error, text) = mcp
        .call(
            "delete_item",
            json!({"short_code": archived, "confirm": true}),
        )
        .await;
    assert!(!is_error, "archiving the parent: {text}");

    // The control for the event check: the creates above sent events, and
    // the listener received them. Without this, "no events" proves nothing.
    let events = drain_events(&mut listener).await;
    assert!(
        !events.is_empty(),
        "the listener receives the events of a create that succeeds"
    );

    // =======================================================================
    // 1. The type rules refuse the relationship (criteria a, b, c)
    // =======================================================================
    //
    // One case for each item type that accepts `parent`. The parent is live
    // and the caller is an org admin, so the relationship is the one reason
    // for the refusal.
    let refused_by_rule: [(&str, &str, &str); 5] = [
        ("strategy", "Zorblatt strategy", initiative.as_str()),
        ("initiative", "Zorblatt initiative", task.as_str()),
        ("task", "Zorblatt task", strategy.as_str()),
        ("adr", "Zorblatt decision", adr.as_str()),
        ("document", "Zorblatt document", document.as_str()),
    ];
    for (item_type, title, parent) in refused_by_rule {
        let what = format!("{item_type} with the parent {parent}");
        let before = footprint(&mut conn);
        let (is_error, text) = mcp
            .call(
                "create_item",
                json!({
                    "item_type": item_type,
                    "title": title,
                    "content": "This content must not be stored.",
                    "parent": parent,
                }),
            )
            .await;
        assert!(is_error, "{what}: the create is refused: {text}");
        assert!(text.starts_with("VALIDATION"), "{what}: {text}");
        assert_nothing_written(
            &mut conn,
            &mut listener,
            &mut mcp,
            &svc,
            title,
            before,
            &what,
        )
        .await;
    }

    // =======================================================================
    // 2. The parent does not exist, or is archived
    // =======================================================================
    for (parent, why) in [("ACME-I-9999", "unknown"), (archived.as_str(), "archived")] {
        for item_type in ["strategy", "initiative", "task", "adr", "document"] {
            let title = format!("Quixlo {item_type} {why}");
            let what = format!("{item_type} with the {why} parent {parent}");
            let before = footprint(&mut conn);
            let (is_error, text) = mcp
                .call(
                    "create_item",
                    json!({"item_type": item_type, "title": title, "parent": parent}),
                )
                .await;
            assert!(is_error, "{what}: the create is refused: {text}");
            assert!(text.starts_with("VALIDATION"), "{what}: {text}");
            assert_nothing_written(
                &mut conn,
                &mut listener,
                &mut mcp,
                &svc,
                &title,
                before,
                &what,
            )
            .await;
        }
    }
    let before = footprint(&mut conn);
    let err = match svc
        .create_document(&CreateDocumentRequest {
            title: "Quixlo document over REST".into(),
            content: Some("This content must not be stored.".into()),
            template_id: None,
            parent_short_code: Some(adr.clone()),
        })
        .await
    {
        Ok(created) => panic!("REST created a document under an ADR: {created:?}"),
        Err(err) => err,
    };
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    assert_nothing_written(
        &mut conn,
        &mut listener,
        &mut mcp,
        &svc,
        "Quixlo document over REST",
        before,
        "REST document with an ADR as parent",
    )
    .await;

    // =======================================================================
    // 3. The edge insert fails after the item insert (the transaction)
    // =======================================================================
    //
    // No check can know this before the write. Each request here is valid:
    // without the trigger it succeeds. The item is written, the edge insert
    // raises, and the item must go with it.
    conn.batch_execute(
        "CREATE FUNCTION org_acme.t0227_refuse_edge() RETURNS trigger \
         LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'COLLIERY-T-0227: the edge insert fails'; END $$; \
         CREATE TRIGGER t0227_refuse_edge BEFORE INSERT ON org_acme.item_relationships \
         FOR EACH ROW EXECUTE FUNCTION org_acme.t0227_refuse_edge();",
    )
    .expect("installing the trigger");
    let fails_at_the_edge: [(&str, &str, &str); 4] = [
        ("initiative", "Vextrum initiative", strategy.as_str()),
        ("task", "Vextrum task", initiative.as_str()),
        ("adr", "Vextrum decision", initiative.as_str()),
        ("document", "Vextrum document", task.as_str()),
    ];
    for (item_type, title, parent) in fails_at_the_edge {
        let what = format!("{item_type} whose edge insert fails");
        let before = footprint(&mut conn);
        let (is_error, text) = mcp
            .call(
                "create_item",
                json!({
                    "item_type": item_type,
                    "title": title,
                    "content": "This content must not be stored.",
                    "parent": parent,
                }),
            )
            .await;
        assert!(is_error, "{what}: the create fails: {text}");
        assert_nothing_written(
            &mut conn,
            &mut listener,
            &mut mcp,
            &svc,
            title,
            before,
            &what,
        )
        .await;
    }
    let before = footprint(&mut conn);
    let result = svc
        .create_document(&CreateDocumentRequest {
            title: "Vextrum document over REST".into(),
            content: Some("This content must not be stored.".into()),
            template_id: None,
            parent_short_code: Some(initiative.clone()),
        })
        .await;
    assert!(result.is_err(), "REST: the create fails: {result:?}");
    assert_nothing_written(
        &mut conn,
        &mut listener,
        &mut mcp,
        &svc,
        "Vextrum document over REST",
        before,
        "REST document whose edge insert fails",
    )
    .await;
    conn.batch_execute(
        "DROP TRIGGER t0227_refuse_edge ON org_acme.item_relationships; \
         DROP FUNCTION org_acme.t0227_refuse_edge();",
    )
    .expect("removing the trigger");

    // =======================================================================
    // 4. An ADR with a parent gets the `supports` edge, as a document does
    // =======================================================================
    //
    // This is the call that found the defect. It was refused, because the
    // tool tried a `parent` edge, which no ADR can have.
    let (is_error, text) = mcp
        .call(
            "create_item",
            json!({"item_type": "adr", "title": "Plimsoll decision", "parent": initiative}),
        )
        .await;
    assert!(!is_error, "an ADR with an initiative as parent: {text}");
    assert!(text.contains("Created adr ACME-A-"), "{text}");
    assert!(
        text.contains(&format!("parent: {initiative} (supports edge created)")),
        "{text}"
    );
    assert_eq!(rows_titled(&mut conn, "Plimsoll decision"), 1);
    assert_eq!(supports_edges_to_adr(&mut conn, "Plimsoll decision"), 1);
    let _ = drain_events(&mut listener).await;

    // A member. `manage_adrs` on the ADR board lets her create the ADR. The
    // parent asks for what a document asks for: `manage_documents` on the
    // board of the parent. Without it the create is refused, and leaves
    // nothing.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    abac::grant_capability(&mut conn, adr_board, alice_id, "manage_adrs", svc_id)
        .expect("grant manage_adrs");
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");
    let mut alice_mcp = McpSession::open(&server.base_url, &alice_token).await;
    let before = footprint(&mut conn);
    let (is_error, text) = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "adr", "title": "Grommet decision", "parent": initiative}),
        )
        .await;
    assert!(
        is_error,
        "a member with no capability on the parent: {text}"
    );
    assert!(text.starts_with("FORBIDDEN"), "{text}");
    assert!(text.contains("manage_documents"), "{text}");
    assert_nothing_written(
        &mut conn,
        &mut listener,
        &mut mcp,
        &svc,
        "Grommet decision",
        before,
        "ADR from a member with no capability on the parent",
    )
    .await;

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    abac::grant_capability(
        &mut conn,
        initiative_board,
        alice_id,
        "manage_documents",
        svc_id,
    )
    .expect("grant manage_documents");
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");
    let (is_error, text) = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "adr", "title": "Grommet decision", "parent": initiative}),
        )
        .await;
    assert!(!is_error, "a member with both capabilities: {text}");
    assert_eq!(rows_titled(&mut conn, "Grommet decision"), 1);
    assert_eq!(supports_edges_to_adr(&mut conn, "Grommet decision"), 1);

    // An ADR with no parent is as it was: no edge.
    let before = footprint(&mut conn);
    let (is_error, text) = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "adr", "title": "Lone decision"}),
        )
        .await;
    assert!(!is_error, "an ADR with no parent: {text}");
    assert_eq!(footprint(&mut conn).edges, before.edges);

    drop(mcp);
    drop(alice_mcp);
    drop(listener);
    drop(conn);
    drop(server);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
