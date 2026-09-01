//! Soak-run configuration (KAIROS-T-0046): defaults → optional TOML
//! profile (`--config`) → CLI flag overrides, in that precedence order.
//!
//! Two documented profiles (see the crate README section in `main.rs`
//! docs and `angreal test soak`):
//!
//! - **smoke** (pre-release): `--duration 10m --rate 8` with the default
//!   mix — the whole tier-5 assertion set on a 10-minute window.
//! - **nightly** (hours-scale): `--duration 4h --rate 8` (or higher on a
//!   standing runner) — identical assertions, longer exposure.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

/// Latency budget defaults (ms), calibrated against the DECIDED contract:
/// the vision fixes 50ms p95 for "common operations (**list, read,
/// transition**)" — exactly those classes (plus `edit`, the other
/// single-row hot path) default to 50ms.
pub const DEFAULT_P95_BUDGET_MS: f64 = 50.0;

/// Classes the vision's 50ms sentence does NOT cover — `create` (short
/// code sequence + history snapshot + activity row + tsvector),
/// `conflict_edit`, `search`/`traverse` (full-text + recursive CTE) —
/// default to a still-tight 100ms (root-caused in KAIROS-T-0046: holding
/// them to 50ms asserted more than the vision decided).
pub const DEFAULT_WRITE_QUERY_P95_BUDGET_MS: f64 = 100.0;

/// Default MCP-session p95 budget: one `mcp` op is a whole session — five
/// sequential round trips (initialize → initialized → 3 tool calls).
pub const DEFAULT_MCP_P95_BUDGET_MS: f64 = 250.0;

/// Parse a human duration: `45s`, `10m`, `4h`, or bare seconds (`600`).
pub fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("duration is empty".to_string());
    }
    let (value, unit) = match raw.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((split, _)) => raw.split_at(split),
        None => (raw, "s"),
    };
    let value: u64 = value
        .parse()
        .map_err(|e| format!("duration {raw:?}: bad number ({e})"))?;
    let seconds = match unit.trim() {
        "s" | "sec" | "secs" => value,
        "m" | "min" | "mins" => value * 60,
        "h" | "hr" | "hrs" => value * 3600,
        other => {
            return Err(format!(
                "duration {raw:?}: unknown unit {other:?} (use s/m/h)"
            ));
        }
    };
    if seconds == 0 {
        return Err(format!("duration {raw:?} must be positive"));
    }
    Ok(Duration::from_secs(seconds))
}

/// The weighted operation mix (weights are relative, not percentages).
/// Defaults model the A-0012 tier-5 "realistic operation mix".
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpMix {
    /// Create a task on the worker's home delivery board.
    pub create: u32,
    /// Optimistic-concurrency content edit of an item this worker created.
    pub edit: u32,
    /// Deliberate 409 collision on a shared, contended item.
    pub conflict_edit: u32,
    /// Column transition along the board's transition graph.
    pub transition: u32,
    /// Plain reads: get item / board items / list.
    pub read: u32,
    /// Full-text search (`POST /api/search` with `q`).
    pub search: u32,
    /// Graph traversal search from the seeded strategy.
    pub traverse: u32,
    /// A full MCP session (initialize + whoami/board_items/get_item).
    pub mcp: u32,
}

impl Default for OpMix {
    fn default() -> Self {
        OpMix {
            create: 10,
            edit: 20,
            conflict_edit: 5,
            transition: 12,
            read: 25,
            search: 12,
            traverse: 8,
            mcp: 8,
        }
    }
}

impl OpMix {
    /// Total weight (never 0: an all-zero mix is a config error).
    pub fn total(&self) -> u32 {
        self.create
            + self.edit
            + self.conflict_edit
            + self.transition
            + self.read
            + self.search
            + self.traverse
            + self.mcp
    }
}

