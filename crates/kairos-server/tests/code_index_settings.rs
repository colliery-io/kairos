//! The provider settings of the code index of a tenant (KAIROS-T-0339,
//! COLLIERY-I-0611): `/api/org/code-index-settings`.
//!
//! 1. An org admin sets Ollama Cloud with a key; the read shows the
//!    provider, the URL, the model and the status of the secret, never the
//!    key. A member reads it too, and cannot write it.
//! 2. A provider that needs a value is refused with the field named; an
//!    unknown provider or field is refused and named.
//! 3. A write with the secret absent keeps the stored secret; an empty
//!    secret removes it, and the provider then refuses to stay hosted.
//! 4. The stored secret opens only with the key of the deployment and the
//!    AAD of its tenant and field.
//! 5. A server with no KAIROS_SECRETS_KEY refuses a secret and names the
//!    setting.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_code_index_settings_t0339_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types_auth::Secret;
use kairos_client::types_code_index::{
    PutCodeIndexSettings, PutSummaryProvider, PutVectorProvider,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;

const SCRATCH_DB: &str = "kairos_code_index_settings_t0339_test";
const KEY: &str = "ollama_t0339_API_SECRET_42";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn ollama(secret: Option<&str>) -> PutCodeIndexSettings {
    PutCodeIndexSettings {
        summary: PutSummaryProvider {
            provider: "ollama-cloud".into(),
            base_url: Some("https://ollama.com/v1".into()),
            model: Some("gemma4:31b".into()),
            region: None,
            secret: secret.map(Secret::new),
        },
        vectors: PutVectorProvider {
            provider: "embedded".into(),
            ..Default::default()
        },
        concurrency: Some(8),
        default_summaries: None,
    }
}

fn field_of(err: &Error) -> Option<String> {
    match err {
        Error::Validation { field, .. } => field.clone(),
        _ => None,
    }
}

#[tokio::test]
async fn the_provider_settings_of_a_tenant() {
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

    let key = SecretsKey::from_bytes([9u8; 32]);
    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let server_with = |secrets_key: Option<SecretsKey>| {
        let mut config = base_config(&scratch_url);
        config.secrets_key = secrets_key;
        app::router(app::state_with(config, pool.clone(), auth.clone()))
    };
    let keyed = spawn_server(server_with(Some(key.clone()))).await;
    let unkeyed = spawn_server(server_with(None)).await;
    let svc = keyed.client(&svc_token, "acme");
    let bob = keyed.client(&bob_token, "acme");
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
    let svc_id = user_id(&mut conn, "svc@kairos.test");

    // A tenant with no row: the defaults.
    let defaults = bob.code_index_settings().await.expect("the defaults");
    assert_eq!(defaults.summary.provider, "embedded");
    assert_eq!(defaults.vectors.provider, "embedded");
    assert_eq!(defaults.concurrency, 4);
    assert!(!defaults.summary.secret.set);
    assert_eq!(defaults.updated_by, None);

    // 1. The admin sets Ollama Cloud with a key; a member cannot.
    let err = bob
        .put_code_index_settings(&ollama(Some(KEY)))
        .await
        .expect_err("bob is a member");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let set = svc
        .put_code_index_settings(&ollama(Some(KEY)))
        .await
        .expect("the admin sets the provider");
    assert_eq!(set.summary.provider, "ollama-cloud");
    assert_eq!(
        set.summary.base_url.as_deref(),
        Some("https://ollama.com/v1")
    );
    assert_eq!(set.summary.model.as_deref(), Some("gemma4:31b"));
    assert!(set.summary.secret.set);
    assert_eq!(
        set.summary.secret.set_by.as_deref(),
        Some(svc_id.to_string().as_str())
    );
    let set_at = set
        .summary
        .secret
        .set_at
        .clone()
        .expect("when the key was set");
    assert_eq!(set.concurrency, 8);
    assert_eq!(set.updated_by.as_deref(), Some(svc_id.to_string().as_str()));
    let shown = serde_json::to_string(&bob.code_index_settings().await.unwrap()).unwrap();
    assert!(!shown.contains(KEY), "the key is in a read: {shown}");

    // 2. Refusals that name the field.
    let mut no_url = ollama(None);
    no_url.summary.base_url = None;
    let err = svc
        .put_code_index_settings(&no_url)
        .await
        .expect_err("no URL");
    assert_eq!(field_of(&err).as_deref(), Some("summary.base_url"), "{err}");
    let mut no_model = ollama(None);
    no_model.summary.model = None;
    let err = svc
        .put_code_index_settings(&no_model)
        .await
        .expect_err("no model");
    assert_eq!(field_of(&err).as_deref(), Some("summary.model"), "{err}");
    let mut unknown = ollama(None);
    unknown.summary.provider = "openai".into();
    let err = svc
        .put_code_index_settings(&unknown)
        .await
        .expect_err("unknown provider");
    assert_eq!(field_of(&err).as_deref(), Some("summary.provider"), "{err}");
    let mut bad_concurrency = ollama(None);
    bad_concurrency.concurrency = Some(0);
    let err = svc
        .put_code_index_settings(&bad_concurrency)
        .await
        .expect_err("concurrency 0");
    assert_eq!(field_of(&err).as_deref(), Some("concurrency"), "{err}");
    let mut bedrock = ollama(None);
    bedrock.summary.provider = "bedrock".into();
    bedrock.summary.base_url = None;
    bedrock.summary.model = Some("anthropic.claude-3-5-haiku-20241022-v1:0".into());
    let err = svc
        .put_code_index_settings(&bedrock)
        .await
        .expect_err("no region");
    assert_eq!(field_of(&err).as_deref(), Some("summary.region"), "{err}");
    bedrock.summary.region = Some("us-east-1".into());
    bedrock.summary.secret = Some(Secret::new("AKIA-only"));
    let err = svc
        .put_code_index_settings(&bedrock)
        .await
        .expect_err("the credentials have 2 or 3 parts");
    assert_eq!(field_of(&err).as_deref(), Some("summary.secret"), "{err}");
    // An unknown field (the body is JSON with a field the DTO does not have).
    let raw = http
        .put(format!("{}/api/org/code-index-settings", keyed.base_url))
        .bearer_auth(&svc_token)
        .header("X-Tenant", "acme")
        .json(&serde_json::json!({
            "summary": {"provider": "embedded", "cost_cap": 10},
            "vectors": {"provider": "embedded"},
        }))
        .send()
        .await
        .expect("send");
    assert_eq!(raw.status(), 422);
    let text = raw.text().await.unwrap();
    assert!(text.contains("cost_cap"), "{text}");

    // 3. The secret stays when the body does not name it, and goes when the
    //    body sends an empty one.
    let kept = svc
        .put_code_index_settings(&ollama(None))
        .await
        .expect("a write with no secret keeps it");
    assert!(kept.summary.secret.set);
    assert_eq!(kept.summary.secret.set_at.as_deref(), Some(set_at.as_str()));
    let err = svc
        .put_code_index_settings(&ollama(Some("")))
        .await
        .expect_err("ollama-cloud with no key");
    assert_eq!(field_of(&err).as_deref(), Some("summary.secret"), "{err}");
    let mut back = ollama(Some(""));
    back.summary.provider = "embedded".into();
    back.summary.base_url = None;
    back.summary.model = None;
    let removed = svc
        .put_code_index_settings(&back)
        .await
        .expect("back to embedded, the key removed");
    assert_eq!(removed.summary.provider, "embedded");
    assert!(!removed.summary.secret.set);

    // 4. The sealed secret opens with the key and the AAD of its field, and
    //    with nothing else.
    svc.put_code_index_settings(&ollama(Some(KEY)))
        .await
        .expect("the key again");
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .unwrap();
    let row = kairos_db::code_index_settings::load(&mut conn)
        .expect("read")
        .expect("a row");
    let sealed = row.summary_secret().expect("a sealed secret");
    let aad = kairos_server::api::org::code_index_settings::settings_aad("acme", "summary.secret");
    let opened = key
        .open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext)
        .expect("opens with the key and the AAD");
    assert_eq!(opened, KEY.as_bytes());
    let other_aad =
        kairos_server::api::org::code_index_settings::settings_aad("acme", "vectors.secret");
    assert!(
        key.open(
            &other_aad,
            &sealed.key_id,
            &sealed.nonce,
            &sealed.ciphertext
        )
        .is_err(),
        "the AAD of another field opens nothing"
    );
    let other_key = SecretsKey::from_bytes([10u8; 32]);
    assert!(
        other_key
            .open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext)
            .is_err(),
        "another key opens nothing"
    );

    // 5. No KAIROS_SECRETS_KEY: a secret is refused with the setting named;
    //    a write with no secret still goes.
    let svc_unkeyed = unkeyed.client(&svc_token, "acme");
    let err = svc_unkeyed
        .put_code_index_settings(&ollama(Some(KEY)))
        .await
        .expect_err("no key on this server");
    match &err {
        Error::Other {
            status,
            code,
            details,
            ..
        } => {
            assert_eq!(*status, 501);
            assert_eq!(code, "SECRETS_NOT_CONFIGURED");
            assert_eq!(details["setting"], "KAIROS_SECRETS_KEY");
        }
        other => panic!("{other}"),
    }
    svc_unkeyed
        .put_code_index_settings(&ollama(None))
        .await
        .expect("a write that keeps the stored secret needs no key");
}
