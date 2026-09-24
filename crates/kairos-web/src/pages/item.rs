//! Item detail (KAIROS-T-0041): one `/items/:code` route serving all five
//! entity families (the short code's type letter picks the family — see
//! [`api::Family`]), per KAIROS-A-0015's v1 scope:
//!
//! - title + markdown content editing with preview ([`editor`], renderer
//!   decision in [`markdown`]) under A-0004 optimistic concurrency — 409
//!   opens the merge UI (server-current vs. yours, keep-mine /
//!   take-theirs / manual-merge, retry carries the new version);
//! - typed metadata panel ([`metadata`], A-0003: enum dropdowns from the
//!   definitions, date inputs, strings; null clears);
//! - board/column display (name via `GET /api/boards/{id}`, linked to the
//!   T-0040 board view) and type-specific facts;
//! - relationships summary (linked; the full explorer is T-0042's
//!   `/search/relationships/:code`) and the history link (T-0044's
//!   `/activity/history/:code`);
//! - create-from-template ([`create_doc`]) and soft delete with cascade
//!   warning ([`delete`], A-0001).

pub(crate) mod api;
mod create_doc;
mod delete;
mod editor;
pub(crate) mod markdown;
mod metadata;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Text,
};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::boards;
use super::copy_link;
use super::repositories;
use crate::auth::use_auth;
use api::{Family, ItemDetail, RelationshipGroup};
use create_doc::CreateDocumentDialog;
use delete::DeleteDialog;
use editor::ContentEditor;
use metadata::MetadataPanel;

/// The Details | Graph tab targets for a resolved short code — `None`
/// while the route param is still empty, so no anchor is ever emitted
/// without its code (KAIROS-T-0124 #2: a first-frame click on a tab built
/// from an empty param landed on `/items/?view=graph`).
fn tab_hrefs(code: &str) -> Option<(String, String)> {
    if code.is_empty() {
        return None;
    }
    Some((
        format!("/items/{code}"),
        format!("/items/{code}?view=graph"),
    ))
}

/// `/items/:code` — parse the family from the short code and hand off.
#[component]
pub fn ItemPage() -> impl IntoView {
    let params = use_params_map();
    let query = leptos_router::hooks::use_query_map();
    view! {
        {move || {
            let code = params.read().get("code").unwrap_or_default();
            // KAIROS-T-0090: Details | Graph as ANCHOR tabs riding a
            // `?view=` query param — plain history entries, so back and
            // refresh keep the choice with zero effect plumbing.
            let graph_mode = query.read().get("view").as_deref() == Some("graph");
            // KAIROS-T-0124 #2: the param can be empty for a frame while
            // the router settles — render nothing clickable until it is
            // resolved, so no tab is ever built from an empty code.
            let Some((details_href, graph_href)) = tab_hrefs(&code) else {
                return view! { <Loading label="Loading item…"/> }.into_any();
            };
            match Family::of_short_code(&code) {
                Some(family) => {
                    let tabs = {
                        view! {
                            <Group gap="xs">
                                <Anchor href=details_href>
                                    <Pill color=if graph_mode { token::MUTED } else { token::ICE }>
                                        "Details"
                                    </Pill>
                                </Anchor>
                                <Anchor href=graph_href>
                                    <Pill color=if graph_mode { token::ICE } else { token::MUTED }>
                                        "Graph"
                                    </Pill>
                                </Anchor>
                            </Group>
                        }
                    };
                    if graph_mode {
                        view! {
                            <PageHeader
                                title=code.clone()
                                sub="What does this depend on, what does it feed into, where does it sit?"
                            />
                            {tabs}
                            <crate::pages::search::graph::GraphView short_code=code/>
                        }.into_any()
                    } else {
                        view! {
                            {tabs}
                            <ItemDetailView family code/>
                        }.into_any()
                    }
                }
                None => view! {
                    <PageHeader title=code.clone() sub="work item"/>
                    <Panel title="Not a short code" caption="items/:code">
                        <Empty message=format!(
                            "\"{code}\" is not an item short code (expected PREFIX-S|I|T|D|A-NNNN)."
                        )/>
                    </Panel>
                }.into_any(),
            }
        }}
    }
}

