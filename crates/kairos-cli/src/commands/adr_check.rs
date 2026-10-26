//! `kairos adrs check` (KAIROS-T-0701): the ADR citations of a checkout.
//!
//! Code, tests and docs cite ADRs by short code (`KAIROS-A-0031`). The check
//! finds each citation in a tree, reads the state of each cited ADR once,
//! and gives a finding for each citation:
//!
//! - fail: the ADR does not exist, or it is archived;
//! - warn: the ADR is superseded (with the successor), or it is in a column
//!   that ends it, or the citation uses a retired code (with the current
//!   code);
//! - info: the ADR has amendments.
//!
//! The states come from the deployment (the item graph of each ADR, which
//! follows a retired code and shows an archived ADR marked, KAIROS-A-0020),
//! or from a snapshot file that an earlier run saved. CI gets no key of a
//! deployment: it reads a snapshot, or the check is skipped with a message.
//!
//! The exit codes stay in the table of KAIROS-A-0031: 0 when no citation
//! fails (and when the check is skipped), 1 when a citation fails, 2 for an
//! auth error, 64 for a usage error. With `--json` the report is one object
//! on stdout, also when a citation fails.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use kairos_client::types_graph::GraphResponse;
use kairos_client::{EntityKind, Error as ApiError, KairosClient};
use serde::{Deserialize, Serialize};

use crate::commands::entities::AdrsCommand;
use crate::context::{self, Common};
use crate::error::CliError;

/// The relationship of an amendment: the source amends the target
/// (KAIROS-T-0700, KAIROS-A-0069): an edge from the amending ADR to the
/// amended one. The amendments of an ADR are the sources of its inbound
/// `amends` edges. This is the one place that reads it.
pub const AMENDS: &str = "amends";

/// The relationship of a replacement: the source supersedes the target.
pub const SUPERSEDES: &str = "supersedes";

/// The version of the snapshot file.
const SNAPSHOT_VERSION: u32 = 1;

/// A file larger than this is not read: it is data, not a citing text.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// `kairos adrs`: the verbs of the family, and `check`.
#[derive(Subcommand)]
pub enum AdrsCli {
    #[command(flatten)]
    Item(Box<AdrsCommand>),
    /// Check the ADR citations of a tree: each cited ADR exists, is not
    /// archived, and is not superseded
    #[command(long_about = "Check the ADR citations of a tree.\n\n\
        The check finds each ADR short code (for example KAIROS-A-0031) in the files of PATH. \
        It skips the files that git ignores and the hidden folders. For each citation it gives \
        a finding:\n  \
        fail  the ADR does not exist, or it is archived\n  \
        warn  the ADR is superseded (the check names the successor), it is in a column that \
        ends it, or the citation uses a retired code (the check names the current code)\n  \
        info  the ADR has amendments\n\n\
        The states of the ADRs come from the deployment, or from a file of --snapshot. \
        --save-snapshot writes the states of this run to a file, for a run with no \
        deployment (CI). With no deployment and no --snapshot, the check is skipped: it says \
        so and exits 0.\n\n\
        The exit code is 0 when no citation fails, 1 when a citation fails, 2 for an error \
        of the login. With --json the report is one JSON object on stdout.")]
    Check(Box<CheckArgs>),
}

impl AdrsCli {
    /// The verbs of the family. `check` runs from [`CheckArgs::run`],
    /// because it has an exit code of its own.
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            AdrsCli::Item(command) => (*command).run().await,
            AdrsCli::Check(args) => args.run().await.map(|_| ()),
        }
    }
}

/// Arguments of `kairos adrs check`.
#[derive(Args, Debug, Default)]
pub struct CheckArgs {
    /// The files and folders to check (default: the current folder)
    pub paths: Vec<PathBuf>,
    /// Check only the codes with this prefix, for example KAIROS (more than one is allowed)
    #[arg(long = "prefix", value_name = "PREFIX")]
    pub prefixes: Vec<String>,
    /// Read the states of the ADRs from this file, not from the deployment
    #[arg(long, value_name = "FILE", conflicts_with = "save_snapshot")]
    pub snapshot: Option<PathBuf>,
    /// Write the states of the cited ADRs to this file, for a later --snapshot
    #[arg(long, value_name = "FILE")]
    pub save_snapshot: Option<PathBuf>,
    /// Flag an ADR when the files that cite it changed by this count of lines (added plus removed) since its accepted_commit
    #[arg(long, value_name = "N", default_value_t = STALE_LINES)]
    pub stale_lines: u64,
    #[command(flatten)]
    pub common: Common,
}

// ---------------------------------------------------------------------------
// The citations
// ---------------------------------------------------------------------------

/// One ADR short code in one line of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Citation {
    pub file: String,
    /// 1-based.
    pub line: usize,
    pub code: String,
}

