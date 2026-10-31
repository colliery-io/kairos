//! `kairos-db` — database access for Kairos: the Diesel `schema.rs`,
//! models, typed queries, embedded migrations (public + tenant trees), and
//! the `bb8`/`diesel-async` connection pool with per-request `search_path`
//! switching.
//!
//! Per KAIROS-A-0009 raw SQL via `diesel::sql_query` is the sanctioned
//! escape hatch for recursive-CTE traversals and the search pipeline.

pub mod abac;
pub mod agent_mark;
pub mod api_keys;
pub mod auth_failures;
pub mod board_items;
pub mod board_policy;
pub mod boards;
pub mod code_index_breakers;
pub mod code_index_builds;
pub mod code_index_settings;
pub mod code_indexes;
pub mod code_rename;
pub mod dictionaries;
pub mod dispatch;
pub mod embeddings;
pub mod events;
pub mod external_refs;
pub mod flow;
pub mod forge;
pub mod forge_actor;
pub mod forge_filters;
pub mod graph;
pub mod history;
pub mod impacts;
pub mod issue_sync;
pub mod issue_writeback;
pub mod item_activity;
pub mod item_notes;
pub mod item_owners;
pub mod item_teams;
pub mod items;
pub mod jobs;
pub mod key_scope;
pub mod local_auth;
pub mod migrations;
pub mod models;
pub mod my_work;
pub mod notices;
pub mod notifications;
pub mod org_read_tokens;
pub mod pool;
pub mod proposals;
pub mod purge;
pub mod repositories;
pub mod repository_credentials;
pub mod request_keys;
pub mod retention;
pub mod retired_codes;
pub mod schema;
pub mod scim;
pub mod search;
pub mod section_checks;
pub mod seed;
pub mod service_accounts;
pub mod site_content;
pub mod sites;
pub mod task_cancellations;
pub mod task_claims;
pub mod task_state;
pub mod team_pages;
pub mod tenant;
pub mod tls;
// KAIROS-T-0694 (KAIROS-A-0068): the count of the MCP refusals.
pub mod tool_refusals;
pub mod watches;
pub mod webhooks;
pub mod writeback;

pub use abac::{
    AbacError, StructureGrant, authorize, check_capability, check_granted_capability,
    grant_capability, holds_structure, is_org_admin, resolve_authorization_board,
    revoke_capability,
};
pub use boards::{
    BoardError, CodePrefix, SeveralDeliveryBoards, add_column, add_transition,
    boards_for_team_delete, check_board_delete, check_board_team, create_board, dead_end_columns,
    delivery_boards_for_team_delete, remove_column, remove_transition, rename_column,
    reorder_columns, teams_with_several_delivery_boards, transition_adr, transition_initiative,
    transition_strategy, transition_task,
};
pub use graph::{
    GraphError, ItemRelationships, Neighbor, link_items, relationships_for, unlink_items,
};
pub use items::{
    CascadePreview, ContentUpdate, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy,
    CreateTask, ItemError, NotReached, NotReachedReason, Principal, SoftDeleteOutcome, create_adr,
    create_document, create_initiative, create_strategy, create_task, edit_item_content,
    next_short_code, preview_cascade, preview_cascade_as, rollback_item, soft_delete_item,
    soft_delete_item_as, update_item_content,
};
pub use migrations::{
    MigrationError, establish_migration_connection, has_pending_public_migrations,
    run_public_migrations,
};
pub use pool::{PgPool, PoolError, TenantConnection, TenantPool};
pub use seed::{DroppedDemo, SeedDemoReport, SeedError, seed_demo};
pub use tenant::{
    MIGRATION_LOCK_KEY, TenantError, TenantInfo, TenantMigrationOutcome, TenantMigrationReport,
    TenantProvisionReport, drop_tenant, list_tenants, migrate_all_tenants, migrate_each_tenant,
    migrate_each_tenant_from, provision_tenant, provision_tenant_from, stale_tenants,
    tenant_schema_is_current, with_migration_lock,
};
