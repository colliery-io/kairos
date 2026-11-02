//! Flow metrics of a board (KAIROS-T-0372): cycle time, throughput per
//! week, the age of the work in progress, and the cumulative flow by
//! column per day. The source is the `activity_log`: each move of an item
//! writes a row.
//!
//! # The rows that move an item
//!
//! - `transition`, details `column:<from name>-><to name>`;
//! - `cancel`, details `column:<from name>-><to name> reason:<text>` (the
//!   cancel moves a task to the done column, KAIROS-T-0362);
//! - `board_move`, details `board:<from id>-><to id> column:<from
//!   id>-><to id>` (a task that comes to the board, or leaves it).
//!
//! A `transition` row names the columns by their name at the time of the
//! move. A column that has a different name now keeps its old name in the
//! history: the cumulative flow shows the old name as a column of its own.
//!
//! # The definitions
//!
//! - **Start**: the first entry of the item into a START column. The start
//!   columns are the columns with `claims` (the Active column of a delivery
//!   board, KAIROS-T-0359). A board with no such column uses the column
//!   with the name "Active", in any case. A board with neither has no start,
//!   and no cycle time.
//! - **Done**: the item is in a column with `is_done` now; the done time is
//!   its last entry into a done column.
//! - **Cancelled**: a task with the cancel mark (`task_cancellations`). It
//!   counts apart: not in the done throughput, and not in the cycle time.
//! - **Cycle time**: from the start to the done time, in days, for each
//!   done item (not cancelled) whose done time is in the range and that has
//!   a start.
//! - **Throughput**: the done items and the cancelled tasks of each week
//!   (weeks start on Monday, UTC) of the range, by the done time and the
//!   time of the cancel.
//! - **Work in progress**: each item that is not in a done column and has
//!   a start. Its age is the time from its start to now.
//! - **Cumulative flow**: for each day of the range (UTC), the number of
//!   live items in each column at the end of the day.
//!
//! The items of the board count. An archived item counts for its time on
//! the board (KAIROS-T-0571): its timeline ends at the archive. So an
//! item archived from a done column keeps its done time and its cycle
//! time, and "Archive completed" keeps the throughput. An archived item is
//! not work in progress, and the cumulative flow drops it on the day of
//! the archive. An item that is on a different board now does not count.
//!
//! # The read is bounded by the range (KAIROS-T-0627)
//!
//! [`load_moves`] reads each move from the start of the range on. Before
//! the range it reads at most four moves of each item, the ones that
//! [`compute`] can use: the first (the column at the creation), the last
//! (the column at the start of the range), the first entry into a start
//! column, and the last entry into a done column. The numbers are the same
//! as from every move. The cumulative flow has an old column name only
//! when it has an item on a day of the range. [`load_board`] reads the
//! archived items whose archive is in the range or after it: an item
//! archived before the range has no part in it.
//!
//! [`FlowBoard::stamp`] is a hash of what the board has now: the columns,
//! the items with their column and the time of their last change, and the
//! cancel marks. A move changes it, so a cache with the stamp in its key
//! needs no drop on a transition.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use uuid::Uuid;

/// The most days that one range can have.
pub const MAX_RANGE_DAYS: i64 = 366;

/// A column of the board, for the metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowColumn {
    pub id: Uuid,
    pub name: String,
    pub is_done: bool,
    pub claims: bool,
}

/// An item of the board: live, or archived (KAIROS-T-0571).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowItem {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    /// The column now; for an archived item, the column it was archived
    /// from.
    pub column_id: Uuid,
    pub created_at: DateTime<Utc>,
    /// The time of the archive; `None` for a live item.
    pub archived_at: Option<DateTime<Utc>>,
}

/// One move of an item: the time and the column after it, by name. `None`
/// for a move that takes the item off the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowMove {
    pub item_id: Uuid,
    pub at: DateTime<Utc>,
    /// The column before the move, by name; `None` when the item came from
    /// a different board.
    pub from: Option<String>,
    /// The column after the move, by name; `None` when the item left the
    /// board.
    pub to: Option<String>,
}

/// What [`compute`] reads.
#[derive(Debug, Clone, Default)]
pub struct FlowInput {
    /// In position order.
    pub columns: Vec<FlowColumn>,
    pub items: Vec<FlowItem>,
    /// In time order.
    pub moves: Vec<FlowMove>,
    /// The cancelled tasks, with the time of the cancel.
    pub cancelled: HashMap<Uuid, DateTime<Utc>>,
}

