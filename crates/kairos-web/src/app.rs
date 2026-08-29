//! Root component (KAIROS-T-0039): router, the protected app shell
//! (nav + whoami + logout), and the login-redirect guard.
//!
//! Route map (the T-0040..T-0044 fan-out fills the stubs in [`crate::pages`]):
//!
//! | path            | page                 | lands with |
//! |-----------------|----------------------|------------|
//! | `/login`        | explicit sign-in     | here       |
//! | `/callback`     | PKCE redirect target | here       |
//! | `/`             | redirect → `/boards` | here       |
//! | `/boards`       | board list           | T-0040     |
//! | `/boards/:board`| board view           | T-0040     |
//! | `/items/:code`  | item detail          | T-0041     |
//! | `/search`       | unified search       | T-0042     |
//! | `/search/relationships/:code` | relationships explorer | T-0042 |
//! | `/teams`        | team directory       | T-0067     |
//! | `/teams/:slug`  | team detail (roster, board, streams) | T-0067 |
//! | `/admin`        | admin overview       | T-0043     |
//! | `/admin/boards` | board list + create/delete | T-0043 |
//! | `/admin/boards/:board` | board config (columns, transitions, members) | T-0043 |
//! | `/admin/teams`  | teams CRUD + members | T-0043     |
//! | `/admin/streams`| delivery streams CRUD + teams | T-0043 |
//! | `/admin/members`| org members + roles  | T-0043     |
//! | `/admin/templates` | templates CRUD    | T-0043     |
//! | `/admin/metadata`  | metadata definitions CRUD | T-0043 |
//! | `/activity`     | activity feed        | T-0044     |
//! | `/activity/history/:code` | item content history | T-0044 |
//!
//! Everything under the shell is auth-guarded: no session → the guard
//! *redirects to the issuer* (A-0015 "unauthenticated hits show the login
//! redirect"), remembering the requested path for after the callback.

