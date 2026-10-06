//! The teams of an initiative or a strategy (KAIROS-T-0320,
//! COLLIERY-I-0602).
//!
//! The owner decided the rule on 2026-10-05. The teams of an item are the
//! teams that it gets FROM ITS TASKS, plus the teams that are SET ON IT BY
//! HAND:
//!
//! | Item | Team from tasks |
//! |---|---|
//! | initiative | the team of the board of each live task below it |
//! | strategy | the teams of the live initiatives below it (two levels) |
//!
//! A team set by hand stays when tasks come. An item with no team has no
//! team. Nothing in [`crate::abac`] reads this module: a team of an item
//! gives no right on the item.
//!
//! # The team set by hand
//!
//! It is a row of `item_impacts` with `target_kind = 'team'`: the link
//! `impacts` from an initiative or a strategy to a team. COLLIERY-I-0019
//! decision 3 names the link to an organizational unit `impacts`, and
//! [`crate::impacts`] kept `target_kind` in each row for this. This
//! amends decision 13 of COLLIERY-I-0019 ("an initiative has no `impacts`
//! link of its own"). The CHECK `item_impacts_kind_pair_check` keeps the
//! pairs apart: a document or an ADR impacts a repository, an initiative
//! or a strategy impacts a team. Each reader of the repository links
//! filters `target_kind = 'repository'`.
//!
//! Audit rows, events and duplicate links follow [`crate::impacts`]:
//! `details` is `relationship:impacts:{short_code}->team:{slug}`.
//!
//! # Liveness
//!
//! A team from tasks comes from LIVE work only, as the progress rollup of
//! [`crate::graph::children_progress`]: an archived task or initiative
//! gives no team. An archived team, and a task on an archived board, give
//! no team, from tasks or by hand. A link to an archived team stays in
//! the table, so a restore of the team needs no repair, and it can be
//! removed.
//!
//! Every public function operates in the CURRENT `search_path` tenant
//! schema.

use std::collections::HashMap;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Array, Bool, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;

use crate::events::{self, EventKind};
use crate::impacts::RELATIONSHIP;
use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;

/// The target kind of a team set by hand, in `item_impacts`.
pub const TARGET_TEAM: &str = "team";

/// Can an item of this type have teams? An initiative and a strategy can.
pub fn is_subject(item_type: ItemType) -> bool {
    matches!(item_type, ItemType::Initiative | ItemType::Strategy)
}

/// Errors from the services of this module.
#[derive(Debug, thiserror::Error)]
pub enum ItemTeamError {
    /// No live item has this id.
    #[error("The item {0} does not exist.")]
    ItemNotFound(Uuid),
    /// The item is not an initiative and not a strategy.
    #[error("{short_code} is a {item_type}. Only an initiative or a strategy can have a team.")]
    SubjectType {
        item_type: ItemType,
        short_code: String,
    },
    /// No team has this id (a link) or no live team has it (a new link).
    #[error("No live team has the id {0}.")]
    TeamNotFound(Uuid),
    /// The link is there already: nothing changed, and no activity row
    /// was written.
    #[error("The team {team:?} is set on {short_code} already.")]
    AlreadyLinked { short_code: String, team: String },
    /// No such link exists to remove.
    #[error("The team {team:?} is not set on {short_code}.")]
    NotLinked { short_code: String, team: String },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// One team of an item.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ItemTeam {
    /// The initiative or the strategy.
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub team_id: Uuid,
    #[diesel(sql_type = Text)]
    pub slug: String,
    #[diesel(sql_type = Text)]
    pub name: String,
    /// The item gets the team from its tasks.
    #[diesel(sql_type = Bool)]
    pub from_tasks: bool,
    /// The team is set on the item by hand.
    #[diesel(sql_type = Bool)]
    pub set_by_hand: bool,
}

