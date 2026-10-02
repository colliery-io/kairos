//! A caller of a function that a macro makes (COLLIERY-T-2531).

use crate::boards;

/// Moves a task, as an MCP tool does.
pub fn transition_item(id: u32) -> String {
    boards::transition_task(id)
}
