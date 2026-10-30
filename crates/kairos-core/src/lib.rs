//! `kairos-core` — pure domain logic for Kairos: board rules, work-item
//! transitions, ABAC capability checks, and short-code generation.
//!
//! Per KAIROS-A-0009 this crate performs no I/O; it is consumed by
//! `kairos-db` and `kairos-server`.

pub mod abac;
pub mod board;
pub mod chunk;
pub mod embed_text;
pub mod external_ref;
pub mod forge;
pub mod forge_filter;
pub mod forge_sync;
pub mod graph;
pub mod history_diff;
pub mod items;
pub mod key_scope;
pub mod notifications;
pub mod primary;
pub mod repositories;
pub mod retention;
pub mod retrieval;
pub mod search;
pub mod sections;
pub mod short_code;
pub mod slug;
