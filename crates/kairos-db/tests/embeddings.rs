//! The embedding store against real PostgreSQL and pgvector (KAIROS-T-0190).
//!
//! `kairos_db::embeddings` is raw SQL over a type diesel does not model, so its
//! unit tests can only cover the pure helpers. Everything that matters — that
//! the vectors actually land, that a re-embed replaces rather than duplicates,
//! that a shortened document does not leave orphaned chunks behind, that the
//! staleness counts mean what an operator will read them to mean — needs the
//! database.
//!
//! Runs against the compose stack (`angreal services up` /
//! `angreal test integration`), per KAIROS-A-0012: the database is never mocked.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use uuid::Uuid;

use kairos_db::embeddings::{
    ChunkWrite, EmbeddingError, StoredModel, counts, forget_item, pending_primary, replace_chunks,
    store_primary, stored_chunk_hashes,
};
use kairos_db::items::{self, CreateTask};
use kairos_db::models::{BoardLevel, NewUser, TaskType, WorkClass};
use kairos_db::{create_board, provision_tenant, run_public_migrations, schema};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_embeddings_test";

/// The width the local model produces (KAIROS-T-0189), so the fixtures exercise
/// the real shape rather than a toy one.
const DIM: usize = 384;

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database path segment");
    format!("{base}/{db_name}")
}

/// A deterministic unit vector, so assertions can be exact.
fn unit(seed: u32) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    v[(seed as usize) % DIM] = 1.0;
    v
}

