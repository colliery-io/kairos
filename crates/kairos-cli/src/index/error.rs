//! The errors of `kairos index` and of the code tools (COLLIERY-T-1852).
//! The texts follow ASD-STE100: `scripts/ste-check.py --code` reads them.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::error::CliError;

/// An error of a `kairos index` command.
#[derive(Debug, thiserror::Error)]
pub enum IndexCommandError {
    #[error("No index is at {0}. Run `kairos index build` first.")]
    NoIndex(PathBuf),
    #[error(
        "No index that this CLI can update is at {path}, and Kairos gave no base index, because {why}. Run `kairos index --full`."
    )]
    NoStart { path: PathBuf, why: String },
    /// More files than the limit changed since the index that the update
    /// starts from (KAIROS-T-0517).
    #[error(transparent)]
    TooFar(Box<TooFar>),
    #[error("The folder {0} is not there.")]
    NoRoot(PathBuf),
    #[error("Git did not run in {path}: {message}.")]
    Git { path: PathBuf, message: String },
    #[error("The CLI did not write {path}: {source}.")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Index(#[from] kairos_index::IndexError),
    #[error("The MCP server stopped: {0}.")]
    Server(String),
    /// The symbol of a command is not found, or is not one symbol: the
    /// error of the code tools (KAIROS-T-0419).
    #[error(transparent)]
    Symbol(Box<ToolError>),
}

/// The text of [`IndexCommandError::TooFar`]: three parts, each of whole
/// sentences.
#[derive(Debug, Clone, PartialEq)]
pub struct TooFar {
    pub since: Since,
    pub stop: Stop,
    pub advice: Advice,
}

impl std::fmt::Display for TooFar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {}", self.since, self.stop, self.advice)
    }
}

impl std::error::Error for TooFar {}

/// The count of changed files, and the index that an update refused to
/// start from: the start of the text of [`IndexCommandError::TooFar`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Since {
    #[error("{changed} files changed since {commit}, the nearest indexed commit in Kairos.")]
    Base { changed: usize, commit: String },
    /// Kairos has no index of the commit that the CLI asked for: the
    /// nearest is `distance` commits before it.
    #[error(
        "{changed} files changed since {commit}, the nearest indexed commit in Kairos. It is {distance} commits before {from}."
    )]
    Below {
        changed: usize,
        commit: String,
        distance: usize,
        from: String,
    },
    /// The local index is nearer to the tree than the base (KAIROS-T-0515).
    #[error(
        "{changed} files changed since the local index. It is nearer to the tree than {commit}, the nearest indexed commit in Kairos."
    )]
    Local { changed: usize, commit: String },
}

/// What refused: `kairos index update` or the tool `fetch_index`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Caller {
    Cli,
    Tool,
}

/// The limit, and what the refusal did not do.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum Stop {
    #[error("The limit is {0}, so the CLI built nothing.")]
    Cli(usize),
    #[error("The limit is {0}, so the server did not change the index.")]
    Tool(usize),
}

/// What to do after [`IndexCommandError::TooFar`] (KAIROS-T-0517).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Advice {
    /// HEAD is an ancestor of `origin/<branch>`: there is nothing to
    /// rebase, and a full build is more work than the update.
    #[error(
        "The checkout is {commits} commits behind origin/{branch}. Run `git pull --ff-only`, or open a worktree at origin/{branch}. {raise}"
    )]
    Behind {
        commits: usize,
        branch: String,
        raise: Raise,
    },
    #[error(
        "Rebase on {branch}, or run `kairos index --full`. A full build takes {estimate}. {raise}"
    )]
    Rebase {
        branch: String,
        estimate: String,
        raise: Raise,
    },
}

/// The limit that lets the update start from this index.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum Raise {
    #[error("Or give `--max-changed {0}` to update from this index.")]
    Cli(usize),
    #[error("Or give `max_changed` {0} to update from this index.")]
    Tool(usize),
}

/// The checkout that the code tools answer for after a call of
/// `fetch_index` that failed (KAIROS-T-0517).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StillOpen {
    #[error("The tools still answer for the checkout {0}.")]
    Root(String),
    #[error(
        "The server has no open checkout, so the other tools refuse until `fetch_index` succeeds."
    )]
    Nothing,
}

/// The checkouts below the start folder of the server, when that folder is
/// not a checkout (#272): one line each, as [`super::mcp`] describes them.
/// The text of [`ToolError::NoCheckout`] ends with them, so the agent does
/// not have to find the paths for `fetch_index` itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Below(pub Vec<String>);

impl std::fmt::Display for Below {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            return Ok(());
        }
        write!(
            f,
            " The checkouts below the start folder of the server: {}.",
            self.0.join("; ")
        )
    }
}

