//! Client-side run statistics: per-op-class latency samples and outcome
//! counters, the percentile math, and the pure threshold checks the
//! continuous-assertion loop applies (KAIROS-T-0046).
//!
//! Latency is measured around the whole client call, so it INCLUDES
//! loopback network and (de)serialization on top of server time — that is
//! deliberate: it is the number a user-facing client experiences, and it
//! can only be pessimistic relative to the vision's 50ms p95 budget.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

/// The operation classes of the A-0012 tier-5 mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpClass {
    Create,
    Edit,
    ConflictEdit,
    Transition,
    Read,
    Search,
    Traverse,
    Mcp,
}

impl OpClass {
    /// Stable name used in config keys, reports, and breach messages.
    pub fn name(self) -> &'static str {
        match self {
            OpClass::Create => "create",
            OpClass::Edit => "edit",
            OpClass::ConflictEdit => "conflict_edit",
            OpClass::Transition => "transition",
            OpClass::Read => "read",
            OpClass::Search => "search",
            OpClass::Traverse => "traverse",
            OpClass::Mcp => "mcp",
        }
    }

    /// Every class, for iteration (test assertions).
    #[cfg(test)]
    pub const ALL: [OpClass; 8] = [
        OpClass::Create,
        OpClass::Edit,
        OpClass::ConflictEdit,
        OpClass::Transition,
        OpClass::Read,
        OpClass::Search,
        OpClass::Traverse,
        OpClass::Mcp,
    ];
}

/// How one operation ended.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Success (2xx / completed protocol exchange).
    Ok,
    /// A DELIBERATE optimistic-concurrency 409 on a designated collision
    /// item — an expected outcome, excluded from the error rate.
    ExpectedConflict,
    /// Anything else: typed API rejection, transport, decode, protocol.
    Error(String),
}

/// Aggregated numbers for one op class.
#[derive(Debug, Clone, Default)]
pub struct ClassStats {
    /// Latencies of non-error operations, microseconds, unsorted.
    latencies_us: Vec<u64>,
    pub ok: u64,
    pub expected_conflicts: u64,
    pub errors: u64,
}

/// A point-in-time percentile summary for one op class.
#[derive(Debug, Clone, Serialize)]
pub struct ClassSummary {
    pub class: String,
    pub ok: u64,
    pub expected_conflicts: u64,
    pub errors: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// Severity of an assertion breach. Policy (KAIROS-T-0046): `Fatal` stops
/// the run immediately (exit 2); `Breach` is announced loudly when
/// detected, the run continues, and the exit code is 1 at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Breach,
    Fatal,
}

/// One attributed assertion breach.
#[derive(Debug, Clone, Serialize)]
pub struct Breach {
    /// RFC 3339 detection time.
    pub at: String,
    pub severity: Severity,
    /// Which check tripped (`error_rate`, `latency_p95`, `pool_metrics`,
    /// `history_bound`, `tenant_isolation`, ...).
    pub check: String,
    /// Human-readable attribution.
    pub detail: String,
}

impl Breach {
    pub fn now(severity: Severity, check: &str, detail: String) -> Breach {
        Breach {
            at: chrono::Utc::now().to_rfc3339(),
            severity,
            check: check.to_string(),
            detail,
        }
    }
}

/// The shared recorder all workers write to.
#[derive(Debug, Default)]
pub struct Recorder {
    inner: Mutex<RecorderInner>,
}

#[derive(Debug, Default)]
struct RecorderInner {
    classes: BTreeMap<&'static str, ClassStats>,
    /// First N distinct-ish error samples, for the report.
    error_samples: Vec<String>,
}

/// Cap on retained error sample strings.
const MAX_ERROR_SAMPLES: usize = 50;

impl Recorder {
    /// Record one completed operation.
    pub fn record(&self, class: OpClass, elapsed: Duration, outcome: Outcome) {
        let mut inner = self.inner.lock().expect("recorder poisoned");
        let stats = inner.classes.entry(class.name()).or_default();
        match outcome {
            Outcome::Ok => {
                stats.ok += 1;
                stats.latencies_us.push(elapsed.as_micros() as u64);
            }
            Outcome::ExpectedConflict => {
                stats.expected_conflicts += 1;
                stats.latencies_us.push(elapsed.as_micros() as u64);
            }
            Outcome::Error(message) => {
                stats.errors += 1;
                if inner.error_samples.len() < MAX_ERROR_SAMPLES {
                    inner
                        .error_samples
                        .push(format!("[{}] {message}", class.name()));
                }
            }
        }
    }

    /// Snapshot every class summary (sorts copies; the hot path only
    /// pushes).
    pub fn summaries(&self) -> Vec<ClassSummary> {
        let inner = self.inner.lock().expect("recorder poisoned");
        inner
            .classes
            .iter()
            .map(|(name, stats)| {
                let mut sorted = stats.latencies_us.clone();
                sorted.sort_unstable();
                ClassSummary {
                    class: (*name).to_string(),
                    ok: stats.ok,
                    expected_conflicts: stats.expected_conflicts,
                    errors: stats.errors,
                    p50_ms: percentile_us(&sorted, 50.0) as f64 / 1000.0,
                    p95_ms: percentile_us(&sorted, 95.0) as f64 / 1000.0,
                    p99_ms: percentile_us(&sorted, 99.0) as f64 / 1000.0,
                    max_ms: sorted.last().copied().unwrap_or(0) as f64 / 1000.0,
                }
            })
            .collect()
    }

    /// The retained error samples.
    pub fn error_samples(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("recorder poisoned")
            .error_samples
            .clone()
    }
}

