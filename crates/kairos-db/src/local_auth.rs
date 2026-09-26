//! Local-account password and session storage (KAIROS-T-0201, KAIROS-T-0203,
//! KAIROS-I-0018).
//!
//! Both tables here are **public**, not tenant-scoped. `users.password_hash`
//! obviously so — one person is one row (KAIROS-T-0197). `local_sessions`
//! deliberately so: a session stands in for an OIDC token, and an OIDC token is
//! deployment-wide, so a person in two organizations logs in once and the tenant
//! middleware enforces membership per request exactly as it already does.
//!
//! The hashing itself lives in `kairos_server::local_auth` (KAIROS-A-0009: this
//! crate does I/O, that one does arithmetic). This module never sees a raw
//! password or a raw session token — only hashes — which is also why
//! [`set_password`] takes a hash rather than a password.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::models::User;
use crate::schema::{local_sessions, users};

/// A session row (`public.local_sessions`). Carries the SHA-256 of the bearer,
/// never the bearer.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = local_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct LocalSession {
    pub id: Uuid,
    /// The person this session authenticates (`public.users.id`).
    pub user_id: Uuid,
    /// Hex SHA-256 of the full token; never the token itself.
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    /// When the session stops working. NOT NULL — a session that never expires
    /// is a permanent credential handed out by a login form.
    pub expires_at: DateTime<Utc>,
    /// Best-effort last-seen; `None` until first use.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Set on logout or on a password change; the row is retained.
    pub revoked_at: Option<DateTime<Utc>>,
}

impl LocalSession {
    /// Whether the session is usable at `now`. Checked AFTER the hash match so
    /// every failure on the auth path collapses into one 401 — the same shape as
    /// [`crate::api_keys::ApiKey::is_valid_at`].
    pub fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }
}

