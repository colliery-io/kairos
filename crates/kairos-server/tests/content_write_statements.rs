//! KAIROS-T-0635 — the statements of one MCP `update_item`, `edit_item` and
//! `append_note`, and the copies of the content that each one moves. The
//! answer text comes from the body of each tool
//! (`kairos_server::mcp::update_item_text` and the others), on a connection
//! that counts each statement (diesel instrumentation).
//!
//! A copy is SENT when the text of the content is a bind of the statement,
//! and READ when the statement gives the content back (a SELECT of the
//! column, or a RETURNING of it).
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_content_write_statements_t0635_test`.

mod common;

use std::sync::{Arc, Mutex};

use axum::http::Method;
use diesel::connection::{Instrumentation, InstrumentationEvent};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{item_history, organization_members, organizations, tasks, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::mcp::{
    AppendNoteParams, EditItemParams, UpdateItemParams, append_note_text, edit_item_text,
    update_item_text,
};
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_content_write_statements_t0635_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// A word of each content of the fixture: a copy of the content has it.
const MARK: &str = "Zebrawood";

/// The most statements, copies sent and copies read of each case. Before
/// KAIROS-T-0635 (statements/sent/read): update_item 10/2/2, edit_item
/// 10/2/2, update_item with a mention 12/2/3, edit_item with a mention
/// 12/2/3, a stale update_item 7/1/3, append_note 14/0/1. The two copies
/// that stay sent are the UPDATE and the `item_history` row.
///
/// The event outbox (#242) writes each event with one INSERT, where the
/// event took three statements before (the code of the item, the schema
/// and `pg_notify`). So each count of statements is 2 lower for each
/// event: 1 event in each content write, 2 in `append_note`. The copies
/// do not change.
///
/// KAIROS-T-0651: a version is a diff from the newest full row of the
/// history, so the write needs the text of that row. The content UPDATE
/// gives it back (a scalar subquery in its RETURNING), so the write has no
/// more statements. That is one more content READ in each content write
/// (the stale case counts it too, but no row comes back). The history row
/// is now a small diff, not a second copy of the content, so a write
/// SENDS one copy fewer (2 stays the ceiling: a full row each 10
/// versions, and a diff that holds the marked line, still send it). The
/// copies on the wire stay the same; the history stores about a fifth.
/// A text with no base (a full row is due) does not come back.
struct Budget {
    statements: usize,
    sent: usize,
    read: usize,
}

/// Records the text of each statement that the connection starts.
struct Recorder {
    queries: Arc<Mutex<Vec<String>>>,
}

impl Instrumentation for Recorder {
    fn on_connection_event(&mut self, event: InstrumentationEvent<'_>) {
        if let InstrumentationEvent::StartQuery { query, .. } = event {
            self.queries
                .lock()
                .expect("queries")
                .push(query.to_string());
        }
    }
}

/// The SQL of a statement, without its binds, in lower case.
fn sql_of(query: &str) -> String {
    query
        .split(" -- binds:")
        .next()
        .unwrap_or(query)
        .to_ascii_lowercase()
}

/// Does the statement give a content column back?
fn reads_content(query: &str) -> bool {
    let sql = sql_of(query);
    let head = sql.trim_start();
    if head.starts_with("update") || head.starts_with("insert") {
        sql.split("returning")
            .nth(1)
            .is_some_and(|r| r.contains("content"))
    } else {
        head.starts_with("select") && sql.contains("content")
            || head.starts_with("with") && sql.contains("content")
    }
}

/// Does the statement send the text of a content?
fn sends_content(query: &str) -> bool {
    query
        .split(" -- binds:")
        .nth(1)
        .is_some_and(|binds| binds.contains(MARK))
}

/// A body of about 4 KB with [`MARK`] in it.
fn body(extra: &str) -> String {
    let mut text = format!("# Pay\n\n{extra}\n\n");
    for n in 0..80 {
        text.push_str(&format!("Line {n} of the {MARK} body, for the count.\n"));
    }
    text
}

#[tokio::test]
async fn content_writes_move_the_content_once_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
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
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let _ = request(
        &router,
        Method::GET,
        "/api/whoami",
        Some(&token),
        &TENANT,
        None,
    )
    .await;
    let svc: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting membership");

    let send = |method: Method, uri: String, body: Option<Value>| {
        let router = router.clone();
        let token = token.clone();
        async move {
            let (status, answer) =
                request(&router, method.clone(), &uri, Some(&token), &TENANT, body).await;
            assert!(status.is_success(), "{method} {uri}: {status} {answer}");
            answer
        }
    };
    send(
        Method::POST,
        "/api/teams".into(),
        Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
    )
    .await;
    let created = send(
        Method::POST,
        "/api/tasks".into(),
        Some(json!({"board_id": "payments-delivery", "title": "Pay", "content": body("")})),
    )
    .await;
    let code = created["short_code"].as_str().expect("code").to_string();

    let mut tenant_conn =
        PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut tenant_conn)
        .expect("pinning search_path");
    let queries = Arc::new(Mutex::new(Vec::new()));
    tenant_conn.set_instrumentation(Recorder {
        queries: queries.clone(),
    });

    let update = |content: String, version: i32| UpdateItemParams {
        short_code: code.clone(),
        title: None,
        content,
        version,
    };
    let edit = |search: &str, replace: &str| EditItemParams {
        short_code: code.clone(),
        search: search.into(),
        replace: replace.into(),
        replace_all: false,
        request_key: None,
    };
    type Run<'a> =
        Box<dyn Fn(&mut PgConnection) -> Result<String, kairos_server::error::ApiError> + 'a>;
    let cases: Vec<(&str, Run<'_>, &str, Budget)> = vec![
        (
            "update_item",
            Box::new(|c| update_item_text(c, OrgRole::Admin, svc, &update(body("One."), 1))),
            "Updated PAYMENTS-T-0001 to version 2.",
            Budget {
                statements: 9, // KAIROS-T-0736: +1, the watcher lookup.
                sent: 2,
                read: 1,
            },
        ),
        (
            "edit_item",
            Box::new(|c| {
                edit_item_text(c, OrgRole::Admin, svc, &edit("Line 7 of", "Line seven of"))
            }),
            "Edited PAYMENTS-T-0001 to version 3 (1 replacement).",
            Budget {
                statements: 9, // KAIROS-T-0736: +1, the watcher lookup.
                sent: 2,
                read: 2,
            },
        ),
        (
            "update_item with a mention",
            Box::new(|c| {
                update_item_text(c, OrgRole::Admin, svc, &update(body("Two, @robin."), 3))
            }),
            "Updated PAYMENTS-T-0001 to version 4.",
            Budget {
                statements: 11, // KAIROS-T-0736: +1, the watcher lookup.
                sent: 2,
                read: 2,
            },
        ),
        (
            "edit_item with a mention",
            Box::new(|c| edit_item_text(c, OrgRole::Admin, svc, &edit("Two,", "Two, @sam,"))),
            "Edited PAYMENTS-T-0001 to version 5 (1 replacement).",
            Budget {
                statements: 10, // KAIROS-T-0736: +1, the watcher lookup.
                sent: 2,
                read: 2,
            },
        ),
        (
            "update_item, stale version",
            Box::new(|c| update_item_text(c, OrgRole::Admin, svc, &update(body("Old."), 1))),
            "CONFLICT",
            Budget {
                statements: 7,
                sent: 1,
                read: 2,
            },
        ),
        (
            "append_note",
            Box::new(|c| {
                append_note_text(
                    c,
                    OrgRole::Admin,
                    svc,
                    &AppendNoteParams {
                        short_code: code.clone(),
                        text: "A note for @robin.".into(),
                        request_key: None,
                    },
                )
            }),
            "Appended note n-1 to PAYMENTS-T-0001. The content stays at version 5.",
            Budget {
                statements: 11, // KAIROS-T-0736: +1, the watcher lookup.
                sent: 0,
                read: 0,
            },
        ),
    ];

    let mut report = String::new();
    let mut over = Vec::new();
    for (what, run, answer, budget) in &cases {
        queries.lock().expect("queries").clear();
        let text = match run(&mut tenant_conn) {
            Ok(text) => text,
            Err(e) => format!("{} {} {}", e.code, e.message, e.details),
        };
        assert!(text.starts_with(answer), "{what}: {text}");
        if *answer == "CONFLICT" {
            // The conflict answer has the current version and content.
            assert!(
                text.contains("\"version\":5") && text.contains(MARK),
                "{what}: {text}"
            );
        }
        let recorded = queries.lock().expect("queries").clone();
        let statements = recorded.len();
        let sent = recorded.iter().filter(|q| sends_content(q)).count();
        let read = recorded.iter().filter(|q| reads_content(q)).count();
        report.push_str(&format!(
            "{what}: {statements} statements, content sent {sent}, content read {read}\n"
        ));
        if std::env::var_os("CONTENT_WRITE_STATEMENTS_VERBOSE").is_some() {
            for query in &recorded {
                let flags = match (sends_content(query), reads_content(query)) {
                    (true, true) => "SR",
                    (true, false) => "S ",
                    (false, true) => " R",
                    (false, false) => "  ",
                };
                let short: String = sql_of(query).chars().take(160).collect();
                report.push_str(&format!("  {flags} {short}\n"));
            }
        }
        if statements > budget.statements || sent > budget.sent || read > budget.read {
            over.push(format!(
                "{what}: {statements}/{sent}/{read}, budget {}/{}/{}",
                budget.statements, budget.sent, budget.read
            ));
        }
    }
    eprintln!("{report}");
    assert!(
        over.is_empty(),
        "over budget:\n{}\n{report}",
        over.join("\n")
    );

    // The writes are what they were: the content, the history and the
    // version.
    let (version, content): (i32, String) = tasks::table
        .filter(tasks::short_code.eq(&code))
        .select((tasks::version, tasks::content))
        .first(&mut tenant_conn)
        .expect("the task");
    assert_eq!(version, 5);
    assert_eq!(content, body("Two, @sam, @robin."));
    let task_id: Uuid = tasks::table
        .filter(tasks::short_code.eq(&code))
        .select(tasks::id)
        .first(&mut tenant_conn)
        .expect("the task id");
    // KAIROS-T-0651: v2..v5 are diff rows; read the full text of each.
    let rows = item_history::table
        .filter(item_history::item_id.eq(task_id))
        .order(item_history::version.asc())
        .select(kairos_db::models::ItemHistory::as_select())
        .load(&mut tenant_conn)
        .expect("the history");
    let history: Vec<(i32, String)> = kairos_db::history::with_full_content(&mut tenant_conn, rows)
        .expect("the text of each version")
        .into_iter()
        .map(|row| (row.version, row.content))
        .collect();
    assert_eq!(history.len(), 5);
    assert_eq!(history[4], (5, content));
    assert_eq!(history[1].1, body("One."));
    assert_eq!(
        history[2].1,
        body("One.").replacen("Line 7 of", "Line seven of", 1)
    );

    drop(tenant_conn);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