/// The ADR short codes in `text`, with their 1-based line: an upper-case
/// prefix (a letter, then letters or digits), `-A-`, then four digits or
/// more. A letter, a digit, `_` or `-` just before the code, or a letter, a
/// digit or `_` just after it, makes it part of a longer word.
///
/// The check reads each text of a file: code, comments, strings and prose.
/// The code index (KAIROS-T-0703) reads the comments, the doc strings and
/// the snake-case names of tests (`retry_kairos_a_0065`); this scan does
/// not read the names of tests. The rule of a code is the one of the index:
/// [`kairos_index::codes_in`].
pub fn scan_text(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .flat_map(|(index, line)| {
            kairos_index::codes_in(line)
                .into_iter()
                .map(move |(_, code)| (index + 1, code))
        })
        .collect()
}

/// The prefix of a code: `KAIROS` of `KAIROS-A-0031`.
fn prefix_of(code: &str) -> &str {
    code.rsplit_once("-A-").map_or(code, |(prefix, _)| prefix)
}

/// The citations in `paths` (a file, or a folder that the walk reads with
/// the rules of git: no ignored file, no hidden folder), in the order of
/// the paths, then of the files, then of the lines. With `prefixes`, only
/// the codes with one of them.
pub fn scan_paths(paths: &[PathBuf], prefixes: &[String]) -> Result<Vec<Citation>, CliError> {
    let wanted: BTreeSet<String> = prefixes.iter().map(|p| p.to_ascii_uppercase()).collect();
    let mut files = Vec::new();
    for path in paths {
        if !path.exists() {
            return Err(CliError::Failure(format!(
                "The path {} does not exist.",
                path.display()
            )));
        }
        if path.is_file() {
            files.push(path.clone());
            continue;
        }
        let mut found = Vec::new();
        for entry in ignore::WalkBuilder::new(path).build() {
            let entry = entry.map_err(|err| {
                CliError::Failure(format!(
                    "The CLI cannot read the tree {}: {err}.",
                    path.display()
                ))
            })?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                found.push(entry.into_path());
            }
        }
        found.sort();
        files.extend(found);
    }
    let mut citations = Vec::new();
    for file in files {
        if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > MAX_FILE_BYTES) {
            continue;
        }
        // A file that is not UTF-8 is not a citing text.
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let name = display_path(&file);
        for (line, code) in scan_text(&text) {
            if wanted.is_empty() || wanted.contains(prefix_of(&code)) {
                citations.push(Citation {
                    file: name.clone(),
                    line,
                    code,
                });
            }
        }
    }
    Ok(citations)
}

fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix("./").map(str::to_string).unwrap_or(text)
}

// ---------------------------------------------------------------------------
// The states of the ADRs
// ---------------------------------------------------------------------------

/// An ADR that another ADR names: a successor or an amendment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdrRef {
    pub code: String,
    pub title: String,
}

/// The state of one cited ADR, as the check reads it. The snapshot file
/// holds a list of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdrState {
    /// The code as the citation gives it, in upper case.
    pub code: String,
    /// False when the deployment has no ADR with the code.
    pub found: bool,
    /// The code of the ADR now. It differs from `code` for a retired code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The column of the ADR (`off-board` for an ADR with no board).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// RFC 3339, when the ADR is archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The ADRs that supersede this one.
    #[serde(default)]
    pub superseded_by: Vec<AdrRef>,
    /// The ADRs that amend this one, in the order of their codes.
    #[serde(default)]
    pub amended_by: Vec<AdrRef>,
    /// The metadata `revisit_when` (KAIROS-A-0070): the condition for a new
    /// look at the ADR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisit_when: Option<String>,
    /// The metadata `revisit_on`, YYYY-MM-DD: the check flags the ADR from
    /// this day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisit_on: Option<String>,
    /// The metadata `accepted_commit`: the main commit when the ADR moved
    /// to Decided. The check compares the citing files with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_commit: Option<String>,
}

impl AdrState {
    fn missing(code: &str) -> Self {
        AdrState {
            code: code.to_string(),
            found: false,
            current_code: None,
            title: None,
            status: None,
            archived_at: None,
            superseded_by: Vec::new(),
            amended_by: Vec::new(),
            revisit_when: None,
            revisit_on: None,
            accepted_commit: None,
        }
    }

    /// Take the values of the metadata of KAIROS-A-0070 by slug. A slug that
    /// the item does not have leaves the field empty.
    pub fn with_metadata(mut self, values: &[(String, String)]) -> Self {
        let get = |slug: &str| {
            values
                .iter()
                .find(|(key, _)| key == slug)
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        self.revisit_when = get(META_REVISIT_WHEN);
        self.revisit_on = get(META_REVISIT_ON);
        self.accepted_commit = get(META_ACCEPTED_COMMIT);
        self
    }

    /// The state from the item graph of the ADR (depth 1): the focus is
    /// the node at depth 0; a successor is the source of a `supersedes`
    /// edge to it; an amendment is the source of an `amends` edge to it.
    pub fn from_graph(code: &str, graph: &GraphResponse) -> Self {
        let Some(focus) = graph.nodes.iter().find(|node| node.depth == 0) else {
            return AdrState::missing(code);
        };
        let sources = |relationship: &str| {
            let mut refs: Vec<AdrRef> = graph
                .edges
                .iter()
                .filter(|edge| edge.relationship == relationship && edge.target_id == focus.id)
                .filter_map(|edge| graph.nodes.iter().find(|node| node.id == edge.source_id))
                .map(|node| AdrRef {
                    code: node.short_code.clone(),
                    title: node.title.clone(),
                })
                .collect();
            refs.sort_by(|a, b| a.code.cmp(&b.code));
            refs.dedup();
            refs
        };
        AdrState {
            code: code.to_string(),
            found: true,
            current_code: Some(focus.short_code.clone()),
            title: Some(focus.title.clone()),
            status: Some(focus.status.clone()),
            archived_at: focus.archived_at.clone(),
            superseded_by: sources(SUPERSEDES),
            amended_by: sources(AMENDS),
            revisit_when: None,
            revisit_on: None,
            accepted_commit: None,
        }
    }
}

/// The snapshot file of `--save-snapshot` and `--snapshot`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub kairos_adr_snapshot: u32,
    pub adrs: Vec<AdrState>,
}

