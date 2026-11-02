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
//! A table has a `Queryable`/`Selectable` row struct when code reads it,
//! an `Insertable` `New*` struct when code inserts into it, and an
//! `AsChangeset` `*Changeset` struct when code updates it with one. Most
//! updates use `.set(column.eq(..))`, and the system tables of `public`
//! use raw SQL (`crate::tenant`), so most tables have no `*Changeset`
//! (KAIROS-T-0743).

pub mod boards;
pub mod claims;
pub mod enums;
pub mod forge;
pub mod graph;
pub mod items;
pub mod public;
pub mod repositories;
pub mod team_pages;
pub mod teams;
pub mod templates;

pub use boards::*;
pub use claims::*;
pub use enums::*;
pub use forge::*;
pub use graph::*;
pub use items::*;
pub use public::*;
pub use team_pages::*;
pub use teams::*;
pub use templates::*;
