//! Prometheus `/metrics` and the `/readyz` probe (KAIROS-A-0013, KAIROS-T-0049).
//!
//! # Why hand-rolled instead of `metrics` + `metrics-exporter-prometheus`
//!
//! The obvious library route installs a **process-global** recorder. This
//! crate's integration suite builds many production routers per process and
//! drives them with `Router::oneshot` (see `tests/common`), so a global
//! recorder would let one test's counters bleed into another's — the
//! "counters advance across requests" assertion (KAIROS-T-0049 AC) would be
//! non-deterministic. Instead every [`crate::app::AppState`] owns an
//! `Arc<Metrics>` registry, so metrics are isolated per router and tests are
//! deterministic. The exposition format is plain Prometheus text, and it
//! costs zero new workspace dependencies.
//!
//! # Exposed families
//!
//! - `http_request_duration_seconds` — a histogram (`_bucket`/`_sum`/`_count`)
//!   labeled by `method`, `route` (the axum [`MatchedPath`] pattern, not the
//!   raw URI, so cardinality stays bounded), and `status`.
//! - `kairos_db_pool_connections` — point-in-time gauges for the async bb8
//!   pool and the blocking r2d2 bridge (`pool="async"|"blocking"`,
//!   `state="total"|"idle"`), read live at scrape time.
//! - `http_requests_by_tenant_total` — per-tenant request counter
//!   (`tenant="<slug>"`), attributed from the resolved [`TenantContext`] the
//!   tenant middleware stamps onto the response.
//! - `kairos_auth_lockouts_total` — authentication lockouts by subject kind
//!   (`subject="identity"|"source"`, KAIROS-T-0202).
//! - `kairos_jobs` — the jobs of the queue by `kind` and `state`
//!   (`queued`, `leased`, `done`, `failed`), of all the organizations
//!   (KAIROS-T-0576). One grouped query reads them, at most once each
//!   [`JOB_GAUGE_REFRESH`]: a frequent scrape does not query each time.
//! - `kairos_job_failures_total` — the jobs that this server wrote as
//!   `failed`, by `kind` (KAIROS-T-0576).
//! - `kairos_job_lease_kills_total` — the jobs that the watchdog of this
//!   server killed because they made no progress for the lease time, by
//!   `kind` (KAIROS-T-0576).
//!
//! `/metrics` and `/readyz` sit OUTSIDE the auth stack (mounted beside
//! `/healthz` in [`crate::app::router`]): Prometheus scrapers and
//! orchestrator probes are unauthenticated by convention, and both endpoints
//! are non-`/api` so they are outside the S-0005 surface entirely.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use diesel_async::SimpleAsyncConnection;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

/// Histogram bucket upper bounds (`le`), in seconds — the Prometheus client
/// default latency buckets. Cumulative counts are emitted at render time.
const BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// One HTTP histogram: per-bucket cumulative counts (each entry counts
/// observations `<= BUCKETS[i]`), the running sum, and the total count
/// (which is also the `+Inf` bucket).
#[derive(Debug, Default, Clone)]
struct Histogram {
    /// `buckets[i]` = number of observations `<= BUCKETS[i]`.
    buckets: [u64; BUCKETS.len()],
    /// Sum of all observed values (seconds).
    sum: f64,
    /// Total number of observations.
    count: u64,
}

impl Histogram {
    fn observe(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        for (i, bound) in BUCKETS.iter().enumerate() {
            if value <= *bound {
                self.buckets[i] += 1;
            }
        }
    }
}

/// The label triple for an HTTP histogram series.
type HttpKey = (String, String, String); // (method, route, status)

/// The time for which `/metrics` keeps the `kairos_jobs` gauges before it
/// reads the queue again (KAIROS-T-0576).
pub const JOB_GAUGE_REFRESH: Duration = Duration::from_secs(15);

/// The `kairos_jobs` gauges and the time of their last read.
#[derive(Debug, Default)]
struct JobGauges {
    read_at: Option<Instant>,
    /// (kind, state) → the number of jobs. A kind that was seen once keeps
    /// its series, at 0 when it has no job in a state.
    counts: BTreeMap<(String, String), i64>,
}

