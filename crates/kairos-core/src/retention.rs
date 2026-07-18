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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, Utc};
use uuid::Uuid;

/// Env var: hot-window length in days (default 90). KAIROS-A-0004/A-0013.
pub const ENV_HISTORY_HOT_DAYS: &str = "KAIROS_HISTORY_HOT_DAYS";
/// Env var: latest-N guard size (default 5). KAIROS-A-0004/A-0013.
pub const ENV_HISTORY_KEEP_LATEST: &str = "KAIROS_HISTORY_KEEP_LATEST";
/// Env var: `activity_log` retention window in days (default 365).
pub const ENV_ACTIVITY_RETENTION_DAYS: &str = "KAIROS_ACTIVITY_RETENTION_DAYS";
/// Env var: archive target (filesystem path or `s3://…` URL); unset/empty
/// means no target is configured.
pub const ENV_ARCHIVE_TARGET: &str = "KAIROS_ARCHIVE_TARGET";
/// Env var: retention mode, one of `archive|discard|off` (default
/// `archive` — the safe default: without a target nothing is destroyed).
pub const ENV_RETENTION_MODE: &str = "KAIROS_RETENTION_MODE";

/// Errors from parsing the retention environment configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RetentionConfigError {
    /// A day/count variable did not parse as an unsigned integer.
    #[error("invalid integer for {var}: {value:?}")]
    InvalidInt { var: &'static str, value: String },
    /// `KAIROS_RETENTION_MODE` was not one of `archive|discard|off`.
    #[error("invalid {ENV_RETENTION_MODE}: {value:?} (expected archive|discard|off)")]
    InvalidMode { value: String },
}

/// `KAIROS_RETENTION_MODE` (KAIROS-A-0004): what the sweeper does with
/// prune candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionMode {
    /// Offload candidates as NDJSON to the archive target, then prune. With
    /// NO target configured, nothing is pruned — a warning is logged and
    /// counted instead (data is never destroyed without an offload copy).
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
}

impl Default for RetentionConfig {
    fn default() -> Self {
        RetentionConfig {
            history_hot_days: 90,
            history_keep_latest: 5,
            activity_retention_days: 365,
            archive_target: None,
            mode: RetentionMode::Archive,
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
        }
        if let Some(value) = lookup(ENV_RETENTION_MODE) {
            config.mode = value.parse()?;
        }
        Ok(config)
    }

    /// [`RetentionConfig::from_lookup`] over the process environment.
    pub fn from_env() -> Result<Self, RetentionConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

/// The planner's view of one `item_history` row: `(item_id, version,
/// edited_at)`. Loading and full-fidelity serialization stay in the I/O
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRowMeta {
    pub item_id: Uuid,
    pub version: i32,
    pub edited_at: DateTime<Utc>,
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

        for (_, mut month_rows) in by_month {
            month_rows.sort_by_key(|r| (r.edited_at, r.version));
            if month_rows.len() <= 2 {
                continue; // first == last or first+last: nothing between them
            }
            for row in &month_rows[1..month_rows.len() - 1] {
                if !guarded.contains(&row.version) {
                    prune.push(PruneKey {
                        item_id: row.item_id,
                        version: row.version,
                    });
                }
            }
        }
    }

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
}
