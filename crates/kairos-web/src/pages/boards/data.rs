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
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
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
    /// The column holds finished work: "Archive completed" archives its
    /// tasks (KAIROS-T-0363).
    #[serde(default)]
    pub is_done: bool,
    /// The WIP limit of the column (KAIROS-T-0371); `None`: no limit.
    #[serde(default)]
    pub wip_limit: Option<i32>,
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

/// mirror of: `kairos_client::types_cards::StrategyCard` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Strategy {
    pub short_code: String,
    pub title: String,
    /// The version of the item (KAIROS-A-0004): the key of the card
    /// (KAIROS-T-0615).
    pub version: i32,
    /// The owner (KAIROS-T-0411); `None` for no owner, and from an older
    /// server.
    #[serde(default)]
    pub owner: Option<ItemOwner>,
    /// When the card entered its column, RFC 3339 (KAIROS-T-0649); `None`
    /// from an older server.
    #[serde(default)]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649).
    #[serde(default)]
    pub notes_count: Option<i64>,
}

/// mirror of: `kairos_client::types::ItemOwner` (partial).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct ItemOwner {
    /// The person (UUID). The board groups the cards by it (KAIROS-T-0416).
    #[serde(default)]
    pub user_id: String,
    pub display_name: String,
}

/// The pill of the owner of a strategy or an initiative on a card
/// (KAIROS-T-0411). Pure, host-tested.
pub(crate) fn owner_pill(owner: Option<&ItemOwner>) -> Option<String> {
    owner.map(|owner| format!("owner: {}", owner.display_name))
}

/// mirror of: `kairos_client::types_cards::InitiativeCard` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Initiative {
    pub short_code: String,
    pub title: String,
    /// The version of the item (KAIROS-A-0004): the key of the card
    /// (KAIROS-T-0615).
    pub version: i32,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub is_bucket: bool,
    #[serde(default)]
    pub bucket_type: Option<String>,
    /// The owner (KAIROS-T-0411); `None` for no owner, and from an older
    /// server.
    #[serde(default)]
    pub owner: Option<ItemOwner>,
    /// When the card entered its column, RFC 3339 (KAIROS-T-0649); `None`
    /// from an older server.
    #[serde(default)]
    pub entered_column_at: Option<String>,
    /// The cancel mark (KAIROS-T-0678); `None` for an initiative that is
    /// not cancelled, and from an older server.
    #[serde(default)]
    pub cancellation: Option<TaskCancellation>,
    /// The number of the notes of the item (KAIROS-T-0649).
    #[serde(default)]
    pub notes_count: Option<i64>,
}

/// mirror of: `kairos_client::types_cards::TaskCard` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Task {
    pub short_code: String,
    pub title: String,
    /// The version of the item (KAIROS-A-0004): the key of the card
    /// (KAIROS-T-0615).
    pub version: i32,
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
    /// The cancel mark (KAIROS-T-0362); `None` for a task that is not
    /// cancelled, and from an older server.
    #[serde(default)]
    pub cancellation: Option<TaskCancellation>,
    /// The live state (KAIROS-T-0506); `None` for a task with no state,
    /// and from an older server.
    #[serde(default)]
    pub state: Option<TaskState>,
    /// When the card entered its column, RFC 3339 (KAIROS-T-0649); `None`
    /// from an older server.
    #[serde(default)]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649).
    #[serde(default)]
    pub notes_count: Option<i64>,
}

/// mirror of: `kairos_client::types::TaskState` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct TaskState {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pushed: bool,
    #[serde(default)]
    pub next_step: Option<String>,
    /// RFC 3339.
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub updated_by_name: String,
}

/// mirror of: `kairos_client::types::TaskCancellation` (partial — card
/// fields).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct TaskCancellation {
    pub reason: String,
}

/// mirror of: `kairos_client::types::TaskClaim` (partial — card fields).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct TaskClaim {
    /// The person (UUID). The board groups the cards by it (KAIROS-T-0416).
    #[serde(default)]
    pub user_id: String,
    pub display_name: String,
    /// True when the agent of the person made the claim.
    #[serde(default)]
    pub agent: bool,
    /// RFC 3339.
    #[serde(default)]
    pub claimed_at: String,
}

/// mirror of: `kairos_client::types_cards::AdrCard` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Adr {
    pub short_code: String,
    pub title: String,
    /// The version of the item (KAIROS-A-0004): the key of the card
    /// (KAIROS-T-0615).
    pub version: i32,
    #[serde(default)]
    pub decision_date: Option<String>,
    /// When the card entered its column, RFC 3339 (KAIROS-T-0649); `None`
    /// from an older server.
    #[serde(default)]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649).
    #[serde(default)]
    pub notes_count: Option<i64>,
}

/// mirror of: `kairos_client::types_org::BoardColumnItems`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumnItems {
    pub column: BoardColumn,
    /// The live items of the column, on each page (KAIROS-T-0371): the
    /// count that the WIP limit is for.
    #[serde(default)]
    pub item_count: i64,
    /// `item_count` is above the WIP limit.
    #[serde(default)]
    pub over_wip_limit: bool,
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
    /// The metadata values of each item, by short code and then by the
    /// slug of the definition (KAIROS-T-0368). The view asks for it with
    /// `with_metadata=true` (KAIROS-T-0582); an item with no value has no
    /// entry.
    #[serde(default)]
    pub item_metadata:
        Option<std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>>,
}

/// The slug of the metadata definition of the priority (seeded, on each
/// item type).
pub const PRIORITY_SLUG: &str = "priority";

impl BoardItemsResponse {
    /// The value of the metadata `slug` of the item `code`, or `None`.
    pub fn metadata_value(&self, code: &str, slug: &str) -> Option<&str> {
        self.item_metadata
            .as_ref()?
            .get(code)?
            .get(slug)
            .map(String::as_str)
    }
}

/// mirror of: `kairos_client::types_org::ItemTeam` (KAIROS-T-0321): one
/// team of an initiative or a strategy, from its tasks or set by hand.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct ItemTeam {
    pub slug: String,
    pub name: String,
    pub from_tasks: bool,
    pub set_by_hand: bool,
}

/// mirror of: `kairos_client::types_org::BlocksCounts` (KAIROS-T-0091).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct BlocksCounts {
    pub blocked_by: i64,
    pub blocks: i64,
}

/// mirror of: `kairos_client::types_org::ProgressCounts` (KAIROS-T-0080).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct ProgressCounts {
    pub done: i64,
    pub total: i64,
    /// False = no involved board has done columns; show composition only.
    #[serde(default)]
    pub has_done: bool,
    /// KAIROS-T-0567: cancelled children, apart from `done`.
    #[serde(default)]
    pub cancelled: i64,
}

impl ProgressCounts {
    /// The words of the card badge: `3/5 done, 1 cancelled`, or the
    /// composition only when no board of the children has a done column.
    /// Pure, host-tested.
    pub fn label(&self) -> String {
        if !self.has_done {
            return format!("{} children", self.total);
        }
        if self.cancelled > 0 {
            format!(
                "{}/{} done, {} cancelled",
                self.done, self.total, self.cancelled
            )
        } else {
            format!("{}/{} done", self.done, self.total)
        }
    }
}

