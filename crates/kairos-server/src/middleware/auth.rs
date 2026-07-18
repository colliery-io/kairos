//! OIDC bearer-token authentication (KAIROS-A-0010, KAIROS-T-0017).
//!
//! Local JWT validation only — no per-request round-trips to the IdP: at
//! startup the issuer's discovery document is fetched once to find the JWKS
//! URI, keys are cached by `kid`, and the cache is refreshed when a token
//! arrives with an unknown `kid` (key rotation) behind a stampede guard so
//! concurrent unknown-kid requests trigger a single fetch.
//!
//! The [`require_auth`] middleware validates RS256 signature, `iss`, `exp`,
//! and `aud` (against `OIDC_AUDIENCE`), JIT-upserts `public.users` from the
//! claims (`sub` → `external_id`, `email`, `name` → `display_name`; org
//! membership is NEVER granted here), and inserts an [`AuthContext`]
//! request extension. Every failure is a 401 with the S-0005 envelope.
//!
//! # Audience note (A-0010)
//!
//! Dex (the dev/test issuer) sets `aud` to the requesting OAuth client's
//! id, so a dev deployment configures `OIDC_AUDIENCE` to the client id
//! whose tokens the API accepts (`kairos-cli` for the compose stack).
//! Production Keycloak adds a deployment-wide audience via a client-scope
//! mapper, so all first-party clients share one configured audience there.

use std::collections::HashMap;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::RunQueryDsl;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use kairos_db::TenantPool;
use kairos_db::models::{NewUser, User};
use kairos_db::schema::users;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

/// The authenticated caller, inserted as a request extension for every
/// request that passes [`require_auth`] (KAIROS-A-0010).
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// `public.users.id` (JIT-provisioned on first sight of the subject).
    pub user_id: Uuid,
    /// The OIDC `sub` claim (`public.users.external_id`).
    pub external_id: String,
    /// The `email` claim.
    pub email: String,
    /// The `name` claim (falls back to email when absent).
    pub display_name: String,
}

/// The token claims this crate consumes. `iss`/`aud`/`exp` are enforced by
/// [`jsonwebtoken`]'s validation, not read from here.
#[derive(Debug, serde::Deserialize)]
pub struct TokenClaims {
    /// OIDC subject — the stable external identity.
    pub sub: String,
    /// Email (requires the `email` scope; required for JIT provisioning).
    pub email: Option<String>,
    /// Display name (`profile` scope).
    pub name: Option<String>,
}

/// Why building an [`Authenticator`] failed (startup-time, fail-fast).
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// The discovery document or JWKS could not be fetched/parsed.
    #[error("OIDC discovery against {issuer} failed: {message}")]
    Discovery {
        /// The configured issuer URL.
        issuer: String,
        /// What went wrong.
        message: String,
    },
}

/// Why a token was rejected (or could not be checked).
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// No `Authorization: Bearer <token>` header.
    #[error("missing bearer token")]
    MissingToken,
    /// The token header has no `kid` or names a key the issuer does not
    /// publish (even after a JWKS refresh).
    #[error("token signed with unknown key {kid:?}")]
    UnknownKey {
        /// The offending key id (empty when the header had none).
        kid: String,
    },
    /// Signature/`iss`/`aud`/`exp` validation failed.
    #[error("token validation failed: {0}")]
    Invalid(#[from] jsonwebtoken::errors::Error),
    /// The token is valid but has no usable `email` claim.
    #[error("token has no email claim (request the openid+email scopes)")]
    MissingEmail,
    /// Refreshing the JWKS failed — a server-side problem, mapped to 500
    /// rather than 401.
    #[error("JWKS refresh failed: {0}")]
    KeyFetch(String),
}

