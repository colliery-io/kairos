//! The file rules: which files the index parses (COLLIERY-I-0264, "Which
//! files"). Rules on the path, the extension, the size, the longest line and
//! the header decide; the Laya trial of 2026-10-01 showed that a model does
//! this worse. The defaults are in `default-rules.toml`; a repository adds
//! rules in `.kairos/index-rules.toml`, which run first.

use std::fmt;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::IndexError;

/// Where a repository keeps its rules, from its root.
pub const REPOSITORY_RULES: &str = ".kairos/index-rules.toml";

/// How many lines the `header` condition reads.
pub const HEADER_LINES: usize = 10;

const DEFAULT_RULES: &str = include_str!("default-rules.toml");

/// The decision for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Hand-written code: parsed, and summarized later.
    Source,
    /// Test code: parsed for the structure, never summarized.
    Test,
    Vendored,
    Generated,
    Fixture,
    Docs,
    /// Left out by a rule that names no other kind (for example, too large).
    Excluded,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Source => "source",
            Decision::Test => "test",
            Decision::Vendored => "vendored",
            Decision::Generated => "generated",
            Decision::Fixture => "fixture",
            Decision::Docs => "docs",
            Decision::Excluded => "excluded",
        }
    }

    /// Whether a file with this decision is parsed for symbols.
    pub fn is_parsed(self) -> bool {
        matches!(self, Decision::Source | Decision::Test)
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a rule comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Default,
    Repository,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Repository => "repository",
        }
    }
}

/// What the rules know about a file.
#[derive(Debug, Clone)]
pub struct FileFacts<'a> {
    /// The path from the root, with `/` between the parts.
    pub path: &'a str,
    pub size: u64,
    /// In characters.
    pub longest_line: u64,
    /// The first line, in lower case.
    pub first_line: &'a str,
    /// The first [`HEADER_LINES`] lines, in lower case.
    pub header: &'a str,
}

/// The decision for a file and the rule that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub decision: Decision,
    pub rule: String,
    pub origin: Origin,
}

/// The id of the decision when no rule matches.
pub const FALLBACK_RULE: &str = "source";

// --- The file format ------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    #[serde(default)]
    rule: Vec<RuleSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleSpec {
    id: String,
    decision: Decision,
    path: Option<OneOrMany>,
    file_name: Option<OneOrMany>,
    extension: Option<OneOrMany>,
    not_extension: Option<OneOrMany>,
    size_over: Option<u64>,
    longest_line_over: Option<u64>,
    first_line: Option<OneOrMany>,
    header: Option<OneOrMany>,
}

// --- Compiled rules --------------------------------------------------------------

#[derive(Debug)]
struct Rule {
    id: String,
    decision: Decision,
    origin: Origin,
    path: Option<GlobSet>,
    file_name: Option<GlobSet>,
    extension: Option<Vec<String>>,
    not_extension: Option<Vec<String>>,
    size_over: Option<u64>,
    longest_line_over: Option<u64>,
    first_line: Option<Vec<String>>,
    header: Option<Vec<String>>,
}

impl Rule {
    fn matches(&self, facts: &FileFacts<'_>) -> bool {
        let name = facts.path.rsplit('/').next().unwrap_or(facts.path);
        let extension = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        let any_in = |list: &Option<Vec<String>>, text: &str| {
            list.as_ref()
                .is_none_or(|items| items.iter().any(|item| text.contains(item.as_str())))
        };
        self.path.as_ref().is_none_or(|g| g.is_match(facts.path))
            && self.file_name.as_ref().is_none_or(|g| g.is_match(name))
            && self
                .extension
                .as_ref()
                .is_none_or(|list| list.contains(&extension))
            && self
                .not_extension
                .as_ref()
                .is_none_or(|list| !list.contains(&extension))
            && self.size_over.is_none_or(|limit| facts.size > limit)
            && self
                .longest_line_over
                .is_none_or(|limit| facts.longest_line > limit)
            && any_in(&self.first_line, facts.first_line)
            && any_in(&self.header, facts.header)
    }
}

