//! The arguments of a tool call (COLLIERY-T-0256).
//!
//! [`Parameters`] is the extractor that each tool uses in place of
//! `rmcp::handler::server::wrapper::Parameters`. It reads the arguments the
//! same way, and the schema that the agent sees is the schema of the type.
//! The difference is the refusal. rmcp gives its own text as a protocol
//! error ("failed to deserialize parameters: unknown field ..."). This
//! extractor gives the refusal that each other tool error of this server
//! has: a tool error with the text `CODE: message` and a `details:` line
//! ([`super::service::tool_error`]).
//!
//! | The call | `details` |
//! |---|---|
//! | has an argument that the tool does not know | `argument`, `allowed` |
//! | does not have an argument that the tool must have | `argument`, `allowed` |
//! | has a different fault of shape | — |
//!
//! The code is `VALIDATION`. The rule is in the types: each has
//! `#[serde(deny_unknown_fields)]`.
//!
//! The name is `Parameters` because the macro `#[tool]` finds the type of
//! the arguments by that name.
//!
//! # How the refusal gets to the agent
//!
//! An extractor of rmcp can only fail with an `ErrorData`. [`refusal`]
//! puts the text of the tool error in the `data` of one, and
//! `KairosMcp::call_tool` makes the tool error from it ([`tool_error_of`]).

use std::borrow::Cow;

use axum::http::StatusCode;
use rmcp::ErrorData;
use rmcp::handler::server::common::{FromContextPart, schema_for_input};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use rmcp::schemars::{self, JsonSchema};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::body::unknown_field;
use crate::error::ApiError;

/// The arguments of a tool call. See the module docs.
#[derive(Debug, Clone)]
pub struct Parameters<P>(pub P);

impl<P: JsonSchema> JsonSchema for Parameters<P> {
    fn schema_name() -> Cow<'static, str> {
        P::schema_name()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        P::json_schema(generator)
    }
}

impl<S, P> FromContextPart<ToolCallContext<'_, S>> for Parameters<P>
where
    P: DeserializeOwned + JsonSchema + 'static,
{
    fn from_context_part(context: &mut ToolCallContext<S>) -> Result<Self, ErrorData> {
        let arguments = context.arguments.take().unwrap_or_default();
        parse::<P>(arguments).map(Parameters).map_err(refusal)
    }
}

/// The arguments as the type of the tool, or the refusal.
///
/// KAIROS-T-0674: the parser keeps the path of the fault
/// (`serde_path_to_error`), so the refusal names the argument or the field
/// in an object of the call (`filter.entity_type`).
fn parse<P>(arguments: JsonObject) -> Result<P, ApiError>
where
    P: DeserializeOwned + JsonSchema + 'static,
{
    let call = Value::Object(arguments);
    serde_path_to_error::deserialize(&call).map_err(|e| {
        let schema = schema_for_input::<P>().unwrap_or_default();
        let path: Vec<Step> = e.path().iter().filter_map(Step::of).collect();
        argument_error(&e.inner().to_string(), &path, &call, &schema)
    })
}

/// One step of the path to a fault: a field of an object, or an entry of
/// a list.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Field(String),
    Entry(usize),
}

impl Step {
    fn of(segment: &serde_path_to_error::Segment) -> Option<Self> {
        match segment {
            serde_path_to_error::Segment::Map { key } => Some(Step::Field(key.clone())),
            serde_path_to_error::Segment::Seq { index } => Some(Step::Entry(*index)),
            _ => None,
        }
    }
}

