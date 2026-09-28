//! Pure board rules (KAIROS-A-0002, KAIROS-T-0010): transition validation,
//! column-management rules, dead-end detection, and default-board-config
//! parsing.
//!
//! Per KAIROS-A-0009 everything here is a function over already-loaded data
//! — no diesel, no async, no I/O. `kairos-db::boards` loads the rows, calls
//! these rules, and persists the outcome.

use uuid::Uuid;

/// A board column as loaded from `board_columns` (the rule-relevant subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub id: Uuid,
    pub name: String,
    /// Display ordering, 0-indexed. Ordering does NOT determine valid
    /// transitions — only [`Transition`] rows do (KAIROS-A-0002).
    pub position: i32,
}

/// An allowed transition edge as loaded from `board_transitions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub from_column_id: Uuid,
    pub to_column_id: Uuid,
}

/// A column reference carried inside errors and warnings (id + name is what
/// the S-0006 error envelope needs to render "allowed targets").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    pub id: Uuid,
    pub name: String,
}

impl ColumnRef {
    fn of(column: &Column) -> Self {
        Self {
            id: column.id,
            name: column.name.clone(),
        }
    }
}

fn find_column(columns: &[Column], id: Uuid) -> Option<&Column> {
    columns.iter().find(|c| c.id == id)
}

/// The allowed target columns out of `from`, ordered by column position.
fn allowed_targets(columns: &[Column], transitions: &[Transition], from: Uuid) -> Vec<ColumnRef> {
    let mut targets: Vec<&Column> = transitions
        .iter()
        .filter(|t| t.from_column_id == from)
        .filter_map(|t| find_column(columns, t.to_column_id))
        .collect();
    targets.sort_by_key(|c| c.position);
    targets.dedup_by_key(|c| c.id);
    targets.into_iter().map(ColumnRef::of).collect()
}

// ---------------------------------------------------------------------------
// Board slug
// ---------------------------------------------------------------------------

/// The form of a board slug, as the refusal gives it (COLLIERY-T-0258).
pub const BOARD_SLUG_RULE: &str = "^[a-z][a-z0-9_-]{1,62}$";

/// Whether `slug` has the form of a board slug (COLLIERY-T-0258):
/// [`BOARD_SLUG_RULE`], the rule of a team slug and of an organization
/// slug, so that `<team-slug>-delivery` has only characters that the rule
/// permits. A slug with the form of a UUID does not pass: a reference to a
/// board is read as an id first (KAIROS-T-0116), so no request could
/// reach that board by its slug.
///
/// The rule is for a slug that a caller SENDS. The server does not apply
/// it to the slug that it makes for the delivery board of a team, and a
/// board that has a different slug from before the rule stays as it is.
pub fn is_valid_board_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
        && !crate::repositories::looks_like_uuid(slug)
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Why a requested item transition is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    /// The item's current column is not on the board (data drift).
    #[error("The source column {0} is not a column of this board.")]
    UnknownFromColumn(Uuid),
    /// The requested target column is not on the board.
    #[error("The target column {0} is not a column of this board.")]
    UnknownToColumn(Uuid),
    /// No `board_transitions` edge allows this move. Carries the allowed
    /// target columns for the item's current column so the API layer can
    /// return them in the error envelope (S-0006 REQ-1.4).
    #[error(
        "This board does not permit the transition from {from} to {to}. {targets}",
        from = .from.name,
        to = .to.name,
        targets = targets_sentence(.from, .allowed_targets),
    )]
    NotAllowed {
        from: ColumnRef,
        to: ColumnRef,
        allowed_targets: Vec<ColumnRef>,
    },
}

/// The sentence of [`TransitionError::NotAllowed`] that gives the columns
/// an item can move to (COLLIERY-T-0258). A column with no transition out of
/// it gets a sentence of its own, and not an empty list.
fn targets_sentence(from: &ColumnRef, allowed_targets: &[ColumnRef]) -> String {
    if allowed_targets.is_empty() {
        return format!("No transition starts at {}.", from.name);
    }
    let names: Vec<&str> = allowed_targets.iter().map(|c| c.name.as_str()).collect();
    format!(
        "From {}, an item can move to: {}.",
        from.name,
        names.join(", ")
    )
}

