//! `/api/documents` (KAIROS-S-0005) — see [`super`] for the shared T-0018
//! handler pattern. Documents do not live on boards and have no transition
//! route. A document has an OWNER, which is a board (COLLIERY-T-0269): the
//! board that the document names, or the board of the workflow item that it
//! supports (KAIROS-A-0006).
//!
//! # The owner contract (COLLIERY-T-0269)
//!
//! `POST /api/documents` needs `board`, or `parent_short_code`, or the two.
//! With none of the two the request is refused: 422 `VALIDATION`.
//!
//! - `board` names a live board of each level. The document names that
//!   board as its owner, and `manage_documents` is checked against it. The
//!   document is not a card of the board: it has no column.
//! - `parent_short_code` names a live strategy, initiative, or task: the
//!   document is created and the `supports` edge (parent = source,
//!   document = target, S-0004 orientation) is written by the T-0013 graph
//!   service. With no `board`, `manage_documents` is checked against the
//!   parent's board (the KAIROS-T-0018 contract). An unknown parent, or a
//!   non-workflow parent, is 422 `VALIDATION`. When the parent resolves to
//!   no board (off-board ADR ancestry cannot happen for workflow parents,
//!   but defense-in-depth), the org-admin-only fallback applies.
//! - With the two, the document supports the item and names the board. The
//!   board that it names is its owner.
//!
//! That is the CREATE gate, and COLLIERY-T-0228 did not change it. Each
//! later write to the document (content, lifecycle, archive, an `impacts`
//! link) takes the edit rule ([`super::require_item_edit`]): its creator,
//! or `manage_documents` on the authorization board, or an org admin.
//!
//! The CHANGE OF THE OWNER is not an edit ([`change_board`]). As the move
//! of a task, it needs the capability on the two boards, and the creator
//! of the document gets no right to it.
//!
//! COLLIERY-T-0227: create + link run in ONE transaction
//! ([`super::atomically`]). They were two, on the reasoning that the parent
//! pre-checks made a link failure after the create unreachable. The checks
//! cannot see a parent that is archived after them, nor a database error,
//! and either one left a document with no parent: no board authorizes it,
//! so only an org admin could remove it.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::boards::Board;
use kairos_db::models::enums::RelationshipType;
use kairos_db::models::items::Document;
use kairos_db::{abac, graph, items, repositories};
use serde_json::json;
use uuid::Uuid;

use super::convert::{IntoDto, attach_impact, attach_impacts};
use super::{
    Liveness, atomically, clamp_pagination, map_abac_error, map_graph_error, map_item_error,
    parse_enum, parse_opt_uuid, require_capability, require_edge_write, require_item_edit,
    resolve_short_code, short_code_not_found,
};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family.
const MANAGE: &str = "manage_documents";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/documents", get(list_documents).post(create_document))
        .route(
            "/api/documents/{short_code}",
            get(get_document)
                .patch(update_document)
                .delete(delete_document),
        )
        .route(
            "/api/documents/{short_code}/lifecycle",
            axum::routing::patch(set_lifecycle),
        )
        .route(
            "/api/documents/{short_code}/board",
            axum::routing::patch(set_board),
        )
}

/// A document as the wire type, with its `impacts` links
/// (COLLIERY-T-0269).
fn render(conn: &mut PgConnection, document: Document) -> Result<dto::Document, ApiError> {
    attach_impact(conn, document.into_dto())
}

/// Set a document's editorial lifecycle (KAIROS-T-0078): a free-transition
/// label — draft | review | published | archived.
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// document, holds `manage_documents` on the board of the document, or is an organization admin.
/// The lifecycle is a label and not a column, so this write is an edit and
/// not a move. Not a
/// content edit: no version bump, no history row; activity-logged and
/// announced via the existing `item_updated` thin event.
#[utoipa::path(
    patch,
    path = "/api/documents/{short_code}/lifecycle",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    request_body = dto::SetLifecycleRequest,
    responses(
        (status = 200, description = "Lifecycle updated", body = dto::Document),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Bad lifecycle value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_lifecycle(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::SetLifecycleRequest>,
) -> Result<Json<dto::Document>, ApiError> {
    let lifecycle = parse_enum(
        &body.lifecycle,
        "lifecycle",
        kairos_db::models::enums::DocumentLifecycle::ALL,
    )?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, document.id, ItemType::Document)?;
            let updated = items::set_document_lifecycle(conn, document.id, lifecycle, user)
                .map_err(map_item_error)?;
            render(conn, updated)
        })
        .await?;
    Ok(Json(updated))
}

