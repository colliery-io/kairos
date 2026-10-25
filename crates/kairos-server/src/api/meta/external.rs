//! `/api/{entity_type}/{short_code}/external` (KAIROS-T-0508,
//! COLLIERY-I-0630): the relationship `external`, from an item to a URI
//! with a label. An RFC in a wiki, a design page, an issue: a document
//! that Kairos does not mirror, and that the tools show next to the
//! Kairos items.
//!
//! # Why these routes and not `/api/relationships`
//!
//! `POST /api/relationships` takes two short codes. A URI is not an item
//! and has no short code. The references are in a table of their own
//! ([`kairos_db::external_refs`]), so the graph view, the traversal of
//! the search and the cascade of an archive do not read them.
//!
//! # The rules
//!
//! The rules are asked in this order, and [`add`] and [`remove`] are the
//! ONE implementation for REST and for the MCP tools `link_items` and
//! `unlink_items`.
//!
//! 1. WHO. The edit rule ([`crate::api::require_item_edit`]): the
//!    principal created the item, or holds `manage_<type>` on its
//!    authorization board, or is an organization admin. The refusal is
//!    403 `FORBIDDEN`.
//! 2. WHICH URI. The scheme is `http`, `https` or `mailto`
//!    ([`kairos_core::external_ref::parse_uri`]). Each other scheme is
//!    422 `VALIDATION`, and the refusal names the scheme. The label is at
//!    most 200 characters.
//! 3. A URI that the item has is 422 `ALREADY_LINKED`, as a duplicate
//!    edge is. The remove of a reference that is not there is 404
//!    `NOT_FOUND`.
//!
//! WHO comes before the URI, so a principal who may not edit the item
//! learns nothing from the answer.
//!
//! Reads are open tenant-wide.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types_meta as dto;
use kairos_core::short_code::ItemType;
use kairos_db::external_refs::{self, ExternalRef, ExternalRefError};
use serde_json::json;
use uuid::Uuid;

use super::resolve_family_item;
use crate::api::convert::external_ref;
use crate::api::{Liveness, manage_capability_of, missing_item_edit, short_code_of};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/{entity_type}/{short_code}/external",
            get(get_external_refs).post(create_external_ref),
        )
        .route(
            "/api/{entity_type}/{short_code}/external/{id}",
            delete(delete_external_ref),
        )
}

/// Rule 1: the edit rule, with a text that says what the reference needs.
fn require_edit(
    conn: &mut PgConnection,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
) -> Result<(), ApiError> {
    let Some(board_id) = missing_item_edit(conn, role, user, item_id, item_type)? else {
        return Ok(());
    };
    let short_code = short_code_of(conn, item_id)?;
    let capability = manage_capability_of(item_type);
    let need = match board_id {
        Some(_) => format!("You need {capability:?} on the board of {short_code}."),
        None => format!("{short_code} has no board."),
    };
    Err(ApiError::forbidden(format!(
        "To change an external reference of {short_code}, you must be able to edit \
         {short_code}. {need} The creator of {short_code} and an organization admin can also \
         change it."
    ))
    .with_details(json!({
        "relationship": external_refs::RELATIONSHIP,
        "required_capability": capability,
        "board_id": board_id,
    })))
}

/// [`ExternalRefError`] → HTTP.
pub(crate) fn map_error(e: ExternalRefError) -> ApiError {
    match e {
        e @ ExternalRefError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        e @ ExternalRefError::NotLinked { .. } => {
            ApiError::not_found(e.to_string()).with_details(json!({ "field": "uri" }))
        }
        ExternalRefError::Form(e) => ApiError::validation(e.to_string()).with_details(match &e {
            kairos_core::external_ref::ExternalRefError::Scheme { scheme } => json!({
                "field": "uri",
                "scheme": scheme,
                "allowed_schemes": kairos_core::external_ref::SCHEMES,
            }),
            kairos_core::external_ref::ExternalRefError::LabelTooLong => json!({
                "field": "label",
                "max_length": kairos_core::external_ref::MAX_LABEL_LEN,
            }),
            _ => json!({ "field": "uri" }),
        }),
        e @ ExternalRefError::ItemNotFound(_) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "short_code" }))
        }
        ExternalRefError::Database(e) => ApiError::internal(e),
    }
}

