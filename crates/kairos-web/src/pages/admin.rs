//! Admin surfaces (KAIROS-T-0043): board configuration, board members +
//! capability grants (KAIROS-A-0006), teams, delivery streams, organization
//! members, templates, and metadata definitions — A-0015's v1 admin scope.
//!
//! Sub-routes (registered in `app.rs`, all under the protected shell):
//! `/admin` (overview), `/admin/boards`, `/admin/boards/:board`,
//! `/admin/teams`, `/admin/streams`, `/admin/members`, `/admin/templates`,
//! `/admin/metadata`.
//!
//! # Access gating (decision, KAIROS-T-0043, upgraded KAIROS-T-0052)
//!
//! `/api/whoami` now reports the caller's org **role** AND their per-board
//! capability grants ([`crate::api::Whoami::capabilities`], KAIROS-T-0052).
//! Gating is therefore two-tier:
//!
//! - **Org admins** (`organization.role == "admin"`, the A-0006 bypass) see
//!   every admin surface — boards, teams, streams, members, templates,
//!   metadata — unchanged.
//! - **Non-admins who hold a board-config capability** (`configure_boards`
//!   or `manage_members`, incl. globs like `configure_*`/`manage_*`/`*`) on
//!   at least one board now reach the admin section too, but only its
//!   per-board configuration surface (`/admin/boards` + `/admin/boards/:id`).
//!   The org-admin-only surfaces (teams, streams, org members, templates,
//!   metadata, and board create/delete) stay hidden from them.
//! - **Everyone else** (a plain member with no grants) gets the graceful
//!   "admin access required" panel instead of a broken screen.
//!
//! The nav entry ([`AdminNavLink`]) follows the same [`gating::can_access`]
//! rule. Any server 403 that slips through (e.g. a grant revoked mid-session)
//! still renders per-section via `ErrorState`/[`MutationNotice`] — the GUI
//! gate is UX, the server is the authority (A-0006).

use aurora_dark::components::{
    Anchor, ErrorState, Group, Loading, PageHeader, Panel, SimpleGrid, Stack, Text,
};
use leptos::prelude::*;
use leptos_router::components::Outlet;
use leptos_router::hooks::use_location;

use crate::auth::use_auth;

pub(crate) mod gating;

mod api;
mod boards;
mod capabilities;
mod members;
mod metadata;
mod repositories;
mod streams;
mod teams;
mod templates;

pub use boards::{AdminBoardPage, AdminBoardsPage};
pub use members::AdminMembersPage;
pub use metadata::AdminMetadataPage;
pub use repositories::AdminRepositoriesPage;
pub use streams::AdminStreamsPage;
pub use teams::AdminTeamsPage;
pub use templates::AdminTemplatesPage;

/// The admin section shell: whoami-probed role gate, section tabs, and the
/// sub-route outlet. Non-admins get the graceful gate panel (see module
/// docs), never a blank screen.
#[component]
pub fn AdminPage() -> impl IntoView {
    let auth = use_auth();
    let whoami = LocalResource::new(move || {
        // Track the session so a fresh login refetches.
        let _ = auth.token();
        crate::api::whoami(auth)
    });
    view! {
        {move || match whoami.get() {
            None => view! { <Loading label="Checking access…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| whoami.refetch())/>
            }.into_any(),
            Some(Ok(me)) if !gating::can_access(&me) => view! {
                <NotAdminGate role=me.organization.role.clone()/>
            }.into_any(),
            Some(Ok(me)) => {
                let is_admin = gating::is_org_admin(&me);
                view! {
                    <Stack gap="md">
                        <SectionTabs is_admin/>
                        <Outlet/>
                    </Stack>
                }.into_any()
            }
        }}
    }
}

