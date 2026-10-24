//! Boards area (KAIROS-T-0040): the board list (`/boards`) and the board
//! view (`/boards/:board` — slug or id) with live `/ws/events` updates.
//!
//! Shape (per docs/gui-conventions.md and A-0015):
//! - columns come from the board's configuration; items arrive grouped by
//!   column from `GET /api/boards/{id}/items` (all four board entity
//!   types: strategies, initiatives, tasks, ADRs);
//! - **transitions are drag-and-drop** (KAIROS-T-0064/T-0075): a card
//!   drags only to the columns the board's `transitions` allow from its
//!   current column (A-0002: invalid moves are never offered); the
//!   keyboard-accessible path is the item detail page's move control;
//!   mutation errors surface in a page-level `Banner`;
//! - live updates: a board-filtered `/ws/events` subscription re-fetches
//!   the grouped items on every event (server state is the source of
//!   truth; `LocalResource` keeps the last value while re-fetching, so
//!   the refresh is silent — no polling, no loading flash);
//! - create-from-column makes the board level's entity type (A-0002);
//!   **documents are off-board** (no `board_id`/`column_id`), so document
//!   creation lives in the board header ("New document": a template picker
//!   and a parent picker over this board's items — POST /api/documents
//!   requires `parent_short_code`), hidden on ADR boards (documents
//!   attach to strategies/initiatives/tasks only).

pub(crate) mod data;
mod flow;
pub(crate) mod live;

use aurora_dark::components::{
    Alert, Anchor, Button, Chip, Empty, ErrorState, Group, Loading, PageHeader, Pill, Select,
    Stack, Text, TextInput, Textarea,
};
use aurora_dark::frame::{Card, ConfirmDialog, Modal, use_toaster};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{query_signal_with_options, use_params_map};

use super::copy_link;
use super::item::code_picker::CodePicker;
use super::repositories;
use crate::auth::use_auth;
use data::EntityKind;
use repositories::api::NO_REPOSITORY;

/// A short human line for a failed mutation (page-level `Banner`; load
/// failures render `ErrorState` instead, which classifies internally).
fn describe(error: &ApiError) -> String {
    match error {
        ApiError::Http { code, message, .. } => match code {
            Some(code) => format!("{code}: {message}"),
            None => message.clone(),
        },
        ApiError::Network => "The page cannot connect to the server.".to_string(),
        ApiError::Unknown(message) => message.clone(),
    }
}

/// The accent token for a board level / entity kind.
fn level_color(level: &str) -> &'static str {
    match level {
        "strategy" => token::VIOLET,
        "initiative" => token::ICE,
        "delivery" => token::TEAL,
        "adr" => token::GOLD,
        _ => token::SKIP,
    }
}

fn kind_color(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Strategy => token::VIOLET,
        EntityKind::Initiative => token::ICE,
        EntityKind::Task => token::TEAL,
        EntityKind::Adr => token::GOLD,
    }
}

/// The in-flight card drag (KAIROS-T-0064): which card, where it started,
/// and the column ids its CURRENT column's transitions allow as drop
/// targets — so only legal drop zones light up and accept the drop
/// (A-0002: invalid moves are never offered). `source_column` and
/// `work_class` make (column, lane) drops decidable (KAIROS-T-0077).
#[derive(Clone, Debug, PartialEq)]
struct DragData {
    kind: EntityKind,
    short_code: String,
    targets: Vec<String>,
    /// The column the card is dragged FROM.
    source_column: String,
    /// The card's current lane — `Some` for tasks, `None` for kinds that
    /// carry no lane (and therefore cannot lane-move).
    work_class: Option<String>,
    /// The user id of the owner of the card (KAIROS-T-0416): an owner
    /// lane accepts only the drops of its own cards.
    owner: Option<String>,
}

// ---------------------------------------------------------------------------
// Planned/Support lanes (KAIROS-T-0077)
// ---------------------------------------------------------------------------

/// The two board lanes: the lane is a projection of `tasks.work_class`.
const LANE_SUPPORT: &str = "support";
const LANE_PLANNED: &str = "planned";

/// Which lane a card renders in: tasks follow their `work_class`;
/// non-task kinds always render in the Planned (default) lane. Pure,
/// host-tested.
fn card_lane(work_class: Option<&str>) -> &'static str {
    match work_class {
        Some(LANE_SUPPORT) => LANE_SUPPORT,
        _ => LANE_PLANNED,
    }
}

/// What dropping the in-flight drag onto (column, lane) would do.
/// `None` = illegal or no-op, and the zone neither lights up nor accepts.
/// Pure, host-tested: same-column cross-lane = lane write only (the rules
/// engine is never consulted); cross-column same-lane = transition as
/// before; diagonal = both; lane moves only exist for cards that carry a
/// lane (tasks).
#[derive(Clone, Debug, PartialEq)]
struct DropEffect {
    transition_to: Option<String>,
    set_work_class: Option<String>,
}

fn drop_effect(drag: &DragData, column_id: &str, lane: Option<&str>) -> Option<DropEffect> {
    let column_change = drag.source_column != column_id;
    let lane_change = match (lane, drag.work_class.as_deref()) {
        (Some(lane), Some(current)) => current != lane,
        _ => false,
    };
    match (column_change, lane_change) {
        (false, false) => None,
        (false, true) => Some(DropEffect {
            transition_to: None,
            set_work_class: lane.map(str::to_string),
        }),
        (true, _) if !drag.targets.iter().any(|t| t == column_id) => None,
        (true, false) => Some(DropEffect {
            transition_to: Some(column_id.to_string()),
            set_work_class: None,
        }),
        (true, true) => Some(DropEffect {
            transition_to: Some(column_id.to_string()),
            set_work_class: lane.map(str::to_string),
        }),
    }
}

/// [`drop_effect`] in a lane of a grouped board. The owner grouping is
/// read-only (KAIROS-T-0416): a drop into the lane of a different owner
/// does nothing. In the lane of the card, a drop in a different column
/// transitions as before. The repository grouping keeps its rule
/// (KAIROS-T-0109). Pure, host-tested.
fn grouped_drop_effect(
    drag: &DragData,
    column_id: &str,
    lane: Option<&str>,
    group: &GroupLane,
) -> Option<DropEffect> {
    if let GroupLane::Owner(owner) = group
        && drag.owner != *owner
    {
        return None;
    }
    drop_effect(drag, column_id, lane)
}

// ---------------------------------------------------------------------------
// Client-side capability mirror (KAIROS-T-0072)
// ---------------------------------------------------------------------------

/// What the signed-in user may do on THIS board — mirrors the A-0006
/// decision (org-admin bypass, explicit grants incl. globs, and the
/// KAIROS-T-0072 team implication) so affordances the server would 403
/// never render. The server remains the authority; this is UX.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BoardPowers {
    /// May move cards (`transition_items`).
    pub(crate) transition: bool,
    /// May create this board level's entity (`manage_<family>`).
    pub(crate) create: bool,
    /// May create documents (`manage_documents`).
    pub(crate) documents: bool,
    /// May manage the members and grants of the board (KAIROS-T-0412):
    /// org admin, or `administer_members` / `configure_boards` on it.
    pub(crate) members: bool,
}

/// The `manage_*` capability that creating this kind requires.
fn create_capability(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Strategy => "manage_strategies",
        EntityKind::Initiative => "manage_initiatives",
        EntityKind::Task => "manage_tasks",
        EntityKind::Adr => "manage_adrs",
    }
}

/// Does a stored grant cover `required`? Client mirror of the A-0006
/// matching semantics for the sanctioned grant forms (exact, `*`, and
/// trailing-`*` globs).
fn grant_covers(grant: &str, required: &str) -> bool {
    grant == "*"
        || grant == required
        || grant
            .strip_suffix('*')
            .is_some_and(|prefix| required.starts_with(prefix))
}

/// The KAIROS-T-0072 implied set (mirror of
/// `kairos_core::abac::TEAM_IMPLIED_CAPABILITIES`).
fn team_implies(required: &str) -> bool {
    matches!(
        required,
        "manage_tasks" | "manage_documents" | "transition_items"
    )
}

/// What membership of the team gives on a KNOWN board of that team: the
/// delivery set, plus `manage_adrs` (COLLIERY-T-3108, mirror of
/// `kairos_core::abac::TEAM_ADR_BOARD_IMPLIED_CAPABILITIES`).
///
/// The server gives `manage_adrs` from the team rule only on the ADR board
/// of the team. The mirror does not know the level of the board, and it
/// does not need to: only an ADR asks for `manage_adrs`, and an ADR on a
/// board of a team is on the ADR board of that team. With no known board
/// (an off-board item), the mirror uses [`team_implies`] only. The server
/// stays the authority.
fn team_implies_on_team_board(required: &str) -> bool {
    // KAIROS-T-0440: the team of a strategy or initiative board gets its
    // `manage_<type>` too; only that board's items ask for it.
    team_implies(required)
        || matches!(
            required,
            "manage_adrs" | "manage_strategies" | "manage_initiatives"
        )
}

/// The link from a board to its members panel (KAIROS-T-0412): the admin
/// page of the board, at the anchor of the panel
/// ([`crate::pages::admin::MEMBERS_ANCHOR`]). Pure, host-tested.
pub(crate) fn members_panel_href(board_id: &str) -> String {
    format!(
        "/admin/boards/{board_id}#{}",
        crate::pages::admin::MEMBERS_ANCHOR
    )
}

/// Compute [`BoardPowers`] from the whoami identity. Pure, host-tested.
/// Also used by the item detail page's move control (KAIROS-T-0075) —
/// the same `transition_items` gate as the board's drag affordance.
pub(crate) fn board_powers(
    me: &crate::api::Whoami,
    board_slug: &str,
    board_team_id: Option<&str>,
    create_kind: Option<EntityKind>,
) -> BoardPowers {
    if me.organization.role == "admin" {
        return BoardPowers {
            transition: true,
            create: create_kind.is_some(),
            documents: true,
            members: true,
        };
    }
    let team_member = board_team_id.is_some_and(|team| me.teams.iter().any(|mine| mine.id == team));
    // KAIROS-T-0410: `implied` is what the server says membership of this
    // board gives (the edit and the move on a strategy or initiative
    // board, for the `member` grant: KAIROS-T-0541). The server computes
    // it; the mirror only reads it.
    let has = |required: &str| {
        (team_member && team_implies_on_team_board(required))
            || me
                .capabilities
                .iter()
                .filter(|board| board.board_slug == board_slug)
                .any(|board| {
                    board
                        .grants
                        .iter()
                        .any(|grant| grant_covers(grant, required))
                        || board.implied.iter().any(|implied| implied == required)
                })
    };
    BoardPowers {
        transition: has("transition_items"),
        create: create_kind.is_some_and(|kind| has(create_capability(kind))),
        documents: has("manage_documents"),
        members: crate::pages::admin::gating::may_administer_board(me, board_slug),
    }
}

/// May the signed-in user EDIT one item (COLLIERY-T-0228)? The client
/// mirror of the edit rule of the server: the user created the item, or
/// holds `required`, its `manage_<type>` capability, where it counts for
/// that item ([`holds_capability`], which has the admin bypass).
///
/// It gates the controls that are edits: the repository of a task, and
/// Restore. It must NOT gate a control that moves the item. The column,
/// the lane and the board of an item belong to the team of the board, and
/// the creator of an item gets no right there. Those controls ask
/// [`board_powers`], which does not know who created what.
///
/// `created_by` is the user id on the item. An empty id matches nobody, so
/// an item that carries no creator gives no right to a user with no id.
///
/// Pure, host-tested. The server remains the authority.
pub(crate) fn may_edit_item(
    me: &crate::api::Whoami,
    created_by: &str,
    board_slug: Option<&str>,
    board_team_id: Option<&str>,
    required: &str,
) -> bool {
    let created_item = !created_by.is_empty() && me.user.id == created_by;
    created_item || holds_capability(me, board_slug, board_team_id, required)
}

/// Does the signed-in user hold `required` where it counts for ONE item
/// (KAIROS-T-0164's Restore affordance)?
///
/// [`board_powers`] answers for a board the caller is looking at; this
/// answers for an item, which may sit on no board at all — a document, an
/// off-board ADR. In that case the server resolves the authorization board
/// through the item's parent, which the client cannot see, so the mirror
/// widens to "anywhere": any grant on any board, or membership of any team
/// for the team-implied capabilities. Widening is the right direction for
/// an affordance whose server-side refusal is a plain 403 shown inline —
/// the opposite mistake hides a button from someone who may press it.
///
/// Pure, host-tested. The server remains the authority.
pub(crate) fn holds_capability(
    me: &crate::api::Whoami,
    board_slug: Option<&str>,
    board_team_id: Option<&str>,
    required: &str,
) -> bool {
    if me.organization.role == "admin" {
        return true;
    }
    let team_implied = match (board_slug, board_team_id) {
        (_, Some(team)) => {
            me.teams.iter().any(|mine| mine.id == team) && team_implies_on_team_board(required)
        }
        // A board we know, that no team owns: no team implication.
        (Some(_), None) => false,
        // No board to check against — see the widening above. An off-board
        // ADR is not on the ADR board of a team, so `manage_adrs` is not in
        // this set.
        (None, None) => !me.teams.is_empty() && team_implies(required),
    };
    if team_implied {
        return true;
    }
    me.capabilities
        .iter()
        .filter(|board| board_slug.is_none_or(|slug| board.board_slug == slug))
        .flat_map(|board| board.grants.iter())
        .any(|grant| grant_covers(grant, required))
}

