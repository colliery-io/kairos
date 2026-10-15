//! Repository DTOs (KAIROS-T-0104 / KAIROS-T-0106, decision KAIROS-A-0019):
//! where the code is. Every repository has exactly one owning team; a task
//! links to at most one repository, of any team (COLLIERY-A-0023).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The repository a task is bound to, embedded on [`super::types::Task`]
/// so cards and agents get the slug without a second read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryRef {
    /// Repository id (UUID).
    pub id: String,
    /// Tenant-unique slug — what agents and the CLI address.
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// The browser URL of the repository on the forge: an absolute `http`
    /// or `https` URL. The web UI links the repository to it.
    #[serde(default)]
    pub repo_url: String,
    /// Owning team (UUID).
    pub team_id: String,
}

/// Body of `PUT /api/tasks/{short_code}/repository` — set the repository
/// the task links to (slug or UUID), or clear it with `null` or an empty
/// string (COLLIERY-T-0231). It can be any
/// live repository, of any team. The board and the team of the task do not
/// change (COLLIERY-T-0217, COLLIERY-A-0023).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTaskRepositoryRequest {
    #[serde(default)]
    pub repository: Option<String>,
}

/// One `impacts` link of a document or of an ADR (COLLIERY-T-0269): a
/// repository that the item is about. The link gives no right on the item
/// and no right on the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Impact {
    /// Always `impacts`.
    pub relationship: String,
    /// Always `repository`. A later version can have other target kinds.
    pub target_kind: String,
    /// The repository.
    pub repository: ImpactedRepository,
    /// When the link was made, RFC 3339.
    pub created_at: String,
}

/// The repository of an [`Impact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImpactedRepository {
    /// Repository id (UUID).
    pub id: String,
    /// The slug of the repository.
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// The browser URL of the repository on the forge: an absolute `http`
    /// or `https` URL. The web UI links the repository to it.
    #[serde(default)]
    pub repo_url: String,
    /// When the repository was archived, RFC 3339. Absent while the
    /// repository is live. The link stays when the repository is archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Body of `POST /api/{entity_type}/{short_code}/impacts`
/// (COLLIERY-T-0269): the repository that the item impacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateImpactRequest {
    /// A live repository of the organization, by slug or UUID. It can be
    /// the repository of each team.
    pub repository: String,
}

/// Response of `GET /api/{entity_type}/{short_code}/impacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemImpactsResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The `impacts` links of the item, by the slug of the repository.
    pub impacts: Vec<Impact>,
}

/// Response of `DELETE /api/{entity_type}/{short_code}/impacts/{repository}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeletedImpactResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The slug of the repository that the item does not impact now.
    pub repository: String,
}

/// One document or ADR that impacts a repository (COLLIERY-T-0269), in
/// [`RepositoryDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImpactingItem {
    /// The short code of the item.
    pub short_code: String,
    pub title: String,
    /// `document` or `adr`.
    pub entity_type: String,
    /// The value of the metadata `document_type` (`vision`,
    /// `architecture`, ...). Null for an ADR, and for a document that has
    /// no document type.
    pub document_type: Option<String>,
    /// The editorial lifecycle of a document:
    /// `draft|review|published|archived`. Null for an ADR.
    pub lifecycle: Option<String>,
    /// The name of the column of an ADR. Null for a document, and for an
    /// ADR that is not on a board.
    pub column: Option<String>,
    /// When the item was archived, RFC 3339. Absent while the item is
    /// live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Query of `GET /api/repositories/{slug}` (COLLIERY-T-0269).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams,
)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDetailQuery {
    /// Include the archived documents and ADRs in `impacted_by`, each
    /// marked with `archived_at`. Default false.
    #[serde(default)]
    pub include_deleted: bool,
}

