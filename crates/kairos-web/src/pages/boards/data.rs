//! Boards data layer (KAIROS-T-0040): partial mirror DTOs + fetch calls
//! for the board list / board view, per the conventions' data-layer rules
//! (docs/gui-conventions.md §4 — mirrors declare only what the view reads;
//! every mirror carries a `mirror of:` line and a decode test).

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api::{get_json, post_json};
use crate::auth::Auth;
use crate::pages::repositories::api::RepositoryRef;

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
    /// `task` | `bug` | `tech_debt` | `support`.
    pub task_type: String,
    /// `planned` | `support` — the lane axis (KAIROS-T-0077).
    pub work_class: String,
    /// The repository the task links to, embedded by the server
    /// (KAIROS-T-0104, COLLIERY-A-0023); `None` for a task with no link.
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
    /// The person who has the task in Active (KAIROS-T-0359); `None` for
    /// a task with no claim, and from an older server.
    #[serde(default)]
    pub claim: Option<TaskClaim>,
}

/// mirror of: `kairos_client::types::TaskClaim` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TaskClaim {
    pub display_name: String,
    /// True when the agent of the person made the claim.
    #[serde(default)]
    pub agent: bool,
    /// RFC 3339.
    #[serde(default)]
    pub claimed_at: String,
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
    /// The number of cards of the board, on all pages (COLLIERY-T-0261).
    /// After [`load_board_view`] it can be larger than the number of cards
    /// that the view has: the view reads [`BOARD_CARD_CAP`] cards at most.
    #[serde(default)]
    pub total: i64,
    /// Children rollups keyed by the PARENT item's short code
    /// (KAIROS-T-0080); absent for items without children.
    #[serde(default)]
    pub children_progress: std::collections::BTreeMap<String, ProgressCounts>,
    /// Blocked-by/blocks counts keyed by short code (KAIROS-T-0091);
    /// absent for items with no open blocks edges. The server counts an
    /// edge only while neither end sits in a terminal column
    /// (COLLIERY-T-0214), so a card in Completed has no entry and shows
    /// no badge. The card renders what it is given.
    #[serde(default)]
    pub blocks_summary: std::collections::BTreeMap<String, BlocksCounts>,
    /// The teams of each strategy and initiative, keyed by short code
    /// (KAIROS-T-0321); absent for an item with no team.
    #[serde(default)]
    pub item_teams: std::collections::BTreeMap<String, Vec<ItemTeam>>,
}

/// mirror of: `kairos_client::types_org::ItemTeam` (KAIROS-T-0321): one
/// team of an initiative or a strategy, from its tasks or set by hand.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ItemTeam {
    pub slug: String,
    pub name: String,
    pub from_tasks: bool,
    pub set_by_hand: bool,
}

/// mirror of: `kairos_client::types_org::BlocksCounts` (KAIROS-T-0091).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct BlocksCounts {
    pub blocked_by: i64,
    pub blocks: i64,
}

/// mirror of: `kairos_client::types_org::ProgressCounts` (KAIROS-T-0080).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct ProgressCounts {
    pub done: i64,
    pub total: i64,
    /// False = no involved board has done columns; show composition only.
    #[serde(default)]
    pub has_done: bool,
}

