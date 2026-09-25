//! KAIROS-T-0024: the typed error-mapping roundtrip for
//! `kairos_client::KairosClient` — every S-0005 rejection family is
//! provoked against the live production router and asserted as the RIGHT
//! `kairos_client::Error` variant with its payload extracted, plus the
//! `TokenProvider` seam the CLI's refreshing credentials plug into.
//!
//! | provoked                              | expected variant                          |
//! |---------------------------------------|-------------------------------------------|
//! | garbage bearer token                  | `Unauthorized` (401 UNAUTHORIZED)          |
//! | authenticated non-member              | `Forbidden` (MEMBERSHIP_REQUIRED, no cap)  |
//! | member without the board capability   | `Forbidden { capability, details.board_id }` |
//! | unknown short code                    | `NotFound` (404 NOT_FOUND)                 |
//! | stale-version PATCH                   | `Conflict { current }` (full DTO)          |
//! | unreachable transition target         | `InvalidTransition { allowed_targets }`    |
//! | malformed board UUID on create        | `Validation { status: 422 }`               |
//! | empty search request                  | `Validation { status: 400, details.fields }` |
//! | demoting the last org admin           | `Other { 422, LAST_ADMIN }`                |
//!
//! Runs against the LIVE compose stack; owns the uniquely named scratch
//! database `kairos_client_roundtrip_m2_test` (shared-services
//! discipline).

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{CreateTaskRequest, Pagination, UpdateContentRequest};
use kairos_client::types_org::{CreateBoardRequest, UpdateOrgMemberRequest};
use kairos_client::types_search::SearchRequest;
use kairos_client::{Error, KairosClient, TokenProvider};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_client_roundtrip_m2_test";

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// A counting token provider: proves the client draws a token from the
/// provider on EVERY request (the seam the CLI's refresh logic needs).
struct CountingProvider {
    token: String,
    calls: AtomicUsize,
}

impl TokenProvider for CountingProvider {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let token = self.token.clone();
        Box::pin(async move { Ok(token) })
    }
}

/// A provider with no credentials: the client surfaces `Error::Token`
/// without ever sending a request.
struct NoCredentials;

impl TokenProvider for NoCredentials {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        Box::pin(async move { Err(Error::Token("no cached credentials".into())) })
    }
}