/// Continuous-assertion thresholds. Breach policy (documented for the
/// KAIROS-T-0046 AC): a **tenant-isolation** breach is FATAL — the run
/// stops immediately and exits 2; every other breach is printed loudly
/// (attributed, timestamped) the moment it is detected, the run continues
/// to gather data, and the process exits 1 at the end.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    /// Maximum tolerated error ratio (typed API errors + transport
    /// failures over completed ops), EXCLUDING the deliberate 409
    /// conflicts, which are counted separately as expected outcomes.
    pub max_error_rate: f64,
    /// p95 budgets in milliseconds per op class (client-side measurement:
    /// includes loopback network + serialization on top of server time —
    /// deliberately the user-visible number). Keys are op-class names
    /// (`create`, `edit`, `conflict_edit`, `transition`, `read`, `search`,
    /// `traverse`, `mcp`); classes without an entry use the defaults at
    /// the top of this module (50ms for the vision-named common ops +
    /// edit, 100ms for create/search/traverse/conflict_edit, 250ms for
    /// the multi-round-trip `mcp` session).
    pub p95_budget_ms: BTreeMap<String, f64>,
    /// Minimum samples in an op class before its p95 is asserted (small
    /// samples make p95 meaningless).
    pub min_samples_for_latency: usize,
    /// Grace period before the MID-RUN latency assertions arm: warm-up
    /// effects (JWKS fetch, pool growth, first MCP session) otherwise trip
    /// p95 on a handful of early samples. Warm-up samples still count in
    /// the cumulative report; the end-of-run check always runs.
    pub warmup_secs: u64,
    /// Fail if `/metrics` is missing (KAIROS-A-0013 decides the endpoint
    /// but the server does not serve it yet — M2 wiring; default `false`
    /// so its absence is REPORTED loudly, not silently passed, without
    /// failing the run on a known, tracked gap).
    pub require_metrics: bool,
    /// Hard cap on `item_history` rows per item (A-0004 boundedness; also
    /// asserted: rows == the item's version — history grows only by edits).
    pub max_history_rows_per_item: i64,
    /// Fail if the bystander tenant cannot be provisioned/read (needs the
    /// caller in `KAIROS_DEPLOYMENT_ADMINS`; `angreal test soak` sets it).
    pub require_bystander: bool,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            max_error_rate: 0.01,
            p95_budget_ms: BTreeMap::new(),
            min_samples_for_latency: 20,
            warmup_secs: 60,
            require_metrics: false,
            max_history_rows_per_item: 500,
            require_bystander: true,
        }
    }
}

impl Thresholds {
    /// The p95 budget for an op class (config override or the defaults).
    pub fn p95_budget(&self, class: &str) -> f64 {
        if let Some(ms) = self.p95_budget_ms.get(class) {
            return *ms;
        }
        match class {
            "mcp" => DEFAULT_MCP_P95_BUDGET_MS,
            "create" | "conflict_edit" | "search" | "traverse" => DEFAULT_WRITE_QUERY_P95_BUDGET_MS,
            // The vision-named common ops (read incl. list, transition)
            // plus edit.
            _ => DEFAULT_P95_BUDGET_MS,
        }
    }
}

/// The TOML profile file shape (everything optional; see [`SoakConfig`]).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileFile {
    pub url: Option<String>,
    pub issuer: Option<String>,
    pub tenant: Option<String>,
    pub bystander_tenant: Option<String>,
    pub duration: Option<String>,
    pub rate: Option<f64>,
    pub agent_workers: Option<usize>,
    pub ws_subscribers: Option<usize>,
    pub assert_interval_secs: Option<u64>,
    pub report: Option<PathBuf>,
    pub token_client_id: Option<String>,
    pub mix: Option<OpMix>,
    pub thresholds: Option<Thresholds>,
}

/// The fully resolved run configuration.
#[derive(Debug, Clone)]
pub struct SoakConfig {
    /// Base URL of the RUNNING deployment under test.
    pub url: String,
    /// The OIDC issuer minting workforce tokens (the dev-stack Dex).
    pub issuer: String,
    /// Tenant slug the workforce operates in (the `seed-demo` fixture).
    pub tenant: String,
    /// The bystander tenant whose state must not change during the run.
    pub bystander_tenant: String,
    /// How long to sustain the load.
    pub duration: Duration,
    /// Target sustained rate, ops/second across ALL workers.
    pub rate: f64,
    /// Simulated agent workers on the `svc@kairos.test` service identity
    /// (Dex has no client_credentials grant — the password grant against
    /// the service user is the decided KAIROS-T-0003 service-account path).
    /// The three humans (alice/bob/carol) are always present.
    pub agent_workers: usize,
    /// Standing `/ws/events` subscriber connections.
    pub ws_subscribers: usize,
    /// Continuous-assertion cadence in seconds.
    pub assert_interval_secs: u64,
    /// Where the JSON run report is written.
    pub report: PathBuf,
    /// OAuth client id to mint tokens through. Must match the server's
    /// `OIDC_AUDIENCE` (Dex stamps `aud` = requesting client id; a token
    /// minted through `kairos-svc` carries `aud=kairos-svc` and is the
    /// wrong-audience negative case in the middleware tests).
    pub token_client_id: String,
    pub mix: OpMix,
    pub thresholds: Thresholds,
}

impl Default for SoakConfig {
    fn default() -> Self {
        SoakConfig {
            url: "http://127.0.0.1:41080".to_string(),
            issuer: "http://localhost:41558/dex".to_string(),
            tenant: "demo".to_string(),
            bystander_tenant: "soak-bystander".to_string(),
            duration: Duration::from_secs(4 * 3600),
            rate: 8.0,
            agent_workers: 3,
            ws_subscribers: 2,
            assert_interval_secs: 30,
            report: PathBuf::from("soak-report.json"),
            token_client_id: "kairos-cli".to_string(),
            mix: OpMix::default(),
            thresholds: Thresholds::default(),
        }
    }
}

