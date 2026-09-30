//! The KAIROS-S-0005 entity endpoint families (KAIROS-T-0018): strategies,
//! initiatives, tasks, documents, and ADRs — list/get/create/PATCH/DELETE
//! (+ transition for board items), wired to the T-0012 write services and
//! T-0010 transition services with A-0006 ABAC on every write.
//!
//! # The handler/module pattern (what the remaining endpoint tasks copy)
//!
//! One module per resource family, each exporting `router()`; this module
//! merges them and owns the shared plumbing:
//!
//! - request DTOs come from [`kairos_client::types`] (the shared wire
//!   types, A-0015); conversions from `kairos-db` models live in
//!   [`convert`];
//! - every write runs inside ONE [`crate::blocking::BlockingTenantPool`]
//!   closure: resolve the short code, [`require_capability`], call the
//!   kairos-db service, convert to the DTO — so the ABAC check and the
//!   write use the same tenant-pinned connection;
//! - reads are open tenant-wide (A-0006): list/get do no capability check
//!   beyond the middleware stack;
//! - service errors map to the S-0005 envelope here (`map_*_error`), NOT
//!   in kairos-db: 404 for unknown short codes, 409 `CONFLICT` with
//!   `details.current` (the handler reloads the entity), 422
//!   `INVALID_TRANSITION` with `details.allowed_targets`, 422
//!   `ITEM_NOT_ON_BOARD`, 422 `VALIDATION` for malformed/unknown body
//!   references, 403 via [`crate::error::ApiError::capability_required`];
//! - every handler carries a `#[utoipa::path]` annotation; the OpenAPI
//!   aggregation endpoint is KAIROS-T-0023.

pub mod adrs;
// KAIROS-T-0051: pre-delete cascade preview (generic {entity_type} read).
pub mod cascade;
pub mod convert;
pub mod convert_meta;
pub mod convert_org;
pub mod documents;
pub mod initiatives;
pub mod local_accounts;
pub mod meta;
pub mod org;
pub mod proposals;
pub mod search;
pub mod strategies;
pub mod tasks;
// KAIROS-T-0023: OpenAPI aggregation (/api/openapi.json) + dev Swagger UI.
pub mod openapi;

use axum::Router;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use kairos_client::types as dto;
use kairos_core::board::TransitionError;
use kairos_core::short_code::ItemType;
use kairos_db::models::enums::RelationshipType;
use kairos_db::{AbacError, BoardError, GraphError, ItemError, abac};
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

/// All five entity family routers, merged (mounted behind the full
/// auth → tenant middleware stack in [`crate::app::router`]).
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(strategies::router())
        .merge(initiatives::router())
        .merge(tasks::router())
        .merge(documents::router())
        .merge(adrs::router())
        // KAIROS-T-0051: GET /api/{entity_type}/{short_code}/cascade-preview.
        .merge(cascade::router())
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Default page size when `?limit=` is omitted.
const DEFAULT_LIMIT: i64 = 50;
/// Hard cap on `?limit=`.
const MAX_LIMIT: i64 = 200;

/// Clamp raw S-0005 pagination params to `(limit, offset)`.
pub fn clamp_pagination(pagination: &dto::Pagination) -> (i64, i64) {
    let limit = pagination
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let offset = pagination.offset.unwrap_or(0).max(0);
    (limit, offset)
}

/// Clamp an entity-family list query to `(limit, offset, liveness)`.
///
/// The liveness comes back beside the page bounds rather than being read
/// separately at each call site, because the one way to get KAIROS-T-0159
/// wrong is for the `count` and the paged `load` to disagree about which
/// rows exist. One value, used twice, cannot.
pub fn clamp_list(query: &dto::ListQuery) -> (i64, i64, Liveness) {
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

// ---------------------------------------------------------------------------
// Body parsing helpers (wire strings → typed values, 422 VALIDATION)
// ---------------------------------------------------------------------------

/// Parse a UUID body field (`422 VALIDATION` on malformed input — the DTO
/// crate carries ids as strings, see `kairos_client::types`).
pub fn parse_uuid(value: &str, field: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value)
        .map_err(|_| ApiError::validation(format!("The value {value:?} of {field} is not a UUID.")))
}

/// [`parse_uuid`] over an optional field.
pub fn parse_opt_uuid(value: Option<&str>, field: &str) -> Result<Option<Uuid>, ApiError> {
    value.map(|v| parse_uuid(v, field)).transpose()
}

