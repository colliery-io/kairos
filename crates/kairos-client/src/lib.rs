//! `kairos-client` — typed HTTP client for the Kairos API, shared by the
//! CLI (`kairos-cli`) and integration tests per KAIROS-A-0009.
//!
//! [`types`] carries the shared wire types (KAIROS-T-0018, per the
//! KAIROS-A-0015 shared-types direction): entity DTOs, request bodies, and
//! the S-0005 envelopes, with serde + utoipa derives. [`client`] is the
//! HTTP client itself (KAIROS-T-0024): [`KairosClient`] with a typed
//! method per endpoint family, [`Error`] mapping the S-0005 error contract
//! to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).

pub mod client;
pub mod error;
pub mod types;
pub mod types_forge;
pub mod types_graph;
pub mod types_org;
pub mod types_search;
pub mod types_service_accounts;
pub mod types_team_pages;
pub mod ws;

pub mod types_meta;

pub mod types_events;

pub use client::{EntityKind, KairosClient, StaticToken, TokenProvider};
pub use error::Error;
pub use ws::EventStream;

/// Placeholder marker kept for early consumers (KAIROS-I-0003).
pub const CRATE_NAME: &str = "kairos-client";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(CRATE_NAME, "kairos-client");
    }

    /// The envelopes round-trip through serde with the S-0005 field names.
    #[test]
    fn envelopes_round_trip() {
        let envelope: types::ErrorEnvelope = serde_json::from_value(serde_json::json!({
            "error": {"code": "CONFLICT", "message": "stale", "details": {"current": {}}}
        }))
        .expect("error envelope deserializes");
        assert_eq!(envelope.error.code, "CONFLICT");

        let list = types::ListEnvelope::<types::DeleteResponse> {
            items: vec![],
            total: 0,
            limit: 50,
            offset: 0,
        };
        let value = serde_json::to_value(&list).expect("list envelope serializes");
        assert_eq!(value["total"], 0);
        assert_eq!(value["limit"], 50);
        assert!(value["items"].as_array().expect("items array").is_empty());
    }
}
