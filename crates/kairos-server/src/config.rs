//! Server configuration per KAIROS-A-0013: env vars only, fail fast at
//! startup with an explicit message naming the variable.
//!
//! Env parsing lives HERE and only here — the rest of the crate (and the
//! integration tests) consume the [`AppConfig`] struct, so tests construct
//! variants directly instead of mutating process environment.

use std::net::SocketAddr;

/// Default bind address when `KAIROS_BIND_ADDR` is unset.
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

/// `KAIROS_LOG_FORMAT` — structured JSON by default (KAIROS-A-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Structured JSON logs (production default).
    #[default]
    Json,
    /// Human-readable logs for local development.
    Pretty,
}

/// `KAIROS_API_BEARER` — which OIDC token the browser GUI (and CLI) present
/// as the `/api` bearer (KAIROS-T-0054).
///
/// The server-side validation (`middleware::auth`) is identical either way:
/// it validates whatever RS256 JWT arrives against `iss`/`aud`/`exp` + JWKS.
/// This flag only tells the *clients* which token to send, so a deployment
/// can point at an issuer whose **access token is opaque** (not a JWT) — most
/// importantly Google / Google Workspace, whose access token is a `ya29.…`
/// string but whose **ID token** is a validatable RS256 JWT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiBearer {
    /// Send the OAuth `access_token` (default; suits Dex, Keycloak, and any
    /// issuer that mints JWT access tokens).
    #[default]
    AccessToken,
    /// Send the OIDC `id_token` (suits issuers with opaque access tokens,
    /// e.g. Google Workspace).
    IdToken,
}

impl ApiBearer {
    /// The wire name, as sent to the SPA in `/api/config` and used as the
    /// token-response JSON key both clients read.
    pub fn as_str(self) -> &'static str {
        match self {
            ApiBearer::AccessToken => "access_token",
            ApiBearer::IdToken => "id_token",
        }
    }
}

/// A configuration error worth failing startup over. The message always
/// names the offending variable (A-0013).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    /// A required variable is unset.
    #[error("{var} is not set; {hint}")]
    Missing {
        /// The environment variable name.
        var: &'static str,
        /// What to set it to.
        hint: &'static str,
    },
    /// A variable is set to something unparseable.
    #[error("{var} is invalid: {message}")]
    Invalid {
        /// The environment variable name.
        var: &'static str,
        /// Why the value was rejected.
        message: String,
    },
}