/// A move from `from` to `to` is allowed iff a matching transition edge
/// exists (KAIROS-A-0002: existence check in `board_transitions`).
///
/// `columns` and `transitions` must all belong to the same board — callers
/// load them by `board_id`.
pub fn can_transition(
    columns: &[Column],
    transitions: &[Transition],
    from: Uuid,
    to: Uuid,
) -> Result<(), TransitionError> {
    let from_column = find_column(columns, from).ok_or(TransitionError::UnknownFromColumn(from))?;
    let to_column = find_column(columns, to).ok_or(TransitionError::UnknownToColumn(to))?;
    if transitions
        .iter()
        .any(|t| t.from_column_id == from && t.to_column_id == to)
    {
        Ok(())
    } else {
        Err(TransitionError::NotAllowed {
            from: ColumnRef::of(from_column),
            to: ColumnRef::of(to_column),
            allowed_targets: allowed_targets(columns, transitions, from),
        })
    }
}

// ---------------------------------------------------------------------------
// Column-management rules
// ---------------------------------------------------------------------------

/// Why a board configuration change is invalid (KAIROS-A-0002
/// customization rules; name/position uniqueness mirrors the S-0004
/// `UNIQUE (board_id, name)` / `UNIQUE (board_id, position)` constraints,
/// enforced here so violations are typed errors, not constraint failures).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColumnRuleError {
    /// The referenced column is not on the board.
    #[error("The column {0} is not a column of this board.")]
    UnknownColumn(Uuid),
    /// Column names must be non-empty.
    #[error("The name of the column is empty. Send a name.")]
    EmptyName,
    /// Another column on the board already has this name.
    #[error("The board has a column with the name {0:?} already.")]
    DuplicateName(String),
    /// Another column on the board already occupies this position.
    #[error("The board has a column at the position {0} already.")]
    DuplicatePosition(i32),
    /// Positions are 0-indexed display order; negatives are reserved.
    #[error("The position {0} is not correct. A position is 0 or more.")]
    NegativePosition(i32),
    /// Removal is only allowed when no items occupy the column
    /// (KAIROS-A-0002: "items in a removed column must be moved first").
    #[error(
        "The column {name:?} has items. The number of items is {item_count}. Move each item to a different column. Then remove the column.",
        name = .column.name,
    )]
    ColumnNotEmpty { column: ColumnRef, item_count: u64 },
    /// A reorder must mention every column of the board exactly once.
    #[error(
        "The board has {expected} columns, and the list has {actual}. Send each column of the board one time."
    )]
    ReorderLengthMismatch { expected: usize, actual: usize },
    /// A reorder listed the same column twice.
    #[error(
        "The list has the column {0} more than one time. Send each column of the board one time."
    )]
    ReorderDuplicateColumn(Uuid),
    /// A transition must connect two distinct columns (mirrors the S-0004
    /// `CHECK (from_column_id != to_column_id)`).
    #[error("A transition goes between two different columns. Send two different columns.")]
    SelfTransition,
    /// The transition edge already exists (mirrors the S-0004
    /// `UNIQUE (board_id, from_column_id, to_column_id)`).
    #[error("The board has this transition already.")]
    DuplicateTransition,
}

/// Rule check for adding a column: non-empty name, unique name, unique
/// non-negative position.
pub fn check_add_column(
    columns: &[Column],
    name: &str,
    position: i32,
) -> Result<(), ColumnRuleError> {
    if name.trim().is_empty() {
        return Err(ColumnRuleError::EmptyName);
    }
    if position < 0 {
        return Err(ColumnRuleError::NegativePosition(position));
    }
    if columns.iter().any(|c| c.name == name) {
        return Err(ColumnRuleError::DuplicateName(name.to_string()));
    }
    if columns.iter().any(|c| c.position == position) {
        return Err(ColumnRuleError::DuplicatePosition(position));
    }
    Ok(())
}

