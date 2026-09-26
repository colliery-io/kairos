//! GUI serving + SPA auth support (KAIROS-T-0039, per A-0015/A-0013).
//!
//! Three things live here, everything the `kairos-web` SPA needs from its
//! own origin:
//!
//! 1. **`GET /api/config`** (public, no auth): the SPA's discovery
//!    endpoint — issuer, the GUI's OAuth client id
//!    (`KAIROS_WEB_CLIENT_ID`), and the issuer's `authorization_endpoint`
//!    (resolved from OIDC discovery server-side, cached). Public by
//!    design: it is exactly the information a login page needs before
//!    any token exists. Decision (T-0039): a config endpoint over
//!    build-time env, so one wasm bundle works against any deployment.
//!
//! 2. **`POST /api/auth/token`** (public): a same-origin *relay* to the
//!    issuer's token endpoint for the PKCE code exchange and the
//!    refresh-token grant. Browsers can only call token endpoints that
//!    serve CORS — many IdPs (the dev Dex included) do not — so the SPA
//!    posts here and the server forwards to the real token endpoint.
//!    The relay forwards only whitelisted grant parameters, always
//!    injects the configured client id, and only ever talks to the
//!    configured issuer. For public clients (Dex/Keycloak) it holds no
//!    secret and PKCE protects the exchange end-to-end; for a *confidential*
//!    client it also injects `KAIROS_WEB_CLIENT_SECRET` server-side
//!    (KAIROS-T-0056) — required by Google's "Web application" clients, and
//!    kept out of the browser precisely because the exchange is server-side.
//!    This is an *auth-event* path (login/refresh), not per-request
//!    validation — A-0010's "no IdP round-trips on the request path" holds.
//!
//! 3. **The SPA fallback** ([`spa_fallback`], mounted as the router
//!    fallback): serves the built GUI at `/` with SPA semantics — real
//!    asset paths get the asset, anything else (a client-side route like
//!    `/boards/x`) gets `index.html`. API surfaces (`/api`, `/mcp`,
//!    `/scim`, `/ws`, health/metrics, `/.well-known`) are RESERVED and
//!    404 with the S-0005 envelope instead of falling back. Assets come
//!    from, in order:
//!    - `KAIROS_WEB_DIST` (dev: point it at `crates/kairos-web/dist`),
//!    - the binary itself when built with the `embed-web` feature
//!      (release path, A-0013 single artifact),
//!    - otherwise a plain placeholder page saying how to build the GUI
//!      (keeps `cargo build`/`cargo run` fully functional with zero wasm
//!      tooling).

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Extension, Form, State};
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::app::AppState;
use crate::error::ApiError;

/// Path prefixes that belong to API surfaces: the SPA fallback never
/// swallows them (an unknown path under these is a real 404).
const RESERVED_PREFIXES: &[&str] = &[
    "/api",
    "/mcp",
    "/scim",
    "/ws",
    "/healthz",
    "/readyz",
    "/metrics",
    "/.well-known",
];

/// Issuer endpoints the SPA flow needs, resolved lazily from OIDC
/// discovery and cached for the process lifetime.
#[derive(Clone, Debug, Deserialize)]
struct IdpEndpoints {
    authorization_endpoint: String,
    token_endpoint: String,
}

/// Shared context for the two auth routes: issuer + client id from
/// config, one HTTP client, and the discovery cache. Self-contained (an
/// `Extension`, not part of [`AppState`]) so in-process test routers
/// built via `state_with` need no live issuer until a route is hit.
struct WebAuth {
    /// `None` when this deployment has no issuer (KAIROS-T-0208).
    issuer: Option<String>,
    client_id: String,
    /// Which token the SPA should present as the `/api` bearer (T-0054).
    api_bearer: crate::config::ApiBearer,
    /// OAuth client secret for a confidential GUI client (KAIROS-T-0056).
    /// `None` for public clients (Dex/Keycloak). When set, the relay presents
    /// it on the token exchange — required by Google's "Web application"
    /// clients. Server-side only; never reaches the browser.
    client_secret: Option<String>,
    http: reqwest::Client,
    endpoints: OnceCell<IdpEndpoints>,
}

