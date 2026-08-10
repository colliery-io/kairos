//! Tenant provisioning and fleet migration (KAIROS-T-0008).
//!
//! Per KAIROS-A-0001 provisioning is an application-level operation: create
//! the `public.organizations` row, create the `org_{slug}` schema, run the
//! embedded tenant migration tree (KAIROS-S-0004) with `search_path` pinned
//! to that schema, and seed defaults (KAIROS-A-0002 board configs,
//! KAIROS-A-0003 system templates/metadata).
//!
//! # Transactionality
//!
//! [`provision_tenant`] runs in ONE outer transaction: the org insert,
//! `CREATE SCHEMA`, every tenant migration (diesel nests each migration's
//! transaction as a savepoint), and all default seeding either commit
//! together or roll back together — a failed provision leaves no partial
//! state. `search_path` is pinned with `SET LOCAL`, so it reverts
//! automatically when the transaction ends.
//!
//! # Which boards exist after provisioning?
//!
//! Delivery boards are per-team (`boards.team_id` is "set for delivery
//! boards" per KAIROS-A-0002/S-0004), and a fresh tenant has no teams yet.
//! Provisioning therefore creates the strategy board, one initiative board,
//! and the ADR board; delivery boards are created later, alongside teams,
//! from the same seeded `system_board_defaults` row. All FOUR default
//! configs (including `delivery`) are seeded into
//! `public.system_board_defaults` so that later step needs no extra data.
//! (Interpretation recorded in KAIROS-T-0008.)

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Bool, Text};
use diesel_migrations::MigrationHarness;

use crate::boards::{BoardError, create_board};
use crate::migrations::TENANT_MIGRATIONS;
use crate::models::enums::BoardLevel;

