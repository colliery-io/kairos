//! Data layer for the item detail page (KAIROS-T-0041): the short-code →
//! family mapping, the mirror DTOs the page reads, and the PATCH/DELETE
//! verbs it needs.
//!
//! GETs and POSTs go through the shared [`crate::api`] helpers. PATCH and
//! DELETE live here rather than as `api.rs` siblings because three GUI
//! tasks are being built concurrently in this crate and `api.rs` is shared
//! surface — recorded in KAIROS-T-0041's status updates as a follow-up
//! (hoist into `api.rs` when the fan-out lands). Same shape as
//! `api::get_json`: bearer header, S-0005 envelope mapping, global 401.
//!
//! The content PATCH is special-cased: a 409 carries the server-current
//! entity in `details.current` (KAIROS-A-0004), which the merge UI needs,
//! so [`update_content`] returns a typed [`SaveError`] instead of
//! flattening the conflict into an `ApiError`.

use std::collections::BTreeMap;

use aurora_dark::tokens::ApiError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::get_json;
use crate::auth::Auth;

// ---------------------------------------------------------------------------
// Entity families (the five S-0005 endpoint families)
// ---------------------------------------------------------------------------

/// The five entity families, resolved from a short code's type letter
/// (`{PREFIX}-{S|I|T|D|A}-{NNNN}`). One `/items/:code` route serves all
/// five; the family picks the API paths and the type-specific facts shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Strategy,
    Initiative,
    Task,
    Document,
    Adr,
}

impl Family {
    /// Parse the family out of a short code. The prefix may itself contain
    /// `-` (tenant slugs allow it), so the type letter is read from the
    /// *end*: last segment = number, second-to-last = type letter.
    pub fn of_short_code(code: &str) -> Option<Family> {
        let mut segments = code.rsplit('-');
        let number = segments.next()?;
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let family = match segments.next()? {
            "S" => Family::Strategy,
            "I" => Family::Initiative,
            "T" => Family::Task,
            "D" => Family::Document,
            "A" => Family::Adr,
            _ => return None,
        };
        // At least one prefix segment must remain.
        segments.next().filter(|s| !s.is_empty())?;
        Some(family)
    }

    /// The singular entity-type name (KAIROS-T-0078 metadata scoping;
    /// matches the server's entity-type vocabulary).
    pub fn entity_type(self) -> &'static str {
        match self {
            Family::Strategy => "strategy",
            Family::Initiative => "initiative",
            Family::Task => "task",
            Family::Document => "document",
            Family::Adr => "adr",
        }
    }

    /// The plural family segment used by every `/api/{family}` route.
    pub fn api_family(self) -> &'static str {
        match self {
            Family::Strategy => "strategies",
            Family::Initiative => "initiatives",
            Family::Task => "tasks",
            Family::Document => "documents",
            Family::Adr => "adrs",
        }
    }

    /// Human label for the page header.
    pub fn label(self) -> &'static str {
        match self {
            Family::Strategy => "Strategy",
            Family::Initiative => "Initiative",
            Family::Task => "Task",
            Family::Document => "Document",
            Family::Adr => "ADR",
        }
    }

    /// Workflow items (strategy/initiative/task) can parent a document
    /// (`POST /api/documents` requires such a parent).
    pub fn is_workflow(self) -> bool {
        matches!(self, Family::Strategy | Family::Initiative | Family::Task)
    }
}

// ---------------------------------------------------------------------------
// Mirrors (partial on purpose — see docs/gui-conventions.md § Data layer)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types::{Strategy,Initiative,Task,Document,Adr}`
/// (partial union — every field the detail page reads; per-type extras are
/// optional and absent for the other families).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemDetail {
    pub short_code: String,
    pub title: String,
    pub content: String,
    /// KAIROS-A-0004 optimistic-concurrency version; PATCH carries it back.
    pub version: i32,
    /// Board placement (optional for ADRs). For a document it is the
    /// OWNER board (COLLIERY-T-0269): the board that the document names,
    /// which gives the right to edit it. It is not a placement: a
    /// document has no column.
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub column_id: Option<String>,
    /// The repositories that a document or an ADR impacts
    /// (COLLIERY-T-0269). Empty for the other families.
    #[serde(default)]
    pub impacts: Vec<Impact>,
    /// Editorial lifecycle (KAIROS-T-0078; documents only) —
    /// `draft|review|published|archived`.
    ///
    /// **Vocabulary collision** (KAIROS-A-0020, recorded for whoever does
    /// the rename): this `archived` is an EDITORIAL state and has nothing
    /// to do with [`ItemDetail::archived_at`] below. A published document
    /// can be editorially archived while being perfectly live, and an
    /// editorially-draft document can be put away. Both can be true at
    /// once, so the page must never use one word for both.
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// When this item was PUT AWAY (RFC 3339), absent while it is live
    /// (KAIROS-T-0154, ADR-20). Set = archived in the `deleted_at` sense:
    /// hidden from boards, queues and default search, read-only until
    /// restored — and still fully readable, which is the whole point.
    #[serde(default)]
    pub archived_at: Option<String>,
    /// Entity UUID — the `activity_log` filter key, so the banner can name
    /// WHO put the item away (KAIROS-T-0164).
    pub id: String,
    /// The user id of the creator of the item. The creator may edit the
    /// item with no capability on its board (COLLIERY-T-0228), so the page
    /// needs it to show the edit controls to the right person. Each of the
    /// five DTOs carries it; the default is for a body that does not, and
    /// an empty id matches nobody.
    #[serde(default)]
    pub created_by: String,
    pub updated_at: String,
    // -- per-type extras --------------------------------------------------
    #[serde(default)]
    pub hypothesis: Option<String>,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub is_bucket: Option<bool>,
    #[serde(default)]
    pub bucket_type: Option<String>,
    #[serde(default)]
    pub task_type: Option<String>,
    /// `planned|support` — the lane axis (KAIROS-T-0077; tasks only).
    #[serde(default)]
    pub work_class: Option<String>,
    /// The bound repository (KAIROS-T-0109, A-0019; tasks only).
    #[serde(default)]
    pub repository: Option<crate::pages::repositories::api::RepositoryRef>,
    #[serde(default)]
    pub decision_maker: Option<String>,
    #[serde(default)]
    pub decision_date: Option<String>,
}

/// mirror of: `kairos_client::types_repositories::Impact` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Impact {
    pub repository: ImpactedRepository,
}

/// mirror of: `kairos_client::types_repositories::ImpactedRepository`
/// (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ImpactedRepository {
    pub slug: String,
    #[serde(default)]
    pub repo_full_name: String,
    /// Set when the repository is archived. The link stays.
    #[serde(default)]
    pub archived_at: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (partial).
