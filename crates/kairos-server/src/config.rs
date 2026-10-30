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

/// `KAIROS_ROLE` (KAIROS-T-0394, KAIROS-A-0027): the work of this server.
///
/// A build of the code index can run out of memory (GitHub #34). With the
/// builds in a server of their own, such a build ends that server only, and
/// the API keeps serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerRole {
    /// The API and each job, the code index builds too (the default, for a
    /// small install).
    #[default]
    All,
    /// The API and each job except the code index builds and the
    /// new-commits sweep.
    Api,
    /// Only the code index builds and the new-commits sweep. Its HTTP
    /// listener has `/healthz`, `/readyz`, `/version` and `/metrics`, and no
    /// API.
    Worker,
}

impl ServerRole {
    /// The value of `KAIROS_ROLE`.
    pub fn as_str(self) -> &'static str {
        match self {
            ServerRole::All => "all",
            ServerRole::Api => "api",
            ServerRole::Worker => "worker",
        }
    }

    /// True: this server serves the API, the MCP endpoint and the GUI.
    pub fn serves_api(self) -> bool {
        self != ServerRole::Worker
    }

    /// True: this server runs the code index builds and the new-commits
    /// sweep.
    pub fn builds(self) -> bool {
        self != ServerRole::Api
    }

    /// True: this server takes the jobs of `kind`. A build kind goes to a
    /// server that builds; the periodic passes go to each server, and each
    /// server takes only the runs of its passes ([`Self::runs_pass`],
    /// KAIROS-T-0466); each other kind
    /// goes to a server that serves the API.
    pub fn takes(self, kind: &str) -> bool {
        use crate::jobs::kinds;
        if kind == kinds::PERIODIC {
            return true;
        }
        if kind == kinds::EMBEDDED_BUILD || kind == kinds::HOSTED_BUILD {
            return self.builds();
        }
        self.serves_api()
    }

    /// True: this server runs the periodic pass `pass`. The new-commits
    /// sweep runs where the builds run; each other pass runs where the API
    /// runs.
    pub fn runs_pass(self, pass: &str) -> bool {
        if pass == crate::periodic::CODE_INDEX_SWEEP {
            self.builds()
        } else {
            self.serves_api()
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
    /// `KAIROS_POOL_SIZE` — the connections of each of the two pools of a
    /// server, the async one of the middleware and the blocking one of the
    /// handlers (default 16; KAIROS-T-0608). A server holds up to twice
    /// this many connections, plus `KAIROS_JOB_POOL_SIZE` for the jobs,
    /// one for LISTEN and one for the event emitter (KAIROS-T-0632).
    pub pool_size: u32,
    /// `KAIROS_POOL_MIN_IDLE` — the connections the blocking pool opens at
    /// start and keeps idle (default 2), so the first requests after a
    /// start do not pay the connect. Capped at `KAIROS_POOL_SIZE`.
    pub pool_min_idle: u32,
    /// `KAIROS_JOB_POOL_SIZE` — the connections of the blocking pool of the
    /// job worker and the jobs (default 6; KAIROS-T-0614). A pool of its
    /// own, so a burst of jobs leaves the handlers their connections.
    pub job_pool_size: u32,
    /// `KAIROS_BIND_ADDR` — where axum listens (default `127.0.0.1:8080`).
    pub bind_addr: SocketAddr,
    /// `OIDC_ISSUER_URL` — the issuer; discovery + JWKS derive from it.
    ///
    /// `None` means this deployment has **no identity provider**
    /// (KAIROS-T-0208), which is valid only with [`Self::local_auth`] on. An
    /// `Option` rather than an empty string, because "the operator set it to an
    /// empty value" and "there is no issuer" must not be the same condition —
    /// the second is a supported deployment and the first is a typo.
    pub oidc_issuer_url: Option<String>,
    /// `OIDC_AUDIENCE` — the `aud` claim tokens must carry (A-0010; for the
    /// Dex dev stack this is the OAuth client id, e.g. `kairos-cli`).
    /// Accepts a comma-separated allow-list (KAIROS-T-0055) for IdPs that
    /// mint a distinct `aud` per client (Google Workspace): a token
    /// matching ANY listed audience validates. Parsed and enforced
    /// non-empty at `Authenticator::discover` (startup). `None` when there is no
    /// issuer (KAIROS-T-0208).
    pub oidc_audience: Option<String>,
    /// `KAIROS_BASE_DOMAIN` — enables Host-subdomain tenant resolution
    /// (`acme.<base>` → tenant `acme`, A-0005 §2).
    pub base_domain: Option<String>,
    /// `KAIROS_SINGLE_TENANT` — fixed tenant slug; skips subdomain/header
    /// resolution entirely (A-0013 single-tenant mode).
    pub single_tenant: Option<String>,
    /// `KAIROS_SINGLE_TENANT_NAME` — the display name the pinned tenant gets
    /// when the first deployment admin to log in provisions it (the slug when
    /// unset).
    pub single_tenant_name: Option<String>,
    /// `KAIROS_AUTO_JOIN_DOMAINS` — comma-separated email domains, lowercase,
    /// with no `@`. A person whose issuer verifies an email at one of them
    /// becomes a `member` of the single tenant on their first request, with no
    /// admin step. Requires `KAIROS_SINGLE_TENANT`. Empty/unset → nobody joins
    /// on their own.
    pub auto_join_domains: Vec<String>,
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
    /// KAIROS-T-0636: a write marks its item, and each run reads the marked
    /// items only, oldest first. The content hash still decides what goes to
    /// the model. Each organization has one job row of the job queue
    /// (KAIROS-T-0375), which queues itself again after each run: this
    /// interval after a run that left no mark, at once after a full page.
    pub embed_refresh_secs: u64,
    /// `KAIROS_CODE_INDEX_DIR` — the folder of the base code indexes
    /// (COLLIERY-T-1853): a bare clone of each indexed repository, and the
    /// work folders of the builder. Unset: no clone, so no nearest-commit
    /// answer and no builder. The indexes themselves are in Postgres.
    pub code_index_dir: Option<std::path::PathBuf>,
    /// `KAIROS_ROLE` (KAIROS-T-0394): `all` (the default), `api` or
    /// `worker`. See [`ServerRole`].
    pub role: ServerRole,
    /// `KAIROS_CODE_INDEX_POLL_SECS` — how often the builder fetches each
    /// indexed repository and updates the index of a new commit of its
    /// default branch (COLLIERY-T-1853). Default 300; `0` turns the builder
    /// off.
    pub code_index_poll_secs: u64,
    /// `KAIROS_CODE_INDEX_THREADS` — the CPU threads of the summary model of
    /// the builder (COLLIERY-T-2529). Default 4, at least 1. The builder
    /// shares the host with the live services.
    pub code_index_threads: u32,
    /// `KAIROS_CODE_INDEX_MIN_REBUILD_SECS` — the least time between 2
    /// updates of one repository by the update lane (KAIROS-T-0334). Default
    /// 0: off. On a slow host, a repository that gets many pushes then waits
    /// while the other repositories are stale.
    pub code_index_min_rebuild_secs: u64,
    /// `KAIROS_CODE_INDEX_DEPS_CAP_MB` — the cap of the total size of the
    /// dependency caches of the SCIP runners of Go, TypeScript and Python
    /// (KAIROS-T-0474). Default 10240 (10 GiB).
    pub code_index_deps_cap_mb: u64,
    /// `KAIROS_CODE_INDEX_MIN_FREE_MB` — under this free disk space, the
    /// builder removes the dependency caches, oldest first, before an
    /// install; if the space is still low, the language gets the edges by
    /// name with a warning. Default 5120 (5 GiB).
    pub code_index_min_free_mb: u64,
    /// `KAIROS_CODE_INDEX_DEPS_ENV` — the names of more variables of the
    /// environment that a dependency step gets, separated by commas
    /// (KAIROS-T-0474). The step gets no other variable of the server.
    pub code_index_deps_env: Vec<String>,
    /// `KAIROS_CODE_INDEX_TOOL_DOWNLOADS` — `true` or `1`: uv fetches the
    /// CPython that a Python project asks for, when the Python of the runner
    /// does not fit it (KAIROS-T-0668). Default off: the server downloads no
    /// Python of its own choice.
    pub code_index_tool_downloads: bool,
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
    /// `KAIROS_SECRETS_KEY` (COLLIERY-T-3105): 32 bytes in base64, the
    /// AES-256-GCM key of the secrets that Kairos keeps in the database (the
    /// read token of a repository, [`crate::secrets`]). Absent: a write of a
    /// token is refused with a text that names the setting, and the builder
    /// fetches each repository with no credential. A value that is not 32
    /// bytes of base64 stops the start.
    pub secrets_key: Option<crate::secrets::SecretsKey>,
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
    /// `KAIROS_AUTH_CACHE_SECS` (KAIROS-T-0609): how long the auth cache
    /// keeps a key, a session, a user, an org and a role, default **60**.
    /// A write that makes an entry wrong drops it on the server that made
    /// the write; another server keeps it for at most this time. `0`
    /// turns the cache off.
    pub auth_cache_secs: u64,
    /// `KAIROS_REQUEST_TIMEOUT_SECS` (KAIROS-T-0613): the most time that a
    /// request under `/api` or `/mcp` gets before a 503 `REQUEST_TIMEOUT`,
    /// default **30**. The uploads of a code index and a site are not in it
    /// (see [`crate::request_timeout`]). `0` turns the timeout off.
    pub request_timeout_secs: u64,
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
    /// `KAIROS_LOCAL_AUTH` (KAIROS-T-0203): accept local password accounts,
    /// default **false**.
    ///
    /// Off means `/api/login` is **not routed at all** — not a 401, not a 404 from
    /// a handler that refuses, simply absent. A deployment that authenticates
    /// through an issuer has no password endpoint to attack, which is a stronger
    /// statement than one that exists and declines.
    ///
    /// It is additive rather than alternative (KAIROS-I-0018): a deployment may
    /// have both an issuer and local accounts, and neither path knows about the
    /// other.
    pub local_auth: bool,
    /// `KAIROS_SESSION_TTL_SECS` (KAIROS-T-0203): how long a session bearer lasts,
    /// default **1209600** (14 days).
    ///
    /// A decision, not an accident. A session is a bearer token a browser keeps and
    /// the CLI may write to disk, so "forever" is a permanent credential handed out
    /// by a login form; an hour is hostile, because it logs people out in the middle
    /// of the work they came to do. Two weeks is about as long as someone keeps a
    /// tab open, and short enough that a token copied off a laptop stops working
    /// without anyone noticing it needed to.
    pub session_ttl_secs: u64,
    /// `KAIROS_API_KEY_TTL_DAYS` (KAIROS-T-0370): the default life of a new
    /// API key or agent key, in days, default **90**. A request that names
    /// `expires_at` sets its own expiry; a request with `no_expiry: true`
    /// makes a key with no expiry. `0` turns the default off: a new key has
    /// no expiry unless the request names one.
    pub api_key_ttl_days: u32,
    /// `KAIROS_STALE_CLAIM_DAYS` (KAIROS-T-0531): a claim on a task with no
    /// write and no state change for this many days is stale, default
    /// **14**. The periodic pass `stale-claims` tells the holder and the
    /// owner of the initiative of the task, once for each quiet period. The
    /// claim stays. `0` turns the pass off. The value applies to each
    /// tenant of the deployment.
    pub stale_claim_days: u32,
    /// `KAIROS_NOTICE_LIMIT_PER_HOUR` (KAIROS-T-0737): the notices that one
    /// agent key (or one person with no key) can send in one hour, default
    /// **20** ([`kairos_core::notifications::NOTICES_PER_HOUR`]). A notice
    /// to a team counts as one. `0` turns the limit off. The value applies
    /// to each tenant of the deployment.
    pub notice_limit_per_hour: u32,
    /// `KAIROS_API_KEY_ROTATION_GRACE_SECS` (KAIROS-T-0370): after a
    /// rotation, the old secret of a key stays valid for this time, default
    /// **86400** (1 day), so that the agents can change to the new secret.
    /// `0` ends the old secret at the rotation.
    pub api_key_rotation_grace_secs: u64,
    /// `KAIROS_JOB_*` (KAIROS-T-0373): the heartbeat, the lease, the tries
    /// and the limits per kind of the job queue ([`crate::jobs`]).
    pub jobs: crate::jobs::JobSettings,
    /// `KAIROS_WEBHOOK_*` (KAIROS-T-0384): the tries, the waits, the time
    /// out, the failures in a row that turn a target off, and the guard of
    /// the addresses of the signed webhook deliveries
    /// ([`crate::webhooks`]).
    pub webhooks: crate::webhooks::WebhookSettings,
    /// `KAIROS_GITHUB_API_URL` (KAIROS-T-0386): the REST API of GitHub for
    /// the write-back ([`crate::forge::writeback`]).
    pub writeback: crate::forge::writeback::WriteBackSettings,
    /// `KAIROS_CLI_DOWNLOAD_URL` (KAIROS-T-0432): where the release
    /// tarballs of the CLI are, for `GET /version`. `None` is the GitHub
    /// releases of Kairos. A mirror has the same layout:
    /// `<url>/download/v<version>/kairos-<version>-<target>.tar.gz`.
    pub cli_download_url: Option<String>,
    /// `KAIROS_PAGES_*` (KAIROS-T-0405): the store of the uploaded
    /// repository sites ([`crate::site_store`]).
    pub pages: crate::site_store::PagesSettings,
    /// `KAIROS_BOOTSTRAP_ADMIN` (KAIROS-T-0204): the email of a first-boot admin,
    /// created **only when the deployment has no users at all**.
    ///
    /// A fresh local-auth deployment cannot be logged into, so something has to make
    /// the first account. The obvious shape has a trap that is worth designing around
    /// rather than discovering: an environment variable in a Helm values file is a
    /// permanent credential, in the deployment manifest and in the release history.
    ///
    /// So it is SINGLE-USE. It is consumed on a boot that finds `public.users` empty
    /// and is inert on every boot after, and both outcomes are logged. A bootstrap
    /// that silently kept working would be a backdoor with a documented name.
    pub bootstrap_admin: Option<String>,
    /// `KAIROS_BOOTSTRAP_PASSWORD` — the first-boot admin's password.
    ///
    /// Prefer [`Self::bootstrap_password_hash`]: a plaintext password here is one
    /// `kubectl get configmap` away from anybody with read access to the namespace.
    pub bootstrap_password: Option<String>,
    /// `KAIROS_BOOTSTRAP_PASSWORD_HASH` — the first-boot admin's password as a PHC
    /// string, so the plaintext never enters the manifest.
    ///
    /// Produce one with `kairos-server hash-password`. Wins over
    /// [`Self::bootstrap_password`] when both are set.
    pub bootstrap_password_hash: Option<String>,
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
        // KAIROS-T-0203, read here rather than further down because the OIDC
        // requirement below depends on it.
        let local_auth = match get("KAIROS_LOCAL_AUTH").as_deref() {
            None | Some("") | Some("false") | Some("0") => false,
            Some("true") | Some("1") => true,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_LOCAL_AUTH",
                    message: format!("{other:?} is not one of: true, 1, false, 0"),
                });
            }
        };

        // KAIROS-T-0208: an issuer is required UNLESS local auth is on, because
        // then there is another way for a person to log in.
        //
        // The condition is "local auth is on", NOT "the variable is empty". A
        // deployment that means to use OIDC and typo'd its issuer must still fail
        // at boot with the message it has always had — coming up with no way in at
        // all would be a far worse failure than refusing to start.
        let (oidc_issuer_url, oidc_audience) = if local_auth {
            (get("OIDC_ISSUER_URL"), get("OIDC_AUDIENCE"))
        } else {
            (
                Some(required(
                    "OIDC_ISSUER_URL",
                    "it is required to validate bearer tokens \
                     (e.g. http://localhost:41558/dex for the dev stack), unless \
                     KAIROS_LOCAL_AUTH is on",
                )?),
                Some(required(
                    "OIDC_AUDIENCE",
                    "it is required to validate the `aud` claim of bearer tokens \
                     (the OAuth client id, e.g. kairos-cli for the dev stack), \
                     unless KAIROS_LOCAL_AUTH is on",
                )?),
            )
        };
        // Half an issuer is nobody's intention. Naming one without the other means
        // every token will be refused for a reason that points at the token.
        match (&oidc_issuer_url, &oidc_audience) {
            (Some(_), None) => {
                return Err(ConfigError::Missing {
                    var: "OIDC_AUDIENCE",
                    hint: "OIDC_ISSUER_URL names an issuer, so the audience its \
                           tokens carry is required too",
                });
            }
            (None, Some(_)) => {
                return Err(ConfigError::Missing {
                    var: "OIDC_ISSUER_URL",
                    hint: "OIDC_AUDIENCE is set but no issuer is; set the issuer, \
                           or unset the audience to run on local accounts alone",
                });
            }
            _ => {}
        }

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

        let role = match get("KAIROS_ROLE").as_deref().map(str::trim) {
            None | Some("all") => ServerRole::All,
            Some("api") => ServerRole::Api,
            Some("worker") => ServerRole::Worker,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    var: "KAIROS_ROLE",
                    message: format!("{other:?} is not one of: all, api, worker"),
                });
            }
        };

        // KAIROS-T-0466: a worker with no folder would start, report ready
        // and build nothing.
        let code_index_dir = get("KAIROS_CODE_INDEX_DIR").map(std::path::PathBuf::from);
        if role == ServerRole::Worker && code_index_dir.is_none() {
            return Err(ConfigError::Invalid {
                var: "KAIROS_CODE_INDEX_DIR",
                message: "a server with KAIROS_ROLE=worker needs KAIROS_CODE_INDEX_DIR, the \
                          folder of its clones and tools"
                    .to_string(),
            });
        }

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
        let auth_cache_secs = parse_num::<u64>(&get, "KAIROS_AUTH_CACHE_SECS", 60)?;
        let request_timeout_secs = parse_num::<u64>(&get, "KAIROS_REQUEST_TIMEOUT_SECS", 30)?;
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

        let session_ttl_secs =
            parse_num::<u64>(&get, "KAIROS_SESSION_TTL_SECS", 14 * 24 * 60 * 60)?;
        if local_auth && session_ttl_secs == 0 {
            return Err(ConfigError::Invalid {
                var: "KAIROS_SESSION_TTL_SECS",
                message: "0 would mint sessions that have already expired; \
                          to switch local login off set KAIROS_LOCAL_AUTH=false"
                    .to_string(),
            });
        }

        // KAIROS-T-0204.
        let bootstrap_admin = get("KAIROS_BOOTSTRAP_ADMIN").map(|e| e.trim().to_lowercase());
        let bootstrap_password = get("KAIROS_BOOTSTRAP_PASSWORD");
        let bootstrap_password_hash = get("KAIROS_BOOTSTRAP_PASSWORD_HASH");
        if bootstrap_admin.is_some()
            && bootstrap_password.is_none()
            && bootstrap_password_hash.is_none()
        {
            return Err(ConfigError::Missing {
                var: "KAIROS_BOOTSTRAP_PASSWORD",
                hint: "KAIROS_BOOTSTRAP_ADMIN names an account with no way to \
                       authenticate; set KAIROS_BOOTSTRAP_PASSWORD, or better \
                       KAIROS_BOOTSTRAP_PASSWORD_HASH (from `kairos-server \
                       hash-password`) so the plaintext is never in the manifest",
            });
        }
        if bootstrap_admin.is_none()
            && (bootstrap_password.is_some() || bootstrap_password_hash.is_some())
        {
            // Refused rather than ignored: a password set with no account to put it
            // on means the operator believes they have bootstrapped an admin, and
            // they will find out otherwise at the login screen.
            return Err(ConfigError::Missing {
                var: "KAIROS_BOOTSTRAP_ADMIN",
                hint: "a bootstrap password is set but no bootstrap admin email is; \
                       set the email, or remove the password",
            });
        }

        let mut deployment_admins: Vec<String> = get("KAIROS_DEPLOYMENT_ADMINS")
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        // KAIROS-T-0204: the first-boot admin is a deployment admin, automatically.
        //
        // Not a convenience. A fresh deployment has no ORGANIZATION either, and
        // creating one is a deployment-admin action, so a bootstrap admin without
        // this can log in and do precisely nothing — which reads as a broken install
        // rather than as a missing variable. The alternative was making the operator
        // also write `local:their@email` into KAIROS_DEPLOYMENT_ADMINS, which is a
        // detail about our synthetic `external_id` scheme that nobody should have to
        // know to get started.
        //
        // Added rather than replacing, so an OIDC deployment that adds a break-glass
        // local admin keeps the admins it already had.
        if let Some(email) = &bootstrap_admin {
            let sub = format!("local:{email}");
            if !deployment_admins.contains(&sub) {
                deployment_admins.push(sub);
            }
        }

        let single_tenant = get("KAIROS_SINGLE_TENANT");
        let auto_join_domains = parse_auto_join_domains(get("KAIROS_AUTO_JOIN_DOMAINS"))?;
        if !auto_join_domains.is_empty() && single_tenant.is_none() {
            // Refused rather than applied to every tenant: on a multi-tenant
            // deployment a domain does not say which organization a person
            // belongs to, and joining all of them would hand every tenant's
            // boards to everyone at that domain.
            return Err(ConfigError::Invalid {
                var: "KAIROS_AUTO_JOIN_DOMAINS",
                message: "applies to the single tenant only; set KAIROS_SINGLE_TENANT, \
                          or remove KAIROS_AUTO_JOIN_DOMAINS"
                    .into(),
            });
        }

        let code_index_threads: u32 = parse_num(&get, "KAIROS_CODE_INDEX_THREADS", 4)?;
        if code_index_threads == 0 {
            return Err(ConfigError::Invalid {
                var: "KAIROS_CODE_INDEX_THREADS",
                message: "must be at least 1".into(),
            });
        }

        // KAIROS-T-0614: a pool of no connections would stop every job.
        let job_pool_size: u32 = parse_num(&get, "KAIROS_JOB_POOL_SIZE", 6)?;
        if job_pool_size == 0 {
            return Err(ConfigError::Invalid {
                var: "KAIROS_JOB_POOL_SIZE",
                message: "must be at least 1".into(),
            });
        }

        let secrets_key = get(crate::secrets::SECRETS_KEY_VAR)
            .map(|raw| {
                crate::secrets::SecretsKey::from_base64(&raw).map_err(|message| {
                    ConfigError::Invalid {
                        var: crate::secrets::SECRETS_KEY_VAR,
                        message,
                    }
                })
            })
            .transpose()?;
        // KAIROS-T-0370: the old keys decrypt during a rotation.
        let secrets_key = match (secrets_key, get(crate::secrets::SECRETS_KEY_OLD_VAR)) {
            (key, None) => key,
            (Some(key), Some(raw)) => {
                Some(
                    key.with_old_base64(&raw)
                        .map_err(|message| ConfigError::Invalid {
                            var: crate::secrets::SECRETS_KEY_OLD_VAR,
                            message,
                        })?,
                )
            }
            (None, Some(_)) => {
                return Err(ConfigError::Invalid {
                    var: crate::secrets::SECRETS_KEY_OLD_VAR,
                    message: "is set, and KAIROS_SECRETS_KEY is not. Set the new key in \
                              KAIROS_SECRETS_KEY and the old keys in KAIROS_SECRETS_KEY_OLD"
                        .into(),
                });
            }
        };

        Ok(Self {
            database_url,
            pool_size: parse_num(&get, "KAIROS_POOL_SIZE", 16)?,
            pool_min_idle: parse_num(&get, "KAIROS_POOL_MIN_IDLE", 2)?,
            job_pool_size,
            bind_addr,
            oidc_issuer_url: oidc_issuer_url.map(|url| url.trim_end_matches('/').to_string()),
            oidc_audience,
            base_domain: get("KAIROS_BASE_DOMAIN"),
            single_tenant,
            single_tenant_name: get("KAIROS_SINGLE_TENANT_NAME"),
            auto_join_domains,
            deployment_admins,
            log_level: get("KAIROS_LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            log_format,
            dev_ui,
            web_dist: get("KAIROS_WEB_DIST").map(std::path::PathBuf::from),
            embed_refresh_secs: get("KAIROS_EMBED_REFRESH_SECS")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(10),
            code_index_dir,
            role,
            code_index_poll_secs: parse_num(&get, "KAIROS_CODE_INDEX_POLL_SECS", 300)?,
            code_index_threads,
            code_index_min_rebuild_secs: parse_num(&get, "KAIROS_CODE_INDEX_MIN_REBUILD_SECS", 0)?,
            code_index_deps_cap_mb: parse_num(&get, "KAIROS_CODE_INDEX_DEPS_CAP_MB", 10240)?,
            code_index_min_free_mb: parse_num(&get, "KAIROS_CODE_INDEX_MIN_FREE_MB", 5120)?,
            code_index_deps_env: get("KAIROS_CODE_INDEX_DEPS_ENV")
                .map(|v| {
                    v.split(',')
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            code_index_tool_downloads: matches!(
                get("KAIROS_CODE_INDEX_TOOL_DOWNLOADS").as_deref(),
                Some("true" | "1")
            ),
            web_client_id: get("KAIROS_WEB_CLIENT_ID").unwrap_or_else(|| "kairos-web".to_string()),
            api_bearer,
            web_client_secret: get("KAIROS_WEB_CLIENT_SECRET"),
            public_url: get("KAIROS_PUBLIC_URL").map(|url| url.trim_end_matches('/').to_string()),
            webhook_signing_key: get("KAIROS_WEBHOOK_SIGNING_KEY"),
            secrets_key,
            otel_endpoint: get("KAIROS_OTEL_ENDPOINT"),
            otel_sample_ratio,
            auth_max_failures,
            auth_failure_window_secs,
            auth_lockout_secs,
            auth_cache_secs,
            request_timeout_secs,
            trusted_proxy,
            local_auth,
            session_ttl_secs,
            api_key_ttl_days: parse_num(&get, "KAIROS_API_KEY_TTL_DAYS", 90)?,
            stale_claim_days: parse_num(&get, "KAIROS_STALE_CLAIM_DAYS", 14)?,
            notice_limit_per_hour: parse_num(
                &get,
                "KAIROS_NOTICE_LIMIT_PER_HOUR",
                kairos_core::notifications::NOTICES_PER_HOUR,
            )?,
            api_key_rotation_grace_secs: parse_num(
                &get,
                "KAIROS_API_KEY_ROTATION_GRACE_SECS",
                24 * 60 * 60,
            )?,
            jobs: parse_jobs(&get)?,
            webhooks: parse_webhooks(&get)?,
            writeback: crate::forge::writeback::WriteBackSettings {
                api_url: get("KAIROS_GITHUB_API_URL")
                    .map(|url| url.trim().trim_end_matches('/').to_string())
                    .unwrap_or_else(|| {
                        crate::forge::writeback::WriteBackSettings::default().api_url
                    }),
                ..Default::default()
            },
            cli_download_url: get("KAIROS_CLI_DOWNLOAD_URL")
                .map(|url| url.trim().trim_end_matches('/').to_string()),
            pages: crate::site_store::parse_settings(&get)?,
            bootstrap_admin,
            bootstrap_password,
            bootstrap_password_hash,
        })
    }
}

/// The settings of the job queue (KAIROS-T-0373): `KAIROS_JOB_HEARTBEAT_SECS`
/// (30), `KAIROS_JOB_LEASE_SECS` (300), `KAIROS_JOB_MAX_TRIES` (3) and a
/// `KAIROS_JOB_LIMIT_<KIND>` for each kind. The heartbeat must be at least 1
/// second, and the lease at least 2 heartbeats, so that a job that makes
/// progress keeps its lease.
fn parse_jobs(
    get: &impl Fn(&str) -> Option<String>,
) -> Result<crate::jobs::JobSettings, ConfigError> {
    use crate::jobs::kinds;
    let mut settings = crate::jobs::JobSettings::default();
    let heartbeat: u64 = parse_num(get, "KAIROS_JOB_HEARTBEAT_SECS", 30)?;
    let lease: u64 = parse_num(get, "KAIROS_JOB_LEASE_SECS", 300)?;
    let max_tries: i32 = parse_num(get, "KAIROS_JOB_MAX_TRIES", 3)?;
    if heartbeat == 0 {
        return Err(ConfigError::Invalid {
            var: "KAIROS_JOB_HEARTBEAT_SECS",
            message: "must be 1 or more".into(),
        });
    }
    if lease < 2 * heartbeat {
        return Err(ConfigError::Invalid {
            var: "KAIROS_JOB_LEASE_SECS",
            message: format!(
                "{lease} is less than 2 heartbeats ({} s): a job that makes progress would \
                 lose its lease",
                2 * heartbeat
            ),
        });
    }
    if max_tries < 1 {
        return Err(ConfigError::Invalid {
            var: "KAIROS_JOB_MAX_TRIES",
            message: "must be 1 or more".into(),
        });
    }
    settings.heartbeat = std::time::Duration::from_secs(heartbeat);
    settings.lease = std::time::Duration::from_secs(lease);
    settings.max_tries = max_tries;
    for (var, kind) in [
        ("KAIROS_JOB_LIMIT_EMBEDDED_BUILD", kinds::EMBEDDED_BUILD),
        ("KAIROS_JOB_LIMIT_HOSTED_BUILD", kinds::HOSTED_BUILD),
        ("KAIROS_JOB_LIMIT_EMBEDDING_BATCH", kinds::EMBEDDING_BATCH),
        ("KAIROS_JOB_LIMIT_PERIODIC", kinds::PERIODIC),
        ("KAIROS_JOB_LIMIT_WEBHOOK_DELIVERY", kinds::WEBHOOK_DELIVERY),
        ("KAIROS_JOB_LIMIT_GITHUB_WRITEBACK", kinds::GITHUB_WRITEBACK),
        ("KAIROS_JOB_LIMIT_ISSUE_IMPORT", kinds::ISSUE_IMPORT),
        ("KAIROS_JOB_LIMIT_ISSUE_WRITEBACK", kinds::ISSUE_WRITEBACK),
    ] {
        let default = settings.limit(kind);
        let limit: usize = parse_num(get, var, default)?;
        settings.limits.insert(kind.to_string(), limit);
    }
    Ok(settings)
}

/// The settings of the webhook deliveries (KAIROS-T-0384):
/// `KAIROS_WEBHOOK_MAX_TRIES` (12), `KAIROS_WEBHOOK_RETRY_BASE_SECS` (30),
/// `KAIROS_WEBHOOK_TIMEOUT_SECS` (10), `KAIROS_WEBHOOK_DISABLE_AFTER` (24)
/// and `KAIROS_WEBHOOK_ALLOW_PRIVATE` (false).
fn parse_webhooks(
    get: &impl Fn(&str) -> Option<String>,
) -> Result<crate::webhooks::WebhookSettings, ConfigError> {
    let defaults = crate::webhooks::WebhookSettings::default();
    let max_tries: i32 = parse_num(get, "KAIROS_WEBHOOK_MAX_TRIES", defaults.max_tries)?;
    if max_tries < 1 {
        return Err(ConfigError::Invalid {
            var: "KAIROS_WEBHOOK_MAX_TRIES",
            message: "must be 1 or more".into(),
        });
    }
    let timeout: u64 = parse_num(
        get,
        "KAIROS_WEBHOOK_TIMEOUT_SECS",
        defaults.timeout.as_secs(),
    )?;
    if timeout == 0 {
        return Err(ConfigError::Invalid {
            var: "KAIROS_WEBHOOK_TIMEOUT_SECS",
            message: "must be 1 or more".into(),
        });
    }
    let allow_private = match get("KAIROS_WEBHOOK_ALLOW_PRIVATE")
        .as_deref()
        .map(str::trim)
    {
        None => defaults.allow_private,
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        Some(other) => {
            return Err(ConfigError::Invalid {
                var: "KAIROS_WEBHOOK_ALLOW_PRIVATE",
                message: format!("{other:?} is not true or false"),
            });
        }
    };
    Ok(crate::webhooks::WebhookSettings {
        max_tries,
        retry_base: std::time::Duration::from_secs(parse_num(
            get,
            "KAIROS_WEBHOOK_RETRY_BASE_SECS",
            defaults.retry_base.as_secs(),
        )?),
        timeout: std::time::Duration::from_secs(timeout),
        disable_after: parse_num(get, "KAIROS_WEBHOOK_DISABLE_AFTER", defaults.disable_after)?,
        allow_private,
        ..defaults
    })
}

/// `KAIROS_AUTO_JOIN_DOMAINS`: comma-separated, trimmed, lowercased, a
/// leading `@` dropped. A value with an `@` inside it, or with no dot, is
/// refused: it is an address or a typo, and matching it would be a guess.
fn parse_auto_join_domains(raw: Option<String>) -> Result<Vec<String>, ConfigError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut domains = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let domain = entry
            .strip_prefix('@')
            .unwrap_or(entry)
            .to_ascii_lowercase();
        if domain.contains('@') || !domain.contains('.') || domain.starts_with('.') {
            return Err(ConfigError::Invalid {
                var: "KAIROS_AUTO_JOIN_DOMAINS",
                message: format!("{entry:?} is not an email domain such as example.com"),
            });
        }
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }
    Ok(domains)
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
        assert_eq!(
            config.oidc_issuer_url.as_deref(),
            Some("http://localhost:5558/dex")
        );
        // Local auth is off unless asked for (KAIROS-T-0203).
        assert!(!config.local_auth);
        assert_eq!(config.session_ttl_secs, 14 * 24 * 60 * 60);
        // KAIROS-T-0370: a new key expires after 90 days; a rotation keeps
        // the old secret for 1 day.
        assert_eq!(config.api_key_ttl_days, 90);
        assert_eq!(config.api_key_rotation_grace_secs, 24 * 60 * 60);
        // KAIROS-T-0531: a claim is stale after 14 days with no write.
        assert_eq!(config.stale_claim_days, 14);
        // KAIROS-T-0737: 20 notices in one hour for each key.
        assert_eq!(config.notice_limit_per_hour, 20);
    }

    #[test]
    fn the_notice_limit_parses_and_0_turns_it_off() {
        for (value, limit) in [("0", 0), ("5", 5)] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_NOTICE_LIMIT_PER_HOUR", value));
            let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
            assert_eq!(config.notice_limit_per_hour, limit, "{value}");
        }
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_NOTICE_LIMIT_PER_HOUR", "many"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(
            err.to_string().contains("KAIROS_NOTICE_LIMIT_PER_HOUR"),
            "{err}"
        );
    }

    #[test]
    fn the_stale_claim_days_parse_and_0_turns_the_pass_off() {
        for (value, days) in [("0", 0), ("30", 30)] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_STALE_CLAIM_DAYS", value));
            let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
            assert_eq!(config.stale_claim_days, days, "{value}");
        }
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_STALE_CLAIM_DAYS", "two weeks"));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().contains("KAIROS_STALE_CLAIM_DAYS"), "{err}");
    }

    #[test]
    fn the_pool_settings() {
        // KAIROS-T-0608, KAIROS-T-0614: the defaults, and a job pool of its
        // own size.
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.pool_size, 16);
        assert_eq!(config.pool_min_idle, 2);
        assert_eq!(config.job_pool_size, 6);

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_JOB_POOL_SIZE", "3"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.job_pool_size, 3);
        assert_eq!(
            config.pool_size, 16,
            "the pool of the handlers is not changed"
        );

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_JOB_POOL_SIZE", "six"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("not a number");
        assert!(
            err.to_string().contains("KAIROS_JOB_POOL_SIZE"),
            "the error names the setting: {err}"
        );

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_JOB_POOL_SIZE", "0"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("no connections");
        assert!(
            err.to_string().contains("KAIROS_JOB_POOL_SIZE"),
            "the error names the setting: {err}"
        );
    }

    #[test]
    fn the_request_timeout_setting() {
        // KAIROS-T-0613: 30 s by default, 0 turns it off, a typo is refused.
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.request_timeout_secs, 30);
        for (value, secs) in [("0", 0), ("120", 120)] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_REQUEST_TIMEOUT_SECS", value));
            let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
            assert_eq!(config.request_timeout_secs, secs, "{value}");
        }
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_REQUEST_TIMEOUT_SECS", "30s"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("not a number");
        assert!(
            err.to_string().contains("KAIROS_REQUEST_TIMEOUT_SECS"),
            "{err}"
        );
    }

    #[test]
    fn the_key_settings_and_the_old_secrets_keys() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_API_KEY_TTL_DAYS", "0"));
        vars.push(("KAIROS_API_KEY_ROTATION_GRACE_SECS", "600"));
        vars.push((
            "KAIROS_SECRETS_KEY",
            "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
        ));
        vars.push((
            "KAIROS_SECRETS_KEY_OLD",
            "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=",
        ));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.api_key_ttl_days, 0);
        assert_eq!(config.api_key_rotation_grace_secs, 600);
        let key = config.secrets_key.expect("a key");
        assert_eq!(
            key.old_ids(),
            vec![crate::secrets::SecretsKey::from_bytes([2u8; 32]).id()]
        );

        // Old keys with no new key: refused.
        let mut vars = MINIMAL.to_vec();
        vars.push((
            "KAIROS_SECRETS_KEY_OLD",
            "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=",
        ));
        let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
        assert!(err.to_string().contains("KAIROS_SECRETS_KEY_OLD"), "{err}");

        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_API_KEY_TTL_DAYS", "ninety"));
        assert!(AppConfig::from_lookup(lookup(&vars)).is_err());
    }

    // ---------------------------------------------------------------------
    // COLLIERY-T-3105: the key of the secrets in the database.

    #[test]
    fn kairos_secrets_key_is_optional_and_32_bytes_of_base64() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert!(config.secrets_key.is_none());

        let mut vars = MINIMAL.to_vec();
        vars.push((
            "KAIROS_SECRETS_KEY",
            "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
        ));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(
            config.secrets_key.expect("a key").id(),
            crate::secrets::SecretsKey::from_bytes([1u8; 32]).id()
        );

        for bad in ["AQEBAQ==", "not base64!"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_SECRETS_KEY", bad));
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(bad);
            assert!(err.to_string().contains("KAIROS_SECRETS_KEY"), "{err}");
            assert!(!err.to_string().contains(bad), "the value is not in {err}");
        }
    }

    // ---------------------------------------------------------------------
    // COLLIERY-T-2529: the threads of the code index builder.

    #[test]
    fn the_code_index_builder_uses_4_threads_by_default() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.code_index_threads, 4);
    }

    #[test]
    fn kairos_code_index_threads_sets_the_threads_of_the_builder() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_CODE_INDEX_THREADS", "8"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.code_index_threads, 8);
    }

    #[test]
    fn a_worker_needs_the_code_index_dir() {
        // KAIROS-T-0466: a worker with no folder builds nothing.
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_ROLE", "worker"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("refused");
        assert!(
            err.to_string().contains("KAIROS_CODE_INDEX_DIR")
                && err.to_string().contains("KAIROS_ROLE=worker"),
            "{err}"
        );
        vars.push(("KAIROS_CODE_INDEX_DIR", "/var/lib/kairos/code-index"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("a worker with a folder");
        assert_eq!(config.role, ServerRole::Worker);
        // The other roles start without it: an api server queues the builds
        // for a worker, and a server of each role builds nothing then.
        for role in ["api", "all"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_ROLE", role));
            AppConfig::from_lookup(lookup(&vars)).expect("no folder, no worker");
        }
    }

    #[test]
    fn kairos_code_index_threads_refuses_0_and_text() {
        for value in ["0", "four", "-2"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_CODE_INDEX_THREADS", value));
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(value);
            assert!(
                err.to_string().contains("KAIROS_CODE_INDEX_THREADS"),
                "{value}: {err}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // KAIROS-T-0334: the least time between 2 updates of one repository.

    #[test]
    fn the_min_rebuild_time_is_off_by_default_and_refuses_text() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.code_index_min_rebuild_secs, 0);
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_CODE_INDEX_MIN_REBUILD_SECS", "3600"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.code_index_min_rebuild_secs, 3600);
        for value in ["an hour", "-1"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_CODE_INDEX_MIN_REBUILD_SECS", value));
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(value);
            assert!(
                err.to_string()
                    .contains("KAIROS_CODE_INDEX_MIN_REBUILD_SECS"),
                "{value}: {err}"
            );
        }
    }

    // KAIROS-T-0474: the dependency cache of the SCIP runners.
    #[test]
    fn the_dependency_cache_settings_have_defaults() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.code_index_deps_cap_mb, 10240);
        assert_eq!(config.code_index_min_free_mb, 5120);
        assert!(config.code_index_deps_env.is_empty());
        assert!(!config.code_index_tool_downloads);
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_CODE_INDEX_DEPS_CAP_MB", "512"));
        vars.push(("KAIROS_CODE_INDEX_MIN_FREE_MB", "100"));
        vars.push(("KAIROS_CODE_INDEX_DEPS_ENV", "CORP_TOKEN, ,GOFLAGS"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.code_index_deps_cap_mb, 512);
        assert_eq!(config.code_index_min_free_mb, 100);
        assert_eq!(config.code_index_deps_env, ["CORP_TOKEN", "GOFLAGS"]);
        vars.push(("KAIROS_CODE_INDEX_TOOL_DOWNLOADS", "true"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert!(config.code_index_tool_downloads);
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_CODE_INDEX_DEPS_CAP_MB", "ten"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("ten");
        assert!(
            err.to_string().contains("KAIROS_CODE_INDEX_DEPS_CAP_MB"),
            "{err}"
        );
    }

    #[test]
    fn the_job_settings_have_defaults_and_refuse_a_bad_value() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.jobs.heartbeat, std::time::Duration::from_secs(30));
        assert_eq!(config.jobs.lease, std::time::Duration::from_secs(300));
        assert_eq!(config.jobs.max_tries, 3);
        assert_eq!(config.jobs.limit("embedded_build"), 1);
        assert_eq!(config.jobs.limit("hosted_build"), 4);
        assert_eq!(config.jobs.limit("embedding_batch"), 2);
        assert_eq!(config.jobs.limit("periodic"), 1);

        let mut vars = MINIMAL.to_vec();
        vars.extend([
            ("KAIROS_JOB_HEARTBEAT_SECS", "10"),
            ("KAIROS_JOB_LEASE_SECS", "60"),
            ("KAIROS_JOB_MAX_TRIES", "5"),
            ("KAIROS_JOB_LIMIT_HOSTED_BUILD", "8"),
            ("KAIROS_JOB_LIMIT_EMBEDDED_BUILD", "0"),
        ]);
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.jobs.heartbeat, std::time::Duration::from_secs(10));
        assert_eq!(config.jobs.lease, std::time::Duration::from_secs(60));
        assert_eq!(config.jobs.max_tries, 5);
        assert_eq!(config.jobs.limit("hosted_build"), 8);
        assert_eq!(config.jobs.limit("embedded_build"), 0);

        for (var, value) in [
            ("KAIROS_JOB_HEARTBEAT_SECS", "0"),
            ("KAIROS_JOB_LEASE_SECS", "45"),
            ("KAIROS_JOB_MAX_TRIES", "0"),
            ("KAIROS_JOB_LIMIT_PERIODIC", "one"),
        ] {
            let mut vars = MINIMAL.to_vec();
            vars.push((var, value));
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(var);
            assert!(err.to_string().contains(var), "{var}={value}: {err}");
        }
    }

    /// KAIROS-T-0384: the settings of the webhook deliveries.
    #[test]
    fn the_webhook_settings_have_defaults_and_refuse_a_bad_value() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.webhooks, crate::webhooks::WebhookSettings::default());
        assert_eq!(config.webhooks.max_tries, 12);
        assert!(!config.webhooks.allow_private);
        assert_eq!(config.jobs.limit("webhook_delivery"), 4);
        assert_eq!(config.jobs.limit("github_writeback"), 2);
        assert_eq!(config.jobs.limit("issue_writeback"), 2);
        assert_eq!(config.writeback.api_url, "https://api.github.com");

        let mut vars = MINIMAL.to_vec();
        vars.extend([
            ("KAIROS_WEBHOOK_MAX_TRIES", "5"),
            ("KAIROS_WEBHOOK_RETRY_BASE_SECS", "2"),
            ("KAIROS_WEBHOOK_TIMEOUT_SECS", "3"),
            ("KAIROS_WEBHOOK_DISABLE_AFTER", "7"),
            ("KAIROS_WEBHOOK_ALLOW_PRIVATE", "true"),
            ("KAIROS_JOB_LIMIT_WEBHOOK_DELIVERY", "9"),
            (
                "KAIROS_GITHUB_API_URL",
                "https://github.example.com/api/v3/",
            ),
        ]);
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(config.webhooks.max_tries, 5);
        assert_eq!(
            config.webhooks.retry_base,
            std::time::Duration::from_secs(2)
        );
        assert_eq!(config.webhooks.timeout, std::time::Duration::from_secs(3));
        assert_eq!(config.webhooks.disable_after, 7);
        assert!(config.webhooks.allow_private);
        assert_eq!(config.jobs.limit("webhook_delivery"), 9);
        assert_eq!(
            config.writeback.api_url,
            "https://github.example.com/api/v3"
        );

        for (var, value) in [
            ("KAIROS_WEBHOOK_MAX_TRIES", "0"),
            ("KAIROS_WEBHOOK_TIMEOUT_SECS", "0"),
            ("KAIROS_WEBHOOK_ALLOW_PRIVATE", "yes"),
            ("KAIROS_WEBHOOK_DISABLE_AFTER", "many"),
        ] {
            let mut vars = MINIMAL.to_vec();
            vars.push((var, value));
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(var);
            assert!(err.to_string().contains(var), "{var}={value}: {err}");
        }
    }

    // ---------------------------------------------------------------------
    // KAIROS-T-0208: the issuer is optional ONLY when local auth is on.

    #[test]
    fn local_auth_makes_the_issuer_optional() {
        let config = AppConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x/y"),
            ("KAIROS_LOCAL_AUTH", "true"),
        ]))
        .expect("local auth alone is a valid deployment");
        assert_eq!(config.oidc_issuer_url, None);
        assert_eq!(config.oidc_audience, None);
        assert!(config.local_auth);
    }

    #[test]
    fn without_local_auth_the_issuer_is_still_required() {
        // The case that must not regress. A deployment that means to use OIDC and
        // typo'd its issuer has to fail HERE — coming up with no way to log in at
        // all would be a far worse failure than refusing to start.
        let err = AppConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://x/y")]))
            .expect_err("no issuer and no local auth is not a deployment");
        assert!(
            err.to_string().starts_with("OIDC_ISSUER_URL is not set"),
            "{err}"
        );
        // And the hint says how to opt out, so the operator is not left guessing.
        assert!(err.to_string().contains("KAIROS_LOCAL_AUTH"), "{err}");
    }

    #[test]
    fn half_an_issuer_is_refused() {
        // Naming one of the pair and not the other is nobody's intention, and the
        // symptom — every token refused — points at the token rather than at this.
        for (present, missing) in [
            ("OIDC_ISSUER_URL", "OIDC_AUDIENCE"),
            ("OIDC_AUDIENCE", "OIDC_ISSUER_URL"),
        ] {
            let err = AppConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://x/y"),
                ("KAIROS_LOCAL_AUTH", "true"),
                (present, "https://idp.example/x"),
            ]))
            .expect_err("half an issuer");
            assert!(
                err.to_string().starts_with(missing),
                "setting only {present} should name {missing}: {err}"
            );
        }
    }

    #[test]
    fn local_auth_and_an_issuer_together_are_valid() {
        // Additive, per the KAIROS-I-0018 decision: both paths at once.
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_LOCAL_AUTH", "true"));
        let config = AppConfig::from_lookup(lookup(&vars)).expect("both is valid");
        assert!(config.local_auth);
        assert!(config.oidc_issuer_url.is_some());
    }

    #[test]
    fn a_zero_session_lifetime_is_refused_when_local_auth_is_on() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_LOCAL_AUTH", "true"));
        vars.push(("KAIROS_SESSION_TTL_SECS", "0"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("0 mints dead sessions");
        assert!(err.to_string().contains("KAIROS_SESSION_TTL_SECS"), "{err}");
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
    fn the_role_is_all_by_default_and_an_unknown_role_is_refused() {
        // KAIROS-T-0394: no KAIROS_ROLE is the server of a small install.
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert_eq!(config.role, ServerRole::All);
        for (value, role) in [
            ("all", ServerRole::All),
            ("api", ServerRole::Api),
            ("worker", ServerRole::Worker),
        ] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_ROLE", value));
            // A worker needs its folder (KAIROS-T-0466).
            vars.push(("KAIROS_CODE_INDEX_DIR", "/var/lib/kairos/code-index"));
            let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
            assert_eq!(config.role, role);
            assert_eq!(config.role.as_str(), value);
        }
        for bad in ["API", "builder", "api,worker"] {
            let mut vars = MINIMAL.to_vec();
            vars.push(("KAIROS_ROLE", bad));
            let err = AppConfig::from_lookup(lookup(&vars)).unwrap_err();
            assert!(
                err.to_string().starts_with("KAIROS_ROLE is invalid")
                    && err.to_string().contains("all, api, worker"),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn each_role_takes_its_kinds_of_job_and_runs_its_passes() {
        use crate::jobs::kinds;
        let builds = [kinds::EMBEDDED_BUILD, kinds::HOSTED_BUILD];
        let api_work = [
            kinds::EMBEDDING_BATCH,
            kinds::WEBHOOK_DELIVERY,
            kinds::GITHUB_WRITEBACK,
            kinds::ISSUE_WRITEBACK,
            "site_cleanup",
        ];
        for kind in builds {
            assert!(ServerRole::All.takes(kind), "{kind}");
            assert!(!ServerRole::Api.takes(kind), "{kind}");
            assert!(ServerRole::Worker.takes(kind), "{kind}");
        }
        for kind in api_work {
            assert!(ServerRole::All.takes(kind), "{kind}");
            assert!(ServerRole::Api.takes(kind), "{kind}");
            assert!(!ServerRole::Worker.takes(kind), "{kind}");
        }
        for role in [ServerRole::All, ServerRole::Api, ServerRole::Worker] {
            assert!(role.takes(kinds::PERIODIC), "{role:?}");
        }
        let sweep = crate::periodic::CODE_INDEX_SWEEP;
        let retention = crate::periodic::RETENTION;
        assert!(ServerRole::All.runs_pass(sweep) && ServerRole::All.runs_pass(retention));
        assert!(!ServerRole::Api.runs_pass(sweep) && ServerRole::Api.runs_pass(retention));
        assert!(ServerRole::Worker.runs_pass(sweep) && !ServerRole::Worker.runs_pass(retention));
        // KAIROS-T-0531: the report of the stale claims runs where the API runs.
        let stale = crate::periodic::STALE_CLAIMS;
        assert!(ServerRole::Api.runs_pass(stale) && !ServerRole::Worker.runs_pass(stale));
        let keys = crate::periodic::KEY_EXPIRING;
        assert!(ServerRole::Api.runs_pass(keys) && !ServerRole::Worker.runs_pass(keys));
        assert!(ServerRole::Api.serves_api() && !ServerRole::Worker.serves_api());
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

    #[test]
    fn auto_join_domains_are_normalised() {
        let mut vars = MINIMAL.to_vec();
        vars.extend([
            ("KAIROS_SINGLE_TENANT", "acme"),
            (
                "KAIROS_AUTO_JOIN_DOMAINS",
                " Acme.example , @corp.acme.example,,acme.example",
            ),
        ]);
        let config = AppConfig::from_lookup(lookup(&vars)).expect("valid config");
        assert_eq!(
            config.auto_join_domains,
            vec!["acme.example".to_string(), "corp.acme.example".to_string()]
        );
    }

    #[test]
    fn auto_join_domains_default_to_none() {
        let config = AppConfig::from_lookup(lookup(MINIMAL)).expect("valid config");
        assert!(config.auto_join_domains.is_empty());
    }

    #[test]
    fn auto_join_domains_need_the_single_tenant() {
        let mut vars = MINIMAL.to_vec();
        vars.push(("KAIROS_AUTO_JOIN_DOMAINS", "acme.example"));
        let err = AppConfig::from_lookup(lookup(&vars)).expect_err("refused");
        assert!(
            matches!(
                err,
                ConfigError::Invalid {
                    var: "KAIROS_AUTO_JOIN_DOMAINS",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn auto_join_domains_refuse_an_address_or_a_typo() {
        for bad in ["alice@acme.example", "acme", ".acme.example"] {
            let mut vars = MINIMAL.to_vec();
            vars.extend([
                ("KAIROS_SINGLE_TENANT", "acme"),
                ("KAIROS_AUTO_JOIN_DOMAINS", bad),
            ]);
            let err = AppConfig::from_lookup(lookup(&vars)).expect_err(bad);
            assert!(
                err.to_string().contains("KAIROS_AUTO_JOIN_DOMAINS"),
                "{err}"
            );
        }
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

        // The site store (KAIROS-T-0405) branches on KAIROS_PAGES_STORE: the
        // disk and the s3 arms read disjoint variables. Drive both.
        for store in ["disk", "s3"] {
            let _ = crate::site_store::parse_settings(&|var: &str| {
                record(
                    var,
                    Some(match var {
                        "KAIROS_PAGES_STORE" => store.to_string(),
                        "KAIROS_PAGES_MAX_SIZE" | "KAIROS_PAGES_MAX_FILES" => "1".to_string(),
                        _ => "x".to_string(),
                    }),
                )
            });
        }

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
        // The tailnet front door (COLLIERY-T-0209, was KAIROS-T-0209). Compose
        // itself reads this one to decide which services start.
        "COMPOSE_PROFILES",
        // What .env.example documents for the `tailscale` profile. Compose maps
        // each to the TS_ name below; none reaches the kairos container.
        "TAILSCALE_AUTHKEY",
        "TAILSCALE_HOSTNAME",
        "TAILSCALE_EXTRA_ARGS",
        // The tailscale/tailscale image's own interface, set on the sidecar
        // service only. That image reads them, not ours.
        "TS_AUTHKEY",
        "TS_HOSTNAME",
        "TS_EXTRA_ARGS",
        "TS_STATE_DIR",
        "TS_USERSPACE",
        "TS_SERVE_CONFIG",
        // The scheduled backup (COLLIERY-T-0229). What .env.example documents;
        // compose maps the first two to the BACKUP_ names below and mounts
        // the third as a host directory.
        "KAIROS_BACKUP_INTERVAL",
        "KAIROS_BACKUP_KEEP",
        "KAIROS_BACKUP_DIR",
        // The backup service's own shell loop reads these two...
        "BACKUP_INTERVAL",
        "BACKUP_KEEP",
        // ...and libpq reads these four, in `pg_dump`. None reaches the
        // kairos container.
        "PGHOST",
        "PGUSER",
        "PGPASSWORD",
        "PGDATABASE",
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
