//! The OAuth/OIDC plumbing for `kairos login` (KAIROS-A-0010):
//!
//! 1. **Issuer discovery** — the deployment's RFC 9728 protected-resource
//!    metadata (`/.well-known/oauth-protected-resource/mcp`, the same
//!    document MCP clients use) names the `authorization_servers`.
//! 2. **OIDC discovery** — `{issuer}/.well-known/openid-configuration`
//!    yields the `device_authorization_endpoint` and `token_endpoint`.
//! 3. **Device Authorization Grant** (RFC 8628) — start the grant, print
//!    the verification URI + user code, poll the token endpoint honoring
//!    `interval`/`slow_down`.
//! 4. **Refresh grant** — trade a refresh token for a new access token.

use serde::{Deserialize, Serialize};

use crate::error::CliError;

/// The scopes requested at login: identity claims + `offline_access` for
/// a refresh token.
pub const SCOPES: &str = "openid profile email offline_access";

/// Which OIDC token the CLI caches and sends as the `/api` bearer
/// (KAIROS-T-0054). `access_token` (default) suits Dex/Keycloak and any
/// issuer that mints JWT access tokens; `id_token` suits issuers whose
/// access token is opaque and unvalidatable by local JWKS — notably Google /
/// Google Workspace, whose `id_token` is a validatable RS256 JWT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiBearer {
    /// Send the OAuth `access_token`.
    #[default]
    #[value(name = "access_token")]
    AccessToken,
    /// Send the OIDC `id_token`.
    #[value(name = "id_token")]
    IdToken,
}

/// The path-suffixed RFC 9728 well-known document for the `/mcp` resource
/// (served unauthenticated by every Kairos deployment).
const PROTECTED_RESOURCE_PATH: &str = "/.well-known/oauth-protected-resource/mcp";

/// The endpoints the device flow needs, from OIDC discovery.
#[derive(Debug, Clone)]
pub struct IssuerEndpoints {
    pub token_endpoint: String,
    pub device_authorization_endpoint: String,
}

/// RFC 8628 device authorization response.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Present when the issuer embeds the code in the URI (Dex does).
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    /// Seconds until the device code expires.
    pub expires_in: u64,
    /// Polling interval in seconds (RFC 8628 default: 5).
    #[serde(default)]
    pub interval: Option<u64>,
}

/// A successful token response (device grant or refresh grant).
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Present when the issuer granted the `openid` scope; the bearer when
    /// the deployment selects `id_token` (T-0054).
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Access-token lifetime in seconds.
    pub expires_in: u64,
}

impl TokenResponse {
    /// The token to cache/send as the `/api` bearer for the given selection.
    /// `None` when `id_token` was selected but the issuer returned none — the
    /// caller turns that into a re-login instruction.
    pub fn bearer_for(&self, kind: ApiBearer) -> Option<&str> {
        match kind {
            ApiBearer::AccessToken => Some(&self.access_token),
            ApiBearer::IdToken => self.id_token.as_deref(),
        }
    }
}

/// One poll of the token endpoint, classified per RFC 8628 §3.5.
#[derive(Debug)]
pub enum PollOutcome {
    /// Tokens issued — the grant is complete.
    Token(TokenResponse),
    /// `authorization_pending` — keep polling at the current interval.
    Pending,
    /// `slow_down` — keep polling, adding 5 seconds to the interval.
    SlowDown,
    /// `access_denied` — the user declined the grant.
    Denied,
    /// `expired_token` — the device code expired before approval.
    Expired,
    /// Anything else — a fatal protocol error.
    Fatal(String),
}

/// Classify a token-endpoint poll response (pure; unit-tested).
pub fn classify_poll_response(status: u16, body: &str) -> PollOutcome {
    if (200..300).contains(&status) {
        return match serde_json::from_str::<TokenResponse>(body) {
            Ok(token) => PollOutcome::Token(token),
            Err(err) => PollOutcome::Fatal(format!(
                "the issuer returned an unreadable token response: {err}"
            )),
        };
    }
    #[derive(Deserialize)]
    struct OAuthError {
        error: String,
        #[serde(default)]
        error_description: Option<String>,
    }
    match serde_json::from_str::<OAuthError>(body) {
        Ok(err) => match err.error.as_str() {
            "authorization_pending" => PollOutcome::Pending,
            "slow_down" => PollOutcome::SlowDown,
            "access_denied" => PollOutcome::Denied,
            "expired_token" => PollOutcome::Expired,
            other => PollOutcome::Fatal(format!(
                "the issuer rejected the device grant: {other}{}",
                err.error_description
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            )),
        },
        Err(_) => PollOutcome::Fatal(format!(
            "unexpected {status} response from the token endpoint: {body}"
        )),
    }
}

