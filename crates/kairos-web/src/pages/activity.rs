//! Activity feed + item content history (KAIROS-T-0044).
//!
//! Two pages, both under this task's `/activity` route segment:
//!
//! - [`ActivityPage`] (`/activity`) — the tenant-wide `activity_log` feed
//!   (KAIROS-A-0004): filterable by entity short code, actor, action, and
//!   `since` timestamp (all combinable, S-0005), paginated, entity cells
//!   linking to the `/items/{short_code}` detail route.
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
//! - Entity *links* need the reverse mapping (id → short code), which the
//!   API does not expose directly; [`fetch_directory`] builds a best-effort
//!   map from the five family list endpoints (first page of 200 each,
//!   failures skipped). Unresolvable ids render as plain text. Follow-up on
//!   record in KAIROS-T-0044: a server-side `short_code` on
//!   `ActivityEntry` would retire this.
//! - Rollback writes through `api::patch_json` — the standard versioned
//!   save path (conventions § Data layer), so the A-0004 409 contract is
//!   handled exactly like any other content edit.

use std::collections::HashMap;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, Modal, PageHeader, Panel, Pill,
    Select, Stack, Table, Text, TextInput,
};
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
}

// ---------------------------------------------------------------------------
// Pure helpers (host-tested)
// ---------------------------------------------------------------------------

/// The `activity_log.action` vocabulary (KAIROS-A-0004; enforcement point
/// is `kairos_db::models::enums::ActivityAction`).
const ACTIONS: &[&str] = &[
    "transition",
    "create",
    "delete",
    "relationship_add",
    "relationship_remove",
    "capability_grant",
    "capability_revoke",
    "board_config",
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
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
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
        "create" => token::OK,
        "delete" => token::BAD,
        "transition" => token::ICE,
        "relationship_add" | "relationship_remove" => token::VIOLET,
        "capability_grant" | "capability_revoke" => token::GOLD,
        _ => token::MUTED,
    }
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
        message: format!("{code:?} is not a Kairos short code (expected e.g. DEMO-T-0001)"),
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

/// `GET /api/members?limit=200` → actor_id → display name map (+ the raw
/// list for the actor filter options).
async fn fetch_members(auth: Auth) -> Result<Vec<Member>, ApiError> {
    let envelope: ListEnvelope<Member> = api::get_json(auth, "/api/members?limit=200").await?;
    Ok(envelope.items)
}

/// Best-effort entity_id → short_code directory from the five family list
/// endpoints (first 200 each; a failing family is skipped — links are an
/// enhancement, the feed renders without them).
async fn fetch_directory(auth: Auth) -> HashMap<String, String> {
    let mut directory = HashMap::new();
    for family in ["strategies", "initiatives", "tasks", "documents", "adrs"] {
        if let Ok(envelope) =
            api::get_json::<ListEnvelope<ItemHead>>(auth, &format!("/api/{family}?limit=200")).await
        {
            for item in envelope.items {
                directory.insert(item.id, item.short_code);
            }
        }
    }
    directory
}

/// The applied activity-feed filters (what the resource fetches for).
#[derive(Clone, Debug, PartialEq, Default)]
struct FeedFilters {
    entity_code: String,
    actor_id: String,
    action: String,
    since: String,
    offset: i64,
}

