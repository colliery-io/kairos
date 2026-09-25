//! # Standing tenant-isolation proof (KAIROS-A-0001, mandated by KAIROS-A-0012)
//!
//! This is the ADVERSARIAL negative-test suite for the product's hardest
//! promise: **the isolation boundary is the PostgreSQL schema itself**
//! (KAIROS-A-0001) — every tenant lives in `org_{slug}`, connections are
//! pinned to one tenant's `search_path`, and NO row of tenant B may ever be
//! reachable, readable, or writable while acting as tenant A, through ANY
//! service surface. KAIROS-A-0012 mandates that this be *continuously
//! proven, not assumed from design*, so this file MUST stay in the
//! integration tier and run in every `angreal test integration` pass. It is
//! written as an attacker, not a QA engineer: it seeds two tenants with
//! colliding short codes and near-identical content, then tries to breach
//! the wall from every angle and asserts each breach fails.
//!
//! Runs against the real compose Postgres (`angreal services up`); the
//! database is never mocked (KAIROS-A-0012). Each test drops and recreates
//! its OWN uniquely named scratch database (`kairos_isolation_*_test`) on the
//! same server, so the suite is parallel-safe and never touches shared
//! services beyond the one server.
//!
//! Test functions (CI failure attribution is instant from the name):
//! - [`cross_read_visibility_sweep`] — acting as tenant A, prove ZERO tenant
//!   B rows are reachable through every read surface: `entity_directory`
//!   get-by-short-code, item_history, item_metadata, `relationships_for`,
//!   capability checks, and `execute_search` (`q` / `filter` / `traverse`),
//!   plus retention: sweeping A never touches B's history.
//! - [`cross_write_battery`] — acting as tenant A, aim every write service
//!   at tenant B's UUIDs; each fails typed (NotFound-class) or affects zero
//!   rows, verified by full-table `md5` checksums of B taken before and
//!   after the attack battery.
//! - [`pool_reuse_stress`] — one small shared pool, concurrent tasks
//!   alternating tenants in barrier-synchronized rounds; every result must
//!   belong to the pinned tenant (catches `search_path` leakage under pool
//!   reuse).
//! - [`short_code_collision`] — identical short codes exist in both tenants
//!   by construction; each resolves only within its own tenant, never cross.

