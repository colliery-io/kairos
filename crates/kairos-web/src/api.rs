//! The SPA data layer (KAIROS-T-0039): a thin fetch wrapper over the
//! same-origin `/api`, plus the mirror DTOs the shell needs.
//!
//! # Why not `kairos-client`? (decision, KAIROS-T-0039)
//!
//! `kairos-client` does not compile for `wasm32-unknown-unknown` — its
//! transport is native (`tokio` "full", `tokio-tungstenite`; `getrandom`
//! without a wasm backend), and A-0015 anticipated exactly this ("or a
//! `kairos-types` sub-crate if WASM feature-gating demands it"). Rather
//! than restructure that crate from this task, the GUI:
//!
//! - talks HTTP through `gloo-net` (the browser `fetch` API), and
//! - declares **mirror DTOs**: local structs with the exact wire field
//!   names, deserialized with serde like everywhere else. Mirrors are
//!   *partial on purpose* — declare only the fields the view consumes;
//!   serde ignores the rest, so additive API changes never break the GUI.
//!
//! Follow-up recorded in the task: split the plain-serde types out of
//! `kairos-client` into a `kairos-types` crate both sides consume. Until
//! then, every mirror carries a `// mirror of:` comment naming its source
//! type so drift is greppable.
//!
//! # Conventions (binding for T-0040..T-0044 — see docs/gui-conventions.md)
//!
//! - Every call goes through [`get_json`] (add `post_json`/`patch_json`
//!   siblings here as needed — same shape).
//! - Errors are `aurora_dark::tokens::ApiError` — exactly what
//!   `<ErrorState/>` consumes; the S-0005 error envelope is mapped in one
//!   place ([`error_from_response`]).
//! - A 401 clears the session (the protected shell then re-runs the
//!   login redirect); components never handle 401 themselves.

use aurora_dark::tokens::ApiError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::Auth;

/// `GET {path}` with the bearer token; JSON-decode the body.
pub async fn get_json<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::get(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `POST {path}` with a JSON body and the bearer token; JSON-decode the
/// response body (same shape as [`get_json`], per docs/gui-conventions.md).
pub async fn post_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::post(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| ApiError::Unknown(format!("encoding {path}: {e}")))?
        .send()
        .await
        .map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `PATCH {path}` with a JSON body and the bearer token; JSON-decode the
/// response body (same shape as [`get_json`], per docs/gui-conventions.md).
pub async fn patch_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::patch(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| ApiError::Unknown(format!("encoding {path}: {e}")))?
        .send()
        .await
        .map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `DELETE {path}` with the bearer token; JSON-decode the response body.
pub async fn delete_json<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::delete(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// The shared response tail of every `*_json` helper: 401 clears the
/// session (the protected shell then re-runs the login redirect), non-2xx
/// maps through the S-0005 envelope, 2xx JSON-decodes.
///
/// `sent_token` is the bearer THIS request carried. A 401 only expires the
/// session when that token is still the current one (KAIROS-T-0071): a 401
/// from a token-less request (resources fire eagerly while the boot-time
/// session restore is in flight) or from a token that has since been
/// replaced proves nothing about the current session — expiring on those
/// stomped the freshly restored session and bounced reloads to the issuer.
async fn decode_response<T: DeserializeOwned>(
    auth: Auth,
    sent_token: Option<String>,
    path: &str,
    response: gloo_net::http::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    if status == 401 && sent_token.is_some() && auth.token() == sent_token {
        // Convention: expired/invalid CURRENT token → drop the session;
        // the protected shell reacts with the issuer redirect (silent
        // re-auth when the IdP still holds a session).
        auth.expire();
    }
    if !(200..300).contains(&status) {
        return Err(error_from_response(status, response).await);
    }
    response
        .json::<T>()
        .await
        .map_err(|e| ApiError::Unknown(format!("decoding {path}: {e}")))
}

/// Map a non-2xx response onto [`ApiError`] via the S-0005 error envelope
/// (`{"error": {"code", "message", …}}`), falling back to the raw body.
async fn error_from_response(status: u16, response: gloo_net::http::Response) -> ApiError {
    let body = response.text().await.unwrap_or_default();
    let envelope: Option<ErrorEnvelope> = serde_json::from_str(&body).ok();
    let (message, code) = match envelope {
        Some(envelope) => (envelope.error.message, Some(envelope.error.code)),
        None => (body, None),
    };
    ApiError::Http {
        status,
        message,
        code,
    }
}

/// mirror of: `kairos_client::types::ErrorEnvelope` (S-0005).
#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

/// mirror of: `kairos_client::types::ErrorBody` (S-0005).
#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

// ---- whoami (the shell's only data need) --------------------------------

/// `GET /api/whoami`.
pub async fn whoami(auth: Auth) -> Result<Whoami, ApiError> {
    get_json(auth, "/api/whoami").await
}

/// mirror of: `kairos_server::app::WhoamiResponse` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Whoami {
    pub user: WhoamiUser,
    pub organization: WhoamiOrganization,
    #[serde(default)]
    pub teams: Vec<WhoamiTeam>,
    /// Board-scoped capability grants (KAIROS-A-0006), grouped by board —
    /// drives per-board admin gating (KAIROS-T-0052) for non-admins.
    #[serde(default)]
    pub capabilities: Vec<WhoamiBoardCapabilities>,
}

/// mirror of: `kairos_server::app::WhoamiBoardCapabilities` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiBoardCapabilities {
    pub board_slug: String,
    pub grants: Vec<String>,
}

/// mirror of: `kairos_server::app::WhoamiUser` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiUser {
    pub display_name: String,
    pub email: String,
}

/// mirror of: `kairos_server::app::WhoamiOrganization` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiOrganization {
    pub slug: String,
    pub role: String,
}

/// mirror of: `kairos_server::app::WhoamiTeam` (partial). `id` feeds the
/// KAIROS-T-0072 client-side capability mirror (team-owned boards carry
/// `team_id`; matching it against my teams decides the implied powers).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiTeam {
    pub id: String,
    pub slug: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mirror decodes a real WhoamiResponse body (field-name lock).
    #[test]
    fn whoami_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "user": {
                "id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                "external_id": "sub-123",
                "email": "alice@kairos.test",
                "display_name": "alice"
            },
            "organization": {
                "id": "0a8e9f7d-58f7-4f6e-9f0f-4dbb1a8f3e21",
                "slug": "demo",
                "role": "admin"
            },
            "teams": [{"id": "b1e2…", "slug": "platform", "name": "Platform"}],
            "capabilities": [{
                "board_id": "c3f4a5b6-1234-4c66-9e46-94e0d9e0f70f",
                "board_slug": "platform-delivery",
                "grants": ["configure_boards", "manage_members"]
            }]
        });
        let whoami: Whoami = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(whoami.user.display_name, "alice");
        assert_eq!(whoami.organization.slug, "demo");
        assert_eq!(whoami.teams[0].slug, "platform");
        assert_eq!(whoami.teams[0].id, "b1e2…");
        assert_eq!(whoami.capabilities[0].board_slug, "platform-delivery");
        assert_eq!(whoami.capabilities[0].grants.len(), 2);
    }
}
