//! Code index DTOs (COLLIERY-T-1853, COLLIERY-I-0264 "The flow"): the base
//! index that Kairos keeps for each indexed commit of a repository.
//!
//! The index file itself goes over the wire as bytes (a SQLite file of the
//! `kairos-index` schema), not as JSON.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One indexed commit of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndex {
    /// The commit, 40 (or 64) lower-case hex characters.
    pub commit: String,
    /// The branch, the pull request or the tag that the index is for, if
    /// the sender gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    /// Each ref that points at the commit now (KAIROS-T-0750): the default
    /// branch, a tag, a pull request (`pull/12`). Empty for an index that
    /// no ref points at, or a server before 0.20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<CodeIndexRef>,
    /// `upload`, or `build` (the server built it after a push).
    pub source: String,
    /// The size of the structure (the index with no summaries), in bytes.
    pub structure_bytes: i64,
    /// The summaries that the structure uses.
    pub summary_keys: i32,
    /// The schema version of the index file (KAIROS-T-0518). It is null
    /// when the stored structure gives no version.
    #[serde(default)]
    pub schema_version: Option<i64>,
    /// The model of the vectors, as `provider/model/dimension`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_model: Option<String>,
    /// When the index was first written, RFC 3339.
    pub created_at: String,
    /// When the index was last written, RFC 3339.
    pub updated_at: String,
}

/// One ref that points at an indexed commit (KAIROS-T-0750).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexRef {
    /// The short name: `main`, `v1.0.0`, `pull/12`, `merge-requests/12`.
    pub name: String,
    /// `branch`, `tag` or `pull`.
    pub kind: String,
}

impl CodeIndexRef {
    /// The ref as a reader sees it: the name, with `(tag)` after a tag.
    /// A pull request names its kind (`pull/12`); a branch is plain.
    pub fn text(&self) -> String {
        if self.kind == "tag" {
            format!("{} (tag)", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// The answer to `PUT /api/repositories/{slug}/code-indexes/{commit}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UploadedCodeIndex {
    /// The index as Kairos keeps it now.
    pub index: CodeIndex,
    /// The summaries of the upload that the pool of the repository did not
    /// have.
    pub new_summaries: usize,
    /// The summaries in the pool of the repository after the upload.
    pub pool_size: i64,
}

/// Query of `PUT /api/repositories/{slug}/code-indexes/{commit}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct UploadCodeIndexQuery {
    /// The branch, the pull request or the tag that the index is for, for
    /// example `main`, `pull/12` or `v1.0.0`.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

/// Query of `GET /api/repositories/{slug}/code-indexes/nearest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct NearestCodeIndexQuery {
    /// The commit to start from (a full commit id).
    pub commit: String,
}

/// The answer to `GET /api/repositories/{slug}/code-indexes/nearest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NearestCodeIndex {
    /// The commit that the request gave.
    pub from: String,
    /// The nearest indexed commit at or below `from`.
    pub index: CodeIndex,
    /// The commits from the indexed commit to `from`. 0 when `from` has an
    /// index.
    pub distance: usize,
}

/// One run of the code index builder (KAIROS-T-0331). The trigger says
/// what started it: a push, a first build, a request, or an upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexBuild {
    pub id: String,
    /// The commit of the run, once the fetch gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The branch that the run indexed, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    /// `push`, `first`, `request` or `upload`.
    pub trigger: String,
    /// `running`, `ok` or `failed`.
    pub outcome: String,
    /// The text of the failure, for a `failed` run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The counts of the index that an `ok` run wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbols: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<i32>,
    /// The summaries that the run made with the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summaries_made: Option<i32>,
    /// The summaries that the run reused with no model call
    /// (KAIROS-T-0335): from the pool of the repository, or from the work
    /// file of a run that did not end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summaries_reused: Option<i32>,
    /// The summaries that the index of the run does not have
    /// (KAIROS-T-0495): the provider refused a call for the rate, or another
    /// call failed. The next build makes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summaries_missing: Option<i32>,
    /// The user who asked for the run or sent the upload. None for the
    /// builder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
    /// When the run started, RFC 3339.
    pub started_at: String,
    /// When the run ended, RFC 3339. None while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// The model that wrote the summaries, as `provider/model`
    /// (KAIROS-T-0341). None for a run that made none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Each language that had no SCIP index in the run, and why
    /// (KAIROS-T-0478): its edges are by name. Empty when each SCIP indexer
    /// of the run gave its index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scip_warnings: Vec<CodeIndexScipWarning>,
}

