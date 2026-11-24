//! The work of a person (COLLIERY-I-0624), on the REST and MCP surfaces.
//!
//! KAIROS-T-0488: `search` filters by person.
//!
//! - `claimed_by: me` finds a task that alice claimed and a task that her
//!   agent key claimed, and not the claim of bob.
//! - `owned_by` and `created_by` work, and compose with the other filters.
//! - An unknown person is refused, and the answer names the field and the
//!   reference.
//!
//! KAIROS-T-0489: `GET /api/me/work` and the MCP tool `my_work`.
//!
//! - A fixture with my claims, my agent's claim, an initiative I own with
//!   unclaimed Todo tasks, and a blocked task gives the right lanes and
//!   states.
//! - Unclaimed Todo tasks not under my initiatives do not show.
//! - Done items do not show.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, TestServer, base_config, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::KairosClient;
use kairos_client::types::{CreateInitiativeRequest, CreateTaskRequest};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_search::{SearchFilter, SearchRequest};
use kairos_client::types_service_accounts::CreateApiKeyRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

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
                    "clientInfo": {"name": "kairos-for-me", "version": "0.0.0"},
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

    /// The text of a tool call, and whether it is an error.
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
}

/// The people, the boards and the clients of a fixture.
struct World {
    server: TestServer,
    svc: KairosClient,
    alice: KairosClient,
    bob: KairosClient,
    alice_token: String,
    agent_key: String,
    agent: KairosClient,
    alice_id: Uuid,
    bob_id: Uuid,
    /// The delivery board of the team `web` (UUID).
    board: String,
    /// The columns of the delivery board, by name.
    columns: HashMap<String, String>,
    /// The columns of the initiatives board, by name.
    initiative_columns: HashMap<String, String>,
    /// The scratch database: it goes away with the world.
    _scratch: common::ScratchConn,
}

