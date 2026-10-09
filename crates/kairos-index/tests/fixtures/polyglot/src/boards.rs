//! Functions that a macro makes (COLLIERY-T-2531), as `transition_item_fn!`
//! makes them in Kairos. The made function has no symbol of its own.

/// Makes a function that moves an item of one kind.
macro_rules! transition_fn {
    ($name:ident, $kind:literal) => {
        pub fn $name(id: u32) -> String {
            format!("{} {id} moved: {}", $kind, check_rule(id))
        }
    };
}

transition_fn!(transition_task, "task");

/// The check of the board rules: only the body of the macro calls it
/// (KAIROS-T-0353).
pub fn check_rule(id: u32) -> bool {
    id > 0
}