/// One repository, as returned by `/api/repositories` (KAIROS-T-0106).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Repository {
    /// Repository id (UUID).
    pub id: String,
    /// Tenant-unique slug (`^[a-z0-9][a-z0-9-]{1,62}$`).
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// Browser URL of the repository.
    pub repo_url: String,
    /// The branch that the code index builder indexes (KAIROS-T-0430):
    /// the branch set by hand, or else the default branch of the forge at
    /// the last fetch. `default_branch_source` says which.
    pub default_branch: String,
    /// Where `default_branch` comes from (KAIROS-T-0430): `forge` (the
    /// builder reads the default branch of the remote at each fetch) or
    /// `hand` (a person set it; it wins over the forge).
    #[serde(default)]
    pub default_branch_source: DefaultBranchSource,
    /// The default branch that the remote gave at the last fetch of the
    /// builder, or `None` before the first fetch (KAIROS-T-0430).
    #[serde(default)]
    pub forge_branch: Option<String>,
    /// Short "how to work here" blurb agents read before starting.
    pub description: String,
    /// The one owning team.
    pub team: RepositoryTeam,
    /// The delivery board of the owning team (UUID). It is a fact about
    /// the owner, not where tasks go: the board or the team of a task
    /// decides that (COLLIERY-T-0219, COLLIERY-A-0023). `None` only in a
    /// misconfigured tenant (team without a delivery board).
    pub delivery_board_id: Option<String>,
    /// Live tasks that link to this repository and are not in a done
    /// column, on all boards of all teams (COLLIERY-T-0219).
    pub open_tasks: i64,
    /// Whether a live webhook connection exists for it.
    pub has_webhook: bool,
    /// The status of the read token of the repository (COLLIERY-T-3105).
    /// It never has the token.
    #[serde(default)]
    pub credential: RepositoryCredential,
    /// The GitHub write-back of the repository (KAIROS-T-0386): on when the
    /// repository has a write token. It never has the token.
    #[serde(default)]
    pub write_back: RepositoryWriteBack,
    /// Whether the code index builder works on the repository
    /// (KAIROS-T-0318): `on` (the default) or `off`.
    #[serde(default)]
    pub code_index_build: CodeIndexBuild,
    /// Where the summaries of the code index of the repository are made
    /// (KAIROS-T-0340): `embedded`, `hosted`, or `organization` (the
    /// default, KAIROS-T-0358): the default of the organization decides.
    #[serde(default)]
    pub code_index_summaries: CodeIndexSummaries,
    /// Where the summaries are made now: `embedded` or `hosted`. For
    /// `organization`, the default of the organization (KAIROS-T-0358).
    #[serde(default)]
    pub code_index_summaries_resolved: CodeIndexSummaries,
    /// The breaker of the code index builder of the repository
    /// (KAIROS-T-0399). A build job that stopped its server or stalled
    /// trips it. Then the builder starts no build of the repository. A
    /// requested build resets it.
    #[serde(default)]
    pub code_index_breaker: CodeIndexBreaker,
    /// The published site of the repository (KAIROS-T-0406).
    #[serde(default)]
    pub site: crate::types_sites::RepositorySite,
    /// The forge filters of the repository (KAIROS-T-0479): the include
    /// and the exclude pattern of each field. Each field is present.
    #[serde(default)]
    pub forge_filters: ForgeFilters,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

impl Repository {
    /// Where `default_branch` comes from, for a person (KAIROS-T-0430):
    /// `set by hand`, `set by hand; the forge gives <branch>`, `from the
    /// forge`, or `from the forge; no fetch yet`.
    pub fn default_branch_summary(&self) -> String {
        match (self.default_branch_source, self.forge_branch.as_deref()) {
            (DefaultBranchSource::Hand, Some(forge)) if forge != self.default_branch => {
                format!("set by hand; the forge gives {forge}")
            }
            (DefaultBranchSource::Hand, _) => "set by hand".to_string(),
            (DefaultBranchSource::Forge, Some(_)) => "from the forge".to_string(),
            (DefaultBranchSource::Forge, None) => "from the forge; no fetch yet".to_string(),
        }
    }
}

/// The owning team, embedded on [`Repository`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryTeam {
    /// Team id (UUID).
    pub id: String,
    pub slug: String,
    pub name: String,
}

