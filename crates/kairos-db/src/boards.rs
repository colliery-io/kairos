//! Board orchestration (KAIROS-T-0010): create boards from the seeded
//! `system_board_defaults` configs, move items between columns, and manage
//! columns/transitions — all validated by the pure rules in
//! [`kairos_core::board`] (KAIROS-A-0009 layering: this module loads rows,
//! calls core rules, persists, and writes `activity_log`).
//!
//! Every function operates in the CURRENT `search_path` tenant schema
//! (unqualified table names, same convention as [`crate::tenant`]) and runs
//! in its own transaction, so callers get all-or-nothing semantics.
//!
//! # Activity logging
//!
//! - Item transitions: `action = 'transition'`, `entity_type` per item,
//!   `details = "column:<From>-><To>"` (KAIROS-A-0004).
//! - Board configuration changes (column add/rename/remove/reorder,
//!   transition add/remove): `action = 'board_config'`,
//!   `entity_id = board_id`, `entity_type = 'board'`.
//! - Board creation: `action = 'create'`, `entity_type = 'board'`.
//! - `actor` is `Option` on [`create_board`] only: tenant provisioning
//!   creates the default boards before any user exists (`actor_id` is NOT
//!   NULL, so system-provisioned boards write no activity row).

use std::collections::HashMap;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use kairos_core::board as rules;
use kairos_core::short_code::{self, ItemType};

use crate::models::boards::{Board, BoardColumn, NewBoard, NewBoardColumn, NewBoardTransition};
use crate::models::enums::{ActivityAction, BoardLevel};
use crate::models::graph::NewActivityLogEntry;
use crate::models::public::SystemBoardDefault;

/// Errors from board creation, item transitions, or board configuration.
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    /// No board with this id exists (or it is soft-deleted).
    #[error("The board {0} does not exist.")]
    BoardNotFound(Uuid),
    /// The rename of a move was refused (COLLIERY-T-3101).
    #[error(transparent)]
    Rename(#[from] crate::code_rename::RenameError),
    /// No column with this id exists.
    #[error("The column {0} does not exist.")]
    ColumnNotFound(Uuid),
    /// No live item of this type with this id exists.
    #[error("The {entity_type} {id} does not exist.")]
    ItemNotFound { entity_type: &'static str, id: Uuid },
    /// The item exists but is not placed on a board (ADRs may have NULL
    /// board_id/column_id).
    #[error("The {entity_type} {id} is not on a board.")]
    ItemNotOnBoard { entity_type: &'static str, id: Uuid },
    /// `move_task`: the target is the board the task is already on.
    #[error("The task is on the board {0} already.")]
    SameBoard(Uuid),
    /// `move_task`: tasks move between DELIVERY boards only.
    #[error("The board {0} is not a delivery board.")]
    NotDeliveryBoard(Uuid),
    /// `move_task`: the target board has no columns to land in.
    #[error(
        "The board {0} has no columns, so the task has no place on it. Add a column to the board."
    )]
    NoEntryColumn(Uuid),
    /// The transition edge to remove does not exist.
    #[error("The board {board_id} has no transition from {from} to {to}.")]
    TransitionNotFound {
        board_id: Uuid,
        from: Uuid,
        to: Uuid,
    },
    /// `create_board`: a delivery board was sent with no team
    /// (COLLIERY-T-0230). The board of a task decides its team
    /// (COLLIERY-T-0216), and any member can send a request to any delivery
    /// board (COLLIERY-T-0218). On a board with no team, a task has no team
    /// and a request goes to nobody.
    #[error("A delivery board needs a team. Send the team as team_id.")]
    DeliveryBoardNeedsTeam,
    /// `create_board`: the team has a live delivery board
    /// (COLLIERY-T-0240). A team has exactly one delivery board. Routing
    /// ([`crate::repositories::delivery_board_for_team`]) finds no board
    /// for a team that has two. Carries the board that the team has.
    #[error(
        "The team has the delivery board {board_name:?}. A team has only one delivery board. \
         Use that board."
    )]
    TeamHasDeliveryBoard {
        team_id: Uuid,
        board_id: Uuid,
        board_name: String,
        board_slug: String,
    },
    /// `create_board`: a board of the organization was sent with a team
    /// (COLLIERY-T-0242). The team of such a board is the list of its
    /// members, so it has no `team_id`.
    #[error("Only a delivery board has a team. Do not send team_id for a board of level {0}.")]
    OrganizationBoardHasNoTeam(BoardLevel),
    /// `check_board_delete`: the board is the only live delivery board of
    /// a live team (COLLIERY-T-0241). The delete of the team removes the
    /// team and its board together.
    #[error(
        "This board is the only delivery board of the team {team_name:?}. A team always has a \
         delivery board. To remove the board, delete the team."
    )]
    LastDeliveryBoard {
        board_id: Uuid,
        team_id: Uuid,
        team_name: String,
    },
    /// `check_board_team`: an update sent a team that is not the team of
    /// the board (COLLIERY-T-0243). The team of a board never changes.
    #[error(
        "The team of a board does not change. To give the work to a different team, move the \
         task to the board of that team."
    )]
    BoardTeamIsFixed {
        board_id: Uuid,
        team_id: Option<Uuid>,
    },
    /// `create_board`, `check_board_slug`: a live board has the slug
    /// (COLLIERY-T-0255). Two live boards cannot have the same slug: a
    /// board is addressed by its slug. Carries the board that has it.
    #[error(
        "The live board {board_name:?} has the slug {slug:?}. Two live boards cannot have the \
         same slug. Send a different slug."
    )]
    SlugTaken {
        slug: String,
        board_id: Uuid,
        board_name: String,
    },
    /// `create_board`: the prefix does not match
    /// [`kairos_core::short_code::PREFIX_RULE`] (COLLIERY-T-3099).
    #[error(
        "The prefix {0:?} is not correct. A board prefix must match {rule}: a capital letter, \
         then 1 to 9 capital letters or digits. Send a different code_prefix.",
        rule = kairos_core::short_code::PREFIX_RULE
    )]
    InvalidCodePrefix(String),
    /// `create_board`: a live board of the same level has the prefix
    /// (COLLIERY-T-3099). The level of a board gives the types that it
    /// holds, and each pair (prefix, type) is unique among the live boards,
    /// so that a code names one board. Carries the board that has it.
    #[error(
        "The live board {board_slug:?} has the prefix {code_prefix:?} for {}. Two live \
         boards cannot have the same prefix for the same type. Send a different code_prefix.",
        held_types(*level)
    )]
    CodePrefixTaken {
        code_prefix: String,
        level: BoardLevel,
        board_id: Uuid,
        board_name: String,
        board_slug: String,
    },
    /// No `system_board_defaults` row is seeded for this level.
    #[error("The table system_board_defaults has no row for the level {0}.")]
    MissingDefaults(BoardLevel),
    /// The seeded `system_board_defaults` row is malformed.
    #[error("The row of system_board_defaults for the level {level} is not correct. {source}")]
    InvalidDefaults {
        level: BoardLevel,
        source: rules::DefaultConfigError,
    },
    /// The requested item move is not allowed by the board's transition
    /// graph; carries the allowed target columns (S-0006 REQ-1.4).
    #[error(transparent)]
    Transition(#[from] rules::TransitionError),
    /// A board configuration rule was violated.
    #[error(transparent)]
    Rule(#[from] rules::ColumnRuleError),
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// Insert one `activity_log` row (current tenant schema).
fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    entity_id: Option<Uuid>,
    entity_type: &str,
    details: String,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id,
            entity_type: Some(entity_type.to_string()),
            details,
        })
        .execute(conn)?;
    Ok(())
}

