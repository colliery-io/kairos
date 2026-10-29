//! The work of a person (KAIROS-T-0489, COLLIERY-I-0624): one query for the
//! "for me" page, the MCP tool `my_work` and the CLI `kairos me`.
//!
//! # The two lanes
//!
//! - **Initiatives**: the strategies and the initiatives that the person
//!   owns (`item_owners`).
//! - **Tasks**:
//!   - the tasks that the person has the claim on (`task_claims`). A claim
//!     that an agent key of the person made names the person, so it counts;
//!   - the tasks under an initiative that the person owns (a `parent` edge
//!     from the initiative), in a Todo, Active or Blocked column, claimed by
//!     anyone or by nobody;
//!   - the tasks that one of the tasks above blocks (a `blocks` edge from
//!     it), in any column that is not done.
//!
//! An item in a done column (`board_columns.is_done`) or archived is not in
//! the work. A cancelled task is in a done column.
//!
//! # The stand-up of a team (COLLIERY-I-0663, KAIROS-A-0077)
//!
//! [`team_work`] gives the work of one person in one team, for the
//! stand-up. Only the items of the team show, so a person in several
//! stand-ups sees each one apart:
//!
//! - **Initiatives**: the initiatives that the person owns and that have
//!   the team ([`crate::item_teams`]: from their tasks or set by hand). No
//!   strategies.
//! - **Tasks**: the tasks on a live board of the team that the person (or
//!   an agent key of the person) claims. Only the claims: not the tasks
//!   under the initiatives of the person, and not the tasks that the claims
//!   block.
//!
//! [`unclaimed_work`] gives the last stop of the round: the tasks on the
//! boards of the team that no one claims, in the state Todo or Blocked. A
//! task in Backlog is not in it. [`team_people`] gives the people of the
//! round.
//!
//! # The state of an item
//!
//! Each item has a state: Todo, Active or Blocked ([`WorkState`]).
//!
//! - **Blocked**: the item has an open blocker (a `blocks` edge from an item
//!   that is live and not in a done column), or it is in a column named
//!   Blocked (Dylan, 2026-10-15).
//! - **Active**: a task in a column that holds claims or that is named
//!   Active. A strategy or an initiative in a column named Active or
//!   Monitoring.
//! - **Todo**: each other item.
//!
//! Each public function operates in the CURRENT `search_path` tenant schema.

use std::collections::{BTreeSet, HashMap, HashSet};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, Bool, Text, Uuid as SqlUuid};
use uuid::Uuid;

use crate::item_owners::{self, ItemOwner};
use crate::item_teams;
use crate::models::claims::TaskClaim;
use crate::models::public::USER_KIND_HUMAN;
use crate::task_claims;

/// The state of an item of the work: the column of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkState {
    Todo,
    Active,
    Blocked,
}

impl WorkState {
    /// The wire name: `todo`, `active` or `blocked`.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkState::Todo => "todo",
            WorkState::Active => "active",
            WorkState::Blocked => "blocked",
        }
    }
}

/// Why an item is in the work of the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkReason {
    /// The person owns the strategy or the initiative.
    Owned,
    /// The person (or an agent key of the person) has the claim on the
    /// task.
    Claimed,
    /// The task is under an initiative that the person owns.
    UnderMyInitiative,
    /// A task of the work blocks the task.
    BlockedByMyWork,
    /// The stand-up: a task of the team that no one claims.
    Unclaimed,
}

impl WorkReason {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkReason::Owned => "owned",
            WorkReason::Claimed => "claimed",
            WorkReason::UnderMyInitiative => "under_my_initiative",
            WorkReason::BlockedByMyWork => "blocked_by_my_work",
            WorkReason::Unclaimed => "unclaimed",
        }
    }
}

/// An open blocker of an item: an item that is live, not in a done column,
/// and has a `blocks` edge to the item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
}

/// One item of the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    /// `strategy`, `initiative` or `task`.
    pub entity_type: String,
    pub board_id: Uuid,
    pub board_slug: String,
    pub board_name: String,
    pub column_id: Uuid,
    pub column_name: String,
    pub state: WorkState,
    /// Why the item is in the work; at least one reason.
    pub reasons: BTreeSet<WorkReason>,
    /// The claim of a task, or `None`.
    pub claim: Option<TaskClaim>,
    /// The owner of a strategy or an initiative, or `None`.
    pub owner: Option<ItemOwner>,
    /// The open blockers, by short code.
    pub blockers: Vec<Blocker>,
}

/// The work of a person: the two lanes, each by short code.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MyWork {
    /// The strategies and the initiatives that the person owns.
    pub initiatives: Vec<WorkItem>,
    /// The tasks of the person (module docs).
    pub tasks: Vec<WorkItem>,
}