/// The path as text: `filter.entity_type`, `metadata[0].default`.
fn path_text(path: &[Step]) -> String {
    let mut text = String::new();
    for step in path {
        match step {
            Step::Field(name) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(name);
            }
            Step::Entry(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

/// The refusal of arguments that the tool cannot read. `serde_message` is
/// the message of `serde` without the path, `path` is the path to the
/// fault, `call` is the arguments of the call, and `schema` is the schema
/// of the tool.
fn argument_error(
    serde_message: &str,
    path: &[Step],
    call: &Value,
    schema: &JsonObject,
) -> ApiError {
    if let Some((argument, allowed)) = unknown_field(serde_message) {
        // The path ends with the unknown field. Before it is the object
        // that has the field.
        let object = path_text(&path[..path.len().saturating_sub(1)]);
        if object.is_empty() {
            let known = if allowed.is_empty() {
                "This tool has no arguments.".to_string()
            } else {
                format!("The arguments of this tool are: {}.", allowed.join(", "))
            };
            return validation(format!(
                "The call has the argument {argument:?}. This tool does not accept that \
                 argument. {known}"
            ))
            .with_details(json!({ "field": argument, "allowed": allowed }));
        }
        let known = if allowed.is_empty() {
            format!("The object {object} has no fields.")
        } else {
            format!("The fields of {object} are: {}.", allowed.join(", "))
        };
        return validation(format!(
            "The call has the field {argument:?} in {object}. The object {object} does not \
             accept that field. {known}"
        ))
        .with_details(json!({ "field": format!("{object}.{argument}"), "allowed": allowed }));
    }
    if let Some(argument) = missing_field(serde_message) {
        // The path is the object that does not have the field.
        let object = path_text(path);
        if object.is_empty() {
            let allowed = properties(schema);
            return validation(format!(
                "The call does not have the argument {argument:?}. This tool must have \
                 that argument. The arguments of this tool are: {}.",
                allowed.join(", ")
            ))
            .with_details(json!({ "field": argument, "allowed": allowed }));
        }
        return validation(format!(
            "The object {object} does not have the field {argument:?}. The tool must have \
             that field."
        ))
        .with_details(json!({ "field": format!("{object}.{argument}") }));
    }
    let field = path_text(path);
    if field.is_empty() {
        return validation(format!(
            "The arguments are not correct for this tool: {serde_message}."
        ));
    }
    // Only a fault of type gets the words of the schema. A value of the
    // correct type that is not correct (an unknown variant, a bad UUID)
    // keeps the message of `serde`, which says what is wrong.
    let given = value_at(call, path);
    let expected = serde_message
        .starts_with("invalid type:")
        .then(|| schema_at(schema, path).and_then(|s| describe(schema, s)))
        .flatten();
    match expected {
        Some(expected) => {
            let mut message = format!("The value of {field} must be {expected}.");
            if let Some(given) = given {
                message.push_str(&format!(" The call gives {}.", kind_of(given)));
                // A single value where the tool must have a list: show the
                // list that has the value.
                if expected.starts_with("a list") && !given.is_array() && !given.is_object() {
                    message.push_str(&format!(
                        " For example: {}.",
                        Value::Array(vec![given.clone()])
                    ));
                }
            }
            validation(message).with_details(json!({ "field": field, "expected": expected }))
        }
        None => validation(format!(
            "The value of {field} is not correct: {serde_message}."
        ))
        .with_details(json!({ "field": field })),
    }
}

/// The value of the call at the path.
fn value_at<'a>(call: &'a Value, path: &[Step]) -> Option<&'a Value> {
    path.iter().try_fold(call, |value, step| match step {
        Step::Field(name) => value.get(name),
        Step::Entry(index) => value.get(index),
    })
}

/// The schema at the path, from the schema of the tool.
fn schema_at<'a>(root: &'a JsonObject, path: &[Step]) -> Option<&'a Value> {
    let (Step::Field(name), rest) = path.split_first()? else {
        return None;
    };
    let mut at = root.get("properties")?.get(name)?;
    for step in rest {
        let schema = resolve(root, at)?;
        at = match step {
            Step::Field(name) => schema.get("properties")?.get(name)?,
            Step::Entry(_) => schema.get("items")?,
        };
    }
    Some(at)
}

/// The schema without `$ref` and without the `null` of an optional value.
fn resolve<'a>(root: &'a JsonObject, schema: &'a Value) -> Option<&'a Value> {
    let mut schema = schema;
    // A chain of references is short; the bound stops a loop.
    for _ in 0..8 {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.strip_prefix("#/$defs/")?;
            schema = root.get("$defs")?.get(name)?;
            continue;
        }
        let choices = schema
            .get("anyOf")
            .or_else(|| schema.get("oneOf"))
            .and_then(Value::as_array);
        if let Some(choices) = choices {
            let not_null: Vec<&Value> = choices
                .iter()
                .filter(|c| c.get("type").and_then(Value::as_str) != Some("null"))
                .collect();
            if let [one] = not_null.as_slice() {
                schema = one;
                continue;
            }
        }
        return Some(schema);
    }
    None
}

