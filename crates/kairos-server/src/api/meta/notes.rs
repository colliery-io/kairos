//! `POST /api/{entity_type}/{short_code}/notes` (KAIROS-T-0367): append a
//! note to an item, in one call, with no version. Since KAIROS-T-0630 a
//! note is a row of `item_notes`, not part of the content.
//!
//! One wildcard route for the five families, as `/restore` and `/history`.
//! The gate is the edit rule (COLLIERY-T-0228), as for each content edit.
//! The write is [`kairos_db::item_notes::append_note`]: it locks the item
//! row, so two notes at the same time both stay, each with its own id.

use axum::Router;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use kairos_client::types as dto;

use super::resolve_family_item;
use crate::api::{Liveness, map_note_error, short_code_of};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/{entity_type}/{short_code}/notes", post(append_note))
        .route(
            "/api/{entity_type}/{short_code}/notes/{note_id}/promote",
            post(promote_note),
        )
}

/// The wording of `POST /api/{entity_type}/{short_code}/notes/{note_id}/promote`.
const PROMOTE_WORDING: crate::api::documents::CreateWording =
    crate::api::documents::CreateWording {
        request: "request",
        details_key: "field",
        board: "board",
        parent: "board",
    };

/// Turn a note into a draft ADR or document (KAIROS-T-0505). The new item
/// has the text of the note. A line names the note, its author and its
/// date. It `supports` the source item when the source is a
/// strategy, an initiative or a task. The note gets `promoted_to`: the
/// code of the new item. Kairos promotes a note one time only: a second
/// promote gets 409 CONFLICT, which names the item.
///
/// The edit rule of the source item applies. The caller needs the right
/// to create on the target board.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/notes/{note_id}/promote",
    tag = "history",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        ("note_id" = String, Path, description = "The id of the note, as `get_item` lists it (n-3)"),
        ("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC),
    ),
    request_body = dto::PromoteNoteRequest,
    responses(
        (status = 201, description = "The new item and its edge", body = dto::PromoteNoteResponse),
        (status = 403, description = "Refused by the edit rule of the source item, or the caller cannot create on the target board", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown family, short code, note id or board", body = dto::ErrorEnvelope),
        (status = 409, description = "The note has the mark of a promote; the refusal names the item; or REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = dto::ErrorEnvelope),
        (status = 422, description = "The kind is not adr or document, or the body has an unknown field", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn promote_note(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code, note_id)): Path<(String, String, String)>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::PromoteNoteRequest>,
) -> Result<crate::api::idempotency::Answer<dto::PromoteNoteResponse>, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let role = tenant.role;
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            crate::api::idempotency::run(conn, keyed, StatusCode::CREATED, |conn| {
                let (item_id, item_type) =
                    resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
                let source_code = short_code_of(conn, item_id)?;
                let promoted = crate::api::atomically(conn, |conn| {
                    crate::promote::promote_note(
                        conn,
                        &slug,
                        role,
                        user,
                        crate::promote::PromoteInput {
                            source: (item_id, item_type, &source_code),
                            note_id: &note_id,
                            kind: &body.kind,
                            board: &body.board,
                            title: body.title.as_deref(),
                        },
                        PROMOTE_WORDING,
                    )
                })?;
                Ok(dto::PromoteNoteResponse {
                    short_code: promoted.source_code,
                    note_id: promoted.note_id,
                    kind: promoted.kind.to_string(),
                    promoted_to: promoted.created_code,
                    supports: promoted.supports,
                })
            })
        })
        .await?;
    Ok(response)
}

/// Append a note to an item. The note gets the next id (`n-3`), the time
/// and the caller as its author. The text is markdown, and it can have
/// headings. The note writes no version of the content: the read of the
/// item shows it in `notes`.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/notes",
    tag = "history",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        ("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC),
    ),
    request_body = dto::AppendNoteRequest,
    responses(
        (status = 200, description = "Appended; the id of the note and the version of the content, which does not change", body = dto::AppendNoteResponse),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code, or the item is archived", body = dto::ErrorEnvelope),
        (status = 409, description = "REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = dto::ErrorEnvelope),
        (status = 422, description = "The text is empty, or the body has an unknown field", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn append_note(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::AppendNoteRequest>,
) -> Result<crate::api::idempotency::Answer<dto::AppendNoteResponse>, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let user = auth.user_id;
    let role = tenant.role;
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            crate::api::idempotency::run(conn, keyed, StatusCode::OK, |conn| {
                let (item_id, item_type) =
                    resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
                crate::api::require_item_edit(conn, role, user, item_id, item_type)?;
                let appended =
                    kairos_db::item_notes::append_note(conn, item_type, item_id, &body.text, user)
                        .map_err(map_note_error)?;
                Ok(dto::AppendNoteResponse {
                    short_code: short_code_of(conn, item_id)?,
                    version: appended.version,
                    note_id: appended.note_id,
                })
            })
        })
        .await?;
    Ok(response)
}