/// `team_id` + `transitions` feed the move control (KAIROS-T-0075): the
/// detail page computes powers and legal targets exactly like the board.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardInfo {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub columns: Vec<BoardColumnInfo>,
    #[serde(default)]
    pub transitions: Vec<BoardTransitionInfo>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumnInfo {
    pub id: String,
    pub name: String,
    /// Set when the column has been REMOVED from the board
    /// (KAIROS-T-0161) — only ever present because [`fetch_board`] asks
    /// for removed columns; see its docs.
    #[serde(default)]
    pub removed_at: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardTransition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardTransitionInfo {
    pub from_column_id: String,
    pub to_column_id: String,
}

/// mirror of: `kairos_client::types::ListEnvelope` (partial — the page
/// only reads `items`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
}

/// mirror of: `kairos_client::types_meta::ItemMetadataResponse` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemMetadata {
    pub values: Vec<MetadataValue>,
}

/// mirror of: `kairos_client::types_meta::MetadataValue` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MetadataValue {
    pub slug: String,
    pub value: String,
}

/// mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MetadataDefinition {
    pub name: String,
    pub slug: String,
    /// `string|enum|date`.
    pub field_type: String,
    #[serde(default)]
    pub enum_options: Vec<String>,
}

/// mirror of: `kairos_client::types_meta::ChildrenProgressResponse`
/// (partial — KAIROS-T-0080).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChildrenProgress {
    pub total: i64,
    pub done: i64,
    /// False = no involved board has done columns; composition only.
    #[serde(default)]
    pub has_done_columns: bool,
    #[serde(default)]
    pub by_column: Vec<ChildColumnProgress>,
}

/// mirror of: `kairos_client::types_meta::ChildColumnProgress` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChildColumnProgress {
    pub column_name: String,
    pub is_done: bool,
    pub count: i64,
}

/// mirror of: `kairos_client::types_meta::ItemRelationshipsResponse`
/// (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemRelationships {
    #[serde(default)]
    pub outgoing: Vec<RelationshipGroup>,
    #[serde(default)]
    pub incoming: Vec<RelationshipGroup>,
}

/// mirror of: `kairos_client::types_meta::RelationshipGroup` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RelationshipGroup {
    pub relationship: String,
    pub items: Vec<RelatedItem>,
}

/// mirror of: `kairos_client::types_meta::RelatedItem` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RelatedItem {
    pub short_code: String,
    pub entity_type: String,
    pub title: String,
    /// When this neighbour was put away, RFC 3339; **absent while live**,
    /// so presence is the marker (KAIROS-T-0158 / ADR-20).
    ///
    /// Relationship lists are archived-INCLUSIVE: an item's edges
    /// describe what it contains and depends on, and dropping an
    /// archived endpoint would silently shrink that answer. The panel
    /// must therefore SHOW the state — an unmarked archived neighbour is
    /// worse than a missing one, because the reader acts on it.
    ///
    /// This mirror is partial, so it compiled perfectly well without
    /// this field and simply never rendered the marker. Adding a wire
    /// field is not enough; the mirror has to want it.
    #[serde(default)]
    pub archived_at: Option<String>,
    /// `true` when this neighbour sits in a terminal column; **absent
    /// otherwise** (COLLIERY-T-0214). Done work does not block and is not
    /// blocked, so the panel marks a done neighbour on a `blocks` edge:
    /// without the mark a finished blocker reads as one still in the way.
    #[serde(default)]
    pub done: bool,
}

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateDetail {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Vec<TemplateField>,
}

/// mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateField {
    pub slug: String,
    pub name: String,
    pub field_type: String,
    #[serde(default)]
    pub enum_options: Vec<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// mirror of: `kairos_client::types::NotReached` (COLLIERY-T-0234): one
/// live descendant that an archive does not reach. It has one reason: the
/// capability that the caller does not hold, or the item above it where
/// the archive stopped.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NotReached {
    pub short_code: String,
    #[serde(default)]
    pub required_capability: Option<String>,
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub below: Option<String>,
}

/// mirror of: `kairos_client::types::DeleteResponse` (the A-0001 soft
/// delete + cascade report).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DeleteOutcome {
    pub short_code: String,
    pub cascade_count: i64,
    #[serde(default)]
    pub cascaded_short_codes: Vec<String>,
    /// Absent when the archive reached each descendant (COLLIERY-T-0234).
    #[serde(default)]
    pub not_reached: Vec<NotReached>,
}

/// mirror of: `kairos_client::types::CascadePreviewResponse` (KAIROS-T-0051
/// — the AUTHORITATIVE pre-delete cascade set; identical shape to
/// [`DeleteOutcome`], so the confirm dialog renders the real transitive
/// descendants BEFORE the delete rather than only its direct children).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CascadePreview {
    pub short_code: String,
    pub cascade_count: i64,
    #[serde(default)]
    pub cascaded_short_codes: Vec<String>,
    /// What an archive by the signed-in user would leave
    /// (COLLIERY-T-0234). Absent when it would leave nothing.
    #[serde(default)]
    pub not_reached: Vec<NotReached>,
}

// ---------------------------------------------------------------------------
// The A-0004 conflict shape
// ---------------------------------------------------------------------------

/// The server-current entity carried by a 409 in `details.current`
/// (mirror of: the full entity DTO — the merge UI reads these three).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CurrentVersion {
    pub version: i32,
    pub title: String,
    pub content: String,
}

/// mirror of: `kairos_client::types::ErrorEnvelope` — with `details`, which
/// the shared `api.rs` mirror deliberately drops.
#[derive(Debug, Deserialize)]
struct DetailedErrorEnvelope {
    error: DetailedErrorBody,
}

/// mirror of: `kairos_client::types::ErrorBody` (partial, + details).
#[derive(Debug, Deserialize)]
struct DetailedErrorBody {
    code: String,
    message: String,
    #[serde(default)]
    details: ErrorDetails,
}

/// The structured extras this page understands (`current` on 409,
/// `missing` on a 422 `RESTORE_BLOCKED`).
#[derive(Debug, Default, Deserialize)]
struct ErrorDetails {
    #[serde(default)]
    current: Option<CurrentVersion>,
    /// Human-readable names of what an archived item needs and no longer
    /// has ("its board column (removed)", …) — KAIROS-T-0160.
    #[serde(default)]
    missing: Vec<String>,
}