/// The detail resource + the four async view states. `reload` is the
/// refetch handle mutations bump (server state is the source of truth).
#[component]
fn ItemDetailView(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    // Page-level transient notice (survives the refetch that recreates
    // the loaded subtree).
    let notice = RwSignal::new(None::<String>);

    let detail = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_item(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    let on_saved = Callback::new(move |version: i32| {
        notice.set(Some(format!("Saved — the item is now at v{version}.")));
        reload.update(|n| *n += 1);
    });
    // The move/lane controls' success path (KAIROS-T-0075/T-0077): the
    // item changed server-side; show the control's message and refetch.
    let on_moved = Callback::new(move |message: String| {
        notice.set(Some(message));
        reload.update(|n| *n += 1);
    });

    view! {
        {move || notice.get().map(|message| view! {
            <div class="kairos-item__notice">
                <Banner color=token::OK icon="✓">
                    <Group justify="between">
                        <Text size="sm">{message}</Text>
                        <Button variant="default" size="xs" on_click=Callback::new(move |_| notice.set(None))>
                            "Dismiss"
                        </Button>
                    </Group>
                </Banner>
            </div>
        })}
        {move || match detail.get() {
            None => view! { <Loading label="Loading item…"/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
            Some(Ok(item)) => view! {
                <ItemLoaded item family on_saved on_moved/>
            }.into_any(),
        }}
    }
}

/// The loaded page: header + actions, the editor column, and the facts /
/// metadata / relationships column.
#[component]
fn ItemLoaded(
    item: ItemDetail,
    family: Family,
    on_saved: Callback<i32>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let create_open = RwSignal::new(false);
    let delete_open = RwSignal::new(false);

    // Pre-compute every view input so the view! closures never capture
    // `item` itself (it is not Copy).
    let facts = item.clone();
    let header_title = item.title.clone();
    // The short code renders as its own element with a copy-link button
    // (KAIROS-T-0076), not folded into the subtitle string.
    let header_code = item.short_code.clone();
    let copy_code = item.short_code.clone();
    let version_label = format!("v{}", item.version);
    let history_href = format!("/activity/history/{}", item.short_code);
    let code = item.short_code.clone();
    let delete_title = item.title.clone();
    let lane = item.work_class.clone();
    let repository = item.repository.as_ref().map(|r| r.slug.clone());
    let lifecycle = item.lifecycle.clone();
    let lifecycle_badge = item.lifecycle.clone();
    // KAIROS-T-0164 / ADR-20: `archived_at` is the "put away" state — the
    // item is hidden by default and read-only, NOT gone. Deliberately not
    // the same thing as `lifecycle == "archived"` two lines up, which is a
    // document's EDITORIAL state (KAIROS-T-0078) and says nothing about
    // visibility; both can be true at once on this page, so the copy keeps
    // them apart: the editorial badge always reads "lifecycle: …", the
    // put-away state always reads "put away".
    let archived_at = item.archived_at.clone();
    let archived = archived_at.is_some();
    let banner_code = item.short_code.clone();
    let item_id = item.id.clone();
    let editorial_archived = item.lifecycle.as_deref() == Some("archived");
    let ItemDetail {
        short_code,
        title,
        content,
        version,
        board_id,
        column_id,
        ..
    } = item;

    // ONE board read for the page (KAIROS-T-0164): the placement panel
    // renders it and the Restore affordance's capability mirror needs the
    // board's slug/team — a second fetch would be the same request.
    // `Ok(None)` = the item sits on no board at all (a document, an
    // off-board ADR).
    let board_id_for_fetch = StoredValue::new(board_id);
    let board = LocalResource::new(move || {
        let _ = auth.token();
        let id = board_id_for_fetch.get_value();
        async move {
            match id {
                Some(id) => api::fetch_board(auth, id).await.map(Some),
                None => Ok(None),
            }
        }
    });
    let can_restore = restore_power(family, board);

    view! {
        <PageHeader title=header_title sub=family.label()/>
        {archived_at.map(|when| view! {
            <ArchivedBanner
                family
                code=banner_code
                item_id
                archived_at=when
                editorial_archived
                can_restore
                on_restored=on_moved
            />
        })}
        <Group justify="between">
            <Group gap="sm">
                <Text mono=true dimmed=true size="sm">{header_code}</Text>
                <copy_link::CopyLinkButton code=copy_code/>
                <Pill color=token::ICE>{version_label}</Pill>
                // The ADR-20 state, in the words the banner uses. Never
                // "archived" on its own next to the lifecycle badge below.
                {archived.then(|| view! {
                    <span class="kairos-archived-badge">
                        <Pill color=token::GOLD>"put away"</Pill>
                    </span>
                })}
                // The document lifecycle badge (KAIROS-T-0078): an
                // editorial label, deliberately distinct from anything
                // board-status-shaped — its own class + state color.
                {lifecycle_badge.map(|state| view! {
                    <span class="kairos-lifecycle-badge">
                        <Pill color=lifecycle_color(&state)>{format!("lifecycle: {state}")}</Pill>
                    </span>
                })}
                <TypeFacts item=facts/>
            </Group>
            <Group gap="sm">
                <Anchor href=history_href>"History"</Anchor>
                // Archived work is read-only server-side (every write path
                // resolves live-only), so the write affordances are
                // DISABLED with the reason next to them rather than
                // removed — a missing button reads as a broken page, and a
                // live one that 404s reads as a bug.
                {archived.then(|| view! {
                    <Text size="xs" dimmed=true>"read-only while put away"</Text>
                })}
                {family.is_workflow().then(|| view! {
                    <Button variant="default" size="xs" disabled=archived
                        on_click=Callback::new(move |_| create_open.set(true))>
                        "New document"
                    </Button>
                })}
                <Button variant="default" size="xs" bad=true disabled=archived
                    on_click=Callback::new(move |_| delete_open.set(true))>
                    "Delete"
                </Button>
            </Group>
        </Group>
        <ChildrenProgressBar family code=short_code.clone()/>
        <div class="kairos-item__grid">
            <ContentEditor
                family
                code=short_code.clone()
                initial_title=title
                initial_content=content
                initial_version=version
                read_only=archived
                on_saved
            />
            <Stack gap="sm">
                <BoardPanel family code=short_code.clone() board column_id
                    work_class=lane repository archived on_moved/>
                {lifecycle.map(|current| view! {
                    <LifecyclePanel code=short_code.clone() current archived on_moved/>
                })}
                <MetadataPanel family code=short_code.clone() read_only=archived/>
                <DevelopmentPanel family code=short_code.clone()/>
                <EdgeProposalsPanel code=short_code.clone()/>
                <RelationshipsPanel family code=short_code/>
            </Stack>
        </div>
        <CreateDocumentDialog parent_code=code.clone() open=create_open/>
        <DeleteDialog family code title=delete_title open=delete_open/>
    }
}

/// The capability that putting an item BACK asks for: the same
/// `manage_<family>` that putting it away asked for (KAIROS-T-0160 —
/// restoring is the inverse of archiving, not a new privilege, so no
/// deployment ends up with someone who can archive and nobody who can
/// restore).
fn manage_capability(family: Family) -> &'static str {
    match family {
        Family::Strategy => "manage_strategies",
        Family::Initiative => "manage_initiatives",
        Family::Task => "manage_tasks",
        Family::Document => "manage_documents",
        Family::Adr => "manage_adrs",
    }
}

/// May the signed-in user restore THIS item? The shared whoami mirror
/// (KAIROS-T-0072), read against the item's own board once it has loaded.
/// `false` until whoami and the board read have both resolved; an
/// off-board item (`Ok(None)`) or an unreadable board falls back to the
/// board-less mirror, because the server resolves such an item's
/// authorization board through its parent and the client cannot.
fn restore_power(
    family: Family,
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
) -> Memo<bool> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    Memo::new(move |_| {
        let Some(me) = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
        else {
            return false;
        };
        let required = manage_capability(family);
        match board.get() {
            None => false,
            Some(Ok(Some(board))) => {
                boards::holds_capability(&me, Some(&board.slug), board.team_id.as_deref(), required)
            }
            Some(Ok(None)) | Some(Err(_)) => boards::holds_capability(&me, None, None, required),
        }
    })
}

/// `2026-09-23T11:30:07.479107Z` → `2026-09-23 11:30 UTC` (display only;
/// anything that does not parse passes through untouched). Pure,
/// host-tested.
///
/// `pub(crate)` since KAIROS-T-0163: the search result list marks
/// put-away hits too, and one rule for rendering the ADR-20 moment is
/// the point — two would drift.
pub(crate) fn put_away_when(rfc3339: &str) -> String {
    match rfc3339.split_once('T') {
        Some((date, time)) => {
            let clock = time.get(..5).unwrap_or(time);
            format!("{date} {clock} UTC")
        }
        None => rfc3339.to_string(),
    }
}

