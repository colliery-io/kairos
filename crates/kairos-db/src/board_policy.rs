//! The policies of a board: rules that an admin turns on or off per board.
//!
//! Each policy is one boolean column on `boards`. There is no policy table:
//! a column per policy is the smallest store, and the board DTO reads it
//! with the board.
//!
//! # `parent_gate` (KAIROS-T-0641)
//!
//! On a delivery board with the policy on, a task moves into a column with
//! the `claims` flag (the Active column) only when its parent initiative is
//! active. The rule:
//!
//! 1. The trigger is a move into a claims column: a transition, a move to
//!    the board (its entry column), or a create into the column. The flag
//!    decides, not the name of the column.
//! 2. An initiative is active in a column named Active or Monitoring (as
//!    [`crate::my_work`] reads it). Each other column refuses, Completed
//!    too.
//! 3. A task with no parent initiative passes.
//! 4. A bucket initiative (`is_bucket`) is always active.
//! 5. The policy is on by default, for each new board and, by the
//!    migration, for each existing one. Only a delivery board reads it.
//! 6. The refusal is [`BoardError::ParentNotActive`]. Kairos does not move
//!    the initiative.
//! 7. A task in Active under an inactive initiative stays there: the rule
//!    checks moves only.
//!
//! # `owner_gate` (KAIROS-T-0643)
//!
//! On an initiative board with the policy on, an initiative that someone
//! answers for is the only one that runs:
//!
//! 1. The trigger is a move of an initiative into its column named Active.
//!    A move from Active to Monitoring is no trigger: the initiative has an
//!    owner already.
//! 2. Initiatives only. A strategy can get the rule later.
//! 3. A release of the owner of an initiative in Active or Monitoring is
//!    refused ([`release_blocked`]). A hand-off to a member of the board
//!    stays allowed.
//! 4. The refusal is [`BoardError::OwnerRequired`].
//! 5. The policy is on by default. Only an initiative board reads it.
//! 6. An initiative in Active with no owner stays there: the rule checks
//!    moves only. A create of an initiative straight into Active is not a
//!    move, and it is not checked.
//!
//! # `blockers_gate` (KAIROS-T-0565)
//!
//! A task can have open blockers: the sources of its `blocks` edges that
//! are live and open ([`crate::dispatch::open_sql`]). A cancelled blocker
//! is open (KAIROS-A-0038). The rule:
//!
//! 1. The trigger is a move of a task into a column with the `claims` flag
//!    or the `is_done` flag: a transition, a reopen, or a move to the board
//!    (its entry column). A cancel is no trigger: "won't do" needs no
//!    finished blocker.
//! 2. With the policy off, the move goes on. The server adds a warning
//!    that names each open blocker ([`blockers_of_move`]).
//! 3. With the policy on, the move is refused ([`BoardError::Blocked`]).
//!    The refusal names each open blocker.
//! 4. The policy is off by default, for each new board and each existing
//!    one. Only a delivery board reads it. Many teams use `blocks` as a
//!    note of order, and start work before the blocker is done. An open
//!    cancelled blocker also stops a gated task until a person decides.
//! 5. A task in Active or Completed with an open blocker stays there: the
//!    rule checks moves only.
//!
//! Each function operates in the CURRENT `search_path` tenant schema.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::boards::BoardError;
use crate::models::enums::RelationshipType;

/// Is an initiative in this column active? A column named Active or
/// Monitoring (case and spaces aside).
pub fn is_active_initiative_column(name: &str) -> bool {
    let name = name.trim();
    name.eq_ignore_ascii_case("Active") || name.eq_ignore_ascii_case("Monitoring")
}

/// The `parent_gate` policy of the board. A board that does not exist reads
/// as off.
pub fn parent_gate(conn: &mut PgConnection, board_id: Uuid) -> Result<bool, DieselError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::id.eq(board_id))
        .select(boards::parent_gate)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false))
}

