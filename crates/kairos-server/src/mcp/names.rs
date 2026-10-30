//! One meaning, one argument name (KAIROS-T-0688, pass 1 of KAIROS-D-0016).
//!
//! The test reads the schema of each tool of the server (each profile: the
//! router has all of them) and the objects in its `$defs`. It finds each
//! argument name, and checks it against the tables of this file:
//!
//! - [`NAMES`] gives the meaning of each name. A name that is not in the
//!   table fails: add it with its meaning, or use the name that the table
//!   gives for that meaning.
//! - A name with more than one meaning has one entry per meaning, and each
//!   entry lists its tools and the reason. A new tool with that name fails
//!   until it is in one entry.
//! - Two names with the same meaning fail, unless [`SAME_MEANING`] lists
//!   them with the reason.
//! - A name with more than one JSON type fails, unless [`SHAPES`] lists it
//!   with the reason.
//! - The other names of an argument are in the alias table of
//!   [`super::arguments::ALIASES`] (KAIROS-T-0673). An alias that is also
//!   the name of an argument fails, unless [`ALIAS_IS_ARGUMENT`] lists it
//!   with the reason.

/// One meaning of an argument name.
struct Name {
    name: &'static str,
    meaning: &'static str,
    /// Empty: the one meaning of the name, on each tool. Else the name has
    /// more than one meaning, and this entry is the meaning on these tools.
    tools: &'static [&'static str],
    /// Why the name has more than one meaning. Empty for one meaning.
    reason: &'static str,
}

const fn one(name: &'static str, meaning: &'static str) -> Name {
    Name {
        name,
        meaning,
        tools: &[],
        reason: "",
    }
}

const fn on(
    name: &'static str,
    meaning: &'static str,
    tools: &'static [&'static str],
    reason: &'static str,
) -> Name {
    Name {
        name,
        meaning,
        tools,
        reason,
    }
}

