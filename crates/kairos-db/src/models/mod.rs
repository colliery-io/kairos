//! Typed models over [`crate::schema`] (KAIROS-T-0009), one module per
//! table family:
//!
//! - [`enums`] — Rust enums for every TEXT-CHECK column (FromSql/ToSql)
//! - [`public`] — shared `public.*` tables (orgs, users, system defaults)
//! - [`teams`] — tenant organizational tables (teams, delivery streams)
//! - [`boards`] — tenant board system + capability grants
//! - [`templates`] — tenant templates & metadata tables
//! - [`items`] — tenant entity tables (strategies … adrs)
//! - [`graph`] — relationship graph, item history, activity log
//!
//! Every table gets a `Queryable`/`Selectable` row struct and an
//! `Insertable` `New*` struct. Tables with updatable non-key columns also
//! get an `AsChangeset` `*Changeset` struct; the append-only tables
//! (`item_history`, `activity_log`) and the pure join table
//! (`team_delivery_streams`) deliberately have none.

pub mod boards;
pub mod enums;
pub mod graph;
pub mod items;
pub mod public;
pub mod forge;
pub mod team_pages;
pub mod teams;
pub mod templates;

pub use boards::*;
pub use enums::*;
pub use graph::*;
pub use items::*;
pub use public::*;
pub use forge::*;
pub use team_pages::*;
pub use teams::*;
pub use templates::*;
