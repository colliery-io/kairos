//! The API error type and its KAIROS-S-0005 JSON envelope
//! (`{"error": {"code", "message", "details"}}`), produced by every
//! middleware layer and handler in this crate (KAIROS-T-0017).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// An API-surface error: an HTTP status plus the S-0005 envelope fields.
///
/// Constructors exist for the codes this crate emits today; new codes are
/// added as new constructors (the code vocabulary is part of the API
/// contract, so it stays centralized here).
#[derive(Debug, Clone)]
pub struct ApiError {
    /// HTTP status the envelope ships with.
    pub status: StatusCode,
    /// Stable machine-readable code (e.g. `UNAUTHORIZED`).
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Structured extras; `{}` when there is nothing structured to add.
    pub details: Value,
}

impl ApiError {
    /// Build an error with empty `details`.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: json!({}),
        }
    }

    /// Attach structured details.
    #[must_use]
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    /// 401 `UNAUTHORIZED` — missing/invalid bearer token (KAIROS-A-0010).
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
    }

    /// 403 `MEMBERSHIP_REQUIRED` — authenticated but not a member of the
    /// resolved organization; per KAIROS-A-0010 the message tells the user
    /// to request access from an org admin.
    pub fn membership_required(slug: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "MEMBERSHIP_REQUIRED",
            format!(
                "you are not a member of organization {slug:?}; \
                 request access from an organization admin"
            ),
        )
        .with_details(json!({ "organization": slug }))
    }

    /// 403 `FORBIDDEN` — authenticated and a member, but not allowed.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }

    /// 404 `TENANT_NOT_FOUND` — no organization for the resolved slug (or
    /// no slug was resolvable from the request at all).
    pub fn tenant_not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "TENANT_NOT_FOUND", message)
    }

    /// 404 `NOT_FOUND` — generic missing resource.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    /// 409 `CONFLICT` — KAIROS-A-0004 optimistic-concurrency rejection.
    /// Callers attach the current entity state as `details.current` (S-0005
    /// "Conflict: 409 with current entity state").
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "CONFLICT", message)
    }

    /// 422 with a caller-chosen code — the semantic-rejection family
    /// (`VALIDATION`, `INVALID_TRANSITION`, `ITEM_NOT_ON_BOARD`, ...).
    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    /// 422 `VALIDATION` — a body/reference the request names is malformed
    /// or does not exist (bad UUID string, unknown board/template/parent,
    /// missing required field, unknown enum value).
    pub fn validation(message: impl Into<String>) -> Self {
        Self::unprocessable("VALIDATION", message)
    }

    /// 403 `FORBIDDEN` naming the missing KAIROS-A-0006 capability in
    /// `details.required_capability` (+ `details.board_id` when the check
    /// was board-scoped; `null` = the org-admin-only fallback applied).
    pub fn capability_required(capability: &str, board_id: Option<uuid::Uuid>) -> Self {
        let scope = match board_id {
            Some(board_id) => format!("capability {capability:?} on board {board_id}"),
            None => "organization admin role".to_string(),
        };
        Self::forbidden(format!("this action requires {scope}")).with_details(json!({
            "required_capability": capability,
            "board_id": board_id,
        }))
    }

    /// 500 `INTERNAL` — the message is logged; the envelope carries a
    /// generic message so internals never leak to clients.
    pub fn internal(context: impl std::fmt::Display) -> Self {
        tracing::error!(error = %context, "internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "internal server error",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "details": self.details,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_matches_s0005_shape() {
        let err = ApiError::unauthorized("bad token");
        let body = json!({
            "error": {
                "code": err.code,
                "message": err.message,
                "details": err.details,
            }
        });
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
        assert_eq!(body["error"]["message"], "bad token");
        assert_eq!(body["error"]["details"], json!({}));
    }

    #[test]
    fn membership_required_names_the_org_and_access_path() {
        let err = ApiError::membership_required("acme");
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert_eq!(err.code, "MEMBERSHIP_REQUIRED");
        assert!(err.message.contains("request access"));
        assert_eq!(err.details["organization"], "acme");
    }
}