/// The graceful denied path: what a plain `member` with no board-config
/// grants sees on a direct visit to any `/admin` route (the nav never shows
/// Admin to them). Holders of `configure_boards`/`manage_members` no longer
/// land here — they get the boards surface (KAIROS-T-0052).
#[component]
fn NotAdminGate(role: String) -> impl IntoView {
    view! {
        <PageHeader title="Admin" sub="organization administration"/>
        <Panel title="Admin access required" caption="not authorized">
            <Stack gap="sm">
                <Text>
                    "Organization administration needs the "<b>"admin"</b>
                    " role, or a board-scoped "<b>"configure_boards"</b>" / "
                    <b>"manage_members"</b>" grant; you are signed in as a "
                    <b>{role}</b>" with no such grants."
                </Text>
                <Text dimmed=true size="sm">
                    "Ask an organization admin to grant you a board capability
                     if you need to configure a specific board here."
                </Text>
                <Anchor href="/boards">"Back to boards"</Anchor>
            </Stack>
        </Panel>
    }
}

/// The left-nav "Admin" entry, rendered only when whoami says the caller
/// is an org admin (route-level gating is [`AdminPage`]'s gate panel).
/// Markup matches `app.rs`'s `NavLink` so styling/active state stay uniform.
#[component]
pub fn AdminNavLink() -> impl IntoView {
    let auth = use_auth();
    let whoami = LocalResource::new(move || {
        let _ = auth.token();
        crate::api::whoami(auth)
    });
    // Memo is Copy, so the per-render attribute closures below can each
    // capture it without fighting over one captured `Location`.
    let pathname = use_location().pathname;
    view! {
        {move || match whoami.get() {
            Some(Ok(me)) if gating::can_access(&me) => view! {
                <a
                    class="kairos-nav__link"
                    href="/admin"
                    aria-current=move || {
                        let path = pathname.get();
                        (path == "/admin" || path.starts_with("/admin/")).then_some("page")
                    }
                >
                    "Admin"
                </a>
            }.into_any(),
            // Loading, error, or no admin access: no admin nav entry.
            _ => ().into_any(),
        }}
    }
}

/// Horizontal section tabs for the admin area (reuses the nav-link styling
/// from app.css; `aria-current` drives the active state). Non-admins with a
/// board-config grant see only Overview + Boards; the org-admin-only tabs
/// (teams, streams, members, templates, metadata) render for admins only
/// (KAIROS-T-0052).
#[component]
fn SectionTabs(is_admin: bool) -> impl IntoView {
    let pathname = use_location().pathname;
    let tab = move |href: &'static str, label: &'static str| {
        let current = move || {
            let path = pathname.get();
            let active = if href == "/admin" {
                path == "/admin"
            } else {
                path == href || path.starts_with(&format!("{href}/"))
            };
            active.then_some("page")
        };
        view! {
            <a class="kairos-nav__link" href=href aria-current=current>{label}</a>
        }
    };
    let admin_only = is_admin.then(|| {
        view! {
            <>
                {tab("/admin/teams", "Teams")}
                {tab("/admin/streams", "Streams")}
                {tab("/admin/repositories", "Repositories")}
                {tab("/admin/members", "Members")}
                {tab("/admin/templates", "Templates")}
                {tab("/admin/metadata", "Metadata")}
            </>
        }
    });
    view! {
        <Group gap="xs" wrap=true>
            {tab("/admin", "Overview")}
            {tab("/admin/boards", "Boards")}
            {admin_only}
        </Group>
    }
}