/// Rule check for renaming a column: column exists, new name non-empty and
/// unique among the OTHER columns (renaming to its own name is a no-op).
pub fn check_rename_column(
    columns: &[Column],
    column_id: Uuid,
    new_name: &str,
) -> Result<(), ColumnRuleError> {
    find_column(columns, column_id).ok_or(ColumnRuleError::UnknownColumn(column_id))?;
    if new_name.trim().is_empty() {
        return Err(ColumnRuleError::EmptyName);
    }
    if columns
        .iter()
        .any(|c| c.id != column_id && c.name == new_name)
    {
        return Err(ColumnRuleError::DuplicateName(new_name.to_string()));
    }
    Ok(())
}

/// Rule check for removing a column: column exists and no items occupy it.
/// `item_count` is the number of workflow items (across every entity table)
/// whose `column_id` references the column — the caller counts, the rule
/// decides.
pub fn check_remove_column(
    columns: &[Column],
    column_id: Uuid,
    item_count: u64,
) -> Result<(), ColumnRuleError> {
    let column =
        find_column(columns, column_id).ok_or(ColumnRuleError::UnknownColumn(column_id))?;
    if item_count > 0 {
        return Err(ColumnRuleError::ColumnNotEmpty {
            column: ColumnRef::of(column),
            item_count,
        });
    }
    Ok(())
}

/// Rule check for reordering: `new_order` must list every column of the
/// board exactly once. Returns the `(column_id, new_position)` assignments
/// (positions `0..n` in the given order) for the caller to persist.
pub fn check_reorder_columns(
    columns: &[Column],
    new_order: &[Uuid],
) -> Result<Vec<(Uuid, i32)>, ColumnRuleError> {
    if new_order.len() != columns.len() {
        return Err(ColumnRuleError::ReorderLengthMismatch {
            expected: columns.len(),
            actual: new_order.len(),
        });
    }
    let mut seen = Vec::with_capacity(new_order.len());
    for &id in new_order {
        if find_column(columns, id).is_none() {
            return Err(ColumnRuleError::UnknownColumn(id));
        }
        if seen.contains(&id) {
            return Err(ColumnRuleError::ReorderDuplicateColumn(id));
        }
        seen.push(id);
    }
    Ok(new_order
        .iter()
        .enumerate()
        .map(|(position, &id)| (id, position as i32))
        .collect())
}

