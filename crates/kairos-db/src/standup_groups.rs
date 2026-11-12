//! The groups of the AdHoc stand-up (KAIROS-T-0764, COLLIERY-I-0674,
//! KAIROS-A-0084).
//!
//! A group is a saved, named group of people of the organization. Its
//! name is unique in the organization, with no regard to case. Each member
//! of the organization can create a group, edit each group (its name and
//! its people) and read each group. Only the creator of a group can delete
//! it ([`delete`] refuses the others with [`DeleteOutcome::NotCreator`]).
//!
//! The stop of a person of a group is [`crate::my_work::person_work`]: the
//! work of the person by ownership on each board.
//!
//! The caller checks that each person is a person of the organization:
//! this module stores the ids that it gets. Each function operates in the
//! CURRENT `search_path` tenant schema.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Array, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

/// One group, with no people.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct StandupGroup {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub name: String,
    #[diesel(sql_type = SqlUuid)]
    pub created_by: Uuid,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Timestamptz)]
    pub updated_at: DateTime<Utc>,
}

/// A person of a group.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct GroupPerson {
    #[diesel(sql_type = SqlUuid)]
    pub group_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub user_id: Uuid,
    #[diesel(sql_type = Text)]
    pub user_name: String,
    #[diesel(sql_type = Text)]
    pub display_name: String,
}

/// Why a write of a group failed.
#[derive(Debug)]
pub enum GroupError {
    /// A different group has the name (no regard to case).
    NameTaken,
    /// No group has the id.
    NotFound,
    Db(DieselError),
}

impl From<DieselError> for GroupError {
    fn from(e: DieselError) -> Self {
        match e {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                GroupError::NameTaken
            }
            DieselError::NotFound => GroupError::NotFound,
            e => GroupError::Db(e),
        }
    }
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupError::NameTaken => f.write_str("a different group has the name"),
            GroupError::NotFound => f.write_str("no group has the id"),
            GroupError::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GroupError {}

/// The result of [`delete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    /// The caller did not create the group: nothing changed.
    NotCreator,
    NotFound,
}

const COLUMNS: &str = "id, name, created_by, created_at, updated_at";

/// Each group, by name.
pub fn list(conn: &mut PgConnection) -> Result<Vec<StandupGroup>, DieselError> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM standup_groups ORDER BY lower(name), id"
    ))
    .load(conn)
}

/// The group of `id`, when there is one.
pub fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<StandupGroup>, DieselError> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM standup_groups WHERE id = $1"
    ))
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
}

/// The people of the groups of `ids`, by group, each by display name. A
/// row of a user that is gone is not in it. One query.
pub fn people_of(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<GroupPerson>>, DieselError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<GroupPerson> = sql_query(
        "SELECT p.group_id, p.user_id, u.user_name, u.display_name \
           FROM standup_group_people p JOIN users u ON u.id = p.user_id \
          WHERE p.group_id = ANY($1) \
          ORDER BY u.display_name, u.id",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)?;
    let mut out: HashMap<Uuid, Vec<GroupPerson>> = HashMap::new();
    for row in rows {
        out.entry(row.group_id).or_default().push(row);
    }
    Ok(out)
}

/// Whether `user` is a person of the group `id`.
pub fn has_person(conn: &mut PgConnection, id: Uuid, user: Uuid) -> Result<bool, DieselError> {
    #[derive(QueryableByName)]
    struct Found {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        found: bool,
    }
    let row: Found = sql_query(
        "SELECT EXISTS (SELECT 1 FROM standup_group_people \
                         WHERE group_id = $1 AND user_id = $2) AS found",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(user)
    .get_result(conn)?;
    Ok(row.found)
}

/// Put the people of the group to `people`: remove the others, add the
/// new ones.
fn set_people(conn: &mut PgConnection, id: Uuid, people: &[Uuid]) -> Result<(), DieselError> {
    sql_query("DELETE FROM standup_group_people WHERE group_id = $1 AND NOT (user_id = ANY($2))")
        .bind::<SqlUuid, _>(id)
        .bind::<Array<SqlUuid>, _>(people)
        .execute(conn)?;
    sql_query(
        "INSERT INTO standup_group_people (group_id, user_id) \
         SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Array<SqlUuid>, _>(people)
    .execute(conn)?;
    Ok(())
}

/// Create a group of `people` with the name `name`, by `creator`.
pub fn create(
    conn: &mut PgConnection,
    name: &str,
    creator: Uuid,
    people: &[Uuid],
) -> Result<StandupGroup, GroupError> {
    conn.transaction(|conn| {
        let group: StandupGroup = sql_query(format!(
            "INSERT INTO standup_groups (name, created_by) VALUES ($1, $2) RETURNING {COLUMNS}"
        ))
        .bind::<Text, _>(name)
        .bind::<SqlUuid, _>(creator)
        .get_result(conn)?;
        set_people(conn, group.id, people)?;
        Ok(group)
    })
}

/// Change the name, the people, or both, of the group `id`. Each member
/// can edit each group. `None` keeps the value.
pub fn update(
    conn: &mut PgConnection,
    id: Uuid,
    name: Option<&str>,
    people: Option<&[Uuid]>,
) -> Result<StandupGroup, GroupError> {
    conn.transaction(|conn| {
        let group: StandupGroup = sql_query(format!(
            "UPDATE standup_groups SET name = COALESCE($2, name), updated_at = now() \
              WHERE id = $1 RETURNING {COLUMNS}"
        ))
        .bind::<SqlUuid, _>(id)
        .bind::<diesel::sql_types::Nullable<Text>, _>(name)
        .get_result(conn)?;
        if let Some(people) = people {
            set_people(conn, id, people)?;
        }
        Ok(group)
    })
}

/// Delete the group `id` and its people, when `caller` created it.
pub fn delete(
    conn: &mut PgConnection,
    id: Uuid,
    caller: Uuid,
) -> Result<DeleteOutcome, DieselError> {
    let Some(group) = get(conn, id)? else {
        return Ok(DeleteOutcome::NotFound);
    };
    if group.created_by != caller {
        return Ok(DeleteOutcome::NotCreator);
    }
    let removed = sql_query("DELETE FROM standup_groups WHERE id = $1 AND created_by = $2")
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(caller)
        .execute(conn)?;
    Ok(if removed > 0 {
        DeleteOutcome::Deleted
    } else {
        DeleteOutcome::NotFound
    })
}