use aurora_dark::AuroraStyles;
use aurora_dark::components::{AppShell, Button, Group, Loading, Pill, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::components::{Outlet, ParentRoute, Redirect, Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;

use crate::api;
use crate::auth::{self, use_auth};
use crate::pages;

/// The application root: provides auth, injects the Aurora stylesheet,
/// declares the route tree.
#[component]
pub fn App() -> impl IntoView {
    auth::provide_auth();
    view! {
        <AuroraStyles/>
        <div class="kairos-root">
            <Router>
                <Routes fallback=pages::NotFoundPage>
                    <Route path=path!("/login") view=pages::LoginPage/>
                    <Route path=path!("/callback") view=pages::CallbackPage/>
                    <ParentRoute path=path!("") view=Shell>
                        <Route path=path!("") view=|| view! { <Redirect path="/boards"/> }/>
                        <Route path=path!("boards") view=pages::BoardsPage/>
                        <Route path=path!("boards/:board") view=pages::BoardPage/>
                        <Route path=path!("items/:code") view=pages::ItemPage/>
                        <Route path=path!("search") view=pages::SearchPage/>
                        <Route path=path!("search/relationships/:code") view=pages::RelationshipsPage/>
                        <Route path=path!("teams") view=pages::TeamsPage/>
                        <Route path=path!("teams/:slug") view=pages::TeamPage/>
                        <Route path=path!("teams/:slug/pages/*path") view=pages::TeamDocPage/>
                        <ParentRoute path=path!("admin") view=pages::AdminPage>
                            <Route path=path!("") view=pages::admin::AdminHomePage/>
                            <Route path=path!("boards") view=pages::admin::AdminBoardsPage/>
                            <Route path=path!("boards/:board") view=pages::admin::AdminBoardPage/>
                            <Route path=path!("teams") view=pages::admin::AdminTeamsPage/>
                            <Route path=path!("streams") view=pages::admin::AdminStreamsPage/>
                            <Route path=path!("members") view=pages::admin::AdminMembersPage/>
                            <Route path=path!("templates") view=pages::admin::AdminTemplatesPage/>
                            <Route path=path!("metadata") view=pages::admin::AdminMetadataPage/>
                        </ParentRoute>
                        <Route path=path!("activity") view=pages::ActivityPage/>
                        <Route path=path!("activity/history/:code") view=pages::ItemHistoryPage/>
                    </ParentRoute>
                </Routes>
            </Router>
        </div>
    }
}

/// The protected shell: aurora `AppShell` with the Kairos header
/// (brand + whoami + logout) and left nav. Unauthenticated → login
/// redirect via [`RedirectToIssuer`].
///
/// Whoami is fetched ONCE here and shared (`LocalResource` is `Copy`) by
/// the header badge and the "My teams" nav section (KAIROS-T-0068) — one
/// identity round-trip per session, not one per consumer.
#[component]
fn Shell() -> impl IntoView {
    let auth = use_auth();
    let whoami = LocalResource::new(move || {
        // Track the session so a fresh login refetches.
        let _ = auth.token();
        api::whoami(auth)
    });
    // Pages under the shell consume the same identity (KAIROS-T-0072: the
    // board view derives its capability mirror from it).
    provide_context(whoami);
    view! {
        <Show when=move || auth.is_authenticated() fallback=GuardFallback>
            <AppShell
                header=Box::new(move || view! {
                    <Group justify="between">
                        <Group gap="sm">
                            <BrandMark/>
                            <Text bright=true bold=true>"Kairos"</Text>
                        </Group>
                        <Group gap="sm">
                            <WhoamiBadge whoami/>
                            <LogoutButton/>
                        </Group>
                    </Group>
                }.into_any())
                navbar=Box::new(move || view! {
                    <Stack gap="xs">
                        <NavLink href="/boards" label="Boards"/>
                        <NavLink href="/teams" label="Teams"/>
                        <NavLink href="/search" label="Search"/>
                        <NavLink href="/activity" label="Activity"/>
                        <pages::admin::AdminNavLink/>
                        <MyTeamsNav whoami/>
                    </Stack>
                }.into_any())
            >
                <Outlet/>
            </AppShell>
        </Show>
    }
}

/// "My teams" (KAIROS-T-0068): the caller's own teams from whoami, each
/// linking to its `/teams/:slug` page. Hidden entirely (no empty-state)
/// for users with no team memberships; whoami load/error states render
/// nothing — the header badge already surfaces those.
#[component]
fn MyTeamsNav(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView {
    view! {
        {move || {
            let teams = match whoami.get() {
                Some(Ok(me)) => me.teams,
                _ => Vec::new(),
            };
            (!teams.is_empty()).then(|| view! {
                <div class="kairos-nav__section">
                    <Text dimmed=true size="xs">"My teams"</Text>
                    <Stack gap="xs">
                        {teams.into_iter().map(|team| view! {
                            <NavLink
                                href=format!("/teams/{}", team.slug)
                                label=team.name
                            />
                        }).collect_view()}
                    </Stack>
                </div>
            })
        }}
    }
}

/// The unauthenticated fallback for the protected shell: while a boot-time
/// session restore is in flight (KAIROS-T-0071 — same-tab reload with a
/// stored refresh token), just wait; after an explicit logout, land on
/// `/login`; otherwise (fresh visit, expired session) run the A-0015
/// issuer redirect.
#[component]
fn GuardFallback() -> impl IntoView {
    let auth = use_auth();
    view! {
        <Show
            when=move || !auth.restoring()
            fallback=|| view! {
                <div class="kairos-center-screen">
                    <Loading label="Restoring session…"/>
                </div>
            }
        >
            <Show when=move || auth.signed_out() fallback=RedirectToIssuer>
                <Redirect path="/login"/>
            </Show>
        </Show>
    }
}

/// The unauthenticated fallback: kick off the PKCE redirect (remembering
/// where the user was headed) and show progress while the browser leaves.
#[component]
fn RedirectToIssuer() -> impl IntoView {
    let auth = use_auth();
    let error = RwSignal::new(None::<String>);
    let location = use_location();
    let return_to = location.pathname.get_untracked();

    // Logout race guard: this component can mount for one tick while a
    // logout is navigating to /login (the session clears before the route
    // swaps). The spawned redirect polls after unmount — cancel it there,
    // or a logout would bounce straight back to the issuer.
    let cancelled = StoredValue::new(false);
    on_cleanup(move || cancelled.set_value(true));

    Effect::new(move |_| {
        let return_to = return_to.clone();
        leptos::task::spawn_local(async move {
            if cancelled.get_value() {
                return;
            }
            if let Err(message) = auth::begin_login(auth, &return_to).await {
                error.set(Some(message));
            }
        });
    });

    view! {
        <div class="kairos-center-screen">
            {move || match error.get() {
                None => view! { <Loading label="Redirecting to sign-in…"/> }.into_any(),
                Some(message) => view! {
                    <Stack gap="sm" center=true>
                        <Text bright=true bold=true>"Sign-in unavailable"</Text>
                        <Text dimmed=true size="sm">{message}</Text>
                    </Stack>
                }.into_any(),
            }}
        </div>
    }
}

/// One left-nav entry. Plain anchors are fine: the leptos router
/// intercepts same-origin clicks, and `aria-current` drives the active
/// style (app.css).
#[component]
fn NavLink(#[prop(into)] href: String, #[prop(into)] label: String) -> impl IntoView {
    let location = use_location();
    let target = href.clone();
    let current = move || {
        let path = location.pathname.get();
        (path == target || path.starts_with(&format!("{target}/"))).then_some("page")
    };
    view! {
        <a class="kairos-nav__link" href=href aria-current=current>{label}</a>
    }
}

/// Who am I, which org, which role — the A-0015 whoami display. Follows
/// the async-view convention (loading → error → value). Consumes the
/// shell's shared whoami resource.
#[component]
fn WhoamiBadge(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView {
    view! {
        {move || match whoami.get() {
            None => view! { <Text dimmed=true size="sm">"…"</Text> }.into_any(),
            Some(Err(_)) => view! {
                <Pill color=token::BAD>"whoami unavailable"</Pill>
            }.into_any(),
            Some(Ok(me)) => view! {
                <Group gap="sm">
                    <Text bright=true size="sm">{me.user.display_name.clone()}</Text>
                    <Pill color=token::ICE>{me.organization.slug.clone()}</Pill>
                    <Pill color=token::VIOLET>{me.organization.role.clone()}</Pill>
                </Group>
            }.into_any(),
        }}
    }
}

/// Drop the in-memory session; the shell guard (which sees the explicit
/// sign-out) then redirects to `/login`.
#[component]
fn LogoutButton() -> impl IntoView {
    let auth = use_auth();
    view! {
        <Button
            variant="default"
            size="xs"
            on_click=Callback::new(move |_| auth.logout())
        >
            "Log out"
        </Button>
    }
}

/// The Kairos brand mark — app-supplied (aurora ships no branding), drawn
/// with token colors only (`style=` so CSS variables apply to SVG).
#[component]
pub fn BrandMark() -> impl IntoView {
    view! {
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M6 4 v16" style="stroke:var(--ice);" stroke-width="2.2" stroke-linecap="round"/>
            <path d="M18 5 L8.5 13" style="stroke:var(--teal);" stroke-width="2.2" stroke-linecap="round"/>
            <path d="M10.5 12 L18 19" style="stroke:var(--violet);" stroke-width="2.2" stroke-linecap="round"/>
        </svg>
    }
}