/// A row of [`cards`]: an item with its board and column.
#[derive(QueryableByName)]
struct CardRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = SqlUuid)]
    board_id: Uuid,
    #[diesel(sql_type = Text)]
    board_slug: String,
    #[diesel(sql_type = Text)]
    board_name: String,
    #[diesel(sql_type = SqlUuid)]
    column_id: Uuid,
    #[diesel(sql_type = Text)]
    column_name: String,
    #[diesel(sql_type = Bool)]
    claims: bool,
}

/// The live strategies, initiatives and tasks of `ids` that are not in a
/// done column, with their board and column. One query.
fn cards(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<CardRow>, DieselError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sql_query(
        "SELECT i.id, i.short_code, i.title, i.entity_type, \
                b.id AS board_id, b.slug AS board_slug, b.name AS board_name, \
                c.id AS column_id, c.name AS column_name, c.claims \
           FROM (SELECT id, short_code, title, 'strategy' AS entity_type, board_id, column_id \
                   FROM strategies WHERE deleted_at IS NULL \
                 UNION ALL SELECT id, short_code, title, 'initiative', board_id, column_id \
                   FROM initiatives WHERE deleted_at IS NULL \
                 UNION ALL SELECT id, short_code, title, 'task', board_id, column_id \
                   FROM tasks WHERE deleted_at IS NULL) i \
           JOIN boards b ON b.id = i.board_id \
           JOIN board_columns c ON c.id = i.column_id \
          WHERE i.id = ANY($1) AND NOT c.is_done",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)
}

#[derive(QueryableByName)]
struct TargetRow {
    #[diesel(sql_type = SqlUuid)]
    target_id: Uuid,
}

/// The targets of the `relationship` edges with a source in `sources`. One
/// query.
fn targets_of(
    conn: &mut PgConnection,
    relationship: &str,
    sources: &[Uuid],
) -> Result<HashSet<Uuid>, DieselError> {
    if sources.is_empty() {
        return Ok(HashSet::new());
    }
    let rows: Vec<TargetRow> = sql_query(
        "SELECT target_id FROM item_relationships \
          WHERE relationship = $1 AND source_id = ANY($2)",
    )
    .bind::<Text, _>(relationship)
    .bind::<Array<SqlUuid>, _>(sources)
    .load(conn)?;
    Ok(rows.into_iter().map(|row| row.target_id).collect())
}

#[derive(QueryableByName)]
struct BlockerRow {
    #[diesel(sql_type = SqlUuid)]
    target_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
}