/// Insert for [`LocalSession`]; `id`/`created_at` come from column defaults.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = local_sessions)]
pub struct NewLocalSession {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

/// Insert a session row.
pub fn create_session(conn: &mut PgConnection, new: NewLocalSession) -> QueryResult<LocalSession> {
    diesel::insert_into(local_sessions::table)
        .values(new)
        .returning(LocalSession::as_returning())
        .get_result(conn)
}

/// The session row with this token hash, if any. Validity is the caller's check —
/// revoked and expired rows come back too, so the caller can keep its failures
/// uniform.
pub fn find_session_by_hash(
    conn: &mut PgConnection,
    token_hash: &str,
) -> QueryResult<Option<LocalSession>> {
    local_sessions::table
        .filter(local_sessions::token_hash.eq(token_hash))
        .select(LocalSession::as_select())
        .first(conn)
        .optional()
}

/// Revoke one session by its token hash, and report whether anything changed.
///
/// By hash rather than by id because that is what a logout has: the bearer it was
/// presented with. Already-revoked rows are left alone, so a repeated logout is
/// idempotent instead of moving the timestamp.
pub fn revoke_session_by_hash(conn: &mut PgConnection, token_hash: &str) -> QueryResult<usize> {
    diesel::update(
        local_sessions::table
            .filter(local_sessions::token_hash.eq(token_hash))
            .filter(local_sessions::revoked_at.is_null()),
    )
    .set(local_sessions::revoked_at.eq(diesel::dsl::now))
    .execute(conn)
}

/// Revoke every live session a user holds, and report how many.
pub fn revoke_all_sessions_for_user(conn: &mut PgConnection, user_id: Uuid) -> QueryResult<usize> {
    diesel::update(
        local_sessions::table
            .filter(local_sessions::user_id.eq(user_id))
            .filter(local_sessions::revoked_at.is_null()),
    )
    .set(local_sessions::revoked_at.eq(diesel::dsl::now))
    .execute(conn)
}

/// Record that a session was just used. Best-effort: the auth path ignores the
/// result, so a write failure never fails a request.
pub fn touch_session(conn: &mut PgConnection, id: Uuid) -> QueryResult<usize> {
    diesel::update(local_sessions::table.filter(local_sessions::id.eq(id)))
        .set(local_sessions::last_used_at.eq(diesel::dsl::now))
        .execute(conn)
}

/// The user with this email, whether or not they have a password.
///
/// Returns OIDC-only users too, on purpose. The caller must not be able to tell
/// "no such person" from "that person has no password" — if this filtered on
/// `password_hash IS NOT NULL`, the two cases would already have diverged before
/// the caller got a chance to make them identical.
pub fn find_user_by_email(conn: &mut PgConnection, email: &str) -> QueryResult<Option<User>> {
    users::table
        .filter(users::email.eq(email))
        .order(users::created_at.asc())
        .select(User::as_select())
        .first(conn)
        .optional()
}

/// Every session a user holds, newest first, revoked and expired rows included.
///
/// The listing is an audit surface — "which devices am I logged in on, and which
/// of those did I already end" — so it does not filter, exactly as
/// [`crate::api_keys::list_keys`] does not.
pub fn list_sessions_for_user(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> QueryResult<Vec<LocalSession>> {
    local_sessions::table
        .filter(local_sessions::user_id.eq(user_id))
        .order(local_sessions::created_at.desc())
        .select(LocalSession::as_select())
        .load(conn)
}

/// The `external_id` prefix for an account that exists only locally
/// (KAIROS-T-0204).
///
/// `external_id` is normally an OIDC `sub`. A local account has none, so it gets a
/// synthetic one — the same device `service_accounts` uses with `svc:`. It must be
/// synthetic rather than empty because `external_id` identifies the principal
/// everywhere downstream, including `KAIROS_DEPLOYMENT_ADMINS`, and two accounts
/// with an empty one would be indistinguishable.
pub const LOCAL_EXTERNAL_ID_PREFIX: &str = "local:";

/// The synthetic `external_id` for a local account with this email.
pub fn local_external_id(email: &str) -> String {
    format!("{LOCAL_EXTERNAL_ID_PREFIX}{}", email.trim().to_lowercase())
}

/// Create a local account, or adopt the existing row for this email.
///
/// **One person is one row** (KAIROS-T-0197). An admin adding a password for an
/// email that already signed in through the issuer must get a password on THAT row,
/// not a second person with the same address — otherwise the same human owns two
/// identities, two sets of memberships and two audit trails, and which one they get
/// depends on how they logged in that day.
///
/// So: if a user with this email exists, the password goes on it and its
/// `external_id` is left ALONE. Rewriting an OIDC `sub` to `local:…` would break
/// that person's next SSO login, and [`crate::local_auth`]'s session path does not
/// need the prefix — only a brand-new row does.
///
/// Returns the user and whether a row was created (`true`) or adopted (`false`),
/// because the caller says different things to the admin in each case.
pub fn upsert_local_user(
    conn: &mut PgConnection,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> QueryResult<(User, bool)> {
    let email = email.trim().to_lowercase();
    conn.transaction(|conn| {
        if let Some(existing) = find_user_by_email(conn, &email)? {
            set_password(conn, existing.id, password_hash)?;
            // Re-read: `existing` predates the password write.
            let refreshed = find_user_by_email(conn, &email)?.expect("just updated");
            return Ok((refreshed, false));
        }
        let user: User = diesel::insert_into(users::table)
            .values(crate::models::NewUser {
                external_id: local_external_id(&email),
                user_name: email.clone(),
                email: email.clone(),
                display_name: display_name.to_string(),
            })
            .returning(User::as_returning())
            .get_result(conn)?;
        // Through `set_password` rather than as part of the INSERT, so there is ONE
        // place a password is ever written and the revoke-on-change guarantee has no
        // second path to leak through. A brand-new user has no sessions to revoke,
        // which makes this free.
        set_password(conn, user.id, password_hash)?;
        let created = find_user_by_email(conn, &email)?.expect("just inserted");
        Ok((created, true))
    })
}

/// Set (or replace) a user's password hash **and revoke every session they
/// hold**, in one transaction.
///
/// The revocation is here, not in the handler, and that placement is the point of
/// the function. A password change after a suspected compromise that leaves the
/// attacker's session working defeats the only thing the user was trying to do.
/// Putting the two statements in one transaction makes "changed the password but
/// forgot to revoke" unrepresentable rather than merely discouraged, and means a
/// later endpoint — an admin reset, a self-service change — cannot get it wrong by
/// omission.
///
/// Returns how many sessions were revoked.
pub fn set_password(
    conn: &mut PgConnection,
    user_id: Uuid,
    password_hash: &str,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        diesel::update(users::table.filter(users::id.eq(user_id)))
            .set(users::password_hash.eq(Some(password_hash)))
            .execute(conn)?;
        revoke_all_sessions_for_user(conn, user_id)
    })
}

/// Remove a user's password, revoking their sessions with it.
///
/// The inverse of [`set_password`] and for the same reason: taking a password away
/// must not leave the sessions it minted working.
pub fn clear_password(conn: &mut PgConnection, user_id: Uuid) -> QueryResult<usize> {
    conn.transaction(|conn| {
        diesel::update(users::table.filter(users::id.eq(user_id)))
            .set(users::password_hash.eq(None::<String>))
            .execute(conn)?;
        revoke_all_sessions_for_user(conn, user_id)
    })
}

/// Why a bootstrap did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapSkipped {
    /// The deployment already has users, so the bootstrap is inert. The normal
    /// outcome on every boot after the first.
    UsersExist,
    /// Another process created the first user between the count and the insert.
    RacedAnotherReplica,
}

