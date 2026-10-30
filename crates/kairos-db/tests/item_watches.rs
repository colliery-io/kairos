//! KAIROS-T-0736 (COLLIERY-I-0664, decision 8) — a person watches an item,
//! with or without its children, in the database layer:
//!
//! - the migration is re-runnable, and down then up gives the table;
//! - each coarse change (moved, edited, note added, done; a board move and
//!   a cancel too) tells the watchers of the item, with a `watch`
//!   notification;
//! - a watch with children tells of a change of a child; a watch without
//!   children does not;
//! - the actor of the change is not told; a person who stopped the watch
//!   is not told; a service account is never told;
//! - the list of the watches of a person.
//!
//! The REST and MCP surfaces are in `kairos-server/tests/item_watches.rs`.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its
//! own scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::items::{ContentUpdate, CreateInitiative, CreateTask};
use kairos_db::models::enums::{BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{
    NewOrganizationMember, NewServiceAccountUser, NewUser, OrgRole, TeamType, User,
};
use kairos_db::{
    boards, create_board, graph, item_notes, items, provision_tenant, run_public_migrations,
    task_cancellations, watches,
};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
static SCRATCH_DB: scratch::ScratchName =
    scratch::ScratchName::new("kairos_db_item_watches_t0736_test");

/// The migration under test, verbatim.
const UP: &str = include_str!("../migrations/tenant/2026-10-30-000500_item_watches/up.sql");
const DOWN: &str = include_str!("../migrations/tenant/2026-10-30-000500_item_watches/down.sql");
const VALIDATE: &str =
    include_str!("../migrations/tenant/2026-10-30-000600_item_watches_validate/up.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

fn insert_user(conn: &mut PgConnection, external_id: &str, name: &str) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: external_id.to_string(),
            user_name: external_id.to_string(),
            email: format!("{external_id}@acme.test"),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn insert_service_account(conn: &mut PgConnection) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{}", Uuid::new_v4()),
            "ci@acme.test",
            "CI",
        ))
        .returning(users::id)
        .get_result(conn)
        .expect("inserting service account")
}

fn delivery_board(conn: &mut PgConnection, name: &str, slug: &str, actor: Uuid) -> Uuid {
    use kairos_db::schema::teams;
    let team_id: Uuid = diesel::insert_into(teams::table)
        .values(NewTeam {
            name: name.to_string(),
            slug: slug.to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team");
    create_board(
        conn,
        BoardLevel::Delivery,
        name,
        &format!("{slug}-delivery"),
        kairos_db::CodePrefix::FromSlug,
        Some(team_id),
        Some(actor),
    )
    .expect("creating delivery board")
    .id
}

fn board_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards;
    boards::table
        .filter(boards::slug.eq(slug))
        .select(boards::id)
        .first(conn)
        .expect("board")
}

fn column(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name}: {e}"))
}

fn task(conn: &mut PgConnection, board_id: Uuid, title: &str, actor: Uuid) -> Uuid {
    items::create_task(
        conn,
        CreateTask {
            board_id,
            column_id: None,
            title,
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        actor,
    )
    .expect("creating task")
    .id
}

#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type = Text)]
    text: String,
}

/// The texts of the `watch` notifications of `user`, oldest first, and
/// then none: each call deletes the ones it read, so each step of the test
/// reads its own.
fn take(conn: &mut PgConnection, user: Uuid) -> Vec<String> {
    let rows: Vec<Row> = sql_query(
        "DELETE FROM notifications WHERE user_id = $1 AND kind = 'watch' \
         RETURNING text",
    )
    .bind::<SqlUuid, _>(user)
    .load(conn)
    .expect("watch notifications");
    let mut texts: Vec<String> = rows.into_iter().map(|r| r.text).collect();
    texts.sort();
    texts
}

fn edit(conn: &mut PgConnection, task_id: Uuid, content: &str, actor: Uuid) {
    let version: i32 = {
        use kairos_db::schema::tasks;
        tasks::table
            .filter(tasks::id.eq(task_id))
            .select(tasks::version)
            .first(conn)
            .expect("version")
    };
    items::update_item_content(
        conn,
        ItemType::Task,
        task_id,
        ContentUpdate {
            new_title: None,
            new_content: content,
            expected_version: version,
        },
        actor,
    )
    .expect("editing");
}

