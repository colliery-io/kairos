# polyglot

A small repository for the kairos-index tests. It has Rust, Python,
TypeScript and Go code with a call graph that we know.

- `expected-symbols.toml` lists each symbol that the index must have, and the
  decision for the special files.
- `expected-edges.toml` lists the call edges.

When you change a file here, change the 2 lists too.
