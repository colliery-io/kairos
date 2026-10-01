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
/// macro that keeps its input as text.
pub fn label(count: usize) -> String {
    let source = quoted!(fixture_name());
    format!("{} ({source})", count_text(count))
}

/// Calls a fixture function inside assert_eq!, format!, println! and vec!.
/// These std macros expand only with a std source of Rust 1.94 or later
/// (COLLIERY-T-1860).
pub fn std_macro_calls() -> Vec<&'static str> {
    assert_eq!(fixture_name(), "polyglot");
    let text = format!("{}", fixture_name());
    println!("{text} {}", fixture_name());
    vec![fixture_name()]
}