/// The cycle time of one done item.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleTime {
    pub short_code: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub done_at: DateTime<Utc>,
    pub days: f64,
}

/// The throughput of one week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekThroughput {
    /// The Monday of the week.
    pub week_start: NaiveDate,
    pub done: i64,
    pub cancelled: i64,
}

/// One item in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct AgeingItem {
    pub short_code: String,
    pub title: String,
    pub column: String,
    pub started_at: DateTime<Utc>,
    pub age_days: f64,
}

/// The counts of one day of the cumulative flow, in the order of
/// [`Flow::cfd_columns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDay {
    pub date: NaiveDate,
    pub counts: Vec<i64>,
}

/// The flow metrics of a board for a range.
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// The names of the start columns; empty when the board has none.
    pub start_columns: Vec<String>,
    pub done_columns: Vec<String>,
    /// Done items in the range (not cancelled), oldest done first.
    pub cycle_times: Vec<CycleTime>,
    /// The done items in the range with no start (no cycle time).
    pub done_without_start: i64,
    pub throughput: Vec<WeekThroughput>,
    /// Oldest first.
    pub ageing: Vec<AgeingItem>,
    /// The columns of the cumulative flow: the columns of the board in
    /// position order, then the old names that have an item on a day of
    /// the range, in name order.
    pub cfd_columns: Vec<String>,
    pub cfd: Vec<FlowDay>,
}

impl Flow {
    /// Set the age of each item in progress to its age at `now`: a cached
    /// result is right at the time of each read.
    pub fn age_at(&mut self, now: DateTime<Utc>) {
        for item in &mut self.ageing {
            item.age_days = days_between(item.started_at, now).max(0.0);
        }
    }

    /// The number of done items in the range (not cancelled).
    pub fn done_count(&self) -> i64 {
        self.throughput.iter().map(|w| w.done).sum()
    }

    /// The number of cancelled tasks in the range.
    pub fn cancelled_count(&self) -> i64 {
        self.throughput.iter().map(|w| w.cancelled).sum()
    }
}

/// A sorted percentile (nearest rank). `None` for no value.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

/// The Monday of the week of `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Days::new(u64::from(date.weekday().num_days_from_monday()))
}

fn days_between(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_seconds() as f64 / 86_400.0
}

/// The end of the day `date` (UTC): the first instant of the next day.
fn end_of(date: NaiveDate) -> DateTime<Utc> {
    (date + Days::new(1))
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
}

/// The columns of an item over time: `(time, column name)`; `None` for a
/// time when the item is not on the board.
type Timeline = Vec<(DateTime<Utc>, Option<String>)>;

/// The names of the start columns and of the done columns (module docs).
fn start_and_done_names(columns: &[FlowColumn]) -> (HashSet<String>, HashSet<String>) {
    let mut start_names: HashSet<String> = columns
        .iter()
        .filter(|c| c.claims)
        .map(|c| c.name.clone())
        .collect();
    if start_names.is_empty() {
        start_names = columns
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case("active"))
            .map(|c| c.name.clone())
            .collect();
    }
    let done_names = columns
        .iter()
        .filter(|c| c.is_done)
        .map(|c| c.name.clone())
        .collect();
    (start_names, done_names)
}

