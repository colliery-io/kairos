//! The run report (KAIROS-T-0046 AC): op counts and error counts per
//! class, latency summary (p50/p95/p99/max), WS delivery counters,
//! metrics/bystander/history findings, and every breach — written as JSON
//! to a file and summarized on stdout.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::stats::{Breach, ClassSummary, Severity, error_rate};
use crate::world::HistoryBound;

/// Echo of the load-shape configuration the run used.
#[derive(Debug, Serialize)]
pub struct ConfigEcho {
    pub duration_secs: u64,
    pub rate_ops_per_sec: f64,
    pub human_workers: usize,
    pub agent_workers: usize,
    pub ws_subscribers: usize,
    pub assert_interval_secs: u64,
    pub max_error_rate: f64,
}

/// Aggregate outcome counters.
#[derive(Debug, Serialize)]
pub struct Totals {
    pub recorded_calls: u64,
    pub ok: u64,
    pub expected_conflicts: u64,
    pub errors: u64,
    /// Errors over all calls; deliberate 409s in the denominator only.
    pub error_rate: f64,
}

/// WS subscriber counters.
#[derive(Debug, Serialize)]
pub struct WsReport {
    pub subscribers: usize,
    pub connects: u64,
    pub events_received: u64,
    pub reconnects: u64,
}

/// `/metrics` scrape findings.
#[derive(Debug, Serialize)]
pub struct MetricsReport {
    /// `None` = never probed; `Some(false)` = endpoint absent (the
    /// KAIROS-A-0013 M2 gap, reported loudly); `Some(true)` = scraped.
    pub available: Option<bool>,
    pub scrapes: usize,
    /// Final values of the pool-related gauges, when available.
    pub final_pool_gauges: BTreeMap<String, f64>,
}

/// Bystander-tenant isolation findings.
#[derive(Debug, Serialize)]
pub struct BystanderReport {
    pub tenant: String,
    /// `unchanged` | `CHANGED` | `unavailable: <why>`.
    pub status: String,
    pub changed_sections: Vec<String>,
}

/// The full run report.
#[derive(Debug, Serialize)]
pub struct RunReport {
    pub tool: String,
    pub target: String,
    pub tenant: String,
    pub started_at: String,
    pub finished_at: String,
    pub configured: ConfigEcho,
    pub totals: Totals,
    pub ops: Vec<ClassSummary>,
    pub ws: WsReport,
    pub metrics: MetricsReport,
    pub history_bound: Vec<HistoryBound>,
    pub bystander: BystanderReport,
    pub breaches: Vec<Breach>,
    pub error_samples: Vec<String>,
    /// `PASS` | `FAIL` | `FATAL`.
    pub outcome: String,
}

/// Derive the run outcome from the breach list.
pub fn outcome(breaches: &[Breach]) -> &'static str {
    if breaches.iter().any(|b| b.severity == Severity::Fatal) {
        "FATAL"
    } else if breaches.is_empty() {
        "PASS"
    } else {
        "FAIL"
    }
}

/// The process exit code for an outcome (PASS=0, FAIL=1, FATAL=2).
pub fn exit_code(outcome: &str) -> u8 {
    match outcome {
        "PASS" => 0,
        "FATAL" => 2,
        _ => 1,
    }
}

/// Compute [`Totals`] from the class summaries.
pub fn totals(summaries: &[ClassSummary]) -> Totals {
    let mut t = Totals {
        recorded_calls: 0,
        ok: 0,
        expected_conflicts: 0,
        errors: 0,
        error_rate: error_rate(summaries),
    };
    for s in summaries {
        t.ok += s.ok;
        t.expected_conflicts += s.expected_conflicts;
        t.errors += s.errors;
    }
    t.recorded_calls = t.ok + t.expected_conflicts + t.errors;
    t
}