/// Errors from tenant provisioning, fleet migration, or teardown.
#[derive(Debug, thiserror::Error)]
pub enum TenantError {
    /// The slug does not match the KAIROS-S-0004 organization slug pattern.
    #[error("invalid tenant slug {0:?}: must match ^[a-z][a-z0-9_-]{{1,62}}$")]
    InvalidSlug(String),
    /// An organization with this slug is already provisioned.
    #[error("tenant {0:?} already exists")]
    AlreadyExists(String),
    /// No organization with this slug exists.
    #[error("tenant {0:?} does not exist")]
    NotFound(String),
    /// `drop_tenant` was called without `confirm = true`.
    #[error(
        "refusing to drop tenant {0:?}: destructive operation requires explicit confirmation \
         (pass confirm = true / --confirm)"
    )]
    ConfirmationRequired(String),
    /// The tenant migration tree failed to apply for one schema.
    #[error("tenant migration failed for {slug:?}: {message}")]
    Migration { slug: String, message: String },
    /// Creating a default board from the seeded `system_board_defaults`
    /// failed (missing/malformed defaults row, or any board-layer error).
    #[error("creating default board failed: {0}")]
    Board(#[from] BoardError),
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// What [`provision_tenant`] created.
#[derive(Debug)]
pub struct TenantProvisionReport {
    /// The organization slug.
    pub slug: String,
    /// The created schema name (`org_{slug}`).
    pub schema: String,
    /// Tenant migration versions applied inside the new schema.
    pub migrations_applied: Vec<String>,
    /// Slugs of the default boards created (strategy/initiatives/adrs).
    pub boards_created: Vec<String>,
    /// System templates copied into the tenant `templates` table.
    pub templates_copied: usize,
    /// System metadata definitions copied into the tenant
    /// `metadata_definitions` table.
    pub metadata_definitions_copied: usize,
}

/// One tenant's outcome from [`migrate_all_tenants`].
#[derive(Debug)]
pub struct TenantMigrationOutcome {
    /// The organization slug.
    pub slug: String,
    /// The schema the migrations ran in.
    pub schema: String,
    /// Migration versions applied by this run (empty = already up to date).
    pub applied: Vec<String>,
}

/// A row from [`list_tenants`].
#[derive(Debug)]
pub struct TenantInfo {
    /// The organization slug.
    pub slug: String,
    /// The organization display name.
    pub name: String,
    /// Whether the `org_{slug}` schema actually exists.
    pub schema_exists: bool,
}

/// The default boards created at provision time: `(board_level, name, slug)`.
///
/// No `delivery` entry on purpose — delivery boards are per-team and are
/// created when teams are (see module docs).
const PROVISION_BOARDS: [(BoardLevel, &str, &str); 3] = [
    (BoardLevel::Strategy, "Strategy", "strategy"),
    (BoardLevel::Initiative, "Initiatives", "initiatives"),
    (BoardLevel::Adr, "Architecture Decisions", "adrs"),
];

/// Idempotent seed of the `public.system_*` default rows (KAIROS-A-0002
/// board configs, KAIROS-A-0003 templates + metadata definitions).
///
/// T-0007's public migration created these tables empty, so provisioning
/// seeds them on demand; every statement is `ON CONFLICT DO NOTHING` keyed
/// on the natural key (`board_level` / `slug`), so re-running never
/// duplicates and never overwrites operator customizations.
const SEED_SYSTEM_DEFAULTS_SQL: &str = r#"
-- KAIROS-A-0002 default board configurations (all four levels; delivery is
-- consumed later when teams/delivery boards are created).
INSERT INTO public.system_board_defaults (board_level, columns, transitions) VALUES
    ('strategy',
     E'Draft\nReview\nActive\nMonitoring\nCompleted',
     E'Draft -> Review\nReview -> Active\nActive -> Monitoring\nMonitoring -> Completed'),
    ('initiative',
     E'Discovery\nDesign\nReady\nDecompose\nActive\nMonitoring\nCompleted',
     E'Discovery -> Design\nDesign -> Ready\nReady -> Decompose\nDecompose -> Active\nActive -> Monitoring\nMonitoring -> Completed'),
    ('delivery',
     E'Backlog\nTodo\nBlocked\nActive\nCompleted',
     E'Backlog -> Todo\nTodo -> Active\nActive -> Completed\nTodo -> Blocked\nBlocked -> Todo\nActive -> Blocked\nBlocked -> Active'),
    ('adr',
     E'Draft\nDiscussion\nDecided\nSuperseded',
     E'Draft -> Discussion\nDiscussion -> Decided\nDecided -> Superseded')
ON CONFLICT (board_level) DO NOTHING;

-- KAIROS-A-0003 default metadata definitions.
INSERT INTO public.system_metadata_definitions (name, slug, field_type) VALUES
    ('Priority', 'priority', 'enum'),
    ('Document status', 'status', 'enum'),
    ('Complexity', 'complexity', 'enum'),
    ('Document Type', 'document_type', 'enum')
ON CONFLICT (slug) DO NOTHING;

INSERT INTO public.system_metadata_enum_options (metadata_definition_id, value, position)
SELECT d.id, o.value, o.position
FROM (VALUES
    ('priority', 'low', 0),
    ('priority', 'medium', 1),
    ('priority', 'high', 2),
    ('priority', 'critical', 3),
    ('status', 'draft', 0),
    ('status', 'review', 1),
    ('status', 'approved', 2),
    ('complexity', 'xs', 0),
    ('complexity', 's', 1),
    ('complexity', 'm', 2),
    ('complexity', 'l', 3),
    ('complexity', 'xl', 4),
    ('document_type', 'prd', 0),
    ('document_type', 'system_context', 1),
    ('document_type', 'architecture', 2),
    ('document_type', 'charter', 3),
    ('document_type', 'social_contract', 4),
    ('document_type', 'vision', 5)
) AS o(slug, value, position)
JOIN public.system_metadata_definitions d ON d.slug = o.slug
ON CONFLICT (metadata_definition_id, value) DO NOTHING;

-- KAIROS-A-0003 default templates. A-0003 specifies which templates exist
-- and their metadata, not their starter markdown; the content below is a
-- minimal sensible skeleton (interpretation recorded in KAIROS-T-0008).
INSERT INTO public.system_templates (name, slug, content) VALUES
    ('PRD', 'prd',
     E'# Product Requirements Document\n\n## Problem\n\n## Goals\n\n## Non-Goals\n\n## Requirements\n\n## Open Questions\n'),
    ('System Context', 'system_context',
     E'# System Context\n\n## Overview\n\n## Actors\n\n## External Systems\n\n## Data Flows\n'),
    ('Architecture Framing', 'architecture_framing',
     E'# Architecture Framing\n\n## Context\n\n## Constraints\n\n## Options Considered\n\n## Decision Drivers\n'),
    ('Team Charter', 'team_charter',
     E'# Team Charter\n\n## Mission\n\n## Scope\n\n## Ways of Working\n\n## Success Measures\n'),
    ('Social Contract', 'social_contract',
     E'# Social Contract\n\n## Values\n\n## Agreements\n\n## Communication\n\n## Conflict Resolution\n'),
    ('Company Vision', 'company_vision',
     E'# Company Vision\n\n## Where We Are Going\n\n## Why It Matters\n\n## How We Will Know\n')
ON CONFLICT (slug) DO NOTHING;

-- KAIROS-A-0003 template-to-metadata associations.
INSERT INTO public.system_template_metadata (template_id, metadata_definition_id, default_value, required)
SELECT t.id, d.id, a.default_value, false
FROM (VALUES
    ('prd', 'document_type', 'prd'),
    ('prd', 'status', 'draft'),
    ('system_context', 'document_type', 'system_context'),
    ('system_context', 'status', 'draft'),
    ('architecture_framing', 'document_type', 'architecture'),
    ('architecture_framing', 'status', 'draft'),
    ('team_charter', 'document_type', 'charter'),
    ('social_contract', 'document_type', 'social_contract'),
    ('company_vision', 'document_type', 'vision')
) AS a(template_slug, def_slug, default_value)
JOIN public.system_templates t ON t.slug = a.template_slug
JOIN public.system_metadata_definitions d ON d.slug = a.def_slug
ON CONFLICT (template_id, metadata_definition_id) DO NOTHING;
"#;

#[derive(QueryableByName)]
struct BoolRow {
    #[diesel(sql_type = Bool)]
    present: bool,
}

#[derive(QueryableByName)]
struct TenantRow {
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Bool)]
    schema_exists: bool,
}