/// Compute the metrics of `input` for the days `from..=to`, at `now`. Pure.
pub fn compute(input: &FlowInput, from: NaiveDate, to: NaiveDate, now: DateTime<Utc>) -> Flow {
    let name_of: HashMap<Uuid, &str> = input
        .columns
        .iter()
        .map(|c| (c.id, c.name.as_str()))
        .collect();
    let (start_names, done_names) = start_and_done_names(&input.columns);
    let mut moves_of: HashMap<Uuid, Vec<&FlowMove>> = HashMap::new();
    for m in &input.moves {
        moves_of.entry(m.item_id).or_default().push(m);
    }
    for list in moves_of.values_mut() {
        list.sort_by_key(|m| m.at);
    }

    // The timeline of each item on the board: (time, column name or None).
    // Before the first move, the item is in the column that the first move
    // leaves (or, with no move, in its column now), from its creation.
    let mut timelines: Vec<(&FlowItem, Timeline)> = Vec::new();
    for item in &input.items {
        let moves = moves_of.get(&item.id).map(Vec::as_slice).unwrap_or(&[]);
        let current = name_of.get(&item.column_id).map(|s| s.to_string());
        let mut line = Vec::new();
        match moves.first() {
            Some(first) => {
                if first.from.is_some() {
                    line.push((item.created_at, first.from.clone()));
                }
            }
            None => line.push((item.created_at, current.clone())),
        }
        for m in moves {
            line.push((m.at, m.to.clone()));
        }
        // KAIROS-T-0571: an archived item leaves the board at its archive.
        if let Some(archived_at) = item.archived_at {
            line.push((archived_at, None));
        }
        timelines.push((item, line));
    }

    let range_start = start_of(from);
    let range_end = end_of(to);
    let in_range = |at: DateTime<Utc>| at >= range_start && at < range_end;

    let mut cycle_times = Vec::new();
    let mut done_without_start = 0;
    let mut weeks: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
    let mut week = week_start(from);
    while week <= to {
        weeks.insert(week, (0, 0));
        week = week + Days::new(7);
    }
    let mut ageing = Vec::new();
    for (item, line) in &timelines {
        let started_at = line
            .iter()
            .find(|(_, column)| column.as_ref().is_some_and(|c| start_names.contains(c)))
            .map(|(at, _)| *at);
        let current = name_of.get(&item.column_id).map(|s| s.to_string());
        let is_done = current.as_ref().is_some_and(|c| done_names.contains(c));
        if let Some(cancelled_at) = input.cancelled.get(&item.id) {
            if in_range(*cancelled_at)
                && let Some(entry) = weeks.get_mut(&week_start(cancelled_at.date_naive()))
            {
                entry.1 += 1;
            }
            continue;
        }
        if is_done {
            let done_at = line
                .iter()
                .rev()
                .find(|(_, column)| column.as_ref().is_some_and(|c| done_names.contains(c)))
                .map(|(at, _)| *at)
                .unwrap_or(item.created_at);
            if !in_range(done_at) {
                continue;
            }
            if let Some(entry) = weeks.get_mut(&week_start(done_at.date_naive())) {
                entry.0 += 1;
            }
            match started_at {
                Some(started_at) if started_at <= done_at => cycle_times.push(CycleTime {
                    short_code: item.short_code.clone(),
                    title: item.title.clone(),
                    started_at,
                    done_at,
                    days: days_between(started_at, done_at),
                }),
                _ => done_without_start += 1,
            }
        } else if item.archived_at.is_none()
            && let Some(started_at) = started_at
        {
            ageing.push(AgeingItem {
                short_code: item.short_code.clone(),
                title: item.title.clone(),
                column: current.unwrap_or_default(),
                started_at,
                age_days: days_between(started_at, now).max(0.0),
            });
        }
    }
    cycle_times.sort_by_key(|c| c.done_at);
    ageing.sort_by(|a, b| b.age_days.total_cmp(&a.age_days));

    // The cumulative flow: the columns of the board, then the old names
    // in name order.
    let mut cfd_columns: Vec<String> = input.columns.iter().map(|c| c.name.clone()).collect();
    let old_names: BTreeSet<&String> = timelines
        .iter()
        .flat_map(|(_, line)| line.iter().filter_map(|(_, column)| column.as_ref()))
        .filter(|column| !cfd_columns.contains(column))
        .collect();
    let board_columns = cfd_columns.len();
    cfd_columns.extend(old_names.into_iter().cloned());
    let index: HashMap<&str, usize> = cfd_columns
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();
    let mut cfd = Vec::new();
    let mut day = from;
    while day <= to {
        let end = end_of(day);
        let mut counts = vec![0i64; cfd_columns.len()];
        for (_, line) in &timelines {
            let at_end = line
                .iter()
                .take_while(|(at, _)| *at < end)
                .last()
                .and_then(|(_, column)| column.as_deref());
            if let Some(column) = at_end
                && let Some(i) = index.get(column)
            {
                counts[*i] += 1;
            }
        }
        cfd.push(FlowDay { date: day, counts });
        day = day + Days::new(1);
    }
    // An old name with no item on any day of the range is not a column of
    // the cumulative flow: the bounded read (module docs) does not see it.
    let keep: Vec<bool> = (0..cfd_columns.len())
        .map(|i| i < board_columns || cfd.iter().any(|d| d.counts[i] > 0))
        .collect();
    if keep.contains(&false) {
        let mut i = 0;
        cfd_columns.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        for day in &mut cfd {
            let mut i = 0;
            day.counts.retain(|_| {
                i += 1;
                keep[i - 1]
            });
        }
    }

    let mut start_columns: Vec<String> = input
        .columns
        .iter()
        .filter(|c| start_names.contains(&c.name))
        .map(|c| c.name.clone())
        .collect();
    start_columns.dedup();
    Flow {
        from,
        to,
        start_columns,
        done_columns: input
            .columns
            .iter()
            .filter(|c| c.is_done)
            .map(|c| c.name.clone())
            .collect(),
        cycle_times,
        done_without_start,
        throughput: weeks
            .into_iter()
            .map(|(week_start, (done, cancelled))| WeekThroughput {
                week_start,
                done,
                cancelled,
            })
            .collect(),
        ageing,
        cfd_columns,
        cfd,
    }
}