/// Where the states come from.
enum Source {
    Deployment(KairosClient),
    Snapshot(BTreeMap<String, AdrState>),
}

impl Source {
    fn name(&self) -> &'static str {
        match self {
            Source::Deployment(_) => "deployment",
            Source::Snapshot(_) => "snapshot",
        }
    }

    /// The state of each code, read once.
    async fn states(&self, codes: &BTreeSet<String>) -> Result<Vec<AdrState>, CliError> {
        let mut states = Vec::new();
        for code in codes {
            let state = match self {
                Source::Deployment(client) => {
                    match client.get_item_graph(EntityKind::Adr, code, Some(1)).await {
                        Ok(graph) => {
                            let state = AdrState::from_graph(code, &graph);
                            metadata_of(client, state).await?
                        }
                        Err(ApiError::NotFound { .. }) => AdrState::missing(code),
                        Err(err) => return Err(err.into()),
                    }
                }
                Source::Snapshot(map) => match map.get(code) {
                    Some(state) => state.clone(),
                    None => {
                        let mut state = AdrState::missing(code);
                        state.status = Some(NOT_IN_SNAPSHOT.to_string());
                        state
                    }
                },
            };
            states.push(state);
        }
        Ok(states)
    }
}

/// The metadata of a live ADR (KAIROS-A-0070). An archived ADR, or a 404 of
/// an older server, gives no values.
async fn metadata_of(client: &KairosClient, state: AdrState) -> Result<AdrState, CliError> {
    let Some(current) = state.current_code.clone() else {
        return Ok(state);
    };
    if state.archived_at.is_some() {
        return Ok(state);
    }
    match client.metadata(EntityKind::Adr, &current).await {
        Ok(response) => {
            let values: Vec<(String, String)> = response
                .values
                .into_iter()
                .map(|value| (value.slug, value.value))
                .collect();
            Ok(state.with_metadata(&values))
        }
        Err(ApiError::NotFound { .. }) => Ok(state),
        Err(err) => Err(err.into()),
    }
}

/// The slugs of the metadata of KAIROS-A-0070.
pub const META_REVISIT_WHEN: &str = "revisit_when";
pub const META_REVISIT_ON: &str = "revisit_on";
pub const META_ACCEPTED_COMMIT: &str = "accepted_commit";

/// The status of a code that the snapshot does not hold.
const NOT_IN_SNAPSHOT: &str = "not-in-snapshot";

fn read_snapshot(path: &Path) -> Result<BTreeMap<String, AdrState>, CliError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        CliError::Failure(format!(
            "The CLI cannot read the snapshot {}: {err}.",
            path.display()
        ))
    })?;
    let snapshot: Snapshot = serde_json::from_str(&text).map_err(|err| {
        CliError::Failure(format!(
            "The file {} is not a snapshot of `kairos adrs check`: {err}.",
            path.display()
        ))
    })?;
    if snapshot.kairos_adr_snapshot != SNAPSHOT_VERSION {
        return Err(CliError::Failure(format!(
            "The snapshot {} has the version {}. This CLI reads the version {SNAPSHOT_VERSION}. \
             Save the snapshot again.",
            path.display(),
            snapshot.kairos_adr_snapshot
        )));
    }
    Ok(snapshot
        .adrs
        .into_iter()
        .map(|state| (state.code.to_ascii_uppercase(), state))
        .collect())
}

fn write_snapshot(path: &Path, states: &[AdrState]) -> Result<(), CliError> {
    let snapshot = Snapshot {
        kairos_adr_snapshot: SNAPSHOT_VERSION,
        adrs: states.to_vec(),
    };
    let text = serde_json::to_string_pretty(&snapshot)
        .map_err(|err| CliError::Failure(format!("The CLI cannot write the JSON: {err}.")))?;
    std::fs::write(path, text + "\n").map_err(|err| {
        CliError::Failure(format!(
            "The CLI cannot write the snapshot {}: {err}.",
            path.display()
        ))
    })
}

// ---------------------------------------------------------------------------
// The findings
// ---------------------------------------------------------------------------

