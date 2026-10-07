//! The tenants whose schema could not migrate at startup (KAIROS-T-0330,
//! the 2026-10-07 amendment of KAIROS-A-0009).
//!
//! `serve` runs the tenant migrations of each tenant before it serves. A
//! tenant whose migration fails does not stop the server: the server
//! starts, and this set refuses that tenant's requests with 503
//! `TENANT_NOT_READY` (in [`crate::middleware::tenant::require_tenant`]).
//!
//! The set re-checks a refused tenant at most once every
//! [`RECHECK_INTERVAL`]: when an operator repairs the schema and runs
//! `kairos-server migrate-tenants`, the tenant answers again with no restart.
//! A check only reads (`has_pending_migration`); it never migrates.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use kairos_db::TenantMigrationReport;

use crate::app::AppState;
use crate::error::ApiError;

/// How often a refused tenant is checked again.
pub const RECHECK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
struct NotReady {
    /// Why the migration failed, from the startup run.
    reason: String,
    /// When the schema was last checked.
    checked: Instant,
}

/// The tenants that are not ready. Empty on a healthy deployment, so the
/// check of a request is one read of an empty map.
#[derive(Debug, Default)]
pub struct TenantReadiness {
    not_ready: RwLock<HashMap<String, NotReady>>,
    interval: Option<Duration>,
}

impl TenantReadiness {
    /// The set from the startup migration: each tenant whose result is an
    /// error.
    pub fn from_reports(reports: &[TenantMigrationReport]) -> Self {
        let set = Self::default();
        for report in reports {
            if let Err(reason) = &report.result {
                set.mark(&report.slug, reason);
            }
        }
        set
    }

    /// The same set with a different re-check interval (tests).
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
        self
    }

    /// Refuse the requests of `slug` until its schema is current.
    pub fn mark(&self, slug: &str, reason: &str) {
        self.not_ready.write().expect("the readiness lock").insert(
            slug.to_string(),
            NotReady {
                reason: reason.to_string(),
                checked: Instant::now(),
            },
        );
    }

    /// The tenants that are not ready, with the reason, by slug.
    pub fn not_ready(&self) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> = self
            .not_ready
            .read()
            .expect("the readiness lock")
            .iter()
            .map(|(slug, entry)| (slug.clone(), entry.reason.clone()))
            .collect();
        list.sort();
        list
    }

    /// `Ok` when the tenant `slug` can serve; else 503 `TENANT_NOT_READY`.
    /// A refused tenant whose last check is older than the interval is
    /// checked again first.
    pub async fn require_ready(&self, state: &AppState, slug: &str) -> Result<(), ApiError> {
        let entry = self
            .not_ready
            .read()
            .expect("the readiness lock")
            .get(slug)
            .cloned();
        let Some(entry) = entry else {
            return Ok(());
        };
        if entry.checked.elapsed() >= self.interval.unwrap_or(RECHECK_INTERVAL) {
            let owned = slug.to_string();
            let current = state
                .blocking
                .run_public(move |conn| {
                    kairos_db::tenant_schema_is_current(conn, &owned).map_err(ApiError::internal)
                })
                .await
                .unwrap_or(false);
            let mut map = self.not_ready.write().expect("the readiness lock");
            if current {
                map.remove(slug);
                tracing::info!(tenant = %slug, "the tenant schema is current again: it serves");
                return Ok(());
            }
            if let Some(stored) = map.get_mut(slug) {
                stored.checked = Instant::now();
            }
        }
        Err(not_ready_error(slug, &entry.reason))
    }
}

/// The 503 of a tenant that is not ready.
fn not_ready_error(slug: &str, reason: &str) -> ApiError {
    ApiError::tenant_not_ready(format!(
        "The data of the organization {slug:?} is not ready: its migration failed at \
             startup. An operator must repair it and run kairos-server migrate-tenants. The \
             error was: {reason}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(slug: &str, result: Result<Vec<String>, String>) -> TenantMigrationReport {
        TenantMigrationReport {
            slug: slug.into(),
            schema: format!("org_{slug}"),
            result,
        }
    }

    #[test]
    fn only_a_failed_tenant_is_not_ready() {
        let set = TenantReadiness::from_reports(&[
            report("acme", Ok(vec!["20261006000000".into()])),
            report("broken", Err("relation x does not exist".into())),
            report("calm", Ok(vec![])),
        ]);
        assert_eq!(
            set.not_ready(),
            [(
                "broken".to_string(),
                "relation x does not exist".to_string()
            )]
        );
    }

    #[test]
    fn the_error_names_the_tenant_and_the_repair() {
        let error = not_ready_error("broken", "boom");
        assert_eq!(error.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.code, "TENANT_NOT_READY");
        let text = error.message;
        assert!(text.contains("\"broken\""), "{text}");
        assert!(text.contains("kairos-server migrate-tenants"), "{text}");
        assert!(text.contains("boom"), "{text}");
    }
}