/// `column:<from>-><to>` (and a `reason:` after it) → the two names.
fn parse_column_move(details: &str) -> Option<(String, String)> {
    let rest = details.strip_prefix("column:")?;
    let rest = rest.split(" reason:").next().unwrap_or(rest);
    let (from, to) = rest.split_once("->")?;
    Some((from.to_string(), to.to_string()))
}

/// `board:<a>-><b> column:<x>-><y>` → the four ids.
fn parse_board_move(details: &str) -> Option<(Uuid, Uuid, Uuid, Uuid)> {
    let rest = details.strip_prefix("board:")?;
    let (boards, columns) = rest.split_once(" column:")?;
    let (from_board, to_board) = boards.split_once("->")?;
    let (from_column, to_column) = columns.split_once("->")?;
    Some((
        from_board.parse().ok()?,
        to_board.parse().ok()?,
        from_column.parse().ok()?,
        to_column.parse().ok()?,
    ))
}

/// What the board has now (module docs): the cheap part of the input of
/// [`compute`], with the stamp of a cache key.
#[derive(Debug, Clone, Default)]
pub struct FlowBoard {
    /// In position order.
    pub columns: Vec<FlowColumn>,
    pub items: Vec<FlowItem>,
    /// The cancelled tasks, with the time of the cancel.
    pub cancelled: HashMap<Uuid, DateTime<Utc>>,
    /// The name of each column of the board, removed or not, by id: a
    /// `board_move` row names its columns by id.
    pub column_names: HashMap<Uuid, String>,
    /// A hash of the columns, the items (with their column and the time of
    /// their last change) and the cancel marks.
    pub stamp: u64,
}