/// `GET /api/repositories/{slug}`: the repository plus its webhook
/// connection (id only — secrets are never re-read) and in-flight links.
///
/// Until COLLIERY-T-0219 this also carried `stale_tasks`: linked tasks on a
/// board of a team that does not own the repository. COLLIERY-A-0023 makes
/// that normal work, so the field is removed. This is a wire change: a
/// client that reads the field finds it absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryDetail {
    #[serde(flatten)]
    pub repository: Repository,
    /// The live forge connection's id, if any.
    pub connection_id: Option<String>,
    /// In-flight (`open`, `draft`) branches and pull requests on this
    /// repository, newest first, each with the work item it belongs to.
    pub in_flight: Vec<crate::types_forge::TeamLink>,
    /// Every in-flight (`open`, `draft`) branch and pull request that the
    /// forge filter admits, linked to an item or not (`linked`), newest
    /// first (KAIROS-T-0480).
    #[serde(default)]
    pub refs: Vec<crate::types_forge::ForgeRef>,
    /// The documents and the ADRs that impact this repository
    /// (COLLIERY-T-0269): the documents first, by short code. Live items
    /// only, unless the request has `include_deleted=true`. Read them
    /// before you work in the repository.
    #[serde(default)]
    pub impacted_by: Vec<ImpactingItem>,
}

/// Body of `POST /api/repositories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRepositoryRequest {
    /// Tenant-unique slug; defaults to one derived from `repo_full_name`
    /// (`acme/payments-api` → `acme-payments-api`).
    #[serde(default)]
    pub slug: Option<String>,
    /// `github|gitlab|other`.
    pub forge: String,
    /// The name on the forge — must match what the forge sends in webhook
    /// payloads. `github`: 2 parts (`owner/repo`). `gitlab`: 2 or more
    /// parts (`group/subgroup/project`). `other`: 1 or more parts. No
    /// space, no `.git` at the end, 255 characters at most.
    pub repo_full_name: String,
    /// Browser URL of the repository: an absolute `http` or `https` URL
    /// with a host, no space, and no user name or password (each member
    /// can read it). 2048 characters at most.
    pub repo_url: String,
    /// A branch name that git accepts, 255 characters at most. With a
    /// value, the branch is set by hand, and the code index builder indexes
    /// it. With no value, the builder follows the default branch of the
    /// forge, read at each fetch; until the first fetch the value is `main`
    /// (KAIROS-T-0430).
    #[serde(default)]
    pub default_branch: Option<String>,
    /// The owning team (UUID or slug). Exactly one (KAIROS-A-0019).
    pub team: String,
    /// Short "how to work here" blurb for agents.
    #[serde(default)]
    pub description: Option<String>,
}

/// Body of `PATCH /api/repositories/{slug}` — every field optional;
/// `team` re-homes the repository. The tasks that link to it do not
/// change: each stays on its board and keeps its link (COLLIERY-T-0219,
/// COLLIERY-A-0023). A field with the value of the repository changes
/// nothing. The server does not examine the form of that value
/// (COLLIERY-T-0267).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// The form is that of `repo_url` in `CreateRepositoryRequest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// The form is that of `default_branch` in `CreateRepositoryRequest`.
    /// A value sets the branch by hand (KAIROS-T-0430): the builder indexes
    /// it, whatever the forge says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// `forge` clears the branch set by hand: the builder follows the
    /// default branch of the forge again, and `default_branch` gets the
    /// forge branch of the last fetch. `hand` keeps the current
    /// `default_branch` by hand. `forge` with `default_branch` is refused
    /// (KAIROS-T-0430).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch_source: Option<DefaultBranchSource>,
    /// New owning team (UUID or slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `on` or `off` (KAIROS-T-0318). Another value is refused, and the
    /// error names it. `off` cancels the build jobs of the repository that
    /// wait for a server; a build that runs now gets no further try
    /// (KAIROS-T-0398).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_index_build: Option<CodeIndexBuild>,
    /// `embedded`, `hosted` (KAIROS-T-0340) or `organization`
    /// (KAIROS-T-0358). `hosted` is refused when the tenant has no hosted
    /// provider (`CODE_INDEX_NO_HOSTED_PROVIDER`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_index_summaries: Option<CodeIndexSummaries>,
    /// The site switch (KAIROS-T-0406). Off: a publish is refused, and the
    /// site is not served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_enabled: Option<bool>,
    /// The forge filters (KAIROS-T-0479). Org admin only. Send only the
    /// fields and the patterns to change; the others stay. The server
    /// refuses an invalid or too long pattern, and the error names it
    /// (`forge_filters.<field>.<include|exclude>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_filters: Option<ForgeFilters>,
}

