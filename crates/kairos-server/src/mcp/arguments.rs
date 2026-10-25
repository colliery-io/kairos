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
            show_field_aliases(object, generator.definitions_mut());
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
///
/// KAIROS-T-0674: the parser keeps the path of the fault
/// (`serde_path_to_error`), so the refusal names the argument or the field
/// in an object of the call (`filter.entity_type`).
fn parse<P>(arguments: JsonObject) -> Result<P, ApiError>
where
    P: DeserializeOwned + JsonSchema + 'static,
{
    let schema = schema_for_input::<P>().unwrap_or_default();
    let arguments = with_names(arguments, &properties(&schema))?;
    let call = Value::Object(arguments);
    serde_path_to_error::deserialize(&call).map_err(|e| {
        let path: Vec<Step> = e.path().iter().filter_map(Step::of).collect();
        argument_error(&e.inner().to_string(), &path, &call, &schema)
    })
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
    // KAIROS-T-0688: the old names of the arguments of `set_transition`,
    // `my_boards`, `delete_metadata_definition` and `delete_template`.
    ("from_column", &["from"]),
    ("board_level", &["level"]),
    ("definition", &["reference"]),
    ("template", &["reference"]),
    ("source", &["from"]),
    ("target", &["to"]),
    ("relationship", &["relation", "edge_type"]),
];

/// The other names of a field of an object argument (KAIROS-T-0696): the
/// argument, the field, and its aliases. The search filter takes `board`,
/// `team` and `column` (a slug, a name or a UUID); `board_id`, `team_id`
/// and `column_id` are their old names, and REST still accepts them.
pub(super) const FIELD_ALIASES: &[(&str, &str, &[&str])] = &[
    ("filter", "board", &["board_id"]),
    ("filter", "team", &["team_id"]),
    ("filter", "column", &["column_id"]),
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
    for (object, field, aliases) in FIELD_ALIASES {
        if !names.iter().any(|n| n == object) {
            continue;
        }
        let Some(Value::Object(fields)) = arguments.get_mut(*object) else {
            continue;
        };
        for alias in *aliases {
            let Some(value) = fields.remove(*alias) else {
                continue;
            };
            if fields.contains_key(*field) {
                return Err(validation(format!(
                    "The call has the field {field:?} and the field {alias:?} in {object}. \
                     They are the same field. Send only one of them."
                ))
                .with_details(json!({
                    "field": format!("{object}.{alias}"),
                    "argument": format!("{object}.{field}"),
                })));
            }
            fields.insert(field.to_string(), value);
        }
    }
    Ok(arguments)
}

/// The aliases of the fields of [`FIELD_ALIASES`] in the schema of the
/// object of the argument (in `$defs`): in the description, and as the
/// list `x-aliases`.
fn show_field_aliases(schema: &JsonObject, definitions: &mut JsonObject) {
    for (object, field, aliases) in FIELD_ALIASES {
        let Some(property) = schema.get("properties").and_then(|p| p.get(*object)) else {
            continue;
        };
        let Some(name) = reference_of(property) else {
            continue;
        };
        let Some(Value::Object(def)) = definitions.get_mut(&name) else {
            continue;
        };
        let Some(Value::Object(fields)) = def.get_mut("properties") else {
            continue;
        };
        if aliases.iter().any(|alias| fields.contains_key(*alias)) {
            continue;
        }
        let Some(Value::Object(property)) = fields.get_mut(*field) else {
            continue;
        };
        let list = aliases
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
        property.insert("x-aliases".into(), json!(aliases));
    }
}

/// The name in `$defs` of the object of a property: its `$ref`, also in an
/// `anyOf` with `null`.
fn reference_of(property: &Value) -> Option<String> {
    if let Some(reference) = property.get("$ref").and_then(Value::as_str) {
        return reference.strip_prefix("#/$defs/").map(str::to_string);
    }
    property
        .get("anyOf")
        .and_then(Value::as_array)?
        .iter()
        .find_map(reference_of)
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
            // KAIROS-T-0673: a known guess names the argument to use.
            if let Some(right) = hint(&argument, &allowed) {
                return validation(format!(
                    "The call has the argument {argument:?}. This tool does not accept that \
                     argument. Use the argument {right:?}. {known}"
                ))
                .with_details(
                    json!({ "field": argument, "allowed": allowed, "did_you_mean": right }),
                );
            }
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
        // KAIROS-T-0690: the words of the schema, not the text of `serde`.
        let allowed = properties(schema);
        return validation(format!(
            "The arguments of this tool must be one object with these arguments: {}.",
            allowed.join(", ")
        ))
        .with_details(json!({ "allowed": allowed }));
    }
    // A fault of type gets the type of the schema. A value of the correct
    // type that is not correct (an unknown variant, a bad UUID) gets the
    // values or the format of the schema, and the value of the call
    // (KAIROS-T-0690: never the text of `serde`).
    let given = value_at(call, path);
    let wrong_type = serde_message.starts_with("invalid type:");
    let expected = schema_at(schema, path).and_then(|s| {
        if wrong_type {
            describe(schema, s)
        } else {
            format_of(schema, s).or_else(|| describe(schema, s))
        }
    });
    match expected {
        Some(expected) if !wrong_type => {
            let mut message = format!("The value of {field} must be {expected}.");
            if let Some(given) = given.filter(|g| g.is_string() || g.is_number()) {
                let text = given.to_string();
                if text.chars().count() <= 80 {
                    message.push_str(&format!(" The call gives {text}."));
                }
            }
            validation(message).with_details(json!({ "field": field, "expected": expected }))
        }
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
            "The value of {field} is not a value that this tool accepts. The schema of the \
             tool in tools/list gives the values."
        ))
        .with_details(json!({ "field": field })),
    }
}