impl WebAuth {
    /// Discovery document fetch (`{issuer}/.well-known/openid-configuration`),
    /// once per process. Failure maps to 502: the deployment's issuer is
    /// down/misconfigured, not the client's fault and not ours.
    /// The issuer's discovery document, fetched once.
    ///
    /// Errors when there is no issuer. Callers that can legitimately be called on a
    /// no-issuer deployment must check first rather than treating this as a
    /// failure — see [`spa_config`].
    async fn endpoints(&self) -> Result<&IdpEndpoints, ApiError> {
        let issuer = self.issuer.clone().ok_or_else(|| {
            idp_unreachable(
                "this deployment has no OIDC issuer configured (KAIROS_LOCAL_AUTH \
                 only); there is no authorization endpoint to discover"
                    .to_string(),
            )
        })?;
        self.endpoints
            .get_or_try_init(|| async {
                let url = format!("{issuer}/.well-known/openid-configuration");
                self.http
                    .get(&url)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| idp_unreachable(format!("fetching {url}: {e}")))?
                    .json::<IdpEndpoints>()
                    .await
                    .map_err(|e| idp_unreachable(format!("parsing discovery document: {e}")))
            })
            .await
    }
}

fn idp_unreachable(message: String) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "IDP_UNREACHABLE", message)
}

/// The web routes: `/api/config` + `/api/auth/token`. Mounted OUTSIDE the
/// auth → tenant stack (both are pre-login by nature; see module docs).
pub fn router(state: &AppState) -> Router<AppState> {
    let web_auth = Arc::new(WebAuth {
        issuer: state.config.oidc_issuer_url.clone(),
        client_id: state.config.web_client_id.clone(),
        api_bearer: state.config.api_bearer,
        client_secret: state.config.web_client_secret.clone(),
        http: reqwest::Client::new(),
        endpoints: OnceCell::new(),
    });
    Router::new()
        .route("/api/config", get(spa_config))
        .route("/api/auth/token", post(token_relay))
        .layer(Extension(web_auth))
}

/// `GET /api/config` response body (mirrored by `kairos-web::auth`).
#[derive(Debug, Serialize)]
struct SpaConfig {
    /// `None` when the deployment has no issuer (KAIROS-T-0208). The SPA reads this
    /// to decide whether to offer an SSO button at all: one that cannot be honoured
    /// is worse than none, because the person clicks it and lands nowhere.
    issuer: Option<String>,
    client_id: String,
    /// `None` with no issuer, for the same reason.
    authorization_endpoint: Option<String>,
    /// Which token the SPA sends as the `/api` bearer: `access_token`
    /// (default) or `id_token` (opaque-access-token issuers, T-0054).
    api_bearer: &'static str,
    /// Whether `POST /api/login` exists (KAIROS-T-0203). The SPA shows a password
    /// form when it does. Both may be true: local accounts are additive.
    local_auth: bool,
}

/// `GET /api/config` — see module docs. OpenAPI doc-stub lives in
/// [`crate::api::openapi`] (the whoami pattern).
async fn spa_config(
    State(state): State<AppState>,
    Extension(web_auth): Extension<Arc<WebAuth>>,
) -> Result<Json<SpaConfig>, ApiError> {
    // KAIROS-T-0208: with no issuer, do not attempt discovery. Returning 502
    // IDP_UNREACHABLE here would be a lie — nothing is unreachable, there is simply
    // no IdP — and it would leave the SPA unable to render the login page it CAN
    // offer.
    let authorization_endpoint = match web_auth.issuer {
        Some(_) => Some(web_auth.endpoints().await?.authorization_endpoint.clone()),
        None => None,
    };
    Ok(Json(SpaConfig {
        issuer: web_auth.issuer.clone(),
        client_id: web_auth.client_id.clone(),
        authorization_endpoint,
        api_bearer: web_auth.api_bearer.as_str(),
        local_auth: state.config.local_auth,
    }))
}

/// What the SPA may relay. Anything else in the form is dropped; the
/// client id is always the server-configured one.
#[derive(Debug, Deserialize)]
struct TokenRelayForm {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

/// Assemble the whitelisted form parameters to relay to the issuer's token
/// endpoint. Pure (no I/O) so the grant whitelist and the confidential-client
/// secret rule are unit-testable. The server-configured `client_id` always
/// replaces whatever the SPA sent; `client_secret` is appended iff configured
/// (KAIROS-T-0056) and applies to both grants.
fn relay_params<'a>(
    client_id: &'a str,
    client_secret: Option<&'a str>,
    form: &'a TokenRelayForm,
) -> Result<Vec<(&'a str, &'a str)>, ApiError> {
    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", form.grant_type.as_str()),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }
    match form.grant_type.as_str() {
        "authorization_code" => {
            let code = form.code.as_deref().ok_or_else(missing("code"))?;
            let redirect_uri = form
                .redirect_uri
                .as_deref()
                .ok_or_else(missing("redirect_uri"))?;
            let verifier = form
                .code_verifier
                .as_deref()
                .ok_or_else(missing("code_verifier"))?;
            params.extend([
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ]);
        }
        "refresh_token" => {
            let refresh = form
                .refresh_token
                .as_deref()
                .ok_or_else(missing("refresh_token"))?;
            params.push(("refresh_token", refresh));
        }
        other => {
            return Err(ApiError::validation(format!(
                "grant_type {other:?} is not relayed; expected authorization_code or refresh_token"
            )));
        }
    }
    Ok(params)
}