/// A language of a run with no SCIP index (KAIROS-T-0478).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexScipWarning {
    /// `rust`, `go`, `typescript` or `python`.
    pub language: String,
    /// What failed: a missing tool, a download, a checksum, an install or
    /// the indexer.
    pub reason: String,
}

impl CodeIndexScipWarning {
    /// One line for the CLI and the MCP tools (KAIROS-T-0668), for example
    /// `python: edges by name (uv sync failed, the project asks for Python
    /// ==3.11.*; the runner has Python 3.13.16 ...)`. A long reason is cut.
    pub fn note(&self) -> String {
        const LONGEST: usize = 240;
        let reason: String = self.reason.chars().take(LONGEST).collect();
        let cut = if self.reason.chars().count() > LONGEST {
            "…"
        } else {
            ""
        };
        format!(
            "{}: edges by name ({}{})",
            self.language,
            reason.trim_end_matches('.'),
            cut
        )
    }
}

/// The disk use of the builder of this server: its tools and its
/// dependency caches (KAIROS-T-0478).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexDiskUse {
    /// Each tool folder of `KAIROS_CODE_INDEX_DIR/tools/`, and the other
    /// files there (the archives) as `other`.
    pub tools: Vec<CodeIndexToolUse>,
    /// The dependency caches of the repositories, by language.
    pub caches: Vec<CodeIndexCacheUse>,
    /// The total of the dependency caches, in bytes.
    pub caches_bytes: u64,
    /// The cap of the dependency caches, in bytes
    /// (`KAIROS_CODE_INDEX_DEPS_CAP_MB`).
    pub caches_cap_bytes: u64,
    /// The free space of the disk of `KAIROS_CODE_INDEX_DIR`, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_bytes: Option<u64>,
}

/// One tool folder and its disk use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexToolUse {
    /// The folder: `go-1.27.2`, `node-24.22.0`, `rust-1.93.0`, … or `other`.
    pub name: String,
    /// The language that uses it: `rust`, `go`, `typescript`, `python`,
    /// `typescript, python` (Node.js), or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub bytes: u64,
}

/// The dependency caches of one language and their disk use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexCacheUse {
    pub language: String,
    /// The count of the repositories with a cache.
    pub repositories: u32,
    pub bytes: u64,
}

/// Query of `GET /api/repositories/{slug}/code-indexes/builds`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct CodeIndexBuildListQuery {
    /// The most runs in the answer: 20 when not given, 100 at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// The runs to skip before the page, newest first: 0 when not given
    /// (KAIROS-T-0584).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

/// The answer to `GET /api/repositories/{slug}/code-indexes/builds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexBuildList {
    /// The runs, newest first.
    pub items: Vec<CodeIndexBuild>,
    /// The runs of the repository, with those not in `items`.
    pub total: i64,
    /// The limit that the answer used.
    pub limit: i64,
    /// The runs before the page, newest first (KAIROS-T-0584).
    #[serde(default)]
    pub offset: i64,
    /// Set when no code index builder runs for the deployment
    /// (KAIROS-T-0394): the server has the role `api`, and no server with
    /// the role `worker` ran the new-commits sweep lately. The text says
    /// what to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder: Option<String>,
    /// The disk use of the tools and the dependency caches of the builder of
    /// the server that answers (KAIROS-T-0478). Only for an organization
    /// admin; None when that server has no `KAIROS_CODE_INDEX_DIR`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<CodeIndexDiskUse>,
}

// ---- the provider settings of a tenant (KAIROS-T-0339) ---------------------

/// Whether a secret is set, and by whom. The secret itself never leaves the
/// server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SecretStatus {
    pub set: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
    /// RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_at: Option<String>,
    /// The display name of `set_by` (KAIROS-T-0415).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by_name: Option<String>,
}

/// Where the summaries of the code index are made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SummaryProviderSettings {
    /// `embedded` (the model in the server), `ollama-cloud` (an
    /// OpenAI-compatible chat endpoint) or `bedrock` (AWS Bedrock).
    pub provider: String,
    /// The base URL of an OpenAI-compatible endpoint, for `ollama-cloud`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The model to ask for: a model name for `ollama-cloud`, a model id for
    /// `bedrock`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The AWS region, for `bedrock`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// The API key (`ollama-cloud`) or the AWS credentials (`bedrock`).
    pub secret: SecretStatus,
}

/// Where the vectors of the code index summaries are made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct VectorProviderSettings {
    /// `embedded` (the model in the server) or `remote` (an OpenAI-compatible
    /// embeddings endpoint).
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The API key of the endpoint. A local Ollama needs none.
    pub secret: SecretStatus,
}

