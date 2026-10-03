//! COLLIERY-T-3099 — each board has a short-code prefix, and the codes come
//! from a sequence for each prefix and type, in the database layer.
//!
//! The scenarios of the task:
//!
//! - a new task on a board gets the prefix of the board, and continues the
//!   sequence of (prefix, type),
//! - the migration `board_code_prefixes` changes no existing code, and the
//!   next task of the board that keeps the tenant prefix continues the old
//!   sequence,
//! - boards can share a prefix for different types, and not for one type,
//! - a board needs a valid prefix,
//! - the migration can run 2 times.
//!
//! The REST part (the field `code_prefix` of the create of a board and of a
//! team) is in `kairos-server/tests/board_code_prefix.rs`.
//!
//! The old data of the migration test is written directly to the tables,
//! after the down migration: that is the state of a tenant before the
//! migration.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use std::collections::BTreeMap;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::items::{self, CreateDocument, CreateInitiative, CreateTask};
use kairos_db::models::enums::{BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{BoardError, CodePrefix, create_board, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const UP: &str = include_str!("../migrations/tenant/2026-10-03-000000_board_code_prefixes/up.sql");
const DOWN: &str =
    include_str!("../migrations/tenant/2026-10-03-000000_board_code_prefixes/down.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// A scratch database with the tenant `colliery`, pinned to its schema, and
/// a user.
fn tenant(scratch_db: &str) -> (PgConnection, PgConnection, Uuid) {
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
    provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning colliery");
    sql_query("SET search_path TO org_colliery, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let user = {
        use kairos_db::schema::users;
        diesel::insert_into(users::table)
            .values(NewUser {
                external_id: "dex|dylan".into(),
                user_name: "dex|dylan".into(),
                email: "dylan@colliery.test".into(),
                display_name: "Dylan".into(),
            })
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("inserting user")
            .id
    };
    (admin_conn, conn, user)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

fn team(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::teams;
    diesel::insert_into(teams::table)
        .values(NewTeam {
            name: slug.to_string(),
            slug: slug.to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team")
}

fn board_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("the board {slug}: {e}"))
}

fn task(conn: &mut PgConnection, board: Uuid, actor: Uuid) -> String {
    items::create_task(
        conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title: "A task",
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        actor,
    )
    .expect("creating a task")
    .short_code
}

fn document(conn: &mut PgConnection, board: Option<Uuid>, actor: Uuid) -> String {
    items::create_document_on_board(
        conn,
        CreateDocument {
            title: "A document",
            content: Some(""),
            template_id: None,
        },
        board,
        actor,
    )
    .expect("creating a document")
    .short_code
}

fn initiative(conn: &mut PgConnection, board: Uuid, actor: Uuid) -> String {
    items::create_initiative(
        conn,
        CreateInitiative {
            board_id: board,
            column_id: None,
            title: "An initiative",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        actor,
    )
    .expect("creating an initiative")
    .short_code
}

#[derive(QueryableByName)]
struct PrefixRow {
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = Text)]
    code_prefix: String,
}

/// `slug -> prefix` of each board, live and deleted.
fn prefixes(conn: &mut PgConnection) -> BTreeMap<String, String> {
    sql_query("SELECT slug, code_prefix FROM boards")
        .load::<PrefixRow>(conn)
        .expect("reading the prefixes")
        .into_iter()
        .map(|r| (r.slug, r.code_prefix))
        .collect()
}

#[derive(QueryableByName)]
struct SequenceRow {
    #[diesel(sql_type = Text)]
    code_prefix: String,
    #[diesel(sql_type = Text)]
    item_type: String,
    #[diesel(sql_type = BigInt)]
    last_number: i64,
}

/// `prefix-type -> last number` of each row of `short_code_sequences`.
fn sequences(conn: &mut PgConnection) -> BTreeMap<String, i64> {
    sql_query("SELECT code_prefix, item_type, last_number FROM short_code_sequences")
        .load::<SequenceRow>(conn)
        .expect("reading the sequences")
        .into_iter()
        .map(|r| (format!("{}-{}", r.code_prefix, r.item_type), r.last_number))
        .collect()
}

#[derive(QueryableByName)]
struct CodeRow {
    #[diesel(sql_type = Text)]
    short_code: String,
}

/// Each short code of the tenant, sorted.
fn codes(conn: &mut PgConnection) -> Vec<String> {
    sql_query(
        "SELECT short_code FROM strategies UNION ALL SELECT short_code FROM initiatives \
         UNION ALL SELECT short_code FROM tasks UNION ALL SELECT short_code FROM documents \
         UNION ALL SELECT short_code FROM adrs ORDER BY 1",
    )
    .load::<CodeRow>(conn)
    .expect("reading the codes")
    .into_iter()
    .map(|r| r.short_code)
    .collect()
}

// ---------------------------------------------------------------------------
// Scenario: A new task on a board gets the board's prefix
// ---------------------------------------------------------------------------

#[test]
fn a_new_task_on_a_board_gets_the_prefix_of_the_board() {
    const DB: &str = "kairos_board_prefix_new_task_t3099";
    let (admin, mut conn, user) = tenant(DB);

    let colliery_team = team(&mut conn, "colliery-io");
    let colliery = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Colliery Delivery",
        "colliery-io-delivery",
        CodePrefix::Given("COLLIERY"),
        Some(colliery_team),
        Some(user),
    )
    .expect("a delivery board can have the prefix of the initiatives board");
    assert_eq!(colliery.code_prefix, "COLLIERY");
    let skadi_team = team(&mut conn, "skadi");
    let skadi = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Skadi",
        "skadi",
        CodePrefix::Given("SKADI"),
        Some(skadi_team),
        Some(user),
    )
    .expect("creating skadi");

    // A new board starts at 1, and each board has its own sequence.
    assert_eq!(task(&mut conn, skadi.id, user), "SKADI-T-0001");
    assert_eq!(task(&mut conn, colliery.id, user), "COLLIERY-T-0001");
    assert_eq!(task(&mut conn, skadi.id, user), "SKADI-T-0002");

    // Given a board "skadi" whose highest task number is 669 (a re-key wrote
    // the code, COLLIERY-T-3103).
    sql_query("UPDATE tasks SET short_code = 'SKADI-T-0669' WHERE short_code = 'SKADI-T-0002'")
        .execute(&mut conn)
        .expect("writing a code directly");
    assert_eq!(
        items::sync_code_sequence(&mut conn, "SKADI", ItemType::Task).expect("sync"),
        669
    );
    // When I create a task on "skadi", then its code is "SKADI-T-0670".
    assert_eq!(task(&mut conn, skadi.id, user), "SKADI-T-0670");
    // The sync does not move a sequence back.
    assert_eq!(
        items::sync_code_sequence(&mut conn, "SKADI", ItemType::Task).expect("sync"),
        670
    );

    // The other types: a document takes the prefix of its owner board, and
    // an item with no board takes the prefix of the tenant.
    assert_eq!(document(&mut conn, Some(skadi.id), user), "SKADI-D-0001");
    assert_eq!(document(&mut conn, None, user), "COLLIERY-D-0001");
    assert_eq!(
        document(&mut conn, Some(colliery.id), user),
        "COLLIERY-D-0002"
    );
    let initiatives = board_id(&mut conn, "initiatives");
    assert_eq!(initiative(&mut conn, initiatives, user), "COLLIERY-I-0001");

    drop_database(admin, conn, DB);
}

// ---------------------------------------------------------------------------
// Scenario: Boards can share a prefix for different types
// Scenario: A board needs a valid prefix
// ---------------------------------------------------------------------------

#[test]
fn boards_share_a_prefix_only_for_different_types() {
    const DB: &str = "kairos_board_prefix_share_t3099";
    let (admin, mut conn, user) = tenant(DB);

    // Given the board "initiatives" with prefix "COLLIERY" (provisioning).
    let all = prefixes(&mut conn);
    assert_eq!(all["initiatives"], "COLLIERY");
    assert_eq!(all["adrs"], "COLLIERY");
    assert_eq!(all["strategy"], "COLLIERY");

    let colliery_team = team(&mut conn, "colliery-io");
    let colliery = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Colliery Delivery",
        "colliery-io-delivery",
        CodePrefix::Given("COLLIERY"),
        Some(colliery_team),
        Some(user),
    )
    .expect("a delivery board shares COLLIERY with the boards of other types");

    // When I create a delivery board with prefix "COLLIERY" for a new team,
    // then the create is refused, and the error names colliery-io-delivery.
    let new_team = team(&mut conn, "newcomers");
    let err = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Newcomers",
        "newcomers-delivery",
        CodePrefix::Given("COLLIERY"),
        Some(new_team),
        Some(user),
    )
    .expect_err("COLLIERY is taken for tasks");
    match &err {
        BoardError::CodePrefixTaken {
            code_prefix,
            level,
            board_id,
            board_slug,
            ..
        } => {
            assert_eq!(code_prefix, "COLLIERY");
            assert_eq!(*level, BoardLevel::Delivery);
            assert_eq!(*board_id, colliery.id);
            assert_eq!(board_slug, "colliery-io-delivery");
        }
        other => panic!("expected CodePrefixTaken, got {other:?}"),
    }
    let message = err.to_string();
    assert!(message.contains("colliery-io-delivery"), "{message}");
    assert!(message.contains("tasks"), "{message}");

    // The same for an ADR board: `adrs` has COLLIERY for ADRs.
    let err = create_board(
        &mut conn,
        BoardLevel::Adr,
        "More decisions",
        "more-adrs",
        CodePrefix::Given("COLLIERY"),
        None,
        Some(user),
    )
    .expect_err("COLLIERY is taken for ADRs");
    assert!(
        matches!(&err, BoardError::CodePrefixTaken { board_slug, .. } if board_slug == "adrs"),
        "{err:?}"
    );

    // A board needs a valid prefix: "sk-adi", lower case, too short, too
    // long, a digit first.
    for bad in ["sk-adi", "skadi", "S", "ABCDEFGHIJK", "1SKADI", ""] {
        let err = create_board(
            &mut conn,
            BoardLevel::Delivery,
            "Newcomers",
            "newcomers-delivery",
            CodePrefix::Given(bad),
            Some(new_team),
            Some(user),
        )
        .expect_err("a bad prefix is refused");
        assert!(
            matches!(&err, BoardError::InvalidCodePrefix(p) if p == bad),
            "{bad}: {err:?}"
        );
        assert!(err.to_string().contains("code_prefix"), "{err}");
    }

    // A deleted board does not keep its prefix.
    sql_query("UPDATE boards SET deleted_at = now() WHERE slug = 'colliery-io-delivery'")
        .execute(&mut conn)
        .expect("deleting the board");
    let again = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Newcomers",
        "newcomers-delivery",
        CodePrefix::Given("COLLIERY"),
        Some(new_team),
        Some(user),
    )
    .expect("a deleted board does not keep its prefix");
    assert_eq!(again.code_prefix, "COLLIERY");

    // A prefix from the slug is made unique among the live boards of the
    // level.
    let web_team = team(&mut conn, "web");
    let web = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Web",
        "newcomers",
        CodePrefix::FromSlug,
        Some(web_team),
        Some(user),
    )
    .expect("a prefix from the slug");
    assert_eq!(web.code_prefix, "NEWCOMERS");
    let other_team = team(&mut conn, "other");
    let other = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Other",
        "new-comers",
        CodePrefix::FromSlug,
        Some(other_team),
        Some(user),
    )
    .expect("a numbered prefix from the slug");
    assert_eq!(other.code_prefix, "NEWCOMERS2");

    drop_database(admin, conn, DB);
}

