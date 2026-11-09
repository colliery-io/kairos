//! Retention policy (KAIROS-A-0004 "History Retention" amendment, config
//! surface per KAIROS-A-0013): the pure configuration model and the pure
//! compaction planner for `item_history`.
//!
//! Per KAIROS-A-0009 this crate performs no I/O. Configuration parsing is
//! written against an injected lookup function
//! ([`RetentionConfig::from_lookup`]); the [`RetentionConfig::from_env`]
//! convenience reads the process environment (the crate's only environment
//! touchpoint, no file/network I/O). The planner
//! ([`plan_history_compaction`]) is a pure function of the loaded history
//! metadata, an injected `now`, and the config — never the wall clock — so
//! every tier boundary is unit-testable with fixed clocks.
//!
//! # Tiers (KAIROS-A-0004)
//!
//! 1. **Hot window** (default 90 days): every snapshot with
//!    `edited_at >= now - hot_days` is retained untouched.
//! 2. **Compaction** (past the hot window): per item, per calendar month
//!    (UTC), only the first and last snapshot are kept; intermediate
//!    versions become prune candidates.
//! 3. **Latest-N guard** (default 5): the latest N versions of an item are
//!    NEVER prune candidates regardless of age, so rollback always has
//!    recent material.
//!
//! Offload-before-prune and the `KAIROS_RETENTION_MODE` semantics are
//! enforced by the I/O layer (`kairos_db::retention`); the planner only
//! decides WHICH rows are candidates.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Utc};
use uuid::Uuid;

/// Env var: hot-window length in days (default 90). KAIROS-A-0004/A-0013.
pub const ENV_HISTORY_HOT_DAYS: &str = "KAIROS_HISTORY_HOT_DAYS";
/// Env var: latest-N guard size (default 5). KAIROS-A-0004/A-0013.
pub const ENV_HISTORY_KEEP_LATEST: &str = "KAIROS_HISTORY_KEEP_LATEST";
/// Env var: `activity_log` retention window in days (default 365).
pub const ENV_ACTIVITY_RETENTION_DAYS: &str = "KAIROS_ACTIVITY_RETENTION_DAYS";
/// The `activity_log` actions that the sweep keeps at each age
/// (KAIROS-T-0571): the rows that move an item. The flow metrics read them
/// for the start of the work, its done time and the age of the work in
/// progress, so a sweep must not remove them.
pub const KEPT_ACTIVITY_ACTIONS: [&str; 3] = ["transition", "cancel", "board_move"];
/// Env var: archive target (filesystem path or `s3://…` URL); unset/empty
/// means no target is configured.
pub const ENV_ARCHIVE_TARGET: &str = "KAIROS_ARCHIVE_TARGET";
/// Env var: the endpoint of an S3-compatible store for an `s3://` archive
/// target (KAIROS-T-0671). Unset: AWS.
pub const ENV_ARCHIVE_S3_ENDPOINT: &str = "KAIROS_ARCHIVE_S3_ENDPOINT";
/// Env var: the region of the bucket of an `s3://` archive target
/// (KAIROS-T-0671). Unset: the AWS environment and the instance decide.
pub const ENV_ARCHIVE_S3_REGION: &str = "KAIROS_ARCHIVE_S3_REGION";
/// Env var: retention mode, one of `archive|discard|off` (default
/// `archive` — the safe default: without a target nothing is destroyed).
pub const ENV_RETENTION_MODE: &str = "KAIROS_RETENTION_MODE";
/// Env var: the days a read notification stays (default 30, KAIROS-T-0381);
/// 0 keeps read notifications.
pub const ENV_NOTIFICATIONS_READ_DAYS: &str = "KAIROS_NOTIFICATIONS_READ_DAYS";
/// Env var: the days that a summary of the code-index pool that no stored
/// index has used stays (default 30, KAIROS-T-0378); 0 keeps them.
pub const ENV_CODE_INDEX_POOL_DAYS: &str = "KAIROS_CODE_INDEX_POOL_DAYS";
/// Env var: the code indexes that stay per repository and branch ref, the
/// newest by last write (default 10, KAIROS-T-0638, KAIROS-T-0751); 0 keeps
/// every index.
pub const ENV_CODE_INDEX_KEEP: &str = "KAIROS_CODE_INDEX_KEEP";
/// Env var: the days a pull request index stays after its ref was last a
/// target of the builder (default 7, KAIROS-T-0751); 0 keeps them.
pub const ENV_CODE_INDEX_PR_DAYS: &str = "KAIROS_CODE_INDEX_PR_DAYS";
/// Env var: the days an unread notification stays (default 180,
/// KAIROS-T-0638); 0 keeps unread notifications.
pub const ENV_NOTIFICATIONS_UNREAD_DAYS: &str = "KAIROS_NOTIFICATIONS_UNREAD_DAYS";

/// Errors from parsing the retention environment configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RetentionConfigError {
    /// A day/count variable did not parse as an unsigned integer.
    #[error("The value {value:?} of {var} is not an integer.")]
    InvalidInt { var: &'static str, value: String },
    /// `KAIROS_RETENTION_MODE` was not one of `archive|discard|off`.
    #[error(
        "The value {value:?} of {ENV_RETENTION_MODE} is not correct. The values are: archive, discard, off."
    )]
    InvalidMode { value: String },
    /// `KAIROS_ARCHIVE_TARGET` is an `s3://` URL with no bucket
    /// (KAIROS-T-0671).
    #[error(
        "The value {value:?} of {ENV_ARCHIVE_TARGET} has no bucket. Use s3://bucket or s3://bucket/prefix."
    )]
    InvalidTarget { value: String },
}

/// `KAIROS_RETENTION_MODE` (KAIROS-A-0004): what the sweeper does with
/// prune candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionMode {
    /// Offload candidates as NDJSON to the archive target, then prune. With
    /// NO target configured, nothing is pruned — the sweep reads no row, and
    /// a warning is logged, counted and recorded as [`SKIPPED_NO_TARGET`]
    /// instead (data is never destroyed without an offload copy,
    /// KAIROS-T-0631).
    Archive,
    /// Prune candidates WITHOUT archiving (explicit operator opt-in to data
    /// destruction).
    Discard,
    /// The sweeper is disabled entirely (no prune, no archive, no
    /// `retention_sweep` activity row).
    Off,
}

impl RetentionMode {
    /// The `KAIROS_RETENTION_MODE` string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            RetentionMode::Archive => "archive",
            RetentionMode::Discard => "discard",
            RetentionMode::Off => "off",
        }
    }
}