/// The counts of an import of the open issues (KAIROS-T-0483). The job
/// writes them while it runs, and they stay after it ends (KAIROS-T-0487).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IssueImportResult {
    /// Issues that became a task.
    pub imported: usize,
    /// Issues that were taken before (by a webhook or an earlier import).
    pub skipped: usize,
    /// Issues that the forge filter does not admit.
    pub filtered: usize,
    /// Issues that no task could be made for; `message` says why.
    pub not_taken: usize,
    /// The short codes of the new tasks.
    pub items: Vec<String>,
    /// A sentence for a person.
    pub message: String,
}

/// An import of the open issues of a repository (KAIROS-T-0487). It is a
/// job of the job queue. `POST` and `GET
/// /api/repositories/{slug}/import-issues` give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IssueImport {
    /// The id of the job (UUID).
    pub job: String,
    /// The slug of the repository.
    pub repository: String,
    /// `queued`, `running`, `waiting` (for the rate limit of the forge),
    /// `done` or `failed`.
    pub state: String,
    /// When the import was asked for (RFC 3339).
    pub created_at: String,
    /// When the job ended (RFC 3339); `None` while it waits or runs.
    pub finished_at: Option<String>,
    /// While the job waits for the rate limit of the forge: the time it
    /// goes on (RFC 3339).
    pub waiting_until: Option<String>,
    /// The pages of issues that the job took.
    pub pages: usize,
    /// The counts so far; final when the job is `done`.
    pub result: IssueImportResult,
    /// Why the job failed, or why its last try stopped.
    pub error: Option<String>,
}

/// The two patterns of one field of the forge filter (KAIROS-T-0479). A
/// pattern is a Rust regular expression of 512 bytes or fewer. It can match
/// any part of the value: use `^` and `$` to match all of it. Kairos admits
/// an event when the value matches `include` and does not match `exclude`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ForgeFilterRule {
    /// Default `.*` (everything). An empty pattern is the same as `.*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<String>,
    /// Default empty: exclude nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
}

/// The forge filters of a repository, one rule per field (KAIROS-T-0479).
/// Kairos reads `branch_name` for a branch; `pr_head_branch`, `pr_title`
/// and `pr_author` for a pull request; `issue_title` and `issue_author` for
/// an issue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ForgeFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<ForgeFilterRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_head_branch: Option<ForgeFilterRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_title: Option<ForgeFilterRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_author: Option<ForgeFilterRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_title: Option<ForgeFilterRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_author: Option<ForgeFilterRule>,
}

impl ForgeFilters {
    /// The field names, in the order of the docs.
    pub const FIELDS: [&'static str; 6] = [
        "branch_name",
        "pr_head_branch",
        "pr_title",
        "pr_author",
        "issue_title",
        "issue_author",
    ];

    /// The rule of a field by name.
    pub fn get(&self, field: &str) -> Option<&ForgeFilterRule> {
        match field {
            "branch_name" => self.branch_name.as_ref(),
            "pr_head_branch" => self.pr_head_branch.as_ref(),
            "pr_title" => self.pr_title.as_ref(),
            "pr_author" => self.pr_author.as_ref(),
            "issue_title" => self.issue_title.as_ref(),
            "issue_author" => self.issue_author.as_ref(),
            _ => None,
        }
    }

