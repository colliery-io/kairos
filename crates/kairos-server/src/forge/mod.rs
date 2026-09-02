//! Git-forge integration (KAIROS-I-0009): webhook credential derivation
//! and, from KAIROS-T-0099, the delivery endpoint itself.
//!
//! The management API (`/api/forge-connections`) lives with the other
//! organizational families in [`crate::api::org::forge`]; this module
//! holds the parts that are NOT part of the authenticated `/api` surface.

pub mod auth;
pub mod webhook;