/// How bad a finding is. The order is the order of the severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Info,
    Warn,
    Fail,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Ok => "ok",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Fail => "fail",
        }
    }
}

/// One finding about one cited ADR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// `missing`, `not_in_snapshot`, `archived`, `superseded`,
    /// `ended_column`, `retired_code`, `amended`, `revisit_due`,
    /// `stale_code` or `commit_unknown`.
    pub reason: &'static str,
    pub message: String,
}

/// The columns that end an ADR, in lower case.
const ENDED_COLUMNS: [&str; 3] = ["superseded", "rejected", "deprecated"];

/// The findings about one ADR state. An ADR that exists, is live and is
/// not superseded, with no amendments, has none.
pub fn findings(state: &AdrState) -> Vec<Finding> {
    let code = &state.code;
    if !state.found {
        if state.status.as_deref() == Some(NOT_IN_SNAPSHOT) {
            return vec![Finding {
                severity: Severity::Fail,
                reason: "not_in_snapshot",
                message: format!(
                    "The snapshot has no state for {code}. Save the snapshot again with \
                     --save-snapshot."
                ),
            }];
        }
        return vec![Finding {
            severity: Severity::Fail,
            reason: "missing",
            message: format!("The ADR {code} does not exist."),
        }];
    }
    let mut out = Vec::new();
    let current = state.current_code.as_deref().unwrap_or(code);
    if !current.eq_ignore_ascii_case(code) {
        out.push(Finding {
            severity: Severity::Warn,
            reason: "retired_code",
            message: format!("{code} is a retired code. Cite the current code {current}."),
        });
    }
    if let Some(at) = &state.archived_at {
        out.push(Finding {
            severity: Severity::Fail,
            reason: "archived",
            message: format!("The ADR {current} is archived (since {at})."),
        });
    }
    if !state.superseded_by.is_empty() {
        let successors = codes_of(&state.superseded_by);
        out.push(Finding {
            severity: Severity::Warn,
            reason: "superseded",
            message: format!(
                "The ADR {current} is superseded by {successors}. Cite the successor, or say \
                 why the old decision applies."
            ),
        });
    } else if let Some(column) = state
        .status
        .as_deref()
        .filter(|column| ENDED_COLUMNS.contains(&column.to_ascii_lowercase().as_str()))
    {
        out.push(Finding {
            severity: Severity::Warn,
            reason: "ended_column",
            message: format!(
                "The ADR {current} is in the column {column}, and no ADR supersedes it."
            ),
        });
    }
    if !state.amended_by.is_empty() {
        out.push(Finding {
            severity: Severity::Info,
            reason: "amended",
            message: format!(
                "The ADR {current} has amendments: {}. Read them with the ADR.",
                codes_of(&state.amended_by)
            ),
        });
    }
    out
}

/// The default of `--stale-lines`.
pub const STALE_LINES: u64 = 50;

/// The answer of the diff of one file since a commit: the added and the
/// removed lines, or `None` when the checkout does not have the commit.
pub type Diff = Option<u64>;

/// The findings of KAIROS-A-0070 for each code: `revisit_due` when
/// `revisit_on` is `today` or earlier; `stale_code` when the files that cite
/// the ADR changed by `threshold` lines or more since `accepted_commit`;
/// `commit_unknown` when the checkout does not have the commit. `diff`
/// gives the lines of one file since one commit.
pub fn time_findings(
    states: &[AdrState],
    citations: &[Citation],
    threshold: u64,
    today: chrono::NaiveDate,
    mut diff: impl FnMut(&str, &str) -> Diff,
) -> BTreeMap<String, Vec<Finding>> {
    let mut out: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    for state in states.iter().filter(|s| s.found && s.archived_at.is_none()) {
        let current = state.current_code.as_deref().unwrap_or(&state.code);
        let mut found = Vec::new();
        if let Some(on) = state.revisit_on.as_deref()
            && let Ok(date) = chrono::NaiveDate::parse_from_str(on, "%Y-%m-%d")
            && date <= today
        {
            let when = state
                .revisit_when
                .as_deref()
                .map(|w| format!(" Revisit when: {w}"))
                .unwrap_or_default();
            found.push(Finding {
                severity: Severity::Warn,
                reason: "revisit_due",
                message: format!("The revisit date {on} of the ADR {current} has passed.{when}"),
            });
        }
        if let Some(commit) = state.accepted_commit.as_deref() {
            let files: BTreeSet<&str> = citations
                .iter()
                .filter(|c| c.code == state.code)
                .map(|c| c.file.as_str())
                .collect();
            let mut changed = Vec::new();
            let mut unknown = false;
            for file in files {
                match diff(commit, file) {
                    Some(0) => {}
                    Some(lines) => changed.push((file, lines)),
                    None => unknown = true,
                }
            }
            let total: u64 = changed.iter().map(|(_, lines)| lines).sum();
            if unknown {
                found.push(Finding {
                    severity: Severity::Info,
                    reason: "commit_unknown",
                    message: format!(
                        "The checkout does not have the commit {commit} at the acceptance of \
                         {current}. The check cannot compare the files with it."
                    ),
                });
            } else if total >= threshold {
                let list = changed
                    .iter()
                    .map(|(file, lines)| format!("{file} ({lines})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                found.push(Finding {
                    severity: Severity::Warn,
                    reason: "stale_code",
                    message: format!(
                        "The files that cite the ADR {current} changed by {total} lines since \
                         the commit {commit} at its acceptance: {list}. Read the ADR again: \
                         make sure that it still applies."
                    ),
                });
            }
        }
        if !found.is_empty() {
            out.insert(state.code.clone(), found);
        }
    }
    out
}

