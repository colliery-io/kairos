//! The `--json` result of `kairos index` (KAIROS-T-0654): one JSON object on
//! stdout for each run.
//!
//! - `build`, `--full` and `update` give what the run did: the index file,
//!   where the update started (the base index or the local index), the
//!   counts of the structure and of the summaries, and the warnings.
//! - `status` gives the counts of the index.
//! - `duplicates` and `references` give the list envelope of KAIROS-A-0057:
//!   `items`, `total` and `next_offset`, and some keys of their own. These
//!   commands have no `--offset`, so `next_offset` is null. `total` counts
//!   each row that the index found; `items` has the first `--limit` rows.
//!
//! A refusal is not in this object: with `--json`, `main` prints the error
//! envelope on stderr, with its code and its text (KAIROS-T-0588).
//!
//! Without `--json`, [`Run`] prints each line of a build or an update when it
//! comes, as before.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kairos_index::{
    BuildReport, Counts, Coverage, DuplicateOptions, Duplicates, IndexError, LevelCount, Reference,
    SummaryReport, SymbolInfo,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::base::Base;
use super::summaries::Outcome;
use crate::context::print_json;
use crate::error::CliError;
use crate::listing::ListJson;

/// What a build or an update did. With `json` false, each part is printed
/// as text when it comes. With `json` true, the parts are kept, and
/// [`Run::finish`] prints them as one object.
pub(super) struct Run {
    json: bool,
    report: RunJson,
}

/// The `--json` object of `build`, `--full` and `update`.
#[derive(Debug, Default, Serialize)]
struct RunJson {
    /// `build` or `update`.
    command: &'static str,
    /// The index file.
    index: Option<PathBuf>,
    /// The old index of the checkout (`.kairos/index.db`), when this run
    /// moved it to the user cache (KAIROS-A-0072).
    moved_from: Option<PathBuf>,
    /// Where the update started; null for a build.
    start: Option<Start>,
    structure: Option<Value>,
    summaries: Option<Value>,
    warnings: Vec<String>,
}

/// Where an update started.
#[derive(Debug, Default, Serialize)]
pub(super) struct Start {
    /// `base`: the update downloaded the base index of Kairos. `local`: it
    /// updated the local index.
    pub from: &'static str,
    /// The base index that Kairos gave, also when the update did not use it.
    pub base: Option<Value>,
    /// The files of the tree that differ from the local index. Null when
    /// the checkout has no local index that this CLI can update.
    pub local_changed: Option<usize>,
    /// Why Kairos gave no base index.
    pub no_base: Option<String>,
    /// What the update did with the summaries of the base index, when the
    /// local index is as near to the tree (KAIROS-T-0355).
    pub summaries_taken: Option<String>,
}

impl Run {
    pub fn new(command: &'static str, json: bool) -> Self {
        Self {
            json,
            report: RunJson {
                command,
                ..RunJson::default()
            },
        }
    }

    /// A line of text. With `--json`, nothing: the object has the facts.
    pub fn say(&self, line: &str) {
        if !self.json {
            println!("{line}");
        }
    }

    /// The old index of the checkout, when this run moved it to the user
    /// cache (KAIROS-A-0072).
    pub fn moved(&mut self, old: Option<PathBuf>) {
        if let Some(old) = old {
            self.say(&format!(
                "The CLI moved the old index {} to the user cache.",
                old.display()
            ));
            self.report.moved_from = Some(old);
        }
    }

    /// The index file.
    pub fn index(&mut self, db: &Path) {
        self.say(&format!("Index: {}", db.display()));
        self.report.index = Some(db.to_path_buf());
    }

    pub fn start(&mut self, start: Start) {
        self.report.start = Some(start);
    }

    /// A warning that does not stop the run.
    pub fn warn(&mut self, warning: String) {
        self.say(&warning);
        self.report.warnings.push(warning);
    }

    /// The structure that the run built.
    pub fn structure(&mut self, report: &BuildReport, elapsed: Duration) {
        if self.json {
            self.report.structure = Some(structure(report, elapsed));
        } else {
            super::print_structure(report, elapsed);
        }
    }

    /// The summaries of the run.
    pub fn summaries(&mut self, outcome: Outcome) {
        if self.json {
            self.report.summaries = Some(summaries(&outcome));
        } else {
            super::print_summaries(outcome);
        }
    }

