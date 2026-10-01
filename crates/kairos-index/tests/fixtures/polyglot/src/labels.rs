//! Calls written inside macro invocations (COLLIERY-T-1851).

/// Keeps the tokens of its input as text, so no call in it is resolved.
macro_rules! quoted {
    ($($tokens:tt)*) => {
        stringify!($($tokens)*)
    };
}

/// The text of a count.
pub fn count_text(count: usize) -> String {
    count.to_string()
}

/// The name of the fixture.
pub fn fixture_name() -> &'static str {
    "polyglot"
}

/// Calls fixture functions inside a format! argument and inside a custom
/// macro_rules! invocation.
pub fn label(count: usize) -> String {
    let source = quoted!(fixture_name());
    format!("{} ({source})", count_text(count))
}
