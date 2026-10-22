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
    /// (KAIROS-A-0020) â anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The owner of the item (KAIROS-T-0411): the person who answers for
    /// it. Absent when the item has no owner. Present on the item
    /// endpoints and the board items. The default is for the client only:
    /// a response of an older server has no `owner`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ItemOwner>,
    /// The notes of the item, in the order of their ids (KAIROS-T-0630).
    /// Present on the read of one item (`GET /api/{family}/{short_code}`),
    /// empty when the item has none. Absent from lists and from the answers
    /// of writes. A note is not part of `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<ItemNote>>,
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
    /// (KAIROS-A-0020) â anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The owner of the item (KAIROS-T-0411): the person who answers for
    /// it. Absent when the item has no owner. Present on the item
    /// endpoints and the board items. The default is for the client only:
    /// a response of an older server has no `owner`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ItemOwner>,
    /// The notes of the item, in the order of their ids (KAIROS-T-0630).
    /// Present on the read of one item (`GET /api/{family}/{short_code}`),
    /// empty when the item has none. Absent from lists and from the answers
    /// of writes. A note is not part of `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<ItemNote>>,
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
    /// Planned/Support lane (`planned|support`, KAIROS-T-0077) â was this
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
    /// (KAIROS-A-0020) â anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The claim of the task: the person who has it in Active
    /// (KAIROS-T-0359). Absent when the task has no claim. Present on the
    /// task endpoints and the board items. The default is for the client
    /// only: a response of an older server has no `claim`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<TaskClaim>,
    /// The cancel mark of the task (KAIROS-T-0362): who cancelled it, why,
    /// and when. Absent when the task is not cancelled. Present on the
    /// task endpoints and the board items. The default is for the client
    /// only: a response of an older server has no `cancellation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<TaskCancellation>,
    /// The live state of the task (KAIROS-T-0506): branch, last commit,
    /// pushed, PR, next step, and who wrote it when. Absent when the task
    /// has no state. Present on the task endpoints and the board items.
    /// The default is for the client only: a response of an older server
    /// has no `state`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TaskState>,
    /// The notes of the item, in the order of their ids (KAIROS-T-0630).
    /// Present on the read of one item (`GET /api/{family}/{short_code}`),
    /// empty when the item has none. Absent from lists and from the answers
    /// of writes. A note is not part of `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<ItemNote>>,
}

/// The live state of a task (KAIROS-T-0506): one current state, separate
/// from the append-only notes. An agent writes it after each push and
/// before each stop. A write is a patch (KAIROS-T-0533).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskState {
    /// The branch of the work.
    #[serde(default)]
    pub branch: Option<String>,
    /// The last commit (a hash or a short hash).
    #[serde(default)]
    pub commit: Option<String>,
    /// True when the last commit is pushed.
    #[serde(default)]
    pub pushed: bool,
    /// The URL of the PR.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// What comes next.
    #[serde(default)]
    pub next_step: Option<String>,
    /// RFC 3339.
    pub updated_at: String,
    /// The user id (UUID) of who wrote the state.
    pub updated_by: String,
    /// The display name of who wrote the state.
    pub updated_by_name: String,
}

/// Body of `PUT /api/tasks/{short_code}/state` (KAIROS-T-0506). Each field
/// is optional. The write is a patch (KAIROS-T-0533): a field that you do
/// not send keeps its value, `null` clears it. Send at least one field.
/// `expected_updated_at` refuses a stale write with CONFLICT.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTaskStateRequest {
    /// The branch of the work. Absent: kept. `null`: cleared (KAIROS-T-0533).
    #[serde(
        default,
        deserialize_with = "crate::types_org::present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable)]
    pub branch: Option<Option<String>>,
    /// The last commit. Absent: kept. `null`: cleared.
    #[serde(
        default,
        deserialize_with = "crate::types_org::present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable)]
    pub commit: Option<Option<String>>,
    /// True when the last commit is pushed. Absent: kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<bool>,
    /// The URL of the PR. Absent: kept. `null`: cleared.
    #[serde(
        default,
        deserialize_with = "crate::types_org::present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable)]
    pub pr_url: Option<Option<String>>,
    /// What comes next. Absent: kept. `null`: cleared.
    #[serde(
        default,
        deserialize_with = "crate::types_org::present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable)]
    pub next_step: Option<Option<String>>,
    /// The `updated_at` of the state you read (RFC 3339). A write against a
    /// state that changed since is refused with `CONFLICT` and the current
    /// state (KAIROS-T-0533).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_updated_at: Option<String>,
}

