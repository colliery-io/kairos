//! Retention sweeper (KAIROS-T-0015, contract per the KAIROS-A-0004
//! "History Retention: Sweeper, Compaction, and Offload" amendment, config
//! per KAIROS-A-0013).
//!
//! KAIROS-A-0009 layering, same shape as [`crate::items`]: this module
//! loads `item_history`/`team_page_history`/`activity_log` rows, delegates
//! the pure decision
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
//! - `archive` + NO target: read NOTHING and prune nothing; a warning is
//!   logged (`tracing::warn!`), counted on the [`SweepReport`], and the
//!   audit row records `skipped: no target` (KAIROS-T-0631).
//! - `archive` + S3 target (KAIROS-T-0671): the lines go to a file in the
//!   temporary folder, the file is fsynced, uploaded through
//!   [`RetentionConfig::archive_store`] and removed, and the rows are
//!   deleted after the upload. With no store in the config, typed
//!   [`RetentionError::ArchiveTargetNotImplemented`]; the server makes the
//!   store at start.
//! - `discard`: prune without archiving.
//! - `off`: complete no-op (no prune, no archive, no `retention_sweep`
//!   activity row).
//!
//! `team_page_history` follows the rules and the mode of `item_history`
//! (KAIROS-T-0638): a page is an item.
//!
//! # Paging (KAIROS-T-0631)
//!
//! The sweep never loads a table whole. It reads `item_history` a page of
//! items at a time, in `item_id` order, and only the metadata columns
//! `(id, item_id, version, edited_at, base_version)`; it reads the content
//! only of the rows that it archives. `team_page_history` pages by
//! `page_id` the same way. It reads `activity_log` a page of rows at a
//! time, in `(occurred_at, id)` order. Each delete names at most one batch
//! of ids. [`SweepPaging`] sets the sizes.
//!
//! # Diff rows (KAIROS-T-0651)
//!
//! A diff row of `item_history` needs its base full row
//! ([`crate::history`]). The planner keeps the base of each row that it
//! keeps, so each kept version keeps its text. The archive line of a diff
//! row has the full text. `team_page_history` has full rows only.
//!
//! # The rows that move an item stay (KAIROS-T-0571)
//!
//! The `activity_log` sweep does not read or remove the rows of the
//! actions in [`kairos_core::retention::KEPT_ACTIVITY_ACTIONS`]
//! (`transition`, `cancel`, `board_move`), at any age. The flow metrics
//! of a board read them: without the first move into Active, a task that
//! is in progress for more than the window has no age.
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
//! when it is wired — no metrics crate is taken here on purpose. The
//! server runs [`run_retention_pass`] as a periodic job
//! (KAIROS-T-0366, KAIROS-T-0376).
//!
//! # The other tables of the pass (KAIROS-T-0638)
//!
//! The pass also deletes, in each mode and with no archive, a page of rows
//! at a time: the old code indexes (the newest `code_index_keep` per
//! repository and ref stay), the summaries that no stored index uses, the
//! read and the unread notifications past their windows, and the webhook
//! deliveries that finished more than [`crate::webhooks::KEEP_DAYS`] days
//! ago. Each of these rows is a cache or a copy of a fact that another row
//! keeps.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{
    Array, BigInt, Bool, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid,
};
use serde::Serialize;
use uuid::Uuid;

use kairos_core::retention::{
    ArchiveStore, ArchiveTarget, HistoryRowMeta, RetentionConfig, RetentionMode,
    plan_code_index_retention, plan_history_compaction,
};

use crate::models::enums::ActivityAction;
use crate::tenant::{self, TenantError};

/// Errors from the retention sweeper.
#[derive(Debug, thiserror::Error)]
pub enum RetentionError {
    /// The slug does not match the KAIROS-S-0004 organization slug pattern.
    #[error(
        "The tenant slug {0:?} is not correct. A tenant slug must match ^[a-z][a-z0-9_-]{{1,62}}$."
    )]
    InvalidSlug(String),
    /// No `org_{slug}` schema exists for this tenant.
    #[error("The tenant {0:?} does not exist.")]
    TenantNotFound(String),
    /// The archive target is an `s3://` URL and the config has no store for
    /// it: the server makes one at start (KAIROS-T-0671), a library caller
    /// sets [`RetentionConfig::archive_store`].
    #[error(
        "The archive target {0:?} is an s3:// URL, and the retention config has no S3 store \
         for it."
    )]
    ArchiveTargetNotImplemented(String),
    /// The upload of an archive file to an `s3://` target failed; nothing
    /// was deleted (KAIROS-T-0671).
    #[error("The server cannot write the archive to {url}: {message}.")]
    ArchiveUpload { url: String, message: String },
    /// Writing (or fsyncing) an archive file failed; nothing was deleted.
    #[error("The server cannot write the archive at {path:?}: {source}.")]
    ArchiveIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Serializing a row for the NDJSON archive failed.
    #[error("The server cannot write a row of the archive as JSON: {0}.")]
    Serialize(#[from] serde_json::Error),
    /// Listing tenants for the fleet sweep failed.
    #[error(transparent)]
    Tenant(#[from] TenantError),
    /// The text of a version to archive cannot be read (KAIROS-T-0651);
    /// nothing was deleted.
    #[error(transparent)]
    History(#[from] crate::history::HistoryError),
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
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
    /// Warnings raised (currently: 1 when `archive` mode had no archive
    /// target configured, so the sweep was skipped and pruning paused).
    pub warnings: u64,
}

/// Why a sweep read and pruned nothing (KAIROS-T-0631).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepSkip {
    /// `archive` mode with no archive target.
    NoTarget,
}

impl SweepSkip {
    /// The record of the skip: `skipped: no target`.
    pub fn as_str(self) -> &'static str {
        match self {
            SweepSkip::NoTarget => kairos_core::retention::SKIPPED_NO_TARGET,
        }
    }
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
    /// `team_page_history` counters (the tiers of `item_history`,
    /// KAIROS-T-0638).
    pub team_page_history: TableCounts,
    /// Archive files written by this sweep (empty unless `archive` mode
    /// pruned something).
    pub archive_files: Vec<PathBuf>,
    /// Set when the sweep read and pruned nothing, with the reason.
    pub skipped: Option<SweepSkip>,
    /// The pages of items of `item_history` that the sweep read.
    pub history_pages: u64,
    /// The pages of rows of `activity_log` that the sweep read.
    pub activity_pages: u64,
    /// The pages of team pages of `team_page_history` that the sweep read.
    pub team_page_history_pages: u64,
}

