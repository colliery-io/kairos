//! PKCE authentication for the SPA (KAIROS-A-0010 flow, KAIROS-T-0039).
//!
//! The flow (documented in `docs/gui-conventions.md` § Auth):
//!
//! 1. `GET /api/config` (public, same-origin) tells the SPA the issuer,
//!    the OAuth client id, and the issuer's `authorization_endpoint`.
//! 2. [`begin_login`] generates a PKCE verifier + S256 challenge and a
//!    `state` nonce (WebCrypto), stashes them in `sessionStorage` (they
//!    must survive the full-page redirect), and navigates to the issuer's
//!    authorization endpoint.
//! 3. The issuer redirects back to `/callback?code=…&state=…`;
//!    [`complete_login`] checks `state` and exchanges the code through the
//!    server's same-origin token relay (`POST /api/auth/token`) — the
//!    relay exists because IdP token endpoints (the dev Dex included) do
//!    not generally serve CORS to SPAs; the server forwards to the real
//!    token endpoint and adds nothing (the client stays public, PKCE
//!    intact).
//! 4. The **access token** lives in memory only (A-0015: no cookies, no
//!    localStorage): [`Auth`] holds it in a reactive signal. The **refresh
//!    token** additionally sits in `sessionStorage` (KAIROS-T-0071,
//!    amending A-0015): per-tab, cleared when the tab closes — so a page
//!    RELOAD restores the session silently ([`restore_session`], the
//!    refresh grant run before the guard redirects) instead of bouncing
//!    through the issuer, which on IdPs without an SSO session (the dev
//!    Dex password connector) meant the login form on every reload.
//! 5. Silent refresh: a timer fires [`REFRESH_MARGIN_SECS`] before expiry
//!    and swaps the session via the `refresh_token` grant on the same
//!    relay. Refresh failure clears the session — the protected shell
//!    then redirects to login.
//! 6. [`Auth::logout`] drops the in-memory session AND the stored refresh
//!    token (a logout must not silently sign back in on reload; the IdP
//!    session, if any, is the IdP's own concern — Kairos keeps no cookies).
//!
//! The bearer sent to `/api` is, by default, the **access token** (the dev
//! Dex mints JWT access tokens carrying `iss`/`aud`/`exp`/`email` — exactly
//! what the server validates per A-0010). When the deployment sets
//! `KAIROS_API_BEARER=id_token` (surfaced as `api_bearer` in `/api/config`),
//! the SPA sends the **id_token** instead — for issuers whose access token is
//! opaque and cannot be validated by local JWKS, notably Google / Google
//! Workspace (KAIROS-T-0054).

use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::JsFuture;

/// Same-origin path of the server's token relay (see `kairos-server::web`).
pub const TOKEN_RELAY_PATH: &str = "/api/auth/token";
/// Same-origin path of the public SPA config endpoint.
pub const CONFIG_PATH: &str = "/api/config";

/// Scopes requested at login. `offline_access` asks the issuer for a
/// refresh token (silent refresh); the rest feed JIT provisioning (A-0010).
const SCOPES: &str = "openid profile email offline_access";

/// Refresh the session this many seconds before the access token expires.
const REFRESH_MARGIN_SECS: f64 = 60.0;

// sessionStorage keys. The PKCE trio survives only the IdP redirect and is
// cleared as soon as the callback consumes it.
const KEY_VERIFIER: &str = "kairos_pkce_verifier";
const KEY_STATE: &str = "kairos_pkce_state";
const KEY_RETURN_TO: &str = "kairos_return_to";
/// The refresh token (KAIROS-T-0071): per-tab reload survival. Written on
/// every grant (rotation-safe), removed on logout and on refresh failure.
const KEY_REFRESH: &str = "kairos_refresh_token";

/// Which token the SPA presents as the `/api` bearer (`api_bearer` in
/// `/api/config`, KAIROS-T-0054). Defaults to [`ApiBearer::AccessToken`] when
/// the field is absent (older server, or the `access_token` default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiBearer {
    /// Send the OAuth `access_token` (Dex, Keycloak, JWT-access-token IdPs).
    #[default]
    AccessToken,
    /// Send the OIDC `id_token` (opaque-access-token IdPs, e.g. Google).
    IdToken,
}

