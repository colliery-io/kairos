//! Integration test for the relationship-graph services (KAIROS-T-0013,
//! semantics per KAIROS-A-0001, layering per KAIROS-A-0009).
//!
//! Runs against the real compose Postgres (`angreal services up`); the
//! database is never mocked (KAIROS-A-0012). For isolation the test drops
//! and recreates a dedicated scratch database (`kairos_graph_test`) on the
//! same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - allowed links across the A-0001 matrix succeed and write
//!   `relationship_add` activity rows in the S-0004 details format
//! - rejected type combinations return the typed rule error carrying the
//!   offending `(relationship, source_type, target_type)`
//! - cycle prevention: A-blocks-B-blocks-A rejected, transitive blocks
//!   cycle rejected, a malformed pre-existing parent chain cannot be
//!   closed into a loop, diamonds are allowed (not cycles)
//! - unknown and soft-deleted endpoints are typed `ItemNotFound`;
//!   self-links are typed `SelfLink`
//! - duplicate edges honor `UNIQUE (source_id, target_id, relationship)`
//!   as typed `AlreadyLinked`; unlink removes the row and logs
//!   `relationship_remove`; a missing edge is typed `NotLinked`
//! - `relationships_for` returns both directions grouped by relationship
//! - EXPLAIN shows both lookup directions are backed by the S-0004 indexes
//!   (`idx_item_relationships_source` / `idx_item_relationships_target`)

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::graph::{self, GraphError, Neighbor};
use kairos_db::items::{
    self, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy, CreateTask,
};
use kairos_db::models::{
    ActivityAction, BoardLevel, NewItemRelationship, NewUser, RelationshipType, TaskType, User,
};
use kairos_db::{create_board, provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_graph_test";

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

/// All `activity_log.details` for relationship actions (`entity_id` is
/// NULL for those per the S-0004 DDL comment), in order.
fn relationship_activity(conn: &mut PgConnection, action: ActivityAction) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq(action.as_str()))
        .filter(schema::activity_log::entity_id.is_null())
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading activity_log")
}

/// How many edges `(source, target, relationship)` exist.
fn edge_count(
    conn: &mut PgConnection,
    source: Uuid,
    target: Uuid,
    relationship: RelationshipType,
) -> i64 {
    schema::item_relationships::table
        .filter(schema::item_relationships::source_id.eq(source))
        .filter(schema::item_relationships::target_id.eq(target))
        .filter(schema::item_relationships::relationship.eq(relationship))
        .count()
        .get_result(conn)
        .expect("counting edges")
}

/// Assert a link attempt is rejected by the type-rule matrix with the
/// offending combination in the typed error.
fn assert_rule_rejected(
    conn: &mut PgConnection,
    source: Uuid,
    target: Uuid,
    relationship: RelationshipType,
    expected_source_type: ItemType,
    expected_target_type: ItemType,
    actor: Uuid,
) {
    let result = graph::link_items(conn, source, target, relationship, actor);
    match result {
        Err(GraphError::Rule(err)) => {
            assert_eq!(err.relationship.as_str(), relationship.as_str());
            assert_eq!(err.source_type, expected_source_type);
            assert_eq!(err.target_type, expected_target_type);
        }
        other => panic!(
            "{relationship} {expected_source_type} -> {expected_target_type} \
             must be a typed rule error, got {other:?}"
        ),
    }
    assert_eq!(
        edge_count(conn, source, target, relationship),
        0,
        "rejected links must not persist"
    );
}

/// One EXPLAIN output line. `QUERY PLAN` contains a space, so this cannot
/// use the derive (field names must be idents); the manual impl reads the
/// column by name.
struct ExplainRow {
    line: String,
}

impl diesel::deserialize::QueryableByName<diesel::pg::Pg> for ExplainRow {
    fn build<'a>(
        row: &impl diesel::row::NamedRow<'a, diesel::pg::Pg>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(ExplainRow {
            line: diesel::row::NamedRow::get::<diesel::sql_types::Text, String>(row, "QUERY PLAN")?,
        })
    }
}