impl SweepReport {
    fn new(slug: &str, mode: RetentionMode) -> Self {
        SweepReport {
            slug: slug.to_string(),
            mode,
            item_history: TableCounts::default(),
            activity_log: TableCounts::default(),
            team_page_history: TableCounts::default(),
            archive_files: Vec::new(),
            skipped: None,
            history_pages: 0,
            activity_pages: 0,
            team_page_history_pages: 0,
        }
    }

    /// The `retention_sweep` activity row's `details` payload: per-table
    /// counts, stable format. A skipped sweep names the reason after the
    /// mode: `mode:archive skipped: no target item_history:{…}`.
    fn details(&self) -> String {
        let skipped = self
            .skipped
            .map(|skip| format!(" {}", skip.as_str()))
            .unwrap_or_default();
        let counts = |name: &str, c: &TableCounts| {
            format!(
                "{name}:{{archived:{},pruned:{},warnings:{}}}",
                c.rows_archived, c.rows_pruned, c.warnings
            )
        };
        format!(
            "mode:{}{skipped} {} {} {}",
            self.mode,
            counts("item_history", &self.item_history),
            counts("activity_log", &self.activity_log),
            counts("team_page_history", &self.team_page_history),
        )
    }
}

// ---------------------------------------------------------------------------
// NDJSON archive rows (full column fidelity)
// ---------------------------------------------------------------------------

/// One whole row of `item_history` or `team_page_history`: the two tables
/// have the same columns, and `owner` is `item_id` or `page_id`.
#[derive(QueryableByName)]
struct HistoryFullRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    owner: Uuid,
    #[diesel(sql_type = Integer)]
    version: i32,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = SqlUuid)]
    edited_by: Uuid,
    #[diesel(sql_type = Timestamptz)]
    edited_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    agent_key_id: Option<Uuid>,
    /// `item_history` only (KAIROS-T-0651); NULL for `team_page_history`.
    #[diesel(sql_type = Nullable<Integer>)]
    base_version: Option<i32>,
    /// `item_history` only (KAIROS-T-0651); NULL for `team_page_history`.
    #[diesel(sql_type = Nullable<Text>)]
    content_diff: Option<String>,
}

/// Full-fidelity `item_history` archive row (every DDL column). `content`
/// is always the full text of the version, also for a diff row
/// (KAIROS-T-0651), so each line of the archive stands alone.
/// `base_version` and `content_diff` are as stored.
#[derive(Serialize)]
struct HistoryArchiveRow<'a> {
    id: Uuid,
    item_id: Uuid,
    version: i32,
    title: &'a str,
    content: &'a str,
    edited_by: Uuid,
    edited_at: DateTime<Utc>,
    agent_key_id: Option<Uuid>,
    base_version: Option<i32>,
    content_diff: Option<&'a str>,
}

/// Full-fidelity `team_page_history` archive row (every DDL column).
#[derive(Serialize)]
struct PageHistoryArchiveRow<'a> {
    id: Uuid,
    page_id: Uuid,
    version: i32,
    title: &'a str,
    content: &'a str,
    edited_by: Uuid,
    edited_at: DateTime<Utc>,
    agent_key_id: Option<Uuid>,
}

/// A history table that the compaction tiers of KAIROS-A-0004 thin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryTable {
    /// `item_history`, by `item_id`.
    Item,
    /// `team_page_history`, by `page_id` (KAIROS-T-0638).
    TeamPage,
}

impl HistoryTable {
    fn table(self) -> &'static str {
        match self {
            HistoryTable::Item => "item_history",
            HistoryTable::TeamPage => "team_page_history",
        }
    }

    fn owner(self) -> &'static str {
        match self {
            HistoryTable::Item => "item_id",
            HistoryTable::TeamPage => "page_id",
        }
    }

    /// The SQL of the diff columns `base_version, content_diff`
    /// (KAIROS-T-0651). `team_page_history` has full rows only.
    fn diff_columns(self) -> &'static str {
        match self {
            HistoryTable::Item => "base_version, content_diff",
            HistoryTable::TeamPage => "NULL::integer AS base_version, NULL::text AS content_diff",
        }
    }

    /// The NDJSON line of a whole row, with the column names of the table.
    fn archive_line(self, row: &HistoryFullRow) -> Result<String, serde_json::Error> {
        match self {
            HistoryTable::Item => serde_json::to_string(&HistoryArchiveRow {
                id: row.id,
                item_id: row.owner,
                version: row.version,
                title: &row.title,
                content: &row.content,
                edited_by: row.edited_by,
                edited_at: row.edited_at,
                agent_key_id: row.agent_key_id,
                base_version: row.base_version,
                content_diff: row.content_diff.as_deref(),
            }),
            HistoryTable::TeamPage => serde_json::to_string(&PageHistoryArchiveRow {
                id: row.id,
                page_id: row.owner,
                version: row.version,
                title: &row.title,
                content: &row.content,
                edited_by: row.edited_by,
                edited_at: row.edited_at,
                agent_key_id: row.agent_key_id,
            }),
        }
    }
}