/// What `GET /api/config` returns (mirror of `kairos-server::web`).
#[derive(Clone, Debug, Deserialize)]
pub struct AuthConfig {
    /// The deployment issuer (`OIDC_ISSUER_URL`), or `None` when the deployment has
    /// no identity provider and authenticates by password alone (KAIROS-T-0208).
    ///
    /// `Option` plus `#[serde(default)]`, so this mirror keeps deserializing a
    /// payload from a server that predates the change — it simply reads as "no
    /// issuer", which for such a server is wrong but harmless, since it also has no
    /// local auth and [`can_sso`] then reports false.
    #[serde(default)]
    pub issuer: Option<String>,
    /// The public OAuth client id registered for the GUI
    /// (`KAIROS_WEB_CLIENT_ID`, `kairos-web` on the dev stack).
    pub client_id: String,
    /// The issuer's authorization endpoint (from OIDC discovery,
    /// resolved server-side). `None` with no issuer.
    #[serde(default)]
    pub authorization_endpoint: Option<String>,
    /// Which token to send as the `/api` bearer. Absent on older servers →
    /// [`ApiBearer::AccessToken`] (backward compatible).
    #[serde(default)]
    pub api_bearer: ApiBearer,
    /// Whether `POST /api/login` exists (KAIROS-T-0203), so the GUI can offer a
    /// password form. Both this and an issuer may be true — local accounts are
    /// additive.
    #[serde(default)]
    pub local_auth: bool,
}

impl AuthConfig {
    /// Whether an SSO redirect can actually be performed.
    ///
    /// Both an issuer and an authorization endpoint, because a redirect needs the
    /// endpoint and a button that leads nowhere is worse than no button: the person
    /// clicks it, lands on an error, and has no way to guess that a password form
    /// was the answer.
    pub fn can_sso(&self) -> bool {
        self.issuer.is_some() && self.authorization_endpoint.is_some()
    }
}

/// A successful token response (authorization_code or refresh_token grant).
#[derive(Clone, Debug, Deserialize)]
pub struct TokenResponse {
    access_token: String,
    /// Present when the issuer granted the `openid` scope; the bearer when
    /// the deployment selects `api_bearer = id_token` (T-0054).
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Seconds until the access token expires.
    #[serde(default)]
    expires_in: Option<f64>,
}

impl TokenResponse {
    /// The token to use as the `/api` bearer for the given selection. Falls
    /// back to the access token if `id_token` was selected but the issuer
    /// returned none (e.g. a refresh response omitting it) — keeps a session
    /// alive rather than dropping it; a subsequent 401 re-runs login.
    fn bearer_for(&self, kind: ApiBearer) -> String {
        match kind {
            ApiBearer::IdToken => self
                .id_token
                .clone()
                .unwrap_or_else(|| self.access_token.clone()),
            ApiBearer::AccessToken => self.access_token.clone(),
        }
    }
}

/// The in-memory session (A-0015: never persisted).
#[derive(Clone, Debug)]
pub struct Session {
    /// Bearer for `/api` calls.
    pub access_token: String,
    /// Present when the issuer granted `offline_access`.
    pub refresh_token: Option<String>,
}

/// Reactive auth state, provided at the app root ([`provide_auth`]) and
/// consumed anywhere via [`use_auth`]. `Copy` — capture it freely.
#[derive(Clone, Copy)]
pub struct Auth {
    session: RwSignal<Option<Session>>,
    config: RwSignal<Option<AuthConfig>>,
    /// `true` only after an explicit [`Auth::logout`]: the shell guard
    /// then lands on `/login` instead of bouncing to the issuer (a fresh
    /// unauthenticated visit keeps the A-0015 issuer redirect).
    signed_out: RwSignal<bool>,
    /// `true` while a boot-time [`restore_session`] is in flight
    /// (KAIROS-T-0071): the shell guard shows a loading state instead of
    /// prematurely redirecting to the issuer.
    restoring: RwSignal<bool>,
    /// Bumped on every install/clear so stale refresh timers no-op.
    generation: StoredValue<u64>,
}