/// Rule check for adding a transition edge: both endpoints are columns of
/// the board, the edge connects distinct columns, and it does not already
/// exist.
pub fn check_add_transition(
    columns: &[Column],
    transitions: &[Transition],
    from: Uuid,
    to: Uuid,
) -> Result<(), ColumnRuleError> {
    find_column(columns, from).ok_or(ColumnRuleError::UnknownColumn(from))?;
    find_column(columns, to).ok_or(ColumnRuleError::UnknownColumn(to))?;
    if from == to {
        return Err(ColumnRuleError::SelfTransition);
    }
    if transitions
        .iter()
        .any(|t| t.from_column_id == from && t.to_column_id == to)
    {
        return Err(ColumnRuleError::DuplicateTransition);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dead-end detection
// ---------------------------------------------------------------------------

/// Columns with no outbound transitions, ordered by position — a
/// warning-producing check, not an error (KAIROS-A-0002 flags dead ends as
/// a possible invalid configuration, but genuinely terminal columns like
/// "Completed" legitimately have no outbound edges; callers decide what to
/// surface).
pub fn dead_end_columns(columns: &[Column], transitions: &[Transition]) -> Vec<ColumnRef> {
    let mut dead_ends: Vec<&Column> = columns
        .iter()
        .filter(|c| !transitions.iter().any(|t| t.from_column_id == c.id))
        .collect();
    dead_ends.sort_by_key(|c| c.position);
    dead_ends.into_iter().map(ColumnRef::of).collect()
}

// ---------------------------------------------------------------------------
// Default board configuration parsing (system_board_defaults)
// ---------------------------------------------------------------------------

/// A parsed, validated `public.system_board_defaults` configuration:
/// ordered column names plus `(from, to)` transition pairs by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultBoardConfig {
    /// Column names in position order (0-indexed).
    pub columns: Vec<String>,
    /// Transitions as `(from_name, to_name)` pairs, each referencing an
    /// entry in `columns`.
    pub transitions: Vec<(String, String)>,
}

/// Why a `system_board_defaults` row is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DefaultConfigError {
    /// The `columns` text contained no column names.
    #[error("The default board configuration has no columns. Add one column or more.")]
    NoColumns,
    /// A column line was empty.
    #[error(
        "The default board configuration has a column with an empty name. Give each column a name."
    )]
    EmptyColumnName,
    /// The same column name appears twice.
    #[error("The default board configuration has the column {0:?} more than one time.")]
    DuplicateColumn(String),
    /// A transition line is not of the form `"From -> To"`.
    #[error("The default transition line {0:?} is not correct. Write the line as \"From -> To\".")]
    MalformedTransition(String),
    /// A transition references a column name not in `columns`.
    #[error(
        "The default transition {line:?} has the column {column:?}. The default board configuration has no column with that name."
    )]
    UnknownTransitionColumn { line: String, column: String },
    /// A transition connects a column to itself.
    #[error("The default transition {0:?} goes from a column to the same column.")]
    SelfTransition(String),
    /// The same transition appears twice.
    #[error("The default board configuration has the transition {0:?} more than one time.")]
    DuplicateTransition(String),
}

/// Parse one `"From -> To"` line from `system_board_defaults.transitions`.
fn parse_transition_line(line: &str) -> Option<(&str, &str)> {
    let (from, to) = line.split_once("->")?;
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((from, to))
}

