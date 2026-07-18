//! Retention sweeper (KAIROS-T-0015, contract per the KAIROS-A-0004
//! "History Retention: Sweeper, Compaction, and Offload" amendment, config
//! per KAIROS-A-0013).
//!
//! KAIROS-A-0009 layering, same shape as [`crate::items`]: this module
//! loads `item_history`/`activity_log` rows, delegates the pure decision
//! (WHICH rows leave the database) to
//! [`kairos_core::retention::plan_history_compaction`], and performs the
//! I/O: **offload-then-prune**. Rows are serialized as NDJSON (one JSON
//! object per row, full column fidelity) to the archive target and the
//! file is fsynced BEFORE any `DELETE` runs — data is never destroyed
//! without a completed offload copy unless the operator explicitly set
//! `KAIROS_RETENTION_MODE=discard`.
//!
//! # Mode semantics (KAIROS-A-0004)
//!
//! - `archive` + configured filesystem target: offload → fsync → prune.
//! - `archive` + NO target: prune NOTHING; a warning is logged
//!   (`tracing::warn!`) and counted on the [`SweepReport`].
//! - `archive` + S3 target: typed [`RetentionError::ArchiveTargetNotImplemented`]
//!   — v1 ships the filesystem target only (KAIROS-T-0015; no S3 SDK
//!   dependency). The target parses so operators get a clear error, not a
//!   misconfiguration.
//! - `discard`: prune without archiving.
//! - `off`: complete no-op (no prune, no archive, no `retention_sweep`
//!   activity row).
//!
//! # Archive layout
//!
//! One file per sweep per tenant per table:
//! `{target}/{tenant_slug}/{table}/{timestamp}.ndjson`, where `timestamp`
//! is the injected sweep clock (`now`) — the sweeper never reads the wall
//! clock, so tests inject fixed clocks end to end. No file is written when
//! a table has nothing to prune.
//!
//! # Audit + metrics seam
//!
//! Every sweep (except `off`) appends one `activity_log` row with action
//! `retention_sweep` (the KAIROS-A-0004 extension of the action set; the
//! DDL leaves `action` unconstrained TEXT on purpose) carrying per-table
//! archived/pruned/warning counts, actor = the nil UUID (system actor —
//! `actor_id` references `public.users` by documentation only, no FK).
//! The same counts are exposed on [`SweepReport`]; the server's `/metrics`
//! endpoint (KAIROS-A-0013) will export them as Prometheus counters
//! (`rows_archived`/`rows_pruned`/`warnings` per table, labelled by tenant)
//! when M2 wires [`spawn_retention_loop`] at startup — no metrics crate is
//! taken here on purpose.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Bool, Text};
use serde::Serialize;
use uuid::Uuid;

use kairos_core::retention::{
    ArchiveTarget, HistoryRowMeta, RetentionConfig, RetentionMode, plan_history_compaction,
};

use crate::models::graph::ItemHistory;
use crate::tenant::{self, TenantError};

/// Errors from the retention sweeper.
#[derive(Debug, thiserror::Error)]
pub enum RetentionError {
    /// The slug does not match the KAIROS-S-0004 organization slug pattern.
    #[error("invalid tenant slug {0:?}: must match ^[a-z][a-z0-9_-]{{1,62}}$")]
    InvalidSlug(String),
    /// No `org_{slug}` schema exists for this tenant.
    #[error("tenant {0:?} does not exist")]
    TenantNotFound(String),
    /// The configured archive target kind is recognized but not shipped in
    /// v1 (KAIROS-T-0015: filesystem only; S3-compatible offload is a
    /// follow-up — NOT IMPLEMENTED).
    #[error(
        "archive target {0:?} is not implemented: v1 ships the filesystem target only \
         (KAIROS-T-0015); S3-compatible offload is a follow-up"
    )]
    ArchiveTargetNotImplemented(String),
    /// Writing (or fsyncing) an archive file failed; nothing was deleted.
    #[error("archive write failed at {path:?}: {source}")]
    ArchiveIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Serializing a row for the NDJSON archive failed.
    #[error("serializing archive row: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Listing tenants for the fleet sweep failed.
    #[error(transparent)]
    Tenant(#[from] TenantError),
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// Per-table sweep counters (the KAIROS-A-0013 metrics seam — see module
/// docs; Prometheus export happens in the server, M2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableCounts {
    /// Rows written to the NDJSON archive (0 in `discard` mode).
    pub rows_archived: u64,
    /// Rows deleted from the table.
    pub rows_pruned: u64,
    /// Warnings raised (currently: 1 when `archive` mode had prune
    /// candidates but no archive target was configured, so pruning paused).
    pub warnings: u64,
}

