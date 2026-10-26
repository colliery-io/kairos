//! The `kairos` command tree over `kairos-client` (KAIROS-T-0037, per
//! KAIROS-A-0015): one module per noun, all thin veneers — flag plumbing
//! and rendering only, no business logic.

pub mod activity;
pub mod admin;
pub mod adr_check;
pub mod boards;
pub mod columns;
pub mod config;
pub mod dispatch;
pub mod entities;
pub mod grants;
pub mod item_views;
pub mod keys;
pub mod links;
pub mod mcp_proxy;
pub mod me;
pub mod members;
pub mod meta;
pub mod notifications;
pub mod orgs;
pub mod pages;
pub mod read_tokens;
pub mod repos;
pub mod repos_local;
pub mod search;
pub mod service_accounts;
pub mod state;
pub mod streams;
pub mod team_pages;
pub mod teams;
pub mod webhooks;
