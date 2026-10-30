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
pub mod idempotency;
pub mod initiatives;
pub mod local_accounts;
pub mod meta;
pub mod notifications;
pub mod org;
pub mod proposals;
pub mod search;
pub mod strategies;
pub mod tasks;
// KAIROS-T-0736: the watches of a person.
pub mod watches;
pub mod work;
// KAIROS-T-0023: OpenAPI aggregation (/api/openapi.json) + dev Swagger UI.
pub mod openapi;

use axum::Router;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use kairos_client::types as dto;
/// What the create of an edge does (KAIROS-T-0569).
pub use kairos_core::abac::EdgeCreate;
use kairos_core::abac::EditUse;
use kairos_core::board::TransitionError;
use kairos_core::short_code::ItemType;
use kairos_db::code_rename::RenameError;
use kairos_db::models::OrgRole;
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
        // KAIROS-T-0381: the notification settings of a person.
        .merge(notifications::router())
        // KAIROS-T-0736: the watches of a person.
        .merge(watches::router())
        // KAIROS-T-0489: the work of a person.
        .merge(work::router())
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
    Uuid::parse_str(value).map_err(|_| {
        ApiError::validation(format!("The value {value:?} of {field} is not a UUID."))
            .with_details(json!({ "field": field }))
    })
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
    // KAIROS-T-0689: the forms of crate::references::BOARD_FORMS.
    let reference = crate::references::text(reference);
    crate::references::board(conn, reference)?
        .map(|board| board.id)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "No live board has the slug or the id {reference:?}."
            ))
            .with_details(json!({ "field": "board" }))
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
        .with_details(json!({ "field": field }))
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
/// Events are safe in here. `emit_event` inserts a row of the event outbox
/// (KAIROS-T-0632), and the emitter sends only committed rows, so a create
/// that is rolled back sends none. Short code sequences are not transactional: a
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
    // KAIROS-T-0536: the scope of the agent key comes first, so the admin
    // role does not pass an action that the scope leaves out.
    if let Some(refusal) = ApiError::outside_key_scope_of(user_id, capability, board_id) {
        return Err(refusal);
    }
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