/// The lines of `file` that changed since `commit`, added plus removed, with
/// `git diff --numstat` in the folder of the file. `None` when git does not
/// know the commit there, or when the file is in no checkout.
pub fn git_diff(commit: &str, file: &str) -> Diff {
    let path = Path::new(file);
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path.file_name()?;
    let git = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
    };
    let object = format!("{commit}^{{commit}}");
    git(&["cat-file".as_ref(), "-e".as_ref(), object.as_ref()])?;
    let out = git(&[
        "diff".as_ref(),
        "--numstat".as_ref(),
        commit.as_ref(),
        "--".as_ref(),
        name,
    ])?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let added = parts.next()?.parse::<u64>().ok()?;
                let removed = parts.next()?.parse::<u64>().ok()?;
                Some(added + removed)
            })
            .sum(),
    )
}

fn codes_of(refs: &[AdrRef]) -> String {
    refs.iter()
        .map(|r| r.code.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One citation with its findings, for the report.
#[derive(Debug, Serialize)]
pub struct CheckedCitation {
    #[serde(flatten)]
    pub citation: Citation,
    pub severity: Severity,
    pub findings: Vec<Finding>,
}

/// The counts of the report.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub citations: usize,
    pub adrs: usize,
    pub fail: usize,
    pub warn: usize,
    pub info: usize,
}

/// The report of `kairos adrs check`, and its `--json` shape.
#[derive(Debug, Serialize)]
pub struct Report {
    /// `deployment`, `snapshot`, or `none` when the check is skipped.
    pub source: &'static str,
    pub skipped: bool,
    /// False when a citation fails.
    pub ok: bool,
    pub summary: Summary,
    /// Each citation, in the order of the files and the lines.
    pub citations: Vec<CheckedCitation>,
    /// The state of each cited ADR, in the order of the codes.
    pub adrs: Vec<AdrState>,
}

/// The report for `citations` and the states of their ADRs.
pub fn build_report(
    source: &'static str,
    citations: Vec<Citation>,
    states: Vec<AdrState>,
    mut extra: BTreeMap<String, Vec<Finding>>,
) -> Report {
    let by_code: BTreeMap<&str, Vec<Finding>> = states
        .iter()
        .map(|state| {
            let mut found = findings(state);
            found.extend(extra.remove(&state.code).unwrap_or_default());
            (state.code.as_str(), found)
        })
        .collect();
    let mut summary = Summary {
        citations: citations.len(),
        adrs: states.len(),
        ..Summary::default()
    };
    let checked: Vec<CheckedCitation> = citations
        .into_iter()
        .map(|citation| {
            let found = by_code
                .get(citation.code.as_str())
                .cloned()
                .unwrap_or_default();
            let severity = found
                .iter()
                .map(|f| f.severity)
                .max()
                .unwrap_or(Severity::Ok);
            match severity {
                Severity::Fail => summary.fail += 1,
                Severity::Warn => summary.warn += 1,
                Severity::Info => summary.info += 1,
                Severity::Ok => {}
            }
            CheckedCitation {
                citation,
                severity,
                findings: found,
            }
        })
        .collect();
    Report {
        source,
        skipped: false,
        ok: summary.fail == 0,
        summary,
        citations: checked,
        adrs: states,
    }
}

/// The text of the report: one line for each finding, then the counts.
pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    for checked in &report.citations {
        for finding in &checked.findings {
            out.push_str(&format!(
                "{}:{}: {} {}: {}\n",
                checked.citation.file,
                checked.citation.line,
                checked.citation.code,
                finding.severity.label(),
                finding.message
            ));
        }
    }
    let s = &report.summary;
    out.push_str(&format!(
        "{} citations of {} ADRs ({}): {} fail, {} warn, {} info.\n",
        s.citations, s.adrs, report.source, s.fail, s.warn, s.info
    ));
    out
}

/// The text of a skipped check.
pub const SKIPPED: &str = "The ADR check is skipped: the CLI knows no deployment, and the \
    command has no --snapshot. Log in, set KAIROS_URL, or give --snapshot FILE.";

/// The text of a check skipped on a snapshot with no ADRs (a placeholder).
pub fn empty_snapshot(path: &Path) -> String {
    format!(
        "The ADR check is skipped: the snapshot {} has no ADRs. Save it with \
         `kairos adrs check --prefix PREFIX --save-snapshot {}` from a login.",
        path.display(),
        path.display()
    )
}

