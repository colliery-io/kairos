//! The symbol extraction and the call graph of narsil-mcp, vendored.
//!
//! Upstream: <https://github.com/postrv/narsil-mcp> at commit
//! a4a2bd7a95f8ef9aab682bf1db97905d8f79b895, MIT OR Apache-2.0. Only
//! `symbols.rs`, `parser.rs` (Rust, Python, TypeScript/TSX and Go) and
//! `callgraph.rs` are here; see NOTICE for the changes. Kairos code does not
//! go in this crate: it goes in `kairos-index`.

// Upstream code, kept as close to the original as possible so that an
// update is a copy. It is not held to the workspace's lint bar.
#![allow(clippy::all, dead_code)]

pub mod callgraph;
pub mod parser;
pub mod symbols;
