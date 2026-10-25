//! The `Idempotency-Key` header of the REST writes (KAIROS-T-0686,
//! KAIROS-A-0065).
//!
//! The REST routes of the writes of decision 7 of the ADR take an optional
//! `Idempotency-Key` header, with the rule of the MCP `request_key`
//! (`crate::mcp` KAIROS-T-0681): 8 to 128 characters of `[A-Za-z0-9._:-]`,
//! the caller is the person and the agent key, 24 hours. The key uses the
//! same `request_keys` table; the `tool` of the row is the method and the
//! route template (`POST /api/tasks`).
//!
//! The handler writes the key row in the transaction of the write, before
//! the write, and keeps the answer (the status and the JSON body) in it
//! before the commit. A second request with the same key, route and
//! arguments writes nothing: it gets the first status and body again, with
//! the header `Idempotent-Replayed: true`. The same key with another route
//! or other arguments is refused with REQUEST_KEY_REUSED (409). A refused
//! write leaves no row, so its retry runs again.

use axum::Json;
use axum::extract::{FromRequestParts, MatchedPath};
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{SecondsFormat, Utc};
use diesel::pg::PgConnection;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use kairos_db::request_keys::{self, Caller, Claim};

use crate::error::ApiError;
use crate::middleware::auth::AuthContext;

/// The request header.
pub const HEADER: &str = "Idempotency-Key";

/// The header of a replayed answer.
pub const REPLAYED_HEADER: &str = "Idempotent-Replayed";

/// The OpenAPI description of the header, for the `params` of each keyed
/// route.
pub const HEADER_DOC: &str = "Optional. A key that you choose for this write: 8 to 128 \
    characters of letters, digits, '.', '_', ':' and '-' (a UUID is a good key). Send it when \
    you may retry the request. For 24 hours, a request with the same key and the same \
    arguments writes nothing and gives the first status and body again, with the header \
    `Idempotent-Replayed: true`. The same key with other arguments is refused with \
    REQUEST_KEY_REUSED (KAIROS-A-0065).";

/// The `Idempotency-Key` of a request, when it has one: the caller, the
/// key and the route. Read it with the extractor; give the body to
/// [`IdempotencyKey::bind`].
#[derive(Debug, Clone)]
pub struct IdempotencyKey(Option<Header>);

#[derive(Debug, Clone)]
struct Header {
    caller: Caller,
    key: String,
    /// The method and the route template: the `tool` of the row.
    route: String,
    /// The path and the query of this request.
    target: String,
}

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        let mut values = parts.headers.get_all(HEADER).iter();
        let Some(value) = values.next() else {
            return Ok(Self(None));
        };
        if values.next().is_some() {
            return Err(invalid(
                "The request has more than one Idempotency-Key header. Send one.",
            ));
        }
        let key = value
            .to_str()
            .ok()
            .filter(|key| request_keys::is_valid_key(key))
            .ok_or_else(|| {
                invalid(
                    "The Idempotency-Key must have 8 to 128 characters. Use only letters, \
                     digits, '.', '_', ':' and '-'. A UUID is a good key.",
                )
            })?
            .to_string();
        let auth = parts
            .extensions
            .get::<AuthContext>()
            .ok_or_else(|| ApiError::internal("an Idempotency-Key route has no auth context"))?;
        let template = parts
            .extensions
            .get::<MatchedPath>()
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| parts.uri.path().to_string());
        Ok(Self(Some(Header {
            caller: Caller {
                user_id: auth.user_id,
                agent_key: auth.agent_key,
            },
            key,
            route: format!("{} {template}", parts.method),
            target: parts
                .uri
                .path_and_query()
                .map(|p| p.as_str().to_string())
                .unwrap_or_default(),
        })))
    }
}

fn invalid(message: &str) -> ApiError {
    ApiError::validation(message).with_details(json!({"header": HEADER}))
}

/// A keyed request: the key and the fingerprint of its arguments.
pub struct KeyedRequest {
    header: Header,
    fingerprint: Vec<u8>,
}

impl IdempotencyKey {
    /// The keyed request, when the request has a key. `body` is the request
    /// body as the handler read it (use `&()` for a route with no body).
    pub fn bind<B: Serialize>(self, body: &B) -> Result<Option<KeyedRequest>, ApiError> {
        let Some(header) = self.0 else {
            return Ok(None);
        };
        let body = serde_json::to_value(body).map_err(ApiError::internal)?;
        let fingerprint = fingerprint(&header.route, &header.target, &body);
        Ok(Some(KeyedRequest {
            header,
            fingerprint,
        }))
    }
}

/// SHA-256 of the route, the path and query, and the JSON of the body.
/// The body goes through its type, so a field that is not sent and one
/// that is sent as null give the same value.
fn fingerprint(route: &str, target: &str, body: &Value) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(route.as_bytes());
    hash.update([0u8]);
    hash.update(target.as_bytes());
    hash.update([0u8]);
    hash.update(body.to_string().as_bytes());
    hash.finalize().to_vec()
}

