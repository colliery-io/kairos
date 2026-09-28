//! [`CachedTokenProvider`] — the CLI's implementation of
//! `kairos_client::TokenProvider` (the KAIROS-T-0024 seam): draws the
//! bearer token from the credential cache and transparently refreshes it
//! on (imminent) expiry, persisting rotated tokens back to disk. A failed
//! refresh surfaces as `Error::Token` with a re-login instruction, which
//! `main` maps to exit code 2 (KAIROS-A-0015).
//!
//! A local session (COLLIERY-T-0213) is not refreshed. It is used until it
//! expires, and then the same `Error::Token` gives the command to log in
//! again.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use kairos_client::{Error, TokenProvider};
use tokio::sync::Mutex;

use crate::credentials::{self, unix_now};
use crate::oidc;

/// A refreshing token provider over one deployment's cache entry.
pub struct CachedTokenProvider {
    /// `credentials.json`.
    path: PathBuf,
    /// Normalized deployment URL (the cache key).
    deployment: String,
    http: reqwest::Client,
    /// Serializes refreshes so concurrent requests don't race the
    /// (rotating) refresh token.
    refresh_lock: Mutex<()>,
}

impl CachedTokenProvider {
    pub fn new(path: PathBuf, deployment: String) -> Self {
        Self {
            path,
            deployment,
            http: reqwest::Client::new(),
            refresh_lock: Mutex::new(()),
        }
    }

    /// The current access token, refreshed (and persisted) when it is
    /// expired or inside the skew window.
    async fn current_token(&self) -> Result<String, Error> {
        let _guard = self.refresh_lock.lock().await;
        // Re-read the cache on every request: another process (or an
        // earlier refresh in this one) may have rotated the tokens.
        let mut store =
            credentials::load(&self.path).map_err(|err| Error::Token(err.to_string()))?;
        let entry = credentials::entry_for(&store, &self.deployment)
            .map_err(|err| Error::Token(err.to_string()))?;

        // A local session (COLLIERY-T-0213) has no issuer and no refresh
        // token, so it never reaches the refresh below. When it is over, the
        // request is not sent: the server would refuse the bearer, and the
        // 401 cannot name the command that the person needs.
        if entry.is_local_session() {
            if entry.is_expired(unix_now()) {
                return Err(Error::Token(
                    entry.session_expired_message(&self.deployment),
                ));
            }
            return Ok(entry.access_token);
        }

        if !entry.needs_refresh(unix_now()) {
            return Ok(entry.access_token);
        }

        let Some(refresh_token) = entry.refresh_token.as_deref() else {
            return Err(Error::Token(format!(
                "The access token for {url} expired, and the cache has no refresh token.\n\
                 Run `kairos login --url {url}` to log in again.",
                url = self.deployment
            )));
        };

        let endpoints = oidc::discover_endpoints(&self.http, &entry.issuer)
            .await
            .map_err(|err| {
                Error::Token(format!(
                    "The CLI cannot refresh the access token ({err}).\n\
                     Run `kairos login --url {url}` to log in again.",
                    url = self.deployment
                ))
            })?;
        let refreshed = oidc::refresh_grant(
            &self.http,
            &endpoints.token_endpoint,
            &entry.client_id,
            refresh_token,
        )
        .await
        .map_err(|reason| {
            Error::Token(format!(
                "The session for {url} expired, and the CLI could not refresh it ({reason}).\n\
                 Run `kairos login --url {url}` to log in again.",
                url = self.deployment
            ))
        })?;

        // Re-select the same bearer kind the session was established with
        // (T-0054): an `id_token` deployment must keep sending the id_token.
        let mut updated = entry;
        let new_bearer = refreshed
            .bearer_for(updated.api_bearer)
            .ok_or_else(|| {
                Error::Token(format!(
                    "The refresh of the session for {url} gave no id_token. This deployment \
                     must have an id_token.\n\
                     Run `kairos login --url {url}` to log in again.",
                    url = self.deployment
                ))
            })?
            .to_string();
        updated.access_token = new_bearer.clone();
        updated.expires_at = unix_now() + refreshed.expires_in;
        // Issuers may rotate the refresh token; keep the old one when the
        // response omits it.
        if refreshed.refresh_token.is_some() {
            updated.refresh_token = refreshed.refresh_token;
        }
        store.deployments.insert(self.deployment.clone(), updated);
        credentials::save(&self.path, &store).map_err(|err| Error::Token(err.to_string()))?;

        Ok(new_bearer)
    }
}

impl TokenProvider for CachedTokenProvider {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        Box::pin(self.current_token())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialStore, DeploymentCredentials};
    use kairos_client::types_auth::Secret;

    const DEPLOYMENT: &str = "http://one.kairos.test";
    const BEARER: &str =
        "kairos_ss_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn provider_with(name: &str, expires_at: u64) -> (CachedTokenProvider, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("kairos-cli-provider-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("credentials.json");
        let mut store = CredentialStore::default();
        store.deployments.insert(
            DEPLOYMENT.into(),
            DeploymentCredentials::local_session(
                &Secret::new(BEARER),
                expires_at,
                "ada@example.test".into(),
                None,
            ),
        );
        credentials::save(&path, &store).expect("save");
        (CachedTokenProvider::new(path, DEPLOYMENT.to_string()), dir)
    }

    /// COLLIERY-T-0213: a local session that is not over gives its bearer.
    #[tokio::test]
    async fn a_live_local_session_gives_its_bearer() {
        let (provider, dir) = provider_with("live", unix_now() + 3600);
        assert_eq!(provider.current_token().await.expect("bearer"), BEARER);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// COLLIERY-T-0213: a local session that is over gives no bearer, so no
    /// request goes out. The error names the login command, and it does not
    /// speak of a refresh: the entry has an empty issuer, and a refresh
    /// would try discovery against the empty string.
    #[tokio::test]
    async fn an_expired_local_session_gives_the_login_command() {
        let (provider, dir) = provider_with("expired", 1);
        let err = provider.current_token().await.expect_err("expired");
        let message = match err {
            Error::Token(message) => message,
            other => panic!("expected a token error, got {other}"),
        };
        assert_eq!(
            message,
            "The session for http://one.kairos.test expired.\n\
             Run `kairos login --url http://one.kairos.test --email ada@example.test` to log \
             in again."
        );
        assert!(!message.contains("refresh"), "{message}");
        assert!(!message.contains(BEARER), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
