//! The arguments of a code tool call (COLLIERY-T-1852), with the rule of
//! the Kairos MCP tools (COLLIERY-T-0256): an argument that the tool does
//! not have is refused, and the refusal names it.
//!
//! [`Parameters`] is the extractor that each tool uses in place of the one
//! of rmcp, as `kairos-server/src/mcp/arguments.rs` does. rmcp gives its own
//! text as a protocol error; this extractor gives a tool error with the
//! text `VALIDATION: message` and a `details:` line. The types of the
//! arguments have `#[serde(deny_unknown_fields)]`.

use std::borrow::Cow;

use rmcp::ErrorData;
use rmcp::handler::server::common::{FromContextPart, schema_for_input};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use rmcp::schemars::{self, JsonSchema};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::error::ToolError;

/// The arguments of a tool call. The name is `Parameters` because the
/// macro `#[tool]` finds the type of the arguments by that name.
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
fn parse<P>(arguments: JsonObject) -> Result<P, ToolError>
where
    P: DeserializeOwned + JsonSchema + 'static,
{
    serde_json::from_value(Value::Object(arguments)).map_err(|e| {
        let schema = schema_for_input::<P>().unwrap_or_default();
        argument_error(&e.to_string(), &schema)
    })
}

fn argument_error(serde_message: &str, schema: &JsonObject) -> ToolError {
    let allowed: Vec<String> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|all| all.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(argument) = quoted_after(serde_message, "unknown field `") {
        let known = if allowed.is_empty() {
            "This tool has no arguments.".to_string()
        } else {
            format!("The arguments of this tool are: {}.", allowed.join(", "))
        };
        return ToolError::UnknownArgument {
            argument,
            allowed,
            known,
        };
    }
    if let Some(argument) = quoted_after(serde_message, "missing field `") {
        return ToolError::MissingArgument {
            argument,
            allowed: allowed.join(", "),
        };
    }
    ToolError::BadArguments(serde_message.to_string())
}

/// The name between backticks after `start` at the start of a message of
/// `serde`.
fn quoted_after(serde_message: &str, start: &str) -> Option<String> {
    let rest = serde_message.strip_prefix(start)?;
    let (name, _) = rest.split_once('`')?;
    Some(name.to_string())
}

/// The key of the text of the tool error, in the `data` of the `ErrorData`.
const TOOL_ERROR: &str = "kairos_tool_error";

/// The refusal as the error of an extractor: an extractor of rmcp can only
/// fail with an `ErrorData`. [`tool_error_of`] makes the tool error again.
fn refusal(e: ToolError) -> ErrorData {
    ErrorData::invalid_params(e.to_string(), Some(json!({ TOOL_ERROR: e.text() })))
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
        symbol: String,
        file: Option<String>,
    }

    fn refusal_of(arguments: Value) -> ToolError {
        let Value::Object(arguments) = arguments else {
            panic!("the arguments are an object");
        };
        parse::<Read>(arguments).expect_err("the call is refused")
    }

    #[test]
    fn an_unknown_argument_is_named() {
        let err = refusal_of(json!({"symbol": "pop", "verbose": true}));
        assert_eq!(
            err.text(),
            "VALIDATION: The call has the argument \"verbose\". This tool does not accept that \
             argument. The arguments of this tool are: file, symbol.\n\
             details: {\"allowed\":[\"file\",\"symbol\"],\"argument\":\"verbose\"}"
        );
    }

    #[test]
    fn a_missing_argument_is_named() {
        let err = refusal_of(json!({"file": "src/lib.rs"}));
        assert_eq!(err.code(), "VALIDATION");
        assert_eq!(
            err.to_string(),
            "The call does not have the argument \"symbol\". This tool must have that \
             argument. The arguments of this tool are: file, symbol."
        );
    }

    #[test]
    fn the_refusal_is_a_tool_error() {
        let err = refusal_of(json!({"symbol": 3}));
        assert_eq!(err.code(), "VALIDATION");
        let result = tool_error_of(refusal(err)).expect("a tool error");
        assert_eq!(result.is_error, Some(true));
        assert!(tool_error_of(ErrorData::internal_error("no", None)).is_err());
    }
}
