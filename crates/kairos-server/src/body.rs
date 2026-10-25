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
//! | has a different fault of shape (a type is wrong) | 422 | `field`, `expected` when known |
//! | is not JSON | 400 | — |
//!
//! KAIROS-T-0697: no refusal passes the text of `serde`. [`Fault`] puts
//! the fault in Kairos words: what the value must be, and the values of a
//! list of values.
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
            not_json(&innermost(e)),
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
/// (COLLIERY-T-0249). Each other fault is in Kairos words ([`Fault`],
/// KAIROS-T-0697).
///
/// `status` is 422, and 400 for `POST /api/search`.
pub fn shape_error(status: StatusCode, serde_message: &str) -> ApiError {
    match unknown_field(serde_message) {
        // COLLIERY-T-3101: `code_prefix` is a field of the create of a board
        // (and of a team). A body of a different route that has it is an
        // attempt to change the prefix, which does not change
        // (COLLIERY-T-3099). Say that, not only "this route does not accept
        // that field".
        Some((field, allowed)) if field == "code_prefix" => ApiError::new(
            status,
            "CODE_PREFIX_IS_FIXED",
            "The short-code prefix of a board does not change. The codes of the items on \
             the board use it. Remove code_prefix from the request. To give an item a code \
             with a different prefix, move it to a different board with rename.",
        )
        .with_details(json!({ "field": field, "allowed": allowed })),
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
        // COLLIERY-T-3099: a refusal of an absent field names the field in
        // `details`, as the refusal of an unknown field does.
        None => match missing_field(serde_message) {
            Some(field) => ApiError::new(
                status,
                "VALIDATION",
                format!(
                    "The body does not have the field {field:?}. This route must have that \
                     field."
                ),
            )
            .with_details(json!({ "field": field })),
            None => {
                let (path, fault) = split_path(serde_message);
                let fault = Fault::of(fault);
                // The root (no path): the body is not one object.
                let message = match path {
                    Some(field) => fault.sentence(&format!("the field {field}"), "body"),
                    None => "The body must be one JSON object with the fields of this route. \
                             The REST reference gives the fields."
                        .to_string(),
                };
                ApiError::new(status, "VALIDATION", message).with_details(fault.details(path))
            }
        },
    }
}

/// The refusal text of a body that is not JSON: where the fault is, from
/// the message of `serde_json` (`... at line 1 column 15`).
fn not_json(serde_message: &str) -> String {
    let place = serde_message
        .rsplit_once(" at line ")
        .and_then(|(_, at)| at.split_once(" column "))
        .and_then(|(line, column)| Some((line.parse::<u64>().ok()?, column.parse::<u64>().ok()?)));
    let ends = serde_message.starts_with("EOF while parsing");
    match (place, ends) {
        (Some((line, column)), true) => format!(
            "The body is not JSON: it ends at line {line}, column {column}, before the JSON is \
             complete. Send one complete JSON object."
        ),
        (Some((line, column)), false) => format!(
            "The body is not JSON: the fault is at line {line}, column {column}. Send one \
             JSON object."
        ),
        (None, _) => "The body is not JSON. Send one JSON object.".to_string(),
    }
}

/// The path before a message of `serde_path_to_error` (`metadata[0].default:
/// invalid type: ...`), and the fault after it. `None` for the root (`.`)
/// and for a message with no path.
pub(crate) fn split_path(serde_message: &str) -> (Option<&str>, &str) {
    match serde_message.split_once(": ") {
        Some((path, fault))
            if !path.is_empty()
                && path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.[]".contains(c)) =>
        {
            if path == "." {
                (None, fault)
            } else {
                (Some(path), fault)
            }
        }
        _ => (None, serde_message),
    }
}

/// A fault of the value of one field, in Kairos words (KAIROS-T-0697):
/// what the value must be, what the request gives, and the values of a
/// list of values. The words come from the forms of the messages of
/// `serde`, `serde_json`, the parsers of numbers and of booleans, `uuid`
/// and `chrono`; no part of the message of the library is passed on, only
/// the names of the values of an enum.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Fault {
    /// What the value must be: `a whole number`, `a list`.
    pub expected: Option<String>,
    /// What the request gives: `a string`.
    pub given: Option<&'static str>,
    /// The values that the field accepts.
    pub values: Vec<String>,
    /// The value of the request that is not one of `values`.
    pub value: Option<String>,
}

impl Fault {
    pub(crate) fn of(fault: &str) -> Self {
        let fault = fault
            .rsplit_once(" at line ")
            .map_or(fault, |(text, _)| text)
            .trim();
        let mut out = Fault::default();
        if let Some(rest) = fault.strip_prefix("unknown variant `") {
            if let Some((value, values)) = rest.split_once('`') {
                out.value = Some(value.to_string());
                out.values = values
                    .split('`')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect();
                return out;
            }
        }
        for (prefix, takes_given) in [("invalid type: ", true), ("invalid value: ", true)] {
            if let Some(rest) = fault.strip_prefix(prefix) {
                if let Some((given, expected)) = rest.rsplit_once(", expected ") {
                    if takes_given {
                        out.given = given_words(given);
                    }
                    out.expected = type_words(expected);
                }
                return out;
            }
        }
        if fault.starts_with("invalid length ") {
            if let Some((_, expected)) = fault.rsplit_once(", expected ") {
                out.expected = type_words(expected);
            }
            return out;
        }
        out.expected = parser_words(fault).map(str::to_string);
        out
    }

