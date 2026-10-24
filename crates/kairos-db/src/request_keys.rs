//! The request keys of the MCP writes (KAIROS-T-0681, KAIROS-A-0065).
//!
//! A write tool that gets a `request_key` calls [`claim`] in the
//! transaction of the write, before the write. A new key inserts its row
//! and the tool runs; [`store_answer`] then keeps the text of the answer in
//! the row, in the same transaction. A key that has a row in the window
//! gives the row back, and the tool does not run. A second call while the
//! first runs waits on the unique index until the first ends: a committed
//! first call gives its row, a rolled-back one leaves the key free.
//!
//! The caller is the person and the agent key (`agent_key` NULL for a call
//! with no agent key). The table is in the schema of the tenant, so a key
//! does not cross tenants.

use chrono::{DateTime, Duration, Utc};
use diesel::OptionalExtension;
use diesel::QueryableByName;
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Bytea, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

/// The time a key holds its answer: 24 hours from the first call
/// (KAIROS-A-0065 decision 3).
pub const WINDOW_HOURS: i64 = 24;

/// The window as a duration.
pub fn window() -> Duration {
    Duration::hours(WINDOW_HOURS)
}

/// True for a key that the table takes: 8 to 128 characters of
/// `[A-Za-z0-9._:-]`. The CHECK of the table has the same rule.
pub fn is_valid_key(key: &str) -> bool {
    (8..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// The caller of a keyed call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub user_id: Uuid,
    pub agent_key: Option<Uuid>,
}

/// A row of an earlier call with the same caller and key.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Earlier {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub tool: String,
    #[diesel(sql_type = Bytea)]
    pub fingerprint: Vec<u8>,
    #[diesel(sql_type = Nullable<Text>)]
    pub answer: Option<String>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// What [`claim`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The key is new (or its old row was past the window): the row with
    /// this id is written, and the tool runs.
    New(Uuid),
    /// A call in the window used the key: its row.
    Earlier(Earlier),
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// Take the key for a call, in the transaction of the write. `now` is the
/// clock of the window: a row older than [`WINDOW_HOURS`] is deleted and
/// the key is taken again.
pub fn claim(
    conn: &mut PgConnection,
    caller: Caller,
    request_key: &str,
    tool: &str,
    fingerprint: &[u8],
    now: DateTime<Utc>,
) -> Result<Claim, DieselError> {
    // Two tries at most: the second after the delete of a row past the
    // window.
    for _ in 0..2 {
        let inserted = sql_query(
            "INSERT INTO request_keys (user_id, agent_key, request_key, tool, fingerprint) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind::<SqlUuid, _>(caller.user_id)
        .bind::<Nullable<SqlUuid>, _>(caller.agent_key)
        .bind::<Text, _>(request_key)
        .bind::<Text, _>(tool)
        .bind::<Bytea, _>(fingerprint)
        .get_result::<IdRow>(conn)
        .optional()?;
        if let Some(row) = inserted {
            return Ok(Claim::New(row.id));
        }
        let earlier = sql_query(
            "SELECT id, tool, fingerprint, answer, created_at FROM request_keys \
             WHERE user_id = $1 AND agent_key IS NOT DISTINCT FROM $2 AND request_key = $3",
        )
        .bind::<SqlUuid, _>(caller.user_id)
        .bind::<Nullable<SqlUuid>, _>(caller.agent_key)
        .bind::<Text, _>(request_key)
        .get_result::<Earlier>(conn)
        .optional()?;
        match earlier {
            Some(earlier) if earlier.created_at > now - window() => {
                return Ok(Claim::Earlier(earlier));
            }
            Some(old) => {
                sql_query("DELETE FROM request_keys WHERE id = $1")
                    .bind::<SqlUuid, _>(old.id)
                    .execute(conn)?;
            }
            // The row went between the two statements (the retention pass):
            // try the insert again.
            None => {}
        }
    }
    Err(DieselError::RollbackTransaction)
}

/// Keep the text of the answer in the row of a new key.
pub fn store_answer(conn: &mut PgConnection, id: Uuid, answer: &str) -> Result<(), DieselError> {
    sql_query("UPDATE request_keys SET answer = $2 WHERE id = $1")
        .bind::<SqlUuid, _>(id)
        .bind::<Text, _>(answer)
        .execute(conn)?;
    Ok(())
}

/// Delete at most `limit` rows made before `cutoff` (the retention pass).
pub fn delete_before(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<usize, DieselError> {
    sql_query(
        "DELETE FROM request_keys WHERE id IN ( \
             SELECT id FROM request_keys WHERE created_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_8_to_128_safe_characters() {
        assert!(is_valid_key("0b6f3c1e-1f4a-4c55-9f0e-6f3e2d1c0b9a"));
        assert!(is_valid_key("retry:create.1_A"));
        assert!(is_valid_key(&"a".repeat(128)));
        assert!(!is_valid_key("short"));
        assert!(!is_valid_key(&"a".repeat(129)));
        assert!(!is_valid_key("has a space"));
        assert!(!is_valid_key("slash/key1"));
        assert!(!is_valid_key("ключ-ключ-ключ"));
    }
}
