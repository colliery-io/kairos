//! The RFC 7644 §3.12 SCIM error envelope:
//! `{"schemas": ["urn:ietf:params:scim:api:messages:2.0:Error"],
//!   "status": "<http status>", "scimType": "...", "detail": "..."}`
//! served as `application/scim+json`. Every `/scim/v2` middleware layer and
//! handler produces this shape — the S-0005 envelope never leaks onto the
//! SCIM surface.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// The RFC 7644 error message URN.
pub const ERROR_URN: &str = "urn:ietf:params:scim:api:messages:2.0:Error";

/// The SCIM media type every `/scim/v2` response (success or error) ships
/// with. Requests are accepted with either `application/scim+json` or
/// `application/json` bodies (IdPs disagree; bodies are parsed manually).
pub const SCIM_CONTENT_TYPE: &str = "application/scim+json";

/// A SCIM-surface error (RFC 7644 §3.12).
#[derive(Debug, Clone)]
pub struct ScimError {
    /// HTTP status the envelope ships with (also echoed as `"status"`).
    pub status: StatusCode,
    /// RFC 7644 `scimType` keyword for 400-class errors, when one applies.
    pub scim_type: Option<&'static str>,
    /// Human-readable detail.
    pub detail: String,
}

impl ScimError {
    fn new(status: StatusCode, scim_type: Option<&'static str>, detail: impl Into<String>) -> Self {
        Self {
            status,
            scim_type,
            detail: detail.into(),
        }
    }

    /// 401 — missing/malformed/unknown/revoked SCIM bearer token. One
    /// uniform message: the response never distinguishes "no such tenant"
    /// from "wrong secret" from "revoked" (no enumeration oracle).
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, None, detail)
    }

    /// 404 — no SCIM resource with that id in this tenant's resource set.
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, None, detail)
    }

    /// 400 `invalidSyntax` — unparseable body / missing message schema.
    pub fn invalid_syntax(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some("invalidSyntax"), detail)
    }

    /// 400 `invalidValue` — a required value is missing or the wrong shape.
    pub fn invalid_value(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some("invalidValue"), detail)
    }

    /// 400 `invalidPath` — a PATCH path outside the supported subset.
    pub fn invalid_path(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some("invalidPath"), detail)
    }

    /// 400 `invalidFilter` — a filter outside the supported subset.
    pub fn invalid_filter(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some("invalidFilter"), detail)
    }

    /// 400 `mutability` — "the attempted modification is not compatible
    /// with the target attribute's mutability or current state" (RFC 7644
    /// §3.12). This is the SCIM rendering of the LAST_ADMIN guard (an org
    /// must retain at least one admin), of immutable-attribute writes
    /// (`userName`, group `displayName`), and of deleting the built-in
    /// admins group.
    pub fn mutability(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some("mutability"), detail)
    }

    /// 409 `uniqueness` — the resource already exists (duplicate user
    /// provision, duplicate group displayName).
    pub fn uniqueness(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, Some("uniqueness"), detail)
    }

    /// 500 — the context is logged; the envelope carries a generic detail
    /// so internals never leak to the IdP.
    pub fn internal(context: impl std::fmt::Display) -> Self {
        tracing::error!(error = %context, "scim internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            "internal server error",
        )
    }
}

/// Build a `/scim/v2` SUCCESS response: `application/scim+json` body.
pub fn scim_response(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, SCIM_CONTENT_TYPE)],
        body.to_string(),
    )
        .into_response()
}

impl IntoResponse for ScimError {
    fn into_response(self) -> Response {
        let mut body = json!({
            "schemas": [ERROR_URN],
            "status": self.status.as_u16().to_string(),
            "detail": self.detail,
        });
        if let Some(scim_type) = self.scim_type {
            body["scimType"] = json!(scim_type);
        }
        scim_response(self.status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_matches_rfc7644_shape() {
        let err = ScimError::mutability("last admin");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.scim_type, Some("mutability"));
        let err = ScimError::unauthorized("bad token");
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.scim_type, None);
        let err = ScimError::uniqueness("dup");
        assert_eq!(err.status, StatusCode::CONFLICT);
    }
}
