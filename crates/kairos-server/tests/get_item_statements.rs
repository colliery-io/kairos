//! KAIROS-T-0626 — the statements of one MCP `get_item`. The text is
//! rendered by `kairos_server::mcp::get_item_text`, the body of the tool,
//! on a connection that counts each statement (diesel instrumentation).
//! The fixture has a parent chain of two, a blocker, a cancelled blocker,
//! a supporting document, metadata, a claim, a state, an external
//! reference, a retired code, an archived task and an initiative with an
//! owner. KAIROS-T-0735: an organization dictionary and a team dictionary,
//! so that the count covers the block `## Terms used`, and the block of the
//! task shows a term of two words and a team entry over an organization
//! entry.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_get_item_statements_t0626_test`.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::http::Method;
use diesel::connection::{Instrumentation, InstrumentationEvent};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_get_item_statements_t0626_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The most statements that one `get_item` may run, per kind of item.
/// Before KAIROS-T-0626 the task of this fixture took 20, the initiative
/// 17, the document 10. A parent chain is one statement at each depth.
const MAX_TASK: usize = 5;
const MAX_OTHER: usize = 6;

/// Counts each statement that the connection starts.
struct Counter {
    count: Arc<AtomicUsize>,
    queries: Arc<Mutex<Vec<String>>>,
}

impl Instrumentation for Counter {
    fn on_connection_event(&mut self, event: InstrumentationEvent<'_>) {
        if let InstrumentationEvent::StartQuery { query, .. } = event {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.queries
                .lock()
                .expect("queries")
                .push(query.to_string());
        }
    }
}

/// The id of the column `name` of the board `board_slug`.
fn column_id(conn: &mut PgConnection, board_slug: &str, name: &str) -> String {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    let row: Row = sql_query(
        "SELECT c.id FROM org_acme.board_columns c \
         JOIN org_acme.boards b ON b.id = c.board_id \
         WHERE b.slug = $1 AND c.name = $2",
    )
    .bind::<Text, _>(board_slug)
    .bind::<Text, _>(name)
    .get_result(conn)
    .expect("column");
    row.id.to_string()
}

