//! COLLIERY-T-3104 — the Metis import keeps the Metis numbers on the boards
//! of a team (`--codes keep`), with the board prefix.
//!
//! - `PUT /api/boards/{id}/code-sequences/{item_type}` sets the number of
//!   the next code of a type on a board. It never goes back, and it refuses
//!   (and names) a code in use, a retired code, a type that the board does
//!   not hold, and bad input.
//! - `scripts/migrate-metis-to-kairos.py --codes keep` imports a small
//!   Metis record onto an empty board. The codes have the board prefix and
//!   the Metis numbers. Two documents that want one code: the second gets
//!   the next free number, and its footer says why. `verify` finds no
//!   difference, and the next task on the board continues after the kept
//!   numbers.
//! - A retired code stops the import, and the message names the code and
//!   the reason.
//!
//! The importer runs as a subprocess (python3) against the booted router
//! on an ephemeral 127.0.0.1 port. Runs against the LIVE compose stack
//! (`angreal services up`). Each test owns a scratch database.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    spawn_server, user_token, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The booted router on a scratch database, with `svc` as the org admin,
/// the team `skadi` (delivery board prefix SKADI), its ADR board
/// `skadi-adrs` and the repository `skadi`.
struct Stack {
    scratch_db: &'static str,
    admin_conn: PgConnection,
    conn: PgConnection,
    pool: TenantPool,
    router: Router,
    token: String,
    delivery_slug: String,
    delivery_id: String,
}