/// Load a board's LIVE columns and the transition edges between them, as
/// core rule inputs. Errors with [`BoardError::BoardNotFound`] if the board
/// does not exist or is soft-deleted.
///
/// A removed column (KAIROS-T-0161) is not part of the board any more, so
/// it must not reach the rules: it would render, it would be offered as a
/// transition target, and `check_add_column` would refuse its name to the
/// admin who just removed it. Its edges are still in `board_transitions` —
/// the `ON DELETE CASCADE` that used to sweep them away never fires on a
/// soft delete — so they are **filtered here** against the live column set
/// rather than deleted. That is deliberate: removing a column no longer
/// destroys the board's wiring, so a future restore has something to
/// restore to.
fn load_board_rules(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<(Vec<rules::Column>, Vec<rules::Transition>), BoardError> {
    use crate::schema::{board_columns, board_transitions, boards};

    let board_exists: Option<Uuid> = boards::table
        .filter(boards::id.eq(board_id))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .optional()?;
    if board_exists.is_none() {
        return Err(BoardError::BoardNotFound(board_id));
    }

    let columns: Vec<rules::Column> = board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::deleted_at.is_null())
        .order(board_columns::position.asc())
        .select((
            board_columns::id,
            board_columns::name,
            board_columns::position,
        ))
        .load::<(Uuid, String, i32)>(conn)?
        .into_iter()
        .map(|(id, name, position)| rules::Column { id, name, position })
        .collect();

    let live: std::collections::HashSet<Uuid> = columns.iter().map(|c| c.id).collect();
    let transitions: Vec<rules::Transition> = board_transitions::table
        .filter(board_transitions::board_id.eq(board_id))
        .select((
            board_transitions::from_column_id,
            board_transitions::to_column_id,
        ))
        .load::<(Uuid, Uuid)>(conn)?
        .into_iter()
        .filter(|(from, to)| live.contains(from) && live.contains(to))
        .map(|(from_column_id, to_column_id)| rules::Transition {
            from_column_id,
            to_column_id,
        })
        .collect();

    Ok((columns, transitions))
}

fn column_name(columns: &[rules::Column], id: Uuid) -> Result<String, BoardError> {
    columns
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.name.clone())
        .ok_or(BoardError::ColumnNotFound(id))
}

// ---------------------------------------------------------------------------
// The team of a board (COLLIERY-T-0240, T-0241, T-0243)
// ---------------------------------------------------------------------------

