//! KAIROS-T-0730: an MCP write must fill and pass each typed section of the
//! template of the document (COLLIERY-I-0664 decisions 1 and 4).
//!
//! A template "brief" with two typed sections: "Goal" (`plain`) and
//! "Notes" (`free`). Over MCP, `create_item`, `update_item` and `edit_item`
//! refuse a missing section, an empty section and a `plain` section that
//! breaks a rule, with `SECTIONS_NOT_PASSED`, and save nothing. A passing
//! write succeeds. Over REST, the same failing content is saved. An edit
//! checks only the sections that it changes; a caller with no right to
//! create gets FORBIDDEN first; the dictionary of the team feeds the
//! vocabulary.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_typed_sections_t0730_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateDocumentRequest, UpdateContentRequest};
use kairos_client::types_meta::{CreateTemplateRequest, TemplateSection};
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_typed_sections_t0730_test";

/// Content that passes: each section has text, and the text of "Goal" is
/// plain and in the vendored word list.
const PASSING: &str = "## Goal\nWrite the item.\n\n## Notes\nAny text, utilize it.\n";

/// The "Goal" section breaks STE-V1 ("server") and the passive rule.
const FAILING: &str = "## Goal\nThe text is checked by the server.\n\n## Notes\nAny text.\n";

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
                    "clientInfo": {"name": "kairos-t0730-sections", "version": "0.0.0"},
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

    /// The text of the result, and whether it is a tool error.
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

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool}: {text}");
        text
    }

    /// The refusal text: `SECTIONS_NOT_PASSED: ...`, and its details.
    async fn call_refused(&mut self, tool: &str, arguments: Value) -> (String, Value) {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(is_error, "{tool} must be refused: {text}");
        assert!(
            text.starts_with("SECTIONS_NOT_PASSED: "),
            "{tool}: the code: {text}"
        );
        let details = text
            .rsplit_once("\ndetails: ")
            .map(|(_, details)| serde_json::from_str(details).expect("details JSON"))
            .unwrap_or_else(|| panic!("{tool}: no details in {text}"));
        (text, details)
    }
}

fn document_count(conn: &mut PgConnection) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query("SELECT count(*) AS n FROM org_acme.documents")
        .get_result::<Count>(conn)
        .expect("count documents")
        .n
}