/// Load the live document with this short code, or 404.
fn load(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<Document, ApiError> {
    use kairos_db::schema::documents::dsl;
    let mut query = dsl::documents
        .filter(dsl::short_code.eq(short_code))
        .into_boxed();
    if liveness == Liveness::LiveOnly {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query
        .select(Document::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("document", short_code))
}

/// List documents (open tenant-wide, S-0005 list envelope).
///
/// `?include_deleted=true` widens the listing to archived work, each row
/// marked with `archived_at` (KAIROS-A-0020 rule 2). Default false: rule 3
/// is that a listing nobody asked hides put-away work. Note this is the
/// `deleted_at` sense of archived, not the editorial `lifecycle` value of
/// the same name (KAIROS-T-0078) — a published document can be
/// editorially archived and perfectly live.
///
/// `?repository=` keeps the documents that impact that repository
/// (COLLIERY-T-0269). The value is the slug or the id of a live
/// repository. An unknown repository is a 422 `VALIDATION`.
#[utoipa::path(
    get,
    path = "/api/documents",
    tag = "documents",
    params(dto::ImpactListQuery),
    responses(
        (status = 200, description = "Page of documents", body = dto::ListEnvelope<dto::Document>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown repository", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_documents(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(query): ApiQuery<dto::ImpactListQuery>,
) -> Result<Json<dto::ListEnvelope<dto::Document>>, ApiError> {
    let (limit, offset, liveness) = clamp_impact_list(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::documents::dsl;
            let impacting = impacting_ids(conn, query.repository.as_deref())?;
            // ONE predicate, applied to both the count and the page
            // (KAIROS-T-0159): the two can never disagree.
            let visible = || {
                let mut query = dsl::documents.into_boxed();
                if liveness == Liveness::LiveOnly {
                    query = query.filter(dsl::deleted_at.is_null());
                }
                if let Some(ids) = &impacting {
                    query = query.filter(dsl::id.eq_any(ids.clone()));
                }
                query
            };
            let total: i64 = visible()
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Document> = visible()
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Document::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut items: Vec<dto::Document> = rows.into_iter().map(IntoDto::into_dto).collect();
            attach_impacts(conn, &mut items)?;
            Ok(dto::ListEnvelope {
                items,
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Clamp the query of the list of documents, or of ADRs, to `(limit,
/// offset, liveness)`: [`super::clamp_list`] for [`dto::ImpactListQuery`]
/// (COLLIERY-T-0269).
pub(crate) fn clamp_impact_list(query: &dto::ImpactListQuery) -> (i64, i64, Liveness) {
    let (limit, offset) = clamp_pagination(&dto::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let liveness = if query.include_deleted {
        Liveness::IncludeArchived
    } else {
        Liveness::LiveOnly
    };
    (limit, offset, liveness)
}

/// The ids of the items that impact the repository of a `?repository=`
/// filter (COLLIERY-T-0269). `None` = the request has no filter. The
/// repository must be live: 422 `VALIDATION` names the one that is not,
/// as the filter of the items of a board does.
pub(crate) fn impacting_ids(
    conn: &mut PgConnection,
    repository: Option<&str>,
) -> Result<Option<Vec<Uuid>>, ApiError> {
    let Some(reference) = repository else {
        return Ok(None);
    };
    let repository =
        repositories::resolve(conn, reference).map_err(super::tasks::map_repository_error)?;
    kairos_db::impacts::item_ids_of_repository(conn, repository.id)
        .map(Some)
        .map_err(ApiError::internal)
}

/// Get one document by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    responses(
        (status = 200, description = "The document", body = dto::Document),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_document(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Document>, ApiError> {
    let document = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code, Liveness::IncludeArchived)?;
            render(conn, document)
        })
        .await?;
    Ok(Json(document))
}

/// The names that a surface gives to the inputs of a document create, for
/// the text of a refusal (COLLIERY-T-0269). REST and the MCP tool
/// `create_item` call one function ([`create`]), and each refusal names
/// the field as the caller sent it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CreateWording {
    /// `request` or `call`.
    pub request: &'static str,
    /// The name of the board input.
    pub board: &'static str,
    /// The name of the parent input.
    pub parent: &'static str,
}

/// The wording of `POST /api/documents`.
pub(crate) const REST_WORDING: CreateWording = CreateWording {
    request: "request",
    board: "board",
    parent: "parent_short_code",
};

/// The inputs of a document create, from each surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewDocument<'a> {
    pub title: &'a str,
    pub content: Option<&'a str>,
    pub template_id: Option<Uuid>,
    /// The owner board, by slug or UUID.
    pub board: Option<&'a str>,
    /// The short code of the item that the document supports.
    pub parent: Option<&'a str>,
}

/// What [`create`] made.
pub(crate) struct CreatedDocument {
    pub document: Document,
    /// The board that the document names, if it names one.
    pub board: Option<Board>,
    /// The short code of the item that the document supports, if one.
    pub parent: Option<String>,
}

/// A live board by slug or UUID, or the 404 of [`super::board_id_by_ref`].
fn board_by_ref(conn: &mut PgConnection, reference: &str) -> Result<Board, ApiError> {
    let id = super::board_id_by_ref(conn, reference)?;
    board_by_id(conn, id)
}

/// A board by id, deleted or not.
fn board_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Board, ApiError> {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::id.eq(id))
        .select(Board::as_select())
        .first(conn)
        .map_err(ApiError::internal)
}

/// `manage_documents` on the board that a document names, or will name
/// (COLLIERY-T-0269). An org admin passes. The refusal is the 403 of
/// [`require_capability`], with the same `details`, and a text that names
/// the board by its slug and says what the board is to the document.
fn require_manage_on_owner(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board: &Board,
    role: &str,
) -> Result<(), ApiError> {
    let refusal = match require_capability(conn, slug, Some(board.id), user, MANAGE) {
        Ok(()) => return Ok(()),
        Err(refusal) => refusal,
    };
    if refusal.code != "FORBIDDEN" {
        return Err(refusal);
    }
    Err(ApiError::forbidden(format!(
        "This action requires the capability {MANAGE:?} on the board {:?}, {role}. You do \
         not have that capability on that board. An organization admin has each \
         capability.",
        board.slug
    ))
    .with_details(refusal.details))
}

/// Create a document as `user`: the ONE implementation of
/// `POST /api/documents` and of the MCP tool `create_item` for a document
/// (COLLIERY-T-0269). The rule and the refusals are here, so the two
/// surfaces cannot disagree. See the module docs for the owner contract.
///
/// The caller runs this in ONE transaction ([`atomically`],
/// COLLIERY-T-0227): the document and its edge, or neither. Each check
/// that can refuse the request runs before the first write.
pub(crate) fn create(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    input: NewDocument<'_>,
    wording: CreateWording,
) -> Result<CreatedDocument, ApiError> {
    let CreateWording {
        request,
        board: board_field,
        parent: parent_field,
    } = wording;
    if input.board.is_none() && input.parent.is_none() {
        return Err(ApiError::validation(format!(
            "The {request} has no {board_field} and no {parent_field}. A document must \
             have an owner. Send {board_field}: the slug or the id of the board that owns \
             the document. Or send {parent_field}: the short code of a strategy, an \
             initiative, or a task that the document supports. You can send the two."
        )));
    }
    let board = input
        .board
        .map(|reference| board_by_ref(conn, reference))
        .transpose()?;
    let parent = input
        .parent
        .map(|parent_code| {
            let (parent_id, parent_type) =
                resolve_short_code(conn, parent_code, Liveness::LiveOnly)?.ok_or_else(|| {
                    ApiError::validation(format!(
                        "The {parent_field} {parent_code:?} is not the short code of a live \
                         item."
                    ))
                })?;
            if !matches!(
                parent_type,
                ItemType::Strategy | ItemType::Initiative | ItemType::Task
            ) {
                return Err(ApiError::validation(format!(
                    "The item {parent_code:?} of {parent_field} has the type {parent_type}. \
                     The parent of a document is a strategy, an initiative, or a task."
                )));
            }
            Ok((parent_code, parent_id, parent_type))
        })
        .transpose()?;

    // THE CREATE GATE. The board that the document names is its owner, so
    // it is the gate. With no board, the gate is the board of the parent,
    // as before COLLIERY-T-0269.
    match (&board, &parent) {
        (Some(board), _) => require_manage_on_owner(
            conn,
            slug,
            user,
            board,
            "which the document names as its owner",
        )?,
        (None, Some((_, parent_id, _))) => {
            let parent_board =
                abac::resolve_authorization_board(conn, *parent_id).map_err(map_abac_error)?;
            require_capability(conn, slug, parent_board, user, MANAGE)?;
        }
        (None, None) => unreachable!("refused above"),
    }

    let created = items::create_document_on_board(
        conn,
        items::CreateDocument {
            title: input.title,
            content: input.content,
            template_id: input.template_id,
        },
        board.as_ref().map(|board| board.id),
        user,
    )
    .map_err(map_item_error)?;
    if let Some((_, parent_id, parent_type)) = parent {
        if board.is_some() {
            // The gate above was about the board and not about the
            // parent. WHO may write the edge is the link rule
            // (COLLIERY-T-0228), as for an ADR with a parent: the caller
            // created the document, so the caller can link it.
            require_edge_write(
                conn,
                slug,
                user,
                RelationshipType::Supports.as_str(),
                (parent_id, parent_type),
                (created.id, ItemType::Document),
            )?;
        }
        graph::link_items(
            conn,
            parent_id,
            created.id,
            RelationshipType::Supports,
            user,
        )
        .map_err(map_graph_error)?;
    }
    Ok(CreatedDocument {
        document: created,
        board,
        parent: parent.map(|(code, ..)| code.to_string()),
    })
}

/// Create a document. It needs an owner: `board`, or `parent_short_code`,
/// or the two (COLLIERY-T-0269, see the module docs).
///
/// With `board`, the caller needs `manage_documents` on that board. With
/// `parent_short_code` and no `board`, the caller needs `manage_documents`
/// on the board of the parent. COLLIERY-T-0228 did not change this gate.
/// With `template_id`, the template's content and metadata
/// defaults are stamped (KAIROS-A-0003).
#[utoipa::path(
    post,
    path = "/api/documents",
    tag = "documents",
    request_body = dto::CreateDocumentRequest,
    responses(
        (status = 201, description = "Created. With parent_short_code, the supports edge is written", body = dto::Document),
        (status = 403, description = "Missing capability on the board that the document names, or on the board of the parent", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = dto::ErrorEnvelope),
        (status = 422, description = "No board and no parent, unknown parent, non-workflow parent, or unknown template", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateDocumentRequest>,
) -> Result<(StatusCode, Json<dto::Document>), ApiError> {
    let template_id = parse_opt_uuid(body.template_id.as_deref(), "template_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            // COLLIERY-T-0227: the document and its edge, or neither.
            let created = atomically(conn, |conn| {
                create(
                    conn,
                    &slug,
                    user,
                    NewDocument {
                        title: &body.title,
                        content: body.content.as_deref(),
                        template_id,
                        board: body.board.as_deref(),
                        parent: body.parent_short_code.as_deref(),
                    },
                    REST_WORDING,
                )
            })?;
            render(conn, created.document)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// What [`change_board`] did.
pub(crate) struct BoardChange {
    /// The document after the call.
    pub document: Document,
    /// The board that the document named before the call.
    pub from: Option<Board>,
    /// The board that the document names after the call.
    pub to: Option<Board>,
    /// The call changed the document. `false` = the document named that
    /// board already, and the call wrote nothing.
    pub changed: bool,
}

/// THE CHANGE OF THE OWNER of a document (COLLIERY-T-0269): set, change or
/// remove the board that the document names. The ONE implementation of
/// `PATCH /api/documents/{short_code}/board` and of the MCP tool
/// `move_item` for a document.
///
/// `board: None` removes the board. The owner of the document is then the
/// board of the earliest item that it supports.
///
/// The function asks two rules, in this order.
///
/// 1. WHO. The change is a MOVE and not an edit, and the rule is that of
///    the move of a task: the principal needs `manage_documents` on the
///    board that answers for the document NOW and on the board that will
///    answer for it AFTER the change. An organization admin passes. The
///    creator of the document gets no right here: creation does not grant
///    movement ([`super::require_item_edit`]). The refusal is 403
///    `FORBIDDEN`.
///
///    The board NOW is the authorization board
///    ([`abac::resolve_authorization_board`]): the board that the document
///    names, or the board of its earliest parent. A document with no
///    board and no parent is old data, and only an organization admin can
///    give it an owner. The board AFTER is the new board, or for a remove
///    the board of the earliest parent.
///
///    WHY the two. With only the board NOW, a manager of one board could
///    give a document to a board whose team did not ask for it. With only
///    the board AFTER, a manager of one board could take each document of
///    the tenant.
///
/// 2. WHICH. A document always has an owner. The remove of the board of a
///    document that supports no item is refused, for each principal, an
///    organization admin too: 422 `LAST_OWNER`. It is a rule of the data
///    and not a permission, as `LAST_PARENT` is.
///
/// WHO comes first, so a principal who may not change the owner learns
/// nothing about what the document supports.
///
/// The board that the document names already is a success that writes
/// nothing. The rule WHO is asked first for that call too.
///
/// The function takes the lock of the document
/// ([`abac::lock_document`]), which the remove of a `supports` edge takes
/// too, and it runs in ONE transaction. So the remove of the last parent
/// and the remove of the board cannot each see the owner that the other
/// removes.
pub(crate) fn change_board(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    document_id: Uuid,
    board: Option<&str>,
) -> Result<BoardChange, ApiError> {
    atomically(conn, |conn| {
        abac::lock_document(conn, document_id).map_err(map_abac_error)?;
        let document = load_by_id(conn, document_id)?;
        let from = document
            .board_id
            .map(|id| board_by_id(conn, id))
            .transpose()?;
        let to = board
            .map(|reference| board_by_ref(conn, reference))
            .transpose()?;
        let parents = abac::document_parents(conn, document_id).map_err(map_abac_error)?;

        // 1. WHO.
        let now = from
            .as_ref()
            .map(|board| board.id)
            .or_else(|| parents.first().map(|parent| parent.board_id));
        let after = to
            .as_ref()
            .map(|board| board.id)
            .or_else(|| parents.first().map(|parent| parent.board_id));
        for (board_id, side) in [(now, "now"), (after, "after")] {
            if side == "after" && (board_id == now || board_id.is_none()) {
                // The same board was asked, or the remove leaves no board:
                // rule 2 refuses that remove.
                continue;
            }
            require_owner_change(conn, slug, user, &document, board_id, side)?;
        }

        // 2. WHICH.
        if to.is_none()
            && from.is_some()
            && !kairos_core::abac::document_board_can_go(parents.len())
        {
            let board = from.as_ref().map(|board| board.slug.clone());
            return Err(ApiError::unprocessable(
                "LAST_OWNER",
                format!(
                    "{} supports no item. A document always has an owner. Link the \
                     document to a work item first, or name a different board.",
                    document.short_code
                ),
            )
            .with_details(json!({
                "document": document.short_code,
                "board": board,
            })));
        }

        let change =
            items::set_document_board(conn, document_id, to.as_ref().map(|board| board.id), user)
                .map_err(map_item_error)?;
        Ok(BoardChange {
            document: change.document,
            from,
            to,
            changed: change.changed,
        })
    })
}

/// Rule 1 of [`change_board`] for one board: `manage_documents` on it.
/// `board_id: None` = the document has no board and no parent, and the
/// organization admin role is what the principal needs.
fn require_owner_change(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    document: &Document,
    board_id: Option<Uuid>,
    side: &str,
) -> Result<(), ApiError> {
    let refusal = match require_capability(conn, slug, board_id, user, MANAGE) {
        Ok(()) => return Ok(()),
        Err(refusal) => refusal,
    };
    if refusal.code != "FORBIDDEN" {
        return Err(refusal);
    }
    let code = &document.short_code;
    let message = match board_id {
        Some(board_id) => {
            let board = board_by_id(conn, board_id)?.slug;
            let which = if side == "now" {
                "the board that owns the document now"
            } else {
                "the new board"
            };
            format!(
                "To change the owner board of {code}, you need {MANAGE:?} on the board \
                 that owns it now and on the new board. You do not have it on the board \
                 {board:?}, {which}. The creator of a document gets no right to change \
                 its owner. An organization admin can change it."
            )
        }
        None => format!(
            "{code} has no owner board and supports no item. Only an organization admin \
             can give it an owner."
        ),
    };
    Err(ApiError::forbidden(message).with_details(refusal.details))
}

/// The live document with this id.
fn load_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Document, ApiError> {
    use kairos_db::schema::documents::dsl;
    dsl::documents
        .filter(dsl::id.eq(id))
        .filter(dsl::deleted_at.is_null())
        .select(Document::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("The document {id} does not exist.")))
}

/// The board of a [`dto::SetDocumentBoardRequest`], or of the argument of
/// a tool: a null and an empty string remove the board.
pub(crate) fn board_to_set(board: Option<&str>) -> Option<&str> {
    board.map(str::trim).filter(|board| !board.is_empty())
}

/// Set, change or remove the owner board of a document (COLLIERY-T-0269).
///
/// This is a move and not an edit. The caller needs `manage_documents` on
/// two boards: the board that owns the document now, and the new board.
/// An organization admin needs no capability. The creator of the document
/// gets no right to change its owner.
///
/// A null or an empty `board` removes the board. After that, the owner is
/// the board of the earliest item that the document supports. The server
/// refuses that request for a document that supports no item: 422
/// `LAST_OWNER`. Link the document to a work item first, or name a
/// different board.
///
/// The board that the document has changes nothing: the response is 200,
/// and the server writes nothing. Not a content edit: no version bump and
/// no history row. The activity log gets one entry with the action
/// `update`.
#[utoipa::path(
    patch,
    path = "/api/documents/{short_code}/board",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    request_body = dto::SetDocumentBoardRequest,
    responses(
        (status = 200, description = "The document, with its owner board", body = dto::Document),
        (status = 403, description = "Missing manage_documents on the board that owns the document now, or on the new board", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code, or unknown board", body = dto::ErrorEnvelope),
        (status = 422, description = "LAST_OWNER: the document supports no item. VALIDATION: the body has no board", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::SetDocumentBoardRequest>,
) -> Result<Json<dto::Document>, ApiError> {
    // A body with no field is an input that does nothing (the rule of the
    // update of a repository, COLLIERY-T-0267).
    let board = body.board.ok_or_else(|| {
        ApiError::validation(
            "The request has no field to change. Send board: the slug or the id of a \
             board, or null to remove the board.",
        )
    })?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code, Liveness::LiveOnly)?;
            let change = change_board(
                conn,
                &slug,
                user,
                document.id,
                board_to_set(board.as_deref()),
            )?;
            render(conn, change.document)
        })
        .await?;
    Ok(Json(updated))
}

/// Update document content (KAIROS-A-0004 optimistic concurrency).
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// document, holds `manage_documents` on the board of the document, or is an organization admin.
/// The board of the document is the board that it names. When it names
/// none, it is the board of the earliest item that it supports
/// (COLLIERY-T-0269).
#[utoipa::path(
    patch,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Document),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateContentRequest>,
) -> Result<Json<dto::Document>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, document.id, ItemType::Document)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Document, document.id, update, user) {
                Ok(_) => {
                    let updated = load(conn, &short_code, Liveness::LiveOnly)?;
                    render(conn, updated)
                }
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code, Liveness::LiveOnly)?;
                    let current = render(conn, current)?;
                    Err(ApiError::conflict(format!(
                        "The request has the version {expected_version}, and the current \
                         version is {current_version}. Get the item again, and make the \
                         edit on the current version."
                    ))
                    .with_details(json!({ "current": current })))
                }
                Err(e) => Err(map_item_error(e)),
            }
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a document.
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// document, holds `manage_documents` on the board of the document, or is an organization admin.
/// The archive removes no `impacts` link, and the restore needs no
/// repair (COLLIERY-T-0269).
#[utoipa::path(
    delete,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::DeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code, Liveness::LiveOnly)?;
            // The edit rule for the document, and then for each descendant
            // (COLLIERY-T-0234): `archive_item` does the two.
            let outcome =
                super::cascade::archive_item(conn, &slug, user, document.id, ItemType::Document)?;
            Ok(super::cascade::delete_response(outcome))
        })
        .await?;
    Ok(Json(outcome))
}
