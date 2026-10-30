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
//! - the owner board of a document, and the repositories that a document
//!   or an ADR impacts ([`owner`], COLLIERY-T-0269);
//! - relationships summary (linked; the full explorer is T-0042's
//!   `/search/relationships/:code`) and the history link (T-0044's
//!   `/activity/history/:code`); add and remove of an edge, and the
//!   Parent picker, for a person who may edit an end ([`relations`],
//!   KAIROS-T-0580);
//! - create-from-template ([`create_doc`]) and the archive (a soft
//!   delete) with cascade warning ([`delete`], A-0001);
//! - for a task: Cancel with a reason, and Delete for good ([`end`],
//!   KAIROS-T-0362); Reopen for a done task ([`end`], KAIROS-T-0648);
//! - for a task: the claim, with Hand off and Release ([`claim`],
//!   KAIROS-T-0369);
//! - who may edit ([`access`], KAIROS-T-0581): the editor and the metadata
//!   panel are read-only, with the reason, for a person who may not edit;
//!   Archive shows only to a person who may archive;
//! - the page stays mounted on a save and a panel action (KAIROS-T-0618):
//!   the new item goes into a signal, and no panel reads again. The lists
//!   of repositories and boards load when their control opens ([`picker`]);
//! - the page is live (KAIROS-T-0578, [`live`]): one socket; an event of
//!   the item reads it, and a change shows the banner "Changed on the
//!   server" with Reload. The page does not mount again, and the editor
//!   keeps the draft.

