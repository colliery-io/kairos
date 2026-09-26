//! Tenant resolution + org-membership enforcement (KAIROS-A-0005 §2,
//! KAIROS-A-0013, KAIROS-T-0017). Runs AFTER [`super::auth`].
//!
//! Resolution order:
//!
//! 1. `KAIROS_SINGLE_TENANT` — fixed slug, skips request inspection
//!    entirely (evaluation installs need no wildcard DNS, A-0013),
//! 2. `Host` subdomain against `KAIROS_BASE_DOMAIN`
//!    (`acme.kairos.example` → `acme`),
//! 3. `X-Tenant` header — the local-development fallback (A-0005 §2).
//!
//! The slug is resolved to `public.organizations` (404 `TENANT_NOT_FOUND`
//! otherwise), the authenticated user must hold an `organization_members`
//! row (403 `MEMBERSHIP_REQUIRED` with a "request access" message
//! otherwise, per A-0010), and handlers receive [`TenantContext`] plus a
//! [`TenantDb`] handle whose connections are pinned to the tenant schema.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use kairos_db::models::{OrgRole, Organization};
use kairos_db::schema::{organization_members, organizations};
use kairos_db::tenant::is_valid_slug;
use kairos_db::{TenantConnection, TenantPool};
use uuid::Uuid;

use tracing::Instrument as _;

use crate::app::AppState;
use crate::config::AppConfig;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;

/// The resolved tenant, inserted as a request extension for every request
/// that passes [`require_tenant`].
#[derive(Debug, Clone)]
pub struct TenantContext {
    /// `public.organizations.id`.
    pub org_id: Uuid,
    /// The organization slug (also the schema suffix: `org_{slug}`).
    pub slug: String,
    /// The caller's role in this organization.
    pub role: OrgRole,
}

/// A tenant-pinned pool handle: connections checked out through it have
/// `search_path = "org_{slug}", public` (see `kairos_db::pool`).
#[derive(Debug, Clone)]
pub struct TenantDb {
    pool: TenantPool,
    slug: String,
}

impl TenantDb {
    /// Check out a connection pinned to this tenant's schema.
    pub async fn conn(&self) -> Result<TenantConnection, ApiError> {
        self.pool
            .tenant(&self.slug)
            .await
            .map_err(ApiError::internal)
    }

    /// The tenant slug this handle is pinned to.
    pub fn slug(&self) -> &str {
        &self.slug
    }
}

/// The host part of a `Host` header value (port stripped; IPv6 literals
/// keep their brackets' contents intact and never match a subdomain).
fn host_without_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.split(':').next().unwrap_or(host)
}

/// Resolve the tenant slug for a request per the module-level order.
/// Pure over `(config, headers)` so it is unit-testable without a server.
pub fn resolve_slug(config: &AppConfig, headers: &HeaderMap) -> Result<String, ApiError> {
    if let Some(slug) = &config.single_tenant {
        return Ok(slug.clone());
    }

    if let Some(base) = &config.base_domain
        && let Some(host) = headers.get("host").and_then(|v| v.to_str().ok())
    {
        let host = host_without_port(host).to_ascii_lowercase();
        if let Some(label) = host.strip_suffix(&format!(".{}", base.to_ascii_lowercase()))
            && !label.is_empty()
            && !label.contains('.')
        {
            return Ok(label.to_string());
        }
    }

    if let Some(slug) = headers.get("x-tenant").and_then(|v| v.to_str().ok()) {
        let slug = slug.trim();
        if !slug.is_empty() {
            return Ok(slug.to_ascii_lowercase());
        }
    }

    Err(ApiError::tenant_not_found(
        "no tenant resolvable from the request \
         (expected a Host subdomain or an X-Tenant header)",
    ))
}

