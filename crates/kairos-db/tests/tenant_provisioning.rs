//! Integration test for tenant provisioning and fleet migration
//! (KAIROS-T-0008, DDL per KAIROS-S-0004, defaults per KAIROS-A-0002/0003).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration` — the database is never mocked, per
//! KAIROS-A-0012). Connection details come from `DATABASE_URL` if set,
//! otherwise the same default the angreal tooling uses.
//!
//! For isolation the test drops and recreates a dedicated scratch database
//! (`kairos_tenant_provisioning_test`) on the same server.
//!
//! Interpretations under test (recorded in KAIROS-T-0008):
//! - S-0004's DDL defines exactly 21 tenant tables + 2 views + 5 sequences
//!   (the spec header's "22 tables" is an off-by-one in the spec, not here).
//!   KAIROS-T-0025 adds `scim_tokens` (A-0016 SCIM provisioning) and
//!   KAIROS-T-0057 adds `api_keys` (A-0017 service-account API keys),
//!   bringing the tenant tree to 23 tables.
//! - Provisioning creates the strategy, initiative, and ADR boards only;
//!   delivery boards are per-team and created later, but all four
//!   `system_board_defaults` rows are seeded.

use std::collections::BTreeSet;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::Text;

use kairos_db::tenant::TenantError;
use kairos_db::{drop_tenant, migrate_all_tenants, provision_tenant, run_public_migrations};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_tenant_provisioning_test";

/// The tenant tables (sorted): the 21 from the KAIROS-S-0004 DDL plus
/// `scim_tokens` (KAIROS-T-0025 / A-0016) and `api_keys` (KAIROS-T-0057 /
/// A-0017 service-account API keys).
const EXPECTED_TABLES: [&str; 29] = [
    "activity_log",
    "adrs",
    "api_keys",
    "board_columns",
    "board_member_capabilities",
    "board_transitions",
    "boards",
    "delivery_streams",
    "documents",
    "forge_connections",
    "initiatives",
    "item_history",
    "item_links",
    "item_metadata",
    "item_relationships",
    "metadata_definition_scopes",
    "metadata_definitions",
    "metadata_enum_options",
    "scim_tokens",
    "strategies",
    "tasks",
    "team_announcements",
    "team_delivery_streams",
    "team_members",
    "team_page_history",
    "team_pages",
    "teams",
    "template_metadata",
    "templates",
];

const EXPECTED_VIEWS: [&str; 2] = ["entity_directory", "searchable_items"];

const EXPECTED_SEQUENCES: [&str; 5] = [
    "seq_adr_code",
    "seq_document_code",
    "seq_initiative_code",
    "seq_strategy_code",
    "seq_task_code",
];

/// Every named index in the S-0004 tenant DDL (partial + GIN included).
const EXPECTED_INDEXES: [&str; 21] = [
    "idx_activity_log_actor",
    "idx_activity_log_entity",
    "idx_activity_log_time",
    "idx_adrs_tsv",
    "idx_board_member_cap_board_user",
    "idx_documents_tsv",
    "idx_initiatives_board",
    "idx_initiatives_column",
    "idx_initiatives_tsv",
    "idx_item_history_item",
    "idx_item_metadata_definition",
    "idx_item_metadata_item",
    "idx_item_relationships_source",
    "idx_item_relationships_target",
    "idx_strategies_board",
    "idx_strategies_column",
    "idx_strategies_tsv",
    "idx_tasks_board",
    "idx_tasks_column",
    "idx_tasks_team",
    "idx_tasks_tsv",
];

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

#[derive(QueryableByName)]
struct NameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

fn names(conn: &mut PgConnection, query: &str, param: &str) -> Vec<String> {
    let mut rows: Vec<String> = sql_query(query)
        .bind::<Text, _>(param)
        .load::<NameRow>(conn)
        .unwrap_or_else(|e| panic!("query failed: {query}: {e}"))
        .into_iter()
        .map(|r| r.name)
        .collect();
    rows.sort();
    rows
}

fn schema_tables(conn: &mut PgConnection, schema: &str) -> Vec<String> {
    names(
        conn,
        "SELECT table_name::text AS name FROM information_schema.tables \
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
           AND table_name <> '__diesel_schema_migrations'",
        schema,
    )
}

