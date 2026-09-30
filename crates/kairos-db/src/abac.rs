//! ABAC orchestration (KAIROS-T-0011, contract per KAIROS-A-0006, layering
//! per KAIROS-A-0009): the single-query capability check, org-admin bypass,
//! grant/revoke with `activity_log` rows, and the resolution of the board
//! of a document (the board that it names, or the board of its parent). The pure matching semantics live in [`kairos_core::abac`];
//! this module is the SQL side of the same contract.
//!
//! Board-scoped functions operate in the CURRENT `search_path` tenant schema
//! (unqualified table names, same convention as [`crate::boards`]). The
//! org-admin check crosses into the `public` schema, which every tenant
//! checkout can reach (the pool pins `search_path` to `org_{slug}, public`
//! and `schema.rs` declares public tables schema-qualified).
//!
//! # Identifying the organization
//!
//! `public.organizations` holds one row per tenant, keyed by slug — the same
//! slug [`crate::pool::TenantPool::tenant`] pins the connection's
//! `search_path` from. Rather than introduce a `TenantContext` type,
//! [`is_org_admin`]/[`authorize`] take that slug explicitly, matching this
//! crate's convention of plain `conn + identifiers` arguments (boards.rs);
//! callers that hold a [`crate::pool::TenantConnection`] already know their
//! slug because they checked the connection out with it.
//!
//! # Duplicate grants / missing revokes
//!
//! `board_member_capabilities` has the composite PK `(board_id, user_id,
//! capability)`. Granting an existing capability returns the typed
//! [`AbacError::AlreadyGranted`] (not an idempotent no-op) and revoking a
//! missing one returns [`AbacError::GrantNotFound`], so callers can
//! distinguish "changed" from "already so" and the activity log records only
//! real changes — one `capability_grant`/`capability_revoke` row per actual
//! mutation (KAIROS-A-0004/S-0004 action list).

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Array, Bool, Nullable, Text, Uuid as SqlUuid};
use kairos_core::short_code::ItemType;
use uuid::Uuid;

use crate::models::boards::NewBoardMemberCapability;
use crate::models::enums::{ActivityAction, OrgRole, RelationshipType};
use crate::models::graph::NewActivityLogEntry;

/// Errors from capability checks, grants, revocations, or resolution.
#[derive(Debug, thiserror::Error)]
pub enum AbacError {
    /// Grants must not be empty strings (a defense-in-depth guard; the
    /// vocabulary itself is validated at the API layer per KAIROS-A-0006's
    /// extensible-text design).
    #[error("The capability is empty. Send the name of a capability.")]
    EmptyCapability,
    /// The `(board_id, user_id, capability)` grant already exists (composite
    /// PK) — the grant is unchanged and no activity row was written.
    #[error(
        "The user {user_id} has the capability {capability:?} on the board {board_id} already."
    )]
    AlreadyGranted {
        board_id: Uuid,
        user_id: Uuid,
        capability: String,
    },
    /// No such grant exists to revoke.
    #[error(
        "The user {user_id} does not have the capability {capability:?} on the board {board_id}."
    )]
    GrantNotFound {
        board_id: Uuid,
        user_id: Uuid,
        capability: String,
    },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

#[derive(QueryableByName)]
struct BoolRow {
    #[diesel(sql_type = Bool)]
    authorized: bool,
}

