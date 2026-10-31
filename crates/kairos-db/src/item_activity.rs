//! The activity of one item (KAIROS-T-0557): the rows of `activity_log`
//! about the item, newest first, each as one line of text with the names
//! of the people, the boards, the columns and the repositories.
//!
//! `get_history` (MCP) and `GET /api/{entity_type}/{short_code}/activity`
//! (REST, the CLI `<noun> history`) read it here, so the two say the same.
//!
//! # Which rows
//!
//! - Each row whose `entity_id` is the item: a move, a cancel, a board
//!   move, a claim, a hand-off, a release, a lane, a repository, a
//!   lifecycle, an owner board, a metadata value, a state write, a link to
//!   a URI, a repository or a team, a create, an archive, a restore and a
//!   rename. A kind that this module does not know is a line too
//!   ([`describe`]): a new kind shows, with its action and its details.
//! - Each link between two items that names the item. Its details name
//!   the two codes (`relationship:parent:ACME-I-0001->ACME-T-0002`). Since
//!   KAIROS-T-0638 the `entity_id` of the row is the source, so the target
//!   finds the row by its code; an older row has no `entity_id`. The match
//!   uses the current code and each retired code of the item.
//!
//! # Names
//!
//! A claim, a hand-off and a release name the people by id; a board move
//! names the boards and the columns by id; a repository change names the
//! repositories by id. [`of_item`] reads the names of one page with one
//! query for each kind of name. An id with no name stays the id.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use diesel::pg::{Pg, PgConnection};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_types::Bool;
use uuid::Uuid;

use crate::models::enums::ActivityAction;

/// One row of the activity of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemActivity {
    pub id: Uuid,
    pub action: ActivityAction,
    /// The details of the row, as they are stored.
    pub details: String,
    /// The row as text, with names ([`describe`]).
    pub text: String,
    pub actor_id: Uuid,
    /// The display name of the actor, or `None` for an unknown actor.
    pub actor_name: Option<String>,
    pub agent_key_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
}

/// One page of the activity of an item, newest first, and the count of
/// all its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemActivityPage {
    pub rows: Vec<ItemActivity>,
    pub total: i64,
}

/// The names that [`describe`] uses, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    pub users: HashMap<Uuid, String>,
    pub boards: HashMap<Uuid, String>,
    pub columns: HashMap<Uuid, String>,
    pub repositories: HashMap<Uuid, String>,
}

/// A raw row: id, action, details, actor, agent key, time.
type Raw = (
    Uuid,
    ActivityAction,
    String,
    Uuid,
    Option<Uuid>,
    DateTime<Utc>,
);

/// The `LIKE` patterns of the links between two items that name `code`:
/// as the source and as the target (module docs).
pub fn link_patterns(code: &str) -> [String; 2] {
    let code = escape_like(code);
    [
        format!("relationship:%:{code}->%"),
        format!("relationship:%->{code}"),
    ]
}

