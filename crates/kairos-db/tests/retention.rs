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
//! - `archive` mode + NO target: reads and prunes NOTHING, reports a
//!   warning per table, and the audit row records `skipped: no target`
//!   (KAIROS-T-0631)
//! - the sweep pages `item_history` by item and `activity_log` by row,
//!   with a page smaller than the fixture, and prunes exactly what the
//!   planner gives for the whole table (KAIROS-T-0631)
//! - S3 target: typed `ArchiveTargetNotImplemented` error, nothing changed
//! - `archive` + filesystem target: NDJSON offload
//!   (`{target}/{tenant}/{table}/{timestamp}.ndjson`) written before the
//!   prune, contents full-fidelity-matching the pruned rows
//! - `activity_log` rows past the retention window archived-then-deleted
//!   (no compaction tiers); recent rows kept; the rows that move an item
//!   (`transition`, `cancel`, `board_move`) kept at each age
//!   (KAIROS-T-0571)
//! - every sweep writes a `retention_sweep` activity row with per-table
//!   counts; sweeps are idempotent (second run prunes nothing)
//! - `discard` prunes without archive files; `off` is a full no-op
//! - `sweep_all_tenants` iterates every provisioned org (fleet pattern)
//! - `run_retention_pass` (the periodic job of the server, KAIROS-T-0376)
//!   removes the old rows and the expired sessions, and reports progress
//! - `team_page_history` follows the rules of `item_history`, paged by
//!   page (KAIROS-T-0638)
//! - the pass removes old code indexes, unused summaries, old unread and
//!   read notifications and old webhook deliveries in each mode, a page at
//!   a time (KAIROS-T-0638)

