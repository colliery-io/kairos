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

The repeated code of the duplicates scenarios (COLLIERY-T-1857) is listed in
`expected-duplicates.toml`: an exact copy (`src/checksum.rs` and
`src/legacy.rs`), a near copy (`python/polyglot/stats.py` and `tally.py`),
2 TypeScript functions that do the same job (`web/src/totals.ts`), 2
one-line getters and 2 test functions that are the same, and pairs that are
not repeated code.

The call-graph scenarios of COLLIERY-T-2531 use these files:

- `src/a.rs`, `src/b.rs` and `src/c.rs`: `a::run` calls `b::start` in the
  text of a macro, and the only `start` is in `c`. The path does not match,
  so the edge is not `certain`.
- `src/boards.rs`, `src/api.rs` and `src/mcp.rs`: `transition_fn!` makes
  `transition_task` in `boards`, with no symbol. The calls of
  `boards::transition_task` keep the place of the macro invocation. They
  do not go to `api::transition_task`.
- `src/routes.rs`: `dispatch` reaches `target` by 2 routes, for `path`.
- `web/src/cents.ts`: `listCents` calls `centsOf`, and the 2 functions have
  the same job. A function and its callee are not a `same-idea` group.