/// Everything the server needs to run, resolved once at startup.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// `DATABASE_URL` — the shared Postgres (sole state, A-0013).
    pub database_url: String,
    /// `KAIROS_BIND_ADDR` — where axum listens (default `127.0.0.1:8080`).
    pub bind_addr: SocketAddr,
    /// `OIDC_ISSUER_URL` — the issuer; discovery + JWKS derive from it.
    pub oidc_issuer_url: String,
    /// `OIDC_AUDIENCE` — the `aud` claim tokens must carry (A-0010; for the
    /// Dex dev stack this is the OAuth client id, e.g. `kairos-cli`).
    /// Accepts a comma-separated allow-list (KAIROS-T-0055) for IdPs that
    /// mint a distinct `aud` per client (Google Workspace): a token
    /// matching ANY listed audience validates. Parsed and enforced
    /// non-empty at `Authenticator::discover` (startup).
    pub oidc_audience: String,
    /// `KAIROS_BASE_DOMAIN` — enables Host-subdomain tenant resolution
    /// (`acme.<base>` → tenant `acme`, A-0005 §2).
    pub base_domain: Option<String>,
    /// `KAIROS_SINGLE_TENANT` — fixed tenant slug; skips subdomain/header
    /// resolution entirely (A-0013 single-tenant mode).
    pub single_tenant: Option<String>,
    /// `KAIROS_DEPLOYMENT_ADMINS` — comma-separated OIDC `sub`s
    /// (`external_id`s, human users or A-0010 service accounts) allowed to
    /// call the cross-tenant `/api/admin/tenants` routes (KAIROS-T-0019).
    /// Empty/unset → those routes always return 403.
    pub deployment_admins: Vec<String>,
    /// `KAIROS_LOG_LEVEL` — tracing filter directive (default `info`).
    pub log_level: String,
    /// `KAIROS_LOG_FORMAT` — `json` (default) or `pretty`.
    pub log_format: LogFormat,
    /// `KAIROS_DEV_UI` — mount the Swagger UI at `/api/docs`
    /// (KAIROS-T-0023, A-0005 §6 "Swagger UI mounted in dev builds").
    /// Default `false`: the route does not exist in a production config.
    pub dev_ui: bool,
    /// `KAIROS_WEB_DIST` — serve the built GUI (`crates/kairos-web/dist`)
    /// from this directory (KAIROS-T-0039 dev flow). Unset in release
    /// builds, where assets are embedded via the `embed-web` feature
    /// (A-0013 single artifact); when both are present the directory
    /// wins (deliberate: lets a dev override an embedded build).
    pub web_dist: Option<std::path::PathBuf>,
    /// `KAIROS_EMBED_REFRESH_SECS` — how often the background refresher looks
    /// for items whose vectors have fallen behind (KAIROS-T-0190). Default 10;
    /// `0` disables it, which is what tests and any deployment that prefers to
    /// drive `embed-backfill` itself should use.
    ///
    /// There is no queue behind this on purpose. Staleness is *derived* — a
    /// content hash that no longer matches — so a sweep is self-healing, where a
    /// queue would be a second source of truth able to drift from the first.
    pub embed_refresh_secs: u64,
    /// `KAIROS_WEB_CLIENT_ID` — the public OAuth client id the SPA uses
    /// for its PKCE flow (KAIROS-T-0039, A-0010). Default `kairos-web`,
    /// matching the dev Dex fixture (`.angreal/dex/config.yaml`).
    pub web_client_id: String,
    /// `KAIROS_API_BEARER` — which OIDC token the GUI/CLI send as the `/api`
    /// bearer (KAIROS-T-0054). Default `access_token`; set `id_token` for
    /// issuers with opaque access tokens (Google Workspace).
    pub api_bearer: ApiBearer,
    /// `KAIROS_WEB_CLIENT_SECRET` — the OAuth client secret for a
    /// **confidential** GUI client (KAIROS-T-0056). `None` (default) keeps the
    /// public-client behavior (Dex/Keycloak). When set, the server-side token
    /// relay presents it on the code/refresh exchange — required for Google
    /// Workspace, whose "Web application" clients are confidential. It is
    /// server-side only: never sent to the browser or `/api/config`.
    pub web_client_secret: Option<String>,
    /// `KAIROS_PUBLIC_URL` — the deployment's externally reachable base
    /// URL (KAIROS-T-0097). Needed to hand operators a webhook delivery
    /// URL to paste into GitHub/GitLab. Deliberately NOT inferred from the
    /// request `Host` header: that is attacker-controlled, and the value
    /// ends up configured in a third party.
    pub public_url: Option<String>,
    /// `KAIROS_WEBHOOK_SIGNING_KEY` — the deployment secret every webhook
    /// secret is derived from (KAIROS-T-0097, see
    /// [`crate::forge::auth`]). Absent ⇒ forge connections cannot be
    /// created or verified; the feature is simply off.
    pub webhook_signing_key: Option<String>,
    /// `KAIROS_OTEL_ENDPOINT` (KAIROS-T-0196): the OTLP/HTTP traces endpoint,
    /// e.g. `http://otel-collector:4318/v1/traces`. **Unset means no tracing at
    /// all** — no exporter is built and no span leaves the process. This is the
    /// switch, and it is off by default.
    ///
    /// Note the port: OTLP over HTTP is 4318. A collector's gRPC port (4317) will
    /// accept the connection and reject the payload, which is a confusing way to
    /// find out, so the how-to says so.
    pub otel_endpoint: Option<String>,
    /// `KAIROS_OTEL_SAMPLE_RATIO` (KAIROS-T-0196): head sampling, 0.0..=1.0,
    /// default **1.0**.
    ///
    /// Sampling everything is the right default for a switch that is off unless
    /// you turned it on: someone who configured a collector wants to see spans in
    /// it, and a default that quietly dropped 99% of them would read as a bug. A
    /// busy deployment turns it down; that is a decision about *volume*, which is
    /// the operator's, not ours.
    ///
    /// Head sampling only. Tail sampling — keep the slow and failed traces, drop
    /// the boring ones — is a collector feature and a better place for the
    /// decision, because the collector can see the whole trace and this process
    /// cannot.
    pub otel_sample_ratio: f64,
    /// `KAIROS_AUTH_MAX_FAILURES` (KAIROS-T-0202): failed authentications within
    /// [`Self::auth_failure_window_secs`] before a lockout, default **5**.
    ///
    /// **0 turns throttling off entirely.** That escape hatch exists because a
    /// throttle that misfires locks people out of their own deployment, and the
    /// fix must not require a new build.
    pub auth_max_failures: u32,
    /// `KAIROS_AUTH_FAILURE_WINDOW_SECS` (KAIROS-T-0202): how long failures
    /// accumulate before they are forgotten, default **300**.
    pub auth_failure_window_secs: u64,
    /// `KAIROS_AUTH_LOCKOUT_SECS` (KAIROS-T-0202): how long a lockout lasts once
    /// tripped, default **60**.
    ///
    /// Short on purpose. A long lockout is a denial-of-service an attacker can
    /// aim at a known account, and it turns one person's typo into a support
    /// request.
    pub auth_lockout_secs: u64,
    /// `KAIROS_TRUSTED_PROXY` (KAIROS-T-0202): trust `X-Forwarded-For` for the
    /// client address, default **false**.
    ///
    /// Off by default because the header is caller-supplied. Anyone can send
    /// `X-Forwarded-For: <anything>`, so trusting it on a directly exposed server
    /// hands an attacker a fresh identity per request — it does not weaken the
    /// throttle, it removes it. Turn it on only when every request arrives through
    /// a proxy you control that appends the peer it saw (the reference compose
    /// stack's Caddy does; a direct deployment has nothing in front of it).
    pub trusted_proxy: bool,
}

