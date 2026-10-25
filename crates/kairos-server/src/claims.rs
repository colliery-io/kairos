//! The claim of a task on the server (KAIROS-T-0359, KAIROS-A-0024). The
//! rules of a claim are in `kairos_db::task_claims`; this module has what
//! the two surfaces (REST and MCP) share:
//!
//! - the claim on the task DTO ([`attach_claim`], [`attach_claims`]) and
//!   in the MCP text ([`claim_text`]);
//! - who may hand off or release a claim ([`require_claim_change`]);
//! - the person of a hand-off ([`resolve_person`]), who must hold
//!   `transition_items` on the board of the task ([`require_claim_target`],
//!   [`hand_off_targets`], KAIROS-T-0530);
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
//! # The warning of a WIP limit (KAIROS-T-0371)
//!
//! A move of an item into a column that is then over its WIP limit is
//! allowed, and the answer has a warning in the same places ([`note_wip`]):
//!
//! - REST: `Kairos-Warning: KAIROS-T-0001 moved into Active, over its WIP
//!   limit (4 of 3)`.
//! - MCP: `Warning: the column Active has 4 items, over its WIP limit of 3.
//!   Your move of KAIROS-T-0001 is recorded.`
//!
//! # The warning of open blockers (KAIROS-T-0565)
//!
//! A move of a task with open blockers into a claims column or a done
//! column is allowed while the board policy `blockers_gate` is off
//! (`kairos_db::board_policy`). The answer has a warning in the same
//! places ([`note_blockers`]):
//!
//! - REST: `Kairos-Warning: KAIROS-T-0001 moved into Active, blocked by
//!   KAIROS-T-0012 (open)`.
//! - MCP: `Warning: KAIROS-T-0001 is blocked by KAIROS-T-0012 (open). Your
//!   move into Active is recorded.`
//!
//! A cancelled blocker is open (KAIROS-A-0038). The text names it
//! `(cancelled)`.
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

/// A move into a column that is over its WIP limit (KAIROS-T-0371).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipWarning {
    pub short_code: String,
    pub column: String,
    pub count: i64,
    pub limit: i32,
}

impl WipWarning {
    /// The value of the header [`WARNING_HEADER`].
    pub fn header_text(&self) -> String {
        format!(
            "{} moved into {}, over its WIP limit ({} of {})",
            self.short_code, self.column, self.count, self.limit
        )
    }

    /// The line at the end of the text of an MCP tool.
    pub fn mcp_text(&self) -> String {
        format!(
            "Warning: the column {} has {} items, over its WIP limit of {}. Your move of {} is \
             recorded.",
            self.column, self.count, self.limit, self.short_code
        )
    }
}

/// A move of a task with open blockers into a claims column or a done
/// column (KAIROS-T-0565).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerWarning {
    pub short_code: String,
    pub column: String,
    pub blockers: Vec<kairos_db::board_policy::OpenBlocker>,
}

impl BlockerWarning {
    fn blockers_text(&self) -> String {
        kairos_db::board_policy::blockers_text(&self.blockers)
    }

    /// The value of the header [`WARNING_HEADER`].
    pub fn header_text(&self) -> String {
        format!(
            "{} moved into {}, blocked by {}",
            self.short_code,
            self.column,
            self.blockers_text()
        )
    }

    /// The line at the end of the text of an MCP tool.
    pub fn mcp_text(&self) -> String {
        format!(
            "Warning: {} is blocked by {}. Your move into {} is recorded.",
            self.short_code,
            self.blockers_text(),
            self.column
        )
    }
}

/// One warning of a request: a claim, a WIP limit, or open blockers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    Claim(ClaimWarning),
    Wip(WipWarning),
    Blockers(BlockerWarning),
}

impl Warning {
    /// The value of the header [`WARNING_HEADER`].
    pub fn header_text(&self) -> String {
        match self {
            Self::Claim(w) => w.header_text(),
            Self::Wip(w) => w.header_text(),
            Self::Blockers(w) => w.header_text(),
        }
    }