/// The format of a string of the schema in words: `a UUID`. `None` for a
/// string with no format that the words help.
fn format_of(root: &JsonObject, schema: &Value) -> Option<String> {
    let schema = resolve(root, schema)?;
    match schema.get("format")?.as_str()? {
        "uuid" => Some("a UUID".into()),
        "date" => Some("a date in the form YYYY-MM-DD".into()),
        "date-time" => Some("an RFC 3339 timestamp".into()),
        _ => None,
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

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Find {
        filter: Option<Filter>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[schemars(crate = "rmcp::schemars")]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Filter {
        board: Option<String>,
        team: Option<String>,
    }

    /// KAIROS-T-0696: an old name of a field of the filter is the field.
    #[test]
    fn an_old_name_of_a_field_of_the_filter_is_the_field() {
        let Value::Object(call) = json!({"filter": {"board_id": "0193a1c2-0000-7000-8000-000000000003", "team_id": "platform"}})
        else {
            unreachable!()
        };
        let found = parse::<Find>(call).expect("the old names are read");
        let filter = found.filter.expect("the filter");
        assert_eq!(
            filter.board.as_deref(),
            Some("0193a1c2-0000-7000-8000-000000000003")
        );
        assert_eq!(filter.team.as_deref(), Some("platform"));
        let err = refusal_of::<Find>(json!({"filter": {"board": "a", "board_id": "b"}}));
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The call has the field \"board\" and the field \"board_id\" in filter. They are \
             the same field. Send only one of them."
        );
        assert_eq!(
            err.details,
            json!({"field": "filter.board_id", "argument": "filter.board"})
        );
        // A tool with no `filter` argument does not rename.
        let err = refusal_of::<Read>(json!({"short_code": "A", "filter": {"board_id": "b"}}));
        assert_eq!(err.details["field"], "filter");
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

    /// KAIROS-T-0674, KAIROS-T-0690: a value of the correct type that is
    /// not correct gets the values of the schema and the value of the
    /// call, with the path; not the text of `serde`.
    #[test]
    fn a_wrong_value_names_the_values_with_the_path() {
        let err = refusal_of::<Read>(
            json!({"short_code": "ACME-T-0001", "within": {"from": "a", "mode": "quick"}}),
        );
        assert_eq!(
            err.message,
            "The value of within.mode must be one of these values: fast, slow. The call \
             gives \"quick\"."
        );
        assert_eq!(
            err.details,
            json!({"field": "within.mode", "expected": "one of these values: fast, slow"})
        );
        assert!(!err.message.contains("variant"), "{}", err.message);
    }

    /// KAIROS-T-0690: arguments that are not an object get the list of the
    /// arguments, not the text of `serde`.
    #[test]
    fn arguments_that_are_not_an_object_get_the_list() {
        let schema = schema_for_input::<Read>().expect("a schema");
        let err = argument_error(
            "invalid type: sequence, expected struct Read",
            &[],
            &json!([]),
            &schema,
        );
        assert_eq!(
            err.message,
            "The arguments of this tool must be one object with these arguments: limit, \
             short_code, within."
        );
        assert!(!err.message.contains("struct"), "{}", err.message);
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
        // KAIROS-T-0696: the old names of the fields of the search filter.
        let search = schema_of("search");
        let filter = reference_of(&search["properties"]["filter"]).expect("the $ref of filter");
        let fields = &search["$defs"][&filter]["properties"];
        assert_eq!(fields["board"]["x-aliases"], json!(["board_id"]));
        assert_eq!(fields["team"]["x-aliases"], json!(["team_id"]));
        assert_eq!(fields["column"]["x-aliases"], json!(["column_id"]));
        assert!(
            fields["board"]["description"]
                .as_str()
                .unwrap()
                .ends_with("Other names that the tool accepts: `board_id`."),
            "{}",
            fields["board"]
        );
        for old in ["board_id", "team_id", "column_id"] {
            assert!(
                fields.get(old).is_none(),
                "{old} is not a field of the filter"
            );
        }
        // KAIROS-T-0688: the old names of the renamed arguments.
        assert_eq!(aliases("set_transition", "from_column"), json!(["from"]));
        assert_eq!(
            aliases("set_transition", "to_column"),
            json!(["to", "column"])
        );
        assert_eq!(aliases("my_boards", "board_level"), json!(["level"]));
        assert_eq!(
            aliases("delete_metadata_definition", "definition"),
            json!(["reference"])
        );
        assert_eq!(aliases("delete_template", "template"), json!(["reference"]));
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
