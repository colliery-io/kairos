//! COLLIERY-T-0269 — the migration `document_board_and_impacts`, on the
//! data of a tenant.
//!
//! The migration runs later on tenants with real data, so the test is about
//! what it does to data:
//!
//! - no row of a table that the tenant has is changed or deleted,
//! - each document names no board after the migration,
//! - the tenant gets the template "Product Vision" one time, with the
//!   metadata default `document_type = vision`,
//! - a tenant that has a template with that name, or with that slug, keeps
//!   its own, and gets no second one,
//! - a second run changes nothing.
//!
//! The state of a tenant before the migration is made with the down
//! migration: no column `documents.board_id`, no table `item_impacts`. The
//! template is removed by SQL, because the down migration keeps it.
//!
//! COLLIERY-T-3109 made the owner board of a document required, and the
//! code of today reads each document with a board. So the test no longer
//! asks the code for the owner and the editors of a document in the state
//! between the two migrations: the code cannot read that state. The SQL of
//! the migration is checked as before. The data of the test gets a board
//! when it is made, and the down migration removes the column.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text};
use uuid::Uuid;

use kairos_db::models::enums::{BoardLevel, RelationshipType};
use kairos_db::{
    CreateDocument, CreateInitiative, create_document, create_initiative, link_items,
    migrate_all_tenants, provision_tenant, run_public_migrations, soft_delete_item,
};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const UP: &str =
    include_str!("../migrations/tenant/2026-09-29-000000_document_board_and_impacts/up.sql");
const DOWN: &str =
    include_str!("../migrations/tenant/2026-09-29-000000_document_board_and_impacts/down.sql");
/// The version of the migration in `__diesel_schema_migrations`.
const VERSION: &str = "20260929000000";

/// The text of the template, as a person reads it.
const PRODUCT_VISION: &str = "# Product Vision\n\n## Purpose\n\n## Who It Is For\n\n\
     ## Current State\n\n## Future State\n\n## Principles\n\n## What It Is Not\n";

/// The tables of a tenant that the migration must not change. `documents`
/// is compared with no `board_id`, which the old state does not have.
const UNCHANGED: &[&str] = &[
    "documents",
    "adrs",
    "initiatives",
    "item_relationships",
    "item_metadata",
    "item_history",
    "activity_log",
    "boards",
    "board_columns",
    "board_member_capabilities",
    "metadata_definitions",
    "metadata_enum_options",
];

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

fn pin(conn: &mut PgConnection) {
    sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("pinning search_path");
}

/// A scratch database with the tenant `acme`, pinned to its schema.
fn tenant(scratch_db: &str) -> (PgConnection, PgConnection) {
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
    let scratch_url = with_database(&admin_url, scratch_db);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    pin(&mut conn);
    (admin_conn, conn)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .count
}

#[derive(QueryableByName)]
struct Snapshot {
    #[diesel(sql_type = Text)]
    rows: String,
}

/// Each row of a table, with each column, as text. Two equal snapshots
/// are two equal tables.
fn snapshot(conn: &mut PgConnection, table: &str) -> String {
    sql_query(format!(
        "SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text)::text, '[]') AS rows \
           FROM {table} t"
    ))
    .get_result::<Snapshot>(conn)
    .unwrap_or_else(|e| panic!("reading {table}: {e}"))
    .rows
}

/// [`snapshot`] of `documents` with no `board_id`: the column is not there
/// before the migration.
fn documents_snapshot(conn: &mut PgConnection) -> String {
    sql_query(
        "SELECT coalesce(jsonb_agg(to_jsonb(t) - 'board_id' ORDER BY t.id)::text, '[]') AS rows \
           FROM documents t",
    )
    .get_result::<Snapshot>(conn)
    .expect("reading documents")
    .rows
}

fn snapshots(conn: &mut PgConnection) -> Vec<(String, String)> {
    UNCHANGED
        .iter()
        .map(|table| {
            let rows = if *table == "documents" {
                documents_snapshot(conn)
            } else {
                snapshot(conn, table)
            };
            (table.to_string(), rows)
        })
        .collect()
}

/// The state of a tenant before the migration: no column, no table, no
/// template, and no record that the migration ran.
fn make_the_old_state(conn: &mut PgConnection) {
    conn.batch_execute(DOWN).expect("the down migration");
    sql_query("DELETE FROM templates WHERE slug = 'product_vision'")
        .execute(conn)
        .expect("removing the template");
    let removed = sql_query(format!(
        "DELETE FROM __diesel_schema_migrations WHERE version = '{VERSION}'"
    ))
    .execute(conn)
    .expect("removing the record of the migration");
    assert_eq!(removed, 1, "the migration has the version {VERSION}");
    assert!(!column_is_there(conn), "the old state has no board_id");
    assert!(!table_is_there(conn), "the old state has no item_impacts");
}

fn column_is_there(conn: &mut PgConnection) -> bool {
    count(
        conn,
        "SELECT count(*) AS count FROM information_schema.columns \
          WHERE table_schema = 'org_acme' AND table_name = 'documents' \
            AND column_name = 'board_id'",
    ) == 1
}