/// The type of a schema in words: `a string`, `a list of strings`.
/// `None` when the words would not help.
fn describe(root: &JsonObject, schema: &Value) -> Option<String> {
    let schema = resolve(root, schema)?;
    if let Some(values) = values_of(schema) {
        return Some(format!("one of these values: {}", values.join(", ")));
    }
    let kind = match schema.get("type")? {
        Value::String(kind) => kind.as_str(),
        Value::Array(kinds) => {
            let not_null: Vec<&str> = kinds
                .iter()
                .filter_map(Value::as_str)
                .filter(|k| *k != "null")
                .collect();
            match not_null.as_slice() {
                [one] => *one,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(match kind {
        "string" => "a string".into(),
        "integer" => "an integer".into(),
        "number" => "a number".into(),
        "boolean" => "true or false".into(),
        "object" => "an object".into(),
        "array" => {
            let items = schema.get("items").and_then(|items| describe(root, items));
            match items.as_deref() {
                Some("a string") => "a list of strings".into(),
                Some("an integer") => "a list of integers".into(),
                Some("a number") => "a list of numbers".into(),
                Some("an object") => "a list of objects".into(),
                Some(other) if other.starts_with("one of these values: ") => {
                    format!("a list of strings, each {other}")
                }
                _ => "a list".into(),
            }
        }
        _ => return None,
    })
}

/// The values of a schema of a fixed set of strings (`enum`, or a `oneOf`
/// of `const`).
fn values_of(schema: &Value) -> Option<Vec<String>> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let values: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        return (!values.is_empty()).then_some(values);
    }
    let choices = schema.get("oneOf").and_then(Value::as_array)?;
    let values: Option<Vec<String>> = choices
        .iter()
        .map(|c| c.get("const").and_then(Value::as_str).map(str::to_string))
        .collect();
    values.filter(|v| !v.is_empty())
}

/// The kind of a value of the call in words.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// 422 `VALIDATION`, the status of the same refusal of a body.
fn validation(message: String) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION", message)
}

/// The field that a message of `serde` says is absent: ``missing field
/// `x` ``. `None` when the message is about a different fault.
fn missing_field(serde_message: &str) -> Option<String> {
    let rest = serde_message.strip_prefix("missing field `")?;
    let (field, _) = rest.split_once('`')?;
    Some(field.to_string())
}

/// The names of the arguments that a schema has.
fn properties(schema: &JsonObject) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|all| all.keys().cloned().collect())
        .unwrap_or_default()
}

/// The key of the text of the tool error, in the `data` of the
/// `ErrorData`.
const TOOL_ERROR: &str = "kairos_tool_error";

/// The refusal as the error of an extractor. See the module docs.
fn refusal(e: ApiError) -> ErrorData {
    let text = super::service::tool_error_text(&e);
    ErrorData::invalid_params(e.message, Some(json!({ TOOL_ERROR: text })))
}

