//! Service-account API-key authentication (KAIROS-A-0017 / KAIROS-T-0058).
//!
//! # Key format and tenant resolution
//!
//! An API key is `kairos_sk_<slug>_<64-hex-secret>` — the same shape as a
//! SCIM token (`super::super::scim::auth`), with the tenant slug EMBEDDED in
//! the key so validation is O(1): hash the presented key, look it up in
//! `org_{slug}.api_keys`. Parsing splits at the LAST `_` (hex never contains
//! `_`, so a slug with underscores is unambiguous).
//!
//! Only the hex SHA-256 of the full key is stored (hashed at rest); the raw
//! key is shown exactly once at creation by the management API
//! (KAIROS-T-0059). Every failure mode — malformed key, unknown tenant, no
//! hash match, expired, revoked — returns the SAME 401, so the endpoint is not
//! a key/tenant-enumeration oracle.
//!
//! Unlike the SCIM path, a valid key resolves to a real user principal — the
//! service-account `public.users` row — so it produces an ordinary
//! [`AuthContext`] and flows through the normal tenant + ABAC stack unchanged
//! (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).

use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use kairos_db::models::{Organization, User};
use kairos_db::schema::{organizations, users};
use kairos_db::tenant::is_valid_tenant_slug;
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;

/// The fixed API-key prefix (distinguishes a key from a JWT at the bearer
/// boundary — a JWT never starts with this).
pub const KEY_PREFIX: &str = "kairos_sk_";

/// Length of the hex secret (32 random bytes → 64 hex chars).
pub const SECRET_LEN: usize = 64;

/// Lowercase hex-encode.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a fresh key string for `slug`: `kairos_sk_<slug>_<64-hex>` from 32
/// bytes of OS randomness (used by the management API, KAIROS-T-0059).
pub fn generate_key(slug: &str) -> String {
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    format!("{KEY_PREFIX}{slug}_{}", hex(&secret))
}

/// The value stored at rest and compared on every request: hex SHA-256 of the
/// FULL key string.
pub fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// A display-only prefix for listing keys: the identifying head plus the first
/// 6 hex chars of the secret (never enough to reconstruct it).
pub fn display_prefix(key: &str) -> String {
    match parse_key(key) {
        Some((slug, secret)) => format!("{KEY_PREFIX}{slug}_{}…", &secret[..6.min(secret.len())]),
        None => KEY_PREFIX.to_string(),
    }
}

/// Split a presented key into `(slug, secret)`, or `None` if it does not have
/// the `kairos_sk_<slug>_<64-hex>` shape (split at the LAST `_`).
pub fn parse_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix(KEY_PREFIX)?;
    let (slug, secret) = rest.rsplit_once('_')?;
    let secret_ok = secret.len() == SECRET_LEN
        && secret
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (secret_ok && is_valid_tenant_slug(slug)).then_some((slug, secret))
}

/// True if the bearer looks like an API key (so `require_auth` takes the
/// key path instead of OIDC). A `kairos_sk_` bearer is ALWAYS treated as a
/// key attempt — an invalid one 401s, it never falls through to OIDC.
pub fn is_api_key(token: &str) -> bool {
    token.starts_with(KEY_PREFIX)
}

/// The uniform 401 for every API-key authentication failure mode.
fn bad_key() -> ApiError {
    ApiError::unauthorized(
        "The API key is not correct, or it expired, or someone revoked it. Make your \
         own agent key with POST /api/me/agent-keys (`kairos keys create`). An \
         organization admin makes the key of a service account with \
         POST /api/service-accounts/{id}/keys.",
    )
}

/// The tenant an API key resolves to, inserted by [`require_auth`] so
/// `require_tenant` pins this tenant instead of resolving one from the `Host`
/// (KAIROS-A-0017: the key carries its own tenant). The org id goes with it
/// (KAIROS-T-0609), so the tenant layer does not read the org again.
#[derive(Debug, Clone)]
pub struct ApiKeyTenant {
    pub slug: String,
    pub org_id: Uuid,
}

/// How fresh a `last_used_at` stamp can be before a request writes it
/// again (KAIROS-T-0610): one minute. The stamp says "this key was in use
/// around then"; a request inside the window adds nothing to that.
pub const LAST_USED_WINDOW: chrono::Duration = chrono::Duration::minutes(1);

