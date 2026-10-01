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
}

impl From<IndexCommandError> for CliError {
    fn from(e: IndexCommandError) -> Self {
        CliError::Failure(e.to_string())
    }
}

/// An error of a code tool. The agent gets it as a tool error with the text
/// `CODE: message`, and a `details:` line when it has details, as from each
/// tool of the Kairos server.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("No index is at {0}. Run `kairos index build` in the checkout.")]
    NoIndex(PathBuf),
    #[error("No symbol has the name {0:?}.")]
    NoSymbol(String),
    #[error("{count} symbols have the name {name:?}. Give the argument `file` to choose one.")]
    Ambiguous {
        name: String,
        count: usize,
        candidates: Vec<String>,
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

impl ToolError {
    /// The stable code of the error, as the Kairos server gives it.
    pub fn code(&self) -> &'static str {
        match self {
            ToolError::NoIndex(_) => "NO_INDEX",
            ToolError::NoSymbol(_) => "NOT_FOUND",
            ToolError::Ambiguous { .. } => "AMBIGUOUS",
            ToolError::UnknownArgument { .. }
            | ToolError::MissingArgument { .. }
            | ToolError::BadArguments(_)
            | ToolError::Limit(_)
            | ToolError::MinLines => "VALIDATION",
            ToolError::Index(_) => "INDEX",
        }
    }

    fn details(&self) -> Option<Value> {
        match self {
            ToolError::Ambiguous { candidates, .. } => Some(json!({ "candidates": candidates })),
            ToolError::UnknownArgument {
                argument, allowed, ..
            } => Some(json!({ "argument": argument, "allowed": allowed })),
            ToolError::MissingArgument { argument, .. } => Some(json!({ "argument": argument })),
            _ => None,
        }
    }

    /// The text of the tool error: `CODE: message`, then `details: {...}`.
    pub fn text(&self) -> String {
        let mut text = format!("{}: {self}", self.code());
        if let Some(details) = self.details() {
            text.push_str("\ndetails: ");
            text.push_str(&details.to_string());
        }
        text
    }
}
