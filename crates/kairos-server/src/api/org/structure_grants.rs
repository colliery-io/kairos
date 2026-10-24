//! `/api/structure-grants` (KAIROS-T-0337, amending KAIROS-A-0006 and
//! KAIROS-A-0017): the grants of the structure capability
//! `manage_structure`.
//!
//! An organization admin grants it to a person or to a service account, at
//! the organization or on one board:
//!
//! - **the organization**: teams, team members, boards, the columns, the
//!   transitions, the members and the grants of each board, and the
//!   repositories ([`crate::api::require_structure`], and
//!   `kairos_db::abac::authorize` for the board capabilities);
//! - **one board**: the columns, the transitions, the members and the
//!   grants of that board. Not new teams, not other boards.
//!
//! The account, key, SCIM and tenant routes stay with the organization
//! admin: a holder of `manage_structure` cannot make an account or a key,
//! and cannot grant `manage_structure`. Only an organization admin writes
//! here. The list is open tenant-wide (the A-0006 read rule: each member can
//! see who can change what).

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_org as dto;
use kairos_db::StructureGrant;
use kairos_db::models::User;
use uuid::Uuid;

use super::super::{map_abac_error, parse_uuid};
use super::{load_board_by_ref, map_grant_error};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/structure-grants",
            get(list_structure_grants).post(create_structure_grant),
        )
        .route(
            "/api/structure-grants/{id}",
            axum::routing::delete(delete_structure_grant),
        )
}

/// The ways in which a person becomes a member of the organization on this
/// deployment (KAIROS-T-0679). The refusal of an unknown member names them
/// when the call adds a person (a team member, a board grant, a structure
/// grant), because the person often has not signed in yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SignInRoutes {
    /// The deployment has an OIDC issuer: a person signs in through the
    /// identity provider, and SCIM can provision people.
    pub identity_provider: bool,
    /// The deployment has local accounts (`KAIROS_LOCAL_AUTH`).
    pub local_accounts: bool,
}

impl SignInRoutes {
    /// The routes of the deployment that `config` describes.
    pub(crate) fn of(config: &crate::config::AppConfig) -> Self {
        Self {
            identity_provider: config.oidc_issuer_url.is_some(),
            local_accounts: config.local_auth,
        }
    }

    /// The sentences that tell how a person becomes a member. Empty when
    /// the deployment has no route.
    fn text(self) -> String {
        let mut routes = Vec::new();
        if self.identity_provider {
            routes.push(
                "the person signs in to Kairos one time through the identity provider, and \
                 then an organization admin adds the person to the organization \
                 (`kairos members add --email <email>`, or POST /api/members)",
            );
            routes.push("the identity provider provisions the person through SCIM (/scim/v2)");
        }
        if self.local_accounts {
            routes.push(
                "an organization admin creates a local account for the person \
                 (POST /api/local-accounts)",
            );
        }
        if routes.is_empty() {
            return String::new();
        }
        let list = routes
            .iter()
            .enumerate()
            .map(|(n, route)| format!("({}) {route}", n + 1))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            " A person becomes a member of the organization in one of these ways: {list}. \
             Then send the reference again."
        )
    }
}

/// The member of the organization that `reference` names: a user id, an
/// email, a user name, or the name of a service account. A person or a
/// service account. `argument` is the name of the field or the argument,
/// for the refusal.
pub(crate) fn resolve_member(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    argument: &str,
) -> Result<User, ApiError> {
    lookup_member(conn, slug, reference, argument, None)
}

/// [`resolve_member`] for a call that adds a person: the refusal of an
/// unknown member also says how a person becomes a member on this
/// deployment (KAIROS-T-0679).
pub(crate) fn resolve_member_to_add(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    argument: &str,
    routes: SignInRoutes,
) -> Result<User, ApiError> {
    lookup_member(conn, slug, reference, argument, Some(routes))
}

