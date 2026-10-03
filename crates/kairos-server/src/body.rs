//! The JSON body of a write route (COLLIERY-T-0249).
//!
//! [`ApiJson`] is the extractor that each write route of the API uses in
//! place of `axum::Json`. It reads the body the same way. The difference is
//! the refusal: a body that the route cannot read gets the KAIROS-S-0005
//! envelope ([`ApiError`]) with the code `VALIDATION`, and not the plain
//! text of axum.
//!
//! The rule of the ticket is in the request types: each has
//! `#[serde(deny_unknown_fields)]`, so a body with a field that the route
//! does not know does not deserialize. This module makes the refusal name
//! the field ([`unknown_field`]).
//!
//! | The body | Status | `details` |
//! |---|---|---|
//! | has a field that the route does not know | 422 | `field`, `allowed` |
//! | has no field that the route must have | 422 | `field` |
//! | has a different fault of shape (a type is wrong) | 422 | — |
//! | is not JSON | 400 | — |
//! | has no `Content-Type: application/json` | 415 | — |
//!
//! The statuses are those that axum gave before, so a client that reads
//! the status only sees no change.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::error::ApiError;

/// The JSON body of a write route. See the module docs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(body)) => Ok(ApiJson(body)),
            Err(rejection) => Err(body_rejection(&rejection)),
        }
    }
}

/// The refusal of a body that the route cannot read.
fn body_rejection(rejection: &JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::JsonDataError(e) => {
            shape_error(StatusCode::UNPROCESSABLE_ENTITY, &innermost(e))
        }
        JsonRejection::JsonSyntaxError(e) => ApiError::new(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            format!("The body is not JSON: {}.", innermost(e)),
        ),
        JsonRejection::MissingJsonContentType(_) => ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "VALIDATION",
            "The request has a body, and the header `Content-Type` is not `application/json`. \
             Send the body as JSON with that header.",
        ),
        // The body did not arrive: too large, or the connection failed.
        other => ApiError::new(other.status(), "VALIDATION", other.body_text()),
    }
}

/// The message of the deepest cause of an error: the message of
/// `serde_json`, without the prefix of axum. For a fault of shape it starts
/// with the path to the fault (`metadata[0].default: `).
pub(crate) fn innermost(error: &dyn std::error::Error) -> String {
    let mut cause = error;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause.to_string()
}

/// The refusal of a body that is JSON and has the wrong shape, from the
/// message of `serde`. An unknown field gets a message that names it
/// (COLLIERY-T-0249). Each other fault keeps the message of `serde`.
///
/// `status` is 422, and 400 for `POST /api/search`.
pub fn shape_error(status: StatusCode, serde_message: &str) -> ApiError {
    match unknown_field(serde_message) {
        Some((field, allowed)) => {
            let known = if allowed.is_empty() {
                "The body of this route has no fields.".to_string()
            } else {
                format!("The fields of the body are: {}.", allowed.join(", "))
            };
            ApiError::new(
                status,
                "VALIDATION",
                format!(
                    "The body has the field {field:?}. This route does not accept that \
                     field. {known}"
                ),
            )
            .with_details(json!({ "field": field, "allowed": allowed }))
        }
        None => {
            let error = ApiError::new(
                status,
                "VALIDATION",
                format!("The body is not correct for this route: {serde_message}."),
            );
            // COLLIERY-T-3099: a refusal of an absent field names the field
            // in `details`, as the refusal of an unknown field does.
            match missing_field(serde_message) {
                Some(field) => error.with_details(json!({ "field": field })),
                None => error,
            }
        }
    }
}

/// The field that a message of `serde` says is absent: ``missing field
/// `x` ``, after the path of the field for a nested field. `None` when the
/// message is about a different fault.
fn missing_field(serde_message: &str) -> Option<String> {
    let (path, rest) = serde_message.split_once("missing field `")?;
    let (field, _) = rest.split_once('`')?;
    // The message of a nested field starts with its path: `a.b: missing
    // field `c``. A value that has the text is not a fault of this kind.
    if !(path.is_empty() || path.ends_with(": ")) || path.contains('"') {
        return None;
    }
    let parent = path.trim_end_matches(": ");
    Some(if parent.is_empty() || parent == "." {
        field.to_string()
    } else {
        format!("{parent}.{field}")
    })
}