mod access;
pub(crate) mod api;
mod claim;
pub(crate) mod code_picker;
mod create_doc;
mod delete;
mod editor;
mod end;
mod external;
mod item_owner;
mod live;
pub(crate) use kairos_markdown as markdown;
mod metadata;
mod notes;
mod owner;
mod picker;
mod relations;
mod sections;
mod state;
mod teams;
mod watch;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Switch, Text,
};
use aurora_dark::data::{RelativeTime, Segment, SegmentedBar};
use aurora_dark::frame::{TabItem, Tabs, use_toaster};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::boards;
use super::copy_link;
use super::editor::ServerCopy;
use super::repositories;
use crate::auth::use_auth;
use api::{Family, ItemDetail, RelationshipGroup};
use claim::ClaimPanel;
use create_doc::CreateDocumentDialog;
use delete::DeleteDialog;
use editor::ContentEditor;
use end::{CancelDialog, CancelledBanner, PurgeDialog, ReopenDialog};
use external::ExternalRefsPanel;
use item_owner::OwnerPanel;
use metadata::MetadataPanel;
use notes::NotesPanel;
use owner::{ImpactsPanel, OwnerBoardPanel};
use picker::{Picker, load_on_open, picker_state};
use state::StatePanel;
use teams::TeamsPanel;
use watch::WatchPanel;
// The dialog of a repository gives a document to a board with the rule of
// the owner panel (COLLIERY-T-0269).
pub(crate) use owner::owner_board_targets;

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
            // COLLIERY-T-3100: the page came here from a retired code (see
            // `ItemDetailView`). The notice names the two codes.
            let retired_notice = query
                .read()
                .get("retired")
                .filter(|retired| Family::of_short_code(retired).is_some() && *retired != code)
                .map(|retired| {
                    let text = format!(
                        "The code {retired} is retired. The current code of this item is {code}."
                    );
                    view! {
                        <div class="kairos-retired-code" data-testid="retired-code-notice">
                            <Alert title="Retired code" color=token::GOLD>
                                <Text size="sm">{text}</Text>
                            </Alert>
                        </div>
                    }
                });
            // KAIROS-T-0124 #2: the param can be empty for a frame while
            // the router settles — render nothing clickable until it is
            // resolved, so no tab is ever built from an empty code.
            let Some((details_href, graph_href)) = tab_hrefs(&code) else {
                return view! { <Loading label="Loading item…"/> }.into_any();
            };
            match Family::of_short_code(&code) {
                Some(family) => {
                    // COLLIERY-T-1836: Aurora route `Tabs` (links with
                    // role="tab"); the query param selects the tab.
                    let tab_value =
                        RwSignal::new(if graph_mode { "graph" } else { "details" }.to_string());
                    let tabs = view! {
                        <Tabs
                            tabs=vec![
                                TabItem::new("details", "Details").href(details_href),
                                TabItem::new("graph", "Graph").href(graph_href),
                            ]
                            value=tab_value
                            label="Item views"
                        />
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
                            {retired_notice}
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

/// The first read of the item, and the four async view states. The read
/// runs again only on Retry or for a new session (KAIROS-T-0618): a save or
/// a panel action gives [`ItemLoaded`] the new item, and the page stays
/// mounted. A new code (a board move with a rename) is a new route, so a
/// new page.
#[component]
fn ItemDetailView(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);

    let detail = LocalResource::new(move || {
        auth.track_session();
        let _ = reload.get();
        api::fetch_item(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    // COLLIERY-T-3100: the server follows a retired code to the item, and
    // the item has its current code. Go to the page of the current code
    // (the history entry is replaced), with the retired code in `?retired=`
    // for the notice of `ItemPage`.
    let navigate = leptos_router::hooks::use_navigate();
    Effect::new(move |_| {
        if let Some(Ok(item)) = detail.get() {
            let asked = code.get_value();
            if item.short_code != asked {
                navigate(
                    &format!("/items/{}?retired={asked}", item.short_code),
                    leptos_router::NavigateOptions {
                        replace: true,
                        ..Default::default()
                    },
                );
            }
        }
    });

    view! {
        {move || match detail.get() {
            None => view! { <Loading label="Loading item…"/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
            Some(Ok(item)) => view! { <ItemLoaded item family/> }.into_any(),
        }}
    }
}

/// Does the page take a copy of the item that a read or a write gave
/// (KAIROS-T-0618)? Not when it is older than the copy on the page. A copy
/// of the same version is taken: a claim does not change the version.
/// Pure, host-tested.
fn takes_copy(shown_version: i32, incoming_version: i32) -> bool {
    incoming_version >= shown_version
}

/// The loaded page: header + actions, the editor column, and the facts /
/// metadata / relationships column.
///
/// KAIROS-T-0618: the page keeps the item in a signal and stays mounted
/// while it shows the item (the code is the key). A save gives the item
/// that the PATCH returned; a panel action reads the item once. The panels
/// take the parts that they show as signals, so no panel reads again, and
/// the socket of the Development panel stays open. `refresh` is the one
/// path for a new copy: the live item page (KAIROS-T-0578) can call it for
/// an event of this item.
#[component]
fn ItemLoaded(item: ItemDetail, family: Family) -> impl IntoView {
    let auth = use_auth();
    // Page-level transient notice (COLLIERY-T-1836: an Aurora toast).
    let toaster = use_toaster();
    let create_open = RwSignal::new(false);
    let delete_open = RwSignal::new(false);
    let cancel_open = RwSignal::new(false);
    let purge_open = RwSignal::new(false);
    let reopen_open = RwSignal::new(false);

    // The facts that do not change while the page shows this code.
    let code = StoredValue::new(item.short_code.clone());
    let item_id = item.id.clone();
    let created_by = item.created_by.clone();
    let history_href = format!("/activity/history/{}", item.short_code);
    // KAIROS-T-0408: the page of a document or an ADR in the docs sites. The
    // org path is enough: the server sends a team's item on to its team site,
    // and answers 404 when the item has no page.
    let site_href = matches!(family, Family::Document | Family::Adr)
        .then(|| {
            let section = if family == Family::Adr {
                "adrs"
            } else {
                "docs"
            };
            crate::app::docs_sites_path().map(|org| format!("{org}{section}/{}/", item.short_code))
        })
        .flatten();
    let site_href = StoredValue::new(site_href);
    let (initial_title, initial_content, initial_version) =
        (item.title.clone(), item.content.clone(), item.version);

    let current = RwSignal::new(item);
    // Show a new copy, unless it is older than the copy on the page. The
    // `try_` reads: the copy can come after the page is gone.
    let show = move |mut next: ItemDetail| {
        let Some((shown, notes)) =
            current.try_with_untracked(|item| (item.version, item.notes.clone()))
        else {
            return;
        };
        if takes_copy(shown, next.version) {
            // KAIROS-T-0630: the answer of a write has no notes. The page
            // keeps the notes that it shows.
            if next.notes.is_none() {
                next.notes = notes;
            }
            let _ = current.try_set(next);
        }
    };
    // One read of the item. Only the answer of the last read counts.
    let reads = StoredValue::new(0u32);
    let refresh = Callback::new(move |()| {
        let mine = reads.get_value().wrapping_add(1);
        reads.set_value(mine);
        leptos::task::spawn_local(async move {
            let Some(code) = code.try_get_value() else {
                return;
            };
            match api::fetch_item(auth, family, code).await {
                Ok(next) if reads.try_get_value() == Some(mine) => show(next),
                Ok(_) => {}
                Err(e) => {
                    toaster.error(format!(
                        "Kairos did not read the item again: {}",
                        api::error_text(&e)
                    ));
                }
            }
        });
    });
    let on_saved = Callback::new(move |item: ItemDetail| {
        toaster.success(format!("Saved — the item is now at v{}.", item.version));
        show(item);
    });
    // The success path of each panel action (KAIROS-T-0075/T-0077): the
    // item changed on the server; show the message and read the item.
    let on_moved = Callback::new(move |message: String| {
        toaster.success(message);
        refresh.run(());
    });

    // The parts of the item, each a memo: a part that did not change
    // notifies nothing.
    let title = Memo::new(move |_| current.with(|item| item.title.clone()));
    let version = Memo::new(move |_| current.with(|item| item.version));
    let board_id = Memo::new(move |_| current.with(|item| item.board_id.clone()));
    let column_id = Memo::new(move |_| current.with(|item| item.column_id.clone()));
    let work_class = Memo::new(move |_| current.with(|item| item.work_class.clone()));
    let complexity = Memo::new(move |_| current.with(|item| item.complexity.clone()));
    let repository =
        Memo::new(move |_| current.with(|item| item.repository.as_ref().map(|r| r.slug.clone())));
    let claim = Memo::new(move |_| current.with(|item| item.claim.clone()));
    let state = Memo::new(move |_| current.with(|item| item.state.clone()));
    let notes = Memo::new(move |_| current.with(|item| item.notes.clone().unwrap_or_default()));
    let owner = Memo::new(move |_| current.with(|item| item.owner.clone()));
    let impacts = Memo::new(move |_| current.with(|item| item.impacts.clone()));
    let lifecycle = Memo::new(move |_| current.with(|item| item.lifecycle.clone()));
    // KAIROS-T-0731: the mark of the typed sections of a document.
    let section_check = Memo::new(move |_| current.with(|item| item.section_check.clone()));
    let cancellation = Memo::new(move |_| current.with(|item| item.cancellation.clone()));
    let cancelled = Memo::new(move |_| cancellation.with(Option::is_some));
    // KAIROS-T-0164 / ADR-20: `archived_at` is the "put away" state — the
    // item is hidden by default and read-only, NOT gone. Deliberately not
    // the same thing as `lifecycle == "archived"`, which is a document's
    // EDITORIAL state (KAIROS-T-0078) and says nothing about visibility;
    // both can be true at once on this page, so the copy keeps them apart:
    // the editorial badge always reads "lifecycle: …", the put-away state
    // always reads "put away".
    let archived_at = Memo::new(move |_| current.with(|item| item.archived_at.clone()));
    let archived = Memo::new(move |_| archived_at.with(Option::is_some));
    let editorial_archived =
        Memo::new(move |_| lifecycle.with(|l| l.as_deref() == Some("archived")));
    let server = Memo::new(move |_| {
        current.with(|item| ServerCopy {
            title: item.title.clone(),
            content: item.content.clone(),
            version: item.version,
        })
    });

    // KAIROS-T-0578: the live page. One socket; an event of this item reads
    // what it changed, and the page compares the read with what it shows.
    // `pending`: the last copy that a live read gave. `metadata_seen`: the
    // values that the metadata panel shows; `metadata_read`: the last
    // values that a live read gave.
    let pending = RwSignal::new(None::<ItemDetail>);
    let metadata_seen = RwSignal::new(None::<Vec<api::MetadataValue>>);
    let metadata_read = RwSignal::new(None::<Vec<api::MetadataValue>>);
    let metadata_reload = RwSignal::new(0u32);
    let links_reload = RwSignal::new(0u32);
    let run_reads = move |reads: live::Reads| {
        if reads.links {
            links_reload.try_update(|n| *n += 1);
        }
        if !(reads.item || reads.metadata) {
            return;
        }
        let Some(code) = code.try_get_value() else {
            return;
        };
        leptos::task::spawn_local(async move {
            // A live read is best-effort: a failed read shows nothing, and
            // the next event or reconnect reads again.
            if reads.item
                && let Ok(next) = api::fetch_item(auth, family, code.clone()).await
            {
                // KAIROS-T-0579: the Notes panel takes new notes at once.
                // No draft holds them, so they need no banner.
                let takes = current.try_with_untracked(|shown| {
                    notes::takes_notes(shown.notes.as_deref(), next.notes.as_deref())
                });
                if takes == Some(true) {
                    let _ = current.try_update(|shown| {
                        // KAIROS-T-0662: the time of the note too, when
                        // nothing else changed: no banner.
                        if live::only_notes_changed(shown, &next) {
                            shown.updated_at.clone_from(&next.updated_at);
                        }
                        shown.notes.clone_from(&next.notes);
                    });
                }
                let _ = pending.try_set(Some(next));
            }
            if reads.metadata
                && let Ok(values) = api::fetch_metadata(auth, family, code).await
            {
                let _ = metadata_read.try_set(Some(values));
            }
        });
    };
    // The reads of the events in one window: the coarse and the precise
    // kind of one write give one read.
    let window = StoredValue::new(live::Reads::default());
    let live_guard: StoredValue<Option<boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(None);
    let guard = boards::live::subscribe_item_events(auth, move |event| {
        let Some(reads) = code.try_with_value(|code| live::reads_for(event, code)) else {
            return;
        };
        if !reads.any() {
            return;
        }
        let first = window.try_update_value(|open| {
            let first = !open.any();
            open.add(reads);
            first
        });
        if first == Some(true) {
            set_timeout(
                move || {
                    if let Some(reads) = window.try_update_value(std::mem::take) {
                        run_reads(reads);
                    }
                },
                std::time::Duration::from_millis(live::LIVE_WINDOW_MS),
            );
        }
    });
    live_guard.set_value(Some(guard));
    on_cleanup(move || live_guard.set_value(None));
    // The banner: `Some(parts)` while the server has a change that the page
    // does not show.
    let live_change = Memo::new(move |_| {
        let mut parts = pending.with(|read| {
            read.as_ref().and_then(|read| {
                current.with(|shown| {
                    live::is_newer_copy(shown, read).then(|| live::changed_parts(shown, read))
                })
            })
        });
        let metadata = metadata_seen.with(|seen| {
            metadata_read.with(|read| live::values_differ(seen.as_deref(), read.as_deref()))
        });
        if metadata {
            parts.get_or_insert_with(Vec::new).push("the metadata");
        }
        parts
    });
    let navigate = leptos_router::hooks::use_navigate();
    // Reload: the page takes the copy that it read. It does not mount
    // again; the editor keeps a dirty draft. A new code is a new page.
    let reload_live = Callback::new(move |()| {
        if let Some(next) = pending.get_untracked() {
            if next.short_code != code.get_value() {
                navigate(&format!("/items/{}", next.short_code), Default::default());
                return;
            }
            show(next);
        }
        pending.set(None);
        let metadata = metadata_seen.with_untracked(|seen| {
            metadata_read
                .with_untracked(|read| live::values_differ(seen.as_deref(), read.as_deref()))
        });
        metadata_read.set(None);
        if metadata {
            metadata_reload.update(|n| *n += 1);
        }
    });

    // ONE board read for the page (KAIROS-T-0164): the placement panel
    // renders it and the Restore affordance's capability mirror needs the
    // board's slug/team — a second fetch would be the same request. It
    // reads again only when the board of the item changes (a board move,
    // KAIROS-T-0618). `Ok(None)` = the item sits on no board at all (an
    // off-board ADR), or it is a document that names no owner board
    // (COLLIERY-T-0269).
    let board = LocalResource::new(move || {
        auth.track_session();
        let id = board_id.get();
        async move {
            match id {
                Some(id) => api::fetch_board(auth, id).await.map(Some),
                None => Ok(None),
            }
        }
    });
    let can_restore = restore_power(family, board, created_by.clone());
    // KAIROS-T-0581, KAIROS-A-0032: the archive takes the rule of the
    // restore (no claim); Archive shows only to a person who passes it,
    // as Cancel does.
    let can_archive = can_restore;
    let task_powers = task_end_powers(family, board, column_id.into());
    // KAIROS-T-0581: the edit rule with the claim, for the editor and the
    // metadata panel. The reason names the board when the page knows it.
    let may_edit = edit_power(family, board, created_by.clone(), claim.into());
    let edit = Memo::new(move |_| {
        let slug = match board.get() {
            Some(Ok(Some(board))) => Some(board.slug),
            _ => None,
        };
        access::edit_access(archived.get(), may_edit.get(), family, slug.as_deref())
    });
    let edit_read_only = Signal::derive(move || edit.with(access::EditAccess::is_read_only));
    // KAIROS-T-0580: the link rule, for the writes of the Relationships
    // panel.
    let may_link = link_power(may_edit);
    let edit_reason = Signal::derive(move || edit.with(access::EditAccess::reason));

    view! {
        {move || view! { <PageHeader title=title.get() sub=family.label()/> }}
        {move || cancellation.get().map(|mark| view! { <CancelledBanner mark/> })}
        {move || view! { <sections::SectionsPanel check=section_check.get()/> }}
        {move || {
            let item_id = item_id.clone();
            archived_at.get().map(|when| view! {
                <ArchivedBanner
                    family
                    code=code.get_value()
                    item_id
                    archived_at=when
                    editorial_archived=editorial_archived.get()
                    can_restore
                    on_restored=on_moved
                />
            })
        }}
        <Group justify="between">
            <Group gap="sm">
                // The short code renders as its own element with a
                // copy-link button (KAIROS-T-0076), not folded into the
                // subtitle string.
                <Text mono=true dimmed=true size="sm">{code.get_value()}</Text>
                <copy_link::CopyLinkButton code=code.get_value()/>
                <Pill color=token::ICE>{move || format!("v{}", version.get())}</Pill>
                // The ADR-20 state, in the words the banner uses. Never
                // "archived" on its own next to the lifecycle badge below.
                {move || archived.get().then(|| view! {
                    <span class="kairos-archived-badge">
                        <Pill color=token::GOLD>"put away"</Pill>
                    </span>
                })}
                // The document lifecycle badge (KAIROS-T-0078): an
                // editorial label, deliberately distinct from anything
                // board-status-shaped — its own class + state color.
                {move || lifecycle.get().map(|state| view! {
                    <span class="kairos-lifecycle-badge">
                        <Pill color=lifecycle_color(&state)>{format!("lifecycle: {state}")}</Pill>
                    </span>
                })}
                {move || view! { <sections::SectionsBadge check=section_check.get()/> }}
                {move || view! { <TypeFacts item=current.get()/> }}
            </Group>
            <Group gap="sm">
                {move || site_href.get_value().filter(|_| !archived.get()).map(|href| view! {
                    <Anchor href=href attr:rel="external" attr:data-testid="open-in-site">
                        "Open in site"
                    </Anchor>
                })}
                <Anchor href=history_href>"History"</Anchor>
                // Archived work is read-only server-side (every write path
                // resolves live-only), so the write affordances are
                // DISABLED with the reason next to them rather than
                // removed — a missing button reads as a broken page, and a
                // live one that 404s reads as a bug.
                {move || archived.get().then(|| view! {
                    <Text size="xs" dimmed=true>"read-only while put away"</Text>
                })}
                {family.is_workflow().then(|| view! {
                    <Button variant="default" size="xs" disabled=archived
                        on_click=Callback::new(move |_| create_open.set(true))>
                        "New document"
                    </Button>
                })}
                // KAIROS-T-0362: a task can be cancelled (not when it is
                // done) and deleted for good. KAIROS-T-0570: the delete is
                // for an archived task, and needs `purge` on the board.
                {move || (task_powers.get().cancel && !archived.get() && !cancelled.get()).then(|| view! {
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| cancel_open.set(true))>
                        "Cancel task"
                    </Button>
                })}
                // KAIROS-T-0648: a done task (completed or cancelled) can
                // be reopened.
                {move || (task_powers.get().reopen && !archived.get()).then(|| view! {
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| reopen_open.set(true))>
                        "Reopen"
                    </Button>
                })}
                {move || can_archive.get().then(|| view! {
                    <Button variant="default" size="xs" disabled=archived
                        on_click=Callback::new(move |_| delete_open.set(true))>
                        "Archive"
                    </Button>
                })}
                {move || (task_powers.get().purge && archived.get()).then(|| view! {
                    <Button variant="default" size="xs" bad=true
                        on_click=Callback::new(move |_| purge_open.set(true))>
                        "Delete"
                    </Button>
                })}
            </Group>
        </Group>
        // KAIROS-T-0578: a change on the server that the page does not
        // show yet. Reload does not mount the page again.
        {move || live_change.get().map(|parts| view! {
            <div data-testid="item-changed-banner">
                <Alert title="Changed on the server" color=token::GOLD>
                    <Group justify="between">
                        <Stack gap="xs">
                            <Text size="sm">{live::banner_text(&parts)}</Text>
                            <Text size="sm" dimmed=true>
                                "Reload to show the change. The editor keeps your draft."
                            </Text>
                        </Stack>
                        <Button variant="default" size="xs" on_click=reload_live>
                            "Reload"
                        </Button>
                    </Group>
                </Alert>
            </div>
        })}
        <ChildrenProgressBar family code=code.get_value()/>
        <div class="kairos-item__grid">
            <Stack gap="sm">
                <ContentEditor
                    family
                    code=code.get_value()
                    initial_title
                    initial_content
                    initial_version
                    read_only=edit_read_only
                    read_only_reason=edit_reason
                    server
                    on_saved
                />
                // KAIROS-T-0630: the notes are not part of the content.
                // KAIROS-T-0579: add a note, its anchor, promote it.
                <NotesPanel
                    family
                    code=code.get_value()
                    notes
                    read_only=edit_read_only
                    read_only_reason=edit_reason
                    on_changed=refresh
                />
            </Stack>
            <Stack gap="sm">
                // COLLIERY-T-0269: the board of a document is its owner,
                // and not a placement. The document has no column, no
                // lane and no transition, so it gets a panel of its own.
                {if family == Family::Document {
                    view! {
                        <OwnerBoardPanel code=code.get_value() board archived on_moved/>
                    }.into_any()
                } else {
                    view! {
                        <BoardPanel family code=code.get_value() board column_id work_class
                            repository complexity edit_read_only created_by=created_by.clone()
                            archived on_moved/>
                    }.into_any()
                }}
                // KAIROS-T-0369: the claim of a task, with Hand off and
                // Release.
                {(family == Family::Task).then(|| view! {
                    <ClaimPanel code=code.get_value() claim board column_id archived on_moved/>
                })}
                // KAIROS-T-0506: the live state of a task.
                {move || (family == Family::Task).then(|| view! {
                    <StatePanel state=state.get()/>
                })}
                // KAIROS-T-0736: the watch of the signed-in person.
                <WatchPanel code=code.get_value() archived/>
                {matches!(family, Family::Document | Family::Adr).then(|| view! {
                    <ImpactsPanel family code=code.get_value() impacts board
                        created_by=created_by.clone() archived on_moved/>
                })}
                // KAIROS-T-0411: the owner of a strategy or an initiative,
                // with Claim, Hand off and Clear.
                {matches!(family, Family::Strategy | Family::Initiative).then(|| view! {
                    <OwnerPanel family code=code.get_value() owner board archived on_moved/>
                })}
                // KAIROS-T-0322: the teams of an initiative or a strategy.
                {matches!(family, Family::Strategy | Family::Initiative).then(|| view! {
                    <TeamsPanel family code=code.get_value() board
                        created_by=created_by.clone() archived/>
                })}
                {move || lifecycle.get().map(|current| view! {
                    <LifecyclePanel code=code.get_value() current archived=archived.get()
                        on_moved/>
                })}
                <MetadataPanel family code=code.get_value() read_only=edit_read_only
                    read_only_reason=edit_reason reload=metadata_reload seen=metadata_seen/>
                // KAIROS-T-0508: the URIs that the item names, with a label.
                <ExternalRefsPanel family code=code.get_value() board
                    created_by=created_by.clone() archived/>
                <DevelopmentPanel family code=code.get_value() reload=links_reload/>
                <EdgeProposalsPanel code=code.get_value()/>
                // KAIROS-T-0195: below the agent-made suggestions, because this
                // is the same idea from the other direction — one you were
                // handed, one you asked for.
                <RelatedWorkPanel code=code.get_value()/>
                <RelationshipsPanel family code=code.get_value() may_link archived/>
            </Stack>
        </div>
        <CreateDocumentDialog parent_code=code.get_value() open=create_open/>
        // The dialogs name the title: they mount again when it changes.
        {move || {
            let title = title.get();
            view! {
                <DeleteDialog family code=code.get_value() title=title.clone() open=delete_open/>
                {(family == Family::Task).then(|| view! {
                    <CancelDialog code=code.get_value() title=title.clone() open=cancel_open
                        on_cancelled=on_moved/>
                    <PurgeDialog code=code.get_value() title=title.clone() open=purge_open/>
                    <ReopenDialog code=code.get_value() title=title.clone() open=reopen_open
                        on_reopened=on_moved/>
                })}
            }
        }}
    }
}

/// The capability that putting an item BACK asks for: the same
/// `manage_<family>` that putting it away asked for (KAIROS-T-0160 —
/// restoring is the inverse of archiving, not a new privilege, so no
/// deployment ends up with someone who can archive and nobody who can
/// restore). It is one of the three ways in of the edit rule
/// (COLLIERY-T-0228); the creator of the item needs no capability.
fn manage_capability(family: Family) -> &'static str {
    match family {
        Family::Strategy => "manage_strategies",
        Family::Initiative => "manage_initiatives",
        Family::Task => "manage_tasks",
        Family::Document => "manage_documents",
        Family::Adr => "manage_adrs",
    }
}

/// May the signed-in user restore THIS item? A restore is an edit, so the
/// answer is the edit rule (COLLIERY-T-0228, [`boards::may_edit_item`]):
/// the user created the item, or holds its `manage_<family>` capability.
/// The shared whoami mirror (KAIROS-T-0072), read against the item's own
/// board once it has loaded.
/// `false` until whoami and the board read have both resolved; an
/// off-board item (`Ok(None)`) or an unreadable board falls back to the
/// board-less mirror, because the server resolves such an item's
/// authorization board through its parent and the client cannot.
fn restore_power(
    family: Family,
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    created_by: String,
) -> Memo<bool> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let created_by = StoredValue::new(created_by);
    Memo::new(move |_| {
        let Some(me) = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
        else {
            return false;
        };
        let required = manage_capability(family);
        created_by.with_value(|created_by| match board.get() {
            None => false,
            Some(Ok(Some(board))) => boards::may_edit_item(
                &me,
                created_by,
                Some(&board.slug),
                board.team_id.as_deref(),
                required,
            ),
            Some(Ok(None)) | Some(Err(_)) => {
                boards::may_edit_item(&me, created_by, None, None, required)
            }
        })
    })
}

/// May the signed-in user EDIT this item (KAIROS-T-0581)? The edit rule
/// of the server with its four facts: [`boards::may_edit_item`] (the
/// creator, `manage_<family>`, an org admin), the `manage_<family>` that
/// membership of the board implies ([`access::membership_implies`],
/// KAIROS-T-0410: an edit, not an archive), or the claim of the task
/// ([`access::holds_claim`], KAIROS-A-0032). `None` until whoami and the
/// board read have both resolved. An off-board item or an unreadable
/// board falls back to the board-less mirror, as [`restore_power`] does.
fn edit_power(
    family: Family,
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    created_by: String,
    claim: Signal<Option<api::Claim>>,
) -> Memo<Option<bool>> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let created_by = StoredValue::new(created_by);
    Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)?;
        let required = manage_capability(family);
        let by_rule = created_by.with_value(|created_by| match board.get()? {
            Ok(Some(board)) => Some(
                boards::may_edit_item(
                    &me,
                    created_by,
                    Some(&board.slug),
                    board.team_id.as_deref(),
                    required,
                ) || access::membership_implies(&me, &board.slug, required),
            ),
            Ok(None) | Err(_) => Some(boards::may_edit_item(&me, created_by, None, None, required)),
        })?;
        Some(by_rule || claim.with(|claim| access::holds_claim(&me, claim.as_ref())))
    })
}

