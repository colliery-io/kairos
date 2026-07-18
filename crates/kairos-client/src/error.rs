//! The typed client error (KAIROS-T-0024): every non-2xx API response is
//! mapped from its HTTP status + S-0005 envelope code into one variant, so
//! callers (CLI, tests, skills verification) match on variants instead of
//! re-parsing `{"error": {...}}` bodies.
//!
//! Mapping (KAIROS-S-0005 error contract):
//!
//! | status | code                 | variant                                |
//! |--------|----------------------|----------------------------------------|
//! | 401    | any                  | [`Error::Unauthorized`]                |
//! | 403    | any                  | [`Error::Forbidden`] (capability from `details.required_capability`) |
//! | 404    | any                  | [`Error::NotFound`]                    |
//! | 409    | any                  | [`Error::Conflict`] (current entity from `details.current`) |
//! | 400/422| `VALIDATION`         | [`Error::Validation`] (field from `details.field`) |
//! | 422    | `INVALID_TRANSITION` | [`Error::InvalidTransition`] (targets from `details.allowed_targets`) |
//! | other  | any                  | [`Error::Other`] (the code stays available) |
//!
//! Every API variant keeps the envelope's `code`, `message`, and full
//! `details` value, so no information is lost relative to the raw body.

use serde_json::Value;

use crate::types::ErrorEnvelope;

/// A failed client call: an API rejection (typed from the S-0005
/// envelope), a transport failure, or a malformed response.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 401 — missing/invalid bearer token (KAIROS-A-0010).
    #[error("401 {code}: {message}")]
    Unauthorized {
        /// Envelope code (`UNAUTHORIZED`).
        code: String,
        message: String,
        details: Value,
    },

    /// 403 — authenticated but not allowed (`FORBIDDEN`,
    /// `MEMBERSHIP_REQUIRED`, ...).
    #[error("403 {code}: {message}")]
    Forbidden {
        /// Envelope code.
        code: String,
        message: String,
        /// `details.required_capability` when the rejection was a
        /// KAIROS-A-0006 capability check; `None` otherwise (e.g.
        /// membership or deployment-admin gates).
        capability: Option<String>,
        details: Value,
    },

    /// 404 — unknown resource (`NOT_FOUND`, `TENANT_NOT_FOUND`, ...).
    #[error("404 {code}: {message}")]
    NotFound {
        /// Envelope code.
        code: String,
        message: String,
        details: Value,
    },

    /// 409 — conflict (`CONFLICT` optimistic-concurrency per KAIROS-A-0004,
    /// duplicate slugs, `DEFINITION_IN_USE`, ...).
    #[error("409 {code}: {message}")]
    Conflict {
        /// Envelope code.
        code: String,
        message: String,
        /// `details.current` — the CURRENT entity DTO on a stale-version
        /// rejection (`Value::Null` when the 409 carries none).
        current: Value,
        details: Value,
    },

    /// 422 `INVALID_TRANSITION` — the move is not in the board's
    /// transition graph.
    #[error("422 INVALID_TRANSITION: {message}")]
    InvalidTransition {
        message: String,
        /// `details.allowed_targets` — the columns reachable from the
        /// item's current column.
        allowed_targets: Value,
        details: Value,
    },

    /// `VALIDATION` at 422 (malformed bodies/references) or 400 (the
    /// search request pipeline, KAIROS-A-0007).
    #[error("{status} VALIDATION: {message}")]
    Validation {
        /// The HTTP status the rejection shipped with (400 or 422).
        status: u16,
        message: String,
        /// `details.field` — the offending field, when the server names
        /// exactly one.
        field: Option<String>,
        details: Value,
    },

    /// Any other enveloped rejection (`ITEM_NOT_ON_BOARD`, `LAST_ADMIN`,
    /// `DUPLICATE_COLUMN_POSITION`, `CONFIRMATION_REQUIRED`, 500
    /// `INTERNAL`, ...).
    #[error("{status} {code}: {message}")]
    Other {
        status: u16,
        /// Envelope code.
        code: String,
        message: String,
        details: Value,
    },

    /// The request never produced an HTTP response (connect/timeout/TLS).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// A 2xx body did not deserialize into the expected DTO.
    #[error("decoding {context}: {source}")]
    Decode {
        /// What was being decoded (method + path).
        context: String,
        #[source]
        source: serde_json::Error,
    },

    /// A non-2xx response without a parseable S-0005 envelope.
    #[error("{status} response without an S-0005 error envelope: {body}")]
    UnexpectedResponse { status: u16, body: String },

    /// The [`crate::TokenProvider`] could not produce a bearer token.
    #[error("token provider: {0}")]
    Token(String),

    /// A WebSocket-level failure on the `/ws/events` helper.
    #[error("websocket: {0}")]
    WebSocket(String),
}

