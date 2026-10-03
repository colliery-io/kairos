//! Activity feed + item content history (KAIROS-T-0044).
//!
//! Two pages, both under this task's `/activity` route segment:
//!
//! - [`ActivityPage`] (`/activity`) — the tenant-wide `activity_log` feed
//!   (KAIROS-A-0004): filterable by entity short code, actor, action, team
//!   and `since` timestamp (all combinable, S-0005), paginated, entity
//!   cells linking to the `/items/{short_code}` detail route.
//! - [`ItemHistoryPage`] (`/activity/history/:code`) — one item's
//!   append-only content history: the version list (version / editor /
//!   edited-at), any version's full snapshot, a client-side line diff
//!   between two selected versions (the `similar` crate — pure Rust,
//!   wasm-clean), and rollback via the A-0004 copy-forward flow: fetch the
//!   old snapshot, save it as a NEW version through the standard
//!   optimistic-concurrency PATCH (a 409 is surfaced and the list
//!   refreshed, per the conventions' conflict handling).
//!
//! **Route contract for KAIROS-T-0041 (item detail's "history link"):**
//! `/activity/history/{short_code}`.
//!
//! # Data-layer notes (docs/gui-conventions.md § Data layer)
//!
//! - The activity API filters by `entity_id`/`actor_id` (UUIDs). The
//!   short-code entity filter resolves client-side: the code's type letter
//!   names the family, `GET /api/{family}/{code}` yields the id. The actor
//!   filter picks from `GET /api/members` (open tenant-wide).
//! - The SERVER applies each filter (COLLIERY-T-0265), so the pager and the
//!   count are those of the entries after the filters. The filter `team`
//!   was a filter of the page that the browser had: a page of 25 entries
//!   could show no entry of a team that had entries on the next page.
//! - Entity *links* come from the entry (COLLIERY-T-0262): the server
//!   gives the short code and the title of the item of each entry, so the
//!   page reads no list of items. [`entity_cell`] makes the cell. An entry
//!   with no item says why it has no link.
//! - Rollback writes through `api::patch_json` — the standard versioned
//!   save path (conventions § Data layer), so the A-0004 409 contract is
//!   handled exactly like any other content edit.

use std::collections::HashMap;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Table, Text, TextInput,
};
use aurora_dark::data::{CodeBlock, Pagination, RelativeTime};
use aurora_dark::frame::{ConfirmDialog, use_toaster};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;

use crate::api;
use crate::auth::{Auth, use_auth};
use crate::pages::teams::api as teams_api;

/// Feed page size. Deliberately below the server default (50) so
/// pagination is exercised on modest datasets.
const PAGE_SIZE: i64 = 25;

/// The "no filter" option label shared by the actor/action selects.
const ALL: &str = "(all)";

// ---------------------------------------------------------------------------
// Mirror DTOs (partial on purpose; serde ignores the rest)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types::ListEnvelope<T>`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ListEnvelope<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// mirror of: `kairos_client::types_meta::HistoryVersion`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct HistoryVersion {
    pub version: i32,
    pub edited_by: String,
    pub edited_at: String,
}

/// mirror of: `kairos_client::types_meta::HistorySnapshot`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct HistorySnapshot {
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: String,
    pub edited_at: String,
}

/// mirror of: `kairos_client::types_meta::ActivityEntry`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ActivityEntry {
    pub id: String,
    pub actor_id: String,
    pub action: String,
    pub entity_id: Option<String>,
    pub entity_type: Option<String>,
    pub details: String,
    pub occurred_at: String,
    /// The short code of the item of the entry (COLLIERY-T-0262). Absent
    /// when the entry is not about an item, and when Kairos does not have
    /// the item.
    #[serde(default)]
    pub entity_short_code: Option<String>,
    /// The title of the item, as it is now.
    #[serde(default)]
    pub entity_title: Option<String>,
    /// When the item was archived (RFC 3339), absent for a live item.
    #[serde(default)]
    pub entity_archived_at: Option<String>,
}

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Member {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
}

/// mirror of: the shared head of `kairos_client::types::{Strategy,
/// Initiative, Task, Document, Adr}` (partial) — every entity family
/// carries these four fields with these names.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemHead {
    pub id: String,
    pub short_code: String,
    pub title: String,
    pub version: i32,
    /// When the item was PUT AWAY (RFC 3339), absent while it is live
    /// (KAIROS-T-0154, ADR-20). History is the audit answer, so this page
    /// serves an archived item exactly as it serves a live one — and says
    /// so, loudly, because the versions read identically either way.
    #[serde(default)]
    pub archived_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Pure helpers (host-tested)
// ---------------------------------------------------------------------------

/// The `activity_log.action` vocabulary (KAIROS-A-0004; enforcement point
/// is `kairos_db::models::enums::ActivityAction`). COLLIERY-T-0265: the
/// list has each action of the server, in the order of the server, so the
/// filter can select each of them. `update` is the action of a change to a
/// team, a delivery stream, a membership or a user.
const ACTIONS: &[&str] = &[
    "transition",
    "create",
    "delete",
    "relationship_add",
    "relationship_remove",
    "capability_grant",
    "capability_revoke",
    "board_config",
    "work_class",
    "lifecycle",
    "repository",
    "board_move",
    "restore",
    "update",
    "rename",
];

/// Map a `{PREFIX}-{LETTER}-{NNNN}` short code (S-0004) onto its API
/// family path segment. `None` for anything that doesn't parse.
fn family_of_short_code(code: &str) -> Option<&'static str> {
    let mut parts = code.rsplitn(3, '-');
    let number = parts.next()?;
    let letter = parts.next()?;
    let prefix = parts.next()?;
    if prefix.is_empty() || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match letter {
        "S" => Some("strategies"),
        "I" => Some("initiatives"),
        "T" => Some("tasks"),
        "D" => Some("documents"),
        "A" => Some("adrs"),
        _ => None,
    }
}

/// Percent-encode one query-string value (conservative: everything but
/// unreserved). RFC 3339 timestamps carry `:`/`+` which serde_urlencoded
/// on the server would otherwise mis-decode (`+` → space).
fn encode_query(value: &str) -> String {
    api::encode_component(value)
}

/// `2026-07-14T23:10:11.123456Z` → `2026-07-14 23:10:11` (display only;
/// unparseable input passes through untouched).
fn format_when(rfc3339: &str) -> String {
    match rfc3339.split_once('T') {
        Some((date, time)) => {
            let clock = time.get(..8).unwrap_or(time);
            format!("{date} {clock}")
        }
        None => rfc3339.to_string(),
    }
}