// ---------------------------------------------------------------------------
// Scenario: Existing COLLIERY codes do not change
// Scenario: The migration can run 2 times
// ---------------------------------------------------------------------------

/// One board of old data. `day` is the day of 2026-01 that the board was
/// made. Returns the board and its one column.
fn old_board(
    conn: &mut PgConnection,
    slug: &str,
    level: &str,
    team: Option<Uuid>,
    day: u32,
    live: bool,
) -> (Uuid, Uuid) {
    let at = format!("2026-01-{day:02}T00:00:00Z");
    let board = Uuid::new_v4();
    sql_query(
        "INSERT INTO boards (id, name, slug, board_level, team_id, deleted_at, created_at, \
                             updated_at) \
         VALUES ($1, $2, $2, $3, $4, CASE WHEN $5 THEN NULL ELSE $6::timestamptz END, \
                 $6::timestamptz, $6::timestamptz)",
    )
    .bind::<SqlUuid, _>(board)
    .bind::<Text, _>(slug)
    .bind::<Text, _>(level)
    .bind::<diesel::sql_types::Nullable<SqlUuid>, _>(team)
    .bind::<diesel::sql_types::Bool, _>(live)
    .bind::<Text, _>(at)
    .execute(conn)
    .unwrap_or_else(|e| panic!("writing the old board {slug}: {e}"));
    let column = Uuid::new_v4();
    sql_query(
        "INSERT INTO board_columns (id, board_id, name, position) VALUES ($1, $2, 'Backlog', 0)",
    )
    .bind::<SqlUuid, _>(column)
    .bind::<SqlUuid, _>(board)
    .execute(conn)
    .unwrap_or_else(|e| panic!("writing the column of {slug}: {e}"));
    (board, column)
}