    /// The line at the end of the text of an MCP tool.
    pub fn mcp_text(&self) -> String {
        match self {
            Self::Claim(w) => w.mcp_text(),
            Self::Wip(w) => w.mcp_text(),
            Self::Blockers(w) => w.mcp_text(),
        }
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

type Sink = Arc<Mutex<Vec<Warning>>>;

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
pub async fn collect<F: std::future::Future>(future: F) -> (F::Output, Vec<Warning>) {
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
    if !warnings
        .iter()
        .any(|w| matches!(w, Warning::Claim(c) if c.short_code == warning.short_code))
    {
        warnings.push(Warning::Claim(warning));
    }
    Ok(())
}

/// Note a move of the item `short_code` into the column `column_id`
/// (KAIROS-T-0371): when the column has a WIP limit and holds more live
/// items than the limit after the move, the response gets a warning
/// (module docs). Nothing outside of a request.
pub fn note_wip(
    conn: &mut PgConnection,
    column_id: Uuid,
    short_code: &str,
) -> Result<(), ApiError> {
    let Some(sink) = BLOCKING_SINK.with(|cell| cell.borrow().clone()) else {
        return Ok(());
    };
    let Some(load) =
        kairos_db::board_items::column_load(conn, column_id).map_err(ApiError::internal)?
    else {
        return Ok(());
    };
    let (Some(limit), true) = (load.wip_limit, load.over_limit()) else {
        return Ok(());
    };
    sink.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Warning::Wip(WipWarning {
            short_code: short_code.to_string(),
            column: load.name,
            count: load.count,
            limit,
        }));
    Ok(())
}

/// Note a move of the task `task_id` (code `short_code`) into the column
/// `column_id` (KAIROS-T-0565): when the column has the `claims` flag or
/// the `is_done` flag and the task has open blockers, the response gets a
/// warning (module docs). Call it after the move: with the board policy
/// `blockers_gate` on, the move was refused. Nothing outside of a request.
pub fn note_blockers(
    conn: &mut PgConnection,
    task_id: Uuid,
    column_id: Uuid,
    short_code: &str,
) -> Result<(), ApiError> {
    let Some(sink) = BLOCKING_SINK.with(|cell| cell.borrow().clone()) else {
        return Ok(());
    };
    let blockers = kairos_db::board_policy::blockers_of_move(conn, task_id, column_id)
        .map_err(ApiError::internal)?;
    if blockers.is_empty() {
        return Ok(());
    }
    let column: String = {
        use kairos_db::schema::board_columns;
        board_columns::table
            .filter(board_columns::id.eq(column_id))
            .select(board_columns::name)
            .first(conn)
            .map_err(ApiError::internal)?
    };
    sink.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Warning::Blockers(BlockerWarning {
            short_code: short_code.to_string(),
            column,
            blockers,
        }));
    Ok(())
}

/// Note a move of the task `task_id` into the column `column_id`: the WIP
/// limit ([`note_wip`]) and the open blockers ([`note_blockers`]). Each
/// path that moves a task calls it after the move: a transition, a reopen
/// and a board move.
pub fn note_task_move(
    conn: &mut PgConnection,
    task_id: Uuid,
    column_id: Uuid,
    short_code: &str,
) -> Result<(), ApiError> {
    note_wip(conn, column_id, short_code)?;
    note_blockers(conn, task_id, column_id, short_code)
}