/// Lock the row of a team until the transaction ends, and return the name
/// of the team. `None` when the team does not exist or is deleted.
///
/// Each function that counts the delivery boards of a team takes this lock
/// first. Two of them cannot then count at the same time and each write.
fn lock_team(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<String>, DieselError> {
    use crate::schema::teams::dsl;
    // The row of a deleted team is locked too: a lock is about the row,
    // and not about the state of the team.
    let team: Option<(String, Option<chrono::DateTime<chrono::Utc>>)> = dsl::teams
        .filter(dsl::id.eq(team_id))
        .select((dsl::name, dsl::deleted_at))
        .for_update()
        .first(conn)
        .optional()?;
    Ok(team.and_then(|(name, deleted_at)| deleted_at.is_none().then_some(name)))
}

/// The name of the partial unique index on the slug of a live board
/// (COLLIERY-T-0255, migration `boards_live_slug_uniqueness`).
const LIVE_SLUG_INDEX: &str = "boards_live_slug_key";

/// True for the refusal of the database that the index of the slug gives.
fn is_slug_violation(e: &DieselError) -> bool {
    matches!(
        e,
        DieselError::DatabaseError(diesel::result::DatabaseErrorKind::UniqueViolation, info)
            if info.constraint_name() == Some(LIVE_SLUG_INDEX)
    )
}

/// The live board that has a slug, if one has it. `except` is the board of
/// an update: a board is not in conflict with itself.
fn live_board_with_slug(
    conn: &mut PgConnection,
    slug: &str,
    except: Option<Uuid>,
) -> Result<Option<Board>, DieselError> {
    use crate::schema::boards::dsl;
    let mut query = dsl::boards
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(Board::as_select())
        .into_boxed();
    if let Some(except) = except {
        query = query.filter(dsl::id.ne(except));
    }
    query.first(conn).optional()
}

/// Refuse a slug that a live board has (COLLIERY-T-0255), with
/// [`BoardError::SlugTaken`], which names that board. For the create of a
/// board, and for an update that sends a slug (`except` is the board of
/// the update).
///
/// The check gives the refusal that a person can read. The index
/// `boards_live_slug_key` is the rule: two writes at the same time can each
/// pass this check, and the index refuses the second.
/// [`slug_violation`] gives that refusal the same form.
pub fn check_board_slug(
    conn: &mut PgConnection,
    slug: &str,
    except: Option<Uuid>,
) -> Result<(), BoardError> {
    match live_board_with_slug(conn, slug, except)? {
        Some(board) => Err(BoardError::SlugTaken {
            slug: slug.to_string(),
            board_id: board.id,
            board_name: board.name,
        }),
        None => Ok(()),
    }
}

/// The error of a write of a board, with the refusal of the index of the
/// slug as [`BoardError::SlugTaken`] (COLLIERY-T-0255). Each other error
/// stays as it is.
///
/// Call it when the failed statement is rolled back: after the transaction
/// of the write, or after a statement that is in no transaction. It reads
/// the board that has the slug.
pub fn slug_violation(
    conn: &mut PgConnection,
    slug: &str,
    except: Option<Uuid>,
    e: DieselError,
) -> BoardError {
    if !is_slug_violation(&e) {
        return BoardError::Database(e);
    }
    match check_board_slug(conn, slug, except) {
        Err(taken) => taken,
        // The board that had the slug is not there now. The caller can
        // send the write again.
        Ok(()) => BoardError::Database(e),
    }
}

/// The live delivery boards of a team, oldest first. One, by the rule. Old
/// data can have more (COLLIERY-T-0240).
fn live_delivery_boards(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<Board>, DieselError> {
    use crate::schema::boards::dsl;
    dsl::boards
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::board_level.eq(BoardLevel::Delivery))
        .filter(dsl::deleted_at.is_null())
        .order((dsl::created_at.asc(), dsl::id.asc()))
        .select(Board::as_select())
        .load(conn)
}

/// The live delivery boards of a team, for the delete of the team
/// (COLLIERY-T-0250), oldest first. The delete of a team removes each of
/// them, and old data can have more than one (COLLIERY-T-0240).
///
/// Call it in the transaction that deletes the team. It locks the row of
/// the team, so a create of a board for the team cannot run between the
/// read and the delete.
pub fn delivery_boards_for_team_delete(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Vec<Board>, DieselError> {
    lock_team(conn, team_id)?;
    live_delivery_boards(conn, team_id)
}

/// A team with 2 or more live delivery boards (COLLIERY-T-0250): one row
/// of [`teams_with_several_delivery_boards`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeveralDeliveryBoards {
    pub team_id: Uuid,
    pub team_name: String,
    pub team_slug: String,
    /// `true` for a deleted team. Its boards are live: a delete of the team
    /// before COLLIERY-T-0250 left them.
    pub team_deleted: bool,
    /// The live delivery boards of the team, oldest first. 2 or more.
    pub boards: Vec<Board>,
}

/// Each team with 2 or more live delivery boards, by the slug of the team
/// (COLLIERY-T-0250). The rule is one board (COLLIERY-T-0240), and only
/// old data has more. A deleted team is in the list too.
///
/// It only reads, and it takes no lock.
pub fn teams_with_several_delivery_boards(
    conn: &mut PgConnection,
) -> Result<Vec<SeveralDeliveryBoards>, DieselError> {
    use crate::schema::boards::dsl;
    use crate::schema::teams::dsl as teams_dsl;
    let boards: Vec<Board> = dsl::boards
        .filter(dsl::team_id.is_not_null())
        .filter(dsl::board_level.eq(BoardLevel::Delivery))
        .filter(dsl::deleted_at.is_null())
        .order((dsl::created_at.asc(), dsl::id.asc()))
        .select(Board::as_select())
        .load(conn)?;
    let mut by_team: HashMap<Uuid, Vec<Board>> = HashMap::new();
    for board in boards {
        if let Some(team_id) = board.team_id {
            by_team.entry(team_id).or_default().push(board);
        }
    }
    by_team.retain(|_, boards| boards.len() > 1);
    let teams: Vec<(Uuid, String, String, Option<chrono::DateTime<chrono::Utc>>)> =
        teams_dsl::teams
            .filter(teams_dsl::id.eq_any(by_team.keys().copied().collect::<Vec<_>>()))
            .order((teams_dsl::slug.asc(), teams_dsl::id.asc()))
            .select((
                teams_dsl::id,
                teams_dsl::name,
                teams_dsl::slug,
                teams_dsl::deleted_at,
            ))
            .load(conn)?;
    Ok(teams
        .into_iter()
        .filter_map(|(team_id, team_name, team_slug, deleted_at)| {
            Some(SeveralDeliveryBoards {
                team_id,
                team_name,
                team_slug,
                team_deleted: deleted_at.is_some(),
                boards: by_team.remove(&team_id)?,
            })
        })
        .collect())
}

/// Refuse the delete of the only delivery board of a team
/// (COLLIERY-T-0241): [`BoardError::LastDeliveryBoard`]. A team always has
/// a delivery board, because the board gives a task its team
/// (COLLIERY-T-0216) and receives the requests to the team
/// (COLLIERY-T-0218).
///
/// These deletes are not refused:
///
/// - a board of the organization, which has no delivery team,
/// - a delivery board with no team, or with a deleted team (old data),
/// - one of the two delivery boards of a team (old data, COLLIERY-T-0240).
///
/// The delete of a team does not call this. It removes the team and each
/// delivery board of the team together
/// ([`delivery_boards_for_team_delete`], COLLIERY-T-0250), so no team is
/// left with no board.
///
/// Call it in the transaction that deletes the board. It locks the row of
/// the team, so two deletes cannot each see the board of the other.
pub fn check_board_delete(conn: &mut PgConnection, board: &Board) -> Result<(), BoardError> {
    let (BoardLevel::Delivery, Some(team_id)) = (board.board_level, board.team_id) else {
        return Ok(());
    };
    let Some(team_name) = lock_team(conn, team_id)? else {
        return Ok(());
    };
    let others = live_delivery_boards(conn, team_id)?
        .into_iter()
        .filter(|other| other.id != board.id)
        .count();
    if others == 0 {
        return Err(BoardError::LastDeliveryBoard {
            board_id: board.id,
            team_id,
            team_name,
        });
    }
    Ok(())
}

/// Refuse a change of the team of a board (COLLIERY-T-0243):
/// [`BoardError::BoardTeamIsFixed`]. The team of a board never changes. The
/// tasks of a board have the team of the board (COLLIERY-T-0216), and a
/// task goes to a different team when it moves to the board of that team.
///
/// `sent` is the `team_id` of the update: `None` when the update has no
/// `team_id`, `Some(None)` for a null. A `team_id` equal to the team of
/// the board is accepted and changes nothing, so a client can send back
/// the `team_id` that it read.
pub fn check_board_team(board: &Board, sent: Option<Option<Uuid>>) -> Result<(), BoardError> {
    match sent {
        Some(team_id) if team_id != board.team_id => Err(BoardError::BoardTeamIsFixed {
            board_id: board.id,
            team_id: board.team_id,
        }),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// The prefix of a board (COLLIERY-T-3099)
// ---------------------------------------------------------------------------

/// The prefix of a new board: the value that an admin sent, or a prefix
/// from the slug of the board for an entry point that has no admin to
/// choose (a SCIM group, the demo seed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePrefix<'a> {
    /// The prefix that the caller sent. It must match
    /// [`kairos_core::short_code::PREFIX_RULE`], and no live board of the
    /// same level can have it.
    Given(&'a str),
    /// [`free_code_prefix`] of the slug of the board.
    FromSlug,
}

/// The item types that a board of `level` holds (COLLIERY-T-3099). The sets
/// of the levels do not overlap, so a unique (prefix, level) of the live
/// boards gives a unique (prefix, type).
///
/// A document of a board of each level takes the prefix of its board. The
/// rule counts the documents for the delivery boards only: a document of a
/// different board shares the sequence of its prefix, so its code is
/// unique.
pub fn level_item_types(level: BoardLevel) -> &'static [ItemType] {
    match level {
        BoardLevel::Strategy => &[ItemType::Strategy],
        BoardLevel::Initiative => &[ItemType::Initiative],
        BoardLevel::Delivery => &[ItemType::Task, ItemType::Document],
        BoardLevel::Adr => &[ItemType::Adr],
    }
}

/// The words for the types of [`level_item_types`], for a refusal.
fn held_types(level: BoardLevel) -> &'static str {
    match level {
        BoardLevel::Strategy => "strategies",
        BoardLevel::Initiative => "initiatives",
        BoardLevel::Delivery => "tasks and documents",
        BoardLevel::Adr => "ADRs",
    }
}

/// The name of the partial unique index on (prefix, level) of a live board
/// (migration `board_code_prefixes`).
const LIVE_CODE_PREFIX_INDEX: &str = "boards_live_code_prefix_key";

/// The live board of `level` that has `prefix`, if one has it.
fn live_board_with_prefix(
    conn: &mut PgConnection,
    prefix: &str,
    level: BoardLevel,
) -> Result<Option<Board>, DieselError> {
    use crate::schema::boards::dsl;
    dsl::boards
        .filter(dsl::code_prefix.eq(prefix))
        .filter(dsl::board_level.eq(level))
        .filter(dsl::deleted_at.is_null())
        .select(Board::as_select())
        .first(conn)
        .optional()
}

/// Refuse a prefix for a new board of `level` (COLLIERY-T-3099): a prefix
/// that does not match the rule ([`BoardError::InvalidCodePrefix`]), or a
/// prefix that a live board of the same level has
/// ([`BoardError::CodePrefixTaken`], which names that board).
///
/// The index `boards_live_code_prefix_key` is the rule for two creates at
/// the same time, and [`create_board`] gives its refusal the same form.
pub fn check_code_prefix(
    conn: &mut PgConnection,
    prefix: &str,
    level: BoardLevel,
) -> Result<(), BoardError> {
    if !short_code::is_valid_prefix(prefix) {
        return Err(BoardError::InvalidCodePrefix(prefix.to_string()));
    }
    match live_board_with_prefix(conn, prefix, level)? {
        Some(board) => Err(BoardError::CodePrefixTaken {
            code_prefix: prefix.to_string(),
            level,
            board_id: board.id,
            board_name: board.name,
            board_slug: board.slug,
        }),
        None => Ok(()),
    }
}

/// A prefix for a new board of `level` from `slug`, that no live board of
/// that level has: [`kairos_core::short_code::prefix_from_slug`], with a
/// number at the end when a board has it (`WEB`, `WEB2`, `WEB3`, ...). The
/// rule of the migration `board_code_prefixes` for a board with no Metis
/// footer.
pub fn free_code_prefix(
    conn: &mut PgConnection,
    slug: &str,
    level: BoardLevel,
) -> Result<String, DieselError> {
    let base = short_code::prefix_from_slug(slug);
    let mut candidate = base.clone();
    let mut n = 2;
    while live_board_with_prefix(conn, &candidate, level)?.is_some() {
        candidate = short_code::numbered_prefix(&base, n);
        n += 1;
    }
    Ok(candidate)
}

// ---------------------------------------------------------------------------
// Board creation (defaults seeding, KAIROS-A-0002)
// ---------------------------------------------------------------------------

/// Which seeded columns start with the done flag (KAIROS-T-0080): the
/// terminal column of each workflow level, and both resting states for
/// ADRs (a superseded decision is as finished as a decided one). Applies
/// to the system defaults only — admins own the flag afterwards.
fn seeded_done_column(level: BoardLevel, name: &str) -> bool {
    match level {
        BoardLevel::Strategy | BoardLevel::Initiative | BoardLevel::Delivery => name == "Completed",
        BoardLevel::Adr => matches!(name, "Decided" | "Superseded"),
    }
}

/// Create a board in the current tenant schema, seeding its columns and
/// transitions from the tenant's `public.system_board_defaults` row for
/// `level` — the ONE implementation of default-board seeding: tenant
/// provisioning ([`crate::tenant::provision_tenant`]) calls this for the
/// strategy/initiative/adr boards, and team/delivery-board creation reuses
/// it later with [`BoardLevel::Delivery`] and a `team_id`.
///
/// `actor`: `Some(user)` logs `action='create'` to `activity_log`; `None`
/// is for system provisioning (no user exists yet).
///
/// # Every board has a team (COLLIERY-T-0230)
///
/// The rule has two forms, and this function enforces the first:
///
/// - A DELIVERY board has a delivery team: a row in `teams`, and `team_id`
///   on the board. A delivery board with no team is refused with
///   [`BoardError::DeliveryBoardNeedsTeam`], before any row is written.
/// - A board of the ORGANIZATION (strategy, initiative, adr) has no row in
///   `teams` and no `team_id`. Its team is the list of the members of the
///   board. Nothing is checked for these levels, so tenant provisioning
///   creates them as before.
///
/// The check is here, and not in a route, because every entry point meets
/// here: `POST /api/boards`, `POST /api/teams`, SCIM group creation, the
/// demo seed and tenant provisioning. A new entry point cannot forget it.
///
/// The check is on create only. No migration and no database constraint
/// goes with it: a delivery board with no team can exist in old data, and
/// readers must continue to accept that row.
///
/// # A team has one delivery board (COLLIERY-T-0240, COLLIERY-T-0242)
///
/// - A delivery board for a team that has a LIVE delivery board is refused
///   with [`BoardError::TeamHasDeliveryBoard`], which names that board. A
///   deleted board does not count: it is not a board of the team any more.
/// - A board of the organization with a `team_id` is refused with
///   [`BoardError::OrganizationBoardHasNoTeam`].
///
/// # A live board has its slug alone (COLLIERY-T-0255)
///
/// A slug that a live board has is refused with [`BoardError::SlugTaken`],
/// which names that board. A deleted board does not keep its slug. The
/// index `boards_live_slug_key` is the rule for two creates at the same
/// time, and its refusal has the same form.
///
/// Two creates at the same time must not each see no board. The
/// transaction thus locks the row of the team (`FOR UPDATE`) before it
/// looks for a board, and the second create waits for the first. A lock,
/// and not a unique index: a tenant can have a team with two delivery
/// boards in old data, and the migration that adds the index fails on that
/// tenant.
///
/// # Each board has a prefix (COLLIERY-T-3099)
///
/// `code_prefix` gives the short-code prefix of the board, which never
/// changes. A [`CodePrefix::Given`] prefix that does not match the rule is
/// refused with [`BoardError::InvalidCodePrefix`]. A prefix that a live
/// board of the same level has is refused with
/// [`BoardError::CodePrefixTaken`], which names that board: each pair
/// (prefix, type) is unique among the live boards
/// ([`level_item_types`]). The index `boards_live_code_prefix_key` is the
/// rule for two creates at the same time. The create also makes the row of
/// `short_code_sequences` for each type of the board, if it is not there.
pub fn create_board(
    conn: &mut PgConnection,
    level: BoardLevel,
    name: &str,
    slug: &str,
    code_prefix: CodePrefix<'_>,
    team_id: Option<Uuid>,
    actor: Option<Uuid>,
) -> Result<Board, BoardError> {
    if level == BoardLevel::Delivery && team_id.is_none() {
        return Err(BoardError::DeliveryBoardNeedsTeam);
    }
    if level != BoardLevel::Delivery && team_id.is_some() {
        return Err(BoardError::OrganizationBoardHasNoTeam(level));
    }
    if let CodePrefix::Given(prefix) = code_prefix
        && !short_code::is_valid_prefix(prefix)
    {
        return Err(BoardError::InvalidCodePrefix(prefix.to_string()));
    }
    let mut chosen_prefix = String::new();
    let created = conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::system_board_defaults;
        use crate::schema::{board_columns, board_transitions, boards};

        // COLLIERY-T-0255: two live boards cannot have the same slug.
        check_board_slug(conn, slug, None)?;

        // COLLIERY-T-3099: the prefix of the board.
        chosen_prefix = match code_prefix {
            CodePrefix::Given(prefix) => prefix.to_string(),
            CodePrefix::FromSlug => free_code_prefix(conn, slug, level)?,
        };
        check_code_prefix(conn, &chosen_prefix, level)?;

        if let Some(team_id) = team_id {
            lock_team(conn, team_id)?;
            if let Some(board) = live_delivery_boards(conn, team_id)?.into_iter().next() {
                return Err(BoardError::TeamHasDeliveryBoard {
                    team_id,
                    board_id: board.id,
                    board_name: board.name,
                    board_slug: board.slug,
                });
            }
        }

        let defaults: SystemBoardDefault = system_board_defaults::table
            .filter(system_board_defaults::board_level.eq(level))
            .first(conn)
            .optional()?
            .ok_or(BoardError::MissingDefaults(level))?;
        let config = rules::parse_default_config(&defaults.columns, &defaults.transitions)
            .map_err(|source| BoardError::InvalidDefaults { level, source })?;

        let board: Board = diesel::insert_into(boards::table)
            .values(NewBoard {
                name: name.to_string(),
                slug: slug.to_string(),
                board_level: level,
                team_id,
                code_prefix: chosen_prefix.clone(),
            })
            .returning(Board::as_returning())
            .get_result(conn)?;
        for item_type in level_item_types(level) {
            crate::items::ensure_code_sequence(conn, &board.code_prefix, *item_type)?;
        }

        let mut column_ids: HashMap<&str, Uuid> = HashMap::new();
        for (position, column) in config.columns.iter().enumerate() {
            let created: BoardColumn = diesel::insert_into(board_columns::table)
                .values(NewBoardColumn {
                    board_id: board.id,
                    name: column.clone(),
                    position: position as i32,
                    is_done: seeded_done_column(level, column),
                })
                .returning(BoardColumn::as_returning())
                .get_result(conn)?;
            column_ids.insert(column.as_str(), created.id);
        }

        for (from, to) in &config.transitions {
            // parse_default_config validated that every transition endpoint
            // is a listed column, so these lookups cannot miss.
            diesel::insert_into(board_transitions::table)
                .values(NewBoardTransition {
                    board_id: board.id,
                    from_column_id: column_ids[from.as_str()],
                    to_column_id: column_ids[to.as_str()],
                })
                .execute(conn)?;
        }

        if let Some(actor_id) = actor {
            log_activity(
                conn,
                actor_id,
                ActivityAction::Create,
                Some(board.id),
                "board",
                format!("board:{slug} level:{level}"),
            )?;
        }
        Ok(board)
    });
    // A create at the same time got the slug or the prefix first: the index
    // refused this one. The transaction is rolled back, so the read is
    // permitted.
    created.map_err(|e| match e {
        BoardError::Database(db) if is_code_prefix_violation(&db) => {
            match check_code_prefix(conn, &chosen_prefix, level) {
                Err(taken) => taken,
                Ok(()) => BoardError::Database(db),
            }
        }
        BoardError::Database(db) => slug_violation(conn, slug, None, db),
        e => e,
    })
}