    /// Set the rule of a field by name. An unknown name changes nothing
    /// and returns false.
    pub fn set(&mut self, field: &str, rule: ForgeFilterRule) -> bool {
        let slot = match field {
            "branch_name" => &mut self.branch_name,
            "pr_head_branch" => &mut self.pr_head_branch,
            "pr_title" => &mut self.pr_title,
            "pr_author" => &mut self.pr_author,
            "issue_title" => &mut self.issue_title,
            "issue_author" => &mut self.issue_author,
            _ => return false,
        };
        *slot = Some(rule);
        true
    }

    /// One line for a person: `default (admit everything)`, or each field
    /// that differs from the default with its patterns.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = Self::FIELDS
            .iter()
            .filter_map(|field| {
                let rule = self.get(field)?;
                let include = rule.include.as_deref().unwrap_or(".*");
                let exclude = rule.exclude.as_deref().unwrap_or("");
                let include_default = include.is_empty() || include == ".*";
                match (include_default, exclude.is_empty()) {
                    (true, true) => None,
                    (true, false) => Some(format!("{field} exclude `{exclude}`")),
                    (false, true) => Some(format!("{field} include `{include}`")),
                    (false, false) => {
                        Some(format!("{field} include `{include}` exclude `{exclude}`"))
                    }
                }
            })
            .collect();
        if parts.is_empty() {
            "default (admit everything)".to_string()
        } else {
            parts.join("; ")
        }
    }

    /// Whether no field is set.
    pub fn is_empty(&self) -> bool {
        Self::FIELDS.iter().all(|f| self.get(f).is_none())
    }
}

/// Where the default branch of a repository comes from (KAIROS-T-0430).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DefaultBranchSource {
    /// The default branch of the remote, read at each fetch of the code
    /// index builder.
    #[default]
    Forge,
    /// A person set the branch. It wins over the forge.
    Hand,
}

impl DefaultBranchSource {
    /// The value on the wire: `forge` or `hand`.
    pub fn as_str(self) -> &'static str {
        match self {
            DefaultBranchSource::Forge => "forge",
            DefaultBranchSource::Hand => "hand",
        }
    }

    /// The value of `text`. `None` for another text.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "forge" => Some(DefaultBranchSource::Forge),
            "hand" => Some(DefaultBranchSource::Hand),
            _ => None,
        }
    }
}

/// Where the summaries of the code index of a repository are made
/// (KAIROS-T-0340).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeIndexSummaries {
    /// The model in the server. The code stays on the host.
    #[default]
    Embedded,
    /// The hosted provider of the tenant (`PUT /api/org/code-index-settings`).
    /// The code of each changed symbol goes to that provider.
    Hosted,
    /// The default of the organization (`default_summaries` of
    /// `PUT /api/org/code-index-settings`, KAIROS-T-0358). Only a
    /// repository has this value.
    Organization,
}

impl CodeIndexSummaries {
    /// The value on the wire: `embedded` or `hosted`.
    pub fn as_str(self) -> &'static str {
        match self {
            CodeIndexSummaries::Embedded => "embedded",
            CodeIndexSummaries::Hosted => "hosted",
            CodeIndexSummaries::Organization => "organization",
        }
    }

    /// The value of `text`. `None` for another text.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "embedded" => Some(CodeIndexSummaries::Embedded),
            "hosted" => Some(CodeIndexSummaries::Hosted),
            "organization" => Some(CodeIndexSummaries::Organization),
            _ => None,
        }
    }
}

impl std::fmt::Display for CodeIndexSummaries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether the code index builder works on a repository (KAIROS-T-0318).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeIndexBuild {
    /// The builder makes the first index of the repository when it has
    /// none, and updates the index after each push to the default branch.
    #[default]
    On,
    /// The builder does nothing for the repository. An upload of an index
    /// still works.
    Off,
}

impl CodeIndexBuild {
    /// The value on the wire: `on` or `off`.
    pub fn as_str(self) -> &'static str {
        match self {
            CodeIndexBuild::On => "on",
            CodeIndexBuild::Off => "off",
        }
    }

    /// The value of `text`: `on` or `off`. `None` for another text.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "on" => Some(CodeIndexBuild::On),
            "off" => Some(CodeIndexBuild::Off),
            _ => None,
        }
    }
}

