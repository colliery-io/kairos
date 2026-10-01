# polyglot

A small repository for the kairos-index tests. It has Rust, Python,
TypeScript and Go code with a call graph that we know.

- `expected-symbols.toml` lists each symbol that the index must have, and the
  decision for the special files.
- `expected-edges.toml` lists the call edges.

When you change a file here, change the 2 lists too.

The Rust test crates `tests/first.rs` and `tests/second.rs` share
`tests/common/mod.rs`. rust-analyzer 1.93.0 `scip` panics on a file in 2
crates, so the index leaves `tests/second.rs` out of the SCIP run.
