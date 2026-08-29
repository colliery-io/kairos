//! Route pages (KAIROS-T-0039). One `*Page` component per route; the
//! T-0040..T-0044 fan-out replaces each stub in place (grow a page into
//! `src/pages/<name>.rs` + submodule when it outgrows a screenful — keep
//! the `pages::XxxPage` export stable so `app.rs` never changes shape).
//!
//! Stub convention: `PageHeader` (real title) + `Panel` + `Empty` naming
//! the task that fills it — so a half-built deployment is honest about
//! what's missing, and every stub already demonstrates the page skeleton
//! later tasks should keep.

use aurora_dark::components::{Anchor, Loading, Panel, Stack, Text};
use leptos::prelude::*;
use leptos_router::components::Redirect;

use crate::app::BrandMark;
use crate::auth::{self, use_auth};

// ---- auth pages ----------------------------------------------------------

/// Explicit sign-in page: logout lands here; a button restarts PKCE.
/// (Unauthenticated deep links never see this — the shell guard redirects
/// straight to the issuer.) Deliberately no authenticated-redirect: a
/// logout arrives here *before* the session clears (see `LogoutButton`),
/// and an authenticated visitor pressing "Sign in" is harmless.
#[component]
pub fn LoginPage() -> impl IntoView {
    let auth = use_auth();
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let sign_in = move |_| {
        busy.set(true);
        leptos::task::spawn_local(async move {
            if let Err(message) = auth::begin_login(auth, "/").await {
                error.set(Some(message));
                busy.set(false);
            }
        });
    };

    view! {
        <div class="kairos-center-screen">
            <Panel title="Kairos" caption="flight levels, running">
                <Stack gap="sm" center=true>
                    <BrandMark/>
                    <Text dimmed=true size="sm">
                        "Sign in with your organization's identity provider."
                    </Text>
                    <button
                        class="cl-btn cl-btn--filled"
                        disabled=move || busy.get()
                        on:click=sign_in
                    >
                        {move || if busy.get() { "Redirecting…" } else { "Sign in" }}
                    </button>
                    {move || error.get().map(|message| view! {
                        <Text size="xs" dimmed=true>{message}</Text>
                    })}
                </Stack>
            </Panel>
        </div>
    }
}

/// PKCE redirect target: exchanges the code (via the server relay), then
/// forwards to wherever the user was originally headed.
#[component]
pub fn CallbackPage() -> impl IntoView {
    let auth = use_auth();
    let exchange = LocalResource::new(move || auth::complete_login(auth));
    view! {
        <div class="kairos-center-screen">
            {move || match exchange.get() {
                None => view! { <Loading label="Completing sign-in…"/> }.into_any(),
                Some(Ok(return_to)) => view! { <Redirect path=return_to/> }.into_any(),
                Some(Err(message)) => view! {
                    <Stack gap="sm" center=true>
                        <Text bright=true bold=true>"Sign-in failed"</Text>
                        <Text dimmed=true size="sm">{message}</Text>
                        <Anchor href="/login">"Try again"</Anchor>
                    </Stack>
                }.into_any(),
            }}
        </div>
    }
}

// ---- feature stubs (T-0040..T-0044 replace these) -------------------------

// Copy-link button (KAIROS-T-0076): shared by the board cards and the
// item detail header.
pub(crate) mod copy_link;

// Boards area (KAIROS-T-0040): board list + board view live in their own
// submodule (the "grew past a screenful" rule); exports stay stable.
mod boards;
pub use boards::{BoardPage, BoardsPage};

// Item detail (KAIROS-T-0041): detail per entity type, markdown edit +
// preview, 409 conflict merge, metadata, create-from-template, soft delete.
pub(crate) mod item;
pub use item::ItemPage;

// Unified search + relationships explorer (KAIROS-T-0042): the A-0007
// composable query page and the graph view at /search/relationships/:code.
mod search;
pub use search::{RelationshipsPage, SearchPage};

// Team pages (KAIROS-T-0067, KAIROS-I-0006): the /teams directory and the
// /teams/:slug detail (roster, delivery board, streams) — the user-facing
// team lens. Its `api` submodule is the shared team data layer that
// `admin::api` re-exports from.
pub(crate) mod editor;

pub(crate) mod teams;
pub use teams::doc::TeamDocPage;
pub use teams::{TeamPage, TeamsPage};

// Admin surfaces (KAIROS-T-0043): board configuration + members/capability
// grants, teams, delivery streams, org members, templates, metadata
// definitions. Sub-routes under /admin are registered in `app.rs`.
pub mod admin;
pub use admin::AdminPage;

// Activity + item history (KAIROS-T-0044): the audit-trail feed and the
// A-0004 version list / snapshot / diff / rollback view. The history route
// (`/activity/history/:code`) is the target of T-0041's "history link".
mod activity;
pub use activity::{ActivityPage, ItemHistoryPage};

/// Router fallback.
#[component]
pub fn NotFoundPage() -> impl IntoView {
    view! {
        <div class="kairos-center-screen">
            <Stack gap="sm" center=true>
                <Text bright=true bold=true>"Nothing here"</Text>
                <Text dimmed=true size="sm">"That page does not exist."</Text>
                <Anchor href="/boards">"Back to boards"</Anchor>
            </Stack>
        </div>
    }
}
