//! Router construction and server runtime (KAIROS-T-0017).
//!
//! [`build_state`] + [`router`] are separated from `main` so integration
//! tests can construct the exact production router in-process (with any
//! [`AppConfig`] variant) and drive it via `tower::ServiceExt::oneshot`.

use std::sync::Arc;

use axum::extract::Extension;
use axum::routing::get;
use axum::{Json, Router, middleware as axum_middleware};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use kairos_db::{TenantPool, schema};
use uuid::Uuid;

use crate::blocking::BlockingTenantPool;
use crate::config::AppConfig;
use crate::error::ApiError;
use crate::middleware::auth::{AuthContext, Authenticator, DiscoveryError};
use crate::middleware::tenant::{TenantContext, TenantDb};
use crate::middleware::{auth, tenant};

/// Shared state behind every request: config, the tenant-pinning pool, and
/// the token validator.
#[derive(Clone)]
pub struct AppState {
    /// Startup configuration (KAIROS-A-0013).
    pub config: Arc<AppConfig>,
    /// The shared bb8 pool with per-checkout `search_path` pinning.
    pub pool: TenantPool,
    /// The sync pool bridging handlers to the sync kairos-db services
    /// (KAIROS-T-0018, see [`crate::blocking`]).
    pub blocking: BlockingTenantPool,
    /// The sync pool of the job worker and the jobs (KAIROS-T-0614,
    /// `KAIROS_JOB_POOL_SIZE`). Separate from [`Self::blocking`], so a
    /// burst of jobs leaves the handlers their connections.
    pub job_pool: BlockingTenantPool,
    /// The OIDC validator (JWKS cache).
    pub auth: Arc<Authenticator>,
    /// The embedding service, when this deployment has one (KAIROS-T-0190).
    ///
    /// `None` means retrieval degrades to lexical, which is A-0021 rule 7 rather
    /// than a failure: embeddings switched off, or a provider that could not
    /// start, must not stop the server serving boards.
    pub embedding: Option<Arc<crate::embedding::EmbeddingService>>,
    /// The per-router Prometheus metrics registry (KAIROS-A-0013,
    /// KAIROS-T-0049). Per-`AppState` (not a process-global recorder) so
    /// in-process test routers stay isolated — see [`crate::metrics`].
    pub metrics: Arc<crate::metrics::Metrics>,
    /// The failed-authentication throttle (KAIROS-T-0202), or `None` when
    /// `KAIROS_AUTH_MAX_FAILURES` is 0 and throttling is off.
    ///
    /// Per-`AppState` for the same reason as [`crate::metrics::Metrics`]: a
    /// process-global would let one in-process test router's lockouts refuse
    /// another's requests.
    pub throttle: Option<Arc<crate::rate_limit::AuthThrottle>>,
    /// The cache of the auth and tenant lookups (KAIROS-T-0609). Per
    /// `AppState`, as the throttle is.
    pub auth_cache: Arc<crate::auth_cache::AuthCache>,
    /// The clones of the indexed repositories (COLLIERY-T-1853), when
    /// `KAIROS_CODE_INDEX_DIR` is set. `None`: no nearest-commit answer and no
    /// builder; the upload and the download of an index still work.
    pub code_index: Option<Arc<crate::code_index::CodeIndexService>>,
    /// The tenants whose schema did not migrate at startup (KAIROS-T-0330):
    /// their requests are refused until the schema is current.
    pub tenant_readiness: Arc<crate::tenant_readiness::TenantReadiness>,
    /// The worker of the job queue of this server (KAIROS-T-0373). Its id
    /// is the id of the start of this server (KAIROS-T-0354): the code-index
    /// runs record the same id.
    pub jobs: Arc<crate::jobs::Worker>,
    /// The store of the uploaded repository sites (KAIROS-T-0405), when
    /// `KAIROS_PAGES_STORE` is set. `None`: a publish is refused; the
    /// rendered sites still work.
    pub site_store: Option<crate::site_store::SiteStore>,
    /// The retention settings of this server (KAIROS-A-0004,
    /// KAIROS-T-0366). `serve` sets them from the environment; a state of
    /// [`build_state`] or [`state_with`] has the defaults. `/readyz` and
    /// `GET /api/org/retention` read them (KAIROS-T-0631).
    pub retention: Arc<kairos_core::retention::RetentionConfig>,
}