// ---- mirrors: templates + events ------------------------------------------

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_events::ThinEvent` (partial — the view
/// only needs to know "something on this board changed" and re-fetches
/// through REST per A-0005 §5; events carry no payloads).
///
/// The KIND is deliberately not inspected: the server filters the socket
/// to one board, so every event that arrives concerns this board and the
/// refetch is the same reconcile for all of them. That is what makes
/// `item_moved` (KAIROS-I-0012) work without a special case — the server
/// emits it once per side of the move, with `board_id` = the board the
/// card left (no column) and once with `board_id` = the board it joined
/// (its entry column), so each board hears its own half and refetches.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ThinEvent {
    pub event: String,
}

// ---- the pages of the items of a board (COLLIERY-T-0261) --------------------

/// The size of a page of the items of a board: the maximum of the server
/// (`BOARD_ITEMS_MAX_LIMIT`). A board with 1000 cards or fewer is thus
/// one request, as it was before the route had pages.
pub const BOARD_PAGE_LIMIT: i64 = 1000;

/// The number of cards that the board view reads at most. A board with
/// more cards says so ([`board_cap_note`]).
pub const BOARD_CARD_CAP: usize = 2000;

impl BoardItemsResponse {
    /// The number of cards of the response, in all columns.
    pub fn card_count(&self) -> usize {
        self.columns
            .iter()
            .map(|c| c.strategies.len() + c.initiatives.len() + c.tasks.len() + c.adrs.len())
            .sum()
    }

    /// Add the next page to this response: the cards go to the end of
    /// their columns, and the two maps get the entries of the page.
    /// `total` becomes that of the newer page. A column that this
    /// response does not have goes to the end of the columns. Pure,
    /// host-tested.
    pub fn add_page(&mut self, page: BoardItemsResponse) {
        for mut group in page.columns {
            match self
                .columns
                .iter_mut()
                .find(|mine| mine.column.id == group.column.id)
            {
                Some(mine) => {
                    mine.strategies.append(&mut group.strategies);
                    mine.initiatives.append(&mut group.initiatives);
                    mine.tasks.append(&mut group.tasks);
                    mine.adrs.append(&mut group.adrs);
                }
                None => self.columns.push(group),
            }
        }
        self.children_progress.extend(page.children_progress);
        self.blocks_summary.extend(page.blocks_summary);
        self.item_teams.extend(page.item_teams);
        self.total = page.total;
    }
}

/// The `(limit, offset)` of the next page of the items of a board, or
/// `None` when the view has each card or has [`BOARD_CARD_CAP`] cards.
/// `fetched` is the number of cards that the view has, and `last_page` is
/// the number of cards of the last page. The last page before the cap is
/// smaller, so that the view does not go above the cap.
///
/// An empty page stops the read, as in [`crate::api::next_offset`]. Pure,
/// host-tested.
pub fn next_board_page(fetched: usize, last_page: usize, total: i64) -> Option<(i64, i64)> {
    let offset = crate::api::next_offset(fetched, last_page, total)?;
    let room = BOARD_CARD_CAP.saturating_sub(fetched);
    if room == 0 {
        return None;
    }
    let limit = i64::try_from(room)
        .unwrap_or(BOARD_PAGE_LIMIT)
        .min(BOARD_PAGE_LIMIT);
    Some((limit, offset))
}

/// The sentences that the board shows when it does not have each card
/// (COLLIERY-T-0261), or `None` when it has each card. The filters of the
/// board apply to the cards that the board has, so the note sends the
/// person to the search. Pure, host-tested.
pub fn board_cap_note(shown: usize, total: i64) -> Option<String> {
    let shown = i64::try_from(shown).unwrap_or(i64::MAX);
    (shown < total).then(|| {
        format!(
            "The board shows {shown} of {total} cards. To find a card that the board does \
             not show, use the search."
        )
    })
}

/// Each card of a board, page after page, up to [`BOARD_CARD_CAP`] cards:
/// `GET /api/boards/{id}/items?limit=&offset=`. `total` of the result is
/// the number of cards of the board. `board` is the slug or the id of the
/// board (COLLIERY-T-0265).
pub async fn load_board_items(auth: Auth, board: &str) -> Result<BoardItemsResponse, ApiError> {
    let path = format!("{}/items", board_path(board));
    let mut items: BoardItemsResponse =
        get_json(auth, &crate::api::page_url(&path, BOARD_PAGE_LIMIT, 0)).await?;
    let mut last_page = items.card_count();
    while let Some((limit, offset)) = next_board_page(items.card_count(), last_page, items.total) {
        let page: BoardItemsResponse =
            get_json(auth, &crate::api::page_url(&path, limit, offset)).await?;
        last_page = page.card_count();
        items.add_page(page);
    }
    Ok(items)
}

/// Lets one read of the board run at a time (COLLIERY-T-0261). A read of
/// a large board is 2 requests or more, and each event of the board asks
/// for a read. With the gate, the events that come during a read give ONE
/// more read after it, and not one read for each event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefetchGate {
    /// The number of reads that run.
    running: u32,
    /// An event came during a read.
    pending: bool,
}

impl RefetchGate {
    /// An event asks for a read. `true`: start the read now. `false`: a
    /// read runs, and the gate gives one more read after it.
    pub fn request(&mut self) -> bool {
        if self.running > 0 {
            self.pending = true;
            return false;
        }
        true
    }

    /// A read starts.
    pub fn started(&mut self) {
        self.running = self.running.saturating_add(1);
    }

    /// A read ends. `true`: an event came during the read, so start one
    /// more read.
    pub fn finished(&mut self) -> bool {
        self.running = self.running.saturating_sub(1);
        self.running == 0 && std::mem::take(&mut self.pending)
    }
}

// ---- the whole board view in one fetch -------------------------------------

/// Everything the board view renders: configuration (columns +
/// transitions) and the grouped items.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardView {
    pub detail: BoardDetail,
    pub items: BoardItemsResponse,
}

/// The board list: each live board, page after page (COLLIERY-T-0258).
/// The first 100 boards were the list before, and a board after them had
/// no card on the overview and no page at `/boards/{slug}`.
pub async fn list_boards(auth: Auth) -> Result<Vec<Board>, ApiError> {
    crate::api::get_all(auth, "/api/boards").await
}

/// The path of the read routes of one board (COLLIERY-T-0265). `param` is
/// the route param of `/boards/:board`: the slug of the board, or its id.
/// The server takes the two, so the path comes from the param only. Pure,
/// host-tested.
pub fn board_path(param: &str) -> String {
    format!("/api/boards/{}", crate::api::encode_component(param))
}

/// The full [`BoardView`] of the board of a route param (the slug of the
/// board, or its id).
///
/// COLLIERY-T-0265: the view reads the one board that it shows. Before,
/// each read of the view (and each live re-fetch) read the full list of the
/// boards to get the id of the board. The server gives the 404 for a param
/// that no live board has.
pub async fn load_board_view(auth: Auth, param: &str) -> Result<BoardView, ApiError> {
    let detail: BoardDetail = get_json(auth, &board_path(param)).await?;
    // COLLIERY-T-0261: the items come in pages.
    let items = load_board_items(auth, param).await?;
    Ok(BoardView { detail, items })
}

/// Templates for the document create flow: each template, page after
/// page (COLLIERY-T-0258).
pub async fn list_templates(auth: Auth) -> Result<Vec<Template>, ApiError> {
    crate::api::get_all(auth, "/api/templates").await
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

/// mirror of: `kairos_client::types::SetWorkClassRequest`.
#[derive(Debug, Serialize)]
struct SetWorkClassRequest<'a> {
    work_class: &'a str,
}

/// `POST /api/tasks/{short_code}/work-class` — move a task between the
/// Planned/Support lanes (KAIROS-T-0077). Tasks only; the response body
/// is discarded like [`transition`]'s.
pub async fn set_work_class(
    auth: Auth,
    short_code: &str,
    work_class: &str,
) -> Result<(), ApiError> {
    let path = format!("/api/tasks/{short_code}/work-class");
    let _: serde_json::Value = post_json(auth, &path, &SetWorkClassRequest { work_class }).await?;
    Ok(())
}

/// The create-from-column form data; [`create_item`] maps it onto the
/// right S-0005 request per entity kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewItem {
    pub title: String,
    pub content: String,
    /// strategy: optional hypothesis.
    pub hypothesis: Option<String>,
    /// initiative: optional complexity (`xs`|`s`|`m`|`l`|`xl`).
    pub complexity: Option<String>,
    /// task: `task` | `bug` | `tech_debt` | `support`.
    pub task_type: Option<String>,
    /// task: `planned` | `support` lane (KAIROS-T-0077). `None` lets the
    /// server apply its default (support type → support lane).
    pub work_class: Option<String>,
    /// task: the owning team (delivery boards carry one).
    pub team_id: Option<String>,
    /// task: the repository it links to (slug, KAIROS-T-0124 #6b). The
    /// link does not choose the board (COLLIERY-A-0023): `board_id` does.
    pub repository: Option<String>,
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
    work_class: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    team_id: Option<&'a str>,
    /// Slug or UUID (KAIROS-T-0115: `repository` is THE wire name).
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<&'a str>,
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

/// mirror of: `kairos_client::types::{Strategy, Initiative, Task, Adr}`
/// (partial): the one field of a created item that the board reads. Each
/// of the four types carries `short_code`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Created {
    short_code: String,
}

/// `POST /api/{family}` — create a board item in the given column.
///
/// Returns the short code of the new item (COLLIERY-T-0232). The board
/// names a request in a notice after the create, and the short code is the
/// one name that the person and the team of the board can both use. The
/// remainder of the body is discarded: server state is re-fetched.
pub async fn create_item(
    auth: Auth,
    kind: EntityKind,
    board_id: &str,
    column_id: &str,
    item: &NewItem,
) -> Result<String, ApiError> {
    let path = format!("/api/{}", kind.api_family());
    let created: Created = match kind {
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
                    work_class: item.work_class.as_deref(),
                    team_id: item.team_id.as_deref(),
                    repository: item.repository.as_deref(),
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
    Ok(created.short_code)
}

/// mirror of: `kairos_client::types::CreateDocumentRequest`.
#[derive(Debug, Serialize)]
struct CreateDocumentRequest<'a> {
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_id: Option<&'a str>,
    /// The owner board (slug), required by the server (COLLIERY-T-3109).
    board: &'a str,
    /// The board item that the document supports.
    parent_short_code: &'a str,
}

/// `POST /api/documents` — create a document on its owner board `board`,
/// attached to a board item.
pub async fn create_document(
    auth: Auth,
    title: &str,
    template_id: Option<&str>,
    board: &str,
    parent_short_code: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = post_json(
        auth,
        "/api/documents",
        &CreateDocumentRequest {
            title,
            template_id,
            board,
            parent_short_code,
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One page of the items of a board: the tasks `first..first + count`
    /// in the column `column`, with an entry in each map for each task.
    fn page(column: &str, first: usize, count: usize, total: i64) -> BoardItemsResponse {
        let column_of = |id: &str, position: i32| {
            serde_json::json!({
                "column": {"id": id, "name": id, "position": position},
                "strategies": [], "initiatives": [], "tasks": [], "adrs": []
            })
        };
        let mut columns = vec![column_of("todo", 0), column_of("doing", 1)];
        let codes: Vec<String> = (first..first + count)
            .map(|number| format!("DEMO-T-{number:04}"))
            .collect();
        let tasks: Vec<serde_json::Value> = codes
            .iter()
            .map(|code| {
                serde_json::json!({
                    "short_code": code, "title": code,
                    "task_type": "task", "work_class": "planned"
                })
            })
            .collect();
        let index = if column == "todo" { 0 } else { 1 };
        columns[index]["tasks"] = serde_json::Value::Array(tasks);
        let progress: serde_json::Map<String, serde_json::Value> = codes
            .iter()
            .map(|code| (code.clone(), serde_json::json!({"done": 0, "total": 1})))
            .collect();
        let blocks: serde_json::Map<String, serde_json::Value> = codes
            .iter()
            .map(|code| {
                (
                    code.clone(),
                    serde_json::json!({"blocked_by": 1, "blocks": 0}),
                )
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "board": {"id": "b", "name": "Demo", "slug": "demo-delivery",
                      "board_level": "delivery"},
            "columns": columns,
            "total": total,
            "children_progress": progress,
            "blocks_summary": blocks
        }))
        .expect("mirror decodes")
    }

    /// COLLIERY-T-0261: the pages of a board go together as one view.
    #[test]
    fn the_pages_of_a_board_go_together() {
        // 5 cards: 3 in `todo` and 2 in `doing`, in pages of 2 cards.
        let mut view = page("todo", 1, 2, 5);
        view.add_page({
            // The second page has the last card of `todo` and the first
            // card of `doing`.
            let mut second = page("todo", 3, 1, 5);
            second.add_page(page("doing", 4, 1, 5));
            second
        });
        view.add_page(page("doing", 5, 1, 5));
        assert_eq!(view.card_count(), 5);
        assert_eq!(view.total, 5);
        assert_eq!(view.columns.len(), 2, "each column is there one time");
        let codes = |index: usize| -> Vec<&str> {
            view.columns[index]
                .tasks
                .iter()
                .map(|task| task.short_code.as_str())
                .collect()
        };
        assert_eq!(codes(0), ["DEMO-T-0001", "DEMO-T-0002", "DEMO-T-0003"]);
        assert_eq!(codes(1), ["DEMO-T-0004", "DEMO-T-0005"]);
        // The maps have the entries of each page.
        assert_eq!(view.children_progress.len(), 5);
        assert_eq!(view.blocks_summary.len(), 5);
        assert_eq!(board_cap_note(view.card_count(), view.total), None);
    }

    /// COLLIERY-T-0261: a response of a server with no pages has no
    /// `total`. The view has one page, and no note.
    #[test]
    fn a_response_with_no_total_is_the_full_board() {
        let items: BoardItemsResponse = serde_json::from_value(serde_json::json!({
            "board": {"id": "b", "name": "Demo", "slug": "demo-delivery",
                      "board_level": "delivery"},
            "columns": []
        }))
        .expect("mirror decodes");
        assert_eq!(items.total, 0);
        assert_eq!(next_board_page(0, 0, items.total), None);
        assert_eq!(board_cap_note(250, items.total), None);
    }

    /// COLLIERY-T-0261: the view reads each page of a board, and it stops
    /// at the cap.
    #[test]
    fn the_view_reads_each_page_up_to_the_cap() {
        /// The pages that the view reads for a board of `total` cards:
        /// `(limit, offset)` of each request.
        fn requests(total: usize) -> Vec<(i64, i64)> {
            let mut requests = vec![(BOARD_PAGE_LIMIT, 0)];
            let mut fetched = 0;
            loop {
                let (limit, offset) = *requests.last().expect("a request");
                let last_page = (total - offset as usize).min(limit as usize);
                fetched += last_page;
                match next_board_page(fetched, last_page, total as i64) {
                    Some(next) => requests.push(next),
                    None => return requests,
                }
            }
        }
        // One request for a board of 1000 cards or fewer, as before.
        assert_eq!(requests(0), [(1000, 0)]);
        assert_eq!(requests(210), [(1000, 0)]);
        assert_eq!(requests(1000), [(1000, 0)]);
        assert_eq!(requests(1001), [(1000, 0), (1000, 1000)]);
        assert_eq!(requests(2000), [(1000, 0), (1000, 1000)]);
        // Above the cap: 2 requests, and no more.
        assert_eq!(requests(2340), [(1000, 0), (1000, 1000)]);
        assert_eq!(requests(50_000), [(1000, 0), (1000, 1000)]);
        // An empty page stops the read.
        assert_eq!(next_board_page(1000, 0, 1500), None);
        // The last page before the cap is smaller.
        assert_eq!(next_board_page(1500, 500, 5000), Some((500, 1500)));
    }

    /// COLLIERY-T-0261: above the cap the board says so, with the numbers.
    #[test]
    fn the_board_says_that_it_does_not_show_each_card() {
        assert_eq!(
            board_cap_note(2000, 2340).as_deref(),
            Some(
                "The board shows 2000 of 2340 cards. To find a card that the board does \
                 not show, use the search."
            )
        );
        assert_eq!(board_cap_note(2000, 2000), None);
        assert_eq!(board_cap_note(1000, 1000), None);
        assert_eq!(board_cap_note(0, 0), None);
    }

    /// COLLIERY-T-0265: the paths of the board view come from the route
    /// param only. No list of the boards is necessary to make them.
    #[test]
    fn the_board_view_reads_the_board_of_the_route_param() {
        assert_eq!(
            board_path("platform-delivery"),
            "/api/boards/platform-delivery"
        );
        assert_eq!(
            board_path("6f1a1f9e-0000-7000-8000-000000000001"),
            "/api/boards/6f1a1f9e-0000-7000-8000-000000000001"
        );
        // A slug from before the rule of a slug.
        assert_eq!(board_path("Road Map"), "/api/boards/Road%20Map");
        assert_eq!(board_path("a/b?c"), "/api/boards/a%2Fb%3Fc");
    }

    /// COLLIERY-T-0261: the events that come during a read of the board
    /// give one more read, and not one read for each event.
    #[test]
    fn the_gate_gives_one_read_for_the_events_of_a_read() {
        let mut gate = RefetchGate::default();
        // No read runs: the event starts a read.
        assert!(gate.request());
        gate.started();
        // 3 events come during the read: no read starts.
        assert!(!gate.request());
        assert!(!gate.request());
        assert!(!gate.request());
        // The read ends: ONE more read.
        assert!(gate.finished());
        gate.started();
        // No event came during that read: no more read.
        assert!(!gate.finished());
        // And the next event starts a read.
        assert!(gate.request());
        // 2 reads run at the same time (the person went to a different
        // board): the read that follows waits for the last of them.
        gate.started();
        gate.started();
        assert!(!gate.request());
        assert!(!gate.finished());
        assert!(gate.finished());
        assert!(!gate.finished(), "a read that ends 2 times does no harm");
    }

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
                           "task_type": "tech_debt", "work_class": "support",
                           "team_id": null, "version": 1,
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
        // children_progress and blocks_summary are optional on the wire
        // (KAIROS-T-0080 / T-0091) — absent decodes to empty maps.
        assert!(items.children_progress.is_empty());
        assert!(items.blocks_summary.is_empty());
        let with_blocks: BoardItemsResponse = serde_json::from_value(serde_json::json!({
            "board": {"id": "b1", "name": "Delivery", "slug": "delivery",
                      "board_level": "delivery", "team_id": null,
                      "created_at": "x", "updated_at": "x"},
            "columns": [],
            "blocks_summary": {"DEMO-T-0003": {"blocked_by": 1, "blocks": 2}}
        }))
        .expect("blocks mirror decodes");
        let counts = with_blocks
            .blocks_summary
            .get("DEMO-T-0003")
            .expect("entry");
        assert_eq!((counts.blocked_by, counts.blocks), (1, 2));
        assert_eq!(group.strategies[0].short_code, "DEMO-S-0001");
        assert_eq!(group.initiatives[0].bucket_type.as_deref(), Some("bug"));
        assert!(group.initiatives[0].is_bucket);
        assert_eq!(group.tasks[0].task_type, "tech_debt");
        assert_eq!(group.tasks[0].work_class, "support");
        assert_eq!(group.adrs[0].decision_date.as_deref(), Some("2026-06-30"));
    }

    /// The template + event mirrors decode their wire shapes.
    #[test]
    fn template_and_event_mirrors_decode() {
        let templates: crate::api::Page<Template> = serde_json::from_value(serde_json::json!({
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

        // KAIROS-I-0012: a board move emits `item_moved` TWICE — the
        // source board without a column, the target with its entry column.
        // Both decode here, so a board refetches for whichever side names
        // it (the kind is not inspected: any event on this board refetches).
        let left: ThinEvent = serde_json::from_str(
            r#"{"event":"item_moved","entity_type":"task",
                "short_code":"DEMO-T-0003","board_id":"b-1","column_id":null,
                "actor":"u-1","occurred_at":"2026-09-23T12:00:00Z"}"#,
        )
        .expect("source-side move event decodes");
        assert_eq!(left.event, "item_moved");
        let arrived: ThinEvent = serde_json::from_str(
            r#"{"event":"item_moved","entity_type":"task",
                "short_code":"DEMO-T-0003","board_id":"b-2","column_id":"c-9",
                "actor":"u-1","occurred_at":"2026-09-23T12:00:00Z"}"#,
        )
        .expect("target-side move event decodes");
        assert_eq!(arrived.event, "item_moved");
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
            work_class: Some("support"),
            team_id: None,
            repository: None,
        })
        .expect("serializes");
        assert_eq!(
            task,
            serde_json::json!({"board_id": "b-1", "column_id": "c-1",
                               "title": "T", "content": "body", "task_type": "bug",
                               "work_class": "support"})
        );

        // KAIROS-T-0124 #6b: a picked repository rides as `repository`
        // (the slug) next to the board — the server routes by it.
        let bound = serde_json::to_value(CreateTaskRequest {
            board_id: "b-1",
            column_id: "c-1",
            title: "T",
            content: "",
            task_type: None,
            work_class: None,
            team_id: Some("team-1"),
            repository: Some("payments-api"),
        })
        .expect("serializes");
        assert_eq!(
            bound,
            serde_json::json!({"board_id": "b-1", "column_id": "c-1",
                               "title": "T", "content": "", "team_id": "team-1",
                               "repository": "payments-api"})
        );

        let doc = serde_json::to_value(CreateDocumentRequest {
            title: "PRD: x",
            template_id: Some("t-1"),
            board: "web-delivery",
            parent_short_code: "DEMO-I-0002",
        })
        .expect("serializes");
        // COLLIERY-T-3109: the owner board is always in the body.
        assert_eq!(
            doc,
            serde_json::json!({"title": "PRD: x", "template_id": "t-1",
                               "board": "web-delivery",
                               "parent_short_code": "DEMO-I-0002"})
        );
    }

    /// COLLIERY-T-0232: the response of a create decodes to the short code,
    /// from a whole task body.
    #[test]
    fn created_mirror_decodes_a_task_body() {
        let created: Created = serde_json::from_value(serde_json::json!({
            "id": "i3", "short_code": "DEMO-T-0042", "title": "T", "content": "",
            "board_id": "b", "column_id": "c", "task_type": "task",
            "work_class": "support", "team_id": "t", "version": 1,
            "created_by": "u", "updated_by": "u",
            "created_at": "x", "updated_at": "x"
        }))
        .expect("mirror decodes");
        assert_eq!(created.short_code, "DEMO-T-0042");
    }

    /// KAIROS-T-0359: a card task decodes its claim, and a task of an
    /// older server (no `claim`) decodes with none.
    #[test]
    fn task_mirror_decodes_the_claim() {
        let task: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0001", "title": "T", "task_type": "task",
            "work_class": "planned",
            "claim": {"user_id": "u", "display_name": "Alice", "agent": true,
                      "claimed_at": "2026-10-09T10:00:00Z"}
        }))
        .expect("mirror decodes");
        let claim = task.claim.expect("the claim");
        assert_eq!(claim.display_name, "Alice");
        assert!(claim.agent);
        let old: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0002", "title": "T", "task_type": "task",
            "work_class": "planned"
        }))
        .expect("mirror decodes");
        assert_eq!(old.claim, None);
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