impl fmt::Display for RetentionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RetentionMode {
    type Err = RetentionConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.trim();
        if value.eq_ignore_ascii_case("archive") {
            Ok(RetentionMode::Archive)
        } else if value.eq_ignore_ascii_case("discard") {
            Ok(RetentionMode::Discard)
        } else if value.eq_ignore_ascii_case("off") {
            Ok(RetentionMode::Off)
        } else {
            Err(RetentionConfigError::InvalidMode {
                value: s.to_string(),
            })
        }
    }
}

/// `KAIROS_ARCHIVE_TARGET` (KAIROS-A-0004): where pruned rows are offloaded
/// as NDJSON before deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveTarget {
    /// A local/mounted filesystem directory (implemented in v1).
    Filesystem(PathBuf),
    /// An S3-compatible URL (`s3://bucket/prefix`). Recognized by the
    /// config so operators get a TYPED not-implemented error rather than a
    /// misparse; the v1 sweeper does not ship an S3 client
    /// (KAIROS-T-0015 interpretation — no object-store SDK dependency).
    S3(String),
}

impl ArchiveTarget {
    /// Parse an archive target string. `s3://…` becomes [`ArchiveTarget::S3`];
    /// anything else is a filesystem directory (an optional `file://` prefix
    /// is stripped). Empty/whitespace input means "no target configured"
    /// and returns `None`.
    pub fn parse(raw: &str) -> Option<ArchiveTarget> {
        let value = raw.trim();
        if value.is_empty() {
            None
        } else if value.len() >= 5 && value[..5].eq_ignore_ascii_case("s3://") {
            Some(ArchiveTarget::S3(value.to_string()))
        } else {
            let path = value.strip_prefix("file://").unwrap_or(value);
            Some(ArchiveTarget::Filesystem(PathBuf::from(path)))
        }
    }
}

/// The bucket and the key prefix of an `s3://` archive target
/// (KAIROS-T-0671). The prefix has no slash at either end, and may be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Location {
    pub bucket: String,
    pub prefix: String,
}

impl ArchiveTarget {
    /// The bucket and the prefix of an `s3://bucket/prefix` target. `None`
    /// for a filesystem target and for an S3 URL with no bucket.
    pub fn s3_location(&self) -> Option<S3Location> {
        let ArchiveTarget::S3(url) = self else {
            return None;
        };
        let rest = url.get(5..)?;
        let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));
        (!bucket.trim().is_empty()).then(|| S3Location {
            bucket: bucket.trim().to_string(),
            prefix: prefix.trim().trim_matches('/').to_string(),
        })
    }
}

/// Where a sweep puts an archive file that its target keeps in an object
/// store (KAIROS-T-0671). `kairos-core` does no I/O, so the server gives
/// the implementation (a S3 store); `kairos-db` calls it.
pub trait ArchiveStore: Send + Sync {
    /// Upload the file at `file` as the object `key` (the prefix of the
    /// target is part of the key). The call returns when the object is
    /// stored: a sweep deletes its rows after it.
    fn put_file(&self, key: &str, file: &Path) -> Result<(), String>;
}

/// The store of an `s3://` target, when the server has made one.
#[derive(Clone, Default)]
pub struct ArchiveStoreHandle(Option<Arc<dyn ArchiveStore>>);

impl ArchiveStoreHandle {
    pub fn new(store: Arc<dyn ArchiveStore>) -> Self {
        ArchiveStoreHandle(Some(store))
    }

    pub fn get(&self) -> Option<&Arc<dyn ArchiveStore>> {
        self.0.as_ref()
    }
}

impl fmt::Debug for ArchiveStoreHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "ArchiveStoreHandle(set)"
        } else {
            "ArchiveStoreHandle(none)"
        })
    }
}

impl PartialEq for ArchiveStoreHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for ArchiveStoreHandle {}

/// The KAIROS-A-0004 retention policy knobs (env surface per
/// KAIROS-A-0013; see the `ENV_*` constants).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionConfig {
    /// Hot window in days: snapshots younger than this are never touched.
    pub history_hot_days: u32,
    /// The latest N versions per item are never pruned regardless of age.
    pub history_keep_latest: u32,
    /// `activity_log` rows older than this many days are archive-then-
    /// deleted (no compaction tiers).
    pub activity_retention_days: u32,
    /// Where pruned rows are offloaded; `None` = not configured (in
    /// [`RetentionMode::Archive`] this pauses pruning with a warning).
    pub archive_target: Option<ArchiveTarget>,
    /// What to do with prune candidates.
    pub mode: RetentionMode,
    /// A read notification is deleted this many days after it was read
    /// (KAIROS-T-0381). 0 keeps read notifications. Not archived, in each
    /// mode: a read notification is a copy of a fact that the item keeps.
    pub notifications_read_days: u32,
    /// A summary of the code-index pool of a repository that no stored
    /// index has used is deleted this many days after a build made it
    /// (KAIROS-T-0378). 0 keeps them. In each mode: such a summary is a
    /// cache that a build can make again.
    pub code_index_pool_days: u32,
    /// The code indexes that stay per repository and branch ref, the
    /// newest by last write (KAIROS-T-0638, KAIROS-T-0751: 10 for the
    /// default branch). 0 keeps every index. In each mode, not archived: a
    /// build or an upload makes an index again.
    pub code_index_keep: u32,
    /// The days a pull request index stays after its ref was last a target
    /// of the builder (KAIROS-T-0751): the pull request merged, closed past
    /// its window, or got a new head. 0 keeps the pull request indexes. A
    /// commit that a tag or a branch points at stays whatever this says.
    pub code_index_pr_days: u32,
    /// An unread notification is deleted this many days after it was made
    /// (KAIROS-T-0638). 0 keeps unread notifications. In each mode, not
    /// archived, as a read notification.
    pub notifications_unread_days: u32,
    /// `KAIROS_ARCHIVE_S3_ENDPOINT` (KAIROS-T-0671): the endpoint of an
    /// S3-compatible store, for an `s3://` target.
    pub archive_s3_endpoint: Option<String>,
    /// `KAIROS_ARCHIVE_S3_REGION` (KAIROS-T-0671): the region of the
    /// bucket, for an `s3://` target.
    pub archive_s3_region: Option<String>,
    /// The store of an `s3://` target. The server sets it at start; a
    /// config from the environment has none, and a sweep to an `s3://` target
    /// with none fails with a typed error.
    pub archive_store: ArchiveStoreHandle,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        RetentionConfig {
            history_hot_days: 90,
            history_keep_latest: 5,
            activity_retention_days: 365,
            archive_target: None,
            mode: RetentionMode::Archive,
            notifications_read_days: 30,
            code_index_pool_days: 30,
            code_index_keep: 10,
            code_index_pr_days: 7,
            notifications_unread_days: 180,
            archive_s3_endpoint: None,
            archive_s3_region: None,
            archive_store: ArchiveStoreHandle::default(),
        }
    }
}