/// Create the auth state and put it into context. Call once, in `App`.
///
/// If a refresh token survives in `sessionStorage` (same-tab reload,
/// KAIROS-T-0071), a silent restore starts immediately; the guard waits on
/// [`Auth::restoring`] before deciding anyone is unauthenticated.
pub fn provide_auth() -> Auth {
    let has_stored_refresh = session_storage()
        .ok()
        .and_then(|s| s.get_item(KEY_REFRESH).ok().flatten())
        .is_some();
    let auth = Auth {
        session: RwSignal::new(None),
        config: RwSignal::new(None),
        signed_out: RwSignal::new(false),
        restoring: RwSignal::new(has_stored_refresh),
        generation: StoredValue::new(0),
    };
    provide_context(auth);
    if has_stored_refresh {
        leptos::task::spawn_local(async move { restore_session(auth).await });
    }
    auth
}

/// Boot-time session restore (KAIROS-T-0071): run the refresh grant with
/// the stored refresh token. Success installs a session (landing the user
/// where the URL says they are); failure removes the dead token and lets
/// the guard fall through to the normal login redirect. Either way,
/// `restoring` ends.
async fn restore_session(auth: Auth) {
    let stored = session_storage()
        .ok()
        .and_then(|s| s.get_item(KEY_REFRESH).ok().flatten());
    if let Some(refresh_token) = stored {
        auth.refresh_with(&refresh_token).await;
    }
    auth.restoring.set(false);
}

/// The app-root [`Auth`] (panics outside the app tree — a bug by
/// construction, every component lives under `App`).
pub fn use_auth() -> Auth {
    expect_context::<Auth>()
}

impl Auth {
    /// Reactive: is there a live session?
    pub fn is_authenticated(&self) -> bool {
        self.session.with(Option::is_some)
    }

    /// Current bearer token (reactive).
    pub fn token(&self) -> Option<String> {
        self.session
            .with(|s| s.as_ref().map(|s| s.access_token.clone()))
    }

    /// Reactive: did the user explicitly log out (vs. never signed in /
    /// session expired)? Drives the shell guard's `/login`-vs-issuer
    /// choice.
    pub fn signed_out(&self) -> bool {
        self.signed_out.get()
    }

    /// Reactive: is the boot-time session restore (KAIROS-T-0071) still in
    /// flight? While true the guard must wait, not redirect.
    pub fn restoring(&self) -> bool {
        self.restoring.get()
    }

    /// Drop the in-memory session AND the stored refresh token (a logout
    /// must not silently sign back in on the next reload). The protected
    /// shell reacts by redirecting to `/login` (see [`Self::signed_out`]);
    /// scheduled refreshes for the old session become no-ops.
    pub fn logout(&self) {
        self.generation.update_value(|g| *g += 1);
        clear_stored_refresh();
        self.signed_out.set(true);
        self.session.set(None);
    }

    /// Drop the session WITHOUT marking an explicit sign-out (refresh
    /// failure, a 401 from the API): the shell guard then re-runs the
    /// issuer redirect — silent re-auth when the IdP holds a session. The
    /// stored refresh token goes too: if it were still good the silent
    /// refresh would have used it, so keeping it only risks a restore loop.
    pub fn expire(&self) {
        self.generation.update_value(|g| *g += 1);
        clear_stored_refresh();
        self.session.set(None);
    }

    /// Install a token response and schedule the silent refresh.
    ///
    /// The bearer is chosen from `api_bearer` in the cached `/api/config`
    /// (T-0054): every caller (`complete_login`, `refresh`) resolves the
    /// config before exchanging tokens, so it is populated here. If it is
    /// somehow absent, the default is the access token — today's behavior.
    fn install(&self, tokens: TokenResponse) {
        self.generation.update_value(|g| *g += 1);
        let generation = self.generation.get_value();
        let refresh_token = tokens.refresh_token.clone();
        let api_bearer = self
            .config
            .with_untracked(|c| c.as_ref().map(|c| c.api_bearer))
            .unwrap_or_default();
        // Reload survival (KAIROS-T-0071): stash the refresh token per-tab.
        // Every grant rewrites it, so issuer-side rotation (Dex rotates on
        // each refresh) never leaves a stale token behind. Best-effort — a
        // blocked sessionStorage just means reloads re-login, as before.
        if let (Some(token), Ok(storage)) = (&refresh_token, session_storage()) {
            let _ = storage.set_item(KEY_REFRESH, token);
        }
        self.signed_out.set(false);
        self.session.set(Some(Session {
            access_token: tokens.bearer_for(api_bearer),
            refresh_token: refresh_token.clone(),
        }));

        let (Some(_), Some(expires_in)) = (refresh_token, tokens.expires_in) else {
            return; // nothing to refresh with, or no known expiry
        };
        let delay = (expires_in - REFRESH_MARGIN_SECS).max(5.0);
        let auth = *self;
        set_timeout(
            move || {
                // Stale timer (logout / newer session): do nothing.
                if auth.generation.get_value() != generation {
                    return;
                }
                leptos::task::spawn_local(async move { auth.refresh().await });
            },
            std::time::Duration::from_secs_f64(delay),
        );
    }