/// What one [`sweep_tenant`] run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// The organization slug.
    pub slug: String,
    /// The mode the sweep ran in.
    pub mode: RetentionMode,
    /// `item_history` counters (compaction tiers per KAIROS-A-0004).
    pub item_history: TableCounts,
    /// `activity_log` counters (plain window, no compaction tiers).
    pub activity_log: TableCounts,
    /// Archive files written by this sweep (empty unless `archive` mode
    /// pruned something).
    pub archive_files: Vec<PathBuf>,
}

impl SweepReport {
    fn new(slug: &str, mode: RetentionMode) -> Self {
        SweepReport {
            slug: slug.to_string(),
            mode,
            item_history: TableCounts::default(),
            activity_log: TableCounts::default(),
            archive_files: Vec::new(),
        }
    }

    /// The `retention_sweep` activity row's `details` payload: per-table
    /// counts, stable format.
    fn details(&self) -> String {
        format!(
            "mode:{} item_history:{{archived:{},pruned:{},warnings:{}}} \
             activity_log:{{archived:{},pruned:{},warnings:{}}}",
            self.mode,
            self.item_history.rows_archived,
            self.item_history.rows_pruned,
            self.item_history.warnings,
            self.activity_log.rows_archived,
            self.activity_log.rows_pruned,
            self.activity_log.warnings,
        )
    }
}

// ---------------------------------------------------------------------------
// NDJSON archive rows (full column fidelity)
// ---------------------------------------------------------------------------

/// Full-fidelity `item_history` archive row (every DDL column).
#[derive(Serialize)]
struct HistoryArchiveRow<'a> {
    id: Uuid,
    item_id: Uuid,
    version: i32,
    title: &'a str,
    content: &'a str,
    edited_by: Uuid,
    edited_at: DateTime<Utc>,
}

impl<'a> From<&'a ItemHistory> for HistoryArchiveRow<'a> {
    fn from(row: &'a ItemHistory) -> Self {
        HistoryArchiveRow {
            id: row.id,
            item_id: row.item_id,
            version: row.version,
            title: &row.title,
            content: &row.content,
            edited_by: row.edited_by,
            edited_at: row.edited_at,
        }
    }
}

/// A loaded `activity_log` row. Loaded with `action` as raw TEXT (NOT the
/// [`crate::models::enums::ActivityAction`] enum) because archived logs may
/// contain actions outside the enum — `retention_sweep` rows from earlier
/// sweeps age out through this same path.
type ActivityRow = (
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<String>,
    String,
    DateTime<Utc>,
);

/// Full-fidelity `activity_log` archive row (every DDL column).
#[derive(Serialize)]
struct ActivityArchiveRow<'a> {
    id: Uuid,
    actor_id: Uuid,
    action: &'a str,
    entity_id: Option<Uuid>,
    entity_type: Option<&'a str>,
    details: &'a str,
    occurred_at: DateTime<Utc>,
}

impl<'a> From<&'a ActivityRow> for ActivityArchiveRow<'a> {
    fn from(row: &'a ActivityRow) -> Self {
        ActivityArchiveRow {
            id: row.0,
            actor_id: row.1,
            action: &row.2,
            entity_id: row.3,
            entity_type: row.4.as_deref(),
            details: &row.5,
            occurred_at: row.6,
        }
    }
}

/// `{target}/{tenant}/{table}/{timestamp}.ndjson` (timestamp = the injected
/// sweep clock, filename-safe UTC format with microseconds).
fn archive_path(base: &Path, slug: &str, table: &str, now: DateTime<Utc>) -> PathBuf {
    base.join(slug)
        .join(table)
        .join(format!("{}.ndjson", now.format("%Y%m%dT%H%M%S%.6fZ")))
}