/// The teams of the items that `subjects` gives, as ONE statement.
/// `subjects` is a SQL query with one column `id`: the initiatives and the
/// strategies to read. Each item gets each of its teams one time, by slug.
fn teams_sql(subjects: &str) -> String {
    format!(
        "WITH subjects AS ({subjects}), \
         from_tasks AS ( \
            SELECT r.source_id AS item_id, b.team_id \
              FROM item_relationships r \
              JOIN initiatives i ON i.id = r.source_id \
              JOIN tasks t ON t.id = r.target_id AND t.deleted_at IS NULL \
              JOIN boards b ON b.id = t.board_id AND b.deleted_at IS NULL \
             WHERE r.relationship = 'parent' \
               AND r.source_id IN (SELECT id FROM subjects) \
            UNION \
            SELECT r1.source_id, b.team_id \
              FROM item_relationships r1 \
              JOIN strategies s ON s.id = r1.source_id \
              JOIN initiatives i ON i.id = r1.target_id AND i.deleted_at IS NULL \
              JOIN item_relationships r2 \
                ON r2.source_id = i.id AND r2.relationship = 'parent' \
              JOIN tasks t ON t.id = r2.target_id AND t.deleted_at IS NULL \
              JOIN boards b ON b.id = t.board_id AND b.deleted_at IS NULL \
             WHERE r1.relationship = 'parent' \
               AND r1.source_id IN (SELECT id FROM subjects) \
         ), \
         all_teams AS ( \
            SELECT item_id, team_id, true AS from_tasks, false AS set_by_hand \
              FROM from_tasks WHERE team_id IS NOT NULL \
            UNION ALL \
            SELECT item_id, target_id, false, true \
              FROM item_impacts \
             WHERE target_kind = 'team' AND item_id IN (SELECT id FROM subjects) \
         ) \
         SELECT a.item_id, tm.id AS team_id, tm.slug, tm.name, \
                bool_or(a.from_tasks) AS from_tasks, \
                bool_or(a.set_by_hand) AS set_by_hand \
           FROM all_teams a \
           JOIN teams tm ON tm.id = a.team_id AND tm.deleted_at IS NULL \
          GROUP BY a.item_id, tm.id, tm.slug, tm.name \
          ORDER BY a.item_id, tm.slug"
    )
}

