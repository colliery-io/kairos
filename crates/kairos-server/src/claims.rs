//! The claim of a task on the server (KAIROS-T-0359, KAIROS-A-0024). The
//! rules of a claim are in `kairos_db::task_claims`; this module has what
//! the two surfaces (REST and MCP) share:
//!
//! - the claim on the task DTO ([`attach_claim`], [`attach_claims`]) and
//!   in the MCP text ([`claim_text`]);
//! - who may hand off or release a claim ([`require_claim_change`]);
//! - the person of a hand-off ([`resolve_person`]);
//! - the WARNING for a write to a task that a different person has the
//!   claim on ([`note`]).
//!
//! # The warning
//!
//! A change to a claimed task by a person who does not have the claim is
//! allowed and recorded. The response says who has the claim:
//!
//! - REST: the header [`WARNING_HEADER`] (`Kairos-Warning`), one value
//!   for each claimed task of the write:
//!   `KAIROS-T-0001 claimed by Alice (agent) since 2026-10-09T10:00:00Z`.
//! - MCP: a line at the end of the text of the tool:
//!   `Warning: Alice (agent) has the claim on KAIROS-T-0001 (since
//!   2026-10-09T10:00:00Z). Your change is recorded.`
//!
//! A write by the holder of the claim (with or without the agent key of
//! the holder) gives no warning. A write by a service account to a claimed
//! task gives the warning too: the person who has the claim is not the
//! caller.
//!
//! The write paths call [`note`] on the blocking connection, in the
//! closure of `BlockingTenantPool::run`. The edit rule
//! (`api::require_item_edit`) and the link rule (`api::require_edge_write`)
//! call it for each task they let through, and the routes and tools that
//! move a task (transition, move, work class) call it themselves. So a new
//! write path that applies the edit rule gets the warning with no new
//! code.
//!
//! [`note`] puts the warning in a SINK of the request: a tokio task-local
//! ([`collect`]) that the tenant middleware and the MCP tool call open.
//! `BlockingTenantPool::run` reads the sink in the task of the request and
//! gives it to the blocking thread ([`with_blocking_sink`]), the pattern of
//! the agent key (`crate::blocking`). The middleware writes the header,
//! and the MCP tool call writes the line, only for a response that is a
//! success.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderName, HeaderValue};
use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_db::models::claims::TaskClaim;
use kairos_db::task_claims::{self, ClaimError};
use uuid::Uuid;

use crate::error::ApiError;

/// The response header of the warning (module docs). Lower case, as HTTP/2
/// sends it; clients match it with no regard to case.
pub const WARNING_HEADER: &str = "kairos-warning";

/// One warning: a task of the write that a different person has the claim
/// on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimWarning {
    pub short_code: String,
    pub holder: String,
    pub agent: bool,
    pub claimed_at: DateTime<Utc>,
}

impl ClaimWarning {
    fn holder_text(&self) -> String {
        holder_text(&self.holder, self.agent)
    }