/// Write NDJSON lines to a NEW file and fsync it before returning — the
/// offload must be durable before any row is deleted. Refuses to overwrite
/// an existing file (two sweeps sharing a tenant/table/timestamp would
/// clobber an archive).
fn write_ndjson(path: &Path, lines: &[String]) -> Result<(), RetentionError> {
    let io_err = |source: std::io::Error| RetentionError::ArchiveIo {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_err)?;
    for line in lines {
        file.write_all(line.as_bytes()).map_err(io_err)?;
        file.write_all(b"\n").map_err(io_err)?;
    }
    file.sync_all().map_err(io_err)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sweep
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct BoolRow {
    #[diesel(sql_type = Bool)]
    present: bool,
}

fn schema_exists(conn: &mut PgConnection, schema: &str) -> Result<bool, DieselError> {
    let row: BoolRow =
        sql_query("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1) AS present")
            .bind::<Text, _>(schema)
            .get_result(conn)?;
    Ok(row.present)
}

/// Run one retention sweep for one tenant (KAIROS-A-0004 tiers, mode
/// semantics per module docs), with an injected clock — `now` is the only
/// time source; the sweeper never calls the wall clock.
///
/// Pins `search_path` to the tenant schema for the duration (restored to
/// DEFAULT afterwards, success or failure, like
/// [`crate::tenant::migrate_all_tenants`]). Archive files are written and
/// fsynced BEFORE the deletes; the deletes and the `retention_sweep`
/// activity row commit in one transaction.
pub fn sweep_tenant(
    conn: &mut PgConnection,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<SweepReport, RetentionError> {
    if config.mode == RetentionMode::Off {
        // Disabled: full no-op — not even the audit row.
        return Ok(SweepReport::new(slug, RetentionMode::Off));
    }
    if !tenant::is_valid_slug(slug) {
        return Err(RetentionError::InvalidSlug(slug.to_string()));
    }
    let schema = tenant::tenant_schema_name(slug);
    if !schema_exists(conn, &schema)? {
        return Err(RetentionError::TenantNotFound(slug.to_string()));
    }

    sql_query(format!("SET search_path TO \"{schema}\"")).execute(conn)?;
    let result = sweep_current_schema(conn, slug, config, now);
    // Always restore the connection's default search_path, even on error,
    // before surfacing the outcome (same discipline as migrate_all_tenants).
    let reset = sql_query("SET search_path TO DEFAULT").execute(conn);
    let report = result?;
    reset?;
    Ok(report)
}

/// The sweep body; assumes `search_path` is pinned to the tenant schema.
fn sweep_current_schema(
    conn: &mut PgConnection,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<SweepReport, RetentionError> {
    use crate::schema::{activity_log, item_history};

    let mut report = SweepReport::new(slug, config.mode);

    // Load every history row's full columns once (M1 fleet scale; the seam
    // for larger fleets is paging this per item_id). The planner sees only
    // the metadata; the same loaded rows are what gets archived, so the
    // NDJSON offload is byte-faithful to what is deleted.
    let mut history_rows: Vec<ItemHistory> = item_history::table
        .select(ItemHistory::as_select())
        .load(conn)?;
    history_rows.sort_by_key(|r| (r.item_id, r.version));
    let meta: Vec<HistoryRowMeta> = history_rows
        .iter()
        .map(|r| HistoryRowMeta {
            item_id: r.item_id,
            version: r.version,
            edited_at: r.edited_at,
        })
        .collect();
    let prune_keys: BTreeSet<(Uuid, i32)> = plan_history_compaction(&meta, now, config)
        .into_iter()
        .map(|k| (k.item_id, k.version))
        .collect();
    let pruned_history: Vec<&ItemHistory> = history_rows
        .iter()
        .filter(|r| prune_keys.contains(&(r.item_id, r.version)))
        .collect();

    // activity_log: plain retention window, no compaction tiers (A-0004 §4).
    let activity_cutoff = now - Duration::days(i64::from(config.activity_retention_days));
    let old_activity: Vec<ActivityRow> = activity_log::table
        .filter(activity_log::occurred_at.lt(activity_cutoff))
        .order((activity_log::occurred_at.asc(), activity_log::id.asc()))
        .select((
            activity_log::id,
            activity_log::actor_id,
            activity_log::action,
            activity_log::entity_id,
            activity_log::entity_type,
            activity_log::details,
            activity_log::occurred_at,
        ))
        .load(conn)?;

    // Offload-then-prune per mode.
    let mut history_delete_ids: Vec<Uuid> = Vec::new();
    let mut activity_delete_ids: Vec<Uuid> = Vec::new();
    match (config.mode, &config.archive_target) {
        (RetentionMode::Off, _) => unreachable!("gated in sweep_tenant"),
        (RetentionMode::Archive, None) => {
            // A-0004: compaction pauses; data is never destroyed without an
            // offload copy unless the operator explicitly chose `discard`.
            if !pruned_history.is_empty() {
                tracing::warn!(
                    tenant = slug,
                    candidates = pruned_history.len(),
                    "retention: archive mode with no {} configured; \
                     item_history compaction paused, nothing pruned",
                    kairos_core::retention::ENV_ARCHIVE_TARGET,
                );
                report.item_history.warnings = 1;
            }
            if !old_activity.is_empty() {
                tracing::warn!(
                    tenant = slug,
                    candidates = old_activity.len(),
                    "retention: archive mode with no {} configured; \
                     activity_log pruning paused, nothing pruned",
                    kairos_core::retention::ENV_ARCHIVE_TARGET,
                );
                report.activity_log.warnings = 1;
            }
        }
        (RetentionMode::Archive, Some(ArchiveTarget::S3(url))) => {
            return Err(RetentionError::ArchiveTargetNotImplemented(url.clone()));
        }
        (RetentionMode::Archive, Some(ArchiveTarget::Filesystem(base))) => {
            if !pruned_history.is_empty() {
                let lines = pruned_history
                    .iter()
                    .map(|row| serde_json::to_string(&HistoryArchiveRow::from(*row)))
                    .collect::<Result<Vec<_>, _>>()?;
                let path = archive_path(base, slug, "item_history", now);
                write_ndjson(&path, &lines)?;
                report.item_history.rows_archived = lines.len() as u64;
                report.archive_files.push(path);
                history_delete_ids = pruned_history.iter().map(|r| r.id).collect();
            }
            if !old_activity.is_empty() {
                let lines = old_activity
                    .iter()
                    .map(|row| serde_json::to_string(&ActivityArchiveRow::from(row)))
                    .collect::<Result<Vec<_>, _>>()?;
                let path = archive_path(base, slug, "activity_log", now);
                write_ndjson(&path, &lines)?;
                report.activity_log.rows_archived = lines.len() as u64;
                report.archive_files.push(path);
                activity_delete_ids = old_activity.iter().map(|r| r.0).collect();
            }
        }
        (RetentionMode::Discard, _) => {
            history_delete_ids = pruned_history.iter().map(|r| r.id).collect();
            activity_delete_ids = old_activity.iter().map(|r| r.0).collect();
        }
    }

    // Prune + audit in ONE transaction (the archive files above are already
    // durable; a failed transaction leaves at worst an orphan archive copy,
    // never a deleted-but-unarchived row).
    conn.transaction::<_, DieselError, _>(|conn| {
        if !history_delete_ids.is_empty() {
            let n = diesel::delete(
                item_history::table.filter(item_history::id.eq_any(&history_delete_ids)),
            )
            .execute(conn)?;
            report.item_history.rows_pruned = n as u64;
        }
        if !activity_delete_ids.is_empty() {
            let n = diesel::delete(
                activity_log::table.filter(activity_log::id.eq_any(&activity_delete_ids)),
            )
            .execute(conn)?;
            report.activity_log.rows_pruned = n as u64;
        }
        // The sweep's own audit row (A-0004: action `retention_sweep`, with
        // row counts). Nil UUID = system actor; occurred_at = the injected
        // sweep clock.
        diesel::insert_into(activity_log::table)
            .values((
                activity_log::actor_id.eq(Uuid::nil()),
                activity_log::action.eq("retention_sweep"),
                activity_log::details.eq(report.details()),
                activity_log::occurred_at.eq(now),
            ))
            .execute(conn)?;
        Ok(())
    })?;

    Ok(report)
}

/// Fleet sweep: run [`sweep_tenant`] for every provisioned organization,
/// in slug order (same iteration pattern as
/// [`crate::tenant::migrate_all_tenants`]). Stops at the first failing
/// tenant. `mode = off` returns an empty report list without touching the
/// database beyond the tenant listing.
pub fn sweep_all_tenants(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<Vec<SweepReport>, RetentionError> {
    if config.mode == RetentionMode::Off {
        return Ok(Vec::new());
    }
    let tenants = tenant::list_tenants(conn)?;
    let mut reports = Vec::with_capacity(tenants.len());
    for tenant_info in tenants {
        if !tenant_info.schema_exists {
            return Err(RetentionError::TenantNotFound(tenant_info.slug));
        }
        reports.push(sweep_tenant(conn, &tenant_info.slug, config, now)?);
    }
    Ok(reports)
}

/// Spawn the in-process retention scheduler (KAIROS-A-0004: "in-process
/// scheduled task in the server binary"): a tokio task that invokes `tick`
/// immediately and then every `interval` until `tick` returns
/// [`ControlFlow::Break`].
///
/// The tick body is injected so the loop stays unit-testable without a
/// database (and sweeps stay testable without the loop — call
/// [`sweep_all_tenants`] directly with an injected clock). The server
/// wires this at startup with a closure that checks out a connection,
/// reads [`RetentionConfig`] from the environment, and calls
/// [`sweep_all_tenants`] with `Utc::now()` — that wiring is an M2 task
/// (KAIROS-A-0013 also hangs the Prometheus export off that seam).
pub fn spawn_retention_loop<F>(
    interval: std::time::Duration,
    mut tick: F,
) -> tokio::task::JoinHandle<()>
where
    F: FnMut() -> ControlFlow<()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(interval);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            timer.tick().await;
            if tick().is_break() {
                break;
            }
        }
    })
}
