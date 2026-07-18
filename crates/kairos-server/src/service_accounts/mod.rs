//! Service accounts and their API keys (KAIROS-A-0017 / initiative I-0005).
//!
//! A service account is a machine principal (a `public.users` row with
//! `kind = "service_account"`) that authenticates with an opaque, hashed,
//! revocable API key instead of interactive OIDC login. It is org-scoped and
//! holds ordinary board ABAC grants, so it flows through the whole `/api` +
//! `/mcp` + `/ws` stack exactly like a human — the key just resolves to its
//! `AuthContext`.
//!
//! - [`auth`] — key format, hashing, and the request-path authentication used
//!   by [`crate::middleware::auth::require_auth`] (KAIROS-T-0058).
//! - [`routes`] — the org-admin management API (create/list/delete accounts,
//!   mint/list/revoke keys), KAIROS-T-0059.

pub mod auth;
pub mod routes;

pub(crate) use routes::router;