/// The KAIROS-A-0006 access check as ONE indexed query (no N+1): EXISTS over
/// `board_member_capabilities` (`idx_board_member_cap_board_user`) with the
/// `*`→`%` LIKE translation plus the explicit `capability = '*'` arm.
///
/// LIKE metacharacters (`\`, `%`, `_`) in STORED capability values are
/// escaped before the `*` translation, so a hostile grant like `manage%`
/// matches only the literal string `manage%` — exactly the semantics of
/// [`kairos_core::abac::capability_matches`] (the pure mirror of this query).
///
/// Second arm (KAIROS-T-0072, A-0006 amendment): membership of the board's
/// OWNING TEAM implies the day-to-day delivery capabilities
/// ([`kairos_core::abac::TEAM_IMPLIED_CAPABILITIES`]). The implication is
/// decided in Rust ([`kairos_core::abac::team_implies`], bound as `$4`) so
/// the vocabulary stays in one place; the membership test joins
/// `boards.team_id` → `team_members` in the same query. Nothing is stored:
/// leaving the team is the revocation.
pub fn check_capability(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    required: &str,
) -> Result<bool, AbacError> {
    let row: BoolRow = sql_query(
        r"SELECT (
            EXISTS (
                SELECT 1 FROM board_member_capabilities
                WHERE board_id = $1
                  AND user_id = $2
                  AND ($3 LIKE replace(replace(replace(replace(
                           capability, '\', '\\'), '%', '\%'), '_', '\_'), '*', '%')
                       OR capability = '*')
            )
            OR (
                $4 AND EXISTS (
                    SELECT 1
                    FROM boards b
                    JOIN team_members tm ON tm.team_id = b.team_id
                    WHERE b.id = $1
                      AND tm.user_id = $2
                )
            )
          ) AS authorized",
    )
    .bind::<SqlUuid, _>(board_id)
    .bind::<SqlUuid, _>(user_id)
    .bind::<Text, _>(required)
    .bind::<Bool, _>(kairos_core::abac::team_implies(required))
    .get_result(conn)?;
    Ok(row.authorized)
}

/// Whether `user_id` is an org admin (`public.organization_members.role =
/// 'admin'`) of the organization identified by `org_slug` (module docs:
/// the tenant's slug is how the org row is identified).
pub fn is_org_admin(
    conn: &mut PgConnection,
    org_slug: &str,
    user_id: Uuid,
) -> Result<bool, AbacError> {
    use crate::schema::{organization_members, organizations};

    let admin: bool = diesel::select(diesel::dsl::exists(
        organization_members::table
            .inner_join(organizations::table)
            .filter(organizations::slug.eq(org_slug))
            .filter(organization_members::user_id.eq(user_id))
            .filter(organization_members::role.eq(OrgRole::Admin)),
    ))
    .get_result(conn)?;
    Ok(admin)
}

/// Whether `user_id` is a member (any role) of the organization `org_slug`.
pub fn is_org_member(
    conn: &mut PgConnection,
    org_slug: &str,
    user_id: Uuid,
) -> Result<bool, AbacError> {
    use crate::schema::{organization_members, organizations};

    let member: bool = diesel::select(diesel::dsl::exists(
        organization_members::table
            .inner_join(organizations::table)
            .filter(organizations::slug.eq(org_slug))
            .filter(organization_members::user_id.eq(user_id)),
    ))
    .get_result(conn)?;
    Ok(member)
}

/// The COMPUTED `file_backlog` capability (KAIROS-T-0105, A-0019 §4): any
/// tenant member holds it on every live DELIVERY board — and only there.
/// Whether the write is actually a REQUEST (a task create into the entry
/// column, in the support lane) is the caller's decision: the server
/// chooses to ask for `file_backlog` only in that exact case. This answers
/// "is this principal allowed to send a request to this board at all".
/// No repository is looked at (COLLIERY-T-0218, COLLIERY-A-0023): until
/// then the server asked only when the task linked to a repository.
pub fn check_file_backlog(
    conn: &mut PgConnection,
    org_slug: &str,
    board_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AbacError> {
    use crate::models::enums::BoardLevel;
    use crate::schema::boards::dsl;

    if !is_org_member(conn, org_slug, user_id)? {
        return Ok(false);
    }
    let delivery: bool = diesel::select(diesel::dsl::exists(
        dsl::boards
            .filter(dsl::id.eq(board_id))
            .filter(dsl::board_level.eq(BoardLevel::Delivery))
            .filter(dsl::deleted_at.is_null()),
    ))
    .get_result(conn)?;
    Ok(delivery)
}

/// The combined KAIROS-A-0006 write-authorization decision for a board
/// action: org admins bypass the whitelist (implicit full access, checked
/// first); everyone else needs a matching `board_member_capabilities` grant
/// ([`check_capability`]) — or, for the computed `file_backlog`
/// (KAIROS-T-0105), tenant membership on a delivery board
/// ([`check_file_backlog`]).
pub fn authorize(
    conn: &mut PgConnection,
    org_slug: &str,
    board_id: Uuid,
    user_id: Uuid,
    required: &str,
) -> Result<bool, AbacError> {
    if is_org_admin(conn, org_slug, user_id)? {
        return Ok(true);
    }
    if required == kairos_core::abac::FILE_BACKLOG {
        return check_file_backlog(conn, org_slug, board_id, user_id);
    }
    check_capability(conn, board_id, user_id, required)
}

/// Insert one `activity_log` row (current tenant schema; same shape as
/// `crate::boards`' logging — `entity_type = 'board'`, `entity_id` = the
/// board the grant is scoped to).
fn log_capability_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    board_id: Uuid,
    user_id: Uuid,
    capability: &str,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(board_id),
            entity_type: Some("board".to_string()),
            details: format!("capability:{capability} user:{user_id}"),
        })
        .execute(conn)?;
    Ok(())
}

/// Grant `capability` to `user_id` on `board_id`, writing the
/// `action='capability_grant'` activity row — one transaction. A duplicate
/// grant is the typed [`AbacError::AlreadyGranted`] (module docs).
pub fn grant_capability(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    capability: &str,
    granted_by: Uuid,
) -> Result<(), AbacError> {
    if capability.is_empty() {
        return Err(AbacError::EmptyCapability);
    }
    conn.transaction::<_, AbacError, _>(|conn| {
        let inserted = diesel::insert_into(crate::schema::board_member_capabilities::table)
            .values(NewBoardMemberCapability {
                board_id,
                user_id,
                capability: capability.to_string(),
                granted_by,
            })
            .execute(conn);
        match inserted {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(AbacError::AlreadyGranted {
                    board_id,
                    user_id,
                    capability: capability.to_string(),
                });
            }
            other => {
                other?;
            }
        }
        log_capability_activity(
            conn,
            granted_by,
            ActivityAction::CapabilityGrant,
            board_id,
            user_id,
            capability,
        )?;
        Ok(())
    })
}

