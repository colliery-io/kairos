//! KAIROS-T-0630 — the tenant migration `2026-10-18-003000_item_notes`
//! moves the `## Note` sections of each item into `item_notes` rows.
//!
//! The scenarios, on items that a tenant has before the migration:
//!
//! - unique ids stay, and a note without an id gets the id of its position;
//! - a duplicate id gets the next free number, and a line in its text;
//! - a `Promoted to CODE.` line sets `promoted_to`;
//! - a name resolves to a member of the tenant, else it stays a name;
//! - a note that a stray `## ` heading cut stays cut;
//! - the content loses the notes, with no version and no history row;
//! - the vector of `item_search` has the notes, and the SQL text of the
//!   notes is the text of the Rust renderer;
//! - a second run changes nothing, and `append_note` takes the next number.
//!
//! The test makes the items with the notes in their content (the create
//! path stores any content), then runs the SQL of the migration again, as
//! a second run of `up.sql` does. Against real Postgres from the compose
//! stack (A-0012 tier 2). The test owns a scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::search::{SearchFilter, SearchRequest};
use kairos_core::short_code::ItemType;
use kairos_db::item_notes::{self, ItemNote};
use kairos_db::items::{self, CreateTask};
use kairos_db::models::enums::{BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, TeamType, User};
use kairos_db::{CodePrefix, create_board, provision_tenant, run_public_migrations, search};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The SQL of the migration, as diesel runs it.
const MIGRATION: &str = include_str!("../migrations/tenant/2026-10-18-003000_item_notes/up.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// A person, a member of the tenant `acme` when `member`.
fn person(conn: &mut PgConnection, org: Uuid, name: &str, member: bool) -> Uuid {
    use kairos_db::schema::{organization_members, users};
    let id = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("dex|{name}"),
            user_name: name.to_lowercase(),
            email: format!("{}@acme.test", name.to_lowercase()),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting a user")
        .id;
    if member {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org,
                user_id: id,
                role: OrgRole::Member,
            })
            .execute(conn)
            .expect("granting membership");
    }
    id
}

#[derive(QueryableByName)]
struct ItemRow {
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    version: i32,
    #[diesel(sql_type = BigInt)]
    history: i64,
}

