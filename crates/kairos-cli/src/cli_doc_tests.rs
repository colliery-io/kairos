//! The reference page of the CLI mirrors `kairos --help` (KAIROS-T-0596).
//!
//! `docs/src/content/docs/reference/cli.md` documents each command of the
//! CLI. These tests walk the clap tree and the page. A new command with no
//! section, a section of a removed command, or a stale verbs table fails.
//! They check the names of the commands, not their options.

use std::collections::BTreeSet;

use clap::{Command, CommandFactory};

use crate::Cli;

/// The reference page, read at compile time: the test needs no checkout.
const PAGE: &str = include_str!("../../../docs/src/content/docs/reference/cli.md");

/// The five nouns of one generated family. The page documents their shared
/// verbs once, under `<noun> <verb>`, and gives the verbs table.
const ENTITY_NOUNS: [&str; 5] = ["strategies", "initiatives", "tasks", "documents", "adrs"];

/// The subcommands of a command, without the `help` of clap.
fn subcommands(command: &Command) -> impl Iterator<Item = &Command> {
    command
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help")
}

/// Each command path that runs: a command with no subcommands, and a group
/// with options of its own.
fn runnable_paths(command: &Command, prefix: &str, out: &mut Vec<String>) {
    let has_subcommands = subcommands(command).next().is_some();
    let own_options = command
        .get_arguments()
        .any(|arg| !matches!(arg.get_id().as_str(), "help" | "version"));
    if !prefix.is_empty() && (!has_subcommands || own_options) {
        out.push(prefix.to_string());
    }
    for sub in subcommands(command) {
        let path = if prefix.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{prefix} {}", sub.get_name())
        };
        runnable_paths(sub, &path, out);
    }
}

/// True when `path` names a command of the tree, by its name or an alias.
fn is_command(root: &Command, path: &str) -> bool {
    let mut command = root;
    for word in path.split_whitespace() {
        let found = subcommands(command)
            .find(|sub| sub.get_name() == word || sub.get_all_aliases().any(|alias| alias == word));
        match found {
            Some(sub) => command = sub,
            None => return false,
        }
    }
    true
}

/// The command path of a synopsis line: the words after `kairos`, up to the
/// first argument or option. `kairos <noun> list` gives none.
fn synopsis_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("kairos ")?;
    let words: Vec<&str> = rest
        .split_whitespace()
        .take_while(|word| !word.starts_with(['<', '[', '-', '(', '|']))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The text in the first backticks of a heading: `kairos repos get`.
fn heading_code(line: &str) -> Option<&str> {
    let text = line.trim_start_matches('#').trim_start();
    text.strip_prefix('`')?.split('`').next()
}

/// What the page documents.
struct Page {
    /// The command paths of the headings and of the synopsis lines.
    documented: BTreeSet<String>,
    /// Each path that a line names with `kairos`, and the line.
    named: Vec<(String, String)>,
}

fn read_page() -> Page {
    let mut documented = BTreeSet::new();
    let mut named = Vec::new();
    let mut in_code = false;
    for line in PAGE.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            if let Some(path) = synopsis_path(line) {
                documented.insert(path.clone());
                named.push((path, line.to_string()));
            }
            continue;
        }
        if !line.starts_with('#') {
            continue;
        }
        let Some(code) = heading_code(line) else {
            continue;
        };
        if let Some(verb) = code.strip_prefix("<noun> ") {
            for noun in ENTITY_NOUNS {
                documented.insert(format!("{noun} {verb}"));
            }
        } else if let Some(path) = synopsis_path(code) {
            documented.insert(path.clone());
            named.push((path, line.to_string()));
        } else {
            documented.insert(code.to_string());
        }
    }
    Page { documented, named }
}

/// The rows of the table "Verbs per noun": the nouns of the header, then
/// each verb with a mark per noun (`yes` is true).
fn verbs_table() -> (Vec<String>, Vec<(String, Vec<bool>)>) {
    let start = PAGE
        .find("### Verbs per noun")
        .expect("cli.md has the section \"Verbs per noun\"");
    let mut rows = PAGE[start..]
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .map(|row| {
            row.trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>()
        });
    let header = rows.next().expect("the verbs table has a header");
    let nouns = header[1..]
        .iter()
        .map(|cell| cell.trim_matches('`').to_string())
        .collect();
    let verbs = rows
        .skip(1)
        .map(|cells| {
            let verb = cells[0]
                .split('`')
                .nth(1)
                .expect("each row of the verbs table names a verb in backticks")
                .to_string();
            let marks = cells[1..].iter().map(|cell| *cell == "yes").collect();
            (verb, marks)
        })
        .collect();
    (nouns, verbs)
}

#[test]
fn each_command_has_a_section_or_a_synopsis_line() {
    let root = Cli::command();
    let mut paths = Vec::new();
    runnable_paths(&root, "", &mut paths);
    let page = read_page();
    let missing: Vec<&String> = paths
        .iter()
        .filter(|path| !page.documented.contains(*path))
        .collect();
    assert!(
        missing.is_empty(),
        "cli.md has no section and no synopsis line for these commands: {missing:?}. \
         Write a section from the `--help` of each."
    );
}

#[test]
fn each_command_that_the_page_names_is_a_command_of_the_cli() {
    let root = Cli::command();
    let page = read_page();
    let stale: Vec<&str> = page
        .named
        .iter()
        .filter(|(path, _)| !is_command(&root, path))
        .map(|(_, line)| line.as_str())
        .collect();
    assert!(
        stale.is_empty(),
        "cli.md names commands that the CLI does not have: {stale:?}"
    );
}

#[test]
fn the_verbs_table_matches_the_five_nouns() {
    let root = Cli::command();
    let (nouns, verbs) = verbs_table();
    assert_eq!(nouns, ENTITY_NOUNS, "the columns of the verbs table");
    for (index, noun) in ENTITY_NOUNS.iter().enumerate() {
        let command = subcommands(&root)
            .find(|sub| sub.get_name() == *noun)
            .unwrap_or_else(|| panic!("the CLI has the noun {noun}"));
        let in_cli: BTreeSet<&str> = subcommands(command).map(Command::get_name).collect();
        let in_table: BTreeSet<&str> = verbs
            .iter()
            .filter(|(_, marks)| marks.get(index).copied().unwrap_or(false))
            .map(|(verb, _)| verb.as_str())
            .collect();
        assert_eq!(
            in_table, in_cli,
            "the verbs of `{noun}` in the verbs table of cli.md (left) and in the CLI (right)"
        );
    }
}
