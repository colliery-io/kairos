//! `kairos-db` — database access for Kairos: the Diesel `schema.rs`,
//! models, typed queries, embedded migrations (public + tenant trees), and
//! the `bb8`/`diesel-async` connection pool with per-request `search_path`
//! switching.
//!
//! Per KAIROS-A-0009 raw SQL via `diesel::sql_query` is the sanctioned
//! escape hatch for recursive-CTE traversals and the search pipeline.

pub mod abac;
pub mod api_keys;
pub mod boards;
pub mod events;
pub mod graph;
pub mod items;
pub mod migrations;
pub mod models;
pub mod pool;
pub mod retention;
pub mod schema;
pub mod scim;
pub mod search;
pub mod seed;
pub mod service_accounts;
pub mod tenant;

pub use abac::{
    AbacError, authorize, check_capability, grant_capability, is_org_admin,
    resolve_authorization_board, revoke_capability,
};
pub use boards::{
    BoardError, add_column, add_transition, create_board, dead_end_columns, remove_column,
    remove_transition, rename_column, reorder_columns, transition_adr, transition_initiative,
    transition_strategy, transition_task,
};
pub use graph::{
    GraphError, ItemRelationships, Neighbor, link_items, relationships_for, unlink_items,
};
pub use items::{
    CascadePreview, ContentUpdate, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy,
    CreateTask, ItemError, SoftDeleteOutcome, create_adr, create_document, create_initiative,
    create_strategy, create_task, next_short_code, preview_cascade, rollback_item,
    soft_delete_item, update_item_content,
};
pub use migrations::{
    MigrationError, establish_migration_connection, has_pending_public_migrations,
    run_public_migrations,
};
pub use pool::{PgPool, PoolError, TenantConnection, TenantPool};
pub use seed::{SeedDemoReport, SeedError, seed_demo};
pub use tenant::{
    TenantError, TenantInfo, TenantMigrationOutcome, TenantProvisionReport, drop_tenant,
    list_tenants, migrate_all_tenants, provision_tenant,
};