impl World {
    /// A scratch tenant `acme`: svc is the admin; alice and bob are members
    /// of the team `web` and have `configure_boards` on the initiatives
    /// board (so that they can own an initiative). alice has an agent key.
    async fn new(scratch_db: &str) -> (World, PgConnection) {
        let scratch = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn = PgConnection::establish(&scratch_url).expect("scratch database");
        run_public_migrations(&mut conn).expect("public migrations");
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
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        let state = app::state_with(base_config(&scratch_url), pool, auth);
        let server = spawn_server(app::router(state)).await;
        let svc = server.client(&svc_token, "acme");
        let alice = server.client(&alice_token, "acme");
        let bob = server.client(&bob_token, "acme");
        for client in [&svc, &alice, &bob] {
            let _ = client.whoami().await;
        }
        let mut ids = Vec::new();
        for (email, role) in [
            ("svc@kairos.test", OrgRole::Admin),
            ("alice@kairos.test", OrgRole::Member),
            ("bob@kairos.test", OrgRole::Member),
        ] {
            let user_id: Uuid = users::table
                .filter(users::email.eq(email))
                .select(users::id)
                .first(&mut conn)
                .expect("user row");
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id,
                    role,
                })
                .execute(&mut conn)
                .expect("membership");
            ids.push(user_id);
        }
        let (alice_id, bob_id) = (ids[1], ids[2]);
        let team = svc
            .create_team(&CreateTeamRequest {
                name: "Web".into(),
                slug: "web".into(),
                code_prefix: "WEB".into(),
                team_type: None,
            })
            .await
            .expect("team");
        for user_id in [alice_id, bob_id] {
            svc.add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
        }
        let board = team.delivery_board_id.clone().expect("delivery board");
        let columns = svc
            .get_board(&board)
            .await
            .expect("board")
            .columns
            .into_iter()
            .map(|c| (c.name, c.id))
            .collect();
        let initiatives = svc.get_board("initiatives").await.expect("initiatives");
        for user_id in [alice_id, bob_id] {
            svc.add_board_member(
                &initiatives.board.id,
                &AddBoardMemberRequest {
                    user_id: user_id.to_string(),
                    capabilities: vec!["configure_boards".into()],
                },
            )
            .await
            .expect("initiative board member");
        }
        let initiative_columns = initiatives
            .columns
            .into_iter()
            .map(|c| (c.name, c.id))
            .collect();
        let agent_key = alice
            .create_agent_key(&CreateApiKeyRequest {
                name: "laptop".into(),
                expires_at: None,
                no_expiry: false,
                scope: None,
            })
            .await
            .expect("agent key")
            .key;
        let agent = server.client(&agent_key, "acme");
        (
            World {
                server,
                svc,
                alice,
                bob,
                alice_token,
                agent_key,
                agent,
                alice_id,
                bob_id,
                board,
                columns,
                initiative_columns,
                _scratch: scratch,
            },
            conn,
        )
    }

    /// A new task on the delivery board, by `client`. Returns its code.
    async fn task(&self, client: &KairosClient, title: &str) -> String {
        client
            .create_task(&CreateTaskRequest {
                board_id: Some(self.board.clone()),
                column_id: None,
                title: title.into(),
                content: "one".into(),
                task_type: None,
                work_class: None,
                team_id: None,
                repository: None,
                from_team: None,
                parent: None,
            })
            .await
            .expect("task")
            .short_code
    }

    /// Move the task through `path` (column names), one hop at a time.
    async fn walk(&self, client: &KairosClient, code: &str, path: &[&str]) {
        for name in path {
            client
                .transition_task(code, &self.columns[*name])
                .await
                .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
        }
    }

    /// A new initiative, by svc. Returns its code.
    async fn initiative(&self, title: &str) -> String {
        self.svc
            .create_initiative(&CreateInitiativeRequest {
                board_id: "initiatives".into(),
                column_id: None,
                title: title.into(),
                content: String::new(),
                complexity: None,
                bucket_type: None,
                parent: None,
            })
            .await
            .expect("initiative")
            .short_code
    }

    /// Move the initiative through `path` (column names), by svc.
    async fn walk_initiative(&self, code: &str, path: &[&str]) {
        for name in path {
            self.svc
                .transition_initiative(code, &self.initiative_columns[*name])
                .await
                .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
        }
    }

    /// The person behind `token` claims the initiative (KAIROS-T-0411).
    async fn own(&self, token: &str, code: &str) {
        let response = reqwest::Client::new()
            .post(format!(
                "{}/api/initiatives/{code}/owner/claim",
                self.server.base_url
            ))
            .bearer_auth(token)
            .header("x-tenant", "acme")
            .send()
            .await
            .expect("claim");
        let status = response.status();
        assert!(
            status.is_success(),
            "{status}: {}",
            response.text().await.unwrap_or_default()
        );
    }

    /// A `relationship` edge from `source` to `target`, by svc.
    async fn link(&self, source: &str, target: &str, relationship: &str) {
        self.svc
            .create_relationship(&CreateRelationshipRequest {
                source_short_code: source.into(),
                target_short_code: target.into(),
                relationship: relationship.into(),
            })
            .await
            .unwrap_or_else(|e| panic!("{source} {relationship} {target}: {e}"));
    }
}

