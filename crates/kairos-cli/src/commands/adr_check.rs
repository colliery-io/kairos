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
/// (KAIROS-T-0700). Until the server has the edge, no graph has it, and an
/// ADR has no amendments. This is the one place that reads it.
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
/// more. A letter, a digit or `_` next to the code makes it no code.
pub fn scan_text(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut from = 0;
        while let Some(at) = line[from..].find("-A-").map(|i| i + from) {
            from = at + 3;
            let mut start = at;
            while start > 0
                && (bytes[start - 1].is_ascii_uppercase() || bytes[start - 1].is_ascii_digit())
            {
                start -= 1;
            }
            // The prefix starts with a letter and has two characters or more.
            while start < at && !bytes[start].is_ascii_uppercase() {
                start += 1;
            }
            if at - start < 2 {
                continue;
            }
            if start > 0 && is_word_byte(bytes[start - 1]) {
                continue;
            }
            let mut end = at + 3;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end - (at + 3) < 4 {
                continue;
            }
            if end < bytes.len() && is_word_byte(bytes[end]) {
                continue;
            }
            found.push((index + 1, line[start..end].to_string()));
            from = end;
        }
    }
    found
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
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
        }
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
                        Ok(graph) => AdrState::from_graph(code, &graph),
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
    /// `ended_column`, `retired_code` or `amended`.
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
) -> Report {
    let by_code: BTreeMap<&str, Vec<Finding>> = states
        .iter()
        .map(|state| (state.code.as_str(), findings(state)))
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

impl CheckArgs {
    pub async fn run(self) -> Result<ExitCode, CliError> {
        let paths = if self.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.paths.clone()
        };
        let citations = scan_paths(&paths, &self.prefixes)?;
        let source = match &self.snapshot {
            Some(path) => Source::Snapshot(read_snapshot(path)?),
            None => {
                if !deployment_known(&self.common) {
                    return self.skip();
                }
                Source::Deployment(context::client(&self.common)?)
            }
        };
        let codes: BTreeSet<String> = citations.iter().map(|c| c.code.clone()).collect();
        let states = source.states(&codes).await?;
        if let Some(path) = &self.save_snapshot {
            write_snapshot(path, &states)?;
        }
        let report = build_report(source.name(), citations, states);
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

    fn skip(&self) -> Result<ExitCode, CliError> {
        if self.common.json {
            context::print_json(&serde_json::json!({
                "source": "none",
                "skipped": true,
                "ok": true,
                "message": SKIPPED,
                "summary": Summary::default(),
                "citations": [],
                "adrs": [],
            }))?;
        } else {
            eprintln!("{SKIPPED}");
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// True when a deployment is named: `--url`, `KAIROS_URL`, a key in the
/// environment, the project config, or a login in the credential cache.
fn deployment_known(common: &Common) -> bool {
    let env = crate::auth::Env::from_process();
    if common.url.is_some() || env.url.is_some() || env.key.is_some() {
        return true;
    }
    if crate::project_config::binding_here()
        .get("deployment")
        .is_some()
    {
        return true;
    }
    crate::credentials::credentials_path()
        .and_then(|path| crate::credentials::load(&path))
        // A cache that the CLI cannot read is an error of the login: the
        // client says so.
        .map_or(true, |store| !store.deployments.is_empty())
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
            "K-A-0031",
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
