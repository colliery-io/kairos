//! KAIROS-T-0049 (A-0013): the `/metrics` Prometheus surface and the
//! `/readyz` probe, exercised against the real production router.
//!
//! Coverage:
//! - `/readyz` returns 503 (pending migrations) on a fresh scratch DB and
//!   flips to 200 (ready) once the public migrations are applied — the
//!   A-0013 DB-connectivity + pending-migration semantics.
//! - KAIROS-T-0631: with the default retention (`archive`, no target) the
//!   ready body adds a `warning:` line and the status stays 200; with a
//!   target the body is `ready`. `GET /api/org/retention` refuses a member
//!   and gives an admin the `no_target` status with the warning.
//! - `/metrics` scrapes: the body parses as Prometheus text, the expected
//!   metric families are present (HTTP duration histogram, pool-connection
//!   gauges, per-tenant counter), and the counters ADVANCE across a second
//!   batch of requests.
//! - KAIROS-T-0576: `kairos_jobs{kind,state}` counts the jobs of all the
//!   organizations and moves when a job changes state; with the default
//!   refresh time a second scrape keeps the cached value. `GET
//!   /api/admin/jobs` refuses a member and gives an admin the counts by
//!   kind and state and the last 20 failures, with each error cut to 500
//!   characters, of the organization and the deployment only.
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
use kairos_db::jobs::NewJob;
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
/// `name{labels}` token. Comments and
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
    // KAIROS-T-0631: the default retention removes nothing, and says so.
    let mut lines = body.lines();
    assert_eq!(lines.next(), Some("ready"), "{body}");
    let warning = lines.next().unwrap_or_default();
    assert!(
        warning.starts_with("warning: ") && warning.contains("KAIROS_ARCHIVE_TARGET"),
        "readyz names the missing archive target: {body}"
    );

    // With an archive target the body is `ready` and nothing more.
    let mut with_target = app::state_with(base_config(&scratch_url), pool.clone(), auth.clone());
    with_target.retention = Arc::new(kairos_core::retention::RetentionConfig {
        archive_target: kairos_core::retention::ArchiveTarget::parse("/var/archive"),
        ..Default::default()
    });
    let target_router = app::router(with_target);
    let (status, _ct, body) = raw_request(&target_router, Method::GET, "/readyz", None, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
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
    // bridge.
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

    // --- KAIROS-T-0631: GET /api/org/retention ------------------------------
    let (status, body) = request(
        &router,
        Method::GET,
        "/api/org/retention",
        Some(&alice),
        TENANT_HEADERS,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a member is refused: {body}");
    diesel::update(
        organization_members::table
            .filter(organization_members::organization_id.eq(org_id))
            .filter(organization_members::user_id.eq(alice_id)),
    )
    .set(organization_members::role.eq(OrgRole::Admin))
    .execute(&mut conn)
    .expect("make alice an admin");
    // The cache of the auth and tenant lookups (KAIROS-T-0609) keeps the
    // member role; a new router reads the admin role.
    let admin_router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let (status, body) = request(
        &admin_router,
        Method::GET,
        "/api/org/retention",
        Some(&alice),
        TENANT_HEADERS,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "archive");
    assert_eq!(body["status"], "no_target");
    assert!(body.get("archive_target").is_none(), "{body}");
    assert!(
        body["warning"]
            .as_str()
            .is_some_and(|w| w.contains("KAIROS_ARCHIVE_TARGET")),
        "{body}"
    );
    assert!(body.get("last_result").is_none(), "no pass ran: {body}");

    jobs_gauges_and_endpoint(&mut conn, &scratch_url, &pool, &auth, &router, &alice).await;

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}

/// The kind of the test jobs of KAIROS-T-0576: no handler runs it.
const JOB_KIND: &str = "t0576_probe";

fn probe_job(tenant: Option<&str>) -> NewJob {
    NewJob {
        tenant: tenant.map(str::to_string),
        ..NewJob::new(JOB_KIND)
    }
}

/// Lease the next probe job and fail it with `error`.
fn fail_next_probe(conn: &mut PgConnection, error: &str) {
    let instance = Uuid::new_v4();
    let job = kairos_db::jobs::take(conn, instance, &[JOB_KIND.to_string()], 60.0)
        .expect("take")
        .expect("a probe job waits");
    assert!(kairos_db::jobs::fail(conn, job.id, instance, error).expect("fail"));
}

/// KAIROS-T-0576: the job gauges of `/metrics` and `GET /api/admin/jobs`.
/// Alice is an admin of `acme` by now; `router` still has her member role
/// in its cache.
async fn jobs_gauges_and_endpoint(
    conn: &mut PgConnection,
    scratch_url: &str,
    pool: &TenantPool,
    auth: &Arc<Authenticator>,
    router: &Router,
    alice: &str,
) {
    // Two jobs of acme, one of the deployment, one of another organization.
    for tenant in [Some("acme"), Some("acme"), None, Some("other")] {
        kairos_db::jobs::enqueue(conn, &probe_job(tenant))
            .expect("enqueue")
            .expect("added");
    }
    let queued = format!("kairos_jobs{{kind=\"{JOB_KIND}\",state=\"queued\"}}");
    let failed = format!("kairos_jobs{{kind=\"{JOB_KIND}\",state=\"failed\"}}");

    // A state whose gauges are read at each scrape.
    let mut fresh = app::state_with(base_config(scratch_url), pool.clone(), auth.clone());
    fresh.metrics = Arc::new(kairos_server::metrics::Metrics::with_job_refresh(
        std::time::Duration::ZERO,
    ));
    let fresh = app::router(fresh);
    let (_, _, body) = raw_request(&fresh, Method::GET, "/metrics", None, &[]).await;
    assert!(body.contains("# TYPE kairos_jobs gauge"), "{body}");
    let sample = parse_prometheus(&body);
    assert_eq!(
        sample.get(&queued),
        Some(&4.0),
        "all the organizations: {body}"
    );
    assert_eq!(
        sample.get(&failed),
        Some(&0.0),
        "each state has a series: {body}"
    );

    // The default refresh time: a scrape reads the queue once.
    let cached = app::router(app::state_with(
        base_config(scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let (_, _, body) = raw_request(&cached, Method::GET, "/metrics", None, &[]).await;
    assert_eq!(parse_prometheus(&body).get(&queued), Some(&4.0), "{body}");

    fail_next_probe(conn, "The probe failed.");
    let (_, _, body) = raw_request(&fresh, Method::GET, "/metrics", None, &[]).await;
    let sample = parse_prometheus(&body);
    assert_eq!(sample.get(&queued), Some(&3.0), "the gauge moves: {body}");
    assert_eq!(sample.get(&failed), Some(&1.0), "the gauge moves: {body}");
    let (_, _, body) = raw_request(&cached, Method::GET, "/metrics", None, &[]).await;
    assert_eq!(
        parse_prometheus(&body).get(&queued),
        Some(&4.0),
        "a scrape inside the refresh time keeps the last read: {body}"
    );
    // The counters of the worker are in the family list, at no series yet.
    assert!(
        body.contains("# TYPE kairos_job_failures_total counter"),
        "{body}"
    );
    assert!(
        body.contains("# TYPE kairos_job_lease_kills_total counter"),
        "{body}"
    );

    // --- GET /api/admin/jobs ---------------------------------------------
    // `router` has the member role of alice in its cache.
    let (status, body) = request(
        router,
        Method::GET,
        "/api/admin/jobs",
        Some(alice),
        TENANT_HEADERS,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a member is refused: {body}");
    assert_eq!(body["error"]["details"]["required_role"], "admin", "{body}");

    // 22 more failures of acme, with a long error, and one of another
    // organization that the admin of acme does not see.
    let long_error = "e".repeat(600);
    for _ in 0..22 {
        kairos_db::jobs::enqueue(conn, &probe_job(Some("acme")))
            .expect("enqueue")
            .expect("added");
    }
    // Fail each queued probe job, the job of `other` too.
    let mut other_failed = false;
    loop {
        let instance = Uuid::new_v4();
        let Some(job) =
            kairos_db::jobs::take(conn, instance, &[JOB_KIND.to_string()], 60.0).expect("take")
        else {
            break;
        };
        let error = if job.tenant.as_deref() == Some("other") {
            other_failed = true;
            "The secret of other.".to_string()
        } else {
            long_error.clone()
        };
        assert!(kairos_db::jobs::fail(conn, job.id, instance, &error).expect("fail"));
    }
    assert!(other_failed);

    let admin_router = app::router(app::state_with(
        base_config(scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let (status, body) = request(
        &admin_router,
        Method::GET,
        "/api/admin/jobs",
        Some(alice),
        TENANT_HEADERS,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let kinds = body["kinds"].as_array().expect("kinds");
    let probe = kinds
        .iter()
        .find(|k| k["kind"] == JOB_KIND)
        .unwrap_or_else(|| panic!("no probe kind: {body}"));
    // acme: 2 + 22 jobs; the deployment: 1. The job of `other` is out.
    assert_eq!(probe["failed"], 25, "{body}");
    assert_eq!(probe["queued"], 0, "{body}");
    assert_eq!(probe["leased"], 0, "{body}");
    assert_eq!(probe["done"], 0, "{body}");
    let failures = body["failures"].as_array().expect("failures");
    assert_eq!(failures.len(), 20, "at most 20 failures: {body}");
    for failure in failures {
        assert_ne!(failure["tenant"], "other", "{body}");
        let error = failure["error"].as_str().expect("an error");
        assert!(!error.contains("other"), "{error}");
    }
    let error = failures[0]["error"].as_str().expect("an error");
    assert_eq!(
        error.chars().count(),
        501,
        "500 characters and the cut mark"
    );
    assert!(error.ends_with('…'), "{error}");
    assert!(failures[0]["finished_at"].is_string(), "{body}");
    assert_eq!(failures[0]["max_tries"], 3, "{body}");
}
