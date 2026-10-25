//! Each tool and each argument that a text names exists (KAIROS-T-0692,
//! pass 7 of KAIROS-D-0016).
//!
//! The test reads the texts that an agent reads:
//!
//! - the description of each tool and of each argument (each profile),
//! - the instructions of the server ([`super::service::INSTRUCTIONS`]),
//! - the string literals of the server code: the refusals, the answers and
//!   the fix details,
//! - the skills of the plugin (`plugin/skills/**/*.md`).
//!
//! In each text it finds the names in backticks. The name of a tool must be
//! a tool that the server serves. The name of an argument must be an
//! argument of the tool that the text is about: the tool of the
//! description, the tool whose code has the literal, or a tool that the
//! same text names. A name that is neither, but has the form of a tool or
//! of an argument, is an error, unless [`WORDS`] lists it with the reason.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Names in backticks that are not a tool and not an argument, and why.
/// Each is a value or a word of Kairos that a text names.
const WORDS: &[(&str, &str)] = &[
    // Board capabilities: `transition_items` has the form of a tool name.
    ("transition_items", "a board capability"),
    // The keys of the settings file of the plugin (.claude/kairos.local.md).
    ("team_board", "a key of the settings file of the plugin"),
    // The tools of the code-index MCP server of the CLI (kairos-code).
    ("fetch_index", "a tool of the kairos-code server"),
    ("module_map", "a tool of the kairos-code server"),
    ("code_search", "a tool of the kairos-code server"),
];

/// Texts that name a tool or an argument that the check cannot place,
/// with the reason. Each entry is the place and the name.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "the description of propose_edge",
        "parent",
        "`parent` is a value of `relationship` there.",
    ),
    (
        "propose_edge.",
        "parent",
        "`parent` is a value of `relationship` there.",
    ),
    (
        "plugin/skills/engineering/code-review/SKILL.md:34",
        "repository",
        "`repository: <slug>` is a line of the answer of get_item.",
    ),
    (
        "plugin/skills/workflow/implement/SKILL.md:16",
        "repository",
        "`repository: <slug>` is a line of the answer of get_item.",
    ),
    (
        "plugin/skills/meta/bootstrap/SKILL.md",
        "repository",
        "`repository:` is a key of the settings file of the plugin.",
    ),
];

/// The tools of the server and the arguments of each: top-level names, and
/// `object.field` for each field of an object in `$defs` (also the field
/// alone, as texts name it: `filter.board` and `board`).
struct Surface {
    tools: BTreeMap<String, BTreeSet<String>>,
    aliases: BTreeMap<String, BTreeSet<String>>,
    /// The first word of each tool name with more than one word.
    prefixes: BTreeSet<String>,
    /// Each argument name of each tool.
    all_arguments: BTreeSet<String>,
}

impl Surface {
    fn new() -> Self {
        let mut tools = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        let mut all_arguments = BTreeSet::new();
        for tool in super::all_tools() {
            let mut names = BTreeSet::new();
            let mut other = BTreeSet::new();
            let schema = &tool.input_schema;
            let mut objects: Vec<(Option<String>, &serde_json::Map<String, Value>)> =
                vec![(None, schema.as_ref())];
            let properties_of = |object: &serde_json::Map<String, Value>| {
                object
                    .get("properties")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default()
            };
            // The argument that holds each object of `$defs`.
            let mut holder: BTreeMap<String, String> = BTreeMap::new();
            for (name, property) in properties_of(schema) {
                let reference = property
                    .get("$ref")
                    .or_else(|| {
                        property
                            .get("anyOf")
                            .and_then(Value::as_array)
                            .and_then(|any| any.iter().find_map(|a| a.get("$ref")))
                    })
                    .or_else(|| property.get("items").and_then(|i| i.get("$ref")))
                    .and_then(Value::as_str);
                if let Some(reference) = reference {
                    holder.insert(
                        reference.trim_start_matches("#/$defs/").to_string(),
                        name.clone(),
                    );
                }
            }
            if let Some(Value::Object(defs)) = schema.get("$defs") {
                for (name, def) in defs {
                    if let Some(def) = def.as_object() {
                        objects.push((holder.get(name).cloned(), def));
                    }
                }
            }
            for (within, object) in objects {
                for (name, property) in properties_of(object) {
                    if let Some(within) = &within {
                        names.insert(format!("{within}.{name}"));
                    }
                    if let Some(Value::Array(list)) = property.get("x-aliases") {
                        other.extend(list.iter().filter_map(Value::as_str).map(String::from));
                    }
                    all_arguments.insert(name.clone());
                    names.insert(name);
                }
            }
            aliases.insert(tool.name.to_string(), other);
            tools.insert(tool.name.to_string(), names);
        }
        let prefixes = tools
            .keys()
            .filter_map(|name| name.split_once('_').map(|(first, _)| first.to_string()))
            .collect();
        Surface {
            tools,
            aliases,
            prefixes,
            all_arguments,
        }
    }