/// The cancel mark of a task (KAIROS-T-0362). A cancel moves the task to
/// the done column of its board. A move out of the done column removes
/// the mark; the history keeps the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskCancellation {
    /// Why the task is cancelled.
    pub reason: String,
    /// The person or the service account that cancelled the task (UUID).
    pub cancelled_by: String,
    /// The display name of `cancelled_by`.
    pub cancelled_by_name: String,
    /// RFC 3339.
    pub cancelled_at: String,
}

/// Body of `POST /api/tasks/{short_code}/cancel` (KAIROS-T-0362).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelTaskRequest {
    /// Why the task is cancelled. Must not be empty.
    pub reason: String,
}

/// Response of `POST /api/tasks/{short_code}/purge` (KAIROS-T-0362): the
/// task that is gone for good.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PurgeTaskResponse {
    /// The short code of the deleted task.
    pub short_code: String,
    /// The title of the deleted task.
    pub title: String,
}

/// The claim of a task (KAIROS-T-0359, KAIROS-A-0024): the person who has
/// the task in Active. A task gets a claim when a person moves it to
/// Active. It loses the claim when it leaves Active, at a hand-off to a
/// different person, or at a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskClaim {
    /// The person (UUID).
    pub user_id: String,
    /// The display name of the person.
    pub display_name: String,
    /// True when the person's agent made the claim with the agent key of
    /// the person.
    #[serde(default)]
    pub agent: bool,
    /// RFC 3339.
    pub claimed_at: String,
}

/// Body of `POST /api/tasks/{short_code}/hand-off` (KAIROS-T-0359): the
/// person who gets the claim, by user id, email or user name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HandOffRequest {
    pub to: String,
}

/// A person who can get the claim of a task (KAIROS-T-0530). The person
/// holds `transition_items` on the board of the task. `GET
/// /api/tasks/{short_code}/hand-off-targets` gives a list of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HandOffTarget {
    /// `public.users.id` (UUID).
    pub user_id: String,
    pub email: String,
    pub display_name: String,
}

/// The owner of a strategy or an initiative (KAIROS-T-0411): the one
/// person who answers for the item.
///
/// A member of the board claims an item with no owner. The owner hands it
/// off. An organization admin sets or clears it. The owner gives no right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemOwner {
    /// The person (UUID).
    pub user_id: String,
    /// The display name of the person.
    pub display_name: String,
    /// When the person became the owner, RFC 3339.
    pub claimed_at: String,
}

/// Response of `GET /api/{entity_type}/{short_code}/owner` and of the
/// changes of the owner (KAIROS-T-0411).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemOwnerResponse {
    /// The short code of the strategy or the initiative.
    pub short_code: String,
    /// The owner, or absent when the item has no owner.
    #[serde(default)]
    pub owner: Option<ItemOwner>,
}

