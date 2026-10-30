//! The watches of a person (KAIROS-T-0736, decision 8 of COLLIERY-I-0664).
//!
//! A person watches an item, with or without its children. An agent key
//! watches for its person: the watch is the person's. A coarse change of a
//! watched item makes one notification of the kind `watch` for each
//! watcher ([`on_watched`]):
//!
//! - moved: the item went to a column that is not done, or to a different
//!   board;
//! - edited: its title or its content changed;
//! - note added: a note was appended;
//! - done: it went into a done column (a cancel too).
//!
//! The watchers of an item are the persons who watch it, and the persons
//! who watch its parent with its children (one level of `parent` edges).
//! [`crate::notifications::notify`] applies the rules of each
//! notification: a person of the organization, never the actor, the kind
//! not turned off. The producer runs in the transaction of the change.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Bool, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use kairos_core::notifications::{NotificationKind, WatchEvent};
use uuid::Uuid;

/// One watch of a person.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Watch {
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    #[diesel(sql_type = Bool)]
    pub with_children: bool,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// One watch of a person, with the item now.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct WatchedItem {
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    #[diesel(sql_type = Text)]
    pub short_code: String,
    #[diesel(sql_type = Text)]
    pub entity_type: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    #[diesel(sql_type = Bool)]
    pub archived: bool,
    #[diesel(sql_type = Bool)]
    pub with_children: bool,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// Watch the item, or change `with_children` of a watch that is there.
/// Returns the watch.
pub fn watch(
    conn: &mut PgConnection,
    user: Uuid,
    item_id: Uuid,
    with_children: bool,
) -> Result<Watch, DieselError> {
    sql_query(
        "INSERT INTO item_watches (user_id, item_id, with_children) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET with_children = EXCLUDED.with_children \
         RETURNING item_id, with_children, created_at",
    )
    .bind::<SqlUuid, _>(user)
    .bind::<SqlUuid, _>(item_id)
    .bind::<Bool, _>(with_children)
    .get_result(conn)
}

/// Stop the watch of the person on the item. Returns whether there was one.
pub fn unwatch(conn: &mut PgConnection, user: Uuid, item_id: Uuid) -> Result<bool, DieselError> {
    let removed = sql_query("DELETE FROM item_watches WHERE user_id = $1 AND item_id = $2")
        .bind::<SqlUuid, _>(user)
        .bind::<SqlUuid, _>(item_id)
        .execute(conn)?;
    Ok(removed > 0)
}

/// The watch of the person on the item, when there is one.
pub fn watch_of(
    conn: &mut PgConnection,
    user: Uuid,
    item_id: Uuid,
) -> Result<Option<Watch>, DieselError> {
    sql_query(
        "SELECT item_id, with_children, created_at FROM item_watches \
          WHERE user_id = $1 AND item_id = $2",
    )
    .bind::<SqlUuid, _>(user)
    .bind::<SqlUuid, _>(item_id)
    .get_result(conn)
    .optional()
}

/// Each watch of the person, by short code. A watch of an item that is
/// gone (purged) is not in the list.
pub fn list(conn: &mut PgConnection, user: Uuid) -> Result<Vec<WatchedItem>, DieselError> {
    sql_query(
        "SELECT w.item_id, d.short_code, d.entity_type, d.title, \
                (d.deleted_at IS NOT NULL) AS archived, w.with_children, w.created_at \
           FROM item_watches w JOIN entity_directory d ON d.id = w.item_id \
          WHERE w.user_id = $1 \
          ORDER BY d.short_code",
    )
    .bind::<SqlUuid, _>(user)
    .load(conn)
}

#[derive(QueryableByName)]
struct Watcher {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
}

/// The watchers of the item: the persons who watch it, and the persons who
/// watch a parent of it with its children. A `parent` edge goes from the
/// parent (`source_id`) to the child (`target_id`).
pub fn watchers(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<Uuid>, DieselError> {
    let rows: Vec<Watcher> = sql_query(
        "SELECT DISTINCT w.user_id FROM item_watches w \
          WHERE w.item_id = $1 \
             OR (w.with_children AND w.item_id IN ( \
                   SELECT r.source_id FROM item_relationships r \
                    WHERE r.target_id = $1 AND r.relationship = 'parent')) \
          ORDER BY w.user_id",
    )
    .bind::<SqlUuid, _>(item_id)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.user_id).collect())
}

#[derive(QueryableByName)]
struct Head {
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Nullable<Text>)]
    actor: Option<String>,
}

/// `watch` (KAIROS-T-0736): `actor` made the change `event` to the item.
/// `place` is the column or the board after a move. Each watcher of the
/// item is told ([`watchers`]). One query when the item has no watcher.
pub fn on_watched(
    conn: &mut PgConnection,
    item_id: Uuid,
    event: WatchEvent,
    place: Option<&str>,
    actor: Uuid,
) -> Result<(), DieselError> {
    let told = watchers(conn, item_id)?;
    if told.iter().all(|user| *user == actor) {
        return Ok(());
    }
    let head: Option<Head> = sql_query(
        "SELECT d.short_code, d.title, \
                (SELECT NULLIF(btrim(u.display_name), '') FROM public.users u WHERE u.id = $2) \
                    AS actor \
           FROM entity_directory d WHERE d.id = $1",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<SqlUuid, _>(actor)
    .get_result(conn)
    .optional()?;
    let Some(head) = head else {
        return Ok(());
    };
    let actor_name = head.actor.unwrap_or_else(|| actor.to_string());
    let text = event.text(&actor_name, &head.short_code, &head.title, place);
    crate::notifications::notify(
        conn,
        &told,
        NotificationKind::Watch,
        Some((item_id, &head.short_code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// Delete the watches of an item (a purge of the item).
pub(crate) fn delete_for_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<usize, DieselError> {
    sql_query("DELETE FROM item_watches WHERE item_id = $1")
        .bind::<SqlUuid, _>(item_id)
        .execute(conn)
}
