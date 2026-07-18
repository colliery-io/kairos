//! Inbound SCIM 2.0 provisioning (KAIROS-T-0025, contract per
//! KAIROS-A-0016): per-tenant `/scim/v2/Users` + `/scim/v2/Groups` so
//! enterprise IdPs (Okta, Entra ID, Auth0, …) push user/group lifecycle
//! into Kairos — including the proactive deprovisioning JIT cannot cover.
//!
//! # Mounting and authentication
//!
//! [`router`] mounts `/scim/v2` OUTSIDE the OIDC auth → tenant stack (like
//! the deployment-admin router): SCIM requests authenticate with a
//! per-tenant bearer token (`kairos_scim_<slug>_<64-hex>`, [`auth`] module
//! docs) and the tenant resolves FROM the token, never from the
//! subdomain/X-Tenant. [`tokens_router`] serves the org-admin token
//! management family (`/api/scim-tokens`) and mounts behind the normal
//! OIDC auth → tenant stack.
//!
//! # Identity join contract (A-0016/A-0010: join key = OIDC `sub`)
//!
//! Inbound resources bind to `public.users` in this order:
//!
//! 1. `externalId` == `users.external_id`,
//! 2. `userName`   == `users.external_id`,
//! 3. first email value (or `userName` when it contains `@`) ==
//!    `users.email` — the fallback join. Linking by email RETAINS the
//!    row's existing `external_id` (it is the OIDC `sub` that JIT login
//!    wrote; SCIM must not clobber the login join key),
//! 4. otherwise a new `public.users` row is created with
//!    `external_id = externalId // userName`.
//!
//! **Operations note (per-IdP mapping):** configure the IdP's SCIM app to
//! send the same subject identifier the OIDC tokens carry (`sub`) as
//! `externalId` (preferred) or `userName`. If the IdP sends something else
//! (e.g. Okta's default `userName` = login email) the email fallback still
//! links users who have logged in once, but a SCIM-created user who later
//! logs in with a different `sub` will mint a second `users` row — exactly
//! the drift the mapping note prevents. On the way OUT, `userName` and
//! `externalId` are both served from `users.external_id`.
//!
//! # Resource model: SCIM Users ARE org memberships
//!
//! The tenant's SCIM `Users` resource set is its `organization_members`
//! rows: a User resource exists iff the joined user holds a membership.
//! `id` is the stable `public.users.id` UUID. POST provisions (link-or-
//! create the user + create the membership); PATCH `active: false`, and
//! DELETE, revoke the membership IMMEDIATELY while RETAINING the
//! `public.users` row (audit integrity — history/activity references
//! survive). A deprovisioned user GETs 404 afterwards; re-activation is a
//! fresh POST (documented subset boundary: we keep no "inactive
//! membership" state). Profile attributes (`displayName`, `emails`) update
//! the GLOBAL `public.users` row — users are deployment-wide (A-0001),
//! memberships are the tenant-scoped part.
//!
//! ## Deprovision vs. live tokens (A-0010)
//!
//! Access tokens stay valid until their TTL (local JWKS validation, no
//! introspection), but membership loss takes effect at the NEXT request:
//! the tenant middleware requires an `organization_members` row per
//! request, so a deprovisioned user's still-valid OIDC token gets 403
//! `MEMBERSHIP_REQUIRED` immediately. Integration-tested in
//! `tests/scim.rs`.
//!
//! # Group mapping contract
//!
//! - **`kairos-admins`** (id = the organization UUID): membership in this
//!   group ⇔ `organization_members.role = 'admin'`. Adding a member
//!   promotes (the user must already be provisioned into the org — 400
//!   `invalidValue` otherwise); removing demotes to `member`, guarded by
//!   the LAST_ADMIN rule (an org must retain ≥ 1 admin) which renders as
//!   400 `scimType: "mutability"` (RFC 7644 §3.12: "not compatible with
//!   the … current state"; chosen over 409 `uniqueness`, which is reserved
//!   for duplicate resources).
//! - **`kairos-team-<slug>`** (id = the team UUID): one group per live
//!   team, members ⇔ `team_members`. POST Groups with this naming creates
//!   the team (name = slug, plus its delivery board, mirroring
//!   `/api/teams`); DELETE soft-deletes team + board (refused while the
//!   board holds items). Any other `displayName` is 400 `invalidValue`
//!   naming the convention. Group renames are 400 `mutability`.
//!
//! # RFC subset boundaries (everything else is a typed SCIM error)
//!
//! - Filters: `userName eq "…"` / `externalId eq "…"` (Users) and
//!   `displayName eq "…"` (Groups) — the operators IdPs actually send for
//!   reconciliation. Anything else → 400 `invalidFilter`.
//! - PATCH (Users): `active` (bool, or Entra's `"True"`/`"False"`
//!   strings) and `displayName`, via `add`/`replace` with an explicit path
//!   or a no-path value object (unsupported attributes inside a no-path
//!   value object are IGNORED for IdP compatibility; an unsupported
//!   EXPLICIT path is 400 `invalidPath`). `remove` → 400 `invalidPath`.
//! - PATCH (Groups): `members` add/remove/replace, including the
//!   `members[value eq "…"]` remove form; `displayName` replace-to-same is
//!   a no-op, any rename → 400 `mutability`.
//! - PUT: profile attributes only; `userName`/`externalId`/`displayName`
//!   changes → 400 `mutability`. `active: false` in a PUT deprovisions.
//! - No bulk, no sorting, no ETags, no `/Me` (ServiceProviderConfig says
//!   so); `startIndex`/`count` pagination is supported (1-based, count
//!   clamped to 200).
//!
//! # Activity logging
//!
//! Every lifecycle mutation writes tenant `activity_log` rows with
//! `actor_id` = the token's creator (`scim_tokens.created_by` — SCIM has
//! no user principal) and details prefixed `scim token:<name>`, so IdP-
//! driven changes are attributable to the admin who issued the credential.

