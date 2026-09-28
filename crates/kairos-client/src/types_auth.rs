//! The wire types of local password login (COLLIERY-T-0213): the bodies of
//! `POST /api/login`, and the part of `GET /api/config` that a client reads
//! before it logs in.
//!
//! These types hold a password and a session bearer. Both are secrets, so
//! both travel in a [`Secret`], and no type in this module can print one.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a [`Secret`] prints in place of its value.
pub const REDACTED: &str = "[REDACTED]";

/// A string that is a credential: a password, or a session bearer
/// (COLLIERY-T-0213).
///
/// The reason for the type is its `Debug`. A derived `Debug` prints every
/// field, so one `{:?}` of a request in an error path or a test failure puts
/// the password on a terminal or in a CI log. Here `Debug` prints a
/// placeholder, and there is no `Display`, so the only way to the value is
/// [`Secret::expose`], which a reviewer can search for.
///
/// The workspace has no `secrecy` crate. This type is small, and it is local
/// so that the feature does not add a dependency for one newtype.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wrap a credential.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The credential itself. Each call is a place where the value can
    /// leave the wrapper, so keep the calls few.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// True when the credential is the empty string.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// `POST /api/login` request body.
#[derive(Debug, Clone, Serialize)]
pub struct LoginRequest {
    /// The email of the account. The server matches it without regard to case.
    pub email: String,
    /// The password. Serialized as a plain JSON string; never printed.
    pub password: Secret,
}

/// `POST /api/login` 200 body. The server returns the token exactly once.
#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    /// The session bearer (`kairos_ss_<64-hex>`).
    pub token: Secret,
    /// When the session stops working (RFC 3339).
    pub expires_at: String,
    /// The account that the session authenticates.
    pub user: LoginUser,
}

/// The `user` object of [`LoginResponse`].
#[derive(Debug, Clone, Deserialize)]
pub struct LoginUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

/// The part of `GET /api/config` that decides how a client logs in
/// (COLLIERY-T-0213). The response has more fields, which are for the GUI.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeploymentConfig {
    /// The OIDC issuer, or `None` on a deployment that has none
    /// (KAIROS-T-0208).
    #[serde(default)]
    pub issuer: Option<String>,
    /// True when `POST /api/login` exists. A server older than local
    /// accounts does not send the field, and has none.
    #[serde(default)]
    pub local_auth: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct-horse-battery-staple";
    const TOKEN: &str =
        "kairos_ss_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// COLLIERY-T-0213: `Debug` of a secret, and of each type that holds
    /// one, prints the placeholder and not the value.
    #[test]
    fn debug_output_never_contains_the_secret() {
        let secret = Secret::new(PASSWORD);
        assert_eq!(format!("{secret:?}"), REDACTED);
        assert_eq!(format!("{secret:#?}"), REDACTED);

        let request = LoginRequest {
            email: "ada@example.test".into(),
            password: Secret::new(PASSWORD),
        };
        for rendered in [format!("{request:?}"), format!("{request:#?}")] {
            assert!(!rendered.contains(PASSWORD), "{rendered}");
            assert!(rendered.contains(REDACTED), "{rendered}");
            assert!(rendered.contains("ada@example.test"), "{rendered}");
        }

        let response: LoginResponse = serde_json::from_value(serde_json::json!({
            "token": TOKEN,
            "expires_at": "2026-10-11T00:00:00+00:00",
            "user": {"id": "u-1", "email": "ada@example.test", "display_name": "Ada"},
        }))
        .expect("login response");
        for rendered in [format!("{response:?}"), format!("{response:#?}")] {
            assert!(!rendered.contains(TOKEN), "{rendered}");
            assert!(!rendered.contains("kairos_ss_"), "{rendered}");
        }
        assert_eq!(response.token.expose(), TOKEN);
    }

    /// COLLIERY-T-0213: the token provider that `kairos logout` makes from
    /// the cached bearer does not print it, and the client that holds the
    /// provider does not print it.
    #[test]
    fn a_static_token_and_its_client_do_not_print_the_bearer() {
        let provider = crate::StaticToken(TOKEN.to_string());
        let client = crate::KairosClient::with_static_token("http://one.kairos.test", TOKEN);
        for rendered in [format!("{provider:?}"), format!("{client:?}")] {
            assert!(!rendered.contains(TOKEN), "{rendered}");
            assert!(!rendered.contains("kairos_ss_"), "{rendered}");
        }
    }

    /// The wrapper does not change the wire format: the server reads a
    /// plain string.
    #[test]
    fn a_secret_serializes_as_a_plain_string() {
        let request = LoginRequest {
            email: "ada@example.test".into(),
            password: Secret::new(PASSWORD),
        };
        assert_eq!(
            serde_json::to_value(&request).expect("json"),
            serde_json::json!({"email": "ada@example.test", "password": PASSWORD})
        );
    }

    /// `/api/config` of a deployment with no issuer, of one with both, and
    /// of a server that does not know the `local_auth` field.
    #[test]
    fn deployment_config_reads_the_two_fields() {
        let local_only: DeploymentConfig = serde_json::from_value(serde_json::json!({
            "issuer": null, "client_id": "kairos-web", "authorization_endpoint": null,
            "api_bearer": "access_token", "local_auth": true,
        }))
        .expect("config");
        assert_eq!(local_only.issuer, None);
        assert!(local_only.local_auth);

        let older: DeploymentConfig = serde_json::from_value(serde_json::json!({
            "issuer": "https://idp.example.test", "client_id": "kairos-web",
        }))
        .expect("config");
        assert_eq!(older.issuer.as_deref(), Some("https://idp.example.test"));
        assert!(!older.local_auth);
    }
}