/// Resolve a board reference — **slug or UUID** — to a live board's id
/// (KAIROS-T-0150).
///
/// Every other board reference a person types already accepts both: `tasks move
/// --to-board`, `--repo` on create and search, and the MCP `board_items` /
/// `move_item` tools. The create endpoints took a UUID only, so the one command
/// someone is most likely to type first was the one that sent them to look up an
/// id.
///
/// Resolved server-side rather than in the CLI, deliberately: one rule, in one
/// place, for the CLI, the REST callers and MCP alike. Resolving it in the CLI
/// would have kept the wire type strict at the cost of leaving the REST API with
/// the wart and every other client to reimplement the lookup.
///
/// An unknown reference is a 404 naming it, not a UUID parse error — the
/// difference between "no board called that" and "that is not a UUID" is the
/// whole point for someone who typed a slug on purpose.
pub fn board_id_by_ref(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError> {
    use kairos_db::schema::boards::dsl;
    let mut query = dsl::boards
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .into_boxed();
    query = match Uuid::parse_str(reference) {
        Ok(id) => query.filter(dsl::id.eq(id)),
        Err(_) => query.filter(dsl::slug.eq(reference)),
    };
    query
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "No live board has the slug or the id {reference:?}."
            ))
        })
}

/// [`board_id_by_ref`] for an optional reference.
pub fn opt_board_id_by_ref(
    conn: &mut PgConnection,
    reference: Option<&str>,
) -> Result<Option<Uuid>, ApiError> {
    reference.map(|r| board_id_by_ref(conn, r)).transpose()
}

/// Parse a TEXT-backed enum body field (`task_type`, `complexity`,
/// `bucket_type`) via its `FromStr`, naming the allowed values on failure.
pub fn parse_enum<T>(value: &str, field: &str, allowed: &[T]) -> Result<T, ApiError>
where
    T: std::str::FromStr + std::fmt::Display + Copy,
{
    value.parse::<T>().map_err(|_| {
        let allowed: Vec<String> = allowed.iter().map(|v| v.to_string()).collect();
        ApiError::validation(format!(
            "The value {value:?} is not a value of {field}. The values are: {}.",
            allowed.join(", ")
        ))
    })
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

/// Why an [`atomically`] transaction stopped: the closure refused, or
/// diesel could not begin or commit.
enum Abort {
    Refused(ApiError),
    Database(diesel::result::Error),
}

impl From<diesel::result::Error> for Abort {
    fn from(e: diesel::result::Error) -> Self {
        Abort::Database(e)
    }
}

/// Run `f` in ONE database transaction: each write in it is committed, or
/// none is (COLLIERY-T-0227).
///
/// The kairos-db services each run "in their own transaction". A handler
/// that calls two of them made two commits, and an error from the second
/// left the first in the tenant: a create that was refused for its edge
/// left the item. Inside this function the transaction of a service is a
/// savepoint, so an `Err` from `f` rolls back all of them.
///
/// Events are safe in here. `emit_event` is a `pg_notify`, and PostgreSQL
/// delivers a notification when its transaction commits, so a create that
/// is rolled back sends none. Short code sequences are not transactional: a
/// number that a refused create took stays taken, which is correct.
pub fn atomically<T>(
    conn: &mut PgConnection,
    f: impl FnOnce(&mut PgConnection) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    conn.transaction::<T, Abort, _>(|conn| f(conn).map_err(Abort::Refused))
        .map_err(|abort| match abort {
            Abort::Refused(e) => e,
            Abort::Database(e) => ApiError::internal(e),
        })
}

// ---------------------------------------------------------------------------
// ABAC (KAIROS-A-0006)
// ---------------------------------------------------------------------------

/// The A-0006 write gate: org admins bypass; otherwise the caller needs a
/// matching `board_member_capabilities` grant on `board_id`. `board_id:
/// None` = no board context exists (off-board ADR, document with no
/// resolvable parent board) → the org-admin-only fallback. Denial is 403
/// with the missing capability named in `details`.
pub fn require_capability(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Option<Uuid>,
    user_id: Uuid,
    capability: &str,
) -> Result<(), ApiError> {
    let allowed = match board_id {
        Some(board_id) => abac::authorize(conn, slug, board_id, user_id, capability),
        None => abac::is_org_admin(conn, slug, user_id),
    }
    .map_err(map_abac_error)?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::capability_required(capability, board_id))
    }
}

