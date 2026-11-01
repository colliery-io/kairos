//! KAIROS-T-0745 (KAIROS-A-0080): the server serves the CLI tarballs of
//! its version out of `KAIROS_CLI_DIR`, open like `/version`, and
//! `/version` names them.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, with_database};
use kairos_db::{TenantPool, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_cli_files_t0745_test";

async fn get(router: &Router, uri: &str) -> (StatusCode, Option<String>, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or_default().to_string());
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    (status, content_type, bytes)
}

#[tokio::test]
async fn the_server_serves_the_cli_of_its_version() {
    let mut admin = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("run public migrations");

    let version = env!("CARGO_PKG_VERSION");
    let file = format!("kairos-{version}-x86_64-unknown-linux-gnu.tar.gz");
    let dir = tempfile::tempdir().expect("cli dir");
    std::fs::write(dir.path().join(&file), b"the tarball").expect("tarball");
    std::fs::write(
        dir.path().join(format!("{file}.sha256")),
        format!("abc  {file}\n"),
    )
    .expect("checksum");
    std::fs::write(dir.path().join("other.txt"), b"not served").expect("other");

    let mut config = base_config(&scratch_url);
    config.cli_dir = Some(dir.path().to_path_buf());
    // No public URL: the links are paths, which the CLI resolves.
    config.public_url = None;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let router = app::router(app::state_with(config, pool, auth));

    // No login: a first install has none.
    let (status, content_type, body) = get(&router, &format!("/cli/{file}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/gzip"));
    assert_eq!(body, b"the tarball");

    let (status, _, body) = get(&router, &format!("/cli/{file}.sha256")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, format!("abc  {file}\n").as_bytes());

    // A file in the folder that is not a tarball of this version, and a
    // tarball of this version that is not there.
    for uri in [
        "/cli/other.txt".to_string(),
        "/cli/..%2Fetc%2Fpasswd".to_string(),
        format!("/cli/kairos-{version}-aarch64-apple-darwin.tar.gz"),
    ] {
        let (status, _, body) = get(&router, &uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("GET /version"), "{uri}: {text}");
    }

    // `/version` names the served tarball by its path, and the published
    // release for a target with none.
    let (status, _, body) = get(&router, "/version").await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let link = |target: &str| {
        body["cli"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["target"] == target)
            .unwrap()
            .clone()
    };
    let linux = link("x86_64-unknown-linux-gnu");
    assert_eq!(linux["url"], format!("/cli/{file}"));
    assert_eq!(linux["sha256_url"], format!("/cli/{file}.sha256"));
    assert!(
        link("aarch64-apple-darwin")["url"]
            .as_str()
            .unwrap()
            .starts_with("https://github.com/")
    );

    drop(conn);
    drop_scratch_db(&mut admin, SCRATCH_DB);
}