/// Read the live columns of the board `board_id`, its items (of the four
/// types) and the cancelled tasks. 4 queries and one for each type. The
/// items are the live items, and the archived items with an archive at
/// `since` or after it (KAIROS-T-0571, module docs).
pub fn load_board(
    conn: &mut PgConnection,
    board_id: Uuid,
    since: DateTime<Utc>,
) -> Result<FlowBoard, DieselError> {
    use crate::schema::{adrs, board_columns, initiatives, strategies, tasks};

    let columns: Vec<FlowColumn> = board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::deleted_at.is_null())
        .order((board_columns::position.asc(), board_columns::id.asc()))
        .select((
            board_columns::id,
            board_columns::name,
            board_columns::is_done,
            board_columns::claims,
        ))
        .load::<(Uuid, String, bool, bool)>(conn)?
        .into_iter()
        .map(|(id, name, is_done, claims)| FlowColumn {
            id,
            name,
            is_done,
            claims,
        })
        .collect();

    type Row = (
        Uuid,
        String,
        String,
        Uuid,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    let mut rows: Vec<Row> = Vec::new();
    macro_rules! board_items {
        ($table:ident) => {
            rows.extend(
                $table::table
                    .filter($table::board_id.eq(board_id))
                    .filter(
                        $table::deleted_at
                            .is_null()
                            .or($table::deleted_at.ge(since)),
                    )
                    .select((
                        $table::id,
                        $table::short_code,
                        $table::title,
                        $table::column_id,
                        $table::created_at,
                        $table::updated_at,
                        $table::deleted_at,
                    ))
                    .load::<Row>(conn)?,
            )
        };
    }
    board_items!(strategies);
    board_items!(initiatives);
    board_items!(tasks);
    rows.extend(
        adrs::table
            .filter(adrs::board_id.eq(Some(board_id)))
            .filter(adrs::deleted_at.is_null().or(adrs::deleted_at.ge(since)))
            .filter(adrs::column_id.is_not_null())
            .select((
                adrs::id,
                adrs::short_code,
                adrs::title,
                adrs::column_id.assume_not_null(),
                adrs::created_at,
                adrs::updated_at,
                adrs::deleted_at,
            ))
            .load::<Row>(conn)?,
    );
    let column_names: HashMap<Uuid, String> = board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .select((board_columns::id, board_columns::name))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .collect();
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let cancelled: HashMap<Uuid, DateTime<Utc>> = {
        use crate::schema::task_cancellations as tc;
        tc::table
            .filter(tc::task_id.eq_any(&ids))
            .select((tc::task_id, tc::cancelled_at))
            .load::<(Uuid, DateTime<Utc>)>(conn)?
            .into_iter()
            .collect()
    };

    let mut hasher = DefaultHasher::new();
    for column in &columns {
        (column.id, &column.name, column.is_done, column.claims).hash(&mut hasher);
    }
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by_key(|row| row.0);
    sorted.hash(&mut hasher);
    let mut marks: Vec<(&Uuid, &DateTime<Utc>)> = cancelled.iter().collect();
    marks.sort();
    marks.hash(&mut hasher);
    let mut names: Vec<(&Uuid, &String)> = column_names.iter().collect();
    names.sort();
    names.hash(&mut hasher);
    let stamp = hasher.finish();

    let items = rows
        .into_iter()
        .map(
            |(id, short_code, title, column_id, created_at, _, archived_at)| FlowItem {
                id,
                short_code,
                title,
                column_id,
                created_at,
                archived_at,
            },
        )
        .collect();
    Ok(FlowBoard {
        columns,
        items,
        cancelled,
        column_names,
        stamp,
    })
}

/// A row of the activity log that moves an item.
#[derive(QueryableByName)]
struct MoveRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    entity_id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Text)]
    action: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    details: String,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    occurred_at: DateTime<Utc>,
}

/// The part of the details of a `transition` or a `cancel` row that
/// [`parse_column_move`] splits: after `column:`, before ` reason:`.
const BODY: &str = "split_part(substr(a.details, 8), ' reason:', 1)";

/// The rows of the items `$1` before `$2` that move an item on the board
/// or onto it or off it (`$5`, `$6`: the patterns of a `board_move` row
/// from and to the board).
fn before_range_sql() -> String {
    let to = format!("substr({BODY}, strpos({BODY}, '->') + 2)");
    let moves = format!(
        "a.entity_id = e.id AND a.occurred_at < $2 \
         AND ((a.action IN ('transition', 'cancel') AND a.details LIKE 'column:%' \
               AND strpos({BODY}, '->') > 0) \
              OR (a.action = 'board_move' AND (a.details LIKE $5 OR a.details LIKE $6)))"
    );
    let into = |names: &str, patterns: &str| {
        format!(
            "((a.action <> 'board_move' AND {to} = ANY({names})) \
              OR (a.action = 'board_move' AND a.details LIKE ANY({patterns})))"
        )
    };
    let pick = |filter: &str, order: &str| {
        format!(
            "(SELECT a.id, a.entity_id, a.action, a.details, a.occurred_at \
               FROM activity_log a WHERE {moves}{filter} \
              ORDER BY a.occurred_at {order}, a.id {order} LIMIT 1)"
        )
    };
    format!(
        "SELECT x.entity_id, x.action, x.details, x.occurred_at \
           FROM unnest($1::uuid[]) AS e(id) \
           CROSS JOIN LATERAL ({} UNION {} UNION {} UNION {}) x",
        pick("", "ASC"),
        pick("", "DESC"),
        pick(&format!(" AND {}", into("$3", "$7")), "ASC"),
        pick(&format!(" AND {}", into("$4", "$8")), "DESC"),
    )
}