/// A loaded `activity_log` row. Loaded with `action` as raw TEXT (NOT the
/// [`crate::models::enums::ActivityAction`] enum) because archived logs may
/// contain actions from older releases that the enum no longer knows; the
/// sweep must age those rows out, not fail on them.
type ActivityRow = (
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<String>,
    String,
    DateTime<Utc>,
    Option<Uuid>,
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
    agent_key_id: Option<Uuid>,
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
            agent_key_id: row.7,
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

/// `{prefix}/{tenant}/{table}/{timestamp}.ndjson`: the key of an archive
/// file in the bucket of an `s3://` target (KAIROS-T-0671), the same layout
/// as [`archive_path`]. `prefix` has no slash at its ends and may be empty.
fn archive_key(prefix: &str, slug: &str, table: &str, now: DateTime<Utc>) -> String {
    let name = format!("{slug}/{table}/{}.ndjson", now.format("%Y%m%dT%H%M%S%.6fZ"));
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// One NDJSON archive file that a sweep writes page by page
/// (KAIROS-T-0631). The file is NEW: [`NdjsonFile::create`] refuses to
/// overwrite an existing file (two sweeps sharing a tenant/table/timestamp
/// would clobber an archive). [`NdjsonFile::finish`] fsyncs it: the offload
/// must be durable before any row is deleted.
struct NdjsonFile {
    path: PathBuf,
    file: std::io::BufWriter<fs::File>,
    lines: u64,
}

impl NdjsonFile {
    fn io_err(path: &Path) -> impl Fn(std::io::Error) -> RetentionError + '_ {
        move |source| RetentionError::ArchiveIo {
            path: path.to_path_buf(),
            source,
        }
    }

    fn create(path: PathBuf) -> Result<Self, RetentionError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(Self::io_err(&path))?;
        }
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(Self::io_err(&path))?;
        Ok(NdjsonFile {
            path,
            file: std::io::BufWriter::new(file),
            lines: 0,
        })
    }

    fn write_line(&mut self, line: &str) -> Result<(), RetentionError> {
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.write_all(b"\n"))
            .map_err(Self::io_err(&self.path))?;
        self.lines += 1;
        Ok(())
    }

    /// Flush and fsync; the path and the number of lines.
    fn finish(self) -> Result<(PathBuf, u64), RetentionError> {
        let NdjsonFile { path, file, lines } = self;
        let file = file.into_inner().map_err(|e| RetentionError::ArchiveIo {
            path: path.clone(),
            source: e.into_error(),
        })?;
        file.sync_all().map_err(Self::io_err(&path))?;
        Ok((path, lines))
    }
}

/// Where the file of an archive goes after it is written and fsynced: an
/// object of a bucket (KAIROS-T-0671).
struct Upload {
    store: Arc<dyn ArchiveStore>,
    bucket: String,
    key: String,
}

/// The archive file of one table, opened at its first line: no file is
/// written when a table has nothing to prune.
///
/// For an `s3://` target the lines go to a file in the temporary folder, and
/// [`LazyArchive::finish`] uploads the file when the plan is complete and
/// removes it. The rows are deleted after that. A file that a failed sweep
/// leaves is removed when the archive is dropped.
struct LazyArchive {
    path: PathBuf,
    file: Option<NdjsonFile>,
    upload: Option<Upload>,
}

impl LazyArchive {
    fn new(path: PathBuf) -> Self {
        LazyArchive {
            path,
            file: None,
            upload: None,
        }
    }

    /// An archive that ends in the object `key` of `bucket`.
    fn to_store(store: Arc<dyn ArchiveStore>, bucket: String, key: String) -> Self {
        let path = std::env::temp_dir().join(format!("kairos-archive-{}.ndjson", Uuid::new_v4()));
        LazyArchive {
            path,
            file: None,
            upload: Some(Upload { store, bucket, key }),
        }
    }

    fn write_line(&mut self, line: &str) -> Result<(), RetentionError> {
        if self.file.is_none() {
            self.file = Some(NdjsonFile::create(self.path.clone())?);
        }
        match self.file.as_mut() {
            Some(file) => file.write_line(line),
            None => Ok(()),
        }
    }

    /// The path (for an object, `s3://bucket/key`) and the lines of the
    /// file, when one was written. An object is stored when this returns.
    fn finish(mut self) -> Result<Option<(PathBuf, u64)>, RetentionError> {
        let Some(file) = self.file.take() else {
            return Ok(None);
        };
        let (path, lines) = file.finish()?;
        let Some(upload) = self.upload.take() else {
            return Ok(Some((path, lines)));
        };
        let url = format!("s3://{}/{}", upload.bucket, upload.key);
        let stored = upload.store.put_file(&upload.key, &path);
        let _ = fs::remove_file(&path);
        stored.map_err(|message| RetentionError::ArchiveUpload {
            url: url.clone(),
            message,
        })?;
        Ok(Some((PathBuf::from(url), lines)))
    }
}