    /// Print the object, with `--json`.
    pub fn finish(self) -> Result<(), CliError> {
        if self.json {
            print_json(&self.report)?;
        }
        Ok(())
    }
}

/// The base index of Kairos.
pub(super) fn base(b: &Base) -> Value {
    json!({
        "repository": b.repository,
        "branch": b.branch,
        "commit": b.commit,
        "changed": b.changed,
        "summaries": b.summaries,
        "hosted": b.hosted,
        "from": b.from,
        "distance": b.distance,
        "behind": b.behind,
    })
}

/// The structure of a build or an update. Pure.
pub(super) fn structure(report: &BuildReport, elapsed: Duration) -> Value {
    let e = &report.edges;
    json!({
        "files": report.files,
        "parsed_files": report.parsed_files,
        "cached_files": report.cached_files,
        "symbols": report.symbols,
        "edges": {
            "total": e.total(),
            "certain_scip": e.certain_scip,
            "external_scip": e.external_scip,
            "certain_name": e.certain_name,
            "possible_name": e.possible_name,
            "external_name": e.external_name,
            "kept_scip": e.kept_scip,
            "pending": e.pending,
        },
        "scip": report.scip_runs.iter().map(|run| json!({
            "language": run.language,
            "documents": run.documents,
            "seconds": run.elapsed.as_secs_f64(),
        })).collect::<Vec<_>>(),
        "scip_warnings": report.scip_warnings,
        "token_vectors": {
            "made": report.token_vectors.made,
            "reused": report.token_vectors.reused,
        },
        "adr_citations": report.citations,
        "seconds": elapsed.as_secs_f64(),
    })
}

fn levels(r: &SummaryReport, count: fn(&LevelCount) -> usize) -> Value {
    json!({
        "symbols": count(&r.symbols),
        "files": count(&r.files),
        "modules": count(&r.modules),
    })
}

/// The summaries of a build or an update. Pure.
pub(super) fn summaries(outcome: &Outcome) -> Value {
    let (result, r, reason, seconds) = match outcome {
        Outcome::Linked(r) => ("linked", r, None, None::<f64>),
        Outcome::NotMade(r, why) => ("not_made", r, Some(why.as_str()), None),
        #[cfg(feature = "llama")]
        Outcome::Made(r, elapsed) => ("made", r, None, Some(elapsed.as_secs_f64())),
    };
    json!({
        "result": result,
        "linked": r.symbols.reused + r.files.reused + r.modules.reused,
        "made": levels(r, |c| c.summarized),
        "left": levels(r, |c| c.left),
        "failed": levels(r, |c| c.failed),
        "reason": reason,
        "first_failure": r.failure.as_ref().map(|f| json!({
            "name": f.name,
            "message": f.message,
        })),
        "seconds": seconds,
    })
}

/// The `--json` object of `status`. Pure.
pub(super) fn status(db: &Path, c: &Counts) -> Value {
    json!({
        "index": db,
        "files": c.files,
        "parsed_files": c.parsed_files,
        "symbols": c.symbols,
        "edges": {
            "total": c.edges,
            "certain": c.certain,
            "possible": c.possible,
            "external": c.external,
            "pending": c.pending,
        },
        "summaries": {
            "total": c.summaries(),
            "symbols": c.symbol_summaries,
            "summarizable": c.summarizable,
            "files": c.file_summaries,
            "modules": c.module_summaries,
            "pool": c.pool,
        },
        "summary_model": c.summary_model,
        "vector_model": c.vector_model,
    })
}

/// A symbol in a list. Pure.
pub(super) fn symbol(s: &SymbolInfo) -> Value {
    json!({
        "name": s.name,
        "qualified": s.qualified(),
        "kind": s.kind,
        "language": s.language,
        "file": s.file,
        "start_line": s.start_line,
        "end_line": s.end_line,
        "is_test": s.is_test,
    })
}

/// The list envelope of KAIROS-A-0057 ([`ListJson`]), for a list with no
/// `--offset`: `items` is the first `limit` rows, and `next_offset` is null.
/// The caller adds the keys of its command.
fn envelope(items: Vec<Value>, total: usize, limit: usize) -> serde_json::Map<String, Value> {
    let items: Vec<Value> = items.into_iter().take(limit).collect();
    let list = ListJson {
        items: &items,
        total: i64::try_from(total).unwrap_or(i64::MAX),
        next_offset: None,
    };
    match serde_json::to_value(&list) {
        Ok(Value::Object(out)) => out,
        _ => serde_json::Map::new(),
    }
}