impl From<VerifyError> for ApiError {
    fn from(err: VerifyError) -> Self {
        match err {
            VerifyError::KeyFetch(_) => ApiError::internal(err),
            other => ApiError::unauthorized(other.to_string()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct DiscoveryDoc {
    jwks_uri: String,
}

#[derive(Debug, serde::Deserialize)]
struct JwksDoc {
    keys: Vec<Jwk>,
}

#[derive(Debug, serde::Deserialize)]
struct Jwk {
    kty: String,
    kid: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

/// Validates bearer tokens against one OIDC issuer: JWKS cache keyed by
/// `kid`, refreshed on unknown `kid` behind a stampede guard.
pub struct Authenticator {
    issuer: String,
    audience: String,
    /// `None` for test instances built with [`Self::with_static_keys`]
    /// (no refresh possible).
    jwks_uri: Option<String>,
    http: reqwest::Client,
    keys: RwLock<HashMap<String, DecodingKey>>,
    /// Stampede guard: concurrent unknown-kid requests serialize here and
    /// re-check the cache before fetching.
    refresh: Mutex<()>,
}

impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("jwks_uri", &self.jwks_uri)
            .finish_non_exhaustive()
    }
}

impl Authenticator {
    /// Resolve the issuer's discovery document, prime the JWKS cache, and
    /// return a ready validator. Fails fast (startup) if the issuer is
    /// unreachable or publishes no usable RSA keys.
    pub async fn discover(issuer: &str, audience: &str) -> Result<Self, DiscoveryError> {
        let issuer = issuer.trim_end_matches('/').to_string();
        let http = reqwest::Client::new();
        let discovery_url = format!("{issuer}/.well-known/openid-configuration");
        let err = |message: String| DiscoveryError::Discovery {
            issuer: issuer.clone(),
            message,
        };

        let doc: DiscoveryDoc = http
            .get(&discovery_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| err(format!("fetching {discovery_url}: {e}")))?
            .json()
            .await
            .map_err(|e| err(format!("parsing discovery document: {e}")))?;

        let auth = Self {
            issuer,
            audience: audience.to_string(),
            jwks_uri: Some(doc.jwks_uri.clone()),
            http,
            keys: RwLock::new(HashMap::new()),
            refresh: Mutex::new(()),
        };
        auth.refresh_keys()
            .await
            .map_err(|e| DiscoveryError::Discovery {
                issuer: auth.issuer.clone(),
                message: e.to_string(),
            })?;
        Ok(auth)
    }

    /// Test constructor: fixed keys, no JWKS endpoint (refresh-on-unknown-
    /// kid becomes a cache miss). Lets unit tests mint tokens with a local
    /// RSA key and exercise every validation branch offline.
    pub fn with_static_keys(
        issuer: &str,
        audience: &str,
        keys: impl IntoIterator<Item = (String, DecodingKey)>,
    ) -> Self {
        Self {
            issuer: issuer.trim_end_matches('/').to_string(),
            audience: audience.to_string(),
            jwks_uri: None,
            http: reqwest::Client::new(),
            keys: RwLock::new(keys.into_iter().collect()),
            refresh: Mutex::new(()),
        }
    }

    /// Fetch the JWKS and replace the cache with its RSA keys.
    async fn refresh_keys(&self) -> Result<(), VerifyError> {
        let Some(jwks_uri) = &self.jwks_uri else {
            return Ok(()); // static-key instance: nothing to refresh
        };
        let doc: JwksDoc = self
            .http
            .get(jwks_uri)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| VerifyError::KeyFetch(format!("fetching {jwks_uri}: {e}")))?
            .json()
            .await
            .map_err(|e| VerifyError::KeyFetch(format!("parsing JWKS: {e}")))?;

        let mut fresh = HashMap::new();
        for key in doc.keys {
            let (Some(kid), Some(n), Some(e)) = (key.kid, key.n, key.e) else {
                continue;
            };
            if key.kty != "RSA" {
                continue;
            }
            match DecodingKey::from_rsa_components(&n, &e) {
                Ok(decoding_key) => {
                    fresh.insert(kid, decoding_key);
                }
                Err(e) => {
                    tracing::warn!(kid, error = %e, "skipping unparseable JWKS key");
                }
            }
        }
        if fresh.is_empty() {
            return Err(VerifyError::KeyFetch(format!(
                "JWKS at {jwks_uri} contains no usable RSA keys"
            )));
        }
        *self.keys.write().await = fresh;
        Ok(())
    }

    /// The decoding key for `kid`, refreshing the JWKS once (behind the
    /// stampede guard) if it is not cached.
    async fn key_for(&self, kid: &str) -> Result<Option<DecodingKey>, VerifyError> {
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(Some(key.clone()));
        }
        if self.jwks_uri.is_none() {
            return Ok(None);
        }
        let _guard = self.refresh.lock().await;
        // Re-check under the guard: a concurrent request may have already
        // refreshed while we waited.
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(Some(key.clone()));
        }
        self.refresh_keys().await?;
        Ok(self.keys.read().await.get(kid).cloned())
    }

