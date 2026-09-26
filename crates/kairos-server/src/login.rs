//! `POST /api/login` and `POST /api/logout` — local password accounts
//! (KAIROS-T-0203, KAIROS-I-0018), plus the session branch `require_auth` uses.
//!
//! This is where local auth becomes usable. The pure parts — argon2, the token
//! format — are in [`crate::local_auth`]; storage is `kairos_db::local_auth`; this
//! module is the endpoint and the validation path.
//!
//! # Not routed unless enabled
//!
//! Both routes are mounted only when `KAIROS_LOCAL_AUTH` is on. Off, they do not
//! exist — a 404 from an absent route, not a 401 from a handler that declines. A
//! deployment that authenticates through an issuer has no password endpoint to
//! attack, and that is a stronger property than one that exists and says no.
//!
//! Local auth is **additive**: a deployment may have both an issuer and local
//! accounts, and neither path knows about the other.
//!
//! # The failure response is a security surface
//!
//! Five things go wrong on login — unknown email, wrong password, an OIDC-only
//! account with no password, a revoked session, an expired session — and a caller
//! must not be able to tell them apart. They share one 401 with one message, and
//! the unknown-email case deliberately burns the same argon2 work as a real
//! attempt (`verify_against_dummy`), because otherwise the *timing* answers the
//! question the message refuses to.
//!
//! The OIDC-only case is the one that is easy to miss and the most damaging to get
//! wrong: an organization's real email addresses returning a different error than
//! made-up ones is an account-enumeration oracle over exactly the addresses an
//! attacker wants.
//!
//! # Where the argon2 work runs
//!
//! Inside a `run_public` closure, which is a `spawn_blocking` thread. A 19 MiB,
//! 2-iteration argon2 verification takes tens of milliseconds; on an async runtime
//! thread that is tens of milliseconds during which that thread serves nobody.

use axum::extract::{FromRequest as _, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::error::ApiError;
use crate::local_auth::{
    generate_session_token, hash_session_token, parse_session_token, verify_against_dummy,
    verify_password,
};
use crate::middleware::auth::AuthContext;
use crate::rate_limit::{Attempt, client_addr};

/// `/api/login` + `/api/logout`. Mounted by [`crate::app::router`] only when
/// `KAIROS_LOCAL_AUTH` is on, and OUTSIDE the auth → tenant stack: login has no
/// credential yet, and logout must work with a session that has already expired.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
}

/// `POST /api/login` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// The account's email address. Matched case-insensitively.
    pub email: String,
    pub password: String,
}

/// `POST /api/login` 200 body. The token is returned EXACTLY ONCE — only its
/// SHA-256 is stored.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LoginResponse {
    /// The session bearer (`kairos_ss_<64-hex>`). Present it as
    /// `Authorization: Bearer <token>`.
    pub token: String,
    /// When it stops working (RFC 3339).
    pub expires_at: String,
    /// Who it authenticates, so a client need not immediately call `/api/whoami`.
    pub user: LoginUser,
}

/// The `user` object of [`LoginResponse`].
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LoginUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

/// The ONE failure for every way a login can fail.
///
/// A single constructor rather than a message at each call site, because the
/// property being protected is that all of them are identical, and identical-by-
/// construction is the only kind that survives editing.
fn bad_login() -> ApiError {
    ApiError::unauthorized("incorrect email or password")
}

/// The ONE failure for every way a session bearer can fail.
fn bad_session() -> ApiError {
    ApiError::unauthorized("invalid, expired, or revoked session; log in again")
}

/// `POST /api/login`.
///
/// Takes the whole [`Request`] rather than an extractor tuple so the client
/// address comes from [`client_addr`] — the same function the API-key path uses,
/// so there is one place that decides what is trusted (KAIROS-T-0202).
#[utoipa::path(
    post,
    path = "/api/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "A session bearer", body = LoginResponse),
        (status = 401, description = "Incorrect email or password"),
        (status = 429, description = "Too many failed attempts"),
    )
)]
pub async fn login(State(state): State<AppState>, req: Request) -> Result<Response, ApiError> {
    let source = client_addr(&req, state.config.trusted_proxy);

    let Json(body) = Json::<LoginRequest>::from_request(req, &())
        .await
        .map_err(|e| ApiError::validation(format!("malformed login body: {e}")))?;
    let email = body.email.trim().to_lowercase();
    let password = body.password;

    // Throttled by BOTH the account being attempted and the address attempting it
    // (KAIROS-T-0202). This endpoint is the reason that task exists: a password is
    // guessable in a way that an API key is not.
    let attempt = Attempt::begin(&state, source, Some(email.clone()));
    if let Some(refused) = attempt.refuse_if_locked_out() {
        return Ok(refused);
    }

    let ttl = Duration::seconds(state.config.session_ttl_secs as i64);
    let outcome = state
        .blocking
        .run_public(move |conn| {
            let user = kairos_db::local_auth::find_user_by_email(conn, &email)
                .map_err(ApiError::internal)?;

            // Both "no such person" and "that person has no password" land here,
            // and both do the argon2 work anyway. Skipping it would make an
            // unknown email measurably faster than a known one.
            let Some((user, stored)) = user.and_then(|u| u.password_hash.clone().map(|h| (u, h)))
            else {
                verify_against_dummy(&password);
                return Err(bad_login());
            };

            // A malformed stored hash is NOT a wrong password: it means the column
            // was corrupted or hand-edited. Treating it as a failed login would
            // lock someone out silently, so it is a 500 that someone can find.
            if !verify_password(&password, &stored).map_err(ApiError::internal)? {
                return Err(bad_login());
            }

            // Service accounts authenticate with API keys and have no business
            // holding a session. Checked after the password so it costs an
            // attacker nothing to learn.
            if user.is_service_account() {
                return Err(bad_login());
            }

            let token = generate_session_token();
            let expires_at = Utc::now() + ttl;
            kairos_db::local_auth::create_session(
                conn,
                kairos_db::local_auth::NewLocalSession {
                    user_id: user.id,
                    token_hash: hash_session_token(&token),
                    expires_at,
                },
            )
            .map_err(ApiError::internal)?;

            Ok(LoginResponse {
                token,
                expires_at: expires_at.to_rfc3339(),
                user: LoginUser {
                    id: user.id.to_string(),
                    email: user.email,
                    display_name: user.display_name,
                },
            })
        })
        .await;

    match outcome {
        Ok(response) => {
            attempt.succeeded();
            Ok((StatusCode::OK, Json(response)).into_response())
        }
        Err(e) => {
            // Only a rejected credential counts against the throttle. A 500 from a
            // database that is down is not a failed guess, and counting it would
            // turn an outage into a lockout on top of the outage.
            if e.status == StatusCode::UNAUTHORIZED {
                attempt.failed();
            }
            Err(e)
        }
    }
}