/// Revoke `capability` from `user_id` on `board_id`, writing the
/// `action='capability_revoke'` activity row — one transaction. Revoking a
/// grant that does not exist is the typed [`AbacError::GrantNotFound`].
pub fn revoke_capability(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    capability: &str,
    revoked_by: Uuid,
) -> Result<(), AbacError> {
    conn.transaction::<_, AbacError, _>(|conn| {
        use crate::schema::board_member_capabilities::dsl;

        let deleted = diesel::delete(
            dsl::board_member_capabilities
                .filter(dsl::board_id.eq(board_id))
                .filter(dsl::user_id.eq(user_id))
                .filter(dsl::capability.eq(capability)),
        )
        .execute(conn)?;
        if deleted == 0 {
            return Err(AbacError::GrantNotFound {
                board_id,
                user_id,
                capability: capability.to_string(),
            });
        }
        log_capability_activity(
            conn,
            revoked_by,
            ActivityAction::CapabilityRevoke,
            board_id,
            user_id,
            capability,
        )?;
        Ok(())
    })
}

/// The board a workflow item (strategy/initiative/task/ADR) sits on.
/// `None` = not a workflow item, or an ADR that is not placed on a board.
///
/// **Archived items resolve too** (KAIROS-A-0020, KAIROS-T-0153). This
/// answers "which board governs this row?", and putting the row away does
/// not move it to a different board. Filtering on `deleted_at` here would
/// make an archived item resolve to `None`, whereupon the caller falls back
/// to the tenant-wide org-admin policy — so archived work would end up
/// *more* restricted than live work, inverting the rule that archiving is a
/// visibility default and not a permission boundary. Liveness is enforced
/// by the callers that mutate (`items.rs`), not here.
fn board_of_workflow_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<Uuid>, DieselError> {
    use crate::schema::{adrs, initiatives, strategies, tasks};

    macro_rules! try_table {
        ($table:ident) => {
            if let Some(board_id) = $table::table
                .filter($table::id.eq(item_id))
                .select($table::board_id)
                .first::<Uuid>(conn)
                .optional()?
            {
                return Ok(Some(board_id));
            }
        };
    }
    try_table!(strategies);
    try_table!(initiatives);
    try_table!(tasks);

    // ADRs may be off-board (nullable placement).
    if let Some(board_id) = adrs::table
        .filter(adrs::id.eq(item_id))
        .select(adrs::board_id)
        .first::<Option<Uuid>>(conn)
        .optional()?
    {
        return Ok(board_id);
    }
    Ok(None)
}