fn table_is_there(conn: &mut PgConnection) -> bool {
    count(
        conn,
        "SELECT count(*) AS count FROM information_schema.tables \
          WHERE table_schema = 'org_acme' AND table_name = 'item_impacts'",
    ) == 1
}

#[derive(QueryableByName, Debug, PartialEq)]
struct TemplateRow {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    is_system_default: bool,
    /// The metadata defaults of the template: `slug=value`, sorted.
    #[diesel(sql_type = Nullable<Text>)]
    defaults: Option<String>,
}

/// The templates that have the name or the slug of the new template.
fn product_vision_templates(conn: &mut PgConnection) -> Vec<TemplateRow> {
    sql_query(
        "SELECT t.name, t.slug, t.content, t.is_system_default, \
                (SELECT string_agg(d.slug || '=' || coalesce(m.default_value, ''), ',' \
                                   ORDER BY d.slug) \
                   FROM template_metadata m \
                   JOIN metadata_definitions d ON d.id = m.metadata_definition_id \
                  WHERE m.template_id = t.id) AS defaults \
           FROM templates t \
          WHERE t.name = 'Product Vision' OR t.slug = 'product_vision' \
          ORDER BY t.created_at, t.slug",
    )
    .load(conn)
    .expect("reading the templates")
}

fn board_of_level(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::board_level.eq(level))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .expect("the board of the level")
}

/// The documents of the old data.
struct OldData {
    initiative_board: Uuid,
}

fn old_data(conn: &mut PgConnection) -> OldData {
    let author = Uuid::new_v4();
    let initiative_board = board_of_level(conn, BoardLevel::Initiative);
    let initiative = create_initiative(
        conn,
        CreateInitiative {
            board_id: initiative_board,
            column_id: None,
            title: "An initiative",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        author,
    )
    .expect("the initiative");
    let mut document = |title: &str, parent: bool| {
        let created = create_document(
            conn,
            CreateDocument {
                board_id: initiative_board,
                title,
                content: Some("the text of the document"),
                template_id: None,
            },
            author,
        )
        .expect("the document");
        if parent {
            link_items(
                conn,
                initiative.id,
                created.id,
                RelationshipType::Supports,
                author,
            )
            .expect("the supports edge");
        }
        created.id
    };
    document("A document with a parent", true);
    let archived = document("An archived document", true);
    document("A document with no parent", false);
    soft_delete_item(
        conn,
        kairos_core::short_code::ItemType::Document,
        archived,
        author,
    )
    .expect("the archive");
    OldData { initiative_board }
}

#[test]
fn the_migration_changes_no_row() {
    const SCRATCH_DB: &str = "kairos_document_board_t0269_data_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);

    let data = old_data(&mut conn);
    make_the_old_state(&mut conn);
    let before = snapshots(&mut conn);
    let templates_before = count(&mut conn, "SELECT count(*) AS count FROM templates");
    assert!(product_vision_templates(&mut conn).is_empty());

    // ------------------------------------------------------------------
    // The migration, by the function that a deployment calls
    // ------------------------------------------------------------------
    let outcomes = migrate_all_tenants(&mut conn).expect("the migration passes on old data");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].applied, [VERSION], "{:?}", outcomes[0]);
    pin(&mut conn);

    assert!(column_is_there(&mut conn));
    assert!(table_is_there(&mut conn));
    // No row of the tenant is changed or deleted.
    for ((table, rows_before), (_, rows_after)) in before.iter().zip(snapshots(&mut conn)) {
        assert_eq!(&rows_after, rows_before, "the table {table} is changed");
    }
    // Each document names no board, the archived one too.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS count FROM documents WHERE board_id IS NOT NULL"
        ),
        0
    );
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS count FROM documents"),
        3
    );
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS count FROM item_impacts"),
        0
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS count FROM documents WHERE board_id = '{}'",
                data.initiative_board
            )
        ),
        0,
        "no document names a board"
    );

    // The template is added, one time.
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS count FROM templates"),
        templates_before + 1
    );
    assert_eq!(
        product_vision_templates(&mut conn),
        [TemplateRow {
            name: "Product Vision".into(),
            slug: "product_vision".into(),
            content: PRODUCT_VISION.into(),
            is_system_default: true,
            defaults: Some("document_type=vision".into()),
        }]
    );

    // ------------------------------------------------------------------
    // A second run changes nothing
    // ------------------------------------------------------------------
    let once = snapshots(&mut conn);
    let templates_once = snapshot(&mut conn, "templates");
    let defaults_once = snapshot(&mut conn, "template_metadata");
    conn.batch_execute(UP).expect("the second run passes");
    assert_eq!(snapshots(&mut conn), once);
    assert_eq!(snapshot(&mut conn, "templates"), templates_once);
    assert_eq!(snapshot(&mut conn, "template_metadata"), defaults_once);
    let outcomes = migrate_all_tenants(&mut conn).expect("no migration is pending");
    assert!(outcomes[0].applied.is_empty(), "{:?}", outcomes[0]);
    pin(&mut conn);

    drop_database(admin_conn, conn, SCRATCH_DB);
}