/// True for the refusal of the database that the index of the prefix
/// gives.
fn is_code_prefix_violation(e: &DieselError) -> bool {
    matches!(
        e,
        DieselError::DatabaseError(diesel::result::DatabaseErrorKind::UniqueViolation, info)
            if info.constraint_name() == Some(LIVE_CODE_PREFIX_INDEX)
    )
}

// ---------------------------------------------------------------------------
// Item transitions (KAIROS-A-0002 existence check; A-0004 audit row)
// ---------------------------------------------------------------------------

/// Validate a move against the board's transition graph and return the
/// `(from, to)` column names for the audit details.
fn validate_transition(
    conn: &mut PgConnection,
    board_id: Uuid,
    from_column_id: Uuid,
    to_column_id: Uuid,
) -> Result<(String, String), BoardError> {
    let (columns, transitions) = load_board_rules(conn, board_id)?;
    rules::can_transition(&columns, &transitions, from_column_id, to_column_id)?;
    Ok((
        column_name(&columns, from_column_id)?,
        column_name(&columns, to_column_id)?,
    ))
}

/// Generate `transition_<entity>` for an item table with NOT NULL
/// `board_id`/`column_id` (strategies, initiatives, tasks): load the item's
/// board/column, validate via [`kairos_core::board::can_transition`], update
/// `column_id`, and append the `activity_log` transition row — one
/// transaction.
macro_rules! transition_item_fn {
    ($(#[$doc:meta])* $fn_name:ident, $table:ident, $entity_type:literal) => {
        $(#[$doc])*
        pub fn $fn_name(
            conn: &mut PgConnection,
            item_id: Uuid,
            to_column_id: Uuid,
            actor_id: Uuid,
        ) -> Result<(), BoardError> {
            conn.transaction::<_, BoardError, _>(|conn| {
                use crate::schema::$table::dsl;

                let placement: Option<(Uuid, Uuid)> = dsl::$table
                    .filter(dsl::id.eq(item_id))
                    .filter(dsl::deleted_at.is_null())
                    .select((dsl::board_id, dsl::column_id))
                    .first(conn)
                    .optional()?;
                let (board_id, from_column_id) = placement.ok_or(BoardError::ItemNotFound {
                    entity_type: $entity_type,
                    id: item_id,
                })?;

                let (from_name, to_name) =
                    validate_transition(conn, board_id, from_column_id, to_column_id)?;

                diesel::update(dsl::$table.filter(dsl::id.eq(item_id)))
                    .set((
                        dsl::column_id.eq(to_column_id),
                        dsl::updated_by.eq(actor_id),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)?;

                log_activity(
                    conn,
                    actor_id,
                    ActivityAction::Transition,
                    Some(item_id),
                    $entity_type,
                    format!("column:{from_name}->{to_name}"),
                )?;
                // KAIROS-T-0022: thin event (new column), delivered on
                // commit.
                crate::events::emit_item_event_by_id(
                    conn,
                    crate::events::EventKind::ItemTransitioned,
                    $entity_type,
                    item_id,
                    actor_id,
                )?;
                Ok(())
            })
        }
    };
}

transition_item_fn!(
    /// Move a strategy to another column of its board.
    transition_strategy,
    strategies,
    "strategy"
);
transition_item_fn!(
    /// Move an initiative to another column of its board.
    transition_initiative,
    initiatives,
    "initiative"
);
transition_item_fn!(
    /// Move a task to another column of its board.
    transition_task,
    tasks,
    "task"
);

// ---------------------------------------------------------------------------
// Moving a task to another delivery board (KAIROS-I-0012)
// ---------------------------------------------------------------------------

/// The placement a task has after [`move_task`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMove {
    pub from_board_id: Uuid,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub team_id: Option<Uuid>,
    /// The rename of [`move_task_with`] with `rename`; `None` when the
    /// task kept its code (COLLIERY-T-3101).
    pub rename: Option<crate::code_rename::CodeRename>,
}

/// Move a live task to another live **delivery** board: it lands in the
/// target's entry column, follows the target's team, gets a `board_move`
/// activity row, and an `item_moved` event goes to both boards (one
/// commit). Authorization (`manage_tasks` on BOTH boards) is the caller's,
/// as for every write here.
///
/// The move does not look at the repository of the task, and the task keeps
/// it (COLLIERY-T-0217, COLLIERY-A-0023): the repository is a link, and any
/// task may link to any live repository. Until COLLIERY-T-0217 a task with
/// a repository could move only to the delivery board of the repository's
/// owning team (the KAIROS-T-0104 rule, with an error variant and a 422
/// code of its own, both removed). To go to any other board, the link had
/// to be cleared first.
pub fn move_task(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_board_id: Uuid,
    actor_id: Uuid,
) -> Result<TaskMove, BoardError> {
    move_task_with(conn, task_id, to_board_id, actor_id, false)
}

/// [`move_task`], and with `rename` the task also gets the next code of the
/// target board in the same transaction
/// ([`crate::code_rename::rename_item`], COLLIERY-T-3101): the old code is
/// retired and the references to it change one time. A refused rename
/// ([`BoardError::Rename`]) leaves the task where it was.
pub fn move_task_with(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_board_id: Uuid,
    actor_id: Uuid,
    rename: bool,
) -> Result<TaskMove, BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::boards::dsl as boards_dsl;
        use crate::schema::tasks::dsl;

        let current: Option<(Uuid, Uuid)> = dsl::tasks
            .filter(dsl::id.eq(task_id))
            .filter(dsl::deleted_at.is_null())
            .select((dsl::board_id, dsl::column_id))
            .first(conn)
            .optional()?;
        let (from_board_id, from_column_id) = current.ok_or(BoardError::ItemNotFound {
            entity_type: "task",
            id: task_id,
        })?;
        if from_board_id == to_board_id {
            return Err(BoardError::SameBoard(to_board_id));
        }
        let target: Option<Board> = boards_dsl::boards
            .filter(boards_dsl::id.eq(to_board_id))
            .filter(boards_dsl::deleted_at.is_null())
            .select(Board::as_select())
            .first(conn)
            .optional()?;
        let target = target.ok_or(BoardError::BoardNotFound(to_board_id))?;
        if target.board_level != BoardLevel::Delivery {
            return Err(BoardError::NotDeliveryBoard(to_board_id));
        }
        let to_column_id =
            entry_column(conn, to_board_id)?.ok_or(BoardError::NoEntryColumn(to_board_id))?;

        diesel::update(dsl::tasks.filter(dsl::id.eq(task_id)))
            .set((
                dsl::board_id.eq(to_board_id),
                dsl::column_id.eq(to_column_id),
                dsl::team_id.eq(target.team_id),
                dsl::updated_by.eq(actor_id),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardMove,
            Some(task_id),
            "task",
            format!("board:{from_board_id}->{to_board_id} column:{from_column_id}->{to_column_id}"),
        )?;
        let renamed = if rename {
            let target_slug = target.slug.clone();
            Some(crate::code_rename::rename_item(
                conn,
                ItemType::Task,
                task_id,
                Some(to_board_id),
                actor_id,
                &format!("The task moved to the board {target_slug}."),
            )?)
        } else {
            None
        };
        // The board it LEFT gets the event with the source board so its
        // subscribers refetch; the board it joined gets the placement.
        let short_code: String = dsl::tasks
            .filter(dsl::id.eq(task_id))
            .select(dsl::short_code)
            .first(conn)?;
        crate::events::emit_event(
            conn,
            &crate::events::ThinEvent {
                event: crate::events::EventKind::ItemMoved,
                entity_type: "task".to_string(),
                short_code: short_code.clone(),
                board_id: Some(from_board_id),
                column_id: None,
                actor: actor_id,
            },
        )?;
        crate::events::emit_item_event_by_id(
            conn,
            crate::events::EventKind::ItemMoved,
            "task",
            task_id,
            actor_id,
        )?;
        Ok(TaskMove {
            from_board_id,
            board_id: to_board_id,
            column_id: to_column_id,
            team_id: target.team_id,
            rename: renamed,
        })
    })
}