/// A supporting document, as returned by `/api/documents`. Documents do not
/// live on boards: a document is never a card, and it has no column. A
/// document has an owner (COLLIERY-T-0269): the board that the document
/// names (`board_id`). Each document has one (COLLIERY-T-3109). The `lifecycle` is an
/// editorial label (KAIROS-T-0078), and never a board position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Document {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-D-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content.
    pub content: String,
    /// The owner board (UUID): the board that the document names
    /// (COLLIERY-T-0269). It gives the right to edit the document, and the
    /// prefix of its code. Each document has one (COLLIERY-T-3109).
    pub board_id: String,
    /// The repositories that the document impacts (COLLIERY-T-0269), by
    /// slug. An `impacts` link says what the document is about. It gives
    /// no right.
    #[serde(default)]
    pub impacts: Vec<crate::types_repositories::Impact>,
    /// Template the document was stamped from (UUID), if any.
    pub template_id: Option<String>,
    /// Editorial lifecycle: `draft|review|published|archived`
    /// (KAIROS-T-0078). A label with free transitions â never a board
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
    /// (KAIROS-A-0020) â anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The notes of the item, in the order of their ids (KAIROS-T-0630).
    /// Present on the read of one item (`GET /api/{family}/{short_code}`),
    /// empty when the item has none. Absent from lists and from the answers
    /// of writes. A note is not part of `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<ItemNote>>,
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
    /// The repositories that the ADR impacts (COLLIERY-T-0269), by slug.
    /// An `impacts` link says what the ADR is about. It gives no right.
    #[serde(default)]
    pub impacts: Vec<crate::types_repositories::Impact>,
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
    /// (KAIROS-A-0020) â anything serving an archived row marks it, so an
    /// auditor never mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The notes of the item, in the order of their ids (KAIROS-T-0630).
    /// Present on the read of one item (`GET /api/{family}/{short_code}`),
    /// empty when the item has none. Absent from lists and from the answers
    /// of writes. A note is not part of `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<ItemNote>>,
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// Body of `POST /api/strategies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStrategyRequest {
    /// Board to create the strategy on — SLUG or UUID (KAIROS-T-0150).
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
#[serde(deny_unknown_fields)]
pub struct CreateInitiativeRequest {
    /// Board to create the initiative on — SLUG or UUID (KAIROS-T-0150).
    pub board_id: String,
    /// The entry column of the board (UUID), the default. An initiative
    /// starts in Discovery (KAIROS-T-0643): a different column is a 422
    /// `VALIDATION` on `column_id`. A move takes it on.
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
    /// The short code of the parent strategy (KAIROS-T-0572). The server
    /// writes the `parent` edge in the transaction of the create: a
    /// refused parent leaves no initiative. A retired code finds its item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Body of `POST /api/tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    /// Board to create the task on — SLUG or UUID (KAIROS-T-0150).
    /// Optional when `team_id` is given: the task then goes to the delivery
    /// board of that team. A request with no board and no team is a 422,
    /// with or without a `repository` (COLLIERY-T-0217, COLLIERY-A-0023).
    /// Until then a `repository` alone chose the board (KAIROS-T-0104).
    #[serde(default)]
    pub board_id: Option<String>,
    /// The entry column of the board (UUID), the default (KAIROS-T-0566).
    /// A task starts there, for every caller: any other column is a 422
    /// that names the entry column. Create the task, then move it.
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
    /// A team (UUID). It names the team whose delivery board gets the task,
    /// when no board is named (COLLIERY-T-0217); a team without exactly one
    /// live delivery board is a 422. With a board it must be that board's
    /// team (COLLIERY-T-0216): any other team is a 422, never silently
    /// ignored. The BOARD decides the team of the task in both cases.
    #[serde(default)]
    pub team_id: Option<String>,
    /// The repository the task links to (slug or UUID, KAIROS-T-0104). An
    /// optional link that says where the code is. It can be any live
    /// repository, of any team, and it does not choose the board
    /// (COLLIERY-T-0217, COLLIERY-A-0023).
    /// `repository` is THE reference field name on the wire (KAIROS-T-0115).
    /// The old name `repository_id` is refused, as each field that the
    /// route does not know (COLLIERY-T-0259).
    #[serde(default)]
    pub repository: Option<String>,
    /// The team that sends the task, by slug or UUID (KAIROS-T-0426). When
    /// it is not the team of the board, the task is a request whoever
    /// sends it: entry column, support lane. Omit it for today's rule.
    /// Kairos does not keep it on the task: name the sending team in the
    /// body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_team: Option<String>,
    /// The short code of the parent initiative (KAIROS-T-0572). The server
    /// writes the `parent` edge in the transaction of the create: a
    /// refused parent leaves no task. A retired code finds its item. In a
    /// claims column, `parent_gate` applies (KAIROS-A-0040).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Body of `POST /api/tasks/{short_code}/work-class` (KAIROS-T-0077): move
/// a task between the Planned/Support lanes. Orthogonal to column
/// Body of `PATCH /api/documents/{short_code}/lifecycle` (KAIROS-T-0078):
/// set the document's editorial state. Free transitions; no version bump
/// (the A-0004 contract covers title/content only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLifecycleRequest {
    /// `draft|review|published|archived`.
    pub lifecycle: String,
}

/// transitions â the board rules engine is never consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetWorkClassRequest {
    /// `planned|support`.
    pub work_class: String,
}