/// Set the `parent_gate` policy of the board. The caller checked
/// `configure_boards`. Returns true when the value changed.
pub fn set_parent_gate(
    conn: &mut PgConnection,
    board_id: Uuid,
    on: bool,
) -> Result<bool, DieselError> {
    use crate::schema::boards;
    let changed = diesel::update(
        boards::table
            .filter(boards::id.eq(board_id))
            .filter(boards::parent_gate.ne(on)),
    )
    .set((
        boards::parent_gate.eq(on),
        boards::updated_at.eq(diesel::dsl::now),
    ))
    .execute(conn)?;
    Ok(changed > 0)
}

/// The `owner_gate` policy of the board. A board that does not exist reads
/// as off.
pub fn owner_gate(conn: &mut PgConnection, board_id: Uuid) -> Result<bool, DieselError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::id.eq(board_id))
        .select(boards::owner_gate)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false))
}

/// Set the `owner_gate` policy of the board. The caller checked
/// `configure_boards`. Returns true when the value changed.
pub fn set_owner_gate(
    conn: &mut PgConnection,
    board_id: Uuid,
    on: bool,
) -> Result<bool, DieselError> {
    use crate::schema::boards;
    let changed = diesel::update(
        boards::table
            .filter(boards::id.eq(board_id))
            .filter(boards::owner_gate.ne(on)),
    )
    .set((
        boards::owner_gate.eq(on),
        boards::updated_at.eq(diesel::dsl::now),
    ))
    .execute(conn)?;
    Ok(changed > 0)
}

/// The `blockers_gate` policy of the board. A board that does not exist
/// reads as off.
pub fn blockers_gate(conn: &mut PgConnection, board_id: Uuid) -> Result<bool, DieselError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::id.eq(board_id))
        .select(boards::blockers_gate)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false))
}

/// Set the `blockers_gate` policy of the board. The caller checked
/// `configure_boards`. Returns true when the value changed.
pub fn set_blockers_gate(
    conn: &mut PgConnection,
    board_id: Uuid,
    on: bool,
) -> Result<bool, DieselError> {
    use crate::schema::boards;
    let changed = diesel::update(
        boards::table
            .filter(boards::id.eq(board_id))
            .filter(boards::blockers_gate.ne(on)),
    )
    .set((
        boards::blockers_gate.eq(on),
        boards::updated_at.eq(diesel::dsl::now),
    ))
    .execute(conn)?;
    Ok(changed > 0)
}

/// One open blocker of a task (module docs, `blockers_gate`).
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct OpenBlocker {
    /// The short code of the blocker.
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub short_code: String,
    /// The blocker is a cancelled task (KAIROS-A-0038).
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub cancelled: bool,
}

impl std::fmt::Display for OpenBlocker {
    /// `KAIROS-T-0012 (open)`, or `KAIROS-T-0012 (cancelled)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = if self.cancelled { "cancelled" } else { "open" };
        write!(f, "{} ({state})", self.short_code)
    }
}