/// Why [`build_state`] failed (startup-time, fail-fast).
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The connection pool could not be built.
    #[error("building connection pool: {0}")]
    Pool(#[from] kairos_db::PoolError),
    /// OIDC discovery/JWKS priming failed.
    #[error(transparent)]
    Oidc(#[from] DiscoveryError),
    /// The site store of `KAIROS_PAGES_STORE` could not start (KAIROS-T-0405).
    #[error(transparent)]
    SiteStore(#[from] crate::site_store::StoreError),
}

/// Build the shared state: connect the pool and resolve the OIDC issuer
/// (discovery + JWKS priming — fails fast if the issuer is unreachable).
pub async fn build_state(config: AppConfig) -> Result<AppState, BuildError> {
    let pool = TenantPool::new(&config.database_url, config.pool_size).await?;
    let blocking = BlockingTenantPool::with_min_idle(
        &config.database_url,
        config.pool_size,
        config.pool_min_idle,
    );
    // KAIROS-T-0614: the jobs on a pool of their own. One idle connection:
    // the worker polls every 2 s, so one is in use at all times.
    let job_pool = BlockingTenantPool::with_min_idle(&config.database_url, config.job_pool_size, 1);
    // KAIROS-T-0208: discover only what is there. With no issuer there is no
    // discovery document to fetch, and a `disabled()` authenticator refuses every
    // JWT with a message that names the misconfiguration. An issuer that IS named
    // is still discovered here and still fails fast, so making it optional did not
    // turn a broken issuer into a runtime surprise.
    let auth = match (&config.oidc_issuer_url, &config.oidc_audience) {
        // KAIROS-T-0394: a worker serves no API, so it authenticates nobody
        // and does not need the issuer to start.
        _ if !config.role.serves_api() => Authenticator::disabled(),
        (Some(issuer), Some(audience)) => Authenticator::discover(issuer, audience).await?,
        _ => {
            tracing::warn!(
                "no OIDC issuer configured: this deployment authenticates people by \
                 password only (KAIROS_LOCAL_AUTH). JWT bearers will be refused."
            );
            Authenticator::disabled()
        }
    };
    // KAIROS-T-0354, KAIROS-T-0373: one random id for this start of the
    // server, for the leases of the jobs and the code-index runs.
    let instance = uuid::Uuid::new_v4();
    let code_index = code_index_service(&config, instance);
    let throttle = crate::rate_limit::from_config(&config, &blocking).map(Arc::new);
    let auth_cache = Arc::new(crate::auth_cache::AuthCache::new(
        std::time::Duration::from_secs(config.auth_cache_secs),
    ));
    // KAIROS-T-0394: the worker registers only the kinds of its role.
    // KAIROS-T-0576: the worker counts its failures in the metrics of
    // the state.
    let metrics = Arc::new(crate::metrics::Metrics::new());
    let jobs = Arc::new(
        crate::jobs::Worker::new(job_pool.clone(), instance, config.jobs.clone())
            .with_role(config.role)
            .with_metrics(Arc::clone(&metrics)),
    );
    // KAIROS-T-0384: the signed webhook deliveries are jobs of the queue.
    crate::webhooks::WebhookService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.webhooks.clone(),
    )
    .with_public_url(config.public_url.clone())
    .register(&jobs);
    // KAIROS-T-0386: the GitHub write-back is a job of the queue too.
    crate::forge::writeback::WriteBackService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.clone(),
        config.public_url.clone(),
    )
    .register(&jobs);
    // KAIROS-T-0575: the write-back of an issue that became a task.
    crate::forge::issue_writeback::IssueWriteBackService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.clone(),
        config.public_url.clone(),
    )
    .register(&jobs);
    // KAIROS-T-0487: the import of the open issues is a job of the queue.
    crate::forge::import::IssueImportService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.api_url.clone(),
    )
    .register(&jobs);
    // KAIROS-T-0394: a worker serves no site, so it opens no store.
    let site_store = if config.role.serves_api() {
        site_store(&config, &jobs)?
    } else {
        None
    };
    Ok(AppState {
        config: Arc::new(config),
        pool,
        blocking,
        job_pool,
        auth: Arc::new(auth),
        embedding: build_embedding_service(),
        metrics,
        throttle,
        auth_cache,
        code_index,
        tenant_readiness: Arc::new(crate::tenant_readiness::TenantReadiness::default()),
        jobs,
        site_store,
        retention: Arc::new(kairos_core::retention::RetentionConfig::default()),
    })
}

/// The site store of `KAIROS_PAGES_STORE` with its cleanup jobs on
/// `worker`, or `None` when no store is set (KAIROS-T-0405).
fn site_store(
    config: &AppConfig,
    worker: &crate::jobs::Worker,
) -> Result<Option<crate::site_store::SiteStore>, crate::site_store::StoreError> {
    let Some(store_config) = &config.pages.store else {
        return Ok(None);
    };
    let store = crate::site_store::SiteStore::new(store_config)?;
    crate::site_store::CleanupHandler::register(store.clone(), worker);
    Ok(Some(store))
}

/// The clones of the base code index (COLLIERY-T-1853), or `None` when
/// `KAIROS_CODE_INDEX_DIR` is not set.
fn code_index_service(
    config: &AppConfig,
    instance: uuid::Uuid,
) -> Option<Arc<crate::code_index::CodeIndexService>> {
    let dir = config.code_index_dir.clone()?;
    Some(Arc::new(
        crate::code_index::CodeIndexService::from_repo_url(dir)
            .with_instance(instance)
            .with_secrets_key(config.secrets_key.clone())
            .with_min_rebuild_secs(config.code_index_min_rebuild_secs)
            .with_dependency_settings(crate::code_index::DependencySettings {
                cap_bytes: config.code_index_deps_cap_mb.saturating_mul(1024 * 1024),
                min_free_bytes: config.code_index_min_free_mb.saturating_mul(1024 * 1024),
                pass_variables: config.code_index_deps_env.clone(),
                tool_downloads: config.code_index_tool_downloads,
            }),
    ))
}

/// Start the builder of the base code index (COLLIERY-T-1853), when the
/// deployment has the clones, the vectors and a summarizer. **Never fails
/// the server**: when a part is missing, it logs why and does not start.
fn start_code_index_builder(state: &AppState) {
    let interval = state.config.code_index_poll_secs;
    let Some(service) = state.code_index.clone() else {
        return;
    };
    if interval == 0 {
        tracing::info!("the code index builder is off (KAIROS_CODE_INDEX_POLL_SECS=0)");
        return;
    }
    let Some(embedding) = state.embedding.clone() else {
        tracing::warn!("the code index builder did not start: embeddings are off");
        return;
    };
    // KAIROS-T-0341: a server with no embedded model still runs the
    // builder, for the repositories on a hosted provider. A repository on
    // the embedded model then gets a failed run that says so.
    if let Err(reason) = crate::code_index::has_summarizer() {
        tracing::warn!(%reason, "the code index builder runs with no embedded model");
    }
    let set = match kairos_index::tools::ToolSet::pinned() {
        // A server with no embedded model downloads no summary model
        // (KAIROS-T-0344): rust-analyzer, the std source and the toolchain
        // are enough for the structure of the indexes.
        Ok(set) if crate::code_index::has_summarizer().is_err() => set.without_model(),
        Ok(set) => set,
        Err(e) => {
            tracing::warn!(reason = %e, "the code index builder did not start");
            return;
        }
    };
    // The tools are checked, and downloaded on the first start
    // (COLLIERY-T-2525), off the start of the server: a download of about
    // 2.7 GB must not keep the server from serving.
    // KAIROS-T-0614: the builds are jobs, so they run on the pool of the jobs.
    let blocking = state.job_pool.clone();
    let worker = Arc::clone(&state.jobs);
    let threads = state.config.code_index_threads;
    tokio::spawn(async move {
        let dir = service.dir().to_path_buf();
        let prepared =
            tokio::task::spawn_blocking(move || crate::code_index::prepare_tools(Some(&dir), &set))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
        let tools = match prepared
            .and_then(|tools| tools.ok_or_else(|| "no KAIROS_CODE_INDEX_DIR".to_string()))
        {
            Ok(tools) => tools,
            Err(reason) => {
                tracing::warn!(%reason, "the code index builder did not start");
                return;
            }
        };
        service.use_tools(&tools);
        let summarizers = match crate::code_index::summarizers(&tools, threads) {
            Ok(summarizers) => Some(summarizers),
            Err(reason) => {
                tracing::warn!(
                    %reason,
                    "the code index builder has no embedded model: only the repositories on a hosted provider build"
                );
                None
            }
        };
        crate::code_index::run_builder(
            worker,
            blocking,
            service,
            summarizers,
            embedding.provider(),
            std::time::Duration::from_secs(interval),
        )
        .await;
    });
}

