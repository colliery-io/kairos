//! The vector of a code search query (KAIROS-T-0360):
//! `POST /api/code-index/query-vector`.
//!
//! 1. A member who is not an admin gets the vector of the embedded model of
//!    the server: the dimension of the model, and the vector of the model.
//! 2. A model that the organization has no provider for is refused with
//!    `NO_QUERY_PROVIDER`, which names the model. A model that is not
//!    `<provider>/<model>/<dimension>`, and a text that is empty or too long,
//!    are refused with `VALIDATION` and the field.
//! 3. With the remote vector provider of the organization (a fake
//!    embeddings endpoint with a key), the server embeds the text there with
//!    the sealed key, in one request. A remote model of another name is
//!    refused; a remote model of another dimension fails with
//!    `QUERY_VECTOR_FAILED`.
//! 4. A request with no token is refused with 401.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_code_index_query_vector_t0360_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, error_code, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types_auth::Secret;
use kairos_client::types_code_index::{
    PutCodeIndexSettings, PutSummaryProvider, PutVectorProvider,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_embed::{DeterministicProvider, EmbeddingProvider};
use kairos_server::app;
use kairos_server::embedding::EmbeddingService;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;

const SCRATCH_DB: &str = "kairos_code_index_query_vector_t0360_test";
const KEY: &str = "embed_t0360_API_SECRET_42";
const EMBEDDED: &str = "deterministic/sha256-384/384";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

/// The code and the `details.model` of a refusal that is not `VALIDATION`.
fn refusal(err: &Error) -> (u16, String, String) {
    match err {
        Error::Other {
            status,
            code,
            details,
            ..
        } => (
            *status,
            code.clone(),
            details["model"].as_str().unwrap_or_default().to_string(),
        ),
        other => panic!("expected an enveloped refusal, got {other:?}"),
    }
}

fn field_of(err: &Error) -> Option<String> {
    match err {
        Error::Validation { field, .. } => field.clone(),
        _ => None,
    }
}

#[tokio::test]
async fn kairos_makes_the_vector_of_a_query() {
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
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let mut config = base_config(&scratch_url);
    config.secrets_key = Some(SecretsKey::from_bytes([13u8; 32]));
    let mut state = app::state_with(config, pool, auth);
    let embedded: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    state.embedding = Some(Arc::new(EmbeddingService::new(Arc::clone(&embedded))));
    let router = app::router(state);
    let server = spawn_server(router.clone()).await;
    let svc = server.client(&svc_token, "acme");
    let bob = server.client(&bob_token, "acme");
    for client in [&svc, &bob] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        let user_id = user_id(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }

    // 1. The embedded model, for a member who is not an admin.
    let text = "the token of the second writer";
    let answer = bob
        .query_vector(EMBEDDED, text)
        .await
        .expect("the vector of the embedded model");
    assert_eq!(answer.model, EMBEDDED);
    assert_eq!(answer.vector.len(), 384);
    assert_eq!(
        answer.vector,
        embedded.embed_one(text).expect("the vector of the model")
    );

    // 2. No provider for the model; a model or a text that is not correct.
    for model in [
        "local/bge-small-en-v1.5-q/384",
        "deterministic/sha256-384/128",
        "remote/fake-embed/8",
    ] {
        let err = bob
            .query_vector(model, text)
            .await
            .expect_err("no provider");
        assert_eq!(
            refusal(&err),
            (422, "NO_QUERY_PROVIDER".to_string(), model.to_string()),
            "{model}"
        );
        assert!(err.to_string().contains(model), "{err}");
    }
    let err = bob
        .query_vector("bge-small", text)
        .await
        .expect_err("not a model");
    assert_eq!(field_of(&err).as_deref(), Some("model"));
    for bad in [String::new(), "  ".to_string(), "x".repeat(2001)] {
        let err = bob
            .query_vector(EMBEDDED, &bad)
            .await
            .expect_err("not a text");
        assert_eq!(field_of(&err).as_deref(), Some("text"), "{err}");
    }
    // 2,000 characters is the limit, not over it.
    let long = "é".repeat(2000);
    assert_eq!(
        bob.query_vector(EMBEDDED, &long)
            .await
            .expect("2,000 characters")
            .vector
            .len(),
        384
    );

    // 3. The remote provider of the organization, with its sealed key.
    let endpoint = common::embed_http::serve(KEY, 8).await;
    svc.put_code_index_settings(&PutCodeIndexSettings {
        summary: PutSummaryProvider {
            provider: "embedded".into(),
            ..Default::default()
        },
        vectors: PutVectorProvider {
            provider: "remote".into(),
            base_url: Some(endpoint.base_url.clone()),
            model: Some("fake-embed".into()),
            secret: Some(Secret::new(KEY)),
        },
        concurrency: None,
        default_summaries: None,
    })
    .await
    .expect("the admin sets the remote provider");
    let answer = bob
        .query_vector("remote/fake-embed/8", "four")
        .await
        .expect("the vector of the remote model");
    assert_eq!(answer.model, "remote/fake-embed/8");
    assert_eq!(answer.vector, vec![4.0; 8]);
    assert_eq!(endpoint.requests(), 1, "one request, with no probe");

    let err = bob
        .query_vector("remote/other-embed/8", "four")
        .await
        .expect_err("another remote model");
    assert_eq!(refusal(&err).1, "NO_QUERY_PROVIDER");
    let err = bob
        .query_vector("remote/fake-embed/16", "four")
        .await
        .expect_err("another dimension");
    assert_eq!(
        refusal(&err),
        (
            502,
            "QUERY_VECTOR_FAILED".to_string(),
            "remote/fake-embed/16".to_string()
        )
    );
    assert!(!err.to_string().contains(KEY), "the key is not in the text");
    // The embedded model still answers.
    assert_eq!(
        bob.query_vector(EMBEDDED, text)
            .await
            .expect("the embedded model")
            .vector
            .len(),
        384
    );

    // 4. No token.
    let (status, body) = common::request(
        &router,
        Method::POST,
        "/api/code-index/query-vector",
        None,
        &[("x-tenant", "acme")],
        Some(json!({ "model": EMBEDDED, "text": text })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "UNAUTHORIZED");
}