/// Outcome of a content save: a version conflict is not a dead end — it
/// opens the merge UI. (No `Debug` derive: `aurora_dark::tokens::ApiError`
/// does not implement it.)
pub enum SaveError {
    /// 409: the edit was based on a stale version; `current` is the
    /// server-current entity from `details.current`.
    Conflict(CurrentVersion),
    /// Anything else, mapped like every other call.
    Api(ApiError),
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `GET /api/{family}/{short_code}` → the entity, whichever family.
pub async fn fetch_item(auth: Auth, family: Family, code: String) -> Result<ItemDetail, ApiError> {
    get_json(auth, &format!("/api/{}/{code}", family.api_family())).await
}

/// `GET /api/boards/{id}?include_removed_columns=true` → board name/slug +
/// columns (for the board/column display).
///
/// The item page is the ONE caller that asks for removed columns
/// (KAIROS-T-0164): an archived card keeps pointing at the column it was
/// put away in, and that column may since have been removed from the board
/// (KAIROS-T-0161). Without the flag this page rendered "unknown column"
/// for exactly the item whose placement is audit material. Removed columns
/// arrive carrying `removed_at`, are labelled as removed, and are never
/// offered as move targets (the transition list is live-only server-side).
pub async fn fetch_board(auth: Auth, board_id: String) -> Result<BoardInfo, ApiError> {
    get_json(
        auth,
        &format!("/api/boards/{board_id}?include_removed_columns=true"),
    )
    .await
}

/// `GET /api/metadata-definitions?entity_type=…` — ONLY the definitions
/// in scope for this item's type (KAIROS-T-0078): the server filter is
/// the same rule the write path enforces, so the picker can never offer
/// a field the PATCH would reject.
pub async fn fetch_definitions(
    auth: Auth,
    family: Family,
) -> Result<Vec<MetadataDefinition>, ApiError> {
    // COLLIERY-T-0258: each definition of the type, page after page.
    crate::api::get_all(
        auth,
        &format!(
            "/api/metadata-definitions?entity_type={}",
            family.entity_type()
        ),
    )
    .await
}

/// `PATCH /api/documents/{short_code}/lifecycle` — set the editorial
/// state (KAIROS-T-0078). The response body is discarded; the page
/// refetches wholesale.
pub async fn set_lifecycle(auth: Auth, code: &str, lifecycle: &str) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct Body<'a> {
        lifecycle: &'a str,
    }
    let path = format!("/api/documents/{code}/lifecycle");
    let _: ItemDetail = send_json(auth, Verb::Patch, &path, Some(&Body { lifecycle })).await?;
    Ok(())
}

/// `GET /api/{family}/{short_code}/metadata` → the item's current values.
pub async fn fetch_metadata(
    auth: Auth,
    family: Family,
    code: String,
) -> Result<Vec<MetadataValue>, ApiError> {
    let response: ItemMetadata = get_json(
        auth,
        &format!("/api/{}/{code}/metadata", family.api_family()),
    )
    .await?;
    Ok(response.values)
}

/// `GET /api/{family}/{short_code}/children-progress` → the direct
/// children rollup (KAIROS-T-0080).
pub async fn fetch_children_progress(
    auth: Auth,
    family: Family,
    code: String,
) -> Result<ChildrenProgress, ApiError> {
    get_json(
        auth,
        &format!("/api/{}/{code}/children-progress", family.api_family()),
    )
    .await
}

/// `GET /api/{family}/{short_code}/relationships` → both directions,
/// grouped.
pub async fn fetch_relationships(
    auth: Auth,
    family: Family,
    code: String,
) -> Result<ItemRelationships, ApiError> {
    get_json(
        auth,
        &format!("/api/{}/{code}/relationships", family.api_family()),
    )
    .await
}

/// `GET /api/{family}/{short_code}/cascade-preview` → the AUTHORITATIVE
/// transitive descendant set a delete would cascade to (KAIROS-T-0051 —
/// what the pre-delete confirm warns with, matching the eventual
/// `DeleteResponse`).
pub async fn fetch_cascade_preview(
    auth: Auth,
    family: Family,
    code: String,
) -> Result<CascadePreview, ApiError> {
    get_json(
        auth,
        &format!("/api/{}/{code}/cascade-preview", family.api_family()),
    )
    .await
}

// ---------------------------------------------------------------------------
// Who put it away (KAIROS-T-0164)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::ActivityEntry` (partial — the
/// archived banner reads the actor; the moment comes off the item itself
/// as `archived_at`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct ArchiveEvent {
    actor_id: String,
}

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct MemberName {
    user_id: String,
    display_name: String,
}

/// Who archived this item, best-effort (KAIROS-T-0164).
///
/// The entity DTOs carry *when* an item was put away but not *by whom* —
/// the activity trail is where the actor lives (ADR-20: the trail records
/// that work existed and who touched it). So: the newest `delete` entry
/// for this entity, its actor resolved through the member directory.
///
/// Returns `None` on any failure. The banner is not optional; the name on
/// it is — a page that refused to say "archived" because the trail was
/// unreadable would be the worst of both worlds.
pub async fn fetch_archived_by(auth: Auth, item_id: String) -> Option<String> {
    let events: Page<ArchiveEvent> = get_json(
        auth,
        &format!("/api/activity?entity_id={item_id}&action=delete&limit=1"),
    )
    .await
    .ok()?;
    let event = events.items.into_iter().next()?;
    // COLLIERY-T-0258: each member, so that the name of a member after the
    // first 200 is on the banner too.
    let members: Vec<MemberName> = crate::api::get_all(auth, "/api/members").await.ok()?;
    let name = members
        .into_iter()
        .find(|member| member.user_id == event.actor_id)
        .map(|member| member.display_name)
        // An actor who has since left the org is still an actor: show the
        // id's head rather than dropping the attribution entirely.
        .unwrap_or_else(|| {
            let head = event.actor_id.get(..8).unwrap_or(&event.actor_id);
            format!("{head}…")
        });
    Some(name)
}

/// `GET /api/templates` → the picker's list: each template, page after
/// page (COLLIERY-T-0258).
pub async fn fetch_templates(auth: Auth) -> Result<Vec<TemplateSummary>, ApiError> {
    crate::api::get_all(auth, "/api/templates").await
}

