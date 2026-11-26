//! KAIROS-T-0367, KAIROS-T-0630 — `POST /api/{entity_type}/{short_code}/notes`:
//! a note is a row of its own, in one call with no version. It writes no
//! version of the content and no history row; the read of the item gives
//! it in `notes`, and `content` has no note. A note can have headings.
//! Notes sent at the same time all stay, each with its own id; the edit
//! rule applies. The MCP tool `append_note` is in `tests/mcp.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_append_note_t0367_test`.

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
use kairos_db::schema::{item_history, organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_append_note_t0367_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];
/// How many notes the test sends at the same time.
const AT_ONCE: usize = 8;

#[tokio::test]
async fn append_note_adds_a_dated_note_in_one_version_against_live_stack() {
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
            "content": "# Pay\n\nThe body.\n",
        })),
    )
    .await;
    assert!(status.is_success(), "{status} {task}");
    let code = task["short_code"].as_str().expect("short code").to_string();
    let id: Uuid = task["id"].as_str().expect("id").parse().expect("uuid");
    let notes = format!("/api/tasks/{code}/notes");

    // --- one note: one call, no version, no new version ---------------------
    let (status, answer) = send(
        Method::POST,
        notes.clone(),
        token.clone(),
        Some(json!({"text": "Found the cause.\n\n## Findings\n\nThe fuse.\n"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["short_code"], code.as_str());
    // KAIROS-T-0792: version 1 is the template, version 2 the text.
    assert_eq!(answer["version"], 2, "a note writes no version");
    // KAIROS-T-0504: the first note is n-1.
    assert_eq!(answer["note_id"], "n-1", "{answer}");
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    // KAIROS-T-0630: the content has no note; `notes` has it, a heading
    // too.
    assert_eq!(got["content"], "# Pay\n\nThe body.\n", "{got}");
    assert_eq!(got["version"], 2, "{got}");
    let note = &got["notes"][0];
    assert_eq!(note["id"], "n-1", "{got}");
    assert_eq!(note["author"], svc_name.as_str(), "{got}");
    assert_eq!(
        note["text"], "Found the cause.\n\n## Findings\n\nThe fuse.",
        "{got}"
    );
    assert!(note["author_id"].is_string(), "{got}");
    assert!(note.get("promoted_to").is_none(), "{got}");
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    assert!(
        note["created_at"]
            .as_str()
            .is_some_and(|at| at.starts_with(&today)),
        "{got}"
    );
    // A list has no notes.
    let (_, list) = send(Method::GET, "/api/tasks".into(), token.clone(), None).await;
    assert!(
        list["items"]
            .as_array()
            .is_some_and(|items| items.iter().all(|item| item.get("notes").is_none())),
        "{list}"
    );
    let history = |conn: &mut PgConnection| -> i64 {
        item_history::table
            .filter(item_history::item_id.eq(id))
            .count()
            .get_result(conn)
            .expect("history count")
    };
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    assert_eq!(history(&mut conn), 2, "a note writes no history row");

    // --- refusals: an empty text, an unknown field, the edit rule ------------
    let (status, answer) = send(
        Method::POST,
        notes.clone(),
        token.clone(),
        Some(json!({"text": " \n "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["code"], "VALIDATION", "{answer}");
    let (status, answer) = send(
        Method::POST,
        notes.clone(),
        token.clone(),
        Some(json!({"text": "x", "version": 2})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    let (status, answer) = send(
        Method::POST,
        notes.clone(),
        bob_token.clone(),
        Some(json!({"text": "Not mine."})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    let (status, answer) = send(
        Method::POST,
        format!("/api/documents/{code}/notes"),
        token.clone(),
        Some(json!({"text": "Wrong family."})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(history(&mut conn), 2, "a refused note writes nothing");

    // --- notes at the same time: each one stays ------------------------------
    let calls = (0..AT_ONCE).map(|n| {
        send(
            Method::POST,
            notes.clone(),
            token.clone(),
            Some(json!({"text": format!("Note number {n}.")})),
        )
    });
    let answers = futures_util::future::join_all(calls).await;
    for (status, answer) in &answers {
        assert_eq!(*status, StatusCode::OK, "{answer}");
        assert_eq!(answer["version"], 2, "{answer}");
    }
    // KAIROS-T-0504: each note gets its own id, n-2 to n-9.
    let mut ids: Vec<String> = answers
        .iter()
        .map(|(_, answer)| answer["note_id"].as_str().expect("note id").to_string())
        .collect();
    ids.sort_by_key(|id| id[2..].parse::<usize>().expect("digits"));
    assert_eq!(
        ids,
        (2..2 + AT_ONCE)
            .map(|n| format!("n-{n}"))
            .collect::<Vec<_>>()
    );
    let (_, got) = send(
        Method::GET,
        format!("/api/tasks/{code}"),
        token.clone(),
        None,
    )
    .await;
    let rows = got["notes"].as_array().expect("notes");
    assert_eq!(rows.len(), AT_ONCE + 1, "{got}");
    for n in 0..AT_ONCE {
        assert_eq!(
            rows.iter()
                .filter(|note| note["text"] == format!("Note number {n}.").as_str())
                .count(),
            1,
            "note {n} is lost: {got}"
        );
    }
    assert_eq!(
        rows.iter()
            .map(|note| note["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        (1..=AT_ONCE + 1)
            .map(|n| format!("n-{n}"))
            .collect::<Vec<_>>(),
        "the notes come in the order of their ids"
    );
    assert_eq!(history(&mut conn), 2, "notes write no history row");

    // --- KAIROS-T-0630: a `## Note` heading in a content is content -------
    let (status, old) = send(
        Method::POST,
        "/api/tasks".into(),
        token.clone(),
        Some(json!({
            "board_id": "payments-delivery",
            "title": "Old notes",
            "content": "# Old\n\n## Note (2026-10-01, Robin)\n\nOne.\n",
        })),
    )
    .await;
    assert!(status.is_success(), "{status} {old}");
    let old_code = old["short_code"].as_str().expect("short code");
    let (status, answer) = send(
        Method::POST,
        format!("/api/tasks/{old_code}/notes"),
        token.clone(),
        Some(json!({"text": "The first row."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["note_id"], "n-1", "{answer}");

    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