/// What a principal lacks to edit one item: the `manage_<type>` capability
/// of its type, on its authorization board. `board_id: None` = the item has
/// no authorization board, and what is missing is the admin role.
struct MissingEdit {
    capability: &'static str,
    board_id: Option<Uuid>,
}

/// The edit rule for one item, as a value: `None` = the principal may edit
/// it. The shared core of [`require_item_edit`] and [`require_edge_write`],
/// so the two read the same facts and decide with the same function
/// ([`kairos_core::abac::may_edit_item`]).
fn missing_edit(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<Option<MissingEdit>, ApiError> {
    let capability = meta::manage_capability(item_type);
    let (facts, board_id) =
        abac::edit_facts(conn, slug, user_id, item_id, capability).map_err(map_abac_error)?;
    Ok(if kairos_core::abac::may_edit_item(facts) {
        None
    } else {
        Some(MissingEdit {
            capability,
            board_id,
        })
    })
}

/// The edit rule as a value, for a caller that writes its own refusal
/// (COLLIERY-T-0269): `None` = the principal may edit the item.
/// `Some(board)` = it may not, and `board` is the authorization board of
/// the item, if it has one. The decision is that of [`require_item_edit`].
pub(crate) fn missing_item_edit(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<Option<Option<Uuid>>, ApiError> {
    Ok(missing_edit(conn, slug, user_id, item_id, item_type)?.map(|missing| missing.board_id))
}

/// The `manage_<type>` capability of an item type, the capability of the
/// edit rule.
pub(crate) fn manage_capability_of(item_type: ItemType) -> &'static str {
    meta::manage_capability(item_type)
}

/// THE EDIT RULE (COLLIERY-T-0228): may this principal edit this item?
///
/// A principal, a person or a service account, may edit an item when ONE
/// of these is true:
///
/// 1. the principal created the item (`created_by`),
/// 2. the principal holds `manage_<type>` on the authorization board of
///    the item ([`abac::resolve_authorization_board`]: the board of the
///    item, or for a document the board that it names, or the board of
///    its `supports` parent when it names none, COLLIERY-T-0269),
/// 3. the principal is an admin of the organization.
///
/// An `impacts` link gives no right (COLLIERY-T-0269). A member of the
/// team that owns a repository cannot edit a document because the
/// document impacts that repository.
///
/// WHY. Creation is the primary mechanism of ownership: the person who
/// wrote an item can correct it. A capability on a board is how a team
/// shares that ownership. Until COLLIERY-T-0228 only rules 2 and 3
/// existed, so a person who sent a request to a different team could not
/// correct a wrong word in it.
///
/// The rule applies to each item type: strategy, initiative, task,
/// document, ADR. It reads who created the item and not where the item is,
/// so the right stays with the creator when the item moves to a different
/// board.
///
/// AN EDIT IS: the title and the content, the metadata, the repository of
/// a task, the `impacts` links of a document or of an ADR
/// (COLLIERY-T-0269), the editorial lifecycle of a document, archive, and
/// restore. Each REST handler and each MCP tool for those writes calls
/// this function, and no other check.
///
/// CREATION DOES NOT GRANT MOVEMENT. `transition`, `work-class`, `move`
/// and the change of the owner board of a document
/// ([`documents::change_board`]) do not call this function. They call [`require_capability`], and the
/// creator of an item gets nothing there. A team controls its own plan
/// (COLLIERY-T-0218, COLLIERY-A-0023): the person who sends a request
/// cannot move it out of the entry column, cannot put it in the planned
/// lane, and cannot move it to a different board. Creation grants nothing
/// on a board, a team, a member, a capability, a repository or the
/// configuration of the tenant. Who may CREATE an item does not change
/// either.
///
/// The refusal is the 403 of [`require_capability`]: it names the
/// `manage_<type>` capability that the principal does not hold, and the
/// board.
pub fn require_item_edit(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<(), ApiError> {
    match missing_edit(conn, slug, user_id, item_id, item_type)? {
        None => Ok(()),
        Some(missing) => Err(ApiError::capability_required(
            missing.capability,
            missing.board_id,
        )),
    }
}

/// One end of an edge: the id and the type of the item.
pub type EdgeEnd = (Uuid, ItemType);

/// THE LINK RULE (COLLIERY-T-0228): may this principal write this edge?
///
/// A principal may create or remove an edge when the principal may EDIT
/// the item at EITHER end, by the edit rule ([`require_item_edit`]): the
/// creator of the source or of the target, or `manage_<type>` on the
/// authorization board of the source or of the target, or an admin of the
/// organization.
///
/// The rule is the same for each relationship type: `parent`, `blocks`,
/// `supports`, `informs`, `supersedes`. No relationship type needs the
/// admin role. Until COLLIERY-T-0228 `supports`, `informs` and
/// `supersedes` did, and `parent` and `blocks` looked at the creator of
/// the source only (KAIROS-T-0111). Each edge that the old rule allowed,
/// this rule allows.
///
/// WHY either end. An edge is a statement about two items, and the two are
/// frequently on the boards of two teams. If the rule needed the two ends,
/// no person could link work across teams, which is what edges are for.
///
/// The rule decides WHO. It does not decide WHICH edges can exist: the
/// type rules (`kairos_core::graph::check_link`), the cycle check and the
/// duplicate check are in the graph service, and they do not change. A
/// caller who may edit the two ends of an impossible edge gets
/// `RELATIONSHIP_RULE`, not `FORBIDDEN`.
///
/// One function for REST `POST /api/relationships` and
/// `DELETE /api/relationships/{id}`, for MCP `link_items` and
/// `unlink_items`, and for the edge that MCP `create_item` writes for
/// `parent`. They cannot give different answers.
///
/// The refusal names the two capabilities, one for each end, of which the
/// principal needs one. `details.required_capability` and
/// `details.board_id` are those of the source. `details.any_of` has the
/// two ends.
///
/// # The one exception: `supports` to a document with no parent
///
/// COLLIERY-T-0235. For a `supports` edge to a DOCUMENT that has NO
/// parent, the principal must be able to edit the DOCUMENT. The right to
/// edit the source is not sufficient. With no parent the document has no
/// board, so that is its creator, or an admin of the organization.
///
/// THE ATTACK that it stops. A document has no board: it takes its
/// authority from the board of its earliest `supports` parent. A principal
/// who can edit some task writes `supports` from the task to a document
/// with no parent. That edge is the first, so the board of the task now
/// answers for the document, and the principal can edit and archive it.
///
/// A document with no parent is old data: the server does not make one
/// ([`require_edge_remove`]), and the data was not migrated. A document
/// that HAS a parent takes the rule above with no change, because a later
/// edge does not change which board answers for it. An ADR does not take
/// its authority from `supports`, so an ADR takes the rule above too.
///
/// COLLIERY-T-0269: a document that NAMES a board takes the rule above
/// too, with a parent or with none. The board that it names answers for
/// it, and no `supports` edge changes that.
///
/// This function is the rule for the CREATE of an edge. The remove is
/// [`require_edge_remove`], which is this rule for each edge but the
/// `supports` edge of a document.
///
/// The refusal of the exception has the same `details` keys. `any_of` has
/// one entry, the target, and `board_id` is null.
pub fn require_edge_write(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    relationship: &str,
    (source_id, source_type): EdgeEnd,
    (target_id, target_type): EdgeEnd,
) -> Result<(), ApiError> {
    let source = missing_edit(conn, slug, user_id, source_id, source_type)?;
    let target = missing_edit(conn, slug, user_id, target_id, target_type)?;
    if is_document_parent_edge(relationship, target_type) {
        // COLLIERY-T-0235: the first parent of a document decides who can
        // edit the document. See the doc comment for the attack.
        // COLLIERY-T-0269: but for a document that names a board.
        let names_board = abac::document_owner_board(conn, target_id)
            .map_err(map_abac_error)?
            .is_some();
        let parents = abac::document_parents(conn, target_id)
            .map_err(map_abac_error)?
            .len();
        let has_parent = kairos_core::abac::document_has_an_owner(names_board, parents);
        if kairos_core::abac::may_link_document_parent(
            has_parent,
            source.is_none(),
            target.is_none(),
        ) {
            return Ok(());
        }
        if !has_parent {
            let document = short_code_of(conn, target_id)?;
            let capability = meta::manage_capability(target_type);
            return Err(ApiError::forbidden(format!(
                "{document} has no parent. Only its creator or an organization admin \
                 can link it to an item."
            ))
            .with_details(json!({
                "relationship": relationship,
                "required_capability": capability,
                "board_id": null,
                "any_of": [
                    {
                        "end": "target",
                        "required_capability": capability,
                        "board_id": null,
                    },
                ],
            })));
        }
    } else if kairos_core::abac::may_write_edge(source.is_none(), target.is_none()) {
        return Ok(());
    }
    // From here the answer is a refusal, whatever follows: the text below
    // only says what is missing.
    let source_capability = meta::manage_capability(source_type);
    let target_capability = meta::manage_capability(target_type);
    let source_board = source.and_then(|missing| missing.board_id);
    let target_board = target.and_then(|missing| missing.board_id);
    let need = |capability: &str, board: Option<Uuid>, end: &str| match board {
        Some(_) => format!("{capability:?} on the board of the {end}"),
        None => format!("the organization admin role for the {end}"),
    };
    Err(ApiError::forbidden(format!(
        "A {relationship} edge needs {}, or {}.",
        need(source_capability, source_board, "source"),
        need(target_capability, target_board, "target"),
    ))
    .with_details(json!({
        "relationship": relationship,
        "required_capability": source_capability,
        "board_id": source_board,
        "any_of": [
            {
                "end": "source",
                "required_capability": source_capability,
                "board_id": source_board,
            },
            {
                "end": "target",
                "required_capability": target_capability,
                "board_id": target_board,
            },
        ],
    })))
}

/// Is this the edge that gives a document a parent: `supports`, to a
/// document (COLLIERY-T-0235)? `supports` to an ADR is not: an ADR has a
/// board of its own, or no board, and takes no authority from the edge.
fn is_document_parent_edge(relationship: &str, target_type: ItemType) -> bool {
    relationship == RelationshipType::Supports.as_str() && target_type == ItemType::Document
}

/// The short code of an item, archived or not, for the text of a refusal.
/// An id that names nothing gives the id.
pub(crate) fn short_code_of(conn: &mut PgConnection, id: Uuid) -> Result<String, ApiError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        short_code: String,
    }
    let row: Option<Row> = sql_query("SELECT short_code FROM entity_directory WHERE id = $1")
        .bind::<SqlUuid, _>(id)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    Ok(row.map_or_else(|| id.to_string(), |row| row.short_code))
}