impl Drop for LazyArchive {
    fn drop(&mut self) {
        // A sweep that failed before `finish`: no file stays in the
        // temporary folder.
        if self.upload.is_some() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Sweep
// ---------------------------------------------------------------------------

/// The page and batch sizes of a sweep (KAIROS-T-0631). The sweep never
/// holds a whole table in memory: it reads `item_history` a page of items
/// at a time (metadata columns only), `activity_log` a page of rows at a
/// time, and names at most `batch` ids in one content read or one
/// `DELETE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepPaging {
    /// The items whose history rows one page reads.
    pub items_per_page: i64,
    /// The `activity_log` rows that one page reads.
    pub activity_per_page: i64,
    /// The ids that one content read or one `DELETE` names.
    pub batch: usize,
}

impl Default for SweepPaging {
    fn default() -> Self {
        SweepPaging {
            items_per_page: 500,
            activity_per_page: 5_000,
            batch: 1_000,
        }
    }
}

impl SweepPaging {
    /// Each size at least 1.
    fn checked(self) -> Self {
        SweepPaging {
            items_per_page: self.items_per_page.max(1),
            activity_per_page: self.activity_per_page.max(1),
            batch: self.batch.max(1),
        }
    }
}

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
/// time source; the sweeper never calls the wall clock. The default
/// [`SweepPaging`]; see [`sweep_tenant_with`].
pub fn sweep_tenant(
    conn: &mut PgConnection,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<SweepReport, RetentionError> {
    sweep_tenant_with(conn, slug, config, now, SweepPaging::default())
}

/// [`sweep_tenant`] with the page and batch sizes `paging`.
///
/// Pins `search_path` to the tenant schema for the duration (restored to
/// DEFAULT afterwards, success or failure, like
/// [`crate::tenant::migrate_all_tenants`]). Archive files are written and
/// fsynced BEFORE the deletes; the deletes and the `retention_sweep`
/// activity row commit in one transaction.
pub fn sweep_tenant_with(
    conn: &mut PgConnection,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<SweepReport, RetentionError> {
    if config.mode == RetentionMode::Off {
        // Disabled: full no-op — not even the audit row.
        return Ok(SweepReport::new(slug, RetentionMode::Off));
    }
    if !tenant::is_valid_tenant_slug(slug) {
        return Err(RetentionError::InvalidSlug(slug.to_string()));
    }
    let schema = tenant::tenant_schema_name(slug);
    if !schema_exists(conn, &schema)? {
        return Err(RetentionError::TenantNotFound(slug.to_string()));
    }

    sql_query(format!("SET search_path TO \"{schema}\"")).execute(conn)?;
    let result = sweep_current_schema(conn, slug, config, now, paging.checked());
    // Always restore the connection's default search_path, even on error,
    // before surfacing the outcome (same discipline as migrate_all_tenants).
    let reset = sql_query("SET search_path TO DEFAULT").execute(conn);
    let report = result?;
    reset?;
    Ok(report)
}

#[derive(QueryableByName)]
struct OwnerRow {
    #[diesel(sql_type = SqlUuid)]
    owner: Uuid,
}

/// The history columns that the planner needs: `(id, owner, version,
/// edited_at, base_version)`. Never the title or the content.
#[derive(QueryableByName)]
struct HistoryMetaRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    owner: Uuid,
    #[diesel(sql_type = Integer)]
    version: i32,
    #[diesel(sql_type = Timestamptz)]
    edited_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Integer>)]
    base_version: Option<i32>,
}

/// Plan a history table page by page (KAIROS-T-0631): a page is the
/// metadata of every row of up to `items_per_page` owners (items, or team
/// pages), in owner order, so the planner sees whole owners. The content
/// is read only for the rows that `archive` writes, `batch` rows at a
/// time. `pages` counts the pages. The ids to delete, in `(owner,
/// version)` order.
#[allow(clippy::too_many_arguments)]
fn plan_history_pages(
    conn: &mut PgConnection,
    table: HistoryTable,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
    mut archive: Option<&mut LazyArchive>,
    pages: &mut u64,
) -> Result<Vec<Uuid>, RetentionError> {
    let (name, owner) = (table.table(), table.owner());
    let mut delete_ids = Vec::new();
    let mut after: Option<Uuid> = None;
    loop {
        let owners: Vec<Uuid> = sql_query(format!(
            "SELECT DISTINCT {owner} AS owner FROM {name} \
             WHERE ($1::uuid IS NULL OR {owner} > $1) ORDER BY owner LIMIT $2"
        ))
        .bind::<Nullable<SqlUuid>, _>(after)
        .bind::<BigInt, _>(paging.items_per_page)
        .load::<OwnerRow>(conn)?
        .into_iter()
        .map(|r| r.owner)
        .collect();
        let Some(last) = owners.last().copied() else {
            break;
        };
        *pages += 1;

        let diff_columns = table.diff_columns();
        let rows: Vec<HistoryMetaRow> = sql_query(format!(
            "SELECT id, {owner} AS owner, version, edited_at, {diff_columns} FROM {name} \
             WHERE {owner} = ANY($1)"
        ))
        .bind::<Array<SqlUuid>, _>(&owners)
        .load(conn)?;
        let ids: HashMap<(Uuid, i32), Uuid> =
            rows.iter().map(|r| ((r.owner, r.version), r.id)).collect();
        let meta: Vec<HistoryRowMeta> = rows
            .iter()
            .map(|r| HistoryRowMeta {
                item_id: r.owner,
                version: r.version,
                edited_at: r.edited_at,
                // KAIROS-T-0651: the planner keeps the base full row of each
                // diff row that it keeps.
                base_version: r.base_version,
            })
            .collect();
        // The plan is sorted by (owner, version), so the ids are too.
        let page_ids: Vec<Uuid> = plan_history_compaction(&meta, now, config)
            .into_iter()
            .filter_map(|k| ids.get(&(k.item_id, k.version)).copied())
            .collect();

        if let Some(archive) = archive.as_deref_mut() {
            for chunk in page_ids.chunks(paging.batch) {
                let mut full: Vec<HistoryFullRow> = sql_query(format!(
                    "SELECT id, {owner} AS owner, version, title, content, edited_by, \
                         edited_at, agent_key_id, {diff_columns} \
                     FROM {name} WHERE id = ANY($1) ORDER BY {owner}, version"
                ))
                .bind::<Array<SqlUuid>, _>(chunk)
                .load(conn)?;
                // KAIROS-T-0651: the archive has the full text of each row.
                // The base rows are still in the table: no delete ran yet.
                let wanted: HashSet<(Uuid, i32)> = full
                    .iter()
                    .filter_map(|row| row.base_version.map(|base| (row.owner, base)))
                    .collect();
                let bases = crate::history::base_texts(conn, &wanted)?;
                for row in &mut full {
                    if let (Some(base_version), Some(diff)) =
                        (row.base_version, row.content_diff.as_deref())
                    {
                        row.content = crate::history::full_text(
                            &bases,
                            row.owner,
                            row.version,
                            base_version,
                            diff,
                        )?;
                    }
                }
                for row in &full {
                    archive.write_line(&table.archive_line(row)?)?;
                }
            }
        }
        delete_ids.extend(page_ids);

        if (owners.len() as i64) < paging.items_per_page {
            break;
        }
        after = Some(last);
    }
    tracing::debug!(
        tenant = slug,
        table = name,
        pages = *pages,
        candidates = delete_ids.len(),
        "retention: history planned"
    );
    Ok(delete_ids)
}

