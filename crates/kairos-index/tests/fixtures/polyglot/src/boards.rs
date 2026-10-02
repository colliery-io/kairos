//! Functions that a macro makes (COLLIERY-T-2531), as `transition_item_fn!`
//! makes them in Kairos. The made function has no symbol of its own.

/// Makes a function that moves an item of one kind.
macro_rules! transition_fn {
    ($name:ident, $kind:literal) => {
        pub fn $name(id: u32) -> String {
            format!("{} {id} moved", $kind)
        }
    };
}

transition_fn!(transition_task, "task");
