//! Shared API wire types (KAIROS-T-0018, the start of the KAIROS-A-0015
//! shared-types story): entity DTOs, request bodies, the list envelope, and
//! the KAIROS-S-0005 error envelope. `kairos-server` serializes these;
//! `kairos-cli` and the integration tests deserialize the same structs, so
//! the wire contract lives in exactly one place.
//!
//! # Dependency discipline
//!
//! This crate stays dependency-light (serde/serde_json/utoipa only), so ids
//! and timestamps travel as strings:
//!
//! - UUIDs: canonical hyphenated form (`"550e8400-e29b-41d4-a716-..."`),
//! - timestamps: RFC 3339 (`"2026-07-10T12:00:00+00:00"`),
//! - dates (`Adr::decision_date`): `"YYYY-MM-DD"`.
//!
//! The server parses inbound id strings and rejects malformed values with
//! 422 `VALIDATION`. Conversions from `kairos-db` models live in
//! `kairos-server` (`api::convert`), keeping this crate free of diesel.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

// ---------------------------------------------------------------------------
// Entity DTOs (responses)
// ---------------------------------------------------------------------------

/// A strategy (Flight Level 3), as returned by `/api/strategies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Strategy {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-S-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// Board the strategy sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    pub hypothesis: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004); submit it back on
    /// PATCH.
    pub version: i32,
    /// Creator user id (UUID).
    pub created_by: String,
    /// Last editor user id (UUID).
    pub updated_by: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live.
    /// Archiving hides work from default listings and nothing more
    /// (KAIROS-A-0020) — anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// An initiative (Flight Level 2), as returned by `/api/initiatives`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Initiative {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-I-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// Board the initiative sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    /// T-shirt sizing (`xs|s|m|l|xl`).
    pub complexity: Option<String>,
    /// Whether this initiative is a bucket.
    pub is_bucket: bool,
    /// Bucket kind (`tech_debt|bug|ad_hoc`), set iff `is_bucket`.
    pub bucket_type: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID).
    pub created_by: String,
    /// Last editor user id (UUID).
    pub updated_by: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live.
    /// Archiving hides work from default listings and nothing more
    /// (KAIROS-A-0020) — anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// A task/bug/tech-debt item (Flight Level 1), as returned by `/api/tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Task {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-T-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// Board the task sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    /// `task|bug|tech_debt|support`.
    pub task_type: String,
    /// Planned/Support lane (`planned|support`, KAIROS-T-0077) — was this
    /// work planned, or unplanned intake? Orthogonal to `task_type`.
    pub work_class: String,
    /// Owning team (UUID), if assigned.
    pub team_id: Option<String>,
    /// The repository this task is issued against (UUID), if bound
    /// (KAIROS-T-0104, A-0019).
    #[serde(default)]
    pub repository_id: Option<String>,
    /// The bound repository, embedded (slug, forge, name, owning team).
    /// Present on the task, board-items and search endpoints; other
    /// carriers (events) send only `repository_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<crate::types_repositories::RepositoryRef>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID).
    pub created_by: String,
    /// Last editor user id (UUID).
    pub updated_by: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live.
    /// Archiving hides work from default listings and nothing more
    /// (KAIROS-A-0020) — anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// A supporting document, as returned by `/api/documents`. Documents do not
/// live on boards; they attach to a workflow item via a `supports` edge and
/// inherit that item's board for authorization (KAIROS-A-0006). Their
/// `lifecycle` is an editorial label (KAIROS-T-0078) — never board
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Document {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-D-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// Template the document was stamped from (UUID), if any.
    pub template_id: Option<String>,
    /// Editorial lifecycle: `draft|review|published|archived`
    /// (KAIROS-T-0078). A label with free transitions — never a board
    /// column, never metadata.
    pub lifecycle: String,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID).
    pub created_by: String,
    /// Last editor user id (UUID).
    pub updated_by: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live.
    /// Archiving hides work from default listings and nothing more
    /// (KAIROS-A-0020) — anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// An Architecture Decision Record, as returned by `/api/adrs`. Board
