//! The live state of a task on the server (KAIROS-T-0506): the write
//! shared by REST and MCP, the DTO, and the texts of `get_item` and
//! `board_items`. The rules are in [`kairos_db::task_state`].
//!
//! Who may write the state: the edit rule of the task
//! ([`crate::api::require_item_edit`]): the creator of the task, or
//! `manage_tasks` on its board. The state is not content: a write makes no
//! new version of the task.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::TaskState;
use kairos_db::task_claims::display_names;
use kairos_db::task_state::{self, Field, PatchError, StatePatch};
use uuid::Uuid;

use crate::error::ApiError;

/// The write of REST and MCP (KAIROS-T-0533): the edit rule, then the patch.
/// A field that is not sent is kept, `null` clears it; an empty patch is
/// refused; a stale `expected_updated_at` is a `CONFLICT` with the current
/// state in the details. The write emits `item_updated`, so the cards and
/// the item page refresh, then `task_state_changed` (KAIROS-T-0574).
pub fn write_state(
    conn: &mut PgConnection,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    task_id: Uuid,
    body: &dto::SetTaskStateRequest,
) -> Result<dto::TaskState, ApiError> {
    let expected_updated_at = body
        .expected_updated_at
        .as_deref()
        .map(|text| {
            DateTime::parse_from_rfc3339(text)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|_| {
                    ApiError::validation(format!(
                        "{text:?} is not an RFC 3339 instant. Send expected_updated_at as the \
                         updated_at of the state you read."
                    ))
                    .with_details(serde_json::json!({ "field": "expected_updated_at" }))
                })
        })
        .transpose()?;
    let patch = StatePatch {
        branch: Field::of(body.branch.clone()),
        commit: Field::of(body.commit.clone()),
        pushed: body.pushed,
        pr_url: Field::of(body.pr_url.clone()),
        next_step: Field::of(body.next_step.clone()),
        expected_updated_at,
    };
    if patch.is_empty() {
        return Err(ApiError::validation(
            "The patch changes nothing. Send at least one of branch, commit, pushed, pr_url and \
             next_step: a field that you send is set, null clears it, a field that you do not \
             send is kept.",
        )
        .with_details(serde_json::json!({
            "field": "branch",
            "fields": ["branch", "commit", "pushed", "pr_url", "next_step"],
        })));
    }
    crate::api::require_item_edit(conn, role, user, task_id, ItemType::Task)?;
    let state = match task_state::patch(conn, task_id, patch, user) {
        Ok(state) => state,
        Err(PatchError::Conflict(current)) => {
            let names = display_names(conn, &[current.updated_by]).map_err(ApiError::internal)?;
            let current = state_dto(&current, &names);
            return Err(ApiError::conflict(format!(
                "The state of the task changed at {} ({}). Read it, then write again with that \
                 updated_at.",
                current.updated_at, current.updated_by_name
            ))
            .with_details(serde_json::json!({ "state": current })));
        }
        Err(PatchError::Database(error)) => return Err(ApiError::internal(error)),
    };
    kairos_db::events::emit_item_event_by_id(
        conn,
        kairos_db::events::EventKind::ItemUpdated,
        "task",
        task_id,
        user,
    )
    .map_err(ApiError::internal)?;
    // KAIROS-T-0574: the precise kind, after the coarse one.
    kairos_db::events::emit_item_event_by_id(
        conn,
        kairos_db::events::EventKind::TaskStateChanged,
        "task",
        task_id,
        user,
    )
    .map_err(ApiError::internal)?;
    let names = display_names(conn, &[state.updated_by]).map_err(ApiError::internal)?;
    Ok(state_dto(&state, &names))
}

/// The DTO of a state row.
pub(crate) fn state_dto(state: &TaskState, names: &HashMap<Uuid, String>) -> dto::TaskState {
    dto::TaskState {
        branch: state.branch.clone(),
        commit: state.commit.clone(),
        pushed: state.pushed,
        pr_url: state.pr_url.clone(),
        next_step: state.next_step.clone(),
        updated_at: state.updated_at.to_rfc3339(),
        updated_by: state.updated_by.to_string(),
        updated_by_name: names
            .get(&state.updated_by)
            .cloned()
            .unwrap_or_else(|| state.updated_by.to_string()),
    }
}

/// Put the states of the tasks on their DTOs. Two queries for the list.
pub fn attach_states<T: crate::api::convert::TaskDto>(
    conn: &mut PgConnection,
    tasks: &mut [T],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = tasks
        .iter()
        .filter_map(|t| t.task_id().parse().ok())
        .collect();
    let states = task_state::states_of(conn, &ids)?;
    if states.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = states.values().map(|s| s.updated_by).collect();
    let names = display_names(conn, &users)?;
    for task in tasks {
        let Ok(id) = task.task_id().parse::<Uuid>() else {
            continue;
        };
        task.set_state(states.get(&id).map(|state| state_dto(state, &names)));
    }
    Ok(())
}