/// `GET /api/templates/{id}` → content preview + declared metadata fields.
pub async fn fetch_template_detail(auth: Auth, id: String) -> Result<TemplateDetail, ApiError> {
    get_json(auth, &format!("/api/templates/{id}")).await
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Body of the content PATCH (mirror of:
/// `kairos_client::types::UpdateContentRequest`).
#[derive(Debug, Serialize)]
struct UpdateContentBody<'a> {
    title: &'a str,
    content: &'a str,
    version: i32,
}

/// `PATCH /api/{family}/{short_code}` — the A-0004 optimistic-concurrency
/// content edit. 409 becomes [`SaveError::Conflict`] with the
/// server-current entity for the merge UI.
pub async fn update_content(
    auth: Auth,
    family: Family,
    code: &str,
    title: &str,
    content: &str,
    version: i32,
) -> Result<ItemDetail, SaveError> {
    let path = format!("/api/{}/{code}", family.api_family());
    let body = UpdateContentBody {
        title,
        content,
        version,
    };
    patch_versioned(auth, &path, &body).await
}

/// The A-0004 versioned PATCH, generically: 409 parses `details.current`
/// into [`SaveError::Conflict`] for the merge UI; every other status maps
/// like a normal call. Shared by item content saves and team-page saves
/// (KAIROS-T-0086's generalized editor).
pub(crate) async fn patch_versioned<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, SaveError> {
    let response = send(auth, Verb::Patch, path, Some(body))
        .await
        .map_err(SaveError::Api)?;
    let status = response.status();
    if status == 401 {
        auth.expire();
    }
    if status == 409 {
        let text = response.text().await.unwrap_or_default();
        return match serde_json::from_str::<DetailedErrorEnvelope>(&text)
            .ok()
            .and_then(|envelope| envelope.error.details.current)
        {
            Some(current) => Err(SaveError::Conflict(current)),
            // A 409 without details.current would be a server contract
            // break; surface it as a plain error rather than guessing.
            None => Err(SaveError::Api(ApiError::Http {
                status,
                message: "The conflict response has no details.current.".to_string(),
                code: Some("CONFLICT".to_string()),
            })),
        };
    }
    if !(200..300).contains(&status) {
        return Err(SaveError::Api(error_from(status, response).await));
    }
    response.json::<T>().await.map_err(|e| {
        SaveError::Api(ApiError::Unknown(format!(
            "The page cannot read the response of {path}: {e}."
        )))
    })
}

/// `PATCH /api/{family}/{short_code}/metadata`: definition slug → value
/// (`None` clears — the A-0003 null-clears contract).
pub async fn update_metadata(
    auth: Auth,
    family: Family,
    code: &str,
    values: BTreeMap<String, Option<String>>,
) -> Result<Vec<MetadataValue>, ApiError> {
    #[derive(Serialize)]
    struct Body {
        values: BTreeMap<String, Option<String>>,
    }
    let path = format!("/api/{}/{code}/metadata", family.api_family());
    let response: ItemMetadata =
        send_json(auth, Verb::Patch, &path, Some(&Body { values })).await?;
    Ok(response.values)
}

/// Body of `POST /api/documents` (mirror of:
/// `kairos_client::types::CreateDocumentRequest`, partial — the
/// template-create flow's fields). A document has an owner
/// (COLLIERY-T-0269): the body has `board`, or `parent_short_code`, or
/// the two. A field with no value is not in the body.
#[derive(Debug, Serialize)]
pub struct CreateDocumentBody {
    pub title: String,
    pub template_id: String,
    /// The owner board, by slug. Required (COLLIERY-T-3109).
    pub board: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_short_code: Option<String>,
}

/// `POST /api/documents` — create-from-template. The document names its
/// owner board (COLLIERY-T-0269, required since COLLIERY-T-3109), and can
/// support a workflow parent.
pub async fn create_document(
    auth: Auth,
    body: &CreateDocumentBody,
) -> Result<ItemDetail, ApiError> {
    crate::api::post_json(auth, "/api/documents", body).await
}

/// Body of `PATCH /api/documents/{short_code}/board` (mirror of:
/// `kairos_client::types::SetDocumentBoardRequest`). `board` is required:
/// the owner board cannot be removed (COLLIERY-T-3109).
#[derive(Debug, Serialize)]
struct SetDocumentBoardBody<'a> {
    board: &'a str,
    /// COLLIERY-T-3101: the document also gets the next code of the new
    /// board. On the wire only when it is set (COLLIERY-T-3104).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    rename: bool,
}

/// `PATCH /api/documents/{short_code}/board` — change the owner board of a
/// document (COLLIERY-T-0269). With `rename`, the document gets the next
/// code of the new board, and the old code is retired (COLLIERY-T-3101).
/// Refusals the panel shows inline: 403 without `manage_documents` on the
/// two boards, 422 `RENAME_NOT_NEEDED` for a new board with the same
/// prefix, 404 for an unknown board.
pub async fn set_document_board(
    auth: Auth,
    code: &str,
    board: &str,
    rename: bool,
) -> Result<ItemDetail, ApiError> {
    send_json(
        auth,
        Verb::Patch,
        &format!("/api/documents/{code}/board"),
        Some(&SetDocumentBoardBody { board, rename }),
    )
    .await
}

/// Body of `POST /api/{family}/{short_code}/impacts` (mirror of:
/// `kairos_client::types_repositories::CreateImpactRequest`).
#[derive(Debug, Serialize)]
struct CreateImpactBody<'a> {
    repository: &'a str,
}

/// `POST /api/{family}/{short_code}/impacts` — say that a document or an
/// ADR impacts a repository (COLLIERY-T-0269).
pub async fn add_impact(
    auth: Auth,
    family: Family,
    code: &str,
    repository: &str,
) -> Result<Impact, ApiError> {
    crate::api::post_json(
        auth,
        &format!("/api/{}/{code}/impacts", family.api_family()),
        &CreateImpactBody { repository },
    )
    .await
}

/// `DELETE /api/{family}/{short_code}/impacts/{repository}` — remove an
/// `impacts` link (COLLIERY-T-0269).
pub async fn remove_impact(
    auth: Auth,
    family: Family,
    code: &str,
    repository: &str,
) -> Result<(), ApiError> {
    let path = format!(
        "/api/{}/{code}/impacts/{}",
        family.api_family(),
        crate::api::encode_component(repository)
    );
    let _: serde_json::Value = send_json(auth, Verb::Delete, &path, None::<&()>).await?;
    Ok(())
}

/// mirror of: `kairos_client::types_org::ItemTeamsResponse`
/// (KAIROS-T-0321).
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct ItemTeams {
    pub short_code: String,
    pub teams: Vec<crate::pages::boards::data::ItemTeam>,
}

/// `GET /api/{family}/{short_code}/teams` — the teams of an initiative or
/// a strategy: from its tasks and set by hand (KAIROS-T-0321).
pub async fn fetch_item_teams(
    auth: Auth,
    family: Family,
    code: &str,
) -> Result<ItemTeams, ApiError> {
    crate::api::get_json(auth, &format!("/api/{}/{code}/teams", family.api_family())).await
}

/// Body of `POST /api/{family}/{short_code}/teams` (mirror of:
/// `kairos_client::types_org::SetItemTeamRequest`).
#[derive(Debug, Serialize)]
struct SetItemTeamBody<'a> {
    team: &'a str,
}

/// `POST /api/{family}/{short_code}/teams` — set a team on an initiative
/// or a strategy by hand (KAIROS-T-0321).
pub async fn set_item_team(
    auth: Auth,
    family: Family,
    code: &str,
    team: &str,
) -> Result<crate::pages::boards::data::ItemTeam, ApiError> {
    crate::api::post_json(
        auth,
        &format!("/api/{}/{code}/teams", family.api_family()),
        &SetItemTeamBody { team },
    )
    .await
}