impl std::fmt::Display for CodeIndexBuild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The status of the read token of a repository (COLLIERY-T-3105): what
/// `GET /api/repositories/{slug}/credential` gives, and what each
/// [`Repository`] carries. Kairos keeps the token encrypted, and no read
/// gives it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryCredential {
    /// Whether the repository has a read token.
    pub set: bool,
    /// The user who set the token (UUID).
    #[serde(default)]
    pub set_by: Option<String>,
    /// The name of that user, else the email.
    #[serde(default)]
    pub set_by_name: Option<String>,
    /// When the token was set, RFC 3339.
    #[serde(default)]
    pub set_at: Option<String>,
    /// When the last access check ran, RFC 3339. Null until a check runs
    /// for this token.
    #[serde(default)]
    pub last_checked_at: Option<String>,
    /// Whether git could read the repository with the token at the last
    /// check.
    #[serde(default)]
    pub last_check_ok: Option<bool>,
    /// The error of git at the last check, with the token removed.
    #[serde(default)]
    pub last_check_error: Option<String>,
}

impl RepositoryCredential {
    /// One line about the token, for the CLI and the MCP tools: `not set`,
    /// or who set it, when, and the result of the last check.
    pub fn summary(&self) -> String {
        if !self.set {
            return "not set".to_string();
        }
        let by = self
            .set_by_name
            .as_deref()
            .or(self.set_by.as_deref())
            .unwrap_or("-");
        let at = self.set_at.as_deref().unwrap_or("-");
        let check = match (self.last_check_ok, self.last_checked_at.as_deref()) {
            (Some(true), Some(when)) => format!("; the last check at {when} passed"),
            (Some(false), Some(when)) => format!(
                "; the last check at {when} failed: {}",
                self.last_check_error.as_deref().unwrap_or("-")
            ),
            _ => "; not checked".to_string(),
        };
        format!("set by {by} at {at}{check}")
    }
}

/// The GitHub write-back of a repository (KAIROS-T-0386), as
/// [`Repository`] carries it. With a write token, Kairos writes a comment
/// and the status `kairos` on each linked pull request. Kairos keeps the
/// token encrypted, and no read gives it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryWriteBack {
    /// True: the repository has a write token, and Kairos writes to GitHub.
    pub enabled: bool,
    /// The user who set the token (UUID).
    #[serde(default)]
    pub set_by: Option<String>,
    /// The name of that user, else the email.
    #[serde(default)]
    pub set_by_name: Option<String>,
    /// When the token was set, RFC 3339.
    #[serde(default)]
    pub set_at: Option<String>,
    /// When the last write ran, RFC 3339.
    #[serde(default)]
    pub last_write_at: Option<String>,
    /// Whether the last write worked.
    #[serde(default)]
    pub last_write_ok: Option<bool>,
    /// The error of the last write, with no token in it.
    #[serde(default)]
    pub last_write_error: Option<String>,
}

impl RepositoryWriteBack {
    /// One line about the write-back, for the CLI and the MCP tools: `off
    /// (no write token)`, or on, who set the token, and the last write.
    pub fn summary(&self) -> String {
        if !self.enabled {
            return "off (no write token)".to_string();
        }
        let by = self
            .set_by_name
            .as_deref()
            .or(self.set_by.as_deref())
            .unwrap_or("-");
        let at = self.set_at.as_deref().unwrap_or("-");
        let last = match (self.last_write_ok, self.last_write_at.as_deref()) {
            (Some(true), Some(when)) => format!("; the last write at {when} worked"),
            (Some(false), Some(when)) => format!(
                "; the last write at {when} failed: {}",
                self.last_write_error.as_deref().unwrap_or("-")
            ),
            _ => "; no write yet".to_string(),
        };
        format!("on (token set by {by} at {at}{last})")
    }
}