/// The answer to `GET /api/org/code-index-settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeIndexSettings {
    pub summary: SummaryProviderSettings,
    pub vectors: VectorProviderSettings,
    /// The requests that a hosted summarizer sends at a time, 1 to 32.
    pub concurrency: i32,
    /// Where the summaries of a repository that follows the organization
    /// are made: `embedded` or `hosted` (KAIROS-T-0358).
    #[serde(default)]
    pub default_summaries: crate::types_repositories::CodeIndexSummaries,
    /// None when the tenant has no settings row (the defaults).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// The display name of `updated_by` (KAIROS-T-0415).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_name: Option<String>,
    /// The version of the settings: 0 for the defaults, then 1 more at each
    /// write (KAIROS-T-0415). Send it back with a write.
    #[serde(default)]
    pub version: i32,
    /// The repositories whose summaries the hosted provider makes now: set
    /// to `hosted`, or following the organization when the default is
    /// `hosted` (KAIROS-T-0415). By slug.
    #[serde(default)]
    pub hosted_repositories: Vec<String>,
}

/// The summaries part of `PUT /api/org/code-index-settings`. `secret`
/// absent keeps the stored secret; an empty secret removes it; a value
/// replaces it. For `bedrock` the secret is `<access key id>:<secret access
/// key>` or `<access key id>:<secret access key>:<session token>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutSummaryProvider {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String, format = Password)]
    pub secret: Option<crate::types_auth::Secret>,
}

/// The vectors part of `PUT /api/org/code-index-settings`. The secret has
/// the rules of [`PutSummaryProvider::secret`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutVectorProvider {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String, format = Password)]
    pub secret: Option<crate::types_auth::Secret>,
}

/// Body of `PUT /api/org/code-index-settings` (an organization admin).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutCodeIndexSettings {
    /// The summaries. Not given: they stay as stored (KAIROS-T-0415).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<PutSummaryProvider>,
    /// The vectors. Not given: they stay as stored (KAIROS-T-0415).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors: Option<PutVectorProvider>,
    /// 1 to 32. Not given: the value stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i32>,
    /// `embedded` or `hosted`: the summarizer of each repository that
    /// follows the organization (KAIROS-T-0358). When not given, the value
    /// stays. `hosted` needs a hosted summary provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_summaries: Option<crate::types_repositories::CodeIndexSummaries>,
    /// The version that the write is based on (KAIROS-T-0415). When it is
    /// not the stored version, the write is refused (409) and nothing
    /// changes. The web app and the CLI always send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
}

/// The read token of the organization for one forge host (KAIROS-T-0431).
/// A read shows the host, who set it and when. It never shows the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OrgReadToken {
    /// The host in lowercase, with `:port` only when the port is not the
    /// default, for example `github.com`.
    pub host: String,
    pub set_by: String,
    /// The display name of `set_by`, else the email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by_name: Option<String>,
    /// RFC 3339.
    pub set_at: String,
}

/// Body of `PUT /api/org/read-tokens/{host}` (KAIROS-T-0431, an
/// organization admin): the read token of the organization for the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetOrgReadTokenRequest {
    /// The token. Kairos encrypts it, and no read gives it back.
    #[schema(value_type = String, format = Password)]
    pub token: crate::types_auth::Secret,
}

/// Body of `POST /api/code-index/query-vector` (KAIROS-T-0360): the text of
/// a code search, and the model of the vectors of the index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryVectorRequest {
    /// The model of the vectors of the index: `<provider>/<model>/<dimension>`,
    /// for example `local/bge-small-en-v1.5-q/384`.
    pub model: String,
    /// The text of the query: 1 to 2,000 characters.
    pub text: String,
}

/// The vector of a query, made by the model that made the vectors of the
/// index (KAIROS-T-0360).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QueryVector {
    /// The model of the request.
    pub model: String,
    /// The vector: as many numbers as the dimension of the model.
    pub vector: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scip_warning_is_one_line_with_a_cut_reason() {
        let w = CodeIndexScipWarning {
            language: "python".into(),
            reason: "uv sync failed, the project asks for Python ==3.11.*; the runner has Python 3.13.16.".into(),
        };
        assert_eq!(
            w.note(),
            "python: edges by name (uv sync failed, the project asks for Python ==3.11.*; the runner has Python 3.13.16)"
        );
        let long = CodeIndexScipWarning {
            language: "go".into(),
            reason: "x".repeat(300),
        };
        let note = long.note();
        assert!(note.starts_with("go: edges by name (xxx"), "{note}");
        assert!(note.ends_with("x…)"), "{note}");
        assert_eq!(note.chars().count(), "go: edges by name (".len() + 240 + 2);
    }
}
