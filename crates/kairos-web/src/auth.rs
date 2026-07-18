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
//! 4. Tokens live **in memory only** (A-0015: no long-lived cookies, no
//!    localStorage): [`Auth`] holds them in a reactive signal, so a page
//!    reload drops the session and re-runs the login redirect.
//! 5. Silent refresh: a timer fires [`REFRESH_MARGIN_SECS`] before expiry
//!    and swaps the session via the `refresh_token` grant on the same
//!    relay. Refresh failure clears the session — the protected shell
//!    then redirects to login.
//! 6. [`Auth::logout`] drops the in-memory session (the IdP session, if
//!    any, is the IdP's own concern; Kairos keeps no cookies).
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

// sessionStorage keys — the only auth state that survives the IdP
// redirect. Cleared as soon as the callback consumes them.
const KEY_VERIFIER: &str = "kairos_pkce_verifier";
const KEY_STATE: &str = "kairos_pkce_state";
const KEY_RETURN_TO: &str = "kairos_return_to";

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
    /// The deployment issuer (`OIDC_ISSUER_URL`).
    pub issuer: String,
    /// The public OAuth client id registered for the GUI
    /// (`KAIROS_WEB_CLIENT_ID`, `kairos-web` on the dev stack).
    pub client_id: String,
    /// The issuer's authorization endpoint (from OIDC discovery,
    /// resolved server-side).
    pub authorization_endpoint: String,
    /// Which token to send as the `/api` bearer. Absent on older servers →
    /// [`ApiBearer::AccessToken`] (backward compatible).
    #[serde(default)]
    pub api_bearer: ApiBearer,
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
    /// Bumped on every install/clear so stale refresh timers no-op.
    generation: StoredValue<u64>,
}

/// Create the auth state and put it into context. Call once, in `App`.
pub fn provide_auth() -> Auth {
    let auth = Auth {
        session: RwSignal::new(None),
        config: RwSignal::new(None),
        signed_out: RwSignal::new(false),
        generation: StoredValue::new(0),
    };
    provide_context(auth);
    auth
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

    /// Drop the in-memory session. The protected shell reacts by
    /// redirecting to `/login` (see [`Self::signed_out`]); scheduled
    /// refreshes for the old session become no-ops.
    pub fn logout(&self) {
        self.generation.update_value(|g| *g += 1);
        self.signed_out.set(true);
        self.session.set(None);
    }

    /// Drop the session WITHOUT marking an explicit sign-out (refresh
    /// failure, a 401 from the API): the shell guard then re-runs the
    /// issuer redirect — silent re-auth when the IdP holds a session.
    pub fn expire(&self) {
        self.generation.update_value(|g| *g += 1);
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

    /// The silent-refresh grant through the relay. On any failure the
    /// session is cleared (the shell redirects to login).
    async fn refresh(self) {
        let Some(refresh_token) = self
            .session
            .with_untracked(|s| s.as_ref().and_then(|s| s.refresh_token.clone()))
        else {
            return;
        };
        let config = match self.config_cached().await {
            Ok(config) => config,
            Err(_) => {
                self.expire();
                return;
            }
        };
        let body = form_encode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
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

    let redirect_uri = format!("{}/callback", origin()?);
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        config.authorization_endpoint,
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