/// Body of `PUT /api/initiatives/{short_code}/complexity`: set the
/// complexity of an initiative. `null` or an empty string clears it. Not
/// content: no version bump (the A-0004 contract covers title/content only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetComplexityRequest {
    /// `xs|s|m|l|xl`; `null` or `""` clears.
    #[serde(default)]
    pub complexity: Option<String>,
}

/// Body of `POST /api/documents`. A document has an owner board from its
/// create (COLLIERY-T-0269), and `board` is required (COLLIERY-T-3109).
/// The document names that board as its owner, and its code gets the
/// prefix of that board. The caller needs `manage_documents` on that
/// board.
///
/// With `parent_short_code`, the document also supports that item: a
/// strategy, an initiative, or a task. The server creates the `supports`
/// edge. The parent gives no authority over the document.
///
/// The server refuses a body with no `board`, or with a null or empty
/// `board`: 422 `VALIDATION`, and `details.field` is `board`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDocumentRequest {
    pub title: String,
    /// The owner board (slug or UUID), required: a live board of each
    /// level. The document is not a card of the board. An absent or null
    /// value reads as empty, so that the server gives the refusal that
    /// names `board`.
    #[serde(default, deserialize_with = "null_as_empty")]
    #[schema(required = true)]
    pub board: String,
    /// Markdown content. Omitted + `template_id` set = the template's
    /// content is stamped in.
    #[serde(default)]
    pub content: Option<String>,
    /// Template to stamp content + metadata defaults from (UUID).
    #[serde(default)]
    pub template_id: Option<String>,
    /// Short code of the workflow item this document supports (optional).
    #[serde(default)]
    pub parent_short_code: Option<String>,
}

/// Body of `PATCH /api/documents/{short_code}/board` (COLLIERY-T-0269):
/// change the owner board of a document.
///
/// `board` is required. Its value is the slug or the id of a live board.
/// Each document has an owner board (COLLIERY-T-3109), and you cannot
/// remove it. The server refuses an absent, null or empty `board`: 422
/// `VALIDATION`, and `details.field` is `board`.
///
/// The board that the document has changes nothing: the response is 200,
/// and the server writes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDocumentBoardRequest {
    /// The new owner board (slug or UUID). An absent or null value reads
    /// as empty, so that the server gives the refusal that names `board`.
    #[serde(default, deserialize_with = "null_as_empty")]
    #[schema(required = true)]
    pub board: String,
    /// COLLIERY-T-3101 (default `false`): the document also gets the next
    /// code of its new owner board. Kairos retires the old code and changes
    /// the references to it one time. A rename needs a new board.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rename: bool,
}

/// A string field that reads a null as an empty string. With
/// `#[serde(default)]` an absent field is empty too. The server then
/// refuses the empty value with a message that names the field
/// (COLLIERY-T-3109).
fn null_as_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Body of `POST /api/adrs`. `board_id`/`column_id` follow the DDL rule:
/// both set (on-board) or both omitted (off-board; creation is then
/// org-admin-only, KAIROS-A-0006 fallback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAdrRequest {
    /// ADR board — SLUG or UUID (KAIROS-T-0150); omit for an off-board ADR.
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
    /// The short code of the item that the ADR supports: a strategy, an
    /// initiative or a task (KAIROS-T-0591). The server writes the
    /// `supports` edge in the transaction of the create: a refused parent
    /// leaves no ADR. A retired code finds its item.
    // Sent only with a value, so that an older server gets the body that it
    // knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Body of `PATCH /api/{family}/{short_code}` â the KAIROS-A-0004
/// optimistic-concurrency content edit. `version` is the version the edit
/// is based on; a stale value gets 409 `CONFLICT` with the current entity
/// in `details.current`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct MoveTaskRequest {
    pub board: String,
    /// Give the task the next code of the target board (COLLIERY-T-3101).
    /// Kairos retires the old code. The references to it change one time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rename: bool,
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