#[tokio::test]
async fn typed_error_mapping_roundtrip() {
    // --- scratch database + tenant ------------------------------------------
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

    // --- live server + clients -----------------------------------------------
    let http = reqwest::Client::new();
    let bob_token = user_token(&http, "bob").await;
    let svc_token = user_token(&http, "svc").await;

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
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
    let server = spawn_server(router).await;
    let bob = server.client(&bob_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // --- 401 Unauthorized: garbage bearer token -------------------------------
    let garbage = server.client("garbage.token.here", "acme");
    let err = rejection(garbage.list_tasks(Pagination::default()).await);
    match &err {
        Error::Unauthorized { code, .. } => assert_eq!(code, "UNAUTHORIZED"),
        other => panic!("expected Unauthorized, got {other}"),
    }
    assert_eq!(err.status(), Some(401));

    // --- 403 Forbidden WITHOUT a capability: authenticated non-member ---------
    // (Also JIT-provisions bob and svc.)
    for client in [&bob, &svc] {
        let err = rejection(client.list_tasks(Pagination::default()).await);
        match &err {
            Error::Forbidden {
                code, capability, ..
            } => {
                assert_eq!(code, "MEMBERSHIP_REQUIRED");
                assert_eq!(
                    *capability, None,
                    "membership gates carry no required_capability"
                );
            }
            other => panic!("expected Forbidden, got {other}"),
        }
    }
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user_id, role) in [(bob_id, OrgRole::Member), (svc_id, OrgRole::Admin)] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }

    // Fixture: svc (org admin, implicit capabilities) creates a delivery
    // board through the API; its detail carries columns + the transition
    // graph, which drives the InvalidTransition case below.
    let board = svc
        .create_board(&CreateBoardRequest {
            name: "Delivery".into(),
            slug: "delivery".into(),
            board_level: "delivery".into(),
            team_id: None,
        })
        .await
        .expect("creating the delivery board");
    let board_id = board.board.id.clone();
    let first_column = board.columns[0].id.clone();
    let reachable: Vec<&str> = board
        .transitions
        .iter()
        .filter(|t| t.from_column_id == first_column)
        .map(|t| t.to_column_id.as_str())
        .collect();
    let unreachable_column = board
        .columns
        .iter()
        .map(|c| c.id.as_str())
        .find(|id| *id != first_column && !reachable.contains(id))
        .expect("default delivery board is not a complete graph");

    // --- 403 Forbidden WITH the capability payload -----------------------------
    let err = rejection(
        bob.create_task(&CreateTaskRequest {
            board_id: Some(board_id.clone()),
            repository: None,
            column_id: None,
            title: "denied".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
        })
        .await,
    );
    match &err {
        Error::Forbidden {
            code,
            capability,
            details,
            ..
        } => {
            assert_eq!(code, "FORBIDDEN");
            assert_eq!(capability.as_deref(), Some("manage_tasks"));
            assert_eq!(details["board_id"], board_id);
        }
        other => panic!("expected Forbidden with capability, got {other}"),
    }

    // --- 404 NotFound -----------------------------------------------------------
    let err = rejection(svc.get_task("ACME-T-9999").await);
    match &err {
        Error::NotFound { code, .. } => assert_eq!(code, "NOT_FOUND"),
        other => panic!("expected NotFound, got {other}"),
    }
    assert_eq!(err.status(), Some(404));

    // --- 409 Conflict { current }: stale optimistic-concurrency PATCH -----------
    let task = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(board_id.clone()),
            repository: None,
            column_id: None,
            title: "Roundtrip task".into(),
            content: "v1".into(),
            task_type: None,
            work_class: None,
            team_id: None,
        })
        .await
        .expect("creating the fixture task");
    let updated = svc
        .update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "v2".into(),
                version: 1,
            },
        )
        .await
        .expect("advancing to version 2");
    assert_eq!(updated.version, 2);
    let err = rejection(
        svc.update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "stale".into(),
                version: 1,
            },
        )
        .await,
    );
    match &err {
        Error::Conflict { code, current, .. } => {
            assert_eq!(code, "CONFLICT");
            assert_eq!(current["version"], 2);
            assert_eq!(
                current["short_code"], task.short_code,
                "details.current is the full entity DTO"
            );
            assert_eq!(current["content"], "v2");
        }
        other => panic!("expected Conflict, got {other}"),
    }
    assert_eq!(err.status(), Some(409));

    // --- 422 InvalidTransition { allowed_targets } -------------------------------
    let err = rejection(
        svc.transition_task(&task.short_code, unreachable_column)
            .await,
    );
    match &err {
        Error::InvalidTransition {
            allowed_targets, ..
        } => {
            let allowed = allowed_targets.as_array().expect("allowed_targets array");
            assert!(!allowed.is_empty(), "{allowed_targets}");
            assert!(
                reachable
                    .iter()
                    .all(|id| allowed.iter().any(|c| c["id"] == **id)),
                "allowed_targets carries the reachable columns: {allowed_targets}"
            );
        }
        other => panic!("expected InvalidTransition, got {other}"),
    }
    assert_eq!(err.status(), Some(422));
    assert_eq!(err.code(), Some("INVALID_TRANSITION"));

    // --- 422 Validation: malformed UUID reference ---------------------------------
    //
    // `column_id` rather than `board_id`: since KAIROS-T-0150 a board reference
    // is a slug OR a UUID, so an unparseable one is a 404 "no such board" rather
    // than a 422. `column_id` is still a UUID-only field, which is what this
    // assertion needs — it is about the Error::Validation mapping, not about
    // boards.
    let err = rejection(
        svc.create_task(&CreateTaskRequest {
            board_id: None,
            repository: None,
            column_id: Some("not-a-uuid".into()),
            title: "x".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
        })
        .await,
    );
    match &err {
        Error::Validation { status, .. } => assert_eq!(*status, 422),
        other => panic!("expected Validation, got {other}"),
    }

    // --- 400 Validation with field detail: empty search --------------------------
    let err = rejection(svc.search(&SearchRequest::default()).await);
    match &err {
        Error::Validation {
            status, details, ..
        } => {
            assert_eq!(*status, 400);
            assert_eq!(
                details["fields"],
                serde_json::json!(["q", "filter", "traverse"])
            );
        }
        other => panic!("expected 400 Validation, got {other}"),
    }

    // --- Other { status, code }: LAST_ADMIN (a 422 outside the typed pairs) ------
    let err = rejection(
        svc.update_org_member(
            &svc_id.to_string(),
            &UpdateOrgMemberRequest {
                role: "member".into(),
            },
        )
        .await,
    );
    match &err {
        Error::Other {
            status,
            code,
            details,
            ..
        } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "LAST_ADMIN");
            assert!(details.is_object(), "{details}");
        }
        other => panic!("expected Other, got {other}"),
    }

    // --- TokenProvider seam: drawn per request; failures surface as Token --------
    let counting = Arc::new(CountingProvider {
        token: svc_token.clone(),
        calls: AtomicUsize::new(0),
    });
    let counted_client = KairosClient::new(&server.base_url, counting.clone()).with_tenant("acme");
    counted_client
        .list_tasks(Pagination::default())
        .await
        .expect("first counted call");
    counted_client
        .get_task(&task.short_code)
        .await
        .expect("second counted call");
    assert_eq!(
        counting.calls.load(Ordering::SeqCst),
        2,
        "the client asks the provider for a token on every request"
    );

    let no_creds = KairosClient::new(&server.base_url, Arc::new(NoCredentials)).with_tenant("acme");
    let err = rejection(no_creds.list_tasks(Pagination::default()).await);
    match &err {
        Error::Token(message) => assert_eq!(message, "no cached credentials"),
        other => panic!("expected Token, got {other}"),
    }

    // The X-Tenant seam: the same token without tenant resolution fails
    // tenant resolution (multi-tenant config, no Host match) → NotFound
    // TENANT_NOT_FOUND, typed.
    let err = rejection(
        server
            .client_untenanted(&svc_token)
            .list_tasks(Pagination::default())
            .await,
    );
    match &err {
        Error::NotFound { code, .. } => assert_eq!(code, "TENANT_NOT_FOUND"),
        other => panic!("expected NotFound TENANT_NOT_FOUND, got {other}"),
    }

    // Decode-safety: a non-JSON body surfaces as a typed Decode error, not
    // a panic (/healthz returns plain text "ok").
    let result = svc
        .raw_request(reqwest::Method::GET, "/healthz", None)
        .await;
    assert!(
        matches!(result, Err(Error::Decode { .. })),
        "non-JSON bodies map to Error::Decode: {result:?}"
    );

    // --- teardown -----------------------------------------------------------------
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