/// Build the embedding service, or `None`.
///
/// **Never fails the server.** A provider that will not start — no model in the
/// image, an unreachable endpoint, a configuration this build was not compiled
/// for — means retrieval degrades to lexical, which is KAIROS-A-0021 rule 7. A
/// deployment whose boards stop serving because an embedding model is missing
/// has traded something valuable for something optional.
///
/// It logs at `warn` rather than `info` on failure, because "search is quietly
/// worse than you think" is exactly the state an operator needs told about.
fn build_embedding_service() -> Option<Arc<crate::embedding::EmbeddingService>> {
    let config = match kairos_embed::EmbedConfig::from_env() {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!(error = %e, "embedding configuration is invalid; retrieval will be lexical only");
            return None;
        }
    };
    match config.build() {
        Ok(Some(provider)) => {
            let service = crate::embedding::EmbeddingService::new(provider);
            tracing::info!(model = %service.model_display(), "embedding provider ready");
            Some(Arc::new(service))
        }
        Ok(None) => {
            tracing::info!("embeddings are disabled; retrieval is lexical only");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "embedding provider unavailable; retrieval will be lexical only");
            None
        }
    }
}

/// Build a state from an existing pool and a pre-built [`Authenticator`]
/// (integration tests construct config variants without re-discovering).
/// The sync service pool ([`crate::blocking`]) is lazy, so it is built
/// here from `config.database_url` either way.
pub fn state_with(config: AppConfig, pool: TenantPool, auth: Arc<Authenticator>) -> AppState {
    // No idle minimum here: a test run builds hundreds of states.
    let blocking = BlockingTenantPool::new(&config.database_url, config.pool_size);
    // KAIROS-T-0614: the jobs on a pool of their own, as in `build_state`.
    let job_pool = BlockingTenantPool::new(&config.database_url, config.job_pool_size);
    // KAIROS-T-0202: built from the passed config like everything else here, so a
    // test that wants a throttle sets the variables and a test that does not set
    // `KAIROS_AUTH_MAX_FAILURES` to 0.
    let throttle = crate::rate_limit::from_config(&config, &blocking).map(Arc::new);
    let auth_cache = Arc::new(crate::auth_cache::AuthCache::new(
        std::time::Duration::from_secs(config.auth_cache_secs),
    ));
    // The worker of a test state runs no job until a test starts it.
    let metrics = Arc::new(crate::metrics::Metrics::new());
    let jobs = Arc::new(
        crate::jobs::Worker::new(job_pool.clone(), uuid::Uuid::new_v4(), config.jobs.clone())
            .with_role(config.role)
            .with_metrics(Arc::clone(&metrics)),
    );
    // KAIROS-T-0384: a test that drains the worker sends the deliveries.
    crate::webhooks::WebhookService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.webhooks.clone(),
    )
    .with_public_url(config.public_url.clone())
    .register(&jobs);
    crate::forge::writeback::WriteBackService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.clone(),
        config.public_url.clone(),
    )
    .register(&jobs);
    // KAIROS-T-0575: the write-back of an issue that became a task.
    crate::forge::issue_writeback::IssueWriteBackService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.clone(),
        config.public_url.clone(),
    )
    .register(&jobs);
    // KAIROS-T-0487: the import of the open issues is a job of the queue.
    crate::forge::import::IssueImportService::new(
        job_pool.clone(),
        config.secrets_key.clone(),
        config.writeback.api_url.clone(),
    )
    .register(&jobs);
    let site_store = site_store(&config, &jobs).expect("the site store of a test config starts");
    AppState {
        config: Arc::new(config),
        pool,
        blocking,
        job_pool,
        auth,
        // Tests drive embedding explicitly; a provider built here would load a
        // 65 MB model into every in-process router.
        embedding: None,
        metrics,
        throttle,
        auth_cache,
        code_index: None,
        tenant_readiness: Arc::new(crate::tenant_readiness::TenantReadiness::default()),
        jobs,
        site_store,
        retention: Arc::new(kairos_core::retention::RetentionConfig::default()),
    }
}