    /// The sentence of the refusal. `what` is `the field x` or `the query
    /// parameter "x"`; `of` is `body` or `request`.
    pub(crate) fn sentence(&self, what: &str, of: &str) -> String {
        if !self.values.is_empty() {
            let value = self
                .value
                .as_deref()
                .map(|v| format!(" {v:?}"))
                .unwrap_or_default();
            return format!(
                "The value{value} of {what} is not a value that this route accepts. Send one \
                 of: {}.",
                self.values.join(", ")
            );
        }
        match (&self.expected, self.given) {
            (Some(expected), Some(given)) => {
                format!("The value of {what} must be {expected}. The {of} gives {given}.")
            }
            (Some(expected), None) => format!("The value of {what} must be {expected}."),
            (None, _) => format!(
                "The value of {what} is not of the form that this route accepts. The REST \
                 reference gives the form of each field."
            ),
        }
    }

    /// The details of the refusal: the field, and what it must be. The
    /// root of the body has no field: `None`.
    pub(crate) fn details(&self, field: Option<&str>) -> serde_json::Value {
        let mut details = match field {
            Some(field) => json!({ "field": field }),
            None => json!({}),
        };
        if let Some(expected) = &self.expected {
            details["expected"] = json!(expected);
        }
        if !self.values.is_empty() {
            details["allowed"] = json!(self.values);
        }
        details
    }
}

/// What a value of the request is, from the `invalid type: <given>` of
/// `serde`.
fn given_words(given: &str) -> Option<&'static str> {
    let first = given.split([' ', '`']).next().unwrap_or_default();
    Some(match first {
        "string" | "char" => "a string",
        "integer" | "floating" => "a number",
        "boolean" => "true or false",
        "null" | "unit" => "null",
        "sequence" => "a list",
        "map" => "an object",
        _ => return None,
    })
}

/// What a value must be, from the `expected <...>` of `serde`.
fn type_words(expected: &str) -> Option<String> {
    let expected = expected.trim();
    let words = match expected {
        "a string" | "string" | "a borrowed string" | "a char" => "a string",
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "an integer" => "a whole number",
        "f32" | "f64" | "a float" => "a number",
        "a boolean" | "bool" => "true or false",
        "a sequence" => "a list",
        "a map" => "an object",
        "unit" | "null" => "null",
        "a UUID string" => "a UUID",
        "a formatted date and time string or a unix timestamp"
        | "an RFC 3339 formatted date and time string" => "an RFC 3339 timestamp",
        "a formatted date string" => "a date in the form YYYY-MM-DD",
        other if other.starts_with("struct ") || other.starts_with("a map") => "an object",
        other if other.starts_with("a sequence") || other.starts_with("an array") => "a list",
        other if other.starts_with("an integer") || other.starts_with("a nonzero") => {
            "a whole number"
        }
        _ => return None,
    };
    Some(words.to_string())
}

/// What a value must be, from the message of a parser that `serde` passes
/// on: the number and boolean parsers of `std`, `uuid` and `chrono`.
fn parser_words(fault: &str) -> Option<&'static str> {
    const WHOLE: [&str; 4] = [
        "invalid digit found in string",
        "cannot parse integer from empty string",
        "number too large to fit in target type",
        "number too small to fit in target type",
    ];
    if WHOLE.contains(&fault) {
        return Some("a whole number");
    }
    if fault == "invalid float literal" {
        return Some("a number");
    }
    if fault.starts_with("provided string was not `true` or `false`") {
        return Some("true or false");
    }
    if fault.contains("urn:uuid:")
        || fault.contains("for simple format")
        || fault.contains("invalid group")
        || fault.starts_with("UUID parsing failed")
    {
        return Some("a UUID");
    }
    const TIME: [&str; 7] = [
        "input contains invalid characters",
        "premature end of input",
        "trailing input",
        "input is out of range",
        "input is not enough for unique date and time",
        "no possible date and time matching input",
        "bad or unsupported format string",
    ];
    if TIME.contains(&fault) {
        return Some(
            "an RFC 3339 timestamp, or a date in the form YYYY-MM-DD where the route takes a date",
        );
    }
    None
}

