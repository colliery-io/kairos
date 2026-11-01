//! KAIROS-T-0624 — the stored tsvector of `item_search`.
//!
//! Triggers on the five item tables write the vector in the transaction of
//! the write. The scenarios:
//!
//! - a create writes the row, with the vector of the old index expression,
//! - an edit of the title or the content rewrites it, and search follows,
//! - a note rewrites it, with the text of the note (KAIROS-T-0630),
//! - a transition, an archive and a restore do not write it,
//! - a content edit is a HOT update of the task row,
//! - a purge removes it.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). The test
//! owns a scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Bool, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::search::{SearchFilter, SearchRequest};
use kairos_core::short_code::ItemType;
use kairos_db::items::{self, ContentUpdate, CreateTask};
use kairos_db::models::enums::{BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{CodePrefix, create_board, provision_tenant, run_public_migrations, search};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type = Text)]
    xmin: String,
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Bool)]
    same: bool,
}

/// The row of the item in `item_search`: its xmin (a new value after each
/// write of the row), its type, and whether its vector is the vector of the
/// old index expression over the title and the content of the task now,
/// with the text of its notes after them (KAIROS-T-0630). With no notes it
/// is the old expression.
fn search_row(conn: &mut PgConnection, id: Uuid) -> Option<Row> {
    sql_query(
        "SELECT s.xmin::text AS xmin, s.entity_type, \
                s.tsv = (setweight(to_tsvector('english', coalesce(t.title, '')), 'A') \
                      || setweight(to_tsvector('english', coalesce(t.content, '')), 'B') \
                      || setweight(to_tsvector('english', coalesce( \
                             (SELECT string_agg(n.text, E'\n\n' ORDER BY n.seq) \
                                FROM item_notes n WHERE n.item_id = t.id), '')), 'B')) AS same \
           FROM item_search s LEFT JOIN tasks t ON t.id = s.item_id \
          WHERE s.item_id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
    .expect("reading item_search")
}

/// The codes of the tasks that match `q`, archived ones too.
fn codes_for(conn: &mut PgConnection, q: &str) -> Vec<String> {
    let request = SearchRequest {
        q: Some(q.to_string()),
        filter: Some(SearchFilter {
            include_deleted: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    search::execute_search(conn, &request)
        .expect("searching")
        .tasks
        .into_iter()
        .map(|t| t.short_code)
        .collect()
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// The updates and the HOT updates of `tasks` in the open transaction.
fn hot_updates(conn: &mut PgConnection) -> (i64, i64) {
    let read = |conn: &mut PgConnection, f: &str| {
        sql_query(format!("SELECT {f}('tasks'::regclass) AS n"))
            .get_result::<Count>(conn)
            .expect("reading the update count")
            .n
    };
    (
        read(conn, "pg_stat_get_xact_tuples_updated"),
        read(conn, "pg_stat_get_xact_tuples_hot_updated"),
    )
}

/// The rows of `embedding_dirty` for the item `id`: 0 or 1.
fn dirty_rows(conn: &mut PgConnection, id: Uuid) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    sql_query("SELECT count(*) AS n FROM embedding_dirty WHERE item_id = $1")
        .bind::<SqlUuid, _>(id)
        .get_result::<Count>(conn)
        .expect("counting the marks")
        .n
}

fn version_of(conn: &mut PgConnection, id: Uuid) -> i32 {
    use kairos_db::schema::tasks;
    tasks::table
        .filter(tasks::id.eq(id))
        .select(tasks::version)
        .get_result(conn)
        .expect("reading the version")
}

#[test]
fn the_vector_follows_the_title_and_the_content_only() {
    let db: &'static str =
        Box::leak(scratch::scratch_db_name("kairos_item_search_t0624").into_boxed_str());
    let _guard = scratch::ScratchGuard::new(db);
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (is the stack up?)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {db}"))
        .execute(&mut admin)
        .expect("creating scratch database");
    let mut conn = PgConnection::establish(&with_database(&admin_url, db))
        .expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let user = {
        use kairos_db::schema::users;
        diesel::insert_into(users::table)
            .values(NewUser {
                external_id: "dex|robin".into(),
                user_name: "dex|robin".into(),
                email: "robin@acme.test".into(),
                display_name: "Robin".into(),
            })
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("inserting user")
            .id
    };
    let team = {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(NewTeam {
                name: "core".into(),
                slug: "core".into(),
                team_type: TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result::<Uuid>(&mut conn)
            .expect("inserting team")
    };
    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Core",
        "core",
        CodePrefix::Given("CORE"),
        Some(team),
        Some(user),
    )
    .expect("creating the board");

    // A create writes the row, with the vector of the old index expression.
    let task = items::create_task(
        &mut conn,
        CreateTask {
            board_id: board.id,
            column_id: None,
            title: "Lantern wiring",
            content: "The amber relay hums.",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        user,
    )
    .expect("creating a task");
    let created = search_row(&mut conn, task.id).expect("a create writes the row");
    assert_eq!(created.entity_type, "task");
    assert!(created.same, "the vector is the old index expression");
    assert_eq!(codes_for(&mut conn, "amber"), vec![task.short_code.clone()]);

    // A content edit rewrites the vector, and search follows. It is a HOT
    // update of the task row: no index of `tasks` covers the content now.
    // The HOT count of the transaction is read inside it; the edit is a
    // savepoint of it.
    let (hot_before, hot_after) = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            let before = hot_updates(conn);
            let expected_version = version_of(conn, task.id);
            items::update_item_content(
                conn,
                ItemType::Task,
                task.id,
                ContentUpdate {
                    new_title: None,
                    new_content: "The cobalt relay is quiet.",
                    expected_version,
                },
                user,
            )
            .expect("editing the content");
            Ok((before, hot_updates(conn)))
        })
        .expect("the edit transaction");
    let updates = hot_after.0 - hot_before.0;
    assert!(updates >= 1, "the edit updates the task row");
    assert_eq!(
        hot_after.1 - hot_before.1,
        updates,
        "a content edit is a HOT update of the task row"
    );
    let edited = search_row(&mut conn, task.id).expect("the row stays");
    assert_ne!(edited.xmin, created.xmin, "a content edit rewrites the row");
    assert!(edited.same, "the vector follows the content");
    assert!(codes_for(&mut conn, "amber").is_empty());
    assert_eq!(
        codes_for(&mut conn, "cobalt"),
        vec![task.short_code.clone()]
    );

    // A title edit rewrites it too.
    let retitle_version = version_of(&mut conn, task.id);
    items::update_item_content(
        &mut conn,
        ItemType::Task,
        task.id,
        ContentUpdate {
            new_title: Some("Beacon wiring"),
            new_content: "The cobalt relay is quiet.",
            expected_version: retitle_version,
        },
        user,
    )
    .expect("editing the title");
    let retitled = search_row(&mut conn, task.id).expect("the row stays");
    assert_ne!(retitled.xmin, edited.xmin, "a title edit rewrites the row");
    assert!(retitled.same, "the vector follows the title");
    assert!(codes_for(&mut conn, "lantern").is_empty());
    assert_eq!(
        codes_for(&mut conn, "beacon"),
        vec![task.short_code.clone()]
    );

    // KAIROS-T-0630: a note is a row, not a content edit. Its text is in
    // the vector, and the version does not change.
    let before_note = version_of(&mut conn, task.id);
    kairos_db::item_notes::append_note(
        &mut conn,
        ItemType::Task,
        task.id,
        "Found the saffron fuse.",
        user,
    )
    .expect("appending a note");
    assert_eq!(
        version_of(&mut conn, task.id),
        before_note,
        "a note writes no version"
    );
    let noted = search_row(&mut conn, task.id).expect("the row stays");
    assert_ne!(noted.xmin, retitled.xmin, "a note rewrites the row");
    assert!(noted.same, "the vector has the note");
    assert_eq!(
        codes_for(&mut conn, "saffron"),
        vec![task.short_code.clone()]
    );

    // A transition does not write the row.
    let other_column: Uuid = {
        use kairos_db::schema::board_columns;
        board_columns::table
            .filter(board_columns::board_id.eq(board.id))
            .filter(board_columns::id.ne(task.column_id))
            .select(board_columns::id)
            .first(&mut conn)
            .expect("a second column")
    };
    sql_query("UPDATE tasks SET column_id = $2, updated_at = now() WHERE id = $1")
        .bind::<SqlUuid, _>(task.id)
        .bind::<SqlUuid, _>(other_column)
        .execute(&mut conn)
        .expect("moving the task to another column");
    assert_eq!(
        search_row(&mut conn, task.id).expect("the row stays").xmin,
        noted.xmin,
        "a transition does not write the row"
    );

    // An archive and a restore do not write it either; search with
    // include_deleted still finds the archived task.
    items::soft_delete_item(&mut conn, ItemType::Task, task.id, user).expect("archiving");
    assert_eq!(
        search_row(&mut conn, task.id).expect("the row stays").xmin,
        noted.xmin,
        "an archive does not write the row"
    );
    assert_eq!(
        codes_for(&mut conn, "saffron"),
        vec![task.short_code.clone()]
    );
    items::restore_item(&mut conn, ItemType::Task, task.id, user)
        .expect("restoring")
        .expect("nothing blocks the restore");
    assert_eq!(
        search_row(&mut conn, task.id).expect("the row stays").xmin,
        noted.xmin,
        "a restore does not write the row"
    );

    // A purge removes it, and the mark of the task for the embedding sweep
    // (KAIROS-T-0743): the purge calls `embeddings::forget_item`.
    assert_eq!(
        dirty_rows(&mut conn, task.id),
        1,
        "a write marks the task for the sweep"
    );
    kairos_db::purge::purge_task(&mut conn, task.id, user).expect("purging");
    assert!(
        search_row(&mut conn, task.id).is_none(),
        "a purge removes the row"
    );
    assert_eq!(
        dirty_rows(&mut conn, task.id),
        0,
        "a purge leaves no embedding_dirty row"
    );
    assert!(codes_for(&mut conn, "saffron").is_empty());

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
}
