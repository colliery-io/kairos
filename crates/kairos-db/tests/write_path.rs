//! Integration test for the item write-path services (KAIROS-T-0012,
//! contract per KAIROS-A-0004 with A-0001 cascade semantics, layering per
//! KAIROS-A-0009).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration`); the database is never mocked
//! (KAIROS-A-0012). For isolation the test drops and recreates a dedicated
//! scratch database (`kairos_write_path_test`) on the same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - create services assign `{PREFIX}-{TYPE}-{NNNN}` short codes (prefix
//!   defaulted from the tenant slug), default board items to the first
//!   column, insert at version 1 with the v1 history baseline, and log
//!   `create` to `activity_log`
//! - short-code uniqueness under concurrency: N concurrent creates yield N
//!   distinct sequential codes (real sequences, real threads)
//! - optimistic concurrency: two concurrent writers on the same version →
//!   exactly one success and one typed `VersionConflict` carrying the
//!   winner's content; the stale single-threaded path conflicts too
//! - `item_history` accumulates exact snapshots v1..vN; rollback copies a
//!   historical snapshot forward as v(N+1)
//! - template-stamped documents copy the template content and its
//!   `template_metadata` defaults into `item_metadata` (KAIROS-A-0003)
//! - soft-delete cascades over `parent` edges (strategy → initiative →
//!   task, plus a document child), drops everything out of the DEFAULT
//!   `searchable_items`/`entity_directory` surface while leaving the rows
//!   reachable to a caller that asks past the default (KAIROS-A-0020), and
//!   records ONE activity row with the cascade count + short codes