/// Read the `activity_log` rows older than the window page by page, in
/// `(occurred_at, id)` order, and archive them when `archive` is set. The
/// ids to delete. The rows that move an item stay (module docs).
fn plan_activity_pages(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    paging: SweepPaging,
    mut archive: Option<&mut LazyArchive>,
    report: &mut SweepReport,
) -> Result<Vec<Uuid>, RetentionError> {
    use crate::schema::activity_log;

    let mut delete_ids = Vec::new();
    let mut after: Option<(DateTime<Utc>, Uuid)> = None;
    loop {
        let mut page = activity_log::table
            .filter(activity_log::occurred_at.lt(cutoff))
            .filter(activity_log::action.ne_all(kairos_core::retention::KEPT_ACTIVITY_ACTIONS))
            .order((activity_log::occurred_at.asc(), activity_log::id.asc()))
            .limit(paging.activity_per_page)
            .select((
                activity_log::id,
                activity_log::actor_id,
                activity_log::action,
                activity_log::entity_id,
                activity_log::entity_type,
                activity_log::details,
                activity_log::occurred_at,
                activity_log::agent_key_id,
            ))
            .into_boxed();
        if let Some((at, id)) = after {
            page = page.filter(
                activity_log::occurred_at
                    .gt(at)
                    .or(activity_log::occurred_at
                        .eq(at)
                        .and(activity_log::id.gt(id))),
            );
        }
        let rows: Vec<ActivityRow> = page.load(conn)?;
        let Some(last) = rows.last().map(|r| (r.6, r.0)) else {
            break;
        };
        report.activity_pages += 1;
        if let Some(archive) = archive.as_deref_mut() {
            for row in &rows {
                archive.write_line(&serde_json::to_string(&ActivityArchiveRow::from(row))?)?;
            }
        }
        delete_ids.extend(rows.iter().map(|r| r.0));
        if (rows.len() as i64) < paging.activity_per_page {
            break;
        }
        after = Some(last);
    }
    Ok(delete_ids)
}

/// Write the `retention_sweep` audit row of `report` (A-0004: with row
/// counts). Nil UUID = system actor; occurred_at = the injected sweep
/// clock.
fn insert_audit_row(
    conn: &mut PgConnection,
    report: &SweepReport,
    now: DateTime<Utc>,
) -> Result<(), DieselError> {
    use crate::schema::activity_log;
    diesel::insert_into(activity_log::table)
        .values((
            activity_log::actor_id.eq(Uuid::nil()),
            activity_log::action.eq(ActivityAction::RetentionSweep),
            activity_log::details.eq(report.details()),
            activity_log::occurred_at.eq(now),
        ))
        .execute(conn)?;
    Ok(())
}

/// Where a sweep puts its archive files.
enum Target<'a> {
    /// A folder.
    Directory(&'a Path),
    /// A bucket, with a key prefix (KAIROS-T-0671).
    Store {
        store: Arc<dyn ArchiveStore>,
        bucket: String,
        prefix: String,
    },
}

impl Target<'_> {
    /// The archive of one table of one tenant for the sweep at `now`.
    fn archive(&self, slug: &str, table: &str, now: DateTime<Utc>) -> LazyArchive {
        match self {
            Target::Directory(base) => LazyArchive::new(archive_path(base, slug, table, now)),
            Target::Store {
                store,
                bucket,
                prefix,
            } => LazyArchive::to_store(
                Arc::clone(store),
                bucket.clone(),
                archive_key(prefix, slug, table, now),
            ),
        }
    }
}

