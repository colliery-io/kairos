//! Integration test for the retention sweeper (KAIROS-T-0015, contract per
//! the KAIROS-A-0004 "History Retention" amendment, env surface per
//! KAIROS-A-0013).
//!
//! Runs against the real compose Postgres (`angreal services up`); the
//! database is never mocked (KAIROS-A-0012). For isolation the test drops
//! and recreates a dedicated scratch database (`kairos_retention_test`) on
//! the same server. Every sweep uses an INJECTED clock — timestamps are
//! fabricated by updating `item_history.edited_at` / `activity_log.
//! occurred_at` directly, and `now` is a parameter; nothing reads the wall
//! clock.
//!
//! Covered, on a freshly provisioned tenant:
//! - hot-window rows untouched; past-window months thinned to first+last
//!   per item per calendar month; the latest 5 versions per item retained
//!   regardless of age; single-version items never pruned
//! - `archive` mode + NO target: prunes NOTHING, reports a warning per
//!   affected table (and still writes the audit row)
//! - S3 target: typed `ArchiveTargetNotImplemented` error, nothing changed
//! - `archive` + filesystem target: NDJSON offload
//!   (`{target}/{tenant}/{table}/{timestamp}.ndjson`) written before the
//!   prune, contents full-fidelity-matching the pruned rows
//! - `activity_log` rows past the retention window archived-then-deleted
//!   (no compaction tiers); recent rows kept
//! - every sweep writes a `retention_sweep` activity row with per-table
//!   counts; sweeps are idempotent (second run prunes nothing)
//! - `discard` prunes without archive files; `off` is a full no-op
//! - `sweep_all_tenants` iterates every provisioned org (fleet pattern)
//! - `spawn_retention_loop` ticks until its closure breaks (manual-tick
//!   scheduler test, no database)

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Duration, TimeZone, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::retention::{ArchiveTarget, RetentionConfig, RetentionMode};
use kairos_core::short_code::ItemType;
use kairos_db::items::{self, ContentUpdate, CreateStrategy};
use kairos_db::models::{ItemHistory, NewUser, User};
use kairos_db::retention::{
    RetentionError, SweepReport, TableCounts, spawn_retention_loop, sweep_all_tenants, sweep_tenant,
};
use kairos_db::{provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_retention_test";

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

/// Pin the connection's `search_path` to a tenant schema (+ public).
/// `sweep_tenant` resets `search_path` to DEFAULT when it finishes, so the
/// test re-pins before every block of direct table assertions.
fn pin(conn: &mut PgConnection, schema: &str) {
    sql_query(format!("SET search_path TO {schema}, public"))
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

fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

/// Create a strategy with `versions` content versions (create + updates).
fn seed_strategy(
    conn: &mut PgConnection,
    board: Uuid,
    name: &str,
    versions: i32,
    actor: Uuid,
) -> Uuid {
    let created = items::create_strategy(
        conn,
        CreateStrategy {
            board_id: board,
            column_id: None,
            title: name,
            content: &format!("{name} v1"),
            hypothesis: None,
        },
        actor,
    )
    .unwrap_or_else(|e| panic!("creating {name}: {e}"));
    for v in 1..versions {
        items::update_item_content(
            conn,
            ItemType::Strategy,
            created.id,
            ContentUpdate {
                new_title: None,
                new_content: &format!("{name} v{}", v + 1),
                expected_version: v,
            },
            actor,
        )
        .unwrap_or_else(|e| panic!("updating {name} to v{}: {e}", v + 1));
    }
    created.id
}

/// Fabricate a snapshot's age (the KAIROS-T-0015 injected-clock seeding:
/// UPDATE `item_history.edited_at` directly).
fn backdate_history(conn: &mut PgConnection, item: Uuid, version: i32, ts: DateTime<Utc>) {
    let n = diesel::update(
        schema::item_history::table
            .filter(schema::item_history::item_id.eq(item))
            .filter(schema::item_history::version.eq(version)),
    )
    .set(schema::item_history::edited_at.eq(ts))
    .execute(conn)
    .expect("backdating item_history row");
    assert_eq!(n, 1, "expected exactly one history row for v{version}");
}

/// Remaining history versions of an item, ascending.
fn history_versions(conn: &mut PgConnection, item: Uuid) -> Vec<i32> {
    schema::item_history::table
        .filter(schema::item_history::item_id.eq(item))
        .order(schema::item_history::version.asc())
        .select(schema::item_history::version)
        .load(conn)
        .expect("loading item_history versions")
}

fn total_history_rows(conn: &mut PgConnection) -> i64 {
    schema::item_history::table
        .count()
        .get_result(conn)
        .expect("counting item_history")
}

/// `retention_sweep` audit rows' details, in `occurred_at` order.
fn sweep_details(conn: &mut PgConnection) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq("retention_sweep"))
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading retention_sweep rows")
}