// Sync `diesel::prelude` is used across the (majority) synchronous tests; the
// one async test confines diesel-async's `RunQueryDsl` to its own module so
// the two `RunQueryDsl`s never collide (same discipline as
// `tests/models_roundtrip.rs`).
use chrono::{TimeZone, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use serde_json::json;
use uuid::Uuid;

use kairos_core::search::SearchRequest;
use kairos_core::short_code::ItemType;
use kairos_db::boards::BoardError;
use kairos_db::graph::GraphError;
use kairos_db::items::{
    self, ContentUpdate, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy, CreateTask,
    ItemError,
};
use kairos_db::models::{
    BoardLevel, NewItemMetadata, NewItemRelationship, NewTask, NewUser, RelationshipType, TaskType,
    User,
};
use kairos_db::retention::sweep_tenant;
use kairos_db::search::{SearchError, SearchResults, execute_search};
use kairos_db::{abac, create_board, graph, provision_tenant, run_public_migrations, schema};

use kairos_core::retention::{RetentionConfig, RetentionMode};

// ===========================================================================
// Scratch-database plumbing + fixtures
// ===========================================================================

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The two tenants every test provisions. `acme` is always the attacker,
/// `zenith` the victim.
const TENANTS: [&str; 2] = ["acme", "zenith"];

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

/// Drop + recreate `db_name`, run public migrations, provision `acme` and
/// `zenith`. Returns the scratch database URL.
fn scratch_database(db_name: &str) -> String {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {db_name}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");

    let scratch_url = with_database(&admin_url, db_name);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    for slug in TENANTS {
        provision_tenant(&mut conn, slug, slug).expect("provisioning tenant");
    }
    scratch_url
}

fn drop_scratch_database(db_name: &str) {
    let mut admin_conn =
        PgConnection::establish(&admin_database_url()).expect("connecting for scratch teardown");
    sql_query(format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}

/// A fresh connection pinned to `org_{slug}` — the exact mechanism the pool
/// uses (`SET search_path TO "org_{slug}", public`).
fn tenant_conn(url: &str, slug: &str) -> PgConnection {
    let mut conn = PgConnection::establish(url).expect("connecting to scratch database");
    pin(&mut conn, slug);
    conn
}

/// (Re)pin an existing connection to `org_{slug}`.
fn pin(conn: &mut PgConnection, slug: &str) {
    sql_query(format!("SET search_path TO \"org_{slug}\", public"))
        .execute(conn)
        .expect("pinning search_path");
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

fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::boards::table
        .filter(schema::boards::slug.eq(slug))
        .select(schema::boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

fn first_column(conn: &mut PgConnection, board: Uuid) -> Uuid {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .order(schema::board_columns::position.asc())
        .select(schema::board_columns::id)
        .first(conn)
        .expect("board has columns")
}

fn metadata_def(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::metadata_definitions::table
        .filter(schema::metadata_definitions::slug.eq(slug))
        .select(schema::metadata_definitions::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("metadata definition {slug:?} not found: {e}"))
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// `count(*)` of `sql_fragment` (a full `SELECT ... FROM ... WHERE id = $1`
/// shaped query, but we wrap it) — used for direct table probes.
fn count_where_id(conn: &mut PgConnection, table: &str, column: &str, id: Uuid) -> i64 {
    let row: CountRow = sql_query(format!(
        "SELECT count(*) AS n FROM {table} WHERE {column} = $1"
    ))
    .bind::<diesel::sql_types::Uuid, _>(id)
    .get_result(conn)
    .unwrap_or_else(|e| panic!("counting {table}.{column}: {e}"));
    row.n
}

/// The id `entity_directory` resolves for `short_code` in the CURRENT tenant
/// schema, or `None` — this IS the "get item by short code" read surface.
/// Live-only, stated rather than inherited: since KAIROS-T-0156 the view
/// reports `deleted_at` instead of filtering on it, and this test is about
/// TENANT isolation, so it asks the same default question it always did.
fn directory_id_by_code(conn: &mut PgConnection, short_code: &str) -> Option<Uuid> {
    #[derive(QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
    }
    let row: Option<IdRow> =
        sql_query("SELECT id FROM entity_directory WHERE short_code = $1 AND deleted_at IS NULL")
            .bind::<Text, _>(short_code)
            .get_result(conn)
            .optional()
            .expect("querying entity_directory");
    row.map(|r| r.id)
}

/// Every seeded item in one tenant, with the short codes services assigned
/// and the marker word embedded in every title/content.
struct Seed {
    marker: String,
    strategy: Uuid,
    initiative: Uuid,
    task: Uuid,
    document: Uuid,
    adr: Uuid,
    strategy_board: Uuid,
    delivery_board: Uuid,
    strategy_code: String,
    task_code: String,
    /// A directly-inserted task carrying the SAME short code in both tenants.
    collide_task: Uuid,
}

/// The short code deliberately shared, byte-for-byte, across both tenants.
const COLLIDE_CODE: &str = "SHARED-T-0001";

/// Seed one tenant (connection already pinned to it). Uses the real write-path
/// services so history/metadata/relationships/capabilities all exist, giving
/// the attacker a full surface to probe. `marker = "secret{slug}"` is a single
/// tsvector lexeme embedded everywhere so full-text search can be aimed
/// precisely at one tenant's content.
fn seed(conn: &mut PgConnection, slug: &str, user: Uuid) -> Seed {
    let marker = format!("secret{slug}");
    let strategy_board = board_id_by_slug(conn, "strategy");
    let initiative_board = board_id_by_slug(conn, "initiatives");
    let adr_board = board_id_by_slug(conn, "adrs");
    let delivery_board = create_board(
        conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("creating delivery board")
    .id;

    let strategy = items::create_strategy(
        conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: &format!("{marker} strategy"),
            content: &format!("{marker} strategy confidential body"),
            hypothesis: Some(&format!("{marker} hypothesis")),
        },
        user,
    )
    .expect("creating strategy");

    let initiative = items::create_initiative(
        conn,
        CreateInitiative {
            board_id: initiative_board,
            column_id: None,
            title: &format!("{marker} initiative"),
            content: &format!("{marker} initiative body"),
            complexity: None,
            bucket_type: None,
        },
        user,
    )
    .expect("creating initiative");

    let task = items::create_task(
        conn,
        CreateTask {
            board_id: delivery_board,
            column_id: None,
            title: &format!("{marker} task"),
            content: &format!("{marker} task body"),
            task_type: TaskType::Task,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: None,
            repository_id: None,
        },
        user,
    )
    .expect("creating task");

    let document = items::create_document(
        conn,
        CreateDocument {
            title: &format!("{marker} document"),
            content: Some(&format!("{marker} document body")),
            template_id: None,
        },
        user,
    )
    .expect("creating document");

    let adr = items::create_adr(
        conn,
        CreateAdr {
            board_id: Some(adr_board),
            column_id: None,
            title: &format!("{marker} adr"),
            content: &format!("{marker} adr body"),
            decision_maker: None,
            decision_date: None,
        },
        user,
    )
    .expect("creating adr");

    // Relationship graph so `relationships_for` / traverse have edges to hide.
    diesel::insert_into(schema::item_relationships::table)
        .values(&[
            NewItemRelationship {
                source_id: strategy.id,
                target_id: initiative.id,
                relationship: RelationshipType::Parent,
            },
            NewItemRelationship {
                source_id: initiative.id,
                target_id: task.id,
                relationship: RelationshipType::Parent,
            },
            NewItemRelationship {
                source_id: initiative.id,
                target_id: document.id,
                relationship: RelationshipType::Parent,
            },
        ])
        .execute(conn)
        .expect("inserting parent edges");

    // Metadata on the task (provision seeds the `priority` definition).
    let priority = metadata_def(conn, "priority");
    diesel::insert_into(schema::item_metadata::table)
        .values(NewItemMetadata {
            item_id: task.id,
            metadata_definition_id: priority,
            value: "critical".into(),
        })
        .execute(conn)
        .expect("inserting item metadata");

    // A capability grant on this tenant's strategy board — a grant in B must
    // convey nothing in A.
    abac::grant_capability(conn, strategy_board, user, "manage:content", user)
        .expect("granting capability");

    // A directly-inserted task carrying the SHARED short code (identical
    // across tenants) — the collision surface.
    let collide_column = first_column(conn, delivery_board);
    let collide_task = diesel::insert_into(schema::tasks::table)
        .values(NewTask {
            short_code: COLLIDE_CODE.into(),
            title: format!("{marker} collide"),
            content: format!("{marker} collide body"),
            board_id: delivery_board,
            column_id: collide_column,
            task_type: TaskType::Task,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: None,
            repository_id: None,
            created_by: user,
            updated_by: user,
        })
        .returning(schema::tasks::id)
        .get_result::<Uuid>(conn)
        .expect("inserting collision task");

    Seed {
        marker,
        strategy: strategy.id,
        initiative: initiative.id,
        task: task.id,
        document: document.id,
        adr: adr.id,
        strategy_board,
        delivery_board,
        strategy_code: strategy.short_code,
        task_code: task.short_code,
        collide_task,
    }
}

// ===========================================================================
// Search helpers
// ===========================================================================

fn run_search(conn: &mut PgConnection, request: serde_json::Value) -> SearchResults {
    let request: SearchRequest = serde_json::from_value(request.clone())
        .unwrap_or_else(|e| panic!("request {request} must parse: {e}"));
    execute_search(conn, &request).unwrap_or_else(|e| panic!("search {request:?} failed: {e}"))
}

fn all_result_ids(results: &SearchResults) -> Vec<Uuid> {
    let mut ids = Vec::new();
    ids.extend(results.strategies.iter().map(|r| r.id));
    ids.extend(results.initiatives.iter().map(|r| r.id));
    ids.extend(results.tasks.iter().map(|r| r.id));
    ids.extend(results.documents.iter().map(|r| r.id));
    ids.extend(results.adrs.iter().map(|r| r.id));
    ids
}

fn result_count(results: &SearchResults) -> usize {
    all_result_ids(results).len()
}

// ===========================================================================
// Full-table checksum (cross-write proof)
// ===========================================================================

/// The tables whose contents must be byte-identical before and after an
/// attack battery. `t::text` casts the whole row (every column, timestamps
/// included) so ANY mutation — including an errant `updated_at` bump — flips
/// the fingerprint.
const CHECKSUM_TABLES: [&str; 10] = [
    "strategies",
    "initiatives",
    "tasks",
    "documents",
    "adrs",
    "item_relationships",
    "item_metadata",
    "item_history",
    "activity_log",
    "board_member_capabilities",
];

/// `count:md5` fingerprint of one table in the CURRENT tenant schema.
fn table_fingerprint(conn: &mut PgConnection, table: &str) -> String {
    #[derive(QueryableByName)]
    struct Fp {
        #[diesel(sql_type = Text)]
        fp: String,
    }
    let row: Fp = sql_query(format!(
        "SELECT count(*)::text || ':' || \
                coalesce(md5(string_agg(r, ',' ORDER BY r)), '') AS fp \
         FROM (SELECT t::text AS r FROM {table} t) sub"
    ))
    .get_result(conn)
    .unwrap_or_else(|e| panic!("fingerprinting {table}: {e}"));
    row.fp
}

/// Fingerprint every [`CHECKSUM_TABLES`] table (connection pinned to the
/// tenant being fingerprinted).
fn fingerprint_all(conn: &mut PgConnection) -> Vec<(String, String)> {
    CHECKSUM_TABLES
        .iter()
        .map(|t| (t.to_string(), table_fingerprint(conn, t)))
        .collect()
}

// ===========================================================================
// Test: cross-read visibility sweep
// ===========================================================================

#[test]
fn cross_read_visibility_sweep() {
    const DB: &str = "kairos_isolation_read_test";
    let url = scratch_database(DB);

    // A single shared user in the public schema (users are cross-tenant per
    // A-0001); both tenants reference it.
    let mut pconn = PgConnection::establish(&url).expect("connecting");
    let user = insert_user(&mut pconn, "dex|mallory", "mallory@example.test", "Mallory");
    drop(pconn);

    let mut acme = tenant_conn(&url, "acme");
    let mut zenith = tenant_conn(&url, "zenith");
    let a = seed(&mut acme, "acme", user);
    let z = seed(&mut zenith, "zenith", user);
    drop(zenith); // the victim's own connection is not needed past seeding

    // Everything below acts through ACME's pinned connection against ZENITH's
    // ids/codes. Nothing of zenith may surface.

    // -- get-by-short-code (entity_directory) --------------------------------
    assert_eq!(
        directory_id_by_code(&mut acme, &z.strategy_code),
        None,
        "zenith's strategy short code must not resolve from acme"
    );
    assert_eq!(
        directory_id_by_code(&mut acme, &z.task_code),
        None,
        "zenith's task short code must not resolve from acme"
    );
    assert_eq!(
        directory_id_by_code(&mut acme, &a.strategy_code),
        Some(a.strategy),
        "acme resolves its OWN short code (positive control)"
    );

    // -- item_history reads --------------------------------------------------
    for zid in [z.strategy, z.initiative, z.task, z.document, z.adr] {
        assert_eq!(
            count_where_id(&mut acme, "item_history", "item_id", zid),
            0,
            "zenith item_history row {zid} visible from acme"
        );
    }
    assert!(
        count_where_id(&mut acme, "item_history", "item_id", a.strategy) >= 1,
        "acme sees its own history (positive control)"
    );

    // -- item_metadata reads -------------------------------------------------
    assert_eq!(
        count_where_id(&mut acme, "item_metadata", "item_id", z.task),
        0,
        "zenith item_metadata visible from acme"
    );
    assert_eq!(
        count_where_id(&mut acme, "item_metadata", "item_id", a.task),
        1,
        "acme sees its own metadata (positive control)"
    );

    // -- relationship queries ------------------------------------------------
    for zid in [z.strategy, z.initiative, z.task] {
        let rels = graph::relationships_for(&mut acme, zid).expect("relationships_for");
        assert!(
            rels.outgoing.is_empty() && rels.incoming.is_empty(),
            "zenith edges for {zid} visible from acme: {rels:?}"
        );
    }
    let own = graph::relationships_for(&mut acme, a.initiative).expect("relationships_for");
    assert!(
        !own.outgoing.is_empty() && !own.incoming.is_empty(),
        "acme sees its own edges (positive control)"
    );

    // -- capability checks ---------------------------------------------------
    assert!(
        !abac::check_capability(&mut acme, z.strategy_board, user, "manage:content")
            .expect("check_capability"),
        "a grant on zenith's board must convey nothing in acme"
    );
    assert!(
        abac::check_capability(&mut acme, a.strategy_board, user, "manage:content")
            .expect("check_capability"),
        "acme's own grant is honored (positive control)"
    );

    // -- search: full-text `q` -----------------------------------------------
    let zenith_hits = run_search(&mut acme, json!({ "q": z.marker }));
    assert_eq!(
        result_count(&zenith_hits),
        0,
        "full-text search for zenith's marker returns nothing from acme"
    );
    let acme_hits = run_search(&mut acme, json!({ "q": a.marker }));
    let acme_hit_ids = all_result_ids(&acme_hits);
    assert!(
        result_count(&acme_hits) >= 5,
        "acme finds its own seeded items (positive control): {acme_hits:?}"
    );
    for zid in [z.strategy, z.initiative, z.task, z.document, z.adr] {
        assert!(
            !acme_hit_ids.contains(&zid),
            "zenith id {zid} leaked into acme's own search results"
        );
    }

    // -- search: `filter` (aim a board filter at zenith's board id) ----------
    let by_zenith_board = run_search(
        &mut acme,
        json!({ "filter": { "board_id": z.strategy_board } }),
    );
    assert_eq!(
        result_count(&by_zenith_board),
        0,
        "filtering by zenith's board id returns nothing from acme"
    );

    // -- search: `traverse` from a zenith root (id and short_code) -----------
    for request in [
        json!({ "traverse": { "from": { "id": z.strategy },
                              "relationships": ["parent"], "direction": "outbound", "depth": 5 } }),
        json!({ "traverse": { "from": { "short_code": z.strategy_code },
                              "relationships": ["parent"], "direction": "outbound", "depth": 5 } }),
    ] {
        let parsed: SearchRequest = serde_json::from_value(request.clone()).unwrap();
        match execute_search(&mut acme, &parsed) {
            Err(SearchError::TraverseRootNotFound { .. }) => {}
            other => panic!(
                "traverse from zenith root {request} must be TraverseRootNotFound, got {other:?}"
            ),
        }
    }
    // Positive control: traversing acme's own root yields acme descendants.
    let own_traverse = run_search(
        &mut acme,
        json!({ "traverse": { "from": { "id": a.strategy },
                              "relationships": ["parent"], "direction": "outbound", "depth": 5 } }),
    );
    assert!(
        result_count(&own_traverse) >= 3,
        "acme traverses its own subtree (positive control): {own_traverse:?}"
    );

    // -- retention: sweeping acme never touches zenith's history -------------
    // Backdate acme's strategy history and add versions so a discard sweep has
    // something to prune; zenith's history must be byte-identical before/after.
    let mut zconn = tenant_conn(&url, "zenith");
    let zenith_history_before = table_fingerprint(&mut zconn, "item_history");
    drop(zconn);

    // Give acme's strategy several old same-month versions.
    for v in 0..8i32 {
        items::update_item_content(
            &mut acme,
            ItemType::Strategy,
            a.strategy,
            ContentUpdate {
                new_title: None,
                new_content: &format!("{} rev {v}", a.marker),
                expected_version: v + 1,
            },
            user,
        )
        .expect("versioning acme strategy");
    }
    sql_query("UPDATE item_history SET edited_at = TIMESTAMPTZ '2020-01-05 00:00:00Z' + (version || ' days')::interval WHERE item_id = $1")
        .bind::<diesel::sql_types::Uuid, _>(a.strategy)
        .execute(&mut acme)
        .expect("backdating acme history");

    let config = RetentionConfig {
        history_hot_days: 1,
        history_keep_latest: 2,
        activity_retention_days: 365,
        archive_target: None,
        mode: RetentionMode::Discard,
    };
    let now = Utc.with_ymd_and_hms(2020, 2, 1, 0, 0, 0).single().unwrap();
    let report = sweep_tenant(&mut acme, "acme", &config, now).expect("sweeping acme");
    assert!(
        report.item_history.rows_pruned >= 1,
        "the acme sweep actually pruned acme history: {report:?}"
    );

    // sweep_tenant resets search_path to DEFAULT; re-pin zenith on a fresh
    // connection and confirm its history is untouched.
    let mut zconn = tenant_conn(&url, "zenith");
    let zenith_history_after = table_fingerprint(&mut zconn, "item_history");
    assert_eq!(
        zenith_history_before, zenith_history_after,
        "sweeping acme's retention must NOT touch zenith's item_history"
    );

    drop(acme);
    drop(zconn);
    drop_scratch_database(DB);
}

// ===========================================================================
// Test: cross-write battery
// ===========================================================================

#[test]
fn cross_write_battery() {
    const DB: &str = "kairos_isolation_write_test";
    let url = scratch_database(DB);

    let mut pconn = PgConnection::establish(&url).expect("connecting");
    let user = insert_user(&mut pconn, "dex|eve", "eve@example.test", "Eve");
    drop(pconn);

    let mut acme = tenant_conn(&url, "acme");
    let mut zenith = tenant_conn(&url, "zenith");
    let a = seed(&mut acme, "acme", user);
    let z = seed(&mut zenith, "zenith", user);

    // Snapshot BOTH tenants: zenith must be provably unchanged; acme must not
    // accidentally mutate either (all attacks target ids absent from acme).
    let zenith_before = fingerprint_all(&mut zenith);
    let acme_before = fingerprint_all(&mut acme);

    // -- attack battery, all through ACME's connection at ZENITH's ids -------
    let mut attacks = 0usize;

    // 1-2. content updates on zenith items → ItemNotFound
    for (ty, id) in [
        (ItemType::Strategy, z.strategy),
        (ItemType::Task, z.task),
        (ItemType::Document, z.document),
    ] {
        let r = items::update_item_content(
            &mut acme,
            ty,
            id,
            ContentUpdate {
                new_title: Some("pwned"),
                new_content: "pwned",
                expected_version: 1,
            },
            user,
        );
        assert!(
            matches!(r, Err(ItemError::ItemNotFound { .. })),
            "update_item_content on zenith {id} must be ItemNotFound, got {r:?}"
        );
        attacks += 1;
    }

    // 3. transition a zenith task via an acme column → BoardError::ItemNotFound
    let acme_delivery_col = first_column(&mut acme, a.delivery_board);
    let r = kairos_db::transition_task(&mut acme, z.task, acme_delivery_col, user);
    assert!(
        matches!(r, Err(BoardError::ItemNotFound { .. })),
        "transitioning zenith task must be ItemNotFound, got {r:?}"
    );
    attacks += 1;

    // 4. link two zenith items → GraphError::ItemNotFound
    let r = graph::link_items(
        &mut acme,
        z.strategy,
        z.initiative,
        RelationshipType::Parent,
        user,
    );
    assert!(
        matches!(r, Err(GraphError::ItemNotFound(_))),
        "linking zenith endpoints must be ItemNotFound, got {r:?}"
    );
    attacks += 1;

    // 5. grant a capability on zenith's board → must fail (FK to acme.boards
    //    has no such board), writing nothing.
    let r = abac::grant_capability(&mut acme, z.strategy_board, user, "pwn", user);
    assert!(
        r.is_err(),
        "granting on zenith's board id from acme must fail, got {r:?}"
    );
    attacks += 1;

    // 6. soft-delete a zenith task → ItemNotFound
    let r = items::soft_delete_item(&mut acme, ItemType::Task, z.task, user);
    assert!(
        matches!(r, Err(ItemError::ItemNotFound { .. })),
        "soft-deleting zenith task must be ItemNotFound, got {r:?}"
    );
    attacks += 1;

    // 7. soft-delete the zenith strategy (cascade root) → ItemNotFound
    let r = items::soft_delete_item(&mut acme, ItemType::Strategy, z.strategy, user);
    assert!(
        matches!(r, Err(ItemError::ItemNotFound { .. })),
        "soft-deleting zenith strategy must be ItemNotFound, got {r:?}"
    );
    attacks += 1;

    // -- prove zenith is byte-for-byte unchanged -----------------------------
    // Fresh connection: acme's may carry an aborted-transaction search_path.
    let mut zenith_after_conn = tenant_conn(&url, "zenith");
    let zenith_after = fingerprint_all(&mut zenith_after_conn);
    assert_eq!(
        zenith_before, zenith_after,
        "zenith state changed under a {attacks}-write attack battery from acme"
    );

    // acme unchanged too (attacks named zenith ids, absent from acme).
    let acme_after = fingerprint_all(&mut acme);
    assert_eq!(
        acme_before, acme_after,
        "acme mutated itself while attacking zenith's ids"
    );

    assert!(attacks >= 8, "expected a full battery, ran {attacks}");
    eprintln!("cross_write_battery: {attacks} cross-tenant write attempts, all repelled");

    drop(acme);
    drop(zenith);
    drop(zenith_after_conn);
    drop_scratch_database(DB);
}

// ===========================================================================
// Test: short-code collision
// ===========================================================================

#[test]
fn short_code_collision() {
    const DB: &str = "kairos_isolation_collision_test";
    let url = scratch_database(DB);

    let mut pconn = PgConnection::establish(&url).expect("connecting");
    let user = insert_user(&mut pconn, "dex|trudy", "trudy@example.test", "Trudy");
    drop(pconn);

    let mut acme = tenant_conn(&url, "acme");
    let mut zenith = tenant_conn(&url, "zenith");
    let a = seed(&mut acme, "acme", user);
    let z = seed(&mut zenith, "zenith", user);

    // The collision short code exists, byte-identical, in BOTH tenants but as
    // distinct rows (distinct UUIDs).
    assert_ne!(
        a.collide_task, z.collide_task,
        "collision rows must be distinct entities"
    );

    // Each tenant resolves the shared code only to its OWN row, never cross.
    assert_eq!(
        directory_id_by_code(&mut acme, COLLIDE_CODE),
        Some(a.collide_task),
        "acme resolves the shared code to acme's row"
    );
    assert_eq!(
        directory_id_by_code(&mut zenith, COLLIDE_CODE),
        Some(z.collide_task),
        "zenith resolves the shared code to zenith's row"
    );

    // Traversal-root resolution (search) honors the same boundary: a traverse
    // rooted at the shared code resolves within-tenant only.
    for (conn, slug, own) in [
        (&mut acme, "acme", a.collide_task),
        (&mut zenith, "zenith", z.collide_task),
    ] {
        let req: SearchRequest = serde_json::from_value(json!({
            "traverse": { "from": { "short_code": COLLIDE_CODE },
                          "relationships": ["parent"], "direction": "both", "depth": 1 }
        }))
        .unwrap();
        // The collide task has no edges, so results are empty — but resolution
        // must SUCCEED (root found within tenant), not error.
        let res = execute_search(conn, &req)
            .unwrap_or_else(|e| panic!("{slug} traverse of shared code failed: {e}"));
        assert_eq!(
            result_count(&res),
            0,
            "{slug} collide task has no descendants"
        );
        // And the marker on the resolved row belongs to this tenant only.
        let marker: String = schema::tasks::table
            .filter(schema::tasks::id.eq(own))
            .select(schema::tasks::title)
            .first(conn)
            .expect("loading collide task title");
        assert!(
            marker.contains(&format!("secret{slug}")),
            "{slug} collide row must carry its own marker: {marker}"
        );
    }

    drop(acme);
    drop(zenith);
    drop_scratch_database(DB);
}

// ===========================================================================
// Test: pool-reuse stress (async)
// ===========================================================================

/// Barrier-synchronized concurrent rounds over ONE small pool, alternating
/// tenants, asserting every read belongs to the pinned tenant. This is the
/// concurrency proof for the pool's per-checkout `search_path` pinning: a
/// physical connection reused across tenants must never leak the other
/// tenant's rows.
mod pool_stress {
    use diesel::prelude::{ExpressionMethods, QueryDsl};
    use diesel::sql_query;
    use diesel::sql_types::Text;
    use diesel_async::RunQueryDsl;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    use kairos_db::models::NewStrategy;
    use kairos_db::pool::TenantPool;
    use kairos_db::schema as s;

    use super::{TENANTS, first_column, insert_user, scratch_database, tenant_conn};

    const ROUNDS: usize = 10;

    #[derive(diesel::QueryableByName)]
    struct TitleRow {
        #[diesel(sql_type = Text)]
        title: String,
    }

    /// Per-tenant board + first column, resolved once (sync) for the async
    /// inserts.
    #[derive(Clone, Copy)]
    struct TenantFixture {
        board: Uuid,
        column: Uuid,
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pool_reuse_stress() {
        const DB: &str = "kairos_isolation_pool_test";
        let url = scratch_database(DB);

        // Shared public user + per-tenant strategy board/column (sync setup).
        // A tenant-pinned connection can insert into `public.users` (the pool
        // pins `org_{slug}, public` and schema.rs qualifies public tables).
        let mut pconn = tenant_conn(&url, "acme");
        let user = insert_user(&mut pconn, "dex|oscar", "oscar@example.test", "Oscar");
        drop(pconn);

        let mut fixtures = std::collections::HashMap::new();
        for slug in TENANTS {
            let mut conn = tenant_conn(&url, slug);
            let board = super::board_id_by_slug(&mut conn, "strategy");
            let column = first_column(&mut conn, board);
            fixtures.insert(slug.to_string(), TenantFixture { board, column });
        }

        // Deliberately tiny: 2 connections serving 2 tenants concurrently
        // guarantees physical connections are reused across tenants.
        let pool = TenantPool::new(&url, 2).await.expect("building pool");
        let ops = Arc::new(AtomicUsize::new(0));

        for round in 0..ROUNDS {
            // One barrier per round across both tenant tasks: they hit the
            // 2-connection pool at the same instant.
            let barrier = Arc::new(tokio::sync::Barrier::new(TENANTS.len()));
            let mut handles = Vec::new();
            for slug in TENANTS {
                let pool = pool.clone();
                let barrier = Arc::clone(&barrier);
                let ops = Arc::clone(&ops);
                let fixture = fixtures[slug];
                let own_marker = format!("secret{slug}");
                let other_marker = if slug == "acme" {
                    "secretzenith".to_string()
                } else {
                    "secretacme".to_string()
                };
                handles.push(tokio::spawn(async move {
                    let mut conn = pool.tenant(slug).await.expect("tenant checkout");
                    barrier.wait().await;

                    // CREATE: a strategy whose short code is identical across
                    // tenants (collision under concurrency) and whose title
                    // carries this tenant's marker.
                    let code = format!("STRESS-S-{round:04}");
                    let title = format!("{own_marker} stress round {round}");
                    diesel::insert_into(s::strategies::table)
                        .values(NewStrategy {
                            short_code: code.clone(),
                            title: title.clone(),
                            content: format!("{own_marker} stress body {round}"),
                            board_id: fixture.board,
                            column_id: fixture.column,
                            hypothesis: None,
                            created_by: user,
                            updated_by: user,
                        })
                        .execute(&mut *conn)
                        .await
                        .expect("concurrent insert");
                    ops.fetch_add(1, Ordering::Relaxed);

                    // SEARCH: every row the tenant's search surface returns
                    // must carry THIS tenant's marker and never the other's.
                    let hits: Vec<TitleRow> = sql_query(
                        "SELECT title FROM searchable_items \
                         WHERE tsv @@ websearch_to_tsquery('english', 'stress') \
                           AND deleted_at IS NULL",
                    )
                    .load(&mut *conn)
                    .await
                    .expect("concurrent search");
                    assert!(
                        !hits.is_empty(),
                        "round {round} {slug}: own inserts must be searchable"
                    );
                    for h in &hits {
                        assert!(
                            h.title.contains(&own_marker),
                            "round {round} {slug}: search returned a foreign row: {}",
                            h.title
                        );
                        assert!(
                            !h.title.contains(&other_marker),
                            "round {round} {slug}: LEAK — other tenant's marker in results: {}",
                            h.title
                        );
                    }
                    ops.fetch_add(1, Ordering::Relaxed);

                    // READ: fetch the just-created row back by short code and
                    // confirm it is this tenant's.
                    let read_title: String = s::strategies::table
                        .filter(s::strategies::short_code.eq(&code))
                        .select(s::strategies::title)
                        .first(&mut *conn)
                        .await
                        .expect("concurrent read-back");
                    assert_eq!(
                        read_title, title,
                        "round {round} {slug}: read-back returned a foreign row"
                    );
                    ops.fetch_add(1, Ordering::Relaxed);

                    conn.release().await.expect("release");
                }));
            }
            for h in handles {
                h.await.expect("stress task panicked");
            }
        }

        // UFCS: `RunQueryDsl` is in scope and its blanket `load` would shadow
        // the inherent `AtomicUsize::load` via method resolution.
        let total = std::sync::atomic::AtomicUsize::load(&ops, Ordering::Relaxed);
        assert!(
            total >= 50,
            "expected >= 50 pool operations across >= 8 rounds, ran {total} in {ROUNDS} rounds"
        );
        eprintln!("pool_reuse_stress: {total} operations across {ROUNDS} rounds, no leakage");

        drop(pool);
        super::drop_scratch_database(DB);
    }
}