pub mod auth;
pub mod discovery;
pub mod error;
pub mod groups;
pub mod tokens;
pub mod users;

use axum::body::Bytes;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Router, middleware as axum_middleware};
use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_db::models::enums::{ActivityAction, OrgRole};
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::{OrganizationMember, User};
use kairos_db::tenant::{is_valid_slug, tenant_schema_name};
use serde_json::Value;
use uuid::Uuid;

pub use auth::ScimContext;
pub use error::ScimError;

use crate::app::AppState;

/// SCIM list-response message URN (RFC 7644 §3.4.2).
pub const LIST_URN: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
/// SCIM PATCH message URN (RFC 7644 §3.5.2).
pub const PATCH_URN: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
/// Core User resource URN (RFC 7643 §4.1).
pub const USER_URN: &str = "urn:ietf:params:scim:core:2.0:User";
/// Core Group resource URN (RFC 7643 §4.2).
pub const GROUP_URN: &str = "urn:ietf:params:scim:core:2.0:Group";

/// The `/scim/v2` router: discovery + Users + Groups, every route behind
/// [`auth::require_scim_token`]. Mounted OUTSIDE the OIDC stack by
/// [`crate::app::router`] (module docs).
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/scim/v2/ServiceProviderConfig",
            get(discovery::service_provider_config),
        )
        .route("/scim/v2/Schemas", get(discovery::schemas))
        .route("/scim/v2/ResourceTypes", get(discovery::resource_types))
        .route(
            "/scim/v2/Users",
            get(users::list_users).post(users::create_user),
        )
        .route(
            "/scim/v2/Users/{id}",
            get(users::get_user)
                .put(users::replace_user)
                .patch(users::patch_user)
                .delete(users::delete_user),
        )
        .route(
            "/scim/v2/Groups",
            get(groups::list_groups).post(groups::create_group),
        )
        .route(
            "/scim/v2/Groups/{id}",
            get(groups::get_group)
                .put(groups::replace_group)
                .patch(groups::patch_group)
                .delete(groups::delete_group),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state,
            auth::require_scim_token,
        ))
}

/// The `/api/scim-tokens` router (org-admin token management). Mounted
/// behind the NORMAL OIDC auth → tenant stack by [`crate::app::router`].
pub fn tokens_router() -> Router<AppState> {
    tokens::router()
}

// ---------------------------------------------------------------------------
// Shared plumbing for the SCIM handlers
// ---------------------------------------------------------------------------