/// THE RULE FOR THE REMOVE OF AN EDGE (COLLIERY-T-0235): may this
/// principal remove this edge, and can the edge be removed?
///
/// For each edge but one, the rule is the link rule
/// ([`require_edge_write`]): the principal may edit the item at either
/// end. The one is the `supports` edge that points at a DOCUMENT, which
/// has two rules of its own. They are asked in this order.
///
/// 1. WHO: the principal must be able to edit the DOCUMENT (its creator,
///    `manage_documents` on its authorization board, or an admin of the
///    organization). The right to edit the parent is not sufficient. The
///    refusal is 403 `FORBIDDEN`. THIS NARROWS THE LINK RULE.
///
///    THE ATTACK that it stops. A document has the parents A, the
///    earliest, and B. A principal can edit A and B, and cannot edit the
///    document: for example, A is a request that the principal sent to the
///    team that wrote the document. The link rule lets the principal
///    remove the edge from A. B is now the earliest parent, the board of B
///    answers for the document, and the principal can edit and archive it.
///
/// 2. WHICH: the LAST `supports` edge of a document that names NO board
///    cannot be removed, by any principal, an admin of the organization
///    too. The refusal is 422 `LAST_PARENT`: a rule of the data, not a
///    permission.
///
///    WHY. Such a document takes its authority from what it supports, so
///    when it supports nothing it has no team to answer for it, and the
///    next `supports` edge would give it to the board of whoever wrote
///    that edge. The owner decided (2026-09-27) that the server does not
///    make a document an orphan. To move a document, the caller links it
///    to the new item first, and then removes the old edge.
///
///    COLLIERY-T-0269: a document that NAMES a board has an owner with
///    no parent, so its last `supports` edge can go
///    ([`kairos_core::abac::document_parent_can_go`]). The owner rule
///    replaces the parent rule for that document: the remove of its
///    board is what is refused when it supports nothing (`LAST_OWNER`,
///    [`documents::change_board`]).
///
/// WHO comes first, so a principal who may not edit the document learns
/// nothing about its parents from the refusal.
///
/// "Last" counts the parents that [`abac::document_parents`] gives, which
/// is what [`abac::resolve_authorization_board`] reads: an edge to an
/// ARCHIVED parent counts, because an archived parent gives its board as
/// it did while live. An edge that is not there is not the last edge: the
/// graph service answers `NOT_FOUND` for it, as before.
///
/// The function takes a lock on the row of the document
/// ([`abac::lock_document`]), so call it in the transaction of the remove.
/// [`remove_edge`] does that, and is the one caller.
pub fn require_edge_remove(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    relationship: &str,
    source: EdgeEnd,
    target: EdgeEnd,
) -> Result<(), ApiError> {
    let (source_id, _) = source;
    let (document_id, target_type) = target;
    if !is_document_parent_edge(relationship, target_type) {
        return require_edge_write(conn, slug, user_id, relationship, source, target);
    }

    // 1. WHO.
    let missing = missing_edit(conn, slug, user_id, document_id, target_type)?;
    if !kairos_core::abac::may_unlink_document_parent(missing.is_none()) {
        let capability = meta::manage_capability(target_type);
        let board_id = missing.and_then(|missing| missing.board_id);
        // COLLIERY-T-0269: the board is the board that the document
        // names, or the board of its parent.
        let names_board = abac::document_owner_board(conn, document_id)
            .map_err(map_abac_error)?
            .is_some();
        let need = match (board_id, names_board) {
            (Some(_), true) => format!("You need {capability:?} on the board of the document."),
            (Some(_), false) => format!("You need {capability:?} on the board of its parent."),
            (None, _) => "The document has no board.".to_string(),
        };
        return Err(ApiError::forbidden(format!(
            "To remove a supports edge of a document, you must be able to edit the document. \
             {need} The creator of the document and an organization admin can also remove it."
        ))
        .with_details(json!({
            "relationship": relationship,
            "required_capability": capability,
            "board_id": board_id,
            "any_of": [
                {
                    "end": "target",
                    "required_capability": capability,
                    "board_id": board_id,
                },
            ],
        })));
    }

    // 2. WHICH.
    abac::lock_document(conn, document_id).map_err(map_abac_error)?;
    // Read AFTER the lock: the change of the owner board takes the same
    // lock (COLLIERY-T-0269).
    let names_board = abac::document_owner_board(conn, document_id)
        .map_err(map_abac_error)?
        .is_some();
    let parents = abac::document_parents(conn, document_id).map_err(map_abac_error)?;
    let is_parent = parents.iter().any(|parent| parent.parent_id == source_id);
    if is_parent && !kairos_core::abac::document_parent_can_go(names_board, parents.len()) {
        let document = short_code_of(conn, document_id)?;
        let parent = short_code_of(conn, source_id)?;
        return Err(ApiError::unprocessable(
            "LAST_PARENT",
            format!(
                "{document} supports only {parent}. A document always has a parent. \
                 Link the document to a different item first, or archive the document."
            ),
        )
        .with_details(json!({
            "relationship": relationship,
            "document": document,
            "parent": parent,
        })));
    }
    Ok(())
}

