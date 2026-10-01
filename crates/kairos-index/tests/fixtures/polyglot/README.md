# polyglot

A small repository for the kairos-index tests. It has Rust, Python,
TypeScript and Go code with a call graph that we know.

- `expected-symbols.toml` lists each symbol that the index must have, and the
  decision for the special files.
- `expected-edges.toml` lists the call edges.

When you change a file here, change the 2 lists too.

The Rust test crates `tests/first.rs`, `tests/second.rs` and
`tests/third.rs` share `tests/common/mod.rs`. rust-analyzer 1.93.0 `scip`
panics on a file in 2 crates; the pinned rust-analyzer does not, so the index
leaves no crate out of the SCIP run (COLLIERY-T-1858).

`src/labels.rs` has calls inside macros. `std_macro_calls` calls
`fixture_name` inside `assert_eq!`, `format!`, `println!` and `vec!`. The
current rust-analyzer expands these std macros only with a std source of
Rust 1.94 or later, so the index gives it the pinned std source
(COLLIERY-T-1860).