/// Move an ADR to another column of its board. Hand-written (not the macro)
/// because `adrs.board_id`/`column_id` are nullable — an ADR not placed on a
/// board cannot be transitioned ([`BoardError::ItemNotOnBoard`]).
pub fn transition_adr(
    conn: &mut PgConnection,
    item_id: Uuid,
    to_column_id: Uuid,
    actor_id: Uuid,
) -> Result<(), BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::adrs::dsl;

        let placement: Option<(Option<Uuid>, Option<Uuid>)> = dsl::adrs
            .filter(dsl::id.eq(item_id))
            .filter(dsl::deleted_at.is_null())
            .select((dsl::board_id, dsl::column_id))
            .first(conn)
            .optional()?;
        let (board_id, from_column_id) = match placement.ok_or(BoardError::ItemNotFound {
            entity_type: "adr",
            id: item_id,
        })? {
            (Some(board_id), Some(column_id)) => (board_id, column_id),
            _ => {
                return Err(BoardError::ItemNotOnBoard {
                    entity_type: "adr",
                    id: item_id,
                });
            }
        };

        let (from_name, to_name) =
            validate_transition(conn, board_id, from_column_id, to_column_id)?;

        diesel::update(dsl::adrs.filter(dsl::id.eq(item_id)))
            .set((
                dsl::column_id.eq(to_column_id),
                dsl::updated_by.eq(actor_id),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::Transition,
            Some(item_id),
            "adr",
            format!("column:{from_name}->{to_name}"),
        )?;
        // KAIROS-T-0022: thin event (new column), delivered on commit.
        crate::events::emit_item_event_by_id(
            conn,
            crate::events::EventKind::ItemTransitioned,
            "adr",
            item_id,
            actor_id,
        )?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Column management (KAIROS-A-0002 customization)
// ---------------------------------------------------------------------------

/// Add a column to a board (unique name and position enforced by
/// [`kairos_core::board::check_add_column`]).
pub fn add_column(
    conn: &mut PgConnection,
    board_id: Uuid,
    name: &str,
    position: i32,
    actor_id: Uuid,
) -> Result<BoardColumn, BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_columns;

        let (columns, _) = load_board_rules(conn, board_id)?;
        rules::check_add_column(&columns, name, position)?;

        let created: BoardColumn = diesel::insert_into(board_columns::table)
            .values(NewBoardColumn {
                board_id,
                name: name.to_string(),
                position,
                // Admin-added columns start un-done; the flag is a
                // deliberate admin choice (KAIROS-T-0080).
                is_done: false,
            })
            .returning(BoardColumn::as_returning())
            .get_result(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("column_add:{name}@{position}"),
        )?;
        Ok(created)
    })
}

