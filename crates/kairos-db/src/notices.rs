//! Notices (KAIROS-T-0737, COLLIERY-I-0664 decision 9, KAIROS-A-0078).
//!
//! A person, or an agent key of a person, sends a notice: a text and an
//! optional item link, to a person or to each member of a team. Each
//! recipient gets one notification of the kind `notice` in the bell, with
//! the push of [`crate::notifications::notify`].
//!
//! - A notice from an agent key names the key and its person: the text
//!   says so, and the notification row has the key
//!   (`notifications.agent_key_id`).
//! - A rate limit: each key can send `limit` notices in one hour. A person
//!   with no key counts by the person. A rotated key keeps the count of
//!   the keys that it replaced. A notice to a team counts as one. The
//!   count and the insert run under one advisory lock of the sender, so two
//!   requests at the same time cannot both pass the last free place.
//! - A recipient can mute an agent key (`notice_mutes`). A notice from a
//!   muted key, or from a key that replaced a muted key, makes no
//!   notification for that person. A mute applies to notices only.
//! - The person of the key can get a notice from the key: an agent can
//!   tell its own person. A person with no key is not told of their own
//!   notice (the rule of `notify`).

use chrono::{DateTime, Duration, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use kairos_core::notifications::NotificationKind;
use uuid::Uuid;

use crate::notifications::{FromKey, name_of, notify_from};

/// The window of the rate limit.
pub const WINDOW: Duration = Duration::hours(1);

/// Who sends a notice: the person, and the agent key of the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub user: Uuid,
    pub key: Option<FromKey>,
}

/// The target of a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One person of the organization.
    Person(Uuid),
    /// Each member of a live team.
    Team(Uuid),
}

/// A notice that was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub id: Uuid,
    /// The persons who got a notification. A recipient who turned the kind
    /// off, or muted the key, is not in it.
    pub told: Vec<Uuid>,
    /// The notices that the sender can still send in the window. `None`:
    /// no limit.
    pub remaining: Option<i64>,
}

/// Why [`send`] sent nothing.
#[derive(Debug)]
pub enum SendError {
    /// The sender sent `limit` notices in the last hour. The next one can
    /// go after `retry_after` seconds.
    RateLimited {
        limit: u32,
        retry_after: i64,
    },
    Database(DieselError),
}

impl From<DieselError> for SendError {
    fn from(e: DieselError) -> Self {
        SendError::Database(e)
    }
}