fn lookup_member(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    argument: &str,
    routes: Option<SignInRoutes>,
) -> Result<User, ApiError> {
    use kairos_db::schema::users;
    let reference = reference.trim();
    let select = users::table.select(User::as_select());
    let looks_like_email = reference.contains('@');
    let mut candidates: Vec<User> = if let Ok(id) = reference.parse::<Uuid>() {
        select.filter(users::id.eq(id)).load(conn)
    } else if looks_like_email {
        select.filter(users::email.eq(reference)).load(conn)
    } else {
        let by_user_name = select
            .filter(users::user_name.eq(reference))
            .load::<User>(conn)
            .map_err(ApiError::internal)?;
        if by_user_name.is_empty() {
            // A service account has no user name: its name is its display
            // name.
            users::table
                .select(User::as_select())
                .filter(users::display_name.eq(reference))
                .filter(users::kind.eq(kairos_db::models::public::USER_KIND_SERVICE_ACCOUNT))
                .load(conn)
        } else {
            Ok(by_user_name)
        }
    }
    .map_err(ApiError::internal)?;
    let accounts = candidates.len();
    let mut members = Vec::new();
    for user in candidates.drain(..) {
        if kairos_db::abac::is_org_member(conn, slug, user.id).map_err(map_abac_error)? {
            members.push(user);
        }
    }
    match members.len() {
        1 => Ok(members.remove(0)),
        0 => Err(ApiError::validation(not_a_member_text(
            reference,
            looks_like_email,
            accounts > 0,
            routes,
        ))
        .with_details(serde_json::json!({ "field": argument }))),
        _ => Err(ApiError::validation(format!(
            "More than one member of the organization has the name {reference:?}. Send the \
             id of the member."
        ))
        .with_details(serde_json::json!({ "field": argument }))),
    }
}

/// The refusal of a reference that names no member (KAIROS-T-0679). For an
/// email it says whether an account has the email. With `routes` (a call
/// that adds a person) it says how a person becomes a member.
fn not_a_member_text(
    reference: &str,
    looks_like_email: bool,
    has_account: bool,
    routes: Option<SignInRoutes>,
) -> String {
    let mut text = format!(
        "No member of the organization has the id, the email, the user name or the \
         service-account name {reference:?}."
    );
    if looks_like_email {
        if has_account {
            text.push_str(&format!(
                " The account with the email {reference:?} is not a member of the organization."
            ));
        } else {
            text.push_str(&format!(" No account has the email {reference:?} yet."));
        }
    }
    match routes.map(SignInRoutes::text) {
        Some(routes) if !routes.is_empty() => text.push_str(&routes),
        _ => text.push_str(" Send a member of the organization."),
    }
    text
}

/// The DTO of one grant: the principal and the board come from their rows.
fn grant_view(
    conn: &mut PgConnection,
    grant: &StructureGrant,
) -> Result<dto::StructureGrant, ApiError> {
    use kairos_db::schema::{boards, users};
    let user: Option<User> = users::table
        .filter(users::id.eq(grant.user_id))
        .select(User::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    let board_slug: Option<String> = match grant.board_id {
        Some(board_id) => boards::table
            .filter(boards::id.eq(board_id))
            .select(boards::slug)
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?,
        None => None,
    };
    Ok(dto::StructureGrant {
        id: grant.id.to_string(),
        user_id: grant.user_id.to_string(),
        email: user.as_ref().map(|u| u.email.clone()).unwrap_or_default(),
        display_name: user
            .as_ref()
            .map(|u| u.display_name.clone())
            .unwrap_or_default(),
        kind: user.map(|u| u.kind).unwrap_or_default(),
        scope: if grant.board_id.is_some() {
            "board"
        } else {
            "organization"
        }
        .to_string(),
        board_id: grant.board_id.map(|id| id.to_string()),
        board_slug,
        granted_by: grant.granted_by.to_string(),
        granted_at: grant
            .granted_at
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
    })
}

/// The refusal for a principal who is not an organization admin: only an
/// organization admin grants or revokes `manage_structure`, also for a
/// holder of it (KAIROS-T-0337, decision 2).
fn require_org_admin_for_grants(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
) -> Result<(), ApiError> {
    // KAIROS-T-0536: the scope of an agent key must have `manage_structure`.
    if let Some(refusal) = ApiError::outside_key_scope(kairos_core::abac::MANAGE_STRUCTURE, None) {
        return Err(refusal);
    }
    if kairos_db::abac::is_org_admin(conn, slug, user).map_err(map_abac_error)? {
        return Ok(());
    }
    let structure = kairos_core::abac::MANAGE_STRUCTURE;
    Err(ApiError::forbidden(format!(
        "Only an organization admin grants and revokes the capability {structure:?}. A \
         holder of {structure:?} cannot grant it. Ask an organization admin to do this."
    ))
    .with_details(serde_json::json!({
        "required_capability": structure,
        "board_id": null,
        "requires": "organization_admin",
    })))
}

/// Grant `manage_structure` to the member `reference` as `user`, at the
/// organization (`board` `None`) or on the board `board` (slug or UUID): the
/// ONE implementation of `POST /api/structure-grants` and of the MCP tool
/// `grant_capability` with the capability `manage_structure`.
pub(crate) fn grant(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    reference: &str,
    board: Option<&str>,
    routes: SignInRoutes,
) -> Result<dto::StructureGrant, ApiError> {
    require_org_admin_for_grants(conn, slug, user)?;
    let target = resolve_member_to_add(conn, slug, reference, "user", routes)?;
    let board_id = board
        .map(|board| load_board_by_ref(conn, board).map(|b| b.id))
        .transpose()?;
    let grant = kairos_db::abac::grant_structure(conn, target.id, board_id, user)
        .map_err(map_grant_error)?;
    grant_view(conn, &grant)
}

/// List each grant of `manage_structure` (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/structure-grants",
    tag = "structure-grants",
    responses(
        (status = 200, description = "The grants: the grants at the organization first, then the grants on live boards", body = [dto::StructureGrant]),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_structure_grants(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<Vec<dto::StructureGrant>>, ApiError> {
    let grants = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let grants = kairos_db::abac::list_structure_grants(conn).map_err(map_abac_error)?;
            grants.iter().map(|g| grant_view(conn, g)).collect()
        })
        .await?;
    Ok(Json(grants))
}

/// Grant `manage_structure` to a person or a service account, on the
/// organization (no `board`) or on one board. Only an organization admin
/// can grant it. A holder of `manage_structure` cannot grant it. A second
/// grant at the same scope is a 409.
#[utoipa::path(
    post,
    path = "/api/structure-grants",
    tag = "structure-grants",
    request_body = dto::CreateStructureGrantRequest,
    responses(
        (status = 201, description = "Granted", body = dto::StructureGrant),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "The principal has the grant at this scope already", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No member of the organization has the reference", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_structure_grant(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateStructureGrantRequest>,
) -> Result<(StatusCode, Json<dto::StructureGrant>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let routes = SignInRoutes::of(&state.config);
    let grant = state
        .blocking
        .run(&tenant.slug, move |conn| {
            grant(conn, &slug, user, &body.user, body.board.as_deref(), routes)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(grant)))
}

/// Revoke a grant of `manage_structure`. Only an organization admin can
/// revoke it.
#[utoipa::path(
    delete,
    path = "/api/structure-grants/{id}",
    tag = "structure-grants",
    params(("id" = String, Path, description = "The id of the grant (UUID)")),
    responses(
        (status = 200, description = "Revoked; the body is the grant that was revoked", body = dto::StructureGrant),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown grant", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_structure_grant(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::StructureGrant>, ApiError> {
    let grant_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let revoked = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_org_admin_for_grants(conn, &slug, user)?;
            let grant =
                kairos_db::abac::revoke_structure(conn, grant_id, user).map_err(map_grant_error)?;
            grant_view(conn, &grant)
        })
        .await?;
    Ok(Json(revoked))
}