/// The argument names of the tools and their meanings.
const NAMES: &[Name] = &[
    // References to a thing.
    one("short_code", "an item, by short code"),
    one("source", "the item at the start of an edge"),
    one("target", "the item at the end of an edge"),
    on(
        "parent",
        "the parent item",
        &["create_item", "set_parent"],
        "The parent of a team page is a folder, not an item.",
    ),
    on(
        "parent",
        "the folder of a team page",
        &["create_team_page", "move_team_page"],
        "The parent of a team page is a folder, not an item.",
    ),
    one("board", "a board"),
    one("to_board", "the board that an item moves to"),
    one("team", "a team"),
    one("from_team", "the team that sends a request"),
    one("column", "a column"),
    one("to_column", "the column that a move ends in"),
    one("from_column", "the column that a move starts in"),
    one("repository", "a repository"),
    one("user", "a member of the organization"),
    one("claimed_by", "the person with the claim"),
    one("created_by", "the person who created the item"),
    one("owned_by", "the person who owns the item"),
    one("template", "a template"),
    one("definition", "a metadata definition"),
    one("page", "a team page or folder"),
    one("note_id", "a note of an item"),
    one("rule_id", "a dispatch rule"),
    one("target_id", "a webhook target"),
    one("ids", "notifications, by id"),
    one("up_to", "the newest notification to mark"),
    one("before", "the cursor of the next page of notifications"),
    one("host", "a forge host"),
    // The item type and its kinds.
    one("item_type", "the item type"),
    one("entity_type", "the item type"),
    one("entity_types", "the item type"),
    one("task_type", "the task type"),
    one("work_class", "the lane of a task"),
    one("bucket_type", "the type of a bucket initiative"),
    one("complexity", "the complexity of an initiative"),
    one("lifecycle", "the lifecycle of a document"),
    one("board_level", "the level of a board"),
    one("team_type", "the type of a team"),
    one("field_type", "the type of a metadata definition"),
    one("relationship", "the type of an edge"),
    one("relationships", "the types of edge to follow"),
    one("capability", "a board capability"),
    one("capabilities", "the board capabilities of a member"),
    one("forge", "the forge of a repository"),
    one("trigger", "when a dispatch rule fires"),
    on(
        "kind",
        "what a team page is: page | folder",
        &["create_team_page"],
        "A page and a folder are not items. The name of the item type is KAIROS-T-0696.",
    ),
    on(
        "kind",
        "the item type",
        &["promote_note"],
        "The REST body of the promote has `kind` (KAIROS-T-0696).",
    ),
    // Text and names.
    one("title", "the title"),
    one("name", "the name"),
    on(
        "slug",
        "the slug that a thing gets",
        &[
            "add_repository",
            "create_board",
            "create_team",
            "create_team_page",
            "move_team_page",
            "save_metadata_definition",
            "save_template",
            "update_repository",
            "update_team",
        ],
        "On list_templates, `slug` names the template to show.",
    ),
    on(
        "slug",
        "a template",
        &["list_templates"],
        "Kept: an alias `slug` of `template` would apply to create_item and save_template \
         too, where it is wrong.",
    ),
    one("content", "a markdown body"),
    one("text", "a markdown body"),
    one("description", "how to work in a repository"),
    one("hypothesis", "the hypothesis of a strategy"),
    one("reason", "why an item is cancelled"),
    one("why", "why an edge is proposed"),
    one("label", "the label of an external reference"),
    one("search", "the text to find in the content"),
    one("replace", "the text that replaces it"),
    one("q", "the full-text query"),
    one("code_prefix", "the short-code prefix of a board or a team"),
    one("decision_date", "the date of a decision"),
    one("decision_maker", "who makes a decision"),
    one("target_name", "the name of a new webhook target"),
    one("url", "the URL of a webhook target"),
    one("repo_url", "the URL of a repository for a browser"),
    one("repo_full_name", "the name of a repository on its forge"),
    one("token", "a read token"),
    // The live state of a task.
    one("branch", "the branch of the work"),
    one("commit", "the last commit"),
    one("pushed", "the last commit is pushed"),
    one("pr_url", "the URL of the pull request"),
    one("next_step", "what comes next"),
    one("expected_updated_at", "the time of the state that you read"),
    // The review state of a task (KAIROS-T-0718).
    one("review", "the review state: not_needed, needed or done"),
    // Values.
    one("values", "metadata values, by definition slug"),
    one(
        "metadata",
        "metadata: the fields of a template, or filter conditions",
    ),
    one("metadata_value", "the metadata value that a rule matches"),
    one("metadata_missing", "definitions with no value"),
    one("default_value", "the default value of a template field"),
    one("required", "a template field must have a value"),
    one("enum_options", "the values of an enum definition"),
    // The typed sections of a template (KAIROS-T-0729).
    one("sections", "the typed sections of a template"),
    one("heading", "the text of the heading of a typed section"),
    one("type", "the type of a typed section: free or plain"),
    one("default_branch", "the default branch"),
    one(
        "default_branch_source",
        "where the default branch comes from",
    ),
    one("code_index_build", "the code index builder is on or off"),
    one("issue_import_interval", "how often issues are imported"),
    one("forge_filters", "the forge filters of a repository"),
    one("branch_name", "the rule of the branch name"),
    one("issue_author", "the rule of the issue author"),
    one("issue_title", "the rule of the issue title"),
    one("pr_author", "the rule of the PR author"),
    one("pr_head_branch", "the rule of the PR head branch"),
    one("pr_title", "the rule of the PR title"),
    one("include", "the include pattern of a rule"),
    one("exclude", "the exclude pattern of a rule"),
    one("position", "a position, from 0"),
    one("wip_limit", "the WIP limit of a column"),
    one("no_wip_limit", "remove the WIP limit"),
    one("version", "a version of an item or a page"),
    one("request_key", "the key of a retry"),
    // Flags.
    one("adr_board", "also make the ADR board"),
    one("agents_decide", "an agent decides triage"),
    one("all", "each unread notification"),
    one("allowed", "the move is allowed"),
    one("blockers_gate", "the policy blockers_gate"),
    one("owner_gate", "the policy owner_gate"),
    one("parent_gate", "the policy parent_gate"),
    one("claim", "claim the item in the move"),
    one("claims", "a task in the column has a claim"),
    one("keeps_claims", "a claim is kept in the column"),
    one("is_done", "the column is a done column"),
    one(
        "confirm",
        "the caller confirms a write that cannot be undone",
    ),
    one("preview", "show the cascade first"),
    one("rename", "give the item a code of the new board"),
    one("replace_all", "replace each occurrence"),
    one("details", "add details to each card"),
    one("include_deleted", "include archived items"),
    one("is_bucket", "a bucket initiative"),
    one("no_team", "with no team"),
    one("site_enabled", "the site of a team is on"),
    one("to_root", "move a page to the root"),
    one("unread", "only the unread notifications"),
    // Paging, sort, ranges and search.
    one("limit", "the number of rows"),
    one("offset", "the number of rows to skip"),
    one("older_than_days", "an age in days"),
    one("days", "the number of days of a range"),
    one("created_after", "created after a time"),
    one("created_before", "created before a time"),
    one("updated_after", "changed after a time"),
    one("updated_before", "changed before a time"),
    one("filter", "the filter of a search"),
    one("sort", "the sort of a search"),
    one("traverse", "the traversal of a search"),
    one("field", "the field to sort by"),
    one("order", "the order of a sort"),
    one("depth", "the depth of a traversal"),
    one("direction", "the direction of a traversal"),
    on(
        "from",
        "the first day of a range",
        &["board_flow"],
        "A date range is `from` and `to`, as in the REST query of the flow.",
    ),
    on(
        "from",
        "the item that a traversal starts at",
        &["search"],
        "The traversal of a search is an object: its `from` cannot be taken for an \
         argument of the tool. It is the meaning of the alias `from` of `source`.",
    ),
    on(
        "to",
        "the last day of a range",
        &["board_flow", "refusal_counts"],
        "A date range is `from` and `to`, as in the REST query of the flow.",
    ),
    on(
        "to",
        "the person who gets the claim or the ownership",
        &["hand_off_item"],
        "KAIROS-T-0673 kept `to` on hand_off_item. It is not an alias there.",
    ),
];