/// Rename a column (name uniqueness enforced by
/// [`kairos_core::board::check_rename_column`]).
/// Set a column's done flag (KAIROS-T-0080) — an explicit admin choice,
/// logged as board configuration. Setting the current value is a no-op.
pub fn set_column_done(
    conn: &mut PgConnection,
    column_id: Uuid,
    is_done: bool,
    actor_id: Uuid,
) -> Result<BoardColumn, BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_columns;

        let board_id = column_board_id(conn, column_id)?;
        let (columns, _) = load_board_rules(conn, board_id)?;
        let name = column_name(&columns, column_id)?;

        let updated: BoardColumn =
            diesel::update(board_columns::table.filter(board_columns::id.eq(column_id)))
                .set((
                    board_columns::is_done.eq(is_done),
                    board_columns::updated_at.eq(diesel::dsl::now),
                ))
                .returning(BoardColumn::as_returning())
                .get_result(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("column_done:{name}={is_done}"),
        )?;
        Ok(updated)
    })
}

pub fn rename_column(
    conn: &mut PgConnection,
    column_id: Uuid,
    new_name: &str,
    actor_id: Uuid,
) -> Result<BoardColumn, BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_columns;

        let board_id = column_board_id(conn, column_id)?;
        let (columns, _) = load_board_rules(conn, board_id)?;
        rules::check_rename_column(&columns, column_id, new_name)?;
        let old_name = column_name(&columns, column_id)?;

        let updated: BoardColumn =
            diesel::update(board_columns::table.filter(board_columns::id.eq(column_id)))
                .set((
                    board_columns::name.eq(new_name),
                    board_columns::updated_at.eq(diesel::dsl::now),
                ))
                .returning(BoardColumn::as_returning())
                .get_result(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("column_rename:{old_name}->{new_name}"),
        )?;
        Ok(updated)
    })
}