/// The EXPLAIN plan for `sql`, one string. The tables here hold a handful
/// of rows, so the planner would legitimately prefer a sequential scan;
/// `enable_seqscan = off` (session-local) forces it to reveal the
/// index-backed path that takes over at real row counts.
fn explain(conn: &mut PgConnection, sql: &str) -> String {
    let rows: Vec<ExplainRow> = sql_query(format!("EXPLAIN {sql}"))
        .load(conn)
        .unwrap_or_else(|e| panic!("EXPLAIN failed for {sql}: {e}"));
    rows.into_iter()
        .map(|r| r.line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn relationship_graph_service() {
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

    // ---- fixtures: two strategies, two initiatives, five tasks, a document,
    // two ADRs -----------------------------------------------------------------
    let make_strategy = |conn: &mut PgConnection, title: &str| {
        items::create_strategy(
            conn,
            CreateStrategy {
                board_id: strategy_board,
                column_id: None,
                title,
                content: "",
                hypothesis: None,
            },
            alice,
        )
        .expect("creating strategy")
    };
    let s1 = make_strategy(&mut conn, "Strategy One");
    let s2 = make_strategy(&mut conn, "Strategy Two");

    let make_initiative = |conn: &mut PgConnection, title: &str| {
        items::create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: None,
                title,
                content: "",
                complexity: None,
                bucket_type: None,
            },
            alice,
        )
        .expect("creating initiative")
    };
    let i1 = make_initiative(&mut conn, "Initiative One");
    let i2 = make_initiative(&mut conn, "Initiative Two");

    let make_task = |conn: &mut PgConnection, title: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: delivery_board,
                column_id: None,
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: kairos_db::models::enums::WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("creating task")
    };
    let t1 = make_task(&mut conn, "Task One");
    let t2 = make_task(&mut conn, "Task Two");
    let t3 = make_task(&mut conn, "Task Three");
    let t4 = make_task(&mut conn, "Task Four");
    let t5 = make_task(&mut conn, "Task Five");

    let d1 = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Document One",
            content: Some(""),
            template_id: None,
        },
        alice,
    )
    .expect("creating document");

    let make_adr = |conn: &mut PgConnection, title: &str| {
        items::create_adr(
            conn,
            CreateAdr {
                board_id: Some(adr_board),
                column_id: None,
                title,
                content: "",
                decision_maker: None,
                decision_date: None,
            },
            alice,
        )
        .expect("creating adr")
    };
    let a1 = make_adr(&mut conn, "ADR One");
    let a2 = make_adr(&mut conn, "ADR Two");

    // ---- allowed links across the matrix, with activity rows ------------------
    // parent: exactly the workflow hierarchy.
    graph::link_items(&mut conn, s1.id, i1.id, RelationshipType::Parent, alice)
        .expect("parent strategy -> initiative");
    graph::link_items(&mut conn, i1.id, t1.id, RelationshipType::Parent, alice)
        .expect("parent initiative -> task");
    // supports: source is the SUPPORTED workflow item, target the
    // document/ADR (S-0004: "target supports source").
    graph::link_items(&mut conn, i1.id, d1.id, RelationshipType::Supports, alice)
        .expect("supports initiative -> document");
    graph::link_items(&mut conn, s1.id, a1.id, RelationshipType::Supports, alice)
        .expect("supports strategy -> adr");
    // Diamond supports: the same document also supports a second item —
    // allowed (supports is not cycle-checked, and UNIQUE is per-pair).
    graph::link_items(&mut conn, t1.id, d1.id, RelationshipType::Supports, alice)
        .expect("diamond supports task -> document");
    // informs: document/adr -> workflow item (v1 restriction, T-0013).
    graph::link_items(&mut conn, d1.id, s1.id, RelationshipType::Informs, alice)
        .expect("informs document -> strategy");
    graph::link_items(&mut conn, d1.id, i1.id, RelationshipType::Informs, alice)
        .expect("informs document -> initiative");
    graph::link_items(&mut conn, a1.id, t1.id, RelationshipType::Informs, alice)
        .expect("informs adr -> task");
    // supersedes: ADR replacement chain.
    graph::link_items(&mut conn, a2.id, a1.id, RelationshipType::Supersedes, alice)
        .expect("supersedes adr -> adr");
    // blocks: same-type and cross-type workflow dependencies.
    graph::link_items(&mut conn, t1.id, t2.id, RelationshipType::Blocks, alice)
        .expect("blocks task -> task");
    graph::link_items(&mut conn, i1.id, t2.id, RelationshipType::Blocks, alice)
        .expect("blocks initiative -> task (cross-type)");

    let added = relationship_activity(&mut conn, ActivityAction::RelationshipAdd);
    assert_eq!(
        added.len(),
        11,
        "one relationship_add row per link: {added:?}"
    );
    for expected in [
        "relationship:parent:ACME-S-0001->ACME-I-0001",
        "relationship:parent:ACME-I-0001->ACME-T-0001",
        "relationship:supports:ACME-I-0001->ACME-D-0001",
        "relationship:supports:ACME-S-0001->ACME-A-0001",
        "relationship:informs:ACME-D-0001->ACME-S-0001",
        "relationship:supersedes:ACME-A-0002->ACME-A-0001",
        "relationship:blocks:ACME-T-0001->ACME-T-0002",
    ] {
        assert!(
            added.iter().any(|d| d == expected),
            "missing activity row {expected:?} in {added:?}"
        );
    }

    // ---- rejected pairs: typed rule errors, nothing persisted ------------------
    use ItemType::{Adr, Document, Initiative, Strategy, Task};
    // parent never skips a level, runs upward, or touches documents/ADRs.
    assert_rule_rejected(
        &mut conn,
        s2.id,
        t3.id,
        RelationshipType::Parent,
        Strategy,
        Task,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        i2.id,
        s2.id,
        RelationshipType::Parent,
        Initiative,
        Strategy,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        i2.id,
        d1.id,
        RelationshipType::Parent,
        Initiative,
        Document,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        s2.id,
        a2.id,
        RelationshipType::Parent,
        Strategy,
        Adr,
        alice,
    );
    // supports is stored workflow-item -> document/adr, never reversed or
    // between workflow items.
    assert_rule_rejected(
        &mut conn,
        d1.id,
        i2.id,
        RelationshipType::Supports,
        Document,
        Initiative,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        s2.id,
        t3.id,
        RelationshipType::Supports,
        Strategy,
        Task,
        alice,
    );
    // informs runs document/adr -> workflow item only.
    assert_rule_rejected(
        &mut conn,
        s2.id,
        d1.id,
        RelationshipType::Informs,
        Strategy,
        Document,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        d1.id,
        a1.id,
        RelationshipType::Informs,
        Document,
        Adr,
        alice,
    );
    // supersedes is ADR -> ADR only.
    assert_rule_rejected(
        &mut conn,
        a1.id,
        d1.id,
        RelationshipType::Supersedes,
        Adr,
        Document,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        t3.id,
        t4.id,
        RelationshipType::Supersedes,
        Task,
        Task,
        alice,
    );
    // blocks is workflow-items-only, both ends.
    assert_rule_rejected(
        &mut conn,
        d1.id,
        t3.id,
        RelationshipType::Blocks,
        Document,
        Task,
        alice,
    );
    assert_rule_rejected(
        &mut conn,
        t3.id,
        a1.id,
        RelationshipType::Blocks,
        Task,
        Adr,
        alice,
    );

    // ---- cycle prevention ------------------------------------------------------
    // Direct: t1 blocks t2 already; t2 blocks t1 closes the 2-cycle.
    let direct = graph::link_items(&mut conn, t2.id, t1.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(
            &direct,
            Err(GraphError::CycleDetected {
                relationship: RelationshipType::Blocks,
                source_id,
                target_id,
            }) if *source_id == t2.id && *target_id == t1.id
        ),
        "A-blocks-B-blocks-A must be rejected: {direct:?}"
    );
    // Transitive: t1 -> t2 -> t3, then t3 -> t1.
    graph::link_items(&mut conn, t2.id, t3.id, RelationshipType::Blocks, alice)
        .expect("blocks t2 -> t3");
    let transitive = graph::link_items(&mut conn, t3.id, t1.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(&transitive, Err(GraphError::CycleDetected { .. })),
        "transitive blocks cycle must be rejected: {transitive:?}"
    );
    // Blocks diamond: t1 -> t2 -> t4 and t1 -> t3 -> ... two paths to the
    // same node are NOT a cycle and must be allowed.
    graph::link_items(&mut conn, t2.id, t4.id, RelationshipType::Blocks, alice)
        .expect("blocks t2 -> t4");
    graph::link_items(&mut conn, t3.id, t4.id, RelationshipType::Blocks, alice)
        .expect("blocks diamond t3 -> t4 (two paths t1..t4 are not a cycle)");

    // Parent: the type matrix alone makes parent cycles unreachable through
    // the service (the hierarchy only descends), so seed a MALFORMED
    // pre-existing chain directly (i2 -> t4, t4 -> s2 — bypassing the
    // service) and verify the defense-in-depth cycle check still refuses to
    // close the loop with an otherwise-legal strategy -> initiative link.
    diesel::insert_into(schema::item_relationships::table)
        .values(&[
            NewItemRelationship {
                source_id: i2.id,
                target_id: t4.id,
                relationship: RelationshipType::Parent,
            },
            NewItemRelationship {
                source_id: t4.id,
                target_id: s2.id,
                relationship: RelationshipType::Parent,
            },
        ])
        .execute(&mut conn)
        .expect("seeding malformed parent chain");
    let parent_cycle = graph::link_items(&mut conn, s2.id, i2.id, RelationshipType::Parent, alice);
    assert!(
        matches!(&parent_cycle, Err(GraphError::CycleDetected { .. })),
        "transitive parent loop must be rejected: {parent_cycle:?}"
    );

    // ---- not-found, soft-deleted, self-link ------------------------------------
    let ghost = Uuid::new_v4();
    let unknown = graph::link_items(&mut conn, ghost, t1.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(&unknown, Err(GraphError::ItemNotFound(id)) if *id == ghost),
        "unknown source is typed ItemNotFound: {unknown:?}"
    );
    let unknown_target =
        graph::link_items(&mut conn, t1.id, ghost, RelationshipType::Blocks, alice);
    assert!(
        matches!(&unknown_target, Err(GraphError::ItemNotFound(id)) if *id == ghost),
        "unknown target is typed ItemNotFound: {unknown_target:?}"
    );
    items::soft_delete_item(&mut conn, ItemType::Task, t5.id, alice).expect("soft-deleting t5");
    let deleted = graph::link_items(&mut conn, t1.id, t5.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(&deleted, Err(GraphError::ItemNotFound(id)) if *id == t5.id),
        "soft-deleted endpoint is typed ItemNotFound: {deleted:?}"
    );
    let self_link = graph::link_items(&mut conn, t1.id, t1.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(&self_link, Err(GraphError::SelfLink(id)) if *id == t1.id),
        "self-link is typed SelfLink: {self_link:?}"
    );

    // ---- duplicate edge: UNIQUE(source, target, relationship) ------------------
    let duplicate = graph::link_items(&mut conn, s1.id, i1.id, RelationshipType::Parent, alice);
    assert!(
        matches!(
            &duplicate,
            Err(GraphError::AlreadyLinked {
                relationship: RelationshipType::Parent,
                source_id,
                target_id,
            }) if *source_id == s1.id && *target_id == i1.id
        ),
        "duplicate link is typed AlreadyLinked: {duplicate:?}"
    );
    assert_eq!(
        edge_count(&mut conn, s1.id, i1.id, RelationshipType::Parent),
        1,
        "the unique edge is stored exactly once"
    );
    // The same pair under a DIFFERENT relationship is a different edge (the
    // constraint is per-relationship).
    graph::link_items(&mut conn, i1.id, t2.id, RelationshipType::Parent, alice)
        .expect("same pair as the existing blocks edge, different relationship");

    // ---- relationships_for: both directions, grouped by relationship -----------
    let i1_rels = graph::relationships_for(&mut conn, i1.id).expect("relationships_for i1");
    assert_eq!(
        i1_rels.outgoing,
        vec![
            Neighbor {
                relationship: RelationshipType::Blocks,
                id: t2.id,
                short_code: "ACME-T-0002".into(),
                entity_type: ItemType::Task,
                title: "Task Two".into(),
                archived_at: None,
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: t1.id,
                short_code: "ACME-T-0001".into(),
                entity_type: ItemType::Task,
                title: "Task One".into(),
                archived_at: None,
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: t2.id,
                short_code: "ACME-T-0002".into(),
                entity_type: ItemType::Task,
                title: "Task Two".into(),
                archived_at: None,
            },
            Neighbor {
                relationship: RelationshipType::Supports,
                id: d1.id,
                short_code: "ACME-D-0001".into(),
                entity_type: ItemType::Document,
                title: "Document One".into(),
                archived_at: None,
            },
        ],
        "outgoing edges of i1, grouped by relationship (alphabetical), then insertion order"
    );
    assert_eq!(
        i1_rels.incoming,
        vec![
            Neighbor {
                relationship: RelationshipType::Informs,
                id: d1.id,
                short_code: "ACME-D-0001".into(),
                entity_type: ItemType::Document,
                title: "Document One".into(),
                archived_at: None,
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: s1.id,
                short_code: "ACME-S-0001".into(),
                entity_type: ItemType::Strategy,
                title: "Strategy One".into(),
                archived_at: None,
            },
        ],
        "incoming edges of i1"
    );
    // Both directions on the blocked task: t2 is blocked by t1 AND i1.
    let t2_rels = graph::relationships_for(&mut conn, t2.id).expect("relationships_for t2");
    let t2_blockers: Vec<Uuid> = t2_rels
        .incoming
        .iter()
        .filter(|n| n.relationship == RelationshipType::Blocks)
        .map(|n| n.id)
        .collect();
    assert_eq!(t2_blockers, vec![t1.id, i1.id], "t2 sees both blockers");
    assert_eq!(
        t2_rels
            .outgoing
            .iter()
            .map(|n| (n.relationship, n.id))
            .collect::<Vec<_>>(),
        vec![
            (RelationshipType::Blocks, t3.id),
            (RelationshipType::Blocks, t4.id),
        ],
        "t2's outgoing blocks"
    );

    // ---- unlink: removes the edge + logs; missing edge is typed ----------------
    graph::unlink_items(&mut conn, i1.id, t2.id, RelationshipType::Blocks, alice)
        .expect("unlinking blocks i1 -> t2");
    assert_eq!(
        edge_count(&mut conn, i1.id, t2.id, RelationshipType::Blocks),
        0,
        "unlink removes the edge"
    );
    let removed = relationship_activity(&mut conn, ActivityAction::RelationshipRemove);
    assert_eq!(
        removed,
        ["relationship:blocks:ACME-I-0001->ACME-T-0002".to_string()],
        "unlink writes a relationship_remove activity row"
    );
    let missing = graph::unlink_items(&mut conn, i1.id, t2.id, RelationshipType::Blocks, alice);
    assert!(
        matches!(
            &missing,
            Err(GraphError::NotLinked {
                relationship: RelationshipType::Blocks,
                source_id,
                target_id,
            }) if *source_id == i1.id && *target_id == t2.id
        ),
        "unlinking a missing edge is typed NotLinked: {missing:?}"
    );
    assert_eq!(
        relationship_activity(&mut conn, ActivityAction::RelationshipRemove).len(),
        1,
        "NotLinked writes no activity row"
    );
    // The unlinked edge no longer appears in either direction.
    let t2_after = graph::relationships_for(&mut conn, t2.id).expect("relationships_for t2");
    assert!(
        t2_after
            .incoming
            .iter()
            .all(|n| !(n.relationship == RelationshipType::Blocks && n.id == i1.id)),
        "unlinked edge is gone from queries"
    );

    // ---- EXPLAIN: both lookup directions are index-backed (S-0004) -------------
    sql_query("SET enable_seqscan = off")
        .execute(&mut conn)
        .expect("disabling seqscan for plan inspection");
    let source_plan = explain(
        &mut conn,
        &format!(
            "SELECT source_id, target_id FROM item_relationships \
             WHERE source_id = '{}' AND relationship = 'parent'",
            i1.id
        ),
    );
    assert!(
        source_plan.contains("idx_item_relationships_source"),
        "source+relationship lookup must use idx_item_relationships_source:\n{source_plan}"
    );
    let target_plan = explain(
        &mut conn,
        &format!(
            "SELECT source_id, target_id FROM item_relationships \
             WHERE target_id = '{}' AND relationship = 'parent'",
            i1.id
        ),
    );
    assert!(
        target_plan.contains("idx_item_relationships_target"),
        "target+relationship lookup must use idx_item_relationships_target:\n{target_plan}"
    );
    println!("EXPLAIN source+relationship:\n{source_plan}\n");
    println!("EXPLAIN target+relationship:\n{target_plan}\n");
    sql_query("RESET enable_seqscan")
        .execute(&mut conn)
        .expect("resetting seqscan");

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}