/// A per-router metrics registry (held as `Arc<Metrics>` in
/// [`crate::app::AppState`]). Cheap to share; interior-mutable behind
/// `Mutex`es because recording happens on the request path.
#[derive(Debug)]
pub struct Metrics {
    http: Mutex<BTreeMap<HttpKey, Histogram>>,
    tenants: Mutex<BTreeMap<String, u64>>,
    /// Authentication lockouts, keyed by subject kind (KAIROS-T-0202).
    lockouts: Mutex<BTreeMap<String, u64>>,
    /// The jobs that this server wrote as failed, by kind (KAIROS-T-0576).
    job_failures: Mutex<BTreeMap<String, u64>>,
    /// The jobs that the watchdog of this server killed, by kind
    /// (KAIROS-T-0576).
    job_lease_kills: Mutex<BTreeMap<String, u64>>,
    job_gauges: Mutex<JobGauges>,
    /// The time for which the job gauges stay ([`JOB_GAUGE_REFRESH`]).
    job_refresh: Duration,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::with_job_refresh(JOB_GAUGE_REFRESH)
    }
}

impl Metrics {
    /// Fresh, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh registry whose job gauges stay for `refresh`. A test sets
    /// [`Duration::ZERO`], so that each scrape reads the queue.
    pub fn with_job_refresh(refresh: Duration) -> Self {
        Metrics {
            http: Mutex::default(),
            tenants: Mutex::default(),
            lockouts: Mutex::default(),
            job_failures: Mutex::default(),
            job_lease_kills: Mutex::default(),
            job_gauges: Mutex::default(),
            job_refresh: refresh,
        }
    }

    /// Count a job of `kind` that this server wrote as `failed`
    /// (KAIROS-T-0576).
    pub fn record_job_failure(&self, kind: &str) {
        *self
            .job_failures
            .lock()
            .expect("metrics.job_failures mutex")
            .entry(kind.to_string())
            .or_insert(0) += 1;
    }

    /// Count a job of `kind` that the watchdog of this server killed
    /// because it made no progress (KAIROS-T-0576).
    pub fn record_job_lease_kill(&self, kind: &str) {
        *self
            .job_lease_kills
            .lock()
            .expect("metrics.job_lease_kills mutex")
            .entry(kind.to_string())
            .or_insert(0) += 1;
    }

    /// The value of `kairos_job_failures_total{kind}`.
    pub fn job_failures(&self, kind: &str) -> u64 {
        self.job_failures
            .lock()
            .expect("metrics.job_failures mutex")
            .get(kind)
            .copied()
            .unwrap_or(0)
    }

    /// The value of `kairos_job_lease_kills_total{kind}`.
    pub fn job_lease_kills(&self, kind: &str) -> u64 {
        self.job_lease_kills
            .lock()
            .expect("metrics.job_lease_kills mutex")
            .get(kind)
            .copied()
            .unwrap_or(0)
    }

    /// True when the job gauges are older than their refresh time. It
    /// marks them as read now, so that the scrapes that come at the same
    /// time do not each query the queue.
    fn job_gauges_due(&self) -> bool {
        let mut gauges = self.job_gauges.lock().expect("metrics.job_gauges mutex");
        if gauges
            .read_at
            .is_some_and(|at| at.elapsed() < self.job_refresh)
        {
            return false;
        }
        gauges.read_at = Some(Instant::now());
        true
    }

    /// Set the job gauges from one read of the queue. Each kind that was
    /// seen has a series for each state, at 0 when no job of the kind is in
    /// the state.
    fn set_job_counts(&self, rows: &[kairos_db::jobs::KindStateCount]) {
        let mut gauges = self.job_gauges.lock().expect("metrics.job_gauges mutex");
        let mut kinds: Vec<String> = gauges.counts.keys().map(|(kind, _)| kind.clone()).collect();
        kinds.extend(rows.iter().map(|row| row.kind.clone()));
        let mut counts = BTreeMap::new();
        for kind in kinds {
            for state in kairos_db::jobs::STATES {
                counts.insert((kind.clone(), state.to_string()), 0);
            }
        }
        for row in rows {
            counts.insert((row.kind.clone(), row.state.clone()), row.count);
        }
        gauges.counts = counts;
    }

