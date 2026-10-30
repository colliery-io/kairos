//! KAIROS-T-0731: a "does not pass" mark on the documents that fail the
//! typed sections of their template (COLLIERY-I-0664 decision 3).
//!
//! A template "brief" with two typed sections: "Goal" (`plain`) and
//! "Notes" (`free`).
//!
//! 1. A REST write of failing content is saved and gets the mark: `GET
//!    /api/documents/{code}` gives `section_check`, and `get_item` gives
//!    `- sections: does not pass (N violations)` and the list.
//! 2. An old document (no check: from before this change) gets the mark
//!    after the periodic pass `section-checks` runs as a job.
//! 3. A fix clears the mark: a REST write with passing content.
//! 4. An MCP edit of the "Notes" section of a failing document is saved
//!    (only the changed sections are refused), and the mark stays.
//! 5. A dictionary on the team board is refused (KAIROS-A-0079). The
//!    dictionary of the organization (on `strategy`) makes each check
//!    stale, also of a document on a team board, and the pass checks them
//!    again: "widget" passes.
//! 6. A change of the typed sections of the template makes the checks
//!    stale; the pass checks again, and the mark goes.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_section_checks_t0731_test`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateDocumentRequest, UpdateContentRequest};
use kairos_client::types_meta::{CreateTemplateRequest, TemplateSection, UpdateTemplateRequest};
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::blocking::BlockingTenantPool;
use kairos_server::jobs::{JobSettings, Worker};
use kairos_server::middleware::auth::Authenticator;
use kairos_server::periodic;

const SCRATCH_DB: &str = "kairos_section_checks_t0731_test";

/// Content that passes.
const PASSING: &str = "## Goal\nWrite the item.\n\n## Notes\nAny text, utilize it.\n";

/// The "Goal" section breaks STE-V1 ("server") and the passive rule.
const FAILING: &str = "## Goal\nThe text is checked by the server.\n\n## Notes\nAny text.\n";

/// "widget" is not in the vocabulary until the organization dictionary has
/// it.
const WIDGET: &str = "## Goal\nWrite the widget.\n\n## Notes\nAny text.\n";

/// One JSON-RPC message of an MCP response: plain JSON or SSE `data:`.
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|value| value.get("jsonrpc").is_some())
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
}

/// A minimal MCP session over HTTP.
struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    session: Option<String>,
    next_id: i64,
}

impl Mcp {
    async fn post(&mut self, body: Value) -> (reqwest::StatusCode, String) {
        let mut request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("x-tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.expect("MCP request");
        let status = response.status();
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().expect("session id").to_string());
        }
        (status, response.text().await.expect("MCP body"))
    }

    async fn connect(base_url: &str, token: &str) -> Mcp {
        let mut mcp = Mcp {
            http: reqwest::Client::new(),
            url: format!("{base_url}/mcp"),
            token: token.to_string(),
            session: None,
            next_id: 0,
        };
        let (status, body) = mcp
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0731-marks", "version": "0.0.0"},
                },
            }))
            .await;
        assert!(status.is_success(), "initialize: {body}");
        let (status, body) = mcp
            .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert!(status.is_success(), "initialized: {body}");
        mcp
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool}: {text}");
        text
    }

    /// Whether the result is an error, and its text.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        self.next_id += 1;
        let (status, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await;
        assert!(status.is_success(), "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string();
        (result["isError"].as_bool().unwrap_or(false), text)
    }
}

/// Run the pass `section-checks` once as a periodic job.
async fn run_pass_as_job(url: &str) {
    let worker = Arc::new(Worker::new(
        BlockingTenantPool::new(url, 8),
        Uuid::new_v4(),
        JobSettings {
            heartbeat: Duration::from_millis(100),
            lease: Duration::from_millis(600),
            max_tries: 3,
            poll: Duration::from_millis(20),
            retry_after: Duration::ZERO,
            ..JobSettings::default()
        },
    ));
    worker
        .add_periodic(
            periodic::SECTION_CHECKS,
            periodic::SECTION_CHECKS_INTERVAL,
            periodic::section_checks_pass(url.to_string()),
        )
        .await
        .expect("add")
        .expect("the first run");
    worker.drain().await;
}

/// Run the pass directly (the next run of the job waits one interval).
fn run_pass(conn: &mut PgConnection) -> kairos_db::section_checks::PendingOutcome {
    kairos_db::section_checks::pass(conn, kairos_db::section_checks::PASS_LIMIT, &mut || {})
        .expect("the pass")
}