/// KAIROS-T-0080: the children-progress rollups — per-parent grouping,
/// multi-board children, done semantics, soft-delete + supports/informs
/// exclusion, and the whole-board batch (one call for every parent, the
/// shape that forbids N+1).
#[test]
fn children_progress_rollups() {
    const PROGRESS_DB: &str = "kairos_progress_test";
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {PROGRESS_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {PROGRESS_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let scratch_url = with_database(&admin_url, PROGRESS_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let alice = insert_user(&mut conn, "dex|alice", "alice@acme.test", "Alice");

    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let board_a = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery A",
        "delivery-a",
        None,
        Some(alice),
    )
    .expect("creating delivery A")
    .id;
    let board_b = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery B",
        "delivery-b",
        None,
        Some(alice),
    )
    .expect("creating delivery B")
    .id;
    let column_by_name = |conn: &mut PgConnection, board: Uuid, name: &str| -> Uuid {
        schema::board_columns::table
            .filter(schema::board_columns::board_id.eq(board))
            .filter(schema::board_columns::name.eq(name))
            .select(schema::board_columns::id)
            .first(conn)
            .expect("column exists")
    };
    let a_completed = column_by_name(&mut conn, board_a, "Completed");
    let a_backlog = column_by_name(&mut conn, board_a, "Backlog");
    let b_backlog = column_by_name(&mut conn, board_b, "Backlog");

    // The provisioning path seeds the done flag on terminal columns.
    let completed_is_done: bool = schema::board_columns::table
        .filter(schema::board_columns::id.eq(a_completed))
        .select(schema::board_columns::is_done)
        .first(&mut conn)
        .expect("loading flag");
    assert!(completed_is_done, "seeded Completed column starts done");

    let make_initiative = |conn: &mut PgConnection, title: &str| {
        items::create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: None,
                title,
                content: "",
                complexity: None,
                bucket_type: None,
            },
            alice,
        )
        .expect("creating initiative")
    };
    let i1 = make_initiative(&mut conn, "Rollup parent");
    let i2 = make_initiative(&mut conn, "Second parent");

    let make_task = |conn: &mut PgConnection, board: Uuid, column: Uuid, title: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: board,
                column_id: Some(column),
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: kairos_db::models::enums::WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("creating task")
    };
    let t1 = make_task(&mut conn, board_a, a_completed, "Done child");
    let t2 = make_task(&mut conn, board_a, a_backlog, "Open child A");
    let t3 = make_task(&mut conn, board_b, b_backlog, "Open child B");
    let t4 = make_task(&mut conn, board_a, a_backlog, "Deleted child");
    let t5 = make_task(&mut conn, board_b, b_backlog, "Other parent's child");

    for child in [t1.id, t2.id, t3.id, t4.id] {
        graph::link_items(&mut conn, i1.id, child, RelationshipType::Parent, alice)
            .expect("linking child");
    }
    graph::link_items(&mut conn, i2.id, t5.id, RelationshipType::Parent, alice)
        .expect("linking second parent's child");
    // Supporting material never counts toward progress.
    let d1 = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Design notes",
            content: Some(""),
            template_id: None,
        },
        alice,
    )
    .expect("creating document");
    graph::link_items(&mut conn, i1.id, d1.id, RelationshipType::Supports, alice)
        .expect("linking supports");
    // Soft-deleted children drop out.
    items::soft_delete_item(&mut conn, ItemType::Task, t4.id, alice).expect("soft-deleting");

    // ---- per-parent rollup ---------------------------------------------------
    let rows = graph::children_progress(&mut conn, i1.id).expect("children_progress");
    let buckets: Vec<(Uuid, bool, i64)> = rows
        .iter()
        .map(|row| (row.column_id, row.is_done, row.count))
        .collect();
    assert!(
        buckets.contains(&(a_completed, true, 1))
            && buckets.contains(&(a_backlog, false, 1))
            && buckets.contains(&(b_backlog, false, 1)),
        "multi-board children group by column: {buckets:?}"
    );
    assert!(rows.iter().all(|row| row.board_has_done));
    let (done, total) = kairos_core::items::children_progress_counts(
        &rows
            .iter()
            .map(|row| (row.is_done, row.count))
            .collect::<Vec<_>>(),
    );
    assert_eq!((done, total), (1, 3), "doc + soft-deleted child excluded");

    // A leaf has no rollup at all.
    assert!(
        graph::children_progress(&mut conn, t1.id)
            .expect("leaf rollup")
            .is_empty()
    );

    // ---- whole-board batch: every parent from ONE call -----------------------
    let batch = graph::board_children_progress(&mut conn, initiative_board).expect("board rollup");
    let p1 = batch.get(&i1.id).expect("i1 present");
    assert_eq!((p1.done, p1.total, p1.has_done), (1, 3, true));
    let p2 = batch.get(&i2.id).expect("i2 present");
    assert_eq!((p2.done, p2.total, p2.has_done), (0, 1, true));

    // ---- zero-done-columns: composition only, never a fraction ---------------
    sql_query("UPDATE board_columns SET is_done = false")
        .execute(&mut conn)
        .expect("unflagging all columns");
    let rows = graph::children_progress(&mut conn, i1.id).expect("children_progress");
    assert!(
        rows.iter().all(|row| !row.is_done && !row.board_has_done),
        "no done semantics anywhere"
    );
    let batch = graph::board_children_progress(&mut conn, initiative_board).expect("board rollup");
    let p1 = batch.get(&i1.id).expect("i1 present");
    assert_eq!((p1.done, p1.total, p1.has_done), (0, 3, false));
}