/// `text` with the `LIKE` wildcards escaped (the default escape `\`).
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The activity of the item `item_id`, whose code is `short_code`: the
/// page at `offset` of at most `limit` rows, newest first (module docs).
/// The caller clamps `limit` and `offset`.
pub fn of_item(
    conn: &mut PgConnection,
    item_id: Uuid,
    short_code: &str,
    limit: i64,
    offset: i64,
) -> Result<ItemActivityPage, DieselError> {
    use crate::schema::activity_log as log;

    let mut codes = vec![short_code.to_string()];
    codes.extend(crate::retired_codes::codes_of_item(conn, item_id)?);
    let patterns: Vec<String> = codes.iter().flat_map(|code| link_patterns(code)).collect();

    type Condition = Box<dyn BoxableExpression<log::table, Pg, SqlType = Bool>>;
    let links = || -> Condition {
        let mut condition: Condition = Box::new(log::details.like(patterns[0].clone()));
        for pattern in &patterns[1..] {
            condition = Box::new(condition.or(log::details.like(pattern.clone())));
        }
        // KAIROS-T-0638: a link row names its source as the entity. The
        // rows of the item hold the links where it is the source; this set
        // takes the rest: the rows with no entity (before KAIROS-T-0638),
        // and the rows of the other end.
        Box::new(
            log::entity_id
                .is_distinct_from(item_id)
                .and(log::action.eq_any(vec![
                    ActivityAction::RelationshipAdd,
                    ActivityAction::RelationshipRemove,
                ]))
                .and(condition),
        )
    };

    // The two sets are apart: the rows of the item use the index on
    // `entity_id`, the links match the text. Each set gives the first
    // `offset + limit` rows; the page is cut from the merge.
    let reach = offset.saturating_add(limit);
    let select = (
        log::id,
        log::action,
        log::details,
        log::actor_id,
        log::agent_key_id,
        log::occurred_at,
    );
    let own_total: i64 = log::table
        .filter(log::entity_id.eq(item_id))
        .count()
        .get_result(conn)?;
    let link_total: i64 = log::table.filter(links()).count().get_result(conn)?;
    let mut raws: Vec<Raw> = log::table
        .filter(log::entity_id.eq(item_id))
        .order((log::occurred_at.desc(), log::id.desc()))
        .limit(reach)
        .select(select)
        .load(conn)?;
    raws.extend(
        log::table
            .filter(links())
            .order((log::occurred_at.desc(), log::id.desc()))
            .limit(reach)
            .select(select)
            .load::<Raw>(conn)?,
    );
    raws.sort_by(|a, b| b.5.cmp(&a.5).then(b.0.cmp(&a.0)));
    let raws: Vec<Raw> = raws
        .into_iter()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(0))
        .collect();

    let names = names_of(conn, &raws)?;
    let rows = raws
        .into_iter()
        .map(
            |(id, action, details, actor_id, agent_key_id, occurred_at)| ItemActivity {
                id,
                text: describe(action, &details, &names),
                action,
                details,
                actor_id,
                actor_name: names.users.get(&actor_id).cloned(),
                agent_key_id,
                occurred_at,
            },
        )
        .collect();
    Ok(ItemActivityPage {
        rows,
        total: own_total.saturating_add(link_total),
    })
}

/// The names of the ids of one page: one query for each kind of name.
fn names_of(conn: &mut PgConnection, raws: &[Raw]) -> Result<Names, DieselError> {
    use crate::schema::{board_columns, boards, repositories, users};

    let mut ids = Ids::default();
    for (_, action, details, actor, _, _) in raws {
        ids.users.insert(*actor);
        ids.add(*action, details);
    }
    let mut names = Names::default();
    if !ids.users.is_empty() {
        names.users = users::table
            .filter(users::id.eq_any(&ids.users))
            .select((users::id, users::display_name))
            .load::<(Uuid, String)>(conn)?
            .into_iter()
            .collect();
    }
    if !ids.boards.is_empty() {
        names.boards = boards::table
            .filter(boards::id.eq_any(&ids.boards))
            .select((boards::id, boards::slug))
            .load::<(Uuid, String)>(conn)?
            .into_iter()
            .collect();
    }
    if !ids.columns.is_empty() {
        names.columns = board_columns::table
            .filter(board_columns::id.eq_any(&ids.columns))
            .select((board_columns::id, board_columns::name))
            .load::<(Uuid, String)>(conn)?
            .into_iter()
            .collect();
    }
    if !ids.repositories.is_empty() {
        names.repositories = repositories::table
            .filter(repositories::id.eq_any(&ids.repositories))
            .select((repositories::id, repositories::slug))
            .load::<(Uuid, String)>(conn)?
            .into_iter()
            .collect();
    }
    Ok(names)
}

/// The ids that the details of a page name, by kind.
#[derive(Debug, Default)]
struct Ids {
    users: BTreeSet<Uuid>,
    boards: BTreeSet<Uuid>,
    columns: BTreeSet<Uuid>,
    repositories: BTreeSet<Uuid>,
}

