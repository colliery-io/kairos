//! The move of a task back one column on the server (KAIROS-T-0774,
//! COLLIERY-I-0678, KAIROS-A-0086): Active → Todo, Todo → Backlog, after a false start.
//! The column rule is in `kairos_db::boards::move_task_back`; this module
//! has the rule of who may do it, for the two surfaces: REST
//! `POST /api/tasks/{short_code}/back` and MCP `transition_item` to the
//! column before.
//!
//! Who may move a task back (Dylan, 2026-11-22: "claimant or a manager"):
//!
//! - each move needs `transition_items` on the board, as each move does;
//! - a task that a different person has the claim on moves back only by a
//!   board manager: `configure_boards` on the board (an organization admin
//!   and a holder of `manage_structure` have it). The claimant moves their
//!   own task back. A task with no claim (Todo, Backlog) needs nothing more.

use diesel::pg::PgConnection;
use kairos_db::boards::{self, MovedBack};
use uuid::Uuid;

use crate::error::ApiError;
use crate::middleware::auth::AuthContext;

/// The capability of each move.
pub const CAPABILITY: &str = "transition_items";

/// The capability of a board manager: the move back of a task that a
/// different person has the claim on.
pub const MANAGER_CAPABILITY: &str = "configure_boards";

/// Move the task back: the capability checks, the rule of the claim, the
/// move, and the notes of the column it goes to (the WIP limit and the
/// open blockers, KAIROS-T-0565).
pub fn move_back(
    conn: &mut PgConnection,
    auth: &AuthContext,
    slug: &str,
    task_id: Uuid,
    board_id: Uuid,
) -> Result<MovedBack, ApiError> {
    let user = auth.user_id;
    crate::api::require_capability(conn, slug, Some(board_id), user, CAPABILITY)?;
    if let Some(claim) =
        kairos_db::task_claims::claim_of(conn, task_id).map_err(ApiError::internal)?
        && claim.user_id != user
        && crate::api::require_capability(conn, slug, Some(board_id), user, MANAGER_CAPABILITY)
            .is_err()
    {
        let names = kairos_db::task_claims::display_names(conn, &[claim.user_id])
            .map_err(ApiError::internal)?;
        let holder = names
            .get(&claim.user_id)
            .cloned()
            .unwrap_or_else(|| claim.user_id.to_string());
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "CLAIMED",
            format!(
                "{holder} has the claim on this task. Only {holder} or a board manager \
                 ({MANAGER_CAPABILITY} on the board) can move it back. Ask {holder}, or ask \
                 them to release it (release_item)."
            ),
        )
        .with_details(serde_json::json!({
            "holder_id": claim.user_id,
            "required": MANAGER_CAPABILITY,
        })));
    }
    // KAIROS-T-0428: a move back into the entry column is the triage of
    // intake, as each move is.
    let column_id: Uuid = {
        use diesel::prelude::*;
        use kairos_db::schema::tasks;
        tasks::table
            .filter(tasks::id.eq(task_id))
            .select(tasks::column_id)
            .first(conn)
            .map_err(ApiError::internal)?
    };
    let triaged =
        match boards::back_column(conn, board_id, column_id).map_err(ApiError::internal)? {
            Some((to, _)) => crate::agent_triage::check(
                conn,
                auth,
                task_id,
                crate::agent_triage::TriageMove::Column(to),
            )?,
            None => None,
        };
    let moved = boards::move_task_back(conn, task_id, user).map_err(crate::api::map_board_error)?;
    crate::agent_triage::record(conn, auth, task_id, triaged)?;
    crate::claims::note_task_move(conn, task_id, moved.to_column_id, &moved.short_code)?;
    Ok(moved)
}

/// The text of a move back for an agent and the CLI.
pub fn move_back_text(moved: &MovedBack) -> String {
    let claim = if moved.ended_claim.is_some() {
        " The claim ended."
    } else {
        ""
    };
    format!(
        "Moved {} back: {} -> {}.{claim}",
        moved.short_code, moved.from_column, moved.to_column
    )
}

// ---------------------------------------------------------------------------
// An initiative moves back, and its tasks with it (KAIROS-T-0775)
// ---------------------------------------------------------------------------

/// The rule of a move back of an initiative (Dylan, 2026-11-22: "initiative
/// owners / initiative admins managers"): `transition_items` on its board,
/// and the person owns the initiative or is a board manager
/// (`configure_boards`). An initiative with no owner moves back as a task
/// with no claim does: by any person who can move it.
fn check_initiative_rule(
    conn: &mut PgConnection,
    slug: &str,
    initiative_id: Uuid,
    board_id: Uuid,
    user: Uuid,
) -> Result<(), ApiError> {
    crate::api::require_capability(conn, slug, Some(board_id), user, CAPABILITY)?;
    if let Some(owner) =
        kairos_db::item_owners::owner_of(conn, initiative_id).map_err(ApiError::internal)?
        && owner.user_id != user
        && crate::api::require_capability(conn, slug, Some(board_id), user, MANAGER_CAPABILITY)
            .is_err()
    {
        let names = kairos_db::task_claims::display_names(conn, &[owner.user_id])
            .map_err(ApiError::internal)?;
        let holder = names
            .get(&owner.user_id)
            .cloned()
            .unwrap_or_else(|| owner.user_id.to_string());
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "OWNED",
            format!(
                "{holder} owns this initiative. Only {holder} or a board manager \
                 ({MANAGER_CAPABILITY} on the board) can move it back. Ask {holder}."
            ),
        )
        .with_details(serde_json::json!({
            "owner_id": owner.user_id,
            "required": MANAGER_CAPABILITY,
        })));
    }
    Ok(())
}

/// The plan of a move back of the initiative, with the same rule as the
/// move: the web shows it in the modal before the move.
pub fn initiative_plan(
    conn: &mut PgConnection,
    slug: &str,
    initiative_id: Uuid,
    board_id: Uuid,
    user: Uuid,
) -> Result<boards::InitiativeBackPlan, ApiError> {
    check_initiative_rule(conn, slug, initiative_id, board_id, user)?;
    boards::initiative_back_plan(conn, initiative_id).map_err(crate::api::map_board_error)
}

/// Move the initiative back, and its Todo and Active tasks to Backlog,
/// with the rule of [`check_initiative_rule`]. One transaction; the
/// notifications go with it.
pub fn move_initiative_back(
    conn: &mut PgConnection,
    slug: &str,
    initiative_id: Uuid,
    board_id: Uuid,
    user: Uuid,
) -> Result<boards::InitiativeBackPlan, ApiError> {
    check_initiative_rule(conn, slug, initiative_id, board_id, user)?;
    let plan = boards::move_initiative_back(conn, initiative_id, user)
        .map_err(crate::api::map_board_error)?;
    crate::claims::note_wip(conn, plan.to_column_id, &plan.short_code)?;
    Ok(plan)
}

/// The text of a move back of an initiative for an agent and the CLI.
pub fn initiative_text(plan: &boards::InitiativeBackPlan) -> String {
    let mut out = format!(
        "Moved {} back: {} -> {}.",
        plan.short_code, plan.from_column, plan.to_column
    );
    if plan.tasks.is_empty() {
        out.push_str(" No task moved.");
    }
    for task in &plan.tasks {
        out.push_str(&format!(
            "\n- {} {} -> {}{}",
            task.short_code,
            task.from_column,
            task.to_column,
            if task.claimant.is_some() {
                " (the claim ended)"
            } else {
                ""
            }
        ));
    }
    out
}
