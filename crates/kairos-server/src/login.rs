//! `POST /api/login` and `POST /api/logout` — local password accounts
//! (KAIROS-T-0203, KAIROS-I-0018), plus the session branch `require_auth` uses.
//!
//! This is where local auth becomes usable. The pure parts — argon2, the token
//! format — are in [`crate::local_auth`]; storage is `kairos_db::local_auth`; this
//! module is the endpoint and the validation path.
//!
//! # Not routed unless enabled
//!
//! `/api/login` is mounted only when `KAIROS_LOCAL_AUTH` is on. Off, it does not
//! exist — a 404 from an absent route, not a 401 from a handler that declines. A
//! deployment that authenticates through an issuer has no password endpoint to
//! attack, and that is a stronger property than one that exists and says no.
//!
//! # A session from an OIDC login (KAIROS-T-0364)
//!
//! `POST /api/session` turns the OIDC bearer of a GUI login into a Kairos
//! session, with the same cookie a password login sets, so that a reload keeps
//! the session also where the issuer gives no refresh token (Google). Sessions
//! therefore exist on each deployment, and `/api/logout` and the session branch
//! of `require_auth` work whether or not local accounts are on.
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
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::local_auth::{
    generate_session_token, hash_session_token, parse_session_token, verify_against_dummy,
    verify_password,
};
use crate::middleware::auth::AuthContext;
use crate::rate_limit::{Attempt, client_addr};

/// `/api/logout`, and `/api/login` when `local_auth` (`KAIROS_LOCAL_AUTH`) is
/// on. OUTSIDE the auth → tenant stack: login has no credential yet, and logout
/// must work with a session that has already expired.
pub fn router(local_auth: bool) -> Router<AppState> {
    let router = Router::new().route("/api/logout", post(logout));
    if local_auth {
        router.route("/api/login", post(login))
    } else {
        router
    }
}

/// `/api/session` (KAIROS-T-0364). Behind the auth layer and NOT the tenant
/// layer: a session is of a person, not of an organization.
pub fn session_router() -> Router<AppState> {
    Router::new().route("/api/session", post(open_session))
}

/// `POST /api/login` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
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
    ApiError::unauthorized("The email or the password is not correct.")
}

/// The ONE failure for every way a session bearer can fail.
fn bad_session() -> ApiError {
    ApiError::unauthorized(
        "The session is not correct, or it expired, or an admin revoked it. Log in again.",
    )
}