/// Accent token for an activity action pill (data-driven color per the
/// aurora token rule).
fn action_color(action: &str) -> &'static str {
    match action {
        "create" | "restore" => token::OK,
        "delete" => token::BAD,
        "transition" | "board_move" | "rename" => token::ICE,
        "update" => token::GOLD,
        "relationship_add" | "relationship_remove" => token::VIOLET,
        "capability_grant" | "capability_revoke" => token::GOLD,
        _ => token::MUTED,
    }
}

/// The entity types of the activity log that are item types
/// (`kairos_core::items::ItemType::entity_type`).
const ITEM_TYPES: &[&str] = &["strategy", "initiative", "task", "document", "adr"];

/// What the cell `Entity` of one entry shows (COLLIERY-T-0262).
#[derive(Clone, Debug, PartialEq)]
pub enum EntityCell {
    /// The entry is about an item that a person can open, live or
    /// archived: a link to `href`.
    Link {
        href: String,
        short_code: String,
        title: String,
        archived: bool,
    },
    /// The entry has no item that a person can open. `label` is the type
    /// of the entity, and `reason` says why the cell has no link.
    NoLink { label: String, reason: &'static str },
    /// The entry names no entity (a relationship: `details` has the short
    /// codes).
    Empty,
}

/// Why an entry about an item has no link: Kairos does not have the item.
const REASON_NO_ITEM: &str = "Kairos does not have this item.";
/// Why an entry about a board, a team or a member has no link.
const REASON_NOT_AN_ITEM: &str = "This entry is not about an item.";

/// The cell `Entity` of one entry, from the entry only (COLLIERY-T-0262).
/// The page reads no list of items, so an item after the first 200 of its
/// type gets its link. Pure, host-tested.
pub fn entity_cell(entry: &ActivityEntry) -> EntityCell {
    if let Some(code) = entry.entity_short_code.as_deref().filter(|c| !c.is_empty()) {
        return EntityCell::Link {
            href: format!("/items/{code}"),
            short_code: code.to_string(),
            title: entry.entity_title.clone().unwrap_or_default(),
            archived: entry.entity_archived_at.is_some(),
        };
    }
    if entry.entity_id.is_none() {
        return EntityCell::Empty;
    }
    let label = entry
        .entity_type
        .clone()
        .unwrap_or_else(|| "entity".to_string());
    let reason = if ITEM_TYPES.contains(&label.as_str()) {
        REASON_NO_ITEM
    } else {
        REASON_NOT_AN_ITEM
    };
    EntityCell::NoLink { label, reason }
}

/// One rendered diff line: `+` inserted, `-` deleted, ` ` unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    pub sign: char,
    pub text: String,
}

/// Client-side line diff (KAIROS-T-0044 AC). `similar`'s `TextDiff` —
/// pure-Rust Myers/LCS, compiles to wasm32 — over full snapshots
/// (A-0004 stores full snapshots precisely so diffing is a comparison,
/// not a replay).
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    similar::TextDiff::from_lines(old, new)
        .iter_all_changes()
        .map(|change| DiffLine {
            sign: match change.tag() {
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
                similar::ChangeTag::Equal => ' ',
            },
            text: change.value().trim_end_matches('\n').to_string(),
        })
        .collect()
}

/// Style + color for one diff line (inline `var(--…)` per the token rule).
fn diff_line_style(sign: char) -> String {
    let color = match sign {
        '+' => "--ok",
        '-' => "--bad",
        _ => "--muted",
    };
    format!(
        "margin:0;white-space:pre-wrap;font-family:var(--font-mono);\
         font-size:var(--fs-xs);line-height:1.5;color:var({color});"
    )
}

/// Resolve an actor id to a display name via the members map, falling
/// back to a shortened UUID.
fn actor_label(members: &HashMap<String, String>, actor_id: &str) -> String {
    members.get(actor_id).cloned().unwrap_or_else(|| {
        let head = actor_id.get(..8).unwrap_or(actor_id);
        format!("{head}…")
    })
}

// ---------------------------------------------------------------------------
// Fetch layer (thin wrappers over api::get_json + a local PATCH sibling)
// ---------------------------------------------------------------------------

/// `GET /api/{family}/{code}` — current id/title/version head.
async fn fetch_item_head(auth: Auth, code: String) -> Result<(ItemHead, &'static str), ApiError> {
    let family = family_of_short_code(&code).ok_or_else(|| ApiError::Http {
        status: 404,
        message: format!("{code:?} is not a short code. A short code has the form DEMO-T-0001."),
        code: Some("NOT_FOUND".to_string()),
    })?;
    let head = api::get_json::<ItemHead>(auth, &format!("/api/{family}/{code}")).await?;
    Ok((head, family))
}

/// `GET /api/{family}/{code}/history` — the version list, newest first.
async fn fetch_versions(
    auth: Auth,
    code: String,
) -> Result<ListEnvelope<HistoryVersion>, ApiError> {
    let (_, family) = fetch_item_head(auth, code.clone()).await?;
    api::get_json(auth, &format!("/api/{family}/{code}/history?limit=200")).await
}

/// `GET /api/{family}/{code}/history?version=N` — one full snapshot.
async fn fetch_snapshot(
    auth: Auth,
    code: String,
    version: i32,
) -> Result<HistorySnapshot, ApiError> {
    let family = family_of_short_code(&code).ok_or(ApiError::Network)?;
    api::get_json(
        auth,
        &format!("/api/{family}/{code}/history?version={version}"),
    )
    .await
}

/// `GET /api/members` → actor_id → display name map (+ the raw list for
/// the actor filter options). Each member, page after page
/// (COLLIERY-T-0258).
async fn fetch_members(auth: Auth) -> Result<Vec<Member>, ApiError> {
    api::get_all(auth, "/api/members").await
}

/// The applied activity-feed filters (what the resource fetches for).
#[derive(Clone, Debug, PartialEq, Default)]
struct FeedFilters {
    entity_code: String,
    actor_id: String,
    action: String,
    since: String,
    /// The slug of the team of the filter `team` (COLLIERY-T-0265), or an
    /// empty text.
    team_slug: String,
    /// The name of that team, for the note above the table.
    team_name: String,
    offset: i64,
}

impl FeedFilters {
    /// The person set no filter.
    fn is_clear(&self) -> bool {
        self.entity_code.is_empty()
            && self.actor_id.is_empty()
            && self.action.is_empty()
            && self.since.is_empty()
            && self.team_slug.is_empty()
    }
}

