//! GUI serving + SPA auth support (KAIROS-T-0039, per A-0015/A-0013).
//!
//! Three things live here, everything the `kairos-web` SPA needs from its
//! own origin:
//!
//! 1. **`GET /api/config`** (public, no auth): the SPA's discovery
//!    endpoint — issuer, the GUI's OAuth client id
//!    (`KAIROS_WEB_CLIENT_ID`), and the issuer's `authorization_endpoint`
//!    (resolved from OIDC discovery server-side, cached; KAIROS-T-0613: the
//!    discovery document of the startup, so no second fetch). Public by
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
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
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
    // The docs sites (KAIROS-T-0403): the `/pages` router answers, never the SPA.
    "/pages",
];

/// Issuer endpoints the SPA flow needs, resolved lazily from OIDC
/// discovery and cached for the process lifetime.
#[derive(Clone, Debug, Deserialize)]
struct IdpEndpoints {
    authorization_endpoint: String,
    token_endpoint: String,
    /// Absent from some discovery documents; then nothing is known and
    /// [`login_scope`] asks for `offline_access` as before.
    #[serde(default)]
    scopes_supported: Option<Vec<String>>,
}

/// The scope the SPA requests at login. `offline_access` (a refresh token,
/// for silent refresh) only when the issuer does not say it lacks it:
/// Google lists `["openid", "email", "profile"]` and answers a request for
/// `offline_access` with `400 invalid_scope`, so no one could sign in.
/// Without it the session ends when the token does, and the person signs
/// in again.
fn login_scope(scopes_supported: Option<&[String]>) -> String {
    let offline = scopes_supported.is_none_or(|s| s.iter().any(|x| x == "offline_access"));
    if offline {
        "openid profile email offline_access".to_string()
    } else {
        "openid profile email".to_string()
    }
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
    /// KAIROS-T-0613: a 5 s timeout ([`crate::middleware::auth::IDP_TIMEOUT`])
    /// on the discovery and on the token relay.
    http: reqwest::Client,
    endpoints: OnceCell<IdpEndpoints>,
}

