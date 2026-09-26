//! Round-trip tests for `public.local_sessions` and the password/session query
//! module (KAIROS-T-0201, KAIROS-T-0203).
//!
//! The test that matters most here is
//! [`changing_a_password_revokes_every_session`]. The guarantee it checks is the
//! reason `set_password` exists at all: a password change after a suspected
//! compromise that leaves the attacker's session working defeats the only thing
//! the person was trying to do. The guarantee lives in the storage layer so that
//! no endpoint — today's, or KAIROS-T-0204's admin reset — can get it wrong by
//! forgetting.
//!
//! Runs against the real compose Postgres (KAIROS-A-0012, never mocked).

use chrono::{Duration, Utc};
use diesel::prelude::*;
use diesel::sql_query;

use kairos_db::local_auth::{self, NewLocalSession};
use kairos_db::models::{NewUser, User};
use kairos_db::run_public_migrations;
use kairos_db::schema::users;
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn admin_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("DATABASE_URL has a db segment");
    format!("{base}/{db}")
}

/// A scratch database with public migrations applied. No tenant needed: both
/// tables under test are public, which is itself the KAIROS-T-0203 design point.
fn setup(db: &str) -> PgConnection {
    let admin = admin_url();
    let mut root = PgConnection::establish(&admin).expect("connect admin");
    let _ = sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).execute(&mut root);
    sql_query(format!("CREATE DATABASE {db}"))
        .execute(&mut root)
        .expect("create scratch db");
    let mut conn = PgConnection::establish(&with_database(&admin, db)).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    conn
}

fn teardown(db: &str) {
    let mut root = PgConnection::establish(&admin_url()).expect("connect admin for teardown");
    let _ = sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).execute(&mut root);
}

fn make_user(conn: &mut PgConnection, email: &str) -> User {
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("oidc:{email}"),
            user_name: email.to_string(),
            email: email.to_string(),
            display_name: email.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("insert user")
}

fn mint(conn: &mut PgConnection, user_id: Uuid, hash: &str, ttl_days: i64) -> Uuid {
    local_auth::create_session(
        conn,
        NewLocalSession {
            user_id,
            token_hash: hash.to_string(),
            expires_at: Utc::now() + Duration::days(ttl_days),
        },
    )
    .expect("create session")
    .id
}

#[test]
fn a_session_round_trips_and_expiry_and_revocation_are_honored() {
    let db = "kairos_local_auth_sessions_test";
    let mut conn = setup(db);
    let user = make_user(&mut conn, "live@example.test");

    let live = mint(&mut conn, user.id, "hash-live", 14);
    let found = local_auth::find_session_by_hash(&mut conn, "hash-live")
        .expect("query")
        .expect("row");
    assert_eq!(found.id, live);
    assert_eq!(found.user_id, user.id);
    assert!(found.last_used_at.is_none(), "unused until used");
    assert!(found.is_valid_at(Utc::now()));

    // last_used_at is best-effort but must actually land when it succeeds — it is
    // what makes a stale session visible later.
    local_auth::touch_session(&mut conn, live).expect("touch");
    let touched = local_auth::find_session_by_hash(&mut conn, "hash-live")
        .expect("query")
        .expect("row");
    assert!(touched.last_used_at.is_some());

    // An expired row is still FOUND, and invalid. Found-but-invalid is the shape
    // the auth path needs, so every failure can collapse into one 401 instead of
    // "not found" and "expired" diverging.
    local_auth::create_session(
        &mut conn,
        NewLocalSession {
            user_id: user.id,
            token_hash: "hash-expired".to_string(),
            expires_at: Utc::now() - Duration::hours(1),
        },
    )
    .expect("create expired");
    let expired = local_auth::find_session_by_hash(&mut conn, "hash-expired")
        .expect("query")
        .expect("row");
    assert!(!expired.is_valid_at(Utc::now()), "past expiry is invalid");

    // Revocation by hash, because that is what a logout holds.
    assert_eq!(
        local_auth::revoke_session_by_hash(&mut conn, "hash-live").expect("revoke"),
        1
    );
    let revoked = local_auth::find_session_by_hash(&mut conn, "hash-live")
        .expect("query")
        .expect("row");
    assert!(!revoked.is_valid_at(Utc::now()));

    // Idempotent: a repeated logout changes nothing rather than moving the
    // timestamp, so the audit record says when the session really ended.
    assert_eq!(
        local_auth::revoke_session_by_hash(&mut conn, "hash-live").expect("revoke again"),
        0
    );
    let again = local_auth::find_session_by_hash(&mut conn, "hash-live")
        .expect("query")
        .expect("row");
    assert_eq!(again.revoked_at, revoked.revoked_at);

    // An unknown hash is None, not an error.
    assert!(
        local_auth::find_session_by_hash(&mut conn, "no-such-hash")
            .expect("query")
            .is_none()
    );

    teardown(db);
}