/// placement is optional (`board_id`/`column_id` both set or both null).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Adr {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-A-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// ADR board (UUID), if placed on one.
    pub board_id: Option<String>,
    /// Current column (UUID), if placed on a board.
    pub column_id: Option<String>,
    pub decision_maker: Option<String>,
    /// `YYYY-MM-DD`.
    pub decision_date: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID).
    pub created_by: String,
    /// Last editor user id (UUID).
    pub updated_by: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live.
    /// Archiving hides work from default listings and nothing more
    /// (KAIROS-A-0020) — anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// Body of `POST /api/strategies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateStrategyRequest {
    /// Board to create the strategy on (UUID).
    pub board_id: String,
    /// Column to place it in (UUID); defaults to the board's first column.
    #[serde(default)]
    pub column_id: Option<String>,
    pub title: String,
    /// Markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub hypothesis: Option<String>,
}

/// Body of `POST /api/initiatives`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateInitiativeRequest {
    /// Board to create the initiative on (UUID).
    pub board_id: String,
    /// Column to place it in (UUID); defaults to the board's first column.
    #[serde(default)]
    pub column_id: Option<String>,
    pub title: String,
    /// Markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    /// T-shirt sizing (`xs|s|m|l|xl`).
    #[serde(default)]
    pub complexity: Option<String>,
    /// Marks the initiative as a bucket of this kind
    /// (`tech_debt|bug|ad_hoc`); `is_bucket` is derived.
    #[serde(default)]
    pub bucket_type: Option<String>,
}

/// Body of `POST /api/tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    /// Board to create the task on (UUID). Optional since KAIROS-T-0104:
    /// when `repository` is given the task is ROUTED to the owning
    /// team's delivery board (A-0019); when both are given they must
    /// agree; neither is a 422.
    #[serde(default)]
    pub board_id: Option<String>,
    /// Column to place it in (UUID); defaults to the board's first column.
    #[serde(default)]
    pub column_id: Option<String>,
    pub title: String,
    /// Markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    /// `task|bug|tech_debt|support`; defaults to `task`.
    #[serde(default)]
    pub task_type: Option<String>,
    /// Planned/Support lane (`planned|support`, KAIROS-T-0077). Defaults
    /// to `support` when `task_type` is `support`, else `planned`.
    #[serde(default)]
    pub work_class: Option<String>,
    /// Owning team (UUID). Defaults to the repository's owning team when
    /// `repository` is given; an explicit different team is a 422.
    #[serde(default)]
    pub team_id: Option<String>,
    /// Repository to issue the task against (slug or UUID, KAIROS-T-0104).
    /// Routes the task: repo -> owning team -> that team's delivery board.
    /// `repository` is THE reference field name on the wire (KAIROS-T-0115);
    /// `repository_id` is accepted as an alias for one release.
    #[serde(default, alias = "repository_id")]
    pub repository: Option<String>,
}

/// Body of `POST /api/tasks/{short_code}/work-class` (KAIROS-T-0077): move
/// a task between the Planned/Support lanes. Orthogonal to column
/// Body of `PATCH /api/documents/{short_code}/lifecycle` (KAIROS-T-0078):
/// set the document's editorial state. Free transitions; no version bump
/// (the A-0004 contract covers title/content only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SetLifecycleRequest {
    /// `draft|review|published|archived`.
    pub lifecycle: String,
}

/// transitions — the board rules engine is never consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SetWorkClassRequest {
    /// `planned|support`.
    pub work_class: String,
}

/// Body of `POST /api/documents`. Documents attach to a workflow item at
/// birth: `parent_short_code` is REQUIRED (KAIROS-T-0018 contract) and must
/// name a strategy, initiative, or task; the server creates the `supports`
/// edge and authorizes `manage_documents` against the parent's board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateDocumentRequest {
    pub title: String,
    /// Markdown content. Omitted + `template_id` set = the template's
    /// content is stamped in.
    #[serde(default)]
    pub content: Option<String>,
    /// Template to stamp content + metadata defaults from (UUID).
    #[serde(default)]
    pub template_id: Option<String>,
    /// Short code of the workflow item this document supports. Required
    /// (422 `VALIDATION` when missing).
    #[serde(default)]
    pub parent_short_code: Option<String>,
}

