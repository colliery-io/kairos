//! Integration test for the board rules engine and defaults seeding
//! (KAIROS-T-0010, contract per KAIROS-A-0002, layering per KAIROS-A-0009).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration`); the database is never mocked
//! (KAIROS-A-0012). For isolation the test drops and recreates a dedicated
//! scratch database (`kairos_board_rules_test`) on the same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - default boards match A-0002 exactly, INCLUDING the transition graphs
//!   (strategy/initiative/adr forward-only chains; delivery forward chain
//!   plus Todo<->Blocked and Active<->Blocked bidirectionals via
//!   `create_board` for a team's delivery board)
//! - item transitions: allowed iff a `board_transitions` edge exists;
//!   valid moves update `column_id` and write an `activity_log` row
//!   (`action='transition'`, `details='column:<From>-><To>'`); invalid
//!   moves return the typed error carrying the allowed target columns
//! - column management: removal only when empty, add with unique
//!   name/position, rename, reorder — each writing `board_config` activity
//! - dead-end detection flags columns after their outbound transitions are
//!   removed

use std::collections::BTreeSet;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::Text;
use uuid::Uuid;

use kairos_core::board::{ColumnRuleError, TransitionError};
use kairos_db::boards::BoardError;
use kairos_db::models::{
    ActivityAction, BoardLevel, NewAdr, NewInitiative, NewStrategy, NewTask, NewTeam, NewUser,
    TaskType, Team, User,
};
use kairos_db::{boards, provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_board_rules_test";

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

/// The board with this slug in the current tenant schema.
fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::boards::table
        .filter(schema::boards::slug.eq(slug))
        .select(schema::boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

/// The LIVE column named `name` on `board`. Names are unique per board
/// only among live columns (KAIROS-T-0161), so the filter is what makes
/// this single-valued once a column has been removed and re-added.
fn column_id_by_name(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .filter(schema::board_columns::name.eq(name))
        .filter(schema::board_columns::deleted_at.is_null())
        .select(schema::board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name:?} not found: {e}"))
}

/// Live column names of `board` in position order — what the board is.
fn column_names(conn: &mut PgConnection, board: Uuid) -> Vec<String> {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .filter(schema::board_columns::deleted_at.is_null())
        .order(schema::board_columns::position.asc())
        .select(schema::board_columns::name)
        .load(conn)
        .expect("loading columns")
}

/// The name stored on a column row, removed ones included — the audit
/// lookup an archived card depends on (KAIROS-T-0161).
fn column_name_of(conn: &mut PgConnection, column: Uuid) -> String {
    schema::board_columns::table
        .filter(schema::board_columns::id.eq(column))
        .select(schema::board_columns::name)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {column} row is gone: {e}"))
}

#[derive(QueryableByName)]
struct NameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

/// Transition pairs `"From -> To"` for a board (current search_path
/// schema) — every stored row, whether or not both endpoints are still
/// live. Use [`live_transition_pairs`] for the graph the board actually
/// runs on.
fn transition_pairs(conn: &mut PgConnection, board: Uuid) -> BTreeSet<String> {
    sql_query(
        "SELECT (f.name || ' -> ' || t.name)::text AS name \
         FROM board_transitions tr \
         JOIN board_columns f ON f.id = tr.from_column_id \
         JOIN board_columns t ON t.id = tr.to_column_id \
         WHERE tr.board_id = $1::uuid",
    )
    .bind::<Text, _>(board.to_string())
    .load::<NameRow>(conn)
    .expect("loading transitions")
    .into_iter()
    .map(|r| r.name)
    .collect()
}

/// Transition pairs between LIVE columns only — the edges a move is
/// actually validated against (KAIROS-T-0161: a soft-deleted column
/// cascades nothing, so its edges are filtered, not deleted).
fn live_transition_pairs(conn: &mut PgConnection, board: Uuid) -> BTreeSet<String> {
    sql_query(
        "SELECT (f.name || ' -> ' || t.name)::text AS name \
         FROM board_transitions tr \
         JOIN board_columns f ON f.id = tr.from_column_id \
         JOIN board_columns t ON t.id = tr.to_column_id \
         WHERE tr.board_id = $1::uuid \
           AND f.deleted_at IS NULL AND t.deleted_at IS NULL",
    )
    .bind::<Text, _>(board.to_string())
    .load::<NameRow>(conn)
    .expect("loading transitions")
    .into_iter()
    .map(|r| r.name)
    .collect()
}

fn pairs(list: &[(&str, &str)]) -> BTreeSet<String> {
    list.iter().map(|(f, t)| format!("{f} -> {t}")).collect()
}

/// The `column_id` an item row currently occupies.
macro_rules! item_column {
    ($conn:expr, $table:ident, $id:expr) => {
        schema::$table::table
            .filter(schema::$table::id.eq($id))
            .select(schema::$table::column_id)
            .first::<Uuid>($conn)
            .expect("loading item column")
    };
}

/// All `activity_log.details` values for (action, entity_id).
fn activity_details(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq(action.as_str()))
        .filter(schema::activity_log::entity_id.eq(entity))
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading activity_log")
}

fn dead_end_names(conn: &mut PgConnection, board: Uuid) -> Vec<String> {
    boards::dead_end_columns(conn, board)
        .expect("dead_end_columns")
        .into_iter()
        .map(|c| c.name)
        .collect()
}

#[test]
fn board_rules_lifecycle() {
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

    // Fresh tenant, then work inside its schema (same mechanism as the
    // pool: unqualified tenant tables resolve via search_path).
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // An actor for transitions/config changes (activity_log.actor_id).
    let actor: User = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|it-actor".into(),
            email: "actor@example.com".into(),
            display_name: "IT Actor".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("inserting actor user");
    let actor_id = actor.id;

    // ---- A-0002 defaults: provisioned boards' transition graphs -----------
    let strategy_board = board_id_by_slug(&mut conn, "strategy");
    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let adr_board = board_id_by_slug(&mut conn, "adrs");

    assert_eq!(
        column_names(&mut conn, strategy_board),
        ["Draft", "Review", "Active", "Monitoring", "Completed"]
    );
    assert_eq!(
        transition_pairs(&mut conn, strategy_board),
        pairs(&[
            ("Draft", "Review"),
            ("Review", "Active"),
            ("Active", "Monitoring"),
            ("Monitoring", "Completed"),
        ]),
        "strategy board is the A-0002 forward-only chain"
    );
    assert_eq!(
        column_names(&mut conn, initiative_board),
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
        transition_pairs(&mut conn, initiative_board),
        pairs(&[
            ("Discovery", "Design"),
            ("Design", "Ready"),
            ("Ready", "Decompose"),
            ("Decompose", "Active"),
            ("Active", "Monitoring"),
            ("Monitoring", "Completed"),
        ]),
        "initiative board is the A-0002 forward-only chain"
    );
    assert_eq!(
        column_names(&mut conn, adr_board),
        ["Draft", "Discussion", "Decided", "Superseded"]
    );
    assert_eq!(
        transition_pairs(&mut conn, adr_board),
        pairs(&[
            ("Draft", "Discussion"),
            ("Discussion", "Decided"),
            ("Decided", "Superseded"),
        ]),
        "adr board is the A-0002 forward-only chain"
    );

    // ---- create_board: per-team delivery board from the same defaults -----
    let team: Team = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Team Alpha".into(),
            slug: "alpha".into(),
            team_type: kairos_db::models::TeamType::StreamAligned,
        })
        .returning(Team::as_returning())
        .get_result(&mut conn)
        .expect("inserting team");

    let delivery = boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Alpha Delivery",
        "alpha-delivery",
        Some(team.id),
        Some(actor_id),
    )
    .expect("creating delivery board");
    assert_eq!(delivery.board_level, BoardLevel::Delivery);
    assert_eq!(delivery.team_id, Some(team.id));
    assert_eq!(
        column_names(&mut conn, delivery.id),
        ["Backlog", "Todo", "Blocked", "Active", "Completed"]
    );
    assert_eq!(
        transition_pairs(&mut conn, delivery.id),
        pairs(&[
            ("Backlog", "Todo"),
            ("Todo", "Active"),
            ("Active", "Completed"),
            ("Todo", "Blocked"),
            ("Blocked", "Todo"),
            ("Active", "Blocked"),
            ("Blocked", "Active"),
        ]),
        "delivery board: A-0002 forward chain plus Todo<->Blocked and Active<->Blocked"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Create, delivery.id),
        ["board:alpha-delivery level:delivery"],
        "board creation by a user writes an activity row"
    );

    // ---- item transitions: strategy ----------------------------------------
    let draft = column_id_by_name(&mut conn, strategy_board, "Draft");
    let review = column_id_by_name(&mut conn, strategy_board, "Review");
    let strategy_id: Uuid = diesel::insert_into(schema::strategies::table)
        .values(NewStrategy {
            short_code: "S-0001".into(),
            title: "Prove the board engine".into(),
            content: "".into(),
            board_id: strategy_board,
            column_id: draft,
            hypothesis: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::strategies::id)
        .get_result(&mut conn)
        .expect("inserting strategy");

    // Valid: Draft -> Review (edge exists). Column updated + audit row.
    boards::transition_strategy(&mut conn, strategy_id, review, actor_id)
        .expect("Draft -> Review must be allowed");
    assert_eq!(item_column!(&mut conn, strategies, strategy_id), review);
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Transition, strategy_id),
        ["column:Draft->Review"]
    );

    // Invalid: Review -> Draft (no edge). Typed error with allowed targets.
    let err = boards::transition_strategy(&mut conn, strategy_id, draft, actor_id)
        .expect_err("Review -> Draft must be rejected");
    match err {
        BoardError::Transition(TransitionError::NotAllowed {
            from,
            to,
            allowed_targets,
        }) => {
            assert_eq!(from.name, "Review");
            assert_eq!(to.name, "Draft");
            assert_eq!(
                allowed_targets
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>(),
                ["Active"],
                "the error carries the allowed target columns (S-0006 REQ-1.4)"
            );
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
    // The rejected move changed nothing and wrote no audit row.
    assert_eq!(item_column!(&mut conn, strategies, strategy_id), review);
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Transition, strategy_id).len(),
        1
    );

    // A target that is not a column of the board is also typed.
    let stranger = Uuid::new_v4();
    let err = boards::transition_strategy(&mut conn, strategy_id, stranger, actor_id)
        .expect_err("unknown target column must be rejected");
    assert!(
        matches!(
            err,
            BoardError::Transition(TransitionError::UnknownToColumn(id)) if id == stranger
        ),
        "expected UnknownToColumn, got {err:?}"
    );

    // ---- item transitions: initiative (macro sibling) ----------------------
    let discovery = column_id_by_name(&mut conn, initiative_board, "Discovery");
    let design = column_id_by_name(&mut conn, initiative_board, "Design");
    let initiative_id: Uuid = diesel::insert_into(schema::initiatives::table)
        .values(NewInitiative {
            short_code: "I-0001".into(),
            title: "Board engine".into(),
            content: "".into(),
            board_id: initiative_board,
            column_id: discovery,
            complexity: None,
            is_bucket: false,
            bucket_type: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::initiatives::id)
        .get_result(&mut conn)
        .expect("inserting initiative");
    boards::transition_initiative(&mut conn, initiative_id, design, actor_id)
        .expect("Discovery -> Design must be allowed");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Transition, initiative_id),
        ["column:Discovery->Design"]
    );

    // ---- item transitions: task on the delivery board (bidirectionals) -----
    let backlog = column_id_by_name(&mut conn, delivery.id, "Backlog");
    let todo = column_id_by_name(&mut conn, delivery.id, "Todo");
    let blocked = column_id_by_name(&mut conn, delivery.id, "Blocked");
    let task_id: Uuid = diesel::insert_into(schema::tasks::table)
        .values(NewTask {
            short_code: "T-0001".into(),
            title: "Ship it".into(),
            content: "".into(),
            board_id: delivery.id,
            column_id: backlog,
            task_type: TaskType::Task,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: Some(team.id),
            repository_id: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::tasks::id)
        .get_result(&mut conn)
        .expect("inserting task");

    boards::transition_task(&mut conn, task_id, todo, actor_id).expect("Backlog -> Todo");
    boards::transition_task(&mut conn, task_id, blocked, actor_id).expect("Todo -> Blocked");
    boards::transition_task(&mut conn, task_id, todo, actor_id)
        .expect("Blocked -> Todo (bidirectional back-edge)");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Transition, task_id),
        [
            "column:Backlog->Todo",
            "column:Todo->Blocked",
            "column:Blocked->Todo"
        ]
    );

    // ---- item transitions: adr (nullable board placement) ------------------
    let adr_draft = column_id_by_name(&mut conn, adr_board, "Draft");
    let adr_discussion = column_id_by_name(&mut conn, adr_board, "Discussion");
    let adr_id: Uuid = diesel::insert_into(schema::adrs::table)
        .values(NewAdr {
            short_code: "A-0001".into(),
            title: "Use the board engine".into(),
            content: "".into(),
            board_id: Some(adr_board),
            column_id: Some(adr_draft),
            decision_maker: None,
            decision_date: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::adrs::id)
        .get_result(&mut conn)
        .expect("inserting adr");
    boards::transition_adr(&mut conn, adr_id, adr_discussion, actor_id)
        .expect("Draft -> Discussion");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::Transition, adr_id),
        ["column:Draft->Discussion"]
    );

    let off_board_adr: Uuid = diesel::insert_into(schema::adrs::table)
        .values(NewAdr {
            short_code: "A-0002".into(),
            title: "Not on a board".into(),
            content: "".into(),
            board_id: None,
            column_id: None,
            decision_maker: None,
            decision_date: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::adrs::id)
        .get_result(&mut conn)
        .expect("inserting off-board adr");
    let err = boards::transition_adr(&mut conn, off_board_adr, adr_discussion, actor_id)
        .expect_err("an ADR without a board cannot transition");
    assert!(
        matches!(err, BoardError::ItemNotOnBoard { entity_type: "adr", id } if id == off_board_adr),
        "expected ItemNotOnBoard, got {err:?}"
    );

    // ---- column management on the strategy board ---------------------------
    // Dead ends before any change: only the terminal column.
    assert_eq!(dead_end_names(&mut conn, strategy_board), ["Completed"]);

    // Removing a non-empty column is rejected (the strategy sits in Review).
    let err = boards::remove_column(&mut conn, review, actor_id)
        .expect_err("removing a non-empty column must fail");
    match err {
        BoardError::Rule(ColumnRuleError::ColumnNotEmpty { column, item_count }) => {
            assert_eq!(column.id, review);
            assert_eq!(column.name, "Review");
            assert_eq!(item_count, 1);
        }
        other => panic!("expected ColumnNotEmpty, got {other:?}"),
    }
    assert_eq!(
        column_names(&mut conn, strategy_board),
        ["Draft", "Review", "Active", "Monitoring", "Completed"],
        "rejected removal changed nothing"
    );

    // Add: unique name + position enforced.
    let spike = boards::add_column(&mut conn, strategy_board, "Spike", 5, actor_id)
        .expect("adding a Spike column");
    assert_eq!((spike.name.as_str(), spike.position), ("Spike", 5));
    let err = boards::add_column(&mut conn, strategy_board, "Spike", 6, actor_id)
        .expect_err("duplicate column name must fail");
    assert!(
        matches!(&err, BoardError::Rule(ColumnRuleError::DuplicateName(n)) if n == "Spike"),
        "expected DuplicateName, got {err:?}"
    );
    let err = boards::add_column(&mut conn, strategy_board, "Parking", 5, actor_id)
        .expect_err("duplicate column position must fail");
    assert!(
        matches!(err, BoardError::Rule(ColumnRuleError::DuplicatePosition(5))),
        "expected DuplicatePosition, got {err:?}"
    );

    // Rename: uniqueness enforced.
    boards::rename_column(&mut conn, spike.id, "Parking Lot", actor_id).expect("renaming Spike");
    let err = boards::rename_column(&mut conn, spike.id, "Draft", actor_id)
        .expect_err("renaming to an existing name must fail");
    assert!(
        matches!(&err, BoardError::Rule(ColumnRuleError::DuplicateName(n)) if n == "Draft"),
        "expected DuplicateName, got {err:?}"
    );

    // The new column has no outbound transitions -> flagged as a dead end.
    assert_eq!(
        dead_end_names(&mut conn, strategy_board),
        ["Completed", "Parking Lot"]
    );

    // Transition add: mirrors the DDL constraints (typed, not a DB error).
    let err = boards::add_transition(&mut conn, strategy_board, spike.id, spike.id, actor_id)
        .expect_err("self transition must fail");
    assert!(
        matches!(err, BoardError::Rule(ColumnRuleError::SelfTransition)),
        "expected SelfTransition, got {err:?}"
    );
    boards::add_transition(&mut conn, strategy_board, spike.id, draft, actor_id)
        .expect("adding Parking Lot -> Draft");
    let err = boards::add_transition(&mut conn, strategy_board, spike.id, draft, actor_id)
        .expect_err("duplicate transition must fail");
    assert!(
        matches!(err, BoardError::Rule(ColumnRuleError::DuplicateTransition)),
        "expected DuplicateTransition, got {err:?}"
    );
    // Wired in, the new column is no longer a dead end.
    assert_eq!(dead_end_names(&mut conn, strategy_board), ["Completed"]);

    // Removing a column's outbound transition flags it as a dead end.
    let monitoring = column_id_by_name(&mut conn, strategy_board, "Monitoring");
    let completed = column_id_by_name(&mut conn, strategy_board, "Completed");
    boards::remove_transition(&mut conn, strategy_board, monitoring, completed, actor_id)
        .expect("removing Monitoring -> Completed");
    assert_eq!(
        dead_end_names(&mut conn, strategy_board),
        ["Monitoring", "Completed"],
        "dead-end detection flags the column whose outbound transition was removed"
    );
    let err = boards::remove_transition(&mut conn, strategy_board, monitoring, completed, actor_id)
        .expect_err("removing a non-existent transition must fail");
    assert!(
        matches!(err, BoardError::TransitionNotFound { .. }),
        "expected TransitionNotFound, got {err:?}"
    );

    // Reorder: must list every column exactly once; positions become 0..n.
    let err = boards::reorder_columns(&mut conn, strategy_board, &[draft, review], actor_id)
        .expect_err("partial reorder must fail");
    assert!(
        matches!(
            err,
            BoardError::Rule(ColumnRuleError::ReorderLengthMismatch {
                expected: 6,
                actual: 2
            })
        ),
        "expected ReorderLengthMismatch, got {err:?}"
    );
    let active = column_id_by_name(&mut conn, strategy_board, "Active");
    boards::reorder_columns(
        &mut conn,
        strategy_board,
        &[spike.id, draft, review, active, monitoring, completed],
        actor_id,
    )
    .expect("reordering Parking Lot to the front");
    assert_eq!(
        column_names(&mut conn, strategy_board),
        [
            "Parking Lot",
            "Draft",
            "Review",
            "Active",
            "Monitoring",
            "Completed"
        ]
    );

    // Removing an EMPTY column succeeds. KAIROS-T-0161: removal is a soft
    // delete, so the row and its transition edges both survive — the board
    // simply stops counting them.
    boards::remove_column(&mut conn, spike.id, actor_id).expect("removing empty Parking Lot");
    assert_eq!(
        column_names(&mut conn, strategy_board),
        ["Draft", "Review", "Active", "Monitoring", "Completed"]
    );
    assert_eq!(
        column_name_of(&mut conn, spike.id),
        "Parking Lot",
        "the removed column's row survives, name intact"
    );
    assert!(
        transition_pairs(&mut conn, strategy_board).contains("Parking Lot -> Draft"),
        "removing a column must no longer destroy the board's wiring"
    );
    assert!(
        !live_transition_pairs(&mut conn, strategy_board).contains("Parking Lot -> Draft"),
        "but the edge is filtered out of the live graph"
    );
    let err = boards::transition_strategy(&mut conn, strategy_id, spike.id, actor_id)
        .expect_err("a removed column is not a legal transition target");
    assert!(
        matches!(
            err,
            BoardError::Transition(TransitionError::UnknownToColumn(id)) if id == spike.id
        ),
        "expected UnknownToColumn, got {err:?}"
    );
    // Its name and position are free again — the uniqueness that matters
    // is uniqueness among the columns the board HAS, which is why the DDL
    // constraints became partial indexes. Position 0 is the one the
    // removed row still occupies.
    let spike_again = boards::add_column(&mut conn, strategy_board, "Parking Lot", 0, actor_id)
        .expect("re-adding a column under the removed one's name and position");
    boards::remove_column(&mut conn, spike_again.id, actor_id).expect("and removing it again");
    assert_eq!(
        dead_end_names(&mut conn, strategy_board),
        ["Monitoring", "Completed"]
    );

    // Every board config change wrote a board_config activity row, in order.
    assert_eq!(
        activity_details(&mut conn, ActivityAction::BoardConfig, strategy_board),
        [
            "column_add:Spike@5",
            "column_rename:Spike->Parking Lot",
            "transition_add:Parking Lot->Draft",
            "transition_remove:Monitoring->Completed",
            "columns_reorder:Parking Lot,Draft,Review,Active,Monitoring,Completed",
            "column_remove:Parking Lot",
            "column_add:Parking Lot@0",
            "column_remove:Parking Lot",
        ]
    );

    // ---- KAIROS-T-0161: an archived card stops pinning its column open ----
    // The delivery board's Blocked column, holding one live task and one
    // archived one. Removal must refuse while the live card is there and
    // succeed once it is not — and afterwards the archived card must still
    // be able to say which column it was put away in (ADR-20: archived
    // means hidden, not gone).
    boards::transition_task(&mut conn, task_id, blocked, actor_id).expect("Todo -> Blocked");
    let archived_task: Uuid = diesel::insert_into(schema::tasks::table)
        .values(NewTask {
            short_code: "T-0002".into(),
            title: "Finished work, put away".into(),
            content: "".into(),
            board_id: delivery.id,
            column_id: blocked,
            task_type: TaskType::Task,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: Some(team.id),
            repository_id: None,
            created_by: actor_id,
            updated_by: actor_id,
        })
        .returning(schema::tasks::id)
        .get_result(&mut conn)
        .expect("inserting the card that will be archived");
    diesel::update(schema::tasks::table.filter(schema::tasks::id.eq(archived_task)))
        .set(schema::tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("archiving it");

    let err = boards::remove_column(&mut conn, blocked, actor_id)
        .expect_err("a live card still blocks removal");
    match err {
        BoardError::Rule(ColumnRuleError::ColumnNotEmpty { column, item_count }) => {
            assert_eq!(column.name, "Blocked");
            assert_eq!(
                item_count, 1,
                "only the live card counts; archived work is not live work"
            );
        }
        other => panic!("expected ColumnNotEmpty, got {other:?}"),
    }

    // Move the live card out. The archived one stays behind, holding the
    // NOT NULL FK that used to make this removal impossible outright.
    boards::transition_task(&mut conn, task_id, todo, actor_id).expect("Blocked -> Todo");
    boards::remove_column(&mut conn, blocked, actor_id)
        .expect("a column whose only occupants are archived can be removed");

    assert_eq!(
        column_names(&mut conn, delivery.id),
        ["Backlog", "Todo", "Active", "Completed"],
        "the removed column is gone from the live board"
    );
    let placement: Uuid = schema::tasks::table
        .filter(schema::tasks::id.eq(archived_task))
        .select(schema::tasks::column_id)
        .first(&mut conn)
        .expect("the archived card kept its placement");
    assert_eq!(placement, blocked);
    assert_eq!(
        column_name_of(&mut conn, blocked),
        "Blocked",
        "and the column it was put away in still has a name to report"
    );
    // Todo <-> Blocked survived as rows, and the live graph no longer
    // offers either direction.
    assert!(
        transition_pairs(&mut conn, delivery.id).contains("Todo -> Blocked"),
        "the edges were filtered, not cascaded away"
    );
    assert!(
        !live_transition_pairs(&mut conn, delivery.id).contains("Todo -> Blocked"),
        "but Blocked is not reachable any more"
    );

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
