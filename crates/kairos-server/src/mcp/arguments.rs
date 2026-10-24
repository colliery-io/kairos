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
//! # Aliases (KAIROS-T-0673)
//!
//! Agents guess the names of arguments (`query` for `q`, `id` for
//! `short_code`). [`ALIASES`] is one fixed list of other names for each
//! argument. The extractor changes an alias to the name of its argument
//! before the parse, and the schema of the tool shows the aliases of each
//! argument. Each other unknown argument is still refused. A guess that is
//! not an alias can get a hint in the refusal ([`HINTS`]).
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
        let mut schema = P::json_schema(generator);
        if let Some(object) = schema.as_object_mut() {
            show_aliases(object);
        }
        schema
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
    let schema = schema_for_input::<P>().unwrap_or_default();
    let arguments = with_names(arguments, &properties(&schema))?;
    let given: Vec<String> = arguments.keys().cloned().collect();
    serde_json::from_value(Value::Object(arguments))
        .map_err(|e| argument_error(&e.to_string(), &given, &schema))
}

/// The other names of each argument (KAIROS-T-0673): the names that agents
/// guess. The list is fixed: each name that is not here is refused.
///
/// An alias applies to a tool only when the tool has the argument, has no
/// argument with the name of the alias, and no other argument of the tool
/// has the same alias ([`aliases_of`]).
pub(super) const ALIASES: &[(&str, &[&str])] = &[
    ("short_code", &["code", "id", "item"]),
    // The body of a note is `text`, the body of an item is `content`: each
    // is an alias of the other.
    ("text", &["note", "body", "content"]),
    ("content", &["body", "text"]),
    ("q", &["query"]),
    ("to_column", &["to", "column"]),
    ("source", &["from"]),
    ("target", &["to"]),
    ("relationship", &["relation", "edge_type"]),
];

/// Guesses that are not aliases, and the argument that the refusal names
/// for each. `state` is not an alias of the body: `set_state` writes the
/// state of a task, and a note written by error is worse than a refusal.
const HINTS: &[(&str, &str)] = &[("state", "text"), ("state", "content")];

/// The aliases of a tool that has the arguments `arguments`: each alias
/// and the name of its argument.
fn aliases_of(arguments: &[String]) -> Vec<(&'static str, &'static str)> {
    let has = |name: &str| arguments.iter().any(|a| a == name);
    let claims: Vec<(&'static str, &'static str)> = ALIASES
        .iter()
        .filter(|(argument, _)| has(argument))
        .flat_map(|(argument, aliases)| aliases.iter().map(move |alias| (*alias, *argument)))
        .filter(|(alias, _)| !has(alias))
        .collect();
    // An alias that two arguments of the tool claim is the alias of neither.
    claims
        .iter()
        .filter(|(alias, _)| claims.iter().filter(|(a, _)| a == alias).count() == 1)
        .copied()
        .collect()
}

/// The arguments of the call with each alias changed to the name of its
/// argument. A call that has an argument and its alias is refused.
fn with_names(mut arguments: JsonObject, names: &[String]) -> Result<JsonObject, ApiError> {
    for (alias, argument) in aliases_of(names) {
        let Some(value) = arguments.remove(alias) else {
            continue;
        };
        if arguments.contains_key(argument) {
            return Err(validation(format!(
                "The call has the argument {argument:?} and the argument {alias:?}. They \
                 are the same argument. Send only one of them."
            ))
            .with_details(json!({ "field": alias, "argument": argument })));
        }
        arguments.insert(argument.to_string(), value);
    }
    Ok(arguments)
}

