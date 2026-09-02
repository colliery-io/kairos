//! Integration test for the seed-demo fixture (KAIROS-T-0035, fixtures per
//! KAIROS-A-0012): `kairos_db::seed_demo` against the real compose
//! Postgres — the same function `kairos-server seed-demo` (and therefore
//! `angreal db seed`) invokes.
//!
//! Covers:
//! - a fresh seed produces the documented inventory (users with the REAL
//!   Dex-shaped external_ids, memberships, teams + delivery boards, stream,
//!   strategy → initiatives → tasks tree, buckets, PRD document from the
//!   prd template, priority metadata, superseded ADR pair, edges);
//! - re-running without force is the typed [`SeedError::AlreadySeeded`]
//!   and CHANGES NOTHING;
//! - `force = true` drops and reseeds (short-code sequences restart);
//! - only the demo tenant is touched: a bystander tenant provisioned
//!   before the forced reseed survives with its data intact.
//!
//! For isolation the test owns the scratch database
//! (`kairos_seed_demo_test`) on the shared compose server.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};

use kairos_db::seed::{DEMO_SLUG, DEMO_USERS, SeedError, demo_tenant_exists};
use kairos_db::{provision_tenant, run_public_migrations, seed_demo};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_seed_demo_test";

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
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count(conn: &mut PgConnection, query: &str) -> i64 {
    sql_query(query)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count query failed: {query}: {e}"))
        .count
}

#[derive(QueryableByName)]
struct TextRow {
    #[diesel(sql_type = Text)]
    value: String,
}

fn text_values(conn: &mut PgConnection, query: &str) -> Vec<String> {
    sql_query(query)
        .load::<TextRow>(conn)
        .unwrap_or_else(|e| panic!("query failed: {query}: {e}"))
        .into_iter()
        .map(|r| r.value)
        .collect()
}