/// The delivery boards a task may be moved TO from `here_slug`: every
/// other live delivery board where [`board_powers`] says the caller may
/// manage tasks (KAIROS-I-0012 D2). The server's rule is two-sided —
/// `manage_tasks` on the source board AND the target, org admins bypass —
/// so the item page gates the control on the source board and lists only
/// these targets, and never offers a move the server would 403. Returns
/// `(slug, name)` in the order the board list came in. Pure, host-tested.
pub(crate) fn movable_delivery_boards(
    me: &crate::api::Whoami,
    boards: &[data::Board],
    here_slug: &str,
) -> Vec<(String, String)> {
    boards
        .iter()
        .filter(|board| board.board_level == "delivery" && board.slug != here_slug)
        .filter(|board| {
            board_powers(
                me,
                &board.slug,
                board.team_id.as_deref(),
                Some(EntityKind::Task),
            )
            .create
        })
        .map(|board| (board.slug.clone(), board.name.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Requests (COLLIERY-T-0232)
// ---------------------------------------------------------------------------

/// The computed capability that lets a member of the tenant send a request
/// (mirror of `kairos_core::abac::FILE_BACKLOG`).
const FILE_BACKLOG: &str = "file_backlog";

/// The board level on which a request is possible (mirror of
/// `BoardLevel::Delivery`, the level that `check_file_backlog` asks for).
const LEVEL_DELIVERY: &str = "delivery";

/// How the signed-in user creates an item on ONE board (COLLIERY-T-0232).
///
/// The server has two answers for a task create
/// (`TaskCreateAccess::{Manager, Request}`), and the two give different
/// results: a manager selects the lane, and a request always goes to the
/// support lane. The dialog must know which answer it will get before it
/// shows a control, so the mode is decided here and not in the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreateMode {
    /// The user manages the board: "New task" and its full dialog.
    Manager,
    /// The user sends a request to the team of the board: "New request".
    Request,
}

/// Does the signed-in user MANAGE this board (COLLIERY-T-0232)? It is the
/// `create` power of [`board_powers`] and nothing more: an organization
/// admin, a member of the team of the board for `manage_tasks`, or an
/// explicit grant that covers the `manage_<type>` capability of the level.
///
/// It has a name of its own because the board now asks two questions where
/// it asked one. Until COLLIERY-T-0232 "can create" and "manages" were the
/// same thing in the GUI, and a member who did not manage a delivery board
/// had no button at all. The rule for a manager did not change.
///
/// `me` is `None` until `whoami` resolves. Nobody manages a board before
/// the GUI knows who they are. Pure, host-tested. The server remains the
/// authority.
pub(crate) fn manages_board(me: Option<&crate::api::Whoami>, board: &data::Board) -> bool {
    me.is_some_and(|me| {
        board_powers(
            me,
            &board.slug,
            board.team_id.as_deref(),
            EntityKind::for_board_level(&board.board_level),
        )
        .create
    })
}

/// May the signed-in user send a REQUEST to the team of this board
/// (COLLIERY-T-0232)? The client mirror of the `Request` answer of
/// `require_task_create_capability` on the server. All of these are true:
///
/// - `whoami` resolved, so the user is a member of the tenant (the server
///   answers `whoami` for a member only);
/// - `whoami` lists `file_backlog` under `implicit`. The GUI READS the
///   capability. It does not assume that each member has it, so a server
///   that stops giving it also stops the button;
/// - the board is a delivery board. `check_file_backlog` gives the
///   capability on a delivery board and nowhere else, and `whoami` cannot
///   say so, because its list is not per board. This half is mirrored;
/// - the board has an entry column, because a request goes there and
///   nowhere else;
/// - the user does NOT manage the board ([`manages_board`]). The server
///   asks `manage_tasks` first and a manager never gets the `Request`
///   answer, so a manager never sees "New request".
///
/// The two other conditions of the server are about the body, not about
/// the person: no column other than the entry column, and no work class
/// `planned`. [`create_body`] keeps them.
///
/// Pure, host-tested. The server remains the authority.
pub(crate) fn may_request(
    me: Option<&crate::api::Whoami>,
    board: &data::Board,
    has_entry_column: bool,
) -> bool {
    let Some(me) = me else {
        return false;
    };
    me.implicit.iter().any(|held| held == FILE_BACKLOG)
        && board.board_level == LEVEL_DELIVERY
        && has_entry_column
        && !manages_board(Some(me), board)
}

/// Which create button the board shows, if any (COLLIERY-T-0232):
/// "New task" (and "New strategy", and so on) for [`CreateMode::Manager`],
/// "New request" for [`CreateMode::Request`], nothing for `None`.
///
/// A board with no entry column shows nothing, as before: a new item has
/// no column to start in. The two modes cannot be true together, because
/// [`may_request`] is false for a manager. Pure, host-tested.
pub(crate) fn create_mode(
    me: Option<&crate::api::Whoami>,
    board: &data::Board,
    has_entry_column: bool,
) -> Option<CreateMode> {
    if !has_entry_column {
        return None;
    }
    if manages_board(me, board) {
        Some(CreateMode::Manager)
    } else if may_request(me, board, has_entry_column) {
        Some(CreateMode::Request)
    } else {
        None
    }
}

/// The bucket value of an initiative that is not a bucket (KAIROS-T-0587).
const NO_BUCKET: &str = "none";

/// The options of the Bucket select: none, then the kinds of the server
/// (`BucketType::ALL`).
const BUCKET_OPTIONS: [&str; 4] = [NO_BUCKET, "tech_debt", "bug", "ad_hoc"];

/// The values of the create dialog, as the person left them. It is a plain
/// copy of the signals of the form, so that [`create_body`] can be a pure
/// function (COLLIERY-T-0232).
#[derive(Clone, Debug, Default, PartialEq)]
struct CreateForm {
    title: String,
    content: String,
    hypothesis: String,
    /// `none` | `xs` | `s` | `m` | `l` | `xl`.
    complexity: String,
    /// [`NO_BUCKET`] | `tech_debt` | `bug` | `ad_hoc` (KAIROS-T-0587).
    bucket_type: String,
    /// `task` | `bug` | `tech_debt` | `support`.
    task_type: String,
    /// `auto` | `planned` | `support`. The request dialog has no control
    /// for it, and [`create_body`] does not read it for a request.
    work_class: String,
    /// A slug, or [`NO_REPOSITORY`].
    repository: String,
    decision_maker: String,
    decision_date: String,
    /// The short code of the parent, or empty (KAIROS-T-0580). The dialog
    /// shows the field for an initiative and a task.
    parent: String,
}

/// What the create dialog SENDS: the column, then the item
/// (COLLIERY-T-0232). Extracted from the submit handler so that the rule
/// for a request has a test that needs no browser.
///
/// The column is always `entry_column_id`. The form has no column, so no
/// state of the form can name a different one (KAIROS-T-0062: a create is
/// intake). For a request this is also the rule of the server: a column
/// that is not the entry column is refused with 403.
///
/// [`CreateMode::Manager`] sends what the dialog sent before this change:
/// the lane of the form, and no `work_class` when the lane is `auto`.
///
/// [`CreateMode::Request`] ALWAYS sends `work_class: support`, and does
/// not read the lane of the form. It sends the value, and does not leave
/// the field out, for two reasons. The server stores `support` for a
/// request in the two cases today, but a body that says `support` does not
/// depend on the default of the server: if that default changes, the
/// request still says which lane it is for. And `support` is the one value
/// that is correct for the two answers of the server. If the GUI decides
/// "request" from a stale `whoami` and the person manages the board by
/// now, the task still goes to the support lane that the caption named,
/// and not to the planned lane of a team that did not plan it.
///
/// A request is a task, so the fields of the other types are left out.
///
/// `team_id` is the team of the board. The board decides the team of a
/// task (COLLIERY-T-0216), and the server accepts the name of that same
/// team, so the two modes send what the dialog always sent.
fn create_body(
    mode: CreateMode,
    form: &CreateForm,
    entry_column_id: &str,
    team_id: Option<&str>,
) -> (String, data::NewItem) {
    let filled = |value: &str| (!value.trim().is_empty()).then(|| value.to_string());
    let shared = data::NewItem {
        title: form.title.clone(),
        content: form.content.clone(),
        task_type: Some(form.task_type.clone()),
        team_id: team_id.map(str::to_string),
        repository: Some(form.repository.clone()).filter(|slug| slug != NO_REPOSITORY),
        parent: filled(&form.parent).map(|code| code.trim().to_string()),
        ..data::NewItem::default()
    };
    let item = match mode {
        CreateMode::Manager => data::NewItem {
            hypothesis: filled(&form.hypothesis),
            complexity: Some(form.complexity.clone()).filter(|c| c != "none"),
            // KAIROS-T-0587: "none" is an initiative that is not a bucket.
            bucket_type: Some(form.bucket_type.clone()).filter(|b| !b.is_empty() && b != NO_BUCKET),
            // KAIROS-T-0077: "auto" leaves the field out, and the server
            // selects the lane from the task type.
            work_class: Some(form.work_class.clone()).filter(|c| c != "auto"),
            decision_maker: filled(&form.decision_maker),
            decision_date: filled(&form.decision_date),
            ..shared
        },
        CreateMode::Request => data::NewItem {
            work_class: Some(LANE_SUPPORT.to_string()),
            ..shared
        },
    };
    (entry_column_id.to_string(), item)
}

/// The name of the lane, as the board shows it in the head of the lane.
fn lane_label(lane: &str) -> &'static str {
    match lane {
        LANE_SUPPORT => "Support",
        _ => "Planned",
    }
}

/// The title of the request dialog (COLLIERY-T-0232). It names the team
/// that gets the request, because the person is on the board of a team
/// that is not theirs. `team` is `None` when the list of teams did not
/// load; the title then does not show a wrong name or an empty one.
fn request_title(team: Option<&str>) -> String {
    format!("Request to {}", team.unwrap_or("the team of this board"))
}

/// The caption of the request dialog (COLLIERY-T-0232): where the request
/// goes, and who moves it. The person cannot select a column or a lane, so
/// the dialog says which ones the server uses.
fn request_caption(team: Option<&str>, entry_column: &str) -> String {
    let lane = lane_label(LANE_SUPPORT);
    match team {
        Some(team) => format!(
            "The request goes to {entry_column}, the entry column of the board of {team}, \
             in the {lane} lane. {team} moves it from there."
        ),
        None => format!(
            "The request goes to {entry_column}, the entry column of this board, \
             in the {lane} lane. The team of the board moves it from there."
        ),
    }
}

/// A request that the person sent from this board (COLLIERY-T-0232). The
/// board shows it as a notice: [`request_notice`] gives the text, and the
/// short code is a link to the item.
#[derive(Clone, Debug, PartialEq)]
struct SentRequest {
    short_code: String,
    /// The repository that the request links to, if any. The repository
    /// lens of the board hides a task that is not in the selection.
    repository: Option<String>,
}

/// The text of the notice after a request, in two parts: before the short
/// code and after it (COLLIERY-T-0232). The short code goes between them
/// as a link.
///
/// The text names the column and the lane. The board shows the Support
/// lane and the Planned lane together, so the new card is on the page. But
/// the person can have the repository lens on, or the lanes grouped by
/// repository, and then the card is not where a reader of "Support lane"
/// looks. `hidden_by_lens` adds the sentence that says why the card is not
/// visible. The lens itself does not change: the person set it.
fn request_notice(
    team: Option<&str>,
    entry_column: &str,
    hidden_by_lens: bool,
) -> (String, String) {
    let before = format!(
        "Request sent to {}: ",
        team.unwrap_or("the team of this board")
    );
    let mut after = format!(
        ". It is in {entry_column}, in the {} lane.",
        lane_label(LANE_SUPPORT)
    );
    if hidden_by_lens {
        after.push_str(" The repository filter hides it on this page.");
    }
    (before, after)
}

/// Does the repository lens hide a request that was sent a moment ago
/// (COLLIERY-T-0232)? The same rule as [`lens_admits`] for a task: with a
/// selection, the board shows only the tasks of the selected repositories.
fn lens_hides_request(request: &SentRequest, selected: &[String]) -> bool {
    !selected.is_empty()
        && !request
            .repository
            .as_ref()
            .is_some_and(|slug| selected.contains(slug))
}

/// One drop of a card (KAIROS-T-0619): the card and what the drop does.
/// [`LiveReads::run_drop`] runs it.
#[derive(Clone, Debug, PartialEq)]
struct CardDrop {
    kind: EntityKind,
    short_code: String,
    effect: DropEffect,
}

/// The toast of a drop of a card while its last drop runs.
const DROP_BUSY: &str = "This card moves now. Wait for the move to end, then try again.";

// ---------------------------------------------------------------------------
// Board list
// ---------------------------------------------------------------------------

/// The flight-level band order for `/boards` (KAIROS-T-0069/T-0063:
/// strategy feeds initiatives feed delivery; ADRs record the decisions).
const LEVEL_BANDS: &[(&str, &str)] = &[
    ("strategy", "Strategy"),
    ("initiative", "Initiatives"),
    ("delivery", "Delivery"),
    ("adr", "Decisions"),
];

/// One board-list group: `(Some((heading, href)), boards)`. The heading
/// says whose boards these are. The href is `/teams/:slug` for a delivery
/// team, and empty for a heading that is text only.
type BoardGroup = (Option<(String, String)>, Vec<data::Board>);

/// The heading of the boards of the organization (strategy, initiative,
/// adr). Every board has a team (COLLIERY-T-0230). A board of the
/// organization has no delivery team: its team is the list of the members
/// of the board. The heading says that these boards belong to the
/// organization, so that a reader does not look for a delivery team.
const ORGANIZATION_GROUP: &str = "Organization";

/// The heading of a delivery board that has no team. The server does not
/// create one (COLLIERY-T-0230), but a board made before the rule can be in
/// the data, and existing boards are not migrated. The board must stay
/// reachable, and the heading says what an administrator must do.
const NEEDS_TEAM_GROUP: &str = "Needs a team";

/// The heading of a delivery board that has a team which is not in the
/// list of teams: the read of the teams failed, or the list is stale. The
/// board HAS a team, so "Needs a team" would be false.
const UNKNOWN_TEAM_GROUP: &str = "Team not found";

/// One rendered board-list band: level heading + its tiles, grouped by
/// the team of the board.
struct BandModel {
    level: String,
    label: String,
    groups: Vec<BoardGroup>,
}

/// Bucket boards into level bands (strategy → initiative → delivery →
/// adr, unknown levels last). The delivery band is grouped by delivery
/// team. The bands of the organization have one group,
/// [`ORGANIZATION_GROUP`]. There is no group "No team": every board has a
/// team (COLLIERY-T-0230). A delivery board from old data that has no
/// team, or a team that is not in `teams`, keeps a group of its own —
/// nothing becomes unreachable. Pure, host-tested.
fn band_models(
    boards: Vec<data::Board>,
    teams: &[crate::pages::teams::api::Team],
) -> Vec<BandModel> {
    let mut bands: Vec<BandModel> = Vec::new();
    let known: Vec<&str> = LEVEL_BANDS.iter().map(|(level, _)| *level).collect();
    for (level, label) in LEVEL_BANDS {
        let of_level: Vec<data::Board> = boards
            .iter()
            .filter(|b| b.board_level == *level)
            .cloned()
            .collect();
        if of_level.is_empty() {
            continue;
        }
        // COLLIERY-T-3102: an ADR board can have a team too. The ADR band
        // has a group for each team, and the ADR boards with no team are
        // the boards of the organization.
        let groups = if *level == "delivery" || *level == "adr" {
            // One group per team (team order = teams list order), then the
            // two fallback groups for old or unreadable data.
            let mut groups: Vec<BoardGroup> = Vec::new();
            for team in teams {
                let of_team: Vec<data::Board> = of_level
                    .iter()
                    .filter(|b| b.team_id.as_deref() == Some(team.id.as_str()))
                    .cloned()
                    .collect();
                if !of_team.is_empty() {
                    groups.push((
                        Some((team.name.clone(), format!("/teams/{}", team.slug))),
                        of_team,
                    ));
                }
            }
            let unknown_team: Vec<data::Board> = of_level
                .iter()
                .filter(|b| {
                    b.team_id
                        .as_deref()
                        .is_some_and(|id| !teams.iter().any(|t| t.id == id))
                })
                .cloned()
                .collect();
            if !unknown_team.is_empty() {
                groups.push((
                    Some((UNKNOWN_TEAM_GROUP.to_string(), String::new())),
                    unknown_team,
                ));
            }
            let needs_team: Vec<data::Board> = of_level
                .iter()
                .filter(|b| b.team_id.is_none())
                .cloned()
                .collect();
            if !needs_team.is_empty() {
                let heading = if *level == "adr" {
                    ORGANIZATION_GROUP
                } else {
                    NEEDS_TEAM_GROUP
                };
                groups.push((Some((heading.to_string(), String::new())), needs_team));
            }
            groups
        } else {
            vec![(
                Some((ORGANIZATION_GROUP.to_string(), String::new())),
                of_level,
            )]
        };
        bands.push(BandModel {
            level: level.to_string(),
            label: label.to_string(),
            groups,
        });
    }
    // Anything with a level outside the known vocabulary still renders.
    let unknown: Vec<data::Board> = boards
        .iter()
        .filter(|b| !known.contains(&b.board_level.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        bands.push(BandModel {
            level: "other".to_string(),
            label: "Other".to_string(),
            groups: vec![(None, unknown)],
        });
    }
    bands
}

/// The tiles of one band, in the order of its groups (KAIROS-T-0319). The
/// band renders as one grid. Each tile keeps the heading of its group (the
/// team and the href of the team page), so each tile names its team. Pure,
/// host-tested.
fn band_tiles(groups: Vec<BoardGroup>) -> Vec<(Option<(String, String)>, data::Board)> {
    groups
        .into_iter()
        .flat_map(|(heading, boards)| {
            boards
                .into_iter()
                .map(move |board| (heading.clone(), board))
        })
        .collect()
}

/// `/boards` — boards in flight-level bands (strategy above initiatives
/// above delivery, KAIROS-T-0069/T-0063). Each band is one grid of tiles
/// that wraps across the page width (KAIROS-T-0319). Each tile names the
/// team of its board with a link into `/teams/:slug`, or "Organization"
/// for a board of the organization (COLLIERY-T-0230).
#[component]
pub fn BoardsPage() -> impl IntoView {
    let auth = use_auth();
    let boards = LocalResource::new(move || {
        auth.track_session();
        data::list_boards(auth)
    });
    // Team names for the delivery grouping. Optional enhancement data: a
    // failed teams read degrades to the "Team not found" group, never a
    // dead page.
    let teams = LocalResource::new(move || {
        auth.track_session();
        crate::pages::teams::api::list_teams(auth)
    });
    view! {
        <PageHeader title="Boards" sub="kanban per flight level — strategy feeds initiatives feed delivery"/>
        {move || match boards.get() {
            None => view! { <Loading label="Loading boards…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| boards.refetch())/>
            }.into_any(),
            Some(Ok(items)) if items.is_empty() => view! {
                <Empty message="No boards yet — an org admin can create one from Admin."/>
            }.into_any(),
            Some(Ok(items)) => {
                let team_list = teams.get().and_then(|r| r.ok()).unwrap_or_default();
                let bands = band_models(items, &team_list);
                view! {
                    <Stack gap="md">
                        {bands.into_iter().map(|band| {
                            let BandModel { level, label, groups } = band;
                            view! {
                                <section class="kairos-board-band">
                                    <Group gap="sm">
                                        <Pill color=level_color(&level)>{label}</Pill>
                                    </Group>
                                    // KAIROS-T-0319: one grid for the band, so it
                                    // wraps across the page width. Each tile names
                                    // the team of its board (a link to the team).
                                    <div class="kairos-board-grid">
                                        {band_tiles(groups).into_iter().map(|(team, board)| {
                                            let href = format!("/boards/{}", board.slug);
                                            let team_view = team.map(|(team_name, team_href)| {
                                                if team_href.is_empty() {
                                                    view! {
                                                        <Text dimmed=true size="xs">{team_name}</Text>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <Anchor href=team_href>{team_name}</Anchor>
                                                    }.into_any()
                                                }
                                            });
                                            view! {
                                                // COLLIERY-T-1836: an Aurora `Card`; the
                                                // wrapper keeps the `kairos-board-tile`
                                                // hook of the e2e/UAT specs. The team is
                                                // under the card: a link in the link of
                                                // the card is not valid HTML.
                                                <div class="kairos-board-tile">
                                                    <Card href=href>
                                                        <Stack gap="xs">
                                                            <Group justify="between">
                                                                <Text bright=true bold=true>{board.name.clone()}</Text>
                                                                <Pill color=level_color(&board.board_level)>
                                                                    {board.board_level.clone()}
                                                                </Pill>
                                                            </Group>
                                                            <Text mono=true dimmed=true size="xs">{board.slug.clone()}</Text>
                                                        </Stack>
                                                    </Card>
                                                    {team_view.map(|v| view! {
                                                        <div class="kairos-board-tile__team">{v}</div>
                                                    })}
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                </section>
                            }
                        }).collect_view()}
                    </Stack>
                }.into_any()
            }
        }}
    }
}

// ---------------------------------------------------------------------------
// Board view
// ---------------------------------------------------------------------------

/// The key of a card in the card `<For>` (KAIROS-T-0074, KAIROS-T-0615).
/// A card with the same key keeps its DOM node (element identity, focus,
/// transient state) across reads; a changed card is rebuilt.
///
/// The `version` of the item changes with each write of its fields, and
/// `column_id` with each move. `extras` is a hash of the fields that
/// change with no new version: the claim, the state, the cancel mark, the
/// repository, the owner, the teams, the progress and the blocks counts,
/// the priority, the days in the column and the notes count.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CardKey {
    short_code: String,
    version: i32,
    column_id: String,
    extras: u64,
}

/// The fields of a card that change with no new version of the item.
#[derive(Hash)]
struct CardExtras<'a> {
    work_class: Option<&'a str>,
    repository: Option<&'a str>,
    claim: Option<&'a data::TaskClaim>,
    state: Option<&'a data::TaskState>,
    cancelled: bool,
    owner: Option<&'a CardOwner>,
    teams: &'a [String],
    progress: Option<data::ProgressCounts>,
    blocks: Option<data::BlocksCounts>,
    /// The priority metadata (KAIROS-T-0582): a change of metadata gives
    /// no new version of the item.
    priority: Option<&'a str>,
    /// The days in the column and the notes count (KAIROS-T-0649): a note
    /// gives no new version of the item.
    days: Option<i64>,
    notes: Option<i64>,
}

impl CardExtras<'_> {
    /// The hash of the extras: one pass over a few fields, and no text is
    /// made.
    fn hash_value(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

/// One card's owned view model with its [`CardKey`] (KAIROS-T-0074): the
/// card `<For>` diffs on the key.
#[derive(Clone, PartialEq)]
struct CardModel {
    kind: EntityKind,
    short_code: String,
    title: String,
    meta: Vec<(String, &'static str)>,
    /// `tasks.work_class` — `Some` for tasks only (KAIROS-T-0077).
    work_class: Option<String>,
    /// The bound repository's slug — tasks only (KAIROS-T-0109, A-0019).
    repository: Option<String>,
    /// The person who has the task in Active (KAIROS-T-0359) — tasks only.
    claim: Option<data::TaskClaim>,
    /// The live state of a task (KAIROS-T-0506) — tasks only.
    state: Option<data::TaskState>,
    /// The owner of the card (KAIROS-T-0416): the person with the claim of
    /// a task, or the owner of a strategy or an initiative.
    owner: Option<CardOwner>,
    /// The slugs of the teams of a strategy or an initiative, by slug
    /// (KAIROS-T-0322): from its tasks and set by hand. Empty for the
    /// other kinds and for an item with no team.
    teams: Vec<String>,
    /// Children rollup — `Some` for parents only (KAIROS-T-0080).
    progress: Option<data::ProgressCounts>,
    /// Blocked-by/blocks counts — `Some` only with live blocks edges
    /// (KAIROS-T-0091).
    blocks: Option<data::BlocksCounts>,
    key: CardKey,
}

/// The owner of a card (KAIROS-T-0416).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CardOwner {
    user_id: String,
    display_name: String,
}

impl CardOwner {
    fn of_claim(claim: &data::TaskClaim) -> Self {
        CardOwner {
            user_id: claim.user_id.clone(),
            display_name: claim.display_name.clone(),
        }
    }

    fn of_owner(owner: &data::ItemOwner) -> Self {
        CardOwner {
            user_id: owner.user_id.clone(),
            display_name: owner.display_name.clone(),
        }
    }
}

/// The text of the chip of a cancelled task (KAIROS-T-0362) or initiative
/// (KAIROS-T-0678).
const CANCELLED_CHIP: &str = "Cancelled";

/// One column's owned view model. Its `key` fingerprints identity + the
/// transition-derived drop targets (config changes rebuild the column);
/// the card list is NOT in the key — cards diff independently.
#[derive(Clone, PartialEq)]
struct ColumnModel {
    id: String,
    name: String,
    /// The WIP chip and whether the column is over its limit
    /// (KAIROS-T-0371); `None` for a column with no limit.
    wip: Option<(String, bool)>,
    /// The column holds finished work (KAIROS-T-0582): it shows the last
    /// [`DONE_COLUMN_CARDS`] cards, and "Hide done" hides it.
    is_done: bool,
    /// The live cards of the full column (KAIROS-T-0649): the board reads
    /// fewer cards of a done column.
    item_count: i64,
    targets: Vec<(String, String)>,
    cards: Vec<CardModel>,
    key: String,
}

/// Flatten the wire shape into owned, keyed view models (leptos children
/// are `'static` move closures, so views must own their data).
fn column_models(view: &data::BoardView) -> Vec<ColumnModel> {
    let now = now_ms();
    let column_name_of = |id: &str| -> String {
        view.detail
            .columns
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    let progress_of = |short_code: &str| view.items.children_progress.get(short_code).copied();
    let blocks_of = |short_code: &str| view.items.blocks_summary.get(short_code).copied();
    let teams_of = |short_code: &str| -> Vec<String> {
        view.items
            .item_teams
            .get(short_code)
            .map(|teams| teams.iter().map(|team| team.slug.clone()).collect())
            .unwrap_or_default()
    };
    let card =
        |kind: EntityKind,
         (short_code, version, column_id): (&str, i32, &str),
         title: &str,
         meta: Vec<(String, &'static str)>,
         work_class: Option<String>,
         repository: Option<String>,
         (claim, state, cancelled): (Option<data::TaskClaim>, Option<data::TaskState>, bool),
         owner: Option<CardOwner>,
         (entered, notes): (Option<&str>, Option<i64>)| {
            let progress = progress_of(short_code);
            let blocks = blocks_of(short_code);
            let teams = teams_of(short_code);
            // KAIROS-T-0582: the priority metadata, as a pill.
            let priority = view.items.metadata_value(short_code, data::PRIORITY_SLUG);
            let mut meta = meta;
            meta.extend(priority.map(priority_pill));
            // KAIROS-T-0649: the days in the column and the notes count.
            let days = entered.and_then(|at| days_in_column(at, now));
            meta.extend(days.and_then(days_pill).map(|pill| (pill, token::MUTED)));
            meta.extend(notes.and_then(notes_pill).map(|pill| (pill, token::MUTED)));
            let extras = CardExtras {
                work_class: work_class.as_deref(),
                repository: repository.as_deref(),
                claim: claim.as_ref(),
                state: state.as_ref(),
                cancelled,
                owner: owner.as_ref(),
                teams: &teams,
                progress,
                blocks,
                priority,
                days,
                notes,
            }
            .hash_value();
            CardModel {
                kind,
                short_code: short_code.to_string(),
                title: title.to_string(),
                key: CardKey {
                    short_code: short_code.to_string(),
                    version,
                    column_id: column_id.to_string(),
                    extras,
                },
                work_class,
                repository,
                claim,
                state,
                owner,
                teams,
                progress,
                blocks,
                meta,
            }
        };
    view.items
        .columns
        .iter()
        .map(|group| {
            // ONLY the targets this board's transitions allow from this
            // column — invalid moves are never offered (S-0005).
            let targets: Vec<(String, String)> = view
                .detail
                .transitions
                .iter()
                .filter(|t| t.from_column_id == group.column.id)
                .map(|t| (t.to_column_id.clone(), column_name_of(&t.to_column_id)))
                .collect();
            let column_id = group.column.id.as_str();
            let mut cards: Vec<CardModel> = Vec::new();
            cards.extend(group.strategies.iter().map(|item| {
                // KAIROS-T-0411: the owner, as a pill.
                let meta = data::owner_pill(item.owner.as_ref())
                    .map(|pill| (pill, token::TEAL))
                    .into_iter()
                    .collect();
                card(
                    EntityKind::Strategy,
                    (&item.short_code, item.version, column_id),
                    &item.title,
                    meta,
                    None,
                    None,
                    (None, None, false),
                    item.owner.as_ref().map(CardOwner::of_owner),
                    (item.entered_column_at.as_deref(), item.notes_count),
                )
            }));
            cards.extend(group.initiatives.iter().map(|item| {
                let mut meta = Vec::new();
                if let Some(complexity) = &item.complexity {
                    meta.push((format!("complexity {complexity}"), token::ICE));
                }
                if item.is_bucket {
                    let bucket = item.bucket_type.clone().unwrap_or_default();
                    meta.push((format!("bucket · {bucket}"), token::GOLD));
                }
                // KAIROS-T-0411: the owner, as a pill.
                if let Some(pill) = data::owner_pill(item.owner.as_ref()) {
                    meta.push((pill, token::TEAL));
                }
                // KAIROS-T-0678: the chip of a cancelled initiative, first,
                // as for a task.
                if item.cancellation.is_some() {
                    meta.insert(0, (CANCELLED_CHIP.to_string(), token::MUTED));
                }
                card(
                    EntityKind::Initiative,
                    (&item.short_code, item.version, column_id),
                    &item.title,
                    meta,
                    None,
                    None,
                    (None, None, item.cancellation.is_some()),
                    item.owner.as_ref().map(CardOwner::of_owner),
                    (item.entered_column_at.as_deref(), item.notes_count),
                )
            }));
            cards.extend(group.tasks.iter().map(|item| {
                let mut meta = match item.task_type.as_str() {
                    "bug" => vec![("bug".to_string(), token::BAD)],
                    "tech_debt" => vec![("tech debt".to_string(), token::GOLD)],
                    "support" => vec![("support".to_string(), token::GOLD)],
                    _ => Vec::new(),
                };
                // KAIROS-T-0362: the chip of a cancelled task, first.
                if item.cancellation.is_some() {
                    meta.insert(0, (CANCELLED_CHIP.to_string(), token::MUTED));
                }
                card(
                    EntityKind::Task,
                    (&item.short_code, item.version, column_id),
                    &item.title,
                    meta,
                    Some(item.work_class.clone()),
                    item.repository.as_ref().map(|r| r.slug.clone()),
                    (
                        item.claim.clone(),
                        item.state.clone(),
                        item.cancellation.is_some(),
                    ),
                    item.claim.as_ref().map(CardOwner::of_claim),
                    (item.entered_column_at.as_deref(), item.notes_count),
                )
            }));
            cards.extend(group.adrs.iter().map(|item| {
                let meta = item
                    .decision_date
                    .iter()
                    .map(|date| (format!("decided {date}"), token::VIOLET))
                    .collect();
                card(
                    EntityKind::Adr,
                    (&item.short_code, item.version, column_id),
                    &item.title,
                    meta,
                    None,
                    None,
                    (None, None, false),
                    None,
                    (item.entered_column_at.as_deref(), item.notes_count),
                )
            }));
            let wip = data::wip_label(group.item_count, group.column.wip_limit)
                .map(|label| (label, group.over_wip_limit));
            ColumnModel {
                id: group.column.id.clone(),
                name: group.column.name.clone(),
                // KAIROS-T-0616: the WIP chip is NOT in the key. Its count
                // changes with every card that enters or leaves the column,
                // and a changed key rebuilds the column and all its cards in
                // every lane; the chip reads the live model instead.
                key: format!(
                    "{}|{}|{}|{:?}",
                    group.column.id, group.column.name, group.column.is_done, targets
                ),
                wip,
                is_done: group.column.is_done,
                item_count: group.item_count,
                targets,
                cards,
            }
        })
        .collect()
}

/// `(short_code, title)` of the board's document-parent candidates
/// (strategies/initiatives/tasks — documents attach via `supports`).
fn doc_parent_options(view: &data::BoardView) -> Vec<(String, String)> {
    view.items
        .columns
        .iter()
        .flat_map(|group| {
            group
                .strategies
                .iter()
                .map(|s| (s.short_code.clone(), s.title.clone()))
                .chain(
                    group
                        .initiatives
                        .iter()
                        .map(|i| (i.short_code.clone(), i.title.clone())),
                )
                .chain(
                    group
                        .tasks
                        .iter()
                        .map(|t| (t.short_code.clone(), t.title.clone())),
                )
        })
        .collect()
}

/// What the board body shows of each card, read from the model in ONE
/// pass for each change (KAIROS-T-0615). The memos of the body read this,
/// and not the model: before, each of them read each card again.
#[derive(Clone, Default, PartialEq)]
struct BoardSummary {
    /// The columns and their cards, before the filters of the view.
    columns: Vec<ColumnModel>,
    /// [`doc_parent_options`].
    doc_parents: Vec<(String, String)>,
    /// Each repository slug of the tasks, sorted, once each.
    repos: Vec<String>,
    /// Each team slug of the strategies and initiatives, sorted, once
    /// each.
    teams: Vec<String>,
    /// [`data::completed_task_count`].
    completed: usize,
}

/// The [`BoardSummary`] of a view. Pure, host-tested.
fn board_summary(view: &data::BoardView) -> BoardSummary {
    let mut repos: Vec<String> = view
        .items
        .columns
        .iter()
        .flat_map(|c| c.tasks.iter())
        .filter_map(|t| t.repository.as_ref().map(|r| r.slug.clone()))
        .collect();
    repos.sort_unstable();
    repos.dedup();
    let mut teams: Vec<String> = view
        .items
        .item_teams
        .values()
        .flat_map(|teams| teams.iter().map(|team| team.slug.clone()))
        .collect();
    teams.sort_unstable();
    teams.dedup();
    BoardSummary {
        columns: column_models(view),
        doc_parents: doc_parent_options(view),
        repos,
        teams,
        completed: data::completed_task_count(&view.items),
    }
}

/// The live reads of the board view (KAIROS-T-0615): the queue, the
/// trigger of a full read, and the model that a card read patches.
#[derive(Clone, Copy)]
struct LiveReads {
    auth: crate::auth::Auth,
    queue: StoredValue<data::LiveQueue>,
    /// Bumped for a full read: re-runs the board resource.
    refresh: RwSignal<u64>,
    model: RwSignal<Option<data::BoardView>>,
    /// The drops that run (KAIROS-T-0619): a read of their card waits.
    drops: StoredValue<data::DropsInFlight>,
}

impl LiveReads {
    /// Ask for a read. The first request of a window starts its timer;
    /// the reads of the window run together when it fires.
    fn request(self, refresh: data::Refresh) {
        let open = self
            .queue
            .try_update_value(|queue| queue.request(refresh))
            .unwrap_or(false);
        if open {
            set_timeout(
                move || {
                    if let Some(batch) =
                        self.queue.try_update_value(data::LiveQueue::fire).flatten()
                    {
                        self.run(batch);
                    }
                },
                std::time::Duration::from_millis(data::LIVE_WINDOW_MS),
            );
        }
    }

    /// Ask for a full read with no window: a change of this person (a
    /// move, a create) shows at once.
    fn read_board_now(self) {
        let batch = self.queue.try_update_value(|queue| {
            queue.ask(data::Refresh::Board);
            queue.fire()
        });
        if let Some(batch) = batch.flatten() {
            self.run(batch);
        }
    }

    /// Run one batch.
    fn run(self, batch: data::Batch) {
        match batch {
            data::Batch::Board => {
                // The board resource runs the read; its `BoardRead` ends it.
                if self.refresh.try_update(|n| *n += 1).is_none() {
                    self.finished();
                }
            }
            data::Batch::Cards(codes) => {
                let end = BatchEnd(self);
                // KAIROS-T-0662: the read names the board by its slug, as
                // the route of the view does (COLLIERY-T-0265), not its id.
                let Some(board) = self
                    .model
                    .with_untracked(|m| m.as_ref().map(|view| view.items.board.slug.clone()))
                else {
                    return;
                };
                let auth = self.auth;
                leptos::task::spawn_local(async move {
                    let reads = codes
                        .iter()
                        .map(|code| data::load_board_card(auth, &board, code));
                    let pages = futures_util::future::join_all(reads).await;
                    // KAIROS-T-0619: the read of a card with a drop that
                    // runs waits for the end of the drop.
                    let pages: Vec<_> = codes
                        .iter()
                        .zip(pages)
                        .filter(|(code, _)| {
                            !self
                                .drops
                                .try_update_value(|drops| drops.holds(code))
                                .unwrap_or(false)
                        })
                        .collect();
                    let mut whole = false;
                    self.model.try_update(|model| match model.as_mut() {
                        // The page shows a different board now.
                        Some(view) if view.items.board.slug != board => {}
                        Some(view) => {
                            for (code, page) in pages {
                                whole |= match page {
                                    Ok(page) => {
                                        view.items.patch_card(code, page) == data::Patched::Board
                                    }
                                    Err(_) => true,
                                };
                            }
                        }
                        None => {}
                    });
                    if whole {
                        self.queue
                            .try_update_value(|queue| queue.ask(data::Refresh::Board));
                    }
                    drop(end);
                });
            }
        }
    }

    /// Run one drop (KAIROS-T-0619). The card moves in the model at once;
    /// then the transition and the lane write go to the server, one after
    /// the other (no route does both). A refusal moves back what the
    /// server did not take, and `on_error` shows the message of the
    /// server. The board is not read again: the echo of the drop patches
    /// the card. `false`: a drop of this card runs, and this drop does
    /// not start.
    fn run_drop(self, card_drop: CardDrop, on_error: Callback<ApiError>) -> bool {
        let CardDrop {
            kind,
            short_code: code,
            effect,
        } = card_drop;
        if !self
            .drops
            .try_update_value(|drops| drops.start(&code))
            .unwrap_or(false)
        {
            return false;
        }
        let end = DropEnd {
            live: self,
            code: code.clone(),
        };
        let mut undo = None;
        self.model.try_update(|model| {
            if let Some(view) = model.as_mut() {
                undo = view.items.move_card(
                    &code,
                    effect.transition_to.as_deref(),
                    effect.set_work_class.as_deref(),
                );
            }
        });
        let auth = self.auth;
        leptos::task::spawn_local(async move {
            let refused = |column: bool, error: ApiError| {
                if let Some(undo) = undo.as_ref() {
                    self.model.try_update(|model| {
                        if let Some(view) = model.as_mut() {
                            view.items.undo_move(undo, column, true);
                        }
                    });
                }
                on_error.run(error);
            };
            if let Some(column_id) = effect.transition_to.as_deref()
                && let Err(error) = data::transition(auth, kind, &code, column_id).await
            {
                refused(true, error);
            } else if let Some(work_class) = effect.set_work_class.as_deref()
                && let Err(error) = data::set_work_class(auth, &code, work_class).await
            {
                refused(false, error);
            }
            drop(end);
        });
        true
    }

    /// A read ended: run the batch that waits, if any.
    fn finished(self) {
        if let Some(batch) = self
            .queue
            .try_update_value(data::LiveQueue::finished)
            .flatten()
        {
            self.run(batch);
        }
    }
}

/// The end of a batch of card reads: on drop, also when the page stops
/// the reads before their end. The queue thus cannot stay closed.
struct BatchEnd(LiveReads);

impl Drop for BatchEnd {
    fn drop(&mut self) {
        self.0.finished();
    }
}

/// The end of a drop (KAIROS-T-0619): on drop, also when the page stops
/// the drop before its end. A read of the card that waited runs now.
struct DropEnd {
    live: LiveReads,
    code: String,
}

impl Drop for DropEnd {
    fn drop(&mut self) {
        let waited = self
            .live
            .drops
            .try_update_value(|drops| drops.end(&self.code))
            .unwrap_or(false);
        if waited {
            self.live
                .request(data::Refresh::Card(std::mem::take(&mut self.code)));
        }
    }
}

/// One read of the board, for the queue (COLLIERY-T-0261). The read ends
/// when this value drops: at the end of the read, and also when the page
/// stops a read before its end. The queue thus cannot stay closed.
struct BoardRead {
    live: LiveReads,
}

impl BoardRead {
    fn start(live: LiveReads) -> Self {
        live.queue
            .try_update_value(data::LiveQueue::board_read_started);
        BoardRead { live }
    }
}

impl Drop for BoardRead {
    fn drop(&mut self) {
        // An event came during the read: the next batch runs.
        self.live.finished();
    }
}

/// `/boards/:board` — columns from the board config, items grouped, a
/// board-filtered live subscription, click-to-move, create-from-column.
#[component]
pub fn BoardPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    // The shell's shared identity (KAIROS-T-0072): powers derive from it.
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();

    // Bumped for a full read: re-runs the resource (silently — the
    // previous value stays up while the fetch runs).
    let refresh = RwSignal::new(0u64);
    // The live model behind the fine-grained board (KAIROS-T-0074):
    // reads update THIS SIGNAL and the mounted BoardBody diffs against
    // it through memos + keyed <For>s — the DOM is not rebuilt, so
    // transient UI state (open modals, element identity) survives
    // WS-driven updates.
    let model: RwSignal<Option<data::BoardView>> = RwSignal::new(None);
    // KAIROS-T-0615: an event of one card reads that card and patches the
    // model; the events of a 250 ms window read together, and one read
    // runs at a time (COLLIERY-T-0261: a full read of a large board is 2
    // requests or more).
    let live = LiveReads {
        auth,
        queue: StoredValue::new(data::LiveQueue::default()),
        refresh,
        model,
        drops: StoredValue::new(data::DropsInFlight::default()),
    };
    // KAIROS-T-0649: the board reads the last done cards, until "Show
    // all" of a done column.
    let done_all = RwSignal::new(false);
    provide_context(DoneAll(done_all));
    let board = LocalResource::new(move || {
        auth.track_session();
        let _ = refresh.get();
        let done_limit = (!done_all.get()).then_some(DONE_COLUMN_CARDS);
        let param = params.read().get("board").unwrap_or_default();
        let read = BoardRead::start(live);
        async move {
            let view = data::load_board_view(auth, &param, done_limit).await;
            drop(read);
            view
        }
    });
    let refetch = move || live.read_board_now();

    // Failed mutations (transition/create) surface as an error toast
    // (COLLIERY-T-1836: it was a fixed banner of this page).
    let toaster = use_toaster();
    let on_error = Callback::new(move |error: ApiError| {
        toaster.error(describe(&error));
    });
    let on_changed = Callback::new(move |_: ()| {
        refetch();
    });
    // KAIROS-T-0619: a drop moves the card at once; the echo patches it.
    let on_drop = Callback::new(move |card_drop: CardDrop| {
        if !live.run_drop(card_drop, on_error) {
            toaster.error(DROP_BUSY.to_string());
        }
    });

    // The live subscription follows whichever board is loaded; the guard
    // drops (closing the socket) on unmount or board change.
    let live_guard: StoredValue<Option<(String, live::LiveBoardGuard)>, LocalStorage> =
        StoredValue::new_local(None);
    Effect::new(move |_| {
        let Some(Ok(view)) = board.get() else {
            return;
        };
        let board_id = view.items.board.id;
        let current = live_guard.with_value(|g| g.as_ref().map(|(id, _)| id.clone()));
        if current.as_deref() == Some(board_id.as_str()) {
            return;
        }
        let subscribed = board_id.clone();
        let guard = live::subscribe_board_events(auth, board_id.clone(), move |event| {
            live.request(data::refresh_of(event, &subscribed));
        });
        live_guard.set_value(Some((board_id, guard)));
    });
    on_cleanup(move || live_guard.set_value(None));

    Effect::new(move |_| {
        if let Some(Ok(view)) = board.get() {
            model.set(Some(view));
        }
    });
    // One BoardBody instance per board id — navigating to another board
    // (or the first load) is the only thing that recreates it.
    let board_key =
        Memo::new(move |_| model.with(|m| m.as_ref().map(|view| view.items.board.id.clone())));
    // Powers re-derive when whoami OR the board changes, so affordances
    // appear as soon as both are known (KAIROS-T-0072). MEMOIZED
    // (KAIROS-T-0074): a plain Signal::derive notifies consumers on every
    // model refetch even when the value is identical — which re-rendered
    // every card and destroyed its DOM node, exactly what the
    // fine-grained rendering exists to prevent.
    let powers: Signal<BoardPowers> = Memo::new(move |_| {
        let identity = whoami
            .and_then(|resource| resource.get())
            .and_then(|result| result.ok());
        model
            .with(|m| {
                m.as_ref().zip(identity.as_ref()).map(|(view, me)| {
                    let board = &view.items.board;
                    board_powers(
                        me,
                        &board.slug,
                        board.team_id.as_deref(),
                        EntityKind::for_board_level(&board.board_level),
                    )
                })
            })
            .unwrap_or_default()
    })
    .into();

    // Which create button the board shows (COLLIERY-T-0232): "New task"
    // for a person who manages the board, "New request" for a member of
    // the tenant who does not. Memoized for the same reason as `powers`.
    let mode: Signal<Option<CreateMode>> = Memo::new(move |_| {
        let identity = whoami
            .and_then(|resource| resource.get())
            .and_then(|result| result.ok());
        model.with(|m| {
            m.as_ref().and_then(|view| {
                create_mode(
                    identity.as_ref(),
                    &view.items.board,
                    !view.detail.columns.is_empty(),
                )
            })
        })
    })
    .into();

    // KAIROS-T-0416: the user id of the reader. Their owner lane is first.
    let me: Signal<Option<String>> = Memo::new(move |_| {
        whoami
            .and_then(|resource| resource.get())
            .and_then(|result| result.ok())
            .map(|me| me.user.id)
    })
    .into();

    view! {
        {move || match board.get() {
            // A load error renders above the (stale) board rather than
            // destroying it; retry refetches in place.
            Some(Err(error)) => Some(view! {
                <ErrorState error on_retry=Callback::new(move |_| refetch())/>
            }.into_any()),
            None if model.with_untracked(|m| m.is_none()) => Some(view! {
                <Loading label="Loading board…"/>
            }.into_any()),
            _ => None,
        }}
        {move || board_key.get().map(|_| view! {
            <BoardBody model powers mode me on_changed on_drop/>
        })}
    }
}

/// The loaded board: header (+ create actions) and the column row.
///
/// Fine-grained rendering (KAIROS-T-0074): created once per board (keyed
/// on board id by [`BoardPage`]) and reads everything through memos over
/// the shared `model` signal, with keyed `<For>`s over columns and cards.
/// A refetch updates only what changed — an untouched card keeps its DOM
/// node across WS-driven updates (the old whole-DOM rebuild replaced
/// every element).
#[component]
fn BoardBody(
    /// The live board view model. ALWAYS `Some` while this component is
    /// mounted — [`BoardPage`] keys it on the model's board id.
    model: RwSignal<Option<data::BoardView>>,
    /// What the user may do here (KAIROS-T-0072) — gates every mutating
    /// affordance; the server stays the authority.
    powers: Signal<BoardPowers>,
    /// Which create button the board shows, if any (COLLIERY-T-0232,
    /// [`create_mode`]).
    mode: Signal<Option<CreateMode>>,
    /// The user id of the reader, when `whoami` resolved (KAIROS-T-0416).
    me: Signal<Option<String>>,
    on_changed: Callback<()>,
    /// A drop of a card (KAIROS-T-0619).
    on_drop: Callback<CardDrop>,
) -> impl IntoView {
    let auth = use_auth();
    // Identity fields are stable for this instance's lifetime (a board-id
    // change recreates the whole component).
    let board = model.with_untracked(|m| {
        m.as_ref()
            .map(|m| m.items.board.clone())
            .unwrap_or_default()
    });
    let create_kind = EntityKind::for_board_level(&board.board_level);
    let is_adr = board.board_level == "adr";
    let is_delivery = board.board_level == "delivery";
    let board_id = StoredValue::new(board.id.clone());
    let team_id = StoredValue::new(board.team_id.clone());

    // The one in-flight drag (KAIROS-T-0064); columns read it to decide
    // whether they are legal drop targets.
    let drag: RwSignal<Option<DragData>> = RwSignal::new(None);

    // Modal open-state lives HERE so refetches never reset it.
    let create_open = RwSignal::new(false);
    let doc_open = RwSignal::new(false);
    let archive_open = RwSignal::new(false);
    // KAIROS-T-0372: the flow panel, open or not.
    let flow_open = RwSignal::new(false);
    let flow_board = StoredValue::new(board.slug.clone());
    // KAIROS-T-0412: the members panel of the board, in its admin page.
    let members_href = StoredValue::new(members_panel_href(&board.id));
    // KAIROS-T-0615: the one read of the cards of the model. The memos
    // below read this, and not the model.
    let summary =
        Memo::new(move |_| model.with(|m| m.as_ref().map(board_summary).unwrap_or_default()));
    // KAIROS-T-0363: the tasks that "Archive completed" takes.
    let completed = Memo::new(move |_| summary.with(|s| s.completed));

    // ---- memos over the live model (notify only on actual change) --------
    let header_text = Memo::new(move |_| {
        model
            .with(|m| {
                m.as_ref().map(|view| {
                    let board = &view.items.board;
                    (
                        board.name.clone(),
                        format!("{} board · {}", board.board_level, board.slug),
                    )
                })
            })
            .unwrap_or_default()
    });
    // KAIROS-T-0062: creation is global and always lands in the board's
    // entry column (lowest position — Backlog/Draft/Discovery on the
    // seeded defaults). Creation is intake; flow happens by transition.
    let entry_column = Memo::new(move |_| {
        model.with(|m| {
            m.as_ref().and_then(|view| {
                view.detail
                    .columns
                    .iter()
                    .min_by_key(|column| column.position)
                    .map(|column| (column.id.clone(), column.name.clone()))
            })
        })
    });
    // Document create ("New document") — not offered on ADR boards:
    // documents attach to strategies/initiatives/tasks.
    let doc_parents = Memo::new(move |_| summary.with(|s| s.doc_parents.clone()));
    let documents_offered = Memo::new(move |_| !is_adr && !doc_parents.with(Vec::is_empty));
    // KAIROS-T-0109 (A-0019): the repository lens. `?repo=a,b` in the URL
    // is the source of truth for the selection, so it survives WS
    // refetches AND reloads; `?by_repo=1` groups the delivery board into
    // one lane per repository instead of Support/Planned. Lens writes
    // REPLACE the history entry (a filter change is not a page the back
    // button should revisit — and the prune write-back below must never
    // push, or "back" would land on the stale URL and bounce forward).
    let lens_nav = || NavigateOptions {
        replace: true,
        scroll: false,
        ..Default::default()
    };
    let (repo_query, set_repo_query) = query_signal_with_options::<String>("repo", lens_nav());
    let (by_repo_query, set_by_repo_query) =
        query_signal_with_options::<String>("by_repo", lens_nav());
    // Every repository slug present on the board, sorted — the chip set.
    let board_repos = Memo::new(move |_| summary.with(|s| s.repos.clone()));
    // What the URL asks for vs. what the board can show (KAIROS-T-0114):
    // a stale `?repo=<slug no longer here>` must not blank every task
    // with no chip lit, so the EFFECTIVE selection is pruned against the
    // board's slugs, and the pruned form is written back to the URL.
    let requested_repos = Memo::new(move |_| parse_repo_query(repo_query.get().as_deref()));
    let selected_repos = Memo::new(move |_| {
        requested_repos.with(|requested| board_repos.with(|known| prune_repos(requested, known)))
    });
    Effect::new(move |_| {
        let requested = requested_repos.get();
        let selected = selected_repos.get();
        if requested != selected {
            set_repo_query.set((!selected.is_empty()).then(|| selected.join(",")));
        }
    });
    // KAIROS-T-0416: `?by_owner=1` groups the cards by owner. One
    // grouping at a time: the toggle of one turns the other off.
    let (by_owner_query, set_by_owner_query) =
        query_signal_with_options::<String>("by_owner", lens_nav());
    let owner_offered = owner_grouping_offered(&board.board_level);
    let board_level = StoredValue::new(board.board_level.clone());
    let grouping = Memo::new(move |_| {
        let by_repo = by_repo_query.get();
        let by_owner = by_owner_query.get();
        board_level.with_value(|level| grouping_of(by_repo.as_deref(), by_owner.as_deref(), level))
    });
    // A URL with both: the owner grouping wins, and `by_repo` goes.
    Effect::new(move |_| {
        if grouping.get() == Grouping::Owner && by_repo_query.get().is_some() {
            set_by_repo_query.set(None);
        }
    });
    let group_by_repo = Memo::new(move |_| grouping.get() == Grouping::Repository);
    let group_by_owner = Memo::new(move |_| grouping.get() == Grouping::Owner);
    // KAIROS-T-0491: the collapsed lanes, kept in this browser for each
    // board and grouping.
    let collapse_storage_key =
        Memo::new(move |_| board_id.with_value(|id| collapse_key(id, grouping.get())));
    let lane_collapse = LaneCollapse::new(collapse_storage_key);
    provide_context(lane_collapse);
    // The lens row renders whenever the board has repositories OR a lens
    // query param is set (so a stale/odd URL always has a visible "clear");
    // the group-by toggle renders when grouping is meaningful (>1 repo) OR
    // already on (so it can always be turned off).
    let lens_active =
        Memo::new(move |_| repo_query.get().is_some() || by_repo_query.get().is_some());
    let lens_shown = Memo::new(move |_| !board_repos.with(Vec::is_empty) || lens_active.get());
    let group_toggle_shown =
        Memo::new(move |_| board_repos.with(|r| r.len() > 1) || group_by_repo.get());
    // The group-by lanes derive from the effective selection (+ unbound).
    let lanes = Memo::new(move |_| {
        selected_repos.with(|selected| board_repos.with(|known| repo_lanes(selected, known)))
    });
    let toggle_repo = move |slug: String| {
        let mut selected = selected_repos.get_untracked();
        match selected.iter().position(|s| *s == slug) {
            Some(i) => {
                selected.remove(i);
            }
            None => selected.push(slug),
        }
        set_repo_query.set((!selected.is_empty()).then(|| selected.join(",")));
    };
    let clear_lens = move || {
        set_repo_query.set(None);
        set_by_repo_query.set(None);
    };
    // KAIROS-T-0322: the team filter of the boards of strategies and of
    // initiatives. `?team=<slug>` or `?no_team=1` in the URL, the names of
    // the REST filter, so the selection survives refetches and reloads.
    // It narrows the strategies and the initiatives only.
    let team_filter_offered = !is_delivery && !is_adr;
    let (team_query, set_team_query) = query_signal_with_options::<String>("team", lens_nav());
    let (no_team_query, set_no_team_query) =
        query_signal_with_options::<String>("no_team", lens_nav());
    let board_teams = Memo::new(move |_| summary.with(|s| s.teams.clone()));
    let team_selection = Memo::new(move |_| {
        team_selection_of(team_query.get().as_deref(), no_team_query.get().as_deref())
    });
    let select_team = move |choice: TeamSelection| {
        let next = if team_selection.get_untracked() == choice {
            TeamSelection::All
        } else {
            choice
        };
        match next {
            TeamSelection::All => {
                set_team_query.set(None);
                set_no_team_query.set(None);
            }
            TeamSelection::NoTeam => {
                set_team_query.set(None);
                set_no_team_query.set(Some("1".to_string()));
            }
            TeamSelection::Team(slug) => {
                set_no_team_query.set(None);
                set_team_query.set(Some(slug));
            }
        }
    };
    // KAIROS-T-0582: the card filters "Mine" (`?mine=1`) and "Hide done"
    // (`?hide_done=1`), in the URL as the other filters.
    let (mine_query, set_mine_query) = query_signal_with_options::<String>("mine", lens_nav());
    let (hide_done_query, set_hide_done_query) =
        query_signal_with_options::<String>("hide_done", lens_nav());
    let mine_offered = owner_grouping_offered(&board.board_level);
    let mine_on = Memo::new(move |_| {
        mine_offered && mine_query.get().as_deref() == Some("1") && me.with(Option::is_some)
    });
    let hide_done_on = Memo::new(move |_| hide_done_query.get().as_deref() == Some("1"));
    let done_offered =
        Memo::new(move |_| summary.with(|s| s.columns.iter().any(|column| column.is_done)));
    let columns = Memo::new(move |_| {
        let selected = selected_repos.get();
        let team = team_selection.get();
        let mine = mine_on.get().then(|| me.get()).flatten();
        let hide_done = hide_done_on.get();
        summary.with(|s| {
            let mut columns = s.columns.clone();
            if hide_done {
                columns.retain(|column| !column.is_done);
            }
            if !selected.is_empty() || team != TeamSelection::All || mine.is_some() {
                for column in columns.iter_mut() {
                    column.cards.retain(|card| {
                        lens_admits(card, &selected)
                            && team_admits(card, &team)
                            && mine_admits(card, mine.as_deref())
                    });
                }
            }
            columns
        })
    });

    // KAIROS-T-0416: the owner lanes follow the cards, so a refetch after
    // a change of a claim or an owner moves the cards and the lanes.
    let owner_lane_list = Memo::new(move |_| {
        let me = me.get();
        columns.with(|columns| owner_lanes(columns, me.as_deref()))
    });

    // KAIROS-T-0491: the ids of the lanes on screen, for "Collapse all"
    // and "Expand all".
    let shown_lanes = Memo::new(move |_| -> Vec<String> {
        match grouping.get() {
            Grouping::Owner => owner_lane_list.with(|list| {
                list.iter()
                    .map(|l| lane_id(None, &GroupLane::Owner(l.user_id.clone())))
                    .collect()
            }),
            Grouping::Repository if is_delivery => lanes.with(|keys| {
                keys.iter()
                    .map(|key| {
                        let group = match key {
                            Some(slug) => GroupLane::Slug(slug.clone()),
                            None => GroupLane::Unbound,
                        };
                        lane_id(None, &group)
                    })
                    .collect()
            }),
            _ if is_delivery => [LANE_SUPPORT, LANE_PLANNED]
                .iter()
                .map(|lane| lane_id(Some(lane), &GroupLane::Any))
                .collect(),
            _ => Vec::new(),
        }
    });
    // KAIROS-T-0582: one WIP chip for each column. The first open lane
    // shows the chips; the other lanes do not repeat them.
    provide_context(WipLane(Memo::new(move |_| {
        shown_lanes.with(|ids| wip_lane(ids, |id| lane_collapse.is_collapsed(id)))
    })));

    let create_label = StoredValue::new(
        create_kind
            .map(|kind| format!("New {}", kind.label()))
            .unwrap_or_default(),
    );

    // COLLIERY-T-0232: the name of the team that gets a request. The board
    // carries the id of its team and not the name, and `whoami` lists only
    // the teams of the person, who is not in this one. The list of teams
    // loads only for a person who can send a request.
    let teams = LocalResource::new(move || {
        auth.track_session();
        let wanted = mode.get() == Some(CreateMode::Request);
        async move {
            if wanted {
                crate::pages::teams::api::list_teams(auth).await
            } else {
                Ok(Vec::new())
            }
        }
    });
    let team_name = Memo::new(move |_| {
        let list = teams
            .get()
            .and_then(|result| result.ok())
            .unwrap_or_default();
        team_id.with_value(|id| {
            id.as_ref().and_then(|id| {
                list.iter()
                    .find(|team| team.id == *id)
                    .map(|team| team.name.clone())
            })
        })
    });
    // The request that the person sent a moment ago. It lives HERE, with
    // the open-state of the dialogs, so that a refetch does not drop it.
    let sent: RwSignal<Option<SentRequest>> = RwSignal::new(None);
    let on_requested = Callback::new(move |request: SentRequest| sent.set(Some(request)));

    view! {
        {move || {
            let (title, sub) = header_text.get();
            let header_right: Children = Box::new(move || view! {
                <Group gap="xs">
                    {move || match mode.get() {
                        Some(CreateMode::Manager) => Some(view! {
                            <Button size="xs" on_click=Callback::new(move |_| create_open.set(true))>
                                {create_label.get_value()}
                            </Button>
                        }.into_any()),
                        // COLLIERY-T-0232: the same place, the same
                        // component, the same dialog in its request mode.
                        Some(CreateMode::Request) => Some(view! {
                            <Button size="xs" on_click=Callback::new(move |_| create_open.set(true))>
                                "New request"
                            </Button>
                        }.into_any()),
                        None => None,
                    }}
                    {move || (documents_offered.get() && powers.get().documents).then(|| view! {
                        <Button variant="default" size="xs" on_click=Callback::new(move |_| doc_open.set(true))>
                            "New document"
                        </Button>
                    })}
                    // KAIROS-T-0372: the flow metrics, for each reader.
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| flow_open.update(|open| *open = !*open))>
                        {move || if flow_open.get() { "Hide flow" } else { "Flow" }}
                    </Button>
                    // KAIROS-T-0363: only for a person who may archive each
                    // task of the board (`manage_tasks`).
                    {move || (is_delivery && powers.get().create).then(|| view! {
                        <Button variant="default" size="xs"
                            disabled=Signal::derive(move || completed.get() == 0)
                            on_click=Callback::new(move |_| archive_open.set(true))>
                            "Archive completed"
                        </Button>
                    })}
                    // KAIROS-T-0412: the members and grants of the board,
                    // for a person who may manage them.
                    {move || powers.get().members.then(|| view! {
                        <Anchor href=members_href.get_value()
                            attr:data-testid="board-members-link">
                            "Members"
                        </Anchor>
                    })}
                </Group>
            }.into_any());
            view! { <PageHeader title sub right=header_right/> }
        }}
        {move || flow_open.get().then(|| view! {
            <flow::FlowPanel board=flow_board.get_value()/>
        })}
        {move || {
            // COLLIERY-T-0261: the board has 2000 cards at most. A board
            // with more cards says so, with the numbers.
            model
                .with(|m| {
                    m.as_ref().and_then(|view| {
                        data::board_cap_note(view.items.card_count(), view.items.total)
                    })
                })
                .map(|note| view! {
                    <div class="kairos-board__notice" role="status" data-testid="board-cap">
                        <Banner color=token::GOLD icon="!">{note}</Banner>
                    </div>
                })
        }}
        {move || sent.get().map(|request| {
            // COLLIERY-T-0232: the notice names the team, the column and
            // the lane, and the short code is a link to the item. The card
            // is in the Support lane, which is on the page with the
            // Planned lane, unless the repository lens hides it.
            let hidden = group_by_repo.get()
                || selected_repos.with(|selected| lens_hides_request(&request, selected));
            let entry_name = entry_column.get().map(|(_, name)| name).unwrap_or_default();
            let (before, after) =
                request_notice(team_name.get().as_deref(), &entry_name, hidden);
            let href = format!("/items/{}", request.short_code);
            view! {
                <div class="kairos-board__notice" role="status" data-testid="request-sent">
                    <Banner color=token::OK icon="✓">
                        <Group justify="between">
                            <Text size="sm">
                                {before}
                                <a class="cl-mono kairos-card__code" href=href>
                                    {request.short_code}
                                </a>
                                {after}
                            </Text>
                            <Button variant="default" size="xs"
                                on_click=Callback::new(move |_| sent.set(None))>
                                "Dismiss"
                            </Button>
                        </Group>
                    </Banner>
                </div>
            }
        })}
        {move || (is_delivery && lens_shown.get()).then(|| view! {
            <div class="kairos-board__lens" data-testid="repo-lens">
                <Group gap="xs" wrap=true>
                    <Text dimmed=true size="xs">"Repository"</Text>
                    <For
                        each=move || board_repos.get()
                        key=|slug| slug.clone()
                        children=move |slug: String| {
                            let on_slug = slug.clone();
                            let is_on = Memo::new(move |_| selected_repos.with(|s| s.contains(&slug)));
                            let label = on_slug.clone();
                            let attr = on_slug.clone();
                            view! {
                                <Chip
                                    label
                                    active=is_on
                                    on_click=Callback::new(move |_| toggle_repo(on_slug.clone()))
                                    attr:data-repo=attr
                                />
                            }
                        }
                    />
                    {move || group_toggle_shown.get().then(|| view! {
                        <Chip
                            label="Group by repository"
                            active=group_by_repo
                            on_click=Callback::new(move |_| {
                                let on = !group_by_repo.get_untracked();
                                if on {
                                    set_by_owner_query.set(None);
                                }
                                set_by_repo_query.set(on.then(|| "1".to_string()))
                            })
                            attr:data-testid="group-by-repo"
                        />
                    })}
                    {move || lens_active.get().then(|| view! {
                        <Button
                            variant="subtle"
                            size="xs"
                            on_click=Callback::new(move |_| clear_lens())
                            attr:data-testid="clear-repo-lens"
                        >
                            "Clear"
                        </Button>
                    })}
                </Group>
            </div>
        })}
        // KAIROS-T-0416: the owner grouping, on each level but ADR.
        {owner_offered.then(|| view! {
            <div class="kairos-board__lens" data-testid="owner-lens">
                <Group gap="xs" wrap=true>
                    <Text dimmed=true size="xs">"Lanes"</Text>
                    <Chip
                        label="Group by owner"
                        active=group_by_owner
                        on_click=Callback::new(move |_| {
                            let on = !group_by_owner.get_untracked();
                            if on {
                                set_by_repo_query.set(None);
                            }
                            set_by_owner_query.set(on.then(|| "1".to_string()))
                        })
                        attr:data-testid="group-by-owner"
                    />
                </Group>
            </div>
        })}
        {move || (team_filter_offered && (!board_teams.with(Vec::is_empty)
            || team_selection.get() != TeamSelection::All)).then(|| view! {
            <div class="kairos-board__lens" data-testid="team-filter">
                <Group gap="xs" wrap=true>
                    <Text dimmed=true size="xs">"Team"</Text>
                    <For
                        each=move || board_teams.get()
                        key=|slug| slug.clone()
                        children=move |slug: String| {
                            let choice = slug.clone();
                            let is_on = Memo::new(move |_| {
                                team_selection.with(|t| *t == TeamSelection::Team(slug.clone()))
                            });
                            let label = choice.clone();
                            let attr = choice.clone();
                            view! {
                                <Chip
                                    label
                                    active=is_on
                                    on_click=Callback::new(move |_| select_team(TeamSelection::Team(choice.clone())))
                                    attr:data-team=attr
                                />
                            }
                        }
                    />
                    {
                        let is_on = Memo::new(move |_| team_selection.get() == TeamSelection::NoTeam);
                        view! {
                            <Chip
                                label="No team"
                                active=is_on
                                on_click=Callback::new(move |_| select_team(TeamSelection::NoTeam))
                                attr:data-testid="no-team"
                            />
                        }
                    }
                    {move || (team_selection.get() != TeamSelection::All).then(|| view! {
                        <Button
                            variant="subtle"
                            size="xs"
                            on_click=Callback::new(move |_| select_team(TeamSelection::All))
                            attr:data-testid="clear-team-filter"
                        >
                            "Clear"
                        </Button>
                    })}
                </Group>
            </div>
        })}
        // KAIROS-T-0582: the card filters.
        {move || ((mine_offered && me.with(Option::is_some)) || done_offered.get()
            || hide_done_on.get()).then(|| view! {
            <div class="kairos-board__lens" data-testid="card-filters">
                <Group gap="xs" wrap=true>
                    <Text dimmed=true size="xs">"Cards"</Text>
                    {move || (mine_offered && me.with(Option::is_some)).then(|| view! {
                        <Chip
                            label="Mine"
                            active=mine_on
                            on_click=Callback::new(move |_| {
                                let on = !mine_on.get_untracked();
                                set_mine_query.set(on.then(|| "1".to_string()))
                            })
                            attr:data-testid="filter-mine"
                        />
                    })}
                    {move || (done_offered.get() || hide_done_on.get()).then(|| view! {
                        <Chip
                            label="Hide done"
                            active=hide_done_on
                            on_click=Callback::new(move |_| {
                                let on = !hide_done_on.get_untracked();
                                set_hide_done_query.set(on.then(|| "1".to_string()))
                            })
                            attr:data-testid="filter-hide-done"
                        />
                    })}
                </Group>
            </div>
        })}
        {
            // KAIROS-T-0077: delivery boards split into Support (on top,
            // the expedite convention) and Planned lanes — the lane is a
            // pure projection of tasks.work_class. Other levels keep the
            // single unlaned row. The level is fixed per BoardBody
            // instance, so this branch is deliberately non-reactive.
            // KAIROS-T-0109: with `?by_repo=1` a delivery board instead
            // shows one lane per repository (plus "No repository"), and
            // drops only transition — the lane axis is not a work_class.
            // Lanes are a keyed <For> (key = slug) so a WS refetch that
            // changes the slug set diffs lanes instead of rebuilding them
            // all (KAIROS-T-0074 rule, KAIROS-T-0114).
            // KAIROS-T-0328: one container scrolls each lane of the board
            // sideways together, so the columns of the lanes stay in line.
            // It knows the number of columns, so a lane is as wide as its
            // columns at their readable floor (app.css).
            let lanes_style =
                move || format!("--kairos-columns: {}", columns.with(Vec::len).max(1));
            let owner_view = move || view! {
                <For
                    each=move || owner_lane_list.get()
                    // KAIROS-T-0616: the owner, not the whole lane; a
                    // relabel of a person does not rebuild their lane.
                    key=|lane: &OwnerLane| (lane.user_id.clone(), lane.mine)
                    children=move |lane: OwnerLane| {
                        view! {
                            <OwnerLaneSection lane columns drag powers on_drop/>
                        }
                    }
                />
            };
            let lanes_view = if is_delivery {
                view! {
                    {move || if group_by_owner.get() {
                        owner_view().into_any()
                    } else if group_by_repo.get() {
                        view! {
                            <For
                                each=move || lanes.get()
                                key=|lane: &LaneKey| lane.clone()
                                children=move |lane: LaneKey| {
                                    view! {
                                        <RepoLaneSection repo=lane columns drag powers on_drop/>
                                    }
                                }
                            />
                        }
                        .into_any()
                    } else {
                        view! {
                            <LaneSection lane=LANE_SUPPORT label="Support" caption="unplanned intake"
                                color=token::GOLD columns drag powers on_drop/>
                            <LaneSection lane=LANE_PLANNED label="Planned" caption="scheduled work"
                                color=token::TEAL columns drag powers on_drop/>
                        }.into_any()
                    }}
                }
                .into_any()
            } else {
                view! {
                    {move || if group_by_owner.get() {
                        owner_view().into_any()
                    } else {
                        view! { <LaneColumns lane=None group=GroupLane::Any columns drag powers on_drop/> }
                            .into_any()
                    }}
                }
                .into_any()
            };
            let shown_lane_ids = move || shown_lanes.get();
            let lane_actions = move || (!shown_lane_ids().is_empty()).then(|| view! {
                <div class="kairos-board__lane-actions">
                    <Group gap="xs">
                        <Button
                            variant="subtle"
                            size="xs"
                            on_click=Callback::new(move |_| lane_collapse.set_all(&shown_lane_ids(), true))
                            attr:data-testid="collapse-all-lanes"
                        >
                            "Collapse all"
                        </Button>
                        <Button
                            variant="subtle"
                            size="xs"
                            on_click=Callback::new(move |_| lane_collapse.set_all(&shown_lane_ids(), false))
                            attr:data-testid="expand-all-lanes"
                        >
                            "Expand all"
                        </Button>
                    </Group>
                </div>
            });
            view! {
                {lane_actions}
                <div class="kairos-board__lanes" style=lanes_style>{lanes_view}</div>
            }
        }
        {move || create_kind.zip(entry_column.get()).zip(mode.get()).map(|((kind, entry), mode)| view! {
            <CreateItemModal
                open=create_open
                kind
                mode
                board_id=board_id.get_value()
                team_id=team_id.get_value()
                team_name=team_name.get()
                entry
                on_changed
                on_requested
            />
        })}
        {move || documents_offered.get().then(|| view! {
            <CreateDocumentModal open=doc_open parents=doc_parents
                board_slug=board.slug.clone() board_name=board.name.clone() on_changed/>
        })}
        {is_delivery.then(|| view! {
            <ArchiveCompletedDialog open=archive_open board_id=board_id.get_value()
                count=completed on_changed/>
        })}
    }
}

/// The question of the "Archive completed" dialog (KAIROS-T-0363). With
/// an age (KAIROS-T-0649), the count of the board is not that of the
/// call, so the question gives the age. Pure, host-tested.
fn archive_completed_title(count: usize, older_than_days: Option<u32>) -> String {
    match older_than_days {
        Some(days) => format!(
            "Archive the completed tasks that entered their column {} ago or before?",
            days_text(days)
        ),
        None => {
            let tasks = if count == 1 { "task" } else { "tasks" };
            format!("Archive {count} completed {tasks}?")
        }
    }
}

/// The age of the "older than" field of the dialog (KAIROS-T-0649): no
/// value for an empty field, else a whole number of days from 1. Pure,
/// host-tested.
fn older_than_days(text: &str) -> Result<Option<u32>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<u32>() {
        Ok(days) if (1..=36_500).contains(&days) => Ok(Some(days)),
        _ => Err("Give a whole number of days from 1 to 36500.".to_string()),
    }
}

/// The "Archive completed" dialog (KAIROS-T-0363): the confirmation gives
/// the count, and says that each task can be restored.
#[component]
fn ArchiveCompletedDialog(
    open: RwSignal<bool>,
    #[prop(into)] board_id: String,
    count: Memo<usize>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let toaster = use_toaster();
    let board_id = StoredValue::new(board_id);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // KAIROS-T-0649: "older than N days"; empty archives each done task.
    let days = RwSignal::new(String::new());
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        let older = match older_than_days(&days.get_untracked()) {
            Ok(older) => older,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = data::archive_completed(auth, &board_id.get_value(), older).await;
            busy.set(false);
            match result {
                Ok(outcome) => {
                    open.set(false);
                    toaster.success(format!(
                        "Kairos archived {} completed {}. You can restore each one.",
                        outcome.count,
                        if outcome.count == 1 { "task" } else { "tasks" }
                    ));
                    on_changed.run(());
                }
                Err(e) => error.set(Some(describe(&e))),
            }
        });
    });
    view! {
        <ConfirmDialog
            open
            title=Signal::derive(move || {
                archive_completed_title(count.get(), older_than_days(&days.get()).ok().flatten())
            })
            message="You can restore each one."
            confirm_label="Archive"
            danger=false
            busy
            on_confirm=confirm
        >
            <TextInput label="Older than (days)" value=days
                placeholder="Empty: each completed task"/>
            {move || error.get().map(|message| view! {
                <Alert title="Archive failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

// ---------------------------------------------------------------------------
// Lanes + columns (KAIROS-T-0077)
// ---------------------------------------------------------------------------

/// One horizontal lane on a delivery board: accent header with a live
/// card count, then the column row filtered to this lane. An empty
/// Support lane renders slim (`--empty`) but its columns stay valid drop
/// targets.
#[component]
fn LaneSection(
    lane: &'static str,
    label: &'static str,
    caption: &'static str,
    color: &'static str,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_drop: Callback<CardDrop>,
) -> impl IntoView {
    let count = Memo::new(move |_| {
        columns.with(|columns| {
            columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .filter(|card| card_lane(card.work_class.as_deref()) == lane)
                .count()
        })
    });
    let id = lane_id(Some(lane), &GroupLane::Any);
    let collapsed = lane_collapsed(id.clone());
    let id_attr = id.clone();
    view! {
        <section
            class="kairos-board__lane"
            class:kairos-board__lane--support=(lane == LANE_SUPPORT)
            class:kairos-board__lane--planned=(lane == LANE_PLANNED)
            class:kairos-board__lane--empty=move || count.get() == 0
            class:kairos-board__lane--collapsed=collapsed
            data-lane-id=id_attr
        >
            <header class="kairos-board__lane-head">
                <Group gap="xs">
                    <LaneToggle id=id label/>
                    <Pill color=color>{label}</Pill>
                    <Text dimmed=true size="xs">{move || count.get().to_string()}</Text>
                    <Text dimmed=true size="xs">{caption}</Text>
                </Group>
            </header>
            {move || if collapsed.get() {
                view! { <LaneSummary lane=Some(lane) group=GroupLane::Any columns/> }.into_any()
            } else {
                view! {
                    <LaneColumns lane=Some(lane) group=GroupLane::Any columns drag powers on_drop/>
                }.into_any()
            }}
        </section>
    }
}

/// Which cards a lane shows: by repository (KAIROS-T-0109) or by owner
/// (KAIROS-T-0416).
#[derive(Clone, Debug, PartialEq)]
enum GroupLane {
    /// Every card.
    Any,
    /// Tasks bound to this slug.
    Slug(String),
    /// Tasks with no repository (and non-task cards).
    Unbound,
    /// The cards of this owner (a user id), or with no owner (`None`).
    Owner(Option<String>),
}

impl GroupLane {
    fn admits(&self, card: &CardModel) -> bool {
        match self {
            GroupLane::Any => true,
            GroupLane::Slug(slug) => card.repository.as_deref() == Some(slug.as_str()),
            GroupLane::Unbound => card.repository.is_none(),
            GroupLane::Owner(owner) => owner_key(card) == owner.as_deref(),
        }
    }
}

/// Does a lane show this card? `lane` is the Support/Planned lane of a
/// delivery board (`None`: every work class); `group` narrows by
/// repository or owner. The columns and the collapsed row of a lane both
/// use it, so their counts agree (KAIROS-T-0491).
fn lane_admits(card: &CardModel, lane: Option<&str>, group: &GroupLane) -> bool {
    group.admits(card)
        && match lane {
            None => true,
            Some(lane) => card_lane(card.work_class.as_deref()) == lane,
        }
}

// ---------------------------------------------------------------------------
// Collapsed lanes (KAIROS-T-0491) — pure, host-tested
// ---------------------------------------------------------------------------

/// The id of a lane in the collapsed set: `support` or `planned`,
/// `repo:<slug>` (`repo:` with no repository), `owner:<user id>`
/// (`owner:` with no owner).
fn lane_id(lane: Option<&str>, group: &GroupLane) -> String {
    match group {
        GroupLane::Any => lane.unwrap_or("all").to_string(),
        GroupLane::Slug(slug) => format!("repo:{slug}"),
        GroupLane::Unbound => "repo:".to_string(),
        GroupLane::Owner(owner) => format!("owner:{}", owner.as_deref().unwrap_or_default()),
    }
}

/// The name of a grouping in the storage key.
fn grouping_name(grouping: Grouping) -> &'static str {
    match grouping {
        Grouping::Level => "level",
        Grouping::Repository => "repository",
        Grouping::Owner => "owner",
    }
}

/// The localStorage key of the collapsed lanes of one board and grouping.
fn collapse_key(board_id: &str, grouping: Grouping) -> String {
    format!(
        "kairos.lanes.collapsed.{board_id}.{}",
        grouping_name(grouping)
    )
}

/// The collapsed set from its stored text: one lane id on each line. A
/// missing or empty value is an empty set.
fn parse_collapsed(raw: Option<&str>) -> std::collections::BTreeSet<String> {
    raw.unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// The stored text of a collapsed set.
fn serialize_collapsed(set: &std::collections::BTreeSet<String>) -> String {
    set.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// The card count of a lane in each column, in column order: the row of a
/// collapsed lane.
fn lane_counts(
    columns: &[ColumnModel],
    lane: Option<&str>,
    group: &GroupLane,
) -> Vec<(String, usize)> {
    columns
        .iter()
        .map(|column| {
            let count = column
                .cards
                .iter()
                .filter(|card| lane_admits(card, lane, group))
                .count();
            (column.name.clone(), count)
        })
        .collect()
}

/// The collapsed lanes of the board, for the grouping on screen. The state
/// lives in this browser (localStorage). Each read and write handles a
/// failure: without storage the lanes collapse for this page only.
#[derive(Clone, Copy)]
struct LaneCollapse {
    collapsed: RwSignal<std::collections::BTreeSet<String>>,
    key: Memo<String>,
}

/// The localStorage of the window, when the browser gives it.
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl LaneCollapse {
    fn new(key: Memo<String>) -> Self {
        let collapsed = RwSignal::new(Self::load(&key.get_untracked()));
        // A different grouping has its own set.
        Effect::new(move |previous: Option<String>| {
            let current = key.get();
            if previous.is_some_and(|p| p != current) {
                collapsed.set(Self::load(&current));
            }
            current
        });
        Self { collapsed, key }
    }

    fn load(key: &str) -> std::collections::BTreeSet<String> {
        let raw = local_storage().and_then(|storage| storage.get_item(key).ok().flatten());
        parse_collapsed(raw.as_deref())
    }

    fn save(&self) {
        let Some(storage) = local_storage() else {
            return;
        };
        let key = self.key.get_untracked();
        let set = self.collapsed.get_untracked();
        let _ = if set.is_empty() {
            storage.remove_item(&key)
        } else {
            storage.set_item(&key, &serialize_collapsed(&set))
        };
    }

    fn is_collapsed(&self, id: &str) -> bool {
        self.collapsed.with(|set| set.contains(id))
    }

    fn toggle(&self, id: &str) {
        self.collapsed.update(|set| {
            if !set.remove(id) {
                set.insert(id.to_string());
            }
        });
        self.save();
    }

    /// Collapse (`on`) or expand each of `ids`.
    fn set_all(&self, ids: &[String], on: bool) {
        self.collapsed.update(|set| {
            for id in ids {
                if on {
                    set.insert(id.clone());
                } else {
                    set.remove(id);
                }
            }
        });
        self.save();
    }
}

// ---------------------------------------------------------------------------
// Done columns, WIP chips, refused drops, priority (KAIROS-T-0582) — pure,
// host-tested
// ---------------------------------------------------------------------------

/// The number of cards that a done column shows until "Show all".
const DONE_COLUMN_CARDS: usize = 20;

/// The cards that a column of a lane shows: each card, or in a done
/// column the last [`DONE_COLUMN_CARDS`] cards (the highest short codes:
/// the server and the patches keep the order of the short codes), until
/// `show_all`.
fn visible_cards<T: Clone>(cards: &[T], is_done: bool, show_all: bool) -> Vec<T> {
    let from = if is_done && !show_all {
        cards.len().saturating_sub(DONE_COLUMN_CARDS)
    } else {
        0
    };
    cards[from..].to_vec()
}

/// The words of the button of a done column, or `None` when the column
/// needs no button. `hidden` is the number of cards that the column does
/// not show, of `total`.
fn show_all_label(is_done: bool, show_all: bool, hidden: usize, total: usize) -> Option<String> {
    if !is_done {
        None
    } else if show_all && total > DONE_COLUMN_CARDS {
        Some(format!("Show the last {DONE_COLUMN_CARDS} cards"))
    } else if hidden > 0 {
        Some(format!("Show all {total} cards"))
    } else {
        None
    }
}

/// Does the "Mine" filter admit this card? `me` is the reader (a user
/// id) while the filter is on, `None` while it is off. The card of the
/// reader: the reader has the claim of the task, or owns the strategy or
/// the initiative.
fn mine_admits(card: &CardModel, me: Option<&str>) -> bool {
    me.is_none_or(|me| owner_key(card) == Some(me))
}

/// The lane that shows the WIP chips of the columns: the first lane on
/// screen that is not collapsed. `None`: no lanes (one row), or each lane
/// is collapsed.
fn wip_lane(shown: &[String], collapsed: impl Fn(&str) -> bool) -> Option<String> {
    shown.iter().find(|id| !collapsed(id)).cloned()
}

/// The lane of the WIP chips, in the context of the board body.
#[derive(Clone, Copy)]
struct WipLane(Memo<Option<String>>);

/// The note of a column that refuses the drag: the board has no
/// transition from the column of the card (`source_name`) to this column.
/// `None` for the column of the card and for an allowed target.
fn refusal_note(drag: &DragData, column_id: &str, source_name: &str) -> Option<String> {
    (drag.source_column != column_id && !drag.targets.iter().any(|t| t == column_id))
        .then(|| format!("No transition from {source_name}."))
}

/// "Show all" of the done columns (KAIROS-T-0649): while it is on, the
/// board reads each done card.
#[derive(Clone, Copy)]
struct DoneAll(RwSignal<bool>);

/// The time now, in milliseconds. The host tests have no browser clock:
/// there it is 0, and no card shows its days.
fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

/// The days from the RFC 3339 time `at` (UTC) to `now_ms`, or `None` for
/// a time that does not parse or is after `now_ms`. Pure, host-tested.
fn days_in_column(at: &str, now_ms: f64) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| at.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hh, mm, ss) = (number(11..13)?, number(14..16)?, number(17..19)?);
    // The days from 1970-01-01 of a civil date (H. Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let epoch_days = era * 146_097 + doe - 719_468;
    let at_ms = ((epoch_days * 86_400 + hh * 3_600 + mm * 60 + ss) * 1_000) as f64;
    (now_ms >= at_ms).then(|| ((now_ms - at_ms) / 86_400_000.0).floor() as i64)
}

/// "1 day" or "N days". Pure.
fn days_text(days: u32) -> String {
    if days == 1 {
        "1 day".to_string()
    } else {
        format!("{days} days")
    }
}

/// The pill of the days in the column: `None` before the first full day.
/// Pure, host-tested.
fn days_pill(days: i64) -> Option<String> {
    u32::try_from(days)
        .ok()
        .filter(|days| *days >= 1)
        .map(|days| format!("{} in column", days_text(days)))
}

/// The pill of the notes count: `None` for no notes. Pure, host-tested.
fn notes_pill(count: i64) -> Option<String> {
    match count {
        ..=0 => None,
        1 => Some("1 note".to_string()),
        n => Some(format!("{n} notes")),
    }
}

/// The pill of the priority metadata of a card. A high priority has the
/// colour of a warning; red stays for faults.
fn priority_pill(value: &str) -> (String, &'static str) {
    let high = matches!(value.to_ascii_lowercase().as_str(), "critical" | "high");
    let color = if high { token::GOLD } else { token::MUTED };
    (format!("priority {value}"), color)
}

// ---------------------------------------------------------------------------
// Repository lens projections (KAIROS-T-0109 / KAIROS-T-0114) — pure,
// host-tested
// ---------------------------------------------------------------------------

/// Parse `?repo=a,b` into the requested slugs: empty segments dropped,
/// duplicates removed (first occurrence wins, order kept).
fn parse_repo_query(query: Option<&str>) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    for slug in query
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        if !slugs.iter().any(|s| s == slug) {
            slugs.push(slug.to_string());
        }
    }
    slugs
}

/// The effective selection: the requested slugs that are actually on the
/// board. A stale `?repo=` (re-homed, unbound, or simply mistyped) is
/// ignored rather than blanking every task with no chip lit.
fn prune_repos(requested: &[String], known: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|slug| known.contains(slug))
        .cloned()
        .collect()
}

/// The team filter of a board of strategies or of initiatives
/// (KAIROS-T-0322).
#[derive(Clone, Debug, PartialEq, Eq)]
enum TeamSelection {
    All,
    Team(String),
    NoTeam,
}

/// The selection from the two query values. `no_team=1` wins over a team:
/// the server refuses the two together, and the GUI never writes both.
fn team_selection_of(team: Option<&str>, no_team: Option<&str>) -> TeamSelection {
    match (team, no_team) {
        (_, Some("1")) => TeamSelection::NoTeam,
        (Some(slug), _) if !slug.is_empty() => TeamSelection::Team(slug.to_string()),
        _ => TeamSelection::All,
    }
}

/// Does the team filter admit this card? It narrows the STRATEGIES and
/// the INITIATIVES only — tasks and ADRs always stay.
fn team_admits(card: &CardModel, selection: &TeamSelection) -> bool {
    if !matches!(card.kind, EntityKind::Strategy | EntityKind::Initiative) {
        return true;
    }
    match selection {
        TeamSelection::All => true,
        TeamSelection::NoTeam => card.teams.is_empty(),
        TeamSelection::Team(slug) => card.teams.contains(slug),
    }
}

/// Does the lens admit this card? The filter narrows TASKS only — other
/// kinds always stay — and an empty selection admits everything.
fn lens_admits(card: &CardModel, selected: &[String]) -> bool {
    selected.is_empty()
        || card.kind != EntityKind::Task
        || card
            .repository
            .as_ref()
            .is_some_and(|slug| selected.contains(slug))
}

/// A group-by-repository lane's identity: `Some(slug)` or the unbound
/// remainder (`None`). The keyed `<For>` diffs lanes on it.
type LaneKey = Option<String>;

/// The group-by-repository lanes: one per EFFECTIVE selection (every
/// board repository when nothing is selected), in board order, then the
/// unbound remainder (`None`).
fn repo_lanes(selected: &[String], known: &[String]) -> Vec<LaneKey> {
    known
        .iter()
        .filter(|slug| selected.is_empty() || selected.contains(slug))
        .cloned()
        .map(Some)
        .chain(std::iter::once(None))
        .collect()
}

// ---------------------------------------------------------------------------
// Owner grouping (KAIROS-T-0416) — pure, host-tested
// ---------------------------------------------------------------------------

/// How the board groups its cards into lanes. One grouping at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grouping {
    /// The lanes of the board level: Support and Planned on a delivery
    /// board, one row on the other levels.
    Level,
    /// One lane for each repository (`?by_repo=1`, delivery boards).
    Repository,
    /// One lane for each owner (`?by_owner=1`, each level but ADR).
    Owner,
}

/// The grouping from the two query values and the board level. When the
/// URL has both, the owner grouping wins (the GUI then removes
/// `by_repo`).
fn grouping_of(by_repo: Option<&str>, by_owner: Option<&str>, board_level: &str) -> Grouping {
    if by_owner == Some("1") && owner_grouping_offered(board_level) {
        Grouping::Owner
    } else if by_repo == Some("1") && board_level == "delivery" {
        Grouping::Repository
    } else {
        Grouping::Level
    }
}

/// The owner grouping is on the delivery, initiative and strategy
/// boards. An ADR has no owner.
fn owner_grouping_offered(board_level: &str) -> bool {
    board_level != "adr"
}

/// The user id of the owner of a card, if any.
fn owner_key(card: &CardModel) -> Option<&str> {
    card.owner.as_ref().map(|owner| owner.user_id.as_str())
}

/// One owner lane: the person (`user_id` `None` for "No owner") and the
/// label of the lane.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct OwnerLane {
    user_id: Option<String>,
    label: String,
    /// The lane of the person who reads the board.
    mine: bool,
}