    /// Validate `token` (RS256 signature, `iss`, `aud`, `exp`) and return
    /// its claims.
    pub async fn verify(&self, token: &str) -> Result<TokenClaims, VerifyError> {
        let header = decode_header(token)?;
        let kid = header.kid.unwrap_or_default();
        let key = self
            .key_for(&kid)
            .await?
            .ok_or(VerifyError::UnknownKey { kid })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        // `exp` is validated by default (with the default leeway).

        Ok(decode::<TokenClaims>(token, &key, &validation)?.claims)
    }
}

/// `Authorization: Bearer <token>` or [`VerifyError::MissingToken`].
fn bearer_token(req: &Request) -> Result<&str, VerifyError> {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(VerifyError::MissingToken)
}

/// JIT user provisioning (A-0010): upsert `public.users` keyed on
/// `external_id` = `sub`. No org membership is granted here, ever — that
/// is an explicit admin action on `organization_members`.
///
/// Fast path: when the row already matches the claims, this is one SELECT
/// and no write.
async fn jit_upsert_user(pool: &TenantPool, claims: &TokenClaims) -> Result<User, ApiError> {
    let email = claims
        .email
        .clone()
        .ok_or_else(|| ApiError::from(VerifyError::MissingEmail))?;
    let display_name = claims.name.clone().unwrap_or_else(|| email.clone());

    let mut conn = pool.public_conn().await.map_err(ApiError::internal)?;

    let existing: Option<User> = users::table
        .filter(users::external_id.eq(&claims.sub))
        .select(User::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(ApiError::internal)?;
    if let Some(user) = existing
        && user.email == email
        && user.display_name == display_name
    {
        return Ok(user);
    }

    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: claims.sub.clone(),
            email,
            display_name,
        })
        .on_conflict(users::external_id)
        .do_update()
        .set((
            users::email.eq(excluded(users::email)),
            users::display_name.eq(excluded(users::display_name)),
            users::updated_at.eq(diesel::dsl::now),
        ))
        .returning(User::as_returning())
        .get_result(&mut conn)
        .await
        .map_err(ApiError::internal)
}