    /// The silent-refresh grant through the relay, from the live session's
    /// refresh token. On any failure the session is cleared (the shell
    /// redirects to login).
    async fn refresh(self) {
        let Some(refresh_token) = self
            .session
            .with_untracked(|s| s.as_ref().and_then(|s| s.refresh_token.clone()))
        else {
            return;
        };
        self.refresh_with(&refresh_token).await;
    }

    /// One refresh grant with an explicit token — shared by the in-session
    /// timer path ([`Self::refresh`]) and the boot restore
    /// ([`restore_session`], KAIROS-T-0071). Success installs; failure
    /// expires (which also removes the stored refresh token).
    async fn refresh_with(self, refresh_token: &str) {
        let config = match self.config_cached().await {
            Ok(config) => config,
            Err(_) => {
                self.expire();
                return;
            }
        };
        let body = form_encode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
        ]);
        match post_token(&body).await {
            Ok(tokens) => self.install(tokens),
            Err(_) => self.expire(),
        }
    }

    /// `/api/config`, fetched once and cached in the signal.
    async fn config_cached(&self) -> Result<AuthConfig, String> {
        if let Some(config) = self.config.get_untracked() {
            return Ok(config);
        }
        let config = fetch_auth_config().await?;
        self.config.set(Some(config.clone()));
        Ok(config)
    }
}

/// `GET /api/config` — the SPA's discovery endpoint (decision documented
/// in `docs/gui-conventions.md` § Auth).
pub async fn fetch_auth_config() -> Result<AuthConfig, String> {
    let response = gloo_net::http::Request::get(CONFIG_PATH)
        .send()
        .await
        .map_err(|e| format!("cannot reach {CONFIG_PATH}: {e}"))?;
    if !response.ok() {
        return Err(format!("{CONFIG_PATH} returned {}", response.status()));
    }
    response
        .json::<AuthConfig>()
        .await
        .map_err(|e| format!("cannot parse {CONFIG_PATH}: {e}"))
}

/// Start the PKCE flow: stash verifier/state/return-path in
/// `sessionStorage`, then navigate to the issuer's authorization endpoint.
/// `return_to` is the in-app path to land on after the callback.
pub async fn begin_login(auth: Auth, return_to: &str) -> Result<(), String> {
    let config = auth.config_cached().await?;

    let verifier = random_urlsafe(32)?;
    let state = random_urlsafe(16)?;
    let challenge = s256_base64url(&verifier).await?;

    let storage = session_storage()?;
    let stash = |k: &str, v: &str| {
        storage
            .set_item(k, v)
            .map_err(|_| "sessionStorage write failed".to_string())
    };
    stash(KEY_VERIFIER, &verifier)?;
    stash(KEY_STATE, &state)?;
    stash(KEY_RETURN_TO, return_to)?;

    // KAIROS-T-0208: refuse before touching sessionStorage or the location bar. A
    // half-started flow leaves a stale verifier behind and the person on a blank
    // page, which is a worse way to learn this than a sentence.
    let authorization_endpoint = config.authorization_endpoint.clone().ok_or_else(|| {
        "this deployment has no identity provider configured; sign in with your \
         email and password instead"
            .to_string()
    })?;

    let redirect_uri = format!("{}/callback", origin()?);
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        authorization_endpoint,
        url_encode(&config.client_id),
        url_encode(&redirect_uri),
        url_encode(SCOPES),
        url_encode(&state),
        url_encode(&challenge),
    );
    window()
        .location()
        .set_href(&url)
        .map_err(|_| "redirect to the issuer failed".to_string())
}

