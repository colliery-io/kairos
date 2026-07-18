//! `/metrics` scraping (KAIROS-A-0013): plain Prometheus text-format
//! parsing — no observability stack — and the pool-stability check over a
//! series of samples.
//!
//! NOTE (recorded in KAIROS-T-0046): KAIROS-A-0013 decides the `/metrics`
//! endpoint but the server does not serve it yet (the Prometheus export
//! hangs off the M2 retention/metrics seam; today only `/healthz`
//! exists). The scraper therefore distinguishes *unavailable* from
//! *unhealthy*: a 404 is reported loudly and, unless
//! `thresholds.require_metrics` is set, does not fail the run.

use std::collections::BTreeMap;

use crate::stats::{Breach, Severity};

/// One scrape: gauge/counter samples keyed by `name{labels}`.
pub type MetricSample = BTreeMap<String, f64>;

/// Parse Prometheus text exposition format: `name{labels} value [ts]`
/// lines; `#` comments and blanks skipped; unparseable lines ignored
/// (scrape robustness beats strictness here).
pub fn parse_prometheus_text(body: &str) -> MetricSample {
    let mut out = MetricSample::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // The metric key ends at the first whitespace OUTSIDE a `{...}`
        // label block (label values may contain spaces).
        let mut in_labels = false;
        let mut split_at = None;
        for (i, c) in line.char_indices() {
            match c {
                '{' => in_labels = true,
                '}' => in_labels = false,
                c if c.is_whitespace() && !in_labels => {
                    split_at = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(split_at) = split_at else { continue };
        let (key, rest) = line.split_at(split_at);
        // Value is the first token after the key (a trailing timestamp may
        // follow).
        if let Some(value) = rest.split_whitespace().next()
            && let Ok(value) = value.parse::<f64>()
        {
            out.insert(key.to_string(), value);
        }
    }
    out
}

/// The metric-key filter for pool health: connection-pool gauges (bb8
/// checkout/idle/size style names) — anything mentioning connections or
/// pools.
pub fn is_pool_metric(key: &str) -> bool {
    let name = key.split('{').next().unwrap_or(key);
    name.contains("pool") || name.contains("connection")
}

/// Minimum consecutive rising steps before a gauge counts as "climbing".
pub const MIN_CLIMB_STEPS: usize = 5;

/// Relative growth (last/first) above which a monotonic climb is a breach.
pub const CLIMB_GROWTH_FACTOR: f64 = 1.5;

/// Detect unbounded pool growth: a pool gauge that rose on EVERY
/// consecutive sample over the trailing window (>= MIN_CLIMB_STEPS steps)
/// AND grew by more than [`CLIMB_GROWTH_FACTOR`] overall. Plateaus and
/// oscillation (normal checkout churn) never trip this.
pub fn check_pool_stability(samples: &[MetricSample]) -> Vec<Breach> {
    let mut breaches = Vec::new();
    if samples.len() < MIN_CLIMB_STEPS + 1 {
        return breaches;
    }
    let window = &samples[samples.len() - (MIN_CLIMB_STEPS + 1)..];
    let keys: Vec<&String> = window[0].keys().filter(|k| is_pool_metric(k)).collect();
    for key in keys {
        let series: Vec<f64> = window.iter().filter_map(|s| s.get(key)).copied().collect();
        if series.len() != window.len() {
            continue; // key missing from some sample; skip
        }
        let strictly_rising = series.windows(2).all(|w| w[1] > w[0]);
        let grew = series[0] <= 0.0 && *series.last().expect("nonempty") > 0.0
            || series[0] > 0.0
                && *series.last().expect("nonempty") / series[0] > CLIMB_GROWTH_FACTOR;
        if strictly_rising && grew {
            breaches.push(Breach::now(
                Severity::Breach,
                "pool_metrics",
                format!(
                    "{key} climbed monotonically across the last {} scrapes ({:.1} -> {:.1})",
                    series.len(),
                    series[0],
                    series.last().expect("nonempty")
                ),
            ));
        }
    }
    breaches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prometheus_text() {
        let body = r#"
# HELP db_pool_connections Current pool connections.
# TYPE db_pool_connections gauge
db_pool_connections{state="idle"} 8
db_pool_connections{state="used"} 2
http_requests_total{route="/api/tasks",status="200"} 1234 1699999999
malformed line without value
just_a_name_no_value
"#;
        let parsed = parse_prometheus_text(body);
        assert_eq!(parsed.len(), 3, "{parsed:?}");
        assert_eq!(parsed[r#"db_pool_connections{state="idle"}"#], 8.0);
        assert_eq!(
            parsed[r#"http_requests_total{route="/api/tasks",status="200"}"#],
            1234.0
        );
    }

    #[test]
    fn pool_metric_filter() {
        assert!(is_pool_metric(r#"db_pool_connections{state="idle"}"#));
        assert!(is_pool_metric("bb8_connections_in_use"));
        assert!(!is_pool_metric(r#"http_requests_total{route="/api"}"#));
    }

    fn sample(value: f64) -> MetricSample {
        let mut s = MetricSample::new();
        s.insert("db_pool_connections".to_string(), value);
        s
    }

    #[test]
    fn monotonic_climb_is_flagged_but_oscillation_is_not() {
        // Strictly rising and >1.5x growth over the window: breach.
        let climbing: Vec<MetricSample> = [4.0, 5.0, 6.0, 8.0, 10.0, 12.0].map(sample).to_vec();
        let breaches = check_pool_stability(&climbing);
        assert_eq!(breaches.len(), 1, "{breaches:?}");
        assert_eq!(breaches[0].check, "pool_metrics");

        // Oscillating (normal checkout churn): fine.
        let oscillating: Vec<MetricSample> = [4.0, 9.0, 3.0, 8.0, 2.0, 7.0].map(sample).to_vec();
        assert!(check_pool_stability(&oscillating).is_empty());

        // Rising but within the growth factor (warm-up plateau): fine.
        let warmup: Vec<MetricSample> = [10.0, 10.5, 11.0, 11.5, 12.0, 12.5].map(sample).to_vec();
        assert!(check_pool_stability(&warmup).is_empty());

        // Too few samples: no verdict.
        let short: Vec<MetricSample> = [1.0, 2.0, 3.0].map(sample).to_vec();
        assert!(check_pool_stability(&short).is_empty());
    }
}