/// The sweep body; assumes `search_path` is pinned to the tenant schema.
fn sweep_current_schema(
    conn: &mut PgConnection,
    slug: &str,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<SweepReport, RetentionError> {
    use crate::schema::activity_log;

    let mut report = SweepReport::new(slug, config.mode);

    // The mode decides first, so a sweep that cannot prune reads nothing.
    let target: Option<Target> = match (config.mode, &config.archive_target) {
        (RetentionMode::Off, _) => unreachable!("gated in sweep_tenant"),
        (RetentionMode::Archive, None) => {
            // A-0004: compaction pauses; data is never destroyed without an
            // offload copy unless the operator explicitly chose `discard`.
            // KAIROS-T-0631: the sweep reads no row, and says so.
            tracing::warn!(
                tenant = slug,
                "retention: archive mode with no {} configured; item_history, \
                 team_page_history and activity_log are not swept, nothing pruned ({})",
                kairos_core::retention::ENV_ARCHIVE_TARGET,
                kairos_core::retention::SKIPPED_NO_TARGET,
            );
            report.skipped = Some(SweepSkip::NoTarget);
            report.item_history.warnings = 1;
            report.activity_log.warnings = 1;
            report.team_page_history.warnings = 1;
            insert_audit_row(conn, &report, now)?;
            return Ok(report);
        }
        (RetentionMode::Archive, Some(target @ ArchiveTarget::S3(url))) => {
            // KAIROS-T-0671: the store is the server's, set at start.
            match (target.s3_location(), config.archive_store.get()) {
                (Some(location), Some(store)) => Some(Target::Store {
                    store: Arc::clone(store),
                    bucket: location.bucket,
                    prefix: location.prefix,
                }),
                _ => return Err(RetentionError::ArchiveTargetNotImplemented(url.clone())),
            }
        }
        (RetentionMode::Archive, Some(ArchiveTarget::Filesystem(base))) => {
            Some(Target::Directory(base.as_path()))
        }
        (RetentionMode::Discard, _) => None,
    };

    let archive_of = |table: &str| target.as_ref().map(|t| t.archive(slug, table, now));
    let mut history_archive = archive_of("item_history");
    let history_delete_ids = plan_history_pages(
        conn,
        HistoryTable::Item,
        slug,
        config,
        now,
        paging,
        history_archive.as_mut(),
        &mut report.history_pages,
    )?;

    // team_page_history: the tiers of item_history (KAIROS-T-0638).
    let mut page_archive = archive_of("team_page_history");
    let page_delete_ids = plan_history_pages(
        conn,
        HistoryTable::TeamPage,
        slug,
        config,
        now,
        paging,
        page_archive.as_mut(),
        &mut report.team_page_history_pages,
    )?;

    // activity_log: plain retention window, no compaction tiers (A-0004 §4).
    let activity_cutoff = now - Duration::days(i64::from(config.activity_retention_days));
    let mut activity_archive = archive_of("activity_log");
    let activity_delete_ids = plan_activity_pages(
        conn,
        activity_cutoff,
        paging,
        activity_archive.as_mut(),
        &mut report,
    )?;

    // The offload is durable before any delete.
    for (archive, counts) in [
        (history_archive, &mut report.item_history),
        (activity_archive, &mut report.activity_log),
        (page_archive, &mut report.team_page_history),
    ] {
        if let Some((path, lines)) = archive.map(LazyArchive::finish).transpose()?.flatten() {
            counts.rows_archived = lines;
            report.archive_files.push(path);
        }
    }

    // Prune + audit in ONE transaction (the archive files above are already
    // durable; a failed transaction leaves at worst an orphan archive copy,
    // never a deleted-but-unarchived row). The deletes go in batches.
    conn.transaction::<_, DieselError, _>(|conn| {
        for (table, ids, counts) in [
            (
                HistoryTable::Item,
                &history_delete_ids,
                &mut report.item_history,
            ),
            (
                HistoryTable::TeamPage,
                &page_delete_ids,
                &mut report.team_page_history,
            ),
        ] {
            for chunk in ids.chunks(paging.batch) {
                let n = sql_query(format!("DELETE FROM {} WHERE id = ANY($1)", table.table()))
                    .bind::<Array<SqlUuid>, _>(chunk)
                    .execute(conn)?;
                counts.rows_pruned += n as u64;
            }
        }
        for chunk in activity_delete_ids.chunks(paging.batch) {
            let n = diesel::delete(activity_log::table.filter(activity_log::id.eq_any(chunk)))
                .execute(conn)?;
            report.activity_log.rows_pruned += n as u64;
        }
        insert_audit_row(conn, &report, now)
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

/// Run `f` in each tenant schema, with `search_path` pinned to it and
/// restored after, and sum the counts.
fn in_each_tenant(
    conn: &mut PgConnection,
    mut f: impl FnMut(&mut PgConnection) -> Result<u64, RetentionError>,
) -> Result<u64, RetentionError> {
    let mut total = 0u64;
    for tenant_info in tenant::list_tenants(conn)? {
        if !tenant_info.schema_exists {
            continue;
        }
        let schema = tenant::tenant_schema_name(&tenant_info.slug);
        sql_query(format!("SET search_path TO \"{schema}\"")).execute(conn)?;
        let result = f(conn);
        let reset = sql_query("SET search_path TO DEFAULT").execute(conn);
        total += result?;
        reset?;
    }
    Ok(total)
}

/// Call `delete_page` with the page size `batch` until a page deletes
/// fewer rows (KAIROS-T-0638): no delete names more than one page of rows.
/// The sum.
fn delete_in_pages(
    conn: &mut PgConnection,
    batch: usize,
    mut delete_page: impl FnMut(&mut PgConnection, i64) -> Result<usize, DieselError>,
) -> Result<u64, DieselError> {
    let mut total = 0u64;
    loop {
        let n = delete_page(conn, batch as i64)?;
        total += n as u64;
        if n < batch {
            return Ok(total);
        }
    }
}

/// Delete, in each tenant, the notifications that were read more than
/// `notifications_read_days` days before `now` (KAIROS-T-0381). In each
/// mode, also `off`: a read notification is not archived. 0 days deletes
/// nothing. Returns the count. The default page size.
pub fn delete_read_notifications(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<u64, RetentionError> {
    delete_read_notifications_with(conn, config, now, SweepPaging::default())
}

fn delete_read_notifications_with(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    if config.notifications_read_days == 0 {
        return Ok(0);
    }
    let cutoff = now - Duration::days(i64::from(config.notifications_read_days));
    in_each_tenant(conn, |conn| {
        Ok(delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::notifications::delete_read_before(conn, cutoff, limit)
        })?)
    })
}

/// Delete, in each tenant, the notifications that nobody read and that
/// were made more than `notifications_unread_days` days before `now`
/// (KAIROS-T-0638). In each mode, also `off`; not archived. 0 days deletes
/// nothing. Returns the count.
fn delete_unread_notifications(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    if config.notifications_unread_days == 0 {
        return Ok(0);
    }
    let cutoff = now - Duration::days(i64::from(config.notifications_unread_days));
    in_each_tenant(conn, |conn| {
        Ok(delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::notifications::delete_unread_before(conn, cutoff, limit)
        })?)
    })
}

/// Delete, in each tenant, the summaries of the code-index pools that no
/// stored index uses (KAIROS-T-0378, KAIROS-T-0638): a summary that no
/// index used and that a build made more than `code_index_pool_days` days
/// before `now`, and a summary whose last use is older than that and older
/// than every stored index of its repository. In each mode, also `off`:
/// such a summary is a cache that a build can make again. 0 days deletes
/// nothing. Returns the count. The default page size.
pub fn delete_unused_summaries(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
) -> Result<u64, RetentionError> {
    delete_unused_summaries_with(conn, config, now, SweepPaging::default())
}

fn delete_unused_summaries_with(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    if config.code_index_pool_days == 0 {
        return Ok(0);
    }
    let cutoff = now - Duration::days(i64::from(config.code_index_pool_days));
    in_each_tenant(conn, |conn| {
        let mut deleted = delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::code_indexes::delete_unused(conn, cutoff, limit)
        })?;
        let mut after = None;
        loop {
            let repositories =
                crate::code_indexes::repositories_page(conn, false, after, paging.items_per_page)?;
            for repository in &repositories {
                deleted += delete_in_pages(conn, paging.batch, |conn, limit| {
                    crate::code_indexes::delete_used_by_no_stored_index(
                        conn,
                        *repository,
                        cutoff,
                        limit,
                    )
                })?;
            }
            if (repositories.len() as i64) < paging.items_per_page {
                return Ok(deleted);
            }
            after = repositories.last().copied();
        }
    })
}