/// Whether `slug` matches the KAIROS-S-0004 organization slug pattern
/// (`^[a-z][a-z0-9_-]{1,62}$` — same CHECK as `public.organizations.slug`).
///
/// Validated slugs contain no quoting metacharacters, which is what makes
/// interpolating `org_{slug}` into DDL identifiers safe.
pub fn is_valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

fn validated_slug(slug: &str) -> Result<(), TenantError> {
    if is_valid_slug(slug) {
        Ok(())
    } else {
        Err(TenantError::InvalidSlug(slug.to_string()))
    }
}

/// The schema name for an organization slug: `org_{slug}`.
pub fn tenant_schema_name(slug: &str) -> String {
    format!("org_{slug}")
}

fn schema_exists(conn: &mut PgConnection, schema: &str) -> Result<bool, TenantError> {
    let row: BoolRow =
        sql_query("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1) AS present")
            .bind::<Text, _>(schema)
            .get_result(conn)?;
    Ok(row.present)
}

/// Idempotently seed the `public.system_*` default rows (see
/// [`SEED_SYSTEM_DEFAULTS_SQL`]). Called automatically by
/// [`provision_tenant`]; exposed for tooling that wants the defaults
/// without provisioning a tenant.
pub fn seed_system_defaults(conn: &mut PgConnection) -> Result<(), TenantError> {
    conn.batch_execute(SEED_SYSTEM_DEFAULTS_SQL)?;
    Ok(())
}

