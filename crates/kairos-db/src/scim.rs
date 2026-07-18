//! SCIM token storage (KAIROS-T-0025, contract per KAIROS-A-0016).
//!
//! `scim_tokens` is a TENANT table: A-0016 makes SCIM traffic
//! "tenant-scoped by the token, not by subdomain", so each organization's
//! tokens live inside its own `org_{slug}` schema and every query here runs
//! on a connection whose `search_path` is pinned to the target tenant.
//!
//! Only the hex SHA-256 of the full token string is stored. Hashing (and
//! the `kairos_scim_<slug>_<64-hex>` token format itself) is owned by the
//! server layer (`kairos-server::scim`); this module never sees a secret.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::scim_tokens;

/// A SCIM bearer token row (`scim_tokens`, tenant schema).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = scim_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ScimToken {
    pub id: Uuid,
    /// Operator label ("okta-prod", ...).
    pub name: String,
    /// Hex SHA-256 of the full token string; never the secret itself.
    pub token_hash: String,
    /// The org admin (`public.users.id`) who created the token.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// Set on revocation; the row is retained for audit.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Insert for [`ScimToken`]; `id`/`created_at` come from column defaults.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = scim_tokens)]
pub struct NewScimToken {
    pub name: String,
    pub token_hash: String,
    pub created_by: Uuid,
}

/// Insert a new token row (connection must be tenant-pinned).
pub fn create_token(conn: &mut PgConnection, new: NewScimToken) -> QueryResult<ScimToken> {
    diesel::insert_into(scim_tokens::table)
        .values(new)
        .returning(ScimToken::as_returning())
        .get_result(conn)
}

/// All token rows for the pinned tenant, newest first (revoked included —
/// the listing is an audit surface).
pub fn list_tokens(conn: &mut PgConnection) -> QueryResult<Vec<ScimToken>> {
    scim_tokens::table
        .order(scim_tokens::created_at.desc())
        .select(ScimToken::as_select())
        .load(conn)
}

/// The token row with this id, if any.
pub fn find_token(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<ScimToken>> {
    scim_tokens::table
        .filter(scim_tokens::id.eq(id))
        .select(ScimToken::as_select())
        .first(conn)
        .optional()
}

/// Mark a token revoked (idempotence is the caller's policy: the returned
/// row's previous `revoked_at` is not inspected here).
pub fn revoke_token(conn: &mut PgConnection, id: Uuid) -> QueryResult<ScimToken> {
    diesel::update(scim_tokens::table.filter(scim_tokens::id.eq(id)))
        .set(scim_tokens::revoked_at.eq(diesel::dsl::now))
        .returning(ScimToken::as_returning())
        .get_result(conn)
}