/// `/admin` — the overview: one card per admin surface. Non-admins with a
/// board-config grant see only the Boards card; the org-admin-only cards
/// render for admins (KAIROS-T-0052). The Boards card blurb adapts to the
/// caller's reach.
#[component]
pub fn AdminHomePage() -> impl IntoView {
    let auth = use_auth();
    let whoami = LocalResource::new(move || {
        let _ = auth.token();
        crate::api::whoami(auth)
    });
    let card = |href: &'static str, title: &'static str, blurb: &'static str| {
        view! {
            <Panel title=title>
                <Stack gap="xs">
                    <Text dimmed=true size="sm">{blurb}</Text>
                    <Anchor href=href>"Open"</Anchor>
                </Stack>
            </Panel>
        }
    };
    view! {
        <PageHeader title="Admin" sub="organization administration"/>
        {move || {
            // Default to the board-config-holder view until whoami resolves
            // (the shell gate already admitted the caller); expand to the full
            // grid only once confirmed org admin.
            let is_admin = matches!(whoami.get(), Some(Ok(me)) if me.organization.role == "admin");
            let boards_blurb = if is_admin {
                "Create and delete boards; configure columns, transitions, \
                 and per-board member capabilities."
            } else {
                "Configure columns, transitions, and member capabilities on \
                 the boards you administer."
            };
            let admin_only = is_admin.then(|| view! {
                <>
                    {card("/admin/teams", "Teams",
                        "Teams and their members; creating a team also creates its \
                         delivery board.")}
                    {card("/admin/streams", "Delivery streams",
                        "Delivery streams and which teams feed them.")}
                    {card("/admin/repositories", "Repositories",
                        "The codebases tickets are issued against — one owning team each — \
                         and their forge webhooks.")}
                    {card("/admin/members", "Organization members",
                        "Who belongs to this organization, and who is an admin.")}
                    {card("/admin/templates", "Templates",
                        "Document templates and the metadata fields they stamp.")}
                    {card("/admin/metadata", "Metadata definitions",
                        "Typed metadata fields (string, enum, date) items can carry.")}
                </>
            });
            view! {
                <SimpleGrid cols=2>
                    {card("/admin/boards", "Boards", boards_blurb)}
                    {admin_only}
                </SimpleGrid>
            }
        }}
    }
}

// ---------------------------------------------------------------------------
// Shared mutation plumbing for the admin panels
// ---------------------------------------------------------------------------

/// Outcome of the latest mutation in a panel: `Ok(what happened)` or the
/// mapped API error (typed 422s like COLUMN_NOT_EMPTY / LAST_ADMIN arrive
/// here with their server message + code).
pub(crate) type MutationOutcome = Option<Result<String, aurora_dark::tokens::ApiError>>;

/// Renders the latest mutation outcome per the conventions: success as a
/// section-scoped `Alert`, failure via `ErrorState` (which shows the
/// classified title, server message, and `code:` line for typed 422s).
#[component]
pub(crate) fn MutationNotice(outcome: RwSignal<MutationOutcome>) -> impl IntoView {
    use aurora_dark::components::{Alert, Button};
    use aurora_dark::tokens::token;
    view! {
        {move || outcome.get().map(|result| view! {
            <Stack gap="xs">
                {match result {
                    Ok(message) => view! {
                        <Alert title="Done" color=token::OK>
                            <Text size="sm">{message}</Text>
                        </Alert>
                    }.into_any(),
                    Err(error) => view! { <ErrorState error/> }.into_any(),
                }}
                <Group justify="end">
                    <Button variant="subtle" size="xs"
                        on_click=Callback::new(move |_| outcome.set(None))>
                        "Dismiss"
                    </Button>
                </Group>
            </Stack>
        })}
    }
}

/// Run one admin mutation: guard against double-submit with `busy`, record
/// the outcome (success message or API error) in `outcome`, and bump
/// `reload` on success so the owning `LocalResource` refetches (server
/// state is the source of truth — no local patching, per conventions).
pub(crate) fn run_mutation<F>(
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    success: String,
    fut: F,
) where
    F: std::future::Future<Output = Result<(), aurora_dark::tokens::ApiError>> + 'static,
{
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    leptos::task::spawn_local(async move {
        let result = fut.await;
        busy.set(false);
        match result {
            Ok(()) => {
                outcome.set(Some(Ok(success)));
                reload.update(|n| *n += 1);
            }
            Err(error) => outcome.set(Some(Err(error))),
        }
    });
}