fn parse_u32(var: &'static str, value: &str) -> Result<u32, RetentionConfigError> {
    value
        .trim()
        .parse()
        .map_err(|_| RetentionConfigError::InvalidInt {
            var,
            value: value.to_string(),
        })
}

impl RetentionConfig {
    /// Build the config from an injected variable lookup (pure — the unit
    /// tests drive this with fixed maps). Unset variables take the
    /// KAIROS-A-0004 defaults; set-but-invalid values are typed errors, not
    /// silent fallbacks (fail-fast per KAIROS-A-0013).
    pub fn from_lookup<F>(lookup: F) -> Result<Self, RetentionConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut config = RetentionConfig::default();
        if let Some(value) = lookup(ENV_HISTORY_HOT_DAYS) {
            config.history_hot_days = parse_u32(ENV_HISTORY_HOT_DAYS, &value)?;
        }
        if let Some(value) = lookup(ENV_HISTORY_KEEP_LATEST) {
            config.history_keep_latest = parse_u32(ENV_HISTORY_KEEP_LATEST, &value)?;
        }
        if let Some(value) = lookup(ENV_ACTIVITY_RETENTION_DAYS) {
            config.activity_retention_days = parse_u32(ENV_ACTIVITY_RETENTION_DAYS, &value)?;
        }
        if let Some(value) = lookup(ENV_ARCHIVE_TARGET) {
            config.archive_target = ArchiveTarget::parse(&value);
            if let Some(target @ ArchiveTarget::S3(url)) = &config.archive_target
                && target.s3_location().is_none()
            {
                return Err(RetentionConfigError::InvalidTarget { value: url.clone() });
            }
        }
        let non_empty = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        config.archive_s3_endpoint = non_empty(ENV_ARCHIVE_S3_ENDPOINT);
        config.archive_s3_region = non_empty(ENV_ARCHIVE_S3_REGION);
        if let Some(value) = lookup(ENV_RETENTION_MODE) {
            config.mode = value.parse()?;
        }
        if let Some(value) = lookup(ENV_NOTIFICATIONS_READ_DAYS) {
            config.notifications_read_days = parse_u32(ENV_NOTIFICATIONS_READ_DAYS, &value)?;
        }
        if let Some(value) = lookup(ENV_CODE_INDEX_POOL_DAYS) {
            config.code_index_pool_days = parse_u32(ENV_CODE_INDEX_POOL_DAYS, &value)?;
        }
        if let Some(value) = lookup(ENV_CODE_INDEX_KEEP) {
            config.code_index_keep = parse_u32(ENV_CODE_INDEX_KEEP, &value)?;
        }
        if let Some(value) = lookup(ENV_CODE_INDEX_PR_DAYS) {
            config.code_index_pr_days = parse_u32(ENV_CODE_INDEX_PR_DAYS, &value)?;
        }
        if let Some(value) = lookup(ENV_NOTIFICATIONS_UNREAD_DAYS) {
            config.notifications_unread_days = parse_u32(ENV_NOTIFICATIONS_UNREAD_DAYS, &value)?;
        }
        Ok(config)
    }

    /// [`RetentionConfig::from_lookup`] over the process environment.
    pub fn from_env() -> Result<Self, RetentionConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

/// The record of a sweep that `archive` mode with no target skipped
/// (KAIROS-T-0631): the audit row of each tenant and the job of the pass
/// carry it.
pub const SKIPPED_NO_TARGET: &str = "skipped: no target";

/// What a retention pass with a config does (KAIROS-T-0631).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionStatus {
    /// The pass archives (or discards) the candidates and prunes them.
    Prunes,
    /// `off`: the sweeper does not run.
    Off,
    /// `archive` with no target: the pass reads no row of `item_history`,
    /// `team_page_history` and `activity_log`, and prunes nothing.
    NoTarget,
    /// `archive` with an `s3://` target and no store for it: each pass
    /// fails. The server makes the store at start, so a server does not
    /// report this (KAIROS-T-0671).
    TargetNotImplemented,
}

impl RetentionStatus {
    /// The wire name of the status.
    pub fn as_str(self) -> &'static str {
        match self {
            RetentionStatus::Prunes => "prunes",
            RetentionStatus::Off => "off",
            RetentionStatus::NoTarget => "no_target",
            RetentionStatus::TargetNotImplemented => "target_not_implemented",
        }
    }
}

impl RetentionConfig {
    /// What a pass with this config does.
    pub fn status(&self) -> RetentionStatus {
        match (self.mode, &self.archive_target) {
            (RetentionMode::Off, _) => RetentionStatus::Off,
            (RetentionMode::Discard, _) => RetentionStatus::Prunes,
            (RetentionMode::Archive, None) => RetentionStatus::NoTarget,
            (RetentionMode::Archive, Some(ArchiveTarget::S3(_))) => {
                if self.archive_store.get().is_some() {
                    RetentionStatus::Prunes
                } else {
                    RetentionStatus::TargetNotImplemented
                }
            }
            (RetentionMode::Archive, Some(ArchiveTarget::Filesystem(_))) => RetentionStatus::Prunes,
        }
    }

    /// The warning for an operator when the pass prunes nothing or fails
    /// by its config (KAIROS-T-0631): `/readyz` and the Admin overview
    /// show it. `None`: the config is in order.
    pub fn warning(&self) -> Option<String> {
        match self.status() {
            RetentionStatus::Prunes | RetentionStatus::Off => None,
            RetentionStatus::NoTarget => Some(format!(
                "{ENV_RETENTION_MODE} is archive and {ENV_ARCHIVE_TARGET} is not set. \
                 The retention pass removes no rows from item_history, team_page_history \
                 and activity_log. \
                 Set {ENV_ARCHIVE_TARGET} to a directory, or set {ENV_RETENTION_MODE} to discard."
            )),
            RetentionStatus::TargetNotImplemented => Some(format!(
                "{ENV_ARCHIVE_TARGET} is an s3:// URL, and no S3 store is set up for it, so \
                 each retention pass fails. Set {ENV_ARCHIVE_TARGET} to a directory."
            )),
        }
    }
}

