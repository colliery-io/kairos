//! KAIROS-T-0637 — one queued write-back job for each key, a success write
//! of a webhook target that changes nothing writes nothing, and the vectors
//! of an index upload in one statement.
//!
//! Proves:
//! 1. Two transitions at the same time, of two tasks that link to one pull
//!    request, add one `github_writeback` job. The second waits for the
//!    first, and adds nothing when the first commits.
//! 2. Two queues of one issue at the same time (a change of the task, and
//!    a new write token) add one `issue_writeback` job.
//! 3. A job that runs does not count: a change while it runs adds a job.
//!    When the job that runs goes back to the queue (release, defer,
//!    requeue) next to that waiting job, it ends `done`. With no waiting
//!    job it is queued again.
//! 4. The migration that ends the queued twins keeps the oldest.
//! 5. A successful try on a target with no failures does not write the
//!    row of the target; after a failure it sets the count to 0.
//! 6. `code_indexes::put` gives the vector of the upload to each row that
//!    has none, and keeps the vector of a row that has one.
//!
//! Runs against the compose Postgres (`angreal services up`); owns the
//! scratch database `kairos_writeback_queued_key_t0637_test`.

#[path = "common/scratch.rs"]
mod scratch;

use std::thread;
use std::time::{Duration, Instant};

use diesel::connection::{AnsiTransactionManager, SimpleConnection, TransactionManager};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_db::code_indexes::{self, NewCodeIndex, PoolRow};
use kairos_db::items::CreateTask;
use kairos_db::jobs;
use kairos_db::models::enums::{BoardLevel, Forge, TaskType, WorkClass};
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::webhooks::{self, NewTarget, SealedSecret, TryResult};
use kairos_db::writeback::{self, NewWriteToken};
use kairos_db::{
    boards, create_board, issue_writeback, items, provision_tenant, repositories,
    run_public_migrations,
};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
static SCRATCH_DB: scratch::ScratchName =
    scratch::ScratchName::new("kairos_writeback_queued_key_t0637_test");
const DEDUPE_SQL: &str =
    include_str!("../migrations/public/2026-10-18-012000_writeback_jobs_dedupe/up.sql");
const PR_INDEX_DOWN: &str =
    include_str!("../migrations/public/2026-10-18-012100_writeback_jobs_pr_key/down.sql");
