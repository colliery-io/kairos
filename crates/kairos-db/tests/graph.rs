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
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:5432/kairos";

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
                team_id: None,
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
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: t1.id,
                short_code: "ACME-T-0001".into(),
                entity_type: ItemType::Task,
                title: "Task One".into(),
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: t2.id,
                short_code: "ACME-T-0002".into(),
                entity_type: ItemType::Task,
                title: "Task Two".into(),
            },
            Neighbor {
                relationship: RelationshipType::Supports,
                id: d1.id,
                short_code: "ACME-D-0001".into(),
                entity_type: ItemType::Document,
                title: "Document One".into(),
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
            },
            Neighbor {
                relationship: RelationshipType::Parent,
                id: s1.id,
                short_code: "ACME-S-0001".into(),
                entity_type: ItemType::Strategy,
                title: "Strategy One".into(),
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