/// Discover the deployment's OIDC issuer from its RFC 9728
/// protected-resource metadata.
pub async fn discover_issuer(
    http: &reqwest::Client,
    deployment_url: &str,
) -> Result<String, CliError> {
    let metadata_url = format!("{deployment_url}{PROTECTED_RESOURCE_PATH}");
    let response = http.get(&metadata_url).send().await.map_err(|err| {
        CliError::Failure(format!(
            "cannot reach the deployment at {deployment_url}: {err}\n\
             Check the URL and your network connection."
        ))
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CliError::Failure(format!(
            "the deployment did not serve OAuth protected-resource metadata \
             ({metadata_url} returned {status}).\n\
             Is the URL a Kairos deployment? You can bypass discovery with \
             `--issuer <oidc-issuer-url>`."
        )));
    }
    #[derive(Deserialize)]
    struct ProtectedResource {
        #[serde(default)]
        authorization_servers: Vec<String>,
    }
    let metadata: ProtectedResource = response.json().await.map_err(|err| {
        CliError::Failure(format!(
            "unreadable protected-resource metadata from {metadata_url}: {err}"
        ))
    })?;
    // An EMPTY list is now a meaningful answer rather than a broken deployment
    // (KAIROS-T-0208): the server is telling us it has no identity provider, so
    // `kairos login` has nothing to log in to. Saying "pass --issuer" here would be
    // advice that cannot work — there is no issuer to name.
    metadata
        .authorization_servers
        .into_iter()
        .next()
        .ok_or_else(|| {
            CliError::Failure(format!(
                "this deployment has no OIDC issuer, so there is nothing for \
                 `kairos login` to authenticate against.\n\
                 It authenticates people by password in the browser \
                 (KAIROS_LOCAL_AUTH). For the CLI, ask an org admin for a \
                 service-account API key.\n\
                 If you believe the deployment does have an issuer, it is not \
                 advertising one at {metadata_url}; you can name it with \
                 `--issuer <oidc-issuer-url>`."
            ))
        })
}

/// OIDC discovery: the issuer's token + device-authorization endpoints.
pub async fn discover_endpoints(
    http: &reqwest::Client,
    issuer: &str,
) -> Result<IssuerEndpoints, CliError> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let response = http
        .get(&discovery_url)
        .send()
        .await
        .map_err(|err| {
            CliError::Failure(format!("cannot reach the OIDC issuer at {issuer}: {err}"))
        })?
        .error_for_status()
        .map_err(|err| {
            CliError::Failure(format!("OIDC discovery failed at {discovery_url}: {err}"))
        })?;
    #[derive(Deserialize)]
    struct Discovery {
        token_endpoint: String,
        #[serde(default)]
        device_authorization_endpoint: Option<String>,
    }
    let discovery: Discovery = response.json().await.map_err(|err| {
        CliError::Failure(format!(
            "unreadable OIDC discovery document from {discovery_url}: {err}"
        ))
    })?;
    let device_authorization_endpoint =
        discovery.device_authorization_endpoint.ok_or_else(|| {
            CliError::Failure(format!(
                "the issuer {issuer} does not advertise a device_authorization_endpoint; \
                 the Device Authorization Grant (required by `kairos login`, KAIROS-A-0010) \
                 must be enabled on the IdP"
            ))
        })?;
    Ok(IssuerEndpoints {
        token_endpoint: discovery.token_endpoint,
        device_authorization_endpoint,
    })
}

/// Start the Device Authorization Grant (RFC 8628 §3.1).
pub async fn start_device_grant(
    http: &reqwest::Client,
    endpoints: &IssuerEndpoints,
    client_id: &str,
) -> Result<DeviceAuthorization, CliError> {
    let response = http
        .post(&endpoints.device_authorization_endpoint)
        .form(&[("client_id", client_id), ("scope", SCOPES)])
        .send()
        .await
        .map_err(|err| {
            CliError::Failure(format!(
                "cannot reach the device authorization endpoint: {err}"
            ))
        })?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CliError::Failure(format!(
            "the issuer refused to start the device grant ({status}): {body}"
        )));
    }
    serde_json::from_str(&body).map_err(|err| {
        CliError::Failure(format!(
            "unreadable device authorization response: {err} (body: {body})"
        ))
    })
}