#[test]
fn a_tenant_keeps_its_own_product_vision_template() {
    const SCRATCH_DB: &str = "kairos_document_board_t0269_template_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);
    make_the_old_state(&mut conn);

    // The tenant made a template with the name, and gave it a default of
    // its own.
    sql_query(
        "INSERT INTO templates (name, slug, content, is_system_default) \
         VALUES ('Product Vision', 'our-vision', '# The vision of Acme', false)",
    )
    .execute(&mut conn)
    .expect("the template of the tenant");
    sql_query(
        "INSERT INTO template_metadata (template_id, metadata_definition_id, default_value) \
         SELECT t.id, d.id, 'high' FROM templates t, metadata_definitions d \
          WHERE t.slug = 'our-vision' AND d.slug = 'priority'",
    )
    .execute(&mut conn)
    .expect("the default of the tenant");
    let own = TemplateRow {
        name: "Product Vision".into(),
        slug: "our-vision".into(),
        content: "# The vision of Acme".into(),
        is_system_default: false,
        defaults: Some("priority=high".into()),
    };
    let templates = snapshot(&mut conn, "templates");
    let defaults = snapshot(&mut conn, "template_metadata");

    conn.batch_execute(UP).expect("the migration passes");
    assert_eq!(product_vision_templates(&mut conn), [own]);
    assert_eq!(snapshot(&mut conn, "templates"), templates);
    assert_eq!(snapshot(&mut conn, "template_metadata"), defaults);

    // A template with the slug and a different name is the template of
    // the tenant too: the slug is unique.
    sql_query("UPDATE templates SET name = 'Vision of a product', slug = 'product_vision' WHERE slug = 'our-vision'")
        .execute(&mut conn)
        .expect("the rename");
    let templates = snapshot(&mut conn, "templates");
    conn.batch_execute(UP).expect("the migration passes");
    assert_eq!(snapshot(&mut conn, "templates"), templates);
    assert_eq!(
        snapshot(&mut conn, "template_metadata"),
        defaults,
        "a template of the tenant gets no default"
    );

    drop_database(admin_conn, conn, SCRATCH_DB);
}

#[test]
fn a_tenant_with_no_vision_value_gets_the_template_with_no_default() {
    const SCRATCH_DB: &str = "kairos_document_board_t0269_option_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);
    make_the_old_state(&mut conn);

    // The tenant removed the value `vision` of the document type.
    let removed = sql_query(
        "DELETE FROM metadata_enum_options o USING metadata_definitions d \
          WHERE d.id = o.metadata_definition_id AND d.slug = 'document_type' \
            AND o.value = 'vision'",
    )
    .execute(&mut conn)
    .expect("removing the value");
    assert_eq!(removed, 1);

    conn.batch_execute(UP).expect("the migration passes");
    assert_eq!(
        product_vision_templates(&mut conn),
        [TemplateRow {
            name: "Product Vision".into(),
            slug: "product_vision".into(),
            content: PRODUCT_VISION.into(),
            is_system_default: true,
            defaults: None,
        }]
    );

    // And a tenant with no definition `document_type`.
    make_the_old_state_again(&mut conn);
    sql_query("DELETE FROM metadata_definitions WHERE slug = 'document_type'")
        .execute(&mut conn)
        .expect("removing the definition");
    conn.batch_execute(UP).expect("the migration passes");
    assert_eq!(product_vision_templates(&mut conn).len(), 1);
    assert_eq!(product_vision_templates(&mut conn)[0].defaults, None);

    drop_database(admin_conn, conn, SCRATCH_DB);
}

/// [`make_the_old_state`] for a tenant that has no record of the
/// migration, because the test ran the SQL and not the harness.
fn make_the_old_state_again(conn: &mut PgConnection) {
    conn.batch_execute(DOWN).expect("the down migration");
    sql_query("DELETE FROM templates WHERE slug = 'product_vision'")
        .execute(conn)
        .expect("removing the template");
}

#[test]
fn a_new_tenant_has_the_template_of_the_system_defaults() {
    const SCRATCH_DB: &str = "kairos_document_board_t0269_new_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);

    // The migration runs before the copy of the system defaults, so the
    // migration makes the template of a new tenant. The copy adds the
    // metadata default, and no second template.
    assert_eq!(
        product_vision_templates(&mut conn),
        [TemplateRow {
            name: "Product Vision".into(),
            slug: "product_vision".into(),
            content: PRODUCT_VISION.into(),
            is_system_default: true,
            defaults: Some("document_type=vision".into()),
        }]
    );
    // The text of the migration is the text of the system default.
    #[derive(QueryableByName)]
    struct Content {
        #[diesel(sql_type = Text)]
        content: String,
    }
    let system =
        sql_query("SELECT content FROM public.system_templates WHERE slug = 'product_vision'")
            .get_result::<Content>(&mut conn)
            .expect("the system default");
    assert_eq!(system.content, PRODUCT_VISION);
    // "Company Vision" stays, with its text.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS count FROM templates \
              WHERE slug = 'company_vision' AND name = 'Company Vision' \
                AND content LIKE '# Company Vision%'"
        ),
        1
    );

    drop_database(admin_conn, conn, SCRATCH_DB);
}