/// The tool error that [`refusal`] put in an error, or the error.
pub(super) fn tool_error_of(e: ErrorData) -> Result<CallToolResult, ErrorData> {
    let text = e
        .data
        .as_ref()
        .and_then(|data| data.get(TOOL_ERROR))
        .and_then(Value::as_str);
    match text {
        Some(text) => Ok(CallToolResult::error(vec![ContentBlock::text(text)])),
        None => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Read {
        short_code: String,
        limit: Option<i64>,
        within: Option<Within>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Within {
        from: String,
        kinds: Option<Vec<String>>,
        mode: Option<Mode>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(rename_all = "snake_case")]
    #[allow(dead_code)]
    enum Mode {
        Fast,
        Slow,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    struct Nothing {}

    fn refusal_of<P>(arguments: Value) -> ApiError
    where
        P: DeserializeOwned + JsonSchema + std::fmt::Debug + 'static,
    {
        let Value::Object(arguments) = arguments else {
            panic!("the arguments are an object");
        };
        parse::<P>(arguments).expect_err("the call is refused")
    }

    /// COLLIERY-T-0256: the refusal names the argument, and it gives the
    /// arguments that the tool has.
    #[test]
    fn an_unknown_argument_is_named() {
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "verbose": true}));
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The call has the argument \"verbose\". This tool does not accept that argument. \
             The arguments of this tool are: short_code, limit, within."
        );
        assert_eq!(
            err.details,
            json!({"field": "verbose", "allowed": ["short_code", "limit", "within"]})
        );
    }

    /// A tool with no arguments refuses each argument.
    #[test]
    fn a_tool_with_no_arguments() {
        assert!(parse::<Nothing>(JsonObject::new()).is_ok());
        let err = refusal_of::<Nothing>(json!({"team": "platform"}));
        assert_eq!(
            err.message,
            "The call has the argument \"team\". This tool does not accept that argument. \
             This tool has no arguments."
        );
        assert_eq!(err.details, json!({"field": "team", "allowed": []}));
    }

    /// An argument that the tool must have, and the call has not.
    #[test]
    fn a_missing_argument_is_named() {
        let err = refusal_of::<Read>(json!({"limit": 5}));
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The call does not have the argument \"short_code\". This tool must have that \
             argument. The arguments of this tool are: limit, short_code, within."
        );
        assert_eq!(err.details["field"], "short_code");
    }

    /// KAIROS-T-0674: the field is absent in an object of the call. The
    /// refusal names the object, and no list of the tool.
    #[test]
    fn a_missing_field_of_an_object() {
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "within": {}}));
        assert_eq!(
            err.message,
            "The object within does not have the field \"from\". The tool must have \
             that field."
        );
        assert_eq!(err.details, json!({"field": "within.from"}));
    }

    /// KAIROS-T-0674: an unknown field in an object of the call names the
    /// object and gives the fields of that object, not the arguments of
    /// the tool.
    #[test]
    fn an_unknown_field_of_an_object_names_the_object() {
        let err = refusal_of::<Read>(
            json!({"short_code": "ACME-T-0001", "within": {"from": "a", "board_slug": "x"}}),
        );
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The call has the field \"board_slug\" in within. The object within does not \
             accept that field. The fields of within are: from, kinds, mode."
        );
        assert_eq!(
            err.details,
            json!({"field": "within.board_slug", "allowed": ["from", "kinds", "mode"]})
        );
    }

    /// KAIROS-T-0674: a wrong type names the argument and the type that the
    /// tool must have.
    #[test]
    fn a_wrong_type_names_the_argument_and_the_type() {
        let err = refusal_of::<Read>(json!({"short_code": 12}));
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The value of short_code must be a string. The call gives a number."
        );
        assert_eq!(
            err.details,
            json!({"field": "short_code", "expected": "a string"})
        );
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "limit": "5"}));
        assert_eq!(
            err.message,
            "The value of limit must be an integer. The call gives a string."
        );
    }

    /// KAIROS-T-0674: a single value where the tool must have a list. The
    /// refusal gives the path in the object and the list to send.
    #[test]
    fn a_string_for_a_list_shows_the_list() {
        let err = refusal_of::<Read>(
            json!({"short_code": "ACME-T-0001", "within": {"from": "a", "kinds": "document"}}),
        );
        assert_eq!(
            err.message,
            "The value of within.kinds must be a list of strings. The call gives a \
             string. For example: [\"document\"]."
        );
        assert_eq!(
            err.details,
            json!({"field": "within.kinds", "expected": "a list of strings"})
        );
        // An object where the tool must have an object of a different
        // shape, and a list where the tool must have a string.
        let err = refusal_of::<Read>(json!({"short_code": ["ACME-T-0001"]}));
        assert_eq!(
            err.message,
            "The value of short_code must be a string. The call gives a list."
        );
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "within": "a"}));
        assert_eq!(
            err.message,
            "The value of within must be an object. The call gives a string."
        );
    }

    /// KAIROS-T-0674: a value of the correct type that is not correct keeps
    /// the message of `serde`, after the path.
    #[test]
    fn a_wrong_value_keeps_the_message_of_serde_with_the_path() {
        let err = refusal_of::<Read>(
            json!({"short_code": "ACME-T-0001", "within": {"from": "a", "mode": "quick"}}),
        );
        assert_eq!(
            err.message,
            "The value of within.mode is not correct: unknown variant `quick`, expected \
             `fast` or `slow`."
        );
        assert_eq!(err.details, json!({"field": "within.mode"}));
    }

    #[test]
    fn the_path_as_text() {
        let path = [
            Step::Field("metadata".into()),
            Step::Entry(0),
            Step::Field("default".into()),
        ];
        assert_eq!(path_text(&path), "metadata[0].default");
        assert_eq!(path_text(&[]), "");
    }

    /// The text goes through the error of the extractor, and it is a tool
    /// error in the form `CODE: message`. A different error stays an error.
    #[test]
    fn the_refusal_is_a_tool_error() {
        let err = refusal_of::<Nothing>(json!({"team": "platform"}));
        let result = tool_error_of(refusal(err)).expect("a tool error");
        assert_eq!(result.is_error, Some(true));
        let text = serde_json::to_value(&result.content).expect("the content serializes");
        assert_eq!(
            text[0]["text"],
            "VALIDATION: The call has the argument \"team\". This tool does not accept that \
             argument. This tool has no arguments.\n\
             details: {\"allowed\":[],\"field\":\"team\"}"
        );
        assert!(tool_error_of(ErrorData::internal_error("no", None)).is_err());
    }
}