impl IndexCommandError {
    /// The code of the error for `--json` (KAIROS-T-0654). A symbol that is
    /// not found, or is not one symbol, has the code of the code tools.
    pub fn code(&self) -> &str {
        match self {
            IndexCommandError::NoIndex(_) => "NO_INDEX",
            IndexCommandError::NoStart { .. } => "NO_START",
            IndexCommandError::TooFar(_) => "TOO_FAR",
            IndexCommandError::NoRoot(_) => "NO_ROOT",
            IndexCommandError::Symbol(e) => e.code(),
            IndexCommandError::Git { .. }
            | IndexCommandError::Write { .. }
            | IndexCommandError::Index(_)
            | IndexCommandError::Server(_) => crate::error::CODE_CLI_ERROR,
        }
    }

    /// The details of the error for `--json`, else null.
    pub fn details(&self) -> Value {
        match self {
            IndexCommandError::NoIndex(path) | IndexCommandError::NoRoot(path) => {
                json!({ "path": path })
            }
            IndexCommandError::NoStart { path, why } => json!({ "path": path, "why": why }),
            IndexCommandError::TooFar(far) => far.details(),
            IndexCommandError::Symbol(e) => e.details().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }
}

impl TooFar {
    /// The counts of the refusal: the changed files, the limit, the commit
    /// of the base, and whether the count is that of the local index.
    pub fn details(&self) -> Value {
        let (changed, commit, local) = match &self.since {
            Since::Base { changed, commit }
            | Since::Below {
                changed, commit, ..
            } => (changed, commit, false),
            Since::Local { changed, commit } => (changed, commit, true),
        };
        let (Stop::Cli(limit) | Stop::Tool(limit)) = self.stop;
        let (branch, behind) = match &self.advice {
            Advice::Behind {
                commits, branch, ..
            } => (branch, Some(*commits)),
            Advice::Rebase { branch, .. } => (branch, None),
        };
        json!({
            "changed": changed,
            "limit": limit,
            "commit": commit,
            "local": local,
            "branch": branch,
            "behind": behind,
        })
    }
}

impl From<IndexCommandError> for CliError {
    /// The error keeps its text, and exits 1. With `--json`, the envelope
    /// has the code and the details of the error (KAIROS-T-0654).
    fn from(e: IndexCommandError) -> Self {
        CliError::Coded {
            auth: false,
            code: e.code().to_string(),
            text: e.to_string(),
            details: e.details(),
        }
    }
}

/// An error of a code tool. The agent gets it as a tool error with the text
/// `CODE: message`, and a `details:` line when it has details, as from each
/// tool of the Kairos server.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error(
        "No index is at {0}. Call `fetch_index` with the checkout as `root`, or run `kairos index build` in the checkout."
    )]
    NoIndex(PathBuf),
    #[error(
        "The server has no open checkout. Call `fetch_index` with `root`, the path of a git checkout. Give `repository`, the slug in Kairos, when the `origin` remote of the checkout is not a repository of Kairos.{0}"
    )]
    NoCheckout(Below),
    #[error("The folder {0} is not in a git checkout.")]
    NotACheckout(PathBuf),
    #[error(
        "Kairos has no index for the checkout {root}, because {why}. There is no local index either. Register the repository in Kairos, and its builder makes the first index. Or run `kairos index --full` in the checkout."
    )]
    NoBaseIndex { root: PathBuf, why: String },
    #[error("The download of the index of {commit} failed: {why}.")]
    Download { commit: String, why: String },
    #[error(transparent)]
    Command(#[from] IndexCommandError),
    #[error("No symbol has the name {name:?}{place}.")]
    NoSymbol { name: String, place: Place },
    #[error("{count} symbols have the name {name:?}. {choose}")]
    Ambiguous {
        name: String,
        count: usize,
        candidates: Vec<String>,
        choose: Choose,
    },
    #[error(
        "The call has the argument {argument:?}. This tool does not accept that argument. {known}"
    )]
    UnknownArgument {
        argument: String,
        allowed: Vec<String>,
        known: String,
    },
    #[error(
        "The call does not have the argument {argument:?}. This tool must have that argument. The arguments of this tool are: {allowed}."
    )]
    MissingArgument { argument: String, allowed: String },
    #[error("The arguments are not correct for this tool: {0}.")]
    BadArguments(String),
    #[error("The argument `limit` must be from 1 to {0}.")]
    Limit(usize),
    #[error("The argument `min_lines` must be 1 or more.")]
    MinLines,
    #[error(transparent)]
    Index(#[from] kairos_index::IndexError),
}

/// Where the agent looked for a symbol that is not there: the end of the
/// text of [`ToolError::NoSymbol`].
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Anywhere,
    File(String),
    Line(u32),
    FileAndLine(String, u32),
}