/// The answer of a keyed route.
#[derive(Debug)]
pub enum Answer<T> {
    /// The write ran now.
    Fresh(StatusCode, T),
    /// An earlier request with the key applied the write: its status and
    /// body.
    Replayed(StatusCode, Value),
}

impl<T> Answer<T> {
    /// True when the write ran now (a replay must not do the work after
    /// the write again).
    pub fn is_fresh(&self) -> bool {
        matches!(self, Self::Fresh(..))
    }
}

impl<T: Serialize> IntoResponse for Answer<T> {
    fn into_response(self) -> Response {
        match self {
            Self::Fresh(status, body) => (status, Json(body)).into_response(),
            Self::Replayed(status, body) => {
                let mut response = (status, Json(body)).into_response();
                response.headers_mut().insert(
                    HeaderName::from_static("idempotent-replayed"),
                    HeaderValue::from_static("true"),
                );
                response
            }
        }
    }
}

/// The stored answer of a keyed route: the text of the `answer` column.
#[derive(Serialize, serde::Deserialize)]
struct Stored {
    status: u16,
    body: Value,
}

/// Run the write `f` of a route, under the key when there is one, in one
/// transaction. `status` is the status of a fresh answer.
pub fn run<T: Serialize>(
    conn: &mut PgConnection,
    keyed: Option<KeyedRequest>,
    status: StatusCode,
    f: impl FnOnce(&mut PgConnection) -> Result<T, ApiError>,
) -> Result<Answer<T>, ApiError> {
    run_with_status(conn, keyed, |conn| f(conn).map(|body| (status, body)))
}

/// [`run`] for a route whose write chooses the status of its answer.
pub fn run_with_status<T: Serialize>(
    conn: &mut PgConnection,
    keyed: Option<KeyedRequest>,
    f: impl FnOnce(&mut PgConnection) -> Result<(StatusCode, T), ApiError>,
) -> Result<Answer<T>, ApiError> {
    let Some(keyed) = keyed else {
        return f(conn).map(|(status, body)| Answer::Fresh(status, body));
    };
    super::atomically(conn, |conn| {
        let claim = request_keys::claim(
            conn,
            keyed.header.caller,
            &keyed.header.key,
            &keyed.header.route,
            &keyed.fingerprint,
            Utc::now(),
        )
        .map_err(ApiError::internal)?;
        match claim {
            Claim::New(id) => {
                let (status, body) = f(conn)?;
                let stored = Stored {
                    status: status.as_u16(),
                    body: serde_json::to_value(&body).map_err(ApiError::internal)?,
                };
                let text = serde_json::to_string(&stored).map_err(ApiError::internal)?;
                request_keys::store_answer(conn, id, &text).map_err(ApiError::internal)?;
                Ok(Answer::Fresh(status, body))
            }
            Claim::Earlier(earlier) => {
                if earlier.tool != keyed.header.route || earlier.fingerprint != keyed.fingerprint {
                    let at = earlier
                        .created_at
                        .to_rfc3339_opts(SecondsFormat::Secs, true);
                    return Err(crate::mcp::request_key_reused(
                        &keyed.header.key,
                        &earlier.tool,
                        &at,
                    ));
                }
                let stored: Stored = earlier
                    .answer
                    .as_deref()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .ok_or_else(|| {
                        ApiError::internal("a committed Idempotency-Key has no answer")
                    })?;
                let status = StatusCode::from_u16(stored.status).map_err(ApiError::internal)?;
                Ok(Answer::Replayed(status, stored.body))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_names_the_route_the_path_and_the_body() {
        let body = json!({"title": "x"});
        let a = fingerprint("POST /api/tasks", "/api/tasks", &body);
        assert_eq!(a, fingerprint("POST /api/tasks", "/api/tasks", &body));
        assert_ne!(
            a,
            fingerprint("POST /api/tasks", "/api/tasks", &json!({"title": "y"}))
        );
        assert_ne!(a, fingerprint("POST /api/adrs", "/api/adrs", &body));
        assert_ne!(
            fingerprint(
                "POST /api/tasks/{short_code}/move",
                "/api/tasks/ACME-T-0001/move",
                &body
            ),
            fingerprint(
                "POST /api/tasks/{short_code}/move",
                "/api/tasks/ACME-T-0002/move",
                &body
            )
        );
    }

    #[test]
    fn a_replay_gives_the_status_and_the_body_with_the_header() {
        let response = Answer::<Value>::Replayed(StatusCode::CREATED, json!({"short_code": "A"}))
            .into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[REPLAYED_HEADER], "true");
        let fresh = Answer::Fresh(StatusCode::OK, json!({})).into_response();
        assert!(fresh.headers().get(REPLAYED_HEADER).is_none());
    }
}