    /// Render the job gauges and counters (KAIROS-T-0576).
    fn render_jobs(&self, out: &mut String) {
        out.push_str(
            "# HELP kairos_jobs The jobs of the queue, by kind and state.\n\
             # TYPE kairos_jobs gauge\n",
        );
        let gauges = self.job_gauges.lock().expect("metrics.job_gauges mutex");
        for ((kind, state), count) in gauges.counts.iter() {
            writeln!(
                out,
                "kairos_jobs{{kind=\"{kind}\",state=\"{state}\"}} {count}"
            )
            .expect("write metrics");
        }
        drop(gauges);
        for (name, help, counter) in [
            (
                "kairos_job_failures_total",
                "The jobs that this server wrote as failed, by kind.",
                &self.job_failures,
            ),
            (
                "kairos_job_lease_kills_total",
                "The jobs that the watchdog of this server killed for no progress, by kind.",
                &self.job_lease_kills,
            ),
        ] {
            writeln!(out, "# HELP {name} {help}\n# TYPE {name} counter").expect("write metrics");
            let counter = counter.lock().expect("metrics job counter mutex");
            for (kind, count) in counter.iter() {
                writeln!(out, "{name}{{kind=\"{kind}\"}} {count}").expect("write metrics");
            }
        }
    }

    /// Record one completed HTTP request into the duration histogram.
    fn record_http(&self, method: &str, route: &str, status: u16, secs: f64) {
        let key = (method.to_string(), route.to_string(), status.to_string());
        self.http
            .lock()
            .expect("metrics.http mutex")
            .entry(key)
            .or_default()
            .observe(secs);
    }

    /// Increment the lockout counter for a subject kind (`"identity"` or
    /// `"source"`). Called by the auth throttle (KAIROS-T-0202); a series that
    /// climbs is either an attack or a broken client, and an operator wants to
    /// see which before users start reporting it.
    pub fn record_lockout(&self, kind: &str) {
        *self
            .lockouts
            .lock()
            .expect("metrics.lockouts mutex")
            .entry(kind.to_string())
            .or_insert(0) += 1;
    }

    /// Increment the per-tenant request counter.
    fn record_tenant(&self, tenant: &str) {
        *self
            .tenants
            .lock()
            .expect("metrics.tenants mutex")
            .entry(tenant.to_string())
            .or_insert(0) += 1;
    }

    /// Render the accumulated HTTP histograms and per-tenant counters into
    /// `out` in Prometheus text-exposition format. Pool gauges are appended
    /// separately by [`metrics_handler`] because they are read live.
    fn render_http_and_tenants(&self, out: &mut String) {
        out.push_str(
            "# HELP http_request_duration_seconds HTTP request latency by route, method, and status.\n\
             # TYPE http_request_duration_seconds histogram\n",
        );
        let http = self.http.lock().expect("metrics.http mutex");
        for ((method, route, status), hist) in http.iter() {
            for (i, bound) in BUCKETS.iter().enumerate() {
                writeln!(
                    out,
                    "http_request_duration_seconds_bucket{{method=\"{method}\",route=\"{route}\",status=\"{status}\",le=\"{bound}\"}} {}",
                    hist.buckets[i]
                )
                .expect("write metrics");
            }
            writeln!(
                out,
                "http_request_duration_seconds_bucket{{method=\"{method}\",route=\"{route}\",status=\"{status}\",le=\"+Inf\"}} {}",
                hist.count
            )
            .expect("write metrics");
            writeln!(
                out,
                "http_request_duration_seconds_sum{{method=\"{method}\",route=\"{route}\",status=\"{status}\"}} {}",
                hist.sum
            )
            .expect("write metrics");
            writeln!(
                out,
                "http_request_duration_seconds_count{{method=\"{method}\",route=\"{route}\",status=\"{status}\"}} {}",
                hist.count
            )
            .expect("write metrics");
        }
        drop(http);

        out.push_str(
            "# HELP http_requests_by_tenant_total Total requests attributed to each resolved tenant.\n\
             # TYPE http_requests_by_tenant_total counter\n",
        );
        let tenants = self.tenants.lock().expect("metrics.tenants mutex");
        for (tenant, count) in tenants.iter() {
            writeln!(
                out,
                "http_requests_by_tenant_total{{tenant=\"{tenant}\"}} {count}"
            )
            .expect("write metrics");
        }
        drop(tenants);

        out.push_str(
            "# HELP kairos_auth_lockouts_total Authentication lockouts, by subject kind.\n\
             # TYPE kairos_auth_lockouts_total counter\n",
        );
        let lockouts = self.lockouts.lock().expect("metrics.lockouts mutex");
        for (kind, count) in lockouts.iter() {
            writeln!(
                out,
                "kairos_auth_lockouts_total{{subject=\"{kind}\"}} {count}"
            )
            .expect("write metrics");
        }
    }
}