/// The production router: `/healthz` open; everything under `/api` behind
/// the full auth → tenant stack.
pub fn router(state: AppState) -> Router {
    // Local password login (KAIROS-T-0203) and logout. OUTSIDE the auth stack:
    // login holds no credential yet, and logout has to work with a session that
    // has already expired. `/api/login` is mounted only when KAIROS_LOCAL_AUTH is
    // on — off, it DOES NOT EXIST, so a deployment with an issuer has no password
    // endpoint to attack. `/api/logout` is always there: an OIDC login of the GUI
    // opens a session too (KAIROS-T-0364).
    // COLLIERY-T-0256: no auth, but the rule of the inputs applies.
    let local_auth = crate::login::router(state.config.local_auth)
        .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input));

    // KAIROS-T-0364: `/api/session` turns the OIDC bearer of a GUI login into a
    // session (cookie included), so a reload keeps it. Behind auth, not the
    // tenant: a session is of a person.
    let open_session = crate::login::session_router()
        .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    // KAIROS-T-0204: the org-admin surfaces for local accounts. Behind the FULL
    // auth → tenant stack, unlike login itself, and likewise absent when local auth
    // is off — there are no local accounts to administer.
    let local_accounts = if state.config.local_auth {
        crate::api::local_accounts::router()
            // COLLIERY-T-0256: added first, so it runs last, after the auth
            // and the tenant. See crate::input.
            .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                tenant::require_tenant,
            ))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                auth::require_auth,
            ))
    } else {
        Router::new()
    };

    let protected = Router::new()
        .route("/api/whoami", get(whoami))
        // The S-0005 entity endpoint families (KAIROS-T-0018).
        .merge(crate::api::router())
        // The S-0005 organizational families (KAIROS-T-0019): boards,
        // teams, delivery streams, board members, org members.
        .merge(crate::api::org::router())
        // The S-0005 unified search endpoint (KAIROS-T-0021): reads are
        // tenant-open (A-0006), so auth + membership only.
        .merge(crate::api::search::router())
        .merge(crate::api::proposals::router())
        // The OpenAPI document + dev Swagger UI (KAIROS-T-0023, A-0005
        // §6): behind the same auth → tenant stack as every /api route —
        // the CI artifact is the unauthenticated copy (module docs).
        .merge(crate::api::openapi::router(state.config.dev_ui))
        // route_layer wraps bottom-up: the auth layer (added last) runs
        // first, then tenant — the A-0010 ordering. The layer of
        // COLLIERY-T-0256 runs after the two: it refuses a query parameter
        // or a body that the route does not accept (crate::input).
        .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            tenant::require_tenant,
        ))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    // The KAIROS-T-0020 families (relationships, metadata, definitions,
    // templates, history, activity) behind the SAME auth → tenant stack;
    // merged here rather than inside `api::router()` so concurrently
    // developed endpoint tasks register at distinct anchors.
    let protected = protected.merge(
        crate::api::meta::router()
            // COLLIERY-T-0256: added first, so it runs last, after the auth
            // and the tenant. See crate::input.
            .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                tenant::require_tenant,
            ))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                auth::require_auth,
            )),
    );

    // KAIROS-T-0025: org-admin SCIM token management (/api/scim-tokens)
    // behind the SAME auth → tenant stack (the SCIM protocol surface itself
    // is token-authed and mounted separately below).
    let protected = protected.merge(
        crate::scim::tokens_router()
            // COLLIERY-T-0256: added first, so it runs last, after the auth
            // and the tenant. See crate::input.
            .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                tenant::require_tenant,
            ))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                auth::require_auth,
            )),
    );

    // KAIROS-T-0059 (A-0017): org-admin service-account + API-key management
    // (/api/service-accounts) behind the SAME auth → tenant stack. The keys it
    // mints authenticate via the branch in require_auth (KAIROS-T-0058).
    let protected = protected.merge(
        crate::service_accounts::router()
            // COLLIERY-T-0256: added first, so it runs last, after the auth
            // and the tenant. See crate::input.
            .route_layer(axum_middleware::from_fn(crate::input::refuse_unknown_input))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                tenant::require_tenant,
            ))
            .route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                auth::require_auth,
            )),
    );

    Router::new()
        // KAIROS-T-0022: the A-0005 §5 WebSocket event channel. Auth →
        // tenant enforcement (plus the browser token fallback) and the
        // per-process LISTEN task live inside `ws::router`.
        .merge(crate::ws::router(state.clone()))
        // KAIROS-T-0026: the A-0011/S-0006 MCP endpoint (streamable HTTP at
        // /mcp behind the same auth → tenant stack) plus its RFC 9728
        // protected-resource metadata routes — all inside `mcp::router`.
        .merge(crate::mcp::router(state.clone()))
        // KAIROS-T-0025: the A-0016 SCIM 2.0 provisioning surface at
        // /scim/v2 — authenticated by per-tenant SCIM bearer tokens (the
        // tenant is resolved FROM the token), so it mounts OUTSIDE the
        // OIDC auth → tenant stack, like the admin router below. See
        // crate::scim module docs.
        .merge(crate::scim::router(state.clone()))
        // KAIROS-T-0099 (KAIROS-I-0009): forge webhook deliveries. Like
        // SCIM above, these carry no bearer token and no tenant header —
        // the URL carries the routing and the HMAC signature carries the
        // authenticity — so this mounts OUTSIDE the auth → tenant stack.
        // Non-/api by design (invisible to the openapi route scanner).
        .merge(crate::forge::webhook::router())
        // COLLIERY-T-0256: the three probe routes below do not have the rule
        // of the inputs. A probe of docker, tailscale or a load balancer is
        // not a client of the API, and a refusal there makes a healthy server
        // look dead.
        .route("/healthz", get(|| async { "ok" }))
        // KAIROS-T-0432: the version of the server and the download links
        // of the CLI of that version, open, for `kairos self-update`.
        // Outside /api like the probes: /api/config can fail on an
        // unreachable issuer.
        .route("/version", get(crate::version::version))
        // KAIROS-T-0049 (A-0013): readiness + Prometheus scrape, both
        // unauthenticated by convention and mounted OUTSIDE the auth stack
        // alongside /healthz. Non-/api, so outside the S-0005 surface (and
        // invisible to the openapi route-vs-spec scanner).
        .route("/readyz", get(crate::metrics::readyz))
        .route("/metrics", get(crate::metrics::metrics_handler))
        // Local password login (KAIROS-T-0203), empty unless KAIROS_LOCAL_AUTH is on.
        .merge(local_auth)
        .merge(open_session)
        .merge(local_accounts)
        .merge(protected)
        // Cross-tenant deployment-admin routes (KAIROS-T-0019): behind auth
        // only, NO tenant middleware — see api::org::admin module docs.
        .merge(crate::api::org::admin::router(state.clone()))
        // KAIROS-T-0039 (A-0015): the SPA's public /api/config + token
        // relay, and the SPA fallback serving the GUI at `/` (reserved
        // API prefixes still 404) — see crate::web module docs.
        .merge(crate::web::router(&state))
        // The docs sites (KAIROS-T-0403): `/pages`, with its own tenant (from
        // the path) and its own auth (none for a repository site).
        .merge(crate::pages::router(&state))
        .fallback(crate::web::spa_fallback)
        // KAIROS-T-0613: the request timeout of `/api` and `/mcp`, with the
        // exceptions of crate::request_timeout. Inside the metrics and the
        // security headers, so a 503 of the timeout is counted and has them.
        .layer(axum_middleware::from_fn_with_state(
            std::time::Duration::from_secs(state.config.request_timeout_secs),
            crate::request_timeout::enforce,
        ))
        // KAIROS-T-0603: gzip or brotli for each response that asks for it
        // with Accept-Encoding: the API JSON and the GUI assets. The default
        // predicate leaves out text/event-stream (the MCP stream), images,
        // bodies under 32 bytes, and a response that already carries a
        // Content-Encoding (the pre-compressed embedded assets of web.rs).
        .layer(tower_http::compression::CompressionLayer::new())
        // KAIROS-T-0049 (A-0013): the HTTP-metrics layer wraps the WHOLE
        // router (added last → outermost), so every request — matched route
        // or SPA fallback — is timed and counted, and it sees the tenant the
        // inner stack stamped onto the response. Added before `with_state`
        // so the state type is still in scope for `from_fn_with_state`.
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            crate::metrics::track_metrics,
        ))
        // KAIROS-T-0370: the security headers on each response, the SPA,
        // the API and each refusal (outermost).
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            crate::security_headers::security_headers,
        ))
        .with_state(state)
}