fn old_activity_count(conn: &mut PgConnection, cutoff: DateTime<Utc>) -> i64 {
    schema::activity_log::table
        .filter(schema::activity_log::occurred_at.lt(cutoff))
        .count()
        .get_result(conn)
        .expect("counting old activity rows")
}

fn counts(archived: u64, pruned: u64, warnings: u64) -> TableCounts {
    TableCounts {
        rows_archived: archived,
        rows_pruned: pruned,
        warnings,
    }
}

/// Parse an NDJSON archive file into one `serde_json::Value` per line.
fn read_ndjson(path: &std::path::Path) -> Vec<serde_json::Value> {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading archive {}: {e}", path.display()));
    assert!(raw.ends_with('\n'), "NDJSON file ends with a newline");
    raw.lines()
        .map(|line| serde_json::from_str(line).expect("archive line parses as JSON"))
        .collect()
}

fn json_ts(value: &serde_json::Value, key: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value[key].as_str().unwrap_or_else(|| {
        panic!("archive row field {key} is a string: {value}");
    }))
    .expect("archive timestamp parses as RFC 3339")
    .with_timezone(&Utc)
}

#[test]
fn retention_sweeper_lifecycle() {
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
    pin(&mut conn, "org_acme");

    let alice = insert_user(&mut conn, "dex|alice", "alice@acme.test", "Alice");
    let strategy_board = board_id_by_slug(&mut conn, "strategy");

    // ---- seed: three items with fabricated timestamps ----------------------
    // Injected sweep clocks (never the wall clock).
    let now1 = at(2026, 7, 15, 11, 58, 0); // archive, no target
    let now2 = at(2026, 7, 15, 12, 0, 0); // archive, filesystem
    let now3 = at(2026, 7, 15, 13, 0, 0); // idempotency re-run
    let now4 = at(2026, 7, 15, 14, 0, 0); // fleet discard sweep
    // With hot_days=90 the now2 cutoff is 2026-04-16T12:00:00Z; with
    // activity_retention_days=365 the activity cutoff is 2025-07-15.

    // Item A: 12 versions. Jan(v1..v4), Feb(v5..v7), Mar(v8..v10) are past
    // the window; v11, v12 are hot. Latest-5 = v8..v12 (guards v9 even
    // though it is a March middle). Expected prunes: v2, v3 (Jan middles),
    // v6 (Feb middle).
    let item_a = seed_strategy(&mut conn, strategy_board, "Item A", 12, alice);
    let a_times = [
        at(2026, 1, 5, 8, 0, 0),
        at(2026, 1, 10, 8, 0, 0),
        at(2026, 1, 20, 8, 0, 0),
        at(2026, 1, 28, 8, 0, 0),
        at(2026, 2, 3, 8, 0, 0),
        at(2026, 2, 14, 8, 0, 0),
        at(2026, 2, 25, 8, 0, 0),
        at(2026, 3, 5, 8, 0, 0),
        at(2026, 3, 12, 8, 0, 0),
        at(2026, 3, 20, 8, 0, 0),
        at(2026, 5, 1, 8, 0, 0),  // hot
        at(2026, 7, 10, 8, 0, 0), // hot
    ];
    for (i, ts) in a_times.iter().enumerate() {
        backdate_history(&mut conn, item_a, (i + 1) as i32, *ts);
    }

    // Item B: a single ancient version — never pruned (its month's first
    // AND last snapshot, and within latest-5).
    let item_b = seed_strategy(&mut conn, strategy_board, "Item B", 1, alice);
    backdate_history(&mut conn, item_b, 1, at(2026, 1, 15, 8, 0, 0));

    // Item C: 8 versions, ALL ancient, one month. Compaction alone would
    // prune v2..v7; latest-5 (v4..v8) overrides, leaving v2, v3.
    let item_c = seed_strategy(&mut conn, strategy_board, "Item C", 8, alice);
    for v in 1..=8 {
        backdate_history(&mut conn, item_c, v, at(2025, 11, v as u32, 9, 0, 0));
    }

    // Three activity rows past the 365-day window (creates stay recent).
    let mut old_activity_ids = Vec::new();
    for i in 0..3 {
        let id: Uuid = diesel::insert_into(schema::activity_log::table)
            .values((
                schema::activity_log::actor_id.eq(alice),
                schema::activity_log::action.eq("transition"),
                schema::activity_log::details.eq(format!("seed:old-activity-{i}")),
                schema::activity_log::occurred_at.eq(at(2025, 5, 1, 10, 0, i)),
            ))
            .returning(schema::activity_log::id)
            .get_result(&mut conn)
            .expect("seeding old activity row");
        old_activity_ids.push(id);
    }

    assert_eq!(total_history_rows(&mut conn), 21, "12 + 1 + 8 seeded");
    let activity_cutoff = now2 - Duration::days(365);
    assert_eq!(old_activity_count(&mut conn, activity_cutoff), 3);

    // The exact rows the planner must select, loaded BEFORE any sweep, in
    // the sweeper's archive order (item_id, version).
    let expected_prune_keys: Vec<(Uuid, i32)> = vec![
        (item_a, 2),
        (item_a, 3),
        (item_a, 6),
        (item_c, 2),
        (item_c, 3),
    ];
    let mut expected_pruned: Vec<ItemHistory> = schema::item_history::table
        .select(ItemHistory::as_select())
        .load(&mut conn)
        .expect("loading history rows")
        .into_iter()
        .filter(|r| expected_prune_keys.contains(&(r.item_id, r.version)))
        .collect();
    expected_pruned.sort_by_key(|r| (r.item_id, r.version));
    assert_eq!(expected_pruned.len(), 5);

    let expected_old_activity: Vec<(Uuid, String, DateTime<Utc>)> = schema::activity_log::table
        .filter(schema::activity_log::id.eq_any(&old_activity_ids))
        .order(schema::activity_log::occurred_at.asc())
        .select((
            schema::activity_log::id,
            schema::activity_log::details,
            schema::activity_log::occurred_at,
        ))
        .load(&mut conn)
        .expect("loading seeded old activity rows");

    // ---- archive mode with NO target: prune NOTHING, warn ------------------
    let no_target = RetentionConfig {
        mode: RetentionMode::Archive,
        archive_target: None,
        ..RetentionConfig::default()
    };
    let report = sweep_tenant(&mut conn, "acme", &no_target, now1).expect("sweep (no target)");
    assert_eq!(report.item_history, counts(0, 0, 1), "warning metered");
    assert_eq!(report.activity_log, counts(0, 0, 1), "warning metered");
    assert!(report.archive_files.is_empty());

    pin(&mut conn, "org_acme");
    assert_eq!(total_history_rows(&mut conn), 21, "nothing deleted");
    assert_eq!(old_activity_count(&mut conn, activity_cutoff), 3);
    assert_eq!(
        sweep_details(&mut conn),
        vec![
            "mode:archive item_history:{archived:0,pruned:0,warnings:1} \
             activity_log:{archived:0,pruned:0,warnings:1}"
        ],
        "the paused sweep still writes its audit row"
    );

    // ---- S3 target: recognized, typed NOT IMPLEMENTED ----------------------
    let s3 = RetentionConfig {
        mode: RetentionMode::Archive,
        archive_target: Some(ArchiveTarget::S3("s3://kairos-archive/acme".into())),
        ..RetentionConfig::default()
    };
    let err = sweep_tenant(&mut conn, "acme", &s3, now1).expect_err("s3 must not be implemented");
    assert!(
        matches!(err, RetentionError::ArchiveTargetNotImplemented(ref url) if url == "s3://kairos-archive/acme"),
        "unexpected error: {err}"
    );
    pin(&mut conn, "org_acme");
    assert_eq!(
        total_history_rows(&mut conn),
        21,
        "s3 error deleted nothing"
    );
    assert_eq!(sweep_details(&mut conn).len(), 1, "no audit row on error");

    // ---- archive mode with filesystem target: offload then prune -----------
    let archive_base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kairos_retention_archive");
    if archive_base.exists() {
        std::fs::remove_dir_all(&archive_base).expect("cleaning archive dir");
    }
    let fs_config = RetentionConfig {
        mode: RetentionMode::Archive,
        archive_target: Some(ArchiveTarget::Filesystem(archive_base.clone())),
        ..RetentionConfig::default()
    };
    let report = sweep_tenant(&mut conn, "acme", &fs_config, now2).expect("sweep (filesystem)");
    assert_eq!(report.item_history, counts(5, 5, 0));
    assert_eq!(report.activity_log, counts(3, 3, 0));
    assert_eq!(report.archive_files.len(), 2);
    assert_eq!(
        report.archive_files[0].parent().unwrap(),
        archive_base.join("acme").join("item_history"),
        "layout is {{target}}/{{tenant}}/{{table}}/{{timestamp}}.ndjson"
    );
    assert_eq!(
        report.archive_files[1].parent().unwrap(),
        archive_base.join("acme").join("activity_log"),
    );

    // Tier proof: hot rows untouched, old months first+last, latest-5 kept.
    pin(&mut conn, "org_acme");
    assert_eq!(
        history_versions(&mut conn, item_a),
        vec![1, 4, 5, 7, 8, 9, 10, 11, 12],
        "A: Jan/Feb thinned to first+last; v9 guarded by latest-5; v11/v12 hot"
    );
    assert_eq!(history_versions(&mut conn, item_b), vec![1], "B untouched");
    assert_eq!(
        history_versions(&mut conn, item_c),
        vec![1, 4, 5, 6, 7, 8],
        "C: first+last of the month plus the guarded latest-5"
    );
    assert_eq!(total_history_rows(&mut conn), 16);

    // Offload fidelity: the NDJSON rows ARE the pruned rows, every column.
    let archived = read_ndjson(&report.archive_files[0]);
    assert_eq!(archived.len(), expected_pruned.len());
    for (value, row) in archived.iter().zip(&expected_pruned) {
        assert_eq!(
            value.as_object().unwrap().len(),
            7,
            "full column fidelity: id, item_id, version, title, content, edited_by, edited_at"
        );
        assert_eq!(value["id"].as_str().unwrap(), row.id.to_string());
        assert_eq!(value["item_id"].as_str().unwrap(), row.item_id.to_string());
        assert_eq!(value["version"].as_i64().unwrap(), i64::from(row.version));
        assert_eq!(value["title"].as_str().unwrap(), row.title);
        assert_eq!(value["content"].as_str().unwrap(), row.content);
        assert_eq!(
            value["edited_by"].as_str().unwrap(),
            row.edited_by.to_string()
        );
        assert_eq!(json_ts(value, "edited_at"), row.edited_at);
    }

    // activity_log rows past the window: archived then deleted.
    let archived_activity = read_ndjson(&report.archive_files[1]);
    assert_eq!(archived_activity.len(), 3);
    for (value, (id, details, occurred_at)) in archived_activity.iter().zip(&expected_old_activity)
    {
        assert_eq!(value.as_object().unwrap().len(), 7);
        assert_eq!(value["id"].as_str().unwrap(), id.to_string());
        assert_eq!(value["actor_id"].as_str().unwrap(), alice.to_string());
        assert_eq!(value["action"].as_str().unwrap(), "transition");
        assert!(value["entity_id"].is_null());
        assert!(value["entity_type"].is_null());
        assert_eq!(value["details"].as_str().unwrap(), details);
        assert_eq!(json_ts(value, "occurred_at"), *occurred_at);
    }
    assert_eq!(
        old_activity_count(&mut conn, activity_cutoff),
        0,
        "old activity rows deleted"
    );
    let remaining_creates: i64 = schema::activity_log::table
        .filter(schema::activity_log::action.eq("create"))
        .count()
        .get_result(&mut conn)
        .unwrap();
    assert_eq!(remaining_creates, 3, "recent activity rows kept");
    assert_eq!(
        sweep_details(&mut conn).last().unwrap(),
        "mode:archive item_history:{archived:5,pruned:5,warnings:0} \
         activity_log:{archived:3,pruned:3,warnings:0}"
    );

    // ---- idempotency: an immediate re-run prunes nothing --------------------
    let report = sweep_tenant(&mut conn, "acme", &fs_config, now3).expect("re-sweep");
    assert_eq!(report.item_history, counts(0, 0, 0));
    assert_eq!(report.activity_log, counts(0, 0, 0));
    assert!(report.archive_files.is_empty(), "no empty archive files");
    pin(&mut conn, "org_acme");
    assert_eq!(total_history_rows(&mut conn), 16);
    assert_eq!(sweep_details(&mut conn).len(), 3);

    // ---- off: full no-op ----------------------------------------------------
    let off = RetentionConfig {
        mode: RetentionMode::Off,
        archive_target: Some(ArchiveTarget::Filesystem(archive_base.clone())),
        history_hot_days: 0, // even a maximally aggressive policy is inert
        activity_retention_days: 0,
        ..RetentionConfig::default()
    };
    let report = sweep_tenant(&mut conn, "acme", &off, now3).expect("sweep (off)");
    assert_eq!(
        report,
        SweepReport {
            slug: "acme".to_string(),
            mode: RetentionMode::Off,
            item_history: counts(0, 0, 0),
            activity_log: counts(0, 0, 0),
            archive_files: vec![],
        }
    );
    pin(&mut conn, "org_acme");
    assert_eq!(total_history_rows(&mut conn), 16, "off deletes nothing");
    assert_eq!(
        sweep_details(&mut conn).len(),
        3,
        "off writes no retention_sweep row"
    );
    assert_eq!(
        sweep_all_tenants(&mut conn, &off, now3).expect("fleet off"),
        vec![],
        "fleet sweep in off mode is a no-op"
    );

    // ---- discard + fleet sweep ----------------------------------------------
    // Second tenant with 8 ancient snapshots of one item in one month
    // (direct inserts: item_history has no FK to the entity tables).
    provision_tenant(&mut conn, "beta", "Beta LLC").expect("provisioning beta");
    pin(&mut conn, "org_beta");
    let beta_item = Uuid::new_v4();
    for v in 1..=8_i32 {
        diesel::insert_into(schema::item_history::table)
            .values((
                schema::item_history::item_id.eq(beta_item),
                schema::item_history::version.eq(v),
                schema::item_history::title.eq("Beta item"),
                schema::item_history::content.eq(format!("beta v{v}")),
                schema::item_history::edited_by.eq(alice),
                schema::item_history::edited_at.eq(at(2025, 11, v as u32, 9, 0, 0)),
            ))
            .execute(&mut conn)
            .expect("seeding beta history");
    }

    let discard = RetentionConfig {
        mode: RetentionMode::Discard,
        archive_target: None,
        ..RetentionConfig::default()
    };
    let reports = sweep_all_tenants(&mut conn, &discard, now4).expect("fleet discard sweep");
    assert_eq!(reports.len(), 2, "one report per provisioned tenant");
    assert_eq!(reports[0].slug, "acme");
    assert_eq!(
        reports[0].item_history,
        counts(0, 0, 0),
        "acme already compacted"
    );
    assert_eq!(reports[1].slug, "beta");
    assert_eq!(
        reports[1].item_history,
        counts(0, 2, 0),
        "discard prunes v2, v3 with NOTHING archived"
    );
    assert_eq!(reports[1].activity_log, counts(0, 0, 0));
    assert!(reports[1].archive_files.is_empty());
    assert!(
        !archive_base.join("beta").exists(),
        "discard mode writes no archive files"
    );

    pin(&mut conn, "org_beta");
    assert_eq!(
        history_versions(&mut conn, beta_item),
        vec![1, 4, 5, 6, 7, 8]
    );
    assert_eq!(
        sweep_details(&mut conn),
        vec![
            "mode:discard item_history:{archived:0,pruned:2,warnings:0} \
             activity_log:{archived:0,pruned:0,warnings:0}"
        ],
        "beta's audit row records the discard counts"
    );
    pin(&mut conn, "org_acme");
    assert_eq!(
        sweep_details(&mut conn).len(),
        4,
        "acme swept by the fleet run too"
    );
}

/// The in-process scheduler (KAIROS-A-0004 "in-process scheduled task"):
/// the loop ticks its injected body until it breaks. Sweep logic itself is
/// exercised above by calling the sweep functions directly — the loop and
/// the sweep are deliberately decoupled (manual-tick testability).
#[tokio::test]
async fn retention_loop_ticks_until_break() {
    let ticks = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ticks);
    let handle = spawn_retention_loop(std::time::Duration::from_millis(1), move || {
        if counter.fetch_add(1, Ordering::SeqCst) + 1 >= 3 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    handle.await.expect("loop task joins cleanly");
    assert_eq!(
        ticks.load(Ordering::SeqCst),
        3,
        "ticked exactly until break"
    );
}