/// Poll the token endpoint until the user approves, declines, or the code
/// expires (RFC 8628 §3.4–3.5, honoring `interval` and `slow_down`).
pub async fn poll_device_grant(
    http: &reqwest::Client,
    endpoints: &IssuerEndpoints,
    client_id: &str,
    grant: &DeviceAuthorization,
) -> Result<TokenResponse, CliError> {
    let mut interval = grant.interval.unwrap_or(5).max(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(grant.expires_in);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if std::time::Instant::now() >= deadline {
            return Err(CliError::Auth(
                "the device code expired before the login was approved.\n\
                 Run `kairos login` again and approve promptly."
                    .to_string(),
            ));
        }
        let response = http
            .post(&endpoints.token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", &grant.device_code),
                ("client_id", client_id),
            ])
            .send()
            .await
            .map_err(|err| CliError::Failure(format!("cannot reach the token endpoint: {err}")))?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        match classify_poll_response(status, &body) {
            PollOutcome::Token(token) => return Ok(token),
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => interval += 5,
            PollOutcome::Denied => {
                return Err(CliError::Auth(
                    "the login was declined at the verification page.\n\
                     Run `kairos login` again if this was a mistake."
                        .to_string(),
                ));
            }
            PollOutcome::Expired => {
                return Err(CliError::Auth(
                    "the device code expired before the login was approved.\n\
                     Run `kairos login` again and approve promptly."
                        .to_string(),
                ));
            }
            PollOutcome::Fatal(message) => return Err(CliError::Failure(message)),
        }
    }
}

/// The refresh grant (RFC 6749 §6). Returns the new tokens; the caller
/// persists them (issuers like Dex rotate the refresh token).
pub async fn refresh_grant(
    http: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let response = http
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("scope", SCOPES),
        ])
        .send()
        .await
        .map_err(|err| format!("cannot reach the token endpoint: {err}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "the issuer rejected the refresh ({status}): {body}"
        ));
    }
    serde_json::from_str(&body).map_err(|err| format!("unreadable token response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every RFC 8628 poll outcome classifies correctly.
    #[test]
    fn poll_classification() {
        let token = classify_poll_response(
            200,
            r#"{"access_token":"a","refresh_token":"r","expires_in":86400}"#,
        );
        match token {
            PollOutcome::Token(t) => {
                assert_eq!(t.access_token, "a");
                assert_eq!(t.refresh_token.as_deref(), Some("r"));
                assert_eq!(t.expires_in, 86400);
            }
            other => panic!("expected Token, got {other:?}"),
        }

        assert!(matches!(
            classify_poll_response(400, r#"{"error":"authorization_pending"}"#),
            PollOutcome::Pending
        ));
        assert!(matches!(
            classify_poll_response(400, r#"{"error":"slow_down"}"#),
            PollOutcome::SlowDown
        ));
        assert!(matches!(
            classify_poll_response(400, r#"{"error":"access_denied"}"#),
            PollOutcome::Denied
        ));
        assert!(matches!(
            classify_poll_response(400, r#"{"error":"expired_token"}"#),
            PollOutcome::Expired
        ));
        // Unknown OAuth error → fatal, keeping the description.
        match classify_poll_response(
            400,
            r#"{"error":"invalid_grant","error_description":"nope"}"#,
        ) {
            PollOutcome::Fatal(message) => {
                assert!(message.contains("invalid_grant"), "{message}");
                assert!(message.contains("nope"), "{message}");
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
        // Non-JSON garbage → fatal with the status.
        assert!(matches!(
            classify_poll_response(502, "<html>bad gateway</html>"),
            PollOutcome::Fatal(_)
        ));
        // 2xx with an unreadable body → fatal, not a panic.
        assert!(matches!(
            classify_poll_response(200, "not json"),
            PollOutcome::Fatal(_)
        ));
    }

    /// A missing refresh_token in a token response stays None (some IdPs
    /// omit it unless offline_access is granted).
    #[test]
    fn token_response_without_refresh() {
        let token: TokenResponse =
            serde_json::from_str(r#"{"access_token":"a","expires_in":300}"#).expect("parses");
        assert_eq!(token.refresh_token, None);
    }

    /// `bearer_for` picks the right token; `id_token` mode fails cleanly when
    /// the issuer returned none (T-0054).
    #[test]
    fn bearer_selection() {
        let with_id: TokenResponse = serde_json::from_str(
            r#"{"access_token":"ya29.opaque","id_token":"eyJ.id","expires_in":3600}"#,
        )
        .expect("parses");
        assert_eq!(
            with_id.bearer_for(ApiBearer::AccessToken),
            Some("ya29.opaque")
        );
        assert_eq!(with_id.bearer_for(ApiBearer::IdToken), Some("eyJ.id"));

        let no_id: TokenResponse =
            serde_json::from_str(r#"{"access_token":"a","expires_in":300}"#).expect("parses");
        assert_eq!(no_id.bearer_for(ApiBearer::IdToken), None);
        assert_eq!(no_id.bearer_for(ApiBearer::AccessToken), Some("a"));
    }
}
