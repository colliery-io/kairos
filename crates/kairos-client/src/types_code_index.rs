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
    /// `upload`, or `build` (the server built it after a push).
    pub source: String,
    /// The size of the structure (the index with no summaries), in bytes.
    pub structure_bytes: i64,
    /// The summaries that the structure uses.
    pub summary_keys: i32,
    /// The model of the vectors, as `provider/model/dimension`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_model: Option<String>,
    /// When the index was first written, RFC 3339.
    pub created_at: String,
    /// When the index was last written, RFC 3339.
    pub updated_at: String,
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
}

/// Query of `GET /api/repositories/{slug}/code-indexes/builds`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct CodeIndexBuildListQuery {
    /// The most runs in the answer: 20 when not given, 100 at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
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
    /// None when the tenant has no settings row (the defaults).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
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
    pub summary: PutSummaryProvider,
    pub vectors: PutVectorProvider,
    /// 1 to 32; 4 when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i32>,
}