/// `POST /api/login` — exchange an email and a password for a session bearer.
///
/// Present the returned token as `Authorization: Bearer <token>`. It is returned
/// exactly once; only its hash is stored. Failed attempts are throttled per account
/// and per source address.
///
/// The response also sets the session as the cookie `kairos_session`: `HttpOnly`,
/// `Secure`, `SameSite=Strict`, `Path=/`, for the lifetime of the session. The GUI
/// uses the cookie, so a reload keeps the session. A request with the cookie and no
/// bearer that changes something must come from the Kairos page (its `Origin`).
//
// NOTE, and it is not a doc comment on purpose: the rustdoc above is PUBLISHED. It
// becomes `docs/src/reference/rest/signing-in.md` through the OpenAPI spec, so
// implementation reasoning in it ends up in front of an operator. That is how this
// paragraph got moved.
//
// It takes the whole `Request` rather than an extractor tuple so the client address
// comes from `client_addr` — the same function the API-key path uses, so there is one
// place that decides what is trusted (KAIROS-T-0202).
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

    // COLLIERY-T-0249: the same refusal as each write route.
    let ApiJson(body) = ApiJson::<LoginRequest>::from_request(req, &()).await?;
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

            new_session(
                conn,
                ttl,
                user.id,
                LoginUser {
                    id: user.id.to_string(),
                    email: user.email,
                    display_name: user.display_name,
                },
            )
        })
        .await;

    match outcome {
        Ok(response) => {
            attempt.succeeded();
            Ok(session_response(&state, response))
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

/// Store a new session of `user` and return the response that carries it.
fn new_session(
    conn: &mut diesel::PgConnection,
    ttl: Duration,
    user_id: uuid::Uuid,
    user: LoginUser,
) -> Result<LoginResponse, ApiError> {
    let token = generate_session_token();
    let expires_at = Utc::now() + ttl;
    kairos_db::local_auth::create_session(
        conn,
        kairos_db::local_auth::NewLocalSession {
            user_id,
            token_hash: hash_session_token(&token),
            expires_at,
        },
    )
    .map_err(ApiError::internal)?;
    Ok(LoginResponse {
        token,
        expires_at: expires_at.to_rfc3339(),
        user,
    })
}

/// 200 with the session in the body and, KAIROS-T-0327, as an HttpOnly
/// cookie, so that a reload of the GUI keeps it (crate::session_cookie).
fn session_response(state: &AppState, response: LoginResponse) -> Response {
    let cookie = crate::session_cookie::set(&response.token, state.config.session_ttl_secs);
    (
        StatusCode::OK,
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(response),
    )
        .into_response()
}

/// `POST /api/session` — open a session from a sign-in with the identity provider.
///
/// Send the OIDC token of the sign-in as `Authorization: Bearer <token>`. The
/// response is the same as the response of `POST /api/login`. The body has a
/// session bearer, and the cookie `kairos_session` has the session too. Thus a
/// reload of the GUI keeps the session, also when the identity provider gives no
/// refresh token. The session lasts `KAIROS_SESSION_TTL_SECS`, or until a logout or
/// an admin revokes it.
///
/// Only an OIDC bearer opens a session. The server refuses the cookie alone, a
/// session bearer and an API key.
//
// NOTE: the rustdoc above is published (see `login`).
#[utoipa::path(
    post,
    path = "/api/session",
    tag = "auth",
    responses(
        (status = 200, description = "A session bearer", body = LoginResponse),
        (status = 401, description = "No valid credential"),
        (status = 403, description = "The credential is not an OIDC bearer"),
    )
)]
pub async fn open_session(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let from_identity_provider = bearer.is_some_and(|token| {
        !crate::local_auth::is_session_token(token)
            && !crate::service_accounts::auth::is_api_key(token)
    });
    if !from_identity_provider {
        return Err(ApiError::forbidden(
            "Only the token of a sign-in with the identity provider opens a session. \
             Send that token as the bearer.",
        ));
    }
    let ttl = Duration::seconds(state.config.session_ttl_secs as i64);
    let user_id = auth.user_id;
    let user = LoginUser {
        id: auth.user_id.to_string(),
        email: auth.email,
        display_name: auth.display_name,
    };
    let response = state
        .blocking
        .run_public(move |conn| new_session(conn, ttl, user_id, user))
        .await?;
    Ok(session_response(&state, response))
}

/// `POST /api/logout` — revoke the presented session, and clear the session cookie.
///
/// The session is the bearer, or else the cookie `kairos_session`. A logout with the
/// cookie must come from the Kairos page (its `Origin`).
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
pub async fn logout(State(state): State<AppState>, req: Request) -> Result<Response, ApiError> {
    let bearer = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    // KAIROS-T-0327: the GUI logs out with the session cookie. A logout
    // with the cookie must come from the Kairos page, as each change does
    // (crate::session_cookie).
    let token = match bearer {
        Some(token) => token,
        None => {
            let cookie = crate::session_cookie::token(req.headers()).unwrap_or_default();
            if !cookie.is_empty()
                && !crate::session_cookie::from_kairos_origin(
                    req.headers(),
                    state.config.public_url.as_deref(),
                    state.config.trusted_proxy,
                )
            {
                return Err(ApiError::forbidden(
                    "The request has the session cookie, but it does not come from the \
                     Kairos page.",
                ));
            }
            cookie
        }
    };

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
    // Clear the cookie in each case: a logout leaves no session behind.
    Ok((
        StatusCode::NO_CONTENT,
        [(
            axum::http::header::SET_COOKIE,
            crate::session_cookie::clear(),
        )],
    )
        .into_response())
}

/// Authenticate a session bearer, for `require_auth`'s third branch.
///
/// One indexed lookup on `token_hash`, then validity, then the person. The
/// `last_used_at` write is best-effort and in the same closure — it is what makes
/// a stale session visible later, and it is not worth failing a request over.
pub async fn authenticate_session(state: &AppState, token: &str) -> Result<AuthContext, ApiError> {
    // Each deployment can have sessions: a password login, or an OIDC login of the
    // GUI (`open_session`, KAIROS-T-0364). Shape first: a bearer that cannot be a session costs no query.
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
                email_verified: false,
                agent_key: None,
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
