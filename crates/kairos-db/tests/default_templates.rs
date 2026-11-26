//! KAIROS-T-0790 (COLLIERY-I-0683, KAIROS-A-0087): the default templates.
//!
//! - A new organization gets each default template, with the text of the
//!   files of `kairos_db::default_templates`, its type and its use_when.
//! - The tenant migration `templates_with_guidance` is an in-place upgrade:
//!   a system template with the old seed content gets the new content; a
//!   changed one keeps its content (and gets a use_when); a missing default
//!   type is added. The migration can run 2 times.
//! - The public migration `system_templates_with_guidance` does the same to
//!   `public.system_templates`.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Text};

use kairos_db::default_templates::{DEFAULT_TEMPLATES, DefaultTemplate};
use kairos_db::{provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migrations under test, verbatim.
const TENANT_UP: &str =
    include_str!("../migrations/tenant/2026-11-25-000200_templates_with_guidance/up.sql");
const PUBLIC_UP: &str =
    include_str!("../migrations/public/2026-11-25-000200_system_templates_with_guidance/up.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// A scratch database with the tenant `colliery`, pinned to its schema.
fn tenant(scratch_db: &str) -> (PgConnection, PgConnection, String) {
    let scratch_db = scratch::scratch_db_name(scratch_db);
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {scratch_db}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let mut conn = PgConnection::establish(&with_database(&admin_url, &scratch_db))
        .expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning colliery");
    sql_query("SET search_path TO org_colliery, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    (admin_conn, conn, scratch_db)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

#[derive(QueryableByName, Debug, PartialEq)]
struct Row {
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Text)]
    item_type: String,
    #[diesel(sql_type = Nullable<Text>)]
    task_type: Option<String>,
    #[diesel(sql_type = Text)]
    use_when: String,
}

fn row(conn: &mut PgConnection, table: &str, slug: &str) -> Option<Row> {
    sql_query(format!(
        "SELECT content, item_type, task_type, use_when FROM {table} WHERE slug = $1"
    ))
    .bind::<Text, _>(slug)
    .get_result(conn)
    .optional()
    .expect("reading a template")
}

fn expected(template: &DefaultTemplate) -> Row {
    Row {
        content: template.content.to_string(),
        item_type: template.item_type.to_string(),
        task_type: template.task_type.map(str::to_string),
        use_when: template.use_when.to_string(),
    }
}

fn by_slug(slug: &str) -> &'static DefaultTemplate {
    DEFAULT_TEMPLATES
        .iter()
        .find(|t| t.slug == slug)
        .unwrap_or_else(|| panic!("no default template {slug}"))
}

/// A new organization has each default template, as the files say.
#[test]
fn a_new_organization_gets_each_default_template() {
    let (admin_conn, mut conn, db) = tenant("default_templates_new");
    for template in DEFAULT_TEMPLATES {
        for table in ["templates", "public.system_templates"] {
            assert_eq!(
                row(&mut conn, table, template.slug),
                Some(expected(&template)),
                "{table}: {}",
                template.slug
            );
        }
    }
    drop_database(admin_conn, conn, &db);
}

/// The tenant upgrade: the old seed content is replaced, a changed template
/// stays, a missing type is added, and a second run changes nothing.
#[test]
fn the_tenant_upgrade_replaces_only_unchanged_templates() {
    let (admin_conn, mut conn, db) = tenant("default_templates_upgrade");
    // The state of a tenant before the migration: the old seed content, no
    // use_when, a template that an admin changed, and no task templates.
    let prd = by_slug("prd");
    let charter = by_slug("team_charter");
    sql_query("UPDATE templates SET content = $1, use_when = '' WHERE slug = 'prd'")
        .bind::<Text, _>(prd.replaces.expect("prd replaces the old seed"))
        .execute(&mut conn)
        .expect("old prd");
    sql_query(
        "UPDATE templates SET content = '## Our charter\n', use_when = '' \
         WHERE slug = 'team_charter'",
    )
    .execute(&mut conn)
    .expect("changed charter");
    sql_query("DELETE FROM templates WHERE item_type = 'task'")
        .execute(&mut conn)
        .expect("no task templates");

    for _ in 0..2 {
        conn.batch_execute(TENANT_UP).expect("the tenant migration");
        assert_eq!(row(&mut conn, "templates", "prd"), Some(expected(prd)));
        let changed = row(&mut conn, "templates", "team_charter").expect("charter");
        assert_eq!(changed.content, "## Our charter\n", "a changed one stays");
        assert_eq!(changed.use_when, charter.use_when, "it gets a use_when");
        for slug in ["task", "bug", "tech_debt", "support"] {
            assert_eq!(
                row(&mut conn, "templates", slug),
                Some(expected(by_slug(slug))),
                "{slug} added"
            );
        }
    }
    drop_database(admin_conn, conn, &db);
}

/// The public upgrade does the same to `public.system_templates`.
#[test]
fn the_public_upgrade_replaces_only_unchanged_templates() {
    let (admin_conn, mut conn, db) = tenant("default_templates_public");
    let vision = by_slug("company_vision");
    sql_query(
        "UPDATE public.system_templates SET content = $1, use_when = '' \
         WHERE slug = 'company_vision'",
    )
    .bind::<Text, _>(vision.replaces.expect("the old seed"))
    .execute(&mut conn)
    .expect("old vision");
    sql_query(
        "UPDATE public.system_templates SET content = '## Ours\n' WHERE slug = 'social_contract'",
    )
    .execute(&mut conn)
    .expect("changed contract");
    sql_query("DELETE FROM public.system_templates WHERE slug = 'runbook'")
        .execute(&mut conn)
        .expect("no runbook");

    conn.batch_execute(PUBLIC_UP).expect("the public migration");
    assert_eq!(
        row(&mut conn, "public.system_templates", "company_vision"),
        Some(expected(vision))
    );
    assert_eq!(
        row(&mut conn, "public.system_templates", "social_contract")
            .expect("contract")
            .content,
        "## Ours\n"
    );
    assert_eq!(
        row(&mut conn, "public.system_templates", "runbook"),
        Some(expected(by_slug("runbook")))
    );
    drop_database(admin_conn, conn, &db);
}