    fn since(&self) -> String {
        self.claimed_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// The value of the header [`WARNING_HEADER`].
    pub fn header_text(&self) -> String {
        format!(
            "{} claimed by {} since {}",
            self.short_code,
            self.holder_text(),
            self.since()
        )
    }

    /// The line at the end of the text of an MCP tool.
    pub fn mcp_text(&self) -> String {
        format!(
            "Warning: {} has the claim on {} (since {}). Your change is recorded.",
            self.holder_text(),
            self.short_code,
            self.since()
        )
    }
}

/// "Alice (agent)" when the agent of the person made the claim, else
/// "Alice".
fn holder_text(name: &str, agent: bool) -> String {
    if agent {
        format!("{name} (agent)")
    } else {
        name.to_string()
    }
}

type Sink = Arc<Mutex<Vec<ClaimWarning>>>;

tokio::task_local! {
    /// The warnings of the request in scope (module docs).
    static SINK: Sink;
}

thread_local! {
    /// The sink of the request whose closure runs on this blocking thread.
    static BLOCKING_SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// Run `future` with a new sink, and return its output and the warnings
/// that the writes in it noted.
pub async fn collect<F: std::future::Future>(future: F) -> (F::Output, Vec<ClaimWarning>) {
    let sink: Sink = Arc::default();
    let output = SINK.scope(sink.clone(), future).await;
    let warnings = std::mem::take(&mut *sink.lock().unwrap_or_else(|e| e.into_inner()));
    (output, warnings)
}

/// The sink of the request in scope, or `None` outside of a request. Read
/// in the task of the request (`BlockingTenantPool::run`).
pub(crate) fn current_sink() -> Option<Sink> {
    SINK.try_with(Arc::clone).ok()
}

/// Run `f` on this (blocking) thread with `sink` as the sink of [`note`].
pub(crate) fn with_blocking_sink<R>(sink: Option<Sink>, f: impl FnOnce() -> R) -> R {
    let previous = BLOCKING_SINK.with(|cell| cell.replace(sink));
    let result = f();
    BLOCKING_SINK.with(|cell| cell.replace(previous));
    result
}

/// Note a write by `actor` to the task `task_id`: when a different person
/// has the claim on the task, the response gets a warning (module docs).
/// Nothing when the task has no claim, when `actor` has it, or outside of
/// a request.
pub fn note(conn: &mut PgConnection, task_id: Uuid, actor: Uuid) -> Result<(), ApiError> {
    let Some(sink) = BLOCKING_SINK.with(|cell| cell.borrow().clone()) else {
        return Ok(());
    };
    let Some(claim) = task_claims::claim_of(conn, task_id).map_err(ApiError::internal)? else {
        return Ok(());
    };
    if claim.user_id == actor {
        return Ok(());
    }
    let short_code: String = {
        use kairos_db::schema::tasks;
        tasks::table
            .filter(tasks::id.eq(task_id))
            .select(tasks::short_code)
            .first(conn)
            .map_err(ApiError::internal)?
    };
    let holder = task_claims::display_names(conn, &[claim.user_id])
        .map_err(ApiError::internal)?
        .remove(&claim.user_id)
        .unwrap_or_else(|| claim.user_id.to_string());
    let warning = ClaimWarning {
        short_code,
        holder,
        agent: claim.agent_key_id.is_some(),
        claimed_at: claim.claimed_at,
    };
    let mut warnings = sink.lock().unwrap_or_else(|e| e.into_inner());
    if !warnings.iter().any(|w| w.short_code == warning.short_code) {
        warnings.push(warning);
    }
    Ok(())
}

/// Add the header [`WARNING_HEADER`] to a response, one value for each
/// warning. A name that is not a correct header value gets its non-ASCII
/// characters escaped.
pub fn add_header(headers: &mut axum::http::HeaderMap, warnings: &[ClaimWarning]) {
    let name = HeaderName::from_static(WARNING_HEADER);
    for warning in warnings {
        let text = warning.header_text();
        let value = HeaderValue::from_str(&text)
            .or_else(|_| HeaderValue::from_str(&text.escape_default().to_string()));
        if let Ok(value) = value {
            headers.append(name.clone(), value);
        }
    }
}

/// The claim DTO of a claim.
fn claim_dto(claim: &TaskClaim, names: &HashMap<Uuid, String>) -> dto::TaskClaim {
    dto::TaskClaim {
        user_id: claim.user_id.to_string(),
        display_name: names
            .get(&claim.user_id)
            .cloned()
            .unwrap_or_else(|| claim.user_id.to_string()),
        agent: claim.agent_key_id.is_some(),
        claimed_at: claim.claimed_at.to_rfc3339(),
    }
}

/// Put the claims of the tasks on their DTOs. Two queries for the list.
pub fn attach_claims(
    conn: &mut PgConnection,
    tasks: &mut [dto::Task],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = tasks.iter().filter_map(|t| t.id.parse().ok()).collect();
    let claims = task_claims::claims_of(conn, &ids)?;
    if claims.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = claims.values().map(|c| c.user_id).collect();
    let names = task_claims::display_names(conn, &users)?;
    for task in tasks {
        let Ok(id) = task.id.parse::<Uuid>() else {
            continue;
        };
        task.claim = claims.get(&id).map(|claim| claim_dto(claim, &names));
    }
    Ok(())
}

/// Put the claim of the task on its DTO.
pub fn attach_claim(
    conn: &mut PgConnection,
    mut task: dto::Task,
) -> Result<dto::Task, diesel::result::Error> {
    attach_claims(conn, std::slice::from_mut(&mut task))?;
    Ok(task)
}

/// The claims of the tasks as the text of MCP: "Alice (agent)" by task.
pub fn claim_texts(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, (String, DateTime<Utc>)>, ApiError> {
    let claims = task_claims::claims_of(conn, task_ids).map_err(ApiError::internal)?;
    let users: Vec<Uuid> = claims.values().map(|c| c.user_id).collect();
    let names = task_claims::display_names(conn, &users).map_err(ApiError::internal)?;
    Ok(claims
        .into_iter()
        .map(|(task_id, claim)| {
            let name = names
                .get(&claim.user_id)
                .cloned()
                .unwrap_or_else(|| claim.user_id.to_string());
            (
                task_id,
                (
                    holder_text(&name, claim.agent_key_id.is_some()),
                    claim.claimed_at,
                ),
            )
        })
        .collect())
}

/// The line of `get_item` for the claim of a task:
/// `- claim: Alice (agent) since 2026-10-09T10:00:00Z`, or
/// `- claim: none` for a task in a claims column with no claim. Empty for
/// a task in a different column.
pub fn claim_text(
    conn: &mut PgConnection,
    task_id: Uuid,
    column_id: Option<Uuid>,
) -> Result<String, ApiError> {
    if let Some((holder, since)) = claim_texts(conn, &[task_id])?.remove(&task_id) {
        return Ok(format!(
            "- claim: {holder} since {}\n",
            since.format("%Y-%m-%dT%H:%M:%SZ")
        ));
    }
    let claims_column = match column_id {
        Some(column) => task_claims::column_claims(conn, column).map_err(ApiError::internal)?,
        None => false,
    };
    Ok(if claims_column {
        "- claim: none. The task is free: the next person who moves it to Active gets the claim.\n"
            .to_string()
    } else {
        String::new()
    })
}

/// WHO MAY HAND OFF OR RELEASE the claim of a task: the person who has
/// the claim, or a principal with `transition_items` on the board of the
/// task (an organization admin has it).
///
/// Why `transition_items`: the claim starts and ends with a move of the
/// task into and out of Active, and that capability is the right to move
/// the task. A principal who may move the task out of Active (and so end
/// the claim) may also end the claim and leave the task in Active, or give
/// it to a different person. The holder needs no capability: a person who
/// got the claim at a hand-off can give it back or release it.
pub fn require_claim_change(
    conn: &mut PgConnection,
    slug: &str,
    task_id: Uuid,
    board_id: Uuid,
    user: Uuid,
) -> Result<(), ApiError> {
    let holder = task_claims::claim_of(conn, task_id)
        .map_err(ApiError::internal)?
        .map(|claim| claim.user_id);
    if holder == Some(user) {
        return Ok(());
    }
    crate::api::require_capability(conn, slug, Some(board_id), user, "transition_items")
}

/// The person that `reference` names, for a hand-off: a user id, an email
/// or a user name of a member of the organization `slug`. A service
/// account is refused: only a person can have a claim.
pub fn resolve_person(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
) -> Result<Uuid, ApiError> {
    use kairos_db::schema::users;
    let reference = reference.trim();
    let found: Option<(Uuid, String)> = if let Ok(id) = reference.parse::<Uuid>() {
        users::table
            .filter(users::id.eq(id))
            .select((users::id, users::kind))
            .first(conn)
            .optional()
    } else if reference.contains('@') {
        users::table
            .filter(users::email.eq(reference))
            .select((users::id, users::kind))
            .first(conn)
            .optional()
    } else {
        users::table
            .filter(users::user_name.eq(reference))
            .select((users::id, users::kind))
            .first(conn)
            .optional()
    }
    .map_err(ApiError::internal)?;
    let member = match found {
        Some((id, kind)) => kairos_db::abac::is_org_member(conn, slug, id)
            .map_err(ApiError::internal)?
            .then_some((id, kind)),
        None => None,
    };
    let Some((id, kind)) = member else {
        return Err(ApiError::validation(format!(
            "No member of the organization has the id, the email or the user name {reference:?}. \
             Send a person of the organization."
        ))
        .with_details(serde_json::json!({ "argument": "to" })));
    };
    if kind != kairos_db::models::public::USER_KIND_HUMAN {
        return Err(ApiError::validation(format!(
            "{reference:?} is a service account. Only a person can have a claim. Send a person \
             of the organization."
        ))
        .with_details(serde_json::json!({ "argument": "to" })));
    }
    Ok(id)
}

/// [`ClaimError`] → HTTP.
pub fn map_claim_error(e: ClaimError) -> ApiError {
    match e {
        ClaimError::TaskNotFound(_) => ApiError::not_found(e.to_string()),
        ClaimError::NotInClaimsColumn { .. } => {
            ApiError::unprocessable("NOT_CLAIMABLE", e.to_string())
        }
        ClaimError::NoClaim { .. } => ApiError::unprocessable("NO_CLAIM", e.to_string()),
        ClaimError::AlreadyHolder { .. } => ApiError::conflict(e.to_string()),
        ClaimError::NotAPerson(_) => ApiError::validation(e.to_string()),
        ClaimError::Database(db) => ApiError::internal(db),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warning(agent: bool) -> ClaimWarning {
        ClaimWarning {
            short_code: "ACME-T-0001".to_string(),
            holder: "Alice".to_string(),
            agent,
            claimed_at: "2026-10-09T10:00:00Z".parse().expect("a time"),
        }
    }

    #[test]
    fn the_header_names_the_holder() {
        assert_eq!(
            warning(true).header_text(),
            "ACME-T-0001 claimed by Alice (agent) since 2026-10-09T10:00:00Z"
        );
        assert_eq!(
            warning(false).header_text(),
            "ACME-T-0001 claimed by Alice since 2026-10-09T10:00:00Z"
        );
    }

    #[test]
    fn the_mcp_line_names_the_holder() {
        assert_eq!(
            warning(true).mcp_text(),
            "Warning: Alice (agent) has the claim on ACME-T-0001 (since \
             2026-10-09T10:00:00Z). Your change is recorded."
        );
    }

    #[test]
    fn a_name_that_is_not_ascii_gets_a_header() {
        let mut headers = axum::http::HeaderMap::new();
        let mut w = warning(false);
        w.holder = "Zoë".to_string();
        add_header(&mut headers, &[w]);
        assert_eq!(headers.get_all(WARNING_HEADER).iter().count(), 1);
    }
}