/// The router of a server with the role `worker` (KAIROS-T-0394): the
/// probes, the version and the metrics, and no API. Each other path is a
/// 404 that names the role.
pub fn worker_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/version", get(crate::version::version))
        .route("/readyz", get(crate::metrics::readyz))
        .route("/metrics", get(crate::metrics::metrics_handler))
        .fallback(|| async {
            ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "This server has the role worker (KAIROS_ROLE=worker). It runs the code index \
                 builds and serves no API. Send the request to a server with the role all or api.",
            )
        })
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            crate::metrics::track_metrics,
        ))
        .with_state(state)
}

/// A team the caller belongs to (from the tenant schema's `team_members`).
#[derive(Debug, serde::Serialize)]
pub struct WhoamiTeam {
    /// `teams.id`.
    pub id: Uuid,
    /// `teams.slug`.
    pub slug: String,
    /// `teams.name`.
    pub name: String,
}

/// One board on which the caller holds explicit capability grants
/// (KAIROS-A-0006 `board_member_capabilities`), with the grant list.
///
/// Org admins (`organization.role == "admin"`) hold implicit full access on
/// every board via the A-0006 org-admin bypass and typically appear with an
/// empty `capabilities` list — the field exists to surface NON-admins'
/// per-board grants so the GUI can gate per-board admin surfaces (T-0052).
#[derive(Debug, serde::Serialize)]
pub struct WhoamiBoardCapabilities {
    /// `boards.id`.
    pub board_id: Uuid,
    /// `boards.slug`.
    pub board_slug: String,
    /// The capability strings granted to the caller on this board (may
    /// include globs like `*`, `manage_*`, `configure_*`), sorted.
    pub grants: Vec<String>,
    /// The capabilities that membership of this board gives
    /// (KAIROS-T-0410, KAIROS-A-0025). Only the `member` grant (or `*`)
    /// makes a member (KAIROS-T-0541). Then, on a strategy board
    /// `manage_strategies` and `transition_items`, on an initiative board
    /// `manage_initiatives` and `transition_items`. Empty on other boards.
    /// Not archive or restore: those need a grant.
    pub implied: Vec<&'static str>,
}

/// `GET /api/whoami` — the S-0006 whoami precursor: proves the full
/// auth → tenant chain by echoing everything the stack resolved.
#[derive(Debug, serde::Serialize)]
pub struct WhoamiResponse {
    /// The authenticated user (JIT-provisioned `public.users` row).
    pub user: WhoamiUser,
    /// The resolved tenant and the caller's role in it.
    pub organization: WhoamiOrganization,
    /// Teams the caller belongs to within this tenant.
    pub teams: Vec<WhoamiTeam>,
    /// Boards on which the caller holds explicit capability grants
    /// (KAIROS-A-0006), grouped by board. Empty for a plain member with no
    /// grants; org admins get their explicit grants only (usually none —
    /// their access is the `organization.role == "admin"` bypass).
    pub capabilities: Vec<WhoamiBoardCapabilities>,
    /// COMPUTED capabilities every tenant member holds without a grant
    /// (KAIROS-T-0105): currently `file_backlog` — send a request to any
    /// team: a task in the entry column of its delivery board, in the
    /// support lane (COLLIERY-T-0218, COLLIERY-A-0023).
    pub implicit: Vec<&'static str>,
    /// Repositories owned by the caller's teams (KAIROS-T-0107, A-0019).
    pub repositories: Vec<WhoamiRepository>,
    /// The caller's grants of `manage_structure` (KAIROS-T-0337): the grant
    /// at the organization first, then the grants on boards.
    pub structure: Vec<WhoamiStructureGrant>,
    /// The agent key of the request (KAIROS-T-0536), or null when the
    /// request does not use an agent key.
    pub agent_key: Option<WhoamiAgentKey>,
}

/// The agent key of [`WhoamiResponse::agent_key`] (KAIROS-T-0536).
#[derive(Debug, serde::Serialize)]
pub struct WhoamiAgentKey {
    /// `api_keys.id`.
    pub id: Uuid,
    /// The scope of the key. `null`: the key has no scope, and so no limit
    /// (a key made before KAIROS-T-0536).
    pub scope: Option<crate::service_accounts::routes::KeyScopeView>,
}

/// One grant of [`WhoamiResponse::structure`].
#[derive(Debug, serde::Serialize)]
pub struct WhoamiStructureGrant {
    /// `organization` | `board`.
    pub scope: &'static str,
    /// The board of a board grant, or null.
    pub board_id: Option<Uuid>,
    /// The slug of the board of a board grant, or null.
    pub board_slug: Option<String>,
}

/// One repository of [`WhoamiResponse::repositories`].
#[derive(Debug, serde::Serialize)]
pub struct WhoamiRepository {
    pub id: Uuid,
    pub slug: String,
    pub forge: String,
    pub repo_full_name: String,
    /// The owning team's slug.
    pub team_slug: String,
}

/// The `user` object of [`WhoamiResponse`].
#[derive(Debug, serde::Serialize)]
pub struct WhoamiUser {
    /// `public.users.id`.
    pub id: Uuid,
    /// OIDC `sub`.
    pub external_id: String,
    /// Email claim.
    pub email: String,
    /// Display name.
    pub display_name: String,
}

/// The `organization` object of [`WhoamiResponse`].
#[derive(Debug, serde::Serialize)]
pub struct WhoamiOrganization {
    /// `public.organizations.id`.
    pub id: Uuid,
    /// Organization slug.
    pub slug: String,
    /// The caller's role (`admin` | `member`).
    pub role: &'static str,
}