/// The meaning of the filter `team`, below the filters (COLLIERY-T-0265).
const TEAM_FILTER_RULE: &str = "The filter Team shows the entries by the members of the team. \
                                The members are those that the team has now.";

/// What the page says when the feed has no entry. Pure, host-tested.
fn empty_feed_message(filters: &FeedFilters) -> &'static str {
    if filters.is_clear() {
        "The organization has no activity. Make a change, then open this page again."
    } else {
        "No entry agrees with the filters. Change a filter, or select Clear."
    }
}

/// The note above the table when the filter `team` applies
/// (COLLIERY-T-0265). It gives the meaning of the filter. Pure,
/// host-tested.
fn team_filter_note(filters: &FeedFilters) -> Option<String> {
    (!filters.team_slug.is_empty()).then(|| {
        format!(
            "The list shows the entries by the members of {}.",
            filters.team_name
        )
    })
}

/// The query of `GET /api/activity` for `filters` (COLLIERY-T-0265).
/// `entity_id` is the id of the item of the filter by short code. Each
/// filter of the page is a parameter of the route: the server applies
/// it, and the page applies none. Pure, host-tested.
fn feed_query(filters: &FeedFilters, entity_id: Option<&str>) -> String {
    let mut query = format!("limit={PAGE_SIZE}&offset={}", filters.offset);
    if let Some(entity_id) = entity_id {
        query.push_str(&format!("&entity_id={}", encode_query(entity_id)));
    }
    if !filters.actor_id.is_empty() {
        query.push_str(&format!("&actor_id={}", encode_query(&filters.actor_id)));
    }
    if !filters.action.is_empty() {
        query.push_str(&format!("&action={}", encode_query(&filters.action)));
    }
    if !filters.since.is_empty() {
        query.push_str(&format!("&since={}", encode_query(filters.since.trim())));
    }
    if !filters.team_slug.is_empty() {
        query.push_str(&format!("&team={}", encode_query(&filters.team_slug)));
    }
    query
}

/// `GET /api/activity` with the S-0005 filters. The entity short code is
/// resolved to its UUID here (letter → family → GET the item).
async fn fetch_feed(
    auth: Auth,
    filters: FeedFilters,
) -> Result<ListEnvelope<ActivityEntry>, ApiError> {
    let entity_id = if filters.entity_code.is_empty() {
        None
    } else {
        let (head, _) = fetch_item_head(auth, filters.entity_code.trim().to_string()).await?;
        Some(head.id)
    };
    let query = feed_query(&filters, entity_id.as_deref());
    api::get_json(auth, &format!("/api/activity?{query}")).await
}

// ---------------------------------------------------------------------------
// Activity feed page
// ---------------------------------------------------------------------------

/// `/activity` — the filterable, paginated audit-trail feed.
#[component]
pub fn ActivityPage() -> impl IntoView {
    let auth = use_auth();

    // Filter inputs (bound signals) vs the *applied* filter set the
    // resource fetches for — Apply copies one into the other.
    let entity_input = RwSignal::new(String::new());
    let actor_input = RwSignal::new(ALL.to_string());
    let action_input = RwSignal::new(ALL.to_string());
    let since_input = RwSignal::new(String::new());
    let team_input = RwSignal::new(ALL.to_string());
    let applied = RwSignal::new(FeedFilters::default());

    let members = LocalResource::new(move || {
        let _ = auth.token();
        fetch_members(auth)
    });
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        teams_api::list_teams(auth)
    });
    // COLLIERY-T-0265: the team is a filter of the route, as each other
    // filter. The page reads the entries after the filters, and it removes
    // none.
    let feed = LocalResource::new(move || {
        let _ = auth.token();
        let filters = applied.get();
        fetch_feed(auth, filters)
    });

    // actor option label → user_id, resolved from the loaded members at
    // Apply time (the option list only exists once members have loaded).
    let actor_id_of = move |label: &str| -> String {
        if label == ALL {
            return String::new();
        }
        members
            .get()
            .and_then(|r| r.ok())
            .and_then(|items| {
                items
                    .iter()
                    .find(|m| member_option(m) == label)
                    .map(|m| m.user_id.clone())
            })
            .unwrap_or_default()
    };
    let apply = move || {
        // Team option labels are team names; resolve to the slug from the
        // loaded list (the option only exists once teams have loaded).
        let team = match team_input.get() {
            label if label == ALL => None,
            label => teams
                .get()
                .and_then(|r| r.ok())
                .and_then(|list| list.into_iter().find(|team| team.name == label)),
        };
        let (team_slug, team_name) = team.map(|team| (team.slug, team.name)).unwrap_or_default();
        applied.set(FeedFilters {
            entity_code: entity_input.get().trim().to_string(),
            actor_id: actor_id_of(&actor_input.get()),
            action: match action_input.get() {
                a if a == ALL => String::new(),
                a => a,
            },
            since: since_input.get().trim().to_string(),
            team_slug,
            team_name,
            offset: 0,
        });
    };
    let clear = move || {
        entity_input.set(String::new());
        actor_input.set(ALL.to_string());
        action_input.set(ALL.to_string());
        since_input.set(String::new());
        team_input.set(ALL.to_string());
        applied.set(FeedFilters::default());
    };

    let retry = Callback::new(move |()| applied.set(applied.get_untracked()));

    let mut action_options = vec![ALL.to_string()];
    action_options.extend(ACTIONS.iter().map(|a| a.to_string()));

    view! {
        <PageHeader
            title="Activity"
            sub="The changes in the organization, with the person and the time of each change."
        />
        <Stack gap="md">
            <Panel title="Filters" caption="You can use 2 or more filters at the same time.">
                <Stack gap="xs">
                    <Group gap="sm" top=true wrap=true>
                        <TextInput
                            label="Entity short code"
                            placeholder="e.g. DEMO-T-0001"
                            value=entity_input
                        />
                        {move || {
                            let loaded = members.get().and_then(|r| r.ok()).unwrap_or_default();
                            let mut options = vec![ALL.to_string()];
                            options.extend(loaded.iter().map(member_option));
                            view! { <Select label="Actor" options=options value=actor_input/> }
                        }}
                        <Select label="Action" options=action_options value=action_input/>
                        {move || {
                            let loaded = teams.get().and_then(|r| r.ok()).unwrap_or_default();
                            let mut options = vec![ALL.to_string()];
                            options.extend(loaded.into_iter().map(|team| team.name));
                            view! { <Select label="Team (by members)" options=options value=team_input/> }
                        }}
                        <TextInput
                            label="Since (RFC 3339)"
                            placeholder="2026-07-14T00:00:00Z"
                            value=since_input
                        />
                        <Button on_click=Callback::new(move |()| apply())>"Apply"</Button>
                        <Button variant="default" on_click=Callback::new(move |()| clear())>
                            "Clear"
                        </Button>
                    </Group>
                    <Text dimmed=true size="xs" attr:data-testid="activity-team-rule">
                        {TEAM_FILTER_RULE}
                    </Text>
                </Stack>
            </Panel>
            {move || match feed.get() {
                None => view! { <Loading label="Loading activity…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(page)) if page.items.is_empty() => {
                    let message = empty_feed_message(&applied.get());
                    view! { <Empty message=message/> }.into_any()
                }
                Some(Ok(page)) => {
                    let names = members
                        .get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| (m.user_id, m.display_name))
                        .collect::<HashMap<_, _>>();
                    let team_note = team_filter_note(&applied.get());
                    view! {
                        <Panel title="Feed" caption="The newest entry is first.">
                            {team_note.map(|note| view! {
                                <Text dimmed=true size="xs" attr:data-testid="activity-team-note">
                                    {note}
                                </Text>
                            })}
                            <FeedTable page=page.clone() names=names/>
                            <FeedPager page=page applied=applied/>
                        </Panel>
                    }.into_any()
                }
            }}
        </Stack>
    }
}