/// The content, the version and the count of history rows of a task.
fn item_row(conn: &mut PgConnection, id: Uuid) -> ItemRow {
    sql_query(
        "SELECT t.content, t.version, \
                (SELECT count(*) FROM item_history h WHERE h.item_id = t.id) AS history \
           FROM tasks t WHERE t.id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .expect("reading the task")
}

#[derive(QueryableByName)]
struct Raw {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    seq: i32,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    author_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    author_name: Option<String>,
    #[diesel(sql_type = Text)]
    date: String,
}

/// The stored author columns and the date of each note of an item.
fn raw_notes(conn: &mut PgConnection, id: Uuid) -> Vec<Raw> {
    sql_query(
        "SELECT seq, author_id, author_name, \
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS date \
           FROM item_notes WHERE item_id = $1 ORDER BY seq",
    )
    .bind::<SqlUuid, _>(id)
    .load(conn)
    .expect("reading the notes")
}

#[derive(QueryableByName)]
struct Markdown {
    #[diesel(sql_type = Nullable<Text>)]
    text: Option<String>,
}

/// The codes of the tasks that match `q`.
fn codes_for(conn: &mut PgConnection, q: &str) -> Vec<String> {
    let request = SearchRequest {
        q: Some(q.to_string()),
        filter: Some(SearchFilter::default()),
        ..Default::default()
    };
    search::execute_search(conn, &request)
        .expect("searching")
        .tasks
        .into_iter()
        .map(|t| t.short_code)
        .collect()
}

fn notes(conn: &mut PgConnection, id: Uuid) -> Vec<ItemNote> {
    item_notes::notes_of(conn, id).expect("reading the notes")
}

#[test]
fn the_migration_moves_each_note_into_a_row() {
    let db: &'static str =
        Box::leak(scratch::scratch_db_name("kairos_item_notes_t0630").into_boxed_str());
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

    let org: Uuid = {
        use kairos_db::schema::organizations;
        organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("the organization")
    };
    let robin = person(&mut conn, org, "Robin", true);
    let sam = person(&mut conn, org, "Sam", true);
    // Jamie is a user, but not of this tenant: the name stays a name.
    let _jamie = person(&mut conn, org, "Jamie", false);
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
        Some(robin),
    )
    .expect("creating the board");
    let task = |conn: &mut PgConnection, title: &str, content: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: board.id,
                column_id: None,
                title,
                content,
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                repository_id: None,
            },
            robin,
        )
        .expect("creating a task")
    };

    // Unique ids, an old note with no id, a promoted note, a name with no
    // user of the tenant, a fence with a heading inside a note.
    let unique_content = "# Pay\n\nThe body.\n\n## Note (2026-10-01, Robin)\n\nFirst, \
        in the old form.\n\n## Note n-2 (2026-10-02, Sam)\n\nSecond.\n\nPromoted to \
        CORE-A-0007.\n\n## Note n-3 (2026-10-03, Jamie)\n\nThe saffron fuse.\n\n```\n\
        ## Note (2026-10-04, Fake)\n```\n";
    let unique = task(&mut conn, "Unique ids", unique_content);
    // A duplicate id: a hand edit removed a note, and the count gave its
    // id again.
    let duplicate_content = "# Twice\n\n## Note n-1 (2026-10-01, Robin)\n\nOne.\n\n\
        ## Note n-2 (2026-10-02, Robin)\n\nTwo.\n\n## Note n-2 (2026-10-03, Sam)\n\nTwo again.";
    let duplicate = task(&mut conn, "Duplicate id", duplicate_content);
    // A stray heading cut the note before the move: the rest stays.
    let cut_content =
        "# Cut\n\n## Note n-1 (2026-10-01, Robin)\n\nStart.\n\n## Findings\n\nThe rest.\n";
    let cut = task(&mut conn, "Cut note", cut_content);
    // A date that is not a date.
    let odd_date = task(
        &mut conn,
        "Odd date",
        "Body.\n\n## Note (yesterday, robin@acme.test)\n\nWhen?",
    );
    // No note: a heading that only looks like one.
    let plain_content = "# Plain\n\n## Notes on the design\n\nNot a note.";
    let plain = task(&mut conn, "Plain", plain_content);

    let before: Vec<ItemRow> = [unique.id, duplicate.id, cut.id, odd_date.id, plain.id]
        .into_iter()
        .map(|id| item_row(&mut conn, id))
        .collect();

    conn.batch_execute(MIGRATION)
        .expect("running the migration");

    // No version and no history row.
    for (id, was) in [unique.id, duplicate.id, cut.id, odd_date.id, plain.id]
        .into_iter()
        .zip(&before)
    {
        let now = item_row(&mut conn, id);
        assert_eq!(now.version, was.version, "the move writes no version");
        assert_eq!(now.history, was.history, "the move writes no history row");
    }

    // --- unique ids --------------------------------------------------------
    let moved = notes(&mut conn, unique.id);
    assert_eq!(
        moved.iter().map(ItemNote::note_id).collect::<Vec<_>>(),
        ["n-1", "n-2", "n-3"],
        "each note keeps its id, and a note with no id gets its position"
    );
    assert_eq!(moved[0].text, "First, in the old form.");
    assert_eq!(moved[0].author_id, Some(robin));
    assert_eq!(moved[0].author, "Robin");
    assert_eq!(moved[1].text, "Second.", "the promote mark leaves the text");
    assert_eq!(moved[1].promoted_to.as_deref(), Some("CORE-A-0007"));
    assert_eq!(moved[1].author_id, Some(sam));
    assert_eq!(
        moved[2].text, "The saffron fuse.\n\n```\n## Note (2026-10-04, Fake)\n```",
        "a fence stays in the note"
    );
    assert!(moved[2].text.contains("## Note (2026-10-04, Fake)"));
    assert_eq!(moved[2].author_id, None, "Jamie is no member of the tenant");
    assert_eq!(moved[2].author, "Jamie");
    let raw = raw_notes(&mut conn, unique.id);
    assert_eq!(
        raw.iter().map(|r| r.date.as_str()).collect::<Vec<_>>(),
        ["2026-10-01", "2026-10-02", "2026-10-03"]
    );
    assert_eq!(raw.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(raw[2].author_name.as_deref(), Some("Jamie"));
    assert_eq!(raw[2].author_id, None);
    assert_eq!(raw[0].author_name, None, "a resolved name is not kept");
    assert_eq!(raw[0].author_id, Some(robin));
    assert_eq!(item_row(&mut conn, unique.id).content, "# Pay\n\nThe body.");
    // The vector has the notes, and the content does not.
    assert_eq!(
        codes_for(&mut conn, "saffron"),
        vec![unique.short_code.clone()]
    );
    // The SQL text of the notes is the text of the Rust renderer, and the
    // content and the notes are the text that the item had.
    let sql_text: Markdown = sql_query("SELECT item_notes_markdown($1) AS text")
        .bind::<SqlUuid, _>(unique.id)
        .get_result(&mut conn)
        .expect("item_notes_markdown");
    let rendered = item_notes::render(&moved);
    assert_eq!(sql_text.text.as_deref(), Some(rendered.as_str()));
    assert_eq!(
        format!("# Pay\n\nThe body.{rendered}"),
        unique_content.trim_end().replace(
            "## Note (2026-10-01, Robin)",
            "## Note n-1 (2026-10-01, Robin)"
        )
    );

    // --- a duplicate id ----------------------------------------------------
    let moved = notes(&mut conn, duplicate.id);
    assert_eq!(
        moved.iter().map(ItemNote::note_id).collect::<Vec<_>>(),
        ["n-1", "n-2", "n-3"]
    );
    assert_eq!(moved[1].text, "Two.", "the first n-2 keeps its id");
    assert_eq!(
        moved[2].text,
        "Two again.\n\nKairos changed the id of this note from n-2 to n-3: an earlier note \
         has the id n-2."
    );
    assert_eq!(moved[2].author_id, Some(sam));
    assert_eq!(item_row(&mut conn, duplicate.id).content, "# Twice");

    // --- a cut note --------------------------------------------------------
    let moved = notes(&mut conn, cut.id);
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].text, "Start.");
    assert_eq!(
        item_row(&mut conn, cut.id).content,
        "# Cut\n\n## Findings\n\nThe rest.",
        "the rest of a cut note stays in the content"
    );

    // --- an odd date, a name that is an email ------------------------------
    let moved = notes(&mut conn, odd_date.id);
    assert_eq!(moved.len(), 1);
    assert_eq!(
        moved[0].text,
        "When?\n\nThe heading of this note gave the date \"yesterday\"."
    );
    assert_eq!(moved[0].author_id, Some(robin), "the email names Robin");
    assert_eq!(item_row(&mut conn, odd_date.id).content, "Body.");

    // --- no note -----------------------------------------------------------
    assert!(notes(&mut conn, plain.id).is_empty());
    assert_eq!(item_row(&mut conn, plain.id).content, plain_content);

    // --- a second run changes nothing --------------------------------------
    let snapshot = |conn: &mut PgConnection| {
        [unique.id, duplicate.id, cut.id, odd_date.id, plain.id]
            .into_iter()
            .map(|id| (item_row(conn, id).content, notes(conn, id)))
            .collect::<Vec<_>>()
    };
    let first = snapshot(&mut conn);
    conn.batch_execute(MIGRATION)
        .expect("running the migration again");
    assert_eq!(snapshot(&mut conn), first, "a second run changes nothing");

    // A note that is a row and also back in the content (a rollback to an
    // old version) is not written again; a new note in the content is.
    let rolled_back = format!(
        "{}\n\n## Note n-1 (2026-10-01, Robin)\n\nOne.\n\n## Note n-4 (2026-10-05, Sam)\n\nFour.",
        item_row(&mut conn, duplicate.id).content
    );
    sql_query("UPDATE tasks SET content = $1 WHERE id = $2")
        .bind::<Text, _>(&rolled_back)
        .bind::<SqlUuid, _>(duplicate.id)
        .execute(&mut conn)
        .expect("a rolled-back content");
    conn.batch_execute(MIGRATION)
        .expect("running the migration a third time");
    let moved = notes(&mut conn, duplicate.id);
    assert_eq!(
        moved.iter().map(ItemNote::note_id).collect::<Vec<_>>(),
        ["n-1", "n-2", "n-3", "n-4"]
    );
    assert_eq!(moved[3].text, "Four.");
    assert_eq!(item_row(&mut conn, duplicate.id).content, "# Twice");

    // --- append_note after the move: the next number, no version -----------
    let version = item_row(&mut conn, unique.id).version;
    let appended = item_notes::append_note(
        &mut conn,
        ItemType::Task,
        unique.id,
        "\n\n## Findings\n\nA heading in a note is fine.\n",
        robin,
    )
    .expect("appending a note");
    assert_eq!(appended.note_id, "n-4");
    assert_eq!(appended.version, version);
    assert_eq!(
        appended.note.text,
        "## Findings\n\nA heading in a note is fine."
    );
    let after = item_row(&mut conn, unique.id);
    assert_eq!(after.version, version, "a note writes no version");
    assert_eq!(after.content, "# Pay\n\nThe body.", "a note is not content");
    assert_eq!(notes(&mut conn, unique.id).len(), 4);
    assert!(matches!(
        item_notes::append_note(&mut conn, ItemType::Task, unique.id, " \n ", robin),
        Err(item_notes::NoteError::EmptyText)
    ));

    // A purge removes the notes of the task.
    kairos_db::purge::purge_task(&mut conn, unique.id, robin).expect("purging");
    assert!(notes(&mut conn, unique.id).is_empty());

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
}