/// THE STRUCTURE GATE (KAIROS-T-0337): may this principal change the
/// structure of the organization? An organization admin may, and a
/// principal with a grant of `manage_structure` at the organization may. A
/// grant on one board does not count here. The refusal is
/// [`ApiError::structure_required`].
///
/// The routes that ask it: create, change and delete a team; add and remove
/// a team member; create a board; reset a code sequence; delete a
/// repository. Each other change to a board (its settings, columns,
/// transitions, members and grants) asks [`require_capability`] for
/// `configure_boards` or `administer_members` on the board, and
/// [`abac::authorize`] accepts a grant of `manage_structure` on that board
/// or at the organization for those two.
///
/// The account, key, SCIM and tenant routes do not ask it. They stay with
/// the organization admin ([`require_capability`] with no board).
pub fn require_structure(
    conn: &mut PgConnection,
    slug: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    // KAIROS-T-0536: the scope of the agent key must have `manage_structure`.
    if let Some(refusal) =
        ApiError::outside_key_scope_of(user_id, kairos_core::abac::MANAGE_STRUCTURE, None)
    {
        return Err(refusal);
    }
    let allowed = abac::is_org_admin(conn, slug, user_id).map_err(map_abac_error)?
        || abac::holds_structure(conn, user_id, None).map_err(map_abac_error)?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::structure_required())
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
///
/// `role` is the role of the principal in the organization, from the
/// request ([`crate::middleware::tenant::TenantContext::role`],
/// KAIROS-T-0625). Each function of the edit rule and the link rule takes
/// it so, and none reads it again. The facts are two queries: the row of
/// the item on the table of `item_type`, and the capability
/// ([`abac::edit_facts`]).
fn missing_edit(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
    edit_use: EditUse,
) -> Result<Option<MissingEdit>, ApiError> {
    let capability = meta::manage_capability(item_type);
    let is_org_admin = role == OrgRole::Admin;
    let (facts, board_id) =
        abac::edit_facts(conn, user_id, is_org_admin, item_type, item_id, edit_use)
            .map_err(map_abac_error)?;
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
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<Option<Option<Uuid>>, ApiError> {
    Ok(
        missing_edit(conn, role, user_id, item_id, item_type, EditUse::Edit)?
            .map(|missing| missing.board_id),
    )
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
/// 3. the principal is an admin of the organization,
/// 4. the item is a task and the principal has its claim (KAIROS-T-0532),
///    for an edit that is not an archive or a restore.
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
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<(), ApiError> {
    require_item_edit_for(conn, role, user_id, item_id, item_type, EditUse::Edit)
}

/// THE EDIT RULE for an archive or a restore (KAIROS-T-0410, KAIROS-A-0025
/// decision 8). The rule of [`require_item_edit`], with one difference:
/// membership of a strategy or initiative board does not count. A member
/// of the board edits and moves each item on it, and archives or restores
/// only with a grant of `manage_<type>`, or as the creator of the item, or
/// as an admin. The refusal is the same 403.
pub fn require_item_archive(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<(), ApiError> {
    require_item_edit_for(conn, role, user_id, item_id, item_type, EditUse::Archive)
}

/// The refusal that an archive of the item by this principal would get, or
/// `None` (KAIROS-T-0558: the preview of MCP `delete_item`). The rule of
/// [`require_item_archive`], as a read: it notes no claim.
pub(crate) fn archive_refusal(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<Option<ApiError>, ApiError> {
    Ok(
        missing_edit(conn, role, user_id, item_id, item_type, EditUse::Archive)?
            .map(|missing| ApiError::capability_required(missing.capability, missing.board_id)),
    )
}

fn require_item_edit_for(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
    edit_use: EditUse,
) -> Result<(), ApiError> {
    match missing_edit(conn, role, user_id, item_id, item_type, edit_use)? {
        None => note_claim(conn, user_id, (item_id, item_type)),
        Some(missing) => Err(ApiError::capability_required(
            missing.capability,
            missing.board_id,
        )),
    }
}

/// One end of an edge: the id and the type of the item.
pub type EdgeEnd = (Uuid, ItemType);

/// A write that the edit rule or the link rule lets through: when the item
/// is a task that a different person has the claim on, the response gets
/// the warning (KAIROS-T-0359, [`crate::claims::note`]).
fn note_claim(
    conn: &mut PgConnection,
    user_id: Uuid,
    (item_id, item_type): EdgeEnd,
) -> Result<(), ApiError> {
    if item_type == ItemType::Task {
        crate::claims::note(conn, item_id, user_id)?;
    }
    Ok(())
}

/// THE LINK RULE (COLLIERY-T-0228): may this principal write this edge?
///
/// A principal may create or remove an edge when the principal may EDIT
/// the item at EITHER end, by the edit rule ([`require_item_edit`]): the
/// creator of the source or of the target, or `manage_<type>` on the
/// authorization board of the source or of the target, or an admin of the
/// organization.
///
/// ONE EXCEPTION (KAIROS-T-0569): the CREATE of a `blocks` edge needs the
/// TARGET, the blocked item. A `blocks` edge changes the work of the
/// target: its card shows it blocked, and its claimant gets the events.
/// So team A cannot block the work of team B unasked. A principal who can
/// edit the source only gets a proposal from [`link_or_propose`]. This
/// function refuses that case: no other path writes a `blocks` edge. The
/// remove keeps either end ([`require_edge_remove`]).
///
/// Until COLLIERY-T-0228 `supports`, `informs` and `supersedes` needed
/// the admin role, and `parent` and `blocks` looked at the creator of the
/// source only (KAIROS-T-0111).
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
/// One rule for REST `POST /api/relationships` and
/// `DELETE /api/relationships/{id}`, for MCP `link_items` and
/// `unlink_items`, and for the edge that MCP `create_item` writes for
/// `parent`. They cannot give different answers.
///
/// The refusal names the two capabilities, one for each end.
/// `details.any_of` has the two ends. `details.required_capability` and
/// `details.board_id` are those of the source, and for a `blocks` edge
/// those of the target.
///
/// COLLIERY-T-0235 had one exception: a `supports` edge to a document
/// with no board and no parent needed the right to edit the document,
/// because the first parent gave such a document its authority. Each
/// document has an owner board now (COLLIERY-T-3109), and no `supports`
/// edge changes who answers for it, so the exception is gone.
pub fn require_edge_write(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: &str,
    source: EdgeEnd,
    target: EdgeEnd,
) -> Result<(), ApiError> {
    match edge_create_rule(conn, role, user_id, relationship, source, target)? {
        EdgeCreate::Link => Ok(()),
        EdgeCreate::Propose | EdgeCreate::Refuse => Err(edge_refusal(
            conn,
            role,
            user_id,
            relationship,
            source,
            target,
            true,
        )?),
    }
}

/// The link rule for the CREATE of an edge, as a value (KAIROS-T-0569):
/// `Link`, `Propose` (a `blocks` edge, and the principal can edit the
/// source only) or `Refuse`. A `Link` notes the claim of each end that is
/// a task ([`crate::claims::note`]).
pub fn edge_create_rule(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: &str,
    (source_id, source_type): EdgeEnd,
    (target_id, target_type): EdgeEnd,
) -> Result<EdgeCreate, ApiError> {
    let kind = relationship_type_of(relationship)?;
    let source = missing_edit(conn, role, user_id, source_id, source_type, EditUse::Edit)?;
    let target = missing_edit(conn, role, user_id, target_id, target_type, EditUse::Edit)?;
    let outcome = kairos_core::abac::edge_create(
        kairos_db::graph::core_relationship(kind),
        source.is_none(),
        target.is_none(),
    );
    if outcome == EdgeCreate::Link {
        note_claim(conn, user_id, (source_id, source_type))?;
        note_claim(conn, user_id, (target_id, target_type))?;
    }
    Ok(outcome)
}

/// The relationship type of a name, for the link rule. Each caller sends
/// the name of a type; any other name is a 400.
fn relationship_type_of(relationship: &str) -> Result<RelationshipType, ApiError> {
    RelationshipType::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str() == relationship)
        .ok_or_else(|| {
            ApiError::validation(format!("{relationship:?} is not a relationship type."))
                .with_details(json!({ "field": "relationship" }))
        })
}

/// The link rule with EITHER end, for each relationship type: the rule of
/// the remove of an edge ([`require_edge_remove`]), `blocks` too.
fn require_edge_either_end(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: &str,
    (source_id, source_type): EdgeEnd,
    (target_id, target_type): EdgeEnd,
) -> Result<(), ApiError> {
    let source = missing_edit(conn, role, user_id, source_id, source_type, EditUse::Edit)?;
    let target = missing_edit(conn, role, user_id, target_id, target_type, EditUse::Edit)?;
    if kairos_core::abac::may_write_edge(source.is_none(), target.is_none()) {
        note_claim(conn, user_id, (source_id, source_type))?;
        return note_claim(conn, user_id, (target_id, target_type));
    }
    Err(edge_refusal(
        conn,
        role,
        user_id,
        relationship,
        (source_id, source_type),
        (target_id, target_type),
        false,
    )?)
}

/// The 403 of the link rule. `create` = the refusal is for the create of
/// an edge: a `blocks` edge then names the target first, and says that
/// the source gives a proposal (KAIROS-T-0569).
fn edge_refusal(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: &str,
    (source_id, source_type): EdgeEnd,
    (target_id, target_type): EdgeEnd,
    create: bool,
) -> Result<ApiError, ApiError> {
    let source = missing_edit(conn, role, user_id, source_id, source_type, EditUse::Edit)?;
    let target = missing_edit(conn, role, user_id, target_id, target_type, EditUse::Edit)?;
    let source_capability = meta::manage_capability(source_type);
    let target_capability = meta::manage_capability(target_type);
    let source_board = source.as_ref().and_then(|missing| missing.board_id);
    let target_board = target.as_ref().and_then(|missing| missing.board_id);
    // KAIROS-T-0536: when the scope of the agent key leaves out both ends,
    // the scope is the reason.
    if source.is_some()
        && target.is_some()
        && let (Some(refusal), Some(_)) = (
            ApiError::outside_key_scope(source_capability, source_board),
            ApiError::outside_key_scope(target_capability, target_board),
        )
    {
        return Ok(refusal);
    }
    let need = |capability: &str, board: Option<Uuid>, end: &str| match board {
        Some(_) => format!("{capability:?} on the board of the {end}"),
        None => format!("the organization admin role for the {end}"),
    };
    let blocks = create && relationship == RelationshipType::Blocks.as_str();
    if !blocks {
        return Ok(ApiError::forbidden(format!(
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
        })));
    }
    // A `blocks` edge: the target links, the source alone proposes.
    let tail = if source.is_none() {
        "You can edit the source only. Use link_items or POST /api/relationships: they make a          proposal of the edge."
            .to_string()
    } else {
        format!(
            "With {}, the call makes a proposal of the edge.",
            need(source_capability, source_board, "source")
        )
    };
    Ok(ApiError::forbidden(format!(
        "A blocks edge needs {}, the blocked item. {tail}",
        need(target_capability, target_board, "target"),
    ))
    .with_details(json!({
        "relationship": relationship,
        "required_capability": target_capability,
        "board_id": target_board,
        "any_of": [
            {
                "end": "target",
                "required_capability": target_capability,
                "board_id": target_board,
                "gives": "edge",
            },
            {
                "end": "source",
                "required_capability": source_capability,
                "board_id": source_board,
                "gives": "proposal",
            },
        ],
    })))
}

/// What the create of an edge did (KAIROS-T-0569).
pub enum LinkOutcome {
    /// The edge exists.
    Linked(kairos_db::models::graph::ItemRelationship),
    /// The call recorded a proposal of the edge. `boards` decide it
    /// (KAIROS-A-0060).
    Proposed {
        proposal: Box<kairos_db::proposals::EdgeProposal>,
        boards: kairos_db::proposals::ApprovalBoards,
    },
}

/// The claim of a proposal that the link rule makes (KAIROS-T-0569).
pub const LINK_PROPOSAL_CLAIM: &str = "a blocks edge from a caller who can edit the source only";

/// The reason of a proposal that the link rule makes, for the decider.
const LINK_PROPOSAL_WHY: &str = "The caller asked for this blocks edge. The caller can edit the \
                                 blocker and not the blocked item, so the edge waits for a \
                                 decision.";

/// Create an edge, or record a proposal of it (KAIROS-T-0569): the link
/// rule ([`edge_create_rule`]) and the write, in ONE transaction.
///
/// One function for REST `POST /api/relationships` and MCP `link_items`.
/// A `blocks` edge from a principal who can edit the source only becomes
/// a proposal ([`kairos_db::proposals::propose`]). The proposal first
/// passes the type rule and the duplicate check of the edge, so a
/// proposal that cannot become an edge is not recorded. The confirm does
/// the cycle check ([`kairos_db::graph::link_items`]).
///
/// `map_graph` gives the refusal of a graph rule for the surface: each
/// surface names its own call in `PARENT_EXISTS` (KAIROS-T-0572).
pub fn link_or_propose(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: RelationshipType,
    (source, target): (EdgeEnd, EdgeEnd),
    map_graph: fn(GraphError) -> ApiError,
) -> Result<LinkOutcome, ApiError> {
    atomically(conn, |conn| {
        match edge_create_rule(conn, role, user_id, relationship.as_str(), source, target)? {
            EdgeCreate::Link => {
                kairos_db::graph::link_items(conn, source.0, target.0, relationship, user_id)
                    .map(LinkOutcome::Linked)
                    .map_err(map_graph)
            }
            EdgeCreate::Refuse => Err(edge_refusal(
                conn,
                role,
                user_id,
                relationship.as_str(),
                source,
                target,
                true,
            )?),
            EdgeCreate::Propose => {
                propose_edge_of_link(conn, user_id, relationship, (source, target), map_graph)
            }
        }
    })
}

/// The proposal of [`link_or_propose`]: the type rule, the duplicate
/// check, then the proposal and the boards that decide it.
fn propose_edge_of_link(
    conn: &mut PgConnection,
    user_id: Uuid,
    relationship: RelationshipType,
    ((source_id, source_type), (target_id, target_type)): (EdgeEnd, EdgeEnd),
    map_graph: fn(GraphError) -> ApiError,
) -> Result<LinkOutcome, ApiError> {
    use kairos_db::schema::item_relationships as edges;
    kairos_core::graph::check_link(
        kairos_db::graph::core_relationship(relationship),
        source_type,
        target_type,
    )
    .map_err(|rule| map_graph(GraphError::Rule(rule)))?;
    let linked: bool = diesel::select(diesel::dsl::exists(
        edges::table
            .filter(edges::source_id.eq(source_id))
            .filter(edges::target_id.eq(target_id))
            .filter(edges::relationship.eq(relationship.as_str())),
    ))
    .get_result(conn)
    .map_err(ApiError::internal)?;
    if linked {
        return Err(map_graph(GraphError::AlreadyLinked {
            relationship,
            source_id,
            target_id,
        }));
    }
    let proposal = kairos_db::proposals::propose(
        conn,
        kairos_db::proposals::NewProposal {
            source_id,
            target_id,
            relationship,
            claim: LINK_PROPOSAL_CLAIM,
            why: LINK_PROPOSAL_WHY,
            score: 0.0,
        },
        user_id,
    )
    .map_err(proposals::map_proposal_error)?;
    let boards = kairos_db::proposals::approval_boards(conn, source_id, target_id)
        .map_err(proposals::map_proposal_error)?;
    Ok(LinkOutcome::Proposed {
        proposal: Box::new(proposal),
        boards,
    })
}

/// The text of a proposal that the link rule made (KAIROS-T-0569), for
/// the answer of MCP `link_items` and of REST `POST /api/relationships`.
pub fn link_proposal_text(
    source_code: &str,
    target_code: &str,
    relationship: &str,
    proposal: &kairos_db::proposals::EdgeProposal,
    boards: &kairos_db::proposals::ApprovalBoards,
) -> String {
    format!(
        "Proposed {source_code} -[{relationship}]-> {target_code} (proposal {}). It is NOT an \
         edge yet. A blocks edge needs the right to edit the blocked item {target_code}. You \
         can edit only {source_code}, so the call made a proposal. {}",
        proposal.id,
        boards.deciders()
    )
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
/// principal remove this edge?
///
/// For each edge but one, the rule is the link rule with either end: the
/// principal may edit the item at either end. A `blocks` edge too: the
/// create needs the target (KAIROS-T-0569), the remove does not. The one
/// is the `supports` edge that points at a DOCUMENT: the principal must
/// be able to edit the DOCUMENT (its creator, `manage_documents` on its
/// owner board, or an admin of the organization). The right to edit the parent is not sufficient. The
/// refusal is 403 `FORBIDDEN`. THIS NARROWS THE LINK RULE.
///
/// Until COLLIERY-T-3109 a document could have no board, and two more
/// rules kept such a document from having no owner: the last `supports`
/// edge of a document with no board could not go (422 `LAST_PARENT`), and
/// the board of a document that supported nothing could not go (422
/// `LAST_OWNER`). Each document has an owner board now, so each
/// `supports` edge of a document can go.
pub fn require_edge_remove(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: &str,
    source: EdgeEnd,
    target: EdgeEnd,
) -> Result<(), ApiError> {
    let (document_id, target_type) = target;
    if !is_document_parent_edge(relationship, target_type) {
        return require_edge_either_end(conn, role, user_id, relationship, source, target);
    }

    let missing = missing_edit(conn, role, user_id, document_id, target_type, EditUse::Edit)?;
    if kairos_core::abac::may_unlink_document_parent(missing.is_none()) {
        return Ok(());
    }
    let capability = meta::manage_capability(target_type);
    let board_id = missing.and_then(|missing| missing.board_id);
    Err(ApiError::forbidden(format!(
        "To remove a supports edge of a document, you must be able to edit the document. \
         You need {capability:?} on the owner board of the document. The creator of the \
         document and an organization admin can also remove it."
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
    })))
}

/// Remove one edge: the rule ([`require_edge_remove`]) and the delete, in
/// ONE transaction (COLLIERY-T-0235).
///
/// One function for REST `DELETE /api/relationships/{id}` and for MCP
/// `unlink_items`, so the two surfaces cannot give different answers.
pub fn remove_edge(
    conn: &mut PgConnection,
    role: OrgRole,
    user_id: Uuid,
    relationship: RelationshipType,
    source: EdgeEnd,
    target: EdgeEnd,
) -> Result<(), ApiError> {
    atomically(conn, |conn| {
        require_edge_remove(conn, role, user_id, relationship.as_str(), source, target)?;
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
///
/// The code is upper-cased first (KAIROS-T-0560): each code is in capitals,
/// so `acme-t-0012` finds `ACME-T-0012`.
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
        .bind::<Text, _>(crate::references::short_code(short_code))
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

/// The current code of the item that had the retired `short_code`
/// (COLLIERY-T-3100). `Ok(None)` when the code is not retired, or when its
/// item is not there now.
pub fn retired_to_current(
    conn: &mut PgConnection,
    short_code: &str,
) -> Result<Option<String>, ApiError> {
    Ok(
        kairos_db::retired_codes::current_code(conn, &crate::references::short_code(short_code))
            .map_err(ApiError::internal)?
            .and_then(|retired| retired.current.map(|(code, _)| code)),
    )
}

/// What a lookup does with a retired code (COLLIERY-T-3100), from its
/// [`Liveness`]: a read ([`Liveness::IncludeArchived`]) follows the code to
/// the current code; a write ([`Liveness::LiveOnly`]) is refused with
/// [`retired_code_refused`], because a write must name the item it changes.
/// `Ok(None)` when the code is not retired.
pub fn follow_retired(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<Option<String>, ApiError> {
    match retired_to_current(conn, short_code)? {
        None => Ok(None),
        Some(current) => match liveness {
            Liveness::IncludeArchived => Ok(Some(current)),
            Liveness::LiveOnly => Err(retired_code_refused(short_code, &current)),
        },
    }
}

/// The 404 for a write that names a retired code (COLLIERY-T-3100). It
/// names the current code in the message and in `details.current_code`.
pub fn retired_code_refused(short_code: &str, current: &str) -> ApiError {
    ApiError::not_found(format!(
        "The item of the short code {short_code:?} has a new code: {current:?}. The old \
         code is a retired code. Use the new code."
    ))
    .with_details(json!({
        "field": "short_code",
        "retired_code": short_code,
        "current_code": current,
    }))
}

/// The end of a load by short code in one family (COLLIERY-T-3100): the
/// row that was `found`, or else the row of the current code when
/// `short_code` is retired (a read; a write is refused, see
/// [`follow_retired`]), or else the 404 of [`short_code_not_found`]. The
/// current code is never retired, so `load` runs one more time at most.
pub fn found_or_follow_retired<T>(
    conn: &mut PgConnection,
    found: Option<T>,
    entity_type: &str,
    short_code: &str,
    liveness: Liveness,
    load: impl FnOnce(&mut PgConnection, &str, Liveness) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    if let Some(row) = found {
        return Ok(row);
    }
    // KAIROS-T-0560: a code in input ignores case. The upper-cased code
    // loads one more time at most.
    let upper = crate::references::short_code(short_code);
    if upper != short_code {
        return load(conn, &upper, liveness);
    }
    match follow_retired(conn, short_code, liveness)? {
        Some(current) => load(conn, &current, liveness),
        None if liveness == Liveness::LiveOnly
            && resolve_short_code(conn, short_code, Liveness::IncludeArchived)?
                .is_some_and(|(_, t)| t.entity_type() == entity_type) =>
        {
            Err(archived_write_refused(short_code))
        }
        None => Err(short_code_not_found(entity_type, short_code)),
    }
}

/// The 404 for a write to an archived item (KAIROS-T-0560). A write needs a
/// live item, so the text names `restore_item`.
pub fn archived_write_refused(short_code: &str) -> ApiError {
    ApiError::not_found(format!(
        "The item {short_code} is archived. A write needs a live item. Restore the item first \
         with restore_item."
    ))
    .with_details(json!({ "field": "short_code" }))
}

/// A live item that a write names as a reference (KAIROS-T-0560): an end
/// of an edge, or the parent of a new item.
#[derive(Debug, Clone)]
pub struct LiveRef {
    /// The id of the item.
    pub id: Uuid,
    /// The type of the item.
    pub item_type: ItemType,
    /// The current short code of the item.
    pub code: String,
    /// The retired code in the input, when the input had one.
    pub retired: Option<String>,
}

impl LiveRef {
    /// The id and the type, as the edge rules take them.
    pub fn typed(&self) -> (Uuid, ItemType) {
        (self.id, self.item_type)
    }

    /// The sentence that names the current code of a retired code, e.g.
    /// `ACME-T-0100 is now ACME-T-0200.`, or `None`.
    pub fn renamed(&self) -> Option<String> {
        self.retired
            .as_ref()
            .map(|old| format!("{old} is now {}.", self.code))
    }
}

/// Resolve a reference of a write (KAIROS-T-0560): the `field` names the
/// argument in a refusal. The code is upper-cased. A retired code follows
/// to its item, as a read does, because Kairos never gives a retired code
/// again. An archived item is a 422 `VALIDATION` that names `restore_item`;
/// an unknown code is a 422 `VALIDATION`, as before.
///
/// The subject of a write (the item that the write changes) does not come
/// here: a retired code there is refused ([`follow_retired`]).
pub fn resolve_live_ref(
    conn: &mut PgConnection,
    short_code: &str,
    field: &str,
) -> Result<LiveRef, ApiError> {
    let code = crate::references::short_code(short_code);
    if let Some((id, item_type)) = resolve_short_code(conn, &code, Liveness::LiveOnly)? {
        return Ok(LiveRef {
            id,
            item_type,
            code,
            retired: None,
        });
    }
    let (current, retired) = match retired_to_current(conn, &code)? {
        Some(current) => (current, Some(code)),
        None => (code, None),
    };
    if let Some((id, item_type)) = resolve_short_code(conn, &current, Liveness::LiveOnly)? {
        return Ok(LiveRef {
            id,
            item_type,
            code: current,
            retired,
        });
    }
    if resolve_short_code(conn, &current, Liveness::IncludeArchived)?.is_some() {
        return Err(ApiError::validation(format!(
            "The {field} {current} is archived. A write needs a live item. Restore the item \
             first with restore_item."
        ))
        .with_details(json!({ "field": field })));
    }
    Err(ApiError::validation(format!(
        "The {field} {short_code:?} is not the short code of a live item."
    ))
    .with_details(json!({ "field": field })))
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
    .with_details(json!({ "field": "short_code" }))
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
/// [`kairos_db::item_notes::NoteError`] → API error (KAIROS-T-0367).
pub fn map_note_error(e: kairos_db::item_notes::NoteError) -> ApiError {
    match e {
        kairos_db::item_notes::NoteError::EmptyText => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "text" }))
        }
        kairos_db::item_notes::NoteError::Item(inner) => map_item_error(inner),
    }
}

/// The notes of the item with the id `item_id` (a UUID, as a DTO has it),
/// for the `notes` field of the read of one item (KAIROS-T-0630).
pub(crate) fn item_notes(
    conn: &mut PgConnection,
    item_id: &str,
) -> Result<Option<Vec<dto::ItemNote>>, ApiError> {
    let id: uuid::Uuid = item_id.parse().map_err(ApiError::internal)?;
    let notes = kairos_db::item_notes::notes_of(conn, id).map_err(ApiError::internal)?;
    Ok(Some(notes.into_iter().map(convert::note).collect()))
}

pub fn map_item_error(e: ItemError) -> ApiError {
    match e {
        ItemError::ItemNotFound { entity_type, id } => {
            ApiError::not_found(format!("The {entity_type} {id} does not exist."))
                .with_details(json!({ "field": "short_code" }))
        }
        ItemError::HistoryNotFound { item_id, version } => ApiError::not_found(format!(
            "The item {item_id} has no version {version} in its history."
        ))
        .with_details(json!({ "field": "version" })),
        e @ ItemError::HistoryCorrupt { .. } => ApiError::internal(e),
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
                .with_details(json!({ "field": "board" }))
        }
        ItemError::BoardHasNoColumns(id) => ApiError::validation(format!(
            "The board {id} has no columns, so the item has no place on it. Add a column \
             to the board."
        ))
        .with_details(json!({ "field": "board" })),
        ItemError::ColumnNotOnBoard {
            board_id,
            column_id,
        } => ApiError::validation(format!(
            "The column {column_id} is not a column of the board {board_id}."
        ))
        .with_details(json!({ "field": "column" })),
        ItemError::TemplateNotFound(id) => {
            ApiError::validation(format!("The template {id} does not exist."))
                .with_details(json!({ "field": "template" }))
        }
        ItemError::RepositoryNotFound(id) => {
            ApiError::validation(format!("The repository {id} does not exist."))
                .with_details(json!({ "field": "repository" }))
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
                .with_details(json!({ "field": "short_code" }))
        }
        BoardError::ItemNotOnBoard { entity_type, id } => ApiError::unprocessable(
            "ITEM_NOT_ON_BOARD",
            format!(
                "The {entity_type} {id} is not on a board, so it cannot move between columns."
            ),
        ),
        // KAIROS-T-0554: the message keeps the sentence of the core that
        // names the targets ("From Todo, an item can move to: Active,
        // Blocked."), so an agent reads where it can go without the details.
        // KAIROS-T-0675: `path` is the shortest chain of columns to the
        // target (empty when none goes there), one transition_item for
        // each step.
        BoardError::Transition(
            ref error @ TransitionError::NotAllowed {
                ref from,
                ref to,
                ref allowed_targets,
                ref route,
            },
        ) => ApiError::unprocessable("INVALID_TRANSITION", error.to_string())
        .with_details(json!({
            "from": {"id": from.id, "name": from.name},
            "to": {"id": to.id, "name": to.name},
            "allowed_targets": allowed_targets
                .iter()
                .map(|c| json!({"id": c.id, "name": c.name}))
                .collect::<Vec<_>>(),
            "path": route
                .path
                .iter()
                .map(|c| json!({"id": c.id, "name": c.name}))
                .collect::<Vec<_>>(),
        })),
        BoardError::Transition(e) => ApiError::unprocessable("INVALID_TRANSITION", e.to_string()),
        // KAIROS-T-0641: the board policy parent_gate.
        // KAIROS-T-0676: an initiative with no owner under owner_gate gets
        // the claim in the same move.
        BoardError::ParentNotActive {
            ref initiative,
            ref initiative_column,
            ref to_column,
            owner_needed,
            board_id,
            ..
        } => ApiError::unprocessable("PARENT_NOT_ACTIVE", e.to_string()).with_details(json!({
            "initiative": initiative,
            "initiative_column": initiative_column,
            "to_column": to_column,
            "board_id": board_id,
            "policy": "parent_gate",
            "owner_needed": owner_needed,
            "fix": if owner_needed {
                format!("transition_item {initiative} Active claim: true")
            } else {
                format!("transition_item {initiative} Active")
            },
        })),
        // KAIROS-T-0643: the board policy owner_gate.
        BoardError::OwnerRequired {
            ref initiative,
            ref to_column,
            board_id,
        } => ApiError::unprocessable("OWNER_REQUIRED", e.to_string()).with_details(json!({
            "initiative": initiative,
            "to_column": to_column,
            "board_id": board_id,
            "policy": "owner_gate",
            "fix": format!("claim_item {initiative}"),
        })),
        // KAIROS-T-0565: the board policy blockers_gate.
        BoardError::Blocked {
            ref task,
            ref to_column,
            ref blockers,
            board_id,
        } => ApiError::unprocessable("BLOCKED", e.to_string()).with_details(json!({
            "task": task,
            "to_column": to_column,
            "board_id": board_id,
            "blockers": blockers
                .iter()
                .map(|b| json!({"short_code": b.short_code, "cancelled": b.cancelled}))
                .collect::<Vec<_>>(),
            "policy": "blockers_gate",
            "fix": "unlink_items",
        })),
        // KAIROS-T-0564: a done task moves to another board only after a
        // reopen; a reopen takes a done task only.
        BoardError::TaskDone { ref task, .. } => {
            ApiError::unprocessable("TASK_DONE", e.to_string()).with_details(json!({
                "short_code": task,
                "fix": format!("reopen_item {task}"),
            }))
        }
        BoardError::TaskNotDone { .. } => ApiError::unprocessable("TASK_NOT_DONE", e.to_string()),
        BoardError::NoOpenColumn { board_id, .. } => {
            ApiError::unprocessable("NO_OPEN_COLUMN", e.to_string())
                .with_details(json!({ "board_id": board_id }))
        }
        BoardError::ColumnNotFound(id) => {
            ApiError::validation(format!("The column {id} does not exist."))
                .with_details(json!({ "field": "column" }))
        }
        BoardError::BoardNotFound(id) => {
            ApiError::validation(format!("The board {id} does not exist."))
                .with_details(json!({ "field": "board" }))
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
        // The create of an ADR board for a team (COLLIERY-T-3102).
        | BoardError::TeamHasAdrBoard { .. }
        | BoardError::TeamAdrBoardPrefix { .. }
        // The create and the update of a board (COLLIERY-T-0255).
        | BoardError::SlugTaken { .. }
        // The create of a board (COLLIERY-T-3099).
        | BoardError::InvalidCodePrefix(_)
        | BoardError::CodePrefixTaken { .. }
        // The delete and the update of a board (COLLIERY-T-0241, T-0243)
        // are configuration calls too.
        | BoardError::LastDeliveryBoard { .. }
        | BoardError::BoardTeamIsFixed { .. }) => ApiError::internal(e),
        BoardError::Rename(e) => map_rename_error(e),
        BoardError::Database(e) => ApiError::internal(e),
    }
}

/// [`RenameError`] → HTTP, for a move with `rename` (COLLIERY-T-3101). A
/// rename to the prefix that the code has already is 422
/// `RENAME_NOT_NEEDED`. A code that cannot be retired is a bug: the rename
/// gives the item its new code first.
pub fn map_rename_error(e: RenameError) -> ApiError {
    match e {
        RenameError::NotNeeded { code, prefix } => ApiError::unprocessable(
            "RENAME_NOT_NEEDED",
            RenameError::NotNeeded {
                code: code.clone(),
                prefix: prefix.clone(),
            }
            .to_string(),
        )
        .with_details(json!({ "field": "rename", "short_code": code, "code_prefix": prefix })),
        RenameError::Item(e) => map_item_error(e),
        RenameError::Retire(e) => ApiError::internal(e),
        RenameError::Database(e) => ApiError::internal(e),
    }
}

/// [`GraphError`] → HTTP, for the document-create `supports` edge: rule
/// violations restate the allowed shape (422); the rest cannot be caused
/// by a request that got this far.
pub fn map_graph_error(e: GraphError) -> ApiError {
    match e {
        // Each caller links a new item to the parent that the call named.
        GraphError::Rule(e) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "parent" }))
        }
        GraphError::ItemNotFound(id) => {
            ApiError::validation(format!("The related item {id} does not exist."))
                .with_details(json!({ "field": "parent" }))
        }
        e @ (GraphError::SelfLink(_)
        | GraphError::CycleDetected { .. }
        | GraphError::AlreadyLinked { .. }
        | GraphError::NotLinked { .. }) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "parent" }))
        }
        // KAIROS-T-0572: a create gives a new item, which has no parent;
        // only a write at the same time can get here.
        e @ GraphError::ParentExists { .. } => {
            meta::relationships::parent_exists(e, "set_parent or PUT .../parent")
        }
        GraphError::Database(e) => ApiError::internal(e),
    }
}