/// One task of old data, with a code from the old sequence and the prefix
/// of the tenant.
fn old_task(conn: &mut PgConnection, board: (Uuid, Uuid), content: &str, actor: Uuid) {
    sql_query(
        "INSERT INTO tasks (short_code, title, content, board_id, column_id, task_type, \
                            created_by, updated_by) \
         VALUES ('COLLIERY-T-' || lpad(nextval('seq_task_code')::text, 4, '0'), 'Old', $1, \
                 $2, $3, 'task', $4, $4)",
    )
    .bind::<Text, _>(content)
    .bind::<SqlUuid, _>(board.0)
    .bind::<SqlUuid, _>(board.1)
    .bind::<SqlUuid, _>(actor)
    .execute(conn)
    .expect("writing an old task");
}

fn footer(code: &str) -> String {
    format!(
        "Body.\n\n---\n\nThis item came from the Metis record of the repository x. Its Metis \
         code was {code}. Metis created it on 2025-12-24."
    )
}

#[test]
fn the_migration_keeps_each_code_and_can_run_2_times() {
    const DB: &str = "kairos_board_prefix_migration_t3099";
    let (admin, mut conn, user) = tenant(DB);

    // Given the tenant before the migration.
    conn.batch_execute(DOWN).expect("the down migration");
    let colliery_team = team(&mut conn, "colliery-io");
    let skadi_team = team(&mut conn, "skadi");
    let crt_team = team(&mut conn, "crt");
    let gq_team = team(&mut conn, "graphqlite");
    let co_team = team(&mut conn, "co");
    let colliery = old_board(
        &mut conn,
        "colliery-io-delivery",
        "delivery",
        Some(colliery_team),
        2,
        true,
    );
    let skadi = old_board(&mut conn, "skadi", "delivery", Some(skadi_team), 1, true);
    let crt = old_board(&mut conn, "crt", "delivery", Some(crt_team), 3, true);
    let graphqlite = old_board(&mut conn, "graphqlite", "delivery", Some(gq_team), 4, true);
    // The slug gives COLLIERY, which colliery-io-delivery keeps.
    let _co = old_board(&mut conn, "co-lliery", "delivery", Some(co_team), 5, true);
    let _gone = old_board(&mut conn, "gone-delivery", "delivery", None, 6, false);

    // colliery-io-delivery holds the most tasks, so it keeps the tenant
    // prefix, though skadi is older. Its tasks have Metis footers of other
    // repositories, as the Kairos import has.
    for _ in 0..4 {
        old_task(&mut conn, colliery, &footer("KAIROS-T-0012"), user);
    }
    old_task(&mut conn, skadi, &footer("SKADI-T-0577"), user);
    old_task(&mut conn, skadi, &footer("SKADI-T-0012"), user);
    old_task(&mut conn, skadi, "No footer.", user);
    old_task(&mut conn, crt, &footer("CRT-T-0001"), user);
    old_task(&mut conn, graphqlite, &footer("GQLITE-T-0008"), user);
    old_task(&mut conn, graphqlite, &footer("GQLITE-T-0009"), user);
    old_task(&mut conn, graphqlite, &footer("OTHER-T-0001"), user);
    // Two creates that failed: the old sequence is past the highest code.
    sql_query("SELECT nextval('seq_task_code'), nextval('seq_task_code')")
        .execute(&mut conn)
        .expect("advancing the old sequence");
    // An initiative and a document with no board, from the old sequences.
    let initiatives = board_id(&mut conn, "initiatives");
    let column: Uuid = {
        use kairos_db::schema::board_columns::dsl;
        dsl::board_columns
            .filter(dsl::board_id.eq(initiatives))
            .select(dsl::id)
            .first(&mut conn)
            .expect("a column of initiatives")
    };
    sql_query(
        "INSERT INTO initiatives (short_code, title, board_id, column_id, created_by, updated_by) \
         VALUES ('COLLIERY-I-' || lpad(nextval('seq_initiative_code')::text, 4, '0'), 'Old', \
                 $1, $2, $3, $3)",
    )
    .bind::<SqlUuid, _>(initiatives)
    .bind::<SqlUuid, _>(column)
    .bind::<SqlUuid, _>(user)
    .execute(&mut conn)
    .expect("writing an old initiative");
    sql_query(
        "INSERT INTO documents (short_code, title, created_by, updated_by) \
         VALUES ('COLLIERY-D-' || lpad(nextval('seq_document_code')::text, 4, '0'), 'Old', \
                 $1, $1)",
    )
    .bind::<SqlUuid, _>(user)
    .execute(&mut conn)
    .expect("writing an old document");
    let before = codes(&mut conn);
    assert_eq!(before.len(), 13);

    // When the migration runs.
    conn.batch_execute(UP).expect("the up migration");

    // Then every existing short code is the same as before.
    assert_eq!(codes(&mut conn), before);
    let all = prefixes(&mut conn);
    let expected: BTreeMap<String, String> = [
        ("strategy", "COLLIERY"),
        ("initiatives", "COLLIERY"),
        ("adrs", "COLLIERY"),
        ("colliery-io-delivery", "COLLIERY"),
        ("skadi", "SKADI"),
        ("crt", "CRT"),
        ("graphqlite", "GQLITE"),
        ("co-lliery", "COLLIERY2"),
        ("gone-delivery", "GONEDELIVE"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(all, expected);

    // Each sequence starts after the highest number of its (prefix, type);
    // the tenant prefix also after the old sequence (11 tasks and 2 failed
    // creates).
    let seqs = sequences(&mut conn);
    assert_eq!(seqs["COLLIERY-T"], 13);
    assert_eq!(seqs["COLLIERY-I"], 1);
    assert_eq!(seqs["COLLIERY-D"], 1);
    assert_eq!(seqs["COLLIERY-S"], 0);
    assert_eq!(seqs["COLLIERY-A"], 0);
    assert_eq!(seqs["SKADI-T"], 0);
    assert_eq!(seqs["SKADI-D"], 0);
    assert_eq!(seqs["GQLITE-T"], 0);
    assert_eq!(seqs["CRT-T"], 0);
    assert_eq!(seqs["COLLIERY2-T"], 0);
    assert!(
        !seqs.contains_key("GONEDELIVE-T"),
        "a deleted board gets no row"
    );

    // The migration can run 2 times: no prefix and no sequence changes.
    conn.batch_execute(UP)
        .expect("the second run of the up migration");
    assert_eq!(prefixes(&mut conn), expected);
    assert_eq!(sequences(&mut conn), seqs);
    assert_eq!(codes(&mut conn), before);

    // And the next task on colliery-io-delivery continues the COLLIERY task
    // sequence.
    assert_eq!(task(&mut conn, colliery.0, user), "COLLIERY-T-0014");
    assert_eq!(task(&mut conn, skadi.0, user), "SKADI-T-0001");
    assert_eq!(task(&mut conn, graphqlite.0, user), "GQLITE-T-0001");
    assert_eq!(initiative(&mut conn, initiatives, user), "COLLIERY-I-0002");
    assert_eq!(document(&mut conn, None, user), "COLLIERY-D-0002");

    // A third run after the creates changes nothing either.
    let seqs = sequences(&mut conn);
    conn.batch_execute(UP)
        .expect("the third run of the up migration");
    assert_eq!(sequences(&mut conn), seqs);

    // The down migration moves the old sequences past the numbers given.
    conn.batch_execute(DOWN).expect("the down migration");
    old_task(&mut conn, colliery, "After the down migration.", user);
    let after = codes(&mut conn);
    assert!(after.contains(&"COLLIERY-T-0015".to_string()), "{after:?}");

    drop_database(admin, conn, DB);
}
