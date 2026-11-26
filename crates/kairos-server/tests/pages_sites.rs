//! KAIROS-T-0404 — the rendered org and team sites (KAIROS-D-0013),
//! against the booted production router:
//!
//! - inclusion: a draft (badged), an archived document (left out), a
//!   superseded ADR (marked, linked to the newer one), an org document
//!   grouped under the initiative it supports, a team page in a folder, a
//!   document about a repository in that repository's section;
//! - a team with its site off: 404, and not on the org landing;
//! - rendering: a `javascript:` link is text, a task code is text, a
//!   document code is a link, a code in a code span is left alone, a code in
//!   the wrong place and a retired code redirect;
//! - the headers: the CSP, an ETag, 304.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`). No Dex.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use common::{AUDIENCE, ISSUER, base_config, recreate_scratch_db, with_database};
use kairos_db::local_auth as db_local_auth;
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::local_auth::hash_password;
use kairos_server::middleware::auth::Authenticator;

const PASSWORD: &str = "correct-horse-battery-staple";
const SCRATCH_DB: &str = "kairos_pages_sites_t0404_test";
const HOST: &str = "kairos.test";

struct Reply {
    status: StatusCode,
    location: Option<String>,
    etag: Option<String>,
    csp: Option<String>,
    body: String,
}