/// The probe endpoint behind the full middleware stack (KAIROS-T-0017).
async fn whoami(
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Extension(db): Extension<TenantDb>,
) -> Result<Json<WhoamiResponse>, ApiError> {
    let mut conn = db.conn().await?;
    let teams: Vec<(Uuid, String, String)> = schema::team_members::table
        .inner_join(schema::teams::table)
        .filter(schema::team_members::user_id.eq(auth.user_id))
        .filter(schema::teams::deleted_at.is_null())
        .order(schema::teams::slug.asc())
        .select((schema::teams::id, schema::teams::slug, schema::teams::name))
        .load(&mut *conn)
        .await
        .map_err(ApiError::internal)?;
    // KAIROS-T-0107: the repositories my teams own (A-0019) — what the
    // plugin's bootstrap matches a git remote against.
    let team_ids: Vec<Uuid> = teams.iter().map(|(id, _, _)| *id).collect();
    let repositories: Vec<(Uuid, String, String, String, String)> = schema::repositories::table
        .inner_join(schema::teams::table)
        .filter(schema::repositories::team_id.eq_any(&team_ids))
        .filter(schema::repositories::deleted_at.is_null())
        .order(schema::repositories::slug.asc())
        .select((
            schema::repositories::id,
            schema::repositories::slug,
            schema::repositories::forge,
            schema::repositories::repo_full_name,
            schema::teams::slug,
        ))
        .load(&mut *conn)
        .await
        .map_err(ApiError::internal)?;

    // The caller's explicit board-scoped capability grants (KAIROS-A-0006),
    // one indexed load over `board_member_capabilities` joined to `boards`
    // (its primary key, board_id first), ordered so grouping is a linear
    // fold and both boards and grants come out sorted. Org admins get only
    // their explicit rows here (usually none — their real access is the
    // org-admin bypass, reported via `organization.role`).
    let grant_rows: Vec<(Uuid, String, kairos_db::models::enums::BoardLevel, String)> =
        schema::board_member_capabilities::table
            .inner_join(schema::boards::table)
            .filter(schema::board_member_capabilities::user_id.eq(auth.user_id))
            .filter(schema::boards::deleted_at.is_null())
            .order((
                schema::boards::slug.asc(),
                schema::board_member_capabilities::capability.asc(),
            ))
            .select((
                schema::boards::id,
                schema::boards::slug,
                schema::boards::board_level,
                schema::board_member_capabilities::capability,
            ))
            .load(&mut *conn)
            .await
            .map_err(ApiError::internal)?;

    let mut capabilities: Vec<WhoamiBoardCapabilities> = Vec::new();
    for (board_id, board_slug, level, capability) in grant_rows {
        match capabilities.last_mut() {
            Some(last) if last.board_id == board_id => last.grants.push(capability),
            _ => capabilities.push(WhoamiBoardCapabilities {
                board_id,
                board_slug,
                grants: vec![capability],
                implied: Vec::new(),
            }),
        }
        // KAIROS-T-0541: only the `member` grant (or `*`) makes a member.
        if let Some(last) = capabilities.last_mut() {
            last.implied =
                kairos_core::abac::member_capabilities_of_grants(level.as_str(), &last.grants)
                    .to_vec();
        }
    }

    // KAIROS-T-0337: the grants of `manage_structure`, at the organization
    // and on live boards.
    let structure_rows: Vec<(Option<Uuid>, Option<String>)> = schema::structure_grants::table
        .left_join(schema::boards::table)
        .filter(schema::structure_grants::user_id.eq(auth.user_id))
        .filter(
            schema::structure_grants::board_id
                .is_null()
                .or(schema::boards::deleted_at.is_null()),
        )
        .order(schema::boards::slug.asc().nulls_first())
        .select((
            schema::structure_grants::board_id,
            schema::boards::slug.nullable(),
        ))
        .load(&mut *conn)
        .await
        .map_err(ApiError::internal)?;
    let structure = structure_rows
        .into_iter()
        .map(|(board_id, board_slug)| WhoamiStructureGrant {
            scope: if board_id.is_some() {
                "board"
            } else {
                "organization"
            },
            board_id,
            board_slug,
        })
        .collect();

    // KAIROS-T-0536: the agent key of the request, and its scope.
    let agent_key = match auth.agent_key {
        None => None,
        Some(id) => {
            let scope = match &auth.key_scope {
                None => None,
                Some(scope) => {
                    let ids: Vec<Uuid> = scope.boards.iter().flatten().copied().collect();
                    let slugs: Vec<(Uuid, String)> = schema::boards::table
                        .filter(schema::boards::id.eq_any(&ids))
                        .filter(schema::boards::deleted_at.is_null())
                        .select((schema::boards::id, schema::boards::slug))
                        .load(&mut *conn)
                        .await
                        .map_err(ApiError::internal)?;
                    Some(crate::service_accounts::routes::KeyScopeView {
                        capabilities: scope.capabilities.iter().cloned().collect(),
                        boards: scope.boards.as_ref().map(|boards| {
                            boards
                                .iter()
                                .map(|board| crate::service_accounts::routes::KeyScopeBoard {
                                    id: board.to_string(),
                                    slug: slugs
                                        .iter()
                                        .find(|(b, _)| b == board)
                                        .map(|(_, slug)| slug.clone()),
                                })
                                .collect()
                        }),
                    })
                }
            };
            Some(WhoamiAgentKey { id, scope })
        }
    };

    Ok(Json(WhoamiResponse {
        agent_key,
        structure,
        user: WhoamiUser {
            id: auth.user_id,
            external_id: auth.external_id,
            email: auth.email,
            display_name: auth.display_name,
        },
        organization: WhoamiOrganization {
            id: tenant.org_id,
            slug: tenant.slug,
            role: tenant.role.as_str(),
        },
        implicit: kairos_core::abac::COMPUTED_CAPABILITIES.to_vec(),
        repositories: repositories
            .into_iter()
            .map(
                |(id, slug, forge, repo_full_name, team_slug)| WhoamiRepository {
                    id,
                    slug,
                    forge,
                    repo_full_name,
                    team_slug,
                },
            )
            .collect(),
        teams: teams
            .into_iter()
            .map(|(id, slug, name)| WhoamiTeam { id, slug, name })
            .collect(),
        capabilities,
    }))
}

