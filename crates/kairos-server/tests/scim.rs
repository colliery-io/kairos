//! Integration test for the KAIROS-T-0025 SCIM 2.0 provisioning surface
//! (contract per KAIROS-A-0016): token management (/api/scim-tokens), the
//! RFC 7643 discovery documents, the full IdP-simulated user lifecycle
//! (provision → filter → PATCH → deprovision → 403), group sync (role
//! mapping + team mapping, LAST_ADMIN guard), malformed payloads → RFC
//! 7644 §3.12 error envelopes, and activity logging.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. Owns the uniquely named scratch
//! database `kairos_scim_t0025_test` (shared-services discipline).
//!
//! Cast:
//! - `svc`   — org admin of `acme` (seeded directly), issues SCIM tokens,
//! - `alice` — JIT-provisioned (logged in once), NOT a member; provisioned
//!   into acme by SCIM via the EMAIL-FALLBACK join, then deprovisioned
//!   (the 403 MEMBERSHIP_REQUIRED proof),
//! - `bob`   — org member used for the last-admin swap in group sync,
//! - `dave`  — never logs in: exists only through SCIM (created user row).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
use kairos_db::schema::users;
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_scim_t0025_test";

const ERROR_URN: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const LIST_URN: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const PATCH_URN: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

/// Every OIDC-authed tenant request resolves the tenant via X-Tenant.
const TENANT_HEADERS: &[(&str, &str)] = &[("x-tenant", "acme")];

/// One tenant-scoped /api request (OIDC bearer + X-Tenant).
async fn api(
    router: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    request(router, method, uri, Some(token), TENANT_HEADERS, body).await
}

/// One SCIM request: bearer only — deliberately NO X-Tenant / Host tenancy
/// (the token itself scopes the tenant, per A-0016).
async fn scim(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        // SCIM media type on purpose: axum's Json extractor would reject
        // it; the handlers must parse bodies manually.
        Some(json) => builder
            .header("content-type", "application/scim+json")
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
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("non-JSON body: {e}"))
    };
    (status, body)
}

/// Assert an RFC 7644 §3.12 error envelope with this status (+ scimType).
fn assert_scim_error(status: StatusCode, body: &Value, want: StatusCode, scim_type: Option<&str>) {
    assert_eq!(status, want, "{body}");
    assert_eq!(body["schemas"][0], ERROR_URN, "{body}");
    assert_eq!(body["status"], want.as_u16().to_string(), "{body}");
    match scim_type {
        Some(t) => assert_eq!(body["scimType"], t, "{body}"),
        None => assert!(body.get("scimType").is_none(), "{body}"),
    }
}

