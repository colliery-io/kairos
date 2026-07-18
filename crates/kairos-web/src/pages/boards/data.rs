//! Boards data layer (KAIROS-T-0040): partial mirror DTOs + fetch calls
//! for the board list / board view, per the conventions' data-layer rules
//! (docs/gui-conventions.md §4 — mirrors declare only what the view reads;
//! every mirror carries a `mirror of:` line and a decode test).

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api::{get_json, post_json};
use crate::auth::Auth;

// ---- mirrors: boards ------------------------------------------------------

/// mirror of: `kairos_client::types_org::Board` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// `strategy` | `initiative` | `delivery` | `adr`.
    pub board_level: String,
    #[serde(default)]
    pub team_id: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub name: String,
    pub position: i32,
}

/// mirror of: `kairos_client::types_org::BoardTransition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardTransition {
    pub from_column_id: String,
    pub to_column_id: String,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (the board's own
/// fields are `#[serde(flatten)]`ed to the top level on the wire).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardDetail {
    #[serde(flatten)]
    pub board: Board,
    pub columns: Vec<BoardColumn>,
    pub transitions: Vec<BoardTransition>,
}

/// mirror of: `kairos_client::types::ListEnvelope<Board>` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardListEnvelope {
    pub items: Vec<Board>,
}

// ---- mirrors: the grouped items view --------------------------------------

/// mirror of: `kairos_client::types::Strategy` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Strategy {
    pub short_code: String,
    pub title: String,
}

/// mirror of: `kairos_client::types::Initiative` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Initiative {
    pub short_code: String,
    pub title: String,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub is_bucket: bool,
    #[serde(default)]
    pub bucket_type: Option<String>,
}

/// mirror of: `kairos_client::types::Task` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Task {
    pub short_code: String,
    pub title: String,
    /// `task` | `bug` | `tech_debt`.
    pub task_type: String,
}

/// mirror of: `kairos_client::types::Adr` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Adr {
    pub short_code: String,
    pub title: String,
    #[serde(default)]
    pub decision_date: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardColumnItems`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumnItems {
    pub column: BoardColumn,
    pub strategies: Vec<Strategy>,
    pub initiatives: Vec<Initiative>,
    pub tasks: Vec<Task>,
    pub adrs: Vec<Adr>,
}

/// mirror of: `kairos_client::types_org::BoardItemsResponse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardItemsResponse {
    pub board: Board,
    pub columns: Vec<BoardColumnItems>,
}

// ---- mirrors: templates + events ------------------------------------------

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
}

/// mirror of: `kairos_client::types::ListEnvelope<Template>` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateListEnvelope {
    pub items: Vec<Template>,
}

/// mirror of: `kairos_client::types_events::ThinEvent` (partial — the view
/// only needs to know "something on this board changed" and re-fetches
/// through REST per A-0005 §5; events carry no payloads).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ThinEvent {
    pub event: String,
}

// ---- the whole board view in one fetch -------------------------------------

/// Everything the board view renders: configuration (columns +
/// transitions) and the grouped items.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardView {
    pub detail: BoardDetail,
    pub items: BoardItemsResponse,
}

/// The board list (one page is plenty for v1 — the demo tenant has 5).
pub async fn list_boards(auth: Auth) -> Result<Vec<Board>, ApiError> {
    let envelope: BoardListEnvelope = get_json(auth, "/api/boards?limit=100").await?;
    Ok(envelope.items)
}

/// Resolve a route param (board slug, or id as a fallback) against the
/// board list, then load the full [`BoardView`].
pub async fn load_board_view(auth: Auth, param: &str) -> Result<BoardView, ApiError> {
    let boards = list_boards(auth).await?;
    let board = boards
        .into_iter()
        .find(|b| b.slug == param || b.id == param)
        .ok_or(ApiError::Http {
            status: 404,
            message: format!("no board named {param:?}"),
            code: Some("NOT_FOUND".to_string()),
        })?;
    let detail: BoardDetail = get_json(auth, &format!("/api/boards/{}", board.id)).await?;
    let items: BoardItemsResponse =
        get_json(auth, &format!("/api/boards/{}/items", board.id)).await?;
    Ok(BoardView { detail, items })
}