/// Names that have the same meaning, and why both stay.
const SAME_MEANING: &[(&[&str], &str)] = &[
    (
        &["text", "content"],
        "The body of a note is `text` and the body of an item is `content`. Each is an alias \
         of the other (KAIROS-T-0673, Dylan's decision n-1).",
    ),
    (
        &["template", "slug"],
        "See the entry of `slug` on list_templates in NAMES.",
    ),
    (
        &["item_type", "entity_type", "entity_types", "kind"],
        "Decision of KAIROS-T-0696 (Dylan, 2026-10-25): `entity_type` (one value) and \
         `entity_types` (a list) are one name. `item_type` names the type of the new item \
         on create_item. `kind` is a choice of two (adr or document) on promote_note. The \
         REST names are the same.",
    ),
];

/// Names that have more than one JSON type, and why.
const SHAPES: &[(&str, &str)] = &[
    (
        "entity_type",
        "A filter of search takes a list (any of the values); list_metadata_definitions \
         takes one value. KAIROS-T-0696.",
    ),
    (
        "task_type",
        "A filter of search takes a list (any of the values); a write and a dispatch rule \
         take one value.",
    ),
    (
        "work_class",
        "A filter of search takes a list (any of the values); a write takes one value.",
    ),
    (
        "metadata",
        "The fields of a template are a list; the filter of a search is an object of \
         conditions by slug.",
    ),
];

/// Aliases of [`super::arguments::ALIASES`] that are also argument names,
/// and why. An alias does not apply to a tool that has an argument with
/// its name.
const ALIAS_IS_ARGUMENT: &[(&str, &str)] = &[
    (
        "column",
        "An argument `column` is a column of a board, as the alias of `to_column` is.",
    ),
    (
        "content",
        "The body of an item. It is the alias of `text` on append_note (KAIROS-T-0673).",
    ),
    (
        "text",
        "The body of a note. It is the alias of `content` on the tools of an item \
         (KAIROS-T-0673).",
    ),
    ("from", "See the entries of `from` in NAMES."),
    ("to", "See the entries of `to` in NAMES."),
];

mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use rmcp::model::JsonObject;
    use serde_json::Value;

    use super::*;
    use crate::mcp::arguments::{ALIASES, FIELD_ALIASES};

    /// One argument of one tool: its name, its JSON type, and where it is
    /// (`""` for an argument of the tool, else the name of the object in
    /// `$defs`).
    struct Argument {
        tool: String,
        within: String,
        name: String,
        shape: String,
    }

    /// The JSON type of a property, without `null`.
    fn shape(property: &Value) -> String {
        if property.get("$ref").is_some() {
            return "object".into();
        }
        if let Some(Value::Array(any)) = property.get("anyOf") {
            let all: BTreeSet<String> = any
                .iter()
                .filter(|a| a.get("type").and_then(Value::as_str) != Some("null"))
                .map(shape)
                .collect();
            return all.into_iter().collect::<Vec<_>>().join("|");
        }
        match property.get("type") {
            Some(Value::String(t)) => t.clone(),
            Some(Value::Array(types)) => types
                .iter()
                .filter_map(Value::as_str)
                .filter(|t| *t != "null")
                .collect::<Vec<_>>()
                .join("|"),
            _ => "any".into(),
        }
    }

    fn arguments_of(tool: &str, within: &str, schema: &JsonObject, out: &mut Vec<Argument>) {
        if let Some(Value::Object(properties)) = schema.get("properties") {
            for (name, property) in properties {
                out.push(Argument {
                    tool: tool.into(),
                    within: within.into(),
                    name: name.clone(),
                    shape: shape(property),
                });
            }
        }
    }

    /// Each argument of each tool, and each field of each object in the
    /// `$defs` of a tool.
    fn all_arguments() -> Vec<Argument> {
        let mut out = Vec::new();
        for tool in crate::mcp::KairosMcp::tool_router().list_all() {
            let schema = &tool.input_schema;
            arguments_of(&tool.name, "", schema, &mut out);
            if let Some(Value::Object(defs)) = schema.get("$defs") {
                for (name, def) in defs {
                    if let Some(def) = def.as_object() {
                        arguments_of(&tool.name, name, def, &mut out);
                    }
                }
            }
        }
        out
    }

    fn place(argument: &Argument) -> String {
        if argument.within.is_empty() {
            format!("{}.{}", argument.tool, argument.name)
        } else {
            format!("{} ({}).{}", argument.tool, argument.within, argument.name)
        }
    }

    /// Each argument name has one meaning, or one entry per meaning with
    /// its tools and a reason.
    #[test]
    fn each_argument_name_has_a_meaning() {
        let arguments = all_arguments();
        assert!(arguments.len() > 300, "{} arguments", arguments.len());
        let mut faults = Vec::new();
        for argument in &arguments {
            let entries: Vec<&Name> = NAMES.iter().filter(|n| n.name == argument.name).collect();
            if entries.is_empty() {
                faults.push(format!(
                    "{}: the name is not in NAMES. Add it with its meaning, or use the name \
                     that NAMES gives for that meaning.",
                    place(argument)
                ));
                continue;
            }
            if entries.len() == 1 && entries[0].tools.is_empty() {
                continue;
            }
            let of_tool = entries
                .iter()
                .filter(|n| n.tools.contains(&argument.tool.as_str()))
                .count();
            if of_tool != 1 {
                faults.push(format!(
                    "{}: the name has more than one meaning, and {} entries of NAMES list the \
                     tool. Use the name of its meaning, or list the tool in one entry.",
                    place(argument),
                    of_tool
                ));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// The table has no stale entry, and each entry has the right form.
    #[test]
    fn the_table_of_names_is_correct() {
        let arguments = all_arguments();
        let tools: BTreeSet<&str> = arguments.iter().map(|a| a.tool.as_str()).collect();
        let used: BTreeSet<(&str, &str)> = arguments
            .iter()
            .map(|a| (a.name.as_str(), a.tool.as_str()))
            .collect();
        let mut faults = Vec::new();
        for entry in NAMES {
            let count = NAMES.iter().filter(|n| n.name == entry.name).count();
            if count > 1 && (entry.tools.is_empty() || entry.reason.is_empty()) {
                faults.push(format!(
                    "{}: a name with more than one meaning lists the tools and the reason of \
                     each meaning.",
                    entry.name
                ));
            }
            if count == 1 && !entry.tools.is_empty() {
                faults.push(format!(
                    "{}: a name with one meaning lists no tools.",
                    entry.name
                ));
            }
            if !used.iter().any(|(name, _)| *name == entry.name) {
                faults.push(format!("{}: no tool has this name.", entry.name));
            }
            for tool in entry.tools {
                if !tools.contains(tool) {
                    faults.push(format!("{}: the tool {tool} is not served.", entry.name));
                } else if !used.contains(&(entry.name, *tool)) {
                    faults.push(format!(
                        "{}: the tool {tool} has no argument with this name.",
                        entry.name
                    ));
                }
            }
        }
        for (names, reason) in SAME_MEANING {
            assert!(!reason.is_empty());
            for name in *names {
                if !NAMES.iter().any(|n| n.name == *name) {
                    faults.push(format!("SAME_MEANING: {name} is not in NAMES."));
                }
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// Two names with the same meaning are a fault (KAIROS-D-0016 pass
    /// 1), unless SAME_MEANING lists them. Two entries of NAMES with the
    /// same text of the meaning are the same meaning.
    #[test]
    fn one_meaning_has_one_name() {
        let mut by_meaning: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for entry in NAMES {
            by_meaning
                .entry(entry.meaning)
                .or_default()
                .insert(entry.name);
        }
        let mut faults = Vec::new();
        for (meaning, names) in &by_meaning {
            if names.len() < 2 {
                continue;
            }
            let listed = SAME_MEANING.iter().any(|(group, _)| {
                let group: BTreeSet<&str> = group.iter().copied().collect();
                *names == group
            });
            if !listed {
                faults.push(format!(
                    "{meaning:?} has the names {names:?}. Use one name, or list them in \
                     SAME_MEANING with the reason."
                ));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// A name has one JSON type, unless SHAPES lists it.
    #[test]
    fn a_name_has_one_shape() {
        let mut shapes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for argument in all_arguments() {
            shapes
                .entry(argument.name)
                .or_default()
                .insert(argument.shape);
        }
        let mut faults = Vec::new();
        for (name, kinds) in &shapes {
            let listed = SHAPES.iter().any(|(n, _)| n == name);
            if kinds.len() > 1 && !listed {
                faults.push(format!(
                    "{name} has the types {kinds:?}. Use one type, or list it in SHAPES with \
                     the reason."
                ));
            }
            if kinds.len() == 1 && listed {
                faults.push(format!("{name} has one type: remove it from SHAPES."));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// An alias of KAIROS-T-0673 is not the name of an argument with a
    /// different meaning, unless ALIAS_IS_ARGUMENT lists it.
    #[test]
    fn an_alias_is_not_an_argument() {
        let names: BTreeSet<String> = all_arguments().into_iter().map(|a| a.name).collect();
        let mut faults = Vec::new();
        for (argument, aliases) in ALIASES {
            if !names.contains(*argument) {
                faults.push(format!("ALIASES: no tool has the argument {argument}."));
            }
            for alias in *aliases {
                let listed = ALIAS_IS_ARGUMENT.iter().any(|(a, _)| a == alias);
                if names.contains(*alias) && !listed {
                    faults.push(format!(
                        "{alias} is an alias of {argument} and the name of an argument. Use \
                         a different name, or list it in ALIAS_IS_ARGUMENT with the reason."
                    ));
                }
            }
        }
        // KAIROS-T-0696: an alias of a field of an object argument is not
        // the name of an argument, and its field is one.
        for (object, field, aliases) in FIELD_ALIASES {
            for name in [object, field] {
                if !names.contains(*name) {
                    faults.push(format!("FIELD_ALIASES: no tool has the name {name}."));
                }
            }
            for alias in *aliases {
                if names.contains(*alias) {
                    faults.push(format!(
                        "{alias} is an alias of {object}.{field} and the name of an argument."
                    ));
                }
            }
        }
        for (alias, _) in ALIAS_IS_ARGUMENT {
            if !names.contains(*alias) {
                faults.push(format!(
                    "ALIAS_IS_ARGUMENT: no tool has the argument {alias}."
                ));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }
}
