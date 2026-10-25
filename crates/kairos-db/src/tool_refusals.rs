//! The count of the MCP refusals (KAIROS-T-0694, KAIROS-A-0068).
//!
//! The server writes one row for each tool call that ends in a tool error
//! ([`record`]): the tool, the code, the `field` of the details, and the
//! kind of the caller. The row has no argument value and no person. The
//! report ([`counts`]) gives the count for each tool and code in a range of
//! time. The retention pass deletes the rows older than [`KEEP_DAYS`] days
//! ([`delete_before`]).

use chrono::{DateTime, Duration, Utc};
use diesel::QueryableByName;
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

/// The days that a row stays: the retention pass deletes the older ones.
pub const KEEP_DAYS: i64 = 90;

/// [`KEEP_DAYS`] as a duration.
pub fn keep() -> Duration {
    Duration::days(KEEP_DAYS)
}

/// The kinds of a caller, as the column `caller_kind` keeps them.
pub const CALLER_KINDS: [&str; 3] = ["person", "agent_key", "service_account"];

/// Write the row of one refusal. The kind of the caller comes from
/// `public.users.kind` and the agent key: `agent_key` when the call has
/// an agent key, `service_account` for a service account, else `person`.
/// The row keeps the kind, not the user.
pub fn record(
    conn: &mut PgConnection,
    tool: &str,
    code: &str,
    field: Option<&str>,
    user_id: Uuid,
    agent_key: Option<Uuid>,
) -> Result<usize, DieselError> {
    sql_query(
        "INSERT INTO tool_refusals (tool, code, field, caller_kind) \
         SELECT $1, $2, $3, CASE \
             WHEN $5::uuid IS NOT NULL THEN 'agent_key' \
             WHEN EXISTS (SELECT 1 FROM public.users u \
                          WHERE u.id = $4 AND u.kind = 'service_account') \
                 THEN 'service_account' \
             ELSE 'person' END",
    )
    .bind::<Text, _>(tool)
    .bind::<Text, _>(code)
    .bind::<Nullable<Text>, _>(field)
    .bind::<SqlUuid, _>(user_id)
    .bind::<Nullable<SqlUuid>, _>(agent_key)
    .execute(conn)
}

/// The count of one tool and code in a range.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Count {
    #[diesel(sql_type = Text)]
    pub tool: String,
    #[diesel(sql_type = Text)]
    pub code: String,
    #[diesel(sql_type = BigInt)]
    pub count: i64,
    /// The field that the refusals of this tool and code name most, or
    /// `None` when none names a field.
    #[diesel(sql_type = Nullable<Text>)]
    pub top_field: Option<String>,
    /// The count of each kind of caller, in the order of [`CALLER_KINDS`].
    #[diesel(sql_type = BigInt)]
    pub person: i64,
    #[diesel(sql_type = BigInt)]
    pub agent_key: i64,
    #[diesel(sql_type = BigInt)]
    pub service_account: i64,
}

/// The counts of the refusals from `since` (inclusive) to `until`
/// (exclusive), by tool and code, the highest first. Two rows with the
/// same count are in the order of tool and code.
pub fn counts(
    conn: &mut PgConnection,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Vec<Count>, DieselError> {
    sql_query(
        "SELECT r.tool, r.code, count(*) AS count, \
             (SELECT f.field FROM tool_refusals f \
              WHERE f.tool = r.tool AND f.code = r.code AND f.field IS NOT NULL \
                AND f.created_at >= $1 AND f.created_at < $2 \
              GROUP BY f.field ORDER BY count(*) DESC, f.field LIMIT 1) AS top_field, \
             count(*) FILTER (WHERE r.caller_kind = 'person') AS person, \
             count(*) FILTER (WHERE r.caller_kind = 'agent_key') AS agent_key, \
             count(*) FILTER (WHERE r.caller_kind = 'service_account') AS service_account \
         FROM tool_refusals r \
         WHERE r.created_at >= $1 AND r.created_at < $2 \
         GROUP BY r.tool, r.code \
         ORDER BY count DESC, r.tool, r.code",
    )
    .bind::<Timestamptz, _>(since)
    .bind::<Timestamptz, _>(until)
    .load(conn)
}

#[derive(QueryableByName)]
struct Total {
    #[diesel(sql_type = BigInt)]
    total: i64,
}

/// The count of all the refusals from `since` (inclusive) to `until`
/// (exclusive).
pub fn total(
    conn: &mut PgConnection,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<i64, DieselError> {
    let row: Total = sql_query(
        "SELECT count(*) AS total FROM tool_refusals WHERE created_at >= $1 AND created_at < $2",
    )
    .bind::<Timestamptz, _>(since)
    .bind::<Timestamptz, _>(until)
    .get_result(conn)?;
    Ok(row.total)
}

/// Delete at most `limit` rows made before `cutoff`. Returns the count.
pub fn delete_before(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<usize, DieselError> {
    sql_query(
        "DELETE FROM tool_refusals WHERE id IN ( \
             SELECT id FROM tool_refusals WHERE created_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}