/// The blockers as one text: `KAIROS-T-0012 (open), KAIROS-T-0013
/// (cancelled)`.
pub fn blockers_text(blockers: &[OpenBlocker]) -> String {
    blockers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The open blockers of the item `item_id`: the sources of its `blocks`
/// edges that are live and open ([`crate::dispatch::blocks_now_sql`]), by
/// short code.
pub fn open_blockers(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<OpenBlocker>, DieselError> {
    diesel::sql_query(format!(
        "SELECT p.short_code, \
                EXISTS (SELECT 1 FROM task_cancellations k WHERE k.task_id = p.id) AS cancelled \
           FROM item_relationships r \
           JOIN {items} p ON p.id = r.source_id \
          WHERE r.relationship = 'blocks' AND r.target_id = $1 AND {blocks} \
          ORDER BY p.short_code",
        items = crate::dispatch::WORKFLOW_ITEMS,
        blocks = crate::dispatch::blocks_now_sql("p"),
    ))
    .bind::<diesel::sql_types::Uuid, _>(item_id)
    .load(conn)
}

/// The open blockers of a move of the task `task_id` into the column
/// `to_column_id` (module docs, `blockers_gate`, decision 1). Empty when
/// the column has neither the `claims` flag nor the `is_done` flag. The
/// server reads it after the move for the warning.
pub fn blockers_of_move(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
) -> Result<Vec<OpenBlocker>, DieselError> {
    use crate::schema::board_columns;
    let flags: Option<(bool, bool)> = board_columns::table
        .filter(board_columns::id.eq(to_column_id))
        .select((board_columns::claims, board_columns::is_done))
        .first(conn)
        .optional()?;
    match flags {
        Some((claims, done)) if claims || done => open_blockers(conn, task_id),
        _ => Ok(Vec::new()),
    }
}

/// Check that the task `task_id` may go into the column `to_column_id`
/// (module docs, `blockers_gate`). Call it in the transaction of the move,
/// before the write. A board with the policy off passes.
pub fn check_blockers(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
) -> Result<(), BoardError> {
    use crate::schema::{board_columns, boards, tasks};

    let target: Option<(Uuid, String, bool, crate::models::enums::BoardLevel)> =
        board_columns::table
            .inner_join(boards::table.on(boards::id.eq(board_columns::board_id)))
            .filter(board_columns::id.eq(to_column_id))
            .select((
                boards::id,
                board_columns::name,
                boards::blockers_gate,
                boards::board_level,
            ))
            .first(conn)
            .optional()?;
    let Some((board_id, to_column, true, crate::models::enums::BoardLevel::Delivery)) = target
    else {
        return Ok(());
    };
    let blockers = blockers_of_move(conn, task_id, to_column_id)?;
    if blockers.is_empty() {
        return Ok(());
    }
    let task: String = tasks::table
        .filter(tasks::id.eq(task_id))
        .select(tasks::short_code)
        .first(conn)?;
    Err(BoardError::Blocked {
        task,
        to_column,
        blockers,
        board_id,
    })
}

/// The board policies of a move of the task `task_id` into the column
/// `to_column_id`: `parent_gate`, then `blockers_gate`. A transition, a
/// reopen and a board move call this one function, in the transaction of
/// the move, before the write.
pub fn check_task_move(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
) -> Result<(), BoardError> {
    check_active_parent(conn, task_id, to_column_id)?;
    check_blockers(conn, task_id, to_column_id)
}

/// Check that the initiative `initiative_id` may go into the column
/// `to_column_id` (module docs, `owner_gate`). Call it in the transaction
/// of the move, before the write. A column that is not named Active
/// passes.
pub fn check_owner(
    conn: &mut PgConnection,
    initiative_id: Uuid,
    to_column_id: Uuid,
) -> Result<(), BoardError> {
    use crate::schema::{board_columns, boards, initiatives, item_owners};

    let target: Option<(Uuid, String, bool, crate::models::enums::BoardLevel)> =
        board_columns::table
            .inner_join(boards::table.on(boards::id.eq(board_columns::board_id)))
            .filter(board_columns::id.eq(to_column_id))
            .select((
                boards::id,
                board_columns::name,
                boards::owner_gate,
                boards::board_level,
            ))
            .first(conn)
            .optional()?;
    let Some((board_id, to_column, gate, level)) = target else {
        return Ok(());
    };
    if !gate
        || level != crate::models::enums::BoardLevel::Initiative
        || !to_column.trim().eq_ignore_ascii_case("Active")
    {
        return Ok(());
    }
    let owned: Option<Uuid> = item_owners::table
        .filter(item_owners::item_id.eq(initiative_id))
        .select(item_owners::item_id)
        .first(conn)
        .optional()?;
    if owned.is_some() {
        return Ok(());
    }
    let initiative: String = initiatives::table
        .filter(initiatives::id.eq(initiative_id))
        .select(initiatives::short_code)
        .first(conn)?;
    Err(BoardError::OwnerRequired {
        initiative,
        to_column,
        board_id,
    })
}

/// The column of the initiative when a release of its owner is refused
/// (module docs, `owner_gate`, decision 3): the initiative is live, in a
/// column named Active or Monitoring, on an initiative board with the
/// policy on. `None`: the release may go on. Any other item is `None`.
pub fn release_blocked(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<String>, DieselError> {
    use crate::schema::{board_columns, boards, initiatives};

    let place: Option<(String, bool, crate::models::enums::BoardLevel)> = initiatives::table
        .inner_join(board_columns::table.on(board_columns::id.eq(initiatives::column_id)))
        .inner_join(boards::table.on(boards::id.eq(initiatives::board_id)))
        .filter(initiatives::id.eq(item_id))
        .filter(initiatives::deleted_at.is_null())
        .select((board_columns::name, boards::owner_gate, boards::board_level))
        .first(conn)
        .optional()?;
    Ok(place.and_then(|(column, gate, level)| {
        (gate
            && level == crate::models::enums::BoardLevel::Initiative
            && is_active_initiative_column(&column))
        .then_some(column)
    }))
}

/// Check that the task `task_id` may go into the column `to_column_id`
/// (module docs, `parent_gate`). Call it in the transaction of the move,
/// before the write, with the column the task goes to. A column that is
/// not a claims column passes.
pub fn check_active_parent(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
) -> Result<(), BoardError> {
    use crate::schema::{
        board_columns, boards, initiatives, item_owners, item_relationships, tasks,
    };

    let column: Option<(Uuid, bool, String)> = board_columns::table
        .filter(board_columns::id.eq(to_column_id))
        .select((
            board_columns::board_id,
            board_columns::claims,
            board_columns::name,
        ))
        .first(conn)
        .optional()?;
    let Some((board_id, claims, to_name)) = column else {
        return Ok(());
    };
    if !claims {
        return Ok(());
    }
    let gate: Option<(bool, crate::models::enums::BoardLevel)> = boards::table
        .filter(boards::id.eq(board_id))
        .select((boards::parent_gate, boards::board_level))
        .first(conn)
        .optional()?;
    if !matches!(
        gate,
        Some((true, crate::models::enums::BoardLevel::Delivery))
    ) {
        return Ok(());
    }
    // The parent initiatives of the task: live ones only. A task has one
    // parent; old data can have more, and each must be active.
    let parents: Vec<(Uuid, String, bool, String, bool)> = item_relationships::table
        .inner_join(initiatives::table.on(initiatives::id.eq(item_relationships::source_id)))
        .inner_join(board_columns::table.on(board_columns::id.eq(initiatives::column_id)))
        .inner_join(boards::table.on(boards::id.eq(initiatives::board_id)))
        .filter(item_relationships::target_id.eq(task_id))
        .filter(item_relationships::relationship.eq(RelationshipType::Parent))
        .filter(initiatives::deleted_at.is_null())
        .select((
            initiatives::id,
            initiatives::short_code,
            initiatives::is_bucket,
            board_columns::name,
            boards::owner_gate,
        ))
        .order(initiatives::short_code.asc())
        .load(conn)?;
    let Some((initiative_id, initiative, _, column, owner_gate)) = parents
        .into_iter()
        .find(|(_, _, bucket, column, _)| !bucket && !is_active_initiative_column(column))
    else {
        return Ok(());
    };
    // KAIROS-T-0676: the initiative also needs an owner to go to Active.
    // The refusal then names both steps.
    let owner_needed = owner_gate
        && item_owners::table
            .filter(item_owners::item_id.eq(initiative_id))
            .select(item_owners::item_id)
            .first::<Uuid>(conn)
            .optional()?
            .is_none();
    let task: String = tasks::table
        .filter(tasks::id.eq(task_id))
        .select(tasks::short_code)
        .first(conn)?;
    Err(BoardError::ParentNotActive {
        task,
        to_column: to_name,
        initiative,
        initiative_column: column,
        owner_needed,
        board_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_blocker_names_its_state() {
        let blockers = [
            OpenBlocker {
                short_code: "ACME-T-0012".into(),
                cancelled: false,
            },
            OpenBlocker {
                short_code: "ACME-T-0013".into(),
                cancelled: true,
            },
        ];
        assert_eq!(
            blockers_text(&blockers),
            "ACME-T-0012 (open), ACME-T-0013 (cancelled)"
        );
    }

    #[test]
    fn active_and_monitoring_are_active() {
        assert!(is_active_initiative_column("Active"));
        assert!(is_active_initiative_column(" monitoring "));
        assert!(!is_active_initiative_column("Discovery"));
        assert!(!is_active_initiative_column("Completed"));
    }
}