/// The open blockers of the items of `targets`, by item. One query.
fn open_blockers(
    conn: &mut PgConnection,
    targets: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Blocker>>, DieselError> {
    if targets.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<BlockerRow> = sql_query(
        "SELECT r.target_id, p.id, p.short_code, p.title \
           FROM item_relationships r \
           JOIN (SELECT id, short_code, title, column_id FROM strategies WHERE deleted_at IS NULL \
                 UNION ALL SELECT id, short_code, title, column_id FROM initiatives \
                   WHERE deleted_at IS NULL \
                 UNION ALL SELECT id, short_code, title, column_id FROM tasks \
                   WHERE deleted_at IS NULL \
                 UNION ALL SELECT id, short_code, title, column_id FROM adrs \
                   WHERE deleted_at IS NULL) p ON p.id = r.source_id \
           JOIN board_columns c ON c.id = p.column_id \
          WHERE r.relationship = 'blocks' AND r.target_id = ANY($1) AND NOT c.is_done \
          ORDER BY p.short_code",
    )
    .bind::<Array<SqlUuid>, _>(targets)
    .load(conn)?;
    let mut out: HashMap<Uuid, Vec<Blocker>> = HashMap::new();
    for row in rows {
        out.entry(row.target_id).or_default().push(Blocker {
            id: row.id,
            short_code: row.short_code,
            title: row.title,
        });
    }
    Ok(out)
}

/// Whether the column name is `name`, with no regard to case.
fn named(column: &str, name: &str) -> bool {
    column.trim().eq_ignore_ascii_case(name)
}

/// The state of a card (module docs).
fn state_of(card: &CardRow, blocked: bool) -> WorkState {
    if blocked || named(&card.column_name, "Blocked") {
        return WorkState::Blocked;
    }
    let active = if card.entity_type == "task" {
        card.claims || named(&card.column_name, "Active")
    } else {
        named(&card.column_name, "Active") || named(&card.column_name, "Monitoring")
    };
    if active {
        WorkState::Active
    } else {
        WorkState::Todo
    }
}

/// Whether a task under an initiative of the person is in the work: a task
/// in a Todo, Active or Blocked column. Not a task in Backlog.
fn in_flight(card: &CardRow) -> bool {
    card.claims
        || named(&card.column_name, "Todo")
        || named(&card.column_name, "Active")
        || named(&card.column_name, "Blocked")
}

/// The work of `user` (module docs). A fixed number of queries, whatever
/// the size of the work.
pub fn my_work(conn: &mut PgConnection, user: Uuid) -> Result<MyWork, DieselError> {
    use crate::schema::{item_owners as owners, task_claims as claims};

    let mut reasons: HashMap<Uuid, BTreeSet<WorkReason>> = HashMap::new();
    let mut add = |id: Uuid, reason: WorkReason| {
        reasons.entry(id).or_default().insert(reason);
    };

    // The strategies and the initiatives that the person owns.
    let owned_ids: Vec<Uuid> = owners::table
        .filter(owners::user_id.eq(user))
        .select(owners::item_id)
        .load(conn)?;
    let owned_cards = cards(conn, &owned_ids)?;
    for card in &owned_cards {
        add(card.id, WorkReason::Owned);
    }
    let my_initiatives: Vec<Uuid> = owned_cards
        .iter()
        .filter(|c| c.entity_type == "initiative")
        .map(|c| c.id)
        .collect();

    // The tasks of the claims of the person, and the tasks under the
    // initiatives.
    let claimed: HashSet<Uuid> = claims::table
        .filter(claims::user_id.eq(user))
        .select(claims::task_id)
        .load::<Uuid>(conn)?
        .into_iter()
        .collect();
    let children = targets_of(conn, "parent", &my_initiatives)?;
    let first: Vec<Uuid> = claimed.union(&children).copied().collect();
    let mut task_cards: Vec<CardRow> = Vec::new();
    for card in cards(conn, &first)? {
        if card.entity_type != "task" {
            continue;
        }
        let mine = claimed.contains(&card.id);
        let under = children.contains(&card.id) && in_flight(&card);
        if mine {
            add(card.id, WorkReason::Claimed);
        }
        if under {
            add(card.id, WorkReason::UnderMyInitiative);
        }
        if mine || under {
            task_cards.push(card);
        }
    }

    // The tasks that a task of the work blocks.
    let in_work: HashSet<Uuid> = task_cards.iter().map(|c| c.id).collect();
    let sources: Vec<Uuid> = in_work.iter().copied().collect();
    let blocked: Vec<Uuid> = targets_of(conn, "blocks", &sources)?.into_iter().collect();
    for card in cards(conn, &blocked)? {
        if card.entity_type != "task" {
            continue;
        }
        add(card.id, WorkReason::BlockedByMyWork);
        if !in_work.contains(&card.id) {
            task_cards.push(card);
        }
    }

    let initiatives = assemble(conn, owned_cards, &mut reasons)?;
    let tasks = assemble(conn, task_cards, &mut reasons)?;
    Ok(MyWork { initiatives, tasks })
}

/// The cards as items of the work, sorted by short code: their blockers,
/// claims and owners, and their states. Three queries.
fn assemble(
    conn: &mut PgConnection,
    cards: Vec<CardRow>,
    reasons: &mut HashMap<Uuid, BTreeSet<WorkReason>>,
) -> Result<Vec<WorkItem>, DieselError> {
    let ids: Vec<Uuid> = cards.iter().map(|c| c.id).collect();
    let mut blockers = open_blockers(conn, &ids)?;
    let mut claims_by_task = task_claims::claims_of(conn, &ids)?;
    let mut owners_by_item = item_owners::owners_of(conn, &ids)?;
    let mut items: Vec<WorkItem> = cards
        .into_iter()
        .map(|card| {
            let item_blockers = blockers.remove(&card.id).unwrap_or_default();
            let state = state_of(&card, !item_blockers.is_empty());
            WorkItem {
                id: card.id,
                reasons: reasons.remove(&card.id).unwrap_or_default(),
                claim: claims_by_task.remove(&card.id),
                owner: owners_by_item.remove(&card.id),
                short_code: card.short_code,
                title: card.title,
                entity_type: card.entity_type,
                board_id: card.board_id,
                board_slug: card.board_slug,
                board_name: card.board_name,
                column_id: card.column_id,
                column_name: card.column_name,
                state,
                blockers: item_blockers,
            }
        })
        .collect();
    items.sort_by(|a, b| a.short_code.cmp(&b.short_code));
    Ok(items)
}

/// The live boards of `team`. One query.
fn team_boards(conn: &mut PgConnection, team: Uuid) -> Result<HashSet<Uuid>, DieselError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::team_id.eq(team))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .load::<Uuid>(conn)?
        .into_iter()
        .collect())
}