/// `DELETE /api/{family}/{short_code}/teams/{team}` — clear a team that
/// is set by hand (KAIROS-T-0321).
pub async fn clear_item_team(
    auth: Auth,
    family: Family,
    code: &str,
    team: &str,
) -> Result<(), ApiError> {
    let path = format!(
        "/api/{}/{code}/teams/{}",
        family.api_family(),
        crate::api::encode_component(team)
    );
    let _: serde_json::Value = send_json(auth, Verb::Delete, &path, None::<&()>).await?;
    Ok(())
}

/// Body of `POST /api/tasks/{short_code}/move` (mirror of:
/// `kairos_client::types::MoveTaskRequest`) — the target board by slug or
/// UUID.
#[derive(Debug, Serialize)]
struct MoveTaskBody<'a> {
    board: &'a str,
    /// COLLIERY-T-3101: the task also gets the next code of the board.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    rename: bool,
}

/// `POST /api/tasks/{short_code}/move` — re-home a task onto another
/// DELIVERY board (KAIROS-I-0012 D2; mirror of
/// `KairosClient::move_task`). It lands in the target's entry column and
/// follows the target's team. Refusals the panel shows inline: 422
/// `SAME_BOARD` / `NOT_DELIVERY_BOARD` / `NO_ENTRY_COLUMN`, 403 without
/// `manage_tasks` on BOTH boards, 404 for an unknown board. The server no
/// longer refuses a move because of the task's repository (COLLIERY-T-0217).
///
/// With `rename` (COLLIERY-T-3101) the task also gets the next code of the
/// target board, and the response has the new code.
pub async fn move_task(
    auth: Auth,
    code: &str,
    board: &str,
    rename: bool,
) -> Result<ItemDetail, ApiError> {
    crate::api::post_json(
        auth,
        &format!("/api/tasks/{code}/move"),
        &MoveTaskBody { board, rename },
    )
    .await
}

/// mirror of: `kairos_client::types::RestoreResponse` (KAIROS-T-0160).
/// `still_archived_*` are the item's descendants that stayed away: a
/// restore puts back the one item it was asked for, never a cascade.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RestoreOutcome {
    pub short_code: String,
    pub still_archived_count: i64,
    #[serde(default)]
    pub still_archived_short_codes: Vec<String>,
}

/// Outcome of a restore attempt. A refusal is not a dead end either — it
/// names what the item needs and no longer has, so the page can say it in
/// words instead of showing a raw 422.
pub enum RestoreError {
    /// 422 `RESTORE_BLOCKED`: `details.missing`, already human-readable
    /// server-side ("its board column (removed)", "its owning team
    /// (deleted)", …).
    Blocked(Vec<String>),
    /// Anything else (403 without the capability, 404, network), mapped
    /// like every other call.
    Api(ApiError),
}

/// `POST /api/{family}/{short_code}/restore` — put an archived item back
/// (KAIROS-T-0160, ADR-20). Needs the same `manage_<family>` capability on
/// the item's board that archiving it needed; the server is the authority
/// and a 403 surfaces inline.
pub async fn restore_item(
    auth: Auth,
    family: Family,
    code: &str,
) -> Result<RestoreOutcome, RestoreError> {
    let path = format!("/api/{}/{code}/restore", family.api_family());
    let response = send(auth, Verb::Post, &path, None::<&()>)
        .await
        .map_err(RestoreError::Api)?;
    let status = response.status();
    if status == 401 {
        auth.expire();
    }
    if status == 422 {
        let text = response.text().await.unwrap_or_default();
        let envelope: Option<DetailedErrorEnvelope> = serde_json::from_str(&text).ok();
        return match envelope {
            Some(envelope) if envelope.error.code == "RESTORE_BLOCKED" => {
                Err(RestoreError::Blocked(envelope.error.details.missing))
            }
            Some(envelope) => Err(RestoreError::Api(ApiError::Http {
                status,
                message: envelope.error.message,
                code: Some(envelope.error.code),
            })),
            None => Err(RestoreError::Api(ApiError::Http {
                status,
                message: text,
                code: None,
            })),
        };
    }
    if !(200..300).contains(&status) {
        return Err(RestoreError::Api(error_from(status, response).await));
    }
    response.json::<RestoreOutcome>().await.map_err(|e| {
        RestoreError::Api(ApiError::Unknown(format!(
            "The page cannot read the response of {path}: {e}."
        )))
    })
}

/// `DELETE /api/{family}/{short_code}` — A-0001 soft delete; the response
/// reports the cascade.
pub async fn delete_item(
    auth: Auth,
    family: Family,
    code: &str,
) -> Result<DeleteOutcome, ApiError> {
    let path = format!("/api/{}/{code}", family.api_family());
    send_json(auth, Verb::Delete, &path, None::<&()>).await
}

/// One-line text for a *write* failure (loads use `<ErrorState/>`; writes
/// surface inline via `Alert`, per the conventions' error rules).
pub fn error_text(error: &ApiError) -> String {
    match error {
        ApiError::Http {
            status,
            message,
            code,
        } => match code {
            Some(code) => format!("{code} ({status}): {message}"),
            None => format!("HTTP {status}: {message}"),
        },
        ApiError::Network => "The page cannot connect to the server.".to_string(),
        ApiError::Unknown(message) => message.clone(),
    }
}

// ---------------------------------------------------------------------------
// Verb plumbing (same shape as `api::get_json`)
// ---------------------------------------------------------------------------

/// The verbs this module drives directly. PATCH and DELETE because the
/// shared `api.rs` does not provide them; POST because the restore call
/// needs the RAW response to read `details.missing` off a 422, which the
/// shared helper flattens away.
#[derive(Clone, Copy)]
enum Verb {
    Post,
    Patch,
    Delete,
}

/// Build + send one authenticated JSON request; no status handling yet.
async fn send<B: Serialize>(
    auth: Auth,
    verb: Verb,
    path: &str,
    body: Option<&B>,
) -> Result<gloo_net::http::Response, ApiError> {
    let mut request = match verb {
        Verb::Post => gloo_net::http::Request::post(path),
        Verb::Patch => gloo_net::http::Request::patch(path),
        Verb::Delete => gloo_net::http::Request::delete(path),
    };
    if let Some(token) = auth.token() {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request.json(body).map_err(|e| {
            ApiError::Unknown(format!(
                "The page cannot write the request for {path}: {e}."
            ))
        })?,
        None => request.build().map_err(|e| {
            ApiError::Unknown(format!("The page cannot make the request for {path}: {e}."))
        })?,
    };
    request.send().await.map_err(|_| ApiError::Network)
}

/// One authenticated JSON round-trip with the standard status handling
/// (401 → expire, non-2xx → envelope-mapped `ApiError`).
async fn send_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    verb: Verb,
    path: &str,
    body: Option<&B>,
) -> Result<T, ApiError> {
    let response = send(auth, verb, path, body).await?;
    let status = response.status();
    if status == 401 {
        auth.expire();
    }
    if !(200..300).contains(&status) {
        return Err(error_from(status, response).await);
    }
    response.json::<T>().await.map_err(|e| {
        ApiError::Unknown(format!("The page cannot read the response of {path}: {e}."))
    })
}

