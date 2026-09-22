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

use crate::models::boards::{Board, BoardColumn, NewBoard, NewBoardColumn, NewBoardTransition};
use crate::models::enums::{ActivityAction, BoardLevel};
use crate::models::graph::NewActivityLogEntry;
use crate::models::public::SystemBoardDefault;

/// Errors from board creation, item transitions, or board configuration.
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    /// No board with this id exists (or it is soft-deleted).
    #[error("board {0} does not exist")]
    BoardNotFound(Uuid),
    /// No column with this id exists.
    #[error("column {0} does not exist")]
    ColumnNotFound(Uuid),
    /// No live item of this type with this id exists.
    #[error("{entity_type} {id} does not exist")]
    ItemNotFound { entity_type: &'static str, id: Uuid },
    /// The item exists but is not placed on a board (ADRs may have NULL
    /// board_id/column_id).
    #[error("{entity_type} {id} is not on a board")]
    ItemNotOnBoard { entity_type: &'static str, id: Uuid },
    /// The transition edge to remove does not exist.
    #[error("transition {from} -> {to} does not exist on board {board_id}")]
    TransitionNotFound {
        board_id: Uuid,
        from: Uuid,
        to: Uuid,
    },
    /// No `system_board_defaults` row is seeded for this level.
    #[error("no system_board_defaults row for level {0}")]
    MissingDefaults(BoardLevel),
    /// The seeded `system_board_defaults` row is malformed.
    #[error("invalid system_board_defaults for level {level}: {source}")]
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
    #[error("database error: {0}")]
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

/// Load a board's columns and transition edges as core rule inputs.
/// Errors with [`BoardError::BoardNotFound`] if the board does not exist or
/// is soft-deleted.
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

    let transitions: Vec<rules::Transition> = board_transitions::table
        .filter(board_transitions::board_id.eq(board_id))
        .select((
            board_transitions::from_column_id,
            board_transitions::to_column_id,
        ))
        .load::<(Uuid, Uuid)>(conn)?
        .into_iter()
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
// Board creation (defaults seeding, KAIROS-A-0002)
// ---------------------------------------------------------------------------

/// Create a board in the current tenant schema, seeding its columns and
/// transitions from the tenant's `public.system_board_defaults` row for
/// `level` — the ONE implementation of default-board seeding: tenant
/// provisioning ([`crate::tenant::provision_tenant`]) calls this for the
/// strategy/initiative/adr boards, and team/delivery-board creation reuses
/// it later with [`BoardLevel::Delivery`] and a `team_id`.
///
/// `actor`: `Some(user)` logs `action='create'` to `activity_log`; `None`
/// is for system provisioning (no user exists yet).
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

pub fn create_board(
    conn: &mut PgConnection,
    level: BoardLevel,
    name: &str,
    slug: &str,
    team_id: Option<Uuid>,
    actor: Option<Uuid>,
) -> Result<Board, BoardError> {
    conn.transaction::<_, BoardError, _>(|conn| {
        use crate::schema::system_board_defaults;
        use crate::schema::{board_columns, board_transitions, boards};

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
            })
            .returning(Board::as_returning())
            .get_result(conn)?;

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
    })
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

/// Remove a column. Only allowed when NO workflow item (strategy,
/// initiative, task, or ADR — soft-deleted rows included, since they still
/// reference the column) occupies it
/// ([`kairos_core::board::check_remove_column`]); its transition edges are
/// removed with it (`ON DELETE CASCADE`).
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

        diesel::delete(board_columns::table.filter(board_columns::id.eq(column_id)))
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

        // UNIQUE (board_id, position) is not deferrable, so park every
        // column on a distinct negative position first, then assign the
        // final 0..n order.
        diesel::update(board_columns::table.filter(board_columns::board_id.eq(board_id)))
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

/// The `board_id` of a column, or [`BoardError::ColumnNotFound`].
fn column_board_id(conn: &mut PgConnection, column_id: Uuid) -> Result<Uuid, BoardError> {
    use crate::schema::board_columns;
    board_columns::table
        .filter(board_columns::id.eq(column_id))
        .select(board_columns::board_id)
        .first(conn)
        .optional()?
        .ok_or(BoardError::ColumnNotFound(column_id))
}

/// How many workflow items reference `column_id` across every entity table.
/// Soft-deleted rows count too: they still hold the FK, so removing the
/// column would fail (and would orphan their placement on restore).
fn count_items_in_column(conn: &mut PgConnection, column_id: Uuid) -> Result<u64, BoardError> {
    use crate::schema::{adrs, initiatives, strategies, tasks};

    let strategies_count: i64 = strategies::table
        .filter(strategies::column_id.eq(column_id))
        .count()
        .get_result(conn)?;
    let initiatives_count: i64 = initiatives::table
        .filter(initiatives::column_id.eq(column_id))
        .count()
        .get_result(conn)?;
    let tasks_count: i64 = tasks::table
        .filter(tasks::column_id.eq(column_id))
        .count()
        .get_result(conn)?;
    let adrs_count: i64 = adrs::table
        .filter(adrs::column_id.eq(column_id))
        .count()
        .get_result(conn)?;

    Ok((strategies_count + initiatives_count + tasks_count + adrs_count) as u64)
}

/// The board's ENTRY column — lowest position, the one creation defaults
/// to (KAIROS-T-0062) and the one `file_backlog` filing is confined to
/// (KAIROS-T-0105). Defined once so both agree even if positions are not
/// 0-based after a renumbering (KAIROS-T-0112).
pub fn entry_column(conn: &mut PgConnection, board_id: Uuid) -> Result<Option<Uuid>, DieselError> {
    use crate::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .order(board_columns::position.asc())
        .select(board_columns::id)
        .first(conn)
        .optional()
}