/// The work of `user` in `team`, for the stand-up (module docs). A fixed
/// number of queries, whatever the size of the work.
pub fn team_work(conn: &mut PgConnection, team: Uuid, user: Uuid) -> Result<MyWork, DieselError> {
    use crate::schema::{item_owners as owners, task_claims as claims};

    let mut reasons: HashMap<Uuid, BTreeSet<WorkReason>> = HashMap::new();

    // The initiatives that the person owns and that have the team.
    let owned_ids: Vec<Uuid> = owners::table
        .filter(owners::user_id.eq(user))
        .select(owners::item_id)
        .load(conn)?;
    let owned: Vec<CardRow> = cards(conn, &owned_ids)?
        .into_iter()
        .filter(|c| c.entity_type == "initiative")
        .collect();
    let ids: Vec<Uuid> = owned.iter().map(|c| c.id).collect();
    let teams = item_teams::teams_of_items(conn, &ids)?;
    let initiative_cards: Vec<CardRow> = owned
        .into_iter()
        .filter(|c| {
            teams
                .get(&c.id)
                .is_some_and(|ts| ts.iter().any(|t| t.team_id == team))
        })
        .collect();
    for card in &initiative_cards {
        reasons
            .entry(card.id)
            .or_default()
            .insert(WorkReason::Owned);
    }

    // The claims of the person on the boards of the team.
    let boards = team_boards(conn, team)?;
    let claimed: Vec<Uuid> = claims::table
        .filter(claims::user_id.eq(user))
        .select(claims::task_id)
        .load(conn)?;
    let task_cards: Vec<CardRow> = cards(conn, &claimed)?
        .into_iter()
        .filter(|c| c.entity_type == "task" && boards.contains(&c.board_id))
        .collect();
    for card in &task_cards {
        reasons
            .entry(card.id)
            .or_default()
            .insert(WorkReason::Claimed);
    }

    let initiatives = assemble(conn, initiative_cards, &mut reasons)?;
    let tasks = assemble(conn, task_cards, &mut reasons)?;
    Ok(MyWork { initiatives, tasks })
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// The tasks of `team` that no one claims, in the state Todo or Blocked
/// (module docs), by short code. A fixed number of queries.
pub fn unclaimed_work(conn: &mut PgConnection, team: Uuid) -> Result<Vec<WorkItem>, DieselError> {
    let ids: Vec<Uuid> = sql_query(
        "SELECT t.id FROM tasks t \
           JOIN boards b ON b.id = t.board_id AND b.deleted_at IS NULL \
           JOIN board_columns c ON c.id = t.column_id \
          WHERE b.team_id = $1 AND t.deleted_at IS NULL AND NOT c.is_done \
            AND NOT EXISTS (SELECT 1 FROM task_claims k WHERE k.task_id = t.id)",
    )
    .bind::<SqlUuid, _>(team)
    .load::<IdRow>(conn)?
    .into_iter()
    .map(|row| row.id)
    .collect();
    let candidates: Vec<CardRow> = cards(conn, &ids)?.into_iter().filter(in_flight).collect();
    let mut reasons: HashMap<Uuid, BTreeSet<WorkReason>> = candidates
        .iter()
        .map(|c| (c.id, BTreeSet::from([WorkReason::Unclaimed])))
        .collect();
    Ok(assemble(conn, candidates, &mut reasons)?
        .into_iter()
        .filter(|item| matches!(item.state, WorkState::Todo | WorkState::Blocked))
        .collect())
}

/// A person of a team, for the round of the stand-up.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct TeamPerson {
    #[diesel(sql_type = SqlUuid)]
    pub user_id: Uuid,
    #[diesel(sql_type = Text)]
    pub user_name: String,
    #[diesel(sql_type = Text)]
    pub display_name: String,
}

/// The people of `team`, by display name. Service accounts are not
/// people. One query.
pub fn team_people(conn: &mut PgConnection, team: Uuid) -> Result<Vec<TeamPerson>, DieselError> {
    sql_query(
        "SELECT u.id AS user_id, u.user_name, u.display_name \
           FROM team_members m JOIN users u ON u.id = m.user_id \
          WHERE m.team_id = $1 AND u.kind = $2 \
          ORDER BY u.display_name, u.id",
    )
    .bind::<SqlUuid, _>(team)
    .bind::<Text, _>(USER_KIND_HUMAN)
    .load(conn)
}