impl SoakConfig {
    /// Layer a parsed TOML profile over `self`.
    pub fn apply_profile(&mut self, profile: ProfileFile) -> Result<(), String> {
        if let Some(v) = profile.url {
            self.url = v;
        }
        if let Some(v) = profile.issuer {
            self.issuer = v;
        }
        if let Some(v) = profile.tenant {
            self.tenant = v;
        }
        if let Some(v) = profile.bystander_tenant {
            self.bystander_tenant = v;
        }
        if let Some(v) = profile.duration {
            self.duration = parse_duration(&v)?;
        }
        if let Some(v) = profile.rate {
            self.rate = v;
        }
        if let Some(v) = profile.agent_workers {
            self.agent_workers = v;
        }
        if let Some(v) = profile.ws_subscribers {
            self.ws_subscribers = v;
        }
        if let Some(v) = profile.assert_interval_secs {
            self.assert_interval_secs = v;
        }
        if let Some(v) = profile.report {
            self.report = v;
        }
        if let Some(v) = profile.token_client_id {
            self.token_client_id = v;
        }
        if let Some(v) = profile.mix {
            self.mix = v;
        }
        if let Some(v) = profile.thresholds {
            self.thresholds = v;
        }
        Ok(())
    }

    /// Validate the resolved configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.rate <= 0.0 || !self.rate.is_finite() {
            return Err(format!("rate must be positive, got {}", self.rate));
        }
        if self.mix.total() == 0 {
            return Err("operation mix has zero total weight".to_string());
        }
        if self.assert_interval_secs == 0 {
            return Err("assert_interval_secs must be positive".to_string());
        }
        if !(0.0..=1.0).contains(&self.thresholds.max_error_rate) {
            return Err(format!(
                "max_error_rate must be in [0,1], got {}",
                self.thresholds.max_error_rate
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("4h").unwrap(), Duration::from_secs(14_400));
        assert_eq!(parse_duration("600").unwrap(), Duration::from_secs(600));
        assert!(parse_duration("0m").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("10x").is_err());
        assert!(parse_duration("m").is_err());
    }

    #[test]
    fn profile_layering_and_precedence() {
        let profile: ProfileFile = toml::from_str(
            r#"
            duration = "30m"
            rate = 12.5
            [mix]
            create = 1
            edit = 2
            conflict_edit = 3
            transition = 4
            read = 5
            search = 6
            traverse = 7
            mcp = 8
            [thresholds]
            max_error_rate = 0.05
            require_metrics = true
            [thresholds.p95_budget_ms]
            search = 80.0
            "#,
        )
        .expect("profile parses");
        let mut config = SoakConfig::default();
        config.apply_profile(profile).expect("applies");
        assert_eq!(config.duration, Duration::from_secs(1800));
        assert_eq!(config.rate, 12.5);
        assert_eq!(config.mix.total(), 36);
        assert!(config.thresholds.require_metrics);
        assert_eq!(config.thresholds.max_error_rate, 0.05);
        // Budget lookup: overridden, the three default tiers.
        assert_eq!(config.thresholds.p95_budget("search"), 80.0);
        assert_eq!(config.thresholds.p95_budget("edit"), DEFAULT_P95_BUDGET_MS);
        assert_eq!(config.thresholds.p95_budget("read"), DEFAULT_P95_BUDGET_MS);
        assert_eq!(
            config.thresholds.p95_budget("transition"),
            DEFAULT_P95_BUDGET_MS
        );
        assert_eq!(
            config.thresholds.p95_budget("create"),
            DEFAULT_WRITE_QUERY_P95_BUDGET_MS
        );
        assert_eq!(
            config.thresholds.p95_budget("traverse"),
            DEFAULT_WRITE_QUERY_P95_BUDGET_MS
        );
        assert_eq!(
            config.thresholds.p95_budget("mcp"),
            DEFAULT_MCP_P95_BUDGET_MS
        );
        assert_eq!(config.thresholds.warmup_secs, 60);
        // Untouched fields keep their defaults.
        assert_eq!(config.tenant, "demo");
        config.validate().expect("valid");
    }

    #[test]
    fn unknown_profile_keys_are_rejected() {
        let parsed: Result<ProfileFile, _> = toml::from_str("not_a_field = 1");
        assert!(
            parsed.is_err(),
            "typo'd profile keys must not pass silently"
        );
    }

    #[test]
    fn validation_rejects_nonsense() {
        let mut config = SoakConfig {
            rate: 0.0,
            ..SoakConfig::default()
        };
        assert!(config.validate().is_err());
        config.rate = 5.0;
        config.mix = OpMix {
            create: 0,
            edit: 0,
            conflict_edit: 0,
            transition: 0,
            read: 0,
            search: 0,
            traverse: 0,
            mcp: 0,
        };
        assert!(config.validate().is_err());
    }
}
