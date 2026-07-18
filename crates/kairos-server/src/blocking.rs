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
    pub async fn run<T, F>(&self, slug: &str, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&mut PgConnection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        // The tenant middleware already validated the slug; this guard is
        // what makes interpolating the schema name safe by construction.
        if !is_valid_slug(slug) {
            return Err(ApiError::internal(format!(
                "invalid tenant slug {slug:?} reached the blocking pool"
            )));
        }
        let schema = tenant_schema_name(slug);
        let pool = self.pool.clone();
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

    /// Run `f` on a sync connection pinned to `search_path = public` (no
    /// tenant reachable unqualified), off the async runtime via
    /// `spawn_blocking`. Used by the `/readyz` probe (KAIROS-T-0049) for the
    /// pending-migration check, which needs a blocking `PgConnection`
    /// (`diesel_migrations` is sync-only). Checking a connection out also
    /// proves sync-pool database connectivity.
    pub async fn run_public<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&mut PgConnection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().map_err(ApiError::internal)?;
            conn.batch_execute("SET search_path TO public")
                .map_err(ApiError::internal)?;
            f(&mut conn)
        })
        .await
        .map_err(ApiError::internal)?
    }

    /// Point-in-time `(total_connections, idle_connections)` for the r2d2
    /// pool — the connection-pool gauge source for `/metrics`
    /// (KAIROS-T-0049, A-0013). Additive, read-only.
    pub fn pool_state(&self) -> (u32, u32) {
        let state = self.pool.state();
        (state.connections, state.idle_connections)
    }
}