/// The label of the lane of the cards with no owner.
const NO_OWNER: &str = "No owner";

/// The owner lanes of the cards: the reader (`me`, a user id) first, then
/// the other owners by display name, then "No owner". A person has a lane
/// when the person owns a card on the board; "No owner" is a lane when a
/// card has no owner (KAIROS-T-0463).
fn owner_lanes(columns: &[ColumnModel], me: Option<&str>) -> Vec<OwnerLane> {
    let mut owners: Vec<&CardOwner> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut unowned = false;
    for card in columns.iter().flat_map(|column| column.cards.iter()) {
        let Some(owner) = card.owner.as_ref() else {
            unowned = true;
            continue;
        };
        // KAIROS-T-0616: a set, not a scan of the owners per card.
        if seen.insert(owner.user_id.as_str()) {
            owners.push(owner);
        }
    }
    let mut lanes: Vec<OwnerLane> = owners
        .into_iter()
        .map(|owner| OwnerLane {
            mine: me == Some(owner.user_id.as_str()),
            user_id: Some(owner.user_id.clone()),
            label: owner.display_name.clone(),
        })
        .collect();
    lanes.sort_by(|a, b| {
        b.mine
            .cmp(&a.mine)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.user_id.cmp(&b.user_id))
    });
    if unowned {
        lanes.push(OwnerLane {
            user_id: None,
            label: NO_OWNER.to_string(),
            mine: false,
        });
    }
    lanes
}

