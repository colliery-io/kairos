//! Module a of the qualified-call scenario (COLLIERY-T-2531).

/// Keeps the tokens of its input as text, so no call in it is resolved.
macro_rules! quoted {
    ($($tokens:tt)*) => {
        stringify!($($tokens)*)
    };
}

/// Calls `b::start` in the text of a macro, so only the name is known.
/// Module b has no `start`: the only `start` is in module c.
pub fn run() -> &'static str {
    quoted!(b::start())
}
