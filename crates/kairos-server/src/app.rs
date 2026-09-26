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
}

/// Pool size for the server. Fixed for now; becomes configuration when a
/// deployment needs it (A-0013 keeps the env surface minimal).
const POOL_SIZE: u32 = 16;

/// Build the shared state: connect the pool and resolve the OIDC issuer
/// (discovery + JWKS priming — fails fast if the issuer is unreachable).
pub async fn build_state(config: AppConfig) -> Result<AppState, BuildError> {
    let pool = TenantPool::new(&config.database_url, POOL_SIZE).await?;
    let blocking = BlockingTenantPool::new(&config.database_url, POOL_SIZE);
    // KAIROS-T-0208: discover only what is there. With no issuer there is no
    // discovery document to fetch, and a `disabled()` authenticator refuses every
    // JWT with a message that names the misconfiguration. An issuer that IS named
    // is still discovered here and still fails fast, so making it optional did not
    // turn a broken issuer into a runtime surprise.
    let auth = match (&config.oidc_issuer_url, &config.oidc_audience) {
        (Some(issuer), Some(audience)) => Authenticator::discover(issuer, audience).await?,
        _ => {
            tracing::warn!(
                "no OIDC issuer configured: this deployment authenticates people by \
                 password only (KAIROS_LOCAL_AUTH). JWT bearers will be refused."
            );
            Authenticator::disabled()
        }
    };
    let throttle = crate::rate_limit::from_config(&config).map(Arc::new);
    Ok(AppState {
        config: Arc::new(config),
        pool,
        blocking,
        auth: Arc::new(auth),
        embedding: build_embedding_service(),
        metrics: Arc::new(crate::metrics::Metrics::new()),
        throttle,
    })
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
    let blocking = BlockingTenantPool::new(&config.database_url, POOL_SIZE);
    // KAIROS-T-0202: built from the passed config like everything else here, so a
    // test that wants a throttle sets the variables and a test that does not set
    // `KAIROS_AUTH_MAX_FAILURES` to 0.
    let throttle = crate::rate_limit::from_config(&config).map(Arc::new);
    AppState {
        config: Arc::new(config),
        pool,
        blocking,
        auth,
        // Tests drive embedding explicitly; a provider built here would load a
        // 65 MB model into every in-process router.
        embedding: None,
        metrics: Arc::new(crate::metrics::Metrics::new()),
        throttle,
    }
}

/// The production router: `/healthz` open; everything under `/api` behind
/// the full auth → tenant stack.
pub fn router(state: AppState) -> Router {
    // Local password login (KAIROS-T-0203). Mounted only when KAIROS_LOCAL_AUTH is
    // on, and OUTSIDE the auth stack: login holds no credential yet, and logout has
    // to work with a session that has already expired. Off, these routes DO NOT
    // EXIST — a deployment with an issuer has no password endpoint to attack, which
    // is a stronger property than a handler that declines.
    // An EMPTY router when it is off, rather than an Option, so the merge below is
    // unconditional and there is no second assembly path to keep in step.
    let local_auth = if state.config.local_auth {
        crate::login::router()
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
        // first, then tenant — the A-0010 ordering.
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
        .route("/healthz", get(|| async { "ok" }))
        // KAIROS-T-0049 (A-0013): readiness + Prometheus scrape, both
        // unauthenticated by convention and mounted OUTSIDE the auth stack
        // alongside /healthz. Non-/api, so outside the S-0005 surface (and
        // invisible to the openapi route-vs-spec scanner).
        .route("/readyz", get(crate::metrics::readyz))
        .route("/metrics", get(crate::metrics::metrics_handler))
        // Local password login (KAIROS-T-0203), empty unless KAIROS_LOCAL_AUTH is on.
        .merge(local_auth)
        .merge(protected)
        // Cross-tenant deployment-admin routes (KAIROS-T-0019): behind auth
        // only, NO tenant middleware — see api::org::admin module docs.
        .merge(crate::api::org::admin::router(state.clone()))
        // KAIROS-T-0039 (A-0015): the SPA's public /api/config + token
        // relay, and the SPA fallback serving the GUI at `/` (reserved
        // API prefixes still 404) — see crate::web module docs.
        .merge(crate::web::router(&state))
        .fallback(crate::web::spa_fallback)
        // KAIROS-T-0049 (A-0013): the HTTP-metrics layer wraps the WHOLE
        // router (added last → outermost), so every request — matched route
        // or SPA fallback — is timed and counted, and it sees the tenant the
        // inner stack stamped onto the response. Added before `with_state`
        // so the state type is still in scope for `from_fn_with_state`.
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
    /// (KAIROS-T-0105): currently `file_backlog` — create a task against
    /// another team's repository into that team's Backlog.
    pub implicit: Vec<&'static str>,
    /// Repositories owned by the caller's teams (KAIROS-T-0107, A-0019).
    pub repositories: Vec<WhoamiRepository>,
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
    // (idx_board_member_cap_board_user), ordered so grouping is a linear
    // fold and both boards and grants come out sorted. Org admins get only
    // their explicit rows here (usually none — their real access is the
    // org-admin bypass, reported via `organization.role`).
    let grant_rows: Vec<(Uuid, String, String)> = schema::board_member_capabilities::table
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
            schema::board_member_capabilities::capability,
        ))
        .load(&mut *conn)
        .await
        .map_err(ApiError::internal)?;

    let mut capabilities: Vec<WhoamiBoardCapabilities> = Vec::new();
    for (board_id, board_slug, capability) in grant_rows {
        match capabilities.last_mut() {
            Some(last) if last.board_id == board_id => last.grants.push(capability),
            _ => capabilities.push(WhoamiBoardCapabilities {
                board_id,
                board_slug,
                grants: vec![capability],
            }),
        }
    }

    Ok(Json(WhoamiResponse {
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
/// graceful shutdown on ctrl-c. Public migrations already ran (main.rs
/// applies them before any subcommand, KAIROS-T-0007).
pub async fn serve(config: AppConfig) -> Result<(), String> {
    let bind_addr = config.bind_addr;
    let refresh_secs = config.embed_refresh_secs;
    let state = build_state(config).await.map_err(|e| e.to_string())?;

    // The background refresher (KAIROS-T-0190): embedding happens off the write
    // path, so a create never waits on a model. Detached deliberately — it is
    // not part of graceful shutdown, because there is nothing to drain: a sweep
    // interrupted mid-page simply re-derives what is stale on the next pass.
    if let (Some(service), true) = (state.embedding.clone(), refresh_secs > 0) {
        tokio::spawn(crate::embedding::run_refresher(
            state.blocking.clone(),
            service,
            std::time::Duration::from_secs(refresh_secs),
            crate::embedding::REFRESH_BATCH,
        ));
    }

    let app = router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("cannot bind KAIROS_BIND_ADDR {bind_addr}: {e}"))?;
    tracing::info!(%bind_addr, "kairos-server listening");

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
    .map_err(|e| format!("server error: {e}"))
}

/// Resolves when ctrl-c (SIGINT) arrives.
async fn shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "failed to listen for ctrl-c; running until killed");
        std::future::pending::<()>().await;
    }
    tracing::info!("shutdown signal received; draining");
}
