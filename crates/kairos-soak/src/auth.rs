//! Workforce token supply: the Dex resource-owner password grant
//! (KAIROS-A-0010 dev/test issuer; Dex has no `client_credentials` grant,
//! so the service identity `svc@kairos.test` uses the same password path —
//! the decided KAIROS-T-0003 service-account pattern).
//!
//! Tokens are minted through the PUBLIC `kairos-cli` client because Dex
//! stamps `aud` = the requesting client id and the server validates `aud`
//! against `OIDC_AUDIENCE`; a token minted through the confidential
//! `kairos-svc` client carries `aud=kairos-svc` and is the wrong-audience
//! negative case in the server's middleware tests.
//!
//! [`PasswordToken`] caches the access token and re-runs the grant before
//! expiry, so hours-scale soak runs never fail on token expiry (the dev
//! Dex mints 24h tokens; refresh margin is generous anyway).

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use kairos_client::{Error, TokenProvider};
use serde_json::Value;
use tokio::sync::Mutex;

/// Refresh when the cached token is within this margin of expiry.
const REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// A self-refreshing password-grant token for one workforce identity.
pub struct PasswordToken {
    http: reqwest::Client,
    issuer: String,
    client_id: String,
    username: String,
    password: String,
    cached: Mutex<Option<(String, Instant)>>,
}

impl PasswordToken {
    pub fn new(
        http: reqwest::Client,
        issuer: &str,
        client_id: &str,
        username: &str,
        password: &str,
    ) -> Self {
        PasswordToken {
            http,
            issuer: issuer.to_string(),
            client_id: client_id.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            cached: Mutex::new(None),
        }
    }

    /// The current access token, minting/refreshing as needed.
    pub async fn token(&self) -> Result<String, Error> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expires_at)) = cached.as_ref()
            && Instant::now() + REFRESH_MARGIN < *expires_at
        {
            return Ok(token.clone());
        }
        let response = self
            .http
            .post(format!("{}/token", self.issuer))
            .form(&[
                ("grant_type", "password"),
                ("username", &self.username),
                ("password", &self.password),
                ("scope", "openid email profile"),
                ("client_id", &self.client_id),
            ])
            .send()
            .await
            .map_err(|e| Error::Token(format!("issuer unreachable at {}: {e}", self.issuer)))?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|e| {
            Error::Token(format!(
                "token response for {} not JSON: {e}",
                self.username
            ))
        })?;
        if !status.is_success() {
            return Err(Error::Token(format!(
                "password grant for {} failed: HTTP {status} {body}",
                self.username
            )));
        }
        let token = body["access_token"]
            .as_str()
            .ok_or_else(|| Error::Token(format!("no access_token for {}: {body}", self.username)))?
            .to_string();
        let expires_in = body["expires_in"].as_u64().unwrap_or(3600);
        *cached = Some((
            token.clone(),
            Instant::now() + Duration::from_secs(expires_in),
        ));
        Ok(token)
    }
}

impl TokenProvider for PasswordToken {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        Box::pin(self.token())
    }
}

/// A tiny deterministic xorshift64* RNG for op-mix selection and jitter —
/// avoids pulling a rand crate for what is a weighted die.
#[derive(Debug, Clone)]
pub struct SmallRng(u64);

impl SmallRng {
    pub fn new(seed: u64) -> SmallRng {
        SmallRng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..bound` (bound > 0).
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    /// Uniform float in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = SmallRng::new(42);
        let mut b = SmallRng::new(42);
        for _ in 0..100 {
            let (x, y) = (a.next_u64(), b.next_u64());
            assert_eq!(x, y);
        }
        for _ in 0..1000 {
            assert!(a.below(7) < 7);
            let u = a.unit();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