// ---------------------------------------------------------------------------
// Focal subgraph (KAIROS-T-0088)
// ---------------------------------------------------------------------------

/// `item_subgraph`: depth bounding with min-depth per node, cross-links
/// between visited nodes, neighbour `degree`, and archived nodes drawn
/// MARKED rather than omitted — the wire contract the graph view draws
/// from.
///
/// The archived leg changed in KAIROS-T-0158. It used to assert that a
/// soft-deleted node was absent; it now asserts the node is present with
/// `archived_at` set, and that the edges through it survive. Absence was
/// never a smaller picture, it was a broken one: the walk reads
/// `item_relationships` directly, so an archived item's neighbours stayed
/// in the node set while the node joining them to the focus was hydrated
/// away.
#[test]
fn focal_subgraph_contract() {
    const SUBGRAPH_DB: &str = "kairos_subgraph_test";
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {SUBGRAPH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SUBGRAPH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let scratch_url = with_database(&admin_url, SUBGRAPH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let alice = insert_user(&mut conn, "dex|subgraph", "subgraph@acme.test", "Alice");

    let strategy_board = board_id_by_slug(&mut conn, "strategy");
    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let delivery = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("creating delivery board")
    .id;

    // S ─parent→ I1 ─parent→ T1, T2(soft-deleted); S ─parent→ I2 ─parent→ T3;
    // T1 ─blocks→ T3 (the cross-initiative dependency); I1 ─supports→ D.
    let s = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: "North star",
            content: "",
            hypothesis: None,
        },
        alice,
    )
    .expect("strategy");
    let mk_initiative = |conn: &mut PgConnection, title: &str| {
        items::create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: None,
                title,
                content: "",
                complexity: None,
                bucket_type: None,
            },
            alice,
        )
        .expect("initiative")
    };
    let i1 = mk_initiative(&mut conn, "Initiative one");
    let i2 = mk_initiative(&mut conn, "Initiative two");
    let mk_task = |conn: &mut PgConnection, title: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: delivery,
                column_id: None,
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: kairos_db::models::WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("task")
    };
    let t1 = mk_task(&mut conn, "Task one");
    let t2 = mk_task(&mut conn, "Task doomed");
    let t3 = mk_task(&mut conn, "Task three");
    let d = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Supporting doc",
            content: Some(""),
            template_id: None,
        },
        alice,
    )
    .expect("document");

    let link = |conn: &mut PgConnection, from: Uuid, to: Uuid, rel: RelationshipType| {
        graph::link_items(conn, from, to, rel, alice).expect("linking");
    };
    link(&mut conn, s.id, i1.id, RelationshipType::Parent);
    link(&mut conn, s.id, i2.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t1.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t2.id, RelationshipType::Parent);
    link(&mut conn, i2.id, t3.id, RelationshipType::Parent);
    link(&mut conn, t1.id, t3.id, RelationshipType::Blocks);
    link(&mut conn, i1.id, d.id, RelationshipType::Supports);
    // t2 also blocks t3 while alive — its soft-delete below must drop it
    // from BOTH the subgraph and the blocks rollup.
    link(&mut conn, t2.id, t3.id, RelationshipType::Blocks);
    diesel::update(schema::tasks::table.find(t2.id))
        .set(schema::tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("soft-deleting t2");

    // --- depth 2 from T1 ----------------------------------------------------
    let (nodes, edges) = graph::item_subgraph(&mut conn, t1.id, 2).expect("subgraph");
    let by_id = |id: Uuid| nodes.iter().find(|n| n.id == id);
    // Visible: T1(0), I1(1), T3(1), S(2), I2(2), T2(2), D(2) — the
    // archived T2 among them (KAIROS-T-0158).
    assert_eq!(
        nodes.len(),
        7,
        "every reachable node, archived too: {nodes:?}"
    );
    let t2_node = by_id(t2.id).expect("the archived node is drawn, not dropped");
    assert!(
        t2_node.archived_at.is_some(),
        "the archived node is MARKED so a client can draw it distinctly: {t2_node:?}"
    );
    assert_eq!(
        t2_node.status, "Backlog",
        "an archived card keeps the column it stood in (ADR-20 rule 1)"
    );
    assert!(
        nodes
            .iter()
            .filter(|n| n.id != t2.id)
            .all(|n| n.archived_at.is_none()),
        "only the archived node carries the marker: {nodes:?}"
    );
    assert_eq!(by_id(t2.id).expect("t2").depth, 2);
    assert_eq!(by_id(t1.id).expect("focus").depth, 0);
    assert_eq!(by_id(i1.id).expect("i1").depth, 1);
    assert_eq!(by_id(t3.id).expect("t3").depth, 1);
    assert_eq!(by_id(s.id).expect("s").depth, 2);
    assert_eq!(by_id(i2.id).expect("i2").depth, 2);
    assert_eq!(by_id(d.id).expect("d").depth, 2);
    // Status vocabulary split (A-0018): workflow = column name, doc = lifecycle.
    assert_eq!(by_id(t1.id).expect("t1").status, "Backlog");
    assert_eq!(by_id(d.id).expect("d").status, "draft");
    assert_eq!(by_id(t1.id).expect("t1").entity_type, ItemType::Task);
    // Degree counts every neighbour that would hydrate, archived included,
    // so `+N` agrees with the node set: I1 touches S, T1, T2, D.
    assert_eq!(by_id(i1.id).expect("i1").degree, 4);
    assert_eq!(by_id(t1.id).expect("t1").degree, 2);
    // Nodes ordered by short code (deterministic layout input).
    let codes: Vec<&str> = nodes.iter().map(|n| n.short_code.as_str()).collect();
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(codes, sorted, "nodes sorted by short code");

    // Edges: ALL live edges among the visible set — including the
    // cross-link I2→T3 the walk did not discover first. Edge depth is the
    // view depth at which both endpoints are visible.
    let edge = |src: Uuid, tgt: Uuid, rel: RelationshipType| {
        edges
            .iter()
            .find(|e| e.source_id == src && e.target_id == tgt && e.relationship == rel)
    };
    assert_eq!(edges.len(), 8, "every edge among visible: {edges:?}");
    assert_eq!(
        edge(i1.id, t1.id, RelationshipType::Parent)
            .expect("i1->t1")
            .depth,
        1
    );
    assert_eq!(
        edge(t1.id, t3.id, RelationshipType::Blocks)
            .expect("blocks")
            .depth,
        1
    );
    assert_eq!(
        edge(i2.id, t3.id, RelationshipType::Parent)
            .expect("cross-link present")
            .depth,
        2
    );
    assert_eq!(
        edge(i1.id, d.id, RelationshipType::Supports)
            .expect("supports")
            .depth,
        2
    );
    // The edges through the archived node survive, which is the point:
    // T3 is reachable from the focus by two routes, and dropping T2 used
    // to delete one of them out of the middle of the picture.
    assert_eq!(
        edge(i1.id, t2.id, RelationshipType::Parent)
            .expect("the archived child's edge is drawn")
            .depth,
        2
    );
    assert_eq!(
        edge(t2.id, t3.id, RelationshipType::Blocks)
            .expect("the archived blocker's edge is drawn")
            .depth,
        2
    );

    // --- blocks rollup for board cards (KAIROS-T-0091) ----------------------
    // One grouped query; soft-deleted neighbors (t2) never count; items
    // without live blocks edges have no entry. The contrast with the
    // subgraph above is the whole KAIROS-T-0158 design call: the picture
    // SHOWS the archived blocker, the card's "blocked by" count does not
    // COUNT it (ADR-20 rule 5 — archived work cannot block anything).
    let summary =
        graph::blocks_summary(&mut conn, &[t1.id, t2.id, t3.id, i1.id]).expect("blocks summary");
    let t1_counts = summary.get(&t1.id).expect("t1 counts");
    assert_eq!((t1_counts.blocked_by, t1_counts.blocks), (0, 1));
    let t3_counts = summary.get(&t3.id).expect("t3 counts");
    assert_eq!(
        (t3_counts.blocked_by, t3_counts.blocks),
        (1, 0),
        "t2's edge is dead weight: only t1 counts"
    );
    assert!(!summary.contains_key(&i1.id), "no blocks edges, no entry");

    // --- depth 1 from T1: strict bound --------------------------------------
    let (near, near_edges) = graph::item_subgraph(&mut conn, t1.id, 1).expect("depth 1");
    assert_eq!(
        near.iter()
            .map(|n| n.id)
            .collect::<std::collections::HashSet<_>>(),
        [t1.id, i1.id, t3.id].into_iter().collect(),
        "depth 1 = focus + direct neighbors"
    );
    assert_eq!(
        near_edges.len(),
        2,
        "only the two incident edges: {near_edges:?}"
    );

    drop(conn);
    sql_query(format!(
        "DROP DATABASE IF EXISTS {SUBGRAPH_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database after test");
}

// ---------------------------------------------------------------------------
// Archived neighbours (KAIROS-T-0158, KAIROS-A-0020)
// ---------------------------------------------------------------------------

/// The case that motivated KAIROS-T-0158: **an initiative with one live
/// and one archived child.**
///
/// Ask "what did this initiative contain?" and the answer used to be one
/// task, not two — for an initiative that is not archived at all. The
/// missing row was not marked, not counted and not recoverable by any
/// flag; `item_relationships` rows are hard-deleted, so the edge was
/// intact the whole time and only the hydrating join hid it.
///
/// This test pins the two halves of the design call together, because
/// either alone would be wrong:
///
/// - the relationship LIST names both children, the archived one marked
///   (containment is a fact about the record);
/// - the progress ROLLUP counts only the live one (progress is a fact
///   about live work — ADR-20 rule 5).
#[test]
fn archived_children_are_listed_marked_but_never_counted() {
    const NEIGHBOUR_DB: &str = "kairos_archived_neighbours_test";
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!(
        "DROP DATABASE IF EXISTS {NEIGHBOUR_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {NEIGHBOUR_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let scratch_url = with_database(&admin_url, NEIGHBOUR_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let alice = insert_user(&mut conn, "dex|neighbours", "neighbours@acme.test", "Alice");

    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let delivery = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("creating delivery board")
    .id;

    let initiative = items::create_initiative(
        &mut conn,
        CreateInitiative {
            board_id: initiative_board,
            column_id: None,
            title: "Ship the thing",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        alice,
    )
    .expect("initiative");
    let mut task = |title: &str| {
        items::create_task(
            &mut conn,
            CreateTask {
                board_id: delivery,
                column_id: None,
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: kairos_db::models::WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("task")
    };
    let live_child = task("Still in flight");
    let archived_child = task("Done and put away");

    for child in [live_child.id, archived_child.id] {
        graph::link_items(
            &mut conn,
            initiative.id,
            child,
            RelationshipType::Parent,
            alice,
        )
        .expect("linking child");
    }
    diesel::update(schema::tasks::table.find(archived_child.id))
        .set(schema::tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("archiving the second child");

    // --- the list: BOTH children, the archived one marked -------------------
    let rels = graph::relationships_for(&mut conn, initiative.id).expect("relationships_for");
    let children: Vec<(&str, bool)> = rels
        .outgoing
        .iter()
        .filter(|n| n.relationship == RelationshipType::Parent)
        .map(|n| (n.short_code.as_str(), n.archived_at.is_some()))
        .collect();
    assert_eq!(
        children,
        vec![
            (live_child.short_code.as_str(), false),
            (archived_child.short_code.as_str(), true),
        ],
        "a live initiative lists BOTH children, the archived one marked: {rels:?}"
    );
    // Marked means marked with a value, not merely present: the GUI, the
    // MCP text and the wire DTO all render the timestamp.
    let archived_neighbour = rels
        .outgoing
        .iter()
        .find(|n| n.id == archived_child.id)
        .expect("archived child present");
    assert!(
        archived_neighbour.archived_at.is_some(),
        "the marker carries WHEN it was put away: {archived_neighbour:?}"
    );
    assert_eq!(
        archived_neighbour.title, "Done and put away",
        "the archived child hydrates fully — title and type, not a stub"
    );
    assert_eq!(archived_neighbour.entity_type, ItemType::Task);

    // --- the archived child's own edges are intact, both directions ---------
    // `item_relationships` has no `deleted_at`, so archiving one endpoint
    // never touched the edge. Reading FROM the archived side is the audit
    // answer ADR-20 rule 1 promises.
    let from_archived =
        graph::relationships_for(&mut conn, archived_child.id).expect("relationships_for archived");
    assert_eq!(
        from_archived
            .incoming
            .iter()
            .map(|n| (n.short_code.as_str(), n.archived_at.is_some()))
            .collect::<Vec<_>>(),
        vec![(initiative.short_code.as_str(), false)],
        "an archived item still knows its LIVE parent, unmarked: {from_archived:?}"
    );

    // --- the rollup: the live child only ------------------------------------
    let progress = graph::children_progress(&mut conn, initiative.id).expect("children_progress");
    let total: i64 = progress.iter().map(|row| row.count).sum();
    assert_eq!(
        total, 1,
        "progress counts LIVE children only — archived work is not live \
         work (ADR-20 rule 5): {progress:?}"
    );

    drop(conn);
    sql_query(format!(
        "DROP DATABASE IF EXISTS {NEIGHBOUR_DB} WITH (FORCE)"
    ))
    .execute(&mut admin_conn)
    .expect("dropping scratch database after test");
}