impl Ids {
    fn add(&mut self, action: ActivityAction, details: &str) {
        let uuids = |text: &str| -> Vec<Uuid> {
            text.split("->")
                .filter_map(|part| Uuid::parse_str(part.trim()).ok())
                .collect()
        };
        match action {
            ActivityAction::Claim => {
                self.users
                    .extend(details.strip_prefix("claim:").and_then(parse_uuid));
            }
            ActivityAction::HandOff => {
                if let Some(rest) = details.strip_prefix("hand_off:") {
                    self.users.extend(uuids(rest));
                }
            }
            ActivityAction::Release => {
                if let Some((user, _)) = release_parts(details) {
                    self.users.extend(parse_uuid(user));
                }
            }
            ActivityAction::BoardMove => {
                if let Some((boards, columns)) = board_move_parts(details) {
                    self.boards.extend(uuids(boards));
                    self.columns.extend(uuids(columns));
                }
            }
            ActivityAction::Repository => {
                if let Some(rest) = details.strip_prefix("repository:") {
                    self.repositories.extend(uuids(rest));
                }
            }
            _ => {}
        }
    }
}

fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text.trim()).ok()
}

/// `release:<user> reason:<reason>` as the user and the reason.
fn release_parts(details: &str) -> Option<(&str, &str)> {
    let rest = details.strip_prefix("release:")?;
    Some(match rest.split_once(" reason:") {
        Some((user, reason)) => (user, reason),
        None => (rest, ""),
    })
}

/// `board:<a>-><b> column:<c>-><d>` as the boards and the columns.
fn board_move_parts(details: &str) -> Option<(&str, &str)> {
    details.strip_prefix("board:")?.split_once(" column:")
}

/// `<from>-><to>` as the two sides, or `None`.
fn arrow(text: &str) -> Option<(&str, &str)> {
    text.split_once("->")
}

/// The name of `id` in `map`, or the text as it is.
fn name_in<'a>(map: &'a HashMap<Uuid, String>, text: &'a str) -> &'a str {
    parse_uuid(text)
        .and_then(|id| map.get(&id))
        .map_or(text, String::as_str)
}

/// `from -> to` with names from `map`.
fn named_change(map: &HashMap<Uuid, String>, text: &str) -> String {
    match arrow(text) {
        Some((from, to)) => format!("{} -> {}", name_in(map, from), name_in(map, to)),
        None => name_in(map, text).to_string(),
    }
}

/// `from -> to` as it is.
fn change(text: &str) -> String {
    match arrow(text) {
        Some((from, to)) => format!("{from} -> {to}"),
        None => text.to_string(),
    }
}

/// One activity row as text, with names. Pure. A kind that this function
/// does not know gives `<action>: <details>`, so a new kind of row shows
/// as it is.
pub fn describe(action: ActivityAction, details: &str, names: &Names) -> String {
    let known = match action {
        ActivityAction::Transition => details
            .strip_prefix("column:")
            .and_then(arrow)
            .map(|(from, to)| format!("moved from {from} to {to}")),
        ActivityAction::Cancel => details.strip_prefix("column:").map(|rest| {
            let (columns, reason) = rest.split_once(" reason:").unwrap_or((rest, ""));
            let moved = match arrow(columns) {
                Some((from, to)) => format!("cancelled: moved from {from} to {to}"),
                None => "cancelled".to_string(),
            };
            if reason.is_empty() {
                moved
            } else {
                format!("{moved}. Reason: {reason}")
            }
        }),
        ActivityAction::BoardMove => board_move_parts(details).map(|(boards, columns)| {
            format!(
                "moved to another board: {}, column {}",
                named_change(&names.boards, boards),
                named_change(&names.columns, columns)
            )
        }),
        ActivityAction::Claim => details
            .strip_prefix("claim:")
            .map(|user| format!("claim taken: {}", name_in(&names.users, user))),
        ActivityAction::HandOff => {
            details
                .strip_prefix("hand_off:")
                .and_then(arrow)
                .map(|(from, to)| {
                    let to = name_in(&names.users, to);
                    if from == "none" {
                        format!("claim handed off: to {to}")
                    } else {
                        format!("claim handed off: {} -> {to}", name_in(&names.users, from))
                    }
                })
        }
        ActivityAction::Release => release_parts(details).map(|(user, reason)| {
            let user = name_in(&names.users, user);
            match reason {
                "" | "release" => format!("claim released: {user}"),
                reason => format!("claim ended: {user} ({reason})"),
            }
        }),
        ActivityAction::WorkClass => details
            .strip_prefix("work_class:")
            .map(|rest| format!("lane: {}", change(rest))),
        ActivityAction::Lifecycle => details
            .strip_prefix("lifecycle:")
            .map(|rest| format!("lifecycle: {}", change(rest))),
        ActivityAction::Complexity => details
            .strip_prefix("complexity:")
            .map(|rest| format!("complexity: {}", change(rest))),
        ActivityAction::Repository => details
            .strip_prefix("repository:")
            .map(|rest| format!("repository: {}", named_change(&names.repositories, rest))),
        ActivityAction::RelationshipAdd | ActivityAction::RelationshipRemove => details
            .strip_prefix("relationship:")
            .and_then(|rest| rest.split_once(':'))
            .map(|(relationship, ends)| {
                let verb = if action == ActivityAction::RelationshipAdd {
                    "link added"
                } else {
                    "link removed"
                };
                let ends = match arrow(ends) {
                    Some((source, target)) => {
                        let target = target.strip_prefix("uri:").unwrap_or(target);
                        format!("{source} -> {target}")
                    }
                    None => ends.to_string(),
                };
                format!("{verb}: {relationship} {ends}")
            }),
        ActivityAction::Update => describe_update(details),
        ActivityAction::Create => details
            .strip_prefix("short_code:")
            .map(|code| format!("created as {code}")),
        ActivityAction::Delete => Some(match field(details, "cascade:") {
            Some(count) if count != "0" => format!("archived, with {count} descendants"),
            _ => "archived".to_string(),
        }),
        ActivityAction::Restore => Some("restored".to_string()),
        ActivityAction::Rename => details
            .strip_prefix("code:")
            .map(|rest| format!("renamed {}", change(rest))),
        _ => None,
    };
    known.unwrap_or_else(|| {
        if details.is_empty() {
            action.as_str().to_string()
        } else {
            format!("{}: {details}", action.as_str())
        }
    })
}