/// Create the first-boot admin, but **only if the deployment has no users at all**
/// (KAIROS-T-0204).
///
/// The emptiness check and the insert share a transaction, so "no users" cannot go
/// stale between them within one process. Across processes it still can — two
/// replicas booting together both see zero — so a unique-violation on
/// `users.email` is reported as [`BootstrapSkipped::RacedAnotherReplica`] rather
/// than failing a boot. The outcome an operator cares about, exactly one admin, holds
/// either way.
///
/// "No users at all" rather than "no admin" is deliberate. It is the only condition
/// that cannot be re-entered: once anybody exists, the bootstrap is dead for the
/// lifetime of the database, so leaving the variable set in a manifest is untidy
/// rather than a standing backdoor.
pub fn bootstrap_admin_if_empty(
    conn: &mut PgConnection,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> QueryResult<Result<User, BootstrapSkipped>> {
    let email = email.trim().to_lowercase();
    conn.transaction(|conn| {
        let existing: i64 = users::table.count().get_result(conn)?;
        if existing > 0 {
            return Ok(Err(BootstrapSkipped::UsersExist));
        }
        let inserted = diesel::insert_into(users::table)
            .values(crate::models::NewUser {
                external_id: local_external_id(&email),
                user_name: email.clone(),
                email: email.clone(),
                display_name: display_name.to_string(),
            })
            .returning(User::as_returning())
            .get_result(conn);
        let user = match inserted {
            Ok(user) => user,
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            )) => return Ok(Err(BootstrapSkipped::RacedAnotherReplica)),
            Err(e) => return Err(e),
        };
        set_password(conn, user.id, password_hash)?;
        // Re-read: `user` came from the INSERT's RETURNING and so predates the
        // password write, which would hand the caller a row whose `password_hash` is
        // still NULL.
        let created = find_user_by_email(conn, &email)?.expect("just inserted");
        Ok(Ok(created))
    })
}

/// Delete sessions that expired before `before`. Housekeeping for an operator or
/// a future sweeper; nothing on the request path depends on it, because expiry is
/// enforced by [`LocalSession::is_valid_at`] rather than by the row's absence.
pub fn delete_expired_sessions(
    conn: &mut PgConnection,
    before: DateTime<Utc>,
) -> QueryResult<usize> {
    diesel::delete(local_sessions::table.filter(local_sessions::expires_at.lt(before)))
        .execute(conn)
}