fn model() -> StoredModel {
    StoredModel {
        provider: "deterministic".into(),
        model: "sha256-384".into(),
        dimension: DIM,
    }
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count_rows(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<Count>(conn)
        .expect("count query")
        .count
}

#[test]
fn embedding_store_lifecycle() {
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (is the stack up?)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch database");

    let url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
            email: "alice@acme.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("inserting alice");

    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("creating the delivery board")
    .id;

    let make_task = |conn: &mut PgConnection, title: &str, content: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: board,
                column_id: None,
                title,
                content,
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("creating a task")
    };

    let one = make_task(&mut conn, "Refund rounding", "Rounded before tax.");
    let two = make_task(&mut conn, "Dark mode toggle", "On the settings page.");

    // ---- nothing embedded yet ---------------------------------------------
    let m = model();
    let c = counts(&mut conn, &m).expect("counts");
    assert_eq!(c.items, 2, "two live items");
    assert_eq!(c.embedded, 0);
    assert_eq!(c.missing(), 2, "both are behind");
    assert_eq!(c.wrong_model, 0);

    let pending = pending_primary(&mut conn, &m, 100).expect("pending");
    assert_eq!(pending.len(), 2);
    assert!(
        pending.iter().all(|p| p.stored_hash.is_none()),
        "never embedded means no stored hash"
    );
    assert!(
        pending
            .iter()
            .any(|p| p.title == "Refund rounding" && p.content == "Rounded before tax."),
        "the text to compose from comes back with the row"
    );
    assert_eq!(
        pending[0].short_code.cmp(&pending[1].short_code),
        std::cmp::Ordering::Less,
        "ordered by short_code, so a resumable backfill walks the same sequence"
    );

    // ---- store one primary vector -----------------------------------------
    store_primary(&mut conn, one.id, "task", &unit(1), &m, "hash-one").expect("store primary");
    let c = counts(&mut conn, &m).expect("counts");
    assert_eq!(c.embedded, 1);
    assert_eq!(c.missing(), 1);

    let pending = pending_primary(&mut conn, &m, 100).expect("pending");
    let stored = pending
        .iter()
        .find(|p| p.id == one.id)
        .expect("the embedded item is still listed");
    assert_eq!(
        stored.stored_hash.as_deref(),
        Some("hash-one"),
        "so the caller can compare it against what the text hashes to NOW"
    );

    // ---- re-embedding replaces rather than duplicating --------------------
    store_primary(&mut conn, one.id, "task", &unit(2), &m, "hash-one-v2").expect("re-embed");
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_embeddings"
        ),
        1,
        "upsert, not a second row"
    );
    let pending = pending_primary(&mut conn, &m, 100).expect("pending");
    assert_eq!(
        pending
            .iter()
            .find(|p| p.id == one.id)
            .unwrap()
            .stored_hash
            .as_deref(),
        Some("hash-one-v2"),
        "the hash moved with the vector"
    );

    // ---- a vector that lies about its width is refused --------------------
    let err = store_primary(&mut conn, two.id, "task", &[0.0, 1.0], &m, "short")
        .expect_err("a 2-wide vector declared as 384 must be refused");
    assert!(matches!(
        err,
        EmbeddingError::DimensionMismatch {
            got: 2,
            declared: 384
        }
    ));
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_embeddings"
        ),
        1,
        "and nothing was written"
    );

    // ---- chunks -----------------------------------------------------------
    let v0 = unit(10);
    let v1 = unit(11);
    let v2 = unit(12);
    let chunks = vec![
        ChunkWrite {
            ordinal: 0,
            heading: Some("Objective"),
            char_start: 0,
            char_end: 10,
            text: "first part",
            vector: &v0,
            content_hash: "c0",
        },
        ChunkWrite {
            ordinal: 1,
            heading: None,
            char_start: 10,
            char_end: 21,
            text: "second part",
            vector: &v1,
            content_hash: "c1",
        },
        ChunkWrite {
            ordinal: 2,
            heading: Some("Status Updates"),
            char_start: 21,
            char_end: 31,
            text: "third part",
            vector: &v2,
            content_hash: "c2",
        },
    ];
    replace_chunks(&mut conn, one.id, "task", &chunks, &m).expect("storing chunks");
    assert_eq!(
        stored_chunk_hashes(&mut conn, one.id, &m).expect("hashes"),
        vec![
            (0, "c0".to_string()),
            (1, "c1".to_string()),
            (2, "c2".to_string())
        ],
        "ordered by ordinal, so a caller can compare position for position"
    );
    // A heading is nullable on purpose: a sliding-window chunk has none.
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_chunks WHERE heading IS NULL"
        ),
        1
    );

    // ---- a shortened document must not leave orphans ----------------------
    // This is why chunks are replaced rather than upserted: ordinals are
    // positions, not identities, so an upsert keyed on (item_id, ordinal) would
    // leave the old tail behind, still matching queries, forever.
    let shorter = vec![ChunkWrite {
        ordinal: 0,
        heading: Some("Objective"),
        char_start: 0,
        char_end: 12,
        text: "rewritten",
        vector: &v0,
        content_hash: "c0-v2",
    }];
    replace_chunks(&mut conn, one.id, "task", &shorter, &m).expect("replacing chunks");
    assert_eq!(
        stored_chunk_hashes(&mut conn, one.id, &m).expect("hashes"),
        vec![(0, "c0-v2".to_string())],
        "the tail is gone, not orphaned"
    );

    // ---- pgvector really stored vectors, not text that looks like them ----
    let distance = count_rows(
        &mut conn,
        "SELECT count(*)::bigint AS count FROM item_embeddings \
         WHERE (embedding <=> embedding) < 0.000001",
    );
    assert_eq!(
        distance, 1,
        "the cosine operator works on the stored column"
    );

    // ---- a model change is visible, not silent ----------------------------
    let other = StoredModel {
        provider: "local".into(),
        model: "bge-small-en-v1.5-q".into(),
        dimension: DIM,
    };
    let c = counts(&mut conn, &other).expect("counts under a different model");
    assert_eq!(
        c.embedded, 0,
        "vectors from another model do not count as embedded"
    );
    assert_eq!(
        c.wrong_model, 1,
        "they count as wrong-model, which is what an operator needs to see"
    );
    let pending = pending_primary(&mut conn, &other, 100).expect("pending");
    assert!(
        pending.iter().all(|p| p.stored_hash.is_none()),
        "and every item reads as never embedded under the new model, so a \
         backfill re-does them rather than trusting a hash from another space"
    );

    // ---- archived items keep their vectors --------------------------------
    // KAIROS-A-0020 made put-away work searchable, and prior art in completed
    // work is one of the three claims retrieval exists to make.
    items::soft_delete_item(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        one.id,
        alice,
    )
    .expect("archiving");
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_embeddings"
        ),
        1,
        "archiving does not drop the vector"
    );
    let c = counts(&mut conn, &m).expect("counts after archiving");
    assert_eq!(
        c.items, 1,
        "but the archived item is not counted as live work still to do"
    );

    // ---- forgetting an item removes both tables ---------------------------
    forget_item(&mut conn, one.id).expect("forget");
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_embeddings"
        ),
        0
    );
    assert_eq!(
        count_rows(
            &mut conn,
            "SELECT count(*)::bigint AS count FROM item_chunks"
        ),
        0
    );
    // Idempotent: a delete path that runs twice must not fail.
    forget_item(&mut conn, one.id).expect("forget again");
}
