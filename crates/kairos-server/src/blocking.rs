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

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use kairos_db::tenant::{is_valid_slug, tenant_schema_name};
use tracing::Instrument as _;

use crate::error::ApiError;

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
        let invalid = (!is_valid_slug(slug))
            .then(|| format!("invalid tenant slug {slug:?} reached the blocking pool"));
        let schema = tenant_schema_name(slug);
        let pool = self.pool.clone();

        async move {
            if let Some(message) = invalid {
                return Err(ApiError::internal(message));
            }
            tokio::task::spawn_blocking(move || {
                let mut conn = pool.get().map_err(ApiError::internal)?;
                conn.batch_execute(&format!("SET search_path TO \"{schema}\", public"))
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
                conn.batch_execute("SET search_path TO public")
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