/// The codes of each item of a search answer.
fn codes(response: &kairos_client::types_search::SearchResponse) -> BTreeSet<String> {
    let groups = &response.results;
    groups
        .strategies
        .iter()
        .map(|s| s.short_code.clone())
        .chain(groups.initiatives.iter().map(|i| i.short_code.clone()))
        .chain(groups.tasks.iter().map(|t| t.short_code.clone()))
        .chain(groups.documents.iter().map(|d| d.short_code.clone()))
        .chain(groups.adrs.iter().map(|a| a.short_code.clone()))
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn filtered(filter: SearchFilter) -> SearchRequest {
    SearchRequest {
        filter: Some(filter),
        ..Default::default()
    }
}

#[tokio::test]
async fn search_finds_the_items_of_a_person() {
    let (world, mut conn) = World::new("kairos_for_me_search_t0488_test").await;
    let w = &world;
    let bob_user_name: String = users::table
        .filter(users::id.eq(w.bob_id))
        .select(users::user_name)
        .first(&mut conn)
        .expect("user name of bob");

    // alice claims mine; her agent claims agents (made by bob); bob claims
    // bobs (made by alice).
    let mine = w.task(&w.alice, "Mine").await;
    let agents = w.task(&w.bob, "Agents").await;
    let bobs = w.task(&w.alice, "Bobs").await;
    w.walk(&w.alice, &mine, &["Todo", "Active"]).await;
    w.walk(&w.agent, &agents, &["Todo", "Active"]).await;
    w.walk(&w.bob, &bobs, &["Todo", "Active"]).await;
    let owned = w.initiative("Owned").await;
    let other = w.initiative("Other").await;
    w.own(&w.alice_token, &owned).await;

    // --- claimed_by: me, also through the agent key.
    for client in [&w.alice, &w.agent] {
        let found = client
            .search(&filtered(SearchFilter {
                claimed_by: Some("me".into()),
                ..Default::default()
            }))
            .await
            .expect("claimed_by me");
        assert_eq!(codes(&found), set(&[&mine, &agents]));
    }
    // By email, by user name and by id.
    for reference in [
        "bob@kairos.test".to_string(),
        bob_user_name,
        w.bob_id.to_string(),
    ] {
        let found = w
            .alice
            .search(&filtered(SearchFilter {
                claimed_by: Some(reference.clone()),
                ..Default::default()
            }))
            .await
            .expect("claimed_by bob");
        assert_eq!(codes(&found), set(&[&bobs]), "{reference}");
    }

    // --- owned_by: only strategies and initiatives.
    let found = w
        .bob
        .search(&filtered(SearchFilter {
            owned_by: Some(w.alice_id.to_string()),
            ..Default::default()
        }))
        .await
        .expect("owned_by alice");
    assert_eq!(codes(&found), set(&[&owned]));
    assert!(!codes(&found).contains(&other));

    // --- created_by, and how it composes.
    let found = w
        .alice
        .search(&filtered(SearchFilter {
            created_by: Some("me".into()),
            entity_type: Some(vec!["task".into()]),
            ..Default::default()
        }))
        .await
        .expect("created_by me");
    assert_eq!(codes(&found), set(&[&mine, &bobs]));
    let found = w
        .alice
        .search(&filtered(SearchFilter {
            created_by: Some("me".into()),
            claimed_by: Some("me".into()),
            ..Default::default()
        }))
        .await
        .expect("created_by and claimed_by");
    assert_eq!(codes(&found), set(&[&mine]));
    let found = w
        .alice
        .search(&SearchRequest {
            q: Some("Agents".into()),
            filter: Some(SearchFilter {
                claimed_by: Some("me".into()),
                board: Some(w.board.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .expect("q, board and claimed_by");
    assert_eq!(codes(&found), set(&[&agents]));
    let found = w
        .svc
        .search(&filtered(SearchFilter {
            created_by: Some("me".into()),
            entity_type: Some(vec!["initiative".into()]),
            ..Default::default()
        }))
        .await
        .expect("svc created the initiatives");
    assert_eq!(codes(&found), set(&[&owned, &other]));

    // --- An unknown person is refused and named.
    let response = reqwest::Client::new()
        .post(format!("{}/api/search", w.server.base_url))
        .bearer_auth(&w.alice_token)
        .header("x-tenant", "acme")
        .json(&json!({"filter": {"owned_by": "nobody@kairos.test"}}))
        .send()
        .await
        .expect("search");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("JSON");
    assert_eq!(body["error"]["code"], json!("VALIDATION"), "{body}");
    assert_eq!(body["error"]["details"]["field"], json!("filter.owned_by"));
    assert!(
        body.to_string().contains("nobody@kairos.test"),
        "the answer names the person: {body}"
    );

    // --- MCP `search`, with the agent key.
    let mut mcp = Mcp::connect(&w.server.base_url, &w.agent_key).await;
    let (text, error) = mcp
        .call("search", json!({"filter": {"claimed_by": "me"}}))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains(&mine) && text.contains(&agents), "{text}");
    assert!(!text.contains(&bobs), "{text}");
    // KAIROS-T-0561: each line names who has the claim, and an agent key.
    let line = |code: &str| {
        text.lines()
            .find(|line| line.starts_with(&format!("- {code} ")))
            .unwrap_or_else(|| panic!("no line for {code}: {text}"))
            .to_string()
    };
    assert!(line(&mine).contains(", claimed by "), "{text}");
    assert!(!line(&mine).contains("(agent)"), "{text}");
    assert!(line(&agents).contains(" (agent)"), "{text}");
    let (text, error) = mcp
        .call("search", json!({"filter": {"owned_by": "me"}}))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains(&owned) && !text.contains(&other), "{text}");
    let (text, error) = mcp
        .call("search", json!({"filter": {"created_by": "carol-unknown"}}))
        .await;
    assert!(error, "{text}");
    assert!(
        text.contains("carol-unknown") && text.contains("filter.created_by"),
        "{text}"
    );
    let (text, error) = mcp
        .call("search", json!({"filter": {"claimed": "me"}}))
        .await;
    assert!(error, "an unknown field is refused: {text}");
    assert!(text.contains("claimed"), "{text}");
}

/// The item of `code` in the lane, or a panic that names the lane.
fn item<'a>(
    lane: &'a [kairos_client::types_work::WorkItem],
    code: &str,
) -> &'a kairos_client::types_work::WorkItem {
    lane.iter()
        .find(|i| i.short_code == code)
        .unwrap_or_else(|| panic!("{code} is not in {lane:?}"))
}

fn lane_codes(lane: &[kairos_client::types_work::WorkItem]) -> BTreeSet<String> {
    lane.iter().map(|i| i.short_code.clone()).collect()
}

#[tokio::test]
async fn my_work_gives_the_lanes_and_the_states() {
    let (world, _conn) = World::new("kairos_for_me_work_t0489_test").await;
    let w = &world;
    let to_active = ["Design", "Decompose", "Ready", "Active"];

    // --- Initiatives: alice owns three, one of them done; bob owns one.
    let active_initiative = w.initiative("Checkout").await;
    let new_initiative = w.initiative("Discovery work").await;
    let done_initiative = w.initiative("Finished").await;
    let bobs_initiative = w.initiative("Bob's").await;
    for code in [&active_initiative, &new_initiative, &done_initiative] {
        w.own(&w.alice_token, code).await;
    }
    let bob_token = {
        let http = reqwest::Client::new();
        common::user_token(&http, "bob").await
    };
    w.own(&bob_token, &bobs_initiative).await;
    w.walk_initiative(&active_initiative, &to_active).await;
    w.walk_initiative(&done_initiative, &to_active).await;
    w.walk_initiative(&done_initiative, &["Monitoring", "Completed"])
        .await;

    // --- Tasks.
    let mine = w.task(&w.alice, "Mine").await;
    w.walk(&w.alice, &mine, &["Todo", "Active"]).await;
    let agents = w.task(&w.alice, "Agent's").await;
    w.walk(&w.agent, &agents, &["Todo", "Active"]).await;
    let blocker = w.task(&w.bob, "Blocker of the agent's task").await;
    w.walk(&w.bob, &blocker, &["Todo"]).await;
    w.link(&blocker, &agents, "blocks").await;

    let free = w.task(&w.bob, "Free under Checkout").await;
    w.walk(&w.bob, &free, &["Todo"]).await;
    let bobs_under = w.task(&w.bob, "Bob's under Checkout").await;
    w.walk(&w.bob, &bobs_under, &["Todo", "Active"]).await;
    let backlog_under = w.task(&w.bob, "Backlog under Checkout").await;
    let done_under = w.task(&w.bob, "Done under Checkout").await;
    w.walk(&w.bob, &done_under, &["Todo", "Active", "Completed"])
        .await;
    let column_blocked = w.task(&w.bob, "In the Blocked column").await;
    w.walk(&w.bob, &column_blocked, &["Todo", "Blocked"]).await;
    for code in [
        &free,
        &bobs_under,
        &backlog_under,
        &done_under,
        &column_blocked,
    ] {
        w.link(&active_initiative, code, "parent").await;
    }
    // A done blocker does not block.
    let done_blocker = w.task(&w.bob, "Done blocker").await;
    w.link(&done_blocker, &free, "blocks").await;
    w.walk(&w.bob, &done_blocker, &["Todo", "Active", "Completed"])
        .await;

    // Not alice's: unclaimed in Todo under bob's initiative.
    let stranger = w.task(&w.bob, "Under bob's initiative").await;
    w.walk(&w.bob, &stranger, &["Todo"]).await;
    w.link(&bobs_initiative, &stranger, "parent").await;
    // A task that mine blocks, in Backlog.
    let waits = w.task(&w.bob, "Waits for mine").await;
    w.link(&mine, &waits, "blocks").await;

    // --- REST, by alice and by her agent key: the same work.
    let work = w.alice.my_work().await.expect("my work");
    assert_eq!(w.agent.my_work().await.expect("agent's work"), work);

    assert_eq!(
        lane_codes(&work.initiatives),
        set(&[&active_initiative, &new_initiative]),
        "the done initiative and bob's do not show"
    );
    let checkout = item(&work.initiatives, &active_initiative);
    assert_eq!(checkout.state, "active");
    assert_eq!(checkout.entity_type, "initiative");
    assert_eq!(checkout.reasons, vec!["owned".to_string()]);
    assert_eq!(
        checkout.owner.as_ref().map(|o| o.user_id.clone()),
        Some(w.alice_id.to_string())
    );
    assert_eq!(item(&work.initiatives, &new_initiative).state, "todo");

    assert_eq!(
        lane_codes(&work.tasks),
        set(&[&mine, &agents, &free, &bobs_under, &column_blocked, &waits]),
        "backlog, done, stranger and blocker do not show"
    );
    let t = item(&work.tasks, &mine);
    assert_eq!(
        (t.state.as_str(), t.reasons.clone()),
        ("active", vec!["claimed".to_string()])
    );
    let claim = t.claim.as_ref().expect("claim");
    assert_eq!(claim.user_id, w.alice_id.to_string());
    assert!(!claim.agent);
    assert_eq!(t.board.id, w.board);
    assert_eq!(t.board.slug, "web-delivery");
    assert_eq!(t.column_name, "Active");

    let t = item(&work.tasks, &agents);
    assert_eq!(t.state, "blocked", "an open blocker blocks: {t:?}");
    assert!(t.claim.as_ref().expect("claim").agent);
    assert_eq!(
        t.blockers
            .iter()
            .map(|b| b.short_code.clone())
            .collect::<Vec<_>>(),
        vec![blocker.clone()]
    );

    let t = item(&work.tasks, &free);
    assert_eq!(t.state, "todo");
    assert_eq!(t.reasons, vec!["under_my_initiative".to_string()]);
    assert!(t.claim.is_none());
    assert!(t.blockers.is_empty(), "a done blocker is not open: {t:?}");

    let t = item(&work.tasks, &bobs_under);
    assert_eq!(t.state, "active");
    assert_eq!(
        t.claim.as_ref().map(|c| c.user_id.clone()),
        Some(w.bob_id.to_string())
    );

    let t = item(&work.tasks, &column_blocked);
    assert_eq!(t.state, "blocked", "the Blocked column blocks");
    assert!(t.blockers.is_empty());

    let t = item(&work.tasks, &waits);
    assert_eq!(t.state, "blocked");
    assert_eq!(t.reasons, vec!["blocked_by_my_work".to_string()]);
    assert_eq!(t.column_name, "Backlog");
    assert_eq!(t.blockers[0].short_code, mine);

    // --- bob: his initiative, his claim, and the stranger under it.
    let bobs = w.bob.my_work().await.expect("bob's work");
    assert_eq!(lane_codes(&bobs.initiatives), set(&[&bobs_initiative]));
    assert_eq!(lane_codes(&bobs.tasks), set(&[&bobs_under, &stranger]));

    // --- svc owns nothing and claims nothing.
    let none = w.svc.my_work().await.expect("svc's work");
    assert!(
        none.initiatives.is_empty() && none.tasks.is_empty(),
        "{none:?}"
    );

    // --- MCP `my_work`, with the agent key.
    let mut mcp = Mcp::connect(&w.server.base_url, &w.agent_key).await;
    let (text, error) = mcp.call("my_work", json!({})).await;
    assert!(!error, "{text}");
    assert!(text.starts_with("# Your work"), "{text}");
    assert!(text.contains("## Initiatives (2)"), "{text}");
    assert!(text.contains("## Tasks (6)"), "{text}");
    assert!(text.contains(&format!("  blocked by {blocker} ")), "{text}");
    assert!(text.contains("(agent)"), "{text}");
    for absent in [&done_initiative, &backlog_under, &done_under, &stranger] {
        assert!(!text.contains(absent.as_str()), "{absent}: {text}");
    }
    let todo = text.find("### Todo").expect("Todo");
    let active = text.rfind("### Active").expect("Active");
    let blocked = text.rfind("### Blocked").expect("Blocked");
    assert!(todo < active && active < blocked, "{text}");
    let (text, error) = mcp.call("my_work", json!({"who": "me"})).await;
    assert!(error, "an unknown argument is refused: {text}");
    assert!(text.contains("who"), "{text}");
}