/// The `--json` object of `duplicates`: the envelope of the groups, and
/// what the search compared. Pure.
pub(super) fn duplicates(found: &Duplicates, options: &DuplicateOptions, limit: usize) -> Value {
    let groups = found
        .groups
        .iter()
        .map(|g| {
            json!({
                "kind": g.kind.as_str(),
                "score": g.score,
                "symbols": g.symbols.iter().map(symbol).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut out = envelope(groups, found.groups.len(), limit);
    out.insert("compared".into(), json!(found.compared));
    out.insert("tests".into(), json!(options.include_tests));
    out.insert("min_lines".into(), json!(options.min_lines));
    out.insert("summary_vectors".into(), json!(found.summary_vectors));
    Value::Object(out)
}

fn reference(r: &Reference) -> Value {
    json!({
        "file": r.file,
        "line": r.line,
        "col": r.col,
        "kind": r.kind,
        "enclosing": r.enclosing.as_ref().map(symbol),
        "origin": r.origin,
        "class": r.class,
        "candidates": r.candidates,
        "scip_pending": r.scip_pending,
    })
}

/// The `--json` object of `references`: the envelope of the uses, the
/// symbol, what the index has for its language, and the count of the
/// possible calls that are not in the list.
pub(super) fn references(
    index: &kairos_index::Index,
    s: &SymbolInfo,
    possible: bool,
    limit: usize,
) -> Result<Value, IndexError> {
    let found = index.references(s.id, possible)?;
    let not_shown = if possible {
        0
    } else {
        index
            .references(s.id, true)?
            .uses
            .len()
            .saturating_sub(found.uses.len())
    };
    let uses = found.uses.iter().map(reference).collect();
    let mut out = envelope(uses, found.uses.len(), limit);
    out.insert("symbol".into(), symbol(s));
    out.insert("coverage".into(), coverage(found.coverage));
    out.insert("possible_not_shown".into(), json!(not_shown));
    Ok(Value::Object(out))
}

/// The `--json` object of `references` for an ADR short code
/// (KAIROS-T-0703): the envelope of the places that cite it, and the code.
pub(super) fn citations(
    index: &kairos_index::Index,
    code: &str,
    limit: usize,
) -> Result<Value, IndexError> {
    let places = index.adr_citations(code)?;
    let items = places
        .iter()
        .map(|p| {
            json!({
                "file": p.file,
                "line": p.line,
                "col": p.col,
                "context": p.context,
                "symbol": p.symbol.as_ref().map(symbol),
            })
        })
        .collect();
    let mut out = envelope(items, places.len(), limit);
    out.insert("adr".into(), json!(code));
    Ok(Value::Object(out))
}

/// What the index has for the uses of a symbol. Pure.
fn coverage(c: Coverage) -> Value {
    match c {
        Coverage::Full => json!({ "kind": "full", "pending_files": 0 }),
        Coverage::Pending(n) => json!({ "kind": "pending", "pending_files": n }),
        Coverage::CallsOnly => json!({ "kind": "calls_only", "pending_files": 0 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_index::{DuplicateGroup, DuplicateKind, EdgeStats, LanguageRun, ScipWarning};

    fn sym(name: &str, file: &str) -> SymbolInfo {
        SymbolInfo {
            id: 1,
            file: file.into(),
            name: name.into(),
            container: Some("Stack".into()),
            kind: "function".into(),
            language: "rust".into(),
            start_line: 3,
            end_line: 9,
            signature: None,
            is_test: false,
            summary: None,
        }
    }

    #[test]
    fn the_structure_has_the_counts_and_each_scip_run() {
        let report = BuildReport {
            files: 10,
            parsed_files: 8,
            cached_files: 2,
            symbols: 40,
            edges: EdgeStats {
                certain_name: 5,
                kept_scip: 3,
                pending: 1,
                ..EdgeStats::default()
            },
            scip: None,
            scip_runs: vec![LanguageRun {
                language: "python".into(),
                elapsed: Duration::from_millis(500),
                documents: 4,
            }],
            scip_warnings: vec![ScipWarning {
                language: "go".into(),
                reason: "scip-go is not on the computer".into(),
            }],
            token_vectors: Default::default(),
            refs: 0,
            citations: 3,
        };
        let v = structure(&report, Duration::from_secs(2));
        assert_eq!(v["files"], 10);
        assert_eq!(v["cached_files"], 2);
        assert_eq!(v["edges"]["total"], 8);
        assert_eq!(v["edges"]["kept_scip"], 3);
        assert_eq!(v["scip"][0]["language"], "python");
        assert_eq!(v["scip"][0]["documents"], 4);
        assert_eq!(v["scip_warnings"][0]["language"], "go");
        assert_eq!(v["adr_citations"], 3);
        assert_eq!(v["seconds"], 2.0);
    }

    #[test]
    fn the_summaries_say_why_none_were_made() {
        let mut r = SummaryReport::default();
        r.symbols.reused = 4;
        r.files.reused = 1;
        r.symbols.left = 3;
        let v = summaries(&Outcome::NotMade(r.clone(), "no model".into()));
        assert_eq!(v["result"], "not_made");
        assert_eq!(v["linked"], 5);
        assert_eq!(v["left"]["symbols"], 3);
        assert_eq!(v["reason"], "no model");
        assert_eq!(v["first_failure"], Value::Null);
        let v = summaries(&Outcome::Linked(r));
        assert_eq!(v["result"], "linked");
        assert_eq!(v["reason"], Value::Null);
    }

    #[test]
    fn duplicates_use_the_list_envelope() {
        let group = |kind| DuplicateGroup {
            kind,
            score: 1.0,
            symbols: vec![sym("pop", "src/a.rs"), sym("pop", "src/b.rs")],
        };
        let found = Duplicates {
            groups: vec![group(DuplicateKind::Exact), group(DuplicateKind::Near)],
            compared: 12,
            summary_vectors: false,
        };
        let v = duplicates(&found, &DuplicateOptions::default(), 1);
        assert_eq!(v["total"], 2);
        assert_eq!(v["next_offset"], Value::Null);
        let items = v["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "the limit applies to items: {v}");
        assert_eq!(items[0]["kind"], "exact");
        assert_eq!(items[0]["symbols"][1]["file"], "src/b.rs");
        assert_eq!(items[0]["symbols"][0]["qualified"], "Stack::pop");
        assert_eq!(v["compared"], 12);
        assert_eq!(v["summary_vectors"], false);
    }

    #[test]
    fn the_status_has_each_count() {
        let c = Counts {
            files: 3,
            symbols: 7,
            edges: 5,
            certain: 4,
            symbol_summaries: 2,
            file_summaries: 1,
            pool: 9,
            ..Counts::default()
        };
        let v = status(Path::new("/r/.kairos/index.db"), &c);
        assert_eq!(v["index"], "/r/.kairos/index.db");
        assert_eq!(v["edges"]["certain"], 4);
        assert_eq!(v["summaries"]["total"], 3);
        assert_eq!(v["summaries"]["pool"], 9);
        assert_eq!(v["summary_model"], Value::Null);
    }

    #[test]
    fn a_text_run_keeps_nothing_and_a_json_run_keeps_each_part() {
        let mut run = Run::new("update", true);
        run.moved(Some(PathBuf::from("/r/.kairos/index.db")));
        run.index(Path::new("/r/.kairos/index.db"));
        run.start(Start {
            from: "local",
            local_changed: Some(2),
            no_base: Some("no Kairos deployment is set".into()),
            ..Start::default()
        });
        run.warn("The summaries of the local index are not kept: x".into());
        let v = serde_json::to_value(&run.report).expect("json");
        assert_eq!(v["command"], "update");
        assert_eq!(v["moved_from"], "/r/.kairos/index.db");
        assert_eq!(v["index"], "/r/.kairos/index.db");
        assert_eq!(v["start"]["from"], "local");
        assert_eq!(v["start"]["local_changed"], 2);
        assert_eq!(v["start"]["base"], Value::Null);
        assert_eq!(v["warnings"].as_array().map(Vec::len), Some(1));
        assert_eq!(coverage(Coverage::Pending(3))["pending_files"], 3);
    }
}
