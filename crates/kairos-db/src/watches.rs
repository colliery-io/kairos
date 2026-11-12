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
//!
//! WATCHERS THAT PEOPLE CAN SEE (KAIROS-T-0762, COLLIERY-I-0674 decisions
//! 1–6). A watch records who added it (`added_by`; the watcher for a
//! self-watch). Any member can add any other member as a watcher of any
//! item ([`add_watcher`]); the added person gets a `watch` notification
//! ([`WatchEvent::Added`]), so the setting of the kind `watch` applies.
//! Only the watcher or the person who added the watch can remove it
//! ([`remove_watcher`]). Each member can read the watchers of an item
//! ([`item_watchers`]).

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
    /// Who added the watch: the watcher for a self-watch (KAIROS-T-0762).
    #[diesel(sql_type = SqlUuid)]
    pub added_by: Uuid,
}

/// One watcher of an item, with the names (KAIROS-T-0762).
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ItemWatcher {
    #[diesel(sql_type = SqlUuid)]
    pub user_id: Uuid,
    #[diesel(sql_type = Text)]
    pub user_name: String,
    #[diesel(sql_type = Text)]
    pub display_name: String,
    #[diesel(sql_type = SqlUuid)]
    pub added_by: Uuid,
    #[diesel(sql_type = Nullable<Text>)]
    pub added_by_name: Option<String>,
    #[diesel(sql_type = Bool)]
    pub with_children: bool,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// The result of [`remove_watcher`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// The watch is gone.
    Removed,
    /// The person did not watch the item. Nothing changed.
    NotWatching,
    /// The caller is not the watcher and did not add the watch. Nothing
    /// changed.
    Refused { added_by: Uuid },
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

/// The person watches the item (a self-watch: `added_by` is the person),
/// or changes `with_children` of a watch that is there. A watch that
/// another person added keeps its `added_by`. Returns the watch.
pub fn watch(
    conn: &mut PgConnection,
    user: Uuid,
    item_id: Uuid,
    with_children: bool,
) -> Result<Watch, DieselError> {
    sql_query(
        "INSERT INTO item_watches (user_id, item_id, with_children, added_by) \
         VALUES ($1, $2, $3, $1) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET with_children = EXCLUDED.with_children \
         RETURNING item_id, with_children, created_at, added_by",
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
        "SELECT item_id, with_children, created_at, added_by FROM item_watches \
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

/// The result of [`add_watcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// The watch now.
    pub watch: Watch,
    /// `true` when this call made the watch.
    pub new: bool,
}

/// `by` adds `user` as a watcher of the item (KAIROS-T-0762). When `by` is
/// `user`, this is [`watch`]. Otherwise a new watch records `by` in
/// `added_by`, and `user` gets a `watch` notification
/// ([`WatchEvent::Added`]). A watch that is there is left as it is: its
/// `with_children` and its `added_by` do not change, and nobody is told.
/// The caller checks that `user` is a person of the organization.
pub fn add_watcher(
    conn: &mut PgConnection,
    user: Uuid,
    item_id: Uuid,
    with_children: bool,
    by: Uuid,
) -> Result<Added, DieselError> {
    if user == by {
        let before = watch_of(conn, user, item_id)?;
        let watch = watch(conn, user, item_id, with_children)?;
        return Ok(Added {
            watch,
            new: before.is_none(),
        });
    }
    let made: Option<Watch> = sql_query(
        "INSERT INTO item_watches (user_id, item_id, with_children, added_by) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, item_id) DO NOTHING \
         RETURNING item_id, with_children, created_at, added_by",
    )
    .bind::<SqlUuid, _>(user)
    .bind::<SqlUuid, _>(item_id)
    .bind::<Bool, _>(with_children)
    .bind::<SqlUuid, _>(by)
    .get_result(conn)
    .optional()?;
    let Some(watch) = made else {
        let watch = watch_of(conn, user, item_id)?.ok_or(DieselError::NotFound)?;
        return Ok(Added { watch, new: false });
    };
    if let Some(head) = head(conn, item_id, by)? {
        let actor_name = head.actor.unwrap_or_else(|| by.to_string());
        let text = WatchEvent::Added.text(&actor_name, &head.short_code, &head.title, None);
        crate::notifications::notify(
            conn,
            &[user],
            NotificationKind::Watch,
            Some((item_id, &head.short_code)),
            &text,
            by,
        )?;
    }
    Ok(Added { watch, new: true })
}

/// `by` removes the watch of `watcher` on the item (KAIROS-T-0762). Only
/// the watcher or the person who added the watch can remove it.
pub fn remove_watcher(
    conn: &mut PgConnection,
    watcher: Uuid,
    item_id: Uuid,
    by: Uuid,
) -> Result<Removal, DieselError> {
    let Some(watch) = watch_of(conn, watcher, item_id)? else {
        return Ok(Removal::NotWatching);
    };
    if by != watcher && by != watch.added_by {
        return Ok(Removal::Refused {
            added_by: watch.added_by,
        });
    }
    unwatch(conn, watcher, item_id)?;
    Ok(Removal::Removed)
}

/// The watchers of the item that watch it directly, oldest watch first,
/// with their names and the name of who added each (KAIROS-T-0762). A
/// person who watches a parent with its children is not in the list.
pub fn item_watchers(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<ItemWatcher>, DieselError> {
    sql_query(
        "SELECT w.user_id, u.user_name, u.display_name, w.added_by, \
                (SELECT COALESCE(NULLIF(btrim(a.display_name), ''), a.user_name) \
                   FROM public.users a WHERE a.id = w.added_by) AS added_by_name, \
                w.with_children, w.created_at \
           FROM item_watches w JOIN public.users u ON u.id = w.user_id \
          WHERE w.item_id = $1 \
          ORDER BY w.created_at, u.user_name",
    )
    .bind::<SqlUuid, _>(item_id)
    .load(conn)
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
    let Some(head) = head(conn, item_id, actor)? else {
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

/// The code and the title of the item, and the display name of `actor`.
fn head(conn: &mut PgConnection, item_id: Uuid, actor: Uuid) -> Result<Option<Head>, DieselError> {
    sql_query(
        "SELECT d.short_code, d.title, \
                (SELECT NULLIF(btrim(u.display_name), '') FROM public.users u WHERE u.id = $2) \
                    AS actor \
           FROM entity_directory d WHERE d.id = $1",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<SqlUuid, _>(actor)
    .get_result(conn)
    .optional()
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