/// Provision a new tenant (KAIROS-A-0001 application-level provisioning):
///
/// 1. idempotently seed `public.system_*` defaults,
/// 2. insert the `public.organizations` row,
/// 3. `CREATE SCHEMA org_{slug}`,
/// 4. run the embedded tenant migration tree with `search_path` pinned to
///    the new schema,
/// 5. copy system templates + metadata definitions into the tenant tables
///    (KAIROS-A-0003) and create the default strategy/initiative/ADR boards
///    from `system_board_defaults` (KAIROS-A-0002; delivery boards are
///    per-team and created later — see module docs).
///
/// All of it runs in a single transaction: on any failure (including
/// [`TenantError::AlreadyExists`]) nothing is left behind.
pub fn provision_tenant(
    conn: &mut PgConnection,
    slug: &str,
    name: &str,
) -> Result<TenantProvisionReport, TenantError> {
    validated_slug(slug)?;
    let schema = tenant_schema_name(slug);

    conn.transaction::<_, TenantError, _>(|conn| {
        seed_system_defaults(conn)?;

        if schema_exists(conn, &schema)? {
            return Err(TenantError::AlreadyExists(slug.to_string()));
        }
        let org_insert = sql_query("INSERT INTO public.organizations (name, slug) VALUES ($1, $2)")
            .bind::<Text, _>(name)
            .bind::<Text, _>(slug)
            .execute(conn);
        match org_insert {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(TenantError::AlreadyExists(slug.to_string()));
            }
            other => {
                other?;
            }
        }

        sql_query(format!("CREATE SCHEMA \"{schema}\"")).execute(conn)?;
        // SET LOCAL: reverts automatically when this transaction ends, so
        // the connection's default search_path is untouched afterwards.
        sql_query(format!("SET LOCAL search_path TO \"{schema}\"")).execute(conn)?;

        let migrations_applied = conn
            .run_pending_migrations(TENANT_MIGRATIONS)
            .map_err(|e| TenantError::Migration {
                slug: slug.to_string(),
                message: e.to_string(),
            })?
            .iter()
            .map(|v| v.to_string())
            .collect();

        // KAIROS-A-0003: copy system defaults into the tenant schema
        // (unqualified names resolve to the tenant schema via search_path).
        let templates_copied = sql_query(
            "INSERT INTO templates (name, slug, content, is_system_default) \
             SELECT name, slug, content, true FROM public.system_templates",
        )
        .execute(conn)?;
        let metadata_definitions_copied = sql_query(
            "INSERT INTO metadata_definitions (name, slug, field_type, is_system_default) \
             SELECT name, slug, field_type, true FROM public.system_metadata_definitions",
        )
        .execute(conn)?;
        sql_query(
            "INSERT INTO metadata_enum_options (metadata_definition_id, value, position) \
             SELECT md.id, seo.value, seo.position \
             FROM public.system_metadata_enum_options seo \
             JOIN public.system_metadata_definitions smd ON smd.id = seo.metadata_definition_id \
             JOIN metadata_definitions md ON md.slug = smd.slug",
        )
        .execute(conn)?;
        sql_query(
            "INSERT INTO template_metadata (template_id, metadata_definition_id, default_value, required) \
             SELECT t.id, md.id, stm.default_value, stm.required \
             FROM public.system_template_metadata stm \
             JOIN public.system_templates st ON st.id = stm.template_id \
             JOIN public.system_metadata_definitions smd ON smd.id = stm.metadata_definition_id \
             JOIN templates t ON t.slug = st.slug \
             JOIN metadata_definitions md ON md.slug = smd.slug",
        )
        .execute(conn)?;

        // KAIROS-A-0002: default boards (strategy/initiative/adr; delivery
        // boards are created per-team later — see module docs). Seeding is
        // owned by crate::boards::create_board (KAIROS-T-0010); actor is
        // None because no user exists at provision time.
        let mut boards_created = Vec::new();
        for (level, board_name, board_slug) in PROVISION_BOARDS {
            create_board(conn, level, board_name, board_slug, None, None)?;
            boards_created.push(board_slug.to_string());
        }

        Ok(TenantProvisionReport {
            slug: slug.to_string(),
            schema: schema.clone(),
            migrations_applied,
            boards_created,
            templates_copied,
            metadata_definitions_copied,
        })
    })
}

