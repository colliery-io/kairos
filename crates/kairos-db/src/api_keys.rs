//! Service-account API key storage (KAIROS-A-0017 / KAIROS-T-0057).
//!
//! `api_keys` is a TENANT table (like `scim_tokens`): each org's keys live in
//! its own `org_{slug}` schema, so every query here runs on a connection whose
//! `search_path` is pinned to the target tenant.
//!
//! Only the hex SHA-256 of the full key is stored. Hashing and the
//! `kairos_sk_<slug>_<hex>` key format are owned by the server layer
//! (`kairos-server`); this module never sees a raw secret. The "is this key
//! currently valid" decision (expiry/revocation) is likewise the caller's — the
//! auth path keeps its failures uniform (KAIROS-T-0058).

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::api_keys;

/// An API key row (`api_keys`, tenant schema). The row never carries the raw
/// secret — only its hash and a display `prefix`.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = api_keys)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ApiKey {
    pub id: Uuid,
    /// The service-account principal (`public.users.id`, kind='service_account').
    pub user_id: Uuid,
    /// Operator label ("ci-deploy", ...).
    pub name: String,
    /// Hex SHA-256 of the full key string; never the secret itself.
    pub token_hash: String,
    /// Display-only leading characters of the key (never the secret).
    pub prefix: String,
    /// The org admin (`public.users.id`) who minted the key.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// Optional expiry; `None` = never expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// Best-effort last-seen; `None` until first use.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Set on revocation; the row is retained for audit.
    pub revoked_at: Option<DateTime<Utc>>,
}

impl ApiKey {
    /// Whether the key is usable at `now`: not revoked and not past expiry.
    /// The auth path uses this AFTER the hash match so every failure is a
    /// uniform 401 (KAIROS-T-0058).
    pub fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|exp| exp > now)
    }
}

/// Insert for [`ApiKey`]; `id`/`created_at` come from column defaults and
/// `last_used_at`/`revoked_at` start NULL.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = api_keys)]
pub struct NewApiKey {
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: String,
    pub prefix: String,
    pub created_by: Uuid,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Insert a new key row (connection must be tenant-pinned).
pub fn create_key(conn: &mut PgConnection, new: NewApiKey) -> QueryResult<ApiKey> {
    diesel::insert_into(api_keys::table)
        .values(new)
        .returning(ApiKey::as_returning())
        .get_result(conn)
}

/// The key row with this hash, if any. Validity (expiry/revocation) is the
/// caller's check — this returns revoked/expired rows too.
pub fn find_by_hash(conn: &mut PgConnection, token_hash: &str) -> QueryResult<Option<ApiKey>> {
    api_keys::table
        .filter(api_keys::token_hash.eq(token_hash))
        .select(ApiKey::as_select())
        .first(conn)
        .optional()
}

/// All key rows for one service account, newest first (revoked included — the
/// listing is an audit surface).
pub fn list_keys(conn: &mut PgConnection, user_id: Uuid) -> QueryResult<Vec<ApiKey>> {
    api_keys::table
        .filter(api_keys::user_id.eq(user_id))
        .order(api_keys::created_at.desc())
        .select(ApiKey::as_select())
        .load(conn)
}

/// The key row with this id, if any.
pub fn find_key(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<ApiKey>> {
    api_keys::table
        .filter(api_keys::id.eq(id))
        .select(ApiKey::as_select())
        .first(conn)
        .optional()
}

/// Mark a key revoked (idempotence is the caller's policy).
pub fn revoke_key(conn: &mut PgConnection, id: Uuid) -> QueryResult<ApiKey> {
    diesel::update(api_keys::table.filter(api_keys::id.eq(id)))
        .set(api_keys::revoked_at.eq(diesel::dsl::now))
        .returning(ApiKey::as_returning())
        .get_result(conn)
}

/// Record that a key was just used. Best-effort: the auth path ignores the
/// result so a write failure never blocks a request (KAIROS-T-0058).
pub fn touch_last_used(conn: &mut PgConnection, id: Uuid) -> QueryResult<usize> {
    diesel::update(api_keys::table.filter(api_keys::id.eq(id)))
        .set(api_keys::last_used_at.eq(diesel::dsl::now))
        .execute(conn)
}