// ---- mirrors: templates + events ------------------------------------------

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_events::ThinEvent` (partial — events
/// carry no payloads; the view reads through REST per A-0005 §5).
///
/// The KIND is not inspected, except `board_configured`. An event of an
/// item of this board reads the card of the item again (KAIROS-T-0615,
/// [`refresh_of`]). That is what
/// makes `item_moved` (KAIROS-I-0012) work without a special case: the
/// server emits it once per side of the move, with `board_id` = the board
/// the card left (no column) and once with `board_id` = the board it
/// joined. The card read on the old board finds no card, and the card
/// goes; the read on the new board finds it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ThinEvent {
    pub event: String,
    /// `notification` for an event of a person (KAIROS-T-0381); empty for
    /// the frame `lagged` (KAIROS-T-0621).
    #[serde(default)]
    pub entity_type: String,
    /// The item of the event; empty when the event names none.
    #[serde(default)]
    pub short_code: String,
    /// The board of the item; `None` for an item on no board.
    #[serde(default)]
    pub board_id: Option<String>,
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
        if let Some(metadata) = page.item_metadata {
            self.item_metadata
                .get_or_insert_with(Default::default)
                .extend(metadata);
        }
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
/// `GET /api/boards/{id}/items?with_metadata=true&limit=&offset=`. `total`
/// of the result is the number of cards of the board. `board` is the slug
/// or the id of the board (COLLIERY-T-0265).
///
/// `done_limit` asks the server for the last cards of each done column
/// only (KAIROS-T-0649); `None` reads each card.
pub async fn load_board_items(
    auth: Auth,
    board: &str,
    done_limit: Option<usize>,
) -> Result<BoardItemsResponse, ApiError> {
    let path = board_items_path(board, done_limit);
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

// ---- one card at a time (KAIROS-T-0615) --------------------------------------

/// The short code of a card of the four kinds.
trait CardCode {
    fn code(&self) -> &str;
}

macro_rules! card_code {
    ($($ty:ty),*) => {$(
        impl CardCode for $ty {
            fn code(&self) -> &str {
                &self.short_code
            }
        }
    )*};
}
card_code!(Strategy, Initiative, Task, Adr);

/// Take the card `code` out of `cards`: its index and the card, or `None`.
fn take_card<T: CardCode>(cards: &mut Vec<T>, code: &str) -> Option<(usize, T)> {
    let index = cards.iter().position(|card| card.code() == code)?;
    Some((index, cards.remove(index)))
}

/// Put the cards of `fresh` into `cards`: at `at` when the card was there
/// before, else in the order of the short codes (the order of the server).
fn put_cards<T: CardCode>(cards: &mut Vec<T>, fresh: Vec<T>, at: Option<usize>) {
    for card in fresh {
        let index = at
            .filter(|&at| at <= cards.len())
            .unwrap_or_else(|| cards.partition_point(|other| other.code() < card.code()));
        cards.insert(index, card);
    }
}

impl BoardColumnItems {
    /// Take the card `code` out of this column: its index in its list, and
    /// a column with that card only.
    fn take_card(&mut self, code: &str) -> Option<(usize, BoardColumnItems)> {
        let mut one = BoardColumnItems {
            column: self.column.clone(),
            item_count: 0,
            over_wip_limit: false,
            strategies: Vec::new(),
            initiatives: Vec::new(),
            tasks: Vec::new(),
            adrs: Vec::new(),
        };
        let index = if let Some((index, card)) = take_card(&mut self.strategies, code) {
            one.strategies.push(card);
            index
        } else if let Some((index, card)) = take_card(&mut self.initiatives, code) {
            one.initiatives.push(card);
            index
        } else if let Some((index, card)) = take_card(&mut self.tasks, code) {
            one.tasks.push(card);
            index
        } else {
            let (index, card) = take_card(&mut self.adrs, code)?;
            one.adrs.push(card);
            index
        };
        Some((index, one))
    }

    /// Add `delta` to the live count of the column, and mark the WIP limit
    /// again (the rule of the server, `kairos_db::board_items`).
    fn add_count(&mut self, delta: i64) {
        self.item_count = (self.item_count + delta).max(0);
        self.over_wip_limit = self
            .column
            .wip_limit
            .is_some_and(|limit| self.item_count > i64::from(limit));
    }

    /// Whether this column has a card.
    fn has_cards(&self) -> bool {
        !(self.strategies.is_empty()
            && self.initiatives.is_empty()
            && self.tasks.is_empty()
            && self.adrs.is_empty())
    }

    /// Put the cards of `fresh` (the same column) into this column.
    fn put_cards(&mut self, fresh: BoardColumnItems, at: Option<usize>) {
        put_cards(&mut self.strategies, fresh.strategies, at);
        put_cards(&mut self.initiatives, fresh.initiatives, at);
        put_cards(&mut self.tasks, fresh.tasks, at);
        put_cards(&mut self.adrs, fresh.adrs, at);
    }
}

/// What [`BoardItemsResponse::patch_card`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Patched {
    /// The view has the card as the server has it now.
    Card,
    /// The patch of one card is not enough: read the whole board.
    Board,
}

impl BoardItemsResponse {
    /// Replace the card `code` with the card of `page`, the response of
    /// [`load_board_card`] (KAIROS-T-0615). A `page` with no card takes
    /// the card out: it left the board, or it was put away. The counts of
    /// the columns and the map entries of the card come from `page`.
    ///
    /// [`Patched::Board`] when one card is not enough:
    /// - the columns of `page` are not the columns of the view (the
    ///   configuration of the board changed);
    /// - the card moved, or came, or went, and it has a "blocks" count
    ///   before or after. The counts of the other end change too, and the
    ///   other end gets no event.
    ///
    /// A view without each card ([`BOARD_CARD_CAP`]) does not take a new
    /// card: a full read would not show it either. Pure, host-tested.
    pub fn patch_card(&mut self, code: &str, page: BoardItemsResponse) -> Patched {
        let same_columns = self.columns.len() == page.columns.len()
            && self
                .columns
                .iter()
                .zip(&page.columns)
                .all(|(mine, fresh)| mine.column == fresh.column);
        if !same_columns {
            return Patched::Board;
        }
        let capped = usize::try_from(self.total).is_ok_and(|total| self.card_count() < total);
        let before: Option<(usize, usize)> = self
            .columns
            .iter_mut()
            .enumerate()
            .find_map(|(column, group)| group.take_card(code).map(|(index, _)| (column, index)));
        let after = page.columns.iter().position(BoardColumnItems::has_cards);
        let had_blocks = self.blocks_summary.remove(code).is_some();
        self.children_progress.remove(code);
        self.item_teams.remove(code);
        if let Some(metadata) = self.item_metadata.as_mut() {
            metadata.remove(code);
        }
        let has_blocks = page.blocks_summary.contains_key(code);
        for (column, (mine, fresh)) in self.columns.iter_mut().zip(page.columns).enumerate() {
            mine.item_count = fresh.item_count;
            mine.over_wip_limit = fresh.over_wip_limit;
            if Some(column) == after && !(capped && before.is_none()) {
                let at = before
                    .filter(|(old, _)| *old == column)
                    .map(|(_, index)| index);
                mine.put_cards(fresh, at);
            }
        }
        if !(capped && before.is_none()) {
            self.children_progress.extend(page.children_progress);
            self.blocks_summary.extend(page.blocks_summary);
            self.item_teams.extend(page.item_teams);
            if let Some(metadata) = page.item_metadata {
                self.item_metadata
                    .get_or_insert_with(Default::default)
                    .extend(metadata);
            }
        }
        self.total += i64::from(after.is_some()) - i64::from(before.is_some());
        let moved = before.map(|(column, _)| column) != after;
        if moved && (had_blocks || has_blocks) {
            Patched::Board
        } else {
            Patched::Card
        }
    }
}

/// The kind of a change of the configuration of a board (KAIROS-T-0574).
pub const BOARD_CONFIGURED: &str = "board_configured";

// ---- optimistic drops (KAIROS-T-0619) ----------------------------------------

/// Where a card was before a drop moved it in the view, for
/// [`BoardItemsResponse::undo_move`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardMove {
    code: String,
    /// The column of the card before the move.
    from_column: String,
    /// The place of the card in its list before the move.
    from_index: usize,
    /// The lane of a task before the move; `None` for the other kinds.
    from_work_class: Option<String>,
}

impl BoardItemsResponse {
    /// The index of the column `id`.
    fn column_index(&self, id: &str) -> Option<usize> {
        self.columns.iter().position(|group| group.column.id == id)
    }

    /// Take the card `code` out of the view: its column, its index in its
    /// list, and a column with that card only.
    fn take_out(&mut self, code: &str) -> Option<(usize, usize, BoardColumnItems)> {
        self.columns
            .iter_mut()
            .enumerate()
            .find_map(|(column, group)| {
                group
                    .take_card(code)
                    .map(|(index, one)| (column, index, one))
            })
    }

    /// Put the card of `one` into the column `to`. It came from the column
    /// `from`: the counts of both columns change when they differ.
    fn put_in(&mut self, from: usize, to: usize, one: BoardColumnItems, at: Option<usize>) {
        if from != to {
            self.columns[from].add_count(-1);
            self.columns[to].add_count(1);
        }
        self.columns[to].put_cards(one, at);
    }

    /// Move the card `code` in the view at once, before the server takes
    /// the drop: to the column `to_column`, and a task to the lane
    /// `work_class`. In a new column the card goes in the order of the
    /// codes; in its own column it keeps its place. The counts of the
    /// columns and the WIP marks follow.
    ///
    /// `None`: the view has no such card or no such column, and nothing
    /// changes. Pure, host-tested.
    pub fn move_card(
        &mut self,
        code: &str,
        to_column: Option<&str>,
        work_class: Option<&str>,
    ) -> Option<CardMove> {
        let to = match to_column {
            Some(id) => Some(self.column_index(id)?),
            None => None,
        };
        let (from, index, mut one) = self.take_out(code)?;
        let from_work_class = one.tasks.first().map(|task| task.work_class.clone());
        if let (Some(task), Some(work_class)) = (one.tasks.first_mut(), work_class) {
            task.work_class = work_class.to_string();
        }
        let undo = CardMove {
            code: code.to_string(),
            from_column: self.columns[from].column.id.clone(),
            from_index: index,
            from_work_class,
        };
        let to = to.unwrap_or(from);
        self.put_in(from, to, one, (to == from).then_some(index));
        Some(undo)
    }

    /// Undo the parts of a move that the server refused: `column` puts the
    /// card back in its column and its place, `lane` puts back the lane of
    /// a task. The other fields of the card stay as the view has them now:
    /// a read can land while the drop runs. A card that left the view
    /// stays out. Pure, host-tested.
    pub fn undo_move(&mut self, undo: &CardMove, column: bool, lane: bool) {
        let Some((now, index, mut one)) = self.take_out(&undo.code) else {
            return;
        };
        if lane
            && let (Some(task), Some(work_class)) =
                (one.tasks.first_mut(), undo.from_work_class.as_ref())
        {
            task.work_class = work_class.clone();
        }
        let (to, at) = match self.column_index(&undo.from_column).filter(|_| column) {
            Some(to) => (to, undo.from_index),
            None => (now, index),
        };
        self.put_in(now, to, one, Some(at));
    }
}

/// The drops that run (KAIROS-T-0619), by the short code of the card. The
/// view shows the card where the drop put it. A read of that card that
/// lands while its drop runs could show the card where it was, so the
/// read waits: the end of the drop asks for it again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DropsInFlight {
    /// The code of each card with a drop that runs: `true` when a read of
    /// the card waits.
    cards: std::collections::BTreeMap<String, bool>,
}

impl DropsInFlight {
    /// A drop of the card `code` starts. `false`: a drop of this card runs
    /// already, and the new drop does not start.
    pub fn start(&mut self, code: &str) -> bool {
        if self.cards.contains_key(code) {
            return false;
        }
        self.cards.insert(code.to_string(), false);
        true
    }

    /// A read of the card `code` lands. `true`: a drop of the card runs,
    /// so the view keeps the card as it is and the read waits.
    pub fn holds(&mut self, code: &str) -> bool {
        match self.cards.get_mut(code) {
            Some(waits) => {
                *waits = true;
                true
            }
            None => false,
        }
    }

    /// The drop of the card `code` ends. `true`: a read of the card waited,
    /// so read the card now.
    pub fn end(&mut self, code: &str) -> bool {
        self.cards.remove(code).unwrap_or(false)
    }
}

/// What one event of the board asks the view to read (KAIROS-T-0615).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// The card of this short code.
    Card(String),
    /// The detail and each card of the board.
    Board,
}

/// The read of an event. `None` is a reconnect: the view reads the board,
/// for the events that it did not get. An event of an item of a different
/// board changes the "blocks" counts of cards that the event does not
/// name, so the view reads the board. `board_configured` (KAIROS-T-0574)
/// names the board, not a card: the columns changed, so the view reads
/// the board. Each other kind of a card of the board (`claim_changed`,
/// `task_state_changed`, `task_cancelled` included) reads the card. Pure,
/// host-tested.
pub fn refresh_of(event: Option<&ThinEvent>, board_id: &str) -> Refresh {
    match event {
        Some(event)
            if !event.short_code.is_empty()
                && event.event != BOARD_CONFIGURED
                && event.board_id.as_deref() == Some(board_id) =>
        {
            Refresh::Card(event.short_code.clone())
        }
        _ => Refresh::Board,
    }
}

/// The time that the view waits after the first event, for more events
/// of a burst (milliseconds).
pub const LIVE_WINDOW_MS: u64 = 250;

/// Above this number of cards in one window, the view reads the board
/// and not each card.
pub const MAX_CARD_READS: usize = 20;

/// The reads of one window (KAIROS-T-0615).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Batch {
    /// Read the detail and each card of the board.
    Board,
    /// Read these cards, one request each.
    Cards(Vec<String>),
}

/// The live reads of a board view (KAIROS-T-0615). The events of a
/// [`LIVE_WINDOW_MS`] window make one [`Batch`], and one batch runs at a
/// time. The events that come while a batch runs go to the next batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveQueue {
    /// The board must be read.
    board: bool,
    /// The cards to read.
    cards: std::collections::BTreeSet<String>,
    /// A window is open: its timer calls [`LiveQueue::fire`].
    armed: bool,
    /// The number of reads that run.
    running: u32,
    /// [`LiveQueue::take`] gave a [`Batch::Board`], and the read of the
    /// board did not start yet: its start does not count again.
    board_given: bool,
}

impl LiveQueue {
    /// An event asks for a read. `true`: open a window now (start the
    /// timer).
    pub fn request(&mut self, refresh: Refresh) -> bool {
        self.ask(refresh);
        !std::mem::replace(&mut self.armed, true)
    }

    /// Ask for a read, with no window: the next batch has it.
    pub fn ask(&mut self, refresh: Refresh) {
        match refresh {
            Refresh::Board => self.board = true,
            Refresh::Card(code) => {
                self.cards.insert(code);
            }
        }
    }

    /// The timer of the window fired: the batch to run now, or `None`.
    pub fn fire(&mut self) -> Option<Batch> {
        self.armed = false;
        self.take()
    }

    /// The batch to run now: `None` when a read runs or nothing waits.
    fn take(&mut self) -> Option<Batch> {
        if self.running > 0 {
            return None;
        }
        let batch = if self.board || self.cards.len() > MAX_CARD_READS {
            self.board_given = true;
            Batch::Board
        } else if self.cards.is_empty() {
            return None;
        } else {
            Batch::Cards(self.cards.iter().cloned().collect())
        };
        self.board = false;
        self.cards.clear();
        self.running = self.running.saturating_add(1);
        Some(batch)
    }

    /// A read of the board starts: the first load, a change of the route,
    /// or a [`Batch::Board`].
    pub fn board_read_started(&mut self) {
        if !std::mem::take(&mut self.board_given) {
            self.running = self.running.saturating_add(1);
        }
    }

    /// A read ends. The batch to run now, or `None` (an open window gives
    /// the next batch when its timer fires).
    pub fn finished(&mut self) -> Option<Batch> {
        self.running = self.running.saturating_sub(1);
        if self.armed { None } else { self.take() }
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
///
/// KAIROS-T-0615: the detail and the cards load together. The two
/// requests run at the same time, and the view waits for the two.
pub async fn load_board_view(
    auth: Auth,
    param: &str,
    done_limit: Option<usize>,
) -> Result<BoardView, ApiError> {
    let path = board_path(param);
    // COLLIERY-T-0261: the items come in pages.
    let (detail, items) = futures_util::future::try_join(
        get_json::<BoardDetail>(auth, &path),
        load_board_items(auth, param, done_limit),
    )
    .await?;
    Ok(BoardView { detail, items })
}

/// The card of one item of a board (KAIROS-T-0615):
/// `GET /api/boards/{id}/items?code=`. The response has each column with
/// its counts, and the card in its column, or no card when the item is not
/// on the board now. [`BoardItemsResponse::patch_card`] puts it in the
/// view.
pub async fn load_board_card(
    auth: Auth,
    board: &str,
    code: &str,
) -> Result<BoardItemsResponse, ApiError> {
    get_json(auth, &board_card_path(board, code)).await
}

/// The path of the pages of [`load_board_items`], before `limit` and
/// `offset`. The cards come with their metadata (KAIROS-T-0582: the
/// priority on the card), and with `done_limit` when it is set
/// (KAIROS-T-0649). Pure, host-tested.
pub fn board_items_path(board: &str, done_limit: Option<usize>) -> String {
    let path = format!("{}/items?with_metadata=true", board_path(board));
    match done_limit {
        Some(limit) => format!("{path}&done_limit={limit}"),
        None => path,
    }
}

/// The path of [`load_board_card`]. Pure, host-tested.
pub fn board_card_path(board: &str, code: &str) -> String {
    format!(
        "{}&code={}",
        board_items_path(board, None),
        crate::api::encode_component(code)
    )
}

/// Templates for the document create flow: each template, page after
/// page (COLLIERY-T-0258).
pub async fn list_templates(auth: Auth) -> Result<Vec<Template>, ApiError> {
    crate::api::get_all(auth, "/api/templates").await
}

// ---- mutations --------------------------------------------------------------

/// mirror of: `kairos_client::types::ArchiveCompletedResponse` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ArchiveCompletedOutcome {
    pub count: i64,
}

/// `POST /api/boards/{id}/archive-completed` — archive each task in the
/// done columns of the board (KAIROS-T-0363). Needs `manage_tasks` on the
/// board. `older_than_days` keeps the tasks that entered their column
/// later (KAIROS-T-0649).
pub async fn archive_completed(
    auth: Auth,
    board_id: &str,
    older_than_days: Option<u32>,
) -> Result<ArchiveCompletedOutcome, ApiError> {
    crate::api::post_empty(auth, &archive_completed_path(board_id, older_than_days)).await
}

/// The path of [`archive_completed`]. Pure, host-tested.
pub fn archive_completed_path(board_id: &str, older_than_days: Option<u32>) -> String {
    let path = format!("{}/archive-completed", board_path(board_id));
    match older_than_days {
        Some(days) => format!("{path}?older_than_days={days}"),
        None => path,
    }
}

/// The WIP chip of a column head (KAIROS-T-0371): `WIP 2/3`, or `None`
/// for a column with no limit. Pure, host-tested.
pub fn wip_label(item_count: i64, wip_limit: Option<i32>) -> Option<String> {
    wip_limit.map(|limit| format!("WIP {item_count}/{limit}"))
}

/// The number of live tasks in the done columns of the board (the tasks
/// that "Archive completed" archives). A done column can have fewer cards
/// than its `item_count` (KAIROS-T-0649: the done limit), so the count is
/// the larger of the two. Pure, host-tested.
pub fn completed_task_count(items: &BoardItemsResponse) -> usize {
    items
        .columns
        .iter()
        .filter(|group| group.column.is_done)
        .map(|group| {
            usize::try_from(group.item_count)
                .unwrap_or(0)
                .max(group.tasks.len())
        })
        .sum()
}

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
/// updated entity) is discarded: the echo of the move patches the card
/// (KAIROS-T-0619).
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
    /// initiative: the kind of a bucket (`tech_debt`|`bug`|`ad_hoc`), or
    /// `None` for an initiative that is not a bucket (KAIROS-T-0587).
    pub bucket_type: Option<String>,
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
    /// initiative or task: the short code of the parent (KAIROS-T-0580).
    /// The server writes the edge in the transaction of the create.
    pub parent: Option<String>,
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
    /// KAIROS-T-0587: a bucket initiative; the server derives `is_bucket`.
    #[serde(skip_serializing_if = "Option::is_none")]
    bucket_type: Option<&'a str>,
    /// The parent strategy (KAIROS-T-0580).
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
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
    /// The parent initiative (KAIROS-T-0580).
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
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
                    bucket_type: item.bucket_type.as_deref(),
                    parent: item.parent.as_deref(),
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
                    parent: item.parent.as_deref(),
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

    /// KAIROS-T-0678: an initiative card reads the cancel mark; an older
    /// server sends none.
    #[test]
    fn an_initiative_card_reads_the_cancel_mark() {
        let card: Initiative = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-I-0001", "title": "Old plan", "version": 1,
            "cancellation": {"reason": "We write a new plan."}
        }))
        .expect("card");
        assert_eq!(
            card.cancellation.map(|m| m.reason),
            Some("We write a new plan.".to_string())
        );
        let old: Initiative = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-I-0001", "title": "Old plan", "version": 1
        }))
        .expect("card");
        assert_eq!(old.cancellation, None);
    }

    /// KAIROS-T-0567: the card badge counts a cancel apart from done.
    #[test]
    fn the_progress_badge_counts_cancelled_apart() {
        let counts = |done, cancelled, total, has_done| ProgressCounts {
            done,
            total,
            has_done,
            cancelled,
        };
        assert_eq!(counts(3, 1, 5, true).label(), "3/5 done, 1 cancelled");
        assert_eq!(counts(3, 0, 5, true).label(), "3/5 done");
        assert_eq!(counts(0, 0, 4, false).label(), "4 children");
        // An older server sends no `cancelled`.
        let old: ProgressCounts =
            serde_json::from_value(serde_json::json!({"done": 2, "total": 3, "has_done": true}))
                .expect("progress");
        assert_eq!(old.label(), "2/3 done");
    }

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
                    "short_code": code, "title": code, "version": 1,
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

    /// A board of 2 columns, `todo` and `doing`, with the tasks of `cards`
    /// `(code, column, version)` and the counts of each column. `blocks`
    /// are the codes with a "blocks" count.
    fn board_of(cards: &[(&str, &str, i32)], blocks: &[&str], total: i64) -> BoardItemsResponse {
        let column = |id: &str, position: i32| {
            let tasks: Vec<serde_json::Value> = cards
                .iter()
                .filter(|(_, column, _)| *column == id)
                .map(|(code, _, version)| {
                    serde_json::json!({
                        "short_code": code, "title": format!("{code} v{version}"),
                        "version": version, "task_type": "task", "work_class": "planned"
                    })
                })
                .collect();
            let count = tasks.len();
            serde_json::json!({
                "column": {"id": id, "name": id, "position": position},
                "item_count": count,
                "strategies": [], "initiatives": [], "tasks": tasks, "adrs": []
            })
        };
        let blocks: serde_json::Map<String, serde_json::Value> = blocks
            .iter()
            .map(|code| {
                (
                    code.to_string(),
                    serde_json::json!({"blocked_by": 1, "blocks": 0}),
                )
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "board": {"id": "b", "name": "Demo", "slug": "demo-delivery",
                      "board_level": "delivery"},
            "columns": [column("todo", 0), column("doing", 1)],
            "total": total,
            "blocks_summary": blocks
        }))
        .expect("mirror decodes")
    }

    /// The response of `?code=`: the card `card` (or none) and the counts
    /// of the board `counts` `(todo, doing)`.
    fn card_page(
        card: Option<(&str, &str, i32)>,
        blocks: bool,
        counts: (i64, i64),
    ) -> BoardItemsResponse {
        let cards: Vec<(&str, &str, i32)> = card.into_iter().collect();
        let codes: Vec<&str> = cards
            .iter()
            .filter(|_| blocks)
            .map(|(code, _, _)| *code)
            .collect();
        let mut page = board_of(&cards, &codes, i64::from(card.is_some()));
        page.columns[0].item_count = counts.0;
        page.columns[1].item_count = counts.1;
        page
    }

    /// The titles of the cards of a column.
    fn column_cards(view: &BoardItemsResponse, index: usize) -> Vec<String> {
        view.columns[index]
            .tasks
            .iter()
            .map(|task| task.title.clone())
            .collect()
    }

    /// KAIROS-T-0615: a note on a card replaces that card in its place.
    /// The other cards and the counts stay.
    #[test]
    fn a_patch_replaces_the_card_in_its_place() {
        let mut view = board_of(
            &[
                ("D-T-1", "todo", 1),
                ("D-T-2", "todo", 1),
                ("D-T-3", "doing", 1),
            ],
            &[],
            3,
        );
        let patched = view.patch_card(
            "D-T-2",
            card_page(Some(("D-T-2", "todo", 2)), false, (2, 1)),
        );
        assert_eq!(patched, Patched::Card);
        assert_eq!(column_cards(&view, 0), ["D-T-1 v1", "D-T-2 v2"]);
        assert_eq!(column_cards(&view, 1), ["D-T-3 v1"]);
        assert_eq!(view.total, 3);
        assert_eq!(view.columns[0].tasks[1].version, 2);
    }

    /// KAIROS-T-0615: a move takes the card to its new column, in the order
    /// of the codes, and the counts of the columns follow.
    #[test]
    fn a_patch_moves_the_card_and_the_counts() {
        let mut view = board_of(
            &[
                ("D-T-1", "todo", 1),
                ("D-T-4", "todo", 1),
                ("D-T-3", "doing", 1),
                ("D-T-5", "doing", 1),
            ],
            &[],
            4,
        );
        let patched = view.patch_card(
            "D-T-4",
            card_page(Some(("D-T-4", "doing", 2)), false, (1, 3)),
        );
        assert_eq!(patched, Patched::Card);
        assert_eq!(column_cards(&view, 0), ["D-T-1 v1"]);
        assert_eq!(column_cards(&view, 1), ["D-T-3 v1", "D-T-4 v2", "D-T-5 v1"]);
        assert_eq!(
            (view.columns[0].item_count, view.columns[1].item_count),
            (1, 3)
        );
        assert_eq!(view.total, 4);
    }

    /// KAIROS-T-0615: a card that left the board goes; a new card comes.
    #[test]
    fn a_patch_takes_out_and_puts_in_a_card() {
        let mut view = board_of(&[("D-T-1", "todo", 1), ("D-T-3", "todo", 1)], &[], 2);
        assert_eq!(
            view.patch_card("D-T-1", card_page(None, false, (1, 0))),
            Patched::Card
        );
        assert_eq!(column_cards(&view, 0), ["D-T-3 v1"]);
        assert_eq!(view.total, 1);
        assert_eq!(view.columns[0].item_count, 1);
        assert_eq!(
            view.patch_card(
                "D-T-2",
                card_page(Some(("D-T-2", "todo", 1)), false, (2, 0))
            ),
            Patched::Card
        );
        assert_eq!(column_cards(&view, 0), ["D-T-2 v1", "D-T-3 v1"]);
        assert_eq!(view.total, 2);
        // A code that is not on the board, before or after: no change.
        let before = view.clone();
        assert_eq!(
            view.patch_card("D-T-9", card_page(None, false, (2, 0))),
            Patched::Card
        );
        assert_eq!(view, before);
    }

    /// KAIROS-T-0615: a card with a "blocks" count that moves changes the
    /// counts of the other end, so the view reads the board. A note on it
    /// does not.
    #[test]
    fn a_move_of_a_blocked_card_reads_the_board() {
        let start = board_of(
            &[("D-T-1", "todo", 1), ("D-T-2", "todo", 1)],
            &["D-T-1", "D-T-2"],
            2,
        );
        let mut view = start.clone();
        assert_eq!(
            view.patch_card("D-T-1", card_page(Some(("D-T-1", "todo", 2)), true, (2, 0))),
            Patched::Card,
            "a note"
        );
        assert!(view.blocks_summary.contains_key("D-T-1"));
        let mut view = start.clone();
        // Into a done column: the count of the card goes.
        assert_eq!(
            view.patch_card(
                "D-T-1",
                card_page(Some(("D-T-1", "doing", 2)), false, (1, 1))
            ),
            Patched::Board
        );
        assert!(!view.blocks_summary.contains_key("D-T-1"));
        // Out of a done column: the count comes.
        let mut view = board_of(&[("D-T-1", "doing", 1)], &[], 1);
        assert_eq!(
            view.patch_card("D-T-1", card_page(Some(("D-T-1", "todo", 2)), true, (1, 0))),
            Patched::Board
        );
        // A card with no count moves: one card is enough.
        let mut view = board_of(&[("D-T-1", "todo", 1)], &[], 1);
        assert_eq!(
            view.patch_card(
                "D-T-1",
                card_page(Some(("D-T-1", "doing", 2)), false, (0, 1))
            ),
            Patched::Card
        );
    }

    /// The `(todo, doing)` counts of a view.
    fn counts(view: &BoardItemsResponse) -> (i64, i64) {
        (view.columns[0].item_count, view.columns[1].item_count)
    }

    /// KAIROS-T-0619: a drop moves the card at once, in the order of the
    /// codes, with the counts and the WIP mark. The undo puts it back in
    /// its place.
    #[test]
    fn a_drop_moves_the_card_and_the_undo_puts_it_back() {
        let mut view = board_of(
            &[
                ("D-T-1", "todo", 1),
                ("D-T-4", "todo", 1),
                ("D-T-6", "todo", 1),
                ("D-T-3", "doing", 1),
                ("D-T-5", "doing", 1),
            ],
            &[],
            5,
        );
        view.columns[1].column.wip_limit = Some(2);
        let start = view.clone();
        let undo = view
            .move_card("D-T-4", Some("doing"), None)
            .expect("the card is on the board");
        assert_eq!(column_cards(&view, 0), ["D-T-1 v1", "D-T-6 v1"]);
        assert_eq!(column_cards(&view, 1), ["D-T-3 v1", "D-T-4 v1", "D-T-5 v1"]);
        assert_eq!(counts(&view), (2, 3));
        assert!(view.columns[1].over_wip_limit);
        assert_eq!(view.total, 5);

        view.undo_move(&undo, true, true);
        assert_eq!(view, start);
    }

    /// KAIROS-T-0619: a lane drop keeps the place of the card in its
    /// column and changes no count. The undo puts back the lane.
    #[test]
    fn a_lane_drop_keeps_the_place_and_the_undo_puts_back_the_lane() {
        let mut view = board_of(&[("D-T-1", "todo", 1), ("D-T-2", "todo", 1)], &[], 2);
        let start = view.clone();
        let undo = view
            .move_card("D-T-1", None, Some("support"))
            .expect("the card is on the board");
        assert_eq!(column_cards(&view, 0), ["D-T-1 v1", "D-T-2 v1"]);
        assert_eq!(view.columns[0].tasks[0].work_class, "support");
        assert_eq!(counts(&view), (2, 0));

        view.undo_move(&undo, false, true);
        assert_eq!(view, start);
    }

    /// KAIROS-T-0619: a diagonal drop moves the column and the lane at
    /// once. The server took the move and refused the lane: the undo puts
    /// back the lane only.
    #[test]
    fn the_undo_of_a_diagonal_drop_puts_back_what_the_server_refused() {
        let mut view = board_of(&[("D-T-1", "todo", 1), ("D-T-3", "doing", 1)], &[], 2);
        let start = view.clone();
        let undo = view
            .move_card("D-T-1", Some("doing"), Some("support"))
            .expect("the card is on the board");
        assert_eq!(column_cards(&view, 1), ["D-T-1 v1", "D-T-3 v1"]);
        assert_eq!(view.columns[1].tasks[0].work_class, "support");

        let mut lane_refused = view.clone();
        lane_refused.undo_move(&undo, false, true);
        assert_eq!(column_cards(&lane_refused, 1), ["D-T-1 v1", "D-T-3 v1"]);
        assert_eq!(lane_refused.columns[1].tasks[0].work_class, "planned");
        assert_eq!(counts(&lane_refused), (0, 2));

        // The server refused the move: no lane write was sent.
        view.undo_move(&undo, true, true);
        assert_eq!(view, start);
    }

    /// KAIROS-T-0619: the undo reads the card where the view has it now. A
    /// read of the board during the drop already put it back: the counts
    /// do not change twice. A card that left the view stays out.
    #[test]
    fn the_undo_starts_from_the_view_as_it_is_now() {
        let start = board_of(&[("D-T-1", "todo", 1), ("D-T-3", "doing", 1)], &[], 2);
        let mut view = start.clone();
        let undo = view
            .move_card("D-T-1", Some("doing"), None)
            .expect("the card is on the board");
        // A read of the board landed during the drop.
        let mut read = start.clone();
        read.undo_move(&undo, true, true);
        assert_eq!(read, start);

        view.patch_card("D-T-1", card_page(None, false, (0, 1)));
        let gone = view.clone();
        view.undo_move(&undo, true, true);
        assert_eq!(view, gone);
    }

    /// KAIROS-T-0619: a drop of a card or a column that the view does not
    /// have changes nothing.
    #[test]
    fn a_drop_of_an_unknown_card_or_column_changes_nothing() {
        let mut view = board_of(&[("D-T-1", "todo", 1)], &[], 1);
        let start = view.clone();
        assert_eq!(view.move_card("D-T-9", Some("doing"), None), None);
        assert_eq!(view.move_card("D-T-1", Some("gone"), None), None);
        assert_eq!(view, start);
    }

    /// KAIROS-T-0619: a read of a card that lands while its drop runs
    /// waits, and the end of the drop asks for it. A second drop of the
    /// card does not start.
    #[test]
    fn a_read_of_a_card_waits_for_the_end_of_its_drop() {
        let mut drops = DropsInFlight::default();
        assert!(drops.start("D-T-1"));
        assert!(!drops.start("D-T-1"));
        assert!(!drops.holds("D-T-2"));
        assert!(!drops.end("D-T-1"), "no read waited");

        assert!(drops.start("D-T-1"));
        assert!(drops.holds("D-T-1"));
        assert!(drops.end("D-T-1"), "a read waited");
        assert!(!drops.holds("D-T-1"), "the drop ended");
        assert_eq!(drops, DropsInFlight::default());
    }

    /// KAIROS-T-0615: columns that are not the columns of the view (the
    /// configuration changed) read the board.
    #[test]
    fn other_columns_read_the_board() {
        let mut view = board_of(&[("D-T-1", "todo", 1)], &[], 1);
        let mut page = card_page(Some(("D-T-1", "todo", 2)), false, (1, 0));
        page.columns[1].column.name = "Doing now".to_string();
        assert_eq!(view.patch_card("D-T-1", page), Patched::Board);
        let mut page = card_page(Some(("D-T-1", "todo", 2)), false, (1, 0));
        page.columns.pop();
        assert_eq!(view.patch_card("D-T-1", page), Patched::Board);
    }

    /// KAIROS-T-0615: a view without each card does not take a new card,
    /// as a full read would not show it. The total counts it.
    #[test]
    fn a_capped_view_does_not_take_a_new_card() {
        let mut view = board_of(&[("D-T-1", "todo", 1)], &[], 2500);
        assert_eq!(
            view.patch_card(
                "D-T-9",
                card_page(Some(("D-T-9", "todo", 1)), false, (2, 0))
            ),
            Patched::Card
        );
        assert_eq!(column_cards(&view, 0), ["D-T-1 v1"]);
        assert_eq!(view.total, 2501);
        assert_eq!(view.columns[0].item_count, 2);
        // A card that the view has is patched.
        view.patch_card(
            "D-T-1",
            card_page(Some(("D-T-1", "todo", 2)), false, (2, 0)),
        );
        assert_eq!(column_cards(&view, 0), ["D-T-1 v2"]);
    }

    /// KAIROS-T-0615: an event of an item of this board reads its card. A
    /// reconnect, an event with no item and an event of a different board
    /// read the board.
    #[test]
    fn an_event_reads_its_card_or_the_board() {
        let event = |code: &str, board: Option<&str>| ThinEvent {
            event: "item_updated".to_string(),
            entity_type: "task".to_string(),
            short_code: code.to_string(),
            board_id: board.map(str::to_string),
        };
        assert_eq!(
            refresh_of(Some(&event("D-T-1", Some("b"))), "b"),
            Refresh::Card("D-T-1".to_string())
        );
        assert_eq!(
            refresh_of(Some(&event("D-T-1", Some("other"))), "b"),
            Refresh::Board
        );
        assert_eq!(refresh_of(Some(&event("", Some("b"))), "b"), Refresh::Board);
        assert_eq!(refresh_of(Some(&event("D-D-1", None)), "b"), Refresh::Board);
        assert_eq!(refresh_of(None, "b"), Refresh::Board);
        // KAIROS-T-0574: the precise kinds of a card read the card, and a
        // change of the board configuration reads the board.
        for kind in ["claim_changed", "task_state_changed", "task_cancelled"] {
            let precise = ThinEvent {
                event: kind.to_string(),
                ..event("D-T-1", Some("b"))
            };
            assert_eq!(
                refresh_of(Some(&precise), "b"),
                Refresh::Card("D-T-1".to_string()),
                "{kind}"
            );
        }
        let configured = ThinEvent {
            event: BOARD_CONFIGURED.to_string(),
            ..event("demo", Some("b"))
        };
        assert_eq!(refresh_of(Some(&configured), "b"), Refresh::Board);
        assert_eq!(
            board_card_path("demo", "D-T-1"),
            "/api/boards/demo/items?with_metadata=true&code=D-T-1"
        );
    }

    /// KAIROS-T-0615: the events of a window make one batch, and one batch
    /// runs at a time.
    #[test]
    fn the_queue_makes_one_batch_of_a_window() {
        let card = |code: &str| Refresh::Card(code.to_string());
        let mut queue = LiveQueue::default();
        // The first load runs.
        queue.board_read_started();
        // An event opens a window; the next ones join it.
        assert!(queue.request(card("D-T-2")));
        assert!(!queue.request(card("D-T-1")));
        assert!(!queue.request(card("D-T-2")));
        // The window ends while the first load runs: no batch yet.
        assert_eq!(queue.fire(), None);
        // The load ends: the batch of the window runs, each card once.
        assert_eq!(
            queue.finished(),
            Some(Batch::Cards(vec!["D-T-1".to_string(), "D-T-2".to_string()]))
        );
        // An event during the batch opens a new window.
        assert!(queue.request(card("D-T-3")));
        // The batch ends while the window is open: the timer runs it.
        assert_eq!(queue.finished(), None);
        assert_eq!(queue.fire(), Some(Batch::Cards(vec!["D-T-3".to_string()])));
        assert_eq!(queue.finished(), None, "nothing waits");
        // A reconnect in a window of cards: the board.
        assert!(queue.request(card("D-T-4")));
        assert!(!queue.request(Refresh::Board));
        assert_eq!(queue.fire(), Some(Batch::Board));
        // The read of the board starts and ends: it counts once.
        queue.board_read_started();
        assert!(queue.request(card("D-T-5")));
        assert_eq!(queue.fire(), None, "the read of the board runs");
        assert_eq!(
            queue.finished(),
            Some(Batch::Cards(vec!["D-T-5".to_string()]))
        );
        assert_eq!(queue.finished(), None);
        // A burst of more than MAX_CARD_READS cards: the board.
        for number in 0..=MAX_CARD_READS {
            queue.request(card(&format!("D-T-{number:04}")));
        }
        assert_eq!(queue.fire(), Some(Batch::Board));
    }

    /// The board-view mirrors decode a realistic
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
            parent: None,
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
            parent: None,
        })
        .expect("serializes");
        assert_eq!(
            bound,
            serde_json::json!({"board_id": "b-1", "column_id": "c-1",
                               "title": "T", "content": "", "team_id": "team-1",
                               "repository": "payments-api"})
        );

        // KAIROS-T-0580: the parent rides as `parent`, on a task and on an
        // initiative.
        let child = serde_json::to_value(CreateTaskRequest {
            board_id: "b-1",
            column_id: "c-1",
            title: "T",
            content: "",
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            parent: Some("DEMO-I-0001"),
        })
        .expect("serializes");
        assert_eq!(child["parent"], "DEMO-I-0001");
        let initiative = serde_json::to_value(CreateInitiativeRequest {
            board_id: "b-2",
            column_id: "c-2",
            title: "I",
            content: "",
            complexity: None,
            bucket_type: None,
            parent: Some("DEMO-S-0001"),
        })
        .expect("serializes");
        assert_eq!(
            initiative,
            serde_json::json!({"board_id": "b-2", "column_id": "c-2",
                               "title": "I", "content": "", "parent": "DEMO-S-0001"})
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
    /// KAIROS-T-0371: the WIP chip.
    #[test]
    fn the_wip_label_names_the_count_and_the_limit() {
        assert_eq!(wip_label(2, None), None);
        assert_eq!(wip_label(4, Some(3)).as_deref(), Some("WIP 4/3"));
    }

    /// KAIROS-T-0411: a card initiative or strategy decodes its owner, and
    /// the card shows it as a pill. A body with no owner has no pill.
    #[test]
    fn the_owner_decodes_and_makes_a_pill() {
        let initiative: Initiative = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-I-0001", "title": "I", "version": 1,
            "owner": {"user_id": "u", "display_name": "Robin",
                      "claimed_at": "2026-10-12T10:00:00Z"}
        }))
        .expect("mirror decodes");
        assert_eq!(
            owner_pill(initiative.owner.as_ref()).as_deref(),
            Some("owner: Robin")
        );
        let strategy: Strategy = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-S-0001", "title": "S", "version": 1
        }))
        .expect("mirror decodes");
        assert_eq!(owner_pill(strategy.owner.as_ref()), None);
    }

    /// older server (no `claim`) decodes with none.
    #[test]
    fn task_mirror_decodes_the_claim() {
        let task: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0001", "title": "T", "task_type": "task", "version": 1,
            "work_class": "planned",
            "claim": {"user_id": "u", "display_name": "Alice", "agent": true,
                      "claimed_at": "2026-10-09T10:00:00Z"}
        }))
        .expect("mirror decodes");
        let claim = task.claim.expect("the claim");
        assert_eq!(claim.display_name, "Alice");
        assert!(claim.agent);
        let old: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0002", "title": "T", "task_type": "task", "version": 1,
            "work_class": "planned"
        }))
        .expect("mirror decodes");
        assert_eq!(old.claim, None);
        assert_eq!(old.cancellation, None);
    }

    /// KAIROS-T-0363: a column decodes `is_done`, and a column of an older
    /// server is not done. The count of "Archive completed" is the tasks
    /// of the done columns only.
    #[test]
    fn completed_tasks_are_the_tasks_of_the_done_columns() {
        let column = |name: &str, done: bool, tasks: &[&str]| {
            serde_json::json!({
                "column": {"id": name, "name": name, "position": 1, "is_done": done},
                "strategies": [], "initiatives": [], "adrs": [],
                "tasks": tasks.iter().map(|code| serde_json::json!({
                    "short_code": code, "title": "T", "task_type": "task", "version": 1,
                    "work_class": "planned"
                })).collect::<Vec<_>>(),
            })
        };
        let items: BoardItemsResponse = serde_json::from_value(serde_json::json!({
            "board": {"id": "b", "name": "B", "slug": "b", "board_level": "delivery"},
            "columns": [
                column("Active", false, &["DEMO-T-0001"]),
                column("Completed", true, &["DEMO-T-0002", "DEMO-T-0003"]),
            ],
            "total": 3, "limit": 200, "offset": 0
        }))
        .expect("items decode");
        assert_eq!(completed_task_count(&items), 2);
        let old: BoardColumn =
            serde_json::from_value(serde_json::json!({"id": "c", "name": "Todo", "position": 1}))
                .expect("column decodes");
        assert!(!old.is_done);
    }

    /// KAIROS-T-0362: a card task decodes its cancel mark.
    #[test]
    fn task_mirror_decodes_the_cancel_mark() {
        let task: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0001", "title": "T", "task_type": "task", "version": 1,
            "work_class": "planned",
            "cancellation": {"reason": "No need", "cancelled_by": "u",
                             "cancelled_by_name": "Robin",
                             "cancelled_at": "2026-10-09T10:00:00Z"}
        }))
        .expect("mirror decodes");
        assert_eq!(task.cancellation.expect("the mark").reason, "No need");
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

    /// `metadata` of a response: the priority of each code.
    fn with_priority(mut page: BoardItemsResponse, values: &[(&str, &str)]) -> BoardItemsResponse {
        page.item_metadata = Some(
            values
                .iter()
                .map(|(code, value)| {
                    (
                        code.to_string(),
                        [(PRIORITY_SLUG.to_string(), value.to_string())].into(),
                    )
                })
                .collect(),
        );
        page
    }

    /// KAIROS-T-0582: the board reads the metadata of the cards.
    #[test]
    fn the_board_reads_the_metadata() {
        assert_eq!(
            board_items_path("demo", None),
            "/api/boards/demo/items?with_metadata=true"
        );
        assert_eq!(
            crate::api::page_url(&board_items_path("demo", None), 1000, 0),
            "/api/boards/demo/items?with_metadata=true&limit=1000&offset=0"
        );
    }

    /// KAIROS-T-0582: the metadata of a page joins the view, and a patch
    /// replaces the metadata of its card only.
    #[test]
    fn the_metadata_follows_the_pages_and_the_patches() {
        let mut view = with_priority(
            board_of(&[("D-T-1", "todo", 1), ("D-T-2", "todo", 1)], &[], 3),
            &[("D-T-1", "high"), ("D-T-2", "low")],
        );
        assert_eq!(view.metadata_value("D-T-1", PRIORITY_SLUG), Some("high"));
        assert_eq!(view.metadata_value("D-T-9", PRIORITY_SLUG), None);
        assert_eq!(view.metadata_value("D-T-1", "size"), None);

        view.add_page(with_priority(
            board_of(&[("D-T-3", "doing", 1)], &[], 3),
            &[("D-T-3", "medium")],
        ));
        assert_eq!(view.metadata_value("D-T-3", PRIORITY_SLUG), Some("medium"));

        // A new priority of D-T-1.
        let page = with_priority(
            card_page(Some(("D-T-1", "todo", 2)), false, (2, 1)),
            &[("D-T-1", "low")],
        );
        assert_eq!(view.patch_card("D-T-1", page), Patched::Card);
        assert_eq!(view.metadata_value("D-T-1", PRIORITY_SLUG), Some("low"));
        assert_eq!(view.metadata_value("D-T-2", PRIORITY_SLUG), Some("low"));

        // The priority of D-T-2 goes: the card read has no entry.
        let page = with_priority(card_page(Some(("D-T-2", "todo", 2)), false, (2, 1)), &[]);
        view.patch_card("D-T-2", page);
        assert_eq!(view.metadata_value("D-T-2", PRIORITY_SLUG), None);
        assert_eq!(view.metadata_value("D-T-3", PRIORITY_SLUG), Some("medium"));
    }

    /// An older server sends no metadata: the view has none.
    #[test]
    fn no_metadata_is_no_value() {
        let view = board_of(&[("D-T-1", "todo", 1)], &[], 1);
        assert_eq!(view.item_metadata, None);
        assert_eq!(view.metadata_value("D-T-1", PRIORITY_SLUG), None);
    }

    /// KAIROS-T-0649: the board asks for the last done cards, a card read
    /// asks for one card, and the archive path takes the age.
    #[test]
    fn the_done_limit_and_the_age_go_in_the_paths() {
        assert_eq!(
            board_items_path("demo", Some(20)),
            "/api/boards/demo/items?with_metadata=true&done_limit=20"
        );
        assert_eq!(
            board_card_path("demo", "D-T-1"),
            "/api/boards/demo/items?with_metadata=true&code=D-T-1"
        );
        assert_eq!(
            archive_completed_path("demo", None),
            "/api/boards/demo/archive-completed"
        );
        assert_eq!(
            archive_completed_path("demo", Some(30)),
            "/api/boards/demo/archive-completed?older_than_days=30"
        );
    }

    /// KAIROS-T-0649: a card decodes its time in the column and its notes
    /// count; a card of an older server has neither. A done column with
    /// fewer cards than its `item_count` counts its `item_count`.
    #[test]
    fn a_card_decodes_its_marks_and_the_count_is_that_of_the_column() {
        let task: Task = serde_json::from_value(serde_json::json!({
            "short_code": "DEMO-T-0001", "title": "T", "task_type": "task", "version": 1,
            "work_class": "planned", "entered_column_at": "2026-10-09T10:00:00+00:00",
            "notes_count": 3
        }))
        .expect("mirror decodes");
        assert_eq!(
            task.entered_column_at.as_deref(),
            Some("2026-10-09T10:00:00+00:00")
        );
        assert_eq!(task.notes_count, Some(3));
        let old: Adr = serde_json::from_value(
            serde_json::json!({"short_code": "DEMO-A-0001", "title": "A", "version": 1}),
        )
        .expect("mirror decodes");
        assert_eq!((old.entered_column_at, old.notes_count), (None, None));

        let items: BoardItemsResponse = serde_json::from_value(serde_json::json!({
            "board": {"id": "b", "name": "B", "slug": "b", "board_level": "delivery"},
            "columns": [{
                "column": {"id": "c", "name": "Completed", "position": 1, "is_done": true},
                "item_count": 40,
                "strategies": [], "initiatives": [], "adrs": [],
                "tasks": [{"short_code": "DEMO-T-0002", "title": "T", "task_type": "task",
                           "version": 1, "work_class": "planned"}],
            }],
            "total": 1, "limit": 200, "offset": 0
        }))
        .expect("items decode");
        assert_eq!(completed_task_count(&items), 40);
    }
}