    /// `name` has the form of a tool name: two words or more, and the first
    /// is the first word of a tool.
    fn looks_like_a_tool(&self, name: &str) -> bool {
        match name.split_once('_') {
            Some((first, rest)) => !rest.is_empty() && self.prefixes.contains(first),
            None => false,
        }
    }

    /// `name` is an argument (or an alias) of `tool`.
    fn has(&self, tool: &str, name: &str) -> bool {
        self.tools.get(tool).is_some_and(|n| n.contains(name))
            || self.aliases.get(tool).is_some_and(|a| a.contains(name))
    }
}

/// One text, the tool that it is about (if any), and where it is.
struct Text {
    place: String,
    tool: Option<String>,
    text: String,
}

/// The names in backticks of a text: the first identifier of each, and
/// whether the backticks give it a value (`board_level: delivery`,
/// `repository=<slug>`) or it is a key of a JSON object (`{"board": ...}`).
fn names_in(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut parts = text.split('`');
    parts.next();
    while let Some(inside) = parts.next() {
        // The text between this pair and the next.
        parts.next();
        let inside = inside.trim();
        if inside.starts_with('{') {
            // The keys of the object: a string with a `:` after it.
            let pieces: Vec<&str> = inside.split('"').collect();
            for (n, key) in pieces.iter().enumerate().skip(1).step_by(2) {
                let after = pieces.get(n + 1).copied().unwrap_or("");
                if is_identifier(key) && after.trim_start().starts_with(':') {
                    out.push((key.to_string(), true));
                }
            }
            continue;
        }
        let end = inside
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'))
            .unwrap_or(inside.len());
        let name = inside[..end].trim_end_matches('.');
        if !is_identifier(name.split('.').next().unwrap_or("")) {
            continue;
        }
        let after = inside[end..].trim_start();
        let valued = (after.starts_with(':') || after.starts_with('=')) && !after.starts_with("::");
        out.push((name.to_string(), valued));
        // `tool arg=value arg: value` in one pair of backticks.
        for word in after.split_whitespace() {
            if let Some((key, _)) = word.split_once('=')
                && is_identifier(key)
            {
                out.push((key.to_string(), true));
            }
            if let Some(key) = word.strip_suffix(':')
                && is_identifier(key)
            {
                out.push((key.to_string(), true));
            }
        }
    }
    out
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The faults of one text.
/// `used` gets each entry of [`WORDS`] and [`EXCEPTIONS`] that keeps a
/// name from being a fault.
fn faults_of(
    surface: &Surface,
    text: &Text,
    faults: &mut Vec<String>,
    used: &mut BTreeSet<String>,
) {
    let names = names_in(&text.text);
    // A text of a tool is about that tool and each tool that it names. A
    // different text is about the tool that it names last before the
    // argument (`get_item` with `short_code: ...`).
    let named: Vec<&str> = names
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| surface.tools.contains_key(*name))
        .collect();
    let mut last: Option<&str> = None;
    for (name, valued) in &names {
        let name = name.as_str();
        if surface.tools.contains_key(name) {
            last = Some(name);
        }
        let about: Vec<&str> = match &text.tool {
            Some(tool) => std::iter::once(tool.as_str())
                .chain(named.iter().copied())
                .collect(),
            None => last.into_iter().collect(),
        };
        if surface.tools.contains_key(name) {
            continue;
        }
        if WORDS.iter().any(|(w, _)| *w == name) {
            used.insert(format!("word {name}"));
            continue;
        }
        if let Some((place, _, _)) = EXCEPTIONS
            .iter()
            .find(|(place, n, _)| *n == name && text.place.starts_with(place))
        {
            used.insert(format!("exception {place} {name}"));
            continue;
        }
        let of_a_tool_about = about.iter().any(|tool| surface.has(tool, name));
        let field = name.rsplit('.').next().unwrap_or(name);
        let an_argument = surface.all_arguments.contains(field);
        if surface.looks_like_a_tool(name) && !an_argument {
            faults.push(format!(
                "{}: `{name}` has the form of a tool, and the server has no tool {name}.",
                text.place
            ));
            continue;
        }
        if of_a_tool_about {
            continue;
        }
        // An argument of a tool that the text is about must be one of its
        // arguments. A text about no tool can name each argument; one
        // with a value (`name: value`) must be about a tool.
        let wrong = !about.is_empty() && an_argument && (*valued || text.tool.is_some());
        if wrong {
            faults.push(format!(
                "{}: `{name}` is not an argument of {}.",
                text.place,
                about.join(", ")
            ));
        }
    }
}