/// The teams of one item, by slug. An item that is not an initiative or a
/// strategy has no team.
pub fn teams_of(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ItemTeam>, DieselError> {
    Ok(teams_of_items(conn, &[item_id])?
        .remove(&item_id)
        .unwrap_or_default())
}

/// [`teams_of`] for MANY items, in one query. An item with no team has no
/// entry.
pub fn teams_of_items(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<ItemTeam>>, DieselError> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<ItemTeam> = sql_query(teams_sql("SELECT unnest($1::uuid[]) AS id"))
        .bind::<Array<SqlUuid>, _>(item_ids)
        .load(conn)?;
    Ok(group(rows))
}

/// The teams of each live initiative and strategy of one board, in ONE
/// query: what the cards and the team filter of a board need. An item with
/// no team has no entry.
pub fn board_teams(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<HashMap<Uuid, Vec<ItemTeam>>, DieselError> {
    let rows: Vec<ItemTeam> = sql_query(teams_sql(
        "SELECT id FROM initiatives WHERE board_id = $1 AND deleted_at IS NULL \
         UNION ALL SELECT id FROM strategies WHERE board_id = $1 AND deleted_at IS NULL",
    ))
    .bind::<SqlUuid, _>(board_id)
    .load(conn)?;
    Ok(group(rows))
}

fn group(rows: Vec<ItemTeam>) -> HashMap<Uuid, Vec<ItemTeam>> {
    let mut by_item: HashMap<Uuid, Vec<ItemTeam>> = HashMap::new();
    for row in rows {
        by_item.entry(row.item_id).or_default().push(row);
    }
    by_item
}

#[derive(QueryableByName)]
struct SubjectRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    short_code: String,
}

/// The type and the short code of a LIVE initiative or strategy.
fn live_subject(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<(ItemType, String), ItemTeamError> {
    let row: Option<SubjectRow> = sql_query(
        "SELECT entity_type, short_code FROM entity_directory \
          WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind::<SqlUuid, _>(item_id)
    .get_result(conn)
    .optional()?;
    let row = row.ok_or(ItemTeamError::ItemNotFound(item_id))?;
    let item_type = ItemType::ALL
        .iter()
        .copied()
        .find(|t| t.entity_type() == row.entity_type)
        .ok_or(ItemTeamError::ItemNotFound(item_id))?;
    if !is_subject(item_type) {
        return Err(ItemTeamError::SubjectType {
            item_type,
            short_code: row.short_code,
        });
    }
    Ok((item_type, row.short_code))
}

/// The slug of a team, archived or not.
fn team_slug(
    conn: &mut PgConnection,
    team_id: Uuid,
    live_only: bool,
) -> Result<Option<String>, DieselError> {
    use crate::schema::teams::dsl;
    let mut query = dsl::teams
        .filter(dsl::id.eq(team_id))
        .select(dsl::slug)
        .into_boxed();
    if live_only {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query.first(conn).optional()
}

fn log_team_activity(
    conn: &mut PgConnection,
    actor: Uuid,
    action: ActivityAction,
    (item_id, item_type, short_code): (Uuid, ItemType, &str),
    team: &str,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: actor,
            action,
            entity_id: Some(item_id),
            entity_type: Some(item_type.entity_type().to_string()),
            details: format!("relationship:{RELATIONSHIP}:{short_code}->{TARGET_TEAM}:{team}"),
        })
        .execute(conn)?;
    Ok(())
}

/// Set the team `team_id` on `item_id` by hand, in ONE transaction:
///
/// 1. the item is live, and it is an initiative or a strategy
///    ([`ItemTeamError::ItemNotFound`], [`ItemTeamError::SubjectType`]);
/// 2. the team is live ([`ItemTeamError::TeamNotFound`]);
/// 3. insert (a link that is there is [`ItemTeamError::AlreadyLinked`]),
///    write the `relationship_add` activity row, and emit
///    `relationship_changed` for the item.
///
/// A team that the item gets from its tasks can also be set by hand: the
/// two are different facts, and the hand-set one stays when the tasks go.
///
/// NO PERMISSION RULE, as the services of [`crate::graph`]: the caller
/// asks the edit rule of the item first.
pub fn link(
    conn: &mut PgConnection,
    item_id: Uuid,
    team_id: Uuid,
    actor: Uuid,
) -> Result<ItemTeam, ItemTeamError> {
    conn.transaction::<_, ItemTeamError, _>(|conn| {
        let (item_type, short_code) = live_subject(conn, item_id)?;
        let slug = team_slug(conn, team_id, true)?.ok_or(ItemTeamError::TeamNotFound(team_id))?;

        let inserted = sql_query(
            "INSERT INTO item_impacts (item_id, item_type, target_kind, target_id, created_by) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind::<SqlUuid, _>(item_id)
        .bind::<Text, _>(item_type.entity_type())
        .bind::<Text, _>(TARGET_TEAM)
        .bind::<SqlUuid, _>(team_id)
        .bind::<SqlUuid, _>(actor)
        .execute(conn);
        match inserted {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(ItemTeamError::AlreadyLinked {
                    short_code,
                    team: slug,
                });
            }
            other => {
                other?;
            }
        }

        log_team_activity(
            conn,
            actor,
            ActivityAction::RelationshipAdd,
            (item_id, item_type, &short_code),
            &slug,
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::RelationshipChanged,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        teams_of(conn, item_id)?
            .into_iter()
            .find(|team| team.team_id == team_id)
            .ok_or(ItemTeamError::Database(DieselError::NotFound))
    })
}

/// Remove the team `team_id` set by hand on `item_id`, in ONE transaction,
/// with the `relationship_remove` activity row and the
/// `relationship_changed` event. The item must be live. The team can be
/// archived. A link that is not there is [`ItemTeamError::NotLinked`]:
/// nothing changed, and no activity row was written. A team that the item
/// gets from its tasks stays: this removes the hand-set link only.
pub fn unlink(
    conn: &mut PgConnection,
    item_id: Uuid,
    team_id: Uuid,
    actor: Uuid,
) -> Result<(), ItemTeamError> {
    conn.transaction::<_, ItemTeamError, _>(|conn| {
        let (item_type, short_code) = live_subject(conn, item_id)?;
        let slug = team_slug(conn, team_id, false)?.ok_or(ItemTeamError::TeamNotFound(team_id))?;

        let deleted = sql_query(
            "DELETE FROM item_impacts \
              WHERE item_id = $1 AND target_kind = $2 AND target_id = $3",
        )
        .bind::<SqlUuid, _>(item_id)
        .bind::<Text, _>(TARGET_TEAM)
        .bind::<SqlUuid, _>(team_id)
        .execute(conn)?;
        if deleted == 0 {
            return Err(ItemTeamError::NotLinked {
                short_code,
                team: slug,
            });
        }

        log_team_activity(
            conn,
            actor,
            ActivityAction::RelationshipRemove,
            (item_id, item_type, &short_code),
            &slug,
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::RelationshipChanged,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        Ok(())
    })
}