/// Which board authorizes writes to `item_id` (KAIROS-A-0006 access-check
/// flow):
///
/// - board items (strategies/initiatives/tasks/ADRs-on-a-board) authorize
///   against their OWN `board_id`;
/// - a document that NAMES a board authorizes against that board
///   (`documents.board_id`, COLLIERY-T-0269): the board is the owner of the
///   document. What the document supports does not change that;
/// - a document that names no board inherits from its parent entity: the
///   `supports` edge where the document is the TARGET and the parent is
///   the SOURCE (S-0004 edge semantics: "target supports source — document
///   supports initiative"), resolved to the parent's board. Should a
///   document support several items, the earliest-created edge whose
///   parent resolves to a board wins (deterministic; A-0006 assumes one
///   parent);
/// - `None` = no board context exists (unknown id, an ADR not placed on a
///   board, or a document with no board and no resolvable parent). Callers
///   fall back to the org-admin-only policy for tenant-wide resources
///   ([`kairos_core::abac::TenantConfigResource`]).
///
/// An `impacts` link is not read here (COLLIERY-T-0269). It says what a
/// document is about, and it gives no right.
///
/// Archived items resolve exactly as they did while live — see
/// [`board_of_workflow_item`] for why that is load bearing.
pub fn resolve_authorization_board(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<Uuid>, AbacError> {
    use crate::schema::documents;

    if let Some(board_id) = board_of_workflow_item(conn, item_id)? {
        return Ok(Some(board_id));
    }

    // Not a board item — a document? (An off-board ADR also lands here and
    // correctly resolves to None: it is not in `documents`.)
    let owner: Option<Option<Uuid>> = documents::table
        .filter(documents::id.eq(item_id))
        .select(documents::board_id)
        .first(conn)
        .optional()?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    // COLLIERY-T-0269: the board that the document names is its owner.
    if let Some(board_id) = owner {
        return Ok(Some(board_id));
    }

    // COLLIERY-T-0235: the parents come from the ONE function that the
    // "last parent" rule counts with, so the two cannot disagree on which
    // edge is a parent.
    Ok(document_parents(conn, item_id)?
        .first()
        .map(|parent| parent.board_id))
}

/// The board that a document NAMES as its owner (COLLIERY-T-0269), read
/// from the row. `None` = no such document, or the document names no
/// board. An archived document answers as it did while live.
///
/// It is not [`resolve_authorization_board`]: that function gives the
/// board of a parent when the document names none. The rules "the last
/// parent" and "the last owner" need to know which of the two the
/// document has.
pub fn document_owner_board(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<Option<Uuid>, AbacError> {
    use crate::schema::documents;

    Ok(documents::table
        .filter(documents::id.eq(document_id))
        .select(documents::board_id)
        .first::<Option<Uuid>>(conn)
        .optional()?
        .flatten())
}

/// One parent of a document (COLLIERY-T-0235): the source of a `supports`
/// edge that points at the document, and the board that the parent gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentParent {
    /// The workflow item that the document supports.
    pub parent_id: Uuid,
    /// The board of that item.
    pub board_id: Uuid,
}

/// The parents of a document, the EARLIEST first (COLLIERY-T-0235). The
/// first one gives the document its authorization board
/// ([`resolve_authorization_board`]), when the document names no board
/// of its own (COLLIERY-T-0269).
///
/// A parent is the source of a `supports` edge that points at the
/// document, when that source has a board. An ARCHIVED parent is a parent:
/// it gives its board as it did while live ([`board_of_workflow_item`]).
/// So the rule "a document always has a parent" counts an edge to an
/// archived item, and the archive of the only parent of a document does
/// not make the document an orphan.
///
/// A document with no parent and no board has no authorization board:
/// only its creator and an organization admin can edit it.
pub fn document_parents(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<Vec<DocumentParent>, AbacError> {
    use crate::schema::item_relationships;

    let sources: Vec<Uuid> = item_relationships::table
        .filter(item_relationships::target_id.eq(document_id))
        .filter(item_relationships::relationship.eq(RelationshipType::Supports))
        .order(item_relationships::created_at.asc())
        .select(item_relationships::source_id)
        .load(conn)?;
    let mut parents = Vec::with_capacity(sources.len());
    for parent_id in sources {
        if let Some(board_id) = board_of_workflow_item(conn, parent_id)? {
            parents.push(DocumentParent {
                parent_id,
                board_id,
            });
        }
    }
    Ok(parents)
}

/// Lock the row of a document until the transaction ends
/// (COLLIERY-T-0235). `false` = no such document.
///
/// WHY. The remove of a `supports` edge counts the parents of the document
/// and then deletes one edge. Two removes at the same time, of the two
/// edges of one document, would each count two parents, and the two
/// deletes would leave a document with no parent. With the lock the second
/// remove waits for the first, and counts one.
///
/// COLLIERY-T-0269: the change of the owner board takes the same lock. The
/// remove of the last parent reads the board of the document, and the
/// remove of the board counts the parents. Without the lock the two could
/// each see the owner that the other removes.
pub fn lock_document(conn: &mut PgConnection, document_id: Uuid) -> Result<bool, AbacError> {
    use crate::schema::documents;

    let locked: Option<Uuid> = documents::table
        .filter(documents::id.eq(document_id))
        .select(documents::id)
        .for_update()
        .first(conn)
        .optional()?;
    Ok(locked.is_some())
}

/// Who created a workflow item or document, if it exists (KAIROS-T-0111).
/// Since COLLIERY-T-0228 it is the first fact of the edit rule
/// ([`edit_facts`]); until then it was one arm of the rule for `parent` and
/// `blocks` edges, and looked at the source only. Items span the five
/// entity tables in one UUID space.
///
/// Archived items answer too (KAIROS-A-0020): who made a thing is a fact
/// about the row, not about whether it is still on a board. The edit rule
/// needs that: the creator of an item can restore it.
pub fn item_created_by(conn: &mut PgConnection, item_id: Uuid) -> Result<Option<Uuid>, AbacError> {
    use crate::schema::{adrs, documents, initiatives, strategies, tasks};
    macro_rules! try_table {
        ($table:ident) => {
            if let Some(creator) = $table::table
                .filter($table::id.eq(item_id))
                .select($table::created_by)
                .first::<Uuid>(conn)
                .optional()?
            {
                return Ok(Some(creator));
            }
        };
    }
    try_table!(strategies);
    try_table!(initiatives);
    try_table!(tasks);
    try_table!(documents);
    try_table!(adrs);
    Ok(None)
}

/// The facts of the edit rule for one principal and one item
/// (COLLIERY-T-0228), and the authorization board that was asked. The
/// decision is [`kairos_core::abac::may_edit_item`]; this function only
/// loads what it needs.
///
/// `manage_capability` is the `manage_<type>` capability of the item type.
/// The board is [`resolve_authorization_board`]: the board of the item, or
/// for a document the board that it names, or the board of its `supports`
/// parent when it names none (COLLIERY-T-0269). With no board,
/// `holds_manage` is `false`: only the creator and an admin can edit such
/// an item.
///
/// The creator is read from the row and not from the board, so the answer
/// does not change when the item moves to a different board. An unknown
/// item has no creator, no board, and so no fact but the admin role.
///
/// The three facts are loaded each time, with no early return. The cost is
/// a small number of indexed reads, and a caller that refuses can then say
/// which capability is missing, on which board.
pub fn edit_facts(
    conn: &mut PgConnection,
    org_slug: &str,
    user_id: Uuid,
    item_id: Uuid,
    manage_capability: &str,
) -> Result<(kairos_core::abac::EditFacts, Option<Uuid>), AbacError> {
    let created_item = item_created_by(conn, item_id)? == Some(user_id);
    let is_org_admin = is_org_admin(conn, org_slug, user_id)?;
    let board = resolve_authorization_board(conn, item_id)?;
    let holds_manage = match board {
        Some(board_id) => check_capability(conn, board_id, user_id, manage_capability)?,
        None => false,
    };
    Ok((
        kairos_core::abac::EditFacts {
            created_item,
            holds_manage,
            is_org_admin,
        },
        board,
    ))
}

/// One item, and the facts of the edit rule for one principal
/// (COLLIERY-T-0234): a row of [`edit_facts_of_items`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEditFacts {
    /// The type of the item.
    pub item_type: ItemType,
    /// The short code of the item.
    pub short_code: String,
    /// The item is not archived.
    pub live: bool,
    /// The three facts. The decision is [`kairos_core::abac::may_edit_item`].
    pub facts: kairos_core::abac::EditFacts,
    /// The `manage_<type>` capability that `facts.holds_manage` is about.
    pub manage_capability: &'static str,
    /// The authorization board that was asked, if the item has one.
    pub board_id: Option<Uuid>,
}

#[derive(QueryableByName)]
struct ItemFactRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    board_id: Option<Uuid>,
    #[diesel(sql_type = Bool)]
    live: bool,
}