/// Templates for the document create flow.
pub async fn list_templates(auth: Auth) -> Result<Vec<Template>, ApiError> {
    let envelope: TemplateListEnvelope = get_json(auth, "/api/templates?limit=100").await?;
    Ok(envelope.items)
}

// ---- mutations --------------------------------------------------------------

/// The four board-item entity kinds (documents are off-board, S-0005).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Strategy,
    Initiative,
    Task,
    Adr,
}

impl EntityKind {
    /// The S-0005 URL family (`/api/{family}/{short_code}/…`).
    pub fn api_family(self) -> &'static str {
        match self {
            EntityKind::Strategy => "strategies",
            EntityKind::Initiative => "initiatives",
            EntityKind::Task => "tasks",
            EntityKind::Adr => "adrs",
        }
    }

    /// The card label.
    pub fn label(self) -> &'static str {
        match self {
            EntityKind::Strategy => "strategy",
            EntityKind::Initiative => "initiative",
            EntityKind::Task => "task",
            EntityKind::Adr => "adr",
        }
    }

    /// The entity type a board level's create flow produces
    /// (KAIROS-A-0002: one item family per board level).
    pub fn for_board_level(level: &str) -> Option<EntityKind> {
        match level {
            "strategy" => Some(EntityKind::Strategy),
            "initiative" => Some(EntityKind::Initiative),
            "delivery" => Some(EntityKind::Task),
            "adr" => Some(EntityKind::Adr),
            _ => None,
        }
    }
}

/// mirror of: `kairos_client::types::TransitionRequest`.
#[derive(Debug, Serialize)]
struct TransitionRequest<'a> {
    to_column_id: &'a str,
}

/// `POST /api/{family}/{short_code}/transition`. The response body (the
/// updated entity) is discarded: server state is re-fetched wholesale.
pub async fn transition(
    auth: Auth,
    kind: EntityKind,
    short_code: &str,
    to_column_id: &str,
) -> Result<(), ApiError> {
    let path = format!("/api/{}/{}/transition", kind.api_family(), short_code);
    let _: serde_json::Value = post_json(auth, &path, &TransitionRequest { to_column_id }).await?;
    Ok(())
}

/// The create-from-column form data; [`create_item`] maps it onto the
/// right S-0005 request per entity kind.
#[derive(Clone, Debug, Default)]
pub struct NewItem {
    pub title: String,
    pub content: String,
    /// strategy: optional hypothesis.
    pub hypothesis: Option<String>,
    /// initiative: optional complexity (`xs`|`s`|`m`|`l`|`xl`).
    pub complexity: Option<String>,
    /// task: `task` | `bug` | `tech_debt`.
    pub task_type: Option<String>,
    /// task: the owning team (delivery boards carry one).
    pub team_id: Option<String>,
    /// adr: optional decision maker.
    pub decision_maker: Option<String>,
    /// adr: optional decision date (`YYYY-MM-DD`).
    pub decision_date: Option<String>,
}

/// mirror of: `kairos_client::types::CreateStrategyRequest`.
#[derive(Debug, Serialize)]
struct CreateStrategyRequest<'a> {
    board_id: &'a str,
    column_id: &'a str,
    title: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hypothesis: Option<&'a str>,
}

/// mirror of: `kairos_client::types::CreateInitiativeRequest`.
#[derive(Debug, Serialize)]
struct CreateInitiativeRequest<'a> {
    board_id: &'a str,
    column_id: &'a str,
    title: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    complexity: Option<&'a str>,
}

/// mirror of: `kairos_client::types::CreateTaskRequest`.
#[derive(Debug, Serialize)]
struct CreateTaskRequest<'a> {
    board_id: &'a str,
    column_id: &'a str,
    title: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    team_id: Option<&'a str>,
}

/// mirror of: `kairos_client::types::CreateAdrRequest`.
#[derive(Debug, Serialize)]
struct CreateAdrRequest<'a> {
    board_id: &'a str,
    column_id: &'a str,
    title: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_maker: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_date: Option<&'a str>,
}