/// Make the reference "`item` -[external]-> `uri`" as `user`. See the
/// module docs for the rules. The activity row and the event come from
/// [`external_refs::link`].
pub(crate) fn add(
    conn: &mut PgConnection,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    item: (Uuid, ItemType),
    uri: &str,
    label: Option<&str>,
) -> Result<ExternalRef, ApiError> {
    require_edit(conn, role, user, item)?;
    external_refs::link(conn, item.0, uri, label, user).map_err(map_error)
}

/// Remove the reference of `item` with this URI as `user`. See the module
/// docs for the rules. The result is the reference that was removed.
pub(crate) fn remove(
    conn: &mut PgConnection,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    item: (Uuid, ItemType),
    uri: &str,
) -> Result<ExternalRef, ApiError> {
    require_edit(conn, role, user, item)?;
    external_refs::unlink(conn, item.0, uri, user).map_err(map_error)
}

/// The external references of an item (open tenant-wide). The item can be
/// archived: its references stay.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/external",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "The external references of the item, oldest first", body = dto::ItemExternalRefsResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_external_refs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ItemExternalRefsResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            let refs = external_refs::refs_of(conn, item_id).map_err(map_error)?;
            Ok(dto::ItemExternalRefsResponse {
                short_code,
                external: refs.into_iter().map(external_ref).collect(),
            })
        })
        .await?;
    Ok(Json(response))
}

/// Link an item to a URI with a label.
///
/// The edit rule applies (COLLIERY-T-0228): the caller can edit the item.
/// The scheme of the URI is `http`, `https` or `mailto`.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/external",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    request_body = dto::CreateExternalRefRequest,
    responses(
        (status = 201, description = "Reference created (relationship_add activity row written)", body = dto::ExternalRef),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "VALIDATION: the scheme of the URI is not http, https or mailto (the refusal names it), or the label is too long. ALREADY_LINKED: the item has that URI", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_external_ref(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::CreateExternalRefRequest>,
) -> Result<(StatusCode, Json<dto::ExternalRef>), ApiError> {
    let user = auth.user_id;
    let role = tenant.role;
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let made = add(conn, role, user, item, &body.uri, body.label.as_deref())?;
            Ok(external_ref(made))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Remove an external reference by its id.
///
/// The edit rule applies (COLLIERY-T-0228), as for the create.
#[utoipa::path(
    delete,
    path = "/api/{entity_type}/{short_code}/external/{id}",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        ("id" = String, Path, description = "The id of the reference (UUID), from the list"),
    ),
    responses(
        (status = 200, description = "Reference removed (relationship_remove activity row written)", body = dto::DeletedExternalRefResponse),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code, or the item has no reference with that id", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_external_ref(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code, id)): Path<(String, String, String)>,
) -> Result<Json<dto::DeletedExternalRefResponse>, ApiError> {
    let user = auth.user_id;
    let role = tenant.role;
    let deleted = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let not_found = || {
                ApiError::not_found(format!(
                    "{short_code} has no external reference with the id {id:?}."
                ))
                .with_details(json!({ "field": "id" }))
            };
            let ref_id: Uuid = id.parse().map_err(|_| not_found())?;
            let found = external_refs::by_id(conn, item.0, ref_id)
                .map_err(map_error)?
                .ok_or_else(not_found)?;
            let removed = remove(conn, role, user, item, &found.uri)?;
            Ok(dto::DeletedExternalRefResponse {
                short_code,
                uri: removed.uri,
            })
        })
        .await?;
    Ok(Json(deleted))
}
