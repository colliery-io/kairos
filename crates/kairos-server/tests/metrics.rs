//! KAIROS-T-0049 (A-0013): the `/metrics` Prometheus surface and the
//! `/readyz` probe, exercised against the real production router.
//!
//! Coverage:
//! - `/readyz` returns 503 (pending migrations) on a fresh scratch DB and
//!   flips to 200 (ready) once the public migrations are applied — the
//!   A-0013 DB-connectivity + pending-migration semantics.
//! - `/metrics` scrapes: the body parses as Prometheus text, the expected
//!   metric families are present (HTTP duration histogram, pool-connection
//!   gauges, per-tenant counter), and the counters ADVANCE across a second
//!   batch of requests.
//!
//! Uses the same in-process oneshot harness as the rest of the suite; the
//! metrics registry is per-`AppState`, so counts are isolated to this
//! router (see `kairos_server::metrics` module docs).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use tower::ServiceExt;
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

const SCRATCH_DB: &str = "kairos_metrics_t0049_test";
const TENANT_HEADERS: &[(&str, &str)] = &[("x-tenant", "acme")];

/// A raw in-process GET returning `(status, content-type, body text)` — for
/// the plain-text `/metrics` and `/readyz` responses (the `common::request`
/// helper assumes a JSON body). Local because `tests/common` is read-only.
async fn raw_request(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, String, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, content_type, String::from_utf8_lossy(&bytes).into())
}

/// Parse Prometheus text into `key -> value`, where `key` is the full
/// `name{labels}` token (same shape the soak scraper keys on). Comments and
/// unparseable lines are ignored.
fn parse_prometheus(body: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut in_labels = false;
        let mut split_at = None;
        for (i, c) in line.char_indices() {
            match c {
                '{' => in_labels = true,
                '}' => in_labels = false,
                c if c.is_whitespace() && !in_labels => {
                    split_at = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(split_at) = split_at else { continue };
        let (key, rest) = line.split_at(split_at);
        if let Some(value) = rest.split_whitespace().next()
            && let Ok(value) = value.parse::<f64>()
        {
            out.insert(key.to_string(), value);
        }
    }
    out
}

#[tokio::test]
async fn metrics_and_readyz_against_live_stack() {
    // --- fresh scratch database (NO migrations yet) --------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);

    let http = reqwest::Client::new();
    let alice = user_token(&http, "alice").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));

    // --- /readyz: NOT ready while public migrations are pending --------------
    let (status, _ct, body) = raw_request(&router, Method::GET, "/readyz", None, &[]).await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "fresh DB has pending migrations: {body}"
    );
    assert!(
        body.contains("pending"),
        "readyz should name the pending migrations: {body}"
    );

    // --- apply migrations + provision the tenant -----------------------------
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("run public migrations");

    // --- /readyz: ready once migrations are applied --------------------------
    let (status, _ct, body) = raw_request(&router, Method::GET, "/readyz", None, &[]).await;
    assert_eq!(status, StatusCode::OK, "ready after migrations: {body}");
    assert_eq!(body, "ready");

    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // First authed call JIT-provisions alice (403 until she is a member).
    let (status, _body) = request(
        &router,
        Method::GET,
        "/api/whoami",
        Some(&alice),
        TENANT_HEADERS,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let alice_id: Uuid = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("alice provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice_id,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("grant alice membership");

    // --- generate tenant-attributed traffic (whoami now 200) -----------------
    const FIRST_BATCH: usize = 3;
    for _ in 0..FIRST_BATCH {
        let (status, _body) = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&alice),
            TENANT_HEADERS,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // --- scrape #1: families present -----------------------------------------
    let (status, content_type, body) =
        raw_request(&router, Method::GET, "/metrics", None, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        content_type.starts_with("text/plain"),
        "Prometheus content-type: {content_type}"
    );
    let sample = parse_prometheus(&body);
    assert!(!sample.is_empty(), "scrape parsed to samples: {body}");

    // HTTP duration histogram family (labeled by route/method/status).
    assert!(
        body.contains("# TYPE http_request_duration_seconds histogram"),
        "histogram TYPE line present: {body}"
    );
    let whoami_count_key =
        "http_request_duration_seconds_count{method=\"GET\",route=\"/api/whoami\",status=\"200\"}";
    let count1 = *sample
        .get(whoami_count_key)
        .unwrap_or_else(|| panic!("missing {whoami_count_key} in:\n{body}"));
    assert_eq!(count1, FIRST_BATCH as f64, "3 successful whoami calls");

    // Connection-pool gauges: both the async bb8 pool and the blocking r2d2
    // bridge (the soak scraper's `is_pool_metric` keys on name substrings).
    for gauge in [
        "kairos_db_pool_connections{pool=\"async\",state=\"total\"}",
        "kairos_db_pool_connections{pool=\"async\",state=\"idle\"}",
        "kairos_db_pool_connections{pool=\"blocking\",state=\"total\"}",
        "kairos_db_pool_connections{pool=\"blocking\",state=\"idle\"}",
    ] {
        assert!(
            sample.contains_key(gauge),
            "missing pool gauge {gauge}:\n{body}"
        );
    }

    // Per-tenant request counter.
    let tenant_key = "http_requests_by_tenant_total{tenant=\"acme\"}";
    let tenant1 = *sample
        .get(tenant_key)
        .unwrap_or_else(|| panic!("missing {tenant_key} in:\n{body}"));
    assert_eq!(tenant1, FIRST_BATCH as f64, "3 tenant-attributed calls");

    // --- second batch → counters must ADVANCE --------------------------------
    const SECOND_BATCH: usize = 2;
    for _ in 0..SECOND_BATCH {
        let (status, _body) = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&alice),
            TENANT_HEADERS,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (_status, _ct, body2) = raw_request(&router, Method::GET, "/metrics", None, &[]).await;
    let sample2 = parse_prometheus(&body2);
    let count2 = *sample2.get(whoami_count_key).expect("whoami count present");
    let tenant2 = *sample2.get(tenant_key).expect("tenant count present");
    assert_eq!(
        count2,
        (FIRST_BATCH + SECOND_BATCH) as f64,
        "HTTP request count advanced across requests"
    );
    assert_eq!(
        tenant2,
        (FIRST_BATCH + SECOND_BATCH) as f64,
        "per-tenant counter advanced across requests"
    );

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
