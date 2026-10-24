//! KAIROS-T-0534 — a transition and a cancel lock the task row, so two
//! moves of one task at the same time cannot both pass:
//!
//! - two moves Todo -> Active: the second waits on the lock, then reads
//!   Active and is refused with INVALID_TRANSITION from Active; one claim,
//!   of the person of the first move;
//! - two cancels: the second waits, then is refused as done; one mark.
//!
//! Each case is in order, not by time: connection A makes its change in an
//! open transaction; a thread on connection B starts the same change; A
//! commits only when Postgres shows B waiting on a lock.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its
//! own scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Integer};
use uuid::Uuid;

use kairos_core::board::TransitionError;
use kairos_db::boards::{self, BoardError};
use kairos_db::items::CreateTask;
use kairos_db::models::enums::{ActivityAction, BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::task_cancellations::{self, CancelError};
use kairos_db::{create_board, items, provision_tenant, run_public_migrations, task_claims};

static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_transition_lock_t0534");

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

/// A connection to the scratch database, in the tenant schema.
fn tenant_conn(url: &str) -> PgConnection {
    let mut conn = PgConnection::establish(url).expect("connecting to scratch database");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    conn
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

fn seed_board(conn: &mut PgConnection, actor: Uuid) -> Uuid {
    use kairos_db::schema::teams;
    let team_id: Uuid = diesel::insert_into(teams::table)
        .values(NewTeam {
            name: "Web".to_string(),
            slug: "web".to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team");
    create_board(
        conn,
        BoardLevel::Delivery,
        "Web",
        "web-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(team_id),
        Some(actor),
    )
    .expect("creating delivery board")
    .id
}

fn column(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::name.eq(name))
        .filter(board_columns::deleted_at.is_null())
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name}: {e}"))
}

fn task_in_todo(conn: &mut PgConnection, board_id: Uuid, title: &str, actor: Uuid) -> Uuid {
    let task = items::create_task(
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
    .id;
    let todo = column(conn, board_id, "Todo");
    boards::transition_task(conn, task, todo, actor).expect("to Todo");
    task
}

fn activity(conn: &mut PgConnection, task: Uuid, action: ActivityAction) -> Vec<String> {
    use kairos_db::schema::activity_log;
    activity_log::table
        .filter(activity_log::entity_id.eq(task))
        .filter(activity_log::action.eq(action))
        .order(activity_log::occurred_at.asc())
        .select(activity_log::details)
        .load(conn)
        .expect("activity")
}

#[derive(QueryableByName)]
struct Pid {
    #[diesel(sql_type = Integer)]
    pid: i32,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Wait until the backend `pid` waits on a lock. Panics after 30 s.
fn wait_for_lock_wait(watcher: &mut PgConnection, pid: i32) {
    let start = Instant::now();
    loop {
        let waiting: Count = sql_query(
            "SELECT count(*) AS n FROM pg_stat_activity \
             WHERE pid = $1 AND wait_event_type = 'Lock'",
        )
        .bind::<Integer, _>(pid)
        .get_result(watcher)
        .expect("pg_stat_activity");
        if waiting.n == 1 {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "connection B never waited on a lock"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Run `first` on `conn_a` in an open transaction; start `second` on a new
/// connection in a thread; commit A when B waits on a lock. Gives the
/// result of `second`.
fn race<T: Send + 'static>(
    url: &str,
    conn_a: &mut PgConnection,
    first: impl FnOnce(&mut PgConnection),
    second: impl FnOnce(&mut PgConnection) -> T + Send + 'static,
) -> T {
    let mut watcher = tenant_conn(url);
    let (pid_tx, pid_rx) = mpsc::channel();
    let b_url = url.to_string();
    let b = conn_a
        .transaction::<_, diesel::result::Error, _>(|a| {
            first(a);
            let b = thread::spawn(move || {
                let mut conn_b = tenant_conn(&b_url);
                let me: Pid = sql_query("SELECT pg_backend_pid() AS pid")
                    .get_result(&mut conn_b)
                    .expect("pid");
                pid_tx.send(me.pid).expect("send pid");
                second(&mut conn_b)
            });
            let pid = pid_rx.recv().expect("pid of B");
            wait_for_lock_wait(&mut watcher, pid);
            Ok(b)
        })
        .expect("transaction A");
    b.join().expect("thread B")
}

#[test]
fn two_moves_of_one_task_cannot_both_pass() {
    let admin_url = scratch::admin_database_url();
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
    let url = with_database(&admin_url, &SCRATCH_DB);
    {
        let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    }
    let mut conn = tenant_conn(&url);
    let alice = insert_user(&mut conn, "alice", "Alice");
    let bob = insert_user(&mut conn, "bob", "Bob");
    let board = seed_board(&mut conn, alice);
    let active = column(&mut conn, board, "Active");

    // --- two moves Todo -> Active ---------------------------------------
    let task = task_in_todo(&mut conn, board, "Raced", alice);
    let second = race(
        &url,
        &mut conn,
        |a| boards::transition_task(a, task, active, alice).expect("A moves the task"),
        move |b| boards::transition_task(b, task, active, bob),
    );
    match second {
        Err(BoardError::Transition(TransitionError::NotAllowed {
            from,
            to,
            allowed_targets,
            ..
        })) => {
            assert_eq!(from.name, "Active", "B reads the column of A's move");
            assert_eq!(to.name, "Active");
            assert!(!allowed_targets.is_empty(), "the targets from Active");
        }
        other => panic!("B is refused with INVALID_TRANSITION, got {other:?}"),
    }
    let claim = task_claims::claim_of(&mut conn, task)
        .expect("claim")
        .expect("the task has a claim");
    assert_eq!(claim.user_id, alice, "the claim is A's");
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Claim),
        vec![format!("claim:{alice}")],
        "one claim row"
    );
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Transition)
            .iter()
            .filter(|d| d.ends_with("->Active"))
            .count(),
        1,
        "one move to Active"
    );

    // --- two cancels ------------------------------------------------------
    let task = task_in_todo(&mut conn, board, "Cancelled twice", alice);
    let second = race(
        &url,
        &mut conn,
        |a| {
            task_cancellations::cancel_task(a, task, "first", alice).expect("A cancels");
        },
        move |b| task_cancellations::cancel_task(b, task, "second", bob),
    );
    assert!(
        matches!(second, Err(CancelError::AlreadyDone { .. })),
        "B is refused as done, got {second:?}"
    );
    let mark = task_cancellations::mark_of(&mut conn, task)
        .expect("mark")
        .expect("the task has a mark");
    assert_eq!(mark.cancelled_by, alice);
    assert_eq!(mark.reason, "first");
    assert_eq!(activity(&mut conn, task, ActivityAction::Cancel).len(), 1);
}
