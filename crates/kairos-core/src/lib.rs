//! `kairos-core` — pure domain logic for Kairos: board rules, work-item
//! transitions, ABAC capability checks, and short-code generation.
//!
//! Per KAIROS-A-0009 this crate performs no I/O; it is consumed by
//! `kairos-db` and `kairos-server`.

pub mod abac;
pub mod board;
pub mod forge;
pub mod graph;
pub mod items;
pub mod retention;
pub mod search;
pub mod short_code;