impl Place {
    pub fn of(file: Option<&str>, line: Option<u32>) -> Place {
        match (file, line) {
            (None, None) => Place::Anywhere,
            (Some(file), None) => Place::File(file.to_string()),
            (None, Some(line)) => Place::Line(line),
            (Some(file), Some(line)) => Place::FileAndLine(file.to_string(), line),
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Place::Anywhere => Ok(()),
            Place::File(file) => write!(f, " in the file {file:?}"),
            Place::Line(line) => write!(f, " at line {line}"),
            Place::FileAndLine(file, line) => write!(f, " in the file {file:?} at line {line}"),
        }
    }
}

/// The advice of [`ToolError::Ambiguous`] (KAIROS-T-0417): `file` when the
/// candidates are in different files, `line` when they are in one file.
/// The fields are the names of the arguments of the tool (`path` has
/// `from_file` and `from_line`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Choose {
    #[error(
        "Give the argument `{file}` to choose one. Or give a candidate from the list as `{symbol}`."
    )]
    File {
        symbol: &'static str,
        file: &'static str,
    },
    #[error(
        "They are in one file. Give the argument `{line}` to choose one. Or give a candidate from the list as `{symbol}`."
    )]
    Line {
        symbol: &'static str,
        line: &'static str,
    },
    #[error(
        "Give the arguments `{file}` and `{line}` to choose one. Or give a candidate from the list as `{symbol}`."
    )]
    FileAndLine {
        symbol: &'static str,
        file: &'static str,
        line: &'static str,
    },
}

/// The names of the arguments that choose one symbol in a tool.
#[derive(Debug, Clone, Copy)]
pub struct SymbolArguments {
    pub symbol: &'static str,
    pub file: &'static str,
    pub line: &'static str,
}

impl Choose {
    /// The advice for candidates in `files` different files of `count`.
    pub fn of(files: usize, count: usize, args: SymbolArguments) -> Choose {
        let SymbolArguments { symbol, file, line } = args;
        if files <= 1 {
            Choose::Line { symbol, line }
        } else if files == count {
            Choose::File { symbol, file }
        } else {
            Choose::FileAndLine { symbol, file, line }
        }
    }
}