/// The option label shown for one member in the actor filter.
fn member_option(member: &Member) -> String {
    format!("{} <{}>", member.display_name, member.email)
}

/// The feed table (extracted so the async-state match stays readable).
#[component]
fn FeedTable(page: ListEnvelope<ActivityEntry>, names: HashMap<String, String>) -> impl IntoView {
    let rows = page
        .items
        .into_iter()
        .map(|entry| {
            let color = action_color(&entry.action).to_string();
            let actor = actor_label(&names, &entry.actor_id);
            let when = entry.occurred_at.clone();
            // COLLIERY-T-0262: the link comes from the entry. An entry
            // with no item says why it has no link.
            let entity = match entity_cell(&entry) {
                EntityCell::Link {
                    href,
                    short_code,
                    title,
                    archived,
                } => view! {
                    <Group gap="xs" wrap=true>
                        <span class="kairos-code"><Anchor href=href>{short_code}</Anchor></span>
                        {archived.then(|| view! {
                            <span class="kairos-archived-badge">
                                <Pill color=token::GOLD>"put away"</Pill>
                            </span>
                        })}
                        <Text dimmed=true size="xs">{title}</Text>
                    </Group>
                }
                .into_any(),
                EntityCell::NoLink { label, reason } => view! {
                    <span data-testid="activity-no-link" title=reason>
                        <Text dimmed=true size="xs">{format!("{label}. {reason}")}</Text>
                    </span>
                }
                .into_any(),
                EntityCell::Empty => view! { <Text dimmed=true size="xs">"—"</Text> }.into_any(),
            };
            view! {
                <tr>
                    <td><Text size="xs"><RelativeTime iso=when/></Text></td>
                    <td><Text size="sm">{actor}</Text></td>
                    <td><Pill color=color>{entry.action}</Pill></td>
                    <td>{entity}</td>
                    <td><Text mono=true dimmed=true size="xs">{entry.details}</Text></td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <Table label="Activity feed" min_width="720px">
            <thead>
                <tr>
                    <th>"When"</th>
                    <th>"Actor"</th>
                    <th>"Action"</th>
                    <th>"Entity"</th>
                    <th>"Details"</th>
                </tr>
            </thead>
            <tbody>{rows}</tbody>
        </Table>
    }
}

/// Offset pagination under the feed table: the Aurora `Pagination`
/// (COLLIERY-T-1836), and the sentence of COLLIERY-T-0258 that says which
/// part of the feed the page shows.
#[component]
fn FeedPager(page: ListEnvelope<ActivityEntry>, applied: RwSignal<FeedFilters>) -> impl IntoView {
    // COLLIERY-T-0258: the feed has no bound, so the page shows a part of
    // it and says which part.
    let range = api::page_range_note(page.offset, page.items.len(), page.total);
    let offset = RwSignal::new(usize::try_from(page.offset).unwrap_or(0));
    let limit = RwSignal::new(usize::try_from(PAGE_SIZE).unwrap_or(25));
    let total = usize::try_from(page.total).unwrap_or(0);
    let go = Callback::new(move |(to, _): (usize, usize)| {
        applied.update(|filters| filters.offset = i64::try_from(to).unwrap_or(0));
    });
    view! {
        <Stack gap="xs">
            <Text dimmed=true size="xs">{range}</Text>
            <Pagination offset limit total=total on_change=go/>
        </Stack>
    }
}

// ---------------------------------------------------------------------------
// Item history page
// ---------------------------------------------------------------------------

/// A computed two-version diff, ready to render.
#[derive(Clone, Debug, PartialEq)]
struct DiffView {
    from: i32,
    to: i32,
    title_lines: Vec<DiffLine>,
    content_lines: Vec<DiffLine>,
}

/// The outcome banner state after a rollback attempt that did not work.
/// A rollback that works is a toast (COLLIERY-T-1836).
#[derive(Clone, Debug, PartialEq)]
enum RollbackNotice {
    Conflict(String),
    Failed(String),
}

/// `/activity/history/:code` — version list, snapshot viewer, two-version
/// diff, and A-0004 copy-forward rollback. KAIROS-T-0041's item detail
/// links here.
#[component]
pub fn ItemHistoryPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let code = Memo::new(move |_| params.read().get("code").unwrap_or_default());

    // Bumped after a successful rollback so the head + version list refetch.
    let refresh = RwSignal::new(0u32);

    let head = LocalResource::new(move || {
        let _ = auth.token();
        let _ = refresh.get();
        fetch_item_head(auth, code.get())
    });
    let versions = LocalResource::new(move || {
        let _ = auth.token();
        let _ = refresh.get();
        fetch_versions(auth, code.get())
    });
    let members = LocalResource::new(move || {
        let _ = auth.token();
        fetch_members(auth)
    });

    // Snapshot viewer ("View" on a row).
    let viewing = RwSignal::new(None::<i32>);
    let snapshot = LocalResource::new(move || {
        let _ = auth.token();
        let code = code.get();
        let wanted = viewing.get();
        async move {
            match wanted {
                Some(version) => fetch_snapshot(auth, code, version).await.map(Some),
                None => Ok(None),
            }
        }
    });

    // Diff selection: "from" (A) and "to" (B) radio columns.
    let diff_from = RwSignal::new(None::<i32>);
    let diff_to = RwSignal::new(None::<i32>);
    let diff = LocalResource::new(move || {
        let _ = auth.token();
        let code = code.get();
        let pair = (diff_from.get(), diff_to.get());
        async move {
            match pair {
                (Some(from), Some(to)) if from != to => {
                    let old = fetch_snapshot(auth, code.clone(), from).await?;
                    let new = fetch_snapshot(auth, code, to).await?;
                    Ok(Some(DiffView {
                        from,
                        to,
                        title_lines: if old.title == new.title {
                            Vec::new()
                        } else {
                            diff_lines(&old.title, &new.title)
                        },
                        content_lines: diff_lines(&old.content, &new.content),
                    }))
                }
                _ => Ok(None),
            }
        }
    });

    // Rollback confirm + outcome.
    let confirm_open = RwSignal::new(false);
    let rollback_target = RwSignal::new(None::<i32>);
    let rollback_busy = RwSignal::new(false);
    let notice = RwSignal::new(None::<RollbackNotice>);
    let toaster = use_toaster();
    let run_rollback = move || {
        if rollback_busy.get_untracked() {
            return; // re-entrancy guard (the confirm button stays visible)
        }
        let Some(version) = rollback_target.get_untracked() else {
            return;
        };
        let code = code.get_untracked();
        rollback_busy.set(true);
        notice.set(None);
        leptos::task::spawn_local(async move {
            let outcome = rollback(auth, code, version).await;
            rollback_busy.set(false);
            confirm_open.set(false);
            match outcome {
                Ok(new_version) => {
                    toaster.success(format!(
                        "Kairos copied version v{version} to the new version v{new_version}."
                    ));
                    // Server state is the source of truth: refetch.
                    refresh.update(|n| *n += 1);
                }
                Err(ApiError::Http {
                    status: 409,
                    message,
                    ..
                }) => {
                    notice.set(Some(RollbackNotice::Conflict(message)));
                    refresh.update(|n| *n += 1);
                }
                Err(error) => {
                    let message = match error {
                        ApiError::Http { message, .. } => message,
                        ApiError::Network => "The page cannot connect to the server.".to_string(),
                        ApiError::Unknown(message) => message,
                    };
                    notice.set(Some(RollbackNotice::Failed(message)));
                }
            }
        });
    };

    let retry = Callback::new(move |()| refresh.update(|n| *n += 1));

    view! {
        {move || view! {
            <PageHeader
                title=code.get()
                sub="The history of the content: each version, its editor and its time."
            />
        }}
        <Stack gap="md">
            {move || match head.get() {
                None => view! { <Loading label="Loading item…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok((item, _))) => {
                    let archived_at = item.archived_at.clone();
                    let is_archived = archived_at.is_some();
                    view! {
                        <Group justify="between">
                            <Group gap="sm">
                                <Text bright=true bold=true>{item.title.clone()}</Text>
                                <Pill color=token::ICE>{format!("v{} current", item.version)}</Pill>
                                {is_archived.then(|| view! {
                                    <span class="kairos-archived-badge">
                                        <Pill color=token::GOLD>"put away"</Pill>
                                    </span>
                                })}
                            </Group>
                            <Anchor href=format!("/items/{}", item.short_code)>"Open the item"</Anchor>
                        </Group>
                        // KAIROS-T-0164 / ADR-20: an archived item's history
                        // is the audit answer this initiative exists for, and
                        // it reads exactly like a live item's — so it carries
                        // the same unmistakable marker the item page does.
                        // (Note for whoever renames things: a DOCUMENT's
                        // editorial "lifecycle: archived" is a different
                        // state entirely — KAIROS-T-0078 — and never appears
                        // on this page.)
                        {archived_at.map(|when| view! {
                            <div class="kairos-item__archived" data-testid="archived-banner">
                                <Banner color=token::GOLD icon="⧉">
                                    <Stack gap="xs">
                                        <Text bright=true bold=true>
                                            {format!("Put away on {}", format_when(&when))}
                                        </Text>
                                        <Text size="sm">
                                            "This item is archived. The boards, the queues and \
                                             the default searches do not show it, and you \
                                             cannot change it. The history is complete: each \
                                             version below shows the content at that time. To \
                                             edit the item or to roll back, restore it from \
                                             the item page."
                                        </Text>
                                    </Stack>
                                </Banner>
                            </div>
                        })}
                    }.into_any()
                }
            }}
            {move || notice.get().map(|state| match state {
                RollbackNotice::Conflict(message) => view! {
                    <Alert title="Version conflict (409)" color=token::GOLD>
                        <Text size="sm">
                            {format!(
                                "A person saved a newer version during the rollback. {message} \
                                 The list below has the new versions. Select a version and \
                                 roll back again."
                            )}
                        </Text>
                    </Alert>
                }.into_any(),
                RollbackNotice::Failed(message) => view! {
                    <Alert title="Rollback failed" color=token::BAD>
                        <Text size="sm">{message}</Text>
                    </Alert>
                }.into_any(),
            })}
            {move || match versions.get() {
                None => view! { <Loading label="Loading history…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(page)) if page.items.is_empty() => view! {
                    <Empty message="This item has no version in its history. An item from an old data set can have no version."/>
                }.into_any(),
                Some(Ok(page)) => {
                    let names = members
                        .get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| (m.user_id, m.display_name))
                        .collect::<HashMap<_, _>>();
                    let head_now = head.get().and_then(|r| r.ok());
                    let current = head_now.as_ref().map(|(item, _)| item.version);
                    // Rollback is a versioned WRITE (copy-forward, A-0004)
                    // and every write path resolves live-only, so it is
                    // disabled — with the reason — rather than left to 404.
                    let archived = head_now
                        .as_ref()
                        .is_some_and(|(item, _)| item.archived_at.is_some());
                    // COLLIERY-T-0258: the list has the newest 200 versions.
                    // An item with more versions says that the list is
                    // not complete.
                    let incomplete = api::incomplete_list_note(page.items.len(), page.total);
                    view! {
                        <Panel title="Versions" caption="The newest version is first. Select A and B to compare 2 versions.">
                            {incomplete.map(|note| view! {
                                <Text size="xs" dimmed=true>
                                    {format!("{note} The list has the newest versions.")}
                                </Text>
                            })}
                            {archived.then(|| view! {
                                <Text size="xs" dimmed=true>
                                    "You can read and compare the versions. You cannot roll back \
                                     an archived item. Restore the item first."
                                </Text>
                            })}
                            <VersionsTable
                                page=page
                                names=names
                                current=current
                                archived=archived
                                viewing=viewing
                                diff_from=diff_from
                                diff_to=diff_to
                                on_rollback=Callback::new(move |version: i32| {
                                    rollback_target.set(Some(version));
                                    confirm_open.set(true);
                                })
                            />
                        </Panel>
                    }.into_any()
                }
            }}
            {move || match diff.get() {
                None => ().into_any(),
                Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                Some(Ok(None)) => ().into_any(),
                Some(Ok(Some(view_model))) => view! { <DiffPanel view_model/> }.into_any(),
            }}
            {move || match snapshot.get() {
                None => ().into_any(),
                Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                Some(Ok(None)) => ().into_any(),
                Some(Ok(Some(snap))) => {
                    let caption = format!(
                        "The version of {} by {}.",
                        format_when(&snap.edited_at),
                        snap.edited_by
                    );
                    view! {
                        <Panel title=format!("Snapshot v{}: {}", snap.version, snap.title) caption=caption>
                            <CodeBlock
                                code=snap.content.clone()
                                wrap=true
                                max_height="480px"
                                label=format!("Snapshot v{}", snap.version)
                            />
                        </Panel>
                    }.into_any()
                }
            }}
        </Stack>
        // COLLIERY-T-1836: an Aurora `ConfirmDialog` (not a danger: a
        // rollback deletes nothing).
        <ConfirmDialog
            open=confirm_open
            title="Roll back?"
            confirm_label="Roll back"
            danger=false
            busy=rollback_busy
            on_confirm=Callback::new(move |()| run_rollback())
        >
            <Text size="sm">
                {move || format!(
                    "A rollback copies the snapshot v{} to a new version. The history \
                     keeps each version, and Kairos deletes nothing.",
                    rollback_target.get().unwrap_or_default()
                )}
            </Text>
        </ConfirmDialog>
    }
}