/// Delete, in each tenant, the code indexes past the newest
/// `code_index_keep` of their repository and ref (KAIROS-T-0638). A page is
/// the indexes of up to `items_per_page` repositories, read with no
/// structure; each delete names at most `batch` commits. In each mode,
/// also `off`: a build or an upload makes an index again. 0 keeps every
/// index. Returns the count.
fn delete_old_code_indexes(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    if config.code_index_keep == 0 {
        return Ok(0);
    }
    in_each_tenant(conn, |conn| {
        let mut deleted = 0u64;
        let mut after = None;
        loop {
            let repositories =
                crate::code_indexes::repositories_page(conn, true, after, paging.items_per_page)?;
            let meta = crate::code_indexes::index_meta(conn, &repositories)?;
            let mut by_repository: HashMap<Uuid, Vec<String>> = HashMap::new();
            for key in plan_code_index_retention(&meta, config.code_index_keep) {
                by_repository
                    .entry(key.repository_id)
                    .or_default()
                    .push(key.commit_sha);
            }
            for (repository, commits) in &by_repository {
                for chunk in commits.chunks(paging.batch) {
                    deleted +=
                        crate::code_indexes::delete_indexes(conn, *repository, chunk)? as u64;
                }
            }
            if (repositories.len() as i64) < paging.items_per_page {
                return Ok(deleted);
            }
            after = repositories.last().copied();
        }
    })
}

/// Delete, in each tenant, the webhook deliveries that finished more than
/// [`crate::webhooks::KEEP_DAYS`] days before `now` (KAIROS-T-0638), of
/// each target: the delivery of a target removes only the old rows of
/// that target, so a quiet target kept its log. In each mode, also `off`;
/// not archived. A `skipped` delivery stays as long as the others, so it
/// can be sent again in that time. Returns the count.
fn delete_old_webhook_deliveries(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    let cutoff = now - Duration::days(crate::webhooks::KEEP_DAYS);
    in_each_tenant(conn, |conn| {
        Ok(delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::webhooks::delete_finished_before(conn, cutoff, limit)
        })?)
    })
}

/// What one [`run_retention_pass`] did (KAIROS-T-0366).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPass {
    /// The mode of the pass.
    pub mode: RetentionMode,
    /// One report per tenant (empty when the mode is `off`).
    pub tenants: Vec<SweepReport>,
    /// The expired password sessions (`public.local_sessions`) the pass
    /// deleted.
    pub expired_sessions_deleted: u64,
    /// The rows of the login throttle (`public.auth_failures`) that can
    /// change no decision, which the pass deleted (KAIROS-T-0377).
    pub expired_auth_failures_deleted: u64,
    /// The read notifications that the pass deleted in all the tenants
    /// (KAIROS-T-0381).
    pub read_notifications_deleted: u64,
    /// The unread notifications past their window that the pass deleted in
    /// all the tenants (KAIROS-T-0638).
    pub unread_notifications_deleted: u64,
    /// The code indexes past the newest of their repository and ref that
    /// the pass deleted in all the tenants (KAIROS-T-0638).
    pub code_indexes_deleted: u64,
    /// The summaries of the code-index pools that no stored index used,
    /// which the pass deleted in all the tenants (KAIROS-T-0378).
    pub unused_summaries_deleted: u64,
    /// The finished webhook deliveries past their window that the pass
    /// deleted in all the tenants (KAIROS-T-0638).
    pub webhook_deliveries_deleted: u64,
    /// The request keys past their window of 24 hours that the pass
    /// deleted in all the tenants (KAIROS-T-0681).
    pub request_keys_deleted: u64,
    /// The rows of the MCP refusals older than 90 days that the pass
    /// deleted in all the tenants (KAIROS-T-0694).
    pub tool_refusals_deleted: u64,
}