/// Read the moves of the items of `board` that [`compute`] needs for a
/// range that starts at `since` (module docs): each move from `since` on,
/// and at most four moves of each item before it. 2 queries.
pub fn load_moves(
    conn: &mut PgConnection,
    board_id: Uuid,
    board: &FlowBoard,
    since: DateTime<Utc>,
) -> Result<Vec<FlowMove>, DieselError> {
    use crate::schema::activity_log;
    use diesel::sql_types::{Array, Text, Timestamptz};

    let ids: Vec<Uuid> = board.items.iter().map(|i| i.id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let (start_names, done_names) = start_and_done_names(&board.columns);
    // A `board_move` onto a column with a start (or done) name.
    let onto = |names: &HashSet<String>| -> Vec<String> {
        board
            .column_names
            .iter()
            .filter(|(_, name)| names.contains(*name))
            .map(|(id, _)| format!("board:%->{board_id} column:%->{id}"))
            .collect()
    };
    let mut rows: Vec<MoveRow> = sql_query(before_range_sql())
        .bind::<Array<diesel::sql_types::Uuid>, _>(&ids)
        .bind::<Timestamptz, _>(since)
        .bind::<Array<Text>, _>(start_names.iter().cloned().collect::<Vec<_>>())
        .bind::<Array<Text>, _>(done_names.iter().cloned().collect::<Vec<_>>())
        .bind::<Text, _>(format!("board:{board_id}->%"))
        .bind::<Text, _>(format!("board:%->{board_id} column:%"))
        .bind::<Array<Text>, _>(onto(&start_names))
        .bind::<Array<Text>, _>(onto(&done_names))
        .load(conn)?;
    rows.extend(
        activity_log::table
            .filter(activity_log::entity_id.eq_any(&ids))
            .filter(activity_log::action.eq_any(["transition", "cancel", "board_move"]))
            .filter(activity_log::occurred_at.ge(since))
            .select((
                activity_log::entity_id.assume_not_null(),
                activity_log::action,
                activity_log::details,
                activity_log::occurred_at,
            ))
            .load::<(Uuid, String, String, DateTime<Utc>)>(conn)?
            .into_iter()
            .map(|(entity_id, action, details, occurred_at)| MoveRow {
                entity_id,
                action,
                details,
                occurred_at,
            }),
    );
    rows.sort_by_key(|row| row.occurred_at);

    let mut moves = Vec::new();
    for MoveRow {
        entity_id: item_id,
        action,
        details,
        occurred_at: at,
    } in rows
    {
        if action == "board_move" {
            let Some((from_board, to_board, from_column, to_column)) = parse_board_move(&details)
            else {
                continue;
            };
            if to_board == board_id {
                moves.push(FlowMove {
                    item_id,
                    at,
                    from: None,
                    to: board.column_names.get(&to_column).cloned(),
                });
            } else if from_board == board_id {
                moves.push(FlowMove {
                    item_id,
                    at,
                    from: board.column_names.get(&from_column).cloned(),
                    to: None,
                });
            }
        } else if let Some((from, to)) = parse_column_move(&details) {
            moves.push(FlowMove {
                item_id,
                at,
                from: Some(from),
                to: Some(to),
            });
        }
    }
    Ok(moves)
}

impl FlowInput {
    /// The input of a board and its moves.
    pub fn new(board: FlowBoard, moves: Vec<FlowMove>) -> Self {
        FlowInput {
            columns: board.columns,
            items: board.items,
            moves,
            cancelled: board.cancelled,
        }
    }
}

/// The first instant of the day `date` (UTC): the `since` of a range that
/// starts on that day.
pub fn start_of(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0).expect("midnight").and_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().expect("a time")
    }

    fn date(text: &str) -> NaiveDate {
        text.parse().expect("a date")
    }

    fn column(name: &str, is_done: bool, claims: bool) -> FlowColumn {
        FlowColumn {
            id: Uuid::new_v4(),
            name: name.to_string(),
            is_done,
            claims,
        }
    }

    fn mv(item: Uuid, when: &str, from: &str, to: &str) -> FlowMove {
        FlowMove {
            item_id: item,
            at: at(when),
            from: Some(from.to_string()),
            to: Some(to.to_string()),
        }
    }

    /// A board Backlog, Todo, Active (claims), Done (done), and 4 tasks:
    /// A done (Active Mon 09:00, Done Wed 09:00 = 2 days), B in Active since
    /// Tue, C cancelled on Thu, D done with no start.
    fn board() -> (FlowInput, [Uuid; 4]) {
        let columns = vec![
            column("Backlog", false, false),
            column("Todo", false, false),
            column("Active", false, true),
            column("Done", true, false),
        ];
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let item = |i: usize, code: &str, column: usize| FlowItem {
            id: ids[i],
            short_code: code.to_string(),
            title: format!("Task {code}"),
            column_id: columns[column].id,
            created_at: at("2026-09-28T08:00:00Z"),
            archived_at: None,
        };
        let items = vec![
            item(0, "A", 3),
            item(1, "B", 2),
            item(2, "C", 3),
            item(3, "D", 3),
        ];
        let moves = vec![
            mv(ids[0], "2026-09-28T08:30:00Z", "Backlog", "Todo"),
            mv(ids[0], "2026-09-28T09:00:00Z", "Todo", "Active"),
            mv(ids[0], "2026-09-30T09:00:00Z", "Active", "Done"),
            mv(ids[1], "2026-09-29T10:00:00Z", "Backlog", "Active"),
            mv(ids[2], "2026-10-01T10:00:00Z", "Backlog", "Done"),
            mv(ids[3], "2026-10-06T10:00:00Z", "Todo", "Done"),
        ];
        let cancelled = HashMap::from([(ids[2], at("2026-10-01T10:00:00Z"))]);
        (
            FlowInput {
                columns,
                items,
                moves,
                cancelled,
            },
            ids,
        )
    }

    #[test]
    fn cycle_time_is_from_the_first_start_to_the_done_time() {
        let (input, _) = board();
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(flow.start_columns, vec!["Active"]);
        assert_eq!(flow.done_columns, vec!["Done"]);
        assert_eq!(flow.cycle_times.len(), 1);
        assert_eq!(flow.cycle_times[0].short_code, "A");
        assert!((flow.cycle_times[0].days - 2.0).abs() < 1e-9);
        assert_eq!(flow.done_without_start, 1, "D has no start");
    }

    #[test]
    fn throughput_counts_cancelled_tasks_apart() {
        let (input, _) = board();
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(
            flow.throughput,
            vec![
                WeekThroughput {
                    week_start: date("2026-09-28"),
                    done: 1,
                    cancelled: 1,
                },
                WeekThroughput {
                    week_start: date("2026-10-05"),
                    done: 1,
                    cancelled: 0,
                },
            ]
        );
        assert_eq!(flow.done_count(), 2);
        assert_eq!(flow.cancelled_count(), 1);
    }

    #[test]
    fn the_age_of_work_in_progress_is_from_its_start() {
        let (input, _) = board();
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(flow.ageing.len(), 1);
        assert_eq!(flow.ageing[0].short_code, "B");
        assert_eq!(flow.ageing[0].column, "Active");
        assert!((flow.ageing[0].age_days - 12.0).abs() < 1e-9);
    }

    #[test]
    fn the_cumulative_flow_counts_each_column_at_the_end_of_each_day() {
        let (input, _) = board();
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-01"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(flow.cfd_columns, vec!["Backlog", "Todo", "Active", "Done"]);
        let days: Vec<(String, Vec<i64>)> = flow
            .cfd
            .iter()
            .map(|d| (d.date.to_string(), d.counts.clone()))
            .collect();
        assert_eq!(
            days,
            vec![
                // Mon: A in Active; B, C in Backlog; D in Todo.
                ("2026-09-28".to_string(), vec![2, 1, 1, 0]),
                // Tue: B to Active.
                ("2026-09-29".to_string(), vec![1, 1, 2, 0]),
                // Wed: A done.
                ("2026-09-30".to_string(), vec![1, 1, 1, 1]),
                // Thu: C cancelled (to Done).
                ("2026-10-01".to_string(), vec![0, 1, 1, 2]),
            ]
        );
    }

    #[test]
    fn a_board_with_no_claims_column_starts_at_active_by_name() {
        let (mut input, _) = board();
        for c in &mut input.columns {
            c.claims = false;
        }
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(flow.start_columns, vec!["Active"]);
        for c in &mut input.columns {
            if c.name == "Active" {
                c.name = "Doing".to_string();
            }
        }
        input.moves.clear();
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        assert!(flow.start_columns.is_empty());
        assert!(flow.cycle_times.is_empty());
    }

    /// KAIROS-T-0627: an old column name is a column of the cumulative flow
    /// only when it has an item on a day of the range.
    #[test]
    fn an_old_name_with_no_item_in_the_range_is_not_a_column() {
        let (mut input, ids) = board();
        // B was in "Doing" (the old name of a column) before the range,
        // and A was in "Review" on 2026-09-29 only.
        input
            .moves
            .push(mv(ids[1], "2026-09-28T08:10:00Z", "Backlog", "Doing"));
        input
            .moves
            .push(mv(ids[1], "2026-09-28T08:20:00Z", "Doing", "Backlog"));
        input
            .moves
            .push(mv(ids[0], "2026-09-29T08:00:00Z", "Active", "Review"));
        input
            .moves
            .push(mv(ids[0], "2026-09-29T23:00:00Z", "Review", "Active"));
        input
            .moves
            .push(mv(ids[0], "2026-09-29T23:30:00Z", "Active", "Review"));
        let flow = compute(
            &input,
            date("2026-09-29"),
            date("2026-10-01"),
            at("2026-10-11T10:00:00Z"),
        );
        assert_eq!(
            flow.cfd_columns,
            vec!["Backlog", "Todo", "Active", "Done", "Review"]
        );
        assert!(flow.cfd.iter().all(|d| d.counts.len() == 5));
    }

    /// KAIROS-T-0571: an archived item counts for its time on the board.
    /// A (done) and C (cancelled) are archived on Fri 2026-10-02, B (in
    /// progress) on Sat 2026-10-03.
    #[test]
    fn an_archived_item_counts_until_its_archive() {
        let (mut input, ids) = board();
        for item in &mut input.items {
            if item.id == ids[0] || item.id == ids[2] {
                item.archived_at = Some(at("2026-10-02T12:00:00Z"));
            }
            if item.id == ids[1] {
                item.archived_at = Some(at("2026-10-03T12:00:00Z"));
            }
        }
        let flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        // The done time, the cycle time and the cancel stay.
        assert_eq!(flow.done_count(), 2);
        assert_eq!(flow.cancelled_count(), 1);
        assert_eq!(flow.cycle_times.len(), 1);
        assert_eq!(flow.cycle_times[0].short_code, "A");
        // An archived item is not work in progress.
        assert!(flow.ageing.is_empty(), "{:?}", flow.ageing);
        // The cumulative flow drops each item on the day of its archive.
        let day = |date: &str| {
            flow.cfd
                .iter()
                .find(|d| d.date.to_string() == date)
                .map(|d| d.counts.clone())
                .expect("a day")
        };
        assert_eq!(day("2026-10-01"), vec![0, 1, 1, 2]);
        assert_eq!(day("2026-10-02"), vec![0, 1, 1, 0]);
        assert_eq!(day("2026-10-03"), vec![0, 1, 0, 0]);
        assert_eq!(day("2026-10-06"), vec![0, 0, 0, 1]);
    }

    #[test]
    fn a_cached_result_gets_the_age_at_the_read() {
        let (input, _) = board();
        let mut flow = compute(
            &input,
            date("2026-09-28"),
            date("2026-10-11"),
            at("2026-10-11T10:00:00Z"),
        );
        flow.age_at(at("2026-10-12T10:00:00Z"));
        assert!((flow.ageing[0].age_days - 13.0).abs() < 1e-9);
    }

    #[test]
    fn the_details_of_the_moves_parse() {
        assert_eq!(
            parse_column_move("column:Todo->Active"),
            Some(("Todo".to_string(), "Active".to_string()))
        );
        assert_eq!(
            parse_column_move("column:Active->Done reason:not -> needed"),
            Some(("Active".to_string(), "Done".to_string()))
        );
        let (a, b, c, d) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        assert_eq!(
            parse_board_move(&format!("board:{a}->{b} column:{c}->{d}")),
            Some((a, b, c, d))
        );
        assert_eq!(parse_column_move("board:x"), None);
    }

    #[test]
    fn percentiles_and_weeks() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&sorted, 50.0), Some(2.0));
        assert_eq!(percentile(&sorted, 85.0), Some(4.0));
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(week_start(date("2026-10-11")), date("2026-10-05"));
        assert_eq!(week_start(date("2026-10-05")), date("2026-10-05"));
    }
}