/// May the signed-in user add or remove an edge here (KAIROS-T-0580)?
/// [`relations::may_link`] with the edit rule of this item. `false`
/// until whoami has resolved.
fn link_power(may_edit: Memo<Option<bool>>) -> Signal<bool> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    Signal::derive(move || {
        whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
            .is_some_and(|me| relations::may_link(&me, may_edit.get() == Some(true)))
    })
}

/// What the signed-in user may do to END a task (KAIROS-T-0362).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TaskEndPowers {
    /// Cancel: `transition_items` on the board (the capability of a move),
    /// and the task is not in a done column.
    cancel: bool,
    /// Delete for good: `purge` on the board (KAIROS-T-0570). Team
    /// membership does not give it. The page shows the control on an
    /// archived task only.
    purge: bool,
    /// Reopen (KAIROS-T-0648): `transition_items` on the board, and the
    /// task is in a done column. A cancelled task is in a done column too.
    reopen: bool,
}

impl TaskEndPowers {
    /// The powers from the capabilities and the column of the task. Pure,
    /// host-tested.
    fn from_rule(transition: bool, purge: bool, done: bool) -> Self {
        Self {
            cancel: transition && !done,
            purge,
            reopen: transition && done,
        }
    }
}

/// The [`TaskEndPowers`] of the page: `false` for each until whoami and
/// the board read have resolved, and for an item that is not a task. The
/// server stays the authority.
fn task_end_powers(
    family: Family,
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    column_id: Signal<Option<String>>,
) -> Memo<TaskEndPowers> {
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    Memo::new(move |_| {
        if family != Family::Task {
            return TaskEndPowers::default();
        }
        let Some(me) = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
        else {
            return TaskEndPowers::default();
        };
        let Some(Ok(Some(board))) = board.get() else {
            return TaskEndPowers::default();
        };
        let done = column_id.with(|column| {
            board
                .columns
                .iter()
                .any(|c| Some(&c.id) == column.as_ref() && c.is_done)
        });
        let powers = boards::board_powers(&me, &board.slug, board.team_id.as_deref(), None);
        let purge =
            boards::holds_capability(&me, Some(&board.slug), board.team_id.as_deref(), "purge");
        TaskEndPowers::from_rule(powers.transition, purge, done)
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
        auth.track_session();
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
                             banner (the document is put away), and the \
                             \"lifecycle: archived\" badge below (its editorial state). \
                             A live document can carry that badge; this \
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
/// who may edit the item (its creator, or a holder of `manage_<family>`;
/// COLLIERY-T-0228), and rendering the 422 `RESTORE_BLOCKED`
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
                <Group align="end" gap="sm">
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
        auth.track_session();
        api::fetch_children_progress(auth, family, code.get_value())
    });
    view! {
        {move || match progress.get() {
            Some(Ok(p)) if p.total > 0 => {
                let label = match (p.has_done_columns, p.cancelled) {
                    (false, _) => format!("{} children", p.total),
                    (true, 0) => format!("{} of {} done", p.done, p.total),
                    (true, cancelled) => {
                        format!("{} of {} done, {cancelled} cancelled", p.done, p.total)
                    }
                };
                // COLLIERY-T-1836: the Aurora `SegmentedBar`: one part per
                // column, a done column in the ok hue, the others neutral.
                // KAIROS-T-0567: the cancelled tasks of a column are a part
                // of their own, in gold: a cancel is not done.
                let mut segments: Vec<Segment> = Vec::new();
                for column in &p.by_column {
                    let color = if column.is_done { token::OK } else { token::MUTED };
                    let kept = column.count - column.cancelled;
                    if kept > 0 {
                        segments.push(Segment::new(column.column_name.clone(), kept as f64, color));
                    }
                    if column.cancelled > 0 {
                        segments.push(Segment::new(
                            format!("{} (cancelled)", column.column_name),
                            column.cancelled as f64,
                            token::GOLD,
                        ));
                    }
                }
                view! {
                    <div class="kairos-progress">
                        <SegmentedBar segments label="Children by column" height=6/>
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
    // KAIROS-T-0109: the repository the task links to (COLLIERY-A-0023).
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
        <Text size="xs" dimmed=true>"updated " <RelativeTime iso=item.updated_at.clone()/></Text>
    }
}

/// Board/column display: ADRs may not sit on a board; the rest always
/// do. A document never sits on a board, and this panel is not for it
/// (COLLIERY-T-0269, [`OwnerBoardPanel`]). Board and column ids resolve to names via
/// `GET /api/boards/{id}`, linked to the T-0040 board view. On-board
/// items also get the move control here (KAIROS-T-0075) — the
/// keyboard-accessible transition path since cards are drag-only.
#[component]
fn BoardPanel(
    family: Family,
    #[prop(into)] code: String,
    /// The page's shared board read (`Ok(None)` = off-board).
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    /// The column of the item. The parts of the item are signals
    /// (KAIROS-T-0618): the page stays mounted when they change.
    #[prop(into)]
    column_id: Signal<Option<String>>,
    /// The task's Planned/Support lane (KAIROS-T-0077) — `Some` enables
    /// the lane control.
    #[prop(into)]
    work_class: Signal<Option<String>>,
    /// The task's bound repository slug (KAIROS-T-0109), if any.
    #[prop(into)]
    repository: Signal<Option<String>>,
    /// The complexity of an initiative, if any.
    #[prop(into)]
    complexity: Signal<Option<String>>,
    /// The page's edit rule (with the archive): it opens the complexity
    /// control, which is an edit, as the editor and the metadata panel.
    #[prop(into)]
    edit_read_only: Signal<bool>,
    /// The user id of the creator of the item. It opens the repository
    /// control, which is an edit, and NO move control (COLLIERY-T-0228).
    created_by: String,
    /// The item is archived (ADR-20): its placement is a record of where
    /// it was put away, and every move endpoint resolves live-only, so the
    /// write controls are replaced by the reason they are gone.
    #[prop(into)]
    archived: Signal<bool>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let code = StoredValue::new(code);
    let created_by = StoredValue::new(created_by);
    view! {
        <Panel title="Board" caption="placement">
            {move || match board.get() {
                None => view! { <Loading label="Loading board…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                Some(Ok(None)) => {
                    // A document does not come here: it has the panel of
                    // its owner board (COLLIERY-T-0269).
                    let message = "Off-board — no column, no transitions (org-admin writes only).";
                    view! { <Text size="sm" dimmed=true>{message}</Text> }.into_any()
                }
                Some(Ok(Some(board))) => {
                    let slug = board.slug.clone();
                    let name = board.name.clone();
                    let team_id = board.team_id.clone();
                    let board = StoredValue::new(board);
                    // The column the item is IN — which, for an archived
                    // card, may be one that has since been removed from
                    // the board (KAIROS-T-0161). That is exactly the fact
                    // the column soft delete exists to keep, so it is
                    // named and marked rather than resolved to "unknown
                    // column"; `fetch_board` asks for removed columns for
                    // this one reason. Removed columns are never move
                    // targets — the transition list is live-only
                    // server-side, so `MoveControl` cannot offer one.
                    let column_pill = move || {
                        let column = column_id.with(|id| {
                            board.with_value(|board| {
                                id.as_ref()
                                    .and_then(|id| board.columns.iter().find(|c| &c.id == id))
                                    .cloned()
                            })
                        });
                        let (column_label, column_color) = match &column {
                            Some(c) if c.removed_at.is_some() => (
                                format!("{} (column since removed)", c.name),
                                token::GOLD,
                            ),
                            Some(c) => (c.name.clone(), token::ICE),
                            None => ("unknown column".to_string(), token::MUTED),
                        };
                        view! { <Pill color=column_color>{column_label}</Pill> }
                    };
                    let board_slug = StoredValue::new(slug.clone());
                    let team = StoredValue::new(team_id);
                    view! {
                        <Stack gap="sm">
                            <Group justify="between">
                                <Anchor href=format!("/boards/{slug}")>{name}</Anchor>
                                {column_pill}
                            </Group>
                            {move || archived.get().then(|| view! {
                                <Text size="xs" dimmed=true>
                                    "Placement is frozen while this item is put away — it is \
                                     the record of where the work sat. Restore it to move it."
                                </Text>
                            })}
                            {move || (!archived.get() && matches!(family, Family::Task)).then(|| view! {
                                <RepositoryControl
                                    code=code.get_value()
                                    board_slug=board_slug.get_value()
                                    team_id=team.get_value()
                                    current=repository
                                    created_by=created_by.get_value()
                                    on_moved
                                />
                            })}
                            {move || (!edit_read_only.get() && matches!(family, Family::Initiative)).then(|| view! {
                                <ComplexityControl code=code.get_value() current=complexity on_moved/>
                            })}
                            // The two controls below MOVE the item. The
                            // creator of an item gets no right to move it
                            // (COLLIERY-T-0228): they take no `created_by`,
                            // and must not.
                            {move || (!archived.get() && matches!(family, Family::Task)).then(|| view! {
                                <MoveBoardControl
                                    code=code.get_value()
                                    board_slug=board_slug.get_value()
                                    team_id=team.get_value()
                                    on_moved
                                />
                            })}
                            // The targets follow the column: this control
                            // mounts again when the column or the lane
                            // changes. It reads nothing.
                            {move || (!archived.get()).then(|| view! {
                                <MoveControl
                                    family
                                    code=code.get_value()
                                    board=board.get_value()
                                    column_id=column_id.get()
                                    work_class=work_class.get()
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
/// by [`MoveControl`] (`transition`) and [`MoveBoardControl`] (`create`
/// tasks = `manage_tasks`) so both controls gate through one derivation
/// (KAIROS-T-0114). `false` until whoami has resolved.
///
/// These are the MOVE controls. A board power does not know who created
/// the item, so the creator gets nothing here (COLLIERY-T-0228).
/// [`RepositoryControl`] left this function for that reason: the
/// repository is an edit, and it asks [`boards::may_edit_item`].
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

/// The task's repository link (KAIROS-T-0109): pick a repository, or none.
/// The server enforces the edit rule (COLLIERY-T-0228): the creator of the
/// task, or `manage_tasks` on its board. A refusal shows inline.
///
/// A task may link to a repository of any team (COLLIERY-A-0023), so the
/// picker offers every live repository of the tenant (COLLIERY-T-0221):
/// those of the team of the board first, then the others with the name of
/// their owning team. A link to a repository of a different team is shown
/// like any other, with no warning (COLLIERY-T-0219).
///
/// The current link is always in that list. The server puts a repository
/// on a task only while the repository is live, and refuses to delete a
/// repository that a live task links to. COLLIERY-T-0219 added the current
/// link as an extra option because the list stopped at one team; that
/// option is removed, because it would now repeat an entry.
///
/// The list loads when the person opens the control (KAIROS-T-0618).
#[component]
fn RepositoryControl(
    code: String,
    /// The board's slug — for the client-side capability mirror.
    board_slug: String,
    /// The board's owning team (UUID): its repositories are first in the
    /// picker.
    team_id: Option<String>,
    /// The current link (slug). A signal (KAIROS-T-0618): the control
    /// stays open after a set.
    current: Signal<Option<String>>,
    /// The user id of the creator of the task.
    created_by: String,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let value = RwSignal::new(
        current
            .get_untracked()
            .unwrap_or_else(|| NO_REPOSITORY.to_string()),
    );
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    // The edit rule (COLLIERY-T-0228): the creator of the task, or
    // `manage_tasks` on its board. Until then this was the `manage_tasks`
    // mirror alone, so the person who sent a request did not see the
    // control that the server lets them use.
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let slug = StoredValue::new(board_slug);
    let team = StoredValue::new(team_id.clone());
    let created_by = StoredValue::new(created_by);
    let can_bind = Memo::new(move |_| {
        whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
            .is_some_and(|me| {
                boards::may_edit_item(
                    &me,
                    &created_by.get_value(),
                    Some(&slug.get_value()),
                    team.get_value().as_deref(),
                    "manage_tasks",
                )
            })
    });
    let board_team = StoredValue::new(team_id);
    // KAIROS-T-0618: the repositories load when the control opens.
    let open = RwSignal::new(false);
    let repos = load_on_open(open, move || {
        auth.track_session();
        async move { repositories::api::list_repositories(auth, None).await }
    });
    let picker = Memo::new(move |_| {
        let read = repos.get().flatten();
        let count = match &read {
            Some(Ok(list)) => list.len(),
            _ => 0,
        };
        picker_state(open.get(), read.as_ref(), count)
    });
    let submit: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        let repository = (chosen != NO_REPOSITORY).then_some(chosen);
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result =
                repositories::api::set_repository(auth, &code.get_value(), repository.as_deref())
                    .await;
            // The control stays mounted (KAIROS-T-0618): it is free for
            // the next change.
            busy.set(false);
            match result {
                Ok(()) => on_moved.run(match repository {
                    Some(slug) => format!("Repository set to {slug}."),
                    None => "Repository cleared.".to_string(),
                }),
                Err(e) => error.set(Some(e)),
            }
        });
    });
    view! {
        {move || can_bind.get().then(|| view! {
        <div class="kairos-item__repository" data-testid="repository-control">
            {move || match picker.get() {
                Picker::Closed => view! {
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| open.set(true))>
                        "Change repository"
                    </Button>
                }.into_any(),
                Picker::Loading => view! {
                    <Text size="xs" dimmed=true>"Loading repositories…"</Text>
                }.into_any(),
                Picker::Failed => view! {
                    <Text size="xs" dimmed=true>"The repositories are not available."</Text>
                }.into_any(),
                Picker::Empty => view! {
                    <Text size="xs" dimmed=true>"No repositories in this organization."</Text>
                }.into_any(),
                Picker::Ready => {
                    let list = match repos.get().flatten() {
                        Some(Ok(list)) => list,
                        _ => Vec::new(),
                    };
                    // Options are `(value, label)`: the slug stays the value
                    // (what the server takes). The label adds the forge full
                    // name and, for a repository of a different team, the
                    // owning team (COLLIERY-T-0221).
                    let options: Vec<(String, String)> = std::iter::once((
                        NO_REPOSITORY.to_string(),
                        NO_REPOSITORY.to_string(),
                    ))
                    .chain(board_team.with_value(|team| {
                        repositories::picker_options(&list, team.as_deref())
                    }))
                    .collect();
                    view! {
                        <Stack gap="xs">
                            <Group align="end" gap="sm">
                                <Select label="Repository" option_pairs=options value=value/>
                                <Button
                                    size="xs"
                                    disabled=move || {
                                        let bound = current
                                            .get()
                                            .unwrap_or_else(|| NO_REPOSITORY.to_string());
                                        value.get() == bound
                                    }
                                    loading=busy
                                    loading_label="Setting…"
                                    on_click=submit
                                >
                                    "Set repository"
                                </Button>
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

/// The value of the complexity picker that clears the complexity.
const NO_COMPLEXITY: &str = "none";

/// The complexity of an initiative: pick a size, or none. The page shows
/// it only to a person who passes the edit rule; the server enforces it.
#[component]
fn ComplexityControl(
    code: String,
    /// The current complexity. A signal: the control stays mounted after a
    /// set.
    current: Signal<Option<String>>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let shown = move || current.get().unwrap_or_else(|| NO_COMPLEXITY.to_string());
    let value = RwSignal::new(
        current
            .get_untracked()
            .unwrap_or_else(|| NO_COMPLEXITY.to_string()),
    );
    // The picker follows the initiative: a refetch after a change made
    // elsewhere (another tab, an agent) shows the new complexity, not the
    // one the page loaded with.
    Effect::new(move |_| value.set(shown()));
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let submit: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::set_complexity(auth, &code.get_value(), &chosen).await;
            busy.set(false);
            match result {
                Ok(()) if chosen == NO_COMPLEXITY => {
                    on_moved.run("Complexity cleared.".to_string())
                }
                Ok(()) => on_moved.run(format!("Complexity set to {chosen}.")),
                Err(e) => error.set(Some(e)),
            }
        });
    });
    view! {
        <div class="kairos-item__complexity" data-testid="complexity-control">
            <Group align="end" gap="sm">
                <Select label="Complexity" value=value
                    options=[NO_COMPLEXITY, "xs", "s", "m", "l", "xl"]
                        .map(str::to_string)
                        .to_vec()/>
                <Button
                    size="xs"
                    disabled=move || value.get() == shown()
                    loading=busy
                    loading_label="Setting…"
                    on_click=submit
                >
                    "Set complexity"
                </Button>
            </Group>
            {move || error.get().map(|e| view! {
                <Alert title="Could not set complexity" color=token::BAD>
                    <Text size="sm">{api::error_text(&e)}</Text>
                </Alert>
            })}
        </div>
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
/// comes back as a 422 and is shown inline. The list of boards loads when
/// the person opens the control (KAIROS-T-0618).
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
    // COLLIERY-T-3101: a move can also give the task a code of the new
    // board. Off by default: a move keeps the code.
    let rename = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let toaster = use_toaster();
    let navigate = leptos_router::hooks::use_navigate();
    // Source half: the same `manage_tasks` mirror as the board's create
    // affordance and the repository picker.
    let can_move = board_power(
        board_slug,
        team_id,
        Some(boards::data::EntityKind::Task),
        |powers| powers.create,
    );
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    // KAIROS-T-0618: the boards load when the control opens, and only for
    // a person who holds the source half.
    let open = RwSignal::new(false);
    let all_boards = load_on_open(open, move || {
        auth.track_session();
        boards::data::list_boards(auth)
    });
    // Target half, from the ONE derivation the board view uses.
    let targets: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(Ok(list))) = (me, all_boards.get().flatten()) else {
            return Vec::new();
        };
        here.with_value(|here| boards::movable_delivery_boards(&me, &list, here))
    });
    let picker = Memo::new(move |_| {
        let read = all_boards.get().flatten();
        picker_state(open.get(), read.as_ref(), targets.with(Vec::len))
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
        let rename = rename.get_untracked();
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let old = code.get_value();
            match api::move_task(auth, &old, &chosen, rename).await {
                // A rename: the page of the old code is not the page of
                // the item now. Go to the page of the new code (a new
                // page, so no reload here).
                Ok(moved) if moved.short_code != old => {
                    toaster.success(format!(
                        "Moved to {label}. The new code is {}. The code {old} is retired.",
                        moved.short_code
                    ));
                    navigate(&format!("/items/{}", moved.short_code), Default::default());
                }
                // Success reads the item (on_moved). The board of the item
                // changes, so the Board panel shows the new board and its
                // entry column, and this control mounts again.
                Ok(_) => on_moved.run(format!("Moved to {label}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    view! {
        {move || can_move.get().then(|| view! {
            <div class="kairos-item__move-board" data-testid="move-board">
                {move || match picker.get() {
                    Picker::Closed => view! {
                        <Button variant="default" size="xs"
                            on_click=Callback::new(move |_| open.set(true))>
                            "Move to another board"
                        </Button>
                    }.into_any(),
                    Picker::Loading => view! {
                        <Text size="xs" dimmed=true>"Loading boards…"</Text>
                    }.into_any(),
                    Picker::Failed => view! {
                        <Text size="xs" dimmed=true>"The boards are not available."</Text>
                    }.into_any(),
                    Picker::Empty => view! {
                        <Text size="xs" dimmed=true>
                            "You cannot move this task to another delivery board."
                        </Text>
                    }.into_any(),
                    Picker::Ready => view! {
                        <Group align="end" gap="sm">
                            <Select
                                label="Board"
                                option_pairs={std::iter::once((THIS_BOARD.to_string(), THIS_BOARD.to_string()))
                                    .chain(targets.get())
                                    .collect::<Vec<_>>()}
                                value=value
                            />
                            <Switch checked=rename label="Give it a code of the new board"/>
                            <Button
                                size="xs"
                                disabled=move || value.get() == THIS_BOARD
                                loading=busy
                                loading_label="Moving…"
                                on_click=submit
                            >
                                "Move board"
                            </Button>
                        </Group>
                    }.into_any(),
                }}
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
                // Success reads the item (on_moved). The column changes,
                // so this control mounts again — no busy reset.
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
                    <Group align="end" gap="sm">
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
                    <Group align="end" gap="sm">
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
        auth.track_session();
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

/// "Possibly related" — the human side of retrieval (KAIROS-T-0195).
///
/// Before this, a person could only ever REACT to what an agent thought to look
/// at: `EdgeProposalsPanel` above shows proposals an agent made, and there was no
/// way to ask the question yourself. "Didn't we try this?" is at least as much a
/// human question — it is the one [[KAIROS-I-0017]] is named after.
///
/// # Why it reads the way it does
///
/// KAIROS-A-0021 rule 5 is that these are PROPOSALS, not findings, and the
/// measurement behind it is uncomfortable: related pairs average 0.80 similarity
/// and unrelated pairs reach 0.82, so roughly half the strongest matches are
/// wrong. A panel that rendered this as a confident list of links would undo in
/// pixels what the MCP text is careful to say in words. Hence: "possibly", the
/// claim as a pill, the *why* as a sentence you can disagree with, and **no score
/// shown** — the score is a fused rank comparable within one response and nowhere
/// else, and a number on screen reads as a confidence whatever the label says.
///
/// Renders nothing when there is nothing to say, including when this deployment
/// has embeddings off (a `503`, which is not an error worth showing) — a
/// permanently visible "no suggestions" box trains people to stop looking.
#[component]
fn RelatedWorkPanel(#[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let asked = RwSignal::new(false);

    // Deliberately NOT fetched on page load. Retrieval costs a vector search per
    // ask, and most visits to an item are not someone wondering what it
    // duplicates. A button also makes it a question the person asked, which is
    // the framing rule 5 wants.
    let related = LocalResource::new(move || {
        auth.track_session();
        let ask = asked.get();
        async move {
            if !ask {
                return None;
            }
            Some(api::fetch_related_work(auth, code.get_value()).await)
        }
    });

    view! {
        <Panel title="Possibly related" caption="ask what else touches this work">
            <Stack gap="sm">
                <Show
                    when=move || asked.get()
                    fallback=move || view! {
                        <Group gap="sm">
                            <Button
                                variant="default"
                                size="xs"
                                on_click=Callback::new(move |_| asked.set(true))
                            >
                                "Find related work"
                            </Button>
                            <Text size="xs" dimmed=true>
                                "Searches titles, content and the graph for work that \
                                 might overlap this."
                            </Text>
                        </Group>
                    }
                >
                    {move || match related.get().flatten() {
                        None => view! { <Text size="xs" dimmed=true>"Searching…"</Text> }.into_any(),
                        // 503 means embeddings are off on this deployment. Not an
                        // error: say so plainly once, since the person just
                        // clicked and deserves an answer rather than silence.
                        Some(Err(ApiError::Http { status: 503, .. })) => view! {
                            <Text size="xs" dimmed=true>
                                "This deployment does not have the search for related work."
                            </Text>
                        }.into_any(),
                        Some(Err(e)) => view! {
                            <Alert title="Could not search" color=token::BAD>
                                <Text size="sm">{api::error_text(&e)}</Text>
                            </Alert>
                        }.into_any(),
                        Some(Ok(found)) => {
                            let degraded = !found.vector;
                            let note = found.note.clone();
                            if found.proposals.is_empty() {
                                return view! {
                                    <Text size="xs" dimmed=true>
                                        // Precise on purpose: an empty answer is one
                                        // search coming up short, not proof.
                                        "Nothing surfaced. That is this search coming up \
                                         short rather than proof that nothing is related."
                                    </Text>
                                }.into_any();
                            }
                            view! {
                                <Stack gap="sm">
                                    <Text dimmed=true size="xs">
                                        "These are suggestions, not findings — similar wording \
                                         is not the same as related work. Open one to judge it."
                                    </Text>
                                    {degraded.then(|| view! {
                                        <Alert title="Text-only answer" color=token::GOLD>
                                            <Text size="sm">{note.clone()}</Text>
                                        </Alert>
                                    })}
                                    {found.proposals.into_iter().map(|p| view! {
                                        <Stack gap="xs">
                                            <Group gap="xs">
                                                <Pill color=token::VIOLET>{p.claim.clone()}</Pill>
                                                <Anchor href=format!("/items/{}", p.short_code)>
                                                    {p.short_code.clone()}
                                                </Anchor>
                                                <Text size="xs">{p.title.clone()}</Text>
                                            </Group>
                                            <Text size="xs" dimmed=true>{p.why.clone()}</Text>
                                        </Stack>
                                    }).collect_view()}
                                </Stack>
                            }.into_any()
                        }
                    }}
                </Show>
            </Stack>
        </Panel>
    }
}

/// Relationships summary: both directions, grouped, every neighbor linked
/// to its own detail route. The full graph explorer is T-0042's.
///
/// Archived-INCLUSIVE since KAIROS-T-0158, with every put-away neighbour
/// marked (KAIROS-T-0163) — deliberately unlike [`ChildrenProgressBar`]
/// above it, which stays live-only per ADR-20 rule 5.
///
/// KAIROS-T-0580: a person who may edit an end ([`relations::may_link`])
/// gets a Remove button on each edge, the add form and the Parent picker.
#[component]
fn RelationshipsPanel(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] may_link: Signal<bool>,
    #[prop(into)] archived: Signal<bool>,
) -> impl IntoView {
    let auth = use_auth();
    let toaster = use_toaster();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let relationships = LocalResource::new(move || {
        auth.track_session();
        let _ = reload.get();
        api::fetch_relationships(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    let on_changed = Callback::new(move |message: String| {
        toaster.success(message);
        reload.update(|n| *n += 1);
    });
    let remove = Callback::new(move |(edge_id, what): (String, String)| {
        leptos::task::spawn_local(async move {
            match super::search::data::delete_relationship(auth, &edge_id).await {
                Ok(_) => on_changed.run(format!("Kairos removed the edge: {what}.")),
                Err(e) => {
                    toaster.error(format!(
                        "Kairos did not remove the edge: {}",
                        api::error_text(&e)
                    ));
                }
            }
        });
    });
    let writes = move || may_link.get() && !archived.get();

    view! {
        <Panel title="Relationships" caption="summary">
            {move || match relationships.get() {
                None => view! { <Loading label="Loading relationships…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(relationships)) => {
                    let has_parent = relationships
                        .incoming
                        .iter()
                        .any(|group| group.relationship == "parent" && !group.items.is_empty());
                    let editor = writes().then(|| view! {
                        <relations::RelationshipEditor family code=code.get_value() has_parent
                            on_changed/>
                    });
                    let on_remove = writes().then_some(remove);
                    view! {
                        <Stack gap="sm">
                            <RelationshipList relationships on_remove code=code.get_value()/>
                            {editor}
                        </Stack>
                    }.into_any()
                }
            }}
        </Panel>
    }
}

/// The list of the panel: each group, or the empty state.
#[component]
fn RelationshipList(
    relationships: api::ItemRelationships,
    on_remove: Option<Callback<(String, String)>>,
    #[prop(into)] code: String,
) -> impl IntoView {
    if relationships.outgoing.is_empty() && relationships.incoming.is_empty() {
        return view! { <Empty message="No relationships yet."/> }.into_any();
    }
    // ADR-20 rule 5, and the difference worth making legible rather than
    // reconciling away: this panel is archived-INCLUSIVE, while the
    // children rollup at the top of the page counts live work only. So
    // "2 children" here beside "1 of 1 done" up there is CORRECT, and the
    // note says why in one line.
    let any_put_away = relationships
        .outgoing
        .iter()
        .chain(relationships.incoming.iter())
        .flat_map(|group| group.items.iter())
        .any(|item| item.archived_at.is_some());
    view! {
        <Stack gap="sm">
            {relationships.outgoing.into_iter().map(|group| view! {
                <RelationshipGroupView group direction="outgoing" on_remove/>
            }).collect_view()}
            {relationships.incoming.into_iter().map(|group| view! {
                <RelationshipGroupView group direction="incoming" on_remove/>
            }).collect_view()}
            {any_put_away.then(|| view! {
                <Text dimmed=true size="xs">
                    "Put-away neighbours are listed: containment is a fact \
                     about the record. The progress bar above counts live \
                     work only — progress is a fact about live work."
                </Text>
            })}
            <Anchor href=format!("/search/relationships/{code}")>
                "Open the graph explorer"
            </Anchor>
        </Stack>
    }
    .into_any()
}

/// The accent for a forge link's state (KAIROS-T-0100). Red stays
/// reserved for violated/at-risk — a closed pull request is a normal
/// outcome, not an error.
/// The label of a row of the Development panel: `#12 Title` for a pull
/// request, `Issue #12 Title (robin)` for an issue (KAIROS-T-0482), the
/// branch name for a branch. Pure, host-tested.
fn dev_link_label(kind: &str, external_id: &str, title: &str, author: &str) -> String {
    match kind {
        "pull_request" => format!("#{external_id} {title}"),
        "issue" if author.is_empty() => format!("Issue #{external_id} {title}"),
        "issue" => format!("Issue #{external_id} {title} ({author})"),
        _ => title.to_string(),
    }
}

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
fn DevelopmentPanel(
    family: Family,
    #[prop(into)] code: String,
    /// Webhook ingestion is the whole point of this panel, and it happens
    /// without the user doing anything. The one socket of the page
    /// (KAIROS-T-0578) bumps this on `item_links_changed` of the item and
    /// on a reconnect; the panel reads the links again.
    #[prop(into)]
    reload: Signal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let links = LocalResource::new(move || {
        auth.track_session();
        let _ = reload.get();
        api::fetch_links(auth, family, code.get_value())
    });

    view! {
        {move || match links.get() {
            Some(Ok(links)) if !links.is_empty() => {
                let rows = links
                    .into_iter()
                    .map(|link| {
                        let label = dev_link_label(&link.kind, &link.external_id, &link.title, &link.author);
                        // KAIROS-T-0482: an issue is not synced after intake,
                        // so its pill says what it is, not a state.
                        let pill = if link.kind == "issue" {
                            "issue".to_string()
                        } else {
                            link.state.clone()
                        };
                        let repo = format!("{} · {}", link.forge, link.repo_full_name);
                        view! {
                            <Group justify="between" wrap=true>
                                <Group gap="sm" wrap=true>
                                    <Pill color=link_state_color(&link.state)>
                                        {pill}
                                    </Pill>
                                    // Leaves the app: open in a new tab and
                                    // sever the opener reference.
                                    <Anchor href=link.url.clone() attr:target="_blank" attr:rel="noopener noreferrer">
                                        {label}
                                    </Anchor>
                                </Group>
                                <Text mono=true dimmed=true size="xs">{repo}</Text>
                            </Group>
                        }
                    })
                    .collect_view();
                view! {
                    <Panel title="Development" caption="branches, pull requests and issues">
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
        // KAIROS-A-0069: source ADR changes a part of the target ADR.
        ("amends", true) => "amends".to_string(),
        ("amends", false) => "amended by".to_string(),
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
    /// KAIROS-T-0580: the remove of an edge, `(edge id, words)`. `None`:
    /// the person may not write here.
    on_remove: Option<Callback<(String, String)>>,
) -> impl IntoView {
    let label = relationship_label(&group.relationship, direction == "outgoing");
    let header = label.clone();
    let is_blocks = group.relationship == "blocks";
    view! {
        <div class="kairos-relationships__group">
            <Group gap="sm">
                <Text size="xs" dimmed=true>{header}</Text>
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
                // COLLIERY-T-0214: done work does not block and is not
                // blocked. The edge stays on the list as history, so the
                // row says which end is finished; an unmarked completed
                // blocker reads as one still in the way. Only on `blocks`
                // edges: that is where the mark changes what the reader
                // does next. "done" is the board admin's word for the
                // column flag, in the same colour.
                // KAIROS-T-0567: a cancelled blocker still blocks, so it is
                // "cancelled", never "done".
                let done = (is_blocks && item.cancelled).then(|| view! {
                    <span
                        class="kairos-done-badge"
                        title="Cancelled: this edge still blocks until a person removes it"
                    >
                        <Pill color=token::GOLD>"cancelled"</Pill>
                    </span>
                }.into_any()).or_else(|| (is_blocks && item.done).then(|| view! {
                    <span
                        class="kairos-done-badge"
                        title="In a done column: this edge no longer blocks"
                    >
                        <Pill color=token::OK>"done"</Pill>
                    </span>
                }.into_any()));
                let remove = on_remove.filter(|_| !item.relationship_id.is_empty()).map(|run| {
                    let edge = (item.relationship_id.clone(), format!("{label} {}", item.short_code));
                    let aria = format!("Remove {label} {}", item.short_code);
                    view! {
                        <Button variant="default" size="xs" bad=true aria_label=aria
                            on_click=Callback::new(move |_| run.run(edge.clone()))>
                            "Remove"
                        </Button>
                    }
                });
                view! {
                    <Group gap="sm" justify="between">
                        <Anchor href=format!("/items/{}", item.short_code)>
                            {format!("{} — {}", item.short_code, item.title)}
                        </Anchor>
                        <Group gap="sm">
                            {done}
                            {put_away}
                            <Pill color=token::VIOLET>{item.entity_type.clone()}</Pill>
                            {remove}
                        </Group>
                    </Group>
                }
            }).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    /// KAIROS-T-0482: the Development panel names an issue and its author.
    #[test]
    fn development_link_labels() {
        assert_eq!(
            super::dev_link_label("pull_request", "7", "Fix", "robin"),
            "#7 Fix"
        );
        assert_eq!(
            super::dev_link_label("issue", "12", "Crash", "robin"),
            "Issue #12 Crash (robin)"
        );
        assert_eq!(
            super::dev_link_label("issue", "12", "Crash", ""),
            "Issue #12 Crash"
        );
        assert_eq!(
            super::dev_link_label("branch", "x", "sam/x", "sam"),
            "sam/x"
        );
    }

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

    /// KAIROS-T-0618: the page takes a copy of the same version or newer,
    /// and not an older copy (a read that answered late).
    #[test]
    fn the_page_takes_no_older_copy() {
        assert!(takes_copy(4, 5));
        // A claim or a hand-off does not change the version.
        assert!(takes_copy(4, 4));
        assert!(!takes_copy(5, 4));
    }

    /// KAIROS-T-0648: Reopen shows for a done task to a person with
    /// `transition_items`; Cancel shows for a task that is not done. A
    /// cancelled task is in a done column, so it gets Reopen.
    #[test]
    fn reopen_shows_for_a_done_task_with_the_move_capability() {
        let done = TaskEndPowers::from_rule(true, false, true);
        assert!(done.reopen);
        assert!(!done.cancel);
        let open = TaskEndPowers::from_rule(true, false, false);
        assert!(!open.reopen);
        assert!(open.cancel);
        let no_move = TaskEndPowers::from_rule(false, true, true);
        assert!(!no_move.reopen);
        assert!(!no_move.cancel);
        assert!(no_move.purge);
        assert_eq!(
            TaskEndPowers::default(),
            TaskEndPowers::from_rule(false, false, false)
        );
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
        assert_eq!(relationship_label("amends", false), "amended by");
        assert_eq!(relationship_label("amends", true), "amends");
        assert_eq!(relationship_label("mystery", true), "mystery (outgoing)");
    }
}