/// Non-2xx → `ApiError` via the S-0005 envelope (the `api.rs` mapping,
/// reproduced for the verbs that live here).
async fn error_from(status: u16, response: gloo_net::http::Response) -> ApiError {
    let body = response.text().await.unwrap_or_default();
    let envelope: Option<DetailedErrorEnvelope> = serde_json::from_str(&body).ok();
    let (message, code) = match envelope {
        Some(envelope) => (envelope.error.message, Some(envelope.error.code)),
        None => (body, None),
    };
    ApiError::Http {
        status,
        message,
        code,
    }
}

// ---------------------------------------------------------------------------
// Forge links (KAIROS-T-0100)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_forge::ItemLink` (partial — the
/// Development panel does not render `id` or `item_id`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemLink {
    /// `branch|pull_request`.
    pub kind: String,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    /// Browser URL on the forge.
    pub url: String,
    /// `open|merged|closed|draft`.
    pub state: String,
    pub author: String,
    /// `github|gitlab`.
    pub forge: String,
    /// `owner/repo`.
    pub repo_full_name: String,
}

/// `GET /api/{family}/{code}/links` — server-ordered (PRs first, newest
/// first), so this never re-sorts.
pub async fn fetch_links(
    auth: Auth,
    family: Family,
    code: String,
) -> Result<Vec<ItemLink>, ApiError> {
    get_json(auth, &format!("/api/{}/{code}/links", family.api_family())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ItemLink` decodes the KAIROS-T-0100 wire shape.
    #[test]
    fn item_link_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "l1",
            "item_id": "i1",
            "kind": "pull_request",
            "external_id": "42",
            "title": "Password-less auth",
            "url": "https://github.com/acme/payments-api/pull/42",
            "state": "merged",
            "author": "dylan",
            "forge": "github",
            "repo_full_name": "acme/payments-api",
            "forge_updated_at": "2026-09-01T12:00:00Z"
        });
        let link: ItemLink = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(link.state, "merged");
        assert_eq!(link.repo_full_name, "acme/payments-api");
    }

    /// Short-code → family across all five letters, multi-segment
    /// prefixes included; junk is rejected.
    #[test]
    fn family_parses_from_short_codes() {
        assert_eq!(Family::of_short_code("DEMO-S-0001"), Some(Family::Strategy));
        assert_eq!(
            Family::of_short_code("DEMO-I-0002"),
            Some(Family::Initiative)
        );
        assert_eq!(Family::of_short_code("DEMO-T-0010"), Some(Family::Task));
        assert_eq!(Family::of_short_code("DEMO-D-0001"), Some(Family::Document));
        assert_eq!(Family::of_short_code("DEMO-A-0002"), Some(Family::Adr));
        // Slugs may contain '-': the type letter reads from the end.
        assert_eq!(Family::of_short_code("ACME-CO-T-0001"), Some(Family::Task));
        assert_eq!(Family::of_short_code(""), None);
        assert_eq!(Family::of_short_code("DEMO-X-0001"), None);
        assert_eq!(Family::of_short_code("T-0001"), None); // no prefix
        assert_eq!(Family::of_short_code("DEMO-T-12ab"), None);
        assert_eq!(Family::of_short_code("not a code"), None);
    }

    /// The union mirror decodes a full Task body (field-name lock).
    #[test]
    fn item_mirror_decodes_task_shape() {
        let body = serde_json::json!({
            "id": "3d9f2f5e-8f5c-4f4e-b7a3-0f1e2d3c4b5a",
            "short_code": "DEMO-T-0002",
            "title": "Password-less email auth",
            "content": "Magic-link issue + verify endpoints.",
            "board_id": "b1a2c3d4-0000-0000-0000-000000000001",
            "column_id": "c1a2c3d4-0000-0000-0000-000000000002",
            "task_type": "task",
            "work_class": "support",
            "team_id": null,
            "version": 3,
            "created_by": "u1",
            "updated_by": "u2",
            "created_at": "2026-07-14T12:00:00+00:00",
            "updated_at": "2026-07-14T12:30:00+00:00"
        });
        let item: ItemDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(item.version, 3);
        assert_eq!(item.task_type.as_deref(), Some("task"));
        assert_eq!(item.work_class.as_deref(), Some("support"));
        assert!(item.board_id.is_some());
        assert!(item.decision_date.is_none());
    }

    /// The board mirror decodes the `GET /api/boards/{id}` shape the move
    /// control reads (KAIROS-T-0075) — flattened board fields, columns,
    /// transitions.
    #[test]
    fn board_mirror_decodes_transitions_and_team() {
        let body = serde_json::json!({
            "id": "b-1", "name": "Platform Delivery", "slug": "platform-delivery",
            "board_level": "delivery", "team_id": "t-1",
            "created_at": "x", "updated_at": "x",
            "columns": [
                {"id": "c-1", "board_id": "b-1", "name": "Todo", "position": 1,
                 "created_at": "x", "updated_at": "x"},
                {"id": "c-2", "board_id": "b-1", "name": "Active", "position": 2,
                 "created_at": "x", "updated_at": "x"}
            ],
            "transitions": [
                {"id": "t-1", "board_id": "b-1",
                 "from_column_id": "c-1", "to_column_id": "c-2"}
            ]
        });
        let board: BoardInfo = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(board.slug, "platform-delivery");
        assert_eq!(board.team_id.as_deref(), Some("t-1"));
        assert_eq!(board.transitions[0].from_column_id, "c-1");
        assert_eq!(board.transitions[0].to_column_id, "c-2");
    }

    /// The union mirror decodes a Document body (no board fields at all).
    #[test]
    fn item_mirror_decodes_document_shape() {
        let body = serde_json::json!({
            "id": "9e8d7c6b-0000-0000-0000-000000000009",
            "short_code": "DEMO-D-0001",
            "title": "PRD: Portal sign-up flow",
            "content": "## Summary\n",
            "template_id": "t1",
            "lifecycle": "published",
            "version": 1,
            "created_by": "u1",
            "updated_by": "u1",
            "created_at": "2026-07-14T12:00:00+00:00",
            "updated_at": "2026-07-14T12:00:00+00:00"
        });
        let item: ItemDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(item.board_id, None);
        assert_eq!(item.column_id, None);
        assert_eq!(item.lifecycle.as_deref(), Some("published"));
    }

    /// The 409 envelope parse finds `details.current` whether it is the
    /// full entity (PATCH handler path) or the minimal fallback shape.
    #[test]
    fn conflict_envelope_extracts_current() {
        let full = serde_json::json!({
            "error": {
                "code": "CONFLICT",
                "message": "The request has the version 2, and the current version is 4. \
                            Get the item again, and make the edit on the current version.",
                "details": { "current": {
                    "id": "x", "short_code": "DEMO-T-0002",
                    "title": "Their title", "content": "their content",
                    "board_id": "b", "column_id": "c", "task_type": "task",
                    "team_id": null, "version": 4, "created_by": "u",
                    "updated_by": "u", "created_at": "t", "updated_at": "t"
                }}
            }
        });
        let envelope: DetailedErrorEnvelope = serde_json::from_value(full).expect("parses");
        let current = envelope.error.details.current.expect("has current");
        assert_eq!(current.version, 4);
        assert_eq!(current.title, "Their title");
        assert_eq!(envelope.error.code, "CONFLICT");

        let minimal = serde_json::json!({
            "error": {
                "code": "CONFLICT", "message": "The request has the version 2.",
                "details": { "current": {"version": 7, "title": "t", "content": "c"} }
            }
        });
        let envelope: DetailedErrorEnvelope = serde_json::from_value(minimal).expect("parses");
        assert_eq!(envelope.error.details.current.expect("current").version, 7);

        let none = serde_json::json!({
            "error": {"code": "VALIDATION", "message": "nope", "details": {}}
        });
        let envelope: DetailedErrorEnvelope = serde_json::from_value(none).expect("parses");
        assert!(envelope.error.details.current.is_none());
    }

    /// COLLIERY-T-3104: the owner-board body has `rename` only when it is
    /// set, and `board` always (COLLIERY-T-3109: it cannot be removed).
    #[test]
    fn the_owner_board_body_has_rename_only_when_it_is_set() {
        let body = serde_json::to_value(SetDocumentBoardBody {
            board: "web-delivery",
            rename: false,
        })
        .expect("serializes");
        assert_eq!(body, serde_json::json!({"board": "web-delivery"}));
        let body = serde_json::to_value(SetDocumentBoardBody {
            board: "web-delivery",
            rename: true,
        })
        .expect("serializes");
        assert_eq!(
            body,
            serde_json::json!({"board": "web-delivery", "rename": true})
        );
    }

    /// The move body carries the target board under the exact wire name
    /// the server takes (`board`: slug or UUID), and the 200 decodes into
    /// the item mirror with the NEW placement (KAIROS-I-0012 D2).
    #[test]
    fn move_body_serializes_and_response_carries_new_placement() {
        let body = serde_json::to_value(MoveTaskBody {
            board: "web-delivery",
            rename: false,
        })
        .expect("serializes");
        assert_eq!(body, serde_json::json!({"board": "web-delivery"}));
        // COLLIERY-T-3101: `rename` goes on the wire only when it is set.
        let body = serde_json::to_value(MoveTaskBody {
            board: "web-delivery",
            rename: true,
        })
        .expect("serializes");
        assert_eq!(
            body,
            serde_json::json!({"board": "web-delivery", "rename": true})
        );

        let moved = serde_json::json!({
            "id": "1b2c3d4e-0000-0000-0000-000000000001",
            "short_code": "DEMO-T-0004",
            "title": "Moved task",
            "content": "",
            "board_id": "b-2",
            "column_id": "c-entry",
            "team_id": "t-2",
            "task_type": "task",
            "work_class": "planned",
            "version": 2,
            "created_by": "u1",
            "updated_by": "u1",
            "created_at": "2026-09-23T12:00:00+00:00",
            "updated_at": "2026-09-23T12:05:00+00:00"
        });
        let item: ItemDetail = serde_json::from_value(moved).expect("mirror decodes");
        assert_eq!(item.board_id.as_deref(), Some("b-2"));
        assert_eq!(item.column_id.as_deref(), Some("c-entry"));
    }

    /// KAIROS-T-0154/T-0164: an ARCHIVED item is served by short code
    /// like any other, carrying `archived_at`; a live one carries no such
    /// field at all (not a null, absent), and the mirror must read both.
    #[test]
    fn item_mirror_decodes_the_archived_state() {
        let live = serde_json::json!({
            "id": "3d9f2f5e-8f5c-4f4e-b7a3-0f1e2d3c4b5a",
            "short_code": "DEMO-T-0002", "title": "Live", "content": "",
            "version": 1, "created_at": "t", "updated_at": "t"
        });
        let item: ItemDetail = serde_json::from_value(live).expect("mirror decodes");
        assert_eq!(item.archived_at, None, "a live item is not archived");

        let mut archived = serde_json::json!({
            "id": "3d9f2f5e-8f5c-4f4e-b7a3-0f1e2d3c4b5a",
            "short_code": "DEMO-T-0002", "title": "Put away", "content": "",
            "version": 1, "created_at": "t", "updated_at": "t"
        });
        archived["archived_at"] = serde_json::json!("2026-09-23T11:30:07.479107Z");
        let item: ItemDetail = serde_json::from_value(archived).expect("mirror decodes");
        assert_eq!(
            item.archived_at.as_deref(),
            Some("2026-09-23T11:30:07.479107Z")
        );
        assert_eq!(item.id, "3d9f2f5e-8f5c-4f4e-b7a3-0f1e2d3c4b5a");
    }

    /// KAIROS-T-0158/T-0163: relationship lists are archived-inclusive
    /// and mark the archived end, so the panel's mirror has to carry the
    /// marker. It compiled fine without it and silently rendered an
    /// archived neighbour as live — the trap this test exists to hold
    /// shut.
    #[test]
    fn related_item_mirror_carries_the_archived_marker() {
        let body = serde_json::json!({
            "short_code": "DEMO-I-0002",
            "outgoing": [{"relationship": "parent", "items": [
                {"relationship_id": "e1", "id": "x", "short_code": "DEMO-T-0001",
                 "entity_type": "task", "title": "Still going"},
                {"relationship_id": "e2", "id": "y", "short_code": "DEMO-T-0002",
                 "entity_type": "task", "title": "Finished, put away",
                 "archived_at": "2026-09-23T11:30:07.479107Z"}
            ]}],
            "incoming": []
        });
        let rels: ItemRelationships = serde_json::from_value(body).expect("mirror decodes");
        let children = &rels.outgoing[0].items;
        assert_eq!(children[0].archived_at, None, "a live neighbour is bare");
        assert_eq!(
            children[1].archived_at.as_deref(),
            Some("2026-09-23T11:30:07.479107Z")
        );
    }

    /// COLLIERY-T-0214: the same trap, for `done`. The wire carries
    /// `done: true` for a neighbour in a terminal column and nothing
    /// otherwise; a mirror without the field compiles and renders a
    /// finished blocker as one still in the way.
    #[test]
    fn related_item_mirror_carries_the_done_marker() {
        let body = serde_json::json!({
            "short_code": "DEMO-T-0003",
            "outgoing": [],
            "incoming": [{"relationship": "blocks", "items": [
                {"relationship_id": "e1", "id": "x", "short_code": "DEMO-T-0001",
                 "entity_type": "task", "title": "Still in the way"},
                {"relationship_id": "e2", "id": "y", "short_code": "DEMO-T-0002",
                 "entity_type": "task", "title": "Finished", "done": true}
            ]}]
        });
        let rels: ItemRelationships = serde_json::from_value(body).expect("mirror decodes");
        let blockers = &rels.incoming[0].items;
        assert!(!blockers[0].done, "an open blocker is bare");
        assert!(blockers[1].done, "a completed blocker is marked");
    }

    /// KAIROS-T-0161/T-0164: with `include_removed_columns=true` the board
    /// carries removed columns too, marked — that is how an archived
    /// card's placement keeps its NAME instead of decaying to "unknown
    /// column".
    #[test]
    fn board_mirror_decodes_a_removed_column() {
        let body = serde_json::json!({
            "id": "b-1", "name": "Strategy", "slug": "strategy",
            "board_level": "strategy", "team_id": null,
            "created_at": "x", "updated_at": "x",
            "columns": [
                {"id": "c-1", "board_id": "b-1", "name": "Todo", "position": 1,
                 "created_at": "x", "updated_at": "x"},
                {"id": "c-9", "board_id": "b-1", "name": "Retired", "position": 6,
                 "created_at": "x", "updated_at": "x",
                 "removed_at": "2026-09-23T11:00:00.000000Z"}
            ],
            "transitions": []
        });
        let board: BoardInfo = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(board.columns[0].removed_at, None, "a live column");
        assert_eq!(
            board.columns[1].removed_at.as_deref(),
            Some("2026-09-23T11:00:00.000000Z"),
            "the removed column arrives, marked"
        );
    }

    /// The restore contract (KAIROS-T-0160): the 200 reports what stayed
    /// away, and the 422 refusal carries `details.missing` — the names the
    /// page renders instead of an error code.
    #[test]
    fn restore_response_and_refusal_decode() {
        let ok = serde_json::json!({
            "short_code": "DEMO-T-0004",
            "still_archived_count": 2,
            "still_archived_short_codes": ["DEMO-T-0005", "DEMO-D-0003"]
        });
        let outcome: RestoreOutcome = serde_json::from_value(ok).expect("decodes");
        assert_eq!(outcome.still_archived_count, 2);
        assert_eq!(outcome.still_archived_short_codes.len(), 2);

        let blocked = serde_json::json!({
            "error": {
                "code": "RESTORE_BLOCKED",
                "message": "The server cannot restore DEMO-S-0002. The item needs its board \
                            column (removed). Move the item to a place that exists, or \
                            first restore the thing that the item needs.",
                "details": {"missing": ["its board column (removed)"]}
            }
        });
        let envelope: DetailedErrorEnvelope = serde_json::from_value(blocked).expect("parses");
        assert_eq!(envelope.error.code, "RESTORE_BLOCKED");
        assert_eq!(
            envelope.error.details.missing,
            ["its board column (removed)"]
        );
        // A refusal with no details is still a refusal, never a panic.
        let bare = serde_json::json!({
            "error": {"code": "RESTORE_BLOCKED", "message": "nope", "details": {}}
        });
        let envelope: DetailedErrorEnvelope = serde_json::from_value(bare).expect("parses");
        assert!(envelope.error.details.missing.is_empty());
    }

    /// The metadata PATCH body serializes `None` as JSON null (the A-0003
    /// "null clears" contract).
    #[test]
    fn metadata_body_serializes_null_clears() {
        #[derive(Serialize)]
        struct Body {
            values: BTreeMap<String, Option<String>>,
        }
        let mut values = BTreeMap::new();
        values.insert("priority".to_string(), Some("high".to_string()));
        values.insert("due_date".to_string(), None);
        let body = serde_json::to_value(Body { values }).expect("serializes");
        assert_eq!(body["values"]["priority"], "high");
        assert!(body["values"]["due_date"].is_null());
    }
}

/// One pending edge proposal, as the item page shows it (KAIROS-T-0192).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeProposal {
    /// The proposal's id, for confirm/reject.
    pub id: String,
    /// The proposed edge's source, by short code.
    pub source: String,
    /// The proposed edge's target, by short code.
    pub target: String,
    /// `parent` or `blocks`.
    pub relationship: String,
    /// What was claimed.
    pub claim: String,
    /// Why, verbatim — what the agent saw, not a summary. A person deciding
    /// needs the evidence, not a label.
    pub why: String,
}

/// `GET /api/items/{short_code}/proposals` → edges an agent has suggested and
/// nobody has ruled on yet (KAIROS-A-0021 rule 6).
pub async fn fetch_edge_proposals(auth: Auth, code: String) -> Result<Vec<EdgeProposal>, ApiError> {
    get_json(auth, &format!("/api/items/{code}/proposals")).await
}

/// `POST /api/proposals/{id}/confirm` — create the edge.
pub async fn confirm_edge_proposal(auth: Auth, id: String) -> Result<EdgeProposal, ApiError> {
    crate::api::post_empty(auth, &format!("/api/proposals/{id}/confirm")).await
}

/// One possibly-related item (KAIROS-T-0195).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RelatedProposal {
    /// The related item's short code.
    pub short_code: String,
    /// Its title.
    pub title: String,
    /// Its entity type.
    pub entity_type: String,
    /// `implicit_dependency` | `near_duplicate` | `prior_art`.
    pub claim: String,
    /// Fused rank score. Comparable within this response and NOWHERE else — it
    /// is not a percentage and not a confidence, which is why the panel does not
    /// render it as one.
    pub score: f32,
    /// Why, in a sentence a person can disagree with.
    pub why: String,
}

/// The answer to "what is related to this?" (KAIROS-T-0195).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RelatedWork {
    /// Bounded, best first. Empty means one search came up short — NOT proof
    /// that nothing is related.
    pub proposals: Vec<RelatedProposal>,
    /// Whether vector search contributed. False is a **degraded** answer: text
    /// only, so it will have missed work phrased differently.
    pub vector: bool,
    /// A sentence saying which searches ran.
    pub note: String,
}

/// `GET /api/items/{short_code}/related` → what might be related, as proposals.
///
/// A `503` here is not a failure: it means this deployment has embeddings turned
/// off. The caller distinguishes it so the panel can stay silent rather than
/// showing an error for a feature nobody enabled.
pub async fn fetch_related_work(auth: Auth, code: String) -> Result<RelatedWork, ApiError> {
    get_json(auth, &format!("/api/items/{code}/related")).await
}

/// `POST /api/proposals/{id}/reject` — recorded, not erased.
pub async fn reject_edge_proposal(auth: Auth, id: String) -> Result<EdgeProposal, ApiError> {
    crate::api::post_empty(auth, &format!("/api/proposals/{id}/reject")).await
}