/// Remove one edge: the rule ([`require_edge_remove`]) and the delete, in
/// ONE transaction (COLLIERY-T-0235).
///
/// One function for REST `DELETE /api/relationships/{id}` and for MCP
/// `unlink_items`, so the two surfaces cannot give different answers. The
/// transaction holds the lock of the rule until the edge is gone.
pub fn remove_edge(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
    relationship: RelationshipType,
    source: EdgeEnd,
    target: EdgeEnd,
) -> Result<(), ApiError> {
    atomically(conn, |conn| {
        require_edge_remove(conn, slug, user_id, relationship.as_str(), source, target)?;
        kairos_db::graph::unlink_items(conn, source.0, target.0, relationship, user_id)
            .map_err(meta::relationships::map_link_error)
    })
}

// ---------------------------------------------------------------------------
// Short-code resolution
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct DirectoryRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    entity_type: String,
}

/// Whether a lookup may return work that has been archived.
///
/// Archiving is a visibility default and nothing more (KAIROS-A-0020), so
/// the question "may I see put-away work?" belongs at the call site rather
/// than buried in a query. Every listing and every write path names
/// [`Liveness::LiveOnly`] explicitly: a reviewer checking that this
/// initiative changed no default behaviour can do it by looking for call
/// sites that say anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Live rows only.
    LiveOnly,
    /// Archived rows too. Callers that pass this MUST mark what they serve
    /// — an auditor may never mistake archived work for live work.
    IncludeArchived,
}