/// Run `f` on a sync connection pinned to `slug`'s tenant schema, off the
/// async runtime. Mirrors [`crate::blocking::BlockingTenantPool::run`] but
/// with [`ScimError`] as the closure error type (the blocking pool is
/// hard-wired to `ApiError`); SCIM traffic is low-rate IdP background
/// sync, so a per-request connection is acceptable (same trade-off as the
/// admin router).
pub(crate) async fn run_scim<T, F>(state: &AppState, slug: &str, f: F) -> Result<T, ScimError>
where
    F: FnOnce(&mut PgConnection) -> Result<T, ScimError> + Send + 'static,
    T: Send + 'static,
{
    // The slug came from a validated ScimContext; this guard is what makes
    // interpolating the schema name safe by construction.
    if !is_valid_slug(slug) {
        return Err(ScimError::internal(format!(
            "invalid tenant slug {slug:?} reached the scim runner"
        )));
    }
    let schema = tenant_schema_name(slug);
    let database_url = state.config.database_url.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = PgConnection::establish(&database_url).map_err(ScimError::internal)?;
        conn.batch_execute(&format!("SET search_path TO \"{schema}\", public"))
            .map_err(ScimError::internal)?;
        f(&mut conn)
    })
    .await
    .map_err(ScimError::internal)?
}

/// Run `f` inside ONE transaction keeping [`ScimError`] as the error type
/// (the `run_in_transaction` pattern from `api::org`, retyped).
pub(crate) fn scim_transaction<T, F>(conn: &mut PgConnection, f: F) -> Result<T, ScimError>
where
    F: FnOnce(&mut PgConnection) -> Result<T, ScimError>,
{
    enum TxError {
        Scim(ScimError),
        Db(diesel::result::Error),
    }
    impl From<diesel::result::Error> for TxError {
        fn from(e: diesel::result::Error) -> Self {
            TxError::Db(e)
        }
    }
    match conn.transaction::<_, TxError, _>(|conn| f(conn).map_err(TxError::Scim)) {
        Ok(value) => Ok(value),
        Err(TxError::Scim(e)) => Err(e),
        Err(TxError::Db(e)) => Err(ScimError::internal(e)),
    }
}

/// Parse a request body as JSON (400 `invalidSyntax` otherwise). SCIM
/// bodies arrive as `application/scim+json` OR `application/json`
/// depending on the IdP, so axum's content-type-strict `Json` extractor is
/// deliberately not used here.
pub(crate) fn parse_json_body(body: &Bytes) -> Result<Value, ScimError> {
    if body.is_empty() {
        return Err(ScimError::invalid_syntax("request body is empty"));
    }
    serde_json::from_slice(body)
        .map_err(|e| ScimError::invalid_syntax(format!("request body is not valid JSON: {e}")))
}

/// One tenant `activity_log` row for a SCIM-driven mutation (see module
/// docs: actor = the token's creator, details prefixed with the token
/// name).
pub(crate) fn log_scim_activity(
    conn: &mut PgConnection,
    ctx_actor: Uuid,
    token_name: &str,
    action: ActivityAction,
    entity_id: Uuid,
    entity_type: &str,
    details: String,
) -> Result<(), ScimError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: ctx_actor,
            action,
            entity_id: Some(entity_id),
            entity_type: Some(entity_type.to_string()),
            details: format!("scim token:{token_name} {details}"),
        })
        .execute(conn)
        .map_err(ScimError::internal)?;
    Ok(())
}

/// The org's membership row for `user_id`, if any.
pub(crate) fn membership_of(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<OrganizationMember>, ScimError> {
    use kairos_db::schema::organization_members::dsl;
    dsl::organization_members
        .filter(dsl::organization_id.eq(org_id))
        .filter(dsl::user_id.eq(user_id))
        .select(OrganizationMember::as_select())
        .first(conn)
        .optional()
        .map_err(ScimError::internal)
}

/// The `public.users` row by id, if any.
pub(crate) fn load_user(conn: &mut PgConnection, user_id: Uuid) -> Result<Option<User>, ScimError> {
    use kairos_db::schema::users;
    users::table
        .filter(users::id.eq(user_id))
        .select(User::as_select())
        .first(conn)
        .optional()
        .map_err(ScimError::internal)
}

/// How many admins the org currently has.
pub(crate) fn admin_count(conn: &mut PgConnection, org_id: Uuid) -> Result<i64, ScimError> {
    use kairos_db::schema::organization_members::dsl;
    dsl::organization_members
        .filter(dsl::organization_id.eq(org_id))
        .filter(dsl::role.eq(OrgRole::Admin))
        .count()
        .get_result(conn)
        .map_err(ScimError::internal)
}

/// The LAST_ADMIN guard as a SCIM error (module docs: 400 `mutability`).
pub(crate) fn last_admin_error() -> ScimError {
    ScimError::mutability(
        "LAST_ADMIN: this organization must retain at least one admin; \
         promote another member before demoting or removing this one",
    )
}

// ---------------------------------------------------------------------------
// List plumbing: eq-filters + startIndex/count pagination (RFC 7644 §3.4.2)
// ---------------------------------------------------------------------------

/// `?filter=`, `?startIndex=`, `?count=` as IdPs send them.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct ListParams {
    pub filter: Option<String>,
    #[serde(rename = "startIndex")]
    pub start_index: Option<i64>,
    pub count: Option<i64>,
}