/// The field that a message of `serde` says is absent: ``missing field
/// `x` ``, after the path of the field for a nested field. `None` when the
/// message is about a different fault.
pub(crate) fn missing_field(serde_message: &str) -> Option<String> {
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
    use serde_json::Value;

    /// COLLIERY-T-3101: a body that has `code_prefix` on a route that does
    /// not take it gets the refusal that a prefix does not change.
    #[test]
    fn a_code_prefix_gets_the_refusal_that_a_prefix_does_not_change() {
        let message = serde_json::from_value::<Three>(json!({"code_prefix": "SKD"}))
            .expect_err("an unknown field")
            .to_string();
        let error = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        assert_eq!(error.code, "CODE_PREFIX_IS_FIXED");
        assert!(
            error.message.contains("does not change"),
            "{}",
            error.message
        );
        assert_eq!(error.details["field"], "code_prefix");
        // A different unknown field keeps the general refusal.
        let message = serde_json::from_value::<Three>(json!({"board_level": "delivery"}))
            .expect_err("an unknown field")
            .to_string();
        assert_eq!(
            shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message).code,
            "VALIDATION"
        );
    }

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
    fn a_missing_field_is_named_in_kairos_words() {
        let message = message_of::<Two>(r#"{"slug": "s"}"#);
        assert_eq!(unknown_field(&message), None);
        let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The body does not have the field \"name\". This route must have that field."
        );
        assert_eq!(err.details, json!({ "field": "name" }));
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Typed {
        #[serde(default)]
        count: Option<i64>,
        #[serde(default)]
        on: Option<bool>,
        #[serde(default)]
        id: Option<uuid::Uuid>,
        #[serde(default)]
        tags: Option<Vec<String>>,
        #[serde(default)]
        mode: Option<Mode>,
        #[serde(default)]
        at: Option<chrono::DateTime<chrono::Utc>>,
        #[serde(default)]
        day: Option<chrono::NaiveDate>,
        #[serde(default)]
        inner: Option<Two>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[allow(dead_code)]
    enum Mode {
        Fast,
        Slow,
    }

    /// The message that axum gives: the path, then the message of serde.
    fn axum_message(body: &str) -> String {
        let mut de = serde_json::Deserializer::from_str(body);
        serde_path_to_error::deserialize::<_, Typed>(&mut de)
            .expect_err("the body is refused")
            .to_string()
    }

    /// KAIROS-T-0697: a fault of type or of value is in Kairos words, with
    /// the field and what it must be, and no text of serde.
    #[test]
    fn a_fault_of_a_value_is_in_kairos_words() {
        let cases = [
            (
                r#"{"count": "seven"}"#,
                "The value of the field count must be a whole number. The body gives a string.",
                json!({"field": "count", "expected": "a whole number"}),
            ),
            (
                r#"{"on": 1}"#,
                "The value of the field on must be true or false. The body gives a number.",
                json!({"field": "on", "expected": "true or false"}),
            ),
            (
                r#"{"id": "not-a-uuid"}"#,
                "The value of the field id must be a UUID.",
                json!({"field": "id", "expected": "a UUID"}),
            ),
            (
                r#"{"tags": "a"}"#,
                "The value of the field tags must be a list. The body gives a string.",
                json!({"field": "tags", "expected": "a list"}),
            ),
            (
                r#"{"mode": "quick"}"#,
                "The value \"quick\" of the field mode is not a value that this route \
                 accepts. Send one of: fast, slow.",
                json!({"field": "mode", "allowed": ["fast", "slow"]}),
            ),
            (
                r#"{"inner": {"name": 7}}"#,
                "The value of the field inner.name must be a string. The body gives a number.",
                json!({"field": "inner.name", "expected": "a string"}),
            ),
            (
                r#"{"inner": []}"#,
                // A struct reads a list too: serde says the length is wrong.
                "The value of the field inner must be an object.",
                json!({"field": "inner", "expected": "an object"}),
            ),
        ];
        for (body, text, details) in cases {
            let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &axum_message(body));
            assert_eq!(err.code, "VALIDATION");
            assert_eq!(err.message, text, "{body}");
            assert_eq!(err.details, details, "{body}");
        }
        for body in [r#"{"at": "yesterday"}"#, r#"{"day": "2026-13-45"}"#] {
            let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &axum_message(body));
            assert!(err.message.contains("RFC 3339"), "{body}: {}", err.message);
        }
        // The root: the body is not an object.
        let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &axum_message("7"));
        assert!(
            err.message.starts_with("The body must be one JSON object"),
            "{}",
            err.message
        );
        // No text of serde in any of them.
        for body in [
            r#"{"count": "seven"}"#,
            r#"{"id": "x"}"#,
            r#"{"at": "x"}"#,
            "7",
        ] {
            let err = shape_error(StatusCode::UNPROCESSABLE_ENTITY, &axum_message(body));
            for word in ["invalid", "expected", "line 1", "column", "struct", "i64"] {
                assert!(!err.message.contains(word), "{body}: {}", err.message);
            }
        }
    }

    /// KAIROS-T-0697: a body that is not JSON names the place of the
    /// fault, not the text of serde_json.
    #[test]
    fn a_body_that_is_not_json_names_the_place() {
        let message = serde_json::from_str::<Value>(r#"{"name": "Web","#)
            .expect_err("not JSON")
            .to_string();
        assert_eq!(
            not_json(&message),
            "The body is not JSON: it ends at line 1, column 15, before the JSON is complete. \
             Send one complete JSON object."
        );
        let message = serde_json::from_str::<Value>("{name}")
            .expect_err("not JSON")
            .to_string();
        assert_eq!(
            not_json(&message),
            "The body is not JSON: the fault is at line 1, column 2. Send one JSON object."
        );
    }
}
