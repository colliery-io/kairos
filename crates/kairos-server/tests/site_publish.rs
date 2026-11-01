//! KAIROS-T-0406 — CI publishes the built site of a repository
//! (KAIROS-D-0012), against the booted production router:
//!
//! - the repository gives the base path; the site is off by default;
//! - rights: no `pages` is 403, a team member is not enough, the site off is
//!   409, `pages` on the delivery board publishes, an agent key is recorded,
//!   an organization admin publishes;
//! - a wrong `Content-MD5` and a refused archive change nothing;
//! - a publish replaces the live one in one step, and the old files are
//!   queued for deletion after the grace time; an unpublish queues them now;
//! - a move to another team sets `needs_republish`;
//! - no site store is 501; a body over the limit is 413.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`). No Dex.

mod common;

use std::io::Write as _;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt as _;
use md5::{Digest as _, Md5};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use common::{AUDIENCE, ISSUER, base_config, recreate_scratch_db, with_database};
use kairos_db::local_auth as db_local_auth;
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::AppConfig;
use kairos_server::local_auth::hash_password;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::site_store::StoreConfig;

const PASSWORD: &str = "correct-horse-battery-staple";
const SCRATCH_DB: &str = "kairos_site_publish_t0406_test";
const SITE: &str = "/api/repositories/payments/site";

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    bearer: &str,
    headers: &[(&str, String)],
    body: Option<Vec<u8>>,
    json_body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "kairos.test")
        .header("x-tenant", "acme")
        .header("authorization", format!("Bearer {bearer}"));
    for (name, value) in headers {
        builder = builder.header(*name, value.as_str());
    }
    let request = match (body, json_body) {
        (Some(bytes), _) => builder
            .header("content-type", "application/gzip")
            .body(Body::from(bytes)),
        (None, Some(json)) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        (None, None) => builder.body(Body::empty()),
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
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

fn add_user(conn: &mut PgConnection, org_id: Uuid, email: &str, role: OrgRole) -> Uuid {
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("local:{email}"),
            user_name: email.into(),
            email: email.into(),
            display_name: email.into(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user.id,
            role,
        })
        .execute(conn)
        .expect("membership");
    let hash = hash_password(PASSWORD).expect("hash");
    db_local_auth::set_password(conn, user.id, &hash).expect("password");
    user.id
}

async fn login(router: &Router, email: &str) -> String {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "email": email, "password": PASSWORD }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    body["token"].as_str().expect("token").to_string()
}

/// A `.tar.gz` of `files`, each a regular file.
fn site(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        for (path, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(data.len() as u64);
            builder
                .append_data(&mut header, path, *data)
                .expect("append");
        }
        builder
            .into_inner()
            .expect("tar")
            .finish()
            .expect("gz")
            .flush()
            .expect("flush");
    }
    out
}

/// A `.tar.gz` with `index.html` and a symlink `link`.
fn site_with_symlink() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(1);
        builder
            .append_data(&mut header, "index.html", &b"x"[..])
            .expect("append");
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        builder
            .append_link(&mut link, "link", "/etc/passwd")
            .expect("link");
        builder.into_inner().expect("tar").finish().expect("gz");
    }
    out
}

fn content_md5(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Md5::digest(bytes))
}

#[derive(QueryableByName, Debug)]
struct JobRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    payload: String,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    later: bool,
}

#[derive(QueryableByName, Debug)]
struct PublishRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    id: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    state: String,
}

/// The publishes of the tenant, newest first: (id, state).
fn publishes(conn: &mut PgConnection) -> Vec<(String, String)> {
    diesel::sql_query(
        "SELECT id::text AS id, state FROM org_acme.site_publishes \
         ORDER BY published_at DESC, id DESC",
    )
    .load::<PublishRow>(conn)
    .expect("publishes")
    .into_iter()
    .map(|row| (row.id, row.state))
    .collect()
}

fn cleanup_jobs(conn: &mut PgConnection) -> Vec<JobRow> {
    diesel::sql_query(
        "SELECT payload::text AS payload, run_after > now() + interval '4 minutes' AS later \
         FROM public.jobs WHERE kind = 'site_cleanup' ORDER BY created_at",
    )
    .load(conn)
    .expect("jobs")
}