/// Handle `/callback`: verify `state`, exchange the code through the
/// relay, install the session. Returns the `return_to` path.
pub async fn complete_login(auth: Auth) -> Result<String, String> {
    let search = window()
        .location()
        .search()
        .map_err(|_| "cannot read callback URL".to_string())?;
    let params = web_sys::UrlSearchParams::new_with_str(&search)
        .map_err(|_| "cannot parse callback URL".to_string())?;

    if let Some(error) = params.get("error") {
        let description = params.get("error_description").unwrap_or_default();
        return Err(
            format!("the issuer rejected the login: {error} {description}")
                .trim()
                .to_string(),
        );
    }
    let code = params.get("code").ok_or("callback is missing ?code")?;
    let state = params.get("state").ok_or("callback is missing ?state")?;

    let storage = session_storage()?;
    let take = |k: &str| -> Option<String> {
        let v = storage.get_item(k).ok().flatten();
        let _ = storage.remove_item(k);
        v
    };
    let verifier =
        take(KEY_VERIFIER).ok_or("no PKCE verifier in this session (login did not start here)")?;
    let expected_state = take(KEY_STATE).ok_or("no login state in this session")?;
    let return_to = take(KEY_RETURN_TO).unwrap_or_else(|| "/".to_string());
    if state != expected_state {
        return Err("state mismatch — possible CSRF, login aborted".to_string());
    }

    let config = auth.config_cached().await?;
    let redirect_uri = format!("{}/callback", origin()?);
    let body = form_encode(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect_uri),
        ("client_id", &config.client_id),
        ("code_verifier", &verifier),
    ]);
    let tokens = post_token(&body).await?;
    auth.install(tokens);
    Ok(return_to)
}

/// POST a form body to the token relay and parse the token response.
async fn post_token(body: &str) -> Result<TokenResponse, String> {
    let response = gloo_net::http::Request::post(TOKEN_RELAY_PATH)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .map_err(|e| format!("building token request: {e}"))?
        .send()
        .await
        .map_err(|e| format!("cannot reach {TOKEN_RELAY_PATH}: {e}"))?;
    if !response.ok() {
        let detail = response.text().await.unwrap_or_default();
        return Err(format!(
            "token exchange failed ({}): {detail}",
            response.status()
        ));
    }
    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("cannot parse token response: {e}"))
}

// ---- small browser/PKCE helpers ----------------------------------------

fn window() -> web_sys::Window {
    web_sys::window().expect("no window (kairos-web only runs in a browser)")
}

fn origin() -> Result<String, String> {
    window()
        .location()
        .origin()
        .map_err(|_| "cannot read window.location.origin".to_string())
}

fn session_storage() -> Result<web_sys::Storage, String> {
    window()
        .session_storage()
        .ok()
        .flatten()
        .ok_or_else(|| "sessionStorage is unavailable".to_string())
}

/// Remove the stored refresh token (logout / dead token). Best-effort.
fn clear_stored_refresh() {
    if let Ok(storage) = session_storage() {
        let _ = storage.remove_item(KEY_REFRESH);
    }
}

fn url_encode(value: &str) -> String {
    js_sys::encode_uri_component(value).into()
}

/// `len` random bytes from WebCrypto, base64url-encoded (RFC 7636 §4.1
/// verifier alphabet).
fn random_urlsafe(len: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; len];
    window()
        .crypto()
        .map_err(|_| "WebCrypto unavailable".to_string())?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(|_| "crypto.getRandomValues failed".to_string())?;
    Ok(base64url(&bytes))
}

/// The S256 code challenge: `BASE64URL(SHA256(verifier))` (RFC 7636 §4.2).
async fn s256_base64url(verifier: &str) -> Result<String, String> {
    let data = verifier.as_bytes().to_vec();
    let promise = window()
        .crypto()
        .map_err(|_| "WebCrypto unavailable".to_string())?
        .subtle()
        .digest_with_str_and_u8_array("SHA-256", &data)
        .map_err(|_| "SubtleCrypto.digest failed".to_string())?;
    let digest = JsFuture::from(promise)
        .await
        .map_err(|_| "SHA-256 digest rejected".to_string())?;
    let bytes = js_sys::Uint8Array::new(&digest).to_vec();
    Ok(base64url(&bytes))
}