/// The S-0005 list envelope: `{items, total, limit, offset}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ListEnvelope<T: ToSchema> {
    pub items: Vec<T>,
    /// Rows matching the request, ignoring pagination. Live rows only
    /// unless the request asked for archived work as well â `total` and
    /// `items` always answer the same question (KAIROS-T-0159). The
    /// activity feed counts at most past the larger of 1000 and
    /// `offset + limit` (KAIROS-T-0627): a `total` above that bound means
    /// "more than the bound".
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
#[serde(deny_unknown_fields)]
pub struct Pagination {
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

/// `?limit=&offset=&include_deleted=` â the query of the five entity
/// family lists: S-0005 pagination plus the KAIROS-A-0020 archived opt-in.
///
/// Separate from [`Pagination`] on purpose. The other paginated listings
/// (boards, teams, members, tenants, templates, streams) have no archived
/// mode, and a shared struct would advertise a parameter they ignore â
/// which is how an auditor comes to believe they asked for archived work
/// and got none.
///
/// [`From<Pagination>`] makes every existing `Pagination` call site mean
/// exactly what it meant before: live rows only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
    /// Include archived (put-away) rows, each marked with `archived_at`
    /// (KAIROS-A-0020 rule 2). Default false â rule 3 is that a listing
    /// nobody asked hides them. `total` widens with the page, never
    /// independently of it.
    #[serde(default)]
    pub include_deleted: bool,
}

impl ListQuery {
    /// The archived-inclusive whole-family listing. Sugar for the one
    /// interesting non-default: `ListQuery::default()` stays live-only.
    pub fn including_archived() -> Self {
        Self {
            include_deleted: true,
            ..Self::default()
        }
    }
}

impl From<Pagination> for ListQuery {
    fn from(page: Pagination) -> Self {
        Self {
            limit: page.limit,
            offset: page.offset,
            include_deleted: false,
        }
    }
}

/// `?limit=&offset=&include_deleted=&repository=`: the query of the list
/// of documents and of the list of ADRs (COLLIERY-T-0269). It is
/// [`ListQuery`], and the repository that the items impact.
///
/// Separate from [`ListQuery`] for the reason that [`ListQuery`] is
/// separate from [`Pagination`]: a strategy, an initiative and a task
/// impact no repository, and a shared struct would show them a parameter
/// that does nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct ImpactListQuery {
    /// Page size (default 50, max 200).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Include archived (put-away) rows, each marked with `archived_at`
    /// (KAIROS-A-0020 rule 2). Default false.
    #[serde(default)]
    pub include_deleted: bool,
    /// Only the items that impact this repository (slug or UUID of a live
    /// repository). An unknown repository is a 404 `NOT_FOUND`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

impl ImpactListQuery {
    /// The items that impact one repository, live rows only.
    pub fn of_repository(repository: &str) -> Self {
        Self {
            repository: Some(repository.to_string()),
            ..Self::default()
        }
    }
}

impl From<ListQuery> for ImpactListQuery {
    fn from(query: ListQuery) -> Self {
        Self {
            limit: query.limit,
            offset: query.offset,
            include_deleted: query.include_deleted,
            repository: None,
        }
    }
}

impl From<Pagination> for ImpactListQuery {
    fn from(page: Pagination) -> Self {
        ListQuery::from(page).into()
    }
}

/// Response of `DELETE /api/{family}/{short_code}` â the soft delete and
/// its KAIROS-A-0001 cascade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeleteResponse {
    /// The deleted item's short code.
    pub short_code: String,
    /// How many live descendants were cascaded to (root excluded).
    pub cascade_count: i64,
    /// Short codes of the cascaded descendants, sorted.
    pub cascaded_short_codes: Vec<String>,
    /// The live descendants that the archive did not reach, sorted by
    /// short code (COLLIERY-T-0234). They stay live and keep their
    /// `parent` edge. Absent when the archive reached each descendant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_reached: Vec<NotReached>,
}

/// Query of `POST /api/boards/{id}/archive-completed` (KAIROS-T-0649).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct ArchiveCompletedQuery {
    /// Archive only the done tasks that entered their done column at
    /// least this many days ago: a whole number from 1 to 36500. With no
    /// value, the call archives each done task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_than_days: Option<i64>,
}

/// Response of `POST /api/boards/{id}/archive-completed` (KAIROS-T-0363):
/// the tasks of the done columns that the call archived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArchiveCompletedResponse {
    /// The board (UUID).
    pub board_id: String,
    /// How many tasks the call archived.
    pub count: i64,
    /// The short codes of the archived tasks, sorted.
    pub short_codes: Vec<String>,
}