async fn router_with(scratch_url: &str, edit: impl FnOnce(&mut AppConfig)) -> Router {
    let pool = TenantPool::new(scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(scratch_url);
    config.local_auth = true;
    edit(&mut config);
    app::router(app::state_with(config, pool, auth))
}

#[tokio::test]
async fn ci_publishes_a_site_against_live_stack() {
    let _scratch = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");
    let robin_id = add_user(&mut conn, org_id, "robin@example.test", OrgRole::Admin);
    let sam = add_user(&mut conn, org_id, "sam@example.test", OrgRole::Member);
    let ci = add_user(&mut conn, org_id, "ci@example.test", OrgRole::Member);

    let store_dir = tempfile::tempdir().expect("tempdir");
    let store_root = store_dir.path().join("pages");
    let root = store_root.clone();
    let router = router_with(&scratch_url, move |config| {
        config.pages.store = Some(StoreConfig::Disk { dir: root });
    })
    .await;
    let robin = login(&router, "robin@example.test").await;
    let sam_token = login(&router, "sam@example.test").await;
    let ci_token = login(&router, "ci@example.test").await;

    // --- a team, a member, a repository -------------------------------------
    let (status, team) = send(
        &router,
        Method::POST,
        "/api/teams",
        &robin,
        &[],
        None,
        Some(json!({"name": "Platform", "slug": "platform", "code_prefix": "PLATFORM"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    let team_id = team["id"].as_str().expect("team id").to_string();
    let board_id: Uuid = team["delivery_board_id"]
        .as_str()
        .expect("delivery board")
        .parse()
        .expect("uuid");
    let (status, body) = send(
        &router,
        Method::POST,
        &format!("/api/teams/{team_id}/members"),
        &robin,
        &[],
        None,
        Some(json!({"user_id": sam.to_string()})),
    )
    .await;
    assert!(status.is_success(), "add member: {status} {body}");
    let (status, repo) = send(
        &router,
        Method::POST,
        "/api/repositories",
        &robin,
        &[],
        None,
        Some(json!({
            "slug": "payments", "forge": "other", "repo_full_name": "payments",
            "repo_url": "https://example.com/payments", "team": "platform"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{repo}");
    assert_eq!(repo["site"]["enabled"], false, "{repo}");
    assert_eq!(
        repo["site"]["base_path"],
        "/pages/acme/platform/repos/payments/"
    );
    assert_eq!(repo["site"]["published"], Value::Null);

    let good = site(&[
        ("index.html", b"<h1>v1</h1>"),
        ("guide/install.html", b"install"),
    ]);
    let md5_header = |bytes: &[u8]| vec![("content-md5", content_md5(bytes))];

    // --- rights: no `pages` ------------------------------------------------
    let (status, body) = send(
        &router,
        Method::PUT,
        SITE,
        &ci_token,
        &[],
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("pages"), "{body}");
    // A member of the owner team is not enough: membership does not imply it.
    let (status, body) = send(
        &router,
        Method::PUT,
        SITE,
        &sam_token,
        &[],
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a team member: {body}");

    // --- the grant, then the site switch ----------------------------------
    diesel::sql_query(format!(
        "INSERT INTO org_acme.board_member_capabilities (board_id, user_id, capability, granted_by) \
         VALUES ('{board_id}', '{ci}', 'pages', '{robin_id}')"
    ))
    .execute(&mut conn)
    .expect("grant pages");
    let (status, body) = send(
        &router,
        Method::PUT,
        SITE,
        &ci_token,
        &[],
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "SITE_OFF", "{body}");
    let (status, body) = send(
        &router,
        Method::PATCH,
        "/api/repositories/payments",
        &robin,
        &[],
        None,
        Some(json!({"site_enabled": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["site"]["enabled"], true, "{body}");

    // --- a wrong Content-MD5 changes nothing ------------------------------
    let (status, body) = send(
        &router,
        Method::PUT,
        SITE,
        &ci_token,
        &[("content-md5", content_md5(b"something else"))],
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "MD5_MISMATCH", "{body}");
    // --- a refused archive changes nothing --------------------------------
    let (status, body) = send(
        &router,
        Method::PUT,
        SITE,
        &ci_token,
        &[],
        Some(site_with_symlink()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "ARCHIVE_REFUSED", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("\"link\""),
        "{body}"
    );
    let history = publishes(&mut conn);
    assert!(history.is_empty(), "nothing was published: {history:?}");
    assert!(
        !store_root.exists() || std::fs::read_dir(&store_root).unwrap().next().is_none(),
        "nothing was stored"
    );

    // --- an agent key of ci publishes; the key is recorded -----------------
    let (status, key) = send(
        &router,
        Method::POST,
        "/api/me/agent-keys",
        &ci_token,
        &[],
        None,
        Some(json!({"name": "ci"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    let agent_key = key["key"].as_str().expect("key").to_string();
    let agent_key_id = key["id"].as_str().expect("key id").to_string();
    let (status, first) = send(
        &router,
        Method::PUT,
        SITE,
        &agent_key,
        &md5_header(&good),
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let expected_md5: String = Md5::digest(&good)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(first["md5"], expected_md5.as_str());
    assert_eq!(first["repository_slug"], "payments");
    assert_eq!(first["base_path"], "/pages/acme/platform/repos/payments/");
    assert_eq!(first["principal_id"], ci.to_string());
    assert_eq!(first["agent_key_id"], agent_key_id.as_str());
    assert_eq!(first["size"], good.len() as i64);
    assert_eq!(first["unpacked_size"], 11 + 7);
    assert_eq!(first["files"], 2);
    assert_eq!(first["state"], "live");
    let first_id = first["publish_id"].as_str().expect("id").to_string();

    // --- an organization admin publishes; it replaces the live one --------
    let second_site = site(&[("index.html", b"<h1>v2</h1>")]);
    let (status, second) = send(
        &router,
        Method::PUT,
        SITE,
        &robin,
        &[],
        Some(second_site),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let second_id = second["publish_id"].as_str().expect("id").to_string();
    let (_, repo) = send(
        &router,
        Method::GET,
        "/api/repositories/payments",
        &robin,
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(
        repo["site"]["published"]["publish_id"],
        second_id.as_str(),
        "{repo}"
    );
    assert_eq!(repo["site"]["needs_republish"], false, "{repo}");
    assert_eq!(
        publishes(&mut conn),
        vec![
            (second_id.clone(), "live".to_string()),
            (first_id.clone(), "replaced".to_string()),
        ]
    );
    let jobs = cleanup_jobs(&mut conn);
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert!(jobs[0].payload.contains(&first_id), "{jobs:?}");
    assert!(
        jobs[0].later,
        "the replaced files wait for the grace time: {jobs:?}"
    );
    let publishes: i64 = diesel::sql_query(
        "SELECT count(*) AS n FROM org_acme.activity_log WHERE action = 'publish'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(publishes, 2);

    // --- a move to another team: publish again ------------------------------
    let (status, other) = send(
        &router,
        Method::POST,
        "/api/teams",
        &robin,
        &[],
        None,
        Some(json!({"name": "Web", "slug": "web", "code_prefix": "WEB"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{other}");
    let (status, moved) = send(
        &router,
        Method::PATCH,
        "/api/repositories/payments",
        &robin,
        &[],
        None,
        Some(json!({"team": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        moved["site"]["base_path"],
        "/pages/acme/web/repos/payments/"
    );
    assert_eq!(moved["site"]["needs_republish"], true, "{moved}");

    // --- unpublish ----------------------------------------------------------
    let (status, body) = send(&router, Method::DELETE, SITE, &robin, &[], None, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, repo) = send(
        &router,
        Method::GET,
        "/api/repositories/payments",
        &robin,
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(repo["site"]["published"], Value::Null, "{repo}");
    let jobs = cleanup_jobs(&mut conn);
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert!(
        jobs[1].payload.contains(&second_id) && !jobs[1].later,
        "{jobs:?}"
    );
    let (status, body) = send(&router, Method::DELETE, SITE, &robin, &[], None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a second unpublish: {body}");

    // --- no site store: 501 --------------------------------------------------
    let no_store = router_with(&scratch_url, |_| {}).await;
    let robin_no_store = login(&no_store, "robin@example.test").await;
    let (status, body) = send(
        &no_store,
        Method::PUT,
        SITE,
        &robin_no_store,
        &[],
        Some(good.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert_eq!(body["error"]["code"], "NO_SITE_STORE", "{body}");

    // --- a body over the limit: 413, before the archive is read ---------------
    let small_dir = tempfile::tempdir().expect("tempdir");
    let small_root = small_dir.path().to_path_buf();
    let small = router_with(&scratch_url, move |config| {
        config.pages.store = Some(StoreConfig::Disk { dir: small_root });
        config.pages.max_size = 64;
    })
    .await;
    let robin_small = login(&small, "robin@example.test").await;
    let (status, body) = send(
        &small,
        Method::PUT,
        SITE,
        &robin_small,
        &[],
        Some(vec![0u8; 1000]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("64 bytes"),
        "{body}"
    );
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}