/// One owner lane (KAIROS-T-0416): the columns narrowed to the cards of
/// one owner. Read-only: a drop from the lane of a different owner does
/// nothing; a drop in a different column transitions as before.
#[component]
fn OwnerLaneSection(
    lane: OwnerLane,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_drop: Callback<CardDrop>,
) -> impl IntoView {
    let group = GroupLane::Owner(lane.user_id.clone());
    let count_group = group.clone();
    let count = Memo::new(move |_| {
        columns.with(|columns| {
            columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .filter(|card| count_group.admits(card))
                .count()
        })
    });
    let attr = lane.user_id.clone().unwrap_or_default();
    let name = lane.label.clone();
    let color = if lane.user_id.is_some() {
        token::OK
    } else {
        token::MUTED
    };
    let id = lane_id(None, &group);
    let collapsed = lane_collapsed(id.clone());
    let id_attr = id.clone();
    let group = StoredValue::new(group);
    view! {
        <section
            class="kairos-board__lane kairos-board__lane--owner"
            class:kairos-board__lane--empty=move || count.get() == 0
            class:kairos-board__lane--collapsed=collapsed
            data-lane-id=id_attr
            data-owner-lane=attr
            data-owner-name=name
            data-owner-mine=lane.mine.to_string()
        >
            <header class="kairos-board__lane-head">
                <Group gap="xs">
                    <LaneToggle id=id label=lane.label.clone()/>
                    <Pill color=color>{lane.label}</Pill>
                    <Text dimmed=true size="xs">{move || count.get().to_string()}</Text>
                </Group>
            </header>
            {move || if collapsed.get() {
                view! { <LaneSummary lane=None group=group.get_value() columns/> }.into_any()
            } else {
                view! {
                    <LaneColumns lane=None group=group.get_value() columns drag powers on_drop/>
                }.into_any()
            }}
        </section>
    }
}

