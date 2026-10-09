//! The mark of a change made by an agent (KAIROS-T-0359, KAIROS-A-0024
//! decision 3).
//!
//! A request made with the agent key of a person acts as the person, so
//! the actor of each row it writes is the person. The rows of
//! `activity_log`, `item_history` and `team_page_history` also record the
//! agent key (`agent_key_id`), so history and activity can show
//! "Alice (agent)".
//!
//! # One session setting, not a parameter on each write
//!
//! The writes are in many services. The id goes to all of them through
//! the session setting [`SETTING`] of the connection: the column default
//! of `agent_key_id` is `NULLIF(current_setting('kairos.agent_key', true),
//! '')::uuid`. No insert names the column.
//!
//! A pooled connection keeps its session settings from one checkout to
//! the next. So the code that checks a connection out for a request must
//! set the setting on EACH checkout: the id, or the empty string for no
//! agent key ([`set`]). The server does this in
//! `kairos_server::blocking::BlockingTenantPool::run`, in the same
//! statement as `search_path`. A connection that never set it (a
//! background job with its own connection, a migration, psql) reads NULL.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Uuid as SqlUuid};
use uuid::Uuid;

/// The name of the session setting.
pub const SETTING: &str = "kairos.agent_key";

/// The statement that sets [`SETTING`] for `agent_key`: the id, or the
/// empty string for no agent key. The value is a formatted UUID, so the
/// statement is safe to send as text.
pub fn set_statement(agent_key: Option<Uuid>) -> String {
    let value = agent_key.map(|id| id.to_string()).unwrap_or_default();
    format!("SET {SETTING} TO '{value}'")
}

/// Set [`SETTING`] on `conn` for the session (see the module docs).
pub fn set(conn: &mut PgConnection, agent_key: Option<Uuid>) -> QueryResult<()> {
    conn.batch_execute(&set_statement(agent_key))
}

#[derive(QueryableByName)]
struct CurrentRow {
    #[diesel(sql_type = Nullable<SqlUuid>)]
    agent_key: Option<Uuid>,
}

/// The agent key that the session setting of `conn` gives, or `None`. The
/// same expression as the column default.
pub fn current(conn: &mut PgConnection) -> QueryResult<Option<Uuid>> {
    let row: CurrentRow = sql_query(
        "SELECT NULLIF(current_setting('kairos.agent_key', true), '')::uuid AS agent_key",
    )
    .get_result(conn)?;
    Ok(row.agent_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statement_sets_the_id_or_the_empty_string() {
        let id = Uuid::nil();
        assert_eq!(
            set_statement(Some(id)),
            "SET kairos.agent_key TO '00000000-0000-0000-0000-000000000000'"
        );
        assert_eq!(set_statement(None), "SET kairos.agent_key TO ''");
    }
}