const PR_INDEX_UP: &str =
    include_str!("../migrations/public/2026-10-18-012100_writeback_jobs_pr_key/up.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

/// A connection to the scratch database, pinned to the tenant `acme`.
fn tenant_conn(url: &str) -> PgConnection {
    let mut conn = PgConnection::establish(url).expect("connecting to scratch db");
    conn.batch_execute("SET search_path TO org_acme, public")
        .expect("pinning search_path");
    conn
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn queued(conn: &mut PgConnection, kind: &str) -> i64 {
    sql_query("SELECT count(*) AS count FROM public.jobs WHERE kind = $1 AND state = 'queued'")
        .bind::<Text, _>(kind)
        .get_result::<CountRow>(conn)
        .expect("queued count")
        .count
}

/// Wait until a session of the scratch database waits for a lock, and
/// return its statement (the second transaction, on the index entry of the
/// first). `None` after 10 s.
fn wait_for_lock_waiter(conn: &mut PgConnection) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let waiting: Vec<DbName> = sql_query(
            "SELECT query AS name FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .load(conn)
        .expect("pg_stat_activity");
        if let Some(row) = waiting.into_iter().next() {
            return Some(row.name);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Run `first` in a transaction on `a`, then `second` in a transaction on
/// another connection of `url` while the first is open, then commit the
/// first. The results of both, and the statement that the second waited
/// in.
fn race<F, S>(url: &str, a: &mut PgConnection, first: F, second: S) -> (usize, usize, String)
where
    F: FnOnce(&mut PgConnection) -> usize,
    S: FnOnce(&mut PgConnection) -> usize + Send + 'static,
{
    AnsiTransactionManager::begin_transaction(a).expect("begin first");
    let first_result = first(a);
    let mut watch = tenant_conn(url);
    let url = url.to_string();
    let other = thread::spawn(move || {
        let mut b = tenant_conn(&url);
        AnsiTransactionManager::begin_transaction(&mut b).expect("begin second");
        let result = second(&mut b);
        AnsiTransactionManager::commit_transaction(&mut b).expect("commit second");
        result
    });
    let waited = wait_for_lock_waiter(&mut watch).unwrap_or_default();
    AnsiTransactionManager::commit_transaction(a).expect("commit first");
    let second_result = other.join().expect("second thread");
    (first_result, second_result, waited)
}

#[derive(QueryableByName)]
struct DbName {
    #[diesel(sql_type = Text)]
    name: String,
}

fn insert_user(conn: &mut PgConnection) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "dex|robin".to_string(),
            user_name: "robin".to_string(),
            email: "robin@acme.test".to_string(),
            display_name: "Robin".to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn column(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)
        .expect("column")
}

fn task(conn: &mut PgConnection, board: Uuid, title: &str, actor: Uuid) -> Uuid {
    items::create_task(
        conn,
        CreateTask {
            board_id: board,
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

fn link_pr(conn: &mut PgConnection, item: Uuid, connection: Uuid, number: i64) {
    sql_query(
        "INSERT INTO item_links (item_id, connection_id, kind, external_id, title, url, state, \
             author, forge_updated_at) \
         VALUES ($1, $2, 'pull_request', $3, 'Work', 'https://github.com/acme/site/pull/7', \
             'open', 'robin', now())",
    )
    .bind::<SqlUuid, _>(item)
    .bind::<SqlUuid, _>(connection)
    .bind::<Text, _>(number.to_string())
    .execute(conn)
    .expect("linking the pull request");
}

#[test]
fn write_back_jobs_dedupe_on_the_queued_key() {
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch db");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch db");
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let url = with_database(&admin_url, &SCRATCH_DB);
    {
        let mut conn = PgConnection::establish(&url).expect("connecting to scratch db");
        run_public_migrations(&mut conn).expect("public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    }
    let mut conn = tenant_conn(&url);

    // A team, its board, a GitHub repository with a write token, and a
    // connection.
    let robin = insert_user(&mut conn);
    let team: Uuid = {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(NewTeam {
                name: "Platform".to_string(),
                slug: "platform".to_string(),
                team_type: TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result(&mut conn)
            .expect("team")
    };
    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform",
        "platform-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(team),
        Some(robin),
    )
    .expect("board")
    .id;
    let repo = repositories::create(
        &mut conn,
        NewRepository {
            slug: "site".to_string(),
            forge: Forge::Github,
            repo_full_name: "acme/site".to_string(),
            repo_url: "https://github.com/acme/site".to_string(),
            default_branch: "main".to_string(),
            team_id: team,
            description: String::new(),
            created_by: robin,
            updated_by: robin,
        },
    )
    .expect("repository")
    .id;
    writeback::put_token(
        &mut conn,
        &NewWriteToken {
            repository_id: repo,
            ciphertext: vec![1, 2, 3],
            nonce: vec![0; 12],
            key_id: "k1".to_string(),
            set_by: robin,
        },
        "site",
    )
    .expect("write token");
    let connection: Uuid = sql_query(
        "INSERT INTO forge_connections (forge, created_by, repository_id) \
         VALUES ('github', $1, $2) RETURNING id AS name",
    )
    .bind::<SqlUuid, _>(robin)
    .bind::<SqlUuid, _>(repo)
    .get_result::<IdRow>(&mut conn)
    .expect("connection")
    .name;

    // --- 1. two transitions at the same time: one job -----------------------
    let a = task(&mut conn, board, "Task A", robin);
    let b = task(&mut conn, board, "Task B", robin);
    link_pr(&mut conn, a, connection, 7);
    link_pr(&mut conn, b, connection, 7);
    let todo = column(&mut conn, board, "Todo");
    let (first, _, waited) = race(
        &url,
        &mut conn,
        move |c| {
            boards::transition_task(c, a, todo, robin).expect("move A");
            1
        },
        move |c| {
            boards::transition_task(c, b, todo, robin).expect("move B");
            1
        },
    );
    assert_eq!(first, 1);
    assert!(
        waited.contains("INSERT INTO public.jobs"),
        "the second transition waits for the job of the first: {waited:?}"
    );
    assert_eq!(
        queued(&mut conn, writeback::JOB_KIND),
        1,
        "two transitions of one pull request add one job"
    );

    // --- 3. a job that runs does not count ---------------------------------
    let instance = Uuid::new_v4();
    let kinds = vec![writeback::JOB_KIND.to_string()];
    let leased = jobs::take(&mut conn, instance, &kinds, 60.0)
        .expect("take")
        .expect("a job");
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 0);
    assert!(
        writeback::queue_pr(&mut conn, repo, 7).expect("queue"),
        "a change while the job runs adds a job"
    );
    assert!(
        !writeback::queue_pr(&mut conn, repo, 7).expect("queue again"),
        "one job waits for each key"
    );
    let row = jobs::release(&mut conn, leased.id, instance, "The forge said 502.", 0.0)
        .expect("release")
        .expect("the lease");
    assert_eq!(row.state, "done", "the waiting job does the write");
    assert_eq!(
        row.last_error.as_deref(),
        Some(format!("The forge said 502. {}", jobs::SUPERSEDED).as_str())
    );
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 1);

    // defer and requeue end the same way.
    let leased = jobs::take(&mut conn, instance, &kinds, 60.0)
        .expect("take")
        .expect("a job");
    assert!(writeback::queue_pr(&mut conn, repo, 7).expect("queue"));
    assert!(jobs::defer(&mut conn, leased.id, instance, "Not here.", 0.0).expect("defer"));
    assert_eq!(
        jobs::get(&mut conn, leased.id).unwrap().unwrap().state,
        "done"
    );
    let leased = jobs::take(&mut conn, instance, &kinds, 60.0)
        .expect("take")
        .expect("a job");
    assert!(writeback::queue_pr(&mut conn, repo, 7).expect("queue"));
    assert!(jobs::requeue(&mut conn, leased.id, instance).expect("requeue"));
    let ended = jobs::get(&mut conn, leased.id).unwrap().unwrap();
    assert_eq!(ended.state, "done");
    assert_eq!(ended.last_error.as_deref(), Some(jobs::SUPERSEDED));
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 1);

    // With no waiting job, a release queues the job again.
    let leased = jobs::take(&mut conn, instance, &kinds, 60.0)
        .expect("take")
        .expect("a job");
    let row = jobs::release(&mut conn, leased.id, instance, "The forge said 502.", 0.0)
        .expect("release")
        .expect("the lease");
    assert_eq!(row.state, "queued");
    assert_eq!(row.last_error.as_deref(), Some("The forge said 502."));

    // --- 4. the migration that ends the twins --------------------------------
    conn.batch_execute(PR_INDEX_DOWN).expect("drop the index");
    let twin = jobs::enqueue(
        &mut conn,
        &jobs::NewJob {
            kind: writeback::JOB_KIND.to_string(),
            tenant: Some("acme".to_string()),
            payload: serde_json::json!({
                "repository_id": repo,
                "pr_number": 7,
                "pr_key": format!("github-writeback:acme:{repo}:7"),
            }),
            ..jobs::NewJob::new(writeback::JOB_KIND)
        },
    )
    .expect("a twin")
    .expect("added");
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 2);
    conn.batch_execute(DEDUPE_SQL)
        .expect("the dedupe migration");
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 1);
    assert_eq!(
        jobs::get(&mut conn, twin.id).unwrap().unwrap().state,
        "done",
        "the newer twin ends, the oldest stays"
    );
    conn.batch_execute(DEDUPE_SQL)
        .expect("the dedupe migration again");
    conn.batch_execute(PR_INDEX_UP).expect("the index again");
    assert_eq!(queued(&mut conn, writeback::JOB_KIND), 1);

    // --- 2. an issue: a change and a new token at the same time -------------
    let done = column(&mut conn, board, "Completed");
    let c = task(&mut conn, board, "Task C", robin);
    sql_query(
        "INSERT INTO forge_issue_intake (repository_id, issue_id, number, item_id, url, author, \
             source) \
         VALUES ($1, '901', 12, $2, 'https://github.com/acme/site/issues/12', 'sam', 'webhook')",
    )
    .bind::<SqlUuid, _>(repo)
    .bind::<SqlUuid, _>(c)
    .execute(&mut conn)
    .expect("intake row");
    // The task is done, and no job waits: as when its job runs.
    sql_query("UPDATE tasks SET column_id = $2 WHERE id = $1")
        .bind::<SqlUuid, _>(c)
        .bind::<SqlUuid, _>(done)
        .execute(&mut conn)
        .expect("task done");
    assert_eq!(queued(&mut conn, issue_writeback::JOB_KIND), 0);
    let (first, second, waited) = race(
        &url,
        &mut conn,
        move |c2| issue_writeback::on_item_changed(c2, c).expect("change"),
        move |c2| issue_writeback::queue_changed_issues(c2, repo).expect("new token"),
    );
    assert_eq!((first, second), (1, 0), "the second adds nothing");
    assert!(
        waited.contains("INSERT INTO public.jobs"),
        "the second queue waits for the job of the first: {waited:?}"
    );
    assert_eq!(queued(&mut conn, issue_writeback::JOB_KIND), 1);

    // --- 5. the success write of a webhook target ---------------------------
    let target = webhooks::insert_target(
        &mut conn,
        &NewTarget {
            id: Uuid::new_v4(),
            board_id: board,
            name: "runner".to_string(),
            url: "https://203.0.113.10/hook".to_string(),
            secret: SealedSecret {
                ciphertext: vec![1],
                nonce: vec![0; 12],
                key_id: "k1".to_string(),
            },
            created_by: robin,
        },
    )
    .expect("target");
    let delivery = webhooks::queue_delivery(
        &mut conn,
        target.id,
        "item.transitioned",
        None,
        None,
        &serde_json::json!({}),
    )
    .expect("queue")
    .expect("a delivery");
    let version = |conn: &mut PgConnection| -> String {
        sql_query("SELECT xmin::text AS name FROM webhook_targets WHERE id = $1")
            .bind::<SqlUuid, _>(target.id)
            .get_result::<DbName>(conn)
            .expect("xmin")
            .name
    };
    let try_of = |n: i32, ok: bool| TryResult {
        try_number: n,
        ok,
        response_code: Some(if ok { 204 } else { 500 }),
        error: (!ok).then(|| "The target said 500.".to_string()),
        duration_ms: 5,
        last: false,
        next_try_at: None,
    };
    let before = version(&mut conn);
    webhooks::record_try(&mut conn, delivery.id, &try_of(1, true), 5).expect("try");
    assert_eq!(
        version(&mut conn),
        before,
        "a success with no failures does not write the row of the target"
    );
    webhooks::record_try(&mut conn, delivery.id, &try_of(2, false), 5).expect("try");
    assert_eq!(
        webhooks::get_target(&mut conn, target.id)
            .unwrap()
            .unwrap()
            .consecutive_failures,
        1
    );
    webhooks::record_try(&mut conn, delivery.id, &try_of(3, true), 5).expect("try");
    assert_eq!(
        webhooks::get_target(&mut conn, target.id)
            .unwrap()
            .unwrap()
            .consecutive_failures,
        0,
        "a success after a failure sets the count to 0"
    );

    // --- 6. the vectors of an upload -----------------------------------------
    let pool_row = |key: &str, vector: Option<Vec<u8>>| PoolRow {
        key: key.into(),
        level: "symbol".into(),
        summary: format!("The summary of {key}."),
        vector,
        model: "fake/fixed".into(),
    };
    let index = |sha: char| NewCodeIndex {
        repository_id: repo,
        commit_sha: sha.to_string().repeat(40),
        ref_name: Some("main".into()),
        source: "upload",
        structure: vec![1, 2, 3],
        structure_bytes: 3,
        summary_keys: 3,
        vector_model: Some("det/m/1".into()),
        created_by: Some(robin),
        citations: Vec::new(),
    };
    code_indexes::put(
        &mut conn,
        &index('a'),
        &[
            pool_row("x", None),
            pool_row("y", None),
            pool_row("z", Some(vec![9, 9, 9, 9])),
        ],
        &[],
    )
    .expect("first put");
    code_indexes::put(
        &mut conn,
        &index('b'),
        &[
            pool_row("x", Some(vec![1, 0, 0, 0])),
            pool_row("y", Some(vec![2, 0, 0, 0])),
            pool_row("z", Some(vec![3, 0, 0, 0])),
        ],
        &[],
    )
    .expect("second put");
    let mut vectors: Vec<(String, Option<Vec<u8>>)> = code_indexes::whole_pool(&mut conn, repo)
        .expect("pool")
        .into_iter()
        .map(|r| (r.key, r.vector))
        .collect();
    vectors.sort();
    assert_eq!(
        vectors,
        vec![
            ("x".to_string(), Some(vec![1, 0, 0, 0])),
            ("y".to_string(), Some(vec![2, 0, 0, 0])),
            ("z".to_string(), Some(vec![9, 9, 9, 9])),
        ],
        "a row with no vector gets the vector of the upload; a row with one keeps it"
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch db");
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    name: Uuid,
}