#[derive(QueryableByName)]
struct Window {
    #[diesel(sql_type = BigInt)]
    total: i64,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    oldest: Option<DateTime<Utc>>,
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// The notices of the sender in the window that ends at `now`, and the
/// time of the oldest one. A key counts with the keys that it replaced; a
/// person with no key counts the notices sent with no key.
fn window(
    conn: &mut PgConnection,
    sender: &Sender,
    now: DateTime<Utc>,
) -> Result<Window, DieselError> {
    let chain: Vec<Uuid> = sender
        .key
        .as_ref()
        .map(|k| k.chain.clone())
        .unwrap_or_default();
    sql_query(
        "SELECT count(*) AS total, min(created_at) AS oldest FROM notices \
          WHERE created_at > $3 AND created_at <= $4 \
            AND (CASE WHEN cardinality($2) > 0 THEN agent_key_id = ANY($2) \
                      ELSE sender_id = $1 AND agent_key_id IS NULL END)",
    )
    .bind::<SqlUuid, _>(sender.user)
    .bind::<Array<SqlUuid>, _>(&chain)
    .bind::<Timestamptz, _>(now - WINDOW)
    .bind::<Timestamptz, _>(now)
    .get_result(conn)
}

/// The text of the notification: who sent it, to whom, and the text.
fn notification_text(
    conn: &mut PgConnection,
    sender: &Sender,
    team: Option<&str>,
    text: &str,
) -> Result<String, DieselError> {
    use crate::schema::api_keys;
    let person = name_of(conn, sender.user)?;
    let from = match &sender.key {
        Some(key) => {
            let name: String = api_keys::table
                .find(key.id)
                .select(api_keys::name)
                .first(conn)
                .optional()?
                .unwrap_or_else(|| key.id.to_string());
            format!("The agent key {name} of {person}")
        }
        None => person,
    };
    Ok(match team {
        Some(team) => format!("{from} sent a notice to the team {team}: {}", text.trim()),
        None => format!("{from} sent you a notice: {}", text.trim()),
    })
}

/// Send a notice at `now`, in one transaction. `limit` is the most notices
/// of the sender in one hour; 0 is no limit. `item` is the id and the code
/// of a live item. Refused with [`SendError::RateLimited`] when the sender
/// is at the limit: nothing is written.
pub fn send(
    conn: &mut PgConnection,
    sender: &Sender,
    target: Target,
    item: Option<(Uuid, &str)>,
    text: &str,
    limit: u32,
    now: DateTime<Utc>,
) -> Result<Sent, SendError> {
    conn.transaction(|conn| {
        // One sender at a time: the count and the insert see the same rows.
        let lock = match &sender.key {
            Some(key) => format!("notice:key:{}", key.chain.iter().min().unwrap_or(&key.id)),
            None => format!("notice:user:{}", sender.user),
        };
        sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind::<Text, _>(&lock)
            .execute(conn)?;
        let seen = window(conn, sender, now)?;
        if limit > 0 && seen.total >= i64::from(limit) {
            let next = seen.oldest.map_or(now, |oldest| oldest + WINDOW);
            return Err(SendError::RateLimited {
                limit,
                retry_after: (next - now).num_seconds().max(1),
            });
        }
        let (recipients, team_name, to_user, to_team) = match target {
            Target::Person(user) => (vec![user], None, Some(user), None),
            Target::Team(team) => {
                use crate::schema::{team_members, teams};
                let name: String = teams::table.find(team).select(teams::name).first(conn)?;
                let members: Vec<Uuid> = team_members::table
                    .filter(team_members::team_id.eq(team))
                    .select(team_members::user_id)
                    .load(conn)?;
                (members, Some(name), None, Some(team))
            }
        };
        let row: IdRow = sql_query(
            "INSERT INTO notices (sender_id, agent_key_id, to_user_id, to_team_id, item_id, \
                                  text, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind::<SqlUuid, _>(sender.user)
        .bind::<Nullable<SqlUuid>, _>(sender.key.as_ref().map(|k| k.id))
        .bind::<Nullable<SqlUuid>, _>(to_user)
        .bind::<Nullable<SqlUuid>, _>(to_team)
        .bind::<Nullable<SqlUuid>, _>(item.map(|(id, _)| id))
        .bind::<Text, _>(text.trim())
        .bind::<Timestamptz, _>(now)
        .get_result(conn)?;
        let message = notification_text(conn, sender, team_name.as_deref(), text)?;
        let told = notify_from(
            conn,
            &recipients,
            NotificationKind::Notice,
            item,
            &message,
            sender.user,
            sender.key.as_ref(),
        )?;
        sql_query("UPDATE notices SET told = $2 WHERE id = $1")
            .bind::<SqlUuid, _>(row.id)
            .bind::<Integer, _>(i32::try_from(told.len()).unwrap_or(i32::MAX))
            .execute(conn)?;
        let remaining = (limit > 0).then(|| i64::from(limit) - seen.total - 1);
        Ok(Sent {
            id: row.id,
            told,
            remaining,
        })
    })
}

/// An agent key that a person muted, with its name and its person.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct MutedKey {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub name: String,
    #[diesel(sql_type = Text)]
    pub prefix: String,
    #[diesel(sql_type = SqlUuid)]
    pub owner_id: Uuid,
    #[diesel(sql_type = Text)]
    pub owner_name: String,
}

/// The agent keys that `user` muted, by the name of the key.
pub fn muted_keys(conn: &mut PgConnection, user: Uuid) -> Result<Vec<MutedKey>, DieselError> {
    sql_query(
        "SELECT k.id, k.name, k.prefix, k.user_id AS owner_id, \
                COALESCE(NULLIF(btrim(u.display_name), ''), u.email) AS owner_name \
           FROM notice_mutes nm \
           JOIN api_keys k ON k.id = nm.api_key_id \
           JOIN public.users u ON u.id = k.user_id \
          WHERE nm.user_id = $1 \
          ORDER BY k.name, k.id",
    )
    .bind::<SqlUuid, _>(user)
    .load(conn)
}

/// The ids of `ids` that are not an agent key: a key of a person, not of a
/// service account. Revoked keys are agent keys too.
pub fn not_agent_keys(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<Uuid>, DieselError> {
    let found: Vec<IdRow> = sql_query(
        "SELECT k.id FROM api_keys k JOIN public.users u ON u.id = k.user_id \
          WHERE k.id = ANY($1) AND u.kind = 'human'",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)?;
    Ok(ids
        .iter()
        .filter(|id| !found.iter().any(|row| row.id == **id))
        .copied()
        .collect())
}

/// Mute and unmute agent keys for `user`. A key muted already, or not
/// muted, does not change.
pub fn set_mutes(
    conn: &mut PgConnection,
    user: Uuid,
    mute: &[Uuid],
    unmute: &[Uuid],
) -> Result<(), DieselError> {
    conn.transaction(|conn| {
        if !mute.is_empty() {
            sql_query(
                "INSERT INTO notice_mutes (user_id, api_key_id) \
                 SELECT $1, k FROM unnest($2::uuid[]) AS k ON CONFLICT DO NOTHING",
            )
            .bind::<SqlUuid, _>(user)
            .bind::<Array<SqlUuid>, _>(mute)
            .execute(conn)?;
        }
        if !unmute.is_empty() {
            sql_query("DELETE FROM notice_mutes WHERE user_id = $1 AND api_key_id = ANY($2)")
                .bind::<SqlUuid, _>(user)
                .bind::<Array<SqlUuid>, _>(unmute)
                .execute(conn)?;
        }
        Ok(())
    })
}