/// Base64url without padding (RFC 4648 §5). Pure so the host test suite
/// covers it (`cargo test -p kairos-web`).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// `application/x-www-form-urlencoded` body from pairs. Pure (host-tested).
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", k, form_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode one form value (conservative: everything but unreserved).
fn form_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ApiBearer, AuthConfig, TokenResponse, base64url, form_component, form_encode};

    fn tokens(access: &str, id: Option<&str>) -> TokenResponse {
        TokenResponse {
            access_token: access.to_string(),
            id_token: id.map(str::to_string),
            refresh_token: None,
            expires_in: None,
        }
    }

    #[test]
    fn bearer_selection_follows_api_bearer() {
        let full = tokens("ya29.opaque", Some("eyJ.id.jwt"));
        // Default / access_token mode sends the access token (Dex/Keycloak).
        assert_eq!(full.bearer_for(ApiBearer::AccessToken), "ya29.opaque");
        // id_token mode sends the id_token (Google / opaque-access-token IdP).
        assert_eq!(full.bearer_for(ApiBearer::IdToken), "eyJ.id.jwt");
    }

    #[test]
    fn id_token_mode_falls_back_when_absent() {
        // A refresh response without an id_token keeps the session alive on
        // the access token rather than dropping it (T-0054).
        let no_id = tokens("access-only", None);
        assert_eq!(no_id.bearer_for(ApiBearer::IdToken), "access-only");
    }

    #[test]
    fn api_bearer_defaults_to_access_token_when_absent() {
        // Older server (no api_bearer field) → access_token, backward compat.
        let config: AuthConfig = serde_json::from_str(
            r#"{"issuer":"https://accounts.google.com","client_id":"c",
                "authorization_endpoint":"https://x/auth"}"#,
        )
        .expect("parses without api_bearer");
        assert_eq!(config.api_bearer, ApiBearer::AccessToken);

        let config: AuthConfig = serde_json::from_str(
            r#"{"issuer":"i","client_id":"c","authorization_endpoint":"a",
                "api_bearer":"id_token"}"#,
        )
        .expect("parses with api_bearer");
        assert_eq!(config.api_bearer, ApiBearer::IdToken);
    }

    #[test]
    fn a_deployment_with_no_issuer_parses_and_offers_no_sso() {
        // KAIROS-T-0208. The server sends nulls; the mirror must accept them rather
        // than fail to deserialize and leave the GUI unable to render a login page
        // it CAN offer.
        let config: AuthConfig = serde_json::from_str(
            r#"{"issuer":null,"client_id":"kairos-web",
                "authorization_endpoint":null,"local_auth":true}"#,
        )
        .expect("a no-issuer payload must parse");
        assert!(!config.can_sso(), "no endpoint to redirect to");
        assert!(config.local_auth, "but there is a password form");
    }

    #[test]
    fn both_paths_at_once_is_representable() {
        // Local accounts are additive (KAIROS-I-0018), so this is a normal state and
        // not a contradiction the GUI has to resolve.
        let config: AuthConfig = serde_json::from_str(
            r#"{"issuer":"https://idp.example","client_id":"c",
                "authorization_endpoint":"https://idp.example/auth","local_auth":true}"#,
        )
        .expect("parses");
        assert!(config.can_sso() && config.local_auth);
    }

    #[test]
    fn a_pre_t0208_server_payload_still_parses() {
        // An older server sends no `local_auth` and a plain string issuer.
        let config: AuthConfig = serde_json::from_str(
            r#"{"issuer":"https://idp.example","client_id":"c",
                "authorization_endpoint":"https://idp.example/auth"}"#,
        )
        .expect("parses");
        assert!(config.can_sso());
        assert!(!config.local_auth);
    }

    /// RFC 4648 §10 test vectors, translated to base64url-no-padding.
    #[test]
    fn base64url_matches_rfc4648_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_uses_urlsafe_alphabet() {
        // 0xfb 0xff maps onto '+'/'/' in plain base64; must be '-'/'_'.
        let encoded = base64url(&[0xfb, 0xff, 0xfe]);
        assert!(!encoded.contains('+') && !encoded.contains('/'));
        assert_eq!(encoded, "-__-");
    }

    #[test]
    fn form_encoding_escapes_reserved_characters() {
        assert_eq!(form_component("abc-._~XYZ09"), "abc-._~XYZ09");
        assert_eq!(form_component("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(
            form_encode(&[("grant_type", "authorization_code"), ("code", "x/y+z")]),
            "grant_type=authorization_code&code=x%2Fy%2Bz"
        );
    }
}