/// The unmistakable marker on an archived item (KAIROS-T-0164, ADR-20).
///
/// An archived item reads exactly like a live one — same title, same
/// content, same history — so without this banner someone quotes a retired
/// ticket as current. It says WHEN the item was put away, WHO put it away
/// (from the activity trail, best-effort: the entity DTOs carry the moment
/// but not the actor), that the item is read-only, and offers Restore to
/// whoever holds the capability.
///
/// It also carries the disambiguation when the page shows BOTH senses of
/// "archived" at once — an editorially-archived document that is also put
/// away (see [`ItemLoaded`]'s note and KAIROS-A-0020's Neutral section).
#[component]
fn ArchivedBanner(
    family: Family,
    #[prop(into)] code: String,
    /// The entity UUID — the activity trail's filter key.
    #[prop(into)]
    item_id: String,
    /// RFC 3339, from the item's `archived_at`.
    #[prop(into)]
    archived_at: String,
    /// The item is a document whose EDITORIAL lifecycle is also
    /// "archived" — the one case where the collision is on screen.
    editorial_archived: bool,
    can_restore: Memo<bool>,
    /// Fired after a successful restore: the page posts the message and
    /// refetches, which is what makes the banner disappear.
    on_restored: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let item_id = StoredValue::new(item_id);
    let who = LocalResource::new(move || {
        let _ = auth.token();
        api::fetch_archived_by(auth, item_id.get_value())
    });
    let when = put_away_when(&archived_at);
    view! {
        <div class="kairos-item__archived" data-testid="archived-banner">
            <Banner color=token::GOLD icon="⧉">
                <Stack gap="xs">
                    <Text bright=true bold=true>
                        {move || match who.get().flatten() {
                            Some(actor) => format!("Put away on {when} by {actor}"),
                            None => format!("Put away on {when}"),
                        }}
                    </Text>
                    <Text size="sm">
                        "This is archived work: hidden from boards, queues and default \
                         searches, and read-only. It is not deleted — what you are reading \
                         is what it said when it was put away."
                    </Text>
                    {editorial_archived.then(|| view! {
                        <Text size="xs" dimmed=true>
                            "Two different things are called \"archived\" on this page: this \
                             banner (the document is put away — KAIROS-A-0020), and the \
                             \"lifecycle: archived\" badge below (its editorial state — \
                             KAIROS-T-0078). A live document can carry that badge; this \
                             banner is about visibility, not editorial status."
                        </Text>
                    })}
                    <RestoreControl family code can_restore on_restored/>
                </Stack>
            </Banner>
        </div>
    }
}

/// The Restore action (KAIROS-T-0160's endpoint): visible only to someone
/// holding `manage_<family>`, and rendering the 422 `RESTORE_BLOCKED`
/// refusal as a sentence naming what is gone — the refusal is the useful
/// half of the feature, so it never shows as a raw error code.
#[component]
fn RestoreControl(
    family: Family,
    #[prop(into)] code: String,
    can_restore: Memo<bool>,
    on_restored: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let blocked = RwSignal::new(None::<Vec<String>>);
    let error = RwSignal::new(None::<ApiError>);
    let restore: Callback<()> = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        blocked.set(None);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::restore_item(auth, family, &code.get_value()).await {
                Ok(outcome) => {
                    // A restore puts back exactly what it was asked for;
                    // descendants archived with it stay away and are named
                    // so nobody assumes the whole cascade came back.
                    let message = if outcome.still_archived_count == 0 {
                        format!("{} is back on its board.", outcome.short_code)
                    } else {
                        format!(
                            "{} is back on its board. {} item(s) archived with it are still \
                             put away: {}.",
                            outcome.short_code,
                            outcome.still_archived_count,
                            outcome.still_archived_short_codes.join(", "),
                        )
                    };
                    on_restored.run(message);
                }
                Err(api::RestoreError::Blocked(missing)) => {
                    blocked.set(Some(missing));
                    busy.set(false);
                }
                Err(api::RestoreError::Api(e)) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    view! {
        <div class="kairos-item__restore">
            {move || can_restore.get().then(|| view! {
                <Group gap="sm">
                    {move || {
                        let disabled = busy.get();
                        view! {
                            <Button size="xs" disabled=disabled on_click=restore>
                                {if busy.get_untracked() { "Restoring…" } else { "Restore" }}
                            </Button>
                        }
                    }}
                    <Text size="xs" dimmed=true>
                        "Puts this item back on its board, where it was."
                    </Text>
                </Group>
            })}
            {move || blocked.get().map(|missing| {
                let names = missing.join(", ");
                view! {
                    <Alert title="It cannot go back yet" color=token::GOLD>
                        <Stack gap="xs">
                            <Text size="sm">
                                {format!(
                                    "This item was put away in a place that no longer exists — \
                                     {names} is gone. Kairos will not quietly re-home it, \
                                     because where it sat is part of what the record says.",
                                )}
                            </Text>
                            <Text size="sm" dimmed=true>
                                "Restore or recreate what it needs first, and the Restore \
                                 button will work."
                            </Text>
                        </Stack>
                    </Alert>
                }
            })}
            {move || error.get().map(|e| view! {
                <Alert title="Could not restore" color=token::BAD>
                    <Text size="sm">{api::error_text(&e)}</Text>
                </Alert>
            })}
        </div>
    }
}

/// The badge color of a document lifecycle state (KAIROS-T-0078):
/// published green, review gold, draft/archived muted — an editorial
/// palette, deliberately unlike the board-column pills.
fn lifecycle_color(state: &str) -> &'static str {
    match state {
        "published" => token::OK,
        "review" => token::GOLD,
        _ => token::MUTED,
    }
}

