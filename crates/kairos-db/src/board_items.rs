//! One page of the cards of a board (COLLIERY-T-0261).
//!
//! The cards of a board are in four tables (`strategies`, `initiatives`,
//! `tasks`, `adrs`). A page of the board needs ONE order for the cards of
//! the four tables, so this module reads the ids of a page with one
//! `UNION ALL` query (KAIROS-A-0009: raw SQL is the sanctioned escape
//! hatch). The caller then loads the rows of each table by id.
//!
//! # The order
//!
//! 1. the position of the column, then the id of the column (a removed
//!    column can have the position of a live column),
//! 2. the type of the card: strategy, initiative, task, ADR,
//! 3. the short code of the card, then the id of the card.
//!
//! The order is total, so two pages of one board have no card in common
//! and each card is on one page. It is the order that the route had
//! before it had pages: the columns by position, and the cards of each
//! type by short code.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Nullable, SmallInt, Uuid as SqlUuid};
use uuid::Uuid;

/// The type of a card, in the order of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CardKind {
    Strategy,
    Initiative,
    Task,
    Adr,
}

impl CardKind {
    fn from_rank(rank: i16) -> Option<Self> {
        match rank {
            0 => Some(Self::Strategy),
            1 => Some(Self::Initiative),
            2 => Some(Self::Task),
            3 => Some(Self::Adr),
            _ => None,
        }
    }
}

/// Which cards of the board are in the list. Each filter applies before
/// the page, and [`BoardItemPage::total`] is the count after the filters.
#[derive(Debug, Clone)]
pub struct BoardItemFilter<'a> {
    pub board_id: Uuid,
    /// The columns that the response has. A card in a different column is
    /// not in the list.
    pub column_ids: &'a [Uuid],
    /// Include the archived cards (KAIROS-T-0159).
    pub include_archived: bool,
    /// Narrow the TASKS to those of this repository (KAIROS-T-0104). The
    /// other types are unaffected.
    pub repository_id: Option<Uuid>,
    /// Narrow the STRATEGIES and the INITIATIVES by their teams
    /// (KAIROS-T-0321, [`crate::item_teams`]). The tasks and the ADRs are
    /// unaffected.
    pub team: Option<TeamFilter>,
}

/// The team filter of a board (KAIROS-T-0321).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamFilter {
    /// The items that have this team, from tasks or set by hand.
    Team(Uuid),
    /// The items that have no team.
    NoTeam,
}

/// One page of the cards of a board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardItemPage {
    /// The number of cards after the filters, on all pages.
    pub total: i64,
    /// The cards of the page, in the order of the page.
    pub cards: Vec<(CardKind, Uuid)>,
}

