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
