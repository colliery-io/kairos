//! Integration test for the unified search/traverse pipeline
//! (KAIROS-T-0014, contract per KAIROS-A-0007, request shape per
//! KAIROS-S-0005).
//!
//! Runs against the real compose Postgres (`angreal services up`); the
//! database is never mocked (KAIROS-A-0012). For isolation the test drops
//! and recreates a dedicated scratch database (`kairos_search_test`) on the
//! same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - each capability alone: `q` (websearch tsquery via `searchable_items`),
//!   `filter` (entity_type, task_type, board_id, column_id, team_id,
//!   is_bucket, metadata exact + trailing-`*` glob with literal LIKE
//!   metacharacters, date ranges), `traverse` (descendants via `parent`
//!   outbound, inbound blockers)
//! - ALL composition examples from S-0005's Unified Search section,
//!   including q+filter+traverse combined
//! - hydration bounded at ≤5 queries regardless of result size, results
//!   grouped by type and fully typed
//! - depth cap enforced (deep chain truncated at the traversal depth;
//!   over-cap depth is a typed validation error)
//! - soft-deleted excluded by default, `include_deleted` honored on the
//!   filter, traverse, full-text (`q`) and traverse-root paths — and
//!   accepted as a whole request on its own (KAIROS-T-0157 / KAIROS-A-0020)
//! - sort + pagination over the combined result set (`total` counted
//!   before the page is cut)
//! - pathological fan-out (200 tasks under one initiative) completes
//!   within the stated budget with exactly one task-hydration query

use std::collections::HashSet;
use std::time::{Duration, Instant};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::json;
use uuid::Uuid;