/// The tenant layer: resolve the slug, load the organization, require
/// membership, and hand the request its [`TenantContext`] + [`TenantDb`].
pub async fn require_tenant(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let auth = req
        .extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| ApiError::internal("tenant middleware ran without AuthContext"))?;

    // A service-account API key carries its own tenant (KAIROS-A-0017): pin
    // that slug instead of resolving one from the Host/X-Tenant. Membership is
    // still enforced below, so a key only works on the org it belongs to.
    let slug = match req
        .extensions()
        .get::<crate::service_accounts::auth::ApiKeyTenant>()
    {
        Some(pinned) => pinned.0.clone(),
        None => resolve_slug(&state.config, req.headers())?,
    };
    if !is_valid_slug(&slug) {
        return Err(ApiError::tenant_not_found(format!(
            "{slug:?} is not a valid organization slug"
        )));
    }

    // KAIROS-T-0199: this lookup happens on EVERY request, so without a span it is
    // invisible time between the request starting and the handler's first query.
    // Named for what it is rather than `db.query` — the function does more than
    // one statement, and "tenant.resolve took 4ms" is the useful sentence.
    let tenant_span = tracing::info_span!(
        "tenant.resolve",
        otel.kind = "client",
        db.system = "postgresql",
        kairos.tenant = %slug,
    );
    // Both statements in one span, sharing one connection as they did before.
    // Two spans would measure the same connection checkout twice and suggest a
    // round trip that is not there.
    let (org, role) = async {
        let mut conn = state.pool.public_conn().await.map_err(ApiError::internal)?;
        let org: Option<Organization> = organizations::table
            .filter(organizations::slug.eq(&slug))
            .select(Organization::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(ApiError::internal)?;
        let org = org.ok_or_else(|| {
            ApiError::tenant_not_found(format!("no organization with slug {slug:?}"))
        })?;
        let role: Option<OrgRole> = organization_members::table
            .filter(organization_members::organization_id.eq(org.id))
            .filter(organization_members::user_id.eq(auth.user_id))
            .select(organization_members::role)
            .first(&mut conn)
            .await
            .optional()
            .map_err(ApiError::internal)?;
        let role = role.ok_or_else(|| ApiError::membership_required(&slug))?;
        Ok::<_, ApiError>((org, role))
    }
    .instrument(tenant_span)
    .await?;

    let context = TenantContext {
        org_id: org.id,
        slug: slug.clone(),
        role,
    };
    req.extensions_mut().insert(context.clone());
    req.extensions_mut().insert(TenantDb {
        pool: state.pool.clone(),
        slug,
    });
    // Stamp the resolved tenant onto the RESPONSE so the outer HTTP-metrics
    // layer (KAIROS-T-0049) can attribute the per-tenant counter — request
    // extensions set here are not visible to an outer layer.
    let mut response = next.run(req).await;
    response.extensions_mut().insert(context);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    fn config(single_tenant: Option<&str>, base_domain: Option<&str>) -> AppConfig {
        AppConfig {
            database_url: "postgres://unused".to_string(),
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            oidc_issuer_url: "http://unused".to_string(),
            oidc_audience: "unused".to_string(),
            base_domain: base_domain.map(str::to_string),
            single_tenant: single_tenant.map(str::to_string),
            deployment_admins: vec![],
            log_level: "info".to_string(),
            log_format: crate::config::LogFormat::Json,
            embed_refresh_secs: 0,
            dev_ui: false,
            web_dist: None,
            web_client_id: "kairos-web".to_string(),
            api_bearer: crate::config::ApiBearer::AccessToken,
            web_client_secret: None,
            public_url: None,
            webhook_signing_key: None,
            otel_endpoint: None,
            otel_sample_ratio: 1.0,
            auth_max_failures: 5,
            auth_failure_window_secs: 300,
            auth_lockout_secs: 60,
            trusted_proxy: false,
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                HeaderName::from_bytes(k.as_bytes()).expect("header name"),
                HeaderValue::from_str(v).expect("header value"),
            );
        }
        map
    }

    #[test]
    fn single_tenant_mode_wins_over_everything() {
        let config = config(Some("fixed"), Some("kairos.example"));
        let headers = headers(&[("host", "acme.kairos.example"), ("x-tenant", "other")]);
        assert_eq!(resolve_slug(&config, &headers).unwrap(), "fixed");
    }

    #[test]
    fn host_subdomain_resolves_against_base_domain() {
        let config = config(None, Some("kairos.example"));
        for host in ["acme.kairos.example", "ACME.Kairos.Example:8080"] {
            let headers = headers(&[("host", host), ("x-tenant", "ignored")]);
            assert_eq!(resolve_slug(&config, &headers).unwrap(), "acme", "{host}");
        }
    }

    #[test]
    fn non_matching_hosts_fall_through_to_x_tenant() {
        let config = config(None, Some("kairos.example"));
        for host in [
            "kairos.example",      // bare base domain: no subdomain
            "a.b.kairos.example",  // nested label is not a tenant
            "acme.other.example",  // different domain
            "localhost:8080",      // dev
            "[::1]:8080",          // IPv6 literal
            "kairos.example.evil", // suffix trick
        ] {
            let headers = headers(&[("host", host), ("x-tenant", "Acme")]);
            assert_eq!(resolve_slug(&config, &headers).unwrap(), "acme", "{host}");
        }
    }

    #[test]
    fn x_tenant_is_the_fallback_without_base_domain() {
        let config = config(None, None);
        let headers = headers(&[("host", "acme.kairos.example"), ("x-tenant", "widgets")]);
        assert_eq!(resolve_slug(&config, &headers).unwrap(), "widgets");
    }

    #[test]
    fn unresolvable_requests_are_tenant_not_found() {
        let config = config(None, Some("kairos.example"));
        let err = resolve_slug(&config, &headers(&[("host", "localhost")])).unwrap_err();
        assert_eq!(err.code, "TENANT_NOT_FOUND");
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }
}
