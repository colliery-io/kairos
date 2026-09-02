//! `kairos-server` library surface (KAIROS-T-0017): everything the binary
//! does apart from CLI parsing, exposed so integration tests can build the
//! exact production router in-process.
//!
//! - [`config`] — env-var configuration (KAIROS-A-0013, fail-fast)
//! - [`error`] — the S-0005 error envelope
//! - [`metrics`] — Prometheus `/metrics` + `/readyz` (KAIROS-A-0013, T-0049)
//! - [`middleware`] — OIDC auth (A-0010) + tenant resolution (A-0005 §2)
//! - [`api`] — the S-0005 entity endpoint families (KAIROS-T-0018)
//! - [`blocking`] — the sync pool bridging handlers to kairos-db services
//! - [`app`] — state, router construction, and the serve loop
//! - [`ws`] — the `/ws/events` channel (A-0005 §5, KAIROS-T-0022)
//! - [`mcp`] — the `/mcp` MCP endpoint (A-0011 / S-0006, KAIROS-T-0026)
//! - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)

pub mod api;
pub mod app;
pub mod blocking;
pub mod config;
pub mod error;
pub mod forge;
pub mod metrics;
pub mod middleware;
pub mod ws;

pub mod mcp;

pub mod scim;

pub mod service_accounts;

pub mod web;

use tracing_subscriber::EnvFilter;

use crate::config::{AppConfig, LogFormat};

/// Initialize `tracing-subscriber` per KAIROS-A-0013: `KAIROS_LOG_LEVEL`
/// filter directive, `KAIROS_LOG_FORMAT` json (default) or pretty.
pub fn init_tracing(config: &AppConfig) {
    let filter = EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    match config.log_format {
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init(),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .pretty()
            .with_env_filter(filter)
            .init(),
    }
}