impl AppConfig {
    /// Read configuration from the process environment, failing fast on
    /// missing/invalid values with a message naming the variable.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|var| std::env::var(var).ok())
    }

    /// Testable core of [`Self::from_env`]: resolve from any lookup
    /// function. Empty values are treated as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |var: &str| lookup(var).filter(|v| !v.trim().is_empty());

        let required = |var: &'static str, hint: &'static str| {
            get(var).ok_or(ConfigError::Missing { var, hint })
        };

        let database_url = required(
            "DATABASE_URL",
            "it is required to reach PostgreSQL \
             (e.g. postgres://kairos:kairos@localhost:41432/kairos)",
        )?;
        let oidc_issuer_url = required(
            "OIDC_ISSUER_URL",
            "it is required to validate bearer tokens \
             (e.g. http://localhost:41558/dex for the dev stack)",
        )?;
        let oidc_audience = required(
            "OIDC_AUDIENCE",
            "it is required to validate the `aud` claim of bearer tokens \
             (the OAuth client id, e.g. kairos-cli for the dev stack)",
        )?;

        let bind_raw = get("KAIROS_BIND_ADDR").unwrap_or_else(|| DEFAULT_BIND_ADDR.to_string());
        let bind_addr: SocketAddr = bind_raw.parse().map_err(|e| ConfigError::Invalid {
            var: "KAIROS_BIND_ADDR",
            message: format!("{bind_raw:?} is not a socket address ({e})"),
        })?;

        let log_format = match get("KAIROS_LOG_FORMAT").as_deref() {
            None | Some("json") => LogFormat::Json,
            Some("pretty") => LogFormat::Pretty,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_LOG_FORMAT",
                    message: format!("{other:?} is not one of: json, pretty"),
                });
            }
        };

        let api_bearer = match get("KAIROS_API_BEARER").as_deref() {
            None | Some("access_token") => ApiBearer::AccessToken,
            Some("id_token") => ApiBearer::IdToken,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_API_BEARER",
                    message: format!("{other:?} is not one of: access_token, id_token"),
                });
            }
        };

        let dev_ui = match get("KAIROS_DEV_UI").as_deref() {
            None | Some("false") | Some("0") => false,
            Some("true") | Some("1") => true,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_DEV_UI",
                    message: format!("{other:?} is not one of: true, 1, false, 0"),
                });
            }
        };

        // KAIROS-T-0196. The ratio is validated here rather than clamped, because
        // a typo'd sampling rate is the kind of thing an operator should hear
        // about at boot rather than discover from a suspiciously empty collector.
        let otel_sample_ratio = match get("KAIROS_OTEL_SAMPLE_RATIO") {
            None => 1.0,
            Some(raw) => {
                let parsed: f64 = raw.trim().parse().map_err(|_| ConfigError::Invalid {
                    var: "KAIROS_OTEL_SAMPLE_RATIO",
                    message: format!("{raw:?} is not a number between 0.0 and 1.0"),
                })?;
                if !(0.0..=1.0).contains(&parsed) {
                    return Err(ConfigError::Invalid {
                        var: "KAIROS_OTEL_SAMPLE_RATIO",
                        message: format!("{parsed} is outside 0.0..=1.0"),
                    });
                }
                parsed
            }
        };

        // KAIROS-T-0202. Parsed strictly for the same reason as the sampling ratio
        // above: a typo in a security limit must be heard at boot, not inferred
        // later from a throttle that never fires.
        let auth_max_failures = parse_num::<u32>(&get, "KAIROS_AUTH_MAX_FAILURES", 5)?;
        let auth_failure_window_secs =
            parse_num::<u64>(&get, "KAIROS_AUTH_FAILURE_WINDOW_SECS", 300)?;
        let auth_lockout_secs = parse_num::<u64>(&get, "KAIROS_AUTH_LOCKOUT_SECS", 60)?;
        let trusted_proxy = match get("KAIROS_TRUSTED_PROXY").as_deref() {
            None | Some("") | Some("false") | Some("0") => false,
            Some("true") | Some("1") => true,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_TRUSTED_PROXY",
                    message: format!("{other:?} is not one of: true, 1, false, 0"),
                });
            }
        };

        let deployment_admins = get("KAIROS_DEPLOYMENT_ADMINS")
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            database_url,
            bind_addr,
            oidc_issuer_url: oidc_issuer_url.trim_end_matches('/').to_string(),
            oidc_audience,
            base_domain: get("KAIROS_BASE_DOMAIN"),
            single_tenant: get("KAIROS_SINGLE_TENANT"),
            deployment_admins,
            log_level: get("KAIROS_LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            log_format,
            dev_ui,
            web_dist: get("KAIROS_WEB_DIST").map(std::path::PathBuf::from),
            embed_refresh_secs: get("KAIROS_EMBED_REFRESH_SECS")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(10),
            web_client_id: get("KAIROS_WEB_CLIENT_ID").unwrap_or_else(|| "kairos-web".to_string()),
            api_bearer,
            web_client_secret: get("KAIROS_WEB_CLIENT_SECRET"),
            public_url: get("KAIROS_PUBLIC_URL").map(|url| url.trim_end_matches('/').to_string()),
            webhook_signing_key: get("KAIROS_WEBHOOK_SIGNING_KEY"),
            otel_endpoint: get("KAIROS_OTEL_ENDPOINT"),
            otel_sample_ratio,
            auth_max_failures,
            auth_failure_window_secs,
            auth_lockout_secs,
            trusted_proxy,
        })
    }
}