impl Stack {
    async fn boot(scratch_db: &'static str) -> Self {
        let admin_conn = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme org row");

        let http = reqwest::Client::new();
        let token = user_token(&http, "svc").await;
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        let router = app::router(app::state_with(
            base_config(&scratch_url),
            pool.clone(),
            auth,
        ));
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&token),
            &TENANT,
            None,
        )
        .await;
        let svc_id: Uuid = users::table
            .filter(users::email.eq("svc@kairos.test"))
            .select(users::id)
            .first(&mut conn)
            .expect("svc provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: svc_id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("granting membership");
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");

        let mut stack = Stack {
            scratch_db,
            admin_conn,
            conn,
            pool,
            router,
            token,
            delivery_slug: String::new(),
            delivery_id: String::new(),
        };
        let team = stack
            .ok(
                Method::POST,
                "/api/teams",
                json!({"name": "Skadi", "slug": "skadi", "code_prefix": "SKADI"}),
            )
            .await;
        let board_id = team["delivery_board_id"]
            .as_str()
            .expect("board")
            .to_string();
        let (_, board) = stack
            .send(Method::GET, &format!("/api/boards/{board_id}"), None)
            .await;
        stack.delivery_slug = board["slug"].as_str().expect("slug").to_string();
        stack.delivery_id = board_id;
        stack
            .ok(
                Method::POST,
                "/api/boards",
                json!({
                    "name": "Skadi ADRs", "slug": "skadi-adrs", "board_level": "adr",
                    "team_id": team["id"], "code_prefix": "SKADI",
                }),
            )
            .await;
        stack
            .ok(
                Method::POST,
                "/api/repositories",
                json!({
                    "slug": "skadi", "forge": "github",
                    "repo_full_name": "colliery-io/skadi",
                    "repo_url": "https://github.com/colliery-io/skadi",
                    "team": "skadi",
                }),
            )
            .await;
        stack
    }

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    async fn ok(&self, method: Method, uri: &str, body: Value) -> Value {
        let (status, answer) = self.send(method, uri, Some(body)).await;
        assert!(status.is_success(), "{uri}: {status} {answer}");
        answer
    }

    async fn get(&self, uri: &str) -> Value {
        let (status, answer) = self.send(Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
        answer
    }

    fn sequence_uri(&self, item_type: &str) -> String {
        format!(
            "/api/boards/{}/code-sequences/{item_type}",
            self.delivery_slug
        )
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

/// One Metis document file.
fn metis_file(dir: &Path, relative: &str, front: &str, body: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(
        path,
        format!(
            "---\n{front}\ncreated_at: 2026-01-02T10:00:00+00:00\narchived: false\n---\n\n{body}\n"
        ),
    )
    .expect("writing a Metis file");
}

fn tags(level: &str, phase: &str) -> String {
    format!("tags:\n  - \"#{level}\"\n  - \"#phase/{phase}\"")
}

/// A small Metis record of the repository skadi:
///
/// - the vision SKADI-V-0001 and the specification SKADI-S-0001, which both
///   want SKADI-D-0001 (the vision comes first);
/// - the specification SKADI-S-0003;
/// - the initiative SKADI-I-0002 (it goes on the shared board, ACME);
/// - the tasks SKADI-T-0002 and SKADI-T-0005 (numbers with gaps), and a
///   second file with the code SKADI-T-0005;
/// - the ADR SKADI-A-0001.
fn write_record(dir: &Path) {
    metis_file(
        dir,
        "vision.md",
        &format!(
            "level: vision\nshort_code: \"SKADI-V-0001\"\ntitle: \"Skadi vision\"\n{}",
            tags("vision", "published")
        ),
        "# Skadi vision\n\nFind the downloads.",
    );
    metis_file(
        dir,
        "specifications/SKADI-S-0001/specification.md",
        &format!(
            "level: specification\nshort_code: \"SKADI-S-0001\"\ntitle: \"Index format\"\n{}",
            tags("specification", "drafting")
        ),
        "# Index format\n\nThe index format.",
    );
    metis_file(
        dir,
        "specifications/SKADI-S-0003/specification.md",
        &format!(
            "level: specification\nshort_code: \"SKADI-S-0003\"\ntitle: \"Search API\"\n{}",
            tags("specification", "review")
        ),
        "# Search API\n\nThe search API.",
    );
    metis_file(
        dir,
        "strategies/initiatives/SKADI-I-0002/initiative.md",
        &format!(
            "level: initiative\nshort_code: \"SKADI-I-0002\"\ntitle: \"Downloads\"\n{}",
            tags("initiative", "active")
        ),
        "# Downloads\n\nThe downloads.",
    );
    metis_file(
        dir,
        "strategies/initiatives/SKADI-I-0002/tasks/SKADI-T-0002.md",
        &format!(
            "level: task\nshort_code: \"SKADI-T-0002\"\ntitle: \"Fetch the list\"\nparent: SKADI-I-0002\n{}",
            tags("task", "todo")
        ),
        "# Fetch the list\n\nAfter SKADI-T-0005. Read SKADI-S-0001 and SKADI-S-0003.",
    );
    metis_file(
        dir,
        "backlog/a/SKADI-T-0005.md",
        &format!(
            "level: task\nshort_code: \"SKADI-T-0005\"\ntitle: \"Parse the list\"\n{}",
            tags("task", "completed")
        ),
        "# Parse the list\n\nParse it.",
    );
    metis_file(
        dir,
        "backlog/b/SKADI-T-0005.md",
        &format!(
            "level: task\nshort_code: \"SKADI-T-0005\"\ntitle: \"Store the list\"\n{}",
            tags("task", "backlog")
        ),
        "# Store the list\n\nStore it.",
    );
    metis_file(
        dir,
        "adrs/SKADI-A-0001.md",
        &format!(
            "level: adr\nshort_code: \"SKADI-A-0001\"\ntitle: \"Use SQLite\"\n{}",
            tags("adr", "decided")
        ),
        "# Use SQLite\n\nWe use SQLite.",
    );
}

fn importer() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/migrate-metis-to-kairos.py")
}

/// Run the importer. Returns (exit code, stdout, stderr).
async fn run_importer(
    base_url: &str,
    token: &str,
    metis: &Path,
    state: &Path,
    delivery_slug: &str,
    mode: &str,
) -> (i32, String, String) {
    let mut command = Command::new("python3");
    command
        .arg("-B")
        .arg(importer())
        .args(["--codes", "keep", "--url", base_url, "--tenant", "acme"])
        .arg("--metis")
        .arg(metis)
        .arg("--state")
        .arg(state)
        .args([
            "--repository",
            "skadi",
            "--prefix",
            "SKADI",
            "--delivery-board",
            delivery_slug,
            "--adr-board",
            "skadi-adrs",
            mode,
        ])
        .env("KAIROS_KEY", token)
        .env_remove("KAIROS_ADMIN_TOKEN");
    let output = tokio::task::spawn_blocking(move || command.output())
        .await
        .expect("join")
        .expect("running python3");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sequence_route_moves_forward_and_names_each_refusal_against_live_stack() {
    let mut stack = Stack::boot("kairos_code_sequence_rest_t3104").await;
    let task = stack
        .ok(
            Method::POST,
            "/api/tasks",
            json!({"board_id": stack.delivery_id, "title": "First"}),
        )
        .await;
    assert_eq!(task["short_code"], "SKADI-T-0001", "{task}");

    // The next task gets 577.
    let uri = stack.sequence_uri("task");
    let answer = stack
        .ok(Method::PUT, &uri, json!({"next_number": 577}))
        .await;
    assert_eq!(
        answer,
        json!({"code_prefix": "SKADI", "item_type": "task", "last_number": 576,
               "next_code": "SKADI-T-0577"})
    );
    // The same request again changes nothing.
    stack
        .ok(Method::PUT, &uri, json!({"next_number": 577}))
        .await;
    let task = stack
        .ok(
            Method::POST,
            "/api/tasks",
            json!({"board_id": stack.delivery_id, "title": "Kept"}),
        )
        .await;
    assert_eq!(task["short_code"], "SKADI-T-0577", "{task}");

    // A code in use.
    let (status, answer) = stack
        .send(Method::PUT, &uri, Some(json!({"next_number": 1})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_code(&answer), "CODE_IN_USE", "{answer}");
    assert_eq!(answer["error"]["details"]["code"], "SKADI-T-0001");

    // A number that the sequence is past.
    let (status, answer) = stack
        .send(Method::PUT, &uri, Some(json!({"next_number": 300})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_code(&answer), "SEQUENCE_IS_PAST", "{answer}");
    assert_eq!(answer["error"]["details"]["next_code"], "SKADI-T-0578");

    // A retired code (COLLIERY-T-3100): the code of a task that moved.
    let task_id: Uuid = Uuid::parse_str(task["id"].as_str().expect("id")).expect("uuid");
    diesel::sql_query(
        "INSERT INTO retired_codes (code, item_id, reason) VALUES ('SKADI-T-0600', $1, 'test')",
    )
    .bind::<diesel::sql_types::Uuid, _>(task_id)
    .execute(&mut stack.conn)
    .expect("retiring a code");
    let (status, answer) = stack
        .send(Method::PUT, &uri, Some(json!({"next_number": 600})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_code(&answer), "CODE_RETIRED", "{answer}");
    assert_eq!(answer["error"]["details"]["code"], "SKADI-T-0600");
    assert_eq!(answer["error"]["details"]["current_code"], "SKADI-T-0577");

    // A type that the board does not hold.
    let (status, answer) = stack
        .send(
            Method::PUT,
            &stack.sequence_uri("initiative"),
            Some(json!({"next_number": 5})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["details"]["parameter"], "item_type");
    assert_eq!(
        answer["error"]["details"]["allowed"],
        json!(["task", "document"])
    );

    // Bad input is refused and named.
    let (status, answer) = stack
        .send(Method::PUT, &uri, Some(json!({"next_number": 0})))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["details"]["field"], "next_number");
    let (status, answer) = stack
        .send(
            Method::PUT,
            &uri,
            Some(json!({"next_number": 700, "number": 700})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("number"),
        "{answer}"
    );

    // Only an org admin.
    let http = reqwest::Client::new();
    let alice = user_token(&http, "alice").await;
    let (status, answer) = request(
        &stack.router,
        Method::PUT,
        &uri,
        Some(&alice),
        &TENANT,
        Some(json!({"next_number": 700})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");

    stack.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_import_keeps_the_metis_numbers_with_the_board_prefix_against_live_stack() {
    let stack = Stack::boot("kairos_metis_keep_t3104").await;
    let work = tempfile::tempdir().expect("tempdir");
    let metis = work.path().join(".metis");
    write_record(&metis);
    let state = work.path().join("import-state.json");
    let server = spawn_server(stack.router.clone()).await;

    let (code, out, err) = run_importer(
        &server.base_url,
        &stack.token,
        &metis,
        &state,
        &stack.delivery_slug,
        "plan",
    )
    .await;
    assert_eq!(code, 0, "plan:\n{out}\n{err}");
    assert!(
        out.contains("Keep mode: 5 items keep their Metis number with the prefix SKADI."),
        "{out}"
    );

    let (code, out, err) = run_importer(
        &server.base_url,
        &stack.token,
        &metis,
        &state,
        &stack.delivery_slug,
        "apply",
    )
    .await;
    assert_eq!(code, 0, "apply:\n{out}\n{err}");

    // The codes use the board prefix and the Metis numbers.
    let ledger: Value =
        serde_json::from_str(&std::fs::read_to_string(&state).expect("state")).expect("json");
    let codes = &ledger["codes"];
    assert_eq!(codes["SKADI-T-0002"], "SKADI-T-0002", "{ledger}");
    assert_eq!(codes["SKADI-T-0005"], "SKADI-T-0005", "{ledger}");
    assert_eq!(codes["SKADI-A-0001"], "SKADI-A-0001", "{ledger}");
    assert_eq!(codes["SKADI-V-0001"], "SKADI-D-0001", "{ledger}");
    assert_eq!(codes["SKADI-S-0003"], "SKADI-D-0003", "{ledger}");
    // The second document that wants a code gets the next free number.
    assert_eq!(codes["SKADI-T-0005~2"], "SKADI-T-0006", "{ledger}");
    assert_eq!(codes["SKADI-S-0001"], "SKADI-D-0004", "{ledger}");
    // The initiative goes on the shared board, with its prefix.
    assert_eq!(codes["SKADI-I-0002"], "ACME-I-0001", "{ledger}");
    assert_eq!(ledger["mode"], "keep", "{ledger}");

    let task = stack.get("/api/tasks/SKADI-T-0002").await;
    assert_eq!(task["title"], "Fetch the list");
    assert!(
        task["content"]
            .as_str()
            .unwrap_or_default()
            .contains("After SKADI-T-0005. Read SKADI-D-0004 and SKADI-D-0003."),
        "the references change to the new codes: {task}"
    );
    let second = stack.get("/api/tasks/SKADI-T-0006").await;
    assert_eq!(second["title"], "Store the list");
    assert!(
        second["content"].as_str().unwrap_or_default().contains(
            "It did not get the code SKADI-T-0005, because the item of the Metis file \
             backlog/a/SKADI-T-0005.md has that code. It got the next free number."
        ),
        "{second}"
    );
    let adr = stack.get("/api/adrs/SKADI-A-0001").await;
    assert_eq!(adr["title"], "Use SQLite");
    let spec = stack.get("/api/documents/SKADI-D-0004").await;
    assert_eq!(spec["title"], "Index format");

    let (code, out, err) = run_importer(
        &server.base_url,
        &stack.token,
        &metis,
        &state,
        &stack.delivery_slug,
        "verify",
    )
    .await;
    assert_eq!(code, 0, "verify:\n{out}\n{err}");
    assert!(out.contains("Verified 8 items: 0 differences."), "{out}");

    // The next task on the board continues after the kept numbers.
    let task = stack
        .ok(
            Method::POST,
            "/api/tasks",
            json!({"board_id": stack.delivery_id, "title": "After the import"}),
        )
        .await;
    assert_eq!(task["short_code"], "SKADI-T-0007", "{task}");

    stack.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retired_code_stops_the_import_and_the_message_names_it_against_live_stack() {
    let mut stack = Stack::boot("kairos_metis_keep_retired_t3104").await;
    let task = stack
        .ok(
            Method::POST,
            "/api/tasks",
            json!({"board_id": stack.delivery_id, "title": "Made before the import"}),
        )
        .await;
    assert_eq!(task["short_code"], "SKADI-T-0001", "{task}");
    let task_id = Uuid::parse_str(task["id"].as_str().expect("id")).expect("uuid");
    diesel::sql_query(
        "INSERT INTO retired_codes (code, item_id, reason) VALUES ('SKADI-T-0002', $1, 'test')",
    )
    .bind::<diesel::sql_types::Uuid, _>(task_id)
    .execute(&mut stack.conn)
    .expect("retiring a code");

    let work = tempfile::tempdir().expect("tempdir");
    let metis = work.path().join(".metis");
    write_record(&metis);
    let state = work.path().join("import-state.json");
    let server = spawn_server(stack.router.clone()).await;
    let (code, out, err) = run_importer(
        &server.base_url,
        &stack.token,
        &metis,
        &state,
        &stack.delivery_slug,
        "apply",
    )
    .await;
    assert_eq!(code, 2, "the import stops:\n{out}\n{err}");
    assert!(
        err.contains(
            "STOP: SKADI-T-0002 cannot keep its Metis number: the server refused the code \
             SKADI-T-0002"
        ),
        "{err}"
    );
    assert!(err.contains("CODE_RETIRED"), "{err}");
    assert!(
        err.contains("The code SKADI-T-0002 is retired (the item now has the code SKADI-T-0001)"),
        "{err}"
    );
    // The items before it exist, and no item has the retired code.
    let ledger: Value =
        serde_json::from_str(&std::fs::read_to_string(&state).expect("state")).expect("json");
    assert_eq!(ledger["codes"]["SKADI-A-0001"], "SKADI-A-0001", "{ledger}");
    assert!(ledger["codes"].get("SKADI-T-0002").is_none(), "{ledger}");

    stack.shutdown();
}