use std::sync::{Arc, Barrier};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::items::{
    self, ContentUpdate, CreateDocument, CreateInitiative, CreateStrategy, CreateTask, ItemError,
};
use kairos_db::models::{
    ActivityAction, BoardLevel, NewItemRelationship, NewUser, RelationshipType, TaskType, User,
};
use kairos_db::{create_board, provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_write_path_test";

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

/// A fresh connection to the scratch database with the tenant search_path
/// pinned (same mechanism as the pool).
fn tenant_connection(scratch_url: &str) -> PgConnection {
    let mut conn = PgConnection::establish(scratch_url).expect("connecting to scratch database");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    conn
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

/// The first column of a board (position order).
fn first_column(conn: &mut PgConnection, board: Uuid) -> Uuid {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .order(schema::board_columns::position.asc())
        .select(schema::board_columns::id)
        .first(conn)
        .expect("board has columns")
}

/// All history snapshots for an item as `(version, title, content)`, in
/// version order.
fn history_of(conn: &mut PgConnection, item: Uuid) -> Vec<(i32, String, String)> {
    schema::item_history::table
        .filter(schema::item_history::item_id.eq(item))
        .order(schema::item_history::version.asc())
        .select((
            schema::item_history::version,
            schema::item_history::title,
            schema::item_history::content,
        ))
        .load(conn)
        .expect("loading item_history")
}

/// All `activity_log.details` values for (action, entity_id), in order.
fn activity_details(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq(action.as_str()))
        .filter(schema::activity_log::entity_id.eq(entity))
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading activity_log")
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// How many LIVE rows of `searchable_items` / `entity_directory` carry
/// this id — the default read surface, which is what every listing shows.
///
/// The `deleted_at IS NULL` is spelled out here because since
/// KAIROS-T-0156 the views no longer apply it themselves (KAIROS-A-0020:
/// archived is a visibility default, and a view that has already dropped
/// the rows leaves callers nothing to widen). This helper therefore
/// measures the same thing it always did, but now it says so.
fn view_count(conn: &mut PgConnection, view: &str, id: Uuid) -> i64 {
    let row: CountRow = sql_query(format!(
        "SELECT count(*) AS n FROM {view} WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind::<diesel::sql_types::Uuid, _>(id)
    .get_result(conn)
    .unwrap_or_else(|e| panic!("querying {view}: {e}"));
    row.n
}

/// How many rows of the view carry this id REGARDLESS of liveness — the
/// wide mode `--include-deleted` widens into. Before KAIROS-T-0156 this
/// could only ever return 0 for an archived item, whatever the caller
/// asked for.
fn view_count_any(conn: &mut PgConnection, view: &str, id: Uuid) -> i64 {
    let row: CountRow = sql_query(format!("SELECT count(*) AS n FROM {view} WHERE id = $1"))
        .bind::<diesel::sql_types::Uuid, _>(id)
        .get_result(conn)
        .unwrap_or_else(|e| panic!("querying {view}: {e}"));
    row.n
}

/// The numeric part of a `{PREFIX}-{L}-{NNNN}` short code.
fn code_number(code: &str) -> i64 {
    code.rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("short code {code:?} has no numeric suffix"))
}

#[test]
fn write_path_lifecycle() {
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
    let bob = insert_user(&mut conn, "dex|bob", "bob@acme.test", "Bob");

    let strategy_board = board_id_by_slug(&mut conn, "strategy");
    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let adr_board = board_id_by_slug(&mut conn, "adrs");
    // Delivery boards are per-team and not provisioned by default; tasks
    // need one (KAIROS-T-0008 interpretation).
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

    // ---- create services: short codes, defaults, v1 baseline, audit ---------
    let strategy = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: "Expand upmarket",
            content: "v1 strategy content",
            hypothesis: Some("enterprise wants this"),
        },
        alice,
    )
    .expect("creating strategy");
    assert_eq!(
        strategy.short_code, "ACME-S-0001",
        "prefix defaults to the upper-cased tenant slug (interpretation in KAIROS-T-0012)"
    );
    assert_eq!(strategy.version, 1);
    assert_eq!((strategy.created_by, strategy.updated_by), (alice, alice));
    assert_eq!(
        strategy.column_id,
        first_column(&mut conn, strategy_board),
        "column_id: None defaults to the board's first column"
    );

    let initiative = items::create_initiative(
        &mut conn,
        CreateInitiative {
            board_id: initiative_board,
            column_id: None,
            title: "Enterprise SSO",
            content: "initiative content",
            complexity: None,
            bucket_type: None,
        },
        alice,
    )
    .expect("creating initiative");
    assert_eq!(initiative.short_code, "ACME-I-0001");

    let task = items::create_task(
        &mut conn,
        CreateTask {
            board_id: delivery_board,
            column_id: None,
            title: "SAML handshake",
            content: "task content",
            task_type: TaskType::Task,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: None,
            repository_id: None,
        },
        alice,
    )
    .expect("creating task");
    assert_eq!(task.short_code, "ACME-T-0001");
    assert_eq!(task.column_id, first_column(&mut conn, delivery_board));

    // ---- KAIROS-T-0077: the Planned/Support lane write path -------------------
    let moved = items::set_task_work_class(
        &mut conn,
        task.id,
        kairos_db::models::enums::WorkClass::Support,
        alice,
    )
    .expect("setting work_class");
    assert_eq!(
        moved.work_class,
        kairos_db::models::enums::WorkClass::Support,
        "lane write lands"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::WorkClass, task.id),
        ["work_class:planned->support".to_string()],
        "lane change writes an activity_log row"
    );
    // The lane is orthogonal to content versioning and board position.
    assert_eq!(
        moved.version, task.version,
        "lane write never bumps the version"
    );
    assert_eq!(
        moved.column_id, task.column_id,
        "lane write never moves columns"
    );
    // Setting the value the task already has is a no-op: no second row.
    items::set_task_work_class(
        &mut conn,
        task.id,
        kairos_db::models::enums::WorkClass::Support,
        alice,
    )
    .expect("no-op lane write");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::WorkClass, task.id).len(),
        1,
        "a no-op lane write logs nothing"
    );

    let adr = items::create_adr(
        &mut conn,
        items::CreateAdr {
            board_id: Some(adr_board),
            column_id: None,
            title: "Use SAML",
            content: "adr content",
            decision_maker: None,
            decision_date: None,
        },
        alice,
    )
    .expect("creating adr");
    assert_eq!(adr.short_code, "ACME-A-0001");
    assert_eq!(adr.column_id, Some(first_column(&mut conn, adr_board)));

    let off_board_adr = items::create_adr(
        &mut conn,
        items::CreateAdr {
            board_id: None,
            column_id: None,
            title: "Off board",
            content: "",
            decision_maker: None,
            decision_date: None,
        },
        alice,
    )
    .expect("creating off-board adr");
    assert_eq!(off_board_adr.short_code, "ACME-A-0002");
    assert_eq!(
        (off_board_adr.board_id, off_board_adr.column_id),
        (None, None)
    );

    // v1 baseline snapshot at create time (KAIROS-T-0012 interpretation of
    // A-0004): history is complete from birth.
    assert_eq!(
        history_of(&mut conn, strategy.id),
        [(
            1,
            "Expand upmarket".to_string(),
            "v1 strategy content".to_string()
        )],
        "create writes the v1 history baseline"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Create, strategy.id),
        ["short_code:ACME-S-0001".to_string()],
        "create writes an activity_log row"
    );

    // ---- short-code uniqueness under concurrency ------------------------------
    const WRITERS: usize = 8;
    let barrier = Arc::new(Barrier::new(WRITERS));
    let concurrent: Vec<(String, Uuid)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..WRITERS)
            .map(|i| {
                let barrier = Arc::clone(&barrier);
                let scratch_url = scratch_url.clone();
                scope.spawn(move || {
                    let mut conn = tenant_connection(&scratch_url);
                    barrier.wait();
                    let task = items::create_task(
                        &mut conn,
                        CreateTask {
                            board_id: delivery_board,
                            column_id: None,
                            title: &format!("concurrent {i}"),
                            content: "",
                            task_type: TaskType::Task,
                            work_class: kairos_db::models::enums::WorkClass::Planned,
                            team_id: None,
                            repository_id: None,
                        },
                        alice,
                    )
                    .expect("concurrent create_task");
                    (task.short_code, task.id)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("concurrent create thread panicked"))
            .collect()
    });

    let mut numbers: Vec<i64> = concurrent
        .iter()
        .map(|(code, _)| code_number(code))
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    assert_eq!(
        numbers.len(),
        WRITERS,
        "concurrent creates must yield distinct codes: {concurrent:?}"
    );
    // ACME-T-0001 consumed nextval 1, so the concurrent batch is exactly the
    // next N sequence values — sequential, no duplicates.
    assert_eq!(
        numbers,
        (2..=WRITERS as i64 + 1).collect::<Vec<_>>(),
        "codes are sequential sequence values"
    );

    // ---- optimistic concurrency: two writers, one version --------------------
    let write_barrier = Arc::new(Barrier::new(2));
    let outcomes: Vec<(&str, Result<i32, ItemError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = [("alice wrote this", alice), ("bob wrote this", bob)]
            .into_iter()
            .map(|(content, actor)| {
                let write_barrier = Arc::clone(&write_barrier);
                let scratch_url = scratch_url.clone();
                let strategy_id = strategy.id;
                scope.spawn(move || {
                    let mut conn = tenant_connection(&scratch_url);
                    write_barrier.wait();
                    let result = items::update_item_content(
                        &mut conn,
                        ItemType::Strategy,
                        strategy_id,
                        ContentUpdate {
                            new_title: None,
                            new_content: content,
                            expected_version: 1,
                        },
                        actor,
                    );
                    (content, result)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("writer thread panicked"))
            .collect()
    });

    let winners: Vec<&str> = outcomes
        .iter()
        .filter(|(_, r)| matches!(r, Ok(2)))
        .map(|(c, _)| *c)
        .collect();
    let conflicts: Vec<&(&str, Result<i32, ItemError>)> =
        outcomes.iter().filter(|(_, r)| r.is_err()).collect();
    assert_eq!(
        (winners.len(), conflicts.len()),
        (1, 1),
        "exactly one success and one conflict: {outcomes:?}"
    );
    let winner_content = winners[0];
    match &conflicts[0].1 {
        Err(ItemError::VersionConflict {
            item_id,
            expected_version,
            current_version,
            current_title,
            current_content,
        }) => {
            assert_eq!(*item_id, strategy.id);
            assert_eq!(*expected_version, 1);
            assert_eq!(
                *current_version, 2,
                "conflict carries the version the winner produced"
            );
            assert_eq!(current_title, "Expand upmarket");
            assert_eq!(
                current_content, winner_content,
                "conflict carries the winner's content for client-side reconciliation"
            );
        }
        other => panic!("expected VersionConflict, got {other:?}"),
    }

    // ---- history accumulates exact snapshots v1..vN ---------------------------
    let v3 = items::update_item_content(
        &mut conn,
        ItemType::Strategy,
        strategy.id,
        ContentUpdate {
            new_title: Some("Expand upmarket (revised)"),
            new_content: "v3 content",
            expected_version: 2,
        },
        alice,
    )
    .expect("editing v2 -> v3");
    assert_eq!(v3, 3);
    assert_eq!(
        history_of(&mut conn, strategy.id),
        [
            (
                1,
                "Expand upmarket".to_string(),
                "v1 strategy content".to_string()
            ),
            (2, "Expand upmarket".to_string(), winner_content.to_string()),
            (
                3,
                "Expand upmarket (revised)".to_string(),
                "v3 content".to_string()
            ),
        ],
        "item_history holds one exact snapshot per version"
    );

    // Stale single-threaded writer gets the same typed conflict.
    let stale = items::update_item_content(
        &mut conn,
        ItemType::Strategy,
        strategy.id,
        ContentUpdate {
            new_title: None,
            new_content: "based on v1",
            expected_version: 1,
        },
        bob,
    );
    assert!(
        matches!(
            &stale,
            Err(ItemError::VersionConflict { current_version: 3, current_content, .. })
                if current_content == "v3 content"
        ),
        "stale write returns the current version + content: {stale:?}"
    );

    // ---- rollback copies a historical snapshot forward as a NEW version ------
    let rolled = items::rollback_item(&mut conn, ItemType::Strategy, strategy.id, 1, alice)
        .expect("rolling back to v1");
    assert_eq!(rolled, 4, "rollback creates v(N+1), never rewrites history");
    let (live_version, live_title, live_content): (i32, String, String) = schema::strategies::table
        .filter(schema::strategies::id.eq(strategy.id))
        .select((
            schema::strategies::version,
            schema::strategies::title,
            schema::strategies::content,
        ))
        .first(&mut conn)
        .expect("loading strategy");
    assert_eq!(
        (live_version, live_title.as_str(), live_content.as_str()),
        (4, "Expand upmarket", "v1 strategy content"),
        "the live row now carries the v1 snapshot at version 4"
    );
    assert_eq!(
        history_of(&mut conn, strategy.id).last(),
        Some(&(
            4,
            "Expand upmarket".to_string(),
            "v1 strategy content".to_string()
        )),
        "the rollback itself is snapshotted"
    );
    let missing = items::rollback_item(&mut conn, ItemType::Strategy, strategy.id, 99, alice);
    assert!(
        matches!(
            &missing,
            Err(ItemError::HistoryNotFound { version: 99, .. })
        ),
        "rolling back to a nonexistent version is a typed error: {missing:?}"
    );

    // ---- template-stamped document (KAIROS-A-0003) ----------------------------
    let (prd_template, prd_content): (Uuid, String) = schema::templates::table
        .filter(schema::templates::slug.eq("prd"))
        .select((schema::templates::id, schema::templates::content))
        .first(&mut conn)
        .expect("provisioned prd template");
    let doc = items::create_document(
        &mut conn,
        CreateDocument {
            title: "SSO PRD",
            content: None,
            template_id: Some(prd_template),
        },
        alice,
    )
    .expect("creating templated document");
    assert_eq!(doc.short_code, "ACME-D-0001");
    assert_eq!(
        doc.content, prd_content,
        "template content is copied into the document"
    );
    let mut stamped: Vec<(String, String)> = schema::item_metadata::table
        .inner_join(schema::metadata_definitions::table)
        .filter(schema::item_metadata::item_id.eq(doc.id))
        .select((
            schema::metadata_definitions::slug,
            schema::item_metadata::value,
        ))
        .load(&mut conn)
        .expect("loading stamped metadata");
    stamped.sort();
    assert_eq!(
        stamped,
        [("document_type".to_string(), "prd".to_string())],
        "template_metadata defaults are stamped as item_metadata rows \
         ('Document status' retired by KAIROS-T-0078 — lifecycle is a column)"
    );
    assert_eq!(
        doc.lifecycle,
        kairos_db::models::enums::DocumentLifecycle::Draft,
        "new documents are born draft"
    );

    // ---- KAIROS-T-0078: lifecycle write path + stamping scope filter ----------
    let published = items::set_document_lifecycle(
        &mut conn,
        doc.id,
        kairos_db::models::enums::DocumentLifecycle::Published,
        alice,
    )
    .expect("setting lifecycle");
    assert_eq!(
        published.lifecycle,
        kairos_db::models::enums::DocumentLifecycle::Published
    );
    assert_eq!(
        published.version, doc.version,
        "lifecycle never bumps the content version"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Lifecycle, doc.id),
        ["lifecycle:draft->published".to_string()],
        "lifecycle change writes an activity row"
    );
    items::set_document_lifecycle(
        &mut conn,
        doc.id,
        kairos_db::models::enums::DocumentLifecycle::Published,
        alice,
    )
    .expect("no-op lifecycle write");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Lifecycle, doc.id).len(),
        1,
        "a no-op lifecycle write logs nothing"
    );

    // Stamping is scope-enforced IN kairos-db: associate a task-scoped
    // definition with the prd template — a fresh document must not stamp
    // it, whatever the template association claims.
    let scoped_def: Uuid = diesel::insert_into(schema::metadata_definitions::table)
        .values((
            schema::metadata_definitions::name.eq("Task-only field"),
            schema::metadata_definitions::slug.eq("task_only"),
            schema::metadata_definitions::field_type.eq("string"),
            schema::metadata_definitions::is_system_default.eq(false),
        ))
        .returning(schema::metadata_definitions::id)
        .get_result(&mut conn)
        .expect("creating scoped definition");
    diesel::insert_into(schema::metadata_definition_scopes::table)
        .values((
            schema::metadata_definition_scopes::metadata_definition_id.eq(scoped_def),
            schema::metadata_definition_scopes::entity_type.eq("task"),
        ))
        .execute(&mut conn)
        .expect("scoping it to tasks");
    diesel::insert_into(schema::template_metadata::table)
        .values((
            schema::template_metadata::template_id.eq(prd_template),
            schema::template_metadata::metadata_definition_id.eq(scoped_def),
            schema::template_metadata::default_value.eq("smuggled"),
            schema::template_metadata::required.eq(false),
        ))
        .execute(&mut conn)
        .expect("associating it with the prd template");
    let filtered_doc = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Scope-filtered PRD",
            content: None,
            template_id: Some(prd_template),
        },
        alice,
    )
    .expect("creating second templated document");
    let smuggled: i64 = schema::item_metadata::table
        .filter(schema::item_metadata::item_id.eq(filtered_doc.id))
        .filter(schema::item_metadata::metadata_definition_id.eq(scoped_def))
        .count()
        .get_result(&mut conn)
        .expect("counting smuggled stamps");
    assert_eq!(
        smuggled, 0,
        "stamping never writes a definition outside its scope"
    );

    // ---- soft-delete cascade over parent edges --------------------------------
    // strategy -> initiative -> task, plus a document child of the
    // initiative (documents cascade too).
    let child_doc = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Initiative notes",
            content: Some("notes"),
            template_id: None,
        },
        alice,
    )
    .expect("creating child document");
    // D-0002 went to the scope-filter fixture above.
    assert_eq!(child_doc.short_code, "ACME-D-0003");
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
                target_id: child_doc.id,
                relationship: RelationshipType::Parent,
            },
        ])
        .execute(&mut conn)
        .expect("inserting parent edges");

    let cascade_set = [strategy.id, initiative.id, task.id, child_doc.id];
    for id in cascade_set {
        assert_eq!(view_count(&mut conn, "searchable_items", id), 1);
        assert_eq!(view_count(&mut conn, "entity_directory", id), 1);
    }

    let outcome = items::soft_delete_item(&mut conn, ItemType::Strategy, strategy.id, alice)
        .expect("soft-deleting the strategy root");
    assert_eq!(outcome.root_short_code, "ACME-S-0001");
    assert_eq!(
        outcome.cascaded_short_codes,
        ["ACME-D-0003", "ACME-I-0001", "ACME-T-0001"],
        "the whole parent-edge subtree is cascaded"
    );

    // Archived means hidden by default, not gone (KAIROS-A-0020). These
    // two assertions used to read "excluded from the view" full stop —
    // KAIROS-T-0156 split that into the two halves it had conflated: the
    // default surface still hides the rows, and the row is still THERE to
    // be asked for. Both halves matter. The first is the entire
    // user-visible purpose of archiving; the second is what lets an
    // auditor ever ask "what did that ticket say?".
    for id in cascade_set {
        assert_eq!(
            view_count(&mut conn, "searchable_items", id),
            0,
            "soft-deleted items are excluded from the DEFAULT searchable_items surface"
        );
        assert_eq!(
            view_count(&mut conn, "entity_directory", id),
            0,
            "soft-deleted items are excluded from the DEFAULT entity_directory surface"
        );
        assert_eq!(
            view_count_any(&mut conn, "searchable_items", id),
            1,
            "but the row is still in searchable_items, carrying its deleted_at"
        );
        assert_eq!(
            view_count_any(&mut conn, "entity_directory", id),
            1,
            "but the row is still in entity_directory, carrying its deleted_at"
        );
    }
    // Unrelated items are untouched (a task from the concurrent batch and
    // the templated document).
    assert_eq!(
        view_count(&mut conn, "entity_directory", concurrent[0].1),
        1
    );
    assert_eq!(view_count(&mut conn, "searchable_items", doc.id), 1);

    assert_eq!(
        activity_details(&mut conn, ActivityAction::Delete, strategy.id),
        ["short_code:ACME-S-0001 cascade:3 \
             descendants:ACME-D-0003,ACME-I-0001,ACME-T-0001"
            .to_string()],
        "one delete activity row on the root records the cascade"
    );

    // Soft-deleted items are gone from the write path too.
    let edit_deleted = items::update_item_content(
        &mut conn,
        ItemType::Task,
        task.id,
        ContentUpdate {
            new_title: None,
            new_content: "zombie write",
            expected_version: 1,
        },
        alice,
    );
    assert!(
        matches!(&edit_deleted, Err(ItemError::ItemNotFound { .. })),
        "editing a soft-deleted item is ItemNotFound: {edit_deleted:?}"
    );

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