/// Authenticate an API-key bearer: parse it, resolve its embedded tenant, hash
/// it, look it up in that tenant's `api_keys`, check validity, and load the
/// owning service-account user. Returns the [`AuthContext`] for that principal
/// and the key's tenant. The key of a person (an agent key, KAIROS-T-0359)
/// sets [`AuthContext::agent_key`]. Every failure is the uniform [`bad_key`] 401.
///
/// A key that verifies goes into the auth cache (KAIROS-T-0609) under its
/// hash. The caller checks the cache first: a hit needs none of this.
pub async fn authenticate_api_key(
    state: &AppState,
    token: &str,
) -> Result<(AuthContext, ApiKeyTenant), ApiError> {
    let (slug, _secret) = parse_key(token).ok_or_else(bad_key)?;
    let slug = slug.to_string();

    // The tenant must exist before a schema-pinned connection can be checked
    // out. Unknown slug → the same 401 as a bad secret.
    let org_id = match state.auth_cache.org(&slug) {
        Some(id) => id,
        None => {
            let mut conn = state.pool.public_conn().await.map_err(ApiError::internal)?;
            let org: Option<Organization> = organizations::table
                .filter(organizations::slug.eq(&slug))
                .select(Organization::as_select())
                .first(&mut conn)
                .await
                .optional()
                .map_err(ApiError::internal)?;
            let org = org.ok_or_else(bad_key)?;
            state.auth_cache.put_org(slug.clone(), org.id);
            org.id
        }
    };

    // Hash lookup inside the tenant's own api_keys table, with the helpers
    // of kairos-db (KAIROS-T-0743), as the session path does (login.rs).
    let hash = hash_key(token);
    let lookup = hash.clone();
    let (row, user) = state
        .blocking
        .run(&slug, move |conn| {
            let row = kairos_db::api_keys::find_by_hash(conn, &lookup)
                .map_err(ApiError::internal)?
                .ok_or_else(bad_key)?;
            if !row.is_valid_at(Utc::now()) {
                return Err(bad_key());
            }

            // Load the service-account principal (public.users is
            // schema-qualified, so it resolves on the tenant-pinned
            // connection).
            let user: User = diesel::RunQueryDsl::first(
                users::table
                    .filter(users::id.eq(row.user_id))
                    .select(User::as_select()),
                conn,
            )
            .optional()
            .map_err(ApiError::internal)?
            .ok_or_else(bad_key)?;
            // KAIROS-T-0481: a system actor holds no key. A row that names
            // one is refused like any bad key.
            if user.is_system() {
                return Err(bad_key());
            }

            // Best-effort last-used stamp: never fail the request on it.
            // KAIROS-T-0610: written only when the last stamp is older than
            // the window, so a read does not write on every call.
            let _ =
                kairos_db::api_keys::touch_last_used(conn, row.id, Utc::now() - LAST_USED_WINDOW);
            Ok((row, user))
        })
        .await?;

    // KAIROS-T-0359: the key of a person is an agent key; it acts as the
    // person. The key of a service account is not.
    let agent_key = (!user.is_service_account()).then_some(row.id);
    // KAIROS-T-0536: the scope of the key. A service-account key has none.
    let key_scope = row.scope().map(std::sync::Arc::new);
    let auth = AuthContext {
        user_id: user.id,
        external_id: user.external_id,
        email: user.email,
        display_name: user.display_name,
        email_verified: false,
        agent_key,
        key_scope,
    };
    let tenant = ApiKeyTenant { slug, org_id };
    state.auth_cache.put_key(
        hash,
        crate::auth_cache::CachedKey {
            auth: auth.clone(),
            slug: tenant.slug.clone(),
            org_id,
            key_id: row.id,
            expires_at: row.expires_at,
        },
    );
    Ok((auth, tenant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trip_parses() {
        let key = generate_key("acme");
        assert!(key.starts_with("kairos_sk_acme_"));
        assert!(is_api_key(&key));
        let (slug, secret) = parse_key(&key).expect("round-trip");
        assert_eq!(slug, "acme");
        assert_eq!(secret.len(), SECRET_LEN);
    }

    #[test]
    fn slug_with_underscores_splits_at_last_separator() {
        let key = generate_key("acme_co_ltd");
        let (slug, _) = parse_key(&key).expect("underscore slug parses");
        assert_eq!(slug, "acme_co_ltd");
    }

    #[test]
    fn malformed_keys_are_rejected() {
        let good = generate_key("acme");
        for bad in [
            "",
            "kairos_sk_",
            "kairos_sk_acme",
            "not_a_key",
            "kairos_sk_acme_deadbeef", // secret too short
            &good.to_uppercase(),      // hex must be lowercase
            &format!("kairos_sk_ACME_{}", "a".repeat(64)), // invalid slug
            &format!("kairos_sk__{}", "a".repeat(64)), // empty slug
            &format!("kairos_sk_acme_{}", "g".repeat(64)), // non-hex secret
        ] {
            assert!(parse_key(bad).is_none(), "{bad:?} must not parse");
        }
        // A JWT-shaped bearer is not an API key.
        assert!(!is_api_key("eyJhbGciOiJSUzI1NiJ9.payload.sig"));
    }

    #[test]
    fn hash_is_stable_and_secret_free() {
        let key = generate_key("acme");
        let hash = hash_key(&key);
        assert_eq!(hash, hash_key(&key));
        assert_eq!(hash.len(), 64);
        assert!(!hash.contains("kairos_sk"));
        assert_ne!(hash, hash_key(&generate_key("acme")));
    }

    #[test]
    fn display_prefix_hides_the_secret() {
        let key = generate_key("acme");
        let shown = display_prefix(&key);
        assert!(shown.starts_with("kairos_sk_acme_"));
        assert!(shown.ends_with('…'));
        // The shown head (minus the ellipsis) is a real prefix of the key…
        let head = &shown[..shown.len() - '…'.len_utf8()];
        assert!(key.starts_with(head));
        // …but far too short to reconstruct the 64-hex secret.
        assert!(head.len() < key.len());
    }
}