/// The descriptions of each tool and of each of its arguments.
fn descriptions() -> Vec<Text> {
    let mut out = Vec::new();
    for tool in super::all_tools() {
        let name = tool.name.to_string();
        out.push(Text {
            place: format!("the description of {name}"),
            tool: Some(name.clone()),
            text: tool.description.as_deref().unwrap_or("").to_string(),
        });
        let schema = &tool.input_schema;
        let mut objects = vec![schema.as_ref().clone()];
        if let Some(Value::Object(defs)) = schema.get("$defs") {
            objects.extend(defs.values().filter_map(|d| d.as_object().cloned()));
        }
        for object in objects {
            if let Some(Value::Object(properties)) = object.get("properties") {
                for (argument, property) in properties {
                    let text = property
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    out.push(Text {
                        place: format!("{name}.{argument}"),
                        tool: Some(name.clone()),
                        text: text.to_string(),
                    });
                }
            }
        }
    }
    out
}

/// The string literals of a Rust file, with the line of each. Comments are
/// not literals.
fn literals(source: &str) -> Vec<(usize, String)> {
    let bytes: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && bytes.get(i + 1) == Some(&'/') {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && bytes.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                if bytes[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c == '\'' {
            // A char literal ('x', '\n', '\'') or a lifetime ('a).
            if bytes.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < bytes.len() && bytes[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if bytes.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c == 'r'
            && (bytes.get(i + 1) == Some(&'"') || bytes.get(i + 1) == Some(&'#'))
            && (i == 0 || !(bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_'))
        {
            let mut hashes = 0;
            let mut j = i + 1;
            while bytes.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if bytes.get(j) != Some(&'"') {
                i += 1;
                continue;
            }
            let start_line = line;
            j += 1;
            let mut text = String::new();
            loop {
                if j >= bytes.len() {
                    break;
                }
                if bytes[j] == '"' && (1..=hashes).all(|k| bytes.get(j + k) == Some(&'#')) {
                    j += 1 + hashes;
                    break;
                }
                if bytes[j] == '\n' {
                    line += 1;
                }
                text.push(bytes[j]);
                j += 1;
            }
            out.push((start_line, text));
            i = j;
        } else if c == '"' {
            let start_line = line;
            let mut j = i + 1;
            let mut text = String::new();
            while j < bytes.len() && bytes[j] != '"' {
                if bytes[j] == '\\' {
                    // A line that goes on: the blanks of the next line go.
                    if bytes.get(j + 1) == Some(&'\n') {
                        j += 2;
                        line += 1;
                        while j < bytes.len() && bytes[j].is_whitespace() {
                            if bytes[j] == '\n' {
                                line += 1;
                            }
                            j += 1;
                        }
                        continue;
                    }
                    text.push(bytes[j]);
                    if let Some(next) = bytes.get(j + 1) {
                        text.push(*next);
                    }
                    j += 2;
                    continue;
                }
                if bytes[j] == '\n' {
                    line += 1;
                }
                text.push(bytes[j]);
                j += 1;
            }
            out.push((start_line, text));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source folder") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a skills folder") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// The tool of each line of `mcp/tools.rs`: the tool whose function the
/// line is in, from `pub async fn <tool>(` to the next function.
fn tool_of_lines(source: &str, surface: &Surface) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("pub async fn ")
            .or_else(|| trimmed.strip_prefix("async fn "))
            .or_else(|| trimmed.strip_prefix("pub fn "))
            .or_else(|| trimmed.strip_prefix("fn "))
            .or_else(|| trimmed.strip_prefix("pub(crate) fn "))
            .or_else(|| trimmed.strip_prefix("pub(super) fn "))
        {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            current = surface.tools.contains_key(&name).then_some(name);
        }
        out.push(current.clone());
    }
    out
}

/// The string literals of the server code that an agent can read.
fn code_texts(surface: &Surface) -> Vec<Text> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("a source file");
        let relative = path
            .strip_prefix(&root)
            .expect("in src")
            .display()
            .to_string();
        // This file names faults on purpose.
        if relative == "mcp/mentions.rs" {
            continue;
        }
        let tools = (relative == "mcp/tools.rs").then(|| tool_of_lines(&source, surface));
        let lines: Vec<&str> = source.lines().collect();
        // A fix detail (`"fix": "claim_item ..."`) is a call: the text in
        // it, on the line of the key or in the four lines after it, is read
        // as a name in backticks.
        let fix_lines: BTreeSet<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("\"fix\":"))
            .flat_map(|(n, _)| (n + 1)..=(n + 5))
            .collect();
        for (line, text) in literals(&source) {
            let text = if fix_lines.contains(&line) && text != "fix" {
                format!("`{text}`")
            } else {
                text
            };
            if !text.contains('`') {
                continue;
            }
            // A tool description: the test of the descriptions reads it.
            let before = lines.get(line - 1).copied().unwrap_or("");
            if before.contains("description =") {
                continue;
            }
            let tool = tools
                .as_ref()
                .and_then(|t| t.get(line - 1).cloned().flatten());
            out.push(Text {
                place: format!("src/{relative}:{line}"),
                tool,
                text,
            });
        }
    }
    out
}

/// The lines of the skills of the plugin.
fn skill_texts() -> Vec<Text> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/skills");
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("a skill file");
        let relative = path
            .strip_prefix(&root)
            .expect("in the skills")
            .display()
            .to_string();
        for (n, line) in source.lines().enumerate() {
            if line.contains('`') {
                out.push(Text {
                    place: format!("plugin/skills/{relative}:{}", n + 1),
                    tool: None,
                    text: line.to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(texts: &[Text]) -> Vec<String> {
        let surface = Surface::new();
        let mut faults = Vec::new();
        let mut used = BTreeSet::new();
        for text in texts {
            faults_of(&surface, text, &mut faults, &mut used);
        }
        faults
    }

    /// Each text of the four sources.
    fn all_texts() -> Vec<Text> {
        let surface = Surface::new();
        let mut texts = descriptions();
        texts.push(Text {
            place: "the instructions of the server".into(),
            tool: None,
            text: super::super::service::INSTRUCTIONS.to_string(),
        });
        texts.extend(code_texts(&surface));
        texts.extend(skill_texts());
        texts
    }

    #[test]
    fn the_names_in_backticks() {
        assert_eq!(
            names_in("Call `my_boards` with `board_level: delivery`, then `get_item`."),
            vec![
                ("my_boards".to_string(), false),
                ("board_level".to_string(), true),
                ("get_item".to_string(), false)
            ]
        );
        assert_eq!(
            names_in("`board_items repository=<slug>` and `{\"filter\": {\"board\": \"x\"}}`"),
            vec![
                ("board_items".to_string(), false),
                ("repository".to_string(), true),
                ("filter".to_string(), true),
                ("board".to_string(), true)
            ]
        );
        assert!(names_in("`ACME-T-0012` and `/mcp?profile=admin`").is_empty());
    }

    #[test]
    fn the_literals_of_a_file() {
        let source = "// a `comment`\nlet a = \"one `x`\";\nlet c = '\"';\nlet b = r#\"two \"`y`\"\"#;\nlet d = \"go \\\n    on\";\n";
        let found: Vec<String> = literals(source).into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, vec!["one `x`", "two \"`y`\"", "go on"]);
    }

    /// The check finds a tool that is not served and an argument of the
    /// wrong tool.
    #[test]
    fn the_check_finds_the_faults() {
        let texts = [
            Text {
                place: "a".into(),
                tool: None,
                text: "Call `list_columns` first.".into(),
            },
            Text {
                place: "b".into(),
                tool: None,
                text: "`my_boards` with `kind: page`.".into(),
            },
            Text {
                place: "c".into(),
                tool: Some("get_item".into()),
                text: "Send `to_column`.".into(),
            },
            Text {
                place: "d".into(),
                tool: None,
                text: "`transition_item` with `to_column: Active`, `get_item` `id`.".into(),
            },
        ];
        let faults = check(&texts);
        assert_eq!(faults.len(), 3, "{faults:#?}");
        assert!(faults[0].starts_with("a: `list_columns`"), "{}", faults[0]);
        assert!(faults[1].starts_with("b: `kind`"), "{}", faults[1]);
        assert!(faults[2].starts_with("c: `to_column`"), "{}", faults[2]);
    }

    #[test]
    fn each_name_in_a_description_exists() {
        let faults = check(&descriptions());
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    #[test]
    fn each_name_in_the_instructions_exists() {
        let faults = check(&[Text {
            place: "the instructions of the server".into(),
            tool: None,
            text: super::super::service::INSTRUCTIONS.to_string(),
        }]);
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    #[test]
    fn each_name_in_a_text_of_the_code_exists() {
        let surface = Surface::new();
        let texts = code_texts(&surface);
        assert!(texts.len() > 50, "{} texts", texts.len());
        let faults = check(&texts);
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    #[test]
    fn each_name_in_a_skill_exists() {
        let texts = skill_texts();
        assert!(texts.len() > 100, "{} lines", texts.len());
        let faults = check(&texts);
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// Each entry of WORDS and EXCEPTIONS has a reason and keeps a name of
    /// a text from being a fault: a stale entry fails.
    #[test]
    fn each_exception_is_used() {
        let surface = Surface::new();
        let mut faults = Vec::new();
        let mut used = BTreeSet::new();
        for text in all_texts() {
            faults_of(&surface, &text, &mut faults, &mut used);
        }
        for (place, name, reason) in EXCEPTIONS {
            assert!(!reason.is_empty(), "{name}");
            assert!(
                used.contains(&format!("exception {place} {name}")),
                "the exception {place} {name} is not used: remove it"
            );
        }
        for (word, reason) in WORDS {
            assert!(!reason.is_empty(), "{word}");
            assert!(
                used.contains(&format!("word {word}")),
                "the word {word} is not used: remove it"
            );
        }
    }
}