/// `(public.users.id, external_id)` by email.
fn user_row(conn: &mut PgConnection, email: &str) -> (Uuid, String) {
    users::table
        .filter(users::email.eq(email))
        .select((users::id, users::external_id))
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The member's role in acme, if any.
fn acme_role(conn: &mut PgConnection, org_id: Uuid, user_id: Uuid) -> Option<String> {
    use kairos_db::schema::organization_members::dsl;
    dsl::organization_members
        .filter(dsl::organization_id.eq(org_id))
        .filter(dsl::user_id.eq(user_id))
        .select(dsl::role)
        .first::<String>(conn)
        .optional()
        .expect("membership query")
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// Count acme activity rows matching an entity type and details pattern.
fn activity_count(conn: &mut PgConnection, entity_type: &str, details_like: &str) -> i64 {
    sql_query(
        "SELECT count(*) FROM org_acme.activity_log \
         WHERE entity_type = $1 AND details LIKE $2",
    )
    .bind::<diesel::sql_types::Text, _>(entity_type)
    .bind::<diesel::sql_types::Text, _>(details_like)
    .get_result::<CountRow>(conn)
    .expect("activity query")
    .count
}

#[tokio::test]
async fn scim_provisioning_against_live_stack() {
    // --- scratch database + tenant -----------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = {
        use kairos_db::schema::organizations::dsl;
        dsl::organizations
            .filter(dsl::slug.eq("acme"))
            .select(dsl::id)
            .first(&mut conn)
            .expect("acme org row")
    };

    // --- live tokens + router ----------------------------------------------
    let http = reqwest::Client::new();
    let alice = user_token(&http, "alice").await;
    let bob = user_token(&http, "bob").await;
    let svc = user_token(&http, "svc").await;

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

    // JIT-provision alice/bob/svc (any authed request creates the user row;
    // /api/whoami 403s on missing membership but auth ran first).
    for token in [&alice, &bob, &svc] {
        let (status, _) = api(&router, Method::GET, "/api/whoami", token, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (alice_id, _alice_sub) = user_row(&mut conn, "alice@kairos.test");
    let (bob_id, _bob_sub) = user_row(&mut conn, "bob@kairos.test");
    let (svc_id, _svc_sub) = user_row(&mut conn, "svc@kairos.test");

    // Seed acme's memberships directly: svc admin, bob member (the
    // deployment-admin bootstrap path is T-0019's test).
    for (user_id, role) in [(svc_id, "admin"), (bob_id, "member")] {
        sql_query(format!(
            "INSERT INTO public.organization_members (organization_id, user_id, role) \
             VALUES ('{org_id}', '{user_id}', '{role}')"
        ))
        .execute(&mut conn)
        .expect("seeding membership");
    }

    // =======================================================================
    // Token management (/api/scim-tokens) + SCIM auth
    // =======================================================================
    // Non-member → tenant middleware rejects; member non-admin → 403.
    let (status, body) = api(&router, Method::GET, "/api/scim-tokens", &alice, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");
    let (status, body) = api(
        &router,
        Method::POST,
        "/api/scim-tokens",
        &bob,
        Some(json!({"name": "sneaky"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["error"]["details"]["required_capability"],
        "manage_scim_tokens"
    );

    // Org admin creates a token; the secret appears exactly once.
    let (status, body) = api(
        &router,
        Method::POST,
        "/api/scim-tokens",
        &svc,
        Some(json!({"name": "okta-prod"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let scim_token = body["token"].as_str().expect("token").to_string();
    let token_id = body["id"].as_str().expect("id").to_string();
    assert!(
        scim_token.starts_with("kairos_scim_acme_"),
        "token embeds the tenant discriminator: {scim_token}"
    );
    // Empty name → 422.
    let (status, body) = api(
        &router,
        Method::POST,
        "/api/scim-tokens",
        &svc,
        Some(json!({"name": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Listing never exposes secrets or hashes; creation was activity-logged.
    let (status, body) = api(&router, Method::GET, "/api/scim-tokens", &svc, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "okta-prod");
    assert!(body["items"][0]["revoked_at"].is_null());
    assert!(body["items"][0].get("token").is_none(), "{body}");
    assert!(body["items"][0].get("token_hash").is_none(), "{body}");
    assert_eq!(
        activity_count(&mut conn, "scim_token", "scim_token:okta-prod"),
        1
    );

    // SCIM auth: missing/garbage/foreign tokens → uniform SCIM 401.
    for bad in [
        None,
        Some("garbage"),
        Some("kairos_scim_acme_deadbeef"),
        Some("kairos_scim_ghost_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
    ] {
        let (status, body) = scim(&router, Method::GET, "/scim/v2/Users", bad, None).await;
        assert_scim_error(status, &body, StatusCode::UNAUTHORIZED, None);
    }
    // The OIDC-side error envelope never leaks onto /scim/v2 — and the
    // valid token works with NO tenant headers at all.
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["schemas"][0], LIST_URN);
    assert_eq!(body["totalResults"], 2, "svc + bob are members: {body}");

    // =======================================================================
    // Discovery documents (RFC 7643)
    // =======================================================================
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/ServiceProviderConfig",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["patch"]["supported"], true);
    assert_eq!(body["bulk"]["supported"], false);
    assert_eq!(body["filter"]["supported"], true);
    assert_eq!(body["authenticationSchemes"][0]["type"], "oauthbearertoken");

    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Schemas",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 2);
    assert_eq!(
        body["Resources"][0]["id"],
        "urn:ietf:params:scim:core:2.0:User"
    );
    assert_eq!(
        body["Resources"][1]["id"],
        "urn:ietf:params:scim:core:2.0:Group"
    );

    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/ResourceTypes",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["Resources"][0]["endpoint"], "/Users");
    assert_eq!(body["Resources"][1]["endpoint"], "/Groups");

    // =======================================================================
    // User lifecycle
    // =======================================================================
    // Provision dave (never logged in): user row + membership created.
    let (status, dave) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({
            "schemas": ["urn:ietf:params:scim:core:2.0:User"],
            "userName": "dave-oidc-sub",
            "displayName": "Dave Lister",
            "emails": [{"value": "dave@kairos.test", "primary": true}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{dave}");
    let dave_id = dave["id"].as_str().expect("id").to_string();
    assert_eq!(dave["userName"], "dave-oidc-sub");
    assert_eq!(dave["externalId"], "dave-oidc-sub");
    assert_eq!(dave["active"], true);
    assert_eq!(dave["meta"]["resourceType"], "User");
    let (dave_uuid, dave_sub) = user_row(&mut conn, "dave@kairos.test");
    assert_eq!(dave_uuid.to_string(), dave_id, "SCIM id = public.users.id");
    assert_eq!(dave_sub, "dave-oidc-sub", "external_id = userName");
    assert_eq!(
        acme_role(&mut conn, org_id, dave_uuid).as_deref(),
        Some("member")
    );
    assert_eq!(
        activity_count(&mut conn, "membership", "%member:dave@kairos.test%"),
        1
    );

    // Duplicate provision → 409 uniqueness.
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({"userName": "dave-oidc-sub", "emails": [{"value": "dave@kairos.test"}]})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::CONFLICT, Some("uniqueness"));

    // Email-fallback join: alice logged in (JIT row exists) but the IdP
    // sends a NON-sub userName — SCIM must link the existing row by email
    // and RETAIN her real OIDC sub as external_id.
    let (status, alice_resource) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({
            "userName": "alice.fromidp",
            "emails": [{"value": "alice@kairos.test"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{alice_resource}");
    assert_eq!(
        alice_resource["id"],
        alice_id.to_string(),
        "linked, not duplicated"
    );
    let (alice_relinked, alice_sub_after) = user_row(&mut conn, "alice@kairos.test");
    assert_eq!(alice_relinked, alice_id);
    assert_eq!(
        alice_resource["userName"], alice_sub_after,
        "userName is served from the retained OIDC sub"
    );

    // Malformed payloads → RFC-compliant errors.
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({"emails": [{"value": "x@y.z"}]})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("invalidValue"));
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({"userName": "no-email-derivable"})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("invalidValue"));
    // Truly invalid JSON body.
    let request = Request::builder()
        .method(Method::POST)
        .uri("/scim/v2/Users")
        .header("authorization", format!("Bearer {scim_token}"))
        .header("content-type", "application/scim+json")
        .body(Body::from("{not json"))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("scim error json");
    assert_eq!(body["scimType"], "invalidSyntax", "{body}");

    // GET by id; unknown/non-UUID ids → 404 SCIM envelope.
    let (status, body) = scim(
        &router,
        Method::GET,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], dave_id);
    let (status, body) = scim(
        &router,
        Method::GET,
        &format!("/scim/v2/Users/{}", Uuid::new_v4()),
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::NOT_FOUND, None);
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users/not-a-uuid",
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::NOT_FOUND, None);

    // Filters: the eq subset works, everything else is invalidFilter.
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users?filter=userName%20eq%20%22dave-oidc-sub%22",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 1, "{body}");
    assert_eq!(body["Resources"][0]["id"], dave_id);
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users?filter=externalId%20eq%20%22no-such-sub%22",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 0);
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users?filter=userName%20co%20%22dave%22",
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(
        status,
        &body,
        StatusCode::BAD_REQUEST,
        Some("invalidFilter"),
    );

    // startIndex/count pagination (4 members now: alice, bob, dave, svc).
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users?startIndex=2&count=2",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 4);
    assert_eq!(body["startIndex"], 2);
    assert_eq!(body["itemsPerPage"], 2);
    assert_eq!(
        body["Resources"][0]["emails"][0]["value"], "bob@kairos.test",
        "email order: {body}"
    );

    // PATCH displayName (+ malformed PATCHes).
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "Replace", "path": "displayName", "value": "Dave L."}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["displayName"], "Dave L.");
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        Some(json!({"Operations": [{"op": "replace", "value": {"active": false}}]})),
    )
    .await;
    assert_scim_error(
        status,
        &body,
        StatusCode::BAD_REQUEST,
        Some("invalidSyntax"),
    );
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "replace", "path": "password", "value": "nope"}],
        })),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("invalidPath"));

    // PUT: profile replace works; changing the join key is mutability.
    let (status, body) = scim(
        &router,
        Method::PUT,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        Some(json!({
            "userName": "dave-oidc-sub",
            "displayName": "David Lister",
            "emails": [{"value": "dave@kairos.test"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["displayName"], "David Lister");
    let (status, body) = scim(
        &router,
        Method::PUT,
        &format!("/scim/v2/Users/{dave_id}"),
        Some(&scim_token),
        Some(json!({"userName": "different-sub", "emails": [{"value": "dave@kairos.test"}]})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("mutability"));

    // =======================================================================
    // Deprovision semantics: membership loss beats a still-valid OIDC token
    // =======================================================================
    // Alice's OIDC token works right now (she was SCIM-provisioned above).
    let (status, body) = api(&router, Method::GET, "/api/whoami", &alice, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["organization"]["role"], "member");

    // Entra-style PATCH: active as the string "False".
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Users/{alice_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "Replace", "value": {"active": "False"}}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["active"], false);
    assert_eq!(
        acme_role(&mut conn, org_id, alice_id),
        None,
        "membership revoked"
    );
    assert_eq!(
        activity_count(&mut conn, "membership", "%member_remove:alice@kairos.test%"),
        1
    );

    // The SAME (still cryptographically valid) OIDC token now gets 403 at
    // the tenant middleware — the A-0010/A-0016 deprovision contract.
    let (status, body) = api(&router, Method::GET, "/api/whoami", &alice, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");

    // Deprovisioned → GET 404; the users row is RETAINED for audit.
    let (status, body) = scim(
        &router,
        Method::GET,
        &format!("/scim/v2/Users/{alice_id}"),
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::NOT_FOUND, None);
    let (still_there, _) = user_row(&mut conn, "alice@kairos.test");
    assert_eq!(
        still_there, alice_id,
        "users row retained after deprovision"
    );

    // DELETE = the same revocation path (re-provision dave's colleague:
    // re-POST alice, then DELETE her).
    let (status, _) = scim(
        &router,
        Method::POST,
        "/scim/v2/Users",
        Some(&scim_token),
        Some(json!({"userName": "ignored", "emails": [{"value": "alice@kairos.test"}]})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "re-provision after deactivation"
    );
    let (status, body) = scim(
        &router,
        Method::DELETE,
        &format!("/scim/v2/Users/{alice_id}"),
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(acme_role(&mut conn, org_id, alice_id), None);

    // =======================================================================
    // Groups: role mapping (kairos-admins) + team mapping (kairos-team-*)
    // =======================================================================
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Groups",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 1, "only kairos-admins so far: {body}");
    assert_eq!(body["Resources"][0]["displayName"], "kairos-admins");
    assert_eq!(body["Resources"][0]["id"], org_id.to_string());
    assert_eq!(
        body["Resources"][0]["members"][0]["value"],
        svc_id.to_string()
    );
    let admins_group = format!("/scim/v2/Groups/{org_id}");

    // Create a team-mapped group with dave as an initial member.
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Groups",
        Some(&scim_token),
        Some(json!({
            "displayName": "kairos-team-platform",
            "members": [{"value": dave_id}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let team_group_id = body["id"].as_str().expect("group id").to_string();
    assert_eq!(body["members"][0]["value"], dave_id);
    // The team + its delivery board exist (mirrors POST /api/teams).
    assert_eq!(
        activity_count(
            &mut conn,
            "team",
            "%team:platform delivery_board:platform-delivery%"
        ),
        1
    );
    assert_eq!(
        activity_count(&mut conn, "team_membership", "%team:platform member:%"),
        1
    );

    // Naming contract enforced; duplicates rejected.
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Groups",
        Some(&scim_token),
        Some(json!({"displayName": "Engineering"})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("invalidValue"));
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Groups",
        Some(&scim_token),
        Some(json!({"displayName": "kairos-admins"})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::CONFLICT, Some("uniqueness"));
    let (status, body) = scim(
        &router,
        Method::POST,
        "/scim/v2/Groups",
        Some(&scim_token),
        Some(json!({"displayName": "kairos-team-platform"})),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::CONFLICT, Some("uniqueness"));

    // Group filter.
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Groups?filter=displayName%20eq%20%22kairos-team-platform%22",
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totalResults"], 1);
    assert_eq!(body["Resources"][0]["id"], team_group_id);

    // Team membership sync: remove dave via the filtered path form, then
    // re-add via a members add op.
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "remove", "path": format!("members[value eq \"{dave_id}\"]")}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["members"].as_array().expect("members").len(), 0);
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "add", "path": "members", "value": [{"value": dave_id}]}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["members"][0]["value"], dave_id);
    // Unprovisioned members are rejected.
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "add", "path": "members", "value": [{"value": alice_id}]}],
        })),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("invalidValue"));
    // Renames are refused.
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "replace", "path": "displayName", "value": "kairos-team-renamed"}],
        })),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("mutability"));

    // Role mapping: promote bob via the admins group, demote svc, then the
    // LAST_ADMIN guard blocks demoting bob.
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &admins_group,
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "add", "path": "members", "value": [{"value": bob_id}]}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        acme_role(&mut conn, org_id, bob_id).as_deref(),
        Some("admin")
    );
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &admins_group,
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "remove", "path": format!("members[value eq \"{svc_id}\"]")}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        acme_role(&mut conn, org_id, svc_id).as_deref(),
        Some("member")
    );
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &admins_group,
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "remove", "path": format!("members[value eq \"{bob_id}\"]")}],
        })),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("mutability"));
    assert_eq!(
        acme_role(&mut conn, org_id, bob_id).as_deref(),
        Some("admin"),
        "LAST_ADMIN guard held"
    );
    // Deactivating the last admin through Users is equally guarded.
    let (status, body) = scim(
        &router,
        Method::PATCH,
        &format!("/scim/v2/Users/{bob_id}"),
        Some(&scim_token),
        Some(json!({
            "schemas": [PATCH_URN],
            "Operations": [{"op": "replace", "path": "active", "value": false}],
        })),
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("mutability"));
    // PUT replace of the admin set swaps admins atomically (svc back in,
    // bob out) — no LAST_ADMIN false positive.
    let (status, body) = scim(
        &router,
        Method::PUT,
        &admins_group,
        Some(&scim_token),
        Some(json!({
            "displayName": "kairos-admins",
            "members": [{"value": svc_id}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        acme_role(&mut conn, org_id, svc_id).as_deref(),
        Some("admin")
    );
    assert_eq!(
        acme_role(&mut conn, org_id, bob_id).as_deref(),
        Some("member")
    );
    assert!(activity_count(&mut conn, "membership", "%membership_role:%") >= 3);

    // The built-in admins group cannot be deleted; team groups can.
    let (status, body) = scim(
        &router,
        Method::DELETE,
        &admins_group,
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::BAD_REQUEST, Some("mutability"));
    let (status, body) = scim(
        &router,
        Method::DELETE,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = scim(
        &router,
        Method::GET,
        &format!("/scim/v2/Groups/{team_group_id}"),
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::NOT_FOUND, None);

    // =======================================================================
    // Token revocation closes the loop
    // =======================================================================
    let (status, body) = api(
        &router,
        Method::DELETE,
        &format!("/api/scim-tokens/{token_id}"),
        &svc,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revoked"], true);
    // Revoked token → 401 on the SCIM surface.
    let (status, body) = scim(
        &router,
        Method::GET,
        "/scim/v2/Users",
        Some(&scim_token),
        None,
    )
    .await;
    assert_scim_error(status, &body, StatusCode::UNAUTHORIZED, None);
    // Double revoke → 409; the row is retained (audit) and listed as revoked.
    let (status, body) = api(
        &router,
        Method::DELETE,
        &format!("/api/scim-tokens/{token_id}"),
        &svc,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = api(&router, Method::GET, "/api/scim-tokens", &svc, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body["items"][0]["revoked_at"].is_null(), "{body}");
    assert_eq!(
        activity_count(&mut conn, "scim_token", "scim_token_revoke:okta-prod"),
        1
    );

    // --- teardown -----------------------------------------------------------
    drop(conn);
    drop(pool);
    drop(router);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