/// Nearest-rank percentile over an ASCENDING-sorted slice (0 for empty).
pub fn percentile_us(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The run-wide error rate: errors over ALL completed ops, with the
/// deliberate 409s counted in the denominator (they are real, successful
/// round trips) but never the numerator.
pub fn error_rate(summaries: &[ClassSummary]) -> f64 {
    let (mut errors, mut total) = (0u64, 0u64);
    for s in summaries {
        errors += s.errors;
        total += s.ok + s.expected_conflicts + s.errors;
    }
    if total == 0 {
        0.0
    } else {
        errors as f64 / total as f64
    }
}

/// Apply the error-rate and per-class p95 checks to a snapshot; returns
/// breaches (all `Severity::Breach`).
pub fn check_snapshot(
    summaries: &[ClassSummary],
    thresholds: &crate::config::Thresholds,
) -> Vec<Breach> {
    let mut breaches = Vec::new();
    let rate = error_rate(summaries);
    if rate > thresholds.max_error_rate {
        let sample_counts: Vec<String> = summaries
            .iter()
            .filter(|s| s.errors > 0)
            .map(|s| format!("{}={}", s.class, s.errors))
            .collect();
        breaches.push(Breach::now(
            Severity::Breach,
            "error_rate",
            format!(
                "error rate {:.4} exceeds max {:.4} (errors by class: {}; deliberate 409s excluded)",
                rate,
                thresholds.max_error_rate,
                sample_counts.join(", ")
            ),
        ));
    }
    for s in summaries {
        let samples = s.ok + s.expected_conflicts;
        if (samples as usize) < thresholds.min_samples_for_latency {
            continue;
        }
        let budget = thresholds.p95_budget(&s.class);
        if s.p95_ms > budget {
            breaches.push(Breach::now(
                Severity::Breach,
                "latency_p95",
                format!(
                    "{}: p95 {:.1}ms exceeds budget {:.1}ms over {} samples \
                     (client-side: includes network + serialization)",
                    s.class, s.p95_ms, budget, samples
                ),
            ));
        }
    }
    breaches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Thresholds;

    fn summary(class: &str, ok: u64, conflicts: u64, errors: u64, p95: f64) -> ClassSummary {
        ClassSummary {
            class: class.to_string(),
            ok,
            expected_conflicts: conflicts,
            errors,
            p50_ms: p95 / 2.0,
            p95_ms: p95,
            p99_ms: p95 * 1.2,
            max_ms: p95 * 2.0,
        }
    }

    #[test]
    fn percentile_nearest_rank() {
        assert_eq!(percentile_us(&[], 95.0), 0);
        assert_eq!(percentile_us(&[7], 50.0), 7);
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile_us(&v, 50.0), 50);
        assert_eq!(percentile_us(&v, 95.0), 95);
        assert_eq!(percentile_us(&v, 99.0), 99);
        assert_eq!(percentile_us(&v, 100.0), 100);
    }

    #[test]
    fn error_rate_excludes_deliberate_conflicts_from_numerator() {
        // 90 ok + 8 deliberate 409s + 2 errors -> 2/100, NOT 10/100.
        let s = vec![summary("edit", 90, 8, 2, 10.0)];
        assert!((error_rate(&s) - 0.02).abs() < 1e-9);
        assert_eq!(error_rate(&[]), 0.0);
    }

    #[test]
    fn snapshot_checks_flag_rate_and_p95() {
        let thresholds = Thresholds::default(); // 1% error rate, 50ms p95
        // Healthy: nothing trips.
        let healthy = vec![summary("read", 1000, 0, 5, 12.0)];
        assert!(check_snapshot(&healthy, &thresholds).is_empty());

        // Error rate breach.
        let erroring = vec![summary("create", 80, 0, 20, 12.0)];
        let breaches = check_snapshot(&erroring, &thresholds);
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].check, "error_rate");
        assert_eq!(breaches[0].severity, Severity::Breach);

        // p95 breach (budget 50ms; mcp gets its own 250ms default).
        let slow = vec![
            summary("transition", 100, 0, 0, 61.0),
            summary("mcp", 100, 0, 0, 200.0),
        ];
        let breaches = check_snapshot(&slow, &thresholds);
        assert_eq!(breaches.len(), 1, "{breaches:?}");
        assert_eq!(breaches[0].check, "latency_p95");
        assert!(breaches[0].detail.starts_with("transition:"));

        // Under the sample floor, p95 is not asserted.
        let tiny = vec![summary("search", 5, 0, 0, 400.0)];
        assert!(check_snapshot(&tiny, &thresholds).is_empty());
    }

    #[test]
    fn recorder_records_and_summarizes() {
        let recorder = Recorder::default();
        for ms in [5u64, 10, 15, 20] {
            recorder.record(OpClass::Read, Duration::from_millis(ms), Outcome::Ok);
        }
        recorder.record(
            OpClass::ConflictEdit,
            Duration::from_millis(8),
            Outcome::ExpectedConflict,
        );
        recorder.record(
            OpClass::Create,
            Duration::from_millis(30),
            Outcome::Error("409 CONFLICT: boom".to_string()),
        );
        let summaries = recorder.summaries();
        let read = summaries.iter().find(|s| s.class == "read").unwrap();
        assert_eq!(read.ok, 4);
        assert_eq!(read.errors, 0);
        assert!(read.p95_ms >= 15.0 && read.max_ms >= 19.0);
        let conflict = summaries
            .iter()
            .find(|s| s.class == "conflict_edit")
            .unwrap();
        assert_eq!(conflict.expected_conflicts, 1);
        let create = summaries.iter().find(|s| s.class == "create").unwrap();
        assert_eq!(create.errors, 1);
        assert_eq!(recorder.error_samples().len(), 1);
    }
}