/// The value of `key` in details of the form `a:1 key:2 c:3`.
fn field<'a>(details: &'a str, key: &str) -> Option<&'a str> {
    details.split(' ').find_map(|part| part.strip_prefix(key))
}

/// An `update` row about an item: the owner board, a metadata value, the
/// state, or the triage of an agent.
fn describe_update(details: &str) -> Option<String> {
    if let Some(rest) = details.strip_prefix("owner_board:") {
        return Some(format!("owner board: {}", change(rest)));
    }
    if let Some(rest) = details.strip_prefix(METADATA_PREFIX) {
        let (slug, value) = rest.split_once('=').unwrap_or((rest, ""));
        return Some(if value.is_empty() {
            format!("metadata {slug}: cleared")
        } else {
            format!("metadata {slug}: {value}")
        });
    }
    if let Some(rest) = details.strip_prefix(STATE_PREFIX) {
        return Some(format!("state changed: {}", rest.replace(',', ", ")));
    }
    details
        .strip_prefix("agent_triage:")
        .map(|what| format!("agent triage: {what}"))
}

/// The prefix of the details of a metadata row: `metadata:<slug>=<value>`,
/// and an empty value for a cleared one.
pub const METADATA_PREFIX: &str = "metadata:";

/// The prefix of the details of a state row: `state:<field>,<field>`, the
/// fields that the write changed.
pub const STATE_PREFIX: &str = "state:";

/// The details of a metadata row.
pub fn metadata_details(slug: &str, value: Option<&str>) -> String {
    format!("{METADATA_PREFIX}{slug}={}", value.unwrap_or(""))
}