/// [`edit_facts`] for MANY items and one principal (COLLIERY-T-0234): the
/// same three facts for each item, loaded for the set and not for each
/// item. The cascade of an archive calls it with each descendant of the
/// root.
///
/// The cost does not grow with the number of items:
///
/// - one read of the five entity tables for the creator, the board, the
///   type and the short code of each item,
/// - one read for the admin role,
/// - one [`check_capability`] for each different pair of board and
///   capability. It is the function that [`edit_facts`] calls, so the two
///   cannot give different answers on a grant, a glob or a team.
///
/// A document that names no board is the exception. Its authorization
/// board is the board of its `supports` parent
/// ([`resolve_authorization_board`]), which is a read for each such
/// document. A document that names a board has the board in its row
/// (COLLIERY-T-0269). No `parent` edge has a document at an end
/// (`kairos_core::graph::check_link`), so the cascade sends none.
///
/// As [`edit_facts`] does, this function loads each fact with no early
/// return, so the caller can say which capability is missing, on which
/// board. An id that is in no table has no row in the answer: the caller
/// must read that as "cannot edit".
pub fn edit_facts_of_items(
    conn: &mut PgConnection,
    org_slug: &str,
    user_id: Uuid,
    item_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, ItemEditFacts>, AbacError> {
    use std::collections::HashMap;

    let mut facts_of = HashMap::new();
    if item_ids.is_empty() {
        return Ok(facts_of);
    }

    let rows: Vec<ItemFactRow> = sql_query(
        r"SELECT id, 'strategy' AS entity_type, short_code, created_by,
                 board_id, deleted_at IS NULL AS live
            FROM strategies WHERE id = ANY($1)
          UNION ALL
          SELECT id, 'initiative', short_code, created_by,
                 board_id, deleted_at IS NULL
            FROM initiatives WHERE id = ANY($1)
          UNION ALL
          SELECT id, 'task', short_code, created_by,
                 board_id, deleted_at IS NULL
            FROM tasks WHERE id = ANY($1)
          UNION ALL
          SELECT id, 'adr', short_code, created_by,
                 board_id, deleted_at IS NULL
            FROM adrs WHERE id = ANY($1)
          UNION ALL
          SELECT id, 'document', short_code, created_by,
                 board_id, deleted_at IS NULL
            FROM documents WHERE id = ANY($1)",
    )
    .bind::<Array<SqlUuid>, _>(item_ids)
    .load(conn)?;

    let is_org_admin = is_org_admin(conn, org_slug, user_id)?;
    let mut held: HashMap<(Uuid, &'static str), bool> = HashMap::new();

    for row in rows {
        let Some(item_type) = ItemType::ALL
            .iter()
            .copied()
            .find(|t| t.entity_type() == row.entity_type)
        else {
            continue;
        };
        let manage_capability = kairos_core::abac::manage_capability(item_type);
        let board_id = match (item_type, row.board_id) {
            // COLLIERY-T-0269: only a document that names no board.
            (ItemType::Document, None) => resolve_authorization_board(conn, row.id)?,
            (_, board_id) => board_id,
        };
        let holds_manage = match board_id {
            Some(board_id) => match held.get(&(board_id, manage_capability)) {
                Some(answer) => *answer,
                None => {
                    let answer = check_capability(conn, board_id, user_id, manage_capability)?;
                    held.insert((board_id, manage_capability), answer);
                    answer
                }
            },
            None => false,
        };
        facts_of.insert(
            row.id,
            ItemEditFacts {
                item_type,
                short_code: row.short_code,
                live: row.live,
                facts: kairos_core::abac::EditFacts {
                    created_item: row.created_by == user_id,
                    holds_manage,
                    is_org_admin,
                },
                manage_capability,
                board_id,
            },
        );
    }
    Ok(facts_of)
}