/// Remove a column: a SOFT delete (KAIROS-T-0161), allowed when no LIVE
/// workflow item (strategy, initiative, task, or ADR) occupies it
/// ([`kairos_core::board::check_remove_column`]).
///
/// Archived cards may stay behind, and that is the whole point. They hold
/// a `NOT NULL` FK to this column with no `ON DELETE` clause, so a hard
/// delete was never available while one existed — the column was pinned
/// open forever by work nobody could see any more. Keeping the row instead
/// satisfies the FK and keeps each archived card's answer to "which column
/// was this in when it was put away?" (ADR-20: archived means hidden, not
/// gone).
///
/// Its transition edges survive too — nothing cascades off a soft delete —
/// and [`load_board_rules`] filters them out of the live graph instead.
pub fn remove_column(
    conn: &mut PgConnection,
    column_id: Uuid,
    actor_id: Uuid,
) -> Result<(), BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_columns;

        let board_id = column_board_id(conn, column_id)?;
        let (columns, _) = load_board_rules(conn, board_id)?;
        let item_count = count_items_in_column(conn, column_id)?;
        rules::check_remove_column(&columns, column_id, item_count)?;
        let name = column_name(&columns, column_id)?;

        diesel::update(board_columns::table.filter(board_columns::id.eq(column_id)))
            .set((
                board_columns::deleted_at.eq(diesel::dsl::now),
                board_columns::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("column_remove:{name}"),
        )?;
        Ok(())
    })
}