/// One repository lane on a delivery board (KAIROS-T-0109): the columns
/// narrowed to tasks bound to `repo` (`None` = the unbound remainder).
/// Drops here transition only — no lane axis to re-lane into.
#[component]
fn RepoLaneSection(
    repo: Option<String>,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_drop: Callback<CardDrop>,
) -> impl IntoView {
    let lane = match &repo {
        Some(slug) => GroupLane::Slug(slug.clone()),
        None => GroupLane::Unbound,
    };
    let count_lane = lane.clone();
    let count = Memo::new(move |_| {
        columns.with(|columns| {
            columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .filter(|card| count_lane.admits(card))
                .count()
        })
    });
    let label = repo.clone().unwrap_or_else(|| "No repository".to_string());
    let attr = repo.clone().unwrap_or_default();
    let id = lane_id(None, &lane);
    let collapsed = lane_collapsed(id.clone());
    let id_attr = id.clone();
    let lane = StoredValue::new(lane);
    view! {
        <section
            class="kairos-board__lane kairos-board__lane--repo"
            class:kairos-board__lane--empty=move || count.get() == 0
            class:kairos-board__lane--collapsed=collapsed
            data-lane-id=id_attr
            data-repo-lane=attr
        >
            <header class="kairos-board__lane-head">
                <Group gap="xs">
                    <LaneToggle id=id label=label.clone()/>
                    <Pill color=token::ICE>{label}</Pill>
                    <Text dimmed=true size="xs">{move || count.get().to_string()}</Text>
                </Group>
            </header>
            {move || if collapsed.get() {
                view! { <LaneSummary lane=None group=lane.get_value() columns/> }.into_any()
            } else {
                view! {
                    <LaneColumns lane=None group=lane.get_value() columns drag powers on_drop/>
                }.into_any()
            }}
        </section>
    }
}

/// Whether the lane `id` is collapsed (KAIROS-T-0491). Without a
/// [`LaneCollapse`] in the context, no lane is collapsed.
fn lane_collapsed(id: String) -> Memo<bool> {
    let collapse = use_context::<LaneCollapse>();
    Memo::new(move |_| collapse.is_some_and(|c| c.is_collapsed(&id)))
}

/// The collapse toggle of a lane head (KAIROS-T-0491): a button with
/// `aria-expanded`, so a keyboard and a screen reader can work it.
#[component]
fn LaneToggle(id: String, #[prop(into)] label: String) -> impl IntoView {
    let collapse = use_context::<LaneCollapse>();
    let collapsed = lane_collapsed(id.clone());
    let id = StoredValue::new(id);
    let label = StoredValue::new(label);
    view! {
        <button
            type="button"
            class="kairos-board__lane-toggle"
            data-testid="lane-toggle"
            aria-expanded=move || (!collapsed.get()).to_string()
            aria-label=move || {
                let verb = if collapsed.get() { "Expand" } else { "Collapse" };
                format!("{verb} the lane {}", label.get_value())
            }
            on:click=move |_| {
                if let Some(collapse) = collapse {
                    collapse.toggle(&id.get_value());
                }
            }
        >
            <span aria-hidden="true">{move || if collapsed.get() { "▸" } else { "▾" }}</span>
        </button>
    }
}

/// A collapsed lane (KAIROS-T-0491): one row with the card count of the
/// lane in each column.
#[component]
fn LaneSummary(
    lane: Option<&'static str>,
    group: GroupLane,
    columns: Memo<Vec<ColumnModel>>,
) -> impl IntoView {
    let counts = Memo::new(move |_| columns.with(|columns| lane_counts(columns, lane, &group)));
    view! {
        <div class="kairos-board__summary" data-testid="lane-summary">
            {move || counts.get().into_iter().map(|(name, count)| { let column = name.clone(); view! {
                <div class="kairos-board__summary-cell" data-column=column>
                    <Text dimmed=true size="xs">{name}</Text>
                    <span class="kairos-board__summary-count" data-testid="lane-summary-count">
                        {count.to_string()}
                    </span>
                </div>
            }}).collect_view()}
        </div>
    }
}