/// Drop a tenant: remove the `org_{slug}` schema (CASCADE) and delete the
/// `public.organizations` row, in one transaction.
///
/// Destructive and unrecoverable, so it refuses to act unless `confirm` is
/// `true` ([`TenantError::ConfirmationRequired`]).
pub fn drop_tenant(conn: &mut PgConnection, slug: &str, confirm: bool) -> Result<(), TenantError> {
    validated_slug(slug)?;
    if !confirm {
        return Err(TenantError::ConfirmationRequired(slug.to_string()));
    }
    let schema = tenant_schema_name(slug);

    conn.transaction::<_, TenantError, _>(|conn| {
        let deleted = sql_query("DELETE FROM public.organizations WHERE slug = $1")
            .bind::<Text, _>(slug)
            .execute(conn)?;
        if deleted == 0 && !schema_exists(conn, &schema)? {
            return Err(TenantError::NotFound(slug.to_string()));
        }
        sql_query(format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE")).execute(conn)?;
        Ok(())
    })
}

/// List provisioned tenants from `public.organizations`, with a
/// schema-existence check to surface drift (org row without schema).
pub fn list_tenants(conn: &mut PgConnection) -> Result<Vec<TenantInfo>, TenantError> {
    let rows: Vec<TenantRow> = sql_query(
        "SELECT o.slug AS slug, o.name AS name, \
                EXISTS (SELECT 1 FROM pg_namespace n WHERE n.nspname = 'org_' || o.slug) AS schema_exists \
         FROM public.organizations o ORDER BY o.slug",
    )
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| TenantInfo {
            slug: r.slug,
            name: r.name,
            schema_exists: r.schema_exists,
        })
        .collect())
}

/// Fleet operation: run pending tenant migrations in every provisioned
/// tenant schema (iterates `public.organizations`, pins `search_path` per
/// schema). Returns one [`TenantMigrationOutcome`] per tenant, in slug
/// order; stops at the first failing tenant with a [`TenantError::Migration`]
/// naming it.
pub fn migrate_all_tenants(
    conn: &mut PgConnection,
) -> Result<Vec<TenantMigrationOutcome>, TenantError> {
    let tenants = list_tenants(conn)?;
    let mut outcomes = Vec::with_capacity(tenants.len());

    for tenant in tenants {
        let schema = tenant_schema_name(&tenant.slug);
        if !tenant.schema_exists {
            return Err(TenantError::Migration {
                slug: tenant.slug,
                message: format!(
                    "schema \"{schema}\" does not exist (organization row present without schema)"
                ),
            });
        }
        sql_query(format!("SET search_path TO \"{schema}\"")).execute(conn)?;
        let applied: Result<Vec<String>, TenantError> = conn
            .run_pending_migrations(TENANT_MIGRATIONS)
            .map(|versions| versions.iter().map(|v| v.to_string()).collect())
            .map_err(|e| TenantError::Migration {
                slug: tenant.slug.clone(),
                message: e.to_string(),
            });
        // Always restore the connection's default search_path, even when a
        // migration failed, before surfacing the error.
        let reset = sql_query("SET search_path TO DEFAULT").execute(conn);
        let applied = applied?;
        reset?;
        outcomes.push(TenantMigrationOutcome {
            slug: tenant.slug,
            schema,
            applied,
        });
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation_matches_s0004_pattern() {
        // valid: starts lowercase, then [a-z0-9_-], total length 2..=63
        for slug in ["acme", "a1", "acme-co", "acme_co", "a23456789"] {
            assert!(is_valid_slug(slug), "{slug:?} should be valid");
        }
        let max = format!("a{}", "b".repeat(62));
        assert!(is_valid_slug(&max), "63 chars is valid");

        // invalid
        let too_long = format!("a{}", "b".repeat(63));
        for slug in [
            "",
            "a",
            "A",
            "ACME",
            "1acme",
            "-acme",
            "_acme",
            "acme co",
            "acme.co",
            "org acme",
            "acme;drop",
            too_long.as_str(),
        ] {
            assert!(!is_valid_slug(slug), "{slug:?} should be invalid");
        }
    }

    #[test]
    fn tenant_schema_name_prefixes_org() {
        assert_eq!(tenant_schema_name("acme"), "org_acme");
    }
}