/// Reorder a board's columns. `new_order` must list every column of the
/// board exactly once ([`kairos_core::board::check_reorder_columns`]);
/// columns get positions `0..n` in the given order.
pub fn reorder_columns(
    conn: &mut PgConnection,
    board_id: Uuid,
    new_order: &[Uuid],
    actor_id: Uuid,
) -> Result<(), BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_columns;

        let (columns, _) = load_board_rules(conn, board_id)?;
        let assignments = rules::check_reorder_columns(&columns, new_order)?;

        // The live-position unique index is not deferrable, so park every
        // column on a distinct negative position first, then assign the
        // final 0..n order. LIVE columns only: a removed column keeps the
        // position it had, and negating it twice would eventually collide
        // it back onto a live one (KAIROS-T-0161).
        diesel::update(
            board_columns::table
                .filter(board_columns::board_id.eq(board_id))
                .filter(board_columns::deleted_at.is_null()),
        )
        .set(board_columns::position.eq(board_columns::position * -1 - 1))
        .execute(conn)?;
        for (column_id, position) in &assignments {
            diesel::update(board_columns::table.filter(board_columns::id.eq(column_id)))
                .set((
                    board_columns::position.eq(position),
                    board_columns::updated_at.eq(diesel::dsl::now),
                ))
                .execute(conn)?;
        }

        let order_names: Vec<String> = new_order
            .iter()
            .map(|&id| column_name(&columns, id))
            .collect::<Result<_, _>>()?;
        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("columns_reorder:{}", order_names.join(",")),
        )?;
        Ok(())
    })
}

/// Add a transition edge to a board
/// ([`kairos_core::board::check_add_transition`]: endpoints on the board,
/// distinct, not duplicate).
pub fn add_transition(
    conn: &mut PgConnection,
    board_id: Uuid,
    from_column_id: Uuid,
    to_column_id: Uuid,
    actor_id: Uuid,
) -> Result<(), BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_transitions;

        let (columns, transitions) = load_board_rules(conn, board_id)?;
        rules::check_add_transition(&columns, &transitions, from_column_id, to_column_id)?;

        diesel::insert_into(board_transitions::table)
            .values(NewBoardTransition {
                board_id,
                from_column_id,
                to_column_id,
            })
            .execute(conn)?;

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!(
                "transition_add:{}->{}",
                column_name(&columns, from_column_id)?,
                column_name(&columns, to_column_id)?
            ),
        )?;
        Ok(())
    })
}

/// Remove a transition edge from a board.
pub fn remove_transition(
    conn: &mut PgConnection,
    board_id: Uuid,
    from_column_id: Uuid,
    to_column_id: Uuid,
    actor_id: Uuid,
) -> Result<(), BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::board_transitions;

        let (columns, _) = load_board_rules(conn, board_id)?;
        let from_name = column_name(&columns, from_column_id)?;
        let to_name = column_name(&columns, to_column_id)?;

        let deleted = diesel::delete(
            board_transitions::table
                .filter(board_transitions::board_id.eq(board_id))
                .filter(board_transitions::from_column_id.eq(from_column_id))
                .filter(board_transitions::to_column_id.eq(to_column_id)),
        )
        .execute(conn)?;
        if deleted == 0 {
            return Err(BoardError::TransitionNotFound {
                board_id,
                from: from_column_id,
                to: to_column_id,
            });
        }

        log_activity(
            conn,
            actor_id,
            ActivityAction::BoardConfig,
            Some(board_id),
            "board",
            format!("transition_remove:{from_name}->{to_name}"),
        )?;
        Ok(())
    })
}

/// Columns of `board_id` with no outbound transitions, in position order —
/// the KAIROS-A-0002 dead-end warning check
/// ([`kairos_core::board::dead_end_columns`]). Genuinely terminal columns
/// (e.g. "Completed") appear here by design; callers decide what to surface.
pub fn dead_end_columns(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Vec<rules::ColumnRef>, BoardError> {
    let (columns, transitions) = load_board_rules(conn, board_id)?;
    Ok(rules::dead_end_columns(&columns, &transitions))
}

/// The `board_id` of a LIVE column, or [`BoardError::ColumnNotFound`] —
/// which is also the answer for a removed one: it is no longer a column of
/// the board, so it cannot be renamed, re-removed or flagged done.
fn column_board_id(conn: &mut PgConnection, column_id: Uuid) -> Result<Uuid, BoardError> {
    use crate::schema::board_columns;
    board_columns::table
        .filter(board_columns::id.eq(column_id))
        .filter(board_columns::deleted_at.is_null())
        .select(board_columns::board_id)
        .first(conn)
        .optional()?
        .ok_or(BoardError::ColumnNotFound(column_id))
}

/// How many LIVE workflow items occupy `column_id`, across every entity
/// table — the count [`remove_column`] refuses on.
///
/// Archived rows are excluded (KAIROS-T-0161). They used to count, on the
/// argument that a soft-deleted row still holds the FK and a hard delete
/// would therefore fail — which was true, and which made one archived card
/// enough to pin a column open permanently. Removal is a soft delete now,
/// so the FK is no longer the obstacle it was, and the question the guard
/// asks is the ADR-20 one that every other guard asks: is there still LIVE
/// work here? (Compare `count_live_board_items` in
/// `kairos-server/api/org/mod.rs`, which asks it of a whole board.)
fn count_items_in_column(conn: &mut PgConnection, column_id: Uuid) -> Result<u64, BoardError> {
    use crate::schema::{adrs, initiatives, strategies, tasks};

    let strategies_count: i64 = strategies::table
        .filter(strategies::column_id.eq(column_id))
        .filter(strategies::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    let initiatives_count: i64 = initiatives::table
        .filter(initiatives::column_id.eq(column_id))
        .filter(initiatives::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    let tasks_count: i64 = tasks::table
        .filter(tasks::column_id.eq(column_id))
        .filter(tasks::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    let adrs_count: i64 = adrs::table
        .filter(adrs::column_id.eq(column_id))
        .filter(adrs::deleted_at.is_null())
        .count()
        .get_result(conn)?;

    Ok((strategies_count + initiatives_count + tasks_count + adrs_count) as u64)
}

/// The board's ENTRY column — lowest position, the one creation defaults
/// to (KAIROS-T-0062) and the one a `file_backlog` request is confined to
/// (KAIROS-T-0105, COLLIERY-T-0218): a caller who does not manage the board
/// can create a task in this column only, with a repository or with none. Defined once so both agree even if positions are not
/// 0-based after a renumbering (KAIROS-T-0112).
pub fn entry_column(conn: &mut PgConnection, board_id: Uuid) -> Result<Option<Uuid>, DieselError> {
    use crate::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::deleted_at.is_null())
        .order(board_columns::position.asc())
        .select(board_columns::id)
        .first(conn)
        .optional()
}