fn schema_views(conn: &mut PgConnection, schema: &str) -> Vec<String> {
    names(
        conn,
        "SELECT table_name::text AS name FROM information_schema.views WHERE table_schema = $1",
        schema,
    )
}

fn schema_sequences(conn: &mut PgConnection, schema: &str) -> Vec<String> {
    names(
        conn,
        "SELECT sequence_name::text AS name FROM information_schema.sequences \
         WHERE sequence_schema = $1",
        schema,
    )
}

fn schema_indexes(conn: &mut PgConnection, schema: &str) -> Vec<String> {
    names(
        conn,
        "SELECT indexname::text AS name FROM pg_indexes WHERE schemaname = $1",
        schema,
    )
}

fn schema_exists(conn: &mut PgConnection, schema: &str) -> bool {
    let rows = names(
        conn,
        "SELECT nspname::text AS name FROM pg_namespace WHERE nspname = $1",
        schema,
    );
    !rows.is_empty()
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count query failed: {sql}: {e}"))
        .count
}

/// Board columns (ordered by position) for a board slug in `org_acme`.
fn board_columns(conn: &mut PgConnection, board_slug: &str) -> Vec<String> {
    sql_query(
        "SELECT c.name::text AS name FROM org_acme.board_columns c \
         JOIN org_acme.boards b ON b.id = c.board_id \
         WHERE b.slug = $1 ORDER BY c.position",
    )
    .bind::<Text, _>(board_slug)
    .load::<NameRow>(conn)
    .expect("loading board columns")
    .into_iter()
    .map(|r| r.name)
    .collect()
}

/// Transition pairs `"From -> To"` for a board slug in `org_acme`.
fn board_transitions(conn: &mut PgConnection, board_slug: &str) -> BTreeSet<String> {
    sql_query(
        "SELECT (f.name || ' -> ' || t.name)::text AS name \
         FROM org_acme.board_transitions tr \
         JOIN org_acme.boards b ON b.id = tr.board_id \
         JOIN org_acme.board_columns f ON f.id = tr.from_column_id \
         JOIN org_acme.board_columns t ON t.id = tr.to_column_id \
         WHERE b.slug = $1",
    )
    .bind::<Text, _>(board_slug)
    .load::<NameRow>(conn)
    .expect("loading board transitions")
    .into_iter()
    .map(|r| r.name)
    .collect()
}

fn transitions(pairs: &[(&str, &str)]) -> BTreeSet<String> {
    pairs.iter().map(|(f, t)| format!("{f} -> {t}")).collect()
}