#[tokio::test]
async fn get_item_runs_few_statements_against_live_stack() {
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
    let code = |answer: &Value| answer["short_code"].as_str().expect("code").to_string();
    let link = |source: String, target: String, relationship: &'static str| {
        send(
            Method::POST,
            "/api/relationships".into(),
            Some(json!({
                "source_short_code": source,
                "target_short_code": target,
                "relationship": relationship,
            })),
        )
    };

    // --- the fixture -----------------------------------------------------------
    send(
        Method::POST,
        "/api/teams".into(),
        Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
    )
    .await;
    let strategy = code(
        &send(
            Method::POST,
            "/api/strategies".into(),
            Some(json!({"board_id": "strategy", "title": "Direction"})),
        )
        .await,
    );
    let initiative = code(
        &send(
            Method::POST,
            "/api/initiatives".into(),
            Some(json!({"board_id": "initiatives", "title": "Shared work"})),
        )
        .await,
    );
    let task = |title: &'static str, content: &'static str| {
        send(
            Method::POST,
            "/api/tasks".into(),
            Some(json!({"board_id": "payments-delivery", "title": title, "content": content})),
        )
    };
    let t1 = code(&task("Pay", "# Pay\n\nThe body.\n").await);
    // KAIROS-T-0630: a note is a row; the facts statement reads it.
    send(
        Method::POST,
        format!("/api/tasks/{t1}/notes"),
        Some(json!({"text": "The first note."})),
    )
    .await;
    let t2 = code(&task("Blocker", "It blocks.").await);
    let t3 = code(&task("Dropped", "It is cancelled.").await);
    let t4 = code(&task("Put away", "It is archived.").await);
    let document = code(
        &send(
            Method::POST,
            "/api/documents".into(),
            Some(json!({"title": "Design", "board": "payments-delivery", "content": "Why."})),
        )
        .await,
    );
    // KAIROS-T-0735: the dictionaries. The organization dictionary is on
    // the board `strategy`, the team dictionary on the delivery board.
    #[derive(QueryableByName)]
    struct TemplateRow {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    let dictionary_template =
        sql_query("SELECT id FROM org_acme.templates WHERE slug = 'dictionary'")
            .get_result::<TemplateRow>(&mut conn)
            .expect("the dictionary template")
            .id;
    let table = "| Word | Part of speech | Meaning |\n|---|---|---|\n";
    let organization_dictionary = code(
        &send(
            Method::POST,
            "/api/documents".into(),
            Some(json!({
                "title": "Words of Acme",
                "board": "strategy",
                "template_id": dictionary_template,
                "content": format!(
                    "{table}| body | noun | The organization meaning. |\n\
                     | first note | noun | The note that starts the record. |\n\
                     | note | noun | A text on an item. |\n\
                     | work | noun | The organization meaning of work. |\n"
                ),
            })),
        )
        .await,
    );
    send(
        Method::POST,
        "/api/documents".into(),
        Some(json!({
            "title": "Words of Payments",
            "board": "payments-delivery",
            "template_id": dictionary_template,
            "content": format!(
                "{table}| Body | noun | The text of a task. |\n\
                 | work | noun | The work of the payments team. |\n"
            ),
        })),
    )
    .await;
    link(strategy.clone(), initiative.clone(), "parent").await;
    link(initiative.clone(), t1.clone(), "parent").await;
    link(t2.clone(), t1.clone(), "blocks").await;
    link(t3.clone(), t1.clone(), "blocks").await;
    link(t1.clone(), document.clone(), "supports").await;
    send(
        Method::PATCH,
        format!("/api/tasks/{t1}/metadata"),
        Some(json!({"values": {"priority": "high"}})),
    )
    .await;
    send(
        Method::PUT,
        format!("/api/tasks/{t1}/state"),
        Some(
            json!({"branch": "pay-1", "commit": "abc1234", "pushed": true,
                    "pr_url": "https://example.com/pr/12", "next_step": "Wait for CI"}),
        ),
    )
    .await;
    send(
        Method::POST,
        format!("/api/tasks/{t1}/external"),
        Some(json!({"uri": "https://example.com/wiki/rfc-42", "label": "RFC 42"})),
    )
    .await;
    // The parent initiative stays in Discovery: turn the parent gate off
    // for this board (KAIROS-T-0641), so the task can go to Active.
    let board = send(Method::GET, "/api/boards/payments-delivery".into(), None).await;
    let board_id = board["id"].as_str().expect("board id").to_string();
    send(
        Method::PATCH,
        format!("/api/boards/{board_id}"),
        Some(json!({"parent_gate": false})),
    )
    .await;
    for column in ["Todo", "Active"] {
        let to = column_id(&mut conn, "payments-delivery", column);
        send(
            Method::POST,
            format!("/api/tasks/{t1}/transition"),
            Some(json!({"to_column_id": to})),
        )
        .await;
    }
    send(
        Method::POST,
        format!("/api/tasks/{t3}/cancel"),
        Some(json!({"reason": "Not needed"})),
    )
    .await;
    send(Method::DELETE, format!("/api/tasks/{t4}"), None).await;
    send(
        Method::POST,
        format!("/api/initiatives/{initiative}/owner/claim"),
        None,
    )
    .await;

    // A retired code: the code of t2 changes, and the old code is retired.
    let mut tenant_conn =
        PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut tenant_conn)
        .expect("pinning search_path");
    let t2_new = "PAYMENTS-T-0900";
    sql_query("UPDATE tasks SET short_code = $2 WHERE short_code = $1")
        .bind::<Text, _>(&t2)
        .bind::<Text, _>(t2_new)
        .execute(&mut tenant_conn)
        .expect("changing the code");
    let t2_id: Uuid = kairos_db::schema::tasks::table
        .filter(kairos_db::schema::tasks::short_code.eq(t2_new))
        .select(kairos_db::schema::tasks::id)
        .first(&mut tenant_conn)
        .expect("t2 id");
    kairos_db::retired_codes::retire_code(&mut tenant_conn, &t2, t2_id, "renamed")
        .expect("retiring the code");

    // --- the count -------------------------------------------------------------
    let count = Arc::new(AtomicUsize::new(0));
    let queries = Arc::new(Mutex::new(Vec::new()));
    tenant_conn.set_instrumentation(Counter {
        count: count.clone(),
        queries: queries.clone(),
    });
    let cases: Vec<(&str, String, usize)> = vec![
        ("task", t1.clone(), MAX_TASK),
        ("task, lower case", t1.to_ascii_lowercase(), MAX_TASK),
        ("task, retired code", t2.clone(), MAX_TASK),
        ("cancelled task", t3.clone(), MAX_TASK),
        ("archived task", t4.clone(), MAX_TASK),
        ("initiative", initiative.clone(), MAX_OTHER),
        ("strategy", strategy.clone(), MAX_OTHER),
        ("document", document.clone(), MAX_OTHER),
        ("dictionary", organization_dictionary.clone(), MAX_OTHER),
    ];
    let mut report = String::new();
    let mut dump = String::new();
    for (what, short_code, max) in &cases {
        count.store(0, Ordering::SeqCst);
        queries.lock().expect("queries").clear();
        let text = kairos_server::mcp::get_item_text(&mut tenant_conn, short_code)
            .unwrap_or_else(|e| panic!("get_item {short_code}: {e:?}"));
        let n = count.load(Ordering::SeqCst);
        report.push_str(&format!("{what} ({short_code}): {n} statements\n"));
        dump.push_str(&format!("=== {what}\n{text}\n"));
        if std::env::var_os("GET_ITEM_STATEMENTS_VERBOSE").is_some() {
            for query in queries.lock().expect("queries").iter() {
                report.push_str(&format!("    {}\n", query.replace('\n', " ")));
            }
        }
        assert!(
            n <= *max,
            "get_item of a {what} ran {n} statements, more than {max}:\n{}",
            queries.lock().expect("queries").join("\n")
        );
    }
    eprintln!("{report}");
    if let Some(path) = std::env::var_os("GET_ITEM_STATEMENTS_DUMP") {
        std::fs::write(path, &dump).expect("writing the dump");
    }

    // The fixture is what it says: the lines that the count covers are there.
    let task_text = dump.split("=== ").nth(1).expect("the task text");
    for line in [
        "- parent chain: ",
        "- blocked by: ",
        "[cancelled]",
        "- supporting docs: ",
        "- claim: ",
        "- state: ",
        "- external: RFC 42",
        "- priority: high",
        "## Notes\n- n-1",
        "The body.\n\n## Note n-1 (",
    ] {
        assert!(task_text.contains(line), "no {line:?} in:\n{task_text}");
    }
    // A code in lower case is the current code, not a retired one; a
    // retired code says so.
    let lower_text = dump.split("=== ").nth(2).expect("the lower-case text");
    assert!(!lower_text.contains("RETIRED CODE"), "{lower_text}");
    assert!(lower_text.starts_with("task, lower case\n# PAYMENTS-T-0001 — Pay\n"));
    let retired_text = dump.split("=== ").nth(3).expect("the retired-code text");
    assert!(
        retired_text.contains(&format!(
            "> **RETIRED CODE** The code {t2} is retired. The current code of this item is \
             {t2_new}. Use the current code."
        )),
        "{retired_text}"
    );
    // KAIROS-T-0735: the terms of the task, at the end. "first note" (two
    // words, in the note) comes before "note", which it hides; "body" is
    // the entry of the team, not of the organization.
    assert!(
        task_text.ends_with(
            "\n## Terms used\n\
             - first note (noun): The note that starts the record.\n\
             - Body (noun): The text of a task.\n\n"
        ),
        "{task_text}"
    );
    let initiative_text = dump.split("=== ").nth(6).expect("the initiative text");
    // An initiative takes the dictionaries of its teams.
    assert!(
        initiative_text
            .contains("\n## Terms used\n- work (noun): The work of the payments team.\n"),
        "{initiative_text}"
    );
    // A dictionary lists every term: it has no block of its own.
    let dictionary_text = dump.split("=== ").nth(9).expect("the dictionary text");
    assert!(
        !dictionary_text.contains("## Terms used"),
        "{dictionary_text}"
    );
    for line in ["- owner: svc since ", "- teams: payments", "- children: "] {
        assert!(
            initiative_text.contains(line),
            "no {line:?} in:\n{initiative_text}"
        );
    }
    drop(tenant_conn);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
