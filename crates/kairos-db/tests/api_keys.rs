//! Round-trip tests for the service-account `api_keys` tenant table and its
//! query module (KAIROS-A-0017 / KAIROS-T-0057).
//!
//! Runs against the real compose Postgres (`angreal test integration`, never
//! mocked — KAIROS-A-0012). Uses a scratch database + a provisioned tenant, so
//! success also proves the new public (`users.kind`) and tenant (`api_keys`)
//! migrations apply cleanly via `run_public_migrations` + `provision_tenant`.
//!
//! Each test uses its OWN scratch database name so the two run in parallel
//! without colliding.

use chrono::{Duration, Utc};
use diesel::prelude::*;
use diesel::sql_query;

use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::{NewServiceAccountUser, USER_KIND_SERVICE_ACCOUNT};
use kairos_db::{provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:5432/kairos";
const SLUG: &str = "acme";

fn admin_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("DATABASE_URL has a db segment");
    format!("{base}/{db}")
}

/// Drop+recreate the named scratch DB, run public migrations, provision one
/// tenant, and return a connection pinned to that tenant's schema.
fn setup(db: &str) -> PgConnection {
    let admin = admin_url();
    let mut root = PgConnection::establish(&admin).expect("connect admin");
    let _ = sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).execute(&mut root);
    sql_query(format!("CREATE DATABASE {db}"))
        .execute(&mut root)
        .expect("create scratch db");

    let scratch = with_database(&admin, db);
    let mut conn = PgConnection::establish(&scratch).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, SLUG, "Acme").expect("provision tenant");
    sql_query(format!("SET search_path TO \"org_{SLUG}\", public"))
        .execute(&mut conn)
        .expect("pin search_path");
    conn
}

fn teardown(db: &str) {
    let mut root = PgConnection::establish(&admin_url()).expect("connect admin for teardown");
    let _ = sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).execute(&mut root);
}

#[test]
fn api_key_round_trip() {
    let db = "kairos_api_keys_roundtrip_test";
    let mut conn = setup(db);

    // A service-account principal is a public.users row with kind='service_account'.
    let sa = diesel::insert_into(kairos_db::schema::users::table)
        .values(NewServiceAccountUser::new(
            "svc:test-uuid",
            "ci@svc.acme.kairos",
            "CI deployer",
        ))
        .returning(kairos_db::models::User::as_returning())
        .get_result::<kairos_db::models::User>(&mut conn)
        .expect("insert service account");
    assert_eq!(sa.kind, USER_KIND_SERVICE_ACCOUNT);
    assert!(sa.is_service_account());

    // Mint a key (hash + prefix only; no secret at rest).
    let created = api_keys::create_key(
        &mut conn,
        NewApiKey {
            user_id: sa.id,
            name: "ci-deploy".to_string(),
            token_hash: "deadbeef".repeat(8),
            prefix: "kairos_sk_acme_dead".to_string(),
            created_by: sa.id,
            expires_at: None,
        },
    )
    .expect("create key");
    assert_eq!(created.user_id, sa.id);
    assert!(created.revoked_at.is_none() && created.last_used_at.is_none());
    assert!(created.is_valid_at(Utc::now()), "fresh key is valid");

    // Lookup by hash.
    let found = api_keys::find_by_hash(&mut conn, &created.token_hash)
        .expect("find_by_hash")
        .expect("key present");
    assert_eq!(found.id, created.id);
    assert!(
        api_keys::find_by_hash(&mut conn, "nomatch")
            .expect("query ok")
            .is_none()
    );

    // Listing a service account's keys.
    let listed = api_keys::list_keys(&mut conn, sa.id).expect("list");
    assert_eq!(listed.len(), 1);

    // Best-effort last-used touch.
    assert_eq!(
        api_keys::touch_last_used(&mut conn, created.id).expect("touch"),
        1
    );
    let touched = api_keys::find_key(&mut conn, created.id)
        .expect("find")
        .expect("present");
    assert!(touched.last_used_at.is_some());

    // Revoke → no longer valid.
    let revoked = api_keys::revoke_key(&mut conn, created.id).expect("revoke");
    assert!(revoked.revoked_at.is_some());
    assert!(!revoked.is_valid_at(Utc::now()), "revoked key is invalid");

    teardown(db);
}

#[test]
fn expiry_is_honored() {
    let db = "kairos_api_keys_expiry_test";
    let mut conn = setup(db);

    let user_id = uuid::Uuid::new_v4();
    let past = api_keys::create_key(
        &mut conn,
        NewApiKey {
            user_id,
            name: "already-expired".to_string(),
            token_hash: "cafe".repeat(16),
            prefix: "kairos_sk_acme_cafe".to_string(),
            created_by: user_id,
            expires_at: Some(Utc::now() - Duration::hours(1)),
        },
    )
    .expect("create key");
    assert!(!past.is_valid_at(Utc::now()), "past-expiry key is invalid");

    let future = api_keys::create_key(
        &mut conn,
        NewApiKey {
            user_id,
            name: "still-good".to_string(),
            token_hash: "beef".repeat(16),
            prefix: "kairos_sk_acme_beef".to_string(),
            created_by: user_id,
            expires_at: Some(Utc::now() + Duration::hours(1)),
        },
    )
    .expect("create key");
    assert!(future.is_valid_at(Utc::now()), "future-expiry key is valid");

    teardown(db);
}