/// The outer HTTP-metrics layer (KAIROS-A-0013): times every request, then
/// records the duration histogram (labeled by the matched route, method, and
/// status) and — when the tenant stack resolved one — the per-tenant
/// counter. Added via `Router::layer` so it wraps the whole router; the
/// tenant middleware stamps its [`TenantContext`] onto the response so this
/// outer layer can attribute the tenant (a request extension set by an inner
/// layer is not visible to an outer one).
pub async fn track_metrics(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_string();
    // The matched route pattern keeps label cardinality bounded (`/api/{id}`
    // rather than every concrete id). Unmatched requests (SPA fallback) fall
    // back to a single `unmatched` label rather than exploding on raw paths.
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());

    // KAIROS-T-0196: one span per request, which is what makes the OTLP export
    // worth having.
    //
    // Wiring the exporter was necessary and not sufficient: before this, Kairos
    // created NO spans anywhere — no TraceLayer, no #[instrument], nothing — so a
    // configured collector received an empty stream and the feature looked broken
    // rather than absent. Found by pointing the exporter at a fake collector and
    // getting zero posts.
    //
    // It lives here rather than in a tower-http TraceLayer because this
    // middleware already wraps the whole router and already computes the route
    // pattern, the status and the tenant. Adding a dependency to re-derive them
    // would be the worse trade.
    //
    // `route` is the MATCHED PATTERN, never the concrete path — the same reason
    // the metrics labels use it. A span name carrying real ids would make every
    // request its own operation in a collector's UI, which is how you turn a
    // trace view into a list.
    let span = tracing::info_span!(
        "http.request",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        http.request.method = %method,
        http.route = %route,
        // Filled after the response, hence Empty: a span's fields are fixed at
        // creation, so anything known only afterwards has to be reserved now.
        http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
        kairos.tenant = tracing::field::Empty,
    );

    let start = Instant::now();
    let response = {
        use tracing::Instrument as _;
        next.run(req).instrument(span.clone()).await
    };
    let elapsed = start.elapsed().as_secs_f64();

    let status = response.status();
    span.record("http.response.status_code", status.as_u16());
    // OpenTelemetry's span status is a three-state thing (unset/ok/error), and
    // only 5xx is OUR error: a 404 or a 403 is the server working correctly, and
    // marking those as errors would make every permission check look like an
    // incident.
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
    }

    state
        .metrics
        .record_http(&method, &route, status.as_u16(), elapsed);
    if let Some(tenant) = response.extensions().get::<TenantContext>() {
        span.record("kairos.tenant", tenant.slug.as_str());
        state.metrics.record_tenant(&tenant.slug);
    }
    response
}

