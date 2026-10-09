//! A small sync (r2d2) pool bridging async handlers to the sync kairos-db
//! services (KAIROS-T-0018).
//!
//! The T-0010/T-0011/T-0012 orchestration services (`kairos_db::{boards,
//! abac, items, graph}`) run on `diesel::PgConnection`, while the request
//! path is async. [`BlockingTenantPool::run`] executes a closure over a
//! pooled sync connection inside `tokio::task::spawn_blocking`, pinning
//! `search_path` to the tenant schema per checkout — the same isolation
//! mechanism as `kairos_db::pool::TenantPool` (every checkout overwrites
//! `search_path` before the closure sees the connection, so a stale path on
//! an idle pooled connection is never observable through this API; the
//! reset after the closure is defense-in-depth).
//!
//! # The agent key of the request (KAIROS-T-0359)
//!
//! Each checkout through [`BlockingTenantPool::run`] also sets the session
//! setting `kairos.agent_key` (see `kairos_db::agent_mark`): the agent key
//! of the request, or the empty string. The column default of
//! `agent_key_id` in `activity_log`, `item_history` and
//! `team_page_history` reads it, so each write of the request gets the
//! mark, and no service takes it as a parameter.
//!
//! Why a tokio task-local and a session setting:
//!
//! - `run` has 130 and more call sites. A task-local ([`with_agent_key`])
//!   carries the key from the tenant middleware to each of them without a
//!   new argument. `run` reads it at the CALL, in the task of the request,
//!   and moves the value into the blocking closure: the threads of the
//!   blocking pool never read the task-local.
//! - The value is set on EACH checkout, in the same statement as
//!   `search_path`, and the empty string when the request has no agent
//!   key. A pooled connection that served a request with an agent key thus
//!   never carries the key to the next request. This is the same rule as
//!   for `search_path`.
//! - A session setting, not `SET LOCAL`: a closure does not always open a
//!   transaction, and `SET LOCAL` outside one has no effect.
//! - A background job on this pool (the code index builder, the embedding
//!   refresher) does not run in the scope of a request. Its checkouts set
//!   the empty string, so its writes get NULL. A task that a handler
//!   starts with `tokio::spawn` does not get the task-local either: also
//!   NULL. A job on a connection of its own (the retention sweep, SCIM)
//!   never sets the setting, and the default reads NULL.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use kairos_db::tenant::{is_valid_tenant_slug, tenant_schema_name};
use tracing::Instrument as _;

use crate::error::ApiError;

tokio::task_local! {
    /// The agent key of the request in scope (KAIROS-T-0359). Set by
    /// [`with_agent_key`]; read by [`BlockingTenantPool::run`].
    static AGENT_KEY: Option<uuid::Uuid>;
}

/// Run `future` with `agent_key` as the agent key of the request: each
/// [`BlockingTenantPool::run`] called in it marks its writes with the key
/// (module docs). The tenant middleware wraps each request in it, and the
/// MCP service wraps each tool call.
pub async fn with_agent_key<F: std::future::Future>(
    agent_key: Option<uuid::Uuid>,
    future: F,
) -> F::Output {
    AGENT_KEY.scope(agent_key, future).await
}

/// The agent key of the request in scope, or `None` outside of a request.
pub fn current_agent_key() -> Option<uuid::Uuid> {
    AGENT_KEY.try_with(|key| *key).ok().flatten()
}

/// The sync pool. Cheap to clone (r2d2 pools are `Arc`s internally).
#[derive(Clone)]
pub struct BlockingTenantPool {
    pool: Pool<ConnectionManager<PgConnection>>,
}

impl std::fmt::Debug for BlockingTenantPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingTenantPool")
            .field("state", &self.pool.state())
            .finish()
    }
}

impl BlockingTenantPool {
    /// Build a lazy pool of at most `max_size` connections against
    /// `database_url`. `build_unchecked` + `min_idle(0)`: connections are
    /// established on first use, so constructing state stays sync and
    /// fail-fast behavior remains with the async pool (KAIROS-T-0017).
    pub fn new(database_url: &str, max_size: u32) -> Self {
        let manager = ConnectionManager::<PgConnection>::new(database_url);
        let pool = Pool::builder()
            .max_size(max_size)
            .min_idle(Some(0))
            .build_unchecked(manager);
        Self { pool }
    }