async fn page(router: &Router, uri: &str, cookie: &str, if_none_match: Option<&str>) -> Reply {
    let mut builder = Request::builder()
        .uri(uri)
        .header("host", HOST)
        .header("cookie", format!("kairos_session={cookie}"));
    if let Some(tag) = if_none_match {
        builder = builder.header("if-none-match", tag);
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .map(|v| v.to_str().expect("ascii").to_string())
    };
    let (status, location, etag, csp) = (
        response.status(),
        header("location"),
        header("etag"),
        header("content-security-policy"),
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    Reply {
        status,
        location,
        etag,
        csp,
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

async fn api(
    router: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> Value {
    // KAIROS-T-0792: a create takes no content; the text is an update.
    if let Some((family, text, create)) = common::create_with_text(&method, uri, body.as_ref()) {
        let created = Box::pin(api_raw(router, method, uri, token, Some(create))).await;
        if text.trim().is_empty() || created["content"] == Value::String(text.clone()) {
            return created;
        }
        let code = created["short_code"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        return api_raw(
            router,
            Method::PATCH,
            &format!("/api/{family}/{code}"),
            token,
            Some(serde_json::json!({ "content": text, "version": created["version"] })),
        )
        .await;
    }
    api_raw(router, method, uri, token, body).await
}

async fn api_raw(
    router: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> Value {
    let builder = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header("host", HOST)
        .header("x-tenant", "acme")
        .header("authorization", format!("Bearer {token}"));
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(status.is_success(), "{method} {uri}: {status} {value}");
    value
}

fn fixture() -> (PgConnection, common::ScratchUrl) {
    let admin = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "local:robin".into(),
            user_name: "robin@example.test".into(),
            email: "robin@example.test".into(),
            display_name: "Robin".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("insert user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user.id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    let hash = hash_password(PASSWORD).expect("hash");
    db_local_auth::set_password(&mut conn, user.id, &hash).expect("password");
    (conn, common::ScratchUrl::new(admin, scratch_url))
}

async fn login(router: &Router) -> String {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "email": "robin@example.test", "password": PASSWORD }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    body["token"].as_str().expect("token").to_string()
}

fn code(item: &Value) -> String {
    item["short_code"].as_str().expect("short code").to_string()
}

#[tokio::test]
async fn the_rendered_sites_against_live_stack() {
    let (mut conn, scratch_url) = fixture();
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    let router = app::router(app::state_with(config, pool, auth));
    let t = login(&router).await;
    let post = |uri: &'static str, body: Value| {
        let router = router.clone();
        let t = t.clone();
        async move { api(&router, Method::POST, uri, &t, Some(body)).await }
    };

    // --- the fixture ----------------------------------------------------------
    let team = post(
        "/api/teams",
        json!({"name": "Platform", "slug": "platform", "code_prefix": "PLAT"}),
    )
    .await;
    let team_id = team["id"].as_str().unwrap().to_string();
    let delivery = team["delivery_board_id"].as_str().unwrap().to_string();
    let adr_board = post(
        "/api/boards",
        json!({"name": "Platform decisions", "slug": "platform-adrs", "board_level": "adr",
               "team_id": team_id, "code_prefix": "PLAT"}),
    )
    .await;
    let adr_board_id = adr_board["id"].as_str().unwrap().to_string();
    let boards = api(&router, Method::GET, "/api/boards?limit=200", &t, None).await;
    let board_id = |slug: &str| {
        boards["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["slug"] == slug)
            .unwrap_or_else(|| panic!("board {slug}: {boards}"))["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let initiatives = board_id("initiatives");
    let org_adrs = board_id("adrs");
    post(
        "/api/repositories",
        json!({"slug": "payments", "forge": "other", "repo_full_name": "payments",
               "repo_url": "https://example.com/payments", "team": "platform"}),
    )
    .await;

    let task = post(
        "/api/tasks",
        json!({"board_id": delivery, "title": "A task", "content": ""}),
    )
    .await;
    let task_code = code(&task);
    let guide = post(
        "/api/documents",
        json!({"board": delivery, "title": "Payments guide", "content": "The guide."}),
    )
    .await;
    let guide_code = code(&guide);
    api(
        &router,
        Method::PATCH,
        &format!("/api/documents/{guide_code}/lifecycle"),
        &t,
        Some(json!({"lifecycle": "published"})),
    )
    .await;
    api(
        &router,
        Method::POST,
        &format!("/api/documents/{guide_code}/impacts"),
        &t,
        Some(json!({"repository": "payments"})),
    )
    .await;
    let body = format!(
        "See {guide_code} and {task_code}, `{guide_code}` in code.\n\n[click](javascript:alert(1))\n"
    );
    let notes = post(
        "/api/documents",
        json!({"board": delivery, "title": "Draft notes", "content": body}),
    )
    .await;
    let notes_code = code(&notes);
    let gone = post(
        "/api/documents",
        json!({"board": delivery, "title": "Gone document", "content": "x"}),
    )
    .await;
    let gone_code = code(&gone);
    api(
        &router,
        Method::DELETE,
        &format!("/api/documents/{gone_code}"),
        &t,
        None,
    )
    .await;

    let old_adr = post(
        "/api/adrs",
        json!({"board_id": adr_board_id, "title": "Old decision", "content": "Old."}),
    )
    .await;
    let new_adr = post(
        "/api/adrs",
        json!({"board_id": adr_board_id, "title": "New decision", "content": "New."}),
    )
    .await;
    let (old_code, new_code) = (code(&old_adr), code(&new_adr));
    post(
        "/api/relationships",
        json!({"source_short_code": new_code, "target_short_code": old_code,
               "relationship": "supersedes"}),
    )
    .await;

    let initiative = post(
        "/api/initiatives",
        json!({"board_id": initiatives, "title": "Docs sites", "content": ""}),
    )
    .await;
    let initiative_code = code(&initiative);
    let org_doc = post(
        "/api/documents",
        json!({"board": initiatives, "title": "Org spec", "content": "Org."}),
    )
    .await;
    let org_doc_code = code(&org_doc);
    post(
        "/api/relationships",
        json!({"source_short_code": initiative_code, "target_short_code": org_doc_code,
               "relationship": "supports"}),
    )
    .await;
    let org_adr = post(
        "/api/adrs",
        json!({"board_id": org_adrs, "title": "Org decision", "content": "Org ADR."}),
    )
    .await;
    let org_adr_code = code(&org_adr);

    let pages = format!("/api/teams/{team_id}/pages");
    let folder = api(
        &router,
        Method::POST,
        &pages,
        &t,
        Some(json!({"kind": "folder", "slug": "runbooks", "title": "Runbooks"})),
    )
    .await;
    api(
        &router,
        Method::POST,
        &pages,
        &t,
        Some(json!({"kind": "page", "slug": "deploy", "title": "Deploy",
                    "content": format!("Read {guide_code}."), "parent_id": folder["id"]})),
    )
    .await;

    // --- the org landing --------------------------------------------------------
    let org = page(&router, "/pages/acme/", &t, None).await;
    assert_eq!(org.status, StatusCode::OK, "{}", org.body);
    assert!(
        org.body.contains(r#"href="/pages/acme/platform/""#),
        "{}",
        org.body
    );
    assert!(org.body.contains(&initiative_code) && org.body.contains("Docs sites"));
    assert!(
        org.body
            .contains(&format!(r#"href="/pages/acme/docs/{org_doc_code}/""#))
    );
    assert!(
        org.body
            .contains(&format!(r#"href="/pages/acme/adrs/{org_adr_code}/""#))
    );

    // --- the team landing -------------------------------------------------------
    let landing = page(&router, "/pages/acme/platform/", &t, None).await;
    assert_eq!(landing.status, StatusCode::OK, "{}", landing.body);
    assert!(
        landing.body.contains("<h1>Platform</h1>"),
        "{}",
        landing.body
    );
    let repo_section = landing
        .body
        .split("Repository <span class=\"kp-code\">payments</span>")
        .nth(1)
        .expect("a section for the repository");
    let repo_section = repo_section.split("</section>").next().unwrap();
    assert!(
        repo_section.contains(&guide_code),
        "the guide is about payments: {repo_section}"
    );
    assert!(landing.body.contains("Other documents"));
    assert!(landing.body.contains(&notes_code));
    assert!(
        landing
            .body
            .contains(r#"<span class="kp-badge">draft</span>"#),
        "a draft is badged"
    );
    assert!(
        !landing.body.contains(&gone_code),
        "an archived document is left out"
    );
    assert!(
        landing.body.contains(&format!("superseded by {new_code}")),
        "{}",
        landing.body
    );
    assert!(
        landing
            .body
            .contains(r#"href="/pages/acme/platform/runbooks/deploy/""#)
    );

    // --- a document page: rendering and links -------------------------------------
    let notes_page = page(
        &router,
        &format!("/pages/acme/platform/docs/{notes_code}/"),
        &t,
        None,
    )
    .await;
    assert_eq!(notes_page.status, StatusCode::OK, "{}", notes_page.body);
    let body = &notes_page.body;
    assert!(
        body.contains(&format!(
            r#"<a href="/pages/acme/platform/docs/{guide_code}/">{guide_code}</a>"#
        )),
        "a document code is a link: {body}"
    );
    assert!(
        body.contains(&format!("and {task_code},")),
        "a task code is text: {body}"
    );
    assert!(
        body.contains(&format!("<code>{guide_code}</code>")),
        "a code span stays: {body}"
    );
    assert!(
        !body.contains("javascript:"),
        "a javascript: link is text: {body}"
    );
    assert!(body.contains("click"));
    assert!(
        body.contains(&format!(r#"href="/items/{notes_code}""#)),
        "Open in Kairos"
    );

    // --- the superseded ADR links to the newer one ---------------------------------
    let old = page(
        &router,
        &format!("/pages/acme/platform/adrs/{old_code}/"),
        &t,
        None,
    )
    .await;
    assert_eq!(old.status, StatusCode::OK, "{}", old.body);
    assert!(
        old.body
            .contains(&format!(r#"href="/pages/acme/platform/adrs/{new_code}/""#))
    );

    // --- a team page in a folder, and the folder ---------------------------------
    let deploy = page(&router, "/pages/acme/platform/runbooks/deploy/", &t, None).await;
    assert_eq!(deploy.status, StatusCode::OK, "{}", deploy.body);
    assert!(deploy.body.contains("<h1>Deploy</h1>"));
    assert!(deploy.body.contains(&format!(
        r#"href="/pages/acme/platform/docs/{guide_code}/""#
    )));
    let folder_page = page(&router, "/pages/acme/platform/runbooks/", &t, None).await;
    assert_eq!(folder_page.status, StatusCode::OK);
    assert!(
        folder_page
            .body
            .contains(r#"href="/pages/acme/platform/runbooks/deploy/""#)
    );

    // --- an archived document, a wrong place, a retired code ----------------------
    let reply = page(
        &router,
        &format!("/pages/acme/platform/docs/{gone_code}/"),
        &t,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = page(
        &router,
        &format!("/pages/acme/docs/{guide_code}/"),
        &t,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        reply.location.as_deref(),
        Some(format!("/pages/acme/platform/docs/{guide_code}/").as_str())
    );
    let guide_id: Uuid = diesel::sql_query(format!(
        "SELECT id FROM org_acme.documents WHERE short_code = '{guide_code}'"
    ))
    .get_result::<IdRow>(&mut conn)
    .expect("guide id")
    .id;
    diesel::sql_query(format!(
        "INSERT INTO org_acme.retired_codes (code, item_id, reason) \
         VALUES ('OLD-D-0001', '{guide_id}', 'rename')"
    ))
    .execute(&mut conn)
    .expect("a retired code");
    let reply = page(&router, "/pages/acme/platform/docs/OLD-D-0001/", &t, None).await;
    assert_eq!(
        reply.status,
        StatusCode::MOVED_PERMANENTLY,
        "{}",
        reply.body
    );
    assert_eq!(
        reply.location.as_deref(),
        Some(format!("/pages/acme/platform/docs/{guide_code}/").as_str())
    );

    // --- the headers: CSP, ETag, 304 -------------------------------------------------
    let first = page(&router, "/pages/acme/platform/", &t, None).await;
    let csp = first.csp.expect("a CSP");
    assert!(
        csp.starts_with("default-src 'none'; script-src 'self';"),
        "{csp}"
    );
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    let etag = first.etag.expect("an ETag");
    let again = page(&router, "/pages/acme/platform/", &t, Some(&etag)).await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);

    // --- the assets ---------------------------------------------------------------------
    let css = page(&router, "/pages/_/pages.css", &t, None).await;
    assert!(css.body.contains("--bg:") && css.body.contains(".kairos-markdown"));
    let js = page(&router, "/pages/_/pages.js", &t, None).await;
    assert!(js.body.contains("securityLevel: \"strict\""));
    let mermaid = page(&router, "/pages/_/mermaid.min.js", &t, None).await;
    assert_eq!(mermaid.status, StatusCode::OK);

    // --- a team with its site off ----------------------------------------------------
    api(
        &router,
        Method::PATCH,
        &format!("/api/teams/{team_id}"),
        &t,
        Some(json!({"site_enabled": false})),
    )
    .await;
    for path in [
        "/pages/acme/platform/".to_string(),
        format!("/pages/acme/platform/docs/{guide_code}/"),
        "/pages/acme/platform/runbooks/deploy/".to_string(),
    ] {
        let reply = page(&router, &path, &t, None).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
    let org = page(&router, "/pages/acme/", &t, None).await;
    assert!(
        !org.body.contains(r#"href="/pages/acme/platform/""#),
        "{}",
        org.body
    );
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    id: Uuid,
}
