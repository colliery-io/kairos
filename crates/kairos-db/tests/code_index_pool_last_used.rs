//! KAIROS-T-0378 — the migration `code_index_summaries_last_used`, and the
//! pool functions on it.
//!
//! Proves: a row of the pool from before the migration gets a
//! `last_used_at` (each such row was a row of a stored index); the up.sql
//! again does not mark a new row; a summary that a build adds as it is made
//! has none; a stored index that uses it marks it; the retention delete
//! removes only the rows that no stored index uses and that are older than
//! the setting (KAIROS-T-0638: also a row whose last use is older than
//! every stored index); the down.sql removes the column, and the up.sql runs after it.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`); owns
//! the scratch database `kairos_code_index_pool_t0378_test`.

#[path = "common/scratch.rs"]
mod scratch;

use chrono::{Duration, Utc};
use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use uuid::Uuid;

use kairos_core::retention::RetentionConfig;
use kairos_db::code_indexes::{self, MadeSummary, NewCodeIndex, PoolRow};
use kairos_db::{provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
static SCRATCH_DB: scratch::ScratchName =
    scratch::ScratchName::new("kairos_code_index_pool_t0378_test");
const UP_SQL: &str =
    include_str!("../migrations/tenant/2026-10-12-000005_code_index_summaries_last_used/up.sql");
const DOWN_SQL: &str =
    include_str!("../migrations/tenant/2026-10-12-000005_code_index_summaries_last_used/down.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .expect("count")
        .count
}

fn unused(conn: &mut PgConnection) -> i64 {
    count(
        conn,
        "SELECT count(*) AS count FROM code_index_summaries WHERE last_used_at IS NULL",
    )
}

fn made(key: &str) -> MadeSummary {
    MadeSummary {
        key: key.into(),
        level: "symbol".into(),
        summary: format!("The summary of {key}."),
        model: "fake/fixed".into(),
    }
}

#[test]
fn the_pool_records_which_rows_an_index_used() {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch db");
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let mut conn = PgConnection::establish(&with_database(&admin_url, &SCRATCH_DB))
        .expect("connecting to scratch db");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    conn.batch_execute("SET search_path TO org_acme, public")
        .unwrap();
    let repo = Uuid::new_v4();

    // A row from before the migration: down.sql, a row, up.sql.
    conn.batch_execute(DOWN_SQL).expect("down.sql");
    sql_query(
        "INSERT INTO code_index_summaries (repository_id, key, level, summary, model) \
         VALUES ($1, 'old', 'symbol', 'Old.', 'fake/fixed')",
    )
    .bind::<diesel::sql_types::Uuid, _>(repo)
    .execute(&mut conn)
    .expect("an old row");
    conn.batch_execute(UP_SQL).expect("up.sql");
    assert_eq!(unused(&mut conn), 0, "the old row is in use");

    // A summary that a build makes: no index used it. The up.sql again does
    // not mark it.
    let added = code_indexes::add_made(&mut conn, repo, &[made("a"), made("b"), made("old")])
        .expect("add made");
    assert_eq!(added, 2, "a key that is there keeps its row");
    conn.batch_execute(UP_SQL).expect("up.sql again");
    assert_eq!(unused(&mut conn), 2);

    // A stored index that uses "a" marks it.
    let row = PoolRow {
        key: "a".into(),
        level: "symbol".into(),
        summary: "Another text.".into(),
        vector: Some(vec![0, 0, 128, 63]),
        model: "fake/fixed".into(),
    };
    code_indexes::put(
        &mut conn,
        &NewCodeIndex {
            repository_id: repo,
            commit_sha: "a".repeat(40),
            ref_name: Some("main".into()),
            source: "build",
            structure: vec![1, 2, 3],
            structure_bytes: 3,
            summary_keys: 1,
            vector_model: Some("det/m/1".into()),
            created_by: None,
            citations: Vec::new(),
        },
        &[row],
        &["a".to_string()],
    )
    .expect("put");
    assert_eq!(unused(&mut conn), 1, "only b is unused");
    let pool = code_indexes::whole_pool(&mut conn, repo).unwrap();
    let a = pool.iter().find(|r| r.key == "a").unwrap();
    assert_eq!(a.summary, "The summary of a.", "the made summary stays");
    assert_eq!(a.vector, Some(vec![0, 0, 128, 63]), "it gets the vector");

    // The retention delete: not before the setting, then only b.
    conn.batch_execute("SET search_path TO public").unwrap();
    let config = RetentionConfig::default();
    assert_eq!(config.code_index_pool_days, 30);
    let soon = Utc::now() + Duration::days(29);
    assert_eq!(
        kairos_db::retention::delete_unused_summaries(&mut conn, &config, soon).unwrap(),
        0
    );
    let later = Utc::now() + Duration::days(31);
    let off = RetentionConfig {
        code_index_pool_days: 0,
        ..RetentionConfig::default()
    };
    assert_eq!(
        kairos_db::retention::delete_unused_summaries(&mut conn, &off, later).unwrap(),
        0,
        "0 keeps them"
    );
    // KAIROS-T-0638: "old" goes too. Its last use is older than the only
    // stored index, and that index does not use it.
    assert_eq!(
        kairos_db::retention::delete_unused_summaries(&mut conn, &config, later).unwrap(),
        2
    );
    conn.batch_execute("SET search_path TO org_acme, public")
        .unwrap();
    let keys: Vec<String> = code_indexes::whole_pool(&mut conn, repo)
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect();
    assert_eq!(
        keys,
        vec!["a".to_string()],
        "the row that the index uses stays"
    );

    // The down.sql removes the column, and the up.sql runs after it.
    conn.batch_execute(DOWN_SQL).expect("down.sql");
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS count FROM information_schema.columns \
             WHERE table_schema = 'org_acme' AND table_name = 'code_index_summaries' \
               AND column_name = 'last_used_at'"
        ),
        0
    );
    conn.batch_execute(UP_SQL).expect("up.sql after down.sql");
    assert_eq!(unused(&mut conn), 0);

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db");
}