/// The planner's view of one `item_history` row: `(item_id, version,
/// edited_at, base_version)`. Loading and full-fidelity serialization stay
/// in the I/O layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRowMeta {
    pub item_id: Uuid,
    pub version: i32,
    pub edited_at: DateTime<Utc>,
    /// The full row that this diff row applies to, or `None` for a full
    /// row (KAIROS-T-0651, [`crate::history_diff`]).
    pub base_version: Option<i32>,
}

/// A row the planner selected for offload-then-prune, identified by the
/// `item_history` natural key `(item_id, version)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PruneKey {
    pub item_id: Uuid,
    pub version: i32,
}

/// The pure KAIROS-A-0004 compaction planner: given every history row's
/// metadata, an injected `now`, and the config, compute the prune set.
///
/// - Rows with `edited_at >= now - history_hot_days` (hot window,
///   inclusive at the boundary) are kept.
/// - Older rows are grouped per item per UTC calendar month; the first and
///   last of each month group (by `edited_at`, version as tiebreaker) are
///   kept, the middles are prune candidates.
/// - The latest `history_keep_latest` versions per item (by version
///   number) are never candidates, regardless of age.
/// - The base full row of each kept diff row is kept (KAIROS-T-0651), so
///   each kept version keeps its text.
///
/// The result is sorted by `(item_id, version)` and is deterministic. The
/// planner is mode-agnostic: callers gate on [`RetentionConfig::mode`]
/// before acting.
pub fn plan_history_compaction(
    rows: &[HistoryRowMeta],
    now: DateTime<Utc>,
    config: &RetentionConfig,
) -> Vec<PruneKey> {
    let hot_cutoff = now - Duration::days(i64::from(config.history_hot_days));

    let mut by_item: BTreeMap<Uuid, Vec<HistoryRowMeta>> = BTreeMap::new();
    for row in rows {
        by_item.entry(row.item_id).or_default().push(*row);
    }

    let mut prune = Vec::new();
    for (_, mut item_rows) in by_item {
        // `(item_id, version)` is UNIQUE in the DDL, so version order is
        // total within an item; the latest N versions are the guard set.
        item_rows.sort_by_key(|r| r.version);
        let guard_from = item_rows
            .len()
            .saturating_sub(config.history_keep_latest as usize);
        let guarded: BTreeSet<i32> = item_rows[guard_from..].iter().map(|r| r.version).collect();

        // Group the past-window rows per UTC calendar month.
        let mut by_month: BTreeMap<(i32, u32), Vec<HistoryRowMeta>> = BTreeMap::new();
        for row in &item_rows {
            if row.edited_at < hot_cutoff {
                by_month
                    .entry((row.edited_at.year(), row.edited_at.month()))
                    .or_default()
                    .push(*row);
            }
        }

        let mut item_prune: BTreeSet<i32> = BTreeSet::new();
        for (_, mut month_rows) in by_month {
            month_rows.sort_by_key(|r| (r.edited_at, r.version));
            if month_rows.len() <= 2 {
                continue; // first == last or first+last: nothing between them
            }
            for row in &month_rows[1..month_rows.len() - 1] {
                if !guarded.contains(&row.version) {
                    item_prune.insert(row.version);
                }
            }
        }
        // KAIROS-T-0651: a kept diff row needs its base full row. A base is
        // a full row, so one pass is enough.
        for row in &item_rows {
            if let Some(base) = row.base_version
                && !item_prune.contains(&row.version)
            {
                item_prune.remove(&base);
            }
        }
        if let Some(first) = item_rows.first() {
            prune.extend(item_prune.into_iter().map(|version| PruneKey {
                item_id: first.item_id,
                version,
            }));
        }
    }

    prune.sort();
    prune
}

/// The planner's view of one `code_indexes` row (KAIROS-T-0638): the key,
/// the ref and the time of the last write. Never the structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexMeta {
    pub repository_id: Uuid,
    pub commit_sha: String,
    pub ref_name: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// A code index that the planner selected for delete: `(repository_id,
/// commit_sha)`, the key of `code_indexes`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeIndexKey {
    pub repository_id: Uuid,
    pub commit_sha: String,
}

/// A ref that points at a stored code index (KAIROS-T-0750): the rows of
/// `code_index_refs`. `kind` is `branch`, `pull` or `tag`; `updated_at` is
/// when the builder last saw the ref as a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexRefMeta {
    pub repository_id: Uuid,
    pub ref_name: String,
    pub kind: String,
    pub commit_sha: String,
    pub updated_at: DateTime<Utc>,
}

/// The kinds of a ref in `code_index_refs` (KAIROS-T-0750): the default
/// branch (or an uploaded branch), a pull request head, a tag.
pub const REF_KIND_BRANCH: &str = "branch";
pub const REF_KIND_PULL: &str = "pull";
pub const REF_KIND_TAG: &str = "tag";

/// Whether a ref name is a pull request or a merge request head
/// (KAIROS-T-0750): `pull/12`, `merge-requests/12`, or the full ref
/// `refs/pull/12/head`, `refs/merge-requests/12/head`.
pub fn is_pull_ref_name(name: &str) -> bool {
    name.starts_with("pull/")
        || name.starts_with("merge-requests/")
        || name.starts_with("refs/pull/")
        || name.starts_with("refs/merge-requests/")
}

/// Whether the ref a build was for names a pull request.
fn is_pull_ref(name: Option<&str>) -> bool {
    name.is_some_and(is_pull_ref_name)
}