/// The auth layer: validate the bearer token, JIT-upsert the user, insert
/// [`AuthContext`]. Runs BEFORE tenant resolution (A-0010).
///
/// Two credential kinds are accepted (KAIROS-A-0017): an OIDC JWT (human,
/// validated via JWKS + JIT-provisioned) or a `kairos_sk_…` service-account
/// **API key**. A `kairos_sk_` bearer always takes the key path — an invalid
/// one is a uniform 401, it never falls through to OIDC — and pins its own
/// tenant for [`super::tenant::require_tenant`] via [`ApiKeyTenant`]. Both
/// kinds produce the same [`AuthContext`], so everything downstream (tenant,
/// ABAC, MCP) is identical.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = bearer_token(&req)?.to_string();

    // Service-account API key (KAIROS-T-0058): resolves to a user principal +
    // its tenant, with the same AuthContext shape as the OIDC path.
    if crate::service_accounts::auth::is_api_key(&token) {
        let (auth, slug) =
            crate::service_accounts::auth::authenticate_api_key(&state, &token).await?;
        req.extensions_mut().insert(auth);
        req.extensions_mut()
            .insert(crate::service_accounts::auth::ApiKeyTenant(slug));
        return Ok(next.run(req).await);
    }

    let claims = state.auth.verify(&token).await?;
    let user = jit_upsert_user(&state.pool, &claims).await?;

    req.extensions_mut().insert(AuthContext {
        user_id: user.id,
        external_id: user.external_id,
        email: user.email,
        display_name: user.display_name,
    });
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    //! Offline validation-branch tests: mint RS256 tokens with a throwaway
    //! test-only RSA key (generated for this test file, never used
    //! anywhere else) and check each rejection path. The live-Dex
    //! integration test (`tests/middleware.rs`) covers the JWKS/discovery
    //! path with real issuer keys.

    use jsonwebtoken::{EncodingKey, Header, encode};

    use super::*;

    /// Throwaway RSA keypair for minting test tokens. NOT a secret — it
    /// exists only so unit tests can exercise expiry/audience/issuer
    /// rejection without an IdP.
    const TEST_RSA_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQDaApB1M6cw9PzP
IrNuPJ2XeX2G0Ue+jT1qRlNhEI0ZX/HQes4fbs6JVty8v/Vk4oi/rdt+CcZjEqan
vgw282qDor4etOXmdxxOCwJo39MvohJ/fFqwgIHPjnVfCMzyp7NJ6TXPAbNbQ7ML
uXvFxDO6ke4X1WGrOlj0uCpXz7YpDL/yvAfn6vefiV7SDO4NmBu8Ef8YWNYXnksm
uYwm+3rWJJ0ufuYSrhjAXVSRFqOn+xHWJpoOdMm5HdHZLjqIPcMUxutk+DK76wLi
pxa6sRAFYqJPOS33WB7YdLyONhnI9HG6iNKqVAxuGVwlZhcRaN8RweCjVnwjD/hP
FTugPOPpAgMBAAECggEAC8afrvT/TvGsxjOFpaq4iHoTgbjEO1K9woPR9ShDtt8r
3KsFf0Uo+toqSjfENZLW+COX+5LjmG5leiIV3tH/KuUbh+UVlgFREhYeJzQP4D7M
6P36mBYY7PEw/dUn3OOaF5/1PB1HZuKdRUDboq8abDV9uuPXxrv8GhvojZ22pTjl
e5KPEI5O8p93PfsJMr9UXS49kWjtyfRCR+9lvEBUp8BD3bWJJmesKh92nG10Xydg
kvxWlMYfxCquhjqbP4aJThzH9rp8ZdrfTQFxFab8tEiUzflyGxi6a7pbZF0Jfq7M
r1ZqXiY1Oz7Bru/Z2q1ovEhEM459G+Gt52uHZzkgAQKBgQD/lEqSguoFgdzS7mhc
31nVmXXts1nLtsmv2jrxON0uRy+22ItVv3G8yEZdWFBz/SFDe0gfY7kwazDHF4il
cF9oqlKVFKJALJYcos6NOGrHmOTsQh/brq3HGpvHu3rAw5ElRxYTZFY4urRz9Uuf
hHehDwaeJyGrTCQlMFgDWKoq6QKBgQDaXnCwd/708PPcM1C9Ty5tukkjpL5NkZF6
4xSc+vxjsZoZGoz0/mvm+doquAvXZbwICYT7mVNsl4L56cXfkc9AVcQGOQRwE5Qq
PFIqsf+IHW25DFuOs30bipdbUNIQuZQ68MPoTurHvFy9ixxvRK3ixQuj+v/VAdHz
cDVZoqdRAQKBgEfId7V0zZPkaIhZ67gCB3JF2uh7UkI0QauBiMKNrRm9ZrpdUa0w
yxoxygmXr2kUdI5GhvhCAxaFVLrmcju9Nx7nj7BNjlCl9TdvxsHFUcBjwhBVdis2
gZqFb7GGh7CyfQbSU3H44Xqnfd1/zNCt3QfAd6Rd60f4Z8KGNIIkGg9ZAoGAbPno
fwjDYfXFnUS2rGMRpozq5zDWD9vvoEYnCVhwEEiXwKNxaOp3auORrvP+ZNZOiixG
A1G3QmAyawnxR+t6ZH7ovrpBrrT2okVMNCZ0gbc+BLVYE9UbQF1fv6CL1PDoOqng
+tQ3cspb9fOwkw7RQHRZpNIkTmcEIdsDpOH5YgECgYAZbx25qX7MvEwssMmTg67y
0METTxWaHyZRLrg8X4odU9MJjG37IORflfIl3JS4iYUFxC8D8+rd4oemf46DCBW5
646V0NUC3fgJXsnmEmvxnHhtFvaPpjeJs3iQD0ClrsvXdhxMmR93/DmMPj4hKvHR
vJCSNfFyiVNn48WmuL9w2w==
-----END PRIVATE KEY-----";

    const TEST_RSA_PUBLIC_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA2gKQdTOnMPT8zyKzbjyd