/// `POST /api/auth/token` — the same-origin token relay (module docs).
/// The issuer's JSON answer (success or OAuth error) passes through with
/// its status code, so the SPA sees standard token-endpoint semantics.
/// Extraction failures (missing/wrong content type, undecodable body)
/// answer with the S-0005 envelope like every other `/api` error.
async fn token_relay(
    Extension(web_auth): Extension<Arc<WebAuth>>,
    form: Result<Form<TokenRelayForm>, axum::extract::rejection::FormRejection>,
) -> Result<Response, ApiError> {
    let Form(form) = form
        .map_err(|rejection| ApiError::validation(format!("malformed form body: {rejection}")))?;
    let params = relay_params(
        &web_auth.client_id,
        web_auth.client_secret.as_deref(),
        &form,
    )?;

    let endpoints = web_auth.endpoints().await?;
    let response = web_auth
        .http
        .post(&endpoints.token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| idp_unreachable(format!("token endpoint: {e}")))?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response
        .bytes()
        .await
        .map_err(|e| idp_unreachable(format!("reading token response: {e}")))?;
    Ok((status, [(header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// Builder for the "required form field is missing" error.
fn missing(field: &'static str) -> impl FnOnce() -> ApiError {
    move || ApiError::validation(format!("{field} is required for this grant_type"))
}

// ---------------------------------------------------------------------------
// SPA fallback
// ---------------------------------------------------------------------------

/// Is this path owned by an API surface (never SPA-fallback material)?
fn is_reserved(path: &str) -> bool {
    RESERVED_PREFIXES.iter().any(|prefix| {
        path == *prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Should a missing file fall back to `index.html`? Yes for route-like
/// paths (`/boards/acme`), no for asset-like ones (`/app.css` — a missing
/// asset must 404, not serve HTML to a stylesheet request).
fn wants_index_fallback(rel: &str) -> bool {
    !rel.rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
}

/// The router fallback (mounted in [`crate::app::router`]): reserved
/// prefixes 404 with the S-0005 envelope; everything else serves the GUI.
pub async fn spa_fallback(State(state): State<AppState>, method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if is_reserved(path) {
        return ApiError::not_found(format!("no route {path}")).into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let rel = path.trim_start_matches('/');
    if rel.split('/').any(|component| component == "..") {
        return ApiError::not_found("no such asset").into_response();
    }

    if let Some(dist) = state.config.web_dist.clone() {
        return serve_from_dir(&dist, rel).await;
    }
    serve_embedded_or_placeholder(rel)
}

/// Dev serving: files out of `KAIROS_WEB_DIST`, with the SPA fallback.
async fn serve_from_dir(dist: &std::path::Path, rel: &str) -> Response {
    let name = if rel.is_empty() { "index.html" } else { rel };
    match tokio::fs::read(dist.join(name)).await {
        Ok(bytes) => asset_response(name, Bytes::from(bytes)),
        Err(_) if wants_index_fallback(name) => {
            match tokio::fs::read(dist.join("index.html")).await {
                Ok(bytes) => asset_response("index.html", Bytes::from(bytes)),
                Err(_) => ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "WEB_DIST_MISSING",
                    format!(
                        "KAIROS_WEB_DIST is set but {}/index.html does not exist — \
                     run `angreal web build` first",
                        dist.display()
                    ),
                )
                .into_response(),
            }
        }
        Err(_) => ApiError::not_found(format!("no such asset: /{rel}")).into_response(),
    }
}

/// Release serving: assets embedded by the `embed-web` feature.
#[cfg(feature = "embed-web")]
fn serve_embedded_or_placeholder(rel: &str) -> Response {
    #[derive(rust_embed::Embed)]
    // Relative to this crate's manifest dir: the trunk output.
    #[folder = "../kairos-web/dist"]
    struct Assets;

    let name = if rel.is_empty() { "index.html" } else { rel };
    match Assets::get(name) {
        Some(file) => asset_response(name, Bytes::from(file.data.into_owned())),
        None if wants_index_fallback(name) => match Assets::get("index.html") {
            Some(file) => asset_response("index.html", Bytes::from(file.data.into_owned())),
            None => ApiError::not_found("embedded GUI has no index.html").into_response(),
        },
        None => ApiError::not_found(format!("no such asset: /{rel}")).into_response(),
    }
}

/// No dist dir, no embedded assets: an honest placeholder so plain
/// `cargo run -p kairos-server -- serve` still answers `/` usefully.
#[cfg(not(feature = "embed-web"))]
fn serve_embedded_or_placeholder(_rel: &str) -> Response {
    const PLACEHOLDER: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
        <title>Kairos</title></head><body>\
        <h1>Kairos</h1>\
        <p>The API is up, but this build serves no GUI assets.</p>\
        <ul>\
        <li>dev: <code>angreal web build</code>, then restart with \
        <code>KAIROS_WEB_DIST=crates/kairos-web/dist</code></li>\
        <li>release: build the server with <code>--features embed-web</code> \
        after <code>angreal web build --release</code></li>\
        </ul>\
        <p>See <code>docs/gui-conventions.md</code>.</p>\
        </body></html>\n";
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        PLACEHOLDER,
    )
        .into_response()
}

/// An asset body with content type + cache policy. Trunk content-hashes
/// every asset filename, so non-index assets are immutable; `index.html`
/// must always revalidate (it is the pointer to the current hashes).
fn asset_response(name: &str, bytes: Bytes) -> Response {
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let cache = if name == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_code_form() -> TokenRelayForm {
        TokenRelayForm {
            grant_type: "authorization_code".to_string(),
            code: Some("the-code".to_string()),
            redirect_uri: Some("https://kairos.test/callback".to_string()),
            code_verifier: Some("the-verifier".to_string()),
            refresh_token: None,
        }
    }

    /// The relay appends `client_secret` iff configured (KAIROS-T-0056), and
    /// never otherwise — public clients (Dex/Keycloak) send exactly as before.
    #[test]
    fn relay_includes_client_secret_only_when_configured() {
        let form = auth_code_form();

        let public = relay_params("kairos-web", None, &form).expect("params");
        assert!(
            !public.iter().any(|(k, _)| *k == "client_secret"),
            "public client must not send a secret"
        );
        // The server-configured client_id is always present.
        assert!(public.contains(&("client_id", "kairos-web")));

        let confidential =
            relay_params("google-client", Some("goog-secret"), &form).expect("params");
        assert!(confidential.contains(&("client_secret", "goog-secret")));
    }

    /// The secret is applied to the refresh grant too (Google refreshes are
    /// confidential just like the code exchange).
    #[test]
    fn relay_secret_applies_to_refresh_grant() {
        let form = TokenRelayForm {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            refresh_token: Some("r3fresh".to_string()),
        };
        let params = relay_params("google-client", Some("goog-secret"), &form).expect("params");
        assert!(params.contains(&("client_secret", "goog-secret")));
        assert!(params.contains(&("refresh_token", "r3fresh")));
    }

    /// An unsupported grant is rejected even with a secret configured.
    #[test]
    fn relay_rejects_unknown_grant() {
        let form = TokenRelayForm {
            grant_type: "password".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            refresh_token: None,
        };
        assert!(relay_params("c", Some("s"), &form).is_err());
    }

    #[test]
    fn reserved_prefixes_match_paths_not_strings() {
        for reserved in [
            "/api",
            "/api/whoami",
            "/api/auth/token",
            "/mcp",
            "/mcp/anything",
            "/scim/v2/Users",
            "/ws/events",
            "/healthz",
            "/readyz",
            "/metrics",
            "/.well-known/oauth-protected-resource",
        ] {
            assert!(is_reserved(reserved), "{reserved} must be reserved");
        }
        for spa in [
            "/",
            "/boards",
            "/boards/acme",
            "/callback",
            "/login",
            "/apifoo",
            "/wsx",
        ] {
            assert!(!is_reserved(spa), "{spa} must fall through to the SPA");
        }
    }

    #[test]
    fn index_fallback_only_for_route_like_paths() {
        assert!(wants_index_fallback(""));
        assert!(wants_index_fallback("boards"));
        assert!(wants_index_fallback("boards/acme"));
        assert!(wants_index_fallback("callback"));
        assert!(!wants_index_fallback("app.css"));
        assert!(!wants_index_fallback("kairos-web-abc123_bg.wasm"));
        assert!(!wants_index_fallback("nested/logo.svg"));
    }

    #[test]
    fn asset_responses_carry_mime_and_cache_policy() {
        let response = asset_response("kairos-web-abc_bg.wasm", Bytes::from_static(b"\0asm"));
        assert_eq!(
            response.headers()[header::CONTENT_TYPE.as_str()],
            "application/wasm"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL.as_str()],
            "public, max-age=31536000, immutable"
        );

        let response = asset_response("index.html", Bytes::from_static(b"<!doctype html>"));
        assert_eq!(
            response.headers()[header::CONTENT_TYPE.as_str()],
            "text/html"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL.as_str()],
            "no-cache"
        );
    }
}