/// The code indexes to delete (KAIROS-T-0638, KAIROS-T-0751), sorted:
///
/// - A commit that a tag or a branch ref points at (`refs`) stays. It
///   counts among the newest `keep` of its group.
/// - An index built for a pull request ref stays while its ref was a target
///   of the builder in the last `pr_days` days and still points at it, or
///   while the index itself was written in the last `pr_days` days. Else it
///   goes: the pull request merged, closed past its window, or moved on.
///   `pr_days` 0 keeps the pull request indexes.
/// - The other indexes keep the newest `keep` per repository and ref (the
///   default branch, an uploaded branch, no ref), by the time of the last
///   write, ties by sha.
/// - `keep` 0 keeps every index.
pub fn plan_code_index_retention(
    rows: &[CodeIndexMeta],
    refs: &[CodeIndexRefMeta],
    keep: u32,
    pr_days: u32,
    now: DateTime<Utc>,
) -> Vec<CodeIndexKey> {
    if keep == 0 {
        return Vec::new();
    }
    let grace = Duration::days(i64::from(pr_days));
    let mut protected: HashSet<(Uuid, &str)> = HashSet::new();
    let mut fresh_pull_heads: HashSet<(Uuid, &str)> = HashSet::new();
    for r in refs {
        let key = (r.repository_id, r.commit_sha.as_str());
        if r.kind == REF_KIND_PULL {
            if pr_days == 0 || r.updated_at + grace >= now {
                fresh_pull_heads.insert(key);
            }
        } else {
            protected.insert(key);
        }
    }
    let mut prune: Vec<CodeIndexKey> = Vec::new();
    let mut groups: BTreeMap<(Uuid, Option<&str>), Vec<&CodeIndexMeta>> = BTreeMap::new();
    for row in rows {
        let key = (row.repository_id, row.commit_sha.as_str());
        if is_pull_ref(row.ref_name.as_deref()) {
            if protected.contains(&key) {
                continue;
            }
            if pr_days == 0 || fresh_pull_heads.contains(&key) || row.updated_at + grace >= now {
                continue;
            }
            prune.push(CodeIndexKey {
                repository_id: row.repository_id,
                commit_sha: row.commit_sha.clone(),
            });
            continue;
        }
        groups
            .entry((row.repository_id, row.ref_name.as_deref()))
            .or_default()
            .push(row);
    }
    // A protected commit never goes. One among the newest `keep` of its
    // group counts as one of them; an older one stays in addition.
    prune.extend(groups.into_values().flat_map(|mut group| {
        group.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.commit_sha.cmp(&a.commit_sha))
        });
        group
            .into_iter()
            .skip(keep as usize)
            .filter(|row| !protected.contains(&(row.repository_id, row.commit_sha.as_str())))
            .map(|row| CodeIndexKey {
                repository_id: row.repository_id,
                commit_sha: row.commit_sha.clone(),
            })
            .collect::<Vec<_>>()
    }));
    prune.sort();
    prune
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::TimeZone;

    use super::*;

    /// Fixed injected clock for every planner test.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap()
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn row(item: Uuid, version: i32, edited_at: DateTime<Utc>) -> HistoryRowMeta {
        HistoryRowMeta {
            item_id: item,
            version,
            edited_at,
            base_version: None,
        }
    }

    fn config(hot_days: u32, keep_latest: u32) -> RetentionConfig {
        RetentionConfig {
            history_hot_days: hot_days,
            history_keep_latest: keep_latest,
            ..RetentionConfig::default()
        }
    }

    fn pruned_versions(plan: &[PruneKey], item: Uuid) -> Vec<i32> {
        plan.iter()
            .filter(|k| k.item_id == item)
            .map(|k| k.version)
            .collect()
    }

    // ---- config parsing --------------------------------------------------

    #[test]
    fn config_defaults_when_nothing_is_set() {
        let config = RetentionConfig::from_lookup(|_| None).unwrap();
        assert_eq!(
            config,
            RetentionConfig {
                history_hot_days: 90,
                history_keep_latest: 5,
                activity_retention_days: 365,
                archive_target: None,
                mode: RetentionMode::Archive,
                notifications_read_days: 30,
                code_index_pool_days: 30,
                code_index_keep: 10,
                code_index_pr_days: 7,
                notifications_unread_days: 180,
                archive_s3_endpoint: None,
                archive_s3_region: None,
                archive_store: ArchiveStoreHandle::default(),
            }
        );
    }

    #[test]
    fn config_reads_every_documented_env_var() {
        let vars: HashMap<&str, &str> = HashMap::from([
            (ENV_HISTORY_HOT_DAYS, "30"),
            (ENV_HISTORY_KEEP_LATEST, "7"),
            (ENV_ACTIVITY_RETENTION_DAYS, "180"),
            (ENV_ARCHIVE_TARGET, "/var/lib/kairos/archive"),
            (ENV_RETENTION_MODE, "discard"),
            (ENV_NOTIFICATIONS_READ_DAYS, "0"),
            (ENV_CODE_INDEX_POOL_DAYS, "7"),
            (ENV_CODE_INDEX_KEEP, "2"),
            (ENV_CODE_INDEX_PR_DAYS, "3"),
            (ENV_NOTIFICATIONS_UNREAD_DAYS, "0"),
        ]);
        let config =
            RetentionConfig::from_lookup(|name| vars.get(name).map(ToString::to_string)).unwrap();
        assert_eq!(config.history_hot_days, 30);
        assert_eq!(config.history_keep_latest, 7);
        assert_eq!(config.activity_retention_days, 180);
        assert_eq!(
            config.archive_target,
            Some(ArchiveTarget::Filesystem(PathBuf::from(
                "/var/lib/kairos/archive"
            )))
        );
        assert_eq!(config.mode, RetentionMode::Discard);
        assert_eq!(config.notifications_read_days, 0);
        assert_eq!(config.code_index_pool_days, 7);
        assert_eq!(config.code_index_keep, 2);
        assert_eq!(config.code_index_pr_days, 3);
        assert_eq!(config.notifications_unread_days, 0);
    }

    #[test]
    fn config_rejects_bad_integers_and_modes() {
        let err = RetentionConfig::from_lookup(|name| {
            (name == ENV_HISTORY_HOT_DAYS).then(|| "ninety".to_string())
        })
        .unwrap_err();
        assert_eq!(
            err,
            RetentionConfigError::InvalidInt {
                var: ENV_HISTORY_HOT_DAYS,
                value: "ninety".to_string()
            }
        );

        let err = RetentionConfig::from_lookup(|name| {
            (name == ENV_RETENTION_MODE).then(|| "purge".to_string())
        })
        .unwrap_err();
        assert_eq!(
            err,
            RetentionConfigError::InvalidMode {
                value: "purge".to_string()
            }
        );
    }

    #[test]
    fn mode_parsing_is_case_insensitive_and_trimmed() {
        assert_eq!(
            " Archive ".parse::<RetentionMode>().unwrap(),
            RetentionMode::Archive
        );
        assert_eq!("OFF".parse::<RetentionMode>().unwrap(), RetentionMode::Off);
        assert_eq!(RetentionMode::Discard.to_string(), "discard");
    }

    #[test]
    fn an_s3_target_has_a_bucket_and_a_prefix() {
        let at = |raw: &str| ArchiveTarget::parse(raw).and_then(|t| t.s3_location());
        assert_eq!(
            at("s3://bucket/some/prefix/"),
            Some(S3Location {
                bucket: "bucket".into(),
                prefix: "some/prefix".into()
            })
        );
        assert_eq!(
            at("S3://bucket"),
            Some(S3Location {
                bucket: "bucket".into(),
                prefix: String::new()
            })
        );
        assert_eq!(at("s3://"), None);
        assert_eq!(at("s3:///prefix"), None);
        assert_eq!(at("/var/archive"), None);
    }

    #[test]
    fn an_s3_target_with_no_bucket_is_refused_and_the_s3_settings_are_read() {
        let lookup = |vars: &'static [(&'static str, &'static str)]| {
            RetentionConfig::from_lookup(move |name| {
                vars.iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            })
        };
        let err = lookup(&[(ENV_ARCHIVE_TARGET, "s3:///prefix")]).unwrap_err();
        assert_eq!(
            err,
            RetentionConfigError::InvalidTarget {
                value: "s3:///prefix".into()
            }
        );
        let config = lookup(&[
            (ENV_ARCHIVE_TARGET, "s3://bucket/p"),
            (ENV_ARCHIVE_S3_ENDPOINT, "http://127.0.0.1:9090"),
            (ENV_ARCHIVE_S3_REGION, "  "),
        ])
        .unwrap();
        assert_eq!(
            config.archive_s3_endpoint.as_deref(),
            Some("http://127.0.0.1:9090")
        );
        assert_eq!(config.archive_s3_region, None);
    }

    #[test]
    fn archive_target_parsing() {
        assert_eq!(
            ArchiveTarget::parse("s3://bucket/prefix"),
            Some(ArchiveTarget::S3("s3://bucket/prefix".to_string()))
        );
        assert_eq!(
            ArchiveTarget::parse("S3://bucket"),
            Some(ArchiveTarget::S3("S3://bucket".to_string()))
        );
        assert_eq!(
            ArchiveTarget::parse("file:///var/archive"),
            Some(ArchiveTarget::Filesystem(PathBuf::from("/var/archive")))
        );
        assert_eq!(
            ArchiveTarget::parse("./relative/dir"),
            Some(ArchiveTarget::Filesystem(PathBuf::from("./relative/dir")))
        );
        assert_eq!(ArchiveTarget::parse(""), None);
        assert_eq!(ArchiveTarget::parse("   "), None);
    }

    #[test]
    fn status_and_warning_follow_the_mode_and_the_target() {
        let default = RetentionConfig::default();
        assert_eq!(default.status(), RetentionStatus::NoTarget);
        let warning = default.warning().expect("no target warns");
        assert!(warning.contains(ENV_ARCHIVE_TARGET), "{warning}");

        let fs = RetentionConfig {
            archive_target: ArchiveTarget::parse("/var/archive"),
            ..RetentionConfig::default()
        };
        assert_eq!(fs.status(), RetentionStatus::Prunes);
        assert_eq!(fs.warning(), None);

        let s3 = RetentionConfig {
            archive_target: ArchiveTarget::parse("s3://bucket"),
            ..RetentionConfig::default()
        };
        assert_eq!(s3.status(), RetentionStatus::TargetNotImplemented);
        assert!(s3.warning().expect("s3 warns").contains("s3://"));

        // With a store for the target, the pass prunes (KAIROS-T-0671).
        struct Nothing;
        impl ArchiveStore for Nothing {
            fn put_file(&self, _: &str, _: &Path) -> Result<(), String> {
                Ok(())
            }
        }
        let s3_with_store = RetentionConfig {
            archive_store: ArchiveStoreHandle::new(Arc::new(Nothing)),
            ..s3.clone()
        };
        assert_eq!(s3_with_store.status(), RetentionStatus::Prunes);
        assert_eq!(s3_with_store.warning(), None);

        let discard = RetentionConfig {
            mode: RetentionMode::Discard,
            ..RetentionConfig::default()
        };
        assert_eq!(discard.status(), RetentionStatus::Prunes);
        assert_eq!(discard.warning(), None);

        let off = RetentionConfig {
            mode: RetentionMode::Off,
            ..RetentionConfig::default()
        };
        assert_eq!(off.status(), RetentionStatus::Off);
        assert_eq!(off.warning(), None);
        assert_eq!(RetentionStatus::NoTarget.as_str(), "no_target");
    }

    // ---- planner ---------------------------------------------------------

    #[test]
    fn empty_history_plans_nothing() {
        assert!(plan_history_compaction(&[], now(), &config(90, 5)).is_empty());
    }

    #[test]
    fn hot_window_rows_are_never_candidates() {
        let item = Uuid::new_v4();
        // hot cutoff = 2026-04-16T12:00:00Z (90 days before the fixed now)
        let rows = vec![
            row(item, 1, at(2026, 4, 16, 12, 0, 0)), // exactly AT the cutoff: hot (inclusive)
            row(item, 2, at(2026, 5, 1, 0, 0, 0)),
            row(item, 3, at(2026, 6, 1, 0, 0, 0)),
            row(item, 4, at(2026, 7, 10, 0, 0, 0)),
        ];
        assert!(plan_history_compaction(&rows, now(), &config(90, 0)).is_empty());
    }

    #[test]
    fn one_second_past_the_cutoff_is_a_candidate() {
        let item = Uuid::new_v4();
        // All three in April, one second older than the cutoff instant and
        // earlier; keep_latest 0 so only compaction applies.
        let rows = vec![
            row(item, 1, at(2026, 4, 1, 0, 0, 0)),
            row(item, 2, at(2026, 4, 10, 0, 0, 0)),
            row(item, 3, at(2026, 4, 16, 11, 59, 59)),
        ];
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item), vec![2]);
    }

    #[test]
    fn old_months_thin_to_first_and_last() {
        let item = Uuid::new_v4();
        let rows = vec![
            // January: 4 rows -> prune v2, v3
            row(item, 1, at(2026, 1, 5, 8, 0, 0)),
            row(item, 2, at(2026, 1, 10, 8, 0, 0)),
            row(item, 3, at(2026, 1, 20, 8, 0, 0)),
            row(item, 4, at(2026, 1, 28, 8, 0, 0)),
            // February: 3 rows -> prune v6
            row(item, 5, at(2026, 2, 3, 8, 0, 0)),
            row(item, 6, at(2026, 2, 14, 8, 0, 0)),
            row(item, 7, at(2026, 2, 25, 8, 0, 0)),
        ];
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item), vec![2, 3, 6]);
    }

    #[test]
    fn a_kept_diff_row_keeps_its_base_full_row() {
        let item = Uuid::new_v4();
        let diff = |version: i32, base: i32, edited_at| HistoryRowMeta {
            base_version: Some(base),
            ..row(item, version, edited_at)
        };
        let rows = vec![
            // January: v1 full, v2..v4 diffs from v1. The planner keeps v1
            // and v4 and prunes v2 and v3.
            row(item, 1, at(2026, 1, 5, 8, 0, 0)),
            diff(2, 1, at(2026, 1, 10, 8, 0, 0)),
            diff(3, 1, at(2026, 1, 20, 8, 0, 0)),
            diff(4, 1, at(2026, 1, 28, 8, 0, 0)),
            // February: v5 full, v6 and v7 diffs from v5; v8 full, v9 and
            // v10 diffs from v8. The planner keeps v5 and v10, and prunes
            // v6..v9 without the base rule. v10 needs v8, so v8 stays.
            row(item, 5, at(2026, 2, 2, 8, 0, 0)),
            diff(6, 5, at(2026, 2, 3, 8, 0, 0)),
            diff(7, 5, at(2026, 2, 14, 8, 0, 0)),
            row(item, 8, at(2026, 2, 20, 8, 0, 0)),
            diff(9, 8, at(2026, 2, 21, 8, 0, 0)),
            diff(10, 8, at(2026, 2, 25, 8, 0, 0)),
        ];
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item), vec![2, 3, 6, 7, 9]);
        // Each kept diff row has its base among the kept rows.
        let pruned = pruned_versions(&plan, item);
        for row in &rows {
            if let Some(base) = row.base_version
                && !pruned.contains(&row.version)
            {
                assert!(
                    !pruned.contains(&base),
                    "v{} lost its base v{base}",
                    row.version
                );
            }
        }
    }

    #[test]
    fn calendar_month_boundaries_are_respected() {
        let item = Uuid::new_v4();
        // Six consecutive old rows straddling Jan 31 / Feb 1: they are two
        // separate month groups, so the boundary rows (v3, v4) survive even
        // though they are "middles" of the combined range.
        let rows = vec![
            row(item, 1, at(2026, 1, 30, 10, 0, 0)),
            row(item, 2, at(2026, 1, 31, 10, 0, 0)),
            row(item, 3, at(2026, 1, 31, 23, 59, 59)),
            row(item, 4, at(2026, 2, 1, 0, 0, 0)),
            row(item, 5, at(2026, 2, 1, 10, 0, 0)),
            row(item, 6, at(2026, 2, 2, 10, 0, 0)),
        ];
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item), vec![2, 5]);
    }

    #[test]
    fn latest_n_guard_overrides_compaction_regardless_of_age() {
        let item = Uuid::new_v4();
        // 8 versions, ALL ancient, all in one month. Compaction alone would
        // keep v1 + v8 and prune v2..v7; the latest-5 guard (v4..v8)
        // protects v4..v7, leaving only v2, v3 to prune.
        let rows: Vec<_> = (1..=8)
            .map(|v| row(item, v, at(2025, 11, v as u32, 9, 0, 0)))
            .collect();
        let plan = plan_history_compaction(&rows, now(), &config(90, 5));
        assert_eq!(pruned_versions(&plan, item), vec![2, 3]);
    }

    #[test]
    fn items_with_at_most_keep_latest_versions_are_untouched() {
        let item = Uuid::new_v4();
        let rows: Vec<_> = (1..=5)
            .map(|v| row(item, v, at(2024, 3, v as u32, 0, 0, 0)))
            .collect();
        assert!(plan_history_compaction(&rows, now(), &config(90, 5)).is_empty());
    }

    #[test]
    fn single_version_items_are_never_pruned() {
        let item = Uuid::new_v4();
        let rows = vec![row(item, 1, at(2020, 1, 1, 0, 0, 0))];
        // Even with keep_latest 0 the single row is its month's first AND
        // last snapshot.
        assert!(plan_history_compaction(&rows, now(), &config(90, 0)).is_empty());
    }

    #[test]
    fn months_with_two_or_fewer_old_rows_are_untouched() {
        let item = Uuid::new_v4();
        let rows = vec![
            row(item, 1, at(2025, 6, 1, 0, 0, 0)),
            row(item, 2, at(2025, 6, 20, 0, 0, 0)),
            row(item, 3, at(2025, 7, 5, 0, 0, 0)),
        ];
        assert!(plan_history_compaction(&rows, now(), &config(90, 0)).is_empty());
    }

    #[test]
    fn items_are_planned_independently_and_output_is_sorted() {
        let item_a = Uuid::new_v4();
        let item_b = Uuid::new_v4();
        let mut rows = Vec::new();
        for v in 1..=3 {
            rows.push(row(item_a, v, at(2025, 2, v as u32, 0, 0, 0)));
            rows.push(row(item_b, v, at(2025, 2, v as u32, 0, 0, 0)));
        }
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item_a), vec![2]);
        assert_eq!(pruned_versions(&plan, item_b), vec![2]);
        let mut sorted = plan.clone();
        sorted.sort();
        assert_eq!(plan, sorted, "plan output is (item_id, version)-sorted");
    }

    #[test]
    fn same_calendar_month_of_different_years_are_distinct_groups() {
        let item = Uuid::new_v4();
        let rows = vec![
            row(item, 1, at(2024, 3, 1, 0, 0, 0)),
            row(item, 2, at(2024, 3, 15, 0, 0, 0)),
            row(item, 3, at(2024, 3, 30, 0, 0, 0)),
            row(item, 4, at(2025, 3, 1, 0, 0, 0)),
            row(item, 5, at(2025, 3, 15, 0, 0, 0)),
            row(item, 6, at(2025, 3, 30, 0, 0, 0)),
        ];
        let plan = plan_history_compaction(&rows, now(), &config(90, 0));
        assert_eq!(pruned_versions(&plan, item), vec![2, 5]);
    }

    // ---- code-index planner (KAIROS-T-0638) -------------------------------

    fn index(
        repo: Uuid,
        sha: &str,
        ref_name: Option<&str>,
        updated_at: DateTime<Utc>,
    ) -> CodeIndexMeta {
        CodeIndexMeta {
            repository_id: repo,
            commit_sha: sha.to_string(),
            ref_name: ref_name.map(str::to_string),
            updated_at,
        }
    }

    fn shas(plan: &[CodeIndexKey]) -> Vec<&str> {
        plan.iter().map(|k| k.commit_sha.as_str()).collect()
    }

    fn index_ref(
        repository_id: Uuid,
        name: &str,
        kind: &str,
        sha: &str,
        updated_at: DateTime<Utc>,
    ) -> CodeIndexRefMeta {
        CodeIndexRefMeta {
            repository_id,
            ref_name: name.to_string(),
            kind: kind.to_string(),
            commit_sha: sha.to_string(),
            updated_at,
        }
    }

    #[test]
    fn code_indexes_keep_the_newest_per_repository_and_ref() {
        let repo = Uuid::new_v4();
        let other = Uuid::new_v4();
        let now = at(2026, 7, 5, 0, 0, 0);
        let mut rows = Vec::new();
        // main: m1 (oldest) .. m4 (newest); a fresh PR ref with 1; no ref
        // with 3; the other repository's main with 3.
        for (i, sha) in ["m1", "m2", "m3", "m4"].iter().enumerate() {
            rows.push(index(
                repo,
                sha,
                Some("main"),
                at(2026, 7, 1 + i as u32, 0, 0, 0),
            ));
        }
        rows.push(index(
            repo,
            "p1",
            Some("refs/pull/7/head"),
            at(2026, 7, 4, 0, 0, 0),
        ));
        for (i, sha) in ["n1", "n2", "n3"].iter().enumerate() {
            rows.push(index(repo, sha, None, at(2026, 7, 1 + i as u32, 0, 0, 0)));
        }
        for (i, sha) in ["o1", "o2", "o3"].iter().enumerate() {
            rows.push(index(
                other,
                sha,
                Some("main"),
                at(2026, 7, 1 + i as u32, 0, 0, 0),
            ));
        }
        let plan = plan_code_index_retention(&rows, &[], 2, 7, now);
        let mut got = shas(&plan);
        got.sort();
        assert_eq!(got, vec!["m1", "m2", "n1", "o1"]);
        let mut sorted = plan.clone();
        sorted.sort();
        assert_eq!(plan, sorted, "the plan is sorted");
    }

    #[test]
    fn code_index_keep_zero_keeps_every_index_and_ties_break_by_sha() {
        let repo = Uuid::new_v4();
        let t = at(2026, 7, 1, 0, 0, 0);
        let rows = vec![
            index(repo, "a", Some("main"), t),
            index(repo, "b", Some("main"), t),
            index(repo, "c", Some("main"), t),
        ];
        assert!(plan_code_index_retention(&rows, &[], 0, 7, t).is_empty());
        assert_eq!(
            shas(&plan_code_index_retention(&rows, &[], 2, 7, t)),
            vec!["a"]
        );
        assert!(plan_code_index_retention(&rows, &[], 3, 7, t).is_empty());
    }

    // KAIROS-T-0751: a commit that a tag points at stays, whatever its
    // age and its group.
    #[test]
    fn code_indexes_with_a_tag_stay() {
        let repo = Uuid::new_v4();
        let now = at(2026, 7, 10, 0, 0, 0);
        let rows: Vec<CodeIndexMeta> = (1..=4)
            .map(|i| {
                index(
                    repo,
                    &format!("m{i}"),
                    Some("main"),
                    at(2026, 7, i, 0, 0, 0),
                )
            })
            .collect();
        let refs = vec![
            index_ref(repo, "main", "branch", "m4", now),
            index_ref(repo, "v1.0.0", "tag", "m1", at(2026, 1, 1, 0, 0, 0)),
        ];
        // keep 2: m3 and m4 stay as the newest; m1 stays for the tag.
        assert_eq!(
            shas(&plan_code_index_retention(&rows, &refs, 2, 7, now)),
            vec!["m2"]
        );
    }

    // KAIROS-T-0751: a pull request keeps its head while the ref is fresh;
    // an old head of the same pull request goes after the grace; a stale
    // pull request (its ref no longer a target) loses its index after the
    // grace; a pull commit that is also a tag stays.
    #[test]
    fn pull_request_indexes_keep_the_fresh_head_and_go_after_the_grace() {
        let repo = Uuid::new_v4();
        let now = at(2026, 7, 20, 0, 0, 0);
        let rows = vec![
            index(repo, "m1", Some("main"), at(2026, 7, 1, 0, 0, 0)),
            // pull/7: p1 then p2 (the head), both written 2 weeks ago.
            index(repo, "p1", Some("pull/7"), at(2026, 7, 5, 0, 0, 0)),
            index(repo, "p2", Some("pull/7"), at(2026, 7, 6, 0, 0, 0)),
            // pull/8: merged a month ago, its ref no longer a target.
            index(repo, "q1", Some("pull/8"), at(2026, 6, 1, 0, 0, 0)),
            // pull/9: written yesterday, no ref row yet.
            index(
                repo,
                "r1",
                Some("merge-requests/9"),
                at(2026, 7, 19, 0, 0, 0),
            ),
            // pull/10: stale, but its commit is a tag.
            index(repo, "t1", Some("pull/10"), at(2026, 6, 1, 0, 0, 0)),
        ];
        let refs = vec![
            index_ref(repo, "main", "branch", "m1", now),
            // The sweep saw pull/7 at p2 today: open and in the window.
            index_ref(repo, "pull/7", "pull", "p2", now),
            // pull/8 was last a target a month ago.
            index_ref(repo, "pull/8", "pull", "q1", at(2026, 6, 2, 0, 0, 0)),
            index_ref(repo, "pull/10", "pull", "t1", at(2026, 6, 2, 0, 0, 0)),
            index_ref(repo, "v2", "tag", "t1", at(2026, 6, 1, 0, 0, 0)),
        ];
        let plan = plan_code_index_retention(&rows, &refs, 10, 7, now);
        let mut got = shas(&plan);
        got.sort();
        assert_eq!(got, vec!["p1", "q1"]);
        // pr_days 0 keeps the pull request indexes.
        assert!(plan_code_index_retention(&rows, &refs, 10, 0, now).is_empty());
        // A fresh pull head with no ref row stays by the time of its write.
        let fresh = vec![index(repo, "r1", Some("pull/9"), at(2026, 7, 19, 0, 0, 0))];
        assert!(plan_code_index_retention(&fresh, &[], 10, 7, now).is_empty());
        let old = vec![index(repo, "r1", Some("pull/9"), at(2026, 7, 1, 0, 0, 0))];
        assert_eq!(
            shas(&plan_code_index_retention(&old, &[], 10, 7, now)),
            vec!["r1"]
        );
    }
}
