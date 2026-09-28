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
fn parse<P>(arguments: JsonObject) -> Result<P, ApiError>
where
    P: DeserializeOwned + JsonSchema + 'static,
{
    let given: Vec<String> = arguments.keys().cloned().collect();
    serde_json::from_value(Value::Object(arguments)).map_err(|e| {
        let schema = schema_for_input::<P>().unwrap_or_default();
        argument_error(&e.to_string(), &given, &schema)
    })
}

/// The refusal of arguments that the tool cannot read, from the message
/// of `serde`. `given` is the names of the arguments of the call, and
/// `schema` is the schema of the tool.
fn argument_error(serde_message: &str, given: &[String], schema: &JsonObject) -> ApiError {
    if let Some((argument, allowed)) = unknown_field(serde_message) {
        let known = if allowed.is_empty() {
            "This tool has no arguments.".to_string()
        } else {
            format!("The arguments of this tool are: {}.", allowed.join(", "))
        };
        return validation(format!(
            "The call has the argument {argument:?}. This tool does not accept that \
             argument. {known}"
        ))
        .with_details(json!({ "argument": argument, "allowed": allowed }));
    }
    if let Some(argument) = missing_field(serde_message) {
        // The list is the arguments of the tool. When the argument is in an
        // object of the call, the list is not about that object: give none.
        let of_the_tool =
            names(&schema["required"]).contains(&argument) && !given.contains(&argument);
        if of_the_tool {
            let allowed = properties(schema);
            return validation(format!(
                "The call does not have the argument {argument:?}. This tool must have \
                 that argument. The arguments of this tool are: {}.",
                allowed.join(", ")
            ))
            .with_details(json!({ "argument": argument, "allowed": allowed }));
        }
        return validation(format!(
            "An object of the call does not have the field {argument:?}. The tool must \
             have that field."
        ))
        .with_details(json!({ "argument": argument }));
    }
    validation(format!(
        "The arguments are not correct for this tool: {serde_message}."
    ))
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

/// The strings of a JSON array.
fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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
            json!({"argument": "verbose", "allowed": ["short_code", "limit", "within"]})
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
        assert_eq!(err.details, json!({"argument": "team", "allowed": []}));
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
        assert_eq!(err.details["argument"], "short_code");
    }

    /// The field is absent in an object of the call: no list of the tool.
    #[test]
    fn a_missing_field_of_an_object() {
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "within": {}}));
        assert_eq!(
            err.message,
            "An object of the call does not have the field \"from\". The tool must have \
             that field."
        );
        assert_eq!(err.details, json!({"argument": "from"}));
    }

    /// A different fault keeps the message of `serde`.
    #[test]
    fn a_wrong_type_keeps_the_message_of_serde() {
        let err = refusal_of::<Read>(json!({"short_code": 12}));
        assert_eq!(err.code, "VALIDATION");
        assert!(
            err.message
                .starts_with("The arguments are not correct for this tool: invalid type"),
            "{err:?}"
        );
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
             details: {\"allowed\":[],\"argument\":\"team\"}"
        );
        assert!(tool_error_of(ErrorData::internal_error("no", None)).is_err());
    }
}