/// The rules for one repository: its own rules, then the defaults.
#[derive(Debug)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    /// The default rules only.
    pub fn defaults() -> Result<Self, IndexError> {
        let rules = parse(DEFAULT_RULES, Origin::Default, Path::new("<default rules>"))?;
        Ok(RuleSet { rules })
    }

    /// The rules of the repository at `root`: `.kairos/index-rules.toml` if
    /// it is there, then the defaults.
    pub fn for_repository(root: &Path) -> Result<Self, IndexError> {
        let path = root.join(REPOSITORY_RULES);
        let mut rules = match std::fs::read_to_string(&path) {
            Ok(text) => parse(&text, Origin::Repository, &path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(IndexError::Io { path, source }),
        };
        rules.extend(Self::defaults()?.rules);
        Ok(RuleSet { rules })
    }

    /// Parse rules in the format of `.kairos/index-rules.toml`, then add the
    /// defaults. `path` names the text in errors.
    pub fn from_repository_text(text: &str, path: &Path) -> Result<Self, IndexError> {
        let mut rules = parse(text, Origin::Repository, path)?;
        rules.extend(Self::defaults()?.rules);
        Ok(RuleSet { rules })
    }

    /// The decision for a file: the first rule that matches.
    pub fn decide(&self, facts: &FileFacts<'_>) -> Verdict {
        self.rules
            .iter()
            .find(|rule| rule.matches(facts))
            .map(|rule| Verdict {
                decision: rule.decision,
                rule: rule.id.clone(),
                origin: rule.origin,
            })
            .unwrap_or_else(|| Verdict {
                decision: Decision::Source,
                rule: FALLBACK_RULE.to_string(),
                origin: Origin::Default,
            })
    }
}

