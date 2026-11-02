//! The forge filter of a repository (KAIROS-T-0479, COLLIERY-I-0623).
//!
//! An org admin sets, per repository, one include and one exclude regular
//! expression for each field below. An event is admitted when, for each
//! field that its kind has, the value matches the include pattern and does
//! not match the exclude pattern. The filter decides which branches, pull
//! requests and issues Kairos records and shows.
//!
//! - branch: `branch_name`
//! - pull request: `pr_head_branch`, `pr_title`, `pr_author`
//! - issue: `issue_title`, `issue_author`
//!
//! Defaults: include `.*` (everything), exclude empty (nothing). An empty
//! exclude is NOT the empty regular expression, which matches every
//! string; it means "exclude nothing". An empty include is the same as
//! `.*`.
//!
//! Patterns use the Rust `regex` crate (linear time, no back-references),
//! are searched for anywhere in the value (anchor with `^` and `$`), and
//! are capped twice: [`MAX_PATTERN_LEN`] bytes of source, and
//! [`COMPILED_SIZE_LIMIT`] bytes once compiled. Pure: no I/O.

use regex::{Regex, RegexBuilder};

use crate::forge::{ForgeEvent, LinkKind};

/// The longest pattern source that is accepted, in bytes.
pub const MAX_PATTERN_LEN: usize = 512;

/// The largest compiled program that is accepted, in bytes. A short
/// pattern such as `\w{1000}` can compile to a large automaton; this caps
/// it.
pub const COMPILED_SIZE_LIMIT: usize = 256 * 1024;

/// The default include pattern: everything.
pub const DEFAULT_INCLUDE: &str = ".*";

/// One field that a filter reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FilterField {
    BranchName,
    PrHeadBranch,
    PrTitle,
    PrAuthor,
    IssueTitle,
    IssueAuthor,
}

impl FilterField {
    /// Every field, in the order of the docs and the API.
    pub const ALL: [FilterField; 6] = [
        FilterField::BranchName,
        FilterField::PrHeadBranch,
        FilterField::PrTitle,
        FilterField::PrAuthor,
        FilterField::IssueTitle,
        FilterField::IssueAuthor,
    ];

    /// The wire and storage name.
    pub fn as_str(self) -> &'static str {
        match self {
            FilterField::BranchName => "branch_name",
            FilterField::PrHeadBranch => "pr_head_branch",
            FilterField::PrTitle => "pr_title",
            FilterField::PrAuthor => "pr_author",
            FilterField::IssueTitle => "issue_title",
            FilterField::IssueAuthor => "issue_author",
        }
    }

    /// The field with this wire name.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == value)
    }
}

/// The two patterns of one field, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRule {
    pub include: String,
    pub exclude: String,
}

impl Default for FilterRule {
    fn default() -> Self {
        Self {
            include: DEFAULT_INCLUDE.to_string(),
            exclude: String::new(),
        }
    }
}

impl FilterRule {
    /// Whether this is the default rule (it admits everything).
    pub fn is_default(&self) -> bool {
        (self.include.is_empty() || self.include == DEFAULT_INCLUDE) && self.exclude.is_empty()
    }
}

/// Which pattern of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternPart {
    Include,
    Exclude,
}

impl PatternPart {
    pub fn as_str(self) -> &'static str {
        match self {
            PatternPart::Include => "include",
            PatternPart::Exclude => "exclude",
        }
    }
}

/// A pattern that was refused. `name()` gives the field and the part, for
/// example `issue_author.exclude`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilterError {
    #[error(
        "The pattern {name} has {len} bytes. The maximum is {max} bytes.",
        name = name(*field, *part),
        max = MAX_PATTERN_LEN
    )]
    TooLong {
        field: FilterField,
        part: PatternPart,
        len: usize,
    },
    #[error(
        "The pattern {name} is not a valid regular expression ({reason}).",
        name = name(*field, *part)
    )]
    Invalid {
        field: FilterField,
        part: PatternPart,
        reason: String,
    },
}

fn name(field: FilterField, part: PatternPart) -> String {
    format!("{}.{}", field.as_str(), part.as_str())
}