/// The endpoints of the discovery document that the authenticator fetched at
/// startup (KAIROS-T-0613), when its issuer is the issuer of `/api/config`.
/// `None` when there is no such document or it does not parse: then the first
/// request fetches the document as before, and a fault gets its own message.
fn boot_endpoints(state: &AppState) -> Option<IdpEndpoints> {
    let issuer = state.config.oidc_issuer_url.as_deref()?;
    if issuer.trim_end_matches('/') != state.auth.issuer() {
        return None;
    }
    let document = state.auth.discovery_document()?;
    serde_json::from_value(document.clone()).ok()
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
                "This deployment has no OIDC issuer, so it has no authorization endpoint. \
                 Log in with a password."
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
                    .map_err(|e| idp_unreachable(format!("The server cannot get {url}: {e}.")))?
                    .json::<IdpEndpoints>()
                    .await
                    .map_err(|e| {
                        idp_unreachable(format!(
                            "The discovery document of the issuer is not correct: {e}."
                        ))
                    })
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
        http: crate::middleware::auth::idp_client(),
        endpoints: OnceCell::new_with(boot_endpoints(state)),
    });
    Router::new()
        // COLLIERY-T-0256: `/api/config` has the rule of the inputs. The
        // token relay has not: OAuth gives the inputs of a token endpoint.
        .route(
            "/api/config",
            get(spa_config).route_layer(axum::middleware::from_fn(
                crate::input::refuse_unknown_input,
            )),
        )
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
    /// The scope to request at login, from [`login_scope`]. `None` with no
    /// issuer.
    scope: Option<String>,
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
    let (authorization_endpoint, scope) = match web_auth.issuer {
        Some(_) => {
            let endpoints = web_auth.endpoints().await?;
            (
                Some(endpoints.authorization_endpoint.clone()),
                Some(login_scope(endpoints.scopes_supported.as_deref())),
            )
        }
        None => (None, None),
    };
    Ok(Json(SpaConfig {
        issuer: web_auth.issuer.clone(),
        client_id: web_auth.client_id.clone(),
        authorization_endpoint,
        scope,
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
                "The server does not accept the grant_type {other:?}. Send authorization_code or refresh_token."
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
    let Form(form) = form.map_err(form_refusal)?;
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
        .map_err(|e| {
            idp_unreachable(format!(
                "The server cannot connect to the token endpoint of the issuer: {e}."
            ))
        })?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response.bytes().await.map_err(|e| {
        idp_unreachable(format!(
            "The server cannot read the response of the token endpoint: {e}."
        ))
    })?;
    Ok((status, [(header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// Builder for the "required form field is missing" error.
fn missing(field: &'static str) -> impl FnOnce() -> ApiError {
    move || {
        ApiError::validation(format!(
            "The request has no {field}. This grant_type must have {field}."
        ))
        .with_details(serde_json::json!({ "field": field }))
    }
}

/// The refusal of a form that the token relay cannot read (KAIROS-T-0697):
/// in Kairos words, not the text of axum.
fn form_refusal(fault: axum::extract::rejection::FormRejection) -> ApiError {
    use axum::extract::rejection::FormRejection;
    match fault {
        FormRejection::InvalidFormContentType(_) => ApiError::validation(
            "The request must have the header Content-Type: \
             application/x-www-form-urlencoded. Send the form with that header.",
        )
        .with_details(serde_json::json!({ "header": "Content-Type" })),
        FormRejection::FailedToDeserializeForm(_)
        | FormRejection::FailedToDeserializeFormBody(_) => {
            let field = crate::body::missing_field(&crate::body::innermost(&fault))
                .unwrap_or_else(|| "grant_type".to_string());
            ApiError::validation(format!(
                "The form is not correct at the field {field}. Send grant_type \
                 (authorization_code or refresh_token) and the fields of that grant: code, \
                 redirect_uri and code_verifier, or refresh_token."
            ))
            .with_details(serde_json::json!({ "field": field }))
        }
        _ => ApiError::validation(
            "The server could not read the body of the request. Send the form again.",
        ),
    }
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
pub async fn spa_fallback(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let path = uri.path();
    if is_reserved(path) {
        return ApiError::not_found(format!("The server has no route {path}.")).into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let rel = path.trim_start_matches('/');
    if rel.split('/').any(|component| component == "..") {
        return ApiError::not_found("The server does not have that asset.").into_response();
    }

    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let encoding = accepted_encoding(&headers);
    if let Some(dist) = state.config.web_dist.clone() {
        return serve_from_dir(&dist, rel, encoding, if_none_match.as_deref()).await;
    }
    serve_embedded_or_placeholder(rel, encoding, if_none_match.as_deref())
}

/// The encoding of a pre-compressed asset that the request accepts: brotli
/// before gzip, none when the request names neither (KAIROS-T-0603). The
/// large GUI assets use it, embedded or from `KAIROS_WEB_DIST`
/// (KAIROS-T-0662); every other answer goes through the compression layer
/// of the router instead.
fn accepted_encoding(headers: &HeaderMap) -> Option<&'static str> {
    let accept = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let names: Vec<&str> = accept
        .split(',')
        .map(|part| part.split(';').next().unwrap_or("").trim())
        .collect();
    if names.contains(&"br") {
        Some("br")
    } else if names.contains(&"gzip") {
        Some("gzip")
    } else {
        None
    }
}

/// Dev serving: files out of `KAIROS_WEB_DIST`, with the SPA fallback. A
/// file over [`precompressed::MIN_BYTES`] comes from its pre-compressed
/// copy when the request accepts brotli or gzip (KAIROS-T-0662). Before,
/// the compression layer compressed it again on each request: about 5 s
/// for the debug wasm bundle (50 MB), on each full load of a page.
async fn serve_from_dir(
    dist: &std::path::Path,
    rel: &str,
    encoding: Option<&'static str>,
    if_none_match: Option<&str>,
) -> Response {
    let name = if rel.is_empty() { "index.html" } else { rel };
    if let Some(encoding) = encoding
        && name != "index.html"
        && let Some(bytes) = precompressed::dist_copy(&dist.join(name), encoding).await
    {
        return asset_response_for(name, bytes, Some(encoding), None);
    }
    match tokio::fs::read(dist.join(name)).await {
        Ok(bytes) => asset_response_for(name, Bytes::from(bytes), None, if_none_match),
        Err(_) if wants_index_fallback(name) => {
            match tokio::fs::read(dist.join("index.html")).await {
                Ok(bytes) => {
                    asset_response_for("index.html", Bytes::from(bytes), None, if_none_match)
                }
                Err(_) => ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "WEB_DIST_MISSING",
                    format!(
                        "KAIROS_WEB_DIST gives a directory, and {}/index.html does not exist. Run \
                         `angreal web build` first.",
                        dist.display()
                    ),
                )
                .into_response(),
            }
        }
        Err(_) => ApiError::not_found(format!("The server has no asset /{rel}.")).into_response(),
    }
}

/// The assets embedded by the `embed-web` feature: the trunk output,
/// relative to this crate's manifest dir.
#[cfg(feature = "embed-web")]
#[derive(rust_embed::Embed)]
#[folder = "../kairos-web/dist"]
struct Assets;

/// Release serving: assets embedded by the `embed-web` feature. An asset
/// over [`precompressed::MIN_BYTES`] is served from its pre-compressed copy
/// when the request accepts brotli or gzip (KAIROS-T-0603): the wasm bundle
/// is compressed once per process, not once per request.
#[cfg(feature = "embed-web")]
fn serve_embedded_or_placeholder(
    rel: &str,
    encoding: Option<&'static str>,
    if_none_match: Option<&str>,
) -> Response {
    let name = if rel.is_empty() { "index.html" } else { rel };
    match Assets::get(name) {
        Some(file) => match encoding.and_then(|enc| precompressed::get(name, enc)) {
            Some(bytes) => asset_response_for(name, bytes, encoding, None),
            None => asset_response_for(name, embedded_bytes(file), None, if_none_match),
        },
        None if wants_index_fallback(name) => match Assets::get("index.html") {
            Some(file) => {
                asset_response_for("index.html", embedded_bytes(file), None, if_none_match)
            }
            None => ApiError::not_found("The GUI in the server has no index.html.").into_response(),
        },
        None => ApiError::not_found(format!("The server has no asset /{rel}.")).into_response(),
    }
}

/// The bytes of an embedded asset with no copy (KAIROS-T-0606): in a release
/// build rust-embed gives a `'static` slice, and `into_owned` copied the
/// whole wasm bundle on every request. A debug build reads the file, so the
/// bytes are owned already.
#[cfg(feature = "embed-web")]
fn embedded_bytes(file: rust_embed::EmbeddedFile) -> Bytes {
    match file.data {
        std::borrow::Cow::Borrowed(bytes) => Bytes::from_static(bytes),
        std::borrow::Cow::Owned(bytes) => Bytes::from(bytes),
    }
}

/// The pre-compressed copies of the large GUI assets (KAIROS-T-0603). Each
/// asset at or over `MIN_BYTES` is compressed once per process, on the
/// first request that asks for it, with gzip and with brotli; `index.html`
/// is left to the compression layer, because its CSP is computed per body.
/// The embedded assets use it, and the files of `KAIROS_WEB_DIST` too
/// (KAIROS-T-0662). A dist file can change on a rebuild, so its copy is
/// keyed on its path and stamped with its length and modified time.
mod precompressed {
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use std::time::UNIX_EPOCH;

    use axum::body::Bytes;

    #[cfg(feature = "embed-web")]
    use super::Assets;

    /// Below this size the compression layer of the router does the work.
    pub const MIN_BYTES: usize = 64 * 1024;

    /// A rebuild of the dist gives the bundle a new hashed name, so a dev
    /// server keeps the copies of the old names. When the copies pass this
    /// size, the cache is emptied before the next one goes in.
    const MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;

    /// What a copy was made from: the length and the modified time (in
    /// nanoseconds) of a dist file; `(0, 0)` for an embedded asset, which
    /// does not change in a process.
    type Stamp = (u64, u128);

    type Cache = HashMap<(String, &'static str), (Stamp, Bytes)>;

    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

    fn cache() -> &'static Mutex<Cache> {
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn lookup(key: &str, stamp: Stamp, encoding: &'static str) -> Option<Bytes> {
        let cache = cache().lock().ok()?;
        match cache.get(&(key.to_string(), encoding)) {
            Some((held, bytes)) if *held == stamp => Some(bytes.clone()),
            _ => None,
        }
    }

    fn store(key: String, stamp: Stamp, encoding: &'static str, bytes: Bytes) {
        if let Ok(mut cache) = cache().lock() {
            let held: usize = cache.values().map(|(_, b)| b.len()).sum();
            if held + bytes.len() > MAX_CACHED_BYTES {
                cache.clear();
            }
            cache.insert((key, encoding), (stamp, bytes));
        }
    }

    /// The `encoding` copy of the embedded asset `name`, made on first use;
    /// `None` for an asset that is small, missing, or `index.html`.
    #[cfg(feature = "embed-web")]
    pub fn get(name: &str, encoding: &'static str) -> Option<Bytes> {
        if name == "index.html" {
            return None;
        }
        let file = Assets::get(name)?;
        if file.data.len() < MIN_BYTES {
            return None;
        }
        let key = format!("embedded:{name}");
        if let Some(bytes) = lookup(&key, (0, 0), encoding) {
            return Some(bytes);
        }
        let bytes = Bytes::from(compress(&file.data, encoding)?);
        store(key, (0, 0), encoding, bytes.clone());
        Some(bytes)
    }

    /// The `encoding` copy of the dist file at `path`, made on first use and
    /// again when the file changes; `None` for a file that is small or
    /// missing (the caller then reads it as it is). The compression runs on
    /// a blocking thread.
    pub async fn dist_copy(path: &Path, encoding: &'static str) -> Option<Bytes> {
        let meta = tokio::fs::metadata(path).await.ok()?;
        if !meta.is_file() || meta.len() < MIN_BYTES as u64 {
            return None;
        }
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        let stamp = (meta.len(), modified.as_nanos());
        let key = format!("dist:{}", path.display());
        if let Some(bytes) = lookup(&key, stamp, encoding) {
            return Some(bytes);
        }
        let data = tokio::fs::read(path).await.ok()?;
        // The file changed between the stat and the read: no copy now.
        if data.len() as u64 != stamp.0 {
            return None;
        }
        let compressed = tokio::task::spawn_blocking(move || compress(&data, encoding))
            .await
            .ok()??;
        let bytes = Bytes::from(compressed);
        store(key, stamp, encoding, bytes.clone());
        Some(bytes)
    }

    fn compress(data: &[u8], encoding: &str) -> Option<Vec<u8>> {
        match encoding {
            "gzip" => {
                let mut out =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
                out.write_all(data).ok()?;
                out.finish().ok()
            }
            "br" => {
                let mut out = Vec::new();
                {
                    // 5: about the ratio of gzip 9 at a fraction of the time;
                    // the bundle is compressed once per process.
                    let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
                    writer.write_all(data).ok()?;
                    writer.flush().ok()?;
                }
                Some(out)
            }
            _ => None,
        }
    }
}

/// No dist dir, no embedded assets: an honest placeholder so plain
/// `cargo run -p kairos-server -- serve` still answers `/` usefully.
#[cfg(not(feature = "embed-web"))]
fn serve_embedded_or_placeholder(
    _rel: &str,
    _encoding: Option<&'static str>,
    _if_none_match: Option<&str>,
) -> Response {
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
/// `encoding` names the Content-Encoding of a pre-compressed body
/// (KAIROS-T-0603); every asset says `Vary: Accept-Encoding`, because a
/// cache may hold it in more than one encoding.
/// The ETag and the CSP of an index.html body, computed once per distinct
/// body (KAIROS-T-0606): the embedded index never changes, a dist index
/// changes on a rebuild. The ETag is the first 32 hex digits of the SHA-256
/// of the body, strong.
fn index_meta(bytes: &Bytes) -> (String, String) {
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<Option<(Bytes, String, String)>>> = OnceLock::new();
    let last = LAST.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = last.lock()
        && let Some((cached, etag, csp)) = guard.as_ref()
        && cached == bytes
    {
        return (etag.clone(), csp.clone());
    }
    let etag = crate::security_headers::etag_of(bytes);
    let csp = crate::security_headers::spa_csp(bytes);
    if let Ok(mut guard) = last.lock() {
        *guard = Some((bytes.clone(), etag.clone(), csp.clone()));
    }
    (etag, csp)
}

/// [`asset_response`] with the `If-None-Match` of the request: an index
/// whose ETag the client already holds gets a 304 with no body
/// (KAIROS-T-0606). The hashed assets are immutable and carry no ETag.
fn asset_response_for(
    name: &str,
    bytes: Bytes,
    encoding: Option<&'static str>,
    if_none_match: Option<&str>,
) -> Response {
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let mut response = if name == "index.html" {
        // KAIROS-T-0370: the CSP of the GUI, with the hash of each inline
        // script of this page.
        let (etag, csp) = index_meta(&bytes);
        let matches = if_none_match.is_some_and(|held| {
            held.split(',')
                .map(|tag| tag.trim().trim_start_matches("W/"))
                .any(|tag| tag == etag || tag == "*")
        });
        let status = if matches {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
        let body = if matches { Bytes::new() } else { bytes };
        (
            status,
            [
                (header::CONTENT_TYPE, mime.as_ref().to_string()),
                (header::CACHE_CONTROL, "no-cache".to_string()),
                (header::CONTENT_SECURITY_POLICY, csp),
                (header::ETAG, etag),
            ],
            body,
        )
            .into_response()
    } else {
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime.as_ref().to_string()),
                (
                    header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".to_string(),
                ),
            ],
            bytes,
        )
            .into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::VARY,
        header::HeaderValue::from_static("Accept-Encoding"),
    );
    if let Some(encoding) = encoding {
        headers.insert(
            header::CONTENT_ENCODING,
            header::HeaderValue::from_static(encoding),
        );
    }
    response
}

/// With the `embed-web` feature: the pre-compressed copies of the embedded
/// assets round-trip and are smaller than the raw bytes. Runs only when the
/// feature is on, against whatever `crates/kairos-web/dist` holds.
#[cfg(all(test, feature = "embed-web"))]
mod embedded_tests {
    use std::io::Read;

    use super::{Assets, precompressed};

    #[test]
    fn large_embedded_assets_have_smaller_compressed_copies_that_round_trip() {
        assert!(precompressed::get("index.html", "gzip").is_none());
        let mut large = 0;
        for name in Assets::iter() {
            let file = Assets::get(&name).expect("embedded");
            if file.data.len() < precompressed::MIN_BYTES || name == "index.html" {
                assert!(
                    precompressed::get(&name, "gzip").is_none(),
                    "{name} is small"
                );
                continue;
            }
            large += 1;
            let gz = precompressed::get(&name, "gzip").expect("gzip copy");
            let br = precompressed::get(&name, "br").expect("brotli copy");
            assert!(gz.len() < file.data.len(), "{name}: gzip is smaller");
            assert!(br.len() < file.data.len(), "{name}: brotli is smaller");
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&gz[..])
                .read_to_end(&mut out)
                .expect("gunzip");
            assert_eq!(out, file.data.as_ref(), "{name}: gzip round-trips");
            let mut out = Vec::new();
            brotli::Decompressor::new(&br[..], 4096)
                .read_to_end(&mut out)
                .expect("unbrotli");
            assert_eq!(out, file.data.as_ref(), "{name}: brotli round-trips");
            // The second call is the cached copy.
            assert_eq!(precompressed::get(&name, "gzip").expect("cached"), gz);
        }
        assert!(large >= 1, "the dist has at least one asset over MIN_BYTES");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `offline_access` only where the issuer does not rule it out.
    #[test]
    fn login_scope_follows_scopes_supported() {
        let google: Vec<String> = ["openid", "email", "profile"].map(String::from).into();
        assert_eq!(login_scope(Some(&google)), "openid profile email");

        let dex: Vec<String> = ["openid", "email", "groups", "profile", "offline_access"]
            .map(String::from)
            .into();
        assert_eq!(
            login_scope(Some(&dex)),
            "openid profile email offline_access"
        );

        assert_eq!(login_scope(None), "openid profile email offline_access");
    }

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
            "/pages",
            "/pages/acme/",
            "/pages/acme/kairos/repos/kairos/index.html",
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
            "/pagesx",
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
    fn the_index_gets_an_etag_and_a_304_when_the_client_holds_it() {
        let body = Bytes::from_static(b"<!doctype html><html><body>index</body></html>");
        let response = asset_response_for("index.html", body.clone(), None, None);
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG.as_str()]
            .to_str()
            .unwrap()
            .to_string();
        assert!(etag.starts_with('"') && etag.len() == 34, "{etag}");
        let again = asset_response_for("index.html", body.clone(), None, Some(&etag));
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(again.headers()[header::ETAG.as_str()], etag.as_str());
        let weak = format!("W/{etag}, \"other\"");
        let again = asset_response_for("index.html", body.clone(), None, Some(&weak));
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        let other = asset_response_for("index.html", body, None, Some("\"nope\""));
        assert_eq!(other.status(), StatusCode::OK);
    }

    #[test]
    fn accepted_encoding_prefers_brotli_then_gzip() {
        let mut headers = HeaderMap::new();
        assert_eq!(accepted_encoding(&headers), None);
        headers.insert(header::ACCEPT_ENCODING, "gzip, deflate".parse().unwrap());
        assert_eq!(accepted_encoding(&headers), Some("gzip"));
        headers.insert(
            header::ACCEPT_ENCODING,
            "gzip;q=1.0, br;q=0.9, *;q=0.1".parse().unwrap(),
        );
        assert_eq!(accepted_encoding(&headers), Some("br"));
        headers.insert(header::ACCEPT_ENCODING, "identity".parse().unwrap());
        assert_eq!(accepted_encoding(&headers), None);
    }

    #[test]
    fn a_precompressed_asset_names_its_encoding_and_varies() {
        let response = asset_response_for(
            "kairos-web-abc_bg.wasm",
            Bytes::from_static(b"\x1f\x8b"),
            Some("gzip"),
            None,
        );
        assert_eq!(
            response.headers()[header::CONTENT_ENCODING.as_str()],
            "gzip"
        );
        assert_eq!(response.headers()[header::VARY.as_str()], "Accept-Encoding");
        let response = asset_response_for("app.css", Bytes::from_static(b"body{}"), None, None);
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
        assert_eq!(response.headers()[header::VARY.as_str()], "Accept-Encoding");
    }

    #[test]
    fn asset_responses_carry_mime_and_cache_policy() {
        let response = asset_response_for(
            "kairos-web-abc_bg.wasm",
            Bytes::from_static(b"\0asm"),
            None,
            None,
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE.as_str()],
            "application/wasm"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL.as_str()],
            "public, max-age=31536000, immutable"
        );

        let response = asset_response_for(
            "index.html",
            Bytes::from_static(b"<!doctype html>"),
            None,
            None,
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE.as_str()],
            "text/html"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL.as_str()],
            "no-cache"
        );
    }

    /// KAIROS-T-0662: a large dist file is compressed once. The second
    /// request gets the same copy (the same buffer), a small file or
    /// `index.html` gets none, and a changed file gets a new copy.
    #[tokio::test]
    async fn a_large_dist_file_is_compressed_once_and_again_when_it_changes() {
        let dist = tempfile::tempdir().expect("dist dir");
        let path = dist.path().join("kairos-web-once_bg.wasm");
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &body).expect("wasm");

        let first = precompressed::dist_copy(&path, "br")
            .await
            .expect("br copy");
        assert!(first.len() < body.len());
        let second = precompressed::dist_copy(&path, "br").await.expect("cached");
        assert_eq!(
            first.as_ptr(),
            second.as_ptr(),
            "the second call is the copy"
        );
        let gzip = precompressed::dist_copy(&path, "gzip")
            .await
            .expect("gzip copy");
        assert_ne!(gzip, first);

        let small = dist.path().join("small.js");
        std::fs::write(&small, b"console.log(1)").expect("small");
        assert!(precompressed::dist_copy(&small, "br").await.is_none());
        assert!(
            precompressed::dist_copy(&dist.path().join("missing.wasm"), "br")
                .await
                .is_none()
        );
        let response =
            serve_from_dir(dist.path(), "kairos-web-once_bg.wasm", Some("br"), None).await;
        assert_eq!(response.headers()[header::CONTENT_ENCODING.as_str()], "br");
        std::fs::write(dist.path().join("index.html"), vec![b' '; 100_000]).expect("index");
        let response = serve_from_dir(dist.path(), "", Some("br"), None).await;
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());

        let mut rebuilt = body.clone();
        rebuilt.extend_from_slice(b"rebuilt");
        std::fs::write(&path, &rebuilt).expect("rebuilt");
        let third = precompressed::dist_copy(&path, "br")
            .await
            .expect("new copy");
        assert_ne!(third, first, "the copy follows the file");
    }
}
