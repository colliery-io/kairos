//! KAIROS-T-0505 — `POST /api/{family}/{short_code}/notes/{note_id}/promote`:
//! a note becomes a draft ADR or document in one call. The new item has the
//! text of the note and a line that names the note; it `supports` the
//! source item; the note gets `promoted_to` (KAIROS-T-0630: a column of the
//! note row; the content and its version do not change). A second promote
//! is CONFLICT and names the item. The edit rule and the right to create on
//! the target board apply. The MCP tool `promote_note` is in
//! `tests/mcp.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_promote_note_t0505_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{item_relationships, organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_promote_note_t0505_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

#[tokio::test]
async fn a_note_becomes_a_draft_adr_or_document_against_live_stack() {
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
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 8).await.expect("pool");
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
    for (user, user_token, role) in [
        ("svc", &token, OrgRole::Admin),
        ("bob", &bob_token, OrgRole::Member),
    ] {
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(user_token),
            &TENANT,
            None,
        )
        .await;
        let user_id: Uuid = users::table
            .filter(users::email.eq(format!("{user}@kairos.test")))
            .select(users::id)
            .first(&mut conn)
            .expect("user provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let svc_name: String = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::display_name)
        .first(&mut conn)
        .expect("svc display name");

    let send = |method: Method, uri: String, token: String, body: Option<Value>| {
        let router = router.clone();
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    let (status, team) = send(
        Method::POST,
        "/api/teams".into(),
        token.clone(),
        Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
    )
    .await;
    assert!(status.is_success(), "{status} {team}");
    let (status, task) = send(
        Method::POST,
        "/api/tasks".into(),
        token.clone(),
        Some(json!({
            "board_id": "payments-delivery",
            "title": "Pay",
            "content": "# Pay\n",
        })),
    )
    .await;
    assert!(status.is_success(), "{status} {task}");
    let code = task["short_code"].as_str().expect("short code").to_string();
    let id: Uuid = task["id"].as_str().expect("id").parse().expect("uuid");
    for (text, note_id) in [
        ("Use one ledger.\n\nOne table for each currency.", "n-1"),
        ("Proposal: keep the refund path.\n\nThe details.", "n-2"),
        ("A third note, for the refusals.", "n-3"),
    ] {
        let (status, answer) = send(
            Method::POST,
            format!("/api/tasks/{code}/notes"),
            token.clone(),
            Some(json!({ "text": text })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(answer["note_id"], note_id, "{answer}");
    }
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    // --- a note becomes an ADR with a title ----------------------------------
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-1/promote"),
        token.clone(),
        Some(json!({"kind": "adr", "board": "adrs", "title": "One ledger"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["short_code"], code.as_str());
    assert_eq!(answer["note_id"], "n-1");
    assert_eq!(answer["kind"], "adr");
    assert_eq!(answer["supports"], true);
    let adr_code = answer["promoted_to"]
        .as_str()
        .expect("adr code")
        .to_string();
    assert!(adr_code.contains("-A-"), "{adr_code}");
    let (status, adr) = send(
        Method::GET,
        format!("/api/adrs/{adr_code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{adr}");
    assert_eq!(adr["title"], "One ledger");
    assert_eq!(
        adr["content"],
        format!(
            "Use one ledger.\n\nOne table for each currency.\n\nFrom note n-1 on {code} ({svc_name}, {today}).\n"
        ),
        "{adr}"
    );
    let adr_id: Uuid = adr["id"].as_str().expect("id").parse().expect("uuid");
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let edges: Vec<(Uuid, Uuid, String)> = item_relationships::table
        .filter(item_relationships::source_id.eq(id))
        .select((
            item_relationships::source_id,
            item_relationships::target_id,
            item_relationships::relationship,
        ))
        .load(&mut conn)
        .expect("edges");
    assert!(
        edges.contains(&(id, adr_id, "supports".to_string())),
        "the ADR supports the task: {edges:?}"
    );
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    // KAIROS-T-0630: the mark is on the note; the content and its version
    // do not change.
    assert_eq!(got["content"], "# Pay\n", "{got}");
    // KAIROS-T-0792: version 1 is the template, version 2 the text.
    assert_eq!(got["version"], 2, "a promote writes no version: {got}");
    assert_eq!(got["notes"][0]["promoted_to"], adr_code.as_str(), "{got}");
    assert!(got["notes"][1].get("promoted_to").is_none(), "{got}");

    // --- KAIROS-T-0551: a second promote is CONFLICT and names the item -------
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-1/promote"),
        token.clone(),
        Some(json!({"kind": "document", "board": "payments-delivery"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(answer["error"]["code"], "CONFLICT", "{answer}");
    assert_eq!(answer["error"]["details"]["promoted_to"], adr_code.as_str());
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains(&adr_code) && m.contains("n-1")),
        "{answer}"
    );

    // --- the new note becomes a document; the title is its first line --------
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-2/promote"),
        token.clone(),
        Some(json!({"kind": "document", "board": "payments-delivery"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["kind"], "document");
    assert_eq!(answer["supports"], true);
    let doc_code = answer["promoted_to"]
        .as_str()
        .expect("doc code")
        .to_string();
    let (status, doc) = send(
        Method::GET,
        format!("/api/documents/{doc_code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["title"], "Proposal: keep the refund path.");
    assert_eq!(
        doc["content"],
        format!(
            "Proposal: keep the refund path.\n\nThe details.\n\nFrom note n-2 on {code} ({svc_name}, {today}).\n"
        ),
        "{doc}"
    );
    let doc_id: Uuid = doc["id"].as_str().expect("id").parse().expect("uuid");
    let edges: Vec<(Uuid, Uuid, String)> = item_relationships::table
        .filter(item_relationships::source_id.eq(id))
        .select((
            item_relationships::source_id,
            item_relationships::target_id,
            item_relationships::relationship,
        ))
        .load(&mut conn)
        .expect("edges");
    assert!(
        edges.contains(&(id, doc_id, "supports".to_string())),
        "the document supports the task: {edges:?}"
    );
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(got["notes"][1]["promoted_to"], doc_code.as_str(), "{got}");
    assert_eq!(
        got["notes"][1]["text"],
        "Proposal: keep the refund path.\n\nThe details."
    );

    // --- a note on a document: no supports edge ---------------------------------
    let (status, answer) = send(
        Method::POST,
        format!("/api/documents/{doc_code}/notes"),
        token.clone(),
        Some(json!({"text": "A decision hides here."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (status, answer) = send(
        Method::POST,
        format!("/api/documents/{doc_code}/notes/n-1/promote"),
        token.clone(),
        Some(json!({"kind": "adr", "board": "adrs"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["supports"], false, "{answer}");

    // --- refusals ---------------------------------------------------------------
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-9/promote"),
        token.clone(),
        Some(json!({"kind": "adr", "board": "adrs"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("n-1, n-2, n-3")),
        "the refusal lists the notes: {answer}"
    );
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-1/promote"),
        token.clone(),
        Some(json!({"kind": "spec", "board": "adrs"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["code"], "VALIDATION", "{answer}");
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-1/promote"),
        token.clone(),
        Some(json!({"kind": "adr", "board": "adrs", "column": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-3/promote"),
        bob_token.clone(),
        Some(json!({"kind": "adr", "board": "adrs"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{code}/notes/n-3/promote"),
        token.clone(),
        Some(json!({"kind": "adr", "board": "no-such-board"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    assert_eq!(got["version"], 2, "{got}");
    assert!(
        got["notes"][2].get("promoted_to").is_none(),
        "a refused promote writes nothing: {got}"
    );

    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