/// The breaker of the code index builder of a repository (KAIROS-T-0399),
/// as [`Repository`] carries it. The lease of a build job can run out:
/// its server stopped, ran out of memory, or stalled. That trips the
/// breaker, and the build does not run again. Then the builder starts no
/// build of the repository: no update, no first build. A build that a person asks for
/// (`POST /api/repositories/{slug}/code-indexes/rebuild`) resets it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexBreaker {
    /// True: the breaker is tripped.
    pub tripped: bool,
    /// When the breaker tripped, RFC 3339.
    #[serde(default)]
    pub tripped_at: Option<String>,
    /// Why: the error of the build job that tripped it.
    #[serde(default)]
    pub reason: Option<String>,
}

impl CodeIndexBreaker {
    /// One line about the breaker, for the CLI and the MCP tools.
    pub fn summary(&self) -> String {
        if !self.tripped {
            return "not tripped".to_string();
        }
        format!(
            "tripped at {}: {}. The builder starts no build of the repository until a person \
             asks for a build (the MCP tool rebuild_code_index, `kairos repos reindex`, or POST \
             /api/repositories/{{slug}}/code-indexes/rebuild).",
            self.tripped_at.as_deref().unwrap_or("-"),
            self.reason.as_deref().unwrap_or("-").trim_end_matches('.')
        )
    }
}

/// Body of `PUT /api/repositories/{slug}/credential` (COLLIERY-T-3105):
/// the read token of the repository. Use a GitHub fine-grained personal
/// access token with only "Contents: read" on the one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRepositoryCredentialRequest {
    /// The token. Kairos encrypts it, and no read gives it back.
    #[schema(value_type = String, format = Password)]
    pub token: crate::types_auth::Secret,
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    #[test]
    fn the_summary_says_who_set_the_token_and_the_last_check() {
        assert_eq!(RepositoryCredential::default().summary(), "not set");
        let status = RepositoryCredential {
            set: true,
            set_by: Some("u-1".into()),
            set_by_name: Some("Ada".into()),
            set_at: Some("2026-10-03T00:00:00+00:00".into()),
            last_checked_at: Some("2026-10-03T01:00:00+00:00".into()),
            last_check_ok: Some(false),
            last_check_error: Some("git ls-remote failed".into()),
        };
        assert_eq!(
            status.summary(),
            "set by Ada at 2026-10-03T00:00:00+00:00; the last check at \
             2026-10-03T01:00:00+00:00 failed: git ls-remote failed"
        );
        let unchecked = RepositoryCredential {
            last_checked_at: None,
            last_check_ok: None,
            set_by_name: None,
            ..status
        };
        assert_eq!(
            unchecked.summary(),
            "set by u-1 at 2026-10-03T00:00:00+00:00; not checked"
        );
    }

    #[test]
    fn the_request_refuses_an_unknown_field_and_hides_the_token() {
        let err =
            serde_json::from_str::<SetRepositoryCredentialRequest>(r#"{"token":"t","user":"x"}"#)
                .unwrap_err();
        assert!(err.to_string().contains("user"), "{err}");
        let ok: SetRepositoryCredentialRequest =
            serde_json::from_str(r#"{"token":"ghp_x"}"#).unwrap();
        assert!(!format!("{ok:?}").contains("ghp_x"));
    }
}

#[cfg(test)]
mod breaker_tests {
    use super::*;

    #[test]
    fn the_breaker_summary_says_when_and_why() {
        assert_eq!(CodeIndexBreaker::default().summary(), "not tripped");
        let tripped = CodeIndexBreaker {
            tripped: true,
            tripped_at: Some("2026-10-12T00:00:00+00:00".into()),
            reason: Some("The lease ran out".into()),
        };
        let line = tripped.summary();
        assert!(line.starts_with("tripped at 2026-10-12T00:00:00+00:00: The lease ran out."));
        assert!(
            line.contains("/api/repositories/{slug}/code-indexes/rebuild"),
            "{line}"
        );
    }

    #[test]
    fn an_older_server_sends_no_breaker() {
        let breaker: CodeIndexBreaker = serde_json::from_str("{\"tripped\":false}").unwrap();
        assert_eq!(breaker, CodeIndexBreaker::default());
    }
}