/// The unknown field that a message of `serde` names, and the fields that
/// the type has. `None` when the message is about a different fault.
///
/// The message is that of `serde::de::Error::unknown_field`:
/// ``unknown field `x`, expected one of `a`, `b`, `c` ``, or
/// ``expected `a` or `b` `` for 2 fields, or ``expected `a` `` for one, or
/// `there are no fields`. `serde_json` adds the line and the column. The
/// extractor of axum puts the path to the fault before the message
/// (`metadata[0].default: `).
pub fn unknown_field(serde_message: &str) -> Option<(String, Vec<String>)> {
    const START: &str = "unknown field `";
    let at = serde_message.find(START)?;
    // Before the message there is nothing, or a path. A path has no quote:
    // a message that only quotes this text from a value is not a match.
    let path = &serde_message[..at];
    if !(path.is_empty() || path.ends_with(": ")) || path.contains(['"', '`']) {
        return None;
    }
    let (field, rest) = serde_message[at + START.len()..].split_once('`')?;
    // Each odd part between backticks is the name of a field.
    let allowed = rest
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    Some((field.to_string(), allowed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Two {
        name: String,
        #[serde(default)]
        slug: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Three {
        name: String,
        #[serde(default)]
        slug: Option<String>,
        #[serde(default)]
        team_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct One {
        name: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Zero {}

    fn message_of<T: DeserializeOwned + std::fmt::Debug>(body: &str) -> String {
        serde_json::from_str::<T>(body)
            .expect_err("the body is refused")
            .to_string()
    }

    /// COLLIERY-T-0249: the refusal names the field, and it gives the
    /// fields that the route has.
    #[test]
    fn an_unknown_field_is_named() {
        let message = message_of::<Three>(r#"{"name": "n", "board_level": "delivery"}"#);
        assert_eq!(
            unknown_field(&message),
            Some((
                "board_level".to_string(),
                vec![
                    "name".to_string(),
                    "slug".to_string(),
                    "team_id".to_string()
                ]
            ))
        );
        let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The body has the field \"board_level\". This route does not accept that field. \
             The fields of the body are: name, slug, team_id."
        );
        assert_eq!(
            err.details,
            json!({"field": "board_level", "allowed": ["name", "slug", "team_id"]})
        );
    }

    /// The message of `serde` has 4 forms, by the number of fields.
    #[test]
    fn a_type_with_one_field_and_a_type_with_none() {
        let message = message_of::<One>(r#"{"name": "n", "id": "x"}"#);
        assert_eq!(
            unknown_field(&message),
            Some(("id".to_string(), vec!["name".to_string()]))
        );
        let message = message_of::<Two>(r#"{"name": "n", "id": "x"}"#);
        assert_eq!(
            unknown_field(&message),
            Some((
                "id".to_string(),
                vec!["name".to_string(), "slug".to_string()]
            ))
        );
        let message = message_of::<Zero>(r#"{"id": "x"}"#);
        assert_eq!(unknown_field(&message), Some(("id".to_string(), vec![])));
        assert_eq!(
            shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message).message,
            "The body has the field \"id\". This route does not accept that field. The body \
             of this route has no fields."
        );
    }

    /// The extractor of axum puts the path before the message.
    #[test]
    fn the_path_before_the_message_is_permitted() {
        assert_eq!(
            unknown_field(
                "metadata[0].default: unknown field `default`, expected one of \
                 `definition_slug`, `default_value`, `required` at line 1 column 80"
            ),
            Some((
                "default".to_string(),
                vec![
                    "definition_slug".to_string(),
                    "default_value".to_string(),
                    "required".to_string()
                ]
            ))
        );
        // A value that has the text of the message is not an unknown field.
        assert_eq!(
            unknown_field("name: invalid type: string \"unknown field `x`\", expected a sequence"),
            None
        );
    }

    /// A different fault is not an unknown field.
    #[test]
    fn a_missing_field_keeps_the_message_of_serde() {
        let message = message_of::<Two>(r#"{"slug": "s"}"#);
        assert_eq!(unknown_field(&message), None);
        let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        assert_eq!(err.code, "VALIDATION");
        assert!(err.message.contains("missing field `name`"), "{err:?}");
        assert_eq!(err.details, json!({ "field": "name" }));
    }
}