    /// Run `f` on a sync connection pinned to `slug`'s tenant schema
    /// (`search_path = "org_{slug}", public`), off the async runtime via
    /// `spawn_blocking`. The closure returns `Result<T, ApiError>` so
    /// service errors are mapped to their HTTP codes where the context
    /// lives (in the handler's closure).
    /// KAIROS-T-0199: a `fn` returning a future, not an `async fn`.
    ///
    /// The signature is the instrumentation. `#[track_caller]` on an `async fn`
    /// reports the function's own body rather than the call site — Rust warns
    /// about it (`ungated_async_fn_track_caller`) and it was verified before
    /// relying on it. A plain `fn` gets the real caller, so the span can name the
    /// query by `file:line` **without touching any of the 134 call sites**.
    ///
    /// Callers are unchanged: `pool.run(slug, |conn| …).await` still compiles and
    /// still means the same thing.
    #[track_caller]
    pub fn run<T, F>(
        &self,
        slug: &str,
        f: F,
    ) -> impl std::future::Future<Output = Result<T, ApiError>> + Send + use<T, F>
    where
        F: FnOnce(&mut PgConnection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        let location = std::panic::Location::caller();
        // Created HERE, in the caller's context, which is what makes it a child of
        // the request span from KAIROS-T-0196 rather than an orphan at the root of
        // the trace. Building it inside the async block would parent it to
        // whatever happened to be current when the future was first polled.
        let span = tracing::info_span!(
            "db.query",
            otel.kind = "client",
            db.system = "postgresql",
            // OpenTelemetry's own convention for "where in the code" — so a
            // collector's UI groups by it without configuration.
            code.filepath = location.file(),
            code.lineno = location.line(),
            kairos.tenant = %slug,
        );

        // The tenant middleware already validated the slug; this guard is
        // what makes interpolating the schema name safe by construction.
        let invalid = (!is_valid_tenant_slug(slug))
            .then(|| format!("invalid tenant slug {slug:?} reached the blocking pool"));
        let schema = tenant_schema_name(slug);
        // KAIROS-T-0359: read here, in the task of the request (module docs).
        let agent_key = kairos_db::agent_mark::set_statement(current_agent_key());
        let pool = self.pool.clone();

        async move {
            if let Some(message) = invalid {
                return Err(ApiError::internal(message));
            }
            tokio::task::spawn_blocking(move || {
                let mut conn = pool.get().map_err(ApiError::internal)?;
                conn.batch_execute(&format!(
                    "SET search_path TO \"{schema}\", public; {agent_key}"
                ))
                .map_err(ApiError::internal)?;
                let result = f(&mut conn);
                // Defense-in-depth only — see module docs.
                let _ = conn.batch_execute("SET search_path TO public");
                result
            })
            .await
            .map_err(ApiError::internal)?
        }
        .instrument(span)
    }

    /// Run `f` on a sync connection pinned to `search_path = public` (no
    /// tenant reachable unqualified), off the async runtime via
    /// `spawn_blocking`. Used by the `/readyz` probe (KAIROS-T-0049) for the
    /// pending-migration check, which needs a blocking `PgConnection`
    /// (`diesel_migrations` is sync-only). Checking a connection out also
    /// proves sync-pool database connectivity.
    #[track_caller]
    pub fn run_public<T, F>(
        &self,
        f: F,
    ) -> impl std::future::Future<Output = Result<T, ApiError>> + Send + use<T, F>
    where
        F: FnOnce(&mut PgConnection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        // Same shape as `run` — see its note on why this is a `fn` and not an
        // `async fn`. No tenant field: this path deliberately cannot reach one.
        let location = std::panic::Location::caller();
        let span = tracing::info_span!(
            "db.query",
            otel.kind = "client",
            db.system = "postgresql",
            code.filepath = location.file(),
            code.lineno = location.line(),
        );
        let pool = self.pool.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let mut conn = pool.get().map_err(ApiError::internal)?;
                // KAIROS-T-0359: clear the agent key, as `run` sets it.
                conn.batch_execute(&format!(
                    "SET search_path TO public; {}",
                    kairos_db::agent_mark::set_statement(None)
                ))
                .map_err(ApiError::internal)?;
                f(&mut conn)
            })
            .await
            .map_err(ApiError::internal)?
        }
        .instrument(span)
    }

    /// Point-in-time `(total_connections, idle_connections)` for the r2d2
    /// pool — the connection-pool gauge source for `/metrics`
    /// (KAIROS-T-0049, A-0013). Additive, read-only.
    pub fn pool_state(&self) -> (u32, u32) {
        let state = self.pool.state();
        (state.connections, state.idle_connections)
    }
}