#[derive(QueryableByName)]
struct CardRow {
    #[diesel(sql_type = SmallInt)]
    kind: i16,
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

/// The cards of the four tables that pass the filters. `$1` is the board,
/// `$2` is "include the archived cards", `$3` is the repository of the
/// tasks (or NULL), and `$4` is the list of the columns. `$5` is a list of
/// strategies and initiatives (or NULL for no team filter), and `$6` says
/// if the filter keeps the items of the list (`true`) or the other items
/// (`false`).
const CARDS_SQL: &str = "\
    SELECT card.kind, card.id, card.short_code, c.position, c.id AS column_id \
    FROM ( \
        SELECT 0::smallint AS kind, id, short_code, column_id FROM strategies \
         WHERE board_id = $1 AND ($2 OR deleted_at IS NULL) \
           AND ($5::uuid[] IS NULL OR (id = ANY($5)) = $6) \
        UNION ALL \
        SELECT 1::smallint, id, short_code, column_id FROM initiatives \
         WHERE board_id = $1 AND ($2 OR deleted_at IS NULL) \
           AND ($5::uuid[] IS NULL OR (id = ANY($5)) = $6) \
        UNION ALL \
        SELECT 2::smallint, id, short_code, column_id FROM tasks \
         WHERE board_id = $1 AND ($2 OR deleted_at IS NULL) \
           AND ($3::uuid IS NULL OR repository_id = $3) \
        UNION ALL \
        SELECT 3::smallint, id, short_code, column_id FROM adrs \
         WHERE board_id = $1 AND ($2 OR deleted_at IS NULL) \
    ) card \
    JOIN board_columns c ON c.id = card.column_id \
    WHERE card.column_id = ANY($4)";

/// One page of the cards of a board: 2 queries (the count and the ids of
/// the page), for a board of each size, and 1 more with a team filter.
pub fn board_item_page(
    conn: &mut PgConnection,
    filter: &BoardItemFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<BoardItemPage, DieselError> {
    // The team filter: the items of the board that have the team (or any
    // team), then keep them or the others. One query for the board.
    let (team_items, keep): (Option<Vec<Uuid>>, bool) = match filter.team {
        None => (None, true),
        Some(team) => {
            let teams = crate::item_teams::board_teams(conn, filter.board_id)?;
            let with = |wanted: &dyn Fn(&crate::item_teams::ItemTeam) -> bool| -> Vec<Uuid> {
                teams
                    .iter()
                    .filter(|(_, item_teams)| item_teams.iter().any(wanted))
                    .map(|(id, _)| *id)
                    .collect()
            };
            match team {
                TeamFilter::Team(team_id) => (Some(with(&|t| t.team_id == team_id)), true),
                TeamFilter::NoTeam => (Some(with(&|_| true)), false),
            }
        }
    };
    let total = sql_query(format!("SELECT COUNT(*) AS count FROM ({CARDS_SQL}) cards"))
        .bind::<SqlUuid, _>(filter.board_id)
        .bind::<Bool, _>(filter.include_archived)
        .bind::<Nullable<SqlUuid>, _>(filter.repository_id)
        .bind::<Array<SqlUuid>, _>(filter.column_ids)
        .bind::<Nullable<Array<SqlUuid>>, _>(team_items.as_deref())
        .bind::<Bool, _>(keep)
        .get_result::<CountRow>(conn)?
        .count;
    let rows: Vec<CardRow> = sql_query(format!(
        "SELECT kind, id FROM ({CARDS_SQL}) cards \
         ORDER BY position, column_id, kind, short_code, id \
         LIMIT $7 OFFSET $8"
    ))
    .bind::<SqlUuid, _>(filter.board_id)
    .bind::<Bool, _>(filter.include_archived)
    .bind::<Nullable<SqlUuid>, _>(filter.repository_id)
    .bind::<Array<SqlUuid>, _>(filter.column_ids)
    .bind::<Nullable<Array<SqlUuid>>, _>(team_items.as_deref())
    .bind::<Bool, _>(keep)
    .bind::<BigInt, _>(limit)
    .bind::<BigInt, _>(offset)
    .load(conn)?;
    Ok(BoardItemPage {
        total,
        cards: rows
            .into_iter()
            .filter_map(|row| CardKind::from_rank(row.kind).map(|kind| (kind, row.id)))
            .collect(),
    })
}

/// One live column of a board, with the number of live cards in it.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ColumnCardCount {
    /// The board of the column.
    #[diesel(sql_type = SqlUuid)]
    pub board_id: Uuid,
    /// The column.
    #[diesel(sql_type = SqlUuid)]
    pub column_id: Uuid,
    /// The name of the column.
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub name: String,
    /// The number of live cards in the column, of the four types.
    #[diesel(sql_type = BigInt)]
    pub count: i64,
}

/// The live columns of the boards `$1`, each with the count of its live
/// cards. The database counts the cards (`GROUP BY`), and no row of a card
/// comes to the server.
const COLUMN_COUNTS_SQL: &str = "\
    SELECT c.board_id, c.id AS column_id, c.name, \
           COALESCE(n.count, 0)::bigint AS count \
    FROM board_columns c \
    LEFT JOIN ( \
        SELECT card.column_id, COUNT(*) AS count \
        FROM ( \
            SELECT column_id FROM strategies \
             WHERE board_id = ANY($1) AND deleted_at IS NULL \
            UNION ALL \
            SELECT column_id FROM initiatives \
             WHERE board_id = ANY($1) AND deleted_at IS NULL \
            UNION ALL \
            SELECT column_id FROM tasks \
             WHERE board_id = ANY($1) AND deleted_at IS NULL \
            UNION ALL \
            SELECT column_id FROM adrs \
             WHERE board_id = ANY($1) AND deleted_at IS NULL \
        ) card \
        GROUP BY card.column_id \
    ) n ON n.column_id = c.id \
    WHERE c.board_id = ANY($1) AND c.deleted_at IS NULL \
    ORDER BY c.board_id, c.position, c.id";

/// The live columns of `board_ids` in position order, each with the count
/// of its live cards (COLLIERY-T-0265): 1 query, for each number of boards
/// and for a board of each size. An archived card does not count (ADR-20
/// rule 5), and a removed column is not in the result.
///
/// The MCP tool `my_boards` prints the counts. Before this function it
/// read each row of each board to count them.
pub fn live_column_counts(
    conn: &mut PgConnection,
    board_ids: &[Uuid],
) -> Result<Vec<ColumnCardCount>, DieselError> {
    if board_ids.is_empty() {
        return Ok(Vec::new());
    }
    sql_query(COLUMN_COUNTS_SQL)
        .bind::<Array<SqlUuid>, _>(board_ids)
        .load(conn)
}