/// Resolve a short code to `(id, entity_type)` across all five entity
/// tables. `Ok(None)` = unknown, or archived when `liveness` is
/// [`Liveness::LiveOnly`].
///
/// Both modes read `entity_directory`. Between KAIROS-T-0154 and
/// KAIROS-T-0156 the archived mode had to re-derive the directory from the
/// five base tables by hand, because the view filtered `deleted_at IS NULL`
/// in its own body and so could not be asked about the rows it had already
/// dropped. T-0156 moved that predicate out here, where it is a mode rather
/// than a fact, and the duplicate UNION went with it.
pub fn resolve_short_code(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<Option<(Uuid, ItemType)>, ApiError> {
    let sql = match liveness {
        Liveness::LiveOnly => {
            "SELECT id, entity_type FROM entity_directory \
             WHERE short_code = $1 AND deleted_at IS NULL"
        }
        Liveness::IncludeArchived => {
            "SELECT id, entity_type FROM entity_directory WHERE short_code = $1"
        }
    };
    let row: Option<DirectoryRow> = sql_query(sql)
        .bind::<Text, _>(short_code)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    row.map(|row| {
        ItemType::ALL
            .iter()
            .copied()
            .find(|t| t.entity_type() == row.entity_type)
            .map(|item_type| (row.id, item_type))
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "entity_directory returned unknown entity_type {:?}",
                    row.entity_type
                ))
            })
    })
    .transpose()
}