/// `GET /api/activity` with the S-0005 filters. The entity short code is
/// resolved to its UUID here (letter → family → GET the item).
async fn fetch_feed(
    auth: Auth,
    filters: FeedFilters,
) -> Result<ListEnvelope<ActivityEntry>, ApiError> {
    let mut query = format!("limit={PAGE_SIZE}&offset={}", filters.offset);
    if !filters.entity_code.is_empty() {
        let (head, _) = fetch_item_head(auth, filters.entity_code.trim().to_string()).await?;
        query.push_str(&format!("&entity_id={}", encode_query(&head.id)));
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

    // The team lens (KAIROS-T-0069, initiative design decision): the
    // activity API has no team parameter, so a selected team resolves to
    // its member set and the FETCHED PAGE filters client-side by actor.
    // `(team name, team id)` once applied.
    let applied_team = RwSignal::new(None::<(String, String)>);

    let members = LocalResource::new(move || {
        let _ = auth.token();
        fetch_members(auth)
    });
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        teams_api::list_teams(auth)
    });
    // The applied team's member ids (None = lens off). A failed roster
    // read surfaces through the feed area rather than silently unfiltering.
    let team_lens = LocalResource::new(move || {
        let _ = auth.token();
        let team = applied_team.get();
        async move {
            match team {
                None => Ok(None),
                Some((_, team_id)) => {
                    teams_api::team_members(auth, &team_id)
                        .await
                        .map(|members| {
                            Some(
                                members
                                    .into_iter()
                                    .map(|member| member.user_id)
                                    .collect::<std::collections::HashSet<_>>(),
                            )
                        })
                }
            }
        }
    });
    let directory = LocalResource::new(move || {
        let _ = auth.token();
        fetch_directory(auth)
    });
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
        applied.set(FeedFilters {
            entity_code: entity_input.get().trim().to_string(),
            actor_id: actor_id_of(&actor_input.get()),
            action: match action_input.get() {
                a if a == ALL => String::new(),
                a => a,
            },
            since: since_input.get().trim().to_string(),
            offset: 0,
        });
        // Team option labels are team names; resolve to the id from the
        // loaded list (the option only exists once teams have loaded).
        applied_team.set(match team_input.get() {
            label if label == ALL => None,
            label => teams
                .get()
                .and_then(|r| r.ok())
                .and_then(|list| list.into_iter().find(|team| team.name == label))
                .map(|team| (team.name, team.id)),
        });
    };
    let clear = move || {
        entity_input.set(String::new());
        actor_input.set(ALL.to_string());
        action_input.set(ALL.to_string());
        since_input.set(String::new());
        team_input.set(ALL.to_string());
        applied.set(FeedFilters::default());
        applied_team.set(None);
    };

    let retry = Callback::new(move |()| applied.set(applied.get_untracked()));

    let mut action_options = vec![ALL.to_string()];
    action_options.extend(ACTIONS.iter().map(|a| a.to_string()));

    view! {
        <PageHeader title="Activity" sub="who changed what, when — the tenant audit trail"/>
        <Stack gap="md">
            <Panel title="Filters" caption="combinable (S-0005)">
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
            </Panel>
            {move || match feed.get() {
                None => view! { <Loading label="Loading activity…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(page)) if page.items.is_empty() => view! {
                    <Empty message="No activity matches these filters — clear them, or make a change somewhere and come back."/>
                }.into_any(),
                Some(Ok(page)) => {
                    let names = members
                        .get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| (m.user_id, m.display_name))
                        .collect::<HashMap<_, _>>();
                    let codes = directory.get().unwrap_or_default();
                    // The team lens filters THIS PAGE by actor membership
                    // (client-side — the API has no team parameter; see the
                    // KAIROS-I-0006 design decision). The pager stays on
                    // the server page so navigation is unaffected.
                    let (table_page, lens_note, lens_error) = match team_lens.get() {
                        Some(Err(error)) => (page.clone(), None, Some(error)),
                        Some(Ok(Some(member_ids))) => {
                            let team_name = applied_team
                                .get()
                                .map(|(name, _)| name)
                                .unwrap_or_default();
                            let mut filtered = page.clone();
                            filtered
                                .items
                                .retain(|entry| member_ids.contains(&entry.actor_id));
                            let note = format!(
                                "{} of {} entries on this page are by members of {} \
                                 (team lens filters the fetched page)",
                                filtered.items.len(),
                                page.items.len(),
                                team_name,
                            );
                            (filtered, Some(note), None)
                        }
                        _ => (page.clone(), None, None),
                    };
                    let table_empty = table_page.items.is_empty();
                    view! {
                        <Panel title="Feed" caption="newest first">
                            {lens_error.map(|error| view! {
                                <Banner color=token::BAD icon="✕">
                                    {format!("Team lens unavailable: {}", match &error {
                                        ApiError::Http { message, .. } => message.clone(),
                                        ApiError::Network => "could not reach the server".to_string(),
                                        ApiError::Unknown(message) => message.clone(),
                                    })}
                                </Banner>
                            })}
                            {lens_note.map(|note| view! {
                                <Text dimmed=true size="xs">{note}</Text>
                            })}
                            {if table_empty {
                                view! {
                                    <Empty message="No entries on this page match the team lens — page through, or clear the team filter."/>
                                }.into_any()
                            } else {
                                view! { <FeedTable page=table_page names=names codes=codes/> }
                                    .into_any()
                            }}
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
fn FeedTable(
    page: ListEnvelope<ActivityEntry>,
    names: HashMap<String, String>,
    codes: HashMap<String, String>,
) -> impl IntoView {
    let rows = page
        .items
        .into_iter()
        .map(|entry| {
            let color = action_color(&entry.action).to_string();
            let actor = actor_label(&names, &entry.actor_id);
            let when = format_when(&entry.occurred_at);
            let entity = match (entry.entity_id, entry.entity_type) {
                (Some(id), entity_type) => match codes.get(&id) {
                    Some(code) => {
                        let code = code.clone();
                        view! {
                            <Anchor href=format!("/items/{code}")>{code.clone()}</Anchor>
                        }
                        .into_any()
                    }
                    None => view! {
                        <Text dimmed=true size="xs">
                            {entity_type.unwrap_or_else(|| "entity".to_string())}
                        </Text>
                    }
                    .into_any(),
                },
                (None, _) => view! { <Text dimmed=true size="xs">"—"</Text> }.into_any(),
            };
            view! {
                <tr>
                    <td><Text mono=true size="xs">{when}</Text></td>
                    <td><Text size="sm">{actor}</Text></td>
                    <td><Pill color=color>{entry.action}</Pill></td>
                    <td>{entity}</td>
                    <td><Text mono=true dimmed=true size="xs">{entry.details}</Text></td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <Table>
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

/// Offset pagination controls under the feed table.
#[component]
fn FeedPager(page: ListEnvelope<ActivityEntry>, applied: RwSignal<FeedFilters>) -> impl IntoView {
    let shown_from = page.offset + 1;
    let shown_to = page.offset + page.items.len() as i64;
    let total = page.total;
    let no_prev = page.offset == 0;
    let no_next = shown_to >= total;
    let prev_offset = (page.offset - PAGE_SIZE).max(0);
    let next_offset = page.offset + PAGE_SIZE;
    let go = move |offset: i64| {
        applied.update(|filters| filters.offset = offset);
    };
    view! {
        <Group justify="between">
            <Text dimmed=true size="xs">{format!("{shown_from}–{shown_to} of {total}")}</Text>
            <Group gap="xs">
                <Button
                    variant="default"
                    size="xs"
                    disabled=no_prev
                    on_click=Callback::new(move |()| go(prev_offset))
                >
                    "Previous"
                </Button>
                <Button
                    variant="default"
                    size="xs"
                    disabled=no_next
                    on_click=Callback::new(move |()| go(next_offset))
                >
                    "Next"
                </Button>
            </Group>
        </Group>
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

/// The outcome banner state after a rollback attempt.
#[derive(Clone, Debug, PartialEq)]
enum RollbackNotice {
    Done { from: i32, new_version: i32 },
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
    let run_rollback = move || {
        if rollback_busy.get_untracked() {
            return; // re-entrancy guard (the confirm button stays visible)
        }
        let Some(version) = rollback_target.get_untracked() else {
            return;
        };
        let code = code.get_untracked();
        rollback_busy.set(true);
        leptos::task::spawn_local(async move {
            let outcome = rollback(auth, code, version).await;
            rollback_busy.set(false);
            confirm_open.set(false);
            match outcome {
                Ok(new_version) => {
                    notice.set(Some(RollbackNotice::Done {
                        from: version,
                        new_version,
                    }));
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
                        ApiError::Network => "the server is unreachable".to_string(),
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
                sub="content history (KAIROS-A-0004): every version, who, when"
            />
        }}
        <Stack gap="md">
            {move || match head.get() {
                None => view! { <Loading label="Loading item…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok((item, _))) => view! {
                    <Group justify="between">
                        <Group gap="sm">
                            <Text bright=true bold=true>{item.title.clone()}</Text>
                            <Pill color=token::ICE>{format!("v{} current", item.version)}</Pill>
                        </Group>
                        <Anchor href=format!("/items/{}", item.short_code)>"Open item detail"</Anchor>
                    </Group>
                }.into_any(),
            }}
            {move || notice.get().map(|state| match state {
                RollbackNotice::Done { from, new_version } => view! {
                    <Banner color=token::OK icon="✓">
                        {format!(
                            "Rolled back: the v{from} snapshot was copied forward as new version v{new_version}."
                        )}
                    </Banner>
                }.into_any(),
                RollbackNotice::Conflict(message) => view! {
                    <Alert title="Version conflict (409)" color=token::GOLD>
                        <Text size="sm">
                            {format!(
                                "Someone saved a newer version while rolling back: {message}. \
                                 The list below has been refreshed — pick a version and try again."
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
                    <Empty message="No history yet — every item gets a v1 snapshot at creation, so this usually means the item was just created by an older data set."/>
                }.into_any(),
                Some(Ok(page)) => {
                    let names = members
                        .get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| (m.user_id, m.display_name))
                        .collect::<HashMap<_, _>>();
                    let current = head.get().and_then(|r| r.ok()).map(|(item, _)| item.version);
                    view! {
                        <Panel title="Versions" caption="newest first — pick A and B to diff">
                            <VersionsTable
                                page=page
                                names=names
                                current=current
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
                        "as of {} by {}",
                        format_when(&snap.edited_at),
                        snap.edited_by
                    );
                    view! {
                        <Panel title=format!("Snapshot v{} — {}", snap.version, snap.title) caption=caption>
                            <pre style="margin:0;white-space:pre-wrap;font-family:var(--font-mono);font-size:var(--fs-sm);color:var(--fg);">
                                {snap.content.clone()}
                            </pre>
                        </Panel>
                    }.into_any()
                }
            }}
        </Stack>
        <Modal open=confirm_open title="Roll back?">
            <Stack gap="sm">
                <Text size="sm">
                    {move || format!(
                        "Rolling back copies the v{} snapshot forward as a NEW version through \
                         the standard versioned save (KAIROS-A-0004) — history is append-only, \
                         nothing is destroyed.",
                        rollback_target.get().unwrap_or_default()
                    )}
                </Text>
                <Group gap="sm">
                    <Button on_click=Callback::new(move |()| run_rollback())>
                        {move || if rollback_busy.get() { "Rolling back…" } else { "Roll back" }}
                    </Button>
                    <Button variant="default" on_click=Callback::new(move |()| confirm_open.set(false))>
                        "Cancel"
                    </Button>
                </Group>
            </Stack>
        </Modal>
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
            let when = format_when(&row.edited_at);
            view! {
                <tr>
                    <td>
                        <Group gap="xs">
                            <Text mono=true bright=true size="sm">{format!("v{version}")}</Text>
                            {is_current.then(|| view! { <Pill color=token::ICE>"current"</Pill> })}
                        </Group>
                    </td>
                    <td><Text size="sm">{editor}</Text></td>
                    <td><Text mono=true size="xs">{when}</Text></td>
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
                                disabled=is_current
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
        <Table>
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
            caption="client-side line diff over full snapshots"
        >
            <Stack gap="sm">
                {(!changed).then(|| view! {
                    <Text dimmed=true size="sm">"These versions have identical title and content."</Text>
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