/// Run the server: build state, bind `KAIROS_BIND_ADDR`, serve with
/// graceful shutdown on ctrl-c or SIGTERM. Public migrations already ran (main.rs
/// applies them before any subcommand, KAIROS-T-0007).
pub async fn serve(
    config: AppConfig,
    readiness: crate::tenant_readiness::TenantReadiness,
) -> Result<(), String> {
    let bind_addr = config.bind_addr;
    // KAIROS-T-0366: a bad retention value stops the start, like any other
    // bad setting.
    let mut retention = kairos_core::retention::RetentionConfig::from_env()
        .map_err(|e| format!("retention configuration: {e}"))?;
    // KAIROS-T-0671: the store of an `s3://` archive target.
    if let Some(store) = crate::archive_store::for_config(&retention)? {
        retention.archive_store = store;
    }
    let mut state = build_state(config).await.map_err(|e| e.to_string())?;
    state.retention = Arc::new(retention.clone());
    // KAIROS-T-0631: a pass that removes nothing, or fails by its config,
    // says so at the start too.
    if let Some(warning) = retention.warning() {
        tracing::warn!(%warning, "retention");
    }
    // KAIROS-T-0330: the tenants that did not migrate at startup.
    state.tenant_readiness = Arc::new(readiness);
    let role = state.config.role;
    tracing::info!(role = role.as_str(), "kairos-server role (KAIROS_ROLE)");

    // KAIROS-T-0204: the first-boot admin, before the listener binds. A fresh
    // local-auth deployment has nobody who can log in, and no way to make anybody.
    //
    // A failure here FAILS THE BOOT. An operator who set these variables is telling
    // us the deployment has no other way in, so carrying on would produce a server
    // that starts, serves a login page and cannot be logged into — the failure mode
    // hardest to diagnose from the outside.
    //
    // KAIROS-T-0394: a worker serves no API; the first-boot admin is the
    // work of a server that does.
    if role.serves_api() {
        bootstrap_first_admin(&state).await?;
    }
    start_background(&state, retention).await;
    if role.serves_api() {
        warm_start(&state).await;
    }

    let app = if role.serves_api() {
        router(state)
    } else {
        worker_router(state)
    };

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("cannot bind KAIROS_BIND_ADDR {bind_addr}: {e}"))?;
    tracing::info!(%bind_addr, role = role.as_str(), "kairos-server listening");

    // KAIROS-T-0202: the throttle needs the socket peer for its source bucket,
    // and `ConnectInfo` is the only place axum will put it. Tests that drive the
    // router with `oneshot` insert no `ConnectInfo`, which the throttle treats as
    // "no address to attribute" rather than as an error.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(|e| format!("server error: {e}"))?;
    // KAIROS-T-0336: the last line of a clean stop, so an operator (and the
    // image smoke check) can tell it from a kill.
    tracing::info!("shutdown complete");
    Ok(())
}

/// The warm start (KAIROS-T-0613), before `bind`: the first requests do not
/// build the route table of the inputs, open the first connection of the
/// blocking pool, or start its first thread. The discovery document of the
/// issuer is the one of [`build_state`] (`web::router` uses it). A failure
/// here does not stop the start: the first request then pays, as before.
async fn warm_start(state: &AppState) {
    let started = std::time::Instant::now();
    let routes = crate::input::warm();
    let checkout = state
        .blocking
        .run_public(|conn| {
            use diesel::connection::SimpleConnection as _;
            conn.batch_execute("SELECT 1").map_err(ApiError::internal)
        })
        .await;
    if let Err(e) = checkout {
        tracing::warn!(error = %e.message, "warm start: the blocking pool did not open a connection");
    }
    tracing::info!(
        routes,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "warm start done"
    );
}

/// The background work of `serve` for the role of the server
/// (KAIROS-T-0394): the embedding refresher, retention, the report of
/// the stale claims (KAIROS-T-0531) and the report of the keys that expire
/// soon (KAIROS-T-0540) where the API runs, the code index builder where the builds run, and the job worker
/// on each server. The job worker of `state` registers only the kinds of
/// its role.
async fn start_background(state: &AppState, retention: kairos_core::retention::RetentionConfig) {
    let role = state.config.role;
    let refresh_secs = state.config.embed_refresh_secs;

    // The background refresher (KAIROS-T-0190): embedding happens off the write
    // path, so a create never waits on a model. Each organization has one job
    // row of the queue (KAIROS-T-0375, KAIROS-T-0636), so 2 servers do not
    // embed one item twice. Detached
    // deliberately — it is not part of graceful shutdown, because there is
    // nothing to drain: a batch interrupted mid-page keeps its lease until it
    // runs out, and a server takes it again.
    if let (Some(service), true, true) =
        (state.embedding.clone(), refresh_secs > 0, role.serves_api())
    {
        tokio::spawn(crate::embedding::run_refresher(
            Arc::clone(&state.jobs),
            state.job_pool.clone(),
            service,
            std::time::Duration::from_secs(refresh_secs),
            crate::embedding::REFRESH_BATCH,
        ));
    }

    // KAIROS-T-0394: an `api` server starts no builder, so it fetches no
    // tools and runs no build.
    if role.builds() {
        start_code_index_builder(state);
    } else {
        tracing::info!(
            "this server runs no code index builder (KAIROS_ROLE=api): a server with \
             KAIROS_ROLE=worker runs the builds"
        );
    }
    // KAIROS-T-0373: the worker of the job queue. Detached like the
    // refresher: a job that a stop cuts short keeps its lease until it runs
    // out, and then a server takes it again.
    tokio::spawn(Arc::clone(&state.jobs).run());
    // KAIROS-T-0531: the report of the stale claims is a periodic job too.
    // `KAIROS_STALE_CLAIM_DAYS=0` turns it off.
    let stale_days = state.config.stale_claim_days;
    if stale_days > 0
        && role.runs_pass(crate::periodic::STALE_CLAIMS)
        && let Err(e) = state
            .jobs
            .add_periodic(
                crate::periodic::STALE_CLAIMS,
                crate::periodic::STALE_CLAIMS_INTERVAL,
                crate::periodic::stale_claims_pass(state.config.database_url.clone(), stale_days),
            )
            .await
    {
        tracing::error!(error = ?e, "stale claims: the first run of the pass was not added");
    }
    // KAIROS-T-0540: the report of the keys that expire soon.
    if role.runs_pass(crate::periodic::KEY_EXPIRING)
        && let Err(e) = state
            .jobs
            .add_periodic(
                crate::periodic::KEY_EXPIRING,
                crate::periodic::KEY_EXPIRING_INTERVAL,
                crate::periodic::key_expiring_pass(state.config.database_url.clone()),
            )
            .await
    {
        tracing::error!(error = ?e, "key expiring: the first run of the pass was not added");
    }
    // KAIROS-T-0731: the check of the typed sections of the documents that
    // no write checked (the backfill), or whose check is stale.
    if role.runs_pass(crate::periodic::SECTION_CHECKS)
        && let Err(e) = state
            .jobs
            .add_periodic(
                crate::periodic::SECTION_CHECKS,
                crate::periodic::SECTION_CHECKS_INTERVAL,
                crate::periodic::section_checks_pass(state.config.database_url.clone()),
            )
            .await
    {
        tracing::error!(error = ?e, "section checks: the first run of the pass was not added");
    }
    // KAIROS-T-0670: the cadence of the issue import. One run at a time looks
    // at the repositories whose import is due and queues their jobs.
    if role.runs_pass(crate::periodic::ISSUE_SYNC)
        && let Err(e) = state
            .jobs
            .add_periodic(
                crate::periodic::ISSUE_SYNC,
                crate::forge::sync::TICK,
                crate::forge::sync::pass(
                    state.config.database_url.clone(),
                    state.config.jobs.max_tries,
                ),
            )
            .await
    {
        tracing::error!(error = ?e, "issue sync: the first run of the pass was not added");
    }
    // KAIROS-T-0366, KAIROS-T-0376: retention is a periodic job. The queue
    // gives each run to one server, and each run adds the next.
    if !role.runs_pass(crate::periodic::RETENTION) {
        return;
    }
    tracing::info!(
        mode = %retention.mode,
        interval_secs = crate::periodic::RETENTION_INTERVAL.as_secs(),
        "retention: the pass is a periodic job"
    );
    if let Err(e) = state
        .jobs
        .add_periodic(
            crate::periodic::RETENTION,
            crate::periodic::RETENTION_INTERVAL,
            crate::periodic::retention_pass(state.config.database_url.clone(), retention),
        )
        .await
    {
        tracing::error!(error = ?e, "retention: the first run of the pass was not added");
    }
}