use kairos_core::search::{SearchRequest, SearchValidationError};
use kairos_core::short_code::ItemType;
use kairos_db::items::{
    self, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy, CreateTask,
};
use kairos_db::models::{
    BoardLevel, BucketType, FieldType, MetadataDefinition, NewItemMetadata, NewMetadataDefinition,
    NewTeam, NewUser, RelationshipType, TaskType, Team, User,
};
use kairos_db::search::{
    self, SearchError, SearchResults, SearchStats, execute_search, execute_search_with_stats,
};
use kairos_db::{create_board, graph, provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_search_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

fn insert_user(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid {
    diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: external_id.into(),
            user_name: external_id.into(),
            email: email.into(),
            display_name: name.into(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

/// The board with this slug in the current tenant schema.
fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::boards::table
        .filter(schema::boards::slug.eq(slug))
        .select(schema::boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

/// The tenant's metadata definition with this slug, creating it if the
/// provisioning defaults did not already seed one.
fn metadata_definition(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid {
    if let Some(id) = schema::metadata_definitions::table
        .filter(schema::metadata_definitions::slug.eq(slug))
        .select(schema::metadata_definitions::id)
        .first::<Uuid>(conn)
        .optional()
        .expect("looking up metadata definition")
    {
        return id;
    }
    diesel::insert_into(schema::metadata_definitions::table)
        .values(NewMetadataDefinition {
            name: name.into(),
            slug: slug.into(),
            field_type: FieldType::String,
            is_system_default: false,
        })
        .returning(MetadataDefinition::as_returning())
        .get_result(conn)
        .expect("inserting metadata definition")
        .id
}

fn set_metadata(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str) {
    diesel::insert_into(schema::item_metadata::table)
        .values(NewItemMetadata {
            item_id,
            metadata_definition_id: definition_id,
            value: value.into(),
        })
        .execute(conn)
        .expect("inserting item metadata");
}

/// Parse a request from its JSON shape (the S-0005 wire format) and run it
/// with stats.
fn run(conn: &mut PgConnection, request: serde_json::Value) -> (SearchResults, SearchStats) {
    let request: SearchRequest =
        serde_json::from_value(request.clone()).unwrap_or_else(|e| panic!("{request} parses: {e}"));
    execute_search_with_stats(conn, &request)
        .unwrap_or_else(|e| panic!("search {request:?} failed: {e}"))
}

fn ids<T>(rows: &[T], id_of: impl Fn(&T) -> Uuid) -> HashSet<Uuid> {
    rows.iter().map(id_of).collect()
}

fn task_ids(results: &SearchResults) -> HashSet<Uuid> {
    ids(&results.tasks, |t| t.id)
}

fn all_ids(results: &SearchResults) -> HashSet<Uuid> {
    let mut set = HashSet::new();
    set.extend(results.strategies.iter().map(|r| r.id));
    set.extend(results.initiatives.iter().map(|r| r.id));
    set.extend(results.tasks.iter().map(|r| r.id));
    set.extend(results.documents.iter().map(|r| r.id));
    set.extend(results.adrs.iter().map(|r| r.id));
    set
}

fn returned_row_count(results: &SearchResults) -> usize {
    results.strategies.len()
        + results.initiatives.len()
        + results.tasks.len()
        + results.documents.len()
        + results.adrs.len()
}

#[test]
fn unified_search_pipeline() {
    let admin_url = admin_database_url();

    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");

    let scratch_url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let alice = insert_user(&mut conn, "dex|alice", "alice@acme.test", "Alice");

    let strategy_board = board_id_by_slug(&mut conn, "strategy");
    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let adr_board = board_id_by_slug(&mut conn, "adrs");
    let delivery_board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("creating delivery board")
    .id;
    // Second delivery board quarantines the depth-cap chain and the
    // fan-out graph so board-scoped assertions stay exact.
    let scratch_board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Scratch",
        "scratch",
        None,
        Some(alice),
    )
    .expect("creating scratch board")
    .id;

    let auth_team: Team = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Auth Team".into(),
            slug: "auth-team".into(),
            team_type: kairos_db::models::TeamType::StreamAligned,
        })
        .returning(Team::as_returning())
        .get_result(&mut conn)
        .expect("inserting team");

    // ---- fixtures: one strategy -> two initiatives -> four tasks, plus a
    // document, an ADR, metadata, blockers, and one soft-deleted task --------
    let s1 = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: "Authentication overhaul",
            content: "Rework authentication and identity across the platform",
            hypothesis: None,
        },
        alice,
    )
    .expect("creating strategy");

    let make_initiative = |conn: &mut PgConnection, title: &str, content: &str, bucket| {
        items::create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: None,
                title,
                content,
                complexity: None,
                bucket_type: bucket,
            },
            alice,
        )
        .expect("creating initiative")
    };
    let i1 = make_initiative(
        &mut conn,
        "Auth service",
        "OAuth flows and token handling",
        None,
    );
    let i2 = make_initiative(
        &mut conn,
        "Session management",
        "Cookies and server-side session storage",
        None,
    );
    let i_bucket = make_initiative(
        &mut conn,
        "Bug bucket",
        "Intake bucket for stray bug reports",
        Some(BucketType::Bug),
    );

    let make_task =
        |conn: &mut PgConnection, board: Uuid, title: &str, content: &str, task_type, team| {
            items::create_task(
                conn,
                CreateTask {
                    board_id: board,
                    column_id: None,
                    title,
                    content,
                    task_type,
                    work_class: kairos_db::models::enums::WorkClass::Planned,
                    team_id: team,
                    repository_id: None,
                },
                alice,
            )
            .expect("creating task")
        };
    let t1 = make_task(
        &mut conn,
        delivery_board,
        "Implement login endpoint",
        "Wire up the auth login endpoint issuing authentication tokens",
        TaskType::Bug,
        Some(auth_team.id),
    );
    let t2 = make_task(
        &mut conn,
        delivery_board,
        "Fix logout redirect",
        "Logout leaves a dangling redirect loop",
        TaskType::Bug,
        None,
    );
    let t3 = make_task(
        &mut conn,
        delivery_board,
        "Refactor session store",
        "Move session persistence behind one interface",
        TaskType::TechDebt,
        None,
    );
    let t4 = make_task(
        &mut conn,
        delivery_board,
        "Write onboarding notes",
        "Collect the delivery onboarding notes",
        TaskType::Task,
        None,
    );

    // KAIROS-T-0186 relevance fixtures. Their own board, for the same reason
    // `scratch_board` has one — board-scoped assertions elsewhere stay exact —
    // and terms that appear nowhere else in the fixture set, so the ranking
    // assertions cannot be perturbed by another case's prose.
    let relevance_board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Relevance",
        "relevance",
        None,
        Some(alice),
    )
    .expect("creating relevance board")
    .id;
    // "zephyr" in the title of one, the body of the other, once each.
    let rank_title = make_task(
        &mut conn,
        relevance_board,
        "Zephyr ingestion pipeline",
        "Batching and retry behaviour for the inbound feed",
        TaskType::Task,
        None,
    );
    let rank_body = make_task(
        &mut conn,
        relevance_board,
        "Inbound feed batching",
        "Retry behaviour for the zephyr feed, which needs care",
        TaskType::Task,
        None,
    );
    // Identical text, so `ts_rank_cd` scores them equally and only the
    // tie-break can order them.
    let tie_first = make_task(
        &mut conn,
        relevance_board,
        "Tessellate the layout",
        "Identical prose, so the scores tie exactly",
        TaskType::Task,
        None,
    );
    let tie_second = make_task(
        &mut conn,
        relevance_board,
        "Tessellate the layout",
        "Identical prose, so the scores tie exactly",
        TaskType::Task,
        None,
    );

    // KAIROS-T-0077: one bug rides the Support lane for the work_class
    // filter cases below (the two axes stay independent).
    items::set_task_work_class(
        &mut conn,
        t2.id,
        kairos_db::models::enums::WorkClass::Support,
        alice,
    )
    .expect("setting work_class");

    let d1 = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Search design notes",
            content: Some("Hydration pipeline and traversal sketches"),
            template_id: None,
        },
        alice,
    )
    .expect("creating document");

    let a1 = items::create_adr(
        &mut conn,
        CreateAdr {
            board_id: Some(adr_board),
            column_id: None,
            title: "Unified search endpoint",
            content: "One endpoint, composable query capabilities",
            decision_maker: None,
            decision_date: None,
        },
        alice,
    )
    .expect("creating adr");

    // Graph: s1 -> {i1, i2, i_bucket} -> tasks; supports edges hang the
    // document and ADR off the tree; blocks edges feed the inbound tests.
    let link = |conn: &mut PgConnection, source: Uuid, target: Uuid, rel| {
        graph::link_items(conn, source, target, rel, alice)
            .unwrap_or_else(|e| panic!("linking {source} -{rel:?}-> {target}: {e}"));
    };
    link(&mut conn, s1.id, i1.id, RelationshipType::Parent);
    link(&mut conn, s1.id, i2.id, RelationshipType::Parent);
    link(&mut conn, s1.id, i_bucket.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t1.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t2.id, RelationshipType::Parent);
    link(&mut conn, i2.id, t3.id, RelationshipType::Parent);
    link(&mut conn, i2.id, t4.id, RelationshipType::Parent);
    link(&mut conn, i1.id, d1.id, RelationshipType::Supports);
    link(&mut conn, s1.id, a1.id, RelationshipType::Supports);
    link(&mut conn, t3.id, i1.id, RelationshipType::Blocks);
    link(&mut conn, t2.id, t1.id, RelationshipType::Blocks);

    // Metadata: priority + component definitions; t1 is the critical
    // auth-api bug, t2 low/web-ui, i1 critical.
    let priority = metadata_definition(&mut conn, "Priority", "priority");
    let component = metadata_definition(&mut conn, "Component", "component");
    set_metadata(&mut conn, t1.id, priority, "critical");
    set_metadata(&mut conn, t1.id, component, "auth-api");
    set_metadata(&mut conn, t2.id, priority, "low");
    set_metadata(&mut conn, t2.id, component, "web-ui");
    set_metadata(&mut conn, i1.id, priority, "critical");

    // Soft-delete t4 AFTER linking so its parent edge stays in the graph
    // (traversal walks raw edges; visibility is a hydration concern).
    items::soft_delete_item(&mut conn, ItemType::Task, t4.id, alice).expect("soft-deleting t4");

    // ==========================================================================
    // Capability: q alone (websearch_to_tsquery via searchable_items)
    // ==========================================================================
    let (results, _) = run(&mut conn, json!({"q": "authentication"}));
    assert_eq!(
        all_ids(&results),
        HashSet::from([s1.id, t1.id]),
        "q matches title/content stems across entity types"
    );
    assert_eq!(results.total, 2);
    assert_eq!(ids(&results.strategies, |r| r.id), HashSet::from([s1.id]));
    assert_eq!(task_ids(&results), HashSet::from([t1.id]));

    // Multi-word user input never errors (websearch semantics).
    let (results, _) = run(&mut conn, json!({"q": "session storage"}));
    assert_eq!(all_ids(&results), HashSet::from([i2.id]));

    // ==========================================================================
    // Capability: filter alone, every S-0005 field
    // ==========================================================================
    // entity_type
    let (results, _) = run(&mut conn, json!({"filter": {"entity_type": ["document"]}}));
    assert_eq!(all_ids(&results), HashSet::from([d1.id]));

    // task_type (implies tasks only)
    let (results, _) = run(&mut conn, json!({"filter": {"task_type": ["tech_debt"]}}));
    assert!(results.tasks.iter().any(|t| t.id == t3.id));
    assert!(
        results.strategies.is_empty()
            && results.initiatives.is_empty()
            && results.documents.is_empty()
            && results.adrs.is_empty(),
        "task_type filter can only ever match tasks"
    );

    // work_class (KAIROS-T-0077; implies tasks only)
    let (results, _) = run(&mut conn, json!({"filter": {"work_class": ["support"]}}));
    assert_eq!(all_ids(&results), HashSet::from([t2.id]));
    // The two axes compose: a Support-lane BUG keeps its bug type.
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"task_type": ["bug"], "work_class": ["support"]}}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t2.id]));

    // board_id (documents structurally excluded; t4 soft-deleted)
    let (results, stats) = run(&mut conn, json!({"filter": {"board_id": delivery_board}}));
    assert_eq!(all_ids(&results), HashSet::from([t1.id, t2.id, t3.id]));
    assert_eq!(
        stats.hydration_queries, 4,
        "board filter hydrates the four board-bearing types only"
    );

    // column_id (tasks land in the delivery board's first column)
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"entity_type": ["task"], "column_id": t1.column_id}}),
    );
    assert_eq!(task_ids(&results), HashSet::from([t1.id, t2.id, t3.id]));

    // team_id
    let (results, _) = run(&mut conn, json!({"filter": {"team_id": auth_team.id}}));
    assert_eq!(all_ids(&results), HashSet::from([t1.id]));

    // is_bucket
    let (results, _) = run(&mut conn, json!({"filter": {"is_bucket": true}}));
    assert_eq!(all_ids(&results), HashSet::from([i_bucket.id]));

    // metadata: exact value matches across entity types, grouped by type
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"metadata": {"priority": "critical"}}}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id, i1.id]));
    assert_eq!(ids(&results.initiatives, |r| r.id), HashSet::from([i1.id]));
    assert_eq!(task_ids(&results), HashSet::from([t1.id]));

    // metadata: trailing-* glob (T-0011 translation)
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"metadata": {"component": "auth*"}}}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id]));

    // metadata: multiple entries AND together
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"metadata": {"priority": "critical", "component": "auth*"}}}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id]));
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"metadata": {"priority": "low", "component": "auth*"}}}),
    );
    assert_eq!(results.total, 0);

    // metadata: LIKE metacharacters in values stay literal — '_' must not
    // act as a single-character wildcard ("web-ui" is not matched).
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"metadata": {"component": "web_ui"}}}),
    );
    assert_eq!(results.total, 0);

    // date ranges (strict bounds around s1's creation instant)
    let (results, _) = run(
        &mut conn,
        json!({"filter": {
            "entity_type": ["strategy"],
            "created_after": s1.created_at - chrono::Duration::seconds(1),
            "created_before": s1.created_at + chrono::Duration::seconds(1),
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([s1.id]));
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"entity_type": ["strategy"], "created_after": s1.created_at}}),
    );
    assert_eq!(results.total, 0, "created_after is strict");

    // ==========================================================================
    // Capability: traverse alone
    // ==========================================================================
    // Descendants of s1 (parent, outbound): initiatives + live tasks; t4 is
    // soft-deleted and excluded by default, the root is never returned.
    let (results, stats) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": s1.short_code},
            "relationships": ["parent"], "direction": "outbound", "depth": 5
        }}),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([i1.id, i2.id, i_bucket.id, t1.id, t2.id, t3.id])
    );
    assert_eq!(
        stats.hydration_queries, 2,
        "only the types present in the traversal are hydrated"
    );

    // traverse by id instead of short_code
    let (results, _) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"id": i1.id},
            "relationships": ["parent"], "direction": "outbound", "depth": 1
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id, t2.id]));

    // Inbound blockers: everything blocking t1.
    let (results, _) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": t1.short_code},
            "relationships": ["blocks"], "direction": "inbound", "depth": 1
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t2.id]));

    // direction: both (t1's blocks neighborhood in either orientation).
    let (results, _) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": t2.short_code},
            "relationships": ["blocks", "parent"], "direction": "both", "depth": 1
        }}),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([t1.id, i1.id]),
        "both follows blocks outbound and parent inbound from t2"
    );

    // Unknown root is a typed error.
    let request: SearchRequest = serde_json::from_value(json!({"traverse": {
        "from": {"short_code": "ACME-S-9999"},
        "relationships": ["parent"], "direction": "outbound", "depth": 1
    }}))
    .unwrap();
    match execute_search(&mut conn, &request) {
        Err(SearchError::TraverseRootNotFound { reference }) => {
            assert_eq!(reference, "ACME-S-9999");
        }
        other => panic!("unknown root must be TraverseRootNotFound, got {other:?}"),
    }

    // ==========================================================================
    // ALL composition examples from S-0005's Unified Search section
    // ==========================================================================
    // 1. Full-text search
    let (results, _) = run(&mut conn, json!({"q": "authentication"}));
    assert_eq!(all_ids(&results), HashSet::from([s1.id, t1.id]));

    // 2. Structured filter: all critical bugs
    let (results, _) = run(
        &mut conn,
        json!({"filter": {
            "entity_type": ["task"], "task_type": ["bug"],
            "metadata": {"priority": "critical"}
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id]));

    // 3. Graph: all tasks under strategy s1
    let (results, _) = run(
        &mut conn,
        json!({
            "traverse": {"from": {"short_code": s1.short_code},
                          "relationships": ["parent"], "direction": "outbound", "depth": 5},
            "filter": {"entity_type": ["task"]}
        }),
    );
    assert_eq!(all_ids(&results), HashSet::from([t1.id, t2.id, t3.id]));

    // 4. Combined: search "auth" in tasks descended from s1
    // (q + filter + traverse — the full composition)
    let (results, _) = run(
        &mut conn,
        json!({
            "q": "auth",
            "traverse": {"from": {"short_code": s1.short_code},
                          "relationships": ["parent"], "direction": "outbound", "depth": 5},
            "filter": {"entity_type": ["task"]}
        }),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([t1.id]),
        "only t1 both contains 'auth' and descends from s1 as a task"
    );

    // 5. Everything blocking initiative i1
    let (results, _) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": i1.short_code},
            "relationships": ["blocks"], "direction": "inbound", "depth": 1
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t3.id]));

    // ==========================================================================
    // Soft-deleted excluded by default; include_deleted honored
    // ==========================================================================
    // Filter path.
    let (results, _) = run(
        &mut conn,
        json!({"filter": {"entity_type": ["task"], "board_id": delivery_board}}),
    );
    assert_eq!(task_ids(&results), HashSet::from([t1.id, t2.id, t3.id]));
    let (results, _) = run(
        &mut conn,
        json!({"filter": {
            "entity_type": ["task"], "board_id": delivery_board, "include_deleted": true
        }}),
    );
    assert_eq!(
        task_ids(&results),
        HashSet::from([t1.id, t2.id, t3.id, t4.id])
    );
    assert!(
        results
            .tasks
            .iter()
            .any(|t| t.id == t4.id && t.deleted_at.is_some()),
        "hydrated soft-deleted rows carry their deleted_at"
    );

    // Traverse path (type resolution must not silently drop deleted ids).
    let (results, _) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": i2.short_code},
            "relationships": ["parent"], "direction": "outbound", "depth": 1
        }}),
    );
    assert_eq!(all_ids(&results), HashSet::from([t3.id]));
    let (results, _) = run(
        &mut conn,
        json!({
            "traverse": {"from": {"short_code": i2.short_code},
                          "relationships": ["parent"], "direction": "outbound", "depth": 1},
            "filter": {"include_deleted": true}
        }),
    );
    assert_eq!(all_ids(&results), HashSet::from([t3.id, t4.id]));

    // ==========================================================================
    // KAIROS-T-0157 / KAIROS-A-0020: the flag reaches `q` and the traverse
    // root, not just the filter
    // ==========================================================================
    // "onboarding" appears only in t4, the soft-deleted task.
    let (results, _) = run(&mut conn, json!({"q": "onboarding"}));
    assert!(
        all_ids(&results).is_empty(),
        "a text search hides archived work by default (ADR-20 rule 3)"
    );
    let (results, _) = run(
        &mut conn,
        json!({"q": "onboarding", "filter": {"include_deleted": true}}),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([t4.id]),
        "asked for, archived work is findable by full text — the candidate \
         sets are intersected, so a live-only `q` set used to drop the \
         archived ids BEFORE include_deleted was ever consulted"
    );
    assert!(
        results.tasks.iter().all(|t| t.deleted_at.is_some()),
        "and the hit carries its deleted_at, so nothing can mistake it for live"
    );

    // `include_deleted` alone is a whole request, not an empty one: the
    // core validator used to reject it as non-constraining, which made
    // "show me the archived work" unaskable.
    let (results, _) = run(&mut conn, json!({"filter": {"include_deleted": true}}));
    assert!(
        all_ids(&results).contains(&t4.id),
        "\"show me everything, archived included\" reaches the archived row"
    );

    // Traverse FROM an archived root: 404 by default, its subgraph when
    // asked. Archive i2 (which has descendants) for the probe, then put it
    // back so the rest of the test sees the fixture it expects. The stamp
    // is written directly rather than through `soft_delete_item`, which
    // cascades — archiving the descendants would prove nothing about
    // whether the ROOT lookup honours the flag.
    let archive_i2 = |conn: &mut PgConnection, at: Option<&str>| {
        sql_query(format!(
            "UPDATE initiatives SET deleted_at = {} WHERE id = $1",
            at.unwrap_or("NULL")
        ))
        .bind::<diesel::sql_types::Uuid, _>(i2.id)
        .execute(conn)
        .expect("stamping i2's deleted_at");
    };
    archive_i2(&mut conn, Some("now()"));
    let root = json!({
        "from": {"short_code": i2.short_code},
        "relationships": ["parent"], "direction": "outbound", "depth": 1
    });
    let live_only: SearchRequest =
        serde_json::from_value(json!({"traverse": root.clone()})).unwrap();
    assert!(
        matches!(
            execute_search(&mut conn, &live_only),
            Err(SearchError::TraverseRootNotFound { .. })
        ),
        "an archived root stays invisible without the flag"
    );
    let (results, _) = run(
        &mut conn,
        json!({"traverse": root, "filter": {"include_deleted": true}}),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([t3.id, t4.id]),
        "\"what hung off this once?\" is exactly the audit question, and it \
         answers instead of 404ing"
    );
    archive_i2(&mut conn, None);

    // ==========================================================================
    // Sort and pagination: total counted before the page is cut
    // ==========================================================================
    let (results, _) = run(
        &mut conn,
        json!({
            "filter": {"entity_type": ["task"], "board_id": delivery_board},
            "sort": {"field": "title", "order": "asc"},
            "limit": 2, "offset": 1
        }),
    );
    assert_eq!(results.total, 3, "total is pre-pagination");
    assert_eq!(results.limit, 2);
    assert_eq!(results.offset, 1);
    let titles: Vec<&str> = results.tasks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Implement login endpoint", "Refactor session store"],
        "title asc, offset 1: Fix logout redirect is skipped"
    );

    // ==========================================================================
    // Relevance (KAIROS-T-0186)
    // ==========================================================================

    // A title hit outranks a body hit. Both rows match "zephyr" exactly once,
    // so the only thing that can separate them is the `A`/`B` weighting the
    // 2026-09-23-000004 migration added — without it these two tie and this
    // assertion fails, which is the point of writing it this way.
    let (results, _) = run(&mut conn, json!({"q": "zephyr"}));
    let ranked: Vec<Uuid> = results.tasks.iter().map(|t| t.id).collect();
    assert_eq!(
        ranked,
        vec![rank_title.id, rank_body.id],
        "a title match ranks above a body match"
    );

    // Relevance is the DEFAULT when q is present: the request above named no
    // sort, and `rank_title` is the newer row, so a `created_at desc` default
    // would have produced the same order by accident. Pin it against a request
    // that asks for chronology explicitly and gets a different answer.
    let (chronological, _) = run(
        &mut conn,
        json!({"q": "zephyr", "sort": {"field": "created_at", "order": "asc"}}),
    );
    let by_age: Vec<Uuid> = chronological.tasks.iter().map(|t| t.id).collect();
    assert_eq!(
        by_age,
        vec![rank_title.id, rank_body.id],
        "created_at asc puts the older row first"
    );
    let (reversed, _) = run(
        &mut conn,
        json!({"q": "zephyr", "sort": {"field": "created_at", "order": "desc"}}),
    );
    assert_eq!(
        reversed.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![rank_body.id, rank_title.id],
        "an explicit sort is still honoured, and disagrees with relevance here"
    );

    // Equal scores must still produce one stable order, or pagination tears.
    // Identical text scores identically, so the `short_code` tie-break is the
    // only thing deciding, and it must decide the same way every time.
    let (tied, _) = run(&mut conn, json!({"q": "tessellate"}));
    let tied_ids: Vec<Uuid> = tied.tasks.iter().map(|t| t.id).collect();
    assert_eq!(tied_ids.len(), 2, "both identical rows come back");
    let mut expected = [
        (tie_first.short_code.clone(), tie_first.id),
        (tie_second.short_code.clone(), tie_second.id),
    ];
    expected.sort();
    assert_eq!(
        tied_ids,
        expected.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
        "tied scores fall back to short_code ascending"
    );
    let (tied_again, _) = run(&mut conn, json!({"q": "tessellate"}));
    assert_eq!(
        tied_again.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
        tied_ids,
        "and the same order on a second run"
    );
    // Paginating across the tie must not repeat or drop either row.
    let (page_one, _) = run(&mut conn, json!({"q": "tessellate", "limit": 1}));
    let (page_two, _) = run(
        &mut conn,
        json!({"q": "tessellate", "limit": 1, "offset": 1}),
    );
    assert_eq!(page_one.total, 2, "total is pre-pagination");
    assert_eq!(
        vec![page_one.tasks[0].id, page_two.tasks[0].id],
        tied_ids,
        "the two pages reconstruct the whole ordered set exactly once"
    );

    // Relevance to nothing is a 400, not a quiet fall back to chronology.
    let no_query: SearchRequest = serde_json::from_value(json!({
        "filter": {"board_id": relevance_board},
        "sort": {"field": "relevance", "order": "desc"}
    }))
    .unwrap();
    assert!(matches!(
        execute_search(&mut conn, &no_query),
        Err(SearchError::Invalid(
            SearchValidationError::RelevanceWithoutQuery
        ))
    ));

    // Validation errors surface typed through execute_search.
    let over_limit: SearchRequest =
        serde_json::from_value(json!({"q": "auth", "limit": 101})).unwrap();
    assert!(matches!(
        execute_search(&mut conn, &over_limit),
        Err(SearchError::Invalid(
            SearchValidationError::LimitOutOfRange { limit: 101, .. }
        ))
    ));

    // ==========================================================================
    // Depth cap: a 12-deep blocks chain truncates at the traversal depth
    // ==========================================================================
    let chain: Vec<Uuid> = (0..12)
        .map(|n| {
            make_task(
                &mut conn,
                scratch_board,
                &format!("Chain link {n:02}"),
                "chain",
                TaskType::Task,
                None,
            )
            .id
        })
        .collect();
    for pair in chain.windows(2) {
        link(&mut conn, pair[0], pair[1], RelationshipType::Blocks);
    }
    let (results, _) = run(
        &mut conn,
        json!({
            "traverse": {"from": {"id": chain[0]},
                          "relationships": ["blocks"], "direction": "outbound", "depth": 10},
            "limit": 100
        }),
    );
    let reached = task_ids(&results);
    assert_eq!(reached.len(), 10, "depth 10 reaches exactly 10 links");
    assert!(reached.contains(&chain[10]), "the 10th hop is included");
    assert!(
        !reached.contains(&chain[11]),
        "the 11th hop is beyond the traversal depth"
    );

    // Over-cap depth is a typed validation error, not a bigger walk.
    let too_deep: SearchRequest = serde_json::from_value(json!({"traverse": {
        "from": {"id": chain[0]},
        "relationships": ["blocks"], "direction": "outbound", "depth": 11
    }}))
    .unwrap();
    assert!(matches!(
        execute_search(&mut conn, &too_deep),
        Err(SearchError::Invalid(
            SearchValidationError::TraverseDepthOutOfRange { depth: 11, cap: 10 }
        ))
    ));

    // ==========================================================================
    // Hydration bound: ≤5 queries regardless of result size
    // ==========================================================================
    // A traversal touching four entity types (initiative, task, document,
    // adr) hydrates with exactly one query per type present.
    let (results, stats) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"short_code": s1.short_code},
            "relationships": ["parent", "supports"], "direction": "outbound", "depth": 5
        }}),
    );
    assert_eq!(
        all_ids(&results),
        HashSet::from([i1.id, i2.id, i_bucket.id, t1.id, t2.id, t3.id, d1.id, a1.id])
    );
    assert_eq!(stats.hydration_queries, 4);
    assert!(stats.hydration_queries <= 5);

    // Unfiltered full-text search across every type is still ≤5.
    let (_, stats) = run(&mut conn, json!({"q": "the"}));
    assert!(stats.hydration_queries <= 5);

    // ==========================================================================
    // Pathological fan-out: 200 tasks under one initiative
    // ==========================================================================
    let i_fan = make_initiative(
        &mut conn,
        "Fan-out initiative",
        "Holds the pathological fan-out",
        None,
    );
    for n in 0..200 {
        let task = make_task(
            &mut conn,
            scratch_board,
            &format!("Fan task {n:03}"),
            "fan-out",
            TaskType::Task,
            None,
        );
        link(&mut conn, i_fan.id, task.id, RelationshipType::Parent);
    }

    let started = Instant::now();
    let (results, stats) = run(
        &mut conn,
        json!({"traverse": {
            "from": {"id": i_fan.id},
            "relationships": ["parent"], "direction": "outbound", "depth": 5
        }}),
    );
    let elapsed = started.elapsed();
    assert_eq!(results.total, 200, "the whole fan is counted");
    assert_eq!(
        returned_row_count(&results),
        25,
        "default limit pages the fan"
    );
    assert_eq!(
        stats.hydration_queries, 1,
        "200 results of one type hydrate in ONE query"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "fan-out search must complete within the 5s budget, took {elapsed:?}"
    );
    eprintln!("fan-out (200 tasks, traverse depth 5): {elapsed:?}, stats {stats:?}");

    // The larger of the two searchable graphs still respects the bound and
    // the cap composes with filters (q + traverse + filter over the fan).
    let (results, stats) = run(
        &mut conn,
        json!({
            "q": "fan-out",
            "traverse": {"from": {"id": i_fan.id},
                          "relationships": ["parent"], "direction": "outbound", "depth": 5},
            "filter": {"entity_type": ["task"], "task_type": ["task"]},
            "limit": 100
        }),
    );
    assert_eq!(results.total, 200);
    assert_eq!(results.tasks.len(), 100);
    assert!(stats.hydration_queries <= 5);

    // Keep the search module's public error surface exercised end-to-end:
    // an unconstraining request never reaches the database.
    assert!(matches!(
        search::execute_search(&mut conn, &SearchRequest::default()),
        Err(SearchError::Invalid(SearchValidationError::NoCapability))
    ));
}
