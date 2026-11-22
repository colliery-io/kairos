//! The move of a task back one column on the server (KAIROS-T-0774,
//! COLLIERY-I-0678): Active → Todo, Todo → Backlog, after a false start.
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