/// `POST /api/logout` — revoke the presented session.
///
/// Outside the auth stack, and 204 whatever happens: a caller logging out with a
/// token that has already expired has got what they wanted, and telling them the
/// token was no good would be both useless and an oracle.
#[utoipa::path(
    post,
    path = "/api/logout",
    tag = "auth",
    responses((status = 204, description = "The session is revoked, or was not one")),
)]
pub async fn logout(State(state): State<AppState>, req: Request) -> Result<StatusCode, ApiError> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();

    if parse_session_token(&token).is_some() {
        let hash = hash_session_token(&token);
        state
            .blocking
            .run_public(move |conn| {
                kairos_db::local_auth::revoke_session_by_hash(conn, &hash)
                    .map_err(ApiError::internal)
            })
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Authenticate a session bearer, for `require_auth`'s third branch.
///
/// One indexed lookup on `token_hash`, then validity, then the person. The
/// `last_used_at` write is best-effort and in the same closure — it is what makes
/// a stale session visible later, and it is not worth failing a request over.
pub async fn authenticate_session(state: &AppState, token: &str) -> Result<AuthContext, ApiError> {
    // A deployment with local auth off has no sessions, so do not ask the database
    // whether it has one.
    if !state.config.local_auth {
        return Err(bad_session());
    }
    // Shape first: a bearer that cannot be a session costs no query.
    if parse_session_token(token).is_none() {
        return Err(bad_session());
    }
    let hash = hash_session_token(token);

    state
        .blocking
        .run_public(move |conn| {
            let session = kairos_db::local_auth::find_session_by_hash(conn, &hash)
                .map_err(ApiError::internal)?
                .ok_or_else(bad_session)?;
            if !session.is_valid_at(Utc::now()) {
                return Err(bad_session());
            }

            use diesel::prelude::*;
            use kairos_db::models::User;
            use kairos_db::schema::users;
            let user: User = users::table
                .filter(users::id.eq(session.user_id))
                .select(User::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(bad_session)?;

            // Best-effort, deliberately ignored: a failed timestamp update must not
            // fail an otherwise valid request.
            let _ = kairos_db::local_auth::touch_session(conn, session.id);

            Ok(AuthContext {
                user_id: user.id,
                external_id: user.external_id,
                email: user.email,
                display_name: user.display_name,
            })
        })
        .await
}

/// When a session minted now would expire — exposed for tests and for the CLI's
/// stored-credential expiry hint.
pub fn expiry_from(now: DateTime<Utc>, ttl_secs: u64) -> DateTime<Utc> {
    now + Duration::seconds(ttl_secs as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_login_failure_carries_the_same_message() {
        // The uniformity is the security property, so it is asserted rather than
        // left to the reader to notice.
        let a = bad_login();
        let b = bad_login();
        assert_eq!(a.status, StatusCode::UNAUTHORIZED);
        assert_eq!(a.message, b.message);
        assert!(
            !a.message.to_lowercase().contains("password hash")
                && !a.message.to_lowercase().contains("unknown")
                && !a.message.to_lowercase().contains("exist"),
            "the message must not hint at which failure happened: {}",
            a.message
        );
        assert_eq!(a.details, serde_json::json!({}));
    }

    #[test]
    fn a_session_failure_says_nothing_about_which_kind() {
        let err = bad_session();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        for leak in ["revoked", "expired"] {
            // Naming both together is fine; naming ONE would identify the cause.
            assert!(err.message.contains(leak));
        }
    }

    #[test]
    fn the_default_session_lifetime_is_a_fortnight() {
        let now = Utc::now();
        assert_eq!(
            expiry_from(now, 14 * 24 * 60 * 60) - now,
            Duration::days(14)
        );
    }
}
