//! `kairos-soak` — the KAIROS-A-0012 tier-5 workforce soak driver
//! (KAIROS-T-0046).
//!
//! Drives a simulated organization against a RUNNING Kairos deployment
//! (`--url`; the driver never boots services — `angreal test soak` does
//! the boot-and-seed choreography): alice/bob/carol (the Dex fixture
//! humans) plus N agent workers on the `svc@kairos.test` service identity
//! execute the tier-5 mix — creates, edits with deliberate 409
//! collisions, transitions, searches, traversals, MCP sessions, standing
//! WS subscribers — at a sustained `--rate` for `--duration`.
//!
//! Continuous assertions (every `--assert-interval` seconds) and their
//! breach policy are documented on [`config::Thresholds`]; the run report
//! goes to `--report` (JSON) and stdout. Exit codes: 0 = all assertions
//! held; 1 = breaches (attributed in the report); 2 = fatal
//! (tenant-isolation) breach, run stopped early; 3 = the run could not
//! even be set up.
//!
//! Profiles: **smoke** `--duration 10m` (pre-release), **nightly**
//! `--duration 4h` (hours-scale). See `config.rs`.

mod auth;
mod config;
mod mcp;
mod prom;
mod report;
mod stats;
mod workforce;
mod world;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use kairos_client::KairosClient;
use tokio::sync::watch;

use auth::PasswordToken;
use config::{ProfileFile, SoakConfig, parse_duration};
use prom::{MetricSample, check_pool_stability, is_pool_metric, parse_prometheus_text};
use stats::{Breach, Recorder, Severity, check_snapshot};
use workforce::{WorkerSpec, WsCounters, worker_loop, ws_subscriber};
use world::{Bystander, BystanderUnavailable, changed_sections, snapshot};

/// KAIROS-A-0012 tier-5 workforce soak driver (KAIROS-T-0046).
#[derive(Debug, Parser)]
#[command(name = "kairos-soak", version, about)]
struct Cli {
    /// Base URL of the RUNNING deployment under test.
    #[arg(long)]
    url: Option<String>,
    /// OIDC issuer for workforce tokens (the dev-stack Dex).
    #[arg(long)]
    issuer: Option<String>,
    /// Workload tenant slug (the seed-demo fixture).
    #[arg(long)]
    tenant: Option<String>,
    /// Bystander tenant slug for the isolation invariant.
    #[arg(long)]
    bystander_tenant: Option<String>,
    /// Run length, e.g. `10m`, `4h`, `45s` (default 4h).
    #[arg(long)]
    duration: Option<String>,
    /// Sustained target rate, ops/second across all workers.
    #[arg(long)]
    rate: Option<f64>,
    /// Agent workers on the svc service identity (humans are always 3).
    #[arg(long)]
    agent_workers: Option<usize>,
    /// Standing /ws/events subscriber connections.
    #[arg(long)]
    ws_subscribers: Option<usize>,
    /// Continuous-assertion cadence, seconds.
    #[arg(long)]
    assert_interval: Option<u64>,
    /// TOML workforce profile (CLI flags override its values).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Where to write the JSON run report.
    #[arg(long)]
    report: Option<PathBuf>,
}

/// Merge defaults <- profile <- CLI into the final config.
fn resolve_config(cli: Cli) -> Result<SoakConfig, String> {
    let mut config = SoakConfig::default();
    if let Some(path) = &cli.config {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read profile {}: {e}", path.display()))?;
        let profile: ProfileFile = toml::from_str(&raw)
            .map_err(|e| format!("profile {} does not parse: {e}", path.display()))?;
        config.apply_profile(profile)?;
    }
    if let Some(v) = cli.url {
        config.url = v.trim_end_matches('/').to_string();
    }
    if let Some(v) = cli.issuer {
        config.issuer = v;
    }
    if let Some(v) = cli.tenant {
        config.tenant = v;
    }
    if let Some(v) = cli.bystander_tenant {
        config.bystander_tenant = v;
    }
    if let Some(v) = cli.duration {
        config.duration = parse_duration(&v)?;
    }
    if let Some(v) = cli.rate {
        config.rate = v;
    }
    if let Some(v) = cli.agent_workers {
        config.agent_workers = v;
    }
    if let Some(v) = cli.ws_subscribers {
        config.ws_subscribers = v;
    }
    if let Some(v) = cli.assert_interval {
        config.assert_interval_secs = v;
    }
    if let Some(v) = cli.report {
        config.report = v;
    }
    config.validate()?;
    Ok(config)
}