#[test]
fn seed_demo_fixture_lifecycle() {
    // --- scratch database ----------------------------------------------------
    let admin_url = admin_database_url();
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
    run_public_migrations(&mut conn).expect("public migrations");

    // A bystander tenant that must survive the demo lifecycle untouched.
    provision_tenant(&mut conn, "bystander", "Bystander Inc").expect("provisioning bystander");

    // --- fresh seed ------------------------------------------------------------
    assert!(!demo_tenant_exists(&mut conn).expect("existence probe"));
    let report = seed_demo(&mut conn, false).expect("fresh seed succeeds");
    assert_eq!(report.slug, DEMO_SLUG);
    assert_eq!(report.schema, "org_demo");
    assert!(!report.recreated);
    assert!(demo_tenant_exists(&mut conn).expect("existence probe"));

    // Users carry the REAL Dex subs (so first login upserts, not duplicates).
    for (external_id, email, _, _) in DEMO_USERS {
        assert_eq!(
            count(
                &mut conn,
                &format!(
                    "SELECT COUNT(*) AS count FROM public.users \
                     WHERE external_id = '{external_id}' AND email = '{email}'"
                ),
            ),
            1,
            "fixture user {email} with its Dex sub"
        );
    }
    // alice is org admin; bob and carol are members.
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) AS count FROM public.organization_members m \
             JOIN public.organizations o ON o.id = m.organization_id \
             WHERE o.slug = 'demo' AND m.role = 'admin'",
        ),
        1
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) AS count FROM public.organization_members m \
             JOIN public.organizations o ON o.id = m.organization_id \
             WHERE o.slug = 'demo'",
        ),
        3
    );

    // The documented inventory.
    let expectations = [
        ("teams", "SELECT COUNT(*) AS count FROM org_demo.teams", 2),
        (
            "boards (3 provision + 2 delivery)",
            "SELECT COUNT(*) AS count FROM org_demo.boards",
            5,
        ),
        (
            "delivery boards are per-team",
            "SELECT COUNT(*) AS count FROM org_demo.boards \
             WHERE board_level = 'delivery' AND team_id IS NOT NULL",
            2,
        ),
        (
            "team members",
            "SELECT COUNT(*) AS count FROM org_demo.team_members",
            3,
        ),
        (
            "streams",
            "SELECT COUNT(*) AS count FROM org_demo.delivery_streams",
            1,
        ),
        (
            "stream links both teams",
            "SELECT COUNT(*) AS count FROM org_demo.team_delivery_streams",
            2,
        ),
        (
            "strategies",
            "SELECT COUNT(*) AS count FROM org_demo.strategies",
            1,
        ),
        (
            "initiatives",
            "SELECT COUNT(*) AS count FROM org_demo.initiatives",
            4,
        ),
        (
            "buckets (bug + tech_debt)",
            "SELECT COUNT(*) AS count FROM org_demo.initiatives WHERE is_bucket",
            2,
        ),
        ("tasks", "SELECT COUNT(*) AS count FROM org_demo.tasks", 10),
        (
            "bug + tech-debt tasks",
            "SELECT COUNT(*) AS count FROM org_demo.tasks \
             WHERE task_type IN ('bug', 'tech_debt')",
            3,
        ),
        (
            "support-lane tasks (KAIROS-T-0077)",
            "SELECT COUNT(*) AS count FROM org_demo.tasks \
             WHERE work_class = 'support'",
            2,
        ),
        (
            "the no-bug-lane fixture: an unplanned BUG in the Support lane",
            "SELECT COUNT(*) AS count FROM org_demo.tasks \
             WHERE work_class = 'support' AND task_type = 'bug'",
            1,
        ),
        (
            "documents (PRD + the platform runbook, KAIROS-T-0087)",
            "SELECT COUNT(*) AS count FROM org_demo.documents",
            2,
        ),
        (
            "PRD created from the prd template",
            "SELECT COUNT(*) AS count FROM org_demo.documents d \
             JOIN org_demo.templates t ON t.id = d.template_id \
             WHERE t.slug = 'prd'",
            1,
        ),
        ("ADRs", "SELECT COUNT(*) AS count FROM org_demo.adrs", 2),
        (
            "parent edges (strategy->2 initiatives, 10 tasks)",
            "SELECT COUNT(*) AS count FROM org_demo.item_relationships \
             WHERE relationship = 'parent'",
            12,
        ),
        (
            "blocks edges",
            "SELECT COUNT(*) AS count FROM org_demo.item_relationships \
             WHERE relationship = 'blocks'",
            2,
        ),
        (
            "supports edges (initiative -> PRD, platform task -> runbook)",
            "SELECT COUNT(*) AS count FROM org_demo.item_relationships \
             WHERE relationship = 'supports'",
            2,
        ),
        (
            "team pages (2 scaffolds of 10 + platform's how-to page)",
            "SELECT COUNT(*) AS count FROM org_demo.team_pages \
             WHERE deleted_at IS NULL",
            21,
        ),
        (
            "charter content saves wrote history (baseline + v2, both teams)",
            "SELECT COUNT(*) AS count FROM org_demo.team_page_history h \
             JOIN org_demo.team_pages p ON p.id = h.page_id \
             WHERE p.slug = 'charter'",
            4,
        ),
        (
            "team announcements (platform pinned + web)",
            "SELECT COUNT(*) AS count FROM org_demo.team_announcements",
            2,
        ),
        (
            "exactly one pinned announcement",
            "SELECT COUNT(*) AS count FROM org_demo.team_announcements WHERE pinned",
            1,
        ),
        (
            "forge connections (one per team, KAIROS-T-0102)",
            "SELECT COUNT(*) AS count FROM org_demo.forge_connections \
             WHERE deleted_at IS NULL",
            2,
        ),
        (
            "both forges represented",
            "SELECT COUNT(DISTINCT forge) AS count FROM org_demo.forge_connections",
            2,
        ),
        (
            "every connection is attributed to a team",
            "SELECT COUNT(*) AS count FROM org_demo.forge_connections \
             WHERE team_id IS NOT NULL",
            2,
        ),
        (
            "item links covering open/merged/draft plus a branch",
            "SELECT COUNT(*) AS count FROM org_demo.item_links",
            4,
        ),
        (
            "every link state the panels render is present",
            "SELECT COUNT(DISTINCT state) AS count FROM org_demo.item_links",
            3,
        ),
        (
            "one branch link (the rest are pull requests)",
            "SELECT COUNT(*) AS count FROM org_demo.item_links WHERE kind = 'branch'",
            1,
        ),
        (
            "supersedes edge (ADR chain)",
            "SELECT COUNT(*) AS count FROM org_demo.item_relationships \
             WHERE relationship = 'supersedes'",
            1,
        ),
        (
            "priority stamps",
            "SELECT COUNT(*) AS count FROM org_demo.item_metadata im \
             JOIN org_demo.metadata_definitions md ON md.id = im.metadata_definition_id \
             WHERE md.slug = 'priority'",
            6,
        ),
        (
            "tasks landed in several columns",
            "SELECT COUNT(DISTINCT bc.name) AS count FROM org_demo.tasks t \
             JOIN org_demo.board_columns bc ON bc.id = t.column_id",
            4,
        ),
    ];
    for (label, query, expected) in expectations {
        assert_eq!(count(&mut conn, query), expected, "{label}");
    }

    // Boards LOOK real: the strategy sits in Active, the superseded ADR in
    // Superseded.
    let strategy_columns = text_values(
        &mut conn,
        "SELECT bc.name AS value FROM org_demo.strategies s \
         JOIN org_demo.board_columns bc ON bc.id = s.column_id",
    );
    assert_eq!(strategy_columns, ["Active"]);
    let adr_columns = text_values(
        &mut conn,
        "SELECT bc.name AS value FROM org_demo.adrs a \
         JOIN org_demo.board_columns bc ON bc.id = a.column_id \
         ORDER BY a.short_code",
    );
    assert_eq!(adr_columns, ["Superseded", "Decided"]);

    // --- idempotency: re-run without force refuses and changes nothing --------
    let tasks_before = count(&mut conn, "SELECT COUNT(*) AS count FROM org_demo.tasks");
    match seed_demo(&mut conn, false) {
        Err(SeedError::AlreadySeeded) => {}
        other => panic!("re-seed without force must be AlreadySeeded, got {other:?}"),
    }
    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) AS count FROM org_demo.tasks"),
        tasks_before,
        "refused seed changed nothing"
    );

    // --- force: drop + reseed ---------------------------------------------------
    let report = seed_demo(&mut conn, true).expect("forced reseed succeeds");
    assert!(report.recreated);
    // Fresh schema: short-code sequences restarted at 0001.
    assert!(
        report.short_codes.contains(&"DEMO-T-0001".to_string()),
        "forced reseed restarts short codes: {:?}",
        report.short_codes
    );
    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) AS count FROM org_demo.tasks"),
        10
    );
    // Users were upserted, not duplicated.
    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) AS count FROM public.users"),
        3
    );

    // The bystander tenant is untouched.
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) AS count FROM public.organizations WHERE slug = 'bystander'",
        ),
        1
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) AS count FROM org_bystander.boards",
        ),
        3,
        "bystander provisioning-default boards survive the demo reseed"
    );

    // --- teardown ----------------------------------------------------------------
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