fn code(conn: &mut PgConnection, task_id: Uuid) -> String {
    use kairos_db::schema::tasks;
    tasks::table
        .filter(tasks::id.eq(task_id))
        .select(tasks::short_code)
        .first(conn)
        .expect("code")
}

#[test]
fn a_watch_tells_of_each_coarse_change() {
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
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let scratch_url = with_database(&admin_url, &SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // --- the migration: re-runnable, and down then up gives the table ------
    conn.batch_execute(UP).expect("a second up changes nothing");
    conn.batch_execute(DOWN).expect("down");
    conn.batch_execute(UP).expect("up");
    conn.batch_execute(VALIDATE).expect("validate");

    let robin = insert_user(&mut conn, "robin", "Robin");
    let sam = insert_user(&mut conn, "sam", "Sam");
    let jamie = insert_user(&mut conn, "jamie", "Jamie");
    let ci = insert_service_account(&mut conn);
    {
        use kairos_db::schema::{organization_members, organizations};
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme");
        for user_id in [robin, sam, jamie, ci] {
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id,
                    role: OrgRole::Member,
                })
                .execute(&mut conn)
                .expect("membership");
        }
    }

    let board = delivery_board(&mut conn, "Core", "core", robin);
    let other_board = delivery_board(&mut conn, "Web", "web", robin);
    let initiatives = board_by_slug(&mut conn, "initiatives");
    let initiative = items::create_initiative(
        &mut conn,
        CreateInitiative {
            board_id: initiatives,
            column_id: None,
            title: "Shared work",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        robin,
    )
    .expect("creating initiative")
    .id;
    let initiative_code: String = {
        use kairos_db::schema::initiatives;
        initiatives::table
            .filter(initiatives::id.eq(initiative))
            .select(initiatives::short_code)
            .first(&mut conn)
            .expect("code")
    };
    let first = task(&mut conn, board, "First", robin);
    let second = task(&mut conn, board, "Second", robin);
    let loose = task(&mut conn, board, "Loose", robin);
    for child in [first, second] {
        graph::set_parent(&mut conn, child, Some(initiative), robin).expect("parent");
    }
    let (c1, c2, c3) = (
        code(&mut conn, first),
        code(&mut conn, second),
        code(&mut conn, loose),
    );

    // --- the watches --------------------------------------------------------
    // Sam watches the first task. Jamie watches the initiative with its
    // children. The service account watches the first task.
    let watch = watches::watch(&mut conn, sam, first, false).expect("watch");
    assert_eq!((watch.item_id, watch.with_children), (first, false));
    watches::watch(&mut conn, jamie, initiative, true).expect("watch");
    watches::watch(&mut conn, ci, first, false).expect("watch");
    assert_eq!(
        watches::watchers(&mut conn, first).expect("watchers").len(),
        3,
        "Sam, the service account, and Jamie by the parent"
    );
    assert_eq!(
        watches::watchers(&mut conn, loose).expect("watchers"),
        Vec::<Uuid>::new(),
        "nobody watches a task with no parent and no watch"
    );

    // --- moved --------------------------------------------------------------
    let todo = column(&mut conn, board, "Todo");
    boards::transition_task(&mut conn, first, todo, robin).expect("to Todo");
    assert_eq!(
        take(&mut conn, sam),
        vec![format!("Robin moved {c1} to Todo: First")]
    );
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("Robin moved {c1} to Todo: First")],
        "a watch with children tells of a move of a child"
    );
    assert!(
        take(&mut conn, ci).is_empty(),
        "a service account is never told"
    );

    // --- edited -------------------------------------------------------------
    edit(&mut conn, first, "The plan.", robin);
    assert_eq!(
        take(&mut conn, sam),
        vec![format!("Robin edited {c1}: First")]
    );
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("Robin edited {c1}: First")]
    );

    // --- note added ---------------------------------------------------------
    item_notes::append_note(&mut conn, ItemType::Task, first, "Started.", robin).expect("note");
    assert_eq!(
        take(&mut conn, sam),
        vec![format!("Robin added a note to {c1}: First")]
    );
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("Robin added a note to {c1}: First")]
    );

    // --- not the actor ------------------------------------------------------
    // Sam writes a note on the task that Sam watches: Sam is not told,
    // Jamie is.
    item_notes::append_note(&mut conn, ItemType::Task, first, "Mine.", sam).expect("note");
    assert!(
        take(&mut conn, sam).is_empty(),
        "the actor is not told of the change"
    );
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("Sam added a note to {c1}: First")]
    );
    // Jamie edits a child of the initiative that Jamie watches.
    edit(&mut conn, second, "Jamie's plan.", jamie);
    assert!(take(&mut conn, jamie).is_empty(), "the actor is not told");
    assert!(
        take(&mut conn, sam).is_empty(),
        "Sam does not watch the second task"
    );

    // --- done ---------------------------------------------------------------
    // A task of an initiative in Discovery cannot go to Active (the board
    // policy), so Sam watches a task with no parent for this step.
    let solo = task(&mut conn, board, "Solo", robin);
    let c5 = code(&mut conn, solo);
    watches::watch(&mut conn, sam, solo, false).expect("watch");
    let active = column(&mut conn, board, "Active");
    let completed = column(&mut conn, board, "Completed");
    for to in [todo, active] {
        boards::transition_task(&mut conn, solo, to, robin).expect("move");
    }
    take(&mut conn, sam);
    boards::transition_task(&mut conn, solo, completed, robin).expect("to Completed");
    assert_eq!(
        take(&mut conn, sam),
        vec![format!("{c5} is done (Completed), by Robin: Solo")]
    );
    watches::unwatch(&mut conn, sam, solo).expect("unwatch");

    // --- a cancel is done; a board move is a move ---------------------------
    task_cancellations::cancel_task(&mut conn, second, "Not needed.", robin).expect("cancel");
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("{c2} is done (Completed), by Robin: Second")]
    );
    let third = task(&mut conn, board, "Third", robin);
    graph::set_parent(&mut conn, third, Some(initiative), robin).expect("parent");
    let c4 = code(&mut conn, third);
    boards::move_task(&mut conn, third, other_board, robin).expect("board move");
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!("Robin moved {c4} to the board Web: Third")]
    );

    // --- the item itself, with children --------------------------------------
    item_notes::append_note(&mut conn, ItemType::Initiative, initiative, "Plan.", robin)
        .expect("note");
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!(
            "Robin added a note to {initiative_code}: Shared work"
        )]
    );

    // --- a watch without children: a child tells nothing ---------------------
    let rewatch = watches::watch(&mut conn, jamie, initiative, false).expect("watch");
    assert!(!rewatch.with_children, "a second watch changes the flag");
    edit(&mut conn, third, "More.", robin);
    assert!(
        take(&mut conn, jamie).is_empty(),
        "a watch without children tells nothing of a child"
    );
    item_notes::append_note(&mut conn, ItemType::Initiative, initiative, "More.", robin)
        .expect("note");
    assert_eq!(
        take(&mut conn, jamie),
        vec![format!(
            "Robin added a note to {initiative_code}: Shared work"
        )]
    );

    // --- no watch: nothing --------------------------------------------------
    edit(&mut conn, loose, "Nobody watches.", robin);
    for user in [robin, sam, jamie] {
        assert!(take(&mut conn, user).is_empty(), "{c3} has no watcher");
    }

    // --- the list, and the end of a watch ------------------------------------
    let listed = watches::list(&mut conn, sam).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].short_code.as_str(), listed[0].with_children),
        (c1.as_str(), false)
    );
    assert!(watches::unwatch(&mut conn, sam, first).expect("unwatch"));
    assert!(
        !watches::unwatch(&mut conn, sam, first).expect("unwatch"),
        "a second unwatch finds nothing"
    );
    assert_eq!(watches::watch_of(&mut conn, sam, first).expect("of"), None);
    item_notes::append_note(&mut conn, ItemType::Task, first, "After.", robin).expect("note");
    assert!(
        take(&mut conn, sam).is_empty(),
        "a person who stopped the watch is not told"
    );

    // --- a kind turned off ---------------------------------------------------
    kairos_db::notifications::set_settings(
        &mut conn,
        jamie,
        &[(kairos_core::notifications::NotificationKind::Watch, false)],
    )
    .expect("settings");
    item_notes::append_note(&mut conn, ItemType::Initiative, initiative, "Off.", robin)
        .expect("note");
    assert!(
        take(&mut conn, jamie).is_empty(),
        "a person who turned the kind off is not told"
    );
}