#[test]
fn changing_a_password_revokes_every_session() {
    let db = "kairos_local_auth_password_revokes_test";
    let mut conn = setup(db);

    let alice = make_user(&mut conn, "alice@example.test");
    let bob = make_user(&mut conn, "bob@example.test");

    // Alice is logged in on three devices; Bob on one.
    for hash in ["a-laptop", "a-phone", "a-cli"] {
        mint(&mut conn, alice.id, hash, 14);
    }
    mint(&mut conn, bob.id, "b-laptop", 14);

    // The whole point: setting a password revokes the sessions in the same
    // transaction, so "changed the password but the attacker is still logged in"
    // cannot happen.
    let revoked =
        local_auth::set_password(&mut conn, alice.id, "$argon2id$fake$hash").expect("set password");
    assert_eq!(revoked, 3, "all three of Alice's sessions");

    for hash in ["a-laptop", "a-phone", "a-cli"] {
        let row = local_auth::find_session_by_hash(&mut conn, hash)
            .expect("query")
            .expect("row");
        assert!(
            !row.is_valid_at(Utc::now()),
            "{hash} should be revoked by the password change"
        );
    }

    // ...and NOT anybody else's. A password change is not a deployment-wide logout.
    let bobs = local_auth::find_session_by_hash(&mut conn, "b-laptop")
        .expect("query")
        .expect("row");
    assert!(bobs.is_valid_at(Utc::now()), "Bob is unaffected");

    // The password did land.
    let stored = local_auth::find_user_by_email(&mut conn, "alice@example.test")
        .expect("query")
        .expect("row");
    assert_eq!(stored.password_hash.as_deref(), Some("$argon2id$fake$hash"));

    // Clearing a password revokes too: taking the password away must not leave the
    // sessions it minted working.
    mint(&mut conn, alice.id, "a-new-laptop", 14);
    let revoked = local_auth::clear_password(&mut conn, alice.id).expect("clear password");
    assert_eq!(revoked, 1);
    let cleared = local_auth::find_user_by_email(&mut conn, "alice@example.test")
        .expect("query")
        .expect("row");
    assert!(cleared.password_hash.is_none());
    assert!(
        !local_auth::find_session_by_hash(&mut conn, "a-new-laptop")
            .expect("query")
            .expect("row")
            .is_valid_at(Utc::now())
    );

    teardown(db);
}

#[test]
fn looking_a_user_up_does_not_filter_out_oidc_only_accounts() {
    // If this query filtered on `password_hash IS NOT NULL`, the caller could no
    // longer make "no such person" and "that person has no password" identical —
    // the two cases would already have diverged before it got the chance. That is
    // an account-enumeration oracle over an organization's real addresses, so the
    // absence of the filter is asserted rather than assumed.
    let db = "kairos_local_auth_lookup_test";
    let mut conn = setup(db);

    make_user(&mut conn, "oidc-only@example.test");
    let found = local_auth::find_user_by_email(&mut conn, "oidc-only@example.test")
        .expect("query")
        .expect("the row must come back even with no password");
    assert!(found.password_hash.is_none());

    assert!(
        local_auth::find_user_by_email(&mut conn, "nobody@example.test")
            .expect("query")
            .is_none()
    );

    teardown(db);
}

#[test]
fn deleting_a_user_takes_their_sessions_with_them() {
    let db = "kairos_local_auth_cascade_test";
    let mut conn = setup(db);
    let user = make_user(&mut conn, "leaver@example.test");
    mint(&mut conn, user.id, "leaver-session", 14);

    diesel::delete(users::table.filter(users::id.eq(user.id)))
        .execute(&mut conn)
        .expect("delete user");

    // ON DELETE CASCADE, not an orphan row whose user_id points nowhere — a
    // session that outlives its user is a credential with no owner.
    assert!(
        local_auth::find_session_by_hash(&mut conn, "leaver-session")
            .expect("query")
            .is_none()
    );

    teardown(db);
}

#[test]
fn expired_sessions_can_be_swept() {
    let db = "kairos_local_auth_sweep_test";
    let mut conn = setup(db);
    let user = make_user(&mut conn, "sweeper@example.test");

    local_auth::create_session(
        &mut conn,
        NewLocalSession {
            user_id: user.id,
            token_hash: "old".to_string(),
            expires_at: Utc::now() - Duration::days(30),
        },
    )
    .expect("create old");
    mint(&mut conn, user.id, "current", 14);

    let deleted = local_auth::delete_expired_sessions(&mut conn, Utc::now()).expect("sweep");
    assert_eq!(deleted, 1);
    assert!(
        local_auth::find_session_by_hash(&mut conn, "current")
            .expect("query")
            .is_some(),
        "the sweep must not touch live sessions"
    );

    teardown(db);
}
