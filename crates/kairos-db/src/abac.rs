//! ABAC orchestration (KAIROS-T-0011, contract per KAIROS-A-0006, layering
//! per KAIROS-A-0009): the single-query capability check, org-admin bypass,
//! grant/revoke with `activity_log` rows, and the resolution of the board
//! of a document (its owner board). The pure matching semantics live in
//! [`kairos_core::abac`];
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
use crate::models::enums::{ActivityAction, OrgRole};
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
    /// The principal has the grant of `manage_structure` at this scope
    /// already (KAIROS-T-0337). No row was written.
    #[error("The user {user_id} has the capability \"manage_structure\" {} already.", scope_words(*board_id))]
    StructureAlreadyGranted {
        user_id: Uuid,
        board_id: Option<Uuid>,
    },
    /// No grant of `manage_structure` has this id (KAIROS-T-0337).
    #[error("No grant of the capability \"manage_structure\" has the id {0}.")]
    StructureGrantNotFound(Uuid),
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// The words for the scope of a grant of `manage_structure`.
fn scope_words(board_id: Option<Uuid>) -> String {
    match board_id {
        Some(board_id) => format!("on the board {board_id}"),
        None => "at the organization".to_string(),
    }
}

#[derive(QueryableByName)]
struct BoolRow {
    #[diesel(sql_type = Bool)]
    authorized: bool,
}

/// The KAIROS-A-0006 access check as ONE indexed query (no N+1): EXISTS over
/// `board_member_capabilities` (its primary key, `(board_id, user_id,
/// capability)`) with the
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
///
/// On an ADR board that a team owns, the implied set is wider
/// ([`kairos_core::abac::team_implies_on_adr_board`], bound as `$5`, tested
/// against `boards.board_level`): it adds `manage_adrs` (COLLIERY-T-3108).
/// The organization ADR board has no team, so the arm never matches there.
/// On a strategy or initiative board, the team (KAIROS-T-0440) gives what
/// board membership gives there (`$6`, `$7` below), with the same exception
/// for an archive and a restore.
///
/// Third arm (KAIROS-T-0410, KAIROS-A-0025 decision 1): on a board of the
/// level `strategy` or `initiative`, a principal who holds the member grant
/// ([`kairos_core::abac::MEMBER`] bound as `$8`, matched as each grant: so
/// `member` or `*`) is a member of it, and membership implies
/// `manage_<type>` of the board and `transition_items`
/// ([`kairos_core::abac::member_implies_on_strategy_board`] bound as `$6`,
/// [`kairos_core::abac::member_implies_on_initiative_board`] as `$7`).
/// Since KAIROS-T-0541 no other grant makes a member: until then any grant
/// did, so `transition_items` or `configure_boards` alone also edited.
/// Nothing is stored: the revoke of the member grant is the revocation.
/// The pure mirror is [`kairos_core::abac::grants_membership`].
/// Archive and restore do not take this arm: see
/// [`check_granted_capability`].
pub fn check_capability(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    required: &str,
) -> Result<bool, AbacError> {
    check_capability_with(conn, board_id, user_id, required, true)
}

/// [`check_capability`] without the membership arm of strategy and
/// initiative boards (KAIROS-T-0410, KAIROS-A-0025 decision 8): a grant, or
/// the team implication of delivery and ADR boards. The edit rule asks it
/// for an archive and a restore ([`kairos_core::abac::EditUse::Archive`]),
/// which stay explicit grants on strategy and initiative boards.
pub fn check_granted_capability(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    required: &str,
) -> Result<bool, AbacError> {
    check_capability_with(conn, board_id, user_id, required, false)
}