/// The breach ledger: records everything, prints each (check, subject)
/// pair at most [`Self::MAX_PRINTS_PER_KEY`] times so an hours-scale run
/// with a persistent breach does not flood the console (the report keeps
/// every record).
#[derive(Debug, Default)]
struct BreachLog {
    printed: BTreeMap<String, u32>,
    all: Vec<Breach>,
    fatal: bool,
}

impl BreachLog {
    const MAX_PRINTS_PER_KEY: u32 = 3;
    const MAX_RECORDS: usize = 500;

    fn record(&mut self, breach: Breach) {
        let subject = breach.detail.split(':').next().unwrap_or("").to_string();
        let key = format!("{}|{subject}", breach.check);
        let count = self.printed.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count <= Self::MAX_PRINTS_PER_KEY {
            eprintln!(
                "SOAK BREACH [{:?}] {} {}: {}",
                breach.severity, breach.at, breach.check, breach.detail
            );
            if *count == Self::MAX_PRINTS_PER_KEY {
                eprintln!(
                    "  (further {key:?} breaches suppressed from live output; \
                     all are recorded in the report)"
                );
            }
        }
        if breach.severity == Severity::Fatal {
            self.fatal = true;
        }
        if self.all.len() < Self::MAX_RECORDS {
            self.all.push(breach);
        }
    }
}

/// What the assertion loop hands back when the run ends.
struct TickerFindings {
    metrics_available: Option<bool>,
    scrapes: usize,
    final_pool_gauges: BTreeMap<String, f64>,
}

/// The continuous-assertion loop: every interval, check error rate and
/// per-class p95, scrape `/metrics` for pool stability, and re-verify the
/// bystander tenant. A tenant-isolation mismatch is FATAL: it stops the
/// whole run immediately (exit 2).
#[allow(clippy::too_many_arguments)]
async fn assertion_loop(
    config: Arc<SoakConfig>,
    recorder: Arc<Recorder>,
    http: reqwest::Client,
    bystander: Option<Arc<Bystander>>,
    log: Arc<Mutex<BreachLog>>,
    stop_tx: Arc<watch::Sender<bool>>,
    mut stop: watch::Receiver<bool>,
) -> TickerFindings {
    let mut findings = TickerFindings {
        metrics_available: None,
        scrapes: 0,
        final_pool_gauges: BTreeMap::new(),
    };
    let mut samples: Vec<MetricSample> = Vec::new();
    let interval = Duration::from_secs(config.assert_interval_secs);
    let started = std::time::Instant::now();
    let warmup = Duration::from_secs(config.thresholds.warmup_secs);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = stop.changed() => break,
        }
        if *stop.borrow() {
            break;
        }

        // 1. Error rate + latency budgets over the run so far. During the
        // warm-up grace the LATENCY assertions stay disarmed (JWKS fetch,
        // pool growth, first MCP session inflate a tiny early sample);
        // error-rate breaches always fire. The end-of-run check in run()
        // asserts everything over the full cumulative data.
        let summaries = recorder.summaries();
        let in_warmup = started.elapsed() < warmup;
        for breach in check_snapshot(&summaries, &config.thresholds) {
            if in_warmup && breach.check == "latency_p95" {
                continue;
            }
            log.lock().expect("breach log").record(breach);
        }

        // 2. /metrics scrape: pool-gauge stability (A-0013).
        match http.get(format!("{}/metrics", config.url)).send().await {
            Ok(response) if response.status().is_success() => {
                findings.metrics_available = Some(true);
                findings.scrapes += 1;
                let body = response.text().await.unwrap_or_default();
                let sample = parse_prometheus_text(&body);
                findings.final_pool_gauges = sample
                    .iter()
                    .filter(|(k, _)| is_pool_metric(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                samples.push(sample);
                if samples.len() > 64 {
                    samples.remove(0);
                }
                for breach in check_pool_stability(&samples) {
                    log.lock().expect("breach log").record(breach);
                }
            }
            Ok(response) => {
                let status = response.status().as_u16();
                if findings.metrics_available.is_none() {
                    eprintln!(
                        "SOAK NOTE: GET /metrics -> {status}. KAIROS-A-0013 decides this \
                         endpoint but the server does not serve it yet (M2 wiring); the \
                         pool-stability assertion is SKIPPED and reported as unavailable."
                    );
                    if config.thresholds.require_metrics {
                        log.lock().expect("breach log").record(Breach::now(
                            Severity::Breach,
                            "pool_metrics",
                            format!(
                                "/metrics unavailable (HTTP {status}) and \
                                 thresholds.require_metrics is set"
                            ),
                        ));
                    }
                }
                findings.metrics_available = Some(false);
            }
            Err(e) => {
                if findings.metrics_available.is_none() {
                    eprintln!("SOAK NOTE: GET /metrics failed ({e}); treating as unavailable.");
                }
                findings.metrics_available = Some(false);
            }
        }

        // 3. Bystander tenant: isolation must hold DURING the run too.
        if let Some(bystander) = &bystander {
            match snapshot(&bystander.client).await {
                Ok(current) => {
                    let changed = changed_sections(&bystander.baseline, &current);
                    if !changed.is_empty() {
                        log.lock().expect("breach log").record(Breach::now(
                            Severity::Fatal,
                            "tenant_isolation",
                            format!(
                                "bystander tenant {:?} changed mid-run (sections: {}) — \
                                 STOPPING the run",
                                bystander.tenant,
                                changed.join(", ")
                            ),
                        ));
                        let _ = stop_tx.send(true);
                        break;
                    }
                }
                Err(e) => {
                    log.lock().expect("breach log").record(Breach::now(
                        Severity::Breach,
                        "tenant_isolation",
                        format!("mid-run bystander snapshot failed: {e}"),
                    ));
                }
            }
        }
    }
    findings
}