/// Create the first-boot admin if `KAIROS_BOOTSTRAP_ADMIN` is set and the
/// deployment has no users (KAIROS-T-0204).
///
/// Every outcome is logged, and the two that are not the happy path are logged at
/// WARN, because each means something the operator should act on:
///
/// - **consumed** — INFO, and it says to remove the variables.
/// - **inert** — WARN. The variables are still in the manifest, where they are a
///   credential nobody is using. Silence here is how a bootstrap password survives
///   in a values file for a year.
/// - **raced** — WARN. Harmless, but worth knowing it happened.
async fn bootstrap_first_admin(state: &AppState) -> Result<(), String> {
    let Some(email) = state.config.bootstrap_admin.clone() else {
        return Ok(());
    };

    // A pre-hashed value wins, and is the documented preference: it keeps the
    // plaintext out of the manifest entirely.
    let hash = match (
        &state.config.bootstrap_password_hash,
        &state.config.bootstrap_password,
    ) {
        (Some(hash), _) => hash.clone(),
        (None, Some(plain)) => {
            crate::local_auth::validate_password(plain)
                .map_err(|e| format!("KAIROS_BOOTSTRAP_PASSWORD: {e}"))?;
            crate::local_auth::hash_password(plain)
                .map_err(|e| format!("KAIROS_BOOTSTRAP_PASSWORD: {e}"))?
        }
        // config.rs refuses this combination, so reaching it means that check was
        // removed.
        (None, None) => {
            return Err("KAIROS_BOOTSTRAP_ADMIN is set with no password".to_string());
        }
    };

    if !state.config.local_auth {
        // Not fatal: the account is real and will work the moment local auth is
        // switched on. But it cannot log in today, and an operator who set these
        // variables plainly expected it to.
        tracing::warn!(
            %email,
            "KAIROS_BOOTSTRAP_ADMIN is set but KAIROS_LOCAL_AUTH is off, so the \
             account that is about to be created cannot log in with its password"
        );
    }

    let for_log = email.clone();
    let outcome = state
        .blocking
        .run_public(move |conn| {
            kairos_db::local_auth::bootstrap_admin_if_empty(conn, &email, &email, &hash)
                .map_err(crate::error::ApiError::internal)
        })
        .await
        .map_err(|e| format!("bootstrap admin: {} {}", e.code, e.message))?;

    match outcome {
        Ok(user) => tracing::info!(
            email = %for_log,
            user_id = %user.id,
            external_id = %user.external_id,
            "created the first-boot admin. REMOVE KAIROS_BOOTSTRAP_ADMIN and its \
             password from your configuration now: the account exists, the variables \
             are a credential sitting in your manifest, and they will do nothing on \
             the next boot because the deployment is no longer empty."
        ),
        Err(kairos_db::local_auth::BootstrapSkipped::UsersExist) => tracing::warn!(
            email = %for_log,
            "KAIROS_BOOTSTRAP_ADMIN is set but this deployment already has users, so \
             it did nothing — as designed, it is single-use. Remove it and its \
             password: they are a credential in your configuration that is no longer \
             reachable."
        ),
        Err(kairos_db::local_auth::BootstrapSkipped::RacedAnotherReplica) => tracing::warn!(
            email = %for_log,
            "another replica created the first user while this one was booting; the \
             bootstrap admin was not created here. Exactly one exists, which is the \
             outcome either way."
        ),
    }
    Ok(())
}

/// Resolves when ctrl-c (SIGINT) or SIGTERM arrives.
///
/// KAIROS-T-0336: `docker stop` and Kubernetes send SIGTERM. Before, the
/// server listened for SIGINT only: as PID 1 the kernel dropped SIGTERM and
/// the runtime killed the server at the end of the stop timeout; behind the
/// init of the image SIGTERM ended it with no drain.
async fn shutdown_signal() {
    let interrupt = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to listen for ctrl-c");
            std::future::pending::<()>().await;
        }
        "SIGINT"
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
        "SIGTERM"
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<&str>();
    let signal = tokio::select! {
        name = interrupt => name,
        name = terminate => name,
    };
    tracing::info!(signal, "shutdown signal received; draining");
}