/// `POST /api/{family}` — create a board item in the given column.
pub async fn create_item(
    auth: Auth,
    kind: EntityKind,
    board_id: &str,
    column_id: &str,
    item: &NewItem,
) -> Result<(), ApiError> {
    let path = format!("/api/{}", kind.api_family());
    let _: serde_json::Value = match kind {
        EntityKind::Strategy => {
            post_json(
                auth,
                &path,
                &CreateStrategyRequest {
                    board_id,
                    column_id,
                    title: &item.title,
                    content: &item.content,
                    hypothesis: item.hypothesis.as_deref(),
                },
            )
            .await?
        }
        EntityKind::Initiative => {
            post_json(
                auth,
                &path,
                &CreateInitiativeRequest {
                    board_id,
                    column_id,
                    title: &item.title,
                    content: &item.content,
                    complexity: item.complexity.as_deref(),
                },
            )
            .await?
        }
        EntityKind::Task => {
            post_json(
                auth,
                &path,
                &CreateTaskRequest {
                    board_id,
                    column_id,
                    title: &item.title,
                    content: &item.content,
                    task_type: item.task_type.as_deref(),
                    team_id: item.team_id.as_deref(),
                },
            )
            .await?
        }
        EntityKind::Adr => {
            post_json(
                auth,
                &path,
                &CreateAdrRequest {
                    board_id,
                    column_id,
                    title: &item.title,
                    content: &item.content,
                    decision_maker: item.decision_maker.as_deref(),
                    decision_date: item.decision_date.as_deref(),
                },
            )
            .await?
        }
    };
    Ok(())
}

/// mirror of: `kairos_client::types::CreateDocumentRequest`.
#[derive(Debug, Serialize)]
struct CreateDocumentRequest<'a> {
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_id: Option<&'a str>,
    /// Required by the server: documents attach to a workflow item via a
    /// `supports` edge (KAIROS-A-0006).
    parent_short_code: &'a str,
}

