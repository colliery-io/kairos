//! A handler with the same name as a function that a macro makes
//! (COLLIERY-T-2531).

use crate::boards;

/// The handler: it calls the made function of the same name.
pub fn transition_task(id: u32) -> String {
    boards::transition_task(id)
}