/// The aliases of each argument in the schema of the tool: in the
/// description, and as the list `x-aliases`.
fn show_aliases(schema: &mut JsonObject) {
    let names = properties(schema);
    let aliases = aliases_of(&names);
    let Some(Value::Object(all)) = schema.get_mut("properties") else {
        return;
    };
    for (name, property) in all.iter_mut() {
        let of_this: Vec<&str> = aliases
            .iter()
            .filter(|(_, argument)| argument == name)
            .map(|(alias, _)| *alias)
            .collect();
        let Some(property) = property.as_object_mut() else {
            continue;
        };
        if of_this.is_empty() {
            continue;
        }
        let list = of_this
            .iter()
            .map(|alias| format!("`{alias}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let note = format!("Other names that the tool accepts: {list}.");
        let description = match property.get("description").and_then(Value::as_str) {
            Some(text) if !text.is_empty() => format!("{text}\n{note}"),
            _ => note,
        };
        property.insert("description".into(), Value::String(description));
        property.insert("x-aliases".into(), json!(of_this));
    }
}

/// The argument that a refusal of the argument `given` names, when the
/// tool has one ([`HINTS`]).
fn hint(given: &str, allowed: &[String]) -> Option<&'static str> {
    HINTS
        .iter()
        .find(|(guess, argument)| *guess == given && allowed.iter().any(|a| a == argument))
        .map(|(_, argument)| *argument)
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
        // KAIROS-T-0673: a known guess names the argument to use.
        if let Some(right) = hint(&argument, &allowed) {
            return validation(format!(
                "The call has the argument {argument:?}. This tool does not accept that \
                 argument. Use the argument {right:?}. {known}"
            ))
            .with_details(json!({ "field": argument, "allowed": allowed, "did_you_mean": right }));
        }
        return validation(format!(
            "The call has the argument {argument:?}. This tool does not accept that \
             argument. {known}"
        ))
        .with_details(json!({ "field": argument, "allowed": allowed }));
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
            .with_details(json!({ "field": argument, "allowed": allowed }));
        }
        return validation(format!(
            "An object of the call does not have the field {argument:?}. The tool must \
             have that field."
        ))
        .with_details(json!({ "field": argument }));
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

    /// The field is absent in an object of the call: no list of the tool.
    #[test]
    fn a_missing_field_of_an_object() {
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "within": {}}));
        assert_eq!(
            err.message,
            "An object of the call does not have the field \"from\". The tool must have \
             that field."
        );
        assert_eq!(err.details, json!({"field": "from"}));
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
             details: {\"allowed\":[],\"field\":\"team\"}"
        );
        assert!(tool_error_of(ErrorData::internal_error("no", None)).is_err());
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Note {
        /// The item's short code.
        short_code: String,
        text: String,
    }

    /// KAIROS-T-0673: an alias is the argument.
    #[test]
    fn an_alias_is_the_argument() {
        let Value::Object(call) = json!({"id": "ACME-T-0001", "note": "Done."}) else {
            unreachable!()
        };
        let note = parse::<Note>(call).expect("the aliases are accepted");
        assert_eq!(note.short_code, "ACME-T-0001");
        assert_eq!(note.text, "Done.");
        for alias in ["code", "item"] {
            let Value::Object(call) = json!({alias: "ACME-T-0002", "body": "x"}) else {
                unreachable!()
            };
            assert_eq!(parse::<Note>(call).expect(alias).short_code, "ACME-T-0002");
        }
        let Value::Object(call) = json!({"short_code": "ACME-T-0003", "content": "y"}) else {
            unreachable!()
        };
        assert_eq!(parse::<Note>(call).expect("content").text, "y");
    }

    /// KAIROS-T-0673: an argument and its alias in one call are refused.
    #[test]
    fn an_argument_and_its_alias_are_refused() {
        let err = refusal_of::<Note>(
            json!({"short_code": "ACME-T-0001", "id": "ACME-T-0002", "text": "x"}),
        );
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The call has the argument \"short_code\" and the argument \"id\". They are the \
             same argument. Send only one of them."
        );
        assert_eq!(
            err.details,
            json!({"field": "id", "argument": "short_code"})
        );
    }

    /// KAIROS-T-0673: a name that is not in the list is still refused.
    #[test]
    fn a_name_that_is_not_an_alias_is_refused() {
        let err = refusal_of::<Note>(json!({"short_code": "ACME-T-0001", "message": "x"}));
        assert_eq!(
            err.message,
            "The call has the argument \"message\". This tool does not accept that \
             argument. The arguments of this tool are: short_code, text."
        );
    }

    /// KAIROS-T-0673: `state` is not an alias of the body; the refusal
    /// names the argument to use.
    #[test]
    fn a_guess_gets_a_hint() {
        let err = refusal_of::<Note>(json!({"short_code": "ACME-T-0001", "state": "x"}));
        assert_eq!(
            err.message,
            "The call has the argument \"state\". This tool does not accept that \
             argument. Use the argument \"text\". The arguments of this tool are: \
             short_code, text."
        );
        assert_eq!(err.details["did_you_mean"], "text");
        // A tool with no body gets no hint.
        let err = refusal_of::<Read>(json!({"short_code": "ACME-T-0001", "state": "x"}));
        assert!(!err.message.contains("Use the argument"), "{}", err.message);
    }

    /// KAIROS-T-0673: an alias that is an argument of the tool, or that two
    /// arguments claim, is not an alias of that tool.
    #[test]
    fn the_aliases_of_a_tool() {
        let names = |all: &[&str]| all.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let aliases = aliases_of(&names(&["short_code", "to"]));
        assert_eq!(
            aliases,
            vec![
                ("code", "short_code"),
                ("id", "short_code"),
                ("item", "short_code")
            ]
        );
        let aliases = aliases_of(&names(&["short_code", "id"]));
        assert!(!aliases.iter().any(|(alias, _)| *alias == "id"));
        assert!(aliases.contains(&("code", "short_code")));
        // `to` is claimed by `to_column` and by `target`: it is neither.
        let aliases = aliases_of(&names(&["to_column", "target"]));
        assert!(!aliases.iter().any(|(alias, _)| *alias == "to"));
        assert!(aliases.contains(&("column", "to_column")));
    }

    /// KAIROS-T-0673: the schema shows the aliases of each argument.
    #[test]
    fn the_schema_shows_the_aliases() {
        let schema = schema_for_input::<Parameters<Note>>().expect("a schema");
        let text = &schema["properties"]["text"];
        assert_eq!(text["x-aliases"], json!(["note", "body", "content"]));
        assert_eq!(
            text["description"],
            "Other names that the tool accepts: `note`, `body`, `content`."
        );
        let short_code = &schema["properties"]["short_code"];
        assert_eq!(short_code["x-aliases"], json!(["code", "id", "item"]));
        assert_eq!(
            short_code["description"],
            "The item's short code.\nOther names that the tool accepts: `code`, `id`, `item`."
        );
    }

    /// KAIROS-T-0673: the aliases of the tools of the server. Each guess of
    /// the task is an alias of the tool where agents made it, and no alias
    /// is claimed by two arguments of one tool.
    #[test]
    fn the_aliases_of_the_tools_of_the_server() {
        let tools = super::super::KairosMcp::tool_router().list_all();
        let schema_of = |name: &str| {
            tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("the tool {name}"))
                .input_schema
                .clone()
        };
        let aliases = |tool: &str, argument: &str| {
            schema_of(tool)["properties"][argument]["x-aliases"].clone()
        };
        assert_eq!(
            aliases("get_item", "short_code"),
            json!(["code", "id", "item"])
        );
        assert_eq!(
            aliases("append_note", "text"),
            json!(["note", "body", "content"])
        );
        assert_eq!(aliases("create_item", "content"), json!(["body", "text"]));
        assert_eq!(aliases("update_item", "content"), json!(["body", "text"]));
        assert_eq!(
            aliases("transition_item", "to_column"),
            json!(["to", "column"])
        );
        assert_eq!(
            aliases("transition_item", "short_code"),
            json!(["code", "id", "item"])
        );
        assert_eq!(aliases("search", "q"), json!(["query"]));
        assert_eq!(aliases("link_items", "source"), json!(["from"]));
        assert_eq!(aliases("link_items", "target"), json!(["to"]));
        assert_eq!(
            aliases("link_items", "relationship"),
            json!(["relation", "edge_type"])
        );
        // `hand_off_item` has an argument `to`: it is not an alias there.
        assert_eq!(
            aliases("hand_off_item", "short_code"),
            json!(["code", "id", "item"])
        );
        for tool in &tools {
            let names = properties(&tool.input_schema);
            for (argument, list) in ALIASES {
                if !names.iter().any(|n| n == argument) {
                    continue;
                }
                for alias in *list {
                    if names.iter().any(|n| n == alias) {
                        continue;
                    }
                    let claims = ALIASES
                        .iter()
                        .filter(|(a, l)| names.iter().any(|n| n == a) && l.contains(alias))
                        .count();
                    assert_eq!(
                        claims, 1,
                        "{}: the alias {alias} is claimed {claims} times",
                        tool.name
                    );
                }
            }
        }
    }
}
