//! [`CachedTokenProvider`] — the CLI's implementation of
//! `kairos_client::TokenProvider` (the KAIROS-T-0024 seam): draws the
//! bearer token from the credential cache and transparently refreshes it
//! on (imminent) expiry, persisting rotated tokens back to disk. A failed
//! refresh surfaces as `Error::Token` with a re-login instruction, which
//! `main` maps to exit code 2 (KAIROS-A-0015).

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

        if !entry.needs_refresh(unix_now()) {
            return Ok(entry.access_token);
        }

        let Some(refresh_token) = entry.refresh_token.as_deref() else {
            return Err(Error::Token(format!(
                "the cached access token for {url} has expired and no refresh token was \
                 issued.\nRun `kairos login --url {url}` to re-authenticate.",
                url = self.deployment
            )));
        };

        let endpoints = oidc::discover_endpoints(&self.http, &entry.issuer)
            .await
            .map_err(|err| {
                Error::Token(format!(
                    "cannot refresh the access token ({err}).\n\
                     Run `kairos login --url {url}` to re-authenticate.",
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
                "the session for {url} has expired and could not be refreshed ({reason}).\n\
                 Run `kairos login --url {url}` to re-authenticate.",
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
                    "the refreshed session for {url} returned no id_token, which this \
                     deployment requires.\nRun `kairos login --url {url}` to re-authenticate.",
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