/// The states of the tasks as the DTOs, by task (MCP `board_items`).
pub fn state_dtos(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, dto::TaskState>, ApiError> {
    let states = task_state::states_of(conn, task_ids).map_err(ApiError::internal)?;
    let users: Vec<Uuid> = states.values().map(|s| s.updated_by).collect();
    let names = display_names(conn, &users).map_err(ApiError::internal)?;
    Ok(states
        .into_iter()
        .map(|(task_id, state)| (task_id, state_dto(&state, &names)))
        .collect())
}

/// The line of the state in `get_item`:
/// `- state: branch x at abc123, pushed, PR <url>. Next: … (updated 5 min ago by Robin)`.
/// Empty when the task has no state.
pub fn state_line(conn: &mut PgConnection, task_id: Uuid) -> Result<String, ApiError> {
    Ok(match state_dtos(conn, &[task_id])?.remove(&task_id) {
        Some(state) => format!("- state: {}\n", state_text(&state, Utc::now())),
        None => String::new(),
    })
}

/// [`state_line`] from a state that was read already (KAIROS-T-0626), with
/// the name of who wrote it.
pub fn state_line_of(state: &TaskState, updated_by_name: &str, now: DateTime<Utc>) -> String {
    let names = HashMap::from([(state.updated_by, updated_by_name.to_string())]);
    format!("- state: {}\n", state_text(&state_dto(state, &names), now))
}

/// The text of a state: the fields that have a value, then the next step,
/// then who wrote it when.
pub fn state_text(state: &dto::TaskState, now: DateTime<Utc>) -> String {
    let mut parts: Vec<String> = Vec::new();
    match (&state.branch, &state.commit) {
        (Some(branch), Some(commit)) => parts.push(format!("branch {branch} at {commit}")),
        (Some(branch), None) => parts.push(format!("branch {branch}")),
        (None, Some(commit)) => parts.push(format!("commit {commit}")),
        (None, None) => {}
    }
    if state.commit.is_some() || state.branch.is_some() {
        parts.push(if state.pushed { "pushed" } else { "not pushed" }.to_string());
    } else if state.pushed {
        parts.push("pushed".to_string());
    }
    if let Some(pr) = &state.pr_url {
        parts.push(format!("PR {pr}"));
    }
    let mut text = parts.join(", ");
    if let Some(next) = &state.next_step {
        if !text.is_empty() {
            text.push_str(". ");
        }
        text.push_str("Next: ");
        text.push_str(next);
    }
    if !text.is_empty() && !text.ends_with('.') {
        text.push('.');
    }
    let when = DateTime::parse_from_rfc3339(&state.updated_at)
        .map(|at| ago(at.with_timezone(&Utc), now))
        .unwrap_or_else(|_| state.updated_at.clone());
    format!("{text} (updated {when} by {})", state.updated_by_name)
}

/// The tag of the state on a `board_items` line: the branch and the next
/// step, when they have a value.
pub fn state_tag(state: &dto::TaskState, now: DateTime<Utc>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(branch) = &state.branch {
        parts.push(format!("branch {branch}"));
    }
    if let Some(next) = &state.next_step {
        parts.push(format!("next: {next}"));
    }
    if parts.is_empty() {
        return String::new();
    }
    // KAIROS-T-0533: the age, so a stale state does not read as current.
    let age = DateTime::parse_from_rfc3339(&state.updated_at)
        .map(|t| ago(t.with_timezone(&Utc), now))
        .unwrap_or_default();
    if age.is_empty() {
        format!(" [state: {}]", parts.join("; "))
    } else {
        format!(" [state: {}; {age}]", parts.join("; "))
    }
}

/// `just now`, `5 min ago`, `3 h ago`, `2 days ago`.
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => {
            let days = seconds / 86_400;
            if days == 1 {
                "1 day ago".to_string()
            } else {
                format!("{days} days ago")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> dto::TaskState {
        dto::TaskState {
            branch: Some("kairos-t-0506".into()),
            commit: Some("abc1234".into()),
            pushed: true,
            pr_url: Some("https://example.com/pr/12".into()),
            next_step: Some("Wait for CI".into()),
            updated_at: "2026-10-15T10:00:00+00:00".into(),
            updated_by: "u1".into(),
            updated_by_name: "Robin".into(),
        }
    }

    #[test]
    fn the_text_names_each_field_and_who_wrote_it_when() {
        let now = DateTime::parse_from_rfc3339("2026-10-15T10:05:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            state_text(&state(), now),
            "branch kairos-t-0506 at abc1234, pushed, PR https://example.com/pr/12. Next: Wait for CI. (updated 5 min ago by Robin)"
        );
        let mut partial = state();
        partial.commit = None;
        partial.pushed = false;
        partial.pr_url = None;
        partial.next_step = None;
        assert_eq!(
            state_text(&partial, now),
            "branch kairos-t-0506, not pushed. (updated 5 min ago by Robin)"
        );
        let mut next_only = state();
        next_only.branch = None;
        next_only.commit = None;
        next_only.pushed = false;
        next_only.pr_url = None;
        assert_eq!(
            state_text(&next_only, now),
            "Next: Wait for CI. (updated 5 min ago by Robin)"
        );
    }

    #[test]
    fn the_tag_has_the_branch_the_next_step_and_the_age() {
        let now = DateTime::parse_from_rfc3339("2026-10-17T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            state_tag(&state(), now),
            " [state: branch kairos-t-0506; next: Wait for CI; 2 days ago]"
        );
        let mut none = state();
        none.branch = None;
        none.next_step = None;
        assert_eq!(state_tag(&none, now), "");
    }

    #[test]
    fn ago_rounds_down() {
        let now = Utc::now();
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now - chrono::Duration::seconds(300), now), "5 min ago");
        assert_eq!(ago(now - chrono::Duration::hours(3), now), "3 h ago");
        assert_eq!(ago(now - chrono::Duration::days(1), now), "1 day ago");
        assert_eq!(ago(now - chrono::Duration::days(9), now), "9 days ago");
    }
}