/// Hard cap on `count` (also advertised in ServiceProviderConfig).
pub(crate) const MAX_COUNT: i64 = 200;

impl ListParams {
    /// `(start_index, count)` per RFC 7644 §3.4.2.4: 1-based start (values
    /// < 1 read as 1), count clamped to `0..=MAX_COUNT`.
    pub(crate) fn page(&self) -> (i64, i64) {
        let start = self.start_index.unwrap_or(1).max(1);
        let count = self.count.unwrap_or(100).clamp(0, MAX_COUNT);
        (start, count)
    }
}

/// Parse the supported filter subset: `attribute eq "value"` (`eq` is
/// case-insensitive per RFC 7644 §3.4.2.2). Returns
/// `(lowercased attribute, value)`; anything else → 400 `invalidFilter`.
pub(crate) fn parse_eq_filter(filter: &str) -> Result<(String, String), ScimError> {
    let unsupported = || {
        ScimError::invalid_filter(format!(
            "unsupported filter {filter:?}: only 'attribute eq \"value\"' is supported"
        ))
    };
    let mut parts = filter.trim().splitn(3, char::is_whitespace);
    let attribute = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(unsupported)?;
    let op = parts.next().ok_or_else(unsupported)?;
    if !op.eq_ignore_ascii_case("eq") {
        return Err(unsupported());
    }
    let value = parts.next().map(str::trim).ok_or_else(unsupported)?;
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or_else(unsupported)?;
    Ok((attribute.to_ascii_lowercase(), value.to_string()))
}

/// A SCIM ListResponse envelope over already-paged resources.
pub(crate) fn list_response(total: i64, start_index: i64, resources: Vec<Value>) -> Value {
    serde_json::json!({
        "schemas": [LIST_URN],
        "totalResults": total,
        "startIndex": start_index,
        "itemsPerPage": resources.len(),
        "Resources": resources,
    })
}

/// Parse a SCIM resource id path segment as a UUID; unknown shapes are 404
/// (the id namespace is UUIDs, so a non-UUID names nothing).
pub(crate) fn parse_resource_id(id: &str) -> Result<Uuid, ScimError> {
    Uuid::parse_str(id).map_err(|_| ScimError::not_found(format!("no resource with id {id:?}")))
}

/// 204 No Content (DELETE success, RFC 7644 §3.6).
pub(crate) fn no_content() -> axum::response::Response {
    use axum::response::IntoResponse;
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_filters_parse_and_everything_else_is_rejected() {
        assert_eq!(
            parse_eq_filter(r#"userName eq "alice""#).unwrap(),
            ("username".to_string(), "alice".to_string())
        );
        assert_eq!(
            parse_eq_filter(r#"externalId EQ "sub|123""#).unwrap(),
            ("externalid".to_string(), "sub|123".to_string())
        );
        for bad in [
            "",
            "userName",
            r#"userName co "ali""#,
            r#"userName eq alice"#,
            r#"userName eq "a" and active eq true"#,
        ] {
            assert!(parse_eq_filter(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn pagination_defaults_and_clamps() {
        let params = ListParams::default();
        assert_eq!(params.page(), (1, 100));
        let params = ListParams {
            start_index: Some(0),
            count: Some(9999),
            ..Default::default()
        };
        assert_eq!(params.page(), (1, MAX_COUNT));
        let params = ListParams {
            start_index: Some(3),
            count: Some(0),
            ..Default::default()
        };
        assert_eq!(params.page(), (3, 0));
    }
}