/// The one query of [`check_capability`] and [`check_granted_capability`].
fn check_capability_with(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
    required: &str,
    membership: bool,
) -> Result<bool, AbacError> {
    // KAIROS-T-0536: the scope of the agent key of the request comes first.
    if !crate::key_scope::allows(user_id, required, Some(board_id)) {
        return Ok(false);
    }
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
            OR EXISTS (
                SELECT 1
                FROM boards b
                JOIN team_members tm ON tm.team_id = b.team_id
                WHERE b.id = $1
                  AND tm.user_id = $2
                  AND ($4 OR ($5 AND b.board_level = 'adr')
                       OR ($6 AND b.board_level = 'strategy')
                       OR ($7 AND b.board_level = 'initiative'))
            )
            OR EXISTS (
                SELECT 1
                FROM boards b
                WHERE b.id = $1
                  AND (($6 AND b.board_level = 'strategy')
                       OR ($7 AND b.board_level = 'initiative'))
                  AND EXISTS (
                      SELECT 1 FROM board_member_capabilities
                      WHERE board_id = $1
                        AND user_id = $2
                        AND ($8 LIKE replace(replace(replace(replace(
                                 capability, '\', '\\'), '%', '\%'), '_', '\_'), '*', '%')
                             OR capability = '*')
                  )
            )
          ) AS authorized",
    )
    .bind::<SqlUuid, _>(board_id)
    .bind::<SqlUuid, _>(user_id)
    .bind::<Text, _>(required)
    .bind::<Bool, _>(kairos_core::abac::team_implies(required))
    .bind::<Bool, _>(kairos_core::abac::team_implies_on_adr_board(required))
    .bind::<Bool, _>(membership && kairos_core::abac::member_implies_on_strategy_board(required))
    .bind::<Bool, _>(membership && kairos_core::abac::member_implies_on_initiative_board(required))
    .bind::<Text, _>(kairos_core::abac::MEMBER)
    .get_result(conn)?;
    Ok(row.authorized)
}

/// Whether `user_id` is a MEMBER of the board: it holds any grant on it
/// (KAIROS-T-0410, KAIROS-A-0025 decision 1). On a strategy or an initiative
/// board, a member may claim an item with no owner (KAIROS-T-0411). The
/// third arm of [`check_capability`] is narrower since KAIROS-T-0541: only
/// the member grant ([`kairos_core::abac::MEMBER`]) gives the edit and the
/// move. This test, for the claim and the hand-off, still counts any grant.
pub fn is_board_member(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AbacError> {
    use crate::schema::board_member_capabilities::dsl;
    let member: bool = diesel::select(diesel::dsl::exists(
        dsl::board_member_capabilities
            .filter(dsl::board_id.eq(board_id))
            .filter(dsl::user_id.eq(user_id)),
    ))
    .get_result(conn)?;
    Ok(member)
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
    // KAIROS-T-0536: the scope of the agent key of the request limits each
    // way in, the admin role too. A scope never adds a right.
    if !crate::key_scope::allows(user_id, required, Some(board_id)) {
        return Ok(false);
    }
    if is_org_admin(conn, org_slug, user_id)? {
        return Ok(true);
    }
    if required == kairos_core::abac::FILE_BACKLOG {
        return check_file_backlog(conn, org_slug, board_id, user_id);
    }
    if check_capability(conn, board_id, user_id, required)? {
        return Ok(true);
    }
    // KAIROS-T-0337: a grant of `manage_structure` on this board or at the
    // organization covers the configuration and the members of the board.
    Ok(kairos_core::abac::structure_covers(required)
        && holds_structure(conn, user_id, Some(board_id))?)
}

// ---------------------------------------------------------------------------
// The structure capability (KAIROS-T-0337)
// ---------------------------------------------------------------------------

/// One grant of `manage_structure` ([`kairos_core::abac::MANAGE_STRUCTURE`]):
/// a row of `structure_grants`. `board_id` `None` is a grant at the
/// organization.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = crate::schema::structure_grants)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct StructureGrant {
    pub id: Uuid,
    pub user_id: Uuid,
    pub board_id: Option<Uuid>,
    pub granted_by: Uuid,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

/// Whether `user_id` holds `manage_structure` at the scope `board_id`.
///
/// - `None`: the organization. Only a grant at the organization counts.
/// - `Some(board)`: that board. A grant at the organization or a grant on
///   that board counts. A grant on a different board does not.
///
/// This does not look at the role: the caller checks the organization
/// admin first, as [`authorize`] does. No glob is read: the grants are not
/// rows of `board_member_capabilities`.
pub fn holds_structure(
    conn: &mut PgConnection,
    user_id: Uuid,
    board_id: Option<Uuid>,
) -> Result<bool, AbacError> {
    use crate::schema::structure_grants::dsl;
    let query = dsl::structure_grants.filter(dsl::user_id.eq(user_id));
    let held: bool = match board_id {
        None => diesel::select(diesel::dsl::exists(query.filter(dsl::board_id.is_null())))
            .get_result(conn)?,
        Some(board_id) => diesel::select(diesel::dsl::exists(
            query.filter(dsl::board_id.is_null().or(dsl::board_id.eq(board_id))),
        ))
        .get_result(conn)?,
    };
    Ok(held)
}

/// Each grant of `manage_structure` of the tenant, on live boards and at
/// the organization: the grants at the organization first, then by board,
/// then by the time of the grant.
pub fn list_structure_grants(conn: &mut PgConnection) -> Result<Vec<StructureGrant>, AbacError> {
    use crate::schema::{boards, structure_grants as sg};
    Ok(sg::table
        .left_join(boards::table)
        .filter(sg::board_id.is_null().or(boards::deleted_at.is_null()))
        .order((sg::board_id.asc().nulls_first(), sg::granted_at.asc()))
        .select(StructureGrant::as_select())
        .load(conn)?)
}

/// Grant `manage_structure` to `user_id` at the scope `board_id` (`None` =
/// the organization), and write the `capability_grant` activity row with
/// `granted_by` as the actor, in one transaction. A second grant at the same
/// scope is [`AbacError::StructureAlreadyGranted`].
///
/// The caller decides who may grant: an organization admin only
/// (KAIROS-T-0337, decision 2). A holder of the capability cannot grant it.
pub fn grant_structure(
    conn: &mut PgConnection,
    user_id: Uuid,
    board_id: Option<Uuid>,
    granted_by: Uuid,
) -> Result<StructureGrant, AbacError> {
    use crate::schema::structure_grants::dsl;
    conn.transaction::<_, AbacError, _>(|conn| {
        let inserted = diesel::insert_into(dsl::structure_grants)
            .values((
                dsl::user_id.eq(user_id),
                dsl::board_id.eq(board_id),
                dsl::granted_by.eq(granted_by),
            ))
            .returning(StructureGrant::as_returning())
            .get_result(conn);
        let grant = match inserted {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(AbacError::StructureAlreadyGranted { user_id, board_id });
            }
            other => other?,
        };
        log_structure_activity(conn, granted_by, ActivityAction::CapabilityGrant, &grant)?;
        Ok(grant)
    })
}

/// Revoke the grant `grant_id` of `manage_structure`, and write the
/// `capability_revoke` activity row with `revoked_by` as the actor, in one
/// transaction. An unknown id is [`AbacError::StructureGrantNotFound`].
pub fn revoke_structure(
    conn: &mut PgConnection,
    grant_id: Uuid,
    revoked_by: Uuid,
) -> Result<StructureGrant, AbacError> {
    use crate::schema::structure_grants::dsl;
    conn.transaction::<_, AbacError, _>(|conn| {
        let grant = diesel::delete(dsl::structure_grants.filter(dsl::id.eq(grant_id)))
            .returning(StructureGrant::as_returning())
            .get_result(conn)
            .optional()?
            .ok_or(AbacError::StructureGrantNotFound(grant_id))?;
        log_structure_activity(conn, revoked_by, ActivityAction::CapabilityRevoke, &grant)?;
        Ok(grant)
    })
}

/// The activity row of a grant or a revoke of `manage_structure`. A grant on
/// a board is an entry of the board (as a board capability is); a grant at
/// the organization is an entry of the type `organization`, with no entity.
fn log_structure_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    grant: &StructureGrant,
) -> Result<(), DieselError> {
    let (entity_type, scope) = match grant.board_id {
        Some(board_id) => ("board", format!("board:{board_id}")),
        None => ("organization", "organization".to_string()),
    };
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: grant.board_id,
            entity_type: Some(entity_type.to_string()),
            details: format!(
                "capability:{} user:{} scope:{scope}",
                kairos_core::abac::MANAGE_STRUCTURE,
                grant.user_id
            ),
        })
        .execute(conn)?;
    Ok(())
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
/// - a document authorizes against its OWNER board (`documents.board_id`,
///   COLLIERY-T-0269). Each document has one (COLLIERY-T-3109). What the
///   document supports does not change that. Until COLLIERY-T-3109 a
///   document could name no board, and then took the board of its earliest
///   `supports` parent;
/// - `None` = no board context exists (unknown id, or an ADR not placed on
///   a board). Callers
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
    Ok(documents::table
        .filter(documents::id.eq(item_id))
        .select(documents::board_id)
        .first::<Uuid>(conn)
        .optional()?)
}