impl FilterError {
    /// `<field>.<include|exclude>`.
    pub fn name(&self) -> String {
        match self {
            FilterError::TooLong { field, part, .. } | FilterError::Invalid { field, part, .. } => {
                name(*field, *part)
            }
        }
    }
}

/// Compile one pattern with the caps. `None` for an empty exclude.
fn compile(
    field: FilterField,
    part: PatternPart,
    pattern: &str,
) -> Result<Option<Regex>, FilterError> {
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(FilterError::TooLong {
            field,
            part,
            len: pattern.len(),
        });
    }
    let source = match (part, pattern.is_empty()) {
        (PatternPart::Exclude, true) => return Ok(None),
        (PatternPart::Include, true) => DEFAULT_INCLUDE,
        _ => pattern,
    };
    RegexBuilder::new(source)
        .size_limit(COMPILED_SIZE_LIMIT)
        .dfa_size_limit(COMPILED_SIZE_LIMIT)
        .build()
        .map(Some)
        .map_err(|e| FilterError::Invalid {
            field,
            part,
            reason: e.to_string(),
        })
}

/// Check the two patterns of one field (on save).
pub fn validate(field: FilterField, rule: &FilterRule) -> Result<(), FilterError> {
    compile(field, PatternPart::Include, &rule.include)?;
    compile(field, PatternPart::Exclude, &rule.exclude)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Compiled {
    include: Regex,
    exclude: Option<Regex>,
}

impl Compiled {
    fn admits(&self, value: &str) -> bool {
        self.include.is_match(value) && !self.exclude.as_ref().is_some_and(|r| r.is_match(value))
    }
}

/// The compiled filter of one repository.
#[derive(Debug, Clone, Default)]
pub struct ForgeFilter {
    /// Only the fields that differ from the default.
    rules: Vec<(FilterField, Compiled)>,
}

impl ForgeFilter {
    /// Compile the stored rules. A field with no rule takes the default.
    pub fn new(
        rules: impl IntoIterator<Item = (FilterField, FilterRule)>,
    ) -> Result<Self, FilterError> {
        let mut compiled = Vec::new();
        for (field, rule) in rules {
            if rule.is_default() {
                continue;
            }
            let include = compile(field, PatternPart::Include, &rule.include)?
                .expect("an include always compiles to a pattern");
            let exclude = compile(field, PatternPart::Exclude, &rule.exclude)?;
            compiled.push((field, Compiled { include, exclude }));
        }
        Ok(Self { rules: compiled })
    }

    fn field_admits(&self, field: FilterField, value: &str) -> bool {
        self.rules
            .iter()
            .filter(|(f, _)| *f == field)
            .all(|(_, rule)| rule.admits(value))
    }

    /// Whether a branch with this name is admitted.
    pub fn admits_branch(&self, name: &str) -> bool {
        self.field_admits(FilterField::BranchName, name)
    }

    /// Whether a pull request is admitted.
    pub fn admits_pull_request(&self, head_branch: &str, title: &str, author: &str) -> bool {
        self.field_admits(FilterField::PrHeadBranch, head_branch)
            && self.field_admits(FilterField::PrTitle, title)
            && self.field_admits(FilterField::PrAuthor, author)
    }

    /// Whether an issue is admitted.
    pub fn admits_issue(&self, title: &str, author: &str) -> bool {
        self.field_admits(FilterField::IssueTitle, title)
            && self.field_admits(FilterField::IssueAuthor, author)
    }

    /// Whether a normalized event is admitted.
    pub fn admits(&self, event: &ForgeEvent) -> bool {
        match event.kind {
            LinkKind::Branch => self.admits_branch(&event.head_branch),
            LinkKind::PullRequest => {
                self.admits_pull_request(&event.head_branch, &event.title, &event.author)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::LinkState;
    use chrono::Utc;

    fn event(kind: LinkKind, branch: &str, title: &str, author: &str) -> ForgeEvent {
        ForgeEvent {
            kind,
            external_id: "1".into(),
            title: title.into(),
            url: String::new(),
            state: LinkState::Open,
            author: author.into(),
            forge_updated_at: Utc::now(),
            match_text: String::new(),
            head_branch: branch.into(),
            repo_full_name: "acme/app".into(),
        }
    }

    fn rule(include: &str, exclude: &str) -> FilterRule {
        FilterRule {
            include: include.into(),
            exclude: exclude.into(),
        }
    }

    #[test]
    fn defaults_admit_everything() {
        let filter = ForgeFilter::default();
        assert!(filter.admits(&event(LinkKind::Branch, "x", "x", "robin")));
        assert!(filter.admits(&event(LinkKind::PullRequest, "", "", "")));
        assert!(filter.admits_issue("Crash on start", "sam"));
        // Explicit default rules for each field: the same answer.
        let filter =
            ForgeFilter::new(FilterField::ALL.map(|f| (f, FilterRule::default()))).unwrap();
        assert!(filter.admits(&event(
            LinkKind::PullRequest,
            "main",
            "t",
            "dependabot[bot]"
        )));
        assert!(filter.admits_issue("", ""));
        // An empty include is the same as `.*`.
        let filter = ForgeFilter::new([(FilterField::PrTitle, rule("", ""))]).unwrap();
        assert!(filter.admits(&event(LinkKind::PullRequest, "b", "anything", "a")));
    }

    #[test]
    fn excluding_an_author_drops_their_prs_and_issues() {
        let filter = ForgeFilter::new([
            (FilterField::PrAuthor, rule(".*", r"^dependabot\[bot\]$")),
            (FilterField::IssueAuthor, rule(".*", r"dependabot\[bot\]")),
        ])
        .unwrap();
        assert!(!filter.admits(&event(
            LinkKind::PullRequest,
            "dependabot/cargo/serde-1.0",
            "Bump serde",
            "dependabot[bot]"
        )));
        assert!(filter.admits(&event(LinkKind::PullRequest, "fix", "Fix", "robin")));
        assert!(!filter.admits_issue("Bump", "dependabot[bot]"));
        assert!(filter.admits_issue("Crash", "sam"));
        // The PR rule does not touch branches.
        assert!(filter.admits(&event(
            LinkKind::Branch,
            "dependabot/x",
            "dependabot/x",
            "dependabot[bot]"
        )));
    }

    #[test]
    fn include_and_exclude_per_field() {
        let filter = ForgeFilter::new([
            (FilterField::BranchName, rule("^(feature|fix)/", "wip")),
            (FilterField::PrHeadBranch, rule("^feature/", "")),
            (FilterField::IssueTitle, rule("", "(?i)^question")),
        ])
        .unwrap();
        assert!(filter.admits_branch("feature/login"));
        assert!(!filter.admits_branch("feature/login-wip"));
        assert!(!filter.admits_branch("main"));
        assert!(filter.admits_pull_request("feature/a", "t", "a"));
        assert!(!filter.admits_pull_request("fix/a", "t", "a"));
        assert!(!filter.admits_issue("Question: how?", "sam"));
        assert!(filter.admits_issue("Bug: crash", "sam"));
    }

    #[test]
    fn invalid_or_oversize_patterns_are_refused_and_named() {
        let err = validate(FilterField::IssueAuthor, &rule(".*", "dependabot[bot")).unwrap_err();
        assert_eq!(err.name(), "issue_author.exclude");
        assert!(err.to_string().contains("issue_author.exclude"), "{err}");
        let long = "a".repeat(MAX_PATTERN_LEN + 1);
        let err = validate(FilterField::PrTitle, &rule(&long, "")).unwrap_err();
        assert_eq!(err.name(), "pr_title.include");
        assert!(matches!(err, FilterError::TooLong { len, .. } if len == MAX_PATTERN_LEN + 1));
        // Short source, large automaton: the compiled cap refuses it.
        let err = validate(FilterField::BranchName, &rule(r"\w{1000}\w{1000}", "")).unwrap_err();
        assert_eq!(err.name(), "branch_name.include");
        assert!(matches!(err, FilterError::Invalid { .. }), "{err}");
        // `new` refuses the same way.
        let err = ForgeFilter::new([(FilterField::PrAuthor, rule("(", ""))]).unwrap_err();
        assert_eq!(err.name(), "pr_author.include");
    }

    #[test]
    fn field_names_round_trip() {
        for field in FilterField::ALL {
            assert_eq!(FilterField::parse(field.as_str()), Some(field));
        }
        assert_eq!(FilterField::parse("issue_body"), None);
    }
}
