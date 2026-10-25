//! The `request_key` of the MCP writes (KAIROS-T-0681, KAIROS-A-0065).
//!
//! A write can commit while its answer is lost: the request timeout of
//! KAIROS-A-0045, a reset during an upgrade, a client timeout. A retry of
//! `create_item` then made a second item, and a retry of `transition_item`
//! was refused with INVALID_TRANSITION, which hid the success. The tools of
//! decision 7 of the ADR take an optional `request_key`. The tool writes the key
//! row in the transaction of the write, before the write, and keeps the
//! answer text in it before the commit (`kairos_db::request_keys`). A
//! second call by the same caller with the same key in 24 hours writes
//! nothing and gives the first answer again, with a `Replayed:` line.
//!
//! The rules of the ADR, in short:
//! - The caller is the person and the agent key of the request.
//! - The key is 8 to 128 characters of `[A-Za-z0-9._:-]`.
//! - The same key with another tool or other arguments is refused with
//!   REQUEST_KEY_REUSED, and writes nothing.
//! - A refused write leaves no row (its transaction rolls back), so its
//!   retry runs again.

use axum::http::StatusCode;
use chrono::{SecondsFormat, Utc};
use diesel::pg::PgConnection;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use kairos_db::request_keys::{self, Caller, Claim};

use crate::api::atomically;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;

/// The arguments of a keyed tool.
pub(super) trait Keyed: Serialize {
    fn request_key(&self) -> Option<&str>;
}

/// A keyed call: the caller, the key, the tool and the fingerprint of the
/// arguments.
pub(super) struct KeyedCall {
    caller: Caller,
    key: String,
    tool: &'static str,
    fingerprint: Vec<u8>,
}

/// The keyed call of a tool, when its arguments have a `request_key`. A key
/// that does not obey the rule is refused with VALIDATION.
pub(super) fn keyed_call<P: Keyed>(
    auth: &AuthContext,
    tool: &'static str,
    params: &P,
) -> Result<Option<KeyedCall>, ApiError> {
    let Some(key) = params.request_key() else {
        return Ok(None);
    };
    if !request_keys::is_valid_key(key) {
        return Err(ApiError::validation(
            "The request_key must have 8 to 128 characters. Use only letters, digits, \
             '.', '_', ':' and '-'. A UUID is a good key.",
        )
        .with_details(json!({"argument": "request_key"})));
    }
    Ok(Some(KeyedCall {
        caller: Caller {
            user_id: auth.user_id,
            agent_key: auth.agent_key,
        },
        key: key.to_string(),
        tool,
        fingerprint: fingerprint(tool, params)?,
    }))
}

/// SHA-256 of the tool name and the JSON of the arguments without
/// `request_key`. The arguments go through their type, so an argument
/// that is not sent and one that is sent as null give the same value.
fn fingerprint<P: Serialize>(tool: &str, params: &P) -> Result<Vec<u8>, ApiError> {
    let mut value = serde_json::to_value(params).map_err(ApiError::internal)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("request_key");
    }
    let mut hash = Sha256::new();
    hash.update(tool.as_bytes());
    hash.update([0u8]);
    hash.update(value.to_string().as_bytes());
    Ok(hash.finalize().to_vec())
}

impl KeyedCall {
    /// Run the write `f` under the key, in one transaction.
    pub(super) fn run(
        self,
        conn: &mut PgConnection,
        f: impl FnOnce(&mut PgConnection) -> Result<String, ApiError>,
    ) -> Result<String, ApiError> {
        atomically(conn, |conn| {
            let claim = request_keys::claim(
                conn,
                self.caller,
                &self.key,
                self.tool,
                &self.fingerprint,
                Utc::now(),
            )
            .map_err(ApiError::internal)?;
            match claim {
                Claim::New(id) => {
                    let text = f(conn)?;
                    request_keys::store_answer(conn, id, &text).map_err(ApiError::internal)?;
                    Ok(text)
                }
                Claim::Earlier(earlier) => {
                    let at = earlier
                        .created_at
                        .to_rfc3339_opts(SecondsFormat::Secs, true);
                    if earlier.tool != self.tool || earlier.fingerprint != self.fingerprint {
                        return Err(reused(&self.key, &earlier.tool, &at));
                    }
                    let answer = earlier.answer.ok_or_else(|| {
                        ApiError::internal("a committed request key has no answer")
                    })?;
                    Ok(format!(
                        "{answer}\n\nReplayed: the call with this request_key at {at} applied \
                         the write. Nothing new was written."
                    ))
                }
            }
        })
    }
}

/// The refusal of a key that an earlier, different request used.
pub(crate) fn reused(key: &str, first_tool: &str, at: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "REQUEST_KEY_REUSED",
        "This request_key was used for a different request. Use a new request_key for a new \
         request.",
    )
    .with_details(json!({
        "request_key": key,
        "first_tool": first_tool,
        "first_call_at": at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Args {
        short_code: String,
        text: Option<String>,
        request_key: Option<String>,
    }

    fn args(text: Option<&str>, key: Option<&str>) -> Args {
        Args {
            short_code: "ACME-T-0001".into(),
            text: text.map(str::to_string),
            request_key: key.map(str::to_string),
        }
    }

    #[test]
    fn the_fingerprint_ignores_the_key_and_names_the_tool() {
        let a = fingerprint("append_note", &args(Some("x"), Some("key-one-1"))).unwrap();
        let b = fingerprint("append_note", &args(Some("x"), Some("key-two-2"))).unwrap();
        let c = fingerprint("append_note", &args(Some("y"), Some("key-one-1"))).unwrap();
        let d = fingerprint("create_item", &args(Some("x"), Some("key-one-1"))).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn the_refusal_of_a_reused_key_names_the_fix() {
        let e = reused("key-one-1", "create_item", "2026-10-24T12:00:00Z");
        assert_eq!(e.code, "REQUEST_KEY_REUSED");
        assert!(e.message.contains("Use a new request_key"), "{}", e.message);
        assert_eq!(e.details["first_tool"], "create_item");
    }
}