impl Error {
    /// Map an S-0005 envelope + HTTP status to the typed variant.
    pub fn from_envelope(status: u16, envelope: ErrorEnvelope) -> Self {
        let ErrorEnvelope { error } = envelope;
        let (code, message, details) = (error.code, error.message, error.details);
        match status {
            401 => Error::Unauthorized {
                code,
                message,
                details,
            },
            403 => Error::Forbidden {
                capability: details["required_capability"].as_str().map(str::to_string),
                code,
                message,
                details,
            },
            404 => Error::NotFound {
                code,
                message,
                details,
            },
            409 => Error::Conflict {
                current: details["current"].clone(),
                code,
                message,
                details,
            },
            422 if code == "INVALID_TRANSITION" => Error::InvalidTransition {
                allowed_targets: details["allowed_targets"].clone(),
                message,
                details,
            },
            400 | 422 if code == "VALIDATION" => Error::Validation {
                status,
                field: details["field"].as_str().map(str::to_string),
                message,
                details,
            },
            _ => Error::Other {
                status,
                code,
                message,
                details,
            },
        }
    }

    /// The HTTP status of an API rejection (`None` for transport/decode
    /// level failures).
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Unauthorized { .. } => Some(401),
            Error::Forbidden { .. } => Some(403),
            Error::NotFound { .. } => Some(404),
            Error::Conflict { .. } => Some(409),
            Error::InvalidTransition { .. } => Some(422),
            Error::Validation { status, .. } | Error::Other { status, .. } => Some(*status),
            Error::UnexpectedResponse { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// The S-0005 envelope code of an API rejection.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Unauthorized { code, .. }
            | Error::Forbidden { code, .. }
            | Error::NotFound { code, .. }
            | Error::Conflict { code, .. }
            | Error::Other { code, .. } => Some(code),
            Error::InvalidTransition { .. } => Some("INVALID_TRANSITION"),
            Error::Validation { .. } => Some("VALIDATION"),
            _ => None,
        }
    }

    /// The S-0005 envelope message of an API rejection.
    pub fn message(&self) -> Option<&str> {
        match self {
            Error::Unauthorized { message, .. }
            | Error::Forbidden { message, .. }
            | Error::NotFound { message, .. }
            | Error::Conflict { message, .. }
            | Error::InvalidTransition { message, .. }
            | Error::Validation { message, .. }
            | Error::Other { message, .. } => Some(message),
            _ => None,
        }
    }

    /// The S-0005 envelope `details` of an API rejection.
    pub fn details(&self) -> Option<&Value> {
        match self {
            Error::Unauthorized { details, .. }
            | Error::Forbidden { details, .. }
            | Error::NotFound { details, .. }
            | Error::Conflict { details, .. }
            | Error::InvalidTransition { details, .. }
            | Error::Validation { details, .. }
            | Error::Other { details, .. } => Some(details),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope(code: &str, message: &str, details: Value) -> ErrorEnvelope {
        serde_json::from_value(json!({
            "error": {"code": code, "message": message, "details": details}
        }))
        .expect("envelope")
    }

    /// Every S-0005 family maps to its variant with the payload extracted.
    #[test]
    fn status_code_mapping() {
        let err = Error::from_envelope(401, envelope("UNAUTHORIZED", "no token", json!({})));
        assert!(matches!(err, Error::Unauthorized { .. }), "{err}");

        let err = Error::from_envelope(
            403,
            envelope(
                "FORBIDDEN",
                "denied",
                json!({"required_capability": "manage_tasks", "board_id": null}),
            ),
        );
        match &err {
            Error::Forbidden { capability, .. } => {
                assert_eq!(capability.as_deref(), Some("manage_tasks"));
            }
            other => panic!("expected Forbidden, got {other}"),
        }
        assert_eq!(err.status(), Some(403));
        assert_eq!(err.code(), Some("FORBIDDEN"));

        let err = Error::from_envelope(404, envelope("NOT_FOUND", "gone", json!({})));
        assert!(matches!(err, Error::NotFound { .. }), "{err}");

        let err = Error::from_envelope(
            409,
            envelope("CONFLICT", "stale", json!({"current": {"version": 3}})),
        );
        match &err {
            Error::Conflict { current, .. } => assert_eq!(current["version"], 3),
            other => panic!("expected Conflict, got {other}"),
        }

        let err = Error::from_envelope(
            422,
            envelope(
                "INVALID_TRANSITION",
                "not allowed",
                json!({"allowed_targets": [{"id": "c1"}]}),
            ),
        );
        match &err {
            Error::InvalidTransition {
                allowed_targets, ..
            } => assert_eq!(allowed_targets[0]["id"], "c1"),
            other => panic!("expected InvalidTransition, got {other}"),
        }

        for status in [400_u16, 422] {
            let err = Error::from_envelope(
                status,
                envelope("VALIDATION", "bad", json!({"field": "traverse.depth"})),
            );
            match &err {
                Error::Validation {
                    status: got, field, ..
                } => {
                    assert_eq!(*got, status);
                    assert_eq!(field.as_deref(), Some("traverse.depth"));
                }
                other => panic!("expected Validation, got {other}"),
            }
        }

        let err = Error::from_envelope(422, envelope("LAST_ADMIN", "no", json!({})));
        match &err {
            Error::Other { status, code, .. } => {
                assert_eq!(*status, 422);
                assert_eq!(code, "LAST_ADMIN");
            }
            other => panic!("expected Other, got {other}"),
        }
    }
}