/// The short code in "Created document ACME-D-0001: ...".
fn created_code(text: &str) -> String {
    text.strip_prefix("Created document ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or_else(|| panic!("not a create answer: {text}"))
        .to_string()
}

#[tokio::test]
async fn mcp_writes_must_fill_and_pass_each_typed_section() {
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

    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let create = |content: Option<&str>| {
        let mut arguments = json!({
            "item_type": "document",
            "title": "A brief",
            "board": board,
            "template": "brief",
        });
        if let Some(content) = content {
            arguments["content"] = json!(content);
        }
        arguments
    };

    // 1. No content: the content of the template, with two empty sections.
    let (text, details) = mcp.call_refused("create_item", create(None)).await;
    assert!(
        text.contains(
            "\n- Section \"Goal\" (plain): the section has no text. Write text under the heading."
        ),
        "{text}"
    );
    assert!(
        text.contains("\n- Section \"Notes\" (free): the section has no text."),
        "{text}"
    );
    assert!(text.contains("The server saved nothing."), "{text}");
    assert_eq!(details["field"], "content");
    assert_eq!(details["sections"][0]["heading"], "Goal");
    assert_eq!(details["sections"][0]["problem"], "empty");
    assert_eq!(details["sections"][1]["heading"], "Notes");
    assert_eq!(
        document_count(&mut conn),
        0,
        "a refused create saves nothing"
    );

    // 2. A missing section.
    let (text, details) = mcp
        .call_refused("create_item", create(Some("## Notes\nSome notes.\n")))
        .await;
    assert!(
        text.contains(
            "\n- Section \"Goal\" (plain): the content does not have this section. Add the \
             heading \"## Goal\" and text under it."
        ),
        "{text}"
    );
    assert_eq!(details["sections"].as_array().unwrap().len(), 1);
    assert_eq!(details["sections"][0]["problem"], "missing");
    assert_eq!(document_count(&mut conn), 0);

    // 3. A plain section that breaks rules: each violation is named.
    let (text, details) = mcp.call_refused("create_item", create(Some(FAILING))).await;
    assert!(
        text.contains(
            "\n- Section \"Goal\" (plain), STE-V1: \"server\". \"server\" is not in the vocabulary."
        ),
        "{text}"
    );
    assert!(
        text.contains("\"is checked\". Passive: is checked."),
        "{text}"
    );
    assert!(
        !text.contains("Section \"Notes\""),
        "a free section is not checked: {text}"
    );
    let violations = details["sections"][0]["violations"].as_array().unwrap();
    assert!(
        violations
            .iter()
            .any(|v| v["rule"] == "STE-V1" && v["text"] == "server"),
        "{details}"
    );
    assert!(
        violations
            .iter()
            .all(|v| v["fix"].as_str().is_some_and(|f| !f.is_empty()))
    );
    assert_eq!(document_count(&mut conn), 0);

    // 4. A passing create.
    let code = created_code(&mcp.call_ok("create_item", create(Some(PASSING))).await);
    assert_eq!(document_count(&mut conn), 1);

    // 5. update_item with failing content: refused, the version stays.
    let (text, _) = mcp
        .call_refused(
            "update_item",
            json!({"short_code": code, "content": FAILING, "version": 1}),
        )
        .await;
    assert!(
        text.starts_with(&format!(
            "SECTIONS_NOT_PASSED: The content of {code} does not pass"
        )),
        "{text}"
    );
    let read = admin.get_document(&code).await.expect("get document");
    assert_eq!((read.version, read.content.as_str()), (1, PASSING));

    // 6. edit_item that empties a section: refused, the version stays.
    let (text, details) = mcp
        .call_refused(
            "edit_item",
            json!({"short_code": code, "search": "Write the item.", "replace": ""}),
        )
        .await;
    assert!(
        text.contains("Section \"Goal\" (plain): the section has no text."),
        "{text}"
    );
    assert_eq!(details["sections"][0]["problem"], "empty");
    let read = admin.get_document(&code).await.expect("get document");
    assert_eq!(read.version, 1);

    // 7. Passing writes succeed.
    mcp.call_ok(
        "edit_item",
        json!({"short_code": code, "search": "Any text, utilize it.", "replace": "Other notes."}),
    )
    .await;
    mcp.call_ok(
        "update_item",
        json!({"short_code": code, "content": PASSING, "version": 2}),
    )
    .await;
    let read = admin.get_document(&code).await.expect("get document");
    assert_eq!((read.version, read.content.as_str()), (3, PASSING));

    // 8. A document with no template: no section check.
    mcp.call_ok(
        "create_item",
        json!({"item_type": "document", "title": "Free", "board": board, "content": FAILING}),
    )
    .await;
    assert_eq!(document_count(&mut conn), 2);

    // 9. REST is not refused: the same failing content is saved.
    let rest = admin
        .create_document(&CreateDocumentRequest {
            title: "From the web".into(),
            board: board.clone(),
            content: Some(FAILING.into()),
            template_id: Some(template.id.clone()),
            parent_short_code: None,
        })
        .await
        .expect("a REST create with failing content is saved");
    assert_eq!(rest.content, FAILING);
    let empty = admin
        .create_document(&CreateDocumentRequest {
            title: "Stamped".into(),
            board: board.clone(),
            content: None,
            template_id: Some(template.id.clone()),
            parent_short_code: None,
        })
        .await
        .expect("a REST create with empty sections is saved");
    assert_eq!(empty.content, "## Goal\n\n## Notes\n");
    let updated = admin
        .update_document(
            &code,
            &UpdateContentRequest {
                title: None,
                content: FAILING.into(),
                version: 3,
            },
        )
        .await
        .expect("a REST update with failing content is saved");
    assert_eq!((updated.version, updated.content.as_str()), (4, FAILING));

    // 10. Only the sections that an edit changes are checked (note n-4):
    // the document fails "Goal" after the REST write, and an MCP edit of
    // "Notes" passes.
    mcp.call_ok(
        "edit_item",
        json!({"short_code": code, "search": "Any text.", "replace": "More text."}),
    )
    .await;
    let read = admin.get_document(&code).await.expect("get document");
    assert_eq!(read.version, 5);
    // An update that leaves "Goal" as it was passes too.
    let notes_only = FAILING.replace("Any text.", "Other text.");
    mcp.call_ok(
        "update_item",
        json!({"short_code": code, "content": notes_only, "version": 5}),
    )
    .await;
    // An edit of "Goal" that still fails is refused.
    let (text, details) = mcp
        .call_refused(
            "edit_item",
            json!({"short_code": code, "search": "by the server", "replace": "by the team"}),
        )
        .await;
    assert!(text.contains("Passive: is checked."), "{text}");
    assert_eq!(details["sections"].as_array().unwrap().len(), 1);
    assert_eq!(details["sections"][0]["heading"], "Goal");
    let read = admin.get_document(&code).await.expect("get document");
    assert_eq!(read.version, 6);

    // 11. The right to create is checked before the typed sections: a
    // member with no right on the board gets FORBIDDEN, not
    // SECTIONS_NOT_PASSED.
    let alice_token = user_token(&http, "alice").await;
    let _ = server.client(&alice_token, "acme").whoami().await;
    let alice: Uuid = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("alice user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("membership");
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;
    let (is_error, text) = alice_mcp.call("create_item", create(Some(FAILING))).await;
    assert!(is_error && text.starts_with("FORBIDDEN"), "{text}");

    // 12. The dictionary of the team of the owner board feeds the
    // vocabulary (KAIROS-T-0733): "widget" fails, then passes when the
    // dictionary of the team has it.
    let widget = "## Goal\nWrite the widget.\n\n## Notes\nAny text.\n";
    let (text, _) = mcp.call_refused("create_item", create(Some(widget))).await;
    assert!(text.contains("STE-V1: \"widget\""), "{text}");
    mcp.call_ok(
        "create_item",
        json!({
            "item_type": "document",
            "title": "Dictionary",
            "board": board,
            "template": "dictionary",
            "content": "| Word | Part of speech | Meaning |\n|---|---|---|\n\
                        | widget | n | A part of the screen. |\n",
        }),
    )
    .await;
    mcp.call_ok("create_item", create(Some(widget))).await;

    // 13. An `ears` section (KAIROS-T-0732): the template takes the type, a
    // requirement that fits no frame is refused with its rule and the
    // closest frame, and nothing is saved. Words inside the frame are free.
    admin
        .create_template(&CreateTemplateRequest {
            name: "Spec".into(),
            slug: "spec".into(),
            content: "## Requirements\n".into(),
            metadata: Vec::new(),
            sections: vec![TemplateSection {
                heading: "Requirements".into(),
                section_type: "ears".into(),
            }],
        })
        .await
        .expect("a template with an ears section");
    let spec = |content: &str| {
        json!({
            "item_type": "document",
            "title": "A spec",
            "board": board,
            "template": "spec",
            "content": content,
        })
    };
    let before = document_count(&mut conn);
    let (text, details) = mcp
        .call_refused(
            "create_item",
            spec(
                "## Requirements\n\
                 - When a write fails, the server shall name the field.\n\
                 - A move names `reopen_item` when the item is done.\n",
            ),
        )
        .await;
    assert!(
        text.contains(
            "\n- Section \"Requirements\" (ears), EARS-1: \"A move names `reopen_item` when the \
             item is done.\". The requirement has no \"shall\". Put the \"when\" clause first"
        ),
        "{text}"
    );
    assert!(text.contains("Closest frame: event-driven"), "{text}");
    assert_eq!(details["sections"][0]["type"], "ears");
    assert_eq!(details["sections"][0]["violations"][0]["rule"], "EARS-1");
    assert_eq!(details["sections"].as_array().unwrap().len(), 1);
    assert_eq!(document_count(&mut conn), before, "nothing is saved");
    mcp.call_ok(
        "create_item",
        spec(
            "## Requirements\n\
             - When a write fails, the server shall utilize the field.\n\
             - If the item is done, then `move_item` shall name `reopen_item`.\n",
        ),
    )
    .await;
    assert_eq!(document_count(&mut conn), before + 1);
}