impl ToolError {
    /// The stable code of the error, as the Kairos server gives it.
    pub fn code(&self) -> &'static str {
        match self {
            ToolError::NoIndex(_) => "NO_INDEX",
            ToolError::NoCheckout(_) => "NO_CHECKOUT",
            ToolError::NotACheckout(_) => "NOT_A_CHECKOUT",
            ToolError::NoBaseIndex { .. } => "NO_BASE_INDEX",
            ToolError::Download { .. } => "DOWNLOAD",
            ToolError::Command(_) => "INDEX",
            ToolError::NoSymbol { .. } => "NOT_FOUND",
            ToolError::Ambiguous { .. } => "AMBIGUOUS",
            ToolError::UnknownArgument { .. }
            | ToolError::MissingArgument { .. }
            | ToolError::BadArguments(_)
            | ToolError::Limit(_)
            | ToolError::MinLines => "VALIDATION",
            ToolError::Index(_) => "INDEX",
        }
    }

    pub(super) fn details(&self) -> Option<Value> {
        match self {
            ToolError::Ambiguous { candidates, .. } => Some(json!({ "candidates": candidates })),
            ToolError::UnknownArgument {
                argument, allowed, ..
            } => Some(json!({ "field": argument, "allowed": allowed })),
            ToolError::MissingArgument { argument, .. } => Some(json!({ "field": argument })),
            _ => None,
        }
    }

    /// The text of the tool error: `CODE: message`, then `details: {...}`.
    pub fn text(&self) -> String {
        self.text_with(None)
    }

    /// [`Self::text`] with `more` after the message, before the details.
    pub fn text_with(&self, more: Option<&dyn std::fmt::Display>) -> String {
        let mut text = format!("{}: {self}", self.code());
        if let Some(more) = more {
            text.push_str(&format!(" {more}"));
        }
        if let Some(details) = self.details() {
            text.push_str("\ndetails: ");
            text.push_str(&details.to_string());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn too_far(since: Since, stop: Stop, advice: Advice) -> String {
        IndexCommandError::TooFar(Box::new(TooFar {
            since,
            stop,
            advice,
        }))
        .to_string()
    }

    /// KAIROS-T-0654: with `--json`, the refusal has a code and its counts.
    #[test]
    fn the_refusal_has_a_code_and_its_counts() {
        let e = CliError::from(IndexCommandError::TooFar(Box::new(TooFar {
            since: Since::Local {
                changed: 250,
                commit: "abc123".into(),
            },
            stop: Stop::Cli(200),
            advice: Advice::Behind {
                commits: 12,
                branch: "main".into(),
                raise: Raise::Cli(250),
            },
        })));
        assert_eq!(e.exit_code(), crate::error::EXIT_FAILURE);
        let envelope = e.to_json();
        assert_eq!(envelope["error"]["code"], "TOO_FAR");
        assert!(
            envelope["error"]["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("250 files changed since the local index.")),
            "{envelope}"
        );
        assert_eq!(
            envelope["error"]["details"],
            json!({"changed": 250, "limit": 200, "commit": "abc123", "local": true,
                   "branch": "main", "behind": 12})
        );
        let e = CliError::from(IndexCommandError::NoIndex(PathBuf::from(
            "/r/.kairos/index.db",
        )));
        assert_eq!(e.code(), "NO_INDEX");
        assert_eq!(e.details()["path"], "/r/.kairos/index.db");
        let e = CliError::from(IndexCommandError::Symbol(Box::new(ToolError::NoSymbol {
            name: "pop".into(),
            place: Place::Anywhere,
        })));
        assert_eq!(e.code(), "NOT_FOUND");
        assert_eq!(e.to_string(), "No symbol has the name \"pop\".");
    }

    /// KAIROS-T-0517: a checkout that is only behind is told to pull.
    #[test]
    fn a_checkout_behind_is_told_to_pull() {
        let text = too_far(
            Since::Base {
                changed: 250,
                commit: "abc123".into(),
            },
            Stop::Cli(200),
            Advice::Behind {
                commits: 12,
                branch: "main".into(),
                raise: Raise::Cli(250),
            },
        );
        assert_eq!(
            text,
            "250 files changed since abc123, the nearest indexed commit in Kairos. The limit is \
             200, so the CLI built nothing. The checkout is 12 commits behind origin/main. Run \
             `git pull --ff-only`, or open a worktree at origin/main. Or give `--max-changed \
             250` to update from this index."
        );
        assert!(!text.contains("Rebase"));
    }

    /// KAIROS-T-0517: else the text names the distance and `--max-changed`.
    #[test]
    fn a_far_branch_is_told_to_rebase_or_to_raise_the_limit() {
        let text = too_far(
            Since::Below {
                changed: 250,
                commit: "abc123".into(),
                distance: 40,
                from: "def456".into(),
            },
            Stop::Cli(200),
            Advice::Rebase {
                branch: "main".into(),
                estimate: "about 2 min".into(),
                raise: Raise::Cli(250),
            },
        );
        assert_eq!(
            text,
            "250 files changed since abc123, the nearest indexed commit in Kairos. It is 40 \
             commits before def456. The limit is 200, so the CLI built nothing. Rebase on main, or run \
             `kairos index --full`. A full build takes about 2 min. Or give `--max-changed 250` \
             to update from this index."
        );
    }

    /// KAIROS-T-0517: the tool names `max_changed`, and the local index
    /// when it is nearer (KAIROS-T-0515).
    #[test]
    fn the_tool_names_max_changed() {
        let text = too_far(
            Since::Local {
                changed: 250,
                commit: "abc123".into(),
            },
            Stop::Tool(200),
            Advice::Rebase {
                branch: "main".into(),
                estimate: "about 2 min".into(),
                raise: Raise::Tool(250),
            },
        );
        assert!(
            text.starts_with(
                "250 files changed since the local index. It is nearer to the tree than abc123"
            ),
            "{text}"
        );
        assert!(
            text.contains("so the server did not change the index."),
            "{text}"
        );
        assert!(
            text.ends_with("Or give `max_changed` 250 to update from this index."),
            "{text}"
        );
        assert!(!text.contains("--max-changed"), "{text}");
    }

    /// KAIROS-T-0517: the error of `fetch_index` says what the tools answer
    /// for, before the details.
    #[test]
    fn a_failed_fetch_says_what_the_tools_answer_for() {
        let e = ToolError::NotACheckout(PathBuf::from("/tmp/x"));
        assert_eq!(
            e.text_with(Some(&StillOpen::Root("/work/a".into()))),
            "NOT_A_CHECKOUT: The folder /tmp/x is not in a git checkout. The tools still \
             answer for the checkout /work/a."
        );
        assert_eq!(
            e.text_with(Some(&StillOpen::Nothing)),
            "NOT_A_CHECKOUT: The folder /tmp/x is not in a git checkout. The server has no open \
             checkout, so the other tools refuse until `fetch_index` succeeds."
        );
        let e = ToolError::MissingArgument {
            argument: "root".into(),
            allowed: "root".into(),
        };
        let text = e.text_with(Some(&StillOpen::Nothing));
        assert!(
            text.contains("succeeds.\ndetails: {\"field\":\"root\"}"),
            "{text}"
        );
    }
}