/// Write the `update` row of one changed metadata value of an item
/// (KAIROS-T-0557). The caller's transaction.
pub fn log_metadata(
    conn: &mut PgConnection,
    actor: Uuid,
    item_id: Uuid,
    entity_type: &str,
    slug: &str,
    value: Option<&str>,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(crate::models::NewActivityLogEntry {
            actor_id: actor,
            action: ActivityAction::Update,
            entity_id: Some(item_id),
            entity_type: Some(entity_type.to_string()),
            details: metadata_details(slug, value),
        })
        .execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> (Names, Uuid, Uuid, Uuid, Uuid) {
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let board = Uuid::from_u128(3);
        let column = Uuid::from_u128(4);
        let mut names = Names::default();
        names.users.insert(alice, "Alice".to_string());
        names.users.insert(bob, "Bob".to_string());
        names.boards.insert(board, "kairos".to_string());
        names.columns.insert(column, "Backlog".to_string());
        (names, alice, bob, board, column)
    }

    #[test]
    fn each_known_kind_reads_as_text_with_names() {
        let (names, alice, bob, board, column) = names();
        let cases = [
            (
                ActivityAction::Transition,
                "column:Todo->Active".to_string(),
                "moved from Todo to Active",
            ),
            (
                ActivityAction::Claim,
                format!("claim:{alice}"),
                "claim taken: Alice",
            ),
            (
                ActivityAction::HandOff,
                format!("hand_off:{alice}->{bob}"),
                "claim handed off: Alice -> Bob",
            ),
            (
                ActivityAction::HandOff,
                format!("hand_off:none->{bob}"),
                "claim handed off: to Bob",
            ),
            (
                ActivityAction::Release,
                format!("release:{bob} reason:release"),
                "claim released: Bob",
            ),
            (
                ActivityAction::Release,
                format!("release:{bob} reason:transition"),
                "claim ended: Bob (transition)",
            ),
            (
                ActivityAction::WorkClass,
                "work_class:planned->support".to_string(),
                "lane: planned -> support",
            ),
            (
                ActivityAction::Complexity,
                "complexity:none->l".to_string(),
                "complexity: none -> l",
            ),
            (
                ActivityAction::BoardMove,
                format!("board:{board}->{board} column:{column}->{column}"),
                "moved to another board: kairos -> kairos, column Backlog -> Backlog",
            ),
            (
                ActivityAction::Update,
                metadata_details("priority", Some("P1")),
                "metadata priority: P1",
            ),
            (
                ActivityAction::Update,
                metadata_details("priority", None),
                "metadata priority: cleared",
            ),
            (
                ActivityAction::Update,
                "state:branch,commit,pushed".to_string(),
                "state changed: branch, commit, pushed",
            ),
            (
                ActivityAction::RelationshipAdd,
                "relationship:blocks:ACME-T-0001->ACME-T-0002".to_string(),
                "link added: blocks ACME-T-0001 -> ACME-T-0002",
            ),
            (
                ActivityAction::RelationshipRemove,
                "relationship:external:ACME-T-0001->uri:https://example.com/a".to_string(),
                "link removed: external ACME-T-0001 -> https://example.com/a",
            ),
            (
                ActivityAction::Cancel,
                "column:Active->Completed reason:Not needed".to_string(),
                "cancelled: moved from Active to Completed. Reason: Not needed",
            ),
            (
                ActivityAction::Create,
                "short_code:ACME-T-0001".to_string(),
                "created as ACME-T-0001",
            ),
            (
                ActivityAction::Delete,
                "short_code:ACME-T-0001 cascade:2 descendants:A,B".to_string(),
                "archived, with 2 descendants",
            ),
            (
                ActivityAction::Rename,
                "code:ACME-T-0001->SKADI-T-0001".to_string(),
                "renamed ACME-T-0001 -> SKADI-T-0001",
            ),
        ];
        for (action, details, want) in cases {
            assert_eq!(describe(action, &details, &names), want, "{details}");
        }
    }

    #[test]
    fn an_unknown_id_stays_the_id_and_an_unknown_kind_shows_as_it_is() {
        let (names, ..) = names();
        let stranger = Uuid::from_u128(9);
        assert_eq!(
            describe(ActivityAction::Claim, &format!("claim:{stranger}"), &names),
            format!("claim taken: {stranger}")
        );
        assert_eq!(
            describe(ActivityAction::Update, "note:n-3", &names),
            "update: note:n-3"
        );
        assert_eq!(describe(ActivityAction::Purge, "", &names), "purge");
    }

    #[test]
    fn the_link_patterns_match_the_code_at_either_end_only() {
        assert_eq!(
            link_patterns("ACME-T-0001"),
            [
                "relationship:%:ACME-T-0001->%".to_string(),
                "relationship:%->ACME-T-0001".to_string()
            ]
        );
        assert_eq!(escape_like("A_B%C\\"), "A\\_B\\%C\\\\");
    }
}