impl RunReport {
    /// The stdout summary (the JSON file carries the full detail).
    pub fn print_summary(&self) {
        println!();
        println!("=== kairos-soak run report ===");
        println!(
            "target {} tenant {:?} | {} -> {}",
            self.target, self.tenant, self.started_at, self.finished_at
        );
        println!(
            "load: {:.1} ops/s target, {} human + {} agent workers, {} ws subscribers",
            self.configured.rate_ops_per_sec,
            self.configured.human_workers,
            self.configured.agent_workers,
            self.configured.ws_subscribers,
        );
        println!(
            "calls: {} recorded | ok {} | deliberate 409s {} | errors {} | error rate {:.4}",
            self.totals.recorded_calls,
            self.totals.ok,
            self.totals.expected_conflicts,
            self.totals.errors,
            self.totals.error_rate,
        );
        println!(
            "{:<15} {:>8} {:>8} {:>8} {:>9} {:>9} {:>9} {:>9}",
            "op", "ok", "409s", "err", "p50ms", "p95ms", "p99ms", "maxms"
        );
        for s in &self.ops {
            println!(
                "{:<15} {:>8} {:>8} {:>8} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
                s.class,
                s.ok,
                s.expected_conflicts,
                s.errors,
                s.p50_ms,
                s.p95_ms,
                s.p99_ms,
                s.max_ms
            );
        }
        println!(
            "ws: {} events received, {} connects, {} reconnects",
            self.ws.events_received, self.ws.connects, self.ws.reconnects
        );
        match self.metrics.available {
            Some(true) => println!("/metrics: scraped {} times", self.metrics.scrapes),
            Some(false) => println!(
                "/metrics: UNAVAILABLE (KAIROS-A-0013 endpoint not implemented yet — M2); \
                 pool-stability check skipped"
            ),
            None => println!("/metrics: never probed"),
        }
        println!(
            "bystander tenant {:?}: {}",
            self.bystander.tenant, self.bystander.status
        );
        if self.history_bound.is_empty() {
            println!("history bound: no items sampled");
        } else {
            let max = self
                .history_bound
                .iter()
                .map(|h| h.history_rows)
                .max()
                .unwrap_or(0);
            println!(
                "history bound: {} items sampled, max {} rows/item, rows==version for all: {}",
                self.history_bound.len(),
                max,
                self.history_bound
                    .iter()
                    .all(|h| h.history_rows == i64::from(h.version)),
            );
        }
        if self.breaches.is_empty() {
            println!("breaches: none");
        } else {
            println!("breaches ({}):", self.breaches.len());
            for b in &self.breaches {
                println!("  [{:?}] {} {}: {}", b.severity, b.at, b.check, b.detail);
            }
        }
        println!("outcome: {}", self.outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breach(severity: Severity) -> Breach {
        Breach::now(severity, "error_rate", "detail".to_string())
    }

    #[test]
    fn outcome_and_exit_code_policy() {
        assert_eq!(outcome(&[]), "PASS");
        assert_eq!(outcome(&[breach(Severity::Breach)]), "FAIL");
        assert_eq!(
            outcome(&[breach(Severity::Breach), breach(Severity::Fatal)]),
            "FATAL"
        );
        assert_eq!(exit_code("PASS"), 0);
        assert_eq!(exit_code("FAIL"), 1);
        assert_eq!(exit_code("FATAL"), 2);
    }

    #[test]
    fn totals_math() {
        let summaries = vec![
            ClassSummary {
                class: "edit".to_string(),
                ok: 90,
                expected_conflicts: 8,
                errors: 2,
                p50_ms: 1.0,
                p95_ms: 2.0,
                p99_ms: 3.0,
                max_ms: 4.0,
            },
            ClassSummary {
                class: "read".to_string(),
                ok: 100,
                expected_conflicts: 0,
                errors: 0,
                p50_ms: 1.0,
                p95_ms: 2.0,
                p99_ms: 3.0,
                max_ms: 4.0,
            },
        ];
        let t = totals(&summaries);
        assert_eq!(t.recorded_calls, 200);
        assert_eq!(t.ok, 190);
        assert_eq!(t.expected_conflicts, 8);
        assert_eq!(t.errors, 2);
        assert!((t.error_rate - 0.01).abs() < 1e-9);
    }
}
