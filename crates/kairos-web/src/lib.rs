//! `kairos-web` — the Leptos CSR frontend (KAIROS-A-0015), served by
//! `kairos-server` at `/` (KAIROS-T-0039).
//!
//! Read `docs/gui-conventions.md` before adding code here: it fixes the
//! module layout, the aurora-dark token rule (no hardcoded colors — CI
//! greps), the data-layer pattern ([`api`]), the auth flow ([`auth`]),
//! and the loading/error/empty-state conventions the fan-out tasks
//! (KAIROS-T-0040..T-0044) must follow.
//!
//! Module map:
//! - [`app`] — root component: router, protected shell (nav + whoami)
//! - [`auth`] — PKCE against the deployment issuer (A-0010): in-memory
//!   tokens, silent refresh, login redirect, logout
//! - [`api`] — thin fetch layer over the same-origin `/api` + mirror DTOs
//! - [`pages`] — one module per route; T-0040..T-0044 replace the stubs

pub mod api;
pub mod app;
pub mod auth;
pub mod pages;

pub use app::App;