/// Parse and validate a `system_board_defaults` row's `columns`
/// (newline-separated names, position order) and `transitions`
/// (newline-separated `"From -> To"` lines). Blank lines are ignored.
pub fn parse_default_config(
    columns_text: &str,
    transitions_text: &str,
) -> Result<DefaultBoardConfig, DefaultConfigError> {
    let mut columns: Vec<String> = Vec::new();
    for line in columns_text.lines() {
        let name = line.trim();
        if name.is_empty() {
            // A blank trailing line is tolerable; an empty name amid content
            // is not distinguishable from it, so skip blanks entirely and
            // catch the pathological all-blank case below.
            continue;
        }
        if columns.iter().any(|c| c == name) {
            return Err(DefaultConfigError::DuplicateColumn(name.to_string()));
        }
        columns.push(name.to_string());
    }
    if columns.is_empty() {
        return Err(DefaultConfigError::NoColumns);
    }

    let mut transitions: Vec<(String, String)> = Vec::new();
    for line in transitions_text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        let (from, to) = parse_transition_line(line)
            .ok_or_else(|| DefaultConfigError::MalformedTransition(line.to_string()))?;
        for column in [from, to] {
            if !columns.iter().any(|c| c == column) {
                return Err(DefaultConfigError::UnknownTransitionColumn {
                    line: line.to_string(),
                    column: column.to_string(),
                });
            }
        }
        if from == to {
            return Err(DefaultConfigError::SelfTransition(line.to_string()));
        }
        if transitions.iter().any(|(f, t)| f == from && t == to) {
            return Err(DefaultConfigError::DuplicateTransition(line.to_string()));
        }
        transitions.push((from.to_string(), to.to_string()));
    }

    Ok(DefaultBoardConfig {
        columns,
        transitions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COLLIERY-T-0258: the form of a board slug.
    #[test]
    fn board_slug_form() {
        for slug in ["roadmap", "platform-delivery", "road_map", "ab", "a1"] {
            assert!(is_valid_board_slug(slug), "{slug:?} is a board slug");
        }
        assert!(is_valid_board_slug(&"a".repeat(63)));
        for slug in [
            "", "a", "Roadmap", "road map", "9lives", "-roadmap", "_roadmap", "road.map",
            "road/map", "roadmäp",
        ] {
            assert!(!is_valid_board_slug(slug), "{slug:?} is not a board slug");
        }
        assert!(!is_valid_board_slug(&"a".repeat(64)));
        // A reference to a board is read as an id first.
        assert!(!is_valid_board_slug("abcdef12-0000-7000-8000-000000000003"));
        assert!(is_valid_board_slug("abcdef12-0000-7000-8000-00000000000x"));
    }

    /// COLLIERY-T-0258: the delivery board of a team gets a slug that has
    /// the form, for a team slug that leaves room for `-delivery`.
    #[test]
    fn the_slug_of_a_delivery_board_has_the_form() {
        for team in ["platform", "web", "data_science", "t2"] {
            assert!(is_valid_board_slug(&format!("{team}-delivery")));
        }
    }

    /// A little board: A(0) -> B(1) -> C(2), plus B <-> D(3) (bidirectional).
    fn fixture() -> (Vec<Column>, Vec<Transition>, [Uuid; 4]) {
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let columns = vec![
            Column {
                id: ids[0],
                name: "A".into(),
                position: 0,
            },
            Column {
                id: ids[1],
                name: "B".into(),
                position: 1,
            },
            Column {
                id: ids[2],
                name: "C".into(),
                position: 2,
            },
            Column {
                id: ids[3],
                name: "D".into(),
                position: 3,
            },
        ];
        let transitions = vec![
            Transition {
                from_column_id: ids[0],
                to_column_id: ids[1],
            },
            Transition {
                from_column_id: ids[1],
                to_column_id: ids[2],
            },
            Transition {
                from_column_id: ids[1],
                to_column_id: ids[3],
            },
            Transition {
                from_column_id: ids[3],
                to_column_id: ids[1],
            },
        ];
        (columns, transitions, ids)
    }

    // -- can_transition ------------------------------------------------------

    #[test]
    fn transition_allowed_iff_edge_exists() {
        let (columns, transitions, ids) = fixture();
        assert_eq!(
            can_transition(&columns, &transitions, ids[0], ids[1]),
            Ok(())
        );
        assert_eq!(
            can_transition(&columns, &transitions, ids[1], ids[2]),
            Ok(())
        );
        // Bidirectional pair: both directions allowed.
        assert_eq!(
            can_transition(&columns, &transitions, ids[1], ids[3]),
            Ok(())
        );
        assert_eq!(
            can_transition(&columns, &transitions, ids[3], ids[1]),
            Ok(())
        );
    }

    #[test]
    fn invalid_transition_carries_allowed_targets_in_position_order() {
        let (columns, transitions, ids) = fixture();
        // B -> A has no edge; from B the allowed targets are C (pos 2) then D (pos 3).
        let err = can_transition(&columns, &transitions, ids[1], ids[0]).unwrap_err();
        match err {
            TransitionError::NotAllowed {
                from,
                to,
                allowed_targets,
            } => {
                assert_eq!(from.id, ids[1]);
                assert_eq!(from.name, "B");
                assert_eq!(to.id, ids[0]);
                assert_eq!(to.name, "A");
                assert_eq!(
                    allowed_targets
                        .iter()
                        .map(|c| (c.id, c.name.as_str()))
                        .collect::<Vec<_>>(),
                    [(ids[2], "C"), (ids[3], "D")]
                );
            }
            other => panic!("expected NotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn transition_from_dead_end_reports_empty_allowed_targets() {
        let (columns, transitions, ids) = fixture();
        // C has no outbound edges.
        let err = can_transition(&columns, &transitions, ids[2], ids[0]).unwrap_err();
        match err {
            TransitionError::NotAllowed {
                allowed_targets, ..
            } => assert!(allowed_targets.is_empty()),
            other => panic!("expected NotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn transition_with_unknown_columns_is_typed() {
        let (columns, transitions, ids) = fixture();
        let stranger = Uuid::new_v4();
        assert_eq!(
            can_transition(&columns, &transitions, stranger, ids[0]),
            Err(TransitionError::UnknownFromColumn(stranger))
        );
        assert_eq!(
            can_transition(&columns, &transitions, ids[0], stranger),
            Err(TransitionError::UnknownToColumn(stranger))
        );
    }

    // -- column rules ---------------------------------------------------------

    #[test]
    fn add_column_requires_unique_name_and_position() {
        let (columns, _, _) = fixture();
        assert_eq!(check_add_column(&columns, "E", 4), Ok(()));
        assert_eq!(
            check_add_column(&columns, "B", 4),
            Err(ColumnRuleError::DuplicateName("B".into()))
        );
        assert_eq!(
            check_add_column(&columns, "E", 1),
            Err(ColumnRuleError::DuplicatePosition(1))
        );
        assert_eq!(
            check_add_column(&columns, "  ", 4),
            Err(ColumnRuleError::EmptyName)
        );
        assert_eq!(
            check_add_column(&columns, "E", -1),
            Err(ColumnRuleError::NegativePosition(-1))
        );
    }

    #[test]
    fn rename_column_enforces_existence_and_uniqueness() {
        let (columns, _, ids) = fixture();
        assert_eq!(check_rename_column(&columns, ids[0], "Alpha"), Ok(()));
        // Renaming to its own current name is a no-op, allowed.
        assert_eq!(check_rename_column(&columns, ids[0], "A"), Ok(()));
        assert_eq!(
            check_rename_column(&columns, ids[0], "B"),
            Err(ColumnRuleError::DuplicateName("B".into()))
        );
        assert_eq!(
            check_rename_column(&columns, ids[0], ""),
            Err(ColumnRuleError::EmptyName)
        );
        let stranger = Uuid::new_v4();
        assert_eq!(
            check_rename_column(&columns, stranger, "X"),
            Err(ColumnRuleError::UnknownColumn(stranger))
        );
    }

    #[test]
    fn remove_column_only_when_empty() {
        let (columns, _, ids) = fixture();
        assert_eq!(check_remove_column(&columns, ids[2], 0), Ok(()));
        let err = check_remove_column(&columns, ids[2], 3).unwrap_err();
        match err {
            ColumnRuleError::ColumnNotEmpty { column, item_count } => {
                assert_eq!(column.id, ids[2]);
                assert_eq!(column.name, "C");
                assert_eq!(item_count, 3);
            }
            other => panic!("expected ColumnNotEmpty, got {other:?}"),
        }
        let stranger = Uuid::new_v4();
        assert_eq!(
            check_remove_column(&columns, stranger, 0),
            Err(ColumnRuleError::UnknownColumn(stranger))
        );
    }

    #[test]
    fn reorder_must_cover_every_column_exactly_once() {
        let (columns, _, ids) = fixture();
        // Reverse order: D C B A -> positions 0..3.
        let assignments =
            check_reorder_columns(&columns, &[ids[3], ids[2], ids[1], ids[0]]).unwrap();
        assert_eq!(
            assignments,
            [(ids[3], 0), (ids[2], 1), (ids[1], 2), (ids[0], 3)]
        );

        assert_eq!(
            check_reorder_columns(&columns, &[ids[0], ids[1]]),
            Err(ColumnRuleError::ReorderLengthMismatch {
                expected: 4,
                actual: 2
            })
        );
        assert_eq!(
            check_reorder_columns(&columns, &[ids[0], ids[1], ids[2], ids[2]]),
            Err(ColumnRuleError::ReorderDuplicateColumn(ids[2]))
        );
        let stranger = Uuid::new_v4();
        assert_eq!(
            check_reorder_columns(&columns, &[ids[0], ids[1], ids[2], stranger]),
            Err(ColumnRuleError::UnknownColumn(stranger))
        );
    }

    #[test]
    fn add_transition_mirrors_ddl_constraints() {
        let (columns, transitions, ids) = fixture();
        // C -> A: new edge, fine (backward edges are legal, A-0002 option C).
        assert_eq!(
            check_add_transition(&columns, &transitions, ids[2], ids[0]),
            Ok(())
        );
        assert_eq!(
            check_add_transition(&columns, &transitions, ids[0], ids[0]),
            Err(ColumnRuleError::SelfTransition)
        );
        assert_eq!(
            check_add_transition(&columns, &transitions, ids[0], ids[1]),
            Err(ColumnRuleError::DuplicateTransition)
        );
        let stranger = Uuid::new_v4();
        assert_eq!(
            check_add_transition(&columns, &transitions, stranger, ids[1]),
            Err(ColumnRuleError::UnknownColumn(stranger))
        );
        assert_eq!(
            check_add_transition(&columns, &transitions, ids[0], stranger),
            Err(ColumnRuleError::UnknownColumn(stranger))
        );
    }

    // -- dead ends -------------------------------------------------------------

    #[test]
    fn dead_end_detection_flags_columns_without_outbound_edges() {
        let (columns, mut transitions, ids) = fixture();
        // Only C is a dead end initially (A->B, B->C/D, D->B).
        assert_eq!(
            dead_end_columns(&columns, &transitions)
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            [ids[2]]
        );
        // Remove B's outbound edges: B becomes a dead end too, position order.
        transitions.retain(|t| t.from_column_id != ids[1]);
        assert_eq!(
            dead_end_columns(&columns, &transitions)
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            [ids[1], ids[2]]
        );
    }

    // -- defaults parsing --------------------------------------------------------

    #[test]
    fn parse_default_config_round_trips_the_a0002_delivery_shape() {
        let config = parse_default_config(
            "Backlog\nTodo\nBlocked\nActive\nCompleted",
            "Backlog -> Todo\nTodo -> Active\nActive -> Completed\nTodo -> Blocked\nBlocked -> Todo\nActive -> Blocked\nBlocked -> Active",
        )
        .unwrap();
        assert_eq!(
            config.columns,
            ["Backlog", "Todo", "Blocked", "Active", "Completed"]
        );
        assert_eq!(config.transitions.len(), 7);
        assert!(
            config
                .transitions
                .contains(&("Blocked".into(), "Active".into()))
        );
    }

    #[test]
    fn parse_default_config_rejects_malformed_rows() {
        assert_eq!(
            parse_default_config("", "").unwrap_err(),
            DefaultConfigError::NoColumns
        );
        assert_eq!(
            parse_default_config("A\nA", "").unwrap_err(),
            DefaultConfigError::DuplicateColumn("A".into())
        );
        assert_eq!(
            parse_default_config("A\nB", "no arrow").unwrap_err(),
            DefaultConfigError::MalformedTransition("no arrow".into())
        );
        assert_eq!(
            parse_default_config("A\nB", "A -> ").unwrap_err(),
            DefaultConfigError::MalformedTransition("A ->".into())
        );
        assert_eq!(
            parse_default_config("A\nB", "A -> Z").unwrap_err(),
            DefaultConfigError::UnknownTransitionColumn {
                line: "A -> Z".into(),
                column: "Z".into()
            }
        );
        assert_eq!(
            parse_default_config("A\nB", "A -> A").unwrap_err(),
            DefaultConfigError::SelfTransition("A -> A".into())
        );
        assert_eq!(
            parse_default_config("A\nB", "A -> B\nA -> B").unwrap_err(),
            DefaultConfigError::DuplicateTransition("A -> B".into())
        );
        // Blank lines are tolerated.
        let config = parse_default_config("A\n\nB\n", "A -> B\n\n").unwrap();
        assert_eq!(config.columns, ["A", "B"]);
        assert_eq!(config.transitions, [("A".to_string(), "B".to_string())]);
    }
}