/// Add the header [`WARNING_HEADER`] to a response, one value for each
/// warning. A name that is not a correct header value gets its non-ASCII
/// characters escaped.
pub fn add_header(headers: &mut axum::http::HeaderMap, warnings: &[Warning]) {
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
pub(crate) fn claim_dto(claim: &TaskClaim, names: &HashMap<Uuid, String>) -> dto::TaskClaim {
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
pub fn attach_claims<T: crate::api::convert::TaskDto>(
    conn: &mut PgConnection,
    tasks: &mut [T],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = tasks
        .iter()
        .filter_map(|t| t.task_id().parse().ok())
        .collect();
    let claims = task_claims::claims_of(conn, &ids)?;
    if claims.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = claims.values().map(|c| c.user_id).collect();
    let names = task_claims::display_names(conn, &users)?;
    for task in tasks {
        let Ok(id) = task.task_id().parse::<Uuid>() else {
            continue;
        };
        task.set_claim(claims.get(&id).map(|claim| claim_dto(claim, &names)));
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
        return Ok(held_line(&holder, since));
    }
    let claims_column = match column_id {
        Some(column) => task_claims::column_claims(conn, column).map_err(ApiError::internal)?,
        None => false,
    };
    Ok(free_line(claims_column))
}

/// [`claim_text`] from what was read already (KAIROS-T-0626): the claim as
/// (name of the holder, made with an agent key, since), and whether the
/// column of the task holds claims.
pub fn claim_line(claim: Option<(&str, bool, DateTime<Utc>)>, claims_column: bool) -> String {
    match claim {
        Some((name, agent, since)) => held_line(&holder_text(name, agent), since),
        None => free_line(claims_column),
    }
}

fn held_line(holder: &str, since: DateTime<Utc>) -> String {
    format!(
        "- claim: {holder} since {}\n",
        since.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

fn free_line(claims_column: bool) -> String {
    if claims_column {
        "- claim: none. The task is free: claim_item takes it, and a move to Active gives it.\n"
            .to_string()
    } else {
        String::new()
    }
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

/// The right to take the claim of a free task (KAIROS-T-0528,
/// KAIROS-T-0642): `transition_items` on the board of the task. The edit
/// rule gives no claim: a claimant who cannot move the task to done is a
/// dead end (KAIROS-A-0030 as amended, KAIROS-A-0037).
pub fn require_claim_take(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Uuid,
    user: Uuid,
) -> Result<(), ApiError> {
    crate::api::require_capability(conn, slug, Some(board_id), user, "transition_items")
}

/// The person that `reference` names, for a hand-off, in the forms of
/// [`crate::references::PERSON_FORMS`]: a member of the organization
/// `slug`, or `me` for the caller `me` (KAIROS-T-0528, KAIROS-T-0689). A
/// service account is refused: only a person can have a claim.
pub fn resolve_person(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    me: Uuid,
) -> Result<Uuid, ApiError> {
    let reference = crate::references::text(reference);
    let mut member = None;
    for user in crate::references::people(conn, me, reference)? {
        if kairos_db::abac::is_org_member(conn, slug, user.id).map_err(ApiError::internal)? {
            if member.is_some() {
                return Err(ApiError::validation(format!(
                    "More than one member of the organization has the name {reference:?}. \
                     Send the id of the member."
                ))
                .with_details(serde_json::json!({ "field": "to" })));
            }
            member = Some((user.id, user.kind));
        }
    }
    let Some((id, kind)) = member else {
        return Err(ApiError::validation(format!(
            "No member of the organization has the id, the email, the user name or the \
             service-account name {reference:?}. Send me, or a person of the organization."
        ))
        .with_details(serde_json::json!({ "field": "to" })));
    };
    if kind != kairos_db::models::public::USER_KIND_HUMAN {
        return Err(ApiError::validation(format!(
            "{reference:?} is a service account. Only a person can have a claim. Send a person \
             of the organization."
        ))
        .with_details(serde_json::json!({ "field": "to" })));
    }
    Ok(id)
}

/// THE TARGET OF A HAND-OFF (KAIROS-T-0530): the person `to` must hold
/// `transition_items` on the board of the task, by the team of the board,
/// by a grant, or as an organization admin. This is the check of
/// [`crate::api::require_capability`], made for the target.
///
/// Why: a claim says who works the task. A person who cannot move the
/// task cannot finish it, so the claim would be a dead end.
///
/// The refusal is 422 VALIDATION with `details.argument = "to"`, not 403:
/// the caller may hand off, and the input `to` is the problem. `me` passes
/// only when the caller holds the capability.
pub fn require_claim_target(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Uuid,
    to: Uuid,
    reference: &str,
    short_code: &str,
) -> Result<(), ApiError> {
    if kairos_db::abac::authorize(conn, slug, board_id, to, TRANSITION_ITEMS)
        .map_err(ApiError::internal)?
    {
        return Ok(());
    }
    use kairos_db::schema::boards;
    let board_slug: String = boards::table
        .find(board_id)
        .select(boards::slug)
        .first(conn)
        .map_err(ApiError::internal)?;
    Err(ApiError::validation(format!(
        "{reference:?} cannot get the claim of {short_code}. The person needs \
         {TRANSITION_ITEMS} on the board {board_slug}. Send a person of the team of the \
         board, or give the person a grant first."
    ))
    .with_details(serde_json::json!({
        "field": "to",
        "board_id": board_id,
        "board": board_slug,
        "required_capability": TRANSITION_ITEMS,
    })))
}

/// The capability that a hand-off target must hold (KAIROS-T-0530).
const TRANSITION_ITEMS: &str = "transition_items";

/// The persons who can get the claim of a task on the board `board_id`
/// (KAIROS-T-0530): each person of the organization `slug` who holds
/// `transition_items` on the board. Sorted by email.
///
/// The candidates are the organization admins, the persons with a grant
/// on the board, and the members of the team of the board. Each other
/// person fails [`kairos_db::abac::authorize`] for `transition_items`.
/// Each candidate is then checked with `authorize`, so the list and
/// [`require_claim_target`] agree. A service account is left out.
pub fn hand_off_targets(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Uuid,
) -> Result<Vec<dto::HandOffTarget>, ApiError> {
    use kairos_db::schema::{
        board_member_capabilities, boards, organization_members, organizations, team_members, users,
    };
    let mut candidates: Vec<Uuid> = organization_members::table
        .inner_join(organizations::table)
        .filter(organizations::slug.eq(slug))
        .filter(organization_members::role.eq(kairos_db::models::OrgRole::Admin))
        .select(organization_members::user_id)
        .load(conn)
        .map_err(ApiError::internal)?;
    candidates.extend(
        board_member_capabilities::table
            .filter(board_member_capabilities::board_id.eq(board_id))
            .select(board_member_capabilities::user_id)
            .load::<Uuid>(conn)
            .map_err(ApiError::internal)?,
    );
    let team: Option<Uuid> = boards::table
        .find(board_id)
        .select(boards::team_id)
        .first(conn)
        .map_err(ApiError::internal)?;
    if let Some(team) = team {
        candidates.extend(
            team_members::table
                .filter(team_members::team_id.eq(team))
                .select(team_members::user_id)
                .load::<Uuid>(conn)
                .map_err(ApiError::internal)?,
        );
    }
    candidates.sort_unstable();
    candidates.dedup();
    let people: Vec<(Uuid, String, String)> = users::table
        .filter(users::id.eq_any(&candidates))
        .filter(users::kind.eq(kairos_db::models::public::USER_KIND_HUMAN))
        .order(users::email.asc())
        .select((users::id, users::email, users::display_name))
        .load(conn)
        .map_err(ApiError::internal)?;
    let mut targets = Vec::new();
    for (id, email, display_name) in people {
        let member = kairos_db::abac::is_org_member(conn, slug, id).map_err(ApiError::internal)?;
        if member
            && kairos_db::abac::authorize(conn, slug, board_id, id, TRANSITION_ITEMS)
                .map_err(ApiError::internal)?
        {
            targets.push(dto::HandOffTarget {
                user_id: id.to_string(),
                email,
                display_name,
            });
        }
    }
    Ok(targets)
}

/// [`ClaimError`] → HTTP.
pub fn map_claim_error(e: ClaimError) -> ApiError {
    match e {
        ClaimError::TaskNotFound(_) => ApiError::not_found(e.to_string())
            .with_details(serde_json::json!({ "field": "short_code" })),
        ClaimError::NotInClaimsColumn { .. } => {
            ApiError::unprocessable("NOT_CLAIMABLE", e.to_string())
        }
        ClaimError::NoClaim { .. } => ApiError::unprocessable("NO_CLAIM", e.to_string()),
        ClaimError::AlreadyHolder { .. } => ApiError::conflict(e.to_string()),
        // KAIROS-T-0528: a different person has the claim.
        ClaimError::Claimed {
            holder_id, since, ..
        } => ApiError::new(axum::http::StatusCode::CONFLICT, "CLAIMED", e.to_string())
            .with_details(serde_json::json!({ "holder_id": holder_id, "since": since })),
        ClaimError::NotAPerson(_) => {
            ApiError::validation(e.to_string()).with_details(serde_json::json!({ "field": "user" }))
        }
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
        add_header(&mut headers, &[Warning::Claim(w)]);
        assert_eq!(headers.get_all(WARNING_HEADER).iter().count(), 1);
    }

    /// KAIROS-T-0565: the warning of open blockers.
    #[test]
    fn the_blocker_warning_names_the_column_and_each_blocker() {
        let w = BlockerWarning {
            short_code: "KAIROS-T-0001".into(),
            column: "Active".into(),
            blockers: vec![
                kairos_db::board_policy::OpenBlocker {
                    short_code: "KAIROS-T-0012".into(),
                    cancelled: false,
                },
                kairos_db::board_policy::OpenBlocker {
                    short_code: "KAIROS-T-0013".into(),
                    cancelled: true,
                },
            ],
        };
        assert_eq!(
            w.header_text(),
            "KAIROS-T-0001 moved into Active, blocked by KAIROS-T-0012 (open), KAIROS-T-0013 \
             (cancelled)"
        );
        assert_eq!(
            w.mcp_text(),
            "Warning: KAIROS-T-0001 is blocked by KAIROS-T-0012 (open), KAIROS-T-0013 \
             (cancelled). Your move into Active is recorded."
        );
    }

    /// KAIROS-T-0371: the warning of a WIP limit.
    #[test]
    fn the_wip_warning_names_the_column_the_count_and_the_limit() {
        let w = Warning::Wip(WipWarning {
            short_code: "ACME-T-0002".to_string(),
            column: "Active".to_string(),
            count: 4,
            limit: 3,
        });
        assert_eq!(
            w.header_text(),
            "ACME-T-0002 moved into Active, over its WIP limit (4 of 3)"
        );
        assert_eq!(
            w.mcp_text(),
            "Warning: the column Active has 4 items, over its WIP limit of 3. Your move of \
             ACME-T-0002 is recorded."
        );
        let mut headers = axum::http::HeaderMap::new();
        add_header(&mut headers, &[w, Warning::Claim(warning(true))]);
        assert_eq!(headers.get_all(WARNING_HEADER).iter().count(), 2);
    }
}