fn parse(text: &str, origin: Origin, path: &Path) -> Result<Vec<Rule>, IndexError> {
    let invalid = |message: String| IndexError::Rules {
        path: PathBuf::from(path),
        message,
    };
    let file: RulesFile = toml::from_str(text).map_err(|e| invalid(e.message().to_string()))?;
    let mut rules = Vec::with_capacity(file.rule.len());
    for spec in file.rule {
        let id = spec.id;
        if id.trim().is_empty() {
            return Err(invalid("a rule has an empty id".to_string()));
        }
        let globs = |patterns: Option<OneOrMany>| -> Result<Option<GlobSet>, IndexError> {
            let Some(patterns) = patterns else {
                return Ok(None);
            };
            let mut set = GlobSetBuilder::new();
            for pattern in patterns.into_vec() {
                let glob = GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| {
                        invalid(format!("rule {id}: bad pattern {pattern}: {}", e.kind()))
                    })?;
                set.add(glob);
            }
            set.build()
                .map(Some)
                .map_err(|e| invalid(format!("rule {id}: {e}")))
        };
        let lower = |items: Option<OneOrMany>| {
            items.map(|items| {
                items
                    .into_vec()
                    .into_iter()
                    .map(|s| s.trim_start_matches('.').to_lowercase())
                    .collect::<Vec<_>>()
            })
        };
        let text_lower = |items: Option<OneOrMany>| {
            items.map(|items| {
                items
                    .into_vec()
                    .into_iter()
                    .map(|s| s.to_lowercase())
                    .collect::<Vec<_>>()
            })
        };
        let rule = Rule {
            decision: spec.decision,
            origin,
            path: globs(spec.path)?,
            file_name: globs(spec.file_name)?,
            extension: lower(spec.extension),
            not_extension: lower(spec.not_extension),
            size_over: spec.size_over,
            longest_line_over: spec.longest_line_over,
            first_line: text_lower(spec.first_line),
            header: text_lower(spec.header),
            id,
        };
        let has_condition = rule.path.is_some()
            || rule.file_name.is_some()
            || rule.extension.is_some()
            || rule.not_extension.is_some()
            || rule.size_over.is_some()
            || rule.longest_line_over.is_some()
            || rule.first_line.is_some()
            || rule.header.is_some();
        if !has_condition {
            return Err(invalid(format!("rule {} has no condition", rule.id)));
        }
        rules.push(rule);
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct File {
        path: &'static str,
        size: u64,
        longest_line: u64,
        head: &'static str,
    }

    fn file(path: &'static str) -> File {
        File {
            path,
            size: 2_000,
            longest_line: 80,
            head: "",
        }
    }

    fn decide(rules: &RuleSet, f: &File) -> (Decision, String) {
        let header = f
            .head
            .lines()
            .take(HEADER_LINES)
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        let first_line = header.lines().next().unwrap_or("").to_string();
        let v = rules.decide(&FileFacts {
            path: f.path,
            size: f.size,
            longest_line: f.longest_line,
            first_line: &first_line,
            header: &header,
        });
        (v.decision, v.rule)
    }

    fn check(f: File, decision: Decision, rule: &str) {
        let rules = RuleSet::defaults().expect("the default rules parse");
        assert_eq!(
            decide(&rules, &f),
            (decision, rule.to_string()),
            "{}",
            f.path
        );
    }

    #[test]
    fn the_defaults_parse() {
        RuleSet::defaults().expect("the default rules parse");
    }

    // The 4 misses of the Laya trial's Python rules.

    #[test]
    fn a_long_line_in_rust_does_not_make_the_file_vendored() {
        let f = File {
            longest_line: 1_400,
            ..file("crates/kairos-server/src/mcp/tools.rs")
        };
        check(f, Decision::Source, FALLBACK_RULE);
    }

    #[test]
    fn a_tool_task_file_is_source() {
        check(
            file(".angreal/task_test.py"),
            Decision::Source,
            "tool-files",
        );
    }

    #[test]
    fn a_tool_written_baseline_is_generated() {
        check(
            file("scripts/ste-baseline.json"),
            Decision::Generated,
            "snapshot-data",
        );
    }

    #[test]
    fn a_json_corpus_with_long_lines_is_a_fixture() {
        let f = File {
            longest_line: 5_000,
            ..file("crates/kairos-server/tests/fixtures/related_work_corpus.json")
        };
        check(f, Decision::Fixture, "fixture-dir");
    }

    // Found on Kairos itself: the extension decides before the line length.

    #[test]
    fn markdown_or_an_image_with_long_lines_is_docs() {
        for path in [
            ".metis/adrs/KAIROS-A-0012.md",
            "docs/src/images/boards-overview.png",
        ] {
            let f = File {
                longest_line: 3_000,
                ..file(path)
            };
            check(f, Decision::Docs, "docs-file");
        }
    }

    // A sample of the trial's other files.

    #[test]
    fn the_trial_cases_keep_their_labels() {
        use Decision::*;
        let cases = [
            ("crates/kairos-core/src/slug.rs", Source),
            ("scripts/render-openapi.py", Source),
            ("crates/kairos-web/app.css", Source),
            ("crates/kairos-server/tests/common/mod.rs", Test),
            ("e2e/tests/theme.spec.ts", Test),
            ("uat/fixtures/team.ts", Test),
            ("scripts/metis_import/test_remap.py", Test),
            ("plugin/hooks/test_session_start.py", Test),
            ("go/greet/greet_test.go", Test),
            ("docs/theme/mermaid.min.js", Vendored),
            ("vendor/weir-tiberius/src/tds.rs", Vendored),
            (
                "docs/themes/hugo-geekdoc/layouts/partials/menu.html",
                Vendored,
            ),
            (
                "docs/themes/hugo-geekdoc/static/js/katex-13a419d8.bundle.min.js",
                Vendored,
            ),
            ("docs/book/theme/mermaid-init-52696e91.js", Generated),
            ("assets/app-0a1b2c3d.css", Generated),
            ("uat/package-lock.json", Generated),
            ("Cargo.lock", Generated),
            ("docs/book/searchindex-8f681a0f.js", Generated),
            ("docs/book/print.html", Generated),
            (
                "crates/kairos-core/tests/fixtures/forge/github_ping.json",
                Fixture,
            ),
            ("test-fixtures/mssql/seed.sql", Fixture),
            ("bench/fixtures/deathmatch.json", Fixture),
            ("docs/src/SUMMARY.md", Docs),
            ("docs/book.toml", Docs),
            ("docs/theme/mermaid-init.js", Docs),
            ("README.md", Docs),
            ("flight-levels-system-flow.svg", Docs),
        ];
        let rules = RuleSet::defaults().unwrap();
        for (path, want) in cases {
            assert_eq!(decide(&rules, &file(path)).0, want, "{path}");
        }
    }

    #[test]
    fn a_generated_header_wins_over_the_path() {
        let f = File {
            head: "// Code generated by openapi-gen. DO NOT EDIT.\n\nexport function f() {}",
            ..file("web/src/api.ts")
        };
        check(f, Decision::Generated, "generated-header");
    }

    #[test]
    fn a_large_file_is_excluded() {
        let f = File {
            size: 2 * 1024 * 1024,
            ..file("src/big.rs")
        };
        check(f, Decision::Excluded, "too-large");
    }

    #[test]
    fn a_repository_rule_runs_before_the_defaults() {
        let text =
            "[[rule]]\nid = \"no-scripts\"\npath = \"scripts/**\"\ndecision = \"excluded\"\n";
        let rules = RuleSet::from_repository_text(text, Path::new(REPOSITORY_RULES)).unwrap();
        let v = rules.decide(&FileFacts {
            path: "scripts/release.py",
            size: 10,
            longest_line: 10,
            first_line: "",
            header: "",
        });
        assert_eq!(
            v,
            Verdict {
                decision: Decision::Excluded,
                rule: "no-scripts".to_string(),
                origin: Origin::Repository,
            }
        );
    }

    #[test]
    fn a_star_does_not_cross_a_slash() {
        let text = "[[rule]]\nid = \"top\"\npath = \"*.py\"\ndecision = \"excluded\"\n";
        let rules = RuleSet::from_repository_text(text, Path::new(REPOSITORY_RULES)).unwrap();
        assert_eq!(decide(&rules, &file("setup.py")).0, Decision::Excluded);
        assert_eq!(decide(&rules, &file("pkg/setup.py")).0, Decision::Source);
    }

    fn refused(text: &str) -> String {
        RuleSet::from_repository_text(text, Path::new(REPOSITORY_RULES))
            .expect_err("the rules must be refused")
            .to_string()
    }

    #[test]
    fn an_unknown_field_is_refused_and_named() {
        let message = refused("[[rule]]\nid = \"x\"\npaths = \"a/**\"\ndecision = \"excluded\"\n");
        assert!(message.contains("paths"), "{message}");
    }

    #[test]
    fn an_unknown_decision_is_refused_and_named() {
        let message = refused("[[rule]]\nid = \"x\"\npath = \"a/**\"\ndecision = \"skip\"\n");
        assert!(message.contains("skip"), "{message}");
    }

    #[test]
    fn an_unknown_table_is_refused_and_named() {
        let message = refused("[[rules]]\nid = \"x\"\n");
        assert!(message.contains("rules"), "{message}");
    }

    #[test]
    fn a_rule_with_no_condition_is_refused() {
        let message = refused("[[rule]]\nid = \"x\"\ndecision = \"excluded\"\n");
        assert!(message.contains("no condition"), "{message}");
    }

    #[test]
    fn a_bad_glob_is_refused_and_named() {
        let message = refused("[[rule]]\nid = \"x\"\npath = \"a/[b\"\ndecision = \"excluded\"\n");
        assert!(message.contains("a/[b"), "{message}");
    }
}
