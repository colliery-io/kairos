//! SCIM bearer-token authentication (KAIROS-T-0025, contract per
//! KAIROS-A-0016): the middleware in front of every `/scim/v2` route.
//!
//! # Token format and tenant resolution
//!
//! A SCIM token is `kairos_scim_<slug>_<64-hex-secret>`: the tenant
//! discriminator is EMBEDDED IN THE TOKEN, because A-0016 makes SCIM
//! traffic "tenant-scoped by the token, not by subdomain" and the request
//! URL carries no tenant. Parsing is unambiguous even though slugs may
//! contain `_`: the secret is exactly 64 lowercase-hex characters (32
//! random bytes) and hex never contains `_`, so the split happens at the
//! LAST underscore. This keeps verification O(1) — hash the presented
//! token, look it up in `org_{slug}.scim_tokens` — instead of iterating
//! every tenant schema for a hash match.
//!
//! Only the hex SHA-256 of the full token string is stored (hashed at
//! rest); the secret is shown exactly once at creation time by
//! `POST /api/scim-tokens` ([`super::tokens`]).
//!
//! Every failure mode — malformed token, unknown tenant, no hash match,
//! revoked token — returns the SAME 401 SCIM error envelope, so the
//! endpoint is not a tenant-enumeration oracle.

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use kairos_db::models::Organization;
use kairos_db::schema::{organizations, scim_tokens};
use kairos_db::scim::ScimToken;
use kairos_db::tenant::is_valid_slug;
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::error::ScimError;
use crate::app::AppState;

/// The fixed token prefix.
pub const TOKEN_PREFIX: &str = "kairos_scim_";

/// Length of the hex secret (32 random bytes → 64 hex chars).
pub const SECRET_LEN: usize = 64;

/// The authenticated SCIM principal, inserted as a request extension for
/// every request that passes [`require_scim_token`].
#[derive(Debug, Clone)]
pub struct ScimContext {
    /// `public.organizations.id` of the token's tenant.
    pub org_id: Uuid,
    /// The tenant slug (schema `org_{slug}`).
    pub slug: String,
    /// `scim_tokens.id` of the presented token.
    pub token_id: Uuid,
    /// The token's operator label — recorded in activity details.
    pub token_name: String,
    /// The org admin who created the token (`scim_tokens.created_by`) —
    /// used as `activity_log.actor_id` for SCIM-driven mutations (SCIM has
    /// no user principal of its own).
    pub actor_id: Uuid,
}

/// Hex-encode bytes (lowercase).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a fresh token string for `slug`: `kairos_scim_<slug>_<64-hex>`
/// from 32 bytes of OS randomness.
pub fn generate_token(slug: &str) -> String {
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    format!("{TOKEN_PREFIX}{slug}_{}", hex(&secret))
}

/// The value stored at rest and compared on every request: hex SHA-256 of
/// the FULL token string.
pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Split a presented token into `(slug, secret)`, or `None` if it does not
/// have the `kairos_scim_<slug>_<64-hex>` shape. Splits at the LAST `_`
/// (see module docs for why that is unambiguous).
pub fn parse_token(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix(TOKEN_PREFIX)?;
    let (slug, secret) = rest.rsplit_once('_')?;
    let secret_ok = secret.len() == SECRET_LEN
        && secret
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (secret_ok && is_valid_slug(slug)).then_some((slug, secret))
}

/// The uniform 401 for every authentication failure mode.
fn bad_token() -> ScimError {
    ScimError::unauthorized(
        "invalid, unknown, or revoked SCIM bearer token; \
         an org admin can issue one via POST /api/scim-tokens",
    )
}

/// The SCIM auth layer: parse the token, resolve its embedded tenant, and
/// verify the hash against that tenant's live (unrevoked) `scim_tokens`.
pub async fn require_scim_token(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ScimError> {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ScimError::unauthorized("missing bearer token"))?
        .trim()
        .to_string();

    let (slug, _secret) = parse_token(&token).ok_or_else(bad_token)?;
    let slug = slug.to_string();

    // The tenant must exist before a schema-pinned connection can be
    // checked out. Unknown slug → the same 401 as a bad secret.
    let mut conn = state
        .pool
        .public_conn()
        .await
        .map_err(ScimError::internal)?;
    let org: Option<Organization> = organizations::table
        .filter(organizations::slug.eq(&slug))
        .select(Organization::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(ScimError::internal)?;
    drop(conn);
    let org = org.ok_or_else(bad_token)?;

    // Hash lookup inside the tenant's own scim_tokens table.
    let hash = hash_token(&token);
    let mut conn = state
        .pool
        .tenant(&slug)
        .await
        .map_err(ScimError::internal)?;
    let row: Option<ScimToken> = scim_tokens::table
        .filter(scim_tokens::token_hash.eq(&hash))
        .select(ScimToken::as_select())
        .first(&mut *conn)
        .await
        .optional()
        .map_err(ScimError::internal)?;
    drop(conn);
    let row = row.ok_or_else(bad_token)?;
    if row.revoked_at.is_some() {
        return Err(bad_token());
    }

    req.extensions_mut().insert(ScimContext {
        org_id: org.id,
        slug,
        token_id: row.id,
        token_name: row.name,
        actor_id: row.created_by,
    });
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip_parses() {
        let token = generate_token("acme");
        assert!(token.starts_with("kairos_scim_acme_"));
        let (slug, secret) = parse_token(&token).expect("round-trip");
        assert_eq!(slug, "acme");
        assert_eq!(secret.len(), SECRET_LEN);
    }

    #[test]
    fn slugs_with_underscores_split_at_the_last_separator() {
        let token = generate_token("acme_co_ltd");
        let (slug, _) = parse_token(&token).expect("underscore slug parses");
        assert_eq!(slug, "acme_co_ltd");
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let good = generate_token("acme");
        for bad in [
            "",
            "kairos_scim_",
            "kairos_scim_acme",
            "not_a_token",
            "kairos_scim_acme_deadbeef", // secret too short
            &good.to_uppercase(),        // hex must be lowercase
            &format!("kairos_scim_ACME_{}", "a".repeat(64)), // invalid slug
            &format!("kairos_scim__{}", "a".repeat(64)), // empty slug
            &format!("kairos_scim_acme_{}", "g".repeat(64)), // non-hex secret
        ] {
            assert!(parse_token(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn hash_is_stable_and_secret_free() {
        let token = generate_token("acme");
        let hash = hash_token(&token);
        assert_eq!(hash, hash_token(&token));
        assert_eq!(hash.len(), 64);
        assert!(!hash.contains("kairos_scim"));
        // A different token hashes differently.
        assert_ne!(hash, hash_token(&generate_token("acme")));
    }
}