/// The lifecycle control (KAIROS-T-0078, documents only): a
/// free-transition select + Set button — an editorial label, never a
/// transition engine. Gating is server-side (`manage_documents` on the
/// authorization board); a 403 surfaces in the error slot.
#[component]
fn LifecyclePanel(
    #[prop(into)] code: String,
    current: String,
    /// The document is archived in the ADR-20 sense (put away). The
    /// editorial state is still worth SHOWING — it is part of what the
    /// record said — but setting it is a write, and writes resolve
    /// live-only.
    archived: bool,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let current = StoredValue::new(current);
    let value = RwSignal::new(current.get_value());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);
    let submit: Callback<()> = Callback::new(move |()| {
        let lifecycle = value.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::set_lifecycle(auth, &code.get_value(), &lifecycle).await {
                Ok(()) => on_moved.run(format!("Lifecycle set to {lifecycle}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    view! {
        <Panel title="Lifecycle" caption="editorial state — not board status">
            <Stack gap="sm">
                <Group gap="sm">
                    <Select label="State" value=value
                        options=vec!["draft".to_string(), "review".to_string(),
                                     "published".to_string(), "archived".to_string()]/>
                    {move || {
                        let unchanged = value.get() == current.get_value();
                        let disabled = archived || busy.get() || unchanged;
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Setting…" } else { "Set" }}
                            </Button>
                        }
                    }}
                </Group>
                {archived.then(|| view! {
                    <Text size="xs" dimmed=true>
                        "The editorial state cannot be changed while the document is put \
                         away — restore it first."
                    </Text>
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Could not set lifecycle" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </Stack>
        </Panel>
    }
}

/// The children rollup under the header (KAIROS-T-0080): a segmented bar
/// by column plus "N of M done" — or "N children" composition-only when
/// no involved board has done columns (never a misleading fraction).
/// Renders nothing for items without children; a failed read renders
/// nothing too (progress is enhancement data, not the page).
///
/// **Live-only, on purpose** (ADR-20 rule 5, KAIROS-T-0163): archived
/// children are not counted here even though the Relationships panel
/// below lists them. "2 children" there beside "1 of 1 done" here is the
/// correct reading, not a discrepancy to reconcile — containment is a
/// fact about the record; progress is a fact about live work.
#[component]
fn ChildrenProgressBar(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let progress = LocalResource::new(move || {
        let _ = auth.token();
        api::fetch_children_progress(auth, family, code.get_value())
    });
    view! {
        {move || match progress.get() {
            Some(Ok(p)) if p.total > 0 => {
                let total = p.total;
                let label = if p.has_done_columns {
                    format!("{} of {} done", p.done, p.total)
                } else {
                    format!("{} children", p.total)
                };
                view! {
                    <div class="kairos-progress">
                        <div class="kairos-progress__track">
                            {p.by_column.iter().map(|column| {
                                let width =
                                    (column.count as f64 / total as f64 * 100.0).max(3.0);
                                view! {
                                    <span
                                        class="kairos-progress__segment"
                                        class:kairos-progress__segment--done=column.is_done
                                        style=format!("width: {width:.1}%")
                                        title=format!("{}: {}", column.column_name, column.count)
                                    ></span>
                                }
                            }).collect_view()}
                        </div>
                        <Text dimmed=true size="xs">{label}</Text>
                    </div>
                }.into_any()
            }
            _ => ().into_any(),
        }}
    }
}

/// The type-specific facts as pills (each family's extra columns).
#[component]
fn TypeFacts(item: ItemDetail) -> impl IntoView {
    let mut facts: Vec<(String, &'static str)> = Vec::new();
    if let Some(task_type) = &item.task_type {
        facts.push((task_type.clone(), token::TEAL));
    }
    // KAIROS-T-0109: the repository the task is issued against (A-0019).
    if let Some(repository) = &item.repository {
        facts.push((format!("repo: {}", repository.slug), token::ICE));
    }
    // KAIROS-T-0077: the lane rides as a fact; Planned stays unmarked as
    // the default lane.
    if item.work_class.as_deref() == Some("support") {
        facts.push(("support lane".to_string(), token::GOLD));
    }
    if let Some(complexity) = &item.complexity {
        facts.push((format!("complexity: {complexity}"), token::TEAL));
    }
    if item.is_bucket == Some(true) {
        let kind = item.bucket_type.clone().unwrap_or_else(|| "bucket".into());
        facts.push((format!("bucket: {kind}"), token::GOLD));
    }
    if let Some(hypothesis) = &item.hypothesis
        && !hypothesis.is_empty()
    {
        facts.push(("has hypothesis".to_string(), token::VIOLET));
    }
    if let Some(decision_maker) = &item.decision_maker {
        facts.push((format!("decided by {decision_maker}"), token::VIOLET));
    }
    if let Some(decision_date) = &item.decision_date {
        facts.push((decision_date.clone(), token::VIOLET));
    }
    view! {
        {facts
            .into_iter()
            .map(|(label, color)| view! { <Pill color=color>{label}</Pill> })
            .collect_view()}
        <Text size="xs" dimmed=true>{format!("updated {}", item.updated_at)}</Text>
    }
}

/// Board/column display: documents never sit on boards; ADRs may not; the
/// rest always do. Board and column ids resolve to names via
/// `GET /api/boards/{id}`, linked to the T-0040 board view. On-board
/// items also get the move control here (KAIROS-T-0075) — the
/// keyboard-accessible transition path since cards are drag-only.
#[component]
fn BoardPanel(
    family: Family,
    #[prop(into)] code: String,
    /// The page's shared board read (`Ok(None)` = off-board).
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    column_id: Option<String>,
    /// The task's Planned/Support lane (KAIROS-T-0077) — `Some` enables
    /// the lane control.
    work_class: Option<String>,
    /// The task's bound repository slug (KAIROS-T-0109), if any.
    repository: Option<String>,
    /// The item is archived (ADR-20): its placement is a record of where
    /// it was put away, and every move endpoint resolves live-only, so the
    /// write controls are replaced by the reason they are gone.
    archived: bool,
    on_moved: Callback<String>,
) -> impl IntoView {
    let code = StoredValue::new(code);
    let work_class = StoredValue::new(work_class);
    let repository = StoredValue::new(repository);
    let column_id = StoredValue::new(column_id);
    view! {
        <Panel title="Board" caption="placement">
            {move || match board.get() {
                None => view! { <Loading label="Loading board…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                Some(Ok(None)) => {
                    let message = match family {
                        Family::Document => "Documents attach to a workflow item (supports edge), not a board.",
                        _ => "Off-board — no column, no transitions (org-admin writes only).",
                    };
                    view! { <Text size="sm" dimmed=true>{message}</Text> }.into_any()
                }
                Some(Ok(Some(board))) => {
                    // The column the item is IN — which, for an archived
                    // card, may be one that has since been removed from
                    // the board (KAIROS-T-0161). That is exactly the fact
                    // the column soft delete exists to keep, so it is
                    // named and marked rather than resolved to "unknown
                    // column"; `fetch_board` asks for removed columns for
                    // this one reason. Removed columns are never move
                    // targets — the transition list is live-only
                    // server-side, so `MoveControl` cannot offer one.
                    let column = column_id.with_value(|id| {
                        id.as_ref()
                            .and_then(|id| board.columns.iter().find(|c| &c.id == id))
                            .cloned()
                    });
                    let (column_label, column_color) = match &column {
                        Some(c) if c.removed_at.is_some() => (
                            format!("{} (column since removed)", c.name),
                            token::GOLD,
                        ),
                        Some(c) => (c.name.clone(), token::ICE),
                        None => ("unknown column".to_string(), token::MUTED),
                    };
                    let slug = board.slug.clone();
                    let name = board.name.clone();
                    view! {
                        <Stack gap="sm">
                            <Group justify="between">
                                <Anchor href=format!("/boards/{slug}")>{name}</Anchor>
                                <Pill color=column_color>{column_label}</Pill>
                            </Group>
                            {archived.then(|| view! {
                                <Text size="xs" dimmed=true>
                                    "Placement is frozen while this item is put away — it is \
                                     the record of where the work sat. Restore it to move it."
                                </Text>
                            })}
                            {(!archived && matches!(family, Family::Task)).then(|| view! {
                                <RepositoryControl
                                    code=code.get_value()
                                    board_slug=board.slug.clone()
                                    team_id=board.team_id.clone()
                                    current=repository.get_value()
                                    on_moved
                                />
                            })}
                            {(!archived && matches!(family, Family::Task)).then(|| view! {
                                <MoveBoardControl
                                    code=code.get_value()
                                    board_slug=board.slug.clone()
                                    team_id=board.team_id.clone()
                                    on_moved
                                />
                            })}
                            {(!archived).then(|| view! {
                                <MoveControl
                                    family
                                    code=code.get_value()
                                    board=board.clone()
                                    column_id=column_id.with_value(Clone::clone)
                                    work_class=work_class.get_value()
                                    on_moved
                                />
                            })}
                        </Stack>
                    }.into_any()
                }
            }}
        </Panel>
    }
}

/// One board power as a memo over the shell's shared whoami identity
/// (KAIROS-T-0072 client mirror; the server remains the authority). Shared
/// by [`MoveControl`] (`transition`) and [`RepositoryControl`] (`create`
/// tasks = `manage_tasks`) so both controls gate through one derivation
/// (KAIROS-T-0114). `false` until whoami has resolved.
fn board_power(
    board_slug: String,
    team_id: Option<String>,
    kind: Option<boards::data::EntityKind>,
    pick: fn(boards::BoardPowers) -> bool,
) -> Memo<bool> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let slug = StoredValue::new(board_slug);
    let team = StoredValue::new(team_id);
    Memo::new(move |_| {
        whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
            .is_some_and(|me| {
                slug.with_value(|slug| {
                    team.with_value(|team| {
                        pick(boards::board_powers(&me, slug, team.as_deref(), kind))
                    })
                })
            })
    })
}

use repositories::api::NO_REPOSITORY;

/// The task's repository binding (KAIROS-T-0109, A-0019): pick one of the
/// owning team's repositories (or none). The server enforces the repo →
/// team → board rule and `manage_tasks`; a refusal shows inline.
#[component]
fn RepositoryControl(
    code: String,
    /// The board's slug — for the client-side capability mirror.
    board_slug: String,
    /// The board's owning team (UUID) — the picker offers its repositories.
    team_id: Option<String>,
    /// The current binding (slug).
    current: Option<String>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let current = StoredValue::new(current);
    let value = RwSignal::new(
        current
            .get_value()
            .unwrap_or_else(|| NO_REPOSITORY.to_string()),
    );
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    // Same `manage_tasks` mirror as the board's create affordance.
    let can_bind = board_power(
        board_slug,
        team_id.clone(),
        Some(boards::data::EntityKind::Task),
        |powers| powers.create,
    );
    let repos = LocalResource::new(move || {
        let _ = auth.token();
        let team = team_id.clone();
        async move { repositories::api::list_repositories(auth, team.as_deref()).await }
    });
    // The binding may name a repository that has since been re-homed to
    // another team: it is not among this team's options, so the control
    // says so instead of silently disabling the button, and the picker
    // starts at "(none)" so the first submit is a deliberate re-bind or
    // clear. (Signal write in an Effect, never inside a tracked render.)
    let elsewhere = Memo::new(move |_| {
        let bound = current.get_value()?;
        let list = repos.get()?.ok()?;
        (!list.iter().any(|r| r.slug == bound)).then_some(bound)
    });
    Effect::new(move |_| {
        if elsewhere.get().is_some() {
            value.set(NO_REPOSITORY.to_string());
        }
    });
    let submit: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        let repository = (chosen != NO_REPOSITORY).then_some(chosen);
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match repositories::api::set_repository(auth, &code.get_value(), repository.as_deref())
                .await
            {
                Ok(()) => on_moved.run(match repository {
                    Some(slug) => format!("Repository set to {slug}."),
                    None => "Repository cleared.".to_string(),
                }),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    view! {
        {move || can_bind.get().then(|| view! {
        <div class="kairos-item__repository" data-testid="repository-control">
            {move || match repos.get() {
                None => view! { <Text size="xs" dimmed=true>"Loading repositories…"</Text> }.into_any(),
                Some(Err(_)) => view! { <Text size="xs" dimmed=true>"Repositories unavailable."</Text> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <Text size="xs" dimmed=true>"No repositories registered for this team."</Text>
                }.into_any(),
                Some(Ok(list)) => {
                    // Options are `(value, label)`: the slug stays the value
                    // (what the server takes) and the label adds the forge
                    // full name next to it.
                    let options: Vec<(String, String)> = std::iter::once((
                        NO_REPOSITORY.to_string(),
                        NO_REPOSITORY.to_string(),
                    ))
                    .chain(list.iter().map(|r| {
                        let label = if r.repo_full_name.is_empty() {
                            r.slug.clone()
                        } else {
                            format!("{} · {}", r.slug, r.repo_full_name)
                        };
                        (r.slug.clone(), label)
                    }))
                    .collect();
                    let bound = current
                        .get_value()
                        .unwrap_or_else(|| NO_REPOSITORY.to_string());
                    view! {
                        <Stack gap="xs">
                            {move || elsewhere.get().map(|slug| view! {
                                <Text size="xs" dimmed=true>
                                    {format!("Bound to {slug}, which now belongs to another team — pick one of this team's repositories, or clear it.")}
                                </Text>
                            })}
                            <Group gap="sm">
                                <div class="cl-field">
                                    <label class="cl-field__label">"Repository"</label>
                                    <select
                                        class="cl-input cl-select"
                                        prop:value=move || value.get()
                                        on:change=move |e| value.set(event_target_value(&e))
                                    >
                                        {options.into_iter().map(|(slug, label)| view! {
                                            <option value=slug>{label}</option>
                                        }).collect_view()}
                                    </select>
                                </div>
                                {move || {
                                    // A stale (re-homed) binding is never
                                    // "unchanged": clearing it is a real write.
                                    let unchanged =
                                        elsewhere.with(Option::is_none) && value.get() == bound;
                                    let disabled = busy.get() || unchanged;
                                    view! {
                                        <Button size="xs" disabled=disabled on_click=submit>
                                            {if busy.get_untracked() { "Setting…" } else { "Set repository" }}
                                        </Button>
                                    }
                                }}
                            </Group>
                        </Stack>
                    }.into_any()
                }
            }}
            {move || error.get().map(|e| view! {
                <Alert title="Could not set repository" color=token::BAD>
                    <Text size="sm">{api::error_text(&e)}</Text>
                </Alert>
            })}
        </div>
        })}
    }
}

/// The board picker's "stay put" option — the default, so a board move is
/// always a deliberate second act (the column picker next to it defaults
/// to a real target, but a board move re-homes the card's TEAM).
const THIS_BOARD: &str = "(this board)";

/// Move a task to another DELIVERY board (KAIROS-I-0012 D2): a team
/// disbands and its live work goes to another team's board. The task
/// lands in the target's entry column and follows the target's team —
/// the server decides both; this panel just refetches afterwards.
///
/// The rule is two-sided (`manage_tasks` on the source board AND the
/// target, org admins bypass), so the control renders only when the
/// shared powers mirror grants it HERE, and offers only the targets
/// [`boards::movable_delivery_boards`] returns — nothing the server would
/// 403. Everything it cannot know (the T-0104 repository rule above all)
/// comes back as a 422 and is shown inline.
#[component]
fn MoveBoardControl(
    code: String,
    /// The board the task sits on now — the source half of the rule, and
    /// the one board the picker never offers.
    board_slug: String,
    /// That board's owning team (UUID) — for the capability mirror.
    team_id: Option<String>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let here = StoredValue::new(board_slug.clone());
    let value = RwSignal::new(THIS_BOARD.to_string());
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    // Source half: the same `manage_tasks` mirror as the board's create
    // affordance and the repository picker.
    let can_move = board_power(
        board_slug,
        team_id,
        Some(boards::data::EntityKind::Task),
        |powers| powers.create,
    );
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let all_boards = LocalResource::new(move || {
        let _ = auth.token();
        boards::data::list_boards(auth)
    });
    // Target half, from the ONE derivation the board view uses.
    let targets: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(Ok(list))) = (me, all_boards.get()) else {
            return Vec::new();
        };
        here.with_value(|here| boards::movable_delivery_boards(&me, &list, here))
    });

    let submit: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        if chosen == THIS_BOARD {
            return;
        }
        // The label the notice names — the slug is the wire value.
        let label = targets
            .with_untracked(|targets| {
                targets
                    .iter()
                    .find(|(slug, _)| *slug == chosen)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_else(|| chosen.clone());
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::move_task(auth, &code.get_value(), &chosen).await {
                // Success refetches the whole detail (on_moved bumps the
                // reload), which re-renders this panel with the new board
                // and its entry column — no busy reset, this view is gone.
                Ok(_) => on_moved.run(format!("Moved to {label}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    view! {
        {move || (can_move.get() && !targets.with(Vec::is_empty)).then(|| view! {
            <div class="kairos-item__move-board" data-testid="move-board">
                <Group gap="sm">
                    <div class="cl-field">
                        <label class="cl-field__label">"Board"</label>
                        <select
                            class="cl-input cl-select"
                            prop:value=move || value.get()
                            on:change=move |e| value.set(event_target_value(&e))
                        >
                            <option value=THIS_BOARD>{THIS_BOARD}</option>
                            {move || targets.get().into_iter().map(|(slug, name)| view! {
                                <option value=slug>{name}</option>
                            }).collect_view()}
                        </select>
                    </div>
                    {move || {
                        let disabled = busy.get() || value.get() == THIS_BOARD;
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Moving…" } else { "Move board" }}
                            </Button>
                        }
                    }}
                </Group>
                {move || error.get().map(|e| view! {
                    <Alert title="Could not move to that board" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </div>
        })}
    }
}

/// The keyboard-accessible transition path (KAIROS-T-0075) plus the lane
/// control (KAIROS-T-0077): cards are drag-only, so moving an item — or
/// switching a task's Planned/Support lane — without a pointer happens
/// here. Renders only when there is a legal move to offer AND the user
/// holds `transition_items` on this board — the same gate as the drag
/// affordance, via the shared powers mirror.
#[component]
fn MoveControl(
    family: Family,
    code: String,
    board: api::BoardInfo,
    column_id: Option<String>,
    work_class: Option<String>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    // Documents never transition; the other four families map onto the
    // board-item kinds the transition endpoint serves.
    let kind = match family {
        Family::Strategy => Some(boards::data::EntityKind::Strategy),
        Family::Initiative => Some(boards::data::EntityKind::Initiative),
        Family::Task => Some(boards::data::EntityKind::Task),
        Family::Adr => Some(boards::data::EntityKind::Adr),
        Family::Document => None,
    };
    // ONLY the targets this board's transitions allow from the current
    // column — invalid moves are never offered (A-0002).
    let targets: Vec<(String, String)> = column_id
        .as_ref()
        .map(|from| {
            board
                .transitions
                .iter()
                .filter(|t| &t.from_column_id == from)
                .filter_map(|t| {
                    board
                        .columns
                        .iter()
                        .find(|c| c.id == t.to_column_id)
                        .map(|c| (c.id.clone(), c.name.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let has_targets = !targets.is_empty();
    // The lane row exists for tasks only (they carry work_class) — and
    // keeps a task in a dead-end column movable across lanes.
    let lane_row = matches!(family, Family::Task).then_some(()).and(work_class);
    let (Some(kind), false) = (kind, !has_targets && lane_row.is_none()) else {
        return ().into_any();
    };

    // Same client-side capability mirror as the board's drag affordance
    // (KAIROS-T-0072); the server remains the authority.
    let can_move = board_power(board.slug.clone(), board.team_id.clone(), None, |powers| {
        powers.transition
    });

    let option_names = StoredValue::new(
        targets
            .iter()
            .map(|(_, name)| name.clone())
            .collect::<Vec<String>>(),
    );
    let targets = StoredValue::new(targets);
    let target =
        RwSignal::new(option_names.with_value(|names| names.first().cloned().unwrap_or_default()));
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    let submit: Callback<()> = Callback::new(move |()| {
        let name = target.get_untracked();
        let Some((to_column_id, column_name)) =
            targets.with_value(|t| t.iter().find(|(_, n)| *n == name).cloned())
        else {
            return;
        };
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match boards::data::transition(auth, kind, &code.get_value(), &to_column_id).await {
                // Success refetches the whole detail (on_moved bumps the
                // reload), which drops this instance — no busy reset.
                Ok(()) => on_moved.run(format!("Moved to {column_name}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    // The lane row (KAIROS-T-0077): tasks switch Planned/Support here —
    // the keyboard counterpart of a cross-lane drag.
    let has_lane = lane_row.is_some();
    let current_lane = StoredValue::new(lane_row.unwrap_or_default());
    let lane_value = RwSignal::new(current_lane.get_value());
    let submit_lane: Callback<()> = Callback::new(move |()| {
        let value = lane_value.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match boards::data::set_work_class(auth, &code.get_value(), &value).await {
                Ok(()) => on_moved.run(format!("Lane set to {value}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    view! {
        {move || can_move.get().then(|| view! {
            <div class="kairos-item__move">
                {has_targets.then(|| view! {
                    <Group gap="sm">
                        <Select label="Move to" value=target
                            options=option_names.get_value()/>
                        {move || {
                            let disabled = busy.get();
                            view! {
                                <Button size="xs" disabled=disabled on_click=submit>
                                    {if busy.get_untracked() { "Moving…" } else { "Move" }}
                                </Button>
                            }
                        }}
                    </Group>
                })}
                {has_lane.then(|| view! {
                    <Group gap="sm">
                        <Select label="Lane" value=lane_value
                            options=vec!["planned".to_string(), "support".to_string()]/>
                        {move || {
                            let unchanged =
                                lane_value.get() == current_lane.get_value();
                            let disabled = busy.get() || unchanged;
                            view! {
                                <Button size="xs" disabled=disabled on_click=submit_lane>
                                    {if busy.get_untracked() { "Setting…" } else { "Set lane" }}
                                </Button>
                            }
                        }}
                    </Group>
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Could not move" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </div>
        })}
    }
    .into_any()
}

/// Edge proposals waiting on this item (KAIROS-A-0021 rule 6, KAIROS-T-0192).
///
/// On the item, not in a queue. A review inbox is a second product with its own
/// notifications and its own backlog of things nobody opens; a proposal shown
/// where the work already is gets seen by the person already looking at it.
///
/// The panel renders nothing at all when there is nothing pending. A permanently
/// visible "no suggestions" box trains people to stop looking at the place
/// suggestions appear.
#[component]
fn EdgeProposalsPanel(#[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let proposals = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_edge_proposals(auth, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    // Confirm and reject differ only in which call they make, so they share a
    // path: the interesting part is that BOTH refresh, because confirming
    // changes the Relationships panel above and a stale page would hide the
    // thing the click just did.
    let decide = move |id: String, confirming: bool| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = if confirming {
                api::confirm_edge_proposal(auth, id).await
            } else {
                api::reject_edge_proposal(auth, id).await
            };
            if let Err(e) = result {
                error.set(Some(e));
            }
            busy.set(false);
            reload.update(|n| *n += 1);
        });
    };

    view! {
        {move || match proposals.get() {
            None => ().into_any(),
            Some(Err(e)) => view! { <ErrorState error=e on_retry=retry/> }.into_any(),
            Some(Ok(list)) if list.is_empty() => ().into_any(),
            Some(Ok(list)) => {
                let this = code.get_value();
                view! {
                    <Panel title="Suggested links" caption="proposed by an agent">
                        <Stack gap="sm">
                            <Text dimmed=true size="xs">
                                "An agent thinks these belong together. Nothing has been \
                                 changed — confirming creates the link, rejecting is kept \
                                 as a record that the suggestion was wrong."
                            </Text>
                            {move || error.get().map(|e| view! {
                                <Alert title="Could not decide" color=token::BAD>
                                    <Text size="sm">{api::error_text(&e)}</Text>
                                </Alert>
                            })}
                            {list.into_iter().map(|p| {
                                let id_confirm = p.id.clone();
                                let id_reject = p.id.clone();
                                // Say it from the reader's point of view: they are
                                // looking at one of these two items already.
                                let other = if p.source == this { p.target.clone() } else { p.source.clone() };
                                let direction = if p.source == this {
                                    format!("this {} {}", p.relationship, other)
                                } else {
                                    format!("{} {} this", other, p.relationship)
                                };
                                view! {
                                    <Stack gap="xs">
                                        <Group gap="xs">
                                            <Pill color=token::VIOLET>{p.claim.clone()}</Pill>
                                            <Anchor href=format!("/items/{other}")>{other.clone()}</Anchor>
                                            <Text size="xs" dimmed=true>{direction}</Text>
                                        </Group>
                                        <Text size="xs">{p.why.clone()}</Text>
                                        <Group gap="xs">
                                            <Button
                                                variant="primary"
                                                size="xs"
                                                disabled=busy.get()
                                                on_click=Callback::new(move |_| decide(id_confirm.clone(), true))
                                            >
                                                "Confirm"
                                            </Button>
                                            <Button
                                                variant="default"
                                                size="xs"
                                                disabled=busy.get()
                                                on_click=Callback::new(move |_| decide(id_reject.clone(), false))
                                            >
                                                "Reject"
                                            </Button>
                                        </Group>
                                    </Stack>
                                }
                            }).collect_view()}
                        </Stack>
                    </Panel>
                }.into_any()
            }
        }}
    }
}

/// Relationships summary: both directions, grouped, every neighbor linked
/// to its own detail route. The full graph explorer is T-0042's.
///
/// Archived-INCLUSIVE since KAIROS-T-0158, with every put-away neighbour
/// marked (KAIROS-T-0163) — deliberately unlike [`ChildrenProgressBar`]
/// above it, which stays live-only per ADR-20 rule 5.
#[component]
fn RelationshipsPanel(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let relationships = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_relationships(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    view! {
        <Panel title="Relationships" caption="summary">
            {move || match relationships.get() {
                None => view! { <Loading label="Loading relationships…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(relationships))
                    if relationships.outgoing.is_empty() && relationships.incoming.is_empty() =>
                {
                    view! { <Empty message="No relationships yet — link items from the graph explorer (KAIROS-T-0042)."/> }
                        .into_any()
                }
                Some(Ok(relationships)) => {
                    // ADR-20 rule 5, and the difference worth making
                    // legible rather than reconciling away: this panel
                    // is archived-INCLUSIVE, while the children rollup
                    // at the top of the page counts live work only. So
                    // "2 children" here beside "1 of 1 done" up there is
                    // CORRECT, and the note says why in one line.
                    let any_put_away = relationships
                        .outgoing
                        .iter()
                        .chain(relationships.incoming.iter())
                        .flat_map(|group| group.items.iter())
                        .any(|item| item.archived_at.is_some());
                    view! {
                        <Stack gap="sm">
                            {relationships.outgoing.into_iter().map(|group| view! {
                                <RelationshipGroupView group direction="outgoing"/>
                            }).collect_view()}
                            {relationships.incoming.into_iter().map(|group| view! {
                                <RelationshipGroupView group direction="incoming"/>
                            }).collect_view()}
                            {any_put_away.then(|| view! {
                                <Text dimmed=true size="xs">
                                    "Put-away neighbours are listed: containment is a fact \
                                     about the record. The progress bar above counts live \
                                     work only — progress is a fact about live work."
                                </Text>
                            })}
                            <Anchor href=format!("/search/relationships/{}", code.get_value())>
                                "Open the graph explorer"
                            </Anchor>
                        </Stack>
                    }.into_any()
                }
            }}
        </Panel>
    }
}

/// The accent for a forge link's state (KAIROS-T-0100). Red stays
/// reserved for violated/at-risk — a closed pull request is a normal
/// outcome, not an error.
fn link_state_color(state: &str) -> &'static str {
    match state {
        "open" => token::ICE,
        "merged" => token::VIOLET,
        "draft" => token::MUTED,
        _ => token::MUTED,
    }
}

/// Branches and pull/merge requests for this item (KAIROS-T-0100).
/// Renders NOTHING when there are no links — an always-present empty
/// panel is exactly what the T-0089 explorer rework removed.
#[component]
fn DevelopmentPanel(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let links = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_links(auth, family, code.get_value())
    });

    // Webhook ingestion is the whole point of this panel, and it happens
    // without the user doing anything — so subscribe to the tenant stream
    // and refetch when an event names THIS item (the T-0074 pattern; the
    // item page has no other live subscription).
    let live_guard: StoredValue<Option<crate::pages::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(None);
    let guard = crate::pages::boards::live::subscribe_all_events(auth, move |event_code| {
        let concerns_me = event_code.is_none_or(|event_code| event_code == code.get_value());
        if concerns_me {
            reload.update(|n| *n += 1);
        }
    });
    live_guard.set_value(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    view! {
        {move || match links.get() {
            Some(Ok(links)) if !links.is_empty() => {
                let rows = links
                    .into_iter()
                    .map(|link| {
                        let label = if link.kind == "pull_request" {
                            format!("#{} {}", link.external_id, link.title)
                        } else {
                            link.title.clone()
                        };
                        let repo = format!("{} · {}", link.forge, link.repo_full_name);
                        view! {
                            <Group justify="between" wrap=true>
                                <Group gap="sm" wrap=true>
                                    <Pill color=link_state_color(&link.state)>
                                        {link.state.clone()}
                                    </Pill>
                                    // Leaves the app: open in a new tab and
                                    // sever the opener reference.
                                    <a
                                        class="cl-anchor"
                                        href=link.url.clone()
                                        target="_blank"
                                        rel="noopener noreferrer"
                                    >
                                        {label}
                                    </a>
                                </Group>
                                <Text mono=true dimmed=true size="xs">{repo}</Text>
                            </Group>
                        }
                    })
                    .collect_view();
                view! {
                    <Panel title="Development" caption="branches and pull requests">
                        <Stack gap="xs">{rows}</Stack>
                    </Panel>
                }
                .into_any()
            }
            // No links, still loading, or the read failed: render nothing.
            // This panel is additive context, never the page's job.
            _ => ().into_any(),
        }}
    }
}

/// The human name of one relationship group, read from THIS item's side —
/// an outgoing `parent` edge points at children, so labeling the group
/// "parent" read exactly backwards (UAT review nit on T-0075/T-0080
/// wave). Vocabulary matches the graph explorer's panels; direction is in
/// the words, not an arrow glyph. Pure, host-tested.
fn relationship_label(relationship: &str, outgoing: bool) -> String {
    match (relationship, outgoing) {
        // source=parent, target=child (A-0001 matrix).
        ("parent", true) => "children".to_string(),
        ("parent", false) => "parent".to_string(),
        ("blocks", true) => "blocks".to_string(),
        ("blocks", false) => "blocked by".to_string(),
        // source=workflow item, target=document/ADR (T-0018 contract).
        ("supports", true) => "supporting material".to_string(),
        ("supports", false) => "supports".to_string(),
        // source=document/ADR, target=workflow item.
        ("informs", true) => "informs".to_string(),
        ("informs", false) => "informed by".to_string(),
        ("supersedes", true) => "supersedes".to_string(),
        ("supersedes", false) => "superseded by".to_string(),
        // Unknown types stay honest about their direction.
        (other, true) => format!("{other} (outgoing)"),
        (other, false) => format!("{other} (incoming)"),
    }
}

/// One direction of one relationship type, neighbors linked.
#[component]
fn RelationshipGroupView(
    group: RelationshipGroup,
    #[prop(into)] direction: String,
) -> impl IntoView {
    let label = relationship_label(&group.relationship, direction == "outgoing");
    view! {
        <div class="kairos-relationships__group">
            <Group gap="sm">
                <Text size="xs" dimmed=true>{label}</Text>
            </Group>
            {group.items.into_iter().map(|item| {
                // KAIROS-T-0158/T-0163: the neighbour lists include
                // archived ends on purpose — an item's edges describe
                // what it contains and depends on, and dropping one
                // would shrink that answer silently. So the row SAYS so.
                // "put away", never a bare "archived": a document
                // neighbour can be editorially `lifecycle: archived`
                // (KAIROS-T-0078) and perfectly live.
                let put_away = item.archived_at.as_deref().map(|when| {
                    let title = format!("Put away on {}", put_away_when(when));
                    view! {
                        <span class="kairos-archived-badge" title=title>
                            <Pill color=token::GOLD>"put away"</Pill>
                        </span>
                    }
                });
                view! {
                    <Group gap="sm" justify="between">
                        <Anchor href=format!("/items/{}", item.short_code)>
                            {format!("{} — {}", item.short_code, item.title)}
                        </Anchor>
                        <Group gap="sm">
                            {put_away}
                            <Pill color=token::VIOLET>{item.entity_type.clone()}</Pill>
                        </Group>
                    </Group>
                }
            }).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0124 #2: the tab anchors always carry the code — an empty
    /// route param (first frame after navigation) yields no anchors at
    /// all rather than `/items/?view=graph`.
    #[test]
    fn tab_hrefs_never_emit_an_empty_code() {
        assert_eq!(tab_hrefs(""), None);
        let (details, graph) = tab_hrefs("DEMO-T-0007").expect("resolved code");
        assert_eq!(details, "/items/DEMO-T-0007");
        assert_eq!(graph, "/items/DEMO-T-0007?view=graph");
    }

    /// The banner says WHEN, in something a person reads (KAIROS-T-0164):
    /// the wire's microsecond RFC 3339 is not it. Junk passes through
    /// rather than being swallowed.
    #[test]
    fn put_away_when_reads_as_a_moment() {
        assert_eq!(
            put_away_when("2026-09-23T11:30:07.479107Z"),
            "2026-09-23 11:30 UTC"
        );
        assert_eq!(
            put_away_when("2026-09-23T11:30:07+00:00"),
            "2026-09-23 11:30 UTC"
        );
        assert_eq!(put_away_when("not a timestamp"), "not a timestamp");
    }

    /// Restoring asks for the same `manage_<family>` the archive asked
    /// for — no new capability (KAIROS-T-0160).
    #[test]
    fn restore_asks_for_the_archive_capability() {
        assert_eq!(manage_capability(Family::Strategy), "manage_strategies");
        assert_eq!(manage_capability(Family::Initiative), "manage_initiatives");
        assert_eq!(manage_capability(Family::Task), "manage_tasks");
        assert_eq!(manage_capability(Family::Document), "manage_documents");
        assert_eq!(manage_capability(Family::Adr), "manage_adrs");
    }

    /// The summary reads from THIS item's side: an outgoing parent edge
    /// lists children; passive voice marks the incoming direction.
    #[test]
    fn relationship_labels_read_from_the_items_side() {
        assert_eq!(relationship_label("parent", true), "children");
        assert_eq!(relationship_label("parent", false), "parent");
        assert_eq!(relationship_label("blocks", false), "blocked by");
        assert_eq!(relationship_label("supports", true), "supporting material");
        assert_eq!(relationship_label("informs", false), "informed by");
        assert_eq!(relationship_label("supersedes", false), "superseded by");
        assert_eq!(relationship_label("mystery", true), "mystery (outgoing)");
    }
}