/// Body of `POST /api/adrs`. `board_id`/`column_id` follow the DDL rule:
/// both set (on-board) or both omitted (off-board; creation is then
/// org-admin-only, KAIROS-A-0006 fallback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateAdrRequest {
    /// ADR board (UUID); omit for an off-board ADR.
    #[serde(default)]
    pub board_id: Option<String>,
    /// Column (UUID); defaults to the board's first column when `board_id`
    /// is set.
    #[serde(default)]
    pub column_id: Option<String>,
    pub title: String,
    /// Markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub decision_maker: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub decision_date: Option<String>,
}

/// Body of `PATCH /api/{family}/{short_code}` — the KAIROS-A-0004
/// optimistic-concurrency content edit. `version` is the version the edit
/// is based on; a stale value gets 409 `CONFLICT` with the current entity
/// in `details.current`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateContentRequest {
    /// New title; omitted = keep the current title.
    #[serde(default)]
    pub title: Option<String>,
    /// New markdown content (full replacement).
    pub content: String,
    /// The version this edit is based on ("I'm editing version N").
    pub version: i32,
}

/// Body of `POST /api/{family}/{short_code}/transition`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TransitionRequest {
    /// Target column (UUID). Must be reachable from the item's current
    /// column per the board's transition graph, else 422
    /// `INVALID_TRANSITION` with `details.allowed_targets`.
    pub to_column_id: String,
}

/// Body of `POST /api/tasks/{short_code}/move` (KAIROS-I-0012): the
/// delivery board to move the task to, by slug or UUID. It lands in that
/// board's entry column and follows its team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MoveTaskRequest {
    pub board: String,
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

/// The S-0005 list envelope: `{items, total, limit, offset}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ListEnvelope<T: ToSchema> {
    pub items: Vec<T>,
    /// Total live rows (ignoring pagination).
    pub total: i64,
    /// The applied limit.
    pub limit: i64,
    /// The applied offset.
    pub offset: i64,
}

/// `?limit=&offset=` pagination for list endpoints (S-0005: the only list
/// parameters; all complex querying is `POST /api/search`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Pagination {
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Response of `DELETE /api/{family}/{short_code}` — the soft delete and
/// its KAIROS-A-0001 cascade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeleteResponse {
    /// The deleted item's short code.
    pub short_code: String,
    /// How many live descendants were cascaded to (root excluded).
    pub cascade_count: i64,
    /// Short codes of the cascaded descendants, sorted.
    pub cascaded_short_codes: Vec<String>,
}

/// Response of `GET /api/{entity_type}/{short_code}/cascade-preview`
/// (KAIROS-T-0051) — the AUTHORITATIVE KAIROS-A-0001 descendant set a
/// soft-delete of this item WOULD cascade to, computed WITHOUT deleting.
/// The fields mirror [`DeleteResponse`] so a client can render the same
/// warning before the delete as it shows after, and `cascaded_short_codes`
/// is identical to the `DeleteResponse` a subsequent delete would return
/// (barring concurrent edits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadePreviewResponse {
    /// The item's short code (the cascade root).
    pub short_code: String,
    /// How many live descendants a delete would cascade to (root excluded).
    pub cascade_count: i64,
    /// Short codes of the live descendants a delete would cascade to,
    /// sorted.
    pub cascaded_short_codes: Vec<String>,
}

/// The S-0005 error envelope: `{"error": {"code", "message", "details"}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

/// The `error` object of [`ErrorEnvelope`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    /// Stable machine-readable code (`CONFLICT`, `INVALID_TRANSITION`,
    /// `FORBIDDEN`, `NOT_FOUND`, `VALIDATION`, ...).
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Structured extras (e.g. `current` on 409, `allowed_targets` on 422
    /// `INVALID_TRANSITION`, `required_capability` on 403); `{}` when there
    /// is nothing structured to add.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}
