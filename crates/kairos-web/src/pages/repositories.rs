//! Repositories (KAIROS-I-0010, decision KAIROS-A-0019): the shared data
//! layer for repository-scoped work. Boards (the lens + repo chips), the
//! item detail's repository picker, the team page's repositories panel and
//! the admin directory all read the same mirrors from [`api`] — one home,
//! so the wire shape is declared once (KAIROS-T-0114).
//!
//! No route page lives here yet; the admin directory is
//! `pages::admin::repositories` and re-exports from this module.

pub(crate) mod api;