l3l9htFHvo09akZTYRCNGV/x0HrOH27OiVbcvL/1ZOKIv63bfgnGYxKmp74MNvNq
g6K+HrTl5nccTgsCaN/TL6ISf3xasICBz451XwjM8qezSek1zwGzW0OzC7l7xcQz
upHuF9VhqzpY9LgqV8+2KQy/8rwH5+r3n4le0gzuDZgbvBH/GFjWF55LJrmMJvt6
1iSdLn7mEq4YwF1UkRajp/sR1iaaDnTJuR3R2S46iD3DFMbrZPgyu+sC4qcWurEQ
BWKiTzkt91ge2HS8jjYZyPRxuojSqlQMbhlcJWYXEWjfEcHgo1Z8Iw/4TxU7oDzj
6QIDAQAB
-----END PUBLIC KEY-----";

    const ISSUER: &str = "http://issuer.test";
    const AUDIENCE: &str = "kairos-test";
    const KID: &str = "test-key";

    fn authenticator() -> Authenticator {
        let key = DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_PEM.as_bytes()).expect("public pem");
        Authenticator::with_static_keys(ISSUER, AUDIENCE, [(KID.to_string(), key)])
    }

    #[derive(serde::Serialize)]
    struct MintClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        sub: &'a str,
        exp: i64,
        email: &'a str,
        name: &'a str,
    }

    fn mint(iss: &str, aud: &str, exp: i64, kid: Option<&str>) -> String {
        let claims = MintClaims {
            iss,
            aud,
            sub: "test-subject",
            exp,
            email: "test@kairos.test",
            name: "Test User",
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = kid.map(str::to_string);
        let key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_PEM.as_bytes()).expect("private pem");
        encode(&header, &claims, &key).expect("minting token")
    }

    fn future_exp() -> i64 {
        (chrono_now() + 3600) as i64
    }

    fn chrono_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs()
    }

    #[tokio::test]
    async fn valid_token_yields_claims() {
        let claims = authenticator()
            .verify(&mint(ISSUER, AUDIENCE, future_exp(), Some(KID)))
            .await
            .expect("valid token accepted");
        assert_eq!(claims.sub, "test-subject");
        assert_eq!(claims.email.as_deref(), Some("test@kairos.test"));
        assert_eq!(claims.name.as_deref(), Some("Test User"));
    }

    #[tokio::test]
    async fn expired_token_is_rejected() {
        let expired = (chrono_now() as i64) - 3600; // beyond default leeway
        let err = authenticator()
            .verify(&mint(ISSUER, AUDIENCE, expired, Some(KID)))
            .await
            .expect_err("expired token rejected");
        assert!(matches!(
            err,
            VerifyError::Invalid(ref e)
                if *e.kind() == jsonwebtoken::errors::ErrorKind::ExpiredSignature
        ));
    }

    #[tokio::test]
    async fn wrong_audience_is_rejected() {
        let err = authenticator()
            .verify(&mint(ISSUER, "some-other-client", future_exp(), Some(KID)))
            .await
            .expect_err("wrong-aud token rejected");
        assert!(matches!(
            err,
            VerifyError::Invalid(ref e)
                if *e.kind() == jsonwebtoken::errors::ErrorKind::InvalidAudience
        ));
    }

    #[tokio::test]
    async fn wrong_issuer_is_rejected() {
        let err = authenticator()
            .verify(&mint("http://evil.test", AUDIENCE, future_exp(), Some(KID)))
            .await
            .expect_err("wrong-iss token rejected");
        assert!(matches!(
            err,
            VerifyError::Invalid(ref e)
                if *e.kind() == jsonwebtoken::errors::ErrorKind::InvalidIssuer
        ));
    }

    #[tokio::test]
    async fn unknown_kid_is_rejected() {
        let err = authenticator()
            .verify(&mint(ISSUER, AUDIENCE, future_exp(), Some("rotated-away")))
            .await
            .expect_err("unknown-kid token rejected");
        assert!(matches!(err, VerifyError::UnknownKey { ref kid } if kid == "rotated-away"));
    }

    #[tokio::test]
    async fn garbage_token_is_rejected() {
        let err = authenticator()
            .verify("not.a.jwt")
            .await
            .expect_err("garbage rejected");
        assert!(matches!(err, VerifyError::Invalid(_)));
    }

    /// KAIROS-T-0054: the middleware is issuer/token-kind agnostic — it
    /// validates ANY RS256 JWT carrying the configured `iss`/`aud`. A
    /// Google-shaped **ID token** (`iss = https://accounts.google.com`,
    /// `aud = <client id>`) therefore validates, while Google's **opaque
    /// access token** (`ya29.…`, not a JWT) is rejected. This is exactly why
    /// `KAIROS_API_BEARER=id_token` makes the GUI/CLI send the id_token: the
    /// server needs no Google-specific code, only a validatable JWT.
    #[tokio::test]
    async fn google_shaped_id_token_validates_and_opaque_access_token_is_rejected() {
        const GOOGLE_ISSUER: &str = "https://accounts.google.com";
        const GOOGLE_CLIENT_ID: &str = "123456789.apps.googleusercontent.com";
        let key = DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_PEM.as_bytes()).expect("public pem");
        let auth = Authenticator::with_static_keys(
            GOOGLE_ISSUER,
            GOOGLE_CLIENT_ID,
            [(KID.to_string(), key)],
        );

        // A Google ID token is a signed RS256 JWT — it validates.
        let id_token = mint(GOOGLE_ISSUER, GOOGLE_CLIENT_ID, future_exp(), Some(KID));
        let claims = auth.verify(&id_token).await.expect("id token accepted");
        assert_eq!(claims.email.as_deref(), Some("test@kairos.test"));

        // A Google access token is opaque (`ya29.…`), not a JWT — rejected.
        let opaque = "ya29.a0AfB_byC3xampleOpaqueAccessTokenNotAJwt";
        let err = auth
            .verify(opaque)
            .await
            .expect_err("opaque access token rejected");
        assert!(matches!(err, VerifyError::Invalid(_)));
    }

    #[test]
    fn verify_errors_map_to_401_envelope() {
        let api: ApiError = VerifyError::MissingToken.into();
        assert_eq!(api.status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(api.code, "UNAUTHORIZED");
    }
}