/// `GET /metrics` (KAIROS-A-0013): the Prometheus scrape endpoint. Renders
/// the accumulated HTTP histograms + per-tenant counters and appends live
/// connection-pool gauges for both the async bb8 pool and the blocking r2d2
/// bridge. Unauthenticated by convention (mounted outside the auth stack).
pub async fn metrics_handler(State(state): State<AppState>) -> Response {
    let mut body = String::with_capacity(4096);
    state.metrics.render_http_and_tenants(&mut body);

    // KAIROS-T-0576: the job gauges, read again when they are older than
    // their refresh time. A failed read keeps the last values.
    if state.metrics.job_gauges_due() {
        match state
            .blocking
            .run_public(|conn| kairos_db::jobs::counts(conn, None).map_err(ApiError::internal))
            .await
        {
            Ok(rows) => state.metrics.set_job_counts(&rows),
            Err(e) => tracing::warn!(error = ?e, "metrics: the job gauges were not read"),
        }
    }
    state.metrics.render_jobs(&mut body);

    // Live pool gauges (point-in-time; A-0013 pool-stability signal).
    body.push_str(
        "# HELP kairos_db_pool_connections Current database connection-pool occupancy.\n\
         # TYPE kairos_db_pool_connections gauge\n",
    );
    let async_state = state.pool.raw_pool().state();
    let _ = writeln!(
        body,
        "kairos_db_pool_connections{{pool=\"async\",state=\"total\"}} {}",
        async_state.connections
    );
    let _ = writeln!(
        body,
        "kairos_db_pool_connections{{pool=\"async\",state=\"idle\"}} {}",
        async_state.idle_connections
    );
    let (blocking_total, blocking_idle) = state.blocking.pool_state();
    let _ = writeln!(
        body,
        "kairos_db_pool_connections{{pool=\"blocking\",state=\"total\"}} {blocking_total}"
    );
    let _ = writeln!(
        body,
        "kairos_db_pool_connections{{pool=\"blocking\",state=\"idle\"}} {blocking_idle}"
    );

    ([("content-type", "text/plain; version=0.0.4")], body).into_response()
}

/// `GET /readyz` (KAIROS-A-0013): readiness = database connectivity AND no
/// pending public-schema migrations. Returns `200 ready` only when the async
/// pool answers `SELECT 1` and the embedded migration tree is fully applied;
/// otherwise `503` with a reason. Distinct from `/healthz` (pure liveness).
///
/// KAIROS-T-0631: when the retention settings make the pass remove nothing
/// (`archive` with no target) or fail (an `s3://` target), the body adds a
/// second line `warning: …`. The status stays 200: a setting that the
/// server cannot parse stops the start, and these settings parse.
pub async fn readyz(State(state): State<AppState>) -> Response {
    // 1. Async-pool connectivity — the runtime data path.
    let connectivity = async {
        let mut conn = state.pool.public_conn().await?;
        conn.batch_execute("SELECT 1").await?;
        Ok::<(), kairos_db::PoolError>(())
    }
    .await;
    if let Err(e) = connectivity {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("not ready: database unreachable: {e}"),
        )
            .into_response();
    }

    // 2. Pending-migration check — a binary whose embedded migrations are
    //    ahead of what is applied is not ready to serve (A-0013). Uses the
    //    sync bridge because `diesel_migrations` is sync-only.
    let pending = state
        .blocking
        .run_public(|conn| {
            kairos_db::has_pending_public_migrations(conn).map_err(ApiError::internal)
        })
        .await;
    match pending {
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("not ready: migration check failed: {e:?}"),
        )
            .into_response(),
        Ok(true) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "not ready: pending database migrations".to_string(),
        )
            .into_response(),
        Ok(false) => (StatusCode::OK, ready_body(&state.retention)).into_response(),
    }
}