#[cfg(test)]
mod t0679_tests {
    //! KAIROS-T-0679: the refusal of a reference that names no member.
    use super::{SignInRoutes, not_a_member_text};

    const BOTH: SignInRoutes = SignInRoutes {
        identity_provider: true,
        local_accounts: true,
    };

    #[test]
    fn an_email_with_no_account_names_each_route() {
        let text = not_a_member_text("sam@example.com", true, false, Some(BOTH));
        assert_eq!(
            text,
            "No member of the organization has the id, the email, the user name or the \
             service-account name \"sam@example.com\". No account has the email \
             \"sam@example.com\" yet. A person becomes a member of the organization in one \
             of these ways: (1) the person signs in to Kairos one time through the identity \
             provider, and then an organization admin adds the person to the organization \
             (`kairos members add --email <email>`, or POST /api/members); (2) the identity \
             provider provisions the person through SCIM (/scim/v2); (3) an organization \
             admin creates a local account for the person (POST /api/local-accounts). Then \
             send the reference again."
        );
    }

    #[test]
    fn local_accounts_only() {
        let routes = SignInRoutes {
            identity_provider: false,
            local_accounts: true,
        };
        let text = not_a_member_text("sam@example.com", true, true, Some(routes));
        assert!(
            text.contains("The account with the email \"sam@example.com\" is not a member"),
            "{text}"
        );
        assert!(
            text.contains("(1) an organization admin creates a local account"),
            "{text}"
        );
        assert!(!text.contains("SCIM"), "{text}");
    }

    #[test]
    fn a_lookup_that_adds_nobody_keeps_the_short_text() {
        assert_eq!(
            not_a_member_text("robin", false, false, None),
            "No member of the organization has the id, the email, the user name or the \
             service-account name \"robin\". Send a member of the organization."
        );
        // No route on the deployment: the short text too.
        assert!(
            not_a_member_text("robin", false, false, Some(SignInRoutes::default()))
                .ends_with("Send a member of the organization.")
        );
    }
}
