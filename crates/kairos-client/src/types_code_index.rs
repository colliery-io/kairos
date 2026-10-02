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
