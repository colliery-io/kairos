//! ABAC orchestration (KAIROS-T-0011, contract per KAIROS-A-0006, layering
//! per KAIROS-A-0009): the single-query capability check, org-admin bypass,
//! grant/revoke with `activity_log` rows, and document-inherits-parent-board
//! resolution. The pure matching semantics live in [`kairos_core::abac`];
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
use diesel::sql_types::{Bool, Text, Uuid as SqlUuid};
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
    #[error("capability must not be empty")]
    EmptyCapability,
    /// The `(board_id, user_id, capability)` grant already exists (composite
    /// PK) — the grant is unchanged and no activity row was written.
    #[error("capability {capability:?} is already granted to user {user_id} on board {board_id}")]
    AlreadyGranted {
        board_id: Uuid,
        user_id: Uuid,
        capability: String,
    },
    /// No such grant exists to revoke.
    #[error("capability {capability:?} is not granted to user {user_id} on board {board_id}")]
    GrantNotFound {
        board_id: Uuid,
        user_id: Uuid,
        capability: String,
    },
    /// Any other database error.
    #[error("database error: {0}")]
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
/// Whether the write is actually a Backlog-only task create is the
/// caller's decision (the server chooses to ask for `file_backlog` only in
/// that exact case); this answers "is this principal allowed to file on
/// this board at all".
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

/// The board a live workflow item (strategy/initiative/task/ADR) sits on.
/// `None` = not a workflow item, soft-deleted, or an ADR that is not placed
/// on a board.
fn board_of_workflow_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<Uuid>, DieselError> {
    use crate::schema::{adrs, initiatives, strategies, tasks};

    macro_rules! try_table {
        ($table:ident) => {
            if let Some(board_id) = $table::table
                .filter($table::id.eq(item_id))
                .filter($table::deleted_at.is_null())
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
        .filter(adrs::deleted_at.is_null())
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
/// - documents inherit from their parent entity: the `supports` edge where
///   the document is the TARGET and the parent is the SOURCE (S-0004 edge
///   semantics: "target supports source — document supports initiative"),
///   resolved to the parent's board. Should a document support several
///   items, the earliest-created edge whose parent resolves to a board wins
///   (deterministic; A-0006 assumes one parent);
/// - `None` = no board context exists (unknown/soft-deleted id, an ADR not
///   placed on a board, or a document with no resolvable parent). Callers
///   fall back to the org-admin-only policy for tenant-wide resources
///   ([`kairos_core::abac::TenantConfigResource`]).
pub fn resolve_authorization_board(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<Uuid>, AbacError> {
    use crate::schema::{documents, item_relationships};

    if let Some(board_id) = board_of_workflow_item(conn, item_id)? {
        return Ok(Some(board_id));
    }

    // Not a board item — a document? (An off-board ADR also lands here and
    // correctly resolves to None: it is not in `documents`.)
    let is_document: Option<Uuid> = documents::table
        .filter(documents::id.eq(item_id))
        .filter(documents::deleted_at.is_null())
        .select(documents::id)
        .first(conn)
        .optional()?;
    if is_document.is_none() {
        return Ok(None);
    }

    let parents: Vec<Uuid> = item_relationships::table
        .filter(item_relationships::target_id.eq(item_id))
        .filter(item_relationships::relationship.eq(RelationshipType::Supports))
        .order(item_relationships::created_at.asc())
        .select(item_relationships::source_id)
        .load(conn)?;
    for parent_id in parents {
        if let Some(board_id) = board_of_workflow_item(conn, parent_id)? {
            return Ok(Some(board_id));
        }
    }
    Ok(None)
}

/// Who created a live workflow item or document, if it exists
/// (KAIROS-T-0111): the "I created the source" arm of the collaborative
/// edge rule. Items span the five entity tables in one UUID space.
pub fn item_created_by(conn: &mut PgConnection, item_id: Uuid) -> Result<Option<Uuid>, AbacError> {
    use crate::schema::{adrs, documents, initiatives, strategies, tasks};
    macro_rules! try_table {
        ($table:ident) => {
            if let Some(creator) = $table::table
                .filter($table::id.eq(item_id))
                .filter($table::deleted_at.is_null())
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