/// `POST /api/documents` — create a document attached to a board item.
pub async fn create_document(
    auth: Auth,
    title: &str,
    template_id: Option<&str>,
    parent_short_code: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = post_json(
        auth,
        "/api/documents",
        &CreateDocumentRequest {
            title,
            template_id,
            parent_short_code,
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board-view mirrors decode a realistic
    /// `GET /api/boards/{id}` + `GET /api/boards/{id}/items` pair
    /// (field-name lock against the S-0005 wire shape).
    #[test]
    fn board_detail_mirror_decodes_server_shape() {
        // NB: the wire flattens the board's own fields to the top level
        // (`#[serde(flatten)]` on `BoardDetail::board` — verified against
        // the live endpoint during T-0040 browser verification).
        let body = serde_json::json!({
            "id": "6f1a1f9e-0000-0000-0000-000000000001",
            "name": "Platform Delivery",
            "slug": "platform-delivery",
            "board_level": "delivery",
            "team_id": "6f1a1f9e-0000-0000-0000-00000000000a",
            "created_at": "2026-07-10T09:00:00Z",
            "updated_at": "2026-07-10T09:00:00Z",
            "columns": [
                {"id": "c-1", "board_id": "b-1", "name": "Backlog", "position": 0,
                 "created_at": "2026-07-10T09:00:00Z", "updated_at": "2026-07-10T09:00:00Z"},
                {"id": "c-2", "board_id": "b-1", "name": "Todo", "position": 1,
                 "created_at": "2026-07-10T09:00:00Z", "updated_at": "2026-07-10T09:00:00Z"}
            ],
            "transitions": [
                {"id": "t-1", "board_id": "b-1", "from_column_id": "c-1", "to_column_id": "c-2"}
            ]
        });
        let detail: BoardDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(detail.board.slug, "platform-delivery");
        assert_eq!(detail.board.board_level, "delivery");
        assert_eq!(detail.columns[1].name, "Todo");
        assert_eq!(detail.transitions[0].to_column_id, "c-2");
    }

    /// The grouped-items mirror decodes all four entity types.
    #[test]
    fn board_items_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "board": {"id": "b-1", "name": "B", "slug": "b", "board_level": "delivery",
                      "team_id": null, "created_at": "x", "updated_at": "x"},
            "columns": [{
                "column": {"id": "c-1", "board_id": "b-1", "name": "Active", "position": 2,
                           "created_at": "x", "updated_at": "x"},
                "strategies": [{"id": "i1", "short_code": "DEMO-S-0001", "title": "S",
                                "content": "…", "board_id": "b", "column_id": "c",
                                "hypothesis": "h", "version": 1, "created_by": "u",
                                "updated_by": "u", "created_at": "x", "updated_at": "x"}],
                "initiatives": [{"id": "i2", "short_code": "DEMO-I-0002", "title": "I",
                                 "content": "…", "board_id": "b", "column_id": "c",
                                 "complexity": "m", "is_bucket": true, "bucket_type": "bug",
                                 "version": 1, "created_by": "u", "updated_by": "u",
                                 "created_at": "x", "updated_at": "x"}],
                "tasks": [{"id": "i3", "short_code": "DEMO-T-0003", "title": "T",
                           "content": "…", "board_id": "b", "column_id": "c",
                           "task_type": "tech_debt", "team_id": null, "version": 1,
                           "created_by": "u", "updated_by": "u",
                           "created_at": "x", "updated_at": "x"}],
                "adrs": [{"id": "i4", "short_code": "DEMO-A-0004", "title": "A",
                          "content": "…", "board_id": "b", "column_id": "c",
                          "decision_maker": "alice", "decision_date": "2026-06-30",
                          "version": 1, "created_by": "u", "updated_by": "u",
                          "created_at": "x", "updated_at": "x"}]
            }]
        });
        let items: BoardItemsResponse = serde_json::from_value(body).expect("mirror decodes");
        let group = &items.columns[0];
        assert_eq!(group.column.name, "Active");
        assert_eq!(group.strategies[0].short_code, "DEMO-S-0001");
        assert_eq!(group.initiatives[0].bucket_type.as_deref(), Some("bug"));
        assert!(group.initiatives[0].is_bucket);
        assert_eq!(group.tasks[0].task_type, "tech_debt");
        assert_eq!(group.adrs[0].decision_date.as_deref(), Some("2026-06-30"));
    }

    /// The template + event mirrors decode their wire shapes.
    #[test]
    fn template_and_event_mirrors_decode() {
        let templates: TemplateListEnvelope = serde_json::from_value(serde_json::json!({
            "items": [{"id": "t-1", "name": "PRD", "slug": "prd", "content": "# …",
                       "is_system_default": true}],
            "total": 1, "limit": 100, "offset": 0
        }))
        .expect("template mirror decodes");
        assert_eq!(templates.items[0].name, "PRD");

        let event: ThinEvent = serde_json::from_str(
            r#"{"event":"item_transitioned","entity_type":"task",
                "short_code":"DEMO-T-0003","board_id":"b-1","column_id":"c-2",
                "actor":"u-1","occurred_at":"2026-07-14T12:00:00Z"}"#,
        )
        .expect("event mirror decodes");
        assert_eq!(event.event, "item_transitioned");
    }

    /// Create requests serialize with the exact S-0005 field names and
    /// omit absent optionals.
    #[test]
    fn create_requests_serialize_wire_shape() {
        let task = serde_json::to_value(CreateTaskRequest {
            board_id: "b-1",
            column_id: "c-1",
            title: "T",
            content: "body",
            task_type: Some("bug"),
            team_id: None,
        })
        .expect("serializes");
        assert_eq!(
            task,
            serde_json::json!({"board_id": "b-1", "column_id": "c-1",
                               "title": "T", "content": "body", "task_type": "bug"})
        );

        let doc = serde_json::to_value(CreateDocumentRequest {
            title: "PRD: x",
            template_id: Some("t-1"),
            parent_short_code: "DEMO-I-0002",
        })
        .expect("serializes");
        assert_eq!(
            doc,
            serde_json::json!({"title": "PRD: x", "template_id": "t-1",
                               "parent_short_code": "DEMO-I-0002"})
        );
    }

    /// Board level → create-flow entity kind (A-0002 one-family-per-level).
    #[test]
    fn entity_kind_per_board_level() {
        assert_eq!(
            EntityKind::for_board_level("strategy"),
            Some(EntityKind::Strategy)
        );
        assert_eq!(
            EntityKind::for_board_level("initiative"),
            Some(EntityKind::Initiative)
        );
        assert_eq!(
            EntityKind::for_board_level("delivery"),
            Some(EntityKind::Task)
        );
        assert_eq!(EntityKind::for_board_level("adr"), Some(EntityKind::Adr));
        assert_eq!(EntityKind::for_board_level("nope"), None);
        assert_eq!(EntityKind::Strategy.api_family(), "strategies");
    }
}