/// One live descendant that an archive does not reach (COLLIERY-T-0234).
///
/// The archive stops at a descendant that the caller cannot edit, and
/// takes nothing below it. An entry has one of two reasons.
///
/// `required_capability`, with `board_id`: the caller cannot edit this
/// item.
///
/// `below`: this item is below the named item, where the archive stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotReached {
    /// The short code of the descendant.
    pub short_code: String,
    /// The `manage_<type>` capability that the caller does not hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    /// The authorization board of the descendant (UUID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    /// The short code of the item above this one where the archive stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<String>,
}

/// Body of `POST /api/{entity_type}/{short_code}/notes` (KAIROS-T-0367):
/// the text of a note. A note is a row of its own (KAIROS-T-0630).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AppendNoteRequest {
    /// The markdown text of the note. It must not be empty.
    pub text: String,
}

/// Body of `POST /api/{entity_type}/{short_code}/notes/{note_id}/promote`
/// (KAIROS-T-0505): turn a note into a draft ADR or document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PromoteNoteRequest {
    /// `adr` or `document`.
    pub kind: String,
    /// The board that gets the new item (slug or UUID). An ADR board for
    /// an ADR; the owner board for a document.
    pub board: String,
    /// The title of the new item. Default: the first line of the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Response of `POST /api/{entity_type}/{short_code}/notes/{note_id}/promote`
/// (KAIROS-T-0505).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PromoteNoteResponse {
    /// The short code of the source item.
    pub short_code: String,
    /// The id of the note: `n-3`.
    pub note_id: String,
    /// `adr` or `document`.
    pub kind: String,
    /// The short code of the new item.
    pub promoted_to: String,
    /// True when the new item `supports` the source item. False for a note
    /// on a document or an ADR: no edge joins two attachments.
    pub supports: bool,
}

/// One note of an item (KAIROS-T-0630): a row of its own, not part of the
/// content of the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemNote {
    /// The id of the note in its item: `n-3`. Cite it as `note n-3 on
    /// ACME-T-0012`.
    pub id: String,
    /// The user that wrote the note (UUID). Absent for an old note whose
    /// name is no user of the organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    /// The name of the author: the display name of the user, or the name
    /// of an old note as written.
    pub author: String,
    /// RFC 3339.
    pub created_at: String,
    /// The markdown text. It can have headings.
    pub text: String,
    /// The short code of the draft ADR or document that a promote made from
    /// the note. Absent until then. A note is promoted once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_to: Option<String>,
}

/// Response of `POST /api/{entity_type}/{short_code}/notes`
/// (KAIROS-T-0367): the id of the note (KAIROS-T-0504). A note writes no
/// version of the content (KAIROS-T-0630).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AppendNoteResponse {
    /// The item's short code.
    pub short_code: String,
    /// The version of the content of the item. A note does not change it.
    pub version: i32,
    /// The id of the note: `n-3`. The item page anchors it (`#n-3`). The
    /// default is for the client only: a response of an older server has
    /// no `note_id`.
    #[serde(default)]
    pub note_id: String,
}

/// Response of `POST /api/{entity_type}/{short_code}/restore`
/// (KAIROS-T-0160, KAIROS-A-0020): the archived item is live again.
///
/// A restore deliberately does NOT un-cascade â a cascade delete was an act
/// on a subtree, and resurrecting descendants would undo decisions nobody
/// asked to revisit. The descendants still away are named here instead, so
/// "I restored it and half of it is missing" is answered before it is asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RestoreResponse {
    /// The restored item's short code.
    pub short_code: String,
    /// How many of its descendants are still archived.
    pub still_archived_count: i64,
    /// Their short codes, sorted â restore them with further calls.
    pub still_archived_short_codes: Vec<String>,
}

/// Response of `GET /api/{entity_type}/{short_code}/cascade-preview`
/// (KAIROS-T-0051) â the AUTHORITATIVE KAIROS-A-0001 descendant set a
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
    /// The live descendants that an archive BY THIS CALLER would not
    /// reach, sorted by short code (COLLIERY-T-0234). The preview answers
    /// for the caller who asks: a different caller can get a different
    /// answer. Absent when the archive would reach each descendant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_reached: Vec<NotReached>,
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