async fn run() -> Result<u8, String> {
    let config = Arc::new(resolve_config(Cli::parse())?);
    let started_at = chrono::Utc::now();
    println!(
        "kairos-soak: {} for {:?} at {:.1} ops/s (tenant {:?}, issuer {}, report {})",
        config.url,
        config.duration,
        config.rate,
        config.tenant,
        config.issuer,
        config.report.display()
    );

    let http = reqwest::Client::new();

    // Reachability gate: the deployment must already be up.
    let mut healthy = false;
    for _ in 0..10 {
        if let Ok(response) = http.get(format!("{}/healthz", config.url)).send().await
            && response.status().is_success()
        {
            healthy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !healthy {
        return Err(format!(
            "{}/healthz never answered — the driver targets a RUNNING deployment \
             (boot one via `angreal test soak`)",
            config.url
        ));
    }

    // Workforce identities (Dex password grants; see auth.rs for why the
    // svc service account also mints through the kairos-cli client).
    let identity = |user: &str| {
        Arc::new(PasswordToken::new(
            http.clone(),
            &config.issuer,
            &config.token_client_id,
            &format!("{user}@kairos.test"),
            &format!("{user}-password"),
        ))
    };
    let tokens: BTreeMap<&str, Arc<PasswordToken>> = ["alice", "bob", "carol", "svc"]
        .into_iter()
        .map(|user| (user, identity(user)))
        .collect();
    let client_for = |user: &str| {
        KairosClient::new(&config.url, tokens[user].clone()).with_tenant(&config.tenant)
    };
    let alice = client_for("alice");
    let bob = client_for("bob");
    let carol = client_for("carol");
    let svc = client_for("svc");

    // World setup: enrollment, grants, shared fixtures.
    let world = Arc::new(
        world::setup_world(&alice, &[("bob", &bob), ("carol", &carol), ("svc", &svc)]).await?,
    );
    println!(
        "setup: {} delivery boards, collision items {:?}, traverse root {}, initiative {}",
        world.delivery_boards.len(),
        world.collision_codes,
        world.strategy_code,
        world.soak_initiative_code
    );

    // Bystander tenant + baseline snapshot.
    let admin = KairosClient::new(&config.url, tokens["alice"].clone());
    let bystander_client = KairosClient::new(&config.url, tokens["alice"].clone())
        .with_tenant(&config.bystander_tenant);
    let log = Arc::new(Mutex::new(BreachLog::default()));
    let mut bystander_status = String::new();
    let bystander =
        match world::setup_bystander(&admin, bystander_client, &config.bystander_tenant).await {
            Ok(bystander) => {
                println!(
                    "setup: bystander tenant {:?} snapshotted ({} sections)",
                    bystander.tenant,
                    bystander.baseline.len()
                );
                Some(Arc::new(bystander))
            }
            Err(BystanderUnavailable::NotDeploymentAdmin(message)) => {
                bystander_status = format!(
                    "unavailable: caller is not a deployment admin ({message}) — set \
                 KAIROS_DEPLOYMENT_ADMINS to alice's OIDC sub (angreal test soak does)"
                );
                eprintln!("SOAK NOTE: bystander isolation check {bystander_status}");
                if config.thresholds.require_bystander {
                    log.lock().expect("breach log").record(Breach::now(
                        Severity::Breach,
                        "tenant_isolation",
                        bystander_status.clone(),
                    ));
                }
                None
            }
            Err(BystanderUnavailable::Failed(message)) => {
                bystander_status = format!("unavailable: {message}");
                eprintln!("SOAK NOTE: bystander isolation check {bystander_status}");
                if config.thresholds.require_bystander {
                    log.lock().expect("breach log").record(Breach::now(
                        Severity::Breach,
                        "tenant_isolation",
                        bystander_status.clone(),
                    ));
                }
                None
            }
        };

    // Fan out: workers, WS subscribers, the assertion ticker.
    let recorder = Arc::new(Recorder::default());
    let (stop_tx, stop_rx) = watch::channel(false);
    let stop_tx = Arc::new(stop_tx);

    let ws_counters = Arc::new(WsCounters::default());
    let mut ws_handles = Vec::new();
    for n in 0..config.ws_subscribers {
        let subscriber = if n % 2 == 0 {
            client_for("alice")
        } else {
            client_for("carol")
        };
        ws_handles.push(tokio::spawn(ws_subscriber(
            subscriber,
            ws_counters.clone(),
            stop_rx.clone(),
        )));
    }

    let mut specs: Vec<WorkerSpec> = Vec::new();
    for (i, user) in ["alice", "bob", "carol"].into_iter().enumerate() {
        specs.push(WorkerSpec {
            name: user.to_string(),
            client: client_for(user),
            token: tokens[user].clone(),
            home_board: i % world.delivery_boards.len(),
            seed: 0xC0FFEE + i as u64,
        });
    }
    for n in 0..config.agent_workers {
        specs.push(WorkerSpec {
            name: format!("agent-{}", n + 1),
            client: client_for("svc"),
            token: tokens["svc"].clone(),
            home_board: n % world.delivery_boards.len(),
            seed: 0xA6E17 + n as u64,
        });
    }
    let human_workers = 3;
    let mut worker_handles = Vec::new();
    for spec in specs {
        worker_handles.push(tokio::spawn(worker_loop(
            spec,
            world.clone(),
            config.clone(),
            recorder.clone(),
            http.clone(),
            stop_rx.clone(),
        )));
    }

    let ticker = tokio::spawn(assertion_loop(
        config.clone(),
        recorder.clone(),
        http.clone(),
        bystander.clone(),
        log.clone(),
        stop_tx.clone(),
        stop_rx.clone(),
    ));

    // Run for the configured duration (or until a fatal breach stops us).
    let mut stop_watch = stop_rx.clone();
    tokio::select! {
        _ = tokio::time::sleep(config.duration) => {
            println!("kairos-soak: duration reached; draining workers...");
        }
        _ = stop_watch.changed() => {
            eprintln!("kairos-soak: stopped early by a fatal breach");
        }
    }
    let _ = stop_tx.send(true);

    let mut created_codes: Vec<String> = Vec::new();
    for handle in worker_handles {
        created_codes.extend(handle.await.map_err(|e| format!("worker panicked: {e}"))?);
    }
    for handle in ws_handles {
        handle
            .await
            .map_err(|e| format!("ws subscriber panicked: {e}"))?;
    }
    let findings = ticker.await.map_err(|e| format!("ticker panicked: {e}"))?;

    // Final assertions.
    let summaries = recorder.summaries();
    for breach in check_snapshot(&summaries, &config.thresholds) {
        log.lock().expect("breach log").record(breach);
    }

    // History boundedness: the contended items plus a worker-pool sample.
    let mut history_codes = world.collision_codes.clone();
    history_codes.extend(created_codes.iter().take(20).cloned());
    let (history_bound, history_breaches) = world::history_bound_check(
        &alice,
        &history_codes,
        config.thresholds.max_history_rows_per_item,
    )
    .await;
    for breach in history_breaches {
        log.lock().expect("breach log").record(breach);
    }

    // Final bystander verdict.
    let mut changed = Vec::new();
    if let Some(bystander) = &bystander {
        match snapshot(&bystander.client).await {
            Ok(current) => {
                changed = changed_sections(&bystander.baseline, &current);
                if changed.is_empty() {
                    bystander_status = "unchanged".to_string();
                } else {
                    bystander_status = "CHANGED".to_string();
                    log.lock().expect("breach log").record(Breach::now(
                        Severity::Fatal,
                        "tenant_isolation",
                        format!(
                            "bystander tenant {:?} changed over the run (sections: {})",
                            bystander.tenant,
                            changed.join(", ")
                        ),
                    ));
                }
            }
            Err(e) => {
                bystander_status = format!("unavailable at end: {e}");
                log.lock().expect("breach log").record(Breach::now(
                    Severity::Breach,
                    "tenant_isolation",
                    format!("final bystander snapshot failed: {e}"),
                ));
            }
        }
    }

    // Report.
    let (breaches, _fatal) = {
        let log = log.lock().expect("breach log");
        (log.all.clone(), log.fatal)
    };
    let outcome = report::outcome(&breaches).to_string();
    let run_report = report::RunReport {
        tool: format!("kairos-soak {}", env!("CARGO_PKG_VERSION")),
        target: config.url.clone(),
        tenant: config.tenant.clone(),
        started_at: started_at.to_rfc3339(),
        finished_at: chrono::Utc::now().to_rfc3339(),
        configured: report::ConfigEcho {
            duration_secs: config.duration.as_secs(),
            rate_ops_per_sec: config.rate,
            human_workers,
            agent_workers: config.agent_workers,
            ws_subscribers: config.ws_subscribers,
            assert_interval_secs: config.assert_interval_secs,
            max_error_rate: config.thresholds.max_error_rate,
        },
        totals: report::totals(&summaries),
        ops: summaries,
        ws: report::WsReport {
            subscribers: config.ws_subscribers,
            connects: ws_counters
                .connects
                .load(std::sync::atomic::Ordering::Relaxed),
            events_received: ws_counters
                .events
                .load(std::sync::atomic::Ordering::Relaxed),
            reconnects: ws_counters
                .reconnects
                .load(std::sync::atomic::Ordering::Relaxed),
        },
        metrics: report::MetricsReport {
            available: findings.metrics_available,
            scrapes: findings.scrapes,
            final_pool_gauges: findings.final_pool_gauges,
        },
        history_bound,
        bystander: report::BystanderReport {
            tenant: config.bystander_tenant.clone(),
            status: bystander_status,
            changed_sections: changed,
        },
        breaches,
        error_samples: recorder.error_samples(),
        outcome: outcome.clone(),
    };

    let json = serde_json::to_string_pretty(&run_report)
        .map_err(|e| format!("serializing the report: {e}"))?;
    std::fs::write(&config.report, &json)
        .map_err(|e| format!("writing {}: {e}", config.report.display()))?;
    println!("report written to {}", config.report.display());
    run_report.print_summary();

    Ok(report::exit_code(&outcome))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("kairos-soak: SETUP FAILED: {message}");
            ExitCode::from(3)
        }
    }
}