#[path = "common/scratch.rs"]
mod scratch;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::retention::{
    ArchiveStore, ArchiveStoreHandle, ArchiveTarget, HistoryRowMeta, RetentionConfig,
    RetentionMode, plan_history_compaction,
};
use kairos_core::short_code::ItemType;
use kairos_db::items::{self, ContentUpdate, CreateStrategy};
use kairos_db::models::{ItemHistory, NewUser, User};
use kairos_db::retention::{
    RetentionError, SweepPaging, SweepReport, SweepSkip, TableCounts, sweep_all_tenants,
    sweep_tenant, sweep_tenant_with,
};
use kairos_db::{provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_retention_test");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
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
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);

    let scratch_url = with_database(&admin_url, &SCRATCH_DB);
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
                schema::activity_log::action.eq("update"),
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
        notifications_read_days: 30,
        code_index_pool_days: 30,
        archive_target: None,
        ..RetentionConfig::default()
    };
    let report = sweep_tenant(&mut conn, "acme", &no_target, now1).expect("sweep (no target)");
    assert_eq!(report.item_history, counts(0, 0, 1), "warning metered");
    assert_eq!(report.activity_log, counts(0, 0, 1), "warning metered");
    assert!(report.archive_files.is_empty());
    assert_eq!(report.skipped, Some(SweepSkip::NoTarget));
    assert_eq!(
        (report.history_pages, report.activity_pages),
        (0, 0),
        "a skipped sweep reads no page"
    );

    pin(&mut conn, "org_acme");
    assert_eq!(total_history_rows(&mut conn), 21, "nothing deleted");
    assert_eq!(old_activity_count(&mut conn, activity_cutoff), 3);
    assert_eq!(
        sweep_details(&mut conn),
        vec![
            "mode:archive skipped: no target item_history:{archived:0,pruned:0,warnings:1} \
             activity_log:{archived:0,pruned:0,warnings:1} \
             team_page_history:{archived:0,pruned:0,warnings:1}"
        ],
        "the paused sweep still writes its audit row, with the skip"
    );

    // ---- S3 target: recognized, typed NOT IMPLEMENTED ----------------------
    let s3 = RetentionConfig {
        mode: RetentionMode::Archive,
        notifications_read_days: 30,
        code_index_pool_days: 30,
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
        notifications_read_days: 30,
        code_index_pool_days: 30,
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
            10,
            "full column fidelity: id, item_id, version, title, content, edited_by, edited_at, \
             agent_key_id, base_version, content_diff"
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
        // KAIROS-T-0359: no agent made these rows.
        assert!(value["agent_key_id"].is_null());
        // KAIROS-T-0651: these short texts are full rows.
        assert!(value["base_version"].is_null());
        assert!(value["content_diff"].is_null());
    }

    // activity_log rows past the window: archived then deleted.
    let archived_activity = read_ndjson(&report.archive_files[1]);
    assert_eq!(archived_activity.len(), 3);
    for (value, (id, details, occurred_at)) in archived_activity.iter().zip(&expected_old_activity)
    {
        assert_eq!(value.as_object().unwrap().len(), 8);
        assert_eq!(value["id"].as_str().unwrap(), id.to_string());
        assert_eq!(value["actor_id"].as_str().unwrap(), alice.to_string());
        assert_eq!(value["action"].as_str().unwrap(), "update");
        assert!(value["entity_id"].is_null());
        assert!(value["entity_type"].is_null());
        assert_eq!(value["details"].as_str().unwrap(), details);
        assert_eq!(json_ts(value, "occurred_at"), *occurred_at);
        assert!(value["agent_key_id"].is_null());
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
         activity_log:{archived:3,pruned:3,warnings:0} \
         team_page_history:{archived:0,pruned:0,warnings:0}"
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
        notifications_read_days: 30,
        code_index_pool_days: 30,
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
            team_page_history: counts(0, 0, 0),
            archive_files: vec![],
            skipped: None,
            history_pages: 0,
            activity_pages: 0,
            team_page_history_pages: 0,
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
        notifications_read_days: 30,
        code_index_pool_days: 30,
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
             activity_log:{archived:0,pruned:0,warnings:0} \
             team_page_history:{archived:0,pruned:0,warnings:0}"
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

/// KAIROS-T-0366: the pass of the server. Rows older than the setting go
/// and newer rows stay; expired password sessions go and live sessions
/// stay. The pass takes no lock: the job queue gives each run to one server
/// (KAIROS-T-0376). It reports progress before each tenant and at the end.
#[test]
fn retention_pass_removes_old_rows_and_expired_sessions() {
    use kairos_db::local_auth::{NewLocalSession, create_session};
    use kairos_db::retention::run_retention_pass;

    static PASS_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_retention_pass_test");
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (`angreal services up`)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {PASS_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {PASS_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&PASS_DB);
    let url = with_database(&admin_url, &PASS_DB);
    let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");

    let now = at(2026, 10, 11, 12, 0, 0);
    let config = RetentionConfig {
        activity_retention_days: 30,
        mode: RetentionMode::Discard,
        notifications_read_days: 30,
        code_index_pool_days: 30,
        ..RetentionConfig::default()
    };

    // activity_log: one row older than the 30-day setting, one newer.
    pin(&mut conn, "org_acme");
    for (action, ts) in [
        ("old_row", now - Duration::days(31)),
        ("new_row", now - Duration::days(29)),
        // KAIROS-T-0571: the rows that move an item stay at each age.
        ("transition", now - Duration::days(400)),
        ("cancel", now - Duration::days(400)),
        ("board_move", now - Duration::days(400)),
    ] {
        diesel::insert_into(schema::activity_log::table)
            .values((
                schema::activity_log::actor_id.eq(Uuid::nil()),
                schema::activity_log::action.eq(action),
                schema::activity_log::details.eq(""),
                schema::activity_log::occurred_at.eq(ts),
            ))
            .execute(&mut conn)
            .expect("inserting activity row");
    }

    // local_sessions: one expired before `now`, one live after it.
    let robin = insert_user(&mut conn, "local|robin", "robin@acme.test", "Robin");
    for (hash, expires_at) in [
        ("expired", now - Duration::hours(1)),
        ("live", now + Duration::hours(1)),
    ] {
        create_session(
            &mut conn,
            NewLocalSession {
                user_id: robin,
                token_hash: hash.to_string(),
                expires_at,
            },
        )
        .expect("inserting session");
    }
    sql_query("SET search_path TO DEFAULT")
        .execute(&mut conn)
        .expect("resetting search_path");

    let session_hashes = |conn: &mut PgConnection| -> Vec<String> {
        schema::local_sessions::table
            .order(schema::local_sessions::token_hash.asc())
            .select(schema::local_sessions::token_hash)
            .load(conn)
            .expect("loading sessions")
    };
    let activity_actions = |conn: &mut PgConnection| -> Vec<String> {
        pin(conn, "org_acme");
        let actions = schema::activity_log::table
            .filter(schema::activity_log::action.ne("retention_sweep"))
            .order(schema::activity_log::action.asc())
            .select(schema::activity_log::action)
            .load(conn)
            .expect("loading activity");
        sql_query("SET search_path TO DEFAULT")
            .execute(conn)
            .expect("resetting search_path");
        actions
    };

    // KAIROS-T-0377: one row of the login throttle that can change no
    // decision, and one that can.
    sql_query(
        "INSERT INTO public.auth_failures \
             (kind, subject, failures, window_started, expires_at) \
         VALUES ('source', 'old', 1, $1, $1), ('source', 'new', 1, $2, $2)",
    )
    .bind::<diesel::sql_types::Timestamptz, _>(now - Duration::hours(1))
    .bind::<diesel::sql_types::Timestamptz, _>(now + Duration::hours(1))
    .execute(&mut conn)
    .expect("inserting throttle rows");
    // KAIROS-T-0681: a request key past its window of 24 hours, and one in it.
    sql_query(
        "INSERT INTO org_acme.request_keys \
             (user_id, request_key, tool, fingerprint, answer, created_at) VALUES \
         (gen_random_uuid(), 'key-old-000001', 'create_item', '\\x00', 'a', $1), \
         (gen_random_uuid(), 'key-new-000001', 'create_item', '\\x00', 'b', $2)",
    )
    .bind::<diesel::sql_types::Timestamptz, _>(now - Duration::hours(25))
    .bind::<diesel::sql_types::Timestamptz, _>(now - Duration::hours(1))
    .execute(&mut conn)
    .expect("inserting request keys");
    // KAIROS-T-0694: a refusal older than 90 days, and one in the window.
    sql_query(
        "INSERT INTO org_acme.tool_refusals (tool, code, field, caller_kind, created_at) VALUES \
         ('get_item', 'NOT_FOUND', NULL, 'person', $1), \
         ('get_item', 'NOT_FOUND', 'short_code', 'agent_key', $2)",
    )
    .bind::<diesel::sql_types::Timestamptz, _>(now - Duration::days(91))
    .bind::<diesel::sql_types::Timestamptz, _>(now - Duration::days(89))
    .execute(&mut conn)
    .expect("inserting refusals");

    let mut steps = 0;
    let pass = run_retention_pass(&mut conn, &config, now, &mut || steps += 1).expect("pass");
    assert_eq!(
        steps, 8,
        "one tenant, then after the read and the unread notifications, the code \
         indexes, the pool summaries, the webhook deliveries, the request keys and the \
         refusals"
    );
    assert_eq!(pass.request_keys_deleted, 1, "the key past 24 hours");
    assert_eq!(pass.tool_refusals_deleted, 1, "the refusal past 90 days");
    assert_eq!(pass.expired_sessions_deleted, 1);
    assert_eq!(pass.unused_summaries_deleted, 0);
    assert_eq!(pass.expired_auth_failures_deleted, 1);
    assert_eq!(pass.tenants.len(), 1);
    assert_eq!(pass.tenants[0].activity_log.rows_pruned, 1);
    assert_eq!(
        pass.result(),
        "swept 1 tenants: item_history archived 0 pruned 0, activity_log archived 0 pruned 1, \
         team_page_history archived 0 pruned 0"
    );
    assert_eq!(session_hashes(&mut conn), vec!["live"]);
    assert_eq!(
        activity_actions(&mut conn),
        vec!["board_move", "cancel", "new_row", "transition"]
    );

    // A second pass finds nothing to remove.
    let pass = run_retention_pass(&mut conn, &config, now, &mut || {}).expect("pass");
    assert_eq!(pass.expired_sessions_deleted, 0);
    assert_eq!(pass.request_keys_deleted, 0);
    assert_eq!(pass.tool_refusals_deleted, 0);
    assert_eq!(pass.tenants.len(), 1);
    assert_eq!(pass.tenants[0].activity_log.rows_pruned, 0);

    // `off` sweeps no tenant but still deletes expired sessions.
    pin(&mut conn, "org_acme");
    create_session(
        &mut conn,
        NewLocalSession {
            user_id: robin,
            token_hash: "expired-2".to_string(),
            expires_at: now - Duration::minutes(5),
        },
    )
    .expect("inserting session");
    let off = RetentionConfig {
        mode: RetentionMode::Off,
        notifications_read_days: 30,
        code_index_pool_days: 30,
        ..RetentionConfig::default()
    };
    let pass = run_retention_pass(&mut conn, &off, now, &mut || {}).expect("pass");
    assert!(pass.tenants.is_empty());
    assert_eq!(pass.expired_sessions_deleted, 1);
    assert_eq!(pass.result(), "off");

    // KAIROS-T-0631: `archive` with no target skips each sweep, and the
    // pass records it.
    let no_target = RetentionConfig {
        activity_retention_days: 0,
        ..RetentionConfig::default()
    };
    let pass = run_retention_pass(&mut conn, &no_target, now, &mut || {}).expect("pass");
    assert_eq!(pass.tenants.len(), 1);
    assert_eq!(pass.tenants[0].skipped, Some(SweepSkip::NoTarget));
    assert_eq!(pass.result(), "skipped: no target");
    assert_eq!(
        activity_actions(&mut conn),
        vec!["board_move", "cancel", "new_row", "transition"],
        "no row removed with no target"
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {PASS_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

/// KAIROS-T-0631: the sweep pages. Seven items with many versions each and
/// a page of two items, and eleven old activity rows with a page of four.
/// The sweep reads four pages of items and three pages of activity, and
/// prunes and archives exactly the rows that the planner gives for the
/// whole table, in `(item_id, version)` order. The deletes go in batches
/// of three ids.
#[test]
fn the_sweep_pages_by_item_and_prunes_what_the_planner_gives() {
    static PAGED_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_retention_paged_test");
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (`angreal services up`)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {PAGED_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {PAGED_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&PAGED_DB);
    let url = with_database(&admin_url, &PAGED_DB);
    let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    pin(&mut conn, "org_acme");
    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");

    let now = at(2026, 7, 15, 12, 0, 0);
    // Seven items; item i has 20 + i versions, two a day from 2025-11-01,
    // so each item has old months to thin and a latest-5 guard.
    let mut items = Vec::new();
    for i in 0..7_i32 {
        let item = Uuid::new_v4();
        items.push(item);
        for v in 1..=(20 + i) {
            let edited_at = at(2025, 11, 1, 0, 0, 0) + Duration::hours(12 * i64::from(v));
            diesel::insert_into(schema::item_history::table)
                .values((
                    schema::item_history::item_id.eq(item),
                    schema::item_history::version.eq(v),
                    schema::item_history::title.eq(format!("Item {i}")),
                    schema::item_history::content.eq(format!("item {i} content v{v}")),
                    schema::item_history::edited_by.eq(robin),
                    schema::item_history::edited_at.eq(edited_at),
                ))
                .execute(&mut conn)
                .expect("seeding history");
        }
    }
    // Eleven activity rows past the 365-day window, two with one time.
    for i in 0..11_u32 {
        diesel::insert_into(schema::activity_log::table)
            .values((
                schema::activity_log::actor_id.eq(robin),
                schema::activity_log::action.eq("update"),
                schema::activity_log::details.eq(format!("old-{i}")),
                schema::activity_log::occurred_at.eq(at(2025, 1, 1, 0, 0, i.min(9))),
            ))
            .execute(&mut conn)
            .expect("seeding activity");
    }
    // KAIROS-T-0571: three old rows that move an item. They stay, and no
    // page reads them.
    for (i, action) in ["transition", "cancel", "board_move"].iter().enumerate() {
        diesel::insert_into(schema::activity_log::table)
            .values((
                schema::activity_log::actor_id.eq(robin),
                schema::activity_log::action.eq(*action),
                schema::activity_log::details.eq(format!("kept-{i}")),
                schema::activity_log::occurred_at.eq(at(2024, 6, 1, 0, 0, i as u32)),
            ))
            .execute(&mut conn)
            .expect("seeding activity");
    }

    let archive_base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kairos_retention_paged");
    if archive_base.exists() {
        std::fs::remove_dir_all(&archive_base).expect("cleaning archive dir");
    }
    let config = RetentionConfig {
        archive_target: Some(ArchiveTarget::Filesystem(archive_base.clone())),
        ..RetentionConfig::default()
    };

    // The plan for the whole table, before the sweep.
    let all: Vec<ItemHistory> = schema::item_history::table
        .select(ItemHistory::as_select())
        .load(&mut conn)
        .expect("loading history");
    let meta: Vec<HistoryRowMeta> = all
        .iter()
        .map(|r| HistoryRowMeta {
            item_id: r.item_id,
            version: r.version,
            edited_at: r.edited_at,
            base_version: r.base_version,
        })
        .collect();
    let expected: Vec<(Uuid, i32)> = plan_history_compaction(&meta, now, &config)
        .into_iter()
        .map(|k| (k.item_id, k.version))
        .collect();
    assert!(
        expected.len() > 7,
        "the fixture gives candidates on every page: {}",
        expected.len()
    );
    let total = all.len();

    let paging = SweepPaging {
        items_per_page: 2,
        activity_per_page: 4,
        batch: 3,
    };
    let report = sweep_tenant_with(&mut conn, "acme", &config, now, paging).expect("paged sweep");
    assert_eq!(report.skipped, None);
    assert_eq!(report.history_pages, 4, "7 items in pages of 2");
    assert_eq!(report.activity_pages, 3, "11 rows in pages of 4");
    assert_eq!(
        report.item_history,
        counts(expected.len() as u64, expected.len() as u64, 0)
    );
    assert_eq!(report.activity_log, counts(11, 11, 0));

    // The archive holds the planned rows with their content, in order.
    let history_file = report
        .archive_files
        .iter()
        .find(|p| p.to_string_lossy().contains("item_history"))
        .expect("item_history archive");
    let archived: Vec<(Uuid, i32)> = read_ndjson(history_file)
        .iter()
        .map(|v| {
            let item: Uuid = v["item_id"].as_str().unwrap().parse().unwrap();
            let version = v["version"].as_i64().unwrap() as i32;
            let i = items.iter().position(|x| *x == item).unwrap();
            assert_eq!(
                v["content"].as_str().unwrap(),
                format!("item {i} content v{version}"),
                "an archived row carries its content"
            );
            (item, version)
        })
        .collect();
    let mut sorted_expected = expected.clone();
    sorted_expected.sort();
    assert_eq!(
        archived, sorted_expected,
        "archive order is (item_id, version)"
    );

    let activity_file = report
        .archive_files
        .iter()
        .find(|p| p.to_string_lossy().contains("activity_log"))
        .expect("activity_log archive");
    let details: Vec<String> = read_ndjson(activity_file)
        .iter()
        .map(|v| v["details"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(details.len(), 11, "each old row once, across the pages");
    let mut unique = details.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 11, "no row twice at a page boundary");

    // The rows that stay are the rows the planner kept.
    pin(&mut conn, "org_acme");
    assert_eq!(
        total_history_rows(&mut conn) as usize,
        total - expected.len()
    );
    let left: Vec<(Uuid, i32)> = schema::item_history::table
        .select((schema::item_history::item_id, schema::item_history::version))
        .load(&mut conn)
        .expect("loading history");
    assert!(left.iter().all(|k| !expected.contains(k)));
    let kept: Vec<String> = schema::activity_log::table
        .filter(schema::activity_log::occurred_at.lt(now - Duration::days(365)))
        .order(schema::activity_log::action.asc())
        .select(schema::activity_log::action)
        .load(&mut conn)
        .expect("loading old activity");
    assert_eq!(
        kept,
        vec!["board_move", "cancel", "transition"],
        "the rows that move an item stay at each age"
    );

    // A second paged sweep finds nothing.
    let again = sweep_tenant_with(&mut conn, "acme", &config, now, paging).expect("re-sweep");
    assert_eq!(again.item_history, counts(0, 0, 0));
    assert_eq!(again.activity_log, counts(0, 0, 0));
    assert!(again.archive_files.is_empty());

    drop(conn);
    std::fs::remove_dir_all(&archive_base).expect("removing archive dir");
}

/// A fresh scratch database with the tenant `acme`, pinned to it.
fn fresh_acme(name: &'static scratch::ScratchName) -> (PgConnection, PgConnection) {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (`angreal services up`)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {name}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let url = with_database(&admin_url, name);
    let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    pin(&mut conn, "org_acme");
    (admin_conn, conn)
}

#[derive(diesel::QueryableByName)]
struct IdRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    id: Uuid,
}

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

fn count_where(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

/// A store that keeps what it is given in memory, or refuses it
/// (KAIROS-T-0671).
struct RecordingStore {
    objects: Mutex<Vec<(String, String)>>,
    refuse: bool,
}

impl ArchiveStore for RecordingStore {
    fn put_file(&self, key: &str, file: &std::path::Path) -> Result<(), String> {
        if self.refuse {
            return Err("the bucket refused the object".to_string());
        }
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        self.objects.lock().unwrap().push((key.to_string(), text));
        Ok(())
    }
}

/// KAIROS-T-0671: an `s3://` target. The sweep writes the archive file, the
/// store gets it under `{prefix}/{tenant}/{table}/{timestamp}.ndjson`, and
/// the rows go after that. A store that refuses the object leaves every row
/// and writes no audit row.
#[test]
fn an_s3_target_gets_the_archive_before_the_rows_go() {
    static S3_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_retention_s3_test");
    let (_admin, mut conn) = fresh_acme(&S3_DB);
    let _guard = scratch::ScratchGuard::new(&S3_DB);
    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");
    let now = at(2026, 7, 15, 12, 0, 0);
    let item = Uuid::new_v4();
    for v in 1..=12_i32 {
        diesel::insert_into(schema::item_history::table)
            .values((
                schema::item_history::item_id.eq(item),
                schema::item_history::version.eq(v),
                schema::item_history::title.eq("Item"),
                schema::item_history::content.eq(format!("content v{v}")),
                schema::item_history::edited_by.eq(robin),
                schema::item_history::edited_at
                    .eq(at(2025, 11, 1, 0, 0, 0) + Duration::hours(12 * i64::from(v))),
            ))
            .execute(&mut conn)
            .expect("seeding history");
    }
    for i in 0..3_u32 {
        diesel::insert_into(schema::activity_log::table)
            .values((
                schema::activity_log::actor_id.eq(robin),
                schema::activity_log::action.eq("update"),
                schema::activity_log::details.eq(format!("old-{i}")),
                schema::activity_log::occurred_at.eq(at(2025, 1, 1, 0, 0, i)),
            ))
            .execute(&mut conn)
            .expect("seeding activity");
    }
    let history_before = total_history_rows(&mut conn);
    let config = |store: &Arc<RecordingStore>| RetentionConfig {
        mode: RetentionMode::Archive,
        archive_target: ArchiveTarget::parse("s3://kairos-archive/pre/fix/"),
        archive_store: ArchiveStoreHandle::new(store.clone()),
        ..RetentionConfig::default()
    };

    // A store that refuses: nothing is deleted, and there is no audit row.
    let refusing = Arc::new(RecordingStore {
        objects: Mutex::new(Vec::new()),
        refuse: true,
    });
    let err = sweep_tenant(&mut conn, "acme", &config(&refusing), now)
        .expect_err("the store refuses the object");
    assert!(
        matches!(&err, RetentionError::ArchiveUpload { url, message }
            if url.starts_with("s3://kairos-archive/pre/fix/acme/item_history/")
                && message.contains("refused")),
        "{err}"
    );
    pin(&mut conn, "org_acme");
    assert_eq!(total_history_rows(&mut conn), history_before);
    assert_eq!(old_activity_count(&mut conn, now - Duration::days(365)), 3);
    assert!(sweep_details(&mut conn).is_empty(), "no audit row on error");

    // A store that takes the objects: the rows go, and the objects hold them.
    let store = Arc::new(RecordingStore {
        objects: Mutex::new(Vec::new()),
        refuse: false,
    });
    let report = sweep_tenant(&mut conn, "acme", &config(&store), now).expect("sweep to s3");
    pin(&mut conn, "org_acme");
    assert!(report.item_history.rows_pruned > 0, "{report:?}");
    assert_eq!(
        report.item_history.rows_archived,
        report.item_history.rows_pruned
    );
    assert_eq!(report.activity_log, counts(3, 3, 0));
    assert_eq!(
        total_history_rows(&mut conn),
        history_before - report.item_history.rows_pruned as i64
    );
    let objects = store.objects.lock().unwrap();
    assert_eq!(objects.len(), 2, "one object per table with rows");
    let (history_key, history_text) = &objects[0];
    assert!(
        history_key.starts_with("pre/fix/acme/item_history/") && history_key.ends_with(".ndjson"),
        "{history_key}"
    );
    assert_eq!(
        history_text.lines().count() as u64,
        report.item_history.rows_archived
    );
    assert!(
        objects[1].0.starts_with("pre/fix/acme/activity_log/"),
        "{}",
        objects[1].0
    );
    assert_eq!(
        report
            .archive_files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        objects
            .iter()
            .map(|(key, _)| format!("s3://kairos-archive/{key}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(sweep_details(&mut conn).len(), 1);
}

/// KAIROS-T-0638: `team_page_history` follows the rules of `item_history`.
/// Five pages with many versions each and a page of two team pages: the
/// sweep reads three pages, and prunes and archives exactly the rows that
/// the planner gives for the whole table. The archive names `page_id`.
/// The deletes go in batches of three ids.
#[test]
fn team_page_history_follows_the_item_history_rules() {
    static PAGES_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_retention_team_pages_test");
    let (mut admin_conn, mut conn) = fresh_acme(&PAGES_DB);
    let _scratch_guard = scratch::ScratchGuard::new(&PAGES_DB);
    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");
    let team = sql_query(
        "INSERT INTO teams (name, slug, team_type) VALUES ('Core', 'core-t0638', 'platform') \
         RETURNING id",
    )
    .get_result::<IdRow>(&mut conn)
    .expect("a team")
    .id;

    let now = at(2026, 7, 15, 12, 0, 0);
    let mut pages = Vec::new();
    for i in 0..5_i32 {
        let page = sql_query(
            "INSERT INTO team_pages (team_id, kind, slug, title, created_by, updated_by) \
             VALUES ($1, 'page', $2, $3, $4, $4) RETURNING id",
        )
        .bind::<diesel::sql_types::Uuid, _>(team)
        .bind::<diesel::sql_types::Text, _>(format!("page-{i}"))
        .bind::<diesel::sql_types::Text, _>(format!("Page {i}"))
        .bind::<diesel::sql_types::Uuid, _>(robin)
        .get_result::<IdRow>(&mut conn)
        .expect("a page")
        .id;
        pages.push(page);
        for v in 1..=(20 + i) {
            let edited_at = at(2025, 11, 1, 0, 0, 0) + Duration::hours(12 * i64::from(v));
            sql_query(
                "INSERT INTO team_page_history (page_id, version, title, content, edited_by, \
                     edited_at) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind::<diesel::sql_types::Uuid, _>(page)
            .bind::<diesel::sql_types::Integer, _>(v)
            .bind::<diesel::sql_types::Text, _>(format!("Page {i}"))
            .bind::<diesel::sql_types::Text, _>(format!("page {i} content v{v}"))
            .bind::<diesel::sql_types::Uuid, _>(robin)
            .bind::<diesel::sql_types::Timestamptz, _>(edited_at)
            .execute(&mut conn)
            .expect("seeding page history");
        }
    }
    // The whole table, with the pages that the tenant came with.
    #[derive(diesel::QueryableByName)]
    struct MetaRow {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        page_id: Uuid,
        #[diesel(sql_type = diesel::sql_types::Integer)]
        version: i32,
        #[diesel(sql_type = diesel::sql_types::Timestamptz)]
        edited_at: DateTime<Utc>,
    }
    let meta: Vec<HistoryRowMeta> =
        sql_query("SELECT page_id, version, edited_at FROM team_page_history")
            .load::<MetaRow>(&mut conn)
            .expect("loading page history")
            .into_iter()
            .map(|r| HistoryRowMeta {
                item_id: r.page_id,
                version: r.version,
                edited_at: r.edited_at,
                base_version: None,
            })
            .collect();
    let total = meta.len() as i64;
    assert!(total >= 110, "the fixture: {total}");

    let archive_base =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kairos_retention_team_pages");
    if archive_base.exists() {
        std::fs::remove_dir_all(&archive_base).expect("cleaning archive dir");
    }

    // archive with no target: the table is not read, nothing goes.
    let report = sweep_tenant_with(
        &mut conn,
        "acme",
        &RetentionConfig::default(),
        now,
        SweepPaging {
            items_per_page: 2,
            activity_per_page: 4,
            batch: 3,
        },
    )
    .expect("skipped sweep");
    assert_eq!(report.skipped, Some(SweepSkip::NoTarget));
    assert_eq!(report.team_page_history, counts(0, 0, 1));
    assert_eq!(report.team_page_history_pages, 0);
    pin(&mut conn, "org_acme");
    assert_eq!(
        count_where(&mut conn, "SELECT count(*) AS n FROM team_page_history"),
        total
    );

    let config = RetentionConfig {
        archive_target: Some(ArchiveTarget::Filesystem(archive_base.clone())),
        ..RetentionConfig::default()
    };
    let mut expected: Vec<(Uuid, i32)> = plan_history_compaction(&meta, now, &config)
        .into_iter()
        .map(|k| (k.item_id, k.version))
        .collect();
    expected.sort();
    assert!(expected.len() > 5, "candidates on every page");

    let paging = SweepPaging {
        items_per_page: 2,
        activity_per_page: 4,
        batch: 3,
    };
    let report = sweep_tenant_with(&mut conn, "acme", &config, now, paging).expect("sweep");
    let owners = count_where(
        &mut conn,
        "SELECT count(DISTINCT page_id) AS n FROM org_acme.team_page_history",
    );
    assert_eq!(
        report.team_page_history_pages as i64,
        (owners + 1) / 2,
        "the pages in pages of 2"
    );
    assert_eq!(
        report.team_page_history,
        counts(expected.len() as u64, expected.len() as u64, 0)
    );
    let file = report
        .archive_files
        .iter()
        .find(|p| p.to_string_lossy().contains("team_page_history"))
        .expect("team_page_history archive");
    let archived: Vec<(Uuid, i32)> = read_ndjson(file)
        .iter()
        .map(|v| {
            let page: Uuid = v["page_id"].as_str().unwrap().parse().unwrap();
            let version = v["version"].as_i64().unwrap() as i32;
            let i = pages.iter().position(|x| *x == page).unwrap();
            assert_eq!(
                v["content"].as_str().unwrap(),
                format!("page {i} content v{version}")
            );
            assert!(v.get("item_id").is_none(), "{v}");
            (page, version)
        })
        .collect();
    assert_eq!(archived, expected);

    pin(&mut conn, "org_acme");
    assert_eq!(
        count_where(&mut conn, "SELECT count(*) AS n FROM team_page_history"),
        total - expected.len() as i64
    );
    for page in &pages {
        let latest = count_where(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM team_page_history h WHERE page_id = '{page}' \
                 AND version > (SELECT max(version) - 5 FROM team_page_history \
                                WHERE page_id = '{page}')"
            ),
        );
        assert_eq!(latest, 5, "the latest 5 versions of each page stay");
    }
    let details = sweep_details(&mut conn);
    assert!(
        details.last().unwrap().ends_with(&format!(
            "team_page_history:{{archived:{n},pruned:{n},warnings:0}}",
            n = expected.len()
        )),
        "{details:?}"
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {PAGES_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

/// KAIROS-T-0638: the pass removes, in each mode (here `off`) and a page at
/// a time, the old code indexes (the newest 5 of each repository and ref
/// stay), the summaries that no stored index uses, the unread
/// notifications older than 180 days, the read ones older than 30 days and
/// the webhook deliveries that finished more than 30 days ago. Each
/// fixture is larger than one page of 3. A second pass removes nothing.
#[test]
fn the_pass_removes_old_rows_of_the_other_tables_a_page_at_a_time() {
    use diesel::sql_types::{Nullable, Text, Timestamptz, Uuid as SqlUuid};
    use kairos_db::retention::run_retention_pass_with;

    static OTHER_DB: scratch::ScratchName =
        scratch::ScratchName::new("kairos_retention_other_tables_test");
    let (mut admin_conn, mut conn) = fresh_acme(&OTHER_DB);
    let _scratch_guard = scratch::ScratchGuard::new(&OTHER_DB);
    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");
    let now = at(2026, 10, 11, 12, 0, 0);

    // ---- code indexes ------------------------------------------------------
    // Repository one: ten indexes of main, one a day from 60 days ago, and
    // one with no ref (40 days ago). Repository two: three of main.
    let (one, two) = (Uuid::new_v4(), Uuid::new_v4());
    let put_index =
        |conn: &mut PgConnection, repo: Uuid, n: u32, ref_name: Option<&str>, at: DateTime<Utc>| {
            sql_query(
                "INSERT INTO code_indexes (repository_id, commit_sha, ref_name, source, structure, \
                 structure_bytes, summary_keys, created_at, updated_at) \
             VALUES ($1, $2, $3, 'build', '\\x01'::bytea, 1, 0, $4, $4)",
            )
            .bind::<SqlUuid, _>(repo)
            .bind::<Text, _>(format!("{n:040x}"))
            .bind::<Nullable<Text>, _>(ref_name)
            .bind::<Timestamptz, _>(at)
            .execute(conn)
            .expect("an index");
        };
    for i in 0..10_u32 {
        put_index(
            &mut conn,
            one,
            i,
            Some("main"),
            now - Duration::days(60) + Duration::days(i64::from(i)),
        );
    }
    put_index(&mut conn, one, 100, None, now - Duration::days(40));
    for i in 0..3_u32 {
        put_index(
            &mut conn,
            two,
            200 + i,
            Some("main"),
            now - Duration::days(i64::from(i)),
        );
    }
    // The pool of repository one. The kept indexes start 55 days ago.
    let put_summary = |conn: &mut PgConnection,
                       key: &str,
                       created: DateTime<Utc>,
                       used: Option<DateTime<Utc>>| {
        sql_query(
            "INSERT INTO code_index_summaries (repository_id, key, level, summary, model, \
                 created_at, last_used_at) VALUES ($1, $2, 'symbol', 'S.', 'fake/fixed', $3, $4)",
        )
        .bind::<SqlUuid, _>(one)
        .bind::<Text, _>(key)
        .bind::<Timestamptz, _>(created)
        .bind::<Nullable<Timestamptz>, _>(used)
        .execute(conn)
        .expect("a summary");
    };
    let long_ago = now - Duration::days(90);
    for i in 0..7 {
        // Used last by an index that the pass removes.
        put_summary(
            &mut conn,
            &format!("gone-{i}"),
            long_ago,
            Some(now - Duration::days(58)),
        );
    }
    put_summary(&mut conn, "kept", long_ago, Some(now - Duration::days(51)));
    put_summary(
        &mut conn,
        "oldest-kept",
        long_ago,
        Some(now - Duration::days(55)),
    );
    put_summary(&mut conn, "never-used", long_ago, None);
    put_summary(&mut conn, "new-unused", now - Duration::days(1), None);

    // ---- notifications -----------------------------------------------------
    let notify = |conn: &mut PgConnection, created: DateTime<Utc>, read: Option<DateTime<Utc>>| {
        sql_query(
            "INSERT INTO notifications (user_id, kind, text, created_at, read_at) \
             VALUES ($1, 'mention', 'You were named.', $2, $3)",
        )
        .bind::<SqlUuid, _>(robin)
        .bind::<Timestamptz, _>(created)
        .bind::<Nullable<Timestamptz>, _>(read)
        .execute(conn)
        .expect("a notification");
    };
    for i in 0..7 {
        notify(&mut conn, now - Duration::days(181 + i), None);
        notify(
            &mut conn,
            now - Duration::days(200),
            Some(now - Duration::days(31 + i)),
        );
    }
    notify(&mut conn, now - Duration::days(179), None);
    notify(
        &mut conn,
        now - Duration::days(200),
        Some(now - Duration::days(29)),
    );

    // ---- webhook deliveries --------------------------------------------------
    let board = board_id_by_slug(&mut conn, "strategy");
    let target = sql_query(
        "INSERT INTO webhook_targets (board_id, name, url, secret_ciphertext, secret_nonce, \
             secret_key_id, created_by) \
         VALUES ($1, 'Quiet', 'https://hooks.example.test/x', '\\x00'::bytea, '\\x00'::bytea, \
             'k1', $2) RETURNING id",
    )
    .bind::<SqlUuid, _>(board)
    .bind::<SqlUuid, _>(robin)
    .get_result::<IdRow>(&mut conn)
    .expect("a target")
    .id;
    let deliver =
        |conn: &mut PgConnection, status: &str, finished: Option<DateTime<Utc>>| -> Uuid {
            sql_query(
                "INSERT INTO webhook_deliveries (target_id, event, body, status, created_at, \
                     finished_at) VALUES ($1, 'item.moved', '{}', $2, $3, $4) RETURNING id",
            )
            .bind::<SqlUuid, _>(target)
            .bind::<Text, _>(status)
            .bind::<Timestamptz, _>(now - Duration::days(90))
            .bind::<Nullable<Timestamptz>, _>(finished)
            .get_result::<IdRow>(conn)
            .expect("a delivery")
            .id
        };
    let mut old_ids = Vec::new();
    for (i, status) in ["delivered", "failed", "skipped"]
        .iter()
        .cycle()
        .take(7)
        .enumerate()
    {
        old_ids.push(deliver(
            &mut conn,
            status,
            Some(now - Duration::days(31 + i as i64)),
        ));
    }
    sql_query(
        "INSERT INTO webhook_delivery_tries (delivery_id, try, ok, duration_ms) \
         VALUES ($1, 1, true, 5)",
    )
    .bind::<SqlUuid, _>(old_ids[0])
    .execute(&mut conn)
    .expect("a try");
    let recent_skip = deliver(&mut conn, "skipped", Some(now - Duration::days(29)));
    let pending = deliver(&mut conn, "pending", None);

    sql_query("SET search_path TO DEFAULT")
        .execute(&mut conn)
        .expect("resetting search_path");

    // `off`: no sweep, but each of these tables in pages of 3.
    let config = RetentionConfig {
        mode: RetentionMode::Off,
        ..RetentionConfig::default()
    };
    let paging = SweepPaging {
        items_per_page: 1,
        activity_per_page: 3,
        batch: 3,
    };
    let mut steps = 0;
    let pass =
        run_retention_pass_with(&mut conn, &config, now, paging, &mut || steps += 1).expect("pass");
    assert!(pass.tenants.is_empty());
    assert_eq!(steps, 7, "after each table");
    assert_eq!(
        pass.code_indexes_deleted, 5,
        "main of repository one keeps 5"
    );
    assert_eq!(
        pass.unused_summaries_deleted, 8,
        "the 7 that only removed indexes used, and the old unused one"
    );
    assert_eq!(pass.unread_notifications_deleted, 7);
    assert_eq!(pass.read_notifications_deleted, 7);
    assert_eq!(pass.webhook_deliveries_deleted, 7);

    pin(&mut conn, "org_acme");
    let mut shas: Vec<String> = schema::code_indexes::table
        .filter(schema::code_indexes::repository_id.eq(one))
        .select(schema::code_indexes::commit_sha)
        .load(&mut conn)
        .expect("indexes");
    shas.sort();
    let mut want: Vec<String> = (5..10_u32)
        .chain([100])
        .map(|n| format!("{n:040x}"))
        .collect();
    want.sort();
    assert_eq!(shas, want, "the newest 5 of main, and the one with no ref");
    assert_eq!(
        count_where(
            &mut conn,
            &format!("SELECT count(*) AS n FROM code_indexes WHERE repository_id = '{two}'")
        ),
        3
    );
    let mut keys: Vec<String> = schema::code_index_summaries::table
        .select(schema::code_index_summaries::key)
        .load(&mut conn)
        .expect("summaries");
    keys.sort();
    assert_eq!(keys, vec!["kept", "new-unused", "oldest-kept"]);
    assert_eq!(
        count_where(
            &mut conn,
            "SELECT count(*) AS n FROM notifications WHERE read_at IS NULL"
        ),
        1
    );
    assert_eq!(
        count_where(
            &mut conn,
            "SELECT count(*) AS n FROM notifications WHERE read_at IS NOT NULL"
        ),
        1
    );
    let mut left: Vec<Uuid> = sql_query("SELECT id FROM webhook_deliveries")
        .load::<IdRow>(&mut conn)
        .expect("deliveries")
        .into_iter()
        .map(|r| r.id)
        .collect();
    left.sort();
    let mut want = vec![recent_skip, pending];
    want.sort();
    assert_eq!(
        left, want,
        "a skipped delivery stays 30 days; a pending one stays"
    );
    assert_eq!(
        count_where(
            &mut conn,
            "SELECT count(*) AS n FROM webhook_delivery_tries"
        ),
        0,
        "the tries go with the delivery"
    );

    // A second pass removes nothing; 0 keeps each of them.
    sql_query("SET search_path TO DEFAULT")
        .execute(&mut conn)
        .expect("resetting search_path");
    let pass = run_retention_pass_with(&mut conn, &config, now, paging, &mut || {}).expect("pass");
    assert_eq!(
        (
            pass.code_indexes_deleted,
            pass.unused_summaries_deleted,
            pass.unread_notifications_deleted,
            pass.read_notifications_deleted,
            pass.webhook_deliveries_deleted
        ),
        (0, 0, 0, 0, 0)
    );
    let keep = RetentionConfig {
        mode: RetentionMode::Off,
        code_index_keep: 0,
        notifications_unread_days: 0,
        ..RetentionConfig::default()
    };
    let later = now + Duration::days(400);
    let pass = run_retention_pass_with(&mut conn, &keep, later, paging, &mut || {}).expect("pass");
    assert_eq!(pass.code_indexes_deleted, 0, "0 keeps every index");
    assert_eq!(pass.unread_notifications_deleted, 0, "0 keeps unread ones");

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {OTHER_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

/// The text of version `v` of the diff fixture (KAIROS-T-0651): 60 lines,
/// one line changes in each version, and each version adds a line, so
/// each version after a full row is a diff row.
fn diff_text_of(v: i32) -> String {
    let mut lines: Vec<String> = (0..60)
        .map(|i| format!("Line {i}: the plan of the item, with some words to read.\n"))
        .collect();
    for u in 2..=v {
        let at = ((u * 7) % 60) as usize;
        lines[at] = format!("Line {at}: version {u} changed this line.\n");
        lines.push(format!("Step {u}: one more line for the record.\n"));
    }
    format!("# Plan\n\n{}", lines.concat())
}

/// KAIROS-T-0651: retention after the change to diff rows. One item has 25
/// versions: full rows v1, v11, v21, and diff rows between them. v1..v12
/// are in January 2025, v13..v25 in February 2025. The planner keeps the
/// first and the last of each month (v1, v12, v13, v25) and the latest 5
/// (v21..v25). v12 and v13 are diffs from v11, so v11 stays too. Each kept
/// version gives its text back, and each archived line has the full text.
#[test]
fn the_sweep_keeps_the_base_of_each_kept_diff_row() {
    static DIFF_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_retention_diff_test");
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (`angreal services up`)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {DIFF_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {DIFF_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let _scratch_guard = scratch::ScratchGuard::new(&DIFF_DB);
    let url = with_database(&admin_url, &DIFF_DB);
    let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    pin(&mut conn, "org_acme");
    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");
    let board = board_id_by_slug(&mut conn, "strategy");

    let item = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: board,
            column_id: None,
            title: "Plan",
            content: &diff_text_of(1),
            hypothesis: None,
        },
        robin,
    )
    .expect("creating the strategy")
    .id;
    for v in 2..=25 {
        items::update_item_content(
            &mut conn,
            ItemType::Strategy,
            item,
            ContentUpdate {
                new_title: None,
                new_content: &diff_text_of(v),
                expected_version: v - 1,
            },
            robin,
        )
        .unwrap_or_else(|e| panic!("editing to v{v}: {e}"));
    }
    for v in 1..=25_i32 {
        let ts = if v <= 12 {
            at(2025, 1, v as u32, 8, 0, 0)
        } else {
            at(2025, 2, (v - 12) as u32, 8, 0, 0)
        };
        backdate_history(&mut conn, item, v, ts);
    }
    let full: Vec<i32> = schema::item_history::table
        .filter(schema::item_history::item_id.eq(item))
        .filter(schema::item_history::base_version.is_null())
        .order(schema::item_history::version.asc())
        .select(schema::item_history::version)
        .load(&mut conn)
        .expect("loading the full rows");
    assert_eq!(full, vec![1, 11, 21], "the fixture has diff rows");

    let archive_base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kairos_retention_diff");
    if archive_base.exists() {
        std::fs::remove_dir_all(&archive_base).expect("cleaning archive dir");
    }
    let config = RetentionConfig {
        archive_target: Some(ArchiveTarget::Filesystem(archive_base.clone())),
        ..RetentionConfig::default()
    };
    let now = at(2026, 7, 15, 12, 0, 0);
    let report = sweep_tenant(&mut conn, "acme", &config, now).expect("sweeping");
    let pruned: Vec<i32> = (2..=10).chain(14..=20).collect();
    assert_eq!(
        report.item_history,
        counts(pruned.len() as u64, pruned.len() as u64, 0)
    );

    pin(&mut conn, "org_acme");
    let kept = history_versions(&mut conn, item);
    assert_eq!(kept, vec![1, 11, 12, 13, 21, 22, 23, 24, 25]);
    for v in kept {
        let row = kairos_db::history::version_row(&mut conn, item, v)
            .unwrap_or_else(|e| panic!("reading kept v{v}: {e}"))
            .unwrap_or_else(|| panic!("v{v} is kept"));
        assert_eq!(row.content, diff_text_of(v), "the text of kept v{v}");
    }

    let history_file = report
        .archive_files
        .iter()
        .find(|p| p.to_string_lossy().contains("item_history"))
        .expect("item_history archive");
    let archived: Vec<i32> = read_ndjson(history_file)
        .iter()
        .map(|line| {
            let version = line["version"].as_i64().unwrap() as i32;
            assert_eq!(
                line["content"].as_str().unwrap(),
                diff_text_of(version),
                "the archive line of v{version} has the full text"
            );
            let base = line["base_version"].as_i64().map(|b| b as i32);
            assert_eq!(
                base,
                (version != 1 && version != 11 && version != 21)
                    .then_some((version - 1) / 10 * 10 + 1),
                "the archive line of v{version} names its base as stored"
            );
            assert_eq!(line["content_diff"].is_string(), base.is_some());
            version
        })
        .collect();
    assert_eq!(archived, pruned);

    drop(conn);
    std::fs::remove_dir_all(&archive_base).expect("removing archive dir");
}