/// The column row (KAIROS-T-0040/T-0074 fine-grained rendering), filtered
/// to one lane when `lane` is `Some` (KAIROS-T-0077). Drop zones are
/// (column, lane) pairs decided by [`drop_effect`]; the column node
/// persists across refetches and only its cards diff.
#[component]
fn LaneColumns(
    lane: Option<&'static str>,
    /// Repository narrowing for the group-by-repo view (KAIROS-T-0109),
    /// or owner narrowing for the group-by-owner view (KAIROS-T-0416).
    group: GroupLane,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_drop: Callback<CardDrop>,
) -> impl IntoView {
    let group = StoredValue::new(group);
    // KAIROS-T-0582: this lane shows the WIP chips when it is the lane of
    // [`WipLane`], or when there is no such lane.
    let wip_lane = use_context::<WipLane>();
    let this_lane = group.with_value(|g| lane_id(lane, g));
    let show_wip = Memo::new(move |_| {
        wip_lane.is_none_or(|w| {
            w.0.with(|id| id.as_deref().is_none_or(|id| id == this_lane))
        })
    });
    view! {
        <div class="kairos-board">
            <For
                each=move || columns.get()
                key=|column| column.key.clone()
                children=move |column: ColumnModel| {
                    let ColumnModel { id, name, is_done, targets, .. } = column;
                    // KAIROS-T-0616: the live WIP chip of this column, read
                    // from the model so the column node persists when only
                    // the count changes. KAIROS-T-0582: one lane only.
                    let wip_column_id = id.clone();
                    let wip = Memo::new(move |_| {
                        if !show_wip.get() {
                            return None;
                        }
                        columns.with(|columns| {
                            columns
                                .iter()
                                .find(|c| c.id == wip_column_id)
                                .and_then(|c| c.wip.clone())
                        })
                    });
                    // This column's live card list — this lane's slice of
                    // it: the column node itself persists across
                    // refetches; only its cards diff.
                    let cards_column_id = id.clone();
                    let cards = Memo::new(move |_| {
                        columns.with(|columns| {
                            columns
                                .iter()
                                .find(|c| c.id == cards_column_id)
                                .map(|c| {
                                    c.cards
                                        .iter()
                                        .filter(|card| group.with_value(|g| lane_admits(card, lane, g)))
                                        .cloned()
                                        .collect::<Vec<CardModel>>()
                                })
                                .unwrap_or_default()
                        })
                    });
                    // KAIROS-T-0582: a done column shows its last cards,
                    // until "Show all". KAIROS-T-0649: "Show all" is of the
                    // board: the board then reads each done card.
                    let show_all = use_context::<DoneAll>()
                        .map_or_else(|| RwSignal::new(false), |all| all.0);
                    // The cards of the column that the board did not read.
                    let cut_column_id = id.clone();
                    let cut = Memo::new(move |_| {
                        columns.with(|columns| {
                            columns
                                .iter()
                                .find(|c| c.id == cut_column_id)
                                .map_or(0, |c| {
                                    usize::try_from(c.item_count)
                                        .unwrap_or(0)
                                        .saturating_sub(c.cards.len())
                                })
                        })
                    });
                    let shown = Memo::new(move |_| {
                        let all = show_all.get();
                        cards.with(|cards| visible_cards(cards, is_done, all))
                    });
                    // KAIROS-T-0582: during a drag, a column that the board
                    // does not allow says why.
                    let note_column_id = id.clone();
                    let refusal = Memo::new(move |_| {
                        drag.with(|d| {
                            d.as_ref().and_then(|d| {
                                let source = columns.with(|columns| {
                                    columns
                                        .iter()
                                        .find(|c| c.id == d.source_column)
                                        .map(|c| c.name.clone())
                                })?;
                                refusal_note(d, &note_column_id, &source)
                            })
                        })
                    });
                    // Per-handler copies of this column's id (drop target).
                    let class_id = id.clone();
                    let over_id = id.clone();
                    let drop_id = id.clone();
                    let targets_for_cards = StoredValue::new(targets);
                    let column_for_cards = StoredValue::new(id);
                    view! {
                        <section
                            class="kairos-board__column"
                            class:kairos-board__column--droppable=move || {
                                drag.with(|d| {
                                    d.as_ref()
                                        .is_some_and(|d| group.with_value(|g| grouped_drop_effect(d, &class_id, lane, g)).is_some())
                                })
                            }
                            on:dragover=move |ev: web_sys::DragEvent| {
                                // preventDefault marks this zone as a valid
                                // drop target — only when the drop would
                                // actually do something legal.
                                let legal = drag.with_untracked(|d| {
                                    d.as_ref()
                                        .is_some_and(|d| group.with_value(|g| grouped_drop_effect(d, &over_id, lane, g)).is_some())
                                });
                                if legal {
                                    ev.prevent_default();
                                }
                            }
                            on:drop=move |ev: web_sys::DragEvent| {
                                ev.prevent_default();
                                let Some(data) = drag.get_untracked() else { return };
                                drag.set(None);
                                let Some(effect) =
                                    group.with_value(|g| grouped_drop_effect(&data, &drop_id, lane, g))
                                else {
                                    return;
                                };
                                on_drop.run(CardDrop {
                                    kind: data.kind,
                                    short_code: data.short_code,
                                    effect,
                                });
                            }
                        >
                            <header class="kairos-board__column-head">
                                <Group gap="xs">
                                    <Text bright=true bold=true size="sm">{name}</Text>
                                    <Text dimmed=true size="xs">{move || cards.with(Vec::len).to_string()}</Text>
                                    // KAIROS-T-0371: the live count against
                                    // the WIP limit, marked when over it.
                                    {move || wip.get().map(|(label, over)| {
                                        let title = if over {
                                            "The column holds more items than its WIP limit."
                                        } else {
                                            "The live items of the column and its WIP limit."
                                        };
                                        view! {
                                            <span class="kairos-board__wip"
                                                class:kairos-board__wip--over=over
                                                data-testid="wip" title=title>
                                                <Pill color=if over { token::BAD } else { token::MUTED }>
                                                    {if over { format!("{label} over") } else { label }}
                                                </Pill>
                                            </span>
                                        }
                                    })}
                                </Group>
                                {move || refusal.get().map(|note| view! {
                                    <span class="kairos-board__refusal" data-testid="no-transition">
                                        <Text dimmed=true size="xs">{note}</Text>
                                    </span>
                                })}
                            </header>
                            // KAIROS-T-0328: the cards of a column scroll on
                            // their own; the column head stays in view.
                            <div class="kairos-board__cards">
                            <Stack gap="xs">
                                {move || ((lane != Some(LANE_SUPPORT)) && cards.with(Vec::is_empty)).then(|| view! {
                                    <Text dimmed=true size="xs">"No items in this column."</Text>
                                })}
                                {move || {
                                    let cut = cut.get();
                                    let total = cards.with(Vec::len) + cut;
                                    let hidden = shown.with(|s| total - s.len());
                                    show_all_label(is_done, show_all.get(), hidden, total).map(|label| view! {
                                        <Button
                                            variant="subtle"
                                            size="xs"
                                            on_click=Callback::new(move |_| show_all.update(|on| *on = !*on))
                                            attr:data-testid="show-all-done"
                                        >
                                            {label}
                                        </Button>
                                    })
                                }}
                                <For
                                    each=move || shown.get()
                                    key=|card| card.key.clone()
                                    children=move |card: CardModel| {
                                        let CardModel {
                                            kind, short_code, title, meta, work_class,
                                            repository, claim, state, owner, teams, progress, blocks, key: _,
                                        } = card;
                                        let owner = owner.map(|owner| owner.user_id);
                                        view! {
                                            <ItemCard
                                                kind short_code title meta work_class repository
                                                claim state owner teams progress blocks
                                                targets=targets_for_cards.get_value()
                                                source_column=column_for_cards.get_value()
                                                drag powers
                                            />
                                        }
                                    }
                                />
                            </Stack>
                            </div>
                        </section>
                    }
                }
            />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// The text of the claim chip of a card (KAIROS-T-0359): the name of the
/// person, and "(agent)" when the agent of the person made the claim.
fn claim_label(claim: &data::TaskClaim) -> String {
    if claim.agent {
        format!("{} (agent)", claim.display_name)
    } else {
        claim.display_name.clone()
    }
}

/// The tooltip of the claim chip: who has the claim, and since when.
fn claim_title(claim: &data::TaskClaim) -> String {
    let since = claim.claimed_at.get(..10).unwrap_or(&claim.claimed_at);
    format!("{} has the claim since {since}.", claim_label(claim))
}

/// The line of the state on a card (KAIROS-T-0506): the branch and the
/// next step, when they have a value. `None` when neither has one.
fn state_label(state: &data::TaskState) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(branch) = &state.branch {
        parts.push(format!("branch {branch}"));
    }
    if let Some(next) = &state.next_step {
        parts.push(format!("Next: {next}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The tooltip of the state line: who wrote it, and when.
fn state_title(state: &data::TaskState) -> String {
    let day = state.updated_at.get(..10).unwrap_or(&state.updated_at);
    let pushed = if state.pushed { "pushed" } else { "not pushed" };
    format!(
        "State ({pushed}), updated {day} by {}.",
        state.updated_by_name
    )
}

/// One board card: short code (the detail link, KAIROS-T-0076) with its
/// copy-link button, plain-text title, type, key metadata, and
/// drag-and-drop between columns (KAIROS-T-0064 — draggable only when the
/// board allows moves from here). The keyboard-accessible transition path
/// is the item detail page's move control (KAIROS-T-0075).
#[component]
fn ItemCard(
    kind: EntityKind,
    short_code: String,
    title: String,
    /// `(label, color-token)` pills — per-type key metadata.
    meta: Vec<(String, &'static str)>,
    /// `tasks.work_class` — `Some` for tasks; carried into the drag so
    /// lane drops are decidable (KAIROS-T-0077).
    work_class: Option<String>,
    /// The bound repository's slug (KAIROS-T-0109) — a chip on the card.
    repository: Option<String>,
    /// The person who has the task in Active (KAIROS-T-0359) — a chip
    /// with the name of the person.
    claim: Option<data::TaskClaim>,
    /// The live state of a task (KAIROS-T-0506) — a line with the branch
    /// and the next step.
    state: Option<data::TaskState>,
    /// The user id of the owner of the card (KAIROS-T-0416), carried into
    /// the drag so an owner lane can refuse a drop.
    owner: Option<String>,
    /// The teams of a strategy or an initiative (KAIROS-T-0322) — one pill
    /// each; none for an item with no team.
    teams: Vec<String>,
    /// Children rollup badge (KAIROS-T-0080) — renders only when `Some`.
    progress: Option<data::ProgressCounts>,
    /// Blocked-by/blocks badges (KAIROS-T-0091) — render only when
    /// nonzero; click-through to the item's graph. Neutral accents only —
    /// red stays reserved.
    blocks: Option<data::BlocksCounts>,
    /// `(column_id, column_name)` — the valid targets from this column.
    targets: Vec<(String, String)>,
    /// The column this card currently sits in (the drag's source).
    source_column: String,
    /// The board's in-flight drag; this card writes itself here on
    /// dragstart so legal drop zones light up and accept the drop.
    drag: RwSignal<Option<DragData>>,
    /// The user's powers on this board (KAIROS-T-0072): no transition
    /// power → no drag.
    powers: Signal<BoardPowers>,
) -> impl IntoView {
    let href = format!("/items/{short_code}");
    let code_text = short_code.clone();
    let graph_code = short_code.clone();
    let code_for_copy = short_code.clone();
    let code_for_drag = short_code.clone();
    let code_for_class = short_code;
    // A card is draggable when a column move is offered OR it carries a
    // lane (KAIROS-T-0077: a task in a dead-end column can still move to
    // the other lane of the same column).
    let has_moves = !targets.is_empty() || work_class.is_some();
    let target_ids: Vec<String> = targets.iter().map(|(id, _)| id.clone()).collect();
    let drag_work_class = work_class;
    // Memoized (KAIROS-T-0074): notifies only when the decision actually
    // flips, so refetches can't needlessly rebuild the card's attributes.
    let movable = Memo::new(move |_| has_moves && powers.get().transition);
    view! {
        <article
            class="kairos-card"
            class:kairos-card--dragging=move || {
                drag.with(|d| d.as_ref().is_some_and(|d| d.short_code == code_for_class))
            }
            draggable=move || if movable.get() { "true" } else { "false" }
            on:dragstart=move |ev: web_sys::DragEvent| {
                if !(has_moves && powers.get_untracked().transition) {
                    return;
                }
                // dataTransfer content is required for some engines to
                // start a drag at all; the real payload is the signal.
                if let Some(dt) = ev.data_transfer() {
                    let _ = dt.set_data("text/plain", &code_for_drag);
                    dt.set_effect_allowed("move");
                }
                drag.set(Some(DragData {
                    kind,
                    short_code: code_for_drag.clone(),
                    targets: target_ids.clone(),
                    source_column: source_column.clone(),
                    work_class: drag_work_class.clone(),
                    owner: owner.clone(),
                }));
            }
            on:dragend=move |_| drag.set(None)
        >
            <Group justify="between" wrap=true>
                <Group gap="xs">
                    // The identifier IS the detail link (KAIROS-T-0076);
                    // `cl-mono` keeps the code scrapeable and mono-set.
                    <a class="cl-mono kairos-card__code" href=href>
                        {code_text}
                    </a>
                    <copy_link::CopyLinkButton code=code_for_copy/>
                </Group>
                <Group gap="xs" wrap=true>
                    {repository.map(|slug| {
                        let attr = slug.clone();
                        view! {
                            <span class="kairos-card__repo" data-repo=attr>
                                <Pill color=token::ICE>{slug}</Pill>
                            </span>
                        }
                    })}
                    {claim.map(|claim| {
                        let label = claim_label(&claim);
                        let tip = claim_title(&claim);
                        let attr = claim.display_name;
                        view! {
                            <span class="kairos-card__claim" data-claim=attr title=tip>
                                <Pill color=token::OK>{label}</Pill>
                            </span>
                        }
                    })}
                    {teams.into_iter().map(|slug| {
                        let attr = slug.clone();
                        view! {
                            <span class="kairos-card__team" data-team=attr>
                                <Pill color=token::TEAL>{slug}</Pill>
                            </span>
                        }
                    }).collect_view()}
                    <Pill color=kind_color(kind)>{kind.label()}</Pill>
                </Group>
            </Group>
            <span class="kairos-card__title">{title}</span>
            {state.as_ref().and_then(|s| state_label(s).map(|label| (label, s))).map(|(label, s)| {
                let tip = state_title(s);
                view! {
                    <span class="kairos-card__state" data-testid="card-state" title=tip>
                        <Text size="xs" dimmed=true>{label}</Text>
                    </span>
                }
            })}
            {(!meta.is_empty()).then(|| view! {
                <Group gap="xs" wrap=true>
                    {meta.into_iter().map(|(label, color)| view! {
                        <Pill color=color>{label}</Pill>
                    }).collect_view()}
                </Group>
            })}
            {progress.map(|p| {
                // KAIROS-T-0080: the N-of-M micro-badge. Without done
                // semantics on the children's boards, composition only —
                // never a misleading fraction.
                let percent = if p.has_done && p.total > 0 {
                    (p.done * 100 / p.total).clamp(0, 100)
                } else {
                    0
                };
                let label = p.label();
                view! {
                    <div class="kairos-card__progress" title="direct children">
                        {p.has_done.then(|| view! {
                            <span class="kairos-progress__bar">
                                <span
                                    class="kairos-progress__fill"
                                    style=format!("width: {percent}%")
                                ></span>
                            </span>
                        })}
                        <Text mono=true dimmed=true size="xs">{label}</Text>
                    </div>
                }
            })}
            {blocks.map(|counts| {
                // KAIROS-T-0091: dependency badges — Jira-style counts,
                // neutral accents (red stays reserved), linking to the
                // item's graph where the web is visible.
                let graph_href = format!("/items/{}?view=graph", graph_code);
                view! {
                    <Group gap="xs" wrap=true>
                        {(counts.blocked_by > 0).then(|| {
                            let href = graph_href.clone();
                            view! {
                                <a class="kairos-card__blocks" href=href title="open the graph">
                                    <Pill color=token::GOLD>
                                        {format!("blocked by {}", counts.blocked_by)}
                                    </Pill>
                                </a>
                            }
                        })}
                        {(counts.blocks > 0).then(|| {
                            let href = graph_href.clone();
                            view! {
                                <a class="kairos-card__blocks" href=href title="open the graph">
                                    <Pill color=token::ICE>
                                        {format!("blocks {}", counts.blocks)}
                                    </Pill>
                                </a>
                            }
                        })}
                    </Group>
                }
            })}
        </article>
    }
}

// ---------------------------------------------------------------------------
// Create flows
// ---------------------------------------------------------------------------

/// The global create flow (KAIROS-T-0062): the board level's entity type
/// with its type-appropriate fields (A-0002 one item family per board
/// level). New items ALWAYS land in the board's entry column — creation
/// is intake; movement happens by transition.
///
/// One dialog, two modes (COLLIERY-T-0232). [`CreateMode::Manager`] is the
/// dialog as it was. [`CreateMode::Request`] is for a member who does not
/// manage the board: the title names the team, a caption says where the
/// request goes, and the Lane control is not there, because a request
/// always goes to the support lane. A second dialog would repeat the
/// title, the content, the task type, the repository picker and the
/// display of a refusal, and the two copies would then go different ways.
/// What each mode sends is [`create_body`].
#[component]
fn CreateItemModal(
    open: RwSignal<bool>,
    kind: EntityKind,
    /// Who creates: a manager of the board, or a person who sends a
    /// request to its team (COLLIERY-T-0232).
    mode: CreateMode,
    board_id: String,
    /// Delivery boards carry their team; new tasks inherit it.
    team_id: Option<String>,
    /// The name of that team, for the text of a request. `None` when the
    /// list of teams did not load.
    team_name: Option<String>,
    /// `(column_id, column_name)` — the board's entry column.
    entry: (String, String),
    on_changed: Callback<()>,
    /// A request was sent: the board shows its notice (COLLIERY-T-0232).
    on_requested: Callback<SentRequest>,
) -> impl IntoView {
    let auth = use_auth();
    let title = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let hypothesis = RwSignal::new(String::new());
    let complexity = RwSignal::new("none".to_string());
    let bucket_type = RwSignal::new(NO_BUCKET.to_string());
    let task_type = RwSignal::new("task".to_string());
    // KAIROS-T-0077: "auto" omits the field — the server defaults the
    // lane (support type → Support lane, else Planned).
    let work_class = RwSignal::new("auto".to_string());
    let decision_maker = RwSignal::new(String::new());
    let decision_date = RwSignal::new(String::new());
    // KAIROS-T-0124 #6b: the repository the task links to. "(none)" omits
    // the field. The item page keeps its own picker to change it later.
    let repository = RwSignal::new(NO_REPOSITORY.to_string());
    // KAIROS-T-0580: the parent, in the transaction of the create.
    let parent = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    // Every live repository of the tenant, loaded when the modal opens on
    // a task board (COLLIERY-T-0221). A task on the board of any team may
    // link to any repository (COLLIERY-A-0023), so the list does not stop
    // at the repositories of the team of the board. The control shows when
    // the tenant has one repository or more.
    let repos_team = StoredValue::new(team_id.clone());
    let repos = LocalResource::new(move || {
        auth.track_session();
        let wanted = open.get() && matches!(kind, EntityKind::Task);
        async move {
            if wanted {
                repositories::api::list_repositories(auth, None).await
            } else {
                Ok(Vec::new())
            }
        }
    });
    let repo_options = Memo::new(move |_| {
        let list = repos
            .get()
            .and_then(|result| result.ok())
            .unwrap_or_default();
        let offered =
            repos_team.with_value(|team| repositories::picker_options(&list, team.as_deref()));
        (!offered.is_empty()).then(|| {
            std::iter::once((NO_REPOSITORY.to_string(), NO_REPOSITORY.to_string()))
                .chain(offered)
                .collect::<Vec<(String, String)>>()
        })
    });

    // A fresh form every time the modal opens.
    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            content.set(String::new());
            hypothesis.set(String::new());
            complexity.set("none".to_string());
            bucket_type.set(NO_BUCKET.to_string());
            task_type.set("task".to_string());
            work_class.set("auto".to_string());
            decision_maker.set(String::new());
            decision_date.set(String::new());
            repository.set(NO_REPOSITORY.to_string());
            parent.set(String::new());
            busy.set(false);
            error.set(None);
        }
    });

    let (entry_id, entry_name) = entry;
    let is_request = mode == CreateMode::Request;

    // `Callback` is `Copy`: the modal's children (a `Fn` closure) can use
    // it without moving anything out of their environment.
    let submit: Callback<()> = Callback::new(move |()| {
        let form = CreateForm {
            title: title.get_untracked(),
            content: content.get_untracked(),
            hypothesis: hypothesis.get_untracked(),
            complexity: complexity.get_untracked(),
            bucket_type: bucket_type.get_untracked(),
            task_type: task_type.get_untracked(),
            work_class: work_class.get_untracked(),
            repository: repository.get_untracked(),
            decision_maker: decision_maker.get_untracked(),
            decision_date: decision_date.get_untracked(),
            parent: parent.get_untracked(),
        };
        // COLLIERY-T-0232: the mode decides the body, not the form.
        let (column_id, item) = create_body(mode, &form, &entry_id, team_id.as_deref());
        let board_id = board_id.clone();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match data::create_item(auth, kind, &board_id, &column_id, &item).await {
                Ok(short_code) => {
                    open.set(false);
                    if is_request {
                        on_requested.run(SentRequest {
                            short_code,
                            repository: item.repository,
                        });
                    }
                    on_changed.run(());
                }
                // A refusal of the server (403, 422) and a network
                // failure show in the dialog, which stays open.
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    });

    let entry_note = StoredValue::new(match mode {
        CreateMode::Manager => format!(
            "New {}s start in {entry_name} — the board's intake column.",
            kind.label()
        ),
        CreateMode::Request => request_caption(team_name.as_deref(), &entry_name),
    });
    let modal_title = match mode {
        CreateMode::Manager => format!("New {}", kind.label()),
        CreateMode::Request => request_title(team_name.as_deref()),
    };
    let (refused_title, submit_label, busy_label) = match mode {
        CreateMode::Manager => ("Could not create", "Create", "Creating…"),
        CreateMode::Request => ("Could not send the request", "Send request", "Sending…"),
    };
    view! {
        <Modal open=open title=modal_title>
            <Stack gap="sm">
                <span data-testid="create-caption">
                    <Text dimmed=true size="xs">
                        {move || entry_note.get_value()}
                    </Text>
                </span>
                <TextInput label="Title" value=title placeholder="What is it?"/>
                <Textarea label="Content (markdown)" value=content rows=5
                    placeholder="Why does it exist? What does done look like?"/>
                {matches!(kind, EntityKind::Strategy).then(|| view! {
                    <TextInput label="Hypothesis (optional)" value=hypothesis
                        placeholder="If we…, then…"/>
                })}
                {matches!(kind, EntityKind::Initiative).then(|| view! {
                    <Select label="Complexity" value=complexity
                        options=vec!["none".into(), "xs".into(), "s".into(),
                                     "m".into(), "l".into(), "xl".into()]/>
                    // KAIROS-T-0587: a bucket initiative holds work of one
                    // kind, and it is always active.
                    <Select label="Bucket" value=bucket_type
                        options={BUCKET_OPTIONS.map(String::from).to_vec()}
                        description="A bucket holds small work of one kind. It is always \
                                     active. Select none for a normal initiative."
                        attr:data-testid="create-bucket"/>
                })}
                {matches!(kind, EntityKind::Task).then(|| view! {
                    <Select label="Task type" value=task_type
                        options=vec!["task".into(), "bug".into(), "tech_debt".into(),
                                     "support".into()]/>
                    // KAIROS-T-0077: the Planned/Support lane; "auto"
                    // follows the type (support → Support lane).
                    // COLLIERY-T-0232: a request has no lane to select.
                    {(!is_request).then(|| view! {
                        <Select label="Lane" value=work_class
                            options=vec!["auto".into(), "planned".into(), "support".into()]/>
                    })}
                    // COLLIERY-T-0221: only when the tenant has a repository.
                    // The value is the slug and the label says more: the
                    // Aurora `Select` with `option_pairs`.
                    {move || repo_options.get().map(|options| view! {
                        <Select
                            label="Repository"
                            option_pairs=options
                            value=repository
                            attr:data-testid="create-repository"
                        />
                    })}
                })}
                // KAIROS-T-0580: a strategy parents an initiative, an
                // initiative parents a task.
                {match kind {
                    EntityKind::Initiative => Some(&["strategy"][..]),
                    EntityKind::Task => Some(&["initiative"][..]),
                    _ => None,
                }.map(|types| view! {
                    <div data-testid="create-parent">
                        <CodePicker label="Parent (optional)" value=parent types/>
                    </div>
                })}
                {matches!(kind, EntityKind::Adr).then(|| view! {
                    <Stack gap="sm">
                        <TextInput label="Decision maker (optional)" value=decision_maker/>
                        <TextInput label="Decision date (optional)" value=decision_date
                            placeholder="YYYY-MM-DD"/>
                    </Stack>
                })}
                {move || error.get().map(|e| view! {
                    <Alert title=refused_title color=token::BAD>
                        <Text size="sm">{describe(&e)}</Text>
                    </Alert>
                })}
                <Group justify="between">
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| open.set(false))>
                        "Cancel"
                    </Button>
                    {move || {
                        let disabled = busy.get() || title.get().trim().is_empty();
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { busy_label } else { submit_label }}
                            </Button>
                        }
                    }}
                </Group>
            </Stack>
        </Modal>
    }
}

/// "New document" (board header): template picker + parent picker.
/// Documents are off-board, so this is the one create flow that is not
/// column-anchored; the server writes the `supports` edge to the parent.
/// The owner board of the document is this board (COLLIERY-T-3109: each
/// document names its owner board, and the code gets its prefix).
#[component]
fn CreateDocumentModal(
    open: RwSignal<bool>,
    /// The slug of this board: the owner board of the new document.
    #[prop(into)]
    board_slug: String,
    /// The name of this board, for the text of the dialog.
    #[prop(into)]
    board_name: String,
    /// `(short_code, title)` of this board's eligible parents — LIVE
    /// (KAIROS-T-0074): the modal instance persists across refetches, so
    /// the option list must follow the board's current items.
    parents: Memo<Vec<(String, String)>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let board_slug = StoredValue::new(board_slug);
    let board_name = StoredValue::new(board_name);
    let title = RwSignal::new(String::new());
    let template = RwSignal::new("(blank)".to_string());
    let parent = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    // Templates load lazily, first time the modal opens.
    let templates = LocalResource::new(move || {
        auth.track_session();
        let wanted = open.get();
        async move {
            if wanted {
                data::list_templates(auth).await
            } else {
                Ok(Vec::new())
            }
        }
    });

    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            template.set("(blank)".to_string());
            parent.set(
                parents
                    .get_untracked()
                    .first()
                    .map(|(code, _)| code.clone())
                    .unwrap_or_default(),
            );
            busy.set(false);
            error.set(None);
        }
    });

    let submit: Callback<()> = Callback::new(move |()| {
        let template_name = template.get_untracked();
        let template_id = templates.get_untracked().and_then(|result| {
            result.ok().and_then(|templates| {
                templates
                    .into_iter()
                    .find(|t| t.name == template_name)
                    .map(|t| t.id)
            })
        });
        // Options render as "CODE · title"; the code is the first token.
        let parent_code = parent
            .get_untracked()
            .split(" · ")
            .next()
            .unwrap_or_default()
            .to_string();
        let doc_title = title.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let owner = board_slug.get_value();
            match data::create_document(
                auth,
                &doc_title,
                template_id.as_deref(),
                &owner,
                &parent_code,
            )
            .await
            {
                Ok(()) => {
                    open.set(false);
                    on_changed.run(());
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    });

    view! {
        <Modal open=open title="New document">
            <Stack gap="sm">
                <TextInput label="Title" value=title placeholder="e.g. PRD: …"/>
                {move || match templates.get() {
                    None => view! { <Text dimmed=true size="xs">"Loading templates…"</Text> }.into_any(),
                    Some(Err(e)) => view! {
                        <Alert title="Templates unavailable" color=token::GOLD>
                            <Text size="sm">{describe(&e)}</Text>
                        </Alert>
                    }.into_any(),
                    Some(Ok(list)) => {
                        let mut options = vec!["(blank)".to_string()];
                        options.extend(list.into_iter().map(|t| t.name));
                        view! { <Select label="Template" value=template options/> }.into_any()
                    }
                }}
                {move || {
                    let options: Vec<String> = parents
                        .get()
                        .iter()
                        .map(|(code, item_title)| format!("{code} · {item_title}"))
                        .collect();
                    view! { <Select label="Attach to (supports)" value=parent options/> }
                }}
                <Text size="xs" dimmed=true attr:data-testid="create-document-owner">
                    {format!(
                        "The owner board is {}. It gives the right to edit the document, and \
                         the code of the document gets the prefix of this board.",
                        board_name.get_value()
                    )}
                </Text>
                {move || error.get().map(|e| view! {
                    <Alert title="Could not create" color=token::BAD>
                        <Text size="sm">{describe(&e)}</Text>
                    </Alert>
                })}
                <Group justify="between">
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| open.set(false))>
                        "Cancel"
                    </Button>
                    {move || {
                        let disabled = busy.get() || title.get().trim().is_empty();
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Creating…" } else { "Create" }}
                            </Button>
                        }
                    }}
                </Group>
            </Stack>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::teams::api::Team;

    /// KAIROS-T-0363: the question of "Archive completed" gives the
    /// count.
    #[test]
    fn the_archive_completed_title_gives_the_count() {
        assert_eq!(
            archive_completed_title(1, None),
            "Archive 1 completed task?"
        );
        assert_eq!(
            archive_completed_title(12, None),
            "Archive 12 completed tasks?"
        );
        assert_eq!(
            archive_completed_title(12, Some(30)),
            "Archive the completed tasks that entered their column 30 days ago or before?"
        );
        assert_eq!(older_than_days(""), Ok(None));
        assert_eq!(older_than_days(" 7 "), Ok(Some(7)));
        for bad in ["0", "-1", "x", "36501"] {
            assert!(older_than_days(bad).is_err(), "{bad}");
        }
    }

    /// KAIROS-T-0359: the claim chip names the person, and the agent.
    #[test]
    fn the_claim_chip_names_the_person() {
        let claim = data::TaskClaim {
            user_id: "u-alice".to_string(),
            display_name: "Alice".to_string(),
            agent: true,
            claimed_at: "2026-10-09T10:00:00Z".to_string(),
        };
        assert_eq!(claim_label(&claim), "Alice (agent)");
        assert_eq!(
            claim_title(&claim),
            "Alice (agent) has the claim since 2026-10-09."
        );
        let person = data::TaskClaim {
            agent: false,
            ..claim
        };
        assert_eq!(claim_label(&person), "Alice");
    }

    fn board(id: &str, level: &str, team_id: Option<&str>) -> data::Board {
        data::Board {
            id: id.to_string(),
            name: id.to_string(),
            slug: id.to_string(),
            board_level: level.to_string(),
            team_id: team_id.map(str::to_string),
        }
    }

    fn team(id: &str, slug: &str) -> Team {
        Team {
            id: id.to_string(),
            name: slug.to_string(),
            slug: slug.to_string(),
            team_type: "platform".to_string(),
            delivery_board_id: None,
            site_enabled: true,
        }
    }

    /// The headings of one band, in order.
    fn headings(band: &BandModel) -> Vec<Option<&str>> {
        band.groups
            .iter()
            .map(|(h, _)| h.as_ref().map(|(name, _)| name.as_str()))
            .collect()
    }

    /// Bands come out in flight-level order, the delivery band grouped by
    /// team. Every delivery board has a team (COLLIERY-T-0230), so the
    /// delivery band has the teams and nothing more.
    #[test]
    fn band_models_orders_levels_and_groups_delivery_by_team() {
        let boards = vec![
            board("adr-board", "adr", None),
            board("web-delivery", "delivery", Some("t2")),
            board("main-strategy", "strategy", None),
            board("platform-delivery", "delivery", Some("t1")),
            board("initiatives", "initiative", None),
        ];
        let teams = vec![team("t1", "platform"), team("t2", "web")];
        let bands = band_models(boards, &teams);

        let levels: Vec<&str> = bands.iter().map(|b| b.level.as_str()).collect();
        assert_eq!(levels, vec!["strategy", "initiative", "delivery", "adr"]);

        let delivery = &bands[2];
        assert_eq!(headings(delivery), vec![Some("platform"), Some("web")]);
        let platform_group = &delivery.groups[0];
        assert_eq!(
            platform_group.0.as_ref().map(|(_, href)| href.as_str()),
            Some("/teams/platform")
        );
        assert_eq!(platform_group.1[0].slug, "platform-delivery");
    }

    /// KAIROS-T-0319: a band is one grid. Its tiles keep the order of the
    /// groups, and each tile has the team of its group and its link.
    #[test]
    fn band_tiles_puts_the_boards_of_a_band_in_one_list_with_their_team() {
        let boards = vec![
            board("web-delivery", "delivery", Some("t2")),
            board("platform-delivery", "delivery", Some("t1")),
            board("orphan-delivery", "delivery", None),
        ];
        let teams = vec![team("t1", "platform"), team("t2", "web")];
        let mut bands = band_models(boards, &teams);
        let tiles = band_tiles(bands.remove(0).groups);
        let got: Vec<(&str, Option<&str>, Option<&str>)> = tiles
            .iter()
            .map(|(team, board)| {
                (
                    board.slug.as_str(),
                    team.as_ref().map(|(name, _)| name.as_str()),
                    team.as_ref().map(|(_, href)| href.as_str()),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "platform-delivery",
                    Some("platform"),
                    Some("/teams/platform")
                ),
                ("web-delivery", Some("web"), Some("/teams/web")),
                ("orphan-delivery", Some("Needs a team"), Some("")),
            ]
        );
    }

    /// COLLIERY-T-0230: the board list has no group "No team", for any
    /// board that the data can hold.
    #[test]
    fn band_models_has_no_group_no_team() {
        let boards = vec![
            board("main-strategy", "strategy", None),
            board("initiatives", "initiative", None),
            board("adr-board", "adr", None),
            board("platform-delivery", "delivery", Some("t1")),
            board("stale-delivery", "delivery", Some("gone")),
            board("orphan-delivery", "delivery", None),
            board("strange", "portfolio", None),
        ];
        let bands = band_models(boards, &[team("t1", "platform")]);
        for band in &bands {
            for heading in headings(band) {
                assert_ne!(heading, Some("No team"), "band {}", band.level);
            }
        }
    }

    /// COLLIERY-T-0230: a board of the organization has no delivery team.
    /// Its group says that it belongs to the organization. The heading is
    /// text only: there is no team page to go to.
    #[test]
    fn band_models_puts_the_boards_of_the_organization_under_organization() {
        let boards = vec![
            board("main-strategy", "strategy", None),
            board("initiatives", "initiative", None),
            board("adr-board", "adr", None),
            board("platform-delivery", "delivery", Some("t1")),
        ];
        let bands = band_models(boards, &[team("t1", "platform")]);
        for band in bands.iter().filter(|b| b.level != "delivery") {
            assert_eq!(headings(band), vec![Some("Organization")], "{}", band.level);
            assert_eq!(
                band.groups[0].0.as_ref().map(|(_, href)| href.as_str()),
                Some(""),
                "{}",
                band.level
            );
            assert_eq!(band.groups[0].1.len(), 1);
        }
        let delivery = bands.iter().find(|b| b.level == "delivery").expect("band");
        assert_eq!(headings(delivery), vec![Some("platform")]);
    }

    /// COLLIERY-T-3102: the Decisions band has a group for each team that
    /// has an ADR board, then the ADR boards of the organization.
    #[test]
    fn band_models_groups_the_adr_boards_by_team() {
        let boards = vec![
            board("org-adrs", "adr", None),
            board("skadi-adrs", "adr", Some("t1")),
            board("skadi", "delivery", Some("t1")),
        ];
        let bands = band_models(boards, &[team("t1", "skadi")]);
        let adr = bands.iter().find(|b| b.level == "adr").expect("band");
        assert_eq!(headings(adr), vec![Some("skadi"), Some("Organization")]);
        assert_eq!(
            adr.groups[0].0.as_ref().map(|(_, href)| href.as_str()),
            Some("/teams/skadi")
        );
        assert_eq!(adr.groups[0].1[0].slug, "skadi-adrs");
        assert_eq!(adr.groups[1].1[0].slug, "org-adrs");
    }

    /// COLLIERY-T-0230: a delivery board with no team is not created any
    /// more, but old data can hold one. It renders, after the teams, under
    /// a heading that says what it needs.
    #[test]
    fn band_models_keeps_an_old_delivery_board_with_no_team_reachable() {
        let boards = vec![
            board("orphan-delivery", "delivery", None),
            board("platform-delivery", "delivery", Some("t1")),
        ];
        let bands = band_models(boards, &[team("t1", "platform")]);
        assert_eq!(bands.len(), 1);
        assert_eq!(
            headings(&bands[0]),
            vec![Some("platform"), Some("Needs a team")]
        );
        let (heading, orphans) = &bands[0].groups[1];
        assert_eq!(heading.as_ref().map(|(_, href)| href.as_str()), Some(""));
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].slug, "orphan-delivery");
    }

    fn me(role: &str, team_ids: &[&str], grants: &[(&str, &[&str])]) -> crate::api::Whoami {
        serde_json::from_value(serde_json::json!({
            "user": {"id": "u-1", "display_name": "u", "email": "u@x.test"},
            "organization": {"slug": "demo", "role": role},
            "teams": team_ids.iter().map(|id| serde_json::json!({
                "id": id, "slug": id, "name": id
            })).collect::<Vec<_>>(),
            "capabilities": grants.iter().map(|(slug, caps)| serde_json::json!({
                "board_slug": slug, "grants": caps
            })).collect::<Vec<_>>(),
        }))
        .expect("test whoami")
    }

    /// KAIROS-T-0072 client mirror: team membership implies the delivery
    /// set on the team's board — and only there, and only that set.
    #[test]
    fn board_powers_mirror_team_implication() {
        let bob = me("member", &["t1"], &[]);
        // On the team's delivery board: transition + create tasks + docs.
        let on_team = board_powers(
            &bob,
            "platform-delivery",
            Some("t1"),
            Some(EntityKind::Task),
        );
        assert!(on_team.transition && on_team.create && on_team.documents);
        // A team-owned STRATEGY board (KAIROS-T-0440): the team also gets
        // manage_strategies there, so create too.
        let strat = board_powers(&bob, "s", Some("t1"), Some(EntityKind::Strategy));
        assert!(strat.transition && strat.documents && strat.create);
        // Someone else's board: nothing.
        let other = board_powers(&bob, "web-delivery", Some("t2"), Some(EntityKind::Task));
        assert_eq!(other, BoardPowers::default());
        // Org-wide (teamless) board: nothing.
        let orgwide = board_powers(&bob, "strategy", None, Some(EntityKind::Strategy));
        assert_eq!(orgwide, BoardPowers::default());
    }

    /// KAIROS-T-0570 client mirror: team membership does not give `purge`.
    /// A grant of `purge` or of `*` gives it, and an organization admin
    /// has it.
    #[test]
    fn purge_is_not_implied_by_the_team() {
        let bob = me("member", &["t1"], &[]);
        assert!(holds_capability(
            &bob,
            Some("d"),
            Some("t1"),
            "manage_tasks"
        ));
        assert!(!holds_capability(&bob, Some("d"), Some("t1"), "purge"));
        assert!(!holds_capability(&bob, None, None, "purge"));
        let manager = me("member", &["t1"], &[("d", &["manage_*"])]);
        assert!(!holds_capability(&manager, Some("d"), Some("t1"), "purge"));
        let granted = me("member", &[], &[("d", &["purge"])]);
        assert!(holds_capability(&granted, Some("d"), Some("t1"), "purge"));
        let full = me("member", &[], &[("d", &["*"])]);
        assert!(holds_capability(&full, Some("d"), None, "purge"));
        let admin = me("admin", &[], &[]);
        assert!(holds_capability(&admin, Some("d"), None, "purge"));
    }

    /// KAIROS-T-0410 client mirror: on a strategy or initiative board, the
    /// `implied` list of the server (membership: the `member` grant,
    /// KAIROS-T-0541)
    /// gives the move and the create of the board's items. It does not give
    /// Restore ([`holds_capability`] reads grants only), nor the members
    /// panel.
    #[test]
    fn board_powers_read_the_implied_capabilities_of_board_membership() {
        let member: crate::api::Whoami = serde_json::from_value(serde_json::json!({
            "user": {"id": "u-1", "display_name": "u", "email": "u@x.test"},
            "organization": {"slug": "demo", "role": "member"},
            "teams": [],
            "capabilities": [{
                "board_slug": "initiatives",
                "grants": ["member"],
                "implied": ["manage_initiatives", "transition_items"],
            }],
        }))
        .expect("test whoami");
        let powers = board_powers(&member, "initiatives", None, Some(EntityKind::Initiative));
        assert!(powers.transition && powers.create);
        assert!(!powers.documents);
        // Restore is an archive: a grant is necessary.
        assert!(!holds_capability(
            &member,
            Some("initiatives"),
            None,
            "manage_initiatives"
        ));
        // Another board: nothing.
        let other = board_powers(&member, "strategy", None, Some(EntityKind::Strategy));
        assert!(!other.transition && !other.create);
        // An older server sends no `implied`: the grants alone decide.
        let old = me("member", &[], &[("initiatives", &["member"])]);
        let powers = board_powers(&old, "initiatives", None, Some(EntityKind::Initiative));
        assert!(!powers.transition && !powers.create);
    }

    /// KAIROS-T-0412: the board page links to its members panel for an
    /// org admin and for a holder of `administer_members` or
    /// `configure_boards` (globs included) on THAT board. A member of the
    /// team, or a holder of other grants, does not see the link.
    #[test]
    fn the_members_link_is_for_who_may_manage_the_members() {
        let admin = me("admin", &[], &[]);
        assert!(board_powers(&admin, "initiatives", None, Some(EntityKind::Initiative)).members);

        let grants = me("member", &[], &[("initiatives", &["administer_members"])]);
        assert!(board_powers(&grants, "initiatives", None, Some(EntityKind::Initiative)).members);
        assert!(!board_powers(&grants, "kairos", Some("t1"), Some(EntityKind::Task)).members);

        let configurer = me("member", &[], &[("kairos", &["configure_*"])]);
        assert!(board_powers(&configurer, "kairos", Some("t1"), Some(EntityKind::Task)).members);

        let team = me("member", &["t1"], &[("kairos", &["manage_*"])]);
        assert!(!board_powers(&team, "kairos", Some("t1"), Some(EntityKind::Task)).members);

        assert_eq!(members_panel_href("b-1"), "/admin/boards/b-1#members");
    }

    /// COLLIERY-T-3108 client mirror: on the ADR board of the team, a member
    /// may create ADRs with no grant. A member of a different team may not,
    /// and the ADR board of the organization (no team) still needs a grant.
    #[test]
    fn board_powers_mirror_team_adr_board() {
        let alice = me("member", &["skadi"], &[]);
        let own = board_powers(&alice, "skadi-adrs", Some("skadi"), Some(EntityKind::Adr));
        assert!(own.create && own.transition && own.documents);
        assert!(holds_capability(
            &alice,
            Some("skadi-adrs"),
            Some("skadi"),
            "manage_adrs"
        ));
        assert!(may_edit_item(
            &alice,
            "someone-else",
            Some("skadi-adrs"),
            Some("skadi"),
            "manage_adrs"
        ));

        let bob = me("member", &["crt"], &[]);
        let other = board_powers(&bob, "skadi-adrs", Some("skadi"), Some(EntityKind::Adr));
        assert_eq!(other, BoardPowers::default());
        assert!(!holds_capability(
            &bob,
            Some("skadi-adrs"),
            Some("skadi"),
            "manage_adrs"
        ));

        let orgwide = board_powers(&alice, "adrs", None, Some(EntityKind::Adr));
        assert!(!orgwide.create);
        // An off-board ADR: a team gives no `manage_adrs` there.
        assert!(!holds_capability(&alice, None, None, "manage_adrs"));
    }

    /// Explicit grants (incl. globs) and the admin bypass keep working.
    #[test]
    fn board_powers_mirror_grants_and_admin() {
        let admin = me("admin", &[], &[]);
        let p = board_powers(&admin, "any", None, Some(EntityKind::Adr));
        assert!(p.transition && p.create && p.documents);

        let granted = me("member", &[], &[("adrs", &["transition_*", "manage_adrs"])]);
        let p = board_powers(&granted, "adrs", None, Some(EntityKind::Adr));
        assert!(p.transition && p.create && !p.documents);
        // Grants are board-scoped: elsewhere they mean nothing.
        let elsewhere = board_powers(&granted, "strategy", None, Some(EntityKind::Strategy));
        assert_eq!(elsewhere, BoardPowers::default());

        assert!(grant_covers("*", "manage_tasks"));
        assert!(grant_covers("manage_*", "manage_tasks"));
        assert!(!grant_covers("manage_*", "transition_items"));
        assert!(!grant_covers("manage_tasks", "manage_taskss"));
    }

    /// KAIROS-T-0164: the Restore affordance asks per ITEM, and an item
    /// may sit on no board — so the mirror narrows to a known board and
    /// widens (never to "nobody") when there is none.
    #[test]
    fn holds_capability_answers_per_item() {
        let admin = me("admin", &[], &[]);
        assert!(holds_capability(&admin, Some("any"), None, "manage_tasks"));
        assert!(holds_capability(&admin, None, None, "manage_documents"));

        // A grant is board-scoped when the board is known…
        let granted = me("member", &[], &[("adrs", &["manage_adrs"])]);
        assert!(holds_capability(
            &granted,
            Some("adrs"),
            None,
            "manage_adrs"
        ));
        assert!(!holds_capability(
            &granted,
            Some("strategy"),
            None,
            "manage_adrs"
        ));
        // …and counts anywhere when it is not (an off-board item resolves
        // its authorization board through a parent the client cannot see).
        assert!(holds_capability(&granted, None, None, "manage_adrs"));
        assert!(!holds_capability(&granted, None, None, "manage_strategies"));

        // Team implication: on the team's own board, and — for an
        // off-board item — for anyone on a team at all.
        let bob = me("member", &["t1"], &[]);
        assert!(holds_capability(
            &bob,
            Some("platform-delivery"),
            Some("t1"),
            "manage_tasks"
        ));
        assert!(!holds_capability(
            &bob,
            Some("web-delivery"),
            Some("t2"),
            "manage_tasks"
        ));
        assert!(holds_capability(&bob, None, None, "manage_documents"));
        // Never implied, board or not: strategies are not in the team set.
        assert!(!holds_capability(&bob, None, None, "manage_strategies"));

        let nobody = me("member", &[], &[]);
        assert!(!holds_capability(&nobody, None, None, "manage_documents"));
    }

    /// COLLIERY-T-0228: the creator of an item may edit it with no
    /// capability, and a user who did not create it needs the capability.
    #[test]
    fn may_edit_item_mirrors_the_edit_rule() {
        let nobody = me("member", &[], &[]);
        let mine = nobody.user.id.clone();
        assert!(!mine.is_empty(), "the fixture gives the user an id");
        // The creator: no team, no grant, no role.
        assert!(may_edit_item(
            &nobody,
            &mine,
            Some("platform-delivery"),
            Some("t1"),
            "manage_tasks"
        ));
        // An item with no board (a document, an off-board ADR).
        assert!(may_edit_item(
            &nobody,
            &mine,
            None,
            None,
            "manage_documents"
        ));
        // A different creator, and no capability: no edit.
        assert!(!may_edit_item(
            &nobody,
            "someone-else",
            Some("platform-delivery"),
            Some("t1"),
            "manage_tasks"
        ));
        // An item that carries no creator gives no right.
        assert!(!may_edit_item(
            &nobody,
            "",
            Some("platform-delivery"),
            Some("t1"),
            "manage_tasks"
        ));
        // The capability and the admin role are as they were.
        let bob = me("member", &["t1"], &[]);
        assert!(may_edit_item(
            &bob,
            "someone-else",
            Some("platform-delivery"),
            Some("t1"),
            "manage_tasks"
        ));
        assert!(!may_edit_item(
            &bob,
            "someone-else",
            Some("web-delivery"),
            Some("t2"),
            "manage_tasks"
        ));
        let admin = me("admin", &[], &[]);
        assert!(may_edit_item(
            &admin,
            "someone-else",
            Some("any"),
            None,
            "manage_strategies"
        ));
    }

    /// COLLIERY-T-0228: creation grants no movement. The powers that gate
    /// the move controls do not take a creator, so they cannot give one a
    /// right: a user with no team and no grant moves nothing.
    #[test]
    fn the_creator_gets_no_power_to_move() {
        let nobody = me("member", &[], &[]);
        let powers = board_powers(
            &nobody,
            "platform-delivery",
            Some("t1"),
            Some(EntityKind::Task),
        );
        assert!(!powers.transition, "no column move, no lane change");
        assert!(!powers.create, "the source half of a board move");
        assert!(
            movable_delivery_boards(
                &nobody,
                &[board("platform-delivery", "delivery", Some("t1"))],
                "web-delivery"
            )
            .is_empty(),
            "no target for a board move"
        );
    }

    // ---- COLLIERY-T-0232: the gate for "New task" and "New request" ------

    /// A `whoami` as the server sends it to a member of the tenant: the
    /// computed `file_backlog` is in `implicit`.
    fn member(role: &str, team_ids: &[&str], grants: &[(&str, &[&str])]) -> crate::api::Whoami {
        let mut who = me(role, team_ids, grants);
        who.implicit = vec!["file_backlog".to_string()];
        who
    }

    fn platform_board() -> data::Board {
        board("platform-delivery", "delivery", Some("t-platform"))
    }

    /// The four persons who manage the delivery board of platform. Each
    /// gets "New task", and none gets "New request".
    #[test]
    fn a_person_who_manages_the_board_gets_new_task_and_no_request() {
        let board = platform_board();
        let managers = [
            (
                "a person with every grant on the board",
                member("member", &[], &[("platform-delivery", &["*"])]),
            ),
            (
                "a member of the team of the board",
                member("member", &["t-platform"], &[]),
            ),
            (
                "a person with an explicit manage_tasks grant",
                member("member", &[], &[("platform-delivery", &["manage_tasks"])]),
            ),
            ("an organization admin", member("admin", &[], &[])),
        ];
        for (who, me) in &managers {
            assert!(manages_board(Some(me), &board), "{who} manages");
            assert!(!may_request(Some(me), &board, true), "{who}: no request");
            assert_eq!(
                create_mode(Some(me), &board, true),
                Some(CreateMode::Manager),
                "{who}"
            );
        }
    }

    /// A member of the tenant with nothing on the board of platform: a
    /// request, and no "New task". A member of a DIFFERENT team and a
    /// person with a grant on a DIFFERENT board are in the same position.
    #[test]
    fn a_member_who_does_not_manage_a_delivery_board_may_request() {
        let board = platform_board();
        let outsiders = [
            ("no team, no grant", member("member", &[], &[])),
            ("a member of team web", member("member", &["t-web"], &[])),
            (
                "a grant on the board of web",
                member("member", &[], &[("web-delivery", &["manage_tasks"])]),
            ),
            (
                "a grant on this board that does not cover manage_tasks",
                member(
                    "member",
                    &[],
                    &[("platform-delivery", &["transition_items"])],
                ),
            ),
        ];
        for (who, me) in &outsiders {
            assert!(!manages_board(Some(me), &board), "{who} does not manage");
            assert!(may_request(Some(me), &board, true), "{who} may request");
            assert_eq!(
                create_mode(Some(me), &board, true),
                Some(CreateMode::Request),
                "{who}"
            );
        }
    }

    /// A board of the organization takes no request: `file_backlog` is a
    /// capability on a delivery board only. The same person sees no button
    /// on the initiative, strategy and ADR boards.
    #[test]
    fn a_board_that_is_not_a_delivery_board_offers_neither() {
        let carol = member("member", &["t-web"], &[]);
        for level in ["initiative", "strategy", "adr", "unknown-level"] {
            let board = board(level, level, None);
            assert!(!manages_board(Some(&carol), &board), "{level}");
            assert!(!may_request(Some(&carol), &board, true), "{level}");
            assert_eq!(create_mode(Some(&carol), &board, true), None, "{level}");
        }
        // A person who manages such a board keeps the button of a manager.
        let granted = member("member", &[], &[("adr", &["manage_adrs"])]);
        let adrs = board("adr", "adr", None);
        assert_eq!(
            create_mode(Some(&granted), &adrs, true),
            Some(CreateMode::Manager)
        );
        assert!(!may_request(Some(&granted), &adrs, true));
    }

    /// A delivery board with no entry column offers neither button: a new
    /// task has no column to start in. This is also true for a manager, as
    /// it was before COLLIERY-T-0232.
    #[test]
    fn a_delivery_board_with_no_entry_column_offers_neither() {
        let board = platform_board();
        let carol = member("member", &["t-web"], &[]);
        assert!(!may_request(Some(&carol), &board, false));
        assert_eq!(create_mode(Some(&carol), &board, false), None);
        let bob = member("member", &["t-platform"], &[]);
        assert!(manages_board(Some(&bob), &board), "bob manages the board");
        assert_eq!(create_mode(Some(&bob), &board, false), None);
    }

    /// Until `whoami` resolves the GUI does not know the person, and it
    /// offers nothing.
    #[test]
    fn a_person_whose_whoami_did_not_resolve_gets_neither() {
        let board = platform_board();
        assert!(!manages_board(None, &board));
        assert!(!may_request(None, &board, true));
        assert_eq!(create_mode(None, &board, true), None);
    }

    /// The GUI reads `file_backlog` from `whoami`. A server that does not
    /// give the capability stops the request button, and the button of a
    /// manager does not depend on it.
    #[test]
    fn the_request_needs_file_backlog_from_whoami() {
        let board = platform_board();
        let without = me("member", &["t-web"], &[]);
        assert!(without.implicit.is_empty());
        assert!(!may_request(Some(&without), &board, true));
        assert_eq!(create_mode(Some(&without), &board, true), None);
        let mut other = without.clone();
        other.implicit = vec!["some_other_capability".to_string()];
        assert!(!may_request(Some(&other), &board, true));
        let bob = me("member", &["t-platform"], &[]);
        assert_eq!(
            create_mode(Some(&bob), &board, true),
            Some(CreateMode::Manager)
        );
    }

    // ---- COLLIERY-T-0232: what the dialog sends ---------------------------

    fn form(task_type: &str, work_class: &str, repository: &str) -> CreateForm {
        CreateForm {
            title: "Export endpoint".to_string(),
            content: "The portal needs it.".to_string(),
            hypothesis: "If we ship it".to_string(),
            complexity: "m".to_string(),
            bucket_type: "bug".to_string(),
            task_type: task_type.to_string(),
            work_class: work_class.to_string(),
            repository: repository.to_string(),
            decision_maker: "alice".to_string(),
            decision_date: "2026-09-27".to_string(),
            parent: String::new(),
        }
    }

    /// A request never asks for the planned lane and never names a column
    /// other than the entry column, for each state that the form can have
    /// (and for some that it cannot).
    #[test]
    fn a_request_is_always_support_in_the_entry_column() {
        for task_type in ["task", "bug", "tech_debt", "support"] {
            for lane in ["auto", "planned", "support", "", "PLANNED", "c-todo"] {
                for repository in [NO_REPOSITORY, "payments-api"] {
                    let (column, item) = create_body(
                        CreateMode::Request,
                        &form(task_type, lane, repository),
                        "c-backlog",
                        Some("t-platform"),
                    );
                    let case = format!("{task_type}/{lane}/{repository}");
                    assert_eq!(column, "c-backlog", "{case}");
                    assert_eq!(item.work_class.as_deref(), Some("support"), "{case}");
                    assert_eq!(item.task_type.as_deref(), Some(task_type), "{case}");
                }
            }
        }
    }

    /// The whole body of a request, with a repository and with none.
    #[test]
    fn a_request_sends_the_task_the_team_and_the_link() {
        let (column, item) = create_body(
            CreateMode::Request,
            &form("bug", "planned", "payments-api"),
            "c-backlog",
            Some("t-platform"),
        );
        assert_eq!(column, "c-backlog");
        assert_eq!(
            item,
            data::NewItem {
                title: "Export endpoint".to_string(),
                content: "The portal needs it.".to_string(),
                hypothesis: None,
                complexity: None,
                bucket_type: None,
                task_type: Some("bug".to_string()),
                work_class: Some("support".to_string()),
                team_id: Some("t-platform".to_string()),
                repository: Some("payments-api".to_string()),
                decision_maker: None,
                decision_date: None,
                parent: None,
            }
        );
        let (_, unlinked) = create_body(
            CreateMode::Request,
            &form("task", "auto", NO_REPOSITORY),
            "c-backlog",
            Some("t-platform"),
        );
        assert_eq!(unlinked.repository, None, "a repository is optional");
    }

    /// A manager sends what the dialog sent before COLLIERY-T-0232: the
    /// lane of the form, no `work_class` for `auto`, and the entry column.
    #[test]
    fn a_manager_sends_the_lane_of_the_form() {
        let send = |lane: &str| {
            create_body(
                CreateMode::Manager,
                &form("task", lane, NO_REPOSITORY),
                "c-backlog",
                Some("t-platform"),
            )
        };
        let (column, auto) = send("auto");
        assert_eq!(column, "c-backlog");
        assert_eq!(auto.work_class, None);
        assert_eq!(send("planned").1.work_class.as_deref(), Some("planned"));
        assert_eq!(send("support").1.work_class.as_deref(), Some("support"));
        assert_eq!(
            auto,
            data::NewItem {
                title: "Export endpoint".to_string(),
                content: "The portal needs it.".to_string(),
                hypothesis: Some("If we ship it".to_string()),
                complexity: Some("m".to_string()),
                bucket_type: Some("bug".to_string()),
                task_type: Some("task".to_string()),
                work_class: None,
                team_id: Some("t-platform".to_string()),
                repository: None,
                decision_maker: Some("alice".to_string()),
                decision_date: Some("2026-09-27".to_string()),
                parent: None,
            }
        );
        // Empty optional fields are left out, and `none` is no complexity.
        let blank = CreateForm {
            title: "T".to_string(),
            complexity: "none".to_string(),
            task_type: "task".to_string(),
            work_class: "auto".to_string(),
            repository: NO_REPOSITORY.to_string(),
            hypothesis: "  ".to_string(),
            ..CreateForm::default()
        };
        let (_, item) = create_body(CreateMode::Manager, &blank, "c-backlog", None);
        assert_eq!(item.hypothesis, None);
        assert_eq!(item.complexity, None);
        assert_eq!(item.bucket_type, None);
        assert_eq!(item.decision_maker, None);
        assert_eq!(item.decision_date, None);
        assert_eq!(item.team_id, None);
        assert_eq!(item.parent, None);
    }

    /// KAIROS-T-0580: the Parent field sends the trimmed code, in both
    /// modes; an empty field sends no parent.
    #[test]
    fn the_parent_field_sends_the_code_or_nothing() {
        for mode in [CreateMode::Manager, CreateMode::Request] {
            let mut child = form("task", "auto", NO_REPOSITORY);
            child.parent = " DEMO-I-0001 ".to_string();
            let (_, item) = create_body(mode, &child, "c-backlog", None);
            assert_eq!(item.parent.as_deref(), Some("DEMO-I-0001"));
            child.parent = "   ".to_string();
            let (_, item) = create_body(mode, &child, "c-backlog", None);
            assert_eq!(item.parent, None);
        }
    }

    /// KAIROS-T-0587: the Bucket select sends its kind, and `none` sends no
    /// bucket. A request never makes a bucket.
    #[test]
    fn the_bucket_select_sends_its_kind_or_nothing() {
        for kind in ["tech_debt", "bug", "ad_hoc"] {
            let mut bucket = form("task", "auto", NO_REPOSITORY);
            bucket.bucket_type = kind.to_string();
            let (_, item) = create_body(CreateMode::Manager, &bucket, "c-discovery", None);
            assert_eq!(item.bucket_type.as_deref(), Some(kind));
            let (_, request) = create_body(CreateMode::Request, &bucket, "c-discovery", None);
            assert_eq!(request.bucket_type, None);
        }
        let mut plain = form("task", "auto", NO_REPOSITORY);
        plain.bucket_type = NO_BUCKET.to_string();
        let (_, item) = create_body(CreateMode::Manager, &plain, "c-discovery", None);
        assert_eq!(item.bucket_type, None);
        assert_eq!(&BUCKET_OPTIONS[1..], &["tech_debt", "bug", "ad_hoc"]);
    }

    // ---- COLLIERY-T-0232: the text of a request ---------------------------

    #[test]
    fn the_request_dialog_names_the_team_the_column_and_the_lane() {
        assert_eq!(request_title(Some("Platform")), "Request to Platform");
        assert_eq!(request_title(None), "Request to the team of this board");
        assert_eq!(
            request_caption(Some("Platform"), "Backlog"),
            "The request goes to Backlog, the entry column of the board of Platform, \
             in the Support lane. Platform moves it from there."
        );
        assert_eq!(
            request_caption(None, "Inbox"),
            "The request goes to Inbox, the entry column of this board, \
             in the Support lane. The team of the board moves it from there."
        );
    }

    #[test]
    fn the_notice_names_the_team_the_column_and_the_lane() {
        assert_eq!(
            request_notice(Some("Platform"), "Backlog", false),
            (
                "Request sent to Platform: ".to_string(),
                ". It is in Backlog, in the Support lane.".to_string()
            )
        );
        assert_eq!(
            request_notice(None, "Backlog", true),
            (
                "Request sent to the team of this board: ".to_string(),
                ". It is in Backlog, in the Support lane. \
                 The repository filter hides it on this page."
                    .to_string()
            )
        );
        assert_eq!(lane_label(LANE_SUPPORT), "Support");
        assert_eq!(lane_label(LANE_PLANNED), "Planned");
    }

    /// The lens hides a request as it hides each task: with a selection,
    /// only the tasks of the selected repositories show.
    #[test]
    fn the_lens_hides_a_request_that_is_not_in_the_selection() {
        let request = |repository: Option<&str>| SentRequest {
            short_code: "DEMO-T-0042".to_string(),
            repository: repository.map(str::to_string),
        };
        let none: Vec<String> = Vec::new();
        assert!(!lens_hides_request(&request(None), &none));
        assert!(!lens_hides_request(&request(Some("payments-api")), &none));
        let selected = slugs(&["payments-api"]);
        assert!(!lens_hides_request(
            &request(Some("payments-api")),
            &selected
        ));
        assert!(lens_hides_request(&request(Some("portal-web")), &selected));
        assert!(lens_hides_request(&request(None), &selected));
        // The rule is the rule of the lens for a task card.
        let mut as_card = card(EntityKind::Task, Some("portal-web"));
        assert_eq!(
            lens_hides_request(&request(Some("portal-web")), &selected),
            !lens_admits(&as_card, &selected)
        );
        as_card.repository = None;
        assert_eq!(
            lens_hides_request(&request(None), &selected),
            !lens_admits(&as_card, &selected)
        );
    }

    /// KAIROS-I-0012: the move picker offers the OTHER delivery boards
    /// the caller may manage tasks on — never this one, never a
    /// non-delivery board, and (for a non-admin) never a board whose
    /// `manage_tasks` the server would refuse.
    #[test]
    fn movable_delivery_boards_are_the_other_manageable_ones() {
        let boards = vec![
            board("platform-delivery", "delivery", Some("t1")),
            board("web-delivery", "delivery", Some("t2")),
            board("data-delivery", "delivery", Some("t3")),
            board("initiatives", "initiative", None),
        ];

        // Bob is on platform only: no target he may manage.
        let bob = me("member", &["t1"], &[]);
        assert!(movable_delivery_boards(&bob, &boards, "platform-delivery").is_empty());

        // A grant on the web board makes it (and only it) a target.
        let dual = me("member", &["t1"], &[("web-delivery", &["manage_tasks"])]);
        assert_eq!(
            movable_delivery_boards(&dual, &boards, "platform-delivery"),
            vec![("web-delivery".to_string(), "web-delivery".to_string())]
        );

        // The org admin sees every OTHER delivery board — the initiative
        // board is never a target (the server's NOT_DELIVERY_BOARD rule).
        let admin = me("admin", &[], &[]);
        let slugs: Vec<String> = movable_delivery_boards(&admin, &boards, "platform-delivery")
            .into_iter()
            .map(|(slug, _)| slug)
            .collect();
        assert_eq!(slugs, vec!["web-delivery", "data-delivery"]);
    }

    /// KAIROS-T-0077: the lane is a pure projection of work_class;
    /// non-task kinds render in the default Planned lane.
    #[test]
    fn card_lane_projects_work_class() {
        assert_eq!(card_lane(Some("support")), LANE_SUPPORT);
        assert_eq!(card_lane(Some("planned")), LANE_PLANNED);
        assert_eq!(card_lane(None), LANE_PLANNED);
    }

    /// KAIROS-T-0077 drop semantics: same-column cross-lane = lane write
    /// only (no transition validation); cross-column same-lane =
    /// transition exactly as before; diagonal = both; column legality
    /// stays with the transitions; unlaned cards cannot lane-move.
    #[test]
    fn drop_effect_decides_column_and_lane_moves() {
        let drag = |targets: &[&str], work_class: Option<&str>| DragData {
            kind: EntityKind::Task,
            short_code: "T-1".into(),
            targets: targets.iter().map(|s| s.to_string()).collect(),
            source_column: "c-src".into(),
            work_class: work_class.map(str::to_string),
            owner: None,
        };
        // Same column, same lane: no-op.
        assert_eq!(
            drop_effect(&drag(&["c-2"], Some("planned")), "c-src", Some("planned")),
            None
        );
        // Same column, other lane: lane write only — legal even from a
        // dead-end column (no targets).
        let effect =
            drop_effect(&drag(&[], Some("planned")), "c-src", Some("support")).expect("legal");
        assert_eq!(effect.transition_to, None);
        assert_eq!(effect.set_work_class.as_deref(), Some("support"));
        // Cross column, same lane: transition exactly as before.
        let effect =
            drop_effect(&drag(&["c-2"], Some("support")), "c-2", Some("support")).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(effect.set_work_class, None);
        // Diagonal: both writes.
        let effect =
            drop_effect(&drag(&["c-2"], Some("planned")), "c-2", Some("support")).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(effect.set_work_class.as_deref(), Some("support"));
        // A column outside the transition targets stays illegal, diagonal
        // or not — lane never affects transition legality.
        assert_eq!(
            drop_effect(&drag(&["c-2"], Some("planned")), "c-3", Some("support")),
            None
        );
        // Unlaned rows (non-delivery boards): pure column semantics.
        let effect = drop_effect(&drag(&["c-2"], None), "c-2", None).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(drop_effect(&drag(&["c-2"], None), "c-src", None), None);
        // A card without a lane cannot lane-move.
        assert_eq!(
            drop_effect(&drag(&[], None), "c-src", Some("support")),
            None
        );
    }

    fn card(kind: EntityKind, repository: Option<&str>) -> CardModel {
        CardModel {
            kind,
            short_code: "X-1".into(),
            title: "x".into(),
            meta: Vec::new(),
            work_class: None,
            repository: repository.map(str::to_string),
            claim: None,
            state: None,
            owner: None,
            teams: Vec::new(),
            progress: None,
            blocks: None,
            key: CardKey {
                short_code: "X-1".into(),
                version: 1,
                column_id: "c".into(),
                extras: 0,
            },
        }
    }

    /// A view of one task `DEMO-T-0001` with the fields of `task`, in the
    /// column `column`.
    fn view_of(task: serde_json::Value, column: &str) -> data::BoardView {
        let mut card = serde_json::json!({
            "short_code": "DEMO-T-0001", "title": "T", "version": 1,
            "task_type": "task", "work_class": "planned"
        });
        if let (Some(card), Some(fields)) = (card.as_object_mut(), task.as_object()) {
            card.extend(fields.clone());
        }
        let group = |id: &str| {
            let tasks = if id == column {
                vec![card.clone()]
            } else {
                Vec::new()
            };
            serde_json::json!({
                "column": {"id": id, "name": id, "position": 0},
                "strategies": [], "initiatives": [], "tasks": tasks, "adrs": []
            })
        };
        data::BoardView {
            detail: serde_json::from_value(serde_json::json!({
                "id": "b", "name": "B", "slug": "b", "board_level": "delivery",
                "columns": [], "transitions": []
            }))
            .expect("detail"),
            items: serde_json::from_value(serde_json::json!({
                "board": {"id": "b", "name": "B", "slug": "b", "board_level": "delivery"},
                "columns": [group("todo"), group("doing")],
                "total": 1
            }))
            .expect("items"),
        }
    }

    /// The key of the one card of a view.
    fn key_of(view: &data::BoardView) -> CardKey {
        board_summary(view)
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .map(|card| card.key.clone())
            .next()
            .expect("one card")
    }

    /// KAIROS-T-0615: the key of a card comes from its version and its
    /// column, and from the fields that change with no new version. The
    /// same card has the same key.
    #[test]
    fn the_key_of_a_card_follows_the_version_the_column_and_the_extras() {
        let base = key_of(&view_of(serde_json::json!({}), "todo"));
        assert_eq!(base.short_code, "DEMO-T-0001");
        assert_eq!(base.version, 1);
        assert_eq!(base.column_id, "todo");
        assert_eq!(
            key_of(&view_of(serde_json::json!({}), "todo")),
            base,
            "stable"
        );
        let new_version = key_of(&view_of(
            serde_json::json!({"version": 2, "title": "U"}),
            "todo",
        ));
        assert_ne!(new_version, base);
        assert_eq!(new_version.extras, base.extras);
        let moved = key_of(&view_of(serde_json::json!({}), "doing"));
        assert_ne!(moved, base);
        // A state write makes no new version: the extras change the key.
        let state = key_of(&view_of(
            serde_json::json!({"state": {"next_step": "Push", "updated_at": "2026-10-17T00:00:00Z"}}),
            "todo",
        ));
        assert_eq!(state.version, base.version);
        assert_ne!(state, base);
        let claim = key_of(&view_of(
            serde_json::json!({"claim": {"user_id": "u", "display_name": "Robin"}}),
            "todo",
        ));
        assert_ne!(claim, base);
        let cancelled = key_of(&view_of(
            serde_json::json!({"cancellation": {"reason": "No need"}}),
            "todo",
        ));
        assert_ne!(cancelled, base);
    }

    /// KAIROS-T-0615: one pass over the model gives the repositories, the
    /// teams, the document parents and the completed count.
    #[test]
    fn the_summary_reads_the_model_once() {
        let mut view = view_of(
            serde_json::json!({"repository": {"id": "r", "slug": "web", "name": "web"}}),
            "todo",
        );
        view.items.item_teams.insert(
            "DEMO-I-0001".to_string(),
            vec![data::ItemTeam {
                slug: "payments".into(),
                name: "Payments".into(),
                from_tasks: true,
                set_by_hand: false,
            }],
        );
        let summary = board_summary(&view);
        assert_eq!(summary.repos, ["web"]);
        assert_eq!(summary.teams, ["payments"]);
        assert_eq!(
            summary.doc_parents,
            [("DEMO-T-0001".to_string(), "T".to_string())]
        );
        assert_eq!(summary.completed, 0);
        assert_eq!(summary.columns.len(), 2);
    }

    fn owned(kind: EntityKind, owner: Option<(&str, &str)>) -> CardModel {
        CardModel {
            owner: owner.map(|(user_id, display_name)| CardOwner {
                user_id: user_id.into(),
                display_name: display_name.into(),
            }),
            ..card(kind, None)
        }
    }

    fn column_of(cards: Vec<CardModel>) -> ColumnModel {
        ColumnModel {
            id: "c".into(),
            name: "c".into(),
            wip: None,
            is_done: false,
            item_count: 0,
            targets: Vec::new(),
            cards,
            key: String::new(),
        }
    }

    /// KAIROS-T-0416: the reader first, then the other owners by name,
    /// then "No owner". One lane for each person, whatever the number of
    /// cards.
    #[test]
    fn owner_lanes_put_the_reader_first_then_names_then_no_owner() {
        let columns = vec![
            column_of(vec![
                owned(EntityKind::Task, Some(("u-sam", "Sam"))),
                owned(EntityKind::Task, None),
                owned(EntityKind::Task, Some(("u-robin", "Robin"))),
            ]),
            column_of(vec![
                owned(EntityKind::Task, Some(("u-jamie", "jamie"))),
                owned(EntityKind::Task, Some(("u-sam", "Sam"))),
            ]),
        ];
        let order = |me: Option<&str>| -> Vec<(Option<String>, String, bool)> {
            owner_lanes(&columns, me)
                .into_iter()
                .map(|lane| (lane.user_id, lane.label, lane.mine))
                .collect()
        };
        assert_eq!(
            order(Some("u-sam")),
            vec![
                (Some("u-sam".into()), "Sam".into(), true),
                (Some("u-jamie".into()), "jamie".into(), false),
                (Some("u-robin".into()), "Robin".into(), false),
                (None, "No owner".into(), false),
            ]
        );
        // A reader with no card: no lane of their own; names in order.
        let names: Vec<String> = owner_lanes(&columns, Some("u-other"))
            .into_iter()
            .map(|lane| lane.label)
            .collect();
        assert_eq!(names, vec!["jamie", "Robin", "Sam", "No owner"]);
        // A board with no owner: only "No owner".
        assert_eq!(
            owner_lanes(&[column_of(vec![card(EntityKind::Task, None)])], None),
            vec![OwnerLane {
                user_id: None,
                label: "No owner".into(),
                mine: false,
            }]
        );
        // KAIROS-T-0463: each card has an owner: no "No owner" lane.
        let all_owned = vec![column_of(vec![
            owned(EntityKind::Task, Some(("u-sam", "Sam"))),
            owned(EntityKind::Task, Some(("u-robin", "Robin"))),
        ])];
        let names: Vec<String> = owner_lanes(&all_owned, None)
            .into_iter()
            .map(|lane| lane.label)
            .collect();
        assert_eq!(names, vec!["Robin", "Sam"]);
        // And no card at all: no lane.
        assert!(owner_lanes(&[column_of(vec![])], None).is_empty());
    }

    /// KAIROS-T-0416: an owner lane admits the cards of its owner; "No
    /// owner" admits the rest (an ADR included).
    #[test]
    fn owner_lane_admits_by_owner() {
        let sam = owned(EntityKind::Initiative, Some(("u-sam", "Sam")));
        let free = owned(EntityKind::Task, None);
        let lane = GroupLane::Owner(Some("u-sam".into()));
        assert!(lane.admits(&sam));
        assert!(!lane.admits(&free));
        assert!(GroupLane::Owner(None).admits(&free));
        assert!(GroupLane::Owner(None).admits(&card(EntityKind::Adr, None)));
        assert!(!GroupLane::Owner(None).admits(&sam));
    }

    /// KAIROS-T-0416: a task moves to the lane of the person who claims
    /// it: the owner comes from the claim of a task, and from the owner of
    /// a strategy or an initiative.
    #[test]
    fn the_owner_comes_from_the_claim_or_the_owner() {
        let claim = data::TaskClaim {
            user_id: "u-robin".into(),
            display_name: "Robin".into(),
            agent: true,
            claimed_at: String::new(),
        };
        assert_eq!(
            CardOwner::of_claim(&claim),
            CardOwner {
                user_id: "u-robin".into(),
                display_name: "Robin".into()
            }
        );
        let owner = data::ItemOwner {
            user_id: "u-sam".into(),
            display_name: "Sam".into(),
        };
        assert_eq!(CardOwner::of_owner(&owner).user_id, "u-sam");
    }

    /// KAIROS-T-0416: one grouping at a time; the owner grouping is on
    /// each level but ADR, the repository grouping on delivery only.
    #[test]
    fn the_grouping_comes_from_the_query_and_the_level() {
        assert_eq!(grouping_of(None, None, "delivery"), Grouping::Level);
        assert_eq!(
            grouping_of(Some("1"), None, "delivery"),
            Grouping::Repository
        );
        assert_eq!(grouping_of(None, Some("1"), "delivery"), Grouping::Owner);
        assert_eq!(
            grouping_of(Some("1"), Some("1"), "delivery"),
            Grouping::Owner
        );
        assert_eq!(grouping_of(None, Some("1"), "initiative"), Grouping::Owner);
        assert_eq!(grouping_of(None, Some("1"), "strategy"), Grouping::Owner);
        assert_eq!(grouping_of(None, Some("1"), "adr"), Grouping::Level);
        assert_eq!(grouping_of(Some("1"), None, "strategy"), Grouping::Level);
        assert_eq!(grouping_of(None, Some("0"), "delivery"), Grouping::Level);
    }

    /// KAIROS-T-0416: a drop into the lane of a different owner does
    /// nothing; in the lane of the card, a drop in a different column
    /// transitions. The repository lanes keep their rule.
    #[test]
    fn an_owner_lane_refuses_the_cards_of_other_owners() {
        let drag = |owner: Option<&str>| DragData {
            kind: EntityKind::Task,
            short_code: "T-1".into(),
            targets: vec!["c-2".into()],
            source_column: "c-src".into(),
            work_class: Some("planned".into()),
            owner: owner.map(str::to_string),
        };
        let sam = GroupLane::Owner(Some("u-sam".into()));
        let nobody = GroupLane::Owner(None);
        // The lane of the card: a move to another column transitions.
        let effect = grouped_drop_effect(&drag(Some("u-sam")), "c-2", None, &sam).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(effect.set_work_class, None);
        let effect = grouped_drop_effect(&drag(None), "c-2", None, &nobody).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        // The same column of the lane: nothing.
        assert_eq!(
            grouped_drop_effect(&drag(Some("u-sam")), "c-src", None, &sam),
            None
        );
        // Another owner lane: nothing, in each column.
        assert_eq!(grouped_drop_effect(&drag(None), "c-2", None, &sam), None);
        assert_eq!(
            grouped_drop_effect(&drag(Some("u-sam")), "c-2", None, &nobody),
            None
        );
        assert_eq!(
            grouped_drop_effect(&drag(Some("u-robin")), "c-src", None, &sam),
            None
        );
        // Repository lanes: as before (a column move transitions).
        assert!(
            grouped_drop_effect(&drag(None), "c-2", None, &GroupLane::Slug("x".into())).is_some()
        );
    }

    fn with_teams(kind: EntityKind, teams: &[&str]) -> CardModel {
        CardModel {
            teams: teams.iter().map(|t| t.to_string()).collect(),
            ..card(kind, None)
        }
    }

    /// KAIROS-T-0322: the team filter narrows the strategies and the
    /// initiatives only; "No team" keeps the items with no team.
    #[test]
    fn the_team_filter_narrows_strategies_and_initiatives_only() {
        let skadi = TeamSelection::Team("skadi".into());
        assert!(team_admits(
            &with_teams(EntityKind::Initiative, &["skadi", "weir"]),
            &skadi
        ));
        assert!(!team_admits(
            &with_teams(EntityKind::Initiative, &["weir"]),
            &skadi
        ));
        assert!(!team_admits(&with_teams(EntityKind::Strategy, &[]), &skadi));
        assert!(team_admits(
            &with_teams(EntityKind::Strategy, &[]),
            &TeamSelection::NoTeam
        ));
        assert!(!team_admits(
            &with_teams(EntityKind::Initiative, &["weir"]),
            &TeamSelection::NoTeam
        ));
        // Tasks and ADRs always stay.
        assert!(team_admits(&card(EntityKind::Task, None), &skadi));
        assert!(team_admits(
            &card(EntityKind::Adr, None),
            &TeamSelection::NoTeam
        ));
        assert!(team_admits(
            &with_teams(EntityKind::Initiative, &[]),
            &TeamSelection::All
        ));
    }

    #[test]
    fn the_team_selection_comes_from_the_query() {
        assert_eq!(team_selection_of(None, None), TeamSelection::All);
        assert_eq!(team_selection_of(Some(""), None), TeamSelection::All);
        assert_eq!(
            team_selection_of(Some("skadi"), None),
            TeamSelection::Team("skadi".into())
        );
        assert_eq!(team_selection_of(None, Some("1")), TeamSelection::NoTeam);
        assert_eq!(
            team_selection_of(Some("skadi"), Some("1")),
            TeamSelection::NoTeam
        );
        assert_eq!(team_selection_of(None, Some("0")), TeamSelection::All);
    }

    fn slugs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// KAIROS-T-0114: `?repo=` parses to a deduplicated slug list and the
    /// effective selection is pruned to what the board can show — a stale
    /// slug is dropped (never a blank board), and the pruned form differs
    /// from the request exactly when a write-back is due.
    #[test]
    fn selected_repos_parse_and_prune_against_the_board() {
        assert!(parse_repo_query(None).is_empty());
        assert!(parse_repo_query(Some("")).is_empty());
        assert_eq!(parse_repo_query(Some(",a,,b,a,")), slugs(&["a", "b"]));

        let known = slugs(&["payments-api", "platform-infra"]);
        let requested = parse_repo_query(Some("platform-infra,gone,payments-api"));
        assert_eq!(
            prune_repos(&requested, &known),
            slugs(&["platform-infra", "payments-api"])
        );
        // Entirely stale → empty selection (the URL is then cleared).
        assert!(prune_repos(&slugs(&["gone"]), &known).is_empty());
        // Nothing on the board → nothing selectable.
        assert!(prune_repos(&requested, &[]).is_empty());
        // A clean request round-trips unchanged (no write-back).
        let clean = slugs(&["payments-api"]);
        assert_eq!(prune_repos(&clean, &known), clean);
    }

    /// KAIROS-T-0109: a repo lane admits exactly its tasks; the unbound
    /// lane takes repository-less cards (tasks and other kinds alike).
    #[test]
    fn repo_lane_admits_by_binding() {
        let bound = card(EntityKind::Task, Some("payments-api"));
        let other = card(EntityKind::Task, Some("platform-infra"));
        let unbound = card(EntityKind::Task, None);
        let adr = card(EntityKind::Adr, None);

        let lane = GroupLane::Slug("payments-api".into());
        assert!(lane.admits(&bound));
        assert!(!lane.admits(&other));
        assert!(!lane.admits(&unbound));
        assert!(!lane.admits(&adr));

        assert!(!GroupLane::Unbound.admits(&bound));
        assert!(GroupLane::Unbound.admits(&unbound));
        assert!(GroupLane::Unbound.admits(&adr));

        assert!(GroupLane::Any.admits(&bound));
        assert!(GroupLane::Any.admits(&adr));
    }

    /// KAIROS-T-0109: the lens retain filter narrows TASKS only — an
    /// empty selection admits everything, and non-task kinds always stay.
    #[test]
    fn lens_filter_narrows_tasks_only() {
        let selected = slugs(&["payments-api"]);
        assert!(lens_admits(
            &card(EntityKind::Task, Some("payments-api")),
            &selected
        ));
        assert!(!lens_admits(
            &card(EntityKind::Task, Some("platform-infra")),
            &selected
        ));
        assert!(!lens_admits(&card(EntityKind::Task, None), &selected));
        assert!(lens_admits(&card(EntityKind::Initiative, None), &selected));
        assert!(lens_admits(&card(EntityKind::Adr, None), &selected));
        assert!(lens_admits(&card(EntityKind::Task, None), &[]));
        assert!(lens_admits(
            &card(EntityKind::Task, Some("platform-infra")),
            &[]
        ));
    }

    /// KAIROS-T-0114: group-by lanes come from the EFFECTIVE selection
    /// (all board repositories when none is selected), in board order,
    /// plus the unbound remainder — never a lane for a chip that is off.
    #[test]
    fn repo_lanes_follow_the_effective_selection() {
        let known = slugs(&["payments-api", "platform-infra"]);
        assert_eq!(
            repo_lanes(&[], &known),
            vec![
                Some("payments-api".to_string()),
                Some("platform-infra".to_string()),
                None
            ]
        );
        assert_eq!(
            repo_lanes(&slugs(&["platform-infra"]), &known),
            vec![Some("platform-infra".to_string()), None]
        );
        // One repository on the board: a single repo lane + unbound.
        assert_eq!(
            repo_lanes(&[], &slugs(&["only"])),
            vec![Some("only".to_string()), None]
        );
        // No repositories at all: just the unbound lane.
        assert_eq!(repo_lanes(&[], &[]), vec![None]);
    }

    /// A board whose team id names an unknown team lands in "Team not
    /// found" (a stale/failed teams read must never hide boards). The
    /// board has a team, so it is not under "Needs a team"
    /// (COLLIERY-T-0230).
    #[test]
    fn band_models_keeps_unknown_team_boards_reachable() {
        let boards = vec![board("d", "delivery", Some("gone"))];
        let bands = band_models(boards, &[]);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].groups.len(), 1);
        assert_eq!(
            bands[0].groups[0].0.as_ref().map(|(name, _)| name.as_str()),
            Some("Team not found")
        );
        assert_eq!(bands[0].groups[0].1.len(), 1);
    }

    /// KAIROS-T-0491: each lane has its own id, and the ids of two groupings
    /// do not meet.
    #[test]
    fn each_lane_has_an_id() {
        assert_eq!(lane_id(Some(LANE_SUPPORT), &GroupLane::Any), "support");
        assert_eq!(lane_id(Some(LANE_PLANNED), &GroupLane::Any), "planned");
        assert_eq!(lane_id(None, &GroupLane::Slug("web".into())), "repo:web");
        assert_eq!(lane_id(None, &GroupLane::Unbound), "repo:");
        assert_eq!(
            lane_id(None, &GroupLane::Owner(Some("u-sam".into()))),
            "owner:u-sam"
        );
        assert_eq!(lane_id(None, &GroupLane::Owner(None)), "owner:");
    }

    /// KAIROS-T-0491: the key names the board and the grouping, so another
    /// board, or another grouping, has its own set.
    #[test]
    fn the_collapse_key_is_per_board_and_grouping() {
        let a = collapse_key("b-1", Grouping::Level);
        assert_eq!(a, "kairos.lanes.collapsed.b-1.level");
        assert_ne!(a, collapse_key("b-2", Grouping::Level));
        assert_ne!(a, collapse_key("b-1", Grouping::Owner));
        assert_ne!(
            collapse_key("b-1", Grouping::Repository),
            collapse_key("b-1", Grouping::Owner)
        );
    }

    /// KAIROS-T-0491: the stored text goes back to the same set; a missing
    /// or a bad value is an empty set.
    #[test]
    fn the_collapsed_set_round_trips() {
        let set: std::collections::BTreeSet<String> = [
            "support".to_string(),
            "repo:".to_string(),
            "owner:u-1".to_string(),
        ]
        .into();
        assert_eq!(parse_collapsed(Some(&serialize_collapsed(&set))), set);
        assert!(parse_collapsed(None).is_empty());
        assert!(parse_collapsed(Some("")).is_empty());
        assert!(parse_collapsed(Some("\n\n")).is_empty());
    }

    /// KAIROS-T-0491: the collapsed row counts the cards that the lane
    /// shows in each column, with the filter of the columns.
    #[test]
    fn the_collapsed_row_counts_the_cards_of_the_lane() {
        let support = |repo: Option<&str>| CardModel {
            work_class: Some(LANE_SUPPORT.into()),
            ..card(EntityKind::Task, repo)
        };
        let planned = |repo: Option<&str>| CardModel {
            work_class: Some(LANE_PLANNED.into()),
            ..card(EntityKind::Task, repo)
        };
        let mut todo = column_of(vec![
            support(Some("web")),
            planned(None),
            planned(Some("web")),
        ]);
        todo.name = "Todo".into();
        let mut active = column_of(vec![planned(Some("api"))]);
        active.name = "Active".into();
        let columns = vec![todo, active];
        assert_eq!(
            lane_counts(&columns, Some(LANE_PLANNED), &GroupLane::Any),
            vec![("Todo".to_string(), 2), ("Active".to_string(), 1)]
        );
        assert_eq!(
            lane_counts(&columns, Some(LANE_SUPPORT), &GroupLane::Any),
            vec![("Todo".to_string(), 1), ("Active".to_string(), 0)]
        );
        assert_eq!(
            lane_counts(&columns, None, &GroupLane::Slug("web".into())),
            vec![("Todo".to_string(), 2), ("Active".to_string(), 0)]
        );
        assert_eq!(
            lane_counts(&columns, None, &GroupLane::Unbound),
            vec![("Todo".to_string(), 1), ("Active".to_string(), 0)]
        );
        // The same predicate as the columns of the lane.
        for column in &columns {
            let shown = column
                .cards
                .iter()
                .filter(|c| lane_admits(c, Some(LANE_PLANNED), &GroupLane::Any))
                .count();
            let counted = lane_counts(
                std::slice::from_ref(column),
                Some(LANE_PLANNED),
                &GroupLane::Any,
            );
            assert_eq!(counted[0].1, shown);
        }
    }

    /// KAIROS-T-0582: a done column shows its last 20 cards, until "Show
    /// all". The other columns show each card.
    #[test]
    fn a_done_column_shows_its_last_cards() {
        let cards: Vec<usize> = (1..=25).collect();
        assert_eq!(
            visible_cards(&cards, true, false),
            (6..=25).collect::<Vec<_>>()
        );
        assert_eq!(visible_cards(&cards, true, true), cards);
        assert_eq!(visible_cards(&cards, false, false), cards);
        let few: Vec<usize> = (1..=5).collect();
        assert_eq!(visible_cards(&few, true, false), few);
        assert_eq!(
            visible_cards::<usize>(&[], true, false),
            Vec::<usize>::new()
        );
    }

    /// KAIROS-T-0582: the button of a done column.
    #[test]
    fn the_button_of_a_done_column() {
        assert_eq!(
            show_all_label(true, false, 5, 25).as_deref(),
            Some("Show all 25 cards")
        );
        assert_eq!(
            show_all_label(true, true, 0, 25).as_deref(),
            Some("Show the last 20 cards")
        );
        assert_eq!(show_all_label(true, false, 0, 20), None, "nothing hidden");
        assert_eq!(show_all_label(true, true, 0, 20), None, "nothing to hide");
        assert_eq!(
            show_all_label(false, false, 5, 25),
            None,
            "not a done column"
        );
    }

    /// KAIROS-T-0582: the first open lane shows the WIP chips.
    #[test]
    fn the_first_open_lane_shows_the_wip_chips() {
        let lanes = vec![LANE_SUPPORT.to_string(), LANE_PLANNED.to_string()];
        assert_eq!(wip_lane(&lanes, |_| false).as_deref(), Some(LANE_SUPPORT));
        assert_eq!(
            wip_lane(&lanes, |id| id == LANE_SUPPORT).as_deref(),
            Some(LANE_PLANNED)
        );
        assert_eq!(wip_lane(&lanes, |_| true), None);
        assert_eq!(wip_lane(&[], |_| false), None, "one row: no lane");
    }

    /// KAIROS-T-0582: a column that the board does not allow names the
    /// column of the card.
    #[test]
    fn a_refused_column_names_the_column_of_the_card() {
        let drag = DragData {
            kind: EntityKind::Task,
            short_code: "T-1".into(),
            targets: vec!["c-active".into()],
            source_column: "c-todo".into(),
            work_class: Some(LANE_PLANNED.into()),
            owner: None,
        };
        assert_eq!(
            refusal_note(&drag, "c-done", "Todo").as_deref(),
            Some("No transition from Todo.")
        );
        assert_eq!(refusal_note(&drag, "c-active", "Todo"), None, "allowed");
        assert_eq!(
            refusal_note(&drag, "c-todo", "Todo"),
            None,
            "its own column"
        );
    }

    /// KAIROS-T-0582: the "Mine" filter keeps the cards of the reader.
    #[test]
    fn mine_keeps_the_cards_of_the_reader() {
        let robin = owned(EntityKind::Task, Some(("u-robin", "Robin")));
        let sam = owned(EntityKind::Initiative, Some(("u-sam", "Sam")));
        let nobody = owned(EntityKind::Task, None);
        assert!(mine_admits(&robin, Some("u-robin")));
        assert!(!mine_admits(&sam, Some("u-robin")));
        assert!(!mine_admits(&nobody, Some("u-robin")));
        for card in [&robin, &sam, &nobody] {
            assert!(mine_admits(card, None), "the filter is off");
        }
    }

    /// KAIROS-T-0649: the days in the column and the notes count.
    #[test]
    fn the_days_in_the_column_and_the_notes_pills() {
        // 2026-10-18T12:00:00Z in milliseconds.
        let now = 1_792_324_800_000.0;
        assert_eq!(days_in_column("2026-10-18T12:00:00+00:00", now), Some(0));
        assert_eq!(days_in_column("2026-10-17T11:59:59Z", now), Some(1));
        assert_eq!(
            days_in_column("2026-10-08T12:00:00.123+00:00", now),
            Some(10)
        );
        assert_eq!(days_in_column("2026-10-19T00:00:00Z", now), None, "later");
        assert_eq!(days_in_column("yesterday", now), None);
        assert_eq!(days_pill(0), None);
        assert_eq!(days_pill(1).as_deref(), Some("1 day in column"));
        assert_eq!(days_pill(12).as_deref(), Some("12 days in column"));
        assert_eq!(notes_pill(0), None);
        assert_eq!(notes_pill(1).as_deref(), Some("1 note"));
        assert_eq!(notes_pill(4).as_deref(), Some("4 notes"));
    }

    /// KAIROS-T-0582: the priority pill.
    #[test]
    fn the_priority_pill() {
        assert_eq!(
            priority_pill("high"),
            ("priority high".to_string(), token::GOLD)
        );
        assert_eq!(priority_pill("Critical").1, token::GOLD);
        assert_eq!(
            priority_pill("low"),
            ("priority low".to_string(), token::MUTED)
        );
    }

    /// KAIROS-T-0582: the card shows its priority, and a new priority
    /// changes the key of the card (a metadata write makes no new
    /// version). The model marks the done column.
    #[test]
    fn the_card_shows_its_priority_and_the_column_is_done() {
        let base_view = view_of(serde_json::json!({}), "todo");
        let base = key_of(&base_view);
        let mut view = view_of(serde_json::json!({}), "todo");
        view.items.item_metadata = Some(
            [(
                "DEMO-T-0001".to_string(),
                [(data::PRIORITY_SLUG.to_string(), "high".to_string())].into(),
            )]
            .into(),
        );
        let summary = board_summary(&view);
        let card = &summary.columns[0].cards[0];
        assert!(
            card.meta
                .contains(&("priority high".to_string(), token::GOLD)),
            "{:?}",
            card.meta
        );
        assert_eq!(key_of(&view).version, base.version);
        assert_ne!(key_of(&view), base);
        assert!(
            board_summary(&base_view).columns[0].cards[0]
                .meta
                .is_empty()
        );

        let mut view = view_of(serde_json::json!({}), "todo");
        view.items.columns[1].column.is_done = true;
        let columns = board_summary(&view).columns;
        assert!(!columns[0].is_done);
        assert!(columns[1].is_done);
        assert_ne!(
            columns[1].key,
            board_summary(&base_view).columns[1].key,
            "a change of the done mark rebuilds the column"
        );
    }
}