/// The creator and the authorization board of one item, in ONE query on
/// the table of `item_type` (KAIROS-T-0625). The caller knows the type: it
/// loaded the item. An id that is not in that table gives `None`.
///
/// The board is that of [`resolve_authorization_board`]: the board of a
/// strategy, an initiative or a task, the board of an ADR (`None` for an ADR
/// that is not placed on a board), the owner board of a document. Archived
/// rows answer too, as there (KAIROS-A-0020).
pub fn creator_and_board(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<Option<(Uuid, Option<Uuid>)>, AbacError> {
    use crate::schema::{adrs, documents, initiatives, strategies, tasks};
    macro_rules! read {
        ($table:ident) => {
            $table::table
                .filter($table::id.eq(item_id))
                .select(($table::created_by, $table::board_id.nullable()))
                .first::<(Uuid, Option<Uuid>)>(conn)
                .optional()?
        };
    }
    Ok(match item_type {
        ItemType::Strategy => read!(strategies),
        ItemType::Initiative => read!(initiatives),
        ItemType::Task => read!(tasks),
        ItemType::Document => read!(documents),
        ItemType::Adr => adrs::table
            .filter(adrs::id.eq(item_id))
            .select((adrs::created_by, adrs::board_id))
            .first::<(Uuid, Option<Uuid>)>(conn)
            .optional()?,
    })
}

/// The facts of the edit rule for one principal and one item
/// (COLLIERY-T-0228), and the authorization board that was asked. The
/// decision is [`kairos_core::abac::may_edit_item`]; this function only
/// loads what it needs.
///
/// The capability is the `manage_<type>` capability of `item_type`
/// ([`kairos_core::abac::manage_capability`]). The creator and the board
/// are [`creator_and_board`]: one query on the table of `item_type`
/// (KAIROS-T-0625). With no board, `holds_manage` is `false`: only the
/// creator and an admin can edit such an item.
///
/// `is_org_admin` is the role of the principal in the organization, which
/// the caller has from the request (the tenant layer reads it, KAIROS-T-0625).
/// It is not read again here.
///
/// The creator is read from the row and not from the board, so the answer
/// does not change when the item moves to a different board. An unknown
/// item, or an id that is not of `item_type`, has no creator, no board, and
/// so no fact but the admin role.
///
/// The capability is asked each time there is a board, with no early
/// return, so a caller that refuses can say which capability is missing,
/// on which board. One edit is two queries: the row, and the capability.
///
/// `edit_use` (KAIROS-T-0410): for [`kairos_core::abac::EditUse::Edit`],
/// `holds_manage` is [`check_capability`], so membership of a strategy or
/// initiative board counts. For an archive or a restore it is
/// [`check_granted_capability`]: only a grant counts.
pub fn edit_facts(
    conn: &mut PgConnection,
    user_id: Uuid,
    is_org_admin: bool,
    item_type: ItemType,
    item_id: Uuid,
    edit_use: kairos_core::abac::EditUse,
) -> Result<(kairos_core::abac::EditFacts, Option<Uuid>), AbacError> {
    let manage_capability = kairos_core::abac::manage_capability(item_type);
    let (creator, board) = creator_and_board(conn, item_type, item_id)?.unzip();
    let board = board.flatten();
    // KAIROS-T-0536: outside the scope of the agent key of the request, no
    // fact counts: not the creator, not the admin role, not the claim.
    if !crate::key_scope::allows(user_id, manage_capability, board) {
        return Ok((kairos_core::abac::EditFacts::default(), board));
    }
    let created_item = creator == Some(user_id);
    let holds_manage = match (board, edit_use) {
        (Some(board_id), kairos_core::abac::EditUse::Edit) => {
            check_capability(conn, board_id, user_id, manage_capability)?
        }
        (Some(board_id), kairos_core::abac::EditUse::Archive) => {
            check_granted_capability(conn, board_id, user_id, manage_capability)?
        }
        (None, _) => false,
    };
    // KAIROS-T-0532: the claim of a task counts for an edit, not for an
    // archive. Read only when the other facts refuse: one query less on
    // the common path.
    let holds_claim = !(created_item || holds_manage || is_org_admin)
        && item_type == ItemType::Task
        && edit_use == kairos_core::abac::EditUse::Edit
        && crate::task_claims::claim_of(conn, item_id)?
            .is_some_and(|claim| claim.user_id == user_id);
    Ok((
        kairos_core::abac::EditFacts {
            created_item,
            holds_manage,
            is_org_admin,
            holds_claim,
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
/// - one [`check_granted_capability`] for each different pair of board
///   and capability. It is the function that [`edit_facts`] calls for an
///   archive, so the two cannot give different answers on a grant, a glob
///   or a team. The cascade is an archive, so membership of a strategy or
///   initiative board does not count here (KAIROS-T-0410).
///
/// A document has its owner board in its row (COLLIERY-T-0269, required
/// since COLLIERY-T-3109). No `parent` edge has a document at an end
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
        let board_id = row.board_id;
        // KAIROS-T-0536: outside the scope of the agent key, no fact counts.
        if !crate::key_scope::allows(user_id, manage_capability, board_id) {
            facts_of.insert(
                row.id,
                ItemEditFacts {
                    item_type,
                    short_code: row.short_code,
                    live: row.live,
                    facts: kairos_core::abac::EditFacts::default(),
                    manage_capability,
                    board_id,
                },
            );
            continue;
        }
        let holds_manage = match board_id {
            Some(board_id) => match held.get(&(board_id, manage_capability)) {
                Some(answer) => *answer,
                None => {
                    let answer =
                        check_granted_capability(conn, board_id, user_id, manage_capability)?;
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
                    // The cascade of an archive: a claim gives no archive.
                    holds_claim: false,
                },
                manage_capability,
                board_id,
            },
        );
    }
    Ok(facts_of)
}