/// The type of a live item by id (documents included), via the same
/// directory view — for callers that hold an id, not a code
/// (KAIROS-T-0111: the edge-permission check on an existing relationship).
/// [`Liveness::LiveOnly`] is not a parameter here on purpose: the one
/// caller is a permission check guarding a WRITE, and writes see live rows
/// only (KAIROS-I-0015 D5).
pub fn resolve_item_type(conn: &mut PgConnection, id: Uuid) -> Result<Option<ItemType>, ApiError> {
    let row: Option<DirectoryRow> = sql_query(
        "SELECT id, entity_type FROM entity_directory WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
    .map_err(ApiError::internal)?;
    Ok(row.and_then(|row| {
        ItemType::ALL
            .iter()
            .copied()
            .find(|t| t.entity_type() == row.entity_type)
    }))
}

/// The 404 for `/{short_code}` path segments that resolve to nothing.
///
/// It used to say "no live …", which was accurate while archiving hid work
/// from every surface. Under KAIROS-A-0020 a reader can ask for archived
/// work, so a 404 now means the code is unknown outright — claiming
/// otherwise would send someone looking for an item that never existed.
pub fn short_code_not_found(entity_type: &str, short_code: &str) -> ApiError {
    ApiError::not_found(format!(
        "No {entity_type} has the short code {short_code:?}."
    ))
}

// ---------------------------------------------------------------------------
// Service-error → S-0005 envelope mapping
// ---------------------------------------------------------------------------

/// [`AbacError`] never carries a client mistake on the check path (grants
/// and revokes surface their typed errors through the admin endpoints,
/// KAIROS-T-0019) — anything here is internal.
pub fn map_abac_error(e: AbacError) -> ApiError {
    ApiError::internal(e)
}

/// [`ItemError`] → HTTP. `VersionConflict` is NOT expected here: PATCH
/// handlers catch it themselves to attach the full current entity as
/// `details.current`; this fallback still returns a correct 409 from the
/// typed fields should another path surface one.
pub fn map_item_error(e: ItemError) -> ApiError {
    match e {
        ItemError::ItemNotFound { entity_type, id } => {
            ApiError::not_found(format!("The {entity_type} {id} does not exist."))
        }
        ItemError::HistoryNotFound { item_id, version } => ApiError::not_found(format!(
            "The item {item_id} has no version {version} in its history."
        )),
        ItemError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => ApiError::conflict(format!(
            "The request has the version {expected_version}, and the current version is \
             {current_version}. Get the item again, and make the edit on the current \
             version."
        ))
        .with_details(json!({
            "current": {
                "version": current_version,
                "title": current_title,
                "content": current_content,
            }
        })),
        ItemError::BoardNotFound(id) => {
            ApiError::validation(format!("The board {id} does not exist."))
        }
        ItemError::BoardHasNoColumns(id) => ApiError::validation(format!(
            "The board {id} has no columns, so the item has no place on it. Add a column \
             to the board."
        )),
        ItemError::ColumnNotOnBoard {
            board_id,
            column_id,
        } => ApiError::validation(format!(
            "The column {column_id} is not a column of the board {board_id}."
        )),
        ItemError::TemplateNotFound(id) => {
            ApiError::validation(format!("The template {id} does not exist."))
        }
        ItemError::RepositoryNotFound(id) => {
            ApiError::validation(format!("The repository {id} does not exist."))
        }
        ItemError::Database(e) => ApiError::internal(e),
    }
}

/// [`BoardError`] → HTTP, for the transition endpoints: invalid moves are
/// 422 `INVALID_TRANSITION` carrying `details.allowed_targets` (S-0005 /
/// core `TransitionError::NotAllowed`); an off-board ADR is 422
/// `ITEM_NOT_ON_BOARD` (T-0010's typed error).
pub fn map_board_error(e: BoardError) -> ApiError {
    match e {
        BoardError::ItemNotFound { entity_type, id } => {
            ApiError::not_found(format!("The {entity_type} {id} does not exist."))
        }
        BoardError::ItemNotOnBoard { entity_type, id } => ApiError::unprocessable(
            "ITEM_NOT_ON_BOARD",
            format!(
                "The {entity_type} {id} is not on a board, so it cannot move between columns."
            ),
        ),
        BoardError::Transition(TransitionError::NotAllowed {
            from,
            to,
            allowed_targets,
        }) => ApiError::unprocessable(
            "INVALID_TRANSITION",
            format!(
                "This board does not permit the transition from {:?} to {:?}.",
                from.name, to.name
            ),
        )
        .with_details(json!({
            "from": {"id": from.id, "name": from.name},
            "to": {"id": to.id, "name": to.name},
            "allowed_targets": allowed_targets
                .iter()
                .map(|c| json!({"id": c.id, "name": c.name}))
                .collect::<Vec<_>>(),
        })),
        BoardError::Transition(e) => ApiError::unprocessable("INVALID_TRANSITION", e.to_string()),
        BoardError::ColumnNotFound(id) => {
            ApiError::validation(format!("The column {id} does not exist."))
        }
        BoardError::BoardNotFound(id) => {
            ApiError::validation(format!("The board {id} does not exist."))
        }
        // KAIROS-I-0012: moving a task between delivery boards.
        BoardError::SameBoard(id) => {
            ApiError::unprocessable(
                "SAME_BOARD",
                format!("The task is on the board {id} already."),
            )
        }
        BoardError::NotDeliveryBoard(id) => ApiError::unprocessable(
            "NOT_DELIVERY_BOARD",
            format!(
                "The board {id} is not a delivery board. A task moves between delivery \
                 boards only."
            ),
        ),
        BoardError::NoEntryColumn(id) => ApiError::unprocessable(
            "NO_ENTRY_COLUMN",
            format!(
                "The board {id} has no columns, so the task has no place on it. Add a \
                 column to the board."
            ),
        ),
        // Board-configuration errors cannot arise from the entity routes;
        // reaching one here is a bug, not a client mistake.
        e @ (BoardError::TransitionNotFound { .. }
        | BoardError::MissingDefaults(_)
        | BoardError::InvalidDefaults { .. }
        | BoardError::Rule(_)
        // Only `create_board` returns these (COLLIERY-T-0230, T-0240,
        // T-0242), and no transition or move endpoint creates a board.
        | BoardError::DeliveryBoardNeedsTeam
        | BoardError::TeamHasDeliveryBoard { .. }
        | BoardError::OrganizationBoardHasNoTeam(_)
        // The create and the update of a board (COLLIERY-T-0255).
        | BoardError::SlugTaken { .. }
        // The delete and the update of a board (COLLIERY-T-0241, T-0243)
        // are configuration calls too.
        | BoardError::LastDeliveryBoard { .. }
        | BoardError::BoardTeamIsFixed { .. }) => ApiError::internal(e),
        BoardError::Database(e) => ApiError::internal(e),
    }
}

/// [`GraphError`] → HTTP, for the document-create `supports` edge: rule
/// violations restate the allowed shape (422); the rest cannot be caused
/// by a request that got this far.
pub fn map_graph_error(e: GraphError) -> ApiError {
    match e {
        GraphError::Rule(e) => ApiError::validation(e.to_string()),
        GraphError::ItemNotFound(id) => {
            ApiError::validation(format!("The related item {id} does not exist."))
        }
        e @ (GraphError::SelfLink(_)
        | GraphError::CycleDetected { .. }
        | GraphError::AlreadyLinked { .. }
        | GraphError::NotLinked { .. }) => ApiError::validation(e.to_string()),
        GraphError::Database(e) => ApiError::internal(e),
    }
}