impl RetentionPass {
    /// The record of the pass for its job (KAIROS-T-0631):
    /// `skipped: no target` when a sweep was skipped, `off` in the mode
    /// `off`, else the rows that the sweeps archived and pruned.
    pub fn result(&self) -> String {
        if let Some(skip) = self.tenants.iter().find_map(|t| t.skipped) {
            return skip.as_str().to_string();
        }
        if self.mode == RetentionMode::Off {
            return "off".to_string();
        }
        let sum = |f: fn(&SweepReport) -> u64| self.tenants.iter().map(f).sum::<u64>();
        format!(
            "swept {} tenants: item_history archived {} pruned {}, activity_log archived {} \
             pruned {}, team_page_history archived {} pruned {}",
            self.tenants.len(),
            sum(|t| t.item_history.rows_archived),
            sum(|t| t.item_history.rows_pruned),
            sum(|t| t.activity_log.rows_archived),
            sum(|t| t.activity_log.rows_pruned),
            sum(|t| t.team_page_history.rows_archived),
            sum(|t| t.team_page_history.rows_pruned),
        )
    }
}

/// One retention pass of the server (KAIROS-T-0366): sweep each tenant (as
/// [`sweep_all_tenants`], which obeys the mode) and delete the password
/// sessions that expired before `now`. The session delete runs in every
/// mode, also `off`: an expired session cannot authenticate, so the row has
/// no use. So does the delete of the rows of the login throttle whose
/// window and lockout ended (KAIROS-T-0377), of the read notifications
/// (KAIROS-T-0381), of the unread notifications, the old code indexes and
/// the old webhook deliveries (KAIROS-T-0638), of the unused summaries
/// of the code-index pools (KAIROS-T-0378), of the request keys
/// (KAIROS-T-0681) and of the MCP refusals (KAIROS-T-0694). `progress` is called before
/// each tenant and after each delete of a tenant table.
///
/// The pass is a periodic job of the job queue of the server
/// (KAIROS-T-0376): the queue gives each run to one server, so the pass
/// takes no lock of its own. The default [`SweepPaging`]; see
/// [`run_retention_pass_with`].
pub fn run_retention_pass(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    progress: &mut dyn FnMut(),
) -> Result<RetentionPass, RetentionError> {
    run_retention_pass_with(conn, config, now, SweepPaging::default(), progress)
}

/// [`run_retention_pass`] with the page and batch sizes `paging`.
pub fn run_retention_pass_with(
    conn: &mut PgConnection,
    config: &RetentionConfig,
    now: DateTime<Utc>,
    paging: SweepPaging,
    progress: &mut dyn FnMut(),
) -> Result<RetentionPass, RetentionError> {
    let paging = paging.checked();
    let mut tenants = Vec::new();
    if config.mode != RetentionMode::Off {
        for tenant_info in tenant::list_tenants(conn)? {
            progress();
            if !tenant_info.schema_exists {
                return Err(RetentionError::TenantNotFound(tenant_info.slug));
            }
            tenants.push(sweep_tenant_with(
                conn,
                &tenant_info.slug,
                config,
                now,
                paging,
            )?);
        }
    }
    let expired = crate::local_auth::delete_expired_sessions(conn, now)?;
    let throttle = crate::auth_failures::delete_expired(conn, now)?;
    let read = delete_read_notifications_with(conn, config, now, paging)?;
    progress();
    let unread = delete_unread_notifications(conn, config, now, paging)?;
    progress();
    // The indexes first, so the summaries that only they used go in the
    // same pass.
    let indexes = delete_old_code_indexes(conn, config, paging)?;
    progress();
    let summaries = delete_unused_summaries_with(conn, config, now, paging)?;
    progress();
    let deliveries = delete_old_webhook_deliveries(conn, now, paging)?;
    progress();
    let request_keys = delete_old_request_keys(conn, now, paging)?;
    progress();
    let refusals = delete_old_tool_refusals(conn, now, paging)?;
    progress();
    Ok(RetentionPass {
        mode: config.mode,
        tenants,
        expired_sessions_deleted: expired as u64,
        expired_auth_failures_deleted: throttle as u64,
        read_notifications_deleted: read,
        unread_notifications_deleted: unread,
        code_indexes_deleted: indexes,
        unused_summaries_deleted: summaries,
        webhook_deliveries_deleted: deliveries,
        request_keys_deleted: request_keys,
        tool_refusals_deleted: refusals,
    })
}

/// Delete, in each tenant, the rows of the MCP refusals made more than
/// [`crate::tool_refusals::KEEP_DAYS`] days before `now` (KAIROS-T-0694,
/// KAIROS-A-0068). In each mode, also `off`. Not archived. Returns the
/// count.
fn delete_old_tool_refusals(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    let cutoff = now - crate::tool_refusals::keep();
    in_each_tenant(conn, |conn| {
        Ok(delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::tool_refusals::delete_before(conn, cutoff, limit)
        })?)
    })
}

/// Delete, in each tenant, the request keys made more than
/// [`crate::request_keys::WINDOW_HOURS`] hours before `now`
/// (KAIROS-T-0681). In each mode, also `off`: a key past its window
/// replays nothing. Not archived. Returns the count.
fn delete_old_request_keys(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    paging: SweepPaging,
) -> Result<u64, RetentionError> {
    let cutoff = now - crate::request_keys::window();
    in_each_tenant(conn, |conn| {
        Ok(delete_in_pages(conn, paging.batch, |conn, limit| {
            crate::request_keys::delete_before(conn, cutoff, limit)
        })?)
    })
}