/// The body of a ready `/readyz`: `ready`, and a warning line when the
/// retention settings remove nothing or fail (KAIROS-T-0631).
fn ready_body(retention: &kairos_core::retention::RetentionConfig) -> String {
    match retention.warning() {
        Some(warning) => format!("ready\nwarning: {warning}"),
        None => "ready".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ready_body_names_a_retention_that_removes_nothing() {
        use kairos_core::retention::{ArchiveTarget, RetentionConfig};
        let body = ready_body(&RetentionConfig::default());
        let mut lines = body.lines();
        assert_eq!(lines.next(), Some("ready"));
        let warning = lines.next().expect("a warning line");
        assert!(warning.starts_with("warning: "), "{warning}");
        assert!(warning.contains("KAIROS_ARCHIVE_TARGET"), "{warning}");
        let with_target = RetentionConfig {
            archive_target: ArchiveTarget::parse("/var/archive"),
            ..RetentionConfig::default()
        };
        assert_eq!(ready_body(&with_target), "ready");
    }

    fn count(kind: &str, state: &str, count: i64) -> kairos_db::jobs::KindStateCount {
        kairos_db::jobs::KindStateCount {
            kind: kind.to_string(),
            state: state.to_string(),
            count,
        }
    }

    /// KAIROS-T-0576: each kind has a series for each state, and a kind
    /// with no job left keeps its series at 0.
    #[test]
    fn the_job_gauges_keep_each_kind_and_state() {
        let metrics = Metrics::new();
        metrics.set_job_counts(&[count("periodic", "queued", 2)]);
        let mut out = String::new();
        metrics.render_jobs(&mut out);
        assert!(
            out.contains("kairos_jobs{kind=\"periodic\",state=\"queued\"} 2\n"),
            "{out}"
        );
        assert!(
            out.contains("kairos_jobs{kind=\"periodic\",state=\"failed\"} 0\n"),
            "{out}"
        );
        metrics.set_job_counts(&[]);
        let mut out = String::new();
        metrics.render_jobs(&mut out);
        assert!(
            out.contains("kairos_jobs{kind=\"periodic\",state=\"queued\"} 0\n"),
            "{out}"
        );
    }

    /// KAIROS-T-0576: the counters render by kind.
    #[test]
    fn the_job_counters_render_by_kind() {
        let metrics = Metrics::new();
        metrics.record_job_failure("hosted_build");
        metrics.record_job_failure("hosted_build");
        metrics.record_job_lease_kill("embedded_build");
        let mut out = String::new();
        metrics.render_jobs(&mut out);
        assert!(
            out.contains("# TYPE kairos_job_failures_total counter\n"),
            "{out}"
        );
        assert!(
            out.contains("kairos_job_failures_total{kind=\"hosted_build\"} 2\n"),
            "{out}"
        );
        assert!(
            out.contains("kairos_job_lease_kills_total{kind=\"embedded_build\"} 1\n"),
            "{out}"
        );
        assert_eq!(metrics.job_failures("embedded_build"), 0);
    }

    /// KAIROS-T-0576: inside the refresh time, a scrape does not read the
    /// queue again.
    #[test]
    fn the_job_gauges_are_due_once_in_the_refresh_time() {
        let metrics = Metrics::new();
        assert!(metrics.job_gauges_due());
        assert!(!metrics.job_gauges_due());
        let each = Metrics::with_job_refresh(Duration::ZERO);
        assert!(each.job_gauges_due());
        assert!(each.job_gauges_due());
    }

    #[test]
    fn histogram_buckets_are_cumulative() {
        let mut h = Histogram::default();
        h.observe(0.003); // <= 0.005
        h.observe(0.2); // <= 0.25 (and every larger bound)
        h.observe(3.0); // <= 5.0
        assert_eq!(h.count, 3);
        assert!((h.sum - 3.203).abs() < 1e-9);
        // le=0.005 caught only the first observation.
        assert_eq!(h.buckets[0], 1);
        // le=0.25 caught the first two.
        let idx_025 = BUCKETS.iter().position(|b| *b == 0.25).unwrap();
        assert_eq!(h.buckets[idx_025], 2);
        // le=10 caught all three.
        assert_eq!(*h.buckets.last().unwrap(), 3);
    }

    #[test]
    fn render_emits_expected_families() {
        let m = Metrics::new();
        m.record_http("GET", "/api/tasks", 200, 0.01);
        m.record_http("GET", "/api/tasks", 200, 0.4);
        m.record_tenant("acme");
        m.record_tenant("acme");
        let mut out = String::new();
        m.render_http_and_tenants(&mut out);

        assert!(out.contains("# TYPE http_request_duration_seconds histogram"));
        assert!(out.contains(
            "http_request_duration_seconds_count{method=\"GET\",route=\"/api/tasks\",status=\"200\"} 2"
        ));
        assert!(out.contains(
            "http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/tasks\",status=\"200\",le=\"+Inf\"} 2"
        ));
        assert!(out.contains("http_requests_by_tenant_total{tenant=\"acme\"} 2"));
    }
}