impl CheckArgs {
    pub async fn run(self) -> Result<ExitCode, CliError> {
        let paths = if self.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.paths.clone()
        };
        let citations = scan_paths(&paths, &self.prefixes)?;
        let source = match &self.snapshot {
            Some(path) => {
                let states = read_snapshot(path)?;
                // A snapshot with no ADRs is a placeholder: the check is
                // skipped, not failed for each citation.
                if states.is_empty() {
                    return self.skip(&empty_snapshot(path));
                }
                Source::Snapshot(states)
            }
            None => {
                if !deployment_known(&self.common)? {
                    return self.skip(SKIPPED);
                }
                Source::Deployment(context::client(&self.common)?)
            }
        };
        let codes: BTreeSet<String> = citations.iter().map(|c| c.code.clone()).collect();
        let states = source.states(&codes).await?;
        if let Some(path) = &self.save_snapshot {
            write_snapshot(path, &states)?;
        }
        let today = chrono::Utc::now().date_naive();
        let extra = time_findings(&states, &citations, self.stale_lines, today, git_diff);
        let report = build_report(source.name(), citations, states, extra);
        if self.common.json {
            context::print_json(&report)?;
        } else {
            print!("{}", render_text(&report));
        }
        Ok(if report.ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(crate::error::EXIT_FAILURE)
        })
    }

    fn skip(&self, message: &str) -> Result<ExitCode, CliError> {
        if self.common.json {
            context::print_json(&serde_json::json!({
                "source": "none",
                "skipped": true,
                "ok": true,
                "message": message,
                "summary": Summary::default(),
                "citations": [],
                "adrs": [],
            }))?;
        } else {
            eprintln!("{message}");
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// True when a deployment is named: `--url`, `KAIROS_URL`, a key in the
/// environment, the project config, or a login in the credential cache.
/// A `config.toml` that does not parse is refused, not skipped
/// (KAIROS-T-0706): the check must not pass on a typo.
fn deployment_known(common: &Common) -> Result<bool, CliError> {
    let binding = crate::project_config::binding_here()?;
    let env = crate::auth::Env::from_process();
    if common.url.is_some() || env.url.is_some() || env.key.is_some() {
        return Ok(true);
    }
    if binding.get("deployment").is_some() {
        return Ok(true);
    }
    Ok(crate::credentials::credentials_path()
        .and_then(|path| crate::credentials::load(&path))
        // A cache that the CLI cannot read is an error of the login: the
        // client says so.
        .map_or(true, |store| !store.deployments.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_client::types_graph::{GraphEdge, GraphNode};

    #[test]
    fn the_scan_finds_the_codes_and_their_lines() {
        let text = "// KAIROS-A-0031: the exit codes.\nno code here\n\
                    see (ACME-A-0002) and WEB2-A-12345, KAIROS-A-0031.\n";
        assert_eq!(
            scan_text(text),
            vec![
                (1, "KAIROS-A-0031".to_string()),
                (3, "ACME-A-0002".to_string()),
                (3, "WEB2-A-12345".to_string()),
                (3, "KAIROS-A-0031".to_string()),
            ]
        );
    }

    #[test]
    fn the_scan_skips_what_is_no_code() {
        for text in [
            "KAIROS-T-0031",
            "kairos-a-0031",
            "KAIROS-A-031",
            "KAIROS-A-0031x",
            "xKAIROS-A-0031",
            "_KAIROS-A-0031",
            "x-KAIROS-A-0031",
            "KAIROS-A-",
            "1KAIROS-A-0031 is not a code",
        ] {
            assert_eq!(scan_text(text), Vec::new(), "{text}");
        }
        // A digit in the prefix is allowed after the first letter, and a
        // code can follow punctuation.
        assert_eq!(
            scan_text("`K8S-A-0001`"),
            vec![(1, "K8S-A-0001".to_string())]
        );
        assert_eq!(scan_text("K-A-0031"), vec![(1, "K-A-0031".to_string())]);
        assert_eq!(
            scan_text("x/KAIROS-A-0001"),
            vec![(1, "KAIROS-A-0001".to_string())]
        );
    }

    fn node(id: &str, code: &str, depth: i32, status: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            short_code: code.into(),
            entity_type: "adr".into(),
            title: format!("Title of {code}"),
            status: status.into(),
            depth,
            degree: 1,
            archived_at: None,
            done: false,
        }
    }

    fn edge(source: &str, target: &str, relationship: &str) -> GraphEdge {
        GraphEdge {
            source_id: source.into(),
            target_id: target.into(),
            relationship: relationship.into(),
            depth: 1,
        }
    }

    #[test]
    fn the_graph_gives_the_successor_and_the_amendments() {
        let graph = GraphResponse {
            focus: "ACME-A-0001".into(),
            depth: 1,
            nodes: vec![
                node("1", "ACME-A-0001", 0, "Superseded"),
                node("2", "ACME-A-0002", 1, "Decided"),
                node("3", "ACME-A-0004", 1, "Decided"),
                node("4", "ACME-A-0003", 1, "Decided"),
                node("5", "ACME-A-0009", 1, "Decided"),
            ],
            edges: vec![
                edge("2", "1", SUPERSEDES),
                edge("3", "1", AMENDS),
                edge("4", "1", AMENDS),
                // The focus supersedes another ADR: no successor of the focus.
                edge("1", "5", SUPERSEDES),
            ],
        };
        let state = AdrState::from_graph("ACME-A-0001", &graph);
        assert!(state.found);
        assert_eq!(state.status.as_deref(), Some("Superseded"));
        assert_eq!(codes_of(&state.superseded_by), "ACME-A-0002");
        assert_eq!(codes_of(&state.amended_by), "ACME-A-0003, ACME-A-0004");
    }

    fn live(code: &str) -> AdrState {
        AdrState {
            code: code.into(),
            found: true,
            current_code: Some(code.into()),
            title: Some("T".into()),
            status: Some("Decided".into()),
            archived_at: None,
            superseded_by: Vec::new(),
            amended_by: Vec::new(),
            revisit_when: None,
            revisit_on: None,
            accepted_commit: None,
        }
    }

    fn reasons(state: &AdrState) -> Vec<(Severity, &'static str)> {
        findings(state)
            .into_iter()
            .map(|f| (f.severity, f.reason))
            .collect()
    }

    #[test]
    fn each_state_gets_its_findings() {
        assert!(findings(&live("ACME-A-0001")).is_empty());
        assert_eq!(
            reasons(&AdrState::missing("ACME-A-0001")),
            vec![(Severity::Fail, "missing")]
        );
        let mut archived = live("ACME-A-0001");
        archived.archived_at = Some("2026-10-01T00:00:00Z".into());
        assert_eq!(reasons(&archived), vec![(Severity::Fail, "archived")]);

        let mut superseded = live("ACME-A-0001");
        superseded.status = Some("Superseded".into());
        superseded.superseded_by = vec![AdrRef {
            code: "ACME-A-0007".into(),
            title: "New".into(),
        }];
        let found = findings(&superseded);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].reason, "superseded");
        assert!(
            found[0].message.contains("ACME-A-0007"),
            "{}",
            found[0].message
        );

        let mut ended = live("ACME-A-0001");
        ended.status = Some("Rejected".into());
        assert_eq!(reasons(&ended), vec![(Severity::Warn, "ended_column")]);

        let mut retired = live("OLD-A-0001");
        retired.current_code = Some("ACME-A-0004".into());
        retired.amended_by = vec![AdrRef {
            code: "ACME-A-0005".into(),
            title: "Amend".into(),
        }];
        assert_eq!(
            reasons(&retired),
            vec![
                (Severity::Warn, "retired_code"),
                (Severity::Info, "amended")
            ]
        );
    }

    #[test]
    fn the_report_counts_each_citation_by_its_worst_finding() {
        let citations = vec![
            Citation {
                file: "a.rs".into(),
                line: 1,
                code: "ACME-A-0001".into(),
            },
            Citation {
                file: "a.rs".into(),
                line: 2,
                code: "ACME-A-0002".into(),
            },
            Citation {
                file: "b.md".into(),
                line: 9,
                code: "ACME-A-0002".into(),
            },
        ];
        let report = build_report(
            "snapshot",
            citations,
            vec![live("ACME-A-0001"), AdrState::missing("ACME-A-0002")],
            BTreeMap::new(),
        );
        assert!(!report.ok);
        assert_eq!(
            report.summary,
            Summary {
                citations: 3,
                adrs: 2,
                fail: 2,
                warn: 0,
                info: 0
            }
        );
        let text = render_text(&report);
        assert!(
            text.contains("b.md:9: ACME-A-0002 fail: The ADR ACME-A-0002 does not exist."),
            "{text}"
        );
        assert!(!text.contains("a.rs:1"), "{text}");
        assert!(text.ends_with("3 citations of 2 ADRs (snapshot): 2 fail, 0 warn, 0 info.\n"));
    }

    #[test]
    fn a_snapshot_round_trips_and_names_a_code_that_it_lacks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adrs.json");
        write_snapshot(&path, &[live("ACME-A-0001")]).unwrap();
        let map = read_snapshot(&path).unwrap();
        assert_eq!(map.get("ACME-A-0001"), Some(&live("ACME-A-0001")));
        let source = Source::Snapshot(map);
        let codes: BTreeSet<String> = ["ACME-A-0001".to_string(), "ACME-A-0002".to_string()].into();
        let states = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(source.states(&codes))
            .unwrap();
        assert_eq!(
            reasons(&states[1]),
            vec![(Severity::Fail, "not_in_snapshot")]
        );

        std::fs::write(&path, r#"{"kairos_adr_snapshot": 9, "adrs": []}"#).unwrap();
        let err = read_snapshot(&path).unwrap_err();
        assert!(err.to_string().contains("version 9"), "{err}");
    }

    fn cite(file: &str, code: &str) -> Citation {
        Citation {
            file: file.into(),
            line: 1,
            code: code.into(),
        }
    }

    #[test]
    fn the_metadata_of_kairos_a_0070_fills_the_state() {
        let state = live("ACME-A-0001").with_metadata(&[
            ("priority".into(), "high".into()),
            ("revisit_when".into(), " a second forge ".into()),
            ("revisit_on".into(), "2026-12-01".into()),
            ("accepted_commit".into(), "".into()),
        ]);
        assert_eq!(state.revisit_when.as_deref(), Some("a second forge"));
        assert_eq!(state.revisit_on.as_deref(), Some("2026-12-01"));
        assert_eq!(state.accepted_commit, None, "an empty value is no value");
    }

    #[test]
    fn a_passed_date_and_changed_files_flag_the_adr() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let mut due = live("ACME-A-0001");
        due.revisit_on = Some("2026-10-25".into());
        due.revisit_when = Some("a second forge".into());
        let mut later = live("ACME-A-0002");
        later.revisit_on = Some("2026-10-26".into());
        let mut stale = live("ACME-A-0003");
        stale.accepted_commit = Some("abc123".into());
        let mut fresh = live("ACME-A-0004");
        fresh.accepted_commit = Some("abc123".into());
        let mut unknown = live("ACME-A-0005");
        unknown.accepted_commit = Some("fff000".into());
        let mut archived = live("ACME-A-0006");
        archived.accepted_commit = Some("abc123".into());
        archived.archived_at = Some("2026-10-01T00:00:00Z".into());
        let citations = vec![
            cite("a.rs", "ACME-A-0003"),
            cite("b.rs", "ACME-A-0003"),
            cite("b.rs", "ACME-A-0003"),
            cite("c.rs", "ACME-A-0004"),
            cite("a.rs", "ACME-A-0005"),
            cite("a.rs", "ACME-A-0006"),
        ];
        let mut asked = Vec::new();
        let found = time_findings(
            &[due, later, stale, fresh, unknown, archived],
            &citations,
            50,
            today,
            |commit, file| {
                asked.push(format!("{commit}:{file}"));
                match (commit, file) {
                    ("fff000", _) => None,
                    (_, "a.rs") => Some(30),
                    (_, "b.rs") => Some(20),
                    _ => Some(49),
                }
            },
        );
        let reasons: BTreeMap<&str, Vec<&str>> = found
            .iter()
            .map(|(code, f)| (code.as_str(), f.iter().map(|f| f.reason).collect()))
            .collect();
        assert_eq!(
            reasons,
            BTreeMap::from([
                ("ACME-A-0001", vec!["revisit_due"]),
                ("ACME-A-0003", vec!["stale_code"]),
                ("ACME-A-0005", vec!["commit_unknown"]),
            ])
        );
        let due = &found["ACME-A-0001"][0];
        assert!(
            due.message.contains("Revisit when: a second forge"),
            "{}",
            due.message
        );
        let stale = &found["ACME-A-0003"][0];
        assert_eq!(stale.severity, Severity::Warn);
        assert!(
            stale.message.contains("changed by 50 lines"),
            "{}",
            stale.message
        );
        assert!(
            stale.message.contains("a.rs (30), b.rs (20)"),
            "{}",
            stale.message
        );
        assert_eq!(found["ACME-A-0005"][0].severity, Severity::Info);
        // Each file once; no diff for an archived ADR.
        assert_eq!(
            asked,
            vec!["abc123:a.rs", "abc123:b.rs", "abc123:c.rs", "fff000:a.rs"]
        );
    }

    #[test]
    fn git_diff_counts_the_lines_since_a_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args([
                    "-c",
                    "user.name=Robin",
                    "-c",
                    "user.email=robin@example.com",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
        git(&["add", "a.rs"]);
        git(&["commit", "-q", "-m", "one"]);
        let commit = git(&["rev-parse", "HEAD"]);
        std::fs::write(root.join("a.rs"), "one\nthree\nfour\n").unwrap();
        let file = root.join("a.rs");
        let file = file.to_str().unwrap();
        assert_eq!(git_diff(&commit, file), Some(3));
        assert_eq!(git_diff(&commit[..8], file), Some(3), "a short hash");
        assert_eq!(
            git_diff("0123456789abcdef0123456789abcdef01234567", file),
            None
        );
        let outside = tempfile::tempdir().unwrap();
        let loose = outside.path().join("b.rs");
        std::fs::write(&loose, "x").unwrap();
        assert_eq!(git_diff(&commit, loose.to_str().unwrap()), None);
    }

    #[test]
    fn the_walk_skips_ignored_files_and_filters_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/x.rs"), "ACME-A-0009").unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join(".hidden/x.md"), "ACME-A-0008").unwrap();
        std::fs::write(root.join("a.rs"), "// ACME-A-0001\n// WEB-A-0002\n").unwrap();
        let all = scan_paths(&[root.to_path_buf()], &[]).unwrap();
        let codes: Vec<&str> = all.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, vec!["ACME-A-0001", "WEB-A-0002"]);
        let acme = scan_paths(&[root.to_path_buf()], &["acme".into()]).unwrap();
        assert_eq!(acme.len(), 1);
        assert_eq!(acme[0].line, 1);
        let err = scan_paths(&[root.join("nope")], &[]).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }
}