/// Is the check of the document `code` stale?
fn stale(conn: &mut PgConnection, code: &str) -> Option<bool> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        stale: bool,
    }
    diesel::sql_query(
        "SELECT c.stale FROM org_acme.section_checks c \
           JOIN org_acme.documents d ON d.id = c.document_id WHERE d.short_code = $1",
    )
    .bind::<diesel::sql_types::Text, _>(code)
    .get_result::<Row>(conn)
    .optional()
    .expect("read the check")
    .map(|row| row.stale)
}

#[tokio::test]
async fn documents_that_fail_their_typed_sections_get_a_mark() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let svc_token = user_token(&http, "svc").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let server = spawn_server(app::router(app::state_with(
        base_config(&scratch_url),
        pool,
        auth,
    )))
    .await;
    let admin = server.client(&svc_token, "acme");
    let _ = admin.whoami().await;
    let svc: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let board = team.delivery_board_id.clone().expect("delivery board");
    let template = admin
        .create_template(&CreateTemplateRequest {
            name: "Brief".into(),
            slug: "brief".into(),
            content: "## Goal\n\n## Notes\n".into(),
            metadata: Vec::new(),
            sections: vec![
                TemplateSection {
                    heading: "Goal".into(),
                    section_type: "plain".into(),
                },
                TemplateSection {
                    heading: "Notes".into(),
                    section_type: "free".into(),
                },
            ],
        })
        .await
        .expect("template");
    let create = |title: &str, content: &str| CreateDocumentRequest {
        title: title.into(),
        board: board.clone(),
        content: Some(content.into()),
        template_id: Some(template.id.clone()),
        parent_short_code: None,
    };
    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;

    // 1. A REST write of failing content is saved, and gets the mark.
    let failing = admin
        .create_document(&create("Failing", FAILING))
        .await
        .expect("a REST create with failing content is saved");
    let code = failing.short_code.clone();
    let read = admin.get_document(&code).await.expect("get document");
    let check = read.section_check.expect("the check of a typed document");
    assert!(!check.passes, "{check:?}");
    assert_eq!(check.version, 1);
    assert!(check.violations >= 2, "{check:?}");
    assert_eq!(check.sections.len(), 1, "only Goal fails: {check:?}");
    assert_eq!(check.sections[0].heading, "Goal");
    assert_eq!(check.sections[0].problem, "violations");
    assert!(
        check.sections[0]
            .violations
            .iter()
            .any(|v| v.rule == "STE-V1" && v.text == "server"),
        "{check:?}"
    );
    let text = mcp.call_ok("get_item", json!({"short_code": code})).await;
    let mark = format!(
        "- sections: does not pass ({} violations)\n",
        check.violations
    );
    assert!(text.contains(&mark), "{text}");
    assert!(
        text.contains("  - Section \"Goal\" (plain), STE-V1: \"server\". \"server\" is not in"),
        "{text}"
    );
    // A document with no template has no check and no mark.
    let free = admin
        .create_document(&CreateDocumentRequest {
            title: "Free".into(),
            board: board.clone(),
            content: Some(FAILING.into()),
            template_id: None,
            parent_short_code: None,
        })
        .await
        .expect("a free document");
    let read = admin.get_document(&free.short_code).await.expect("get");
    assert!(read.section_check.is_none());
    let text = mcp
        .call_ok("get_item", json!({"short_code": free.short_code}))
        .await;
    assert!(!text.contains("- sections:"), "{text}");

    // 2. An old document: no check (as before this change). The pass of
    // the job checks it.
    let old = admin
        .create_document(&create("Old", FAILING))
        .await
        .expect("an old document");
    diesel::sql_query(
        "DELETE FROM org_acme.section_checks WHERE document_id = \
         (SELECT id FROM org_acme.documents WHERE short_code = $1)",
    )
    .bind::<diesel::sql_types::Text, _>(&old.short_code)
    .execute(&mut conn)
    .expect("remove the check");
    let read = admin.get_document(&old.short_code).await.expect("get");
    assert!(read.section_check.is_none(), "no check before the pass");
    run_pass_as_job(&scratch_url).await;
    let read = admin.get_document(&old.short_code).await.expect("get");
    let check = read
        .section_check
        .expect("the pass checked the old document");
    assert!(!check.passes && !check.stale, "{check:?}");
    let text = mcp
        .call_ok("get_item", json!({"short_code": old.short_code}))
        .await;
    assert!(text.contains("- sections: does not pass ("), "{text}");
    // A second pass has nothing to do.
    assert_eq!(run_pass(&mut conn).checked, 0);

    // 3. A fix clears the mark: the answer of the write has the new check.
    let fixed = admin
        .update_document(
            &code,
            &UpdateContentRequest {
                title: None,
                content: PASSING.into(),
                version: 1,
            },
        )
        .await
        .expect("a fix");
    let check = fixed.section_check.expect("the check in the answer");
    assert!(check.passes && check.violations == 0 && check.sections.is_empty());
    assert_eq!(check.version, 2);
    let text = mcp.call_ok("get_item", json!({"short_code": code})).await;
    assert!(!text.contains("- sections:"), "{text}");

    // 4. An MCP edit of "Notes" on a failing document is saved; the whole
    // content still fails, so the mark stays.
    mcp.call_ok(
        "edit_item",
        json!({"short_code": old.short_code, "search": "Any text.", "replace": "Other text."}),
    )
    .await;
    let read = admin.get_document(&old.short_code).await.expect("get");
    let check = read.section_check.expect("check");
    assert!(!check.passes);
    assert_eq!(check.version, 2);

    // 5. KAIROS-A-0079: a dictionary on the team board is refused. The
    // dictionary of the organization makes each check stale, also of a
    // document on the team board, and the pass checks them again.
    let widget = admin
        .create_document(&create("Widget", WIDGET))
        .await
        .expect("widget");
    let read = admin.get_document(&widget.short_code).await.expect("get");
    let check = read.section_check.expect("check");
    assert!(
        check.sections[0]
            .violations
            .iter()
            .any(|v| v.text == "widget"),
        "{check:?}"
    );
    let dictionary = |board: &str| {
        json!({
            "item_type": "document",
            "title": "Dictionary",
            "board": board,
            "template": "dictionary",
            "content": "| Word | Part of speech | Meaning |\n|---|---|---|\n\
                        | widget | n | A part of the screen. |\n",
        })
    };
    let (is_error, text) = mcp.call("create_item", dictionary(&board)).await;
    assert!(
        is_error && text.starts_with("DICTIONARY_NOT_ON_STRATEGY"),
        "{text}"
    );
    assert_eq!(stale(&mut conn, &widget.short_code), Some(false));
    mcp.call_ok("create_item", dictionary("strategy")).await;
    assert_eq!(stale(&mut conn, &widget.short_code), Some(true));
    assert_eq!(
        stale(&mut conn, &old.short_code),
        Some(true),
        "each check is stale"
    );
    let read = admin.get_document(&widget.short_code).await.expect("get");
    assert!(read.section_check.expect("check").stale);
    let outcome = run_pass(&mut conn);
    assert!(outcome.checked >= 1, "{outcome:?}");
    let read = admin.get_document(&widget.short_code).await.expect("get");
    let check = read.section_check.expect("check");
    assert!(check.passes && !check.stale, "{check:?}");

    // 6. The template loses "Goal": each check of its documents is stale,
    // and the pass clears the mark of the failing documents.
    admin
        .update_template(
            &template.id,
            &UpdateTemplateRequest {
                name: None,
                slug: None,
                content: None,
                metadata: None,
                sections: Some(vec![TemplateSection {
                    heading: "Notes".into(),
                    section_type: "free".into(),
                }]),
            },
        )
        .await
        .expect("new sections");
    assert_eq!(stale(&mut conn, &old.short_code), Some(true));
    run_pass(&mut conn);
    let read = admin.get_document(&old.short_code).await.expect("get");
    let check = read.section_check.expect("check");
    assert!(check.passes && !check.stale, "{check:?}");

    // 7. A template with no typed section now: the pass removes the stale
    // checks.
    admin
        .update_template(
            &template.id,
            &UpdateTemplateRequest {
                name: None,
                slug: None,
                content: None,
                metadata: None,
                sections: Some(Vec::new()),
            },
        )
        .await
        .expect("no sections");
    let outcome = run_pass(&mut conn);
    assert!(outcome.removed >= 3, "{outcome:?}");
    let read = admin.get_document(&old.short_code).await.expect("get");
    assert!(read.section_check.is_none());
}
