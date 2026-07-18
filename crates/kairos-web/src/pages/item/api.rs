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
    /// Board placement (absent for documents, optional for ADRs).
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub column_id: Option<String>,
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
    #[serde(default)]
    pub decision_maker: Option<String>,
    #[serde(default)]
    pub decision_date: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardInfo {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub columns: Vec<BoardColumnInfo>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumnInfo {
    pub id: String,
    pub name: String,
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

/// mirror of: `kairos_client::types::DeleteResponse` (the A-0001 soft
/// delete + cascade report).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DeleteOutcome {
    pub short_code: String,
    pub cascade_count: i64,
    #[serde(default)]
    pub cascaded_short_codes: Vec<String>,
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

/// The structured extras this page understands (`current` on 409).
#[derive(Debug, Default, Deserialize)]
struct ErrorDetails {
    #[serde(default)]
    current: Option<CurrentVersion>,
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

/// `GET /api/boards/{id}` → board name/slug + columns (for the
/// board/column display).
pub async fn fetch_board(auth: Auth, board_id: String) -> Result<BoardInfo, ApiError> {
    get_json(auth, &format!("/api/boards/{board_id}")).await
}

/// `GET /api/metadata-definitions` (all of them — the typed editors render
/// every definition, valued or not).
pub async fn fetch_definitions(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError> {
    let page: Page<MetadataDefinition> =
        get_json(auth, "/api/metadata-definitions?limit=200").await?;
    Ok(page.items)
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

/// `GET /api/templates` → the picker's list.
pub async fn fetch_templates(auth: Auth) -> Result<Vec<TemplateSummary>, ApiError> {
    let page: Page<TemplateSummary> = get_json(auth, "/api/templates?limit=200").await?;
    Ok(page.items)
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
    let response = send(auth, Verb::Patch, &path, Some(&body))
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
                message: "conflict response carried no details.current".to_string(),
                code: Some("CONFLICT".to_string()),
            })),
        };
    }
    if !(200..300).contains(&status) {
        return Err(SaveError::Api(error_from(status, response).await));
    }
    response
        .json::<ItemDetail>()
        .await
        .map_err(|e| SaveError::Api(ApiError::Unknown(format!("decoding {path}: {e}"))))
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
/// template-create flow's fields).
#[derive(Debug, Serialize)]
pub struct CreateDocumentBody {
    pub title: String,
    pub template_id: String,
    pub parent_short_code: String,
}

/// `POST /api/documents` — create-from-template, attached to a workflow
/// parent (the T-0018 contract).
pub async fn create_document(
    auth: Auth,
    body: &CreateDocumentBody,
) -> Result<ItemDetail, ApiError> {
    crate::api::post_json(auth, "/api/documents", body).await
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
        ApiError::Network => "Cannot reach the server.".to_string(),
        ApiError::Unknown(message) => message.clone(),
    }
}

// ---------------------------------------------------------------------------
// Verb plumbing (same shape as `api::get_json`)
// ---------------------------------------------------------------------------

/// The two verbs the shared `api.rs` does not provide yet.
#[derive(Clone, Copy)]
enum Verb {
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
        Verb::Patch => gloo_net::http::Request::patch(path),
        Verb::Delete => gloo_net::http::Request::delete(path),
    };
    if let Some(token) = auth.token() {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .json(body)
            .map_err(|e| ApiError::Unknown(format!("encoding {path}: {e}")))?,
        None => request
            .build()
            .map_err(|e| ApiError::Unknown(format!("building {path}: {e}")))?,
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
    response
        .json::<T>()
        .await
        .map_err(|e| ApiError::Unknown(format!("decoding {path}: {e}")))
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(item.board_id.is_some());
        assert!(item.decision_date.is_none());
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
            "version": 1,
            "created_by": "u1",
            "updated_by": "u1",
            "created_at": "2026-07-14T12:00:00+00:00",
            "updated_at": "2026-07-14T12:00:00+00:00"
        });
        let item: ItemDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(item.board_id, None);
        assert_eq!(item.column_id, None);
    }

    /// The 409 envelope parse finds `details.current` whether it is the
    /// full entity (PATCH handler path) or the minimal fallback shape.
    #[test]
    fn conflict_envelope_extracts_current() {
        let full = serde_json::json!({
            "error": {
                "code": "CONFLICT",
                "message": "version mismatch: expected 2, current is 4",
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
                "code": "CONFLICT", "message": "version mismatch",
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