/// The A-0004 copy-forward rollback: old snapshot → standard versioned
/// PATCH (fresh `version` fetched immediately before; a concurrent edit in
/// the gap still 409s and is surfaced to the caller).
async fn rollback(auth: Auth, code: String, version: i32) -> Result<i32, ApiError> {
    let snapshot = fetch_snapshot(auth, code.clone(), version).await?;
    let (current, family) = fetch_item_head(auth, code.clone()).await?;
    let body = serde_json::json!({
        "title": snapshot.title,
        "content": snapshot.content,
        "version": current.version,
    });
    let updated: ItemHead = api::patch_json(auth, &format!("/api/{family}/{code}"), &body).await?;
    Ok(updated.version)
}

/// The version-list table: view / diff-select / rollback per row.
#[component]
fn VersionsTable(
    page: ListEnvelope<HistoryVersion>,
    names: HashMap<String, String>,
    current: Option<i32>,
    /// The item is archived (ADR-20): rollback is disabled per row.
    archived: bool,
    viewing: RwSignal<Option<i32>>,
    diff_from: RwSignal<Option<i32>>,
    diff_to: RwSignal<Option<i32>>,
    on_rollback: Callback<i32>,
) -> impl IntoView {
    let rows = page
        .items
        .into_iter()
        .map(|row| {
            let version = row.version;
            let is_current = current == Some(version);
            let editor = actor_label(&names, &row.edited_by);
            let when = row.edited_at.clone();
            view! {
                <tr>
                    <td>
                        <Group gap="xs">
                            <Text mono=true bright=true size="sm">{format!("v{version}")}</Text>
                            {is_current.then(|| view! { <Pill color=token::ICE>"current"</Pill> })}
                        </Group>
                    </td>
                    <td><Text size="sm">{editor}</Text></td>
                    <td><Text size="xs"><RelativeTime iso=when/></Text></td>
                    <td>
                        <input
                            type="radio"
                            name="kairos-diff-from"
                            aria-label=format!("diff from v{version}")
                            prop:checked=move || diff_from.get() == Some(version)
                            on:change=move |_| diff_from.set(Some(version))
                        />
                    </td>
                    <td>
                        <input
                            type="radio"
                            name="kairos-diff-to"
                            aria-label=format!("diff to v{version}")
                            prop:checked=move || diff_to.get() == Some(version)
                            on:change=move |_| diff_to.set(Some(version))
                        />
                    </td>
                    <td>
                        <Group gap="xs">
                            <Button
                                variant="default"
                                size="xs"
                                on_click=Callback::new(move |()| viewing.set(Some(version)))
                            >
                                "View"
                            </Button>
                            <Button
                                variant="default"
                                size="xs"
                                disabled=is_current || archived
                                on_click=Callback::new(move |()| on_rollback.run(version))
                            >
                                "Roll back"
                            </Button>
                        </Group>
                    </td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <Table label="Versions">
            <thead>
                <tr>
                    <th>"Version"</th>
                    <th>"Editor"</th>
                    <th>"Edited at"</th>
                    <th>"A"</th>
                    <th>"B"</th>
                    <th>"Actions"</th>
                </tr>
            </thead>
            <tbody>{rows}</tbody>
        </Table>
    }
}

/// Rendered diff between the two selected versions.
#[component]
fn DiffPanel(view_model: DiffView) -> impl IntoView {
    let DiffView {
        from,
        to,
        title_lines,
        content_lines,
    } = view_model;
    let render = |lines: Vec<DiffLine>| {
        lines
            .into_iter()
            .map(|line| {
                let style = diff_line_style(line.sign);
                view! { <pre style=style>{format!("{} {}", line.sign, line.text)}</pre> }
            })
            .collect_view()
    };
    let changed = title_lines
        .iter()
        .chain(content_lines.iter())
        .any(|line| line.sign != ' ');
    view! {
        <Panel
            title=format!("Diff v{from} → v{to}")
            caption="The differences between the 2 versions, line by line."
        >
            <Stack gap="sm">
                {(!changed).then(|| view! {
                    <Text dimmed=true size="sm">"The 2 versions have the same title and the same content."</Text>
                })}
                {(!title_lines.is_empty()).then(|| view! {
                    <Stack gap="xs">
                        <Text dimmed=true size="xs" bold=true>"Title"</Text>
                        {render(title_lines.clone())}
                    </Stack>
                })}
                <Stack gap="xs">
                    <Text dimmed=true size="xs" bold=true>"Content"</Text>
                    {render(content_lines.clone())}
                </Stack>
            </Stack>
        </Panel>
    }
}

// ---------------------------------------------------------------------------
// Host unit tests (pure logic + mirror decode locks, per conventions § 8)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Letter → family mapping covers all five S-0004 types and rejects
    /// non-codes (drives both the entity filter and the history routes).
    #[test]
    fn short_code_family_mapping() {
        assert_eq!(family_of_short_code("DEMO-S-0001"), Some("strategies"));
        assert_eq!(family_of_short_code("DEMO-I-0002"), Some("initiatives"));
        assert_eq!(family_of_short_code("KAIROS-T-0044"), Some("tasks"));
        assert_eq!(family_of_short_code("DEMO-D-0003"), Some("documents"));
        assert_eq!(family_of_short_code("A-A-9999"), Some("adrs"));
        assert_eq!(family_of_short_code("DEMO-T-10000"), Some("tasks"));
        for bogus in [
            "",
            "DEMO",
            "DEMO-X-0001",
            "demo-t-0001",
            "-T-0001",
            "DEMO-T-",
            "DEMO-T-12a4",
            "plain text",
        ] {
            assert_eq!(
                family_of_short_code(bogus),
                None,
                "{bogus:?} must not resolve"
            );
        }
    }

    /// Query values are percent-encoded so RFC 3339 `+00:00` offsets and
    /// UUIDs survive serde_urlencoded on the server.
    #[test]
    fn query_encoding_escapes_reserved() {
        assert_eq!(
            encode_query("2026-07-14T00:00:00Z"),
            "2026-07-14T00%3A00%3A00Z"
        );
        assert_eq!(
            encode_query("2026-07-14T00:00:00+02:00"),
            "2026-07-14T00%3A00%3A00%2B02%3A00"
        );
        assert_eq!(encode_query("abc-._~XYZ09"), "abc-._~XYZ09");
    }

    /// COLLIERY-T-0265: each filter of the page is a parameter of the
    /// route, the team too. The page has no filter of its own.
    #[test]
    fn each_filter_is_a_parameter_of_the_route() {
        assert_eq!(
            feed_query(&FeedFilters::default(), None),
            "limit=25&offset=0"
        );
        let filters = FeedFilters {
            entity_code: "DEMO-T-0001".to_string(),
            actor_id: "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f".to_string(),
            action: "update".to_string(),
            since: "2026-07-14T00:00:00Z".to_string(),
            team_slug: "platform".to_string(),
            team_name: "Platform".to_string(),
            offset: 50,
        };
        assert_eq!(
            feed_query(&filters, Some("b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001")),
            "limit=25&offset=50&entity_id=b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001\
             &actor_id=6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f&action=update\
             &since=2026-07-14T00%3A00%3A00Z&team=platform"
        );
        // A slug from before the rule of a slug.
        let old = FeedFilters {
            team_slug: "Road Map".to_string(),
            ..FeedFilters::default()
        };
        assert_eq!(feed_query(&old, None), "limit=25&offset=0&team=Road%20Map");
    }

    /// COLLIERY-T-0265: the texts of the page about the filters.
    #[test]
    fn the_page_says_what_the_filters_do() {
        let clear = FeedFilters::default();
        assert_eq!(
            empty_feed_message(&clear),
            "The organization has no activity. Make a change, then open this page again."
        );
        assert_eq!(team_filter_note(&clear), None);
        // The offset is not a filter.
        let next_page = FeedFilters {
            offset: 25,
            ..FeedFilters::default()
        };
        assert!(next_page.is_clear());
        let team = FeedFilters {
            team_slug: "platform".to_string(),
            team_name: "Platform".to_string(),
            ..FeedFilters::default()
        };
        assert_eq!(
            empty_feed_message(&team),
            "No entry agrees with the filters. Change a filter, or select Clear."
        );
        assert_eq!(
            team_filter_note(&team).as_deref(),
            Some("The list shows the entries by the members of Platform.")
        );
    }

    /// COLLIERY-T-0265: the filter by action has each action of the
    /// server (`kairos_db::models::enums::ActivityAction`).
    #[test]
    fn the_filter_has_each_action() {
        assert_eq!(ACTIONS.len(), 15);
        for action in ["update", "restore", "board_move", "repository", "rename"] {
            assert!(ACTIONS.contains(&action), "{action}");
        }
    }

    /// Timestamps render as date + clock; non-timestamps pass through.
    #[test]
    fn when_formatting() {
        assert_eq!(
            format_when("2026-07-14T23:10:11.123456Z"),
            "2026-07-14 23:10:11"
        );
        assert_eq!(format_when("2026-07-14T23:10:11Z"), "2026-07-14 23:10:11");
        assert_eq!(format_when("not a timestamp"), "not a timestamp");
    }

    /// Every documented activity action gets a deliberate accent; unknown
    /// values (future actions like retention_sweep) fall back to muted.
    #[test]
    fn action_colors_cover_the_vocabulary() {
        for action in ACTIONS {
            assert!(!action_color(action).is_empty());
        }
        assert_eq!(action_color("retention_sweep"), token::MUTED);
    }

    /// The `similar` line diff marks inserts/deletes/context the way the
    /// panel renders them.
    #[test]
    fn diff_lines_marks_changes() {
        let old = "alpha\nbravo\ncharlie\n";
        let new = "alpha\nBRAVO\ncharlie\ndelta\n";
        let lines = diff_lines(old, new);
        let rendered: Vec<(char, &str)> = lines.iter().map(|l| (l.sign, l.text.as_str())).collect();
        assert_eq!(
            rendered,
            vec![
                (' ', "alpha"),
                ('-', "bravo"),
                ('+', "BRAVO"),
                (' ', "charlie"),
                ('+', "delta"),
            ]
        );
        assert!(diff_lines("same\n", "same\n").iter().all(|l| l.sign == ' '));
    }

    /// mirror decode lock: history version list envelope (server shape from
    /// `GET /api/{family}/{code}/history`).
    #[test]
    fn history_mirrors_decode_server_shape() {
        let body = serde_json::json!({
            "items": [
                {"version": 2, "edited_by": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                 "edited_at": "2026-07-14T10:00:00.000000Z"},
                {"version": 1, "edited_by": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                 "edited_at": "2026-07-13T09:00:00.000000Z"}
            ],
            "total": 2, "limit": 200, "offset": 0
        });
        let envelope: ListEnvelope<HistoryVersion> =
            serde_json::from_value(body).unwrap_or_else(|e| panic!("mirror decodes: {e}"));
        assert_eq!(envelope.items[0].version, 2);
        assert_eq!(envelope.total, 2);

        let snapshot = serde_json::json!({
            "version": 1, "title": "T", "content": "C",
            "edited_by": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
            "edited_at": "2026-07-13T09:00:00.000000Z"
        });
        let snapshot: HistorySnapshot =
            serde_json::from_value(snapshot).unwrap_or_else(|e| panic!("mirror decodes: {e}"));
        assert_eq!((snapshot.version, snapshot.title.as_str()), (1, "T"));
    }

    /// mirror decode lock: activity entries (nullable entity fields) and
    /// the member + item-head partial mirrors.
    #[test]
    fn activity_and_head_mirrors_decode_server_shape() {
        let body = serde_json::json!({
            "items": [
                {"id": "0a8e9f7d-58f7-4f6e-9f0f-4dbb1a8f3e21",
                 "actor_id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                 "action": "transition",
                 "entity_id": "b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001",
                 "entity_type": "task",
                 "details": "column:Ready->Doing",
                 "occurred_at": "2026-07-14T10:00:00.000000Z"},
                {"id": "1b9eaf8e-69f8-4f7f-a010-5ecc2b9f4f32",
                 "actor_id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                 "action": "relationship_add",
                 "entity_id": null,
                 "entity_type": null,
                 "details": "parent:DEMO-T-0001->DEMO-I-0001",
                 "occurred_at": "2026-07-14T09:00:00.000000Z"}
            ],
            "total": 2, "limit": 25, "offset": 0
        });
        let envelope: ListEnvelope<ActivityEntry> =
            serde_json::from_value(body).unwrap_or_else(|e| panic!("mirror decodes: {e}"));
        assert_eq!(envelope.items[0].entity_type.as_deref(), Some("task"));
        assert_eq!(envelope.items[1].entity_id, None);

        let member = serde_json::json!({
            "user_id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
            "external_id": "sub-123", "email": "alice@kairos.test",
            "display_name": "alice", "role": "admin",
            "joined_at": "2026-07-01T00:00:00Z"
        });
        let member: Member =
            serde_json::from_value(member).unwrap_or_else(|e| panic!("mirror decodes: {e}"));
        assert_eq!(member.display_name, "alice");

        // The head mirror decodes a full Task body (extra fields ignored).
        let task = serde_json::json!({
            "id": "b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001",
            "short_code": "DEMO-T-0001", "title": "Fix login", "content": "…",
            "board_id": "x", "column_id": "y", "task_type": "task",
            "team_id": null, "version": 3,
            "created_by": "u", "updated_by": "u",
            "created_at": "2026-07-01T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let head: ItemHead =
            serde_json::from_value(task).unwrap_or_else(|e| panic!("mirror decodes: {e}"));
        assert_eq!((head.short_code.as_str(), head.version), ("DEMO-T-0001", 3));
    }

    /// One entry of the feed, as the server gives it.
    fn entry(value: serde_json::Value) -> ActivityEntry {
        serde_json::from_value(value).unwrap_or_else(|e| panic!("mirror decodes: {e}"))
    }

    /// COLLIERY-T-0262: the link of an entry comes from the entry. The
    /// function has no list of items, so the position of the item in a
    /// list (item 1 or item 5000 of its type) cannot change the result.
    #[test]
    fn an_entry_for_an_item_gets_a_link_from_the_entry() {
        let live = entry(serde_json::json!({
            "id": "0a8e9f7d-58f7-4f6e-9f0f-4dbb1a8f3e21",
            "actor_id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
            "action": "create",
            "entity_id": "b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001",
            "entity_type": "task",
            "details": "task:DEMO-T-5000",
            "occurred_at": "2026-07-14T10:00:00.000000Z",
            "entity_short_code": "DEMO-T-5000",
            "entity_title": "Task 5000",
            "entity_archived_at": null
        }));
        assert_eq!(
            entity_cell(&live),
            EntityCell::Link {
                href: "/items/DEMO-T-5000".to_string(),
                short_code: "DEMO-T-5000".to_string(),
                title: "Task 5000".to_string(),
                archived: false,
            }
        );
        // A person can open an archived item, so it has a link.
        let archived = ActivityEntry {
            entity_archived_at: Some("2026-07-15T10:00:00.000000Z".to_string()),
            ..live.clone()
        };
        assert_eq!(
            entity_cell(&archived),
            EntityCell::Link {
                href: "/items/DEMO-T-5000".to_string(),
                short_code: "DEMO-T-5000".to_string(),
                title: "Task 5000".to_string(),
                archived: true,
            }
        );
    }

    /// COLLIERY-T-0262: an entry with no item that a person can open says
    /// why it has no link.
    #[test]
    fn an_entry_with_no_item_says_why_it_has_no_link() {
        let base = entry(serde_json::json!({
            "id": "0a8e9f7d-58f7-4f6e-9f0f-4dbb1a8f3e21",
            "actor_id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
            "action": "create",
            "entity_id": "b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001",
            "entity_type": "team",
            "details": "team:payments",
            "occurred_at": "2026-07-14T10:00:00.000000Z"
        }));
        assert_eq!(
            entity_cell(&base),
            EntityCell::NoLink {
                label: "team".to_string(),
                reason: "This entry is not about an item.",
            }
        );
        // An item type with no short code: Kairos does not have the item.
        for item_type in ITEM_TYPES {
            let gone = ActivityEntry {
                entity_type: Some(item_type.to_string()),
                ..base.clone()
            };
            assert_eq!(
                entity_cell(&gone),
                EntityCell::NoLink {
                    label: item_type.to_string(),
                    reason: "Kairos does not have this item.",
                }
            );
        }
        // No entity: a relationship entry. The details have the short codes.
        let relationship = ActivityEntry {
            entity_id: None,
            entity_type: None,
            ..base
        };
        assert_eq!(entity_cell(&relationship), EntityCell::Empty);
    }

    /// Actor labels prefer the members map and degrade to a shortened id.
    #[test]
    fn actor_labels_resolve_or_shorten() {
        let mut members = HashMap::new();
        members.insert("6e4ff04d-1c92".to_string(), "alice".to_string());
        assert_eq!(actor_label(&members, "6e4ff04d-1c92"), "alice");
        assert_eq!(
            actor_label(&members, "b7a7f5b6-83fb-46f6-a3ed-9a0d1a11e001"),
            "b7a7f5b6…"
        );
    }
}