#[test]
fn tenant_provisioning_lifecycle() {
    let admin_url = admin_database_url();

    // Fresh scratch database for isolation.
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");

    let scratch_url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    // Public migrations first (organizations + system_* tables).
    run_public_migrations(&mut conn).expect("running public migrations");

    // ---- provision `acme` -------------------------------------------------
    let report = provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    assert_eq!(report.schema, "org_acme");
    assert!(
        !report.migrations_applied.is_empty(),
        "first provision should apply the tenant migration tree"
    );
    assert_eq!(report.boards_created, ["strategy", "initiatives", "adrs"]);
    assert_eq!(
        report.templates_copied, 6,
        "A-0003 ships 6 system templates"
    );
    assert_eq!(
        report.metadata_definitions_copied, 3,
        "A-0003 ships 3 system metadata definitions (KAIROS-T-0078 retired \
         'Document status' — lifecycle is a documents column)"
    );

    // Every S-0004 tenant table / view / sequence / index exists in org_acme.
    assert_eq!(
        schema_tables(&mut conn, "org_acme"),
        EXPECTED_TABLES,
        "org_acme should contain exactly the expected tenant tables"
    );
    assert_eq!(schema_views(&mut conn, "org_acme"), EXPECTED_VIEWS);
    assert_eq!(schema_sequences(&mut conn, "org_acme"), EXPECTED_SEQUENCES);
    let indexes = schema_indexes(&mut conn, "org_acme");
    for index in EXPECTED_INDEXES {
        assert!(
            indexes.contains(&index.to_string()),
            "missing index {index} in org_acme"
        );
    }

    // system_board_defaults seeded with all FOUR level configs (A-0002).
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM public.system_board_defaults"
        ),
        4
    );

    // Boards: strategy/initiative/adr present, NO delivery board at
    // provision time (delivery boards are per-team; created with teams).
    assert_eq!(
        names(
            &mut conn,
            "SELECT board_level::text AS name FROM org_acme.boards WHERE board_level <> $1",
            "__none__",
        ),
        ["adr", "initiative", "strategy"],
        "provisioning creates exactly the strategy, initiative, and adr boards"
    );

    // Columns and transitions match the A-0002 defaults exactly.
    assert_eq!(
        board_columns(&mut conn, "strategy"),
        ["Draft", "Review", "Active", "Monitoring", "Completed"]
    );
    assert_eq!(
        board_transitions(&mut conn, "strategy"),
        transitions(&[
            ("Draft", "Review"),
            ("Review", "Active"),
            ("Active", "Monitoring"),
            ("Monitoring", "Completed"),
        ])
    );
    assert_eq!(
        board_columns(&mut conn, "initiatives"),
        [
            "Discovery",
            "Design",
            "Ready",
            "Decompose",
            "Active",
            "Monitoring",
            "Completed"
        ]
    );
    assert_eq!(
        board_transitions(&mut conn, "initiatives"),
        transitions(&[
            ("Discovery", "Design"),
            ("Design", "Ready"),
            ("Ready", "Decompose"),
            ("Decompose", "Active"),
            ("Active", "Monitoring"),
            ("Monitoring", "Completed"),
        ])
    );
    assert_eq!(
        board_columns(&mut conn, "adrs"),
        ["Draft", "Discussion", "Decided", "Superseded"]
    );
    assert_eq!(
        board_transitions(&mut conn, "adrs"),
        transitions(&[
            ("Draft", "Discussion"),
            ("Discussion", "Decided"),
            ("Decided", "Superseded"),
        ])
    );

    // Templates / metadata copied from the system defaults (A-0003).
    assert_eq!(
        names(
            &mut conn,
            "SELECT slug::text AS name FROM org_acme.templates WHERE is_system_default AND slug <> $1",
            "__none__",
        ),
        [
            "architecture_framing",
            "company_vision",
            "prd",
            "social_contract",
            "system_context",
            "team_charter",
        ]
    );
    assert_eq!(
        names(
            &mut conn,
            "SELECT slug::text AS name FROM org_acme.metadata_definitions \
             WHERE is_system_default AND slug <> $1",
            "__none__",
        ),
        ["complexity", "document_type", "priority"]
    );
    // KAIROS-T-0078: entity-type scopes travel with the definitions —
    // document_type is documents-only, complexity excludes initiatives.
    assert_eq!(
        names(
            &mut conn,
            "SELECT md.slug || ':' || s.entity_type AS name \
             FROM org_acme.metadata_definition_scopes s \
             JOIN org_acme.metadata_definitions md ON md.id = s.metadata_definition_id \
             WHERE md.slug <> $1 ORDER BY name",
            "__none__",
        ),
        [
            "complexity:adr",
            "complexity:document",
            "complexity:strategy",
            "complexity:task",
            "document_type:document",
        ]
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.metadata_enum_options"
        ),
        15
    );
    assert_eq!(
        count(&mut conn, "SELECT count(*) FROM org_acme.template_metadata"),
        6,
        "one document_type association per template ('status' retired)"
    );

    // ---- provisioning an existing slug: typed error, no partial state -----
    let err = provision_tenant(&mut conn, "acme", "Acme Again")
        .expect_err("re-provisioning acme must fail");
    assert!(
        matches!(&err, TenantError::AlreadyExists(slug) if slug == "acme"),
        "expected TenantError::AlreadyExists(\"acme\"), got {err:?}"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM public.organizations WHERE slug = 'acme'"
        ),
        1,
        "no duplicate organization row after failed re-provision"
    );

    // Invalid slug is also a typed error, before touching the database.
    let err = provision_tenant(&mut conn, "ACME", "Shouty").expect_err("invalid slug must fail");
    assert!(matches!(&err, TenantError::InvalidSlug(slug) if slug == "ACME"));

    // ---- fleet migration across 3 tenants ---------------------------------
    provision_tenant(&mut conn, "widgets", "Widgets LLC").expect("provisioning widgets");
    provision_tenant(&mut conn, "globex", "Globex Corp").expect("provisioning globex");

    // Marker row in acme: fleet migration must not disturb tenant data.
    sql_query(
        "INSERT INTO org_acme.teams (name, slug, team_type) \
         VALUES ('Platform', 'platform', 'platform')",
    )
    .execute(&mut conn)
    .expect("inserting marker team into acme");

    let outcomes = migrate_all_tenants(&mut conn).expect("fleet migration");
    let summary: Vec<(String, usize)> = outcomes
        .iter()
        .map(|o| (o.slug.clone(), o.applied.len()))
        .collect();
    assert_eq!(
        summary,
        [
            ("acme".to_string(), 0),
            ("globex".to_string(), 0),
            ("widgets".to_string(), 0),
        ],
        "all three tenants report up-to-date (no pending tenant migrations)"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.teams WHERE slug = 'platform'"
        ),
        1,
        "marker row survives fleet migration"
    );

    // ---- fleet migration applies a NEW migration to an EXISTING tenant ----
    // (KAIROS-T-0025 pattern check: the migrate-tenants path is how already
    // provisioned schemas pick up later tenant migrations.) Simulate a tenant
    // that predates the NEWEST tenant migration (currently `forge_links`,
    // KAIROS-T-0097): revert its DDL and drop its bookkeeping row in
    // widgets only, then fleet-migrate and expect exactly that one
    // migration to re-apply.
    //
    // NOTE: this block is hand-re-pinned to the newest migration on every
    // schema wave — the recurring maintenance chore KAIROS-T-0093 exists
    // to remove by deriving the target from the embedded migration list.
    sql_query("DROP TABLE org_widgets.item_links")
        .execute(&mut conn)
        .expect("dropping item links in widgets to simulate an old tenant");
    sql_query("DROP TABLE org_widgets.forge_connections")
        .execute(&mut conn)
        .expect("dropping forge connections in widgets to simulate an old tenant");
    sql_query(
        "DELETE FROM org_widgets.__diesel_schema_migrations \
         WHERE version = (SELECT max(version) FROM org_widgets.__diesel_schema_migrations)",
    )
    .execute(&mut conn)
    .expect("deleting the newest migration bookkeeping row in widgets");

    let outcomes = migrate_all_tenants(&mut conn).expect("fleet migration (upgrade path)");
    let summary: Vec<(String, usize)> = outcomes
        .iter()
        .map(|o| (o.slug.clone(), o.applied.len()))
        .collect();
    assert_eq!(
        summary,
        [
            ("acme".to_string(), 0),
            ("globex".to_string(), 0),
            ("widgets".to_string(), 1),
        ],
        "only the out-of-date tenant applies the pending migration"
    );
    assert_eq!(
        count(&mut conn, "SELECT count(*) FROM org_widgets.scim_tokens"),
        0,
        "scim_tokens exists (and is empty) in widgets after the fleet upgrade"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_schema = 'org_widgets' AND table_name = 'item_links'"
        ),
        1,
        "item_links is back in widgets after the fleet upgrade"
    );

    // ---- drop-tenant -------------------------------------------------------
    // Without --confirm: refused, nothing removed.
    let err = drop_tenant(&mut conn, "widgets", false).expect_err("drop without confirm must fail");
    assert!(matches!(&err, TenantError::ConfirmationRequired(slug) if slug == "widgets"));
    assert!(schema_exists(&mut conn, "org_widgets"));
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM public.organizations WHERE slug = 'widgets'"
        ),
        1
    );

    // With confirm: schema gone, org row gone, acme untouched.
    drop_tenant(&mut conn, "widgets", true).expect("dropping widgets with confirm");
    assert!(!schema_exists(&mut conn, "org_widgets"));
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM public.organizations WHERE slug = 'widgets'"
        ),
        0
    );
    assert!(
        schema_exists(&mut conn, "org_acme"),
        "acme schema untouched by dropping widgets"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.teams WHERE slug = 'platform'"
        ),
        1,
        "acme data untouched by dropping widgets"
    );

    // Dropping a non-existent tenant is a typed NotFound.
    let err = drop_tenant(&mut conn, "widgets", true).expect_err("double drop must fail");
    assert!(matches!(&err, TenantError::NotFound(slug) if slug == "widgets"));

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