/// Parse an optional numeric variable, or fail naming it. An empty value is
/// unset, matching every other variable here — the chart emits empty strings for
/// values the operator left alone.
fn parse_num<T: std::str::FromStr>(
    get: &impl Fn(&str) -> Option<String>,
    var: &'static str,
    default: T,
) -> Result<T, ConfigError> {
    match get(var) {
        None => Ok(default),
        Some(raw) => raw.trim().parse().map_err(|_| ConfigError::Invalid {
            var,
            message: format!("{raw:?} is not a whole number"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |var| map.get(var).cloned()
    }

    const MINIMAL: &[(&str, &str)] = &[
        ("DATABASE_URL", "postgres://k:k@localhost:5432/kairos"),
        ("OIDC_ISSUER_URL", "http://localhost:5558/dex/"),
        ("OIDC_AUDIENCE", "kairos-cli"),
    ];

    #[test]
    fn minimal_config_applies_defaults() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.bind_addr.to_string(), DEFAULT_BIND_ADDR);
        assert_eq!(config.log_level, "info");
        assert_eq!(config.log_format, LogFormat::Json);
        assert_eq!(config.base_domain, None);
        assert_eq!(config.single_tenant, None);
        // The Swagger UI is absent from a default (production) config.
        assert!(!config.dev_ui);
        // GUI defaults (KAIROS-T-0039): no dev dist dir; dev-stack client id.
        assert_eq!(config.web_dist, None);
        assert_eq!(config.web_client_id, "kairos-web");
        // Default bearer is the access token (Dex/Keycloak behavior, T-0054).
        assert_eq!(config.api_bearer, ApiBearer::AccessToken);
        // No web client secret by default (public client, T-0056).
        assert_eq!(config.web_client_secret, None);
        // Trailing slash on the issuer is normalized away.
        assert_eq!(config.oidc_issuer_url, "http://localhost:5558/dex");
    }

    #[test]
    fn missing_required_var_names_it() {
        let err = AppConfig::from_lookup(lookup(&MINIMAL[..2])).unwrap_err();
        assert!(err.to_string().starts_with("OIDC_AUDIENCE is not set"));
    }

    #[test]
    fn empty_value_is_treated_as_unset() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_SINGLE_TENANT", "  "));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.single_tenant, None);
    }

    #[test]
    fn invalid_bind_addr_and_log_format_are_rejected() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_BIND_ADDR", "not-an-addr"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().starts_with("KAIROS_BIND_ADDR is invalid"));

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_LOG_FORMAT", "yaml"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().starts_with("KAIROS_LOG_FORMAT is invalid"));

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_DEV_UI", "yes"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().starts_with("KAIROS_DEV_UI is invalid"));

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_API_BEARER", "jwt"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().starts_with("KAIROS_API_BEARER is invalid"));
    }

    #[test]
    fn api_bearer_id_token_is_parsed() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_API_BEARER", "id_token"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.api_bearer, ApiBearer::IdToken);
        assert_eq!(config.api_bearer.as_str(), "id_token");
    }

    #[test]
    fn optional_vars_are_carried_through() {
        let mut vars = MINIMAL.to_vec();
        vars.extend([
            ("KAIROS_BASE_DOMAIN", "kairos.example"),
            ("KAIROS_SINGLE_TENANT", "acme"),
            ("KAIROS_BIND_ADDR", "0.0.0.0:9999"),
            ("KAIROS_LOG_FORMAT", "pretty"),
            ("KAIROS_LOG_LEVEL", "debug"),
            ("KAIROS_DEV_UI", "true"),
            ("KAIROS_WEB_DIST", "crates/kairos-web/dist"),
            ("KAIROS_WEB_CLIENT_ID", "my-idp-spa-client"),
            ("KAIROS_WEB_CLIENT_SECRET", "s3cr3t"),
        ]);
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert!(config.dev_ui);
        assert_eq!(config.web_client_secret.as_deref(), Some("s3cr3t"));
        assert_eq!(
            config.web_dist.as_deref(),
            Some(std::path::Path::new("crates/kairos-web/dist"))
        );
        assert_eq!(config.web_client_id, "my-idp-spa-client");
        assert_eq!(config.base_domain.as_deref(), Some("kairos.example"));
        assert_eq!(config.single_tenant.as_deref(), Some("acme"));
        assert_eq!(config.bind_addr.to_string(), "0.0.0.0:9999");
        assert_eq!(config.log_format, LogFormat::Pretty);
        assert_eq!(config.log_level, "debug");
    }

    // ---------------------------------------------------------------------
    // KAIROS-T-0196: trace export is off unless asked for, and a bad sampling
    // rate is heard about at boot.

    #[test]
    fn tracing_is_off_by_default_and_samples_everything_when_on() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(
            config.otel_endpoint, None,
            "no endpoint means no exporter is built and no span leaves the process"
        );
        assert_eq!(
            config.otel_sample_ratio, 1.0,
            "someone who configures a collector wants to see spans in it; a default \
             that quietly dropped most of them would read as a bug"
        );
    }

    #[test]
    fn a_bad_sample_ratio_fails_at_boot_rather_than_silently() {
        // Refused rather than clamped, deliberately. A typo'd sampling rate that
        // is clamped produces a collector that is mysteriously emptier than
        // expected, and nothing anywhere says why.
        for bad in ["1.5", "-0.1", "half", "", "100%"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_OTEL_SAMPLE_RATIO", bad));
            let result = AppConfig::from_lookup(lookup(&vars));
            if bad.is_empty() {
                // Empty is "unset" everywhere else in this config, and must stay
                // so here — an operator who blanks a value is turning it off.
                assert_eq!(
                    result.expect("empty is unset").otel_sample_ratio,
                    1.0,
                    "an empty value is unset, not invalid"
                );
            } else {
                let err = result.expect_err("{bad} must be refused");
                assert!(
                    err.to_string()
                        .starts_with("KAIROS_OTEL_SAMPLE_RATIO is invalid"),
                    "the error must name the variable: {err}"
                );
            }
        }
    }

    #[test]
    fn the_sampling_bounds_are_inclusive() {
        for (value, expected) in [("0", 0.0), ("0.0", 0.0), ("1", 1.0), ("0.25", 0.25)] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_OTEL_SAMPLE_RATIO", value));
            let config = AppConfig::from_lookup(lookup(&vars)).expect("valid ratio");
            assert_eq!(config.otel_sample_ratio, expected, "ratio {value}");
        }
    }

    // ---------------------------------------------------------------------
    // KAIROS-T-0177: three places must agree about the environment, and
    // nothing used to check that they did.
    //
    //   1. what the BINARY reads     (this crate, kairos-embed, kairos-core)
    //   2. what the DEPLOYMENTS set  (docker-compose.yaml, the chart templates)
    //   3. what the OPERATOR is told (deploy/.env.example)
    //
    // All three had drifted independently and in both directions: compose
    // documented `KAIROS_API_BEARER` and `KAIROS_WEB_CLIENT_SECRET` and
    // forwarded neither, while the chart emitted `KAIROS_OTEL_ENDPOINT`, which
    // no crate has ever read.
    //
    // Both failures are invisible by construction. Every one of these
    // variables is legitimately optional, so "dropped on the floor" and "not
    // set on purpose" produce the same observable behaviour — right up until
    // someone's Google Workspace login fails for reasons that point nowhere
    // near a compose file.
    //
    // The set the binary reads is RECORDED rather than grepped: all three
    // config types take a lookup closure, so a closure that remembers what it
    // was asked for is the authoritative answer and cannot drift from the code
    // the way a regex over `config.rs` would.

    use std::cell::RefCell;
    use std::collections::BTreeSet;

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    fn read_repo(rel: &str) -> String {
        let path = repo_root().join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Every variable the binary asks for, across all three config types and
    /// every branch of them.
    fn read_by_binary() -> BTreeSet<String> {
        let seen = RefCell::new(BTreeSet::new());
        let record = |var: &str, value: Option<String>| {
            seen.borrow_mut().insert(var.to_string());
            value
        };

        // The server's own config. MINIMAL satisfies the three required vars so
        // the function runs to completion instead of bailing at the first one;
        // every other `get` is unconditional, so one pass sees them all.
        let minimal: HashMap<&str, &str> = MINIMAL.iter().copied().collect();
        AppConfig::from_lookup(|var| record(var, minimal.get(var).map(|v| v.to_string())))
            .expect("MINIMAL is a valid config");

        // Embedding config branches on the provider, and the remote and local
        // arms read disjoint variables. Drive every arm, or the union is short.
        for provider in ["none", "local", "remote", "deterministic"] {
            let _ = kairos_embed::EmbedConfig::from_vars(|var| {
                record(
                    var,
                    Some(match var {
                        "KAIROS_EMBED_PROVIDER" => provider.to_string(),
                        "KAIROS_EMBED_URL" => "http://embed.invalid/v1".to_string(),
                        "KAIROS_EMBED_MODEL" => "a-model".to_string(),
                        "KAIROS_EMBED_TIMEOUT_SECS" => "5".to_string(),
                        "KAIROS_EMBED_ALLOW_DOWNLOAD" => "false".to_string(),
                        _ => "x".to_string(),
                    }),
                )
            });
        }

        // Retention (KAIROS-A-0004) reads each variable only when it is set, so
        // answer every lookup to reach all five.
        kairos_core::retention::RetentionConfig::from_lookup(|var| {
            record(
                var,
                Some(match var {
                    "KAIROS_RETENTION_MODE" => "archive".to_string(),
                    "KAIROS_ARCHIVE_TARGET" => "table".to_string(),
                    _ => "1".to_string(),
                }),
            )
        })
        .expect("those values are valid");

        seen.into_inner()
    }

    /// Environment keys a YAML `environment:`/`data:` block sets, by the shape
    /// both compose and the chart templates share: `SCREAMING_SNAKE: value` at
    /// some indentation. Helm control lines (`{{- if ... }}`) do not match.
    fn env_keys_in(yaml: &str) -> BTreeSet<String> {
        yaml.lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') {
                    return None;
                }
                let (key, _) = trimmed.split_once(':')?;
                let ok = !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && key.starts_with(|c: char| c.is_ascii_uppercase());
                ok.then(|| key.to_string())
            })
            .collect()
    }

    /// Variables `.env.example` tells an operator to set (`NAME=` at column 0,
    /// commented-out examples included — being commented out is how that file
    /// shows an optional value, not how it hides one).
    fn documented_in_env_example() -> BTreeSet<String> {
        read_repo("deploy/.env.example")
            .lines()
            .filter_map(|line| {
                let line = line.trim_start_matches('#').trim_start();
                let (key, _) = line.split_once('=')?;
                let ok = !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && key.starts_with(|c: char| c.is_ascii_uppercase());
                ok.then(|| key.to_string())
            })
            .collect()
    }

    /// Consumed by the deployment machinery itself rather than by the binary,
    /// so they are correctly absent from the server's environment.
    const NOT_THE_BINARYS: &[&str] = &[
        // Caddy templates its site address from this.
        "KAIROS_SITE_ADDRESS",
        // Selects the image tag; compose reads it, the container never sees it.
        "KAIROS_VERSION",
        // Composed into DATABASE_URL by compose; the server gets the URL.
        "POSTGRES_PASSWORD",
        // These two initialise the PostgreSQL container itself (compose's
        // postgres service and the chart's optional bundled StatefulSet).
        // They are that image's interface, not ours.
        "POSTGRES_DB",
        "POSTGRES_USER",
    ];

    #[test]
    fn deployments_set_only_variables_the_binary_reads() {
        let read = read_by_binary();
        let mut emitted = env_keys_in(&read_repo("deploy/docker-compose.yaml"));
        for template in [
            "configmap.yaml",
            "secret.yaml",
            "embed-secret.yaml",
            "webauth-secret.yaml",
            "deployment.yaml",
            "postgresql.yaml",
        ] {
            emitted.extend(env_keys_in(&read_repo(&format!(
                "deploy/helm/kairos/templates/{template}"
            ))));
        }

        let unread: Vec<_> = emitted
            .iter()
            .filter(|v| !read.contains(*v) && !NOT_THE_BINARYS.contains(&v.as_str()))
            .cloned()
            .collect();

        assert!(
            unread.is_empty(),
            "the reference deployments set variables no crate reads, so setting \
             them does nothing and saying so in values.yaml is a lie: {unread:?}\n\
             Either wire them up, or remove them from the deployment AND its \
             documentation. If one is consumed by compose/Helm rather than by \
             the server, add it to NOT_THE_BINARYS with the reason."
        );
    }

    #[test]
    fn compose_forwards_every_variable_the_env_example_documents() {
        let documented = documented_in_env_example();
        let forwarded = env_keys_in(&read_repo("deploy/docker-compose.yaml"));

        let dropped: Vec<_> = documented
            .iter()
            .filter(|v| !forwarded.contains(*v) && !NOT_THE_BINARYS.contains(&v.as_str()))
            .cloned()
            .collect();

        assert!(
            dropped.is_empty(),
            "deploy/.env.example tells the operator to set these and \
             docker-compose.yaml does not forward them, so the server never \
             sees them: {dropped:?}\n\
             This is the KAIROS-T-0177 defect. It is silent, because these \
             variables are optional and unset is indistinguishable from \
             ignored."
        );
    }

    #[test]
    fn the_variables_google_workspace_requires_reach_the_container() {
        // Named separately from the set-diff tests above because these two are
        // the ones that actually broke a real deployment, and a future
        // refactor of the generic checks should not be able to lose them.
        let compose = read_repo("deploy/docker-compose.yaml");
        let forwarded = env_keys_in(&compose);
        for var in ["KAIROS_API_BEARER", "KAIROS_WEB_CLIENT_SECRET"] {
            assert!(
                forwarded.contains(var),
                "{var} is required for Google Workspace and is not forwarded by \
                 the compose stack"
            );
        }
    }
}
