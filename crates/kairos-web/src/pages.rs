//! Route pages (KAIROS-T-0039). One `*Page` component per route; the
//! T-0040..T-0044 fan-out replaces each stub in place (grow a page into
//! `src/pages/<name>.rs` + submodule when it outgrows a screenful — keep
//! the `pages::XxxPage` export stable so `app.rs` never changes shape).
//!
//! Stub convention: `PageHeader` (real title) + `Panel` + `Empty` naming
//! the task that fills it — so a half-built deployment is honest about
//! what's missing, and every stub already demonstrates the page skeleton
//! later tasks should keep.

use aurora_dark::components::{Anchor, Button, Loading, Stack, Text, TextInput};
use aurora_dark::frame::{AuthCard, CenterScreen};
use leptos::prelude::*;
use leptos_router::components::Redirect;

use crate::app::BrandMark;
use crate::auth::{self, use_auth};

// ---- auth pages ----------------------------------------------------------

/// Explicit sign-in page: logout lands here, and so does any unauthenticated
/// visitor on a deployment that cannot start an SSO redirect.
///
/// Three deployment shapes, and all three have to look deliberate rather than
/// half-configured (KAIROS-T-0205):
///
/// | Deployment | What renders |
/// |---|---|
/// | an issuer only | the provider button, exactly as before |
/// | local accounts only | the password form, and no provider button |
/// | both | the provider first, a separator, then the form |
///
/// The provider goes first when there are both because it is the path an
/// organization with an issuer wants its people to take; the form is the fallback for
/// a break-glass admin.
///
/// Deliberately no authenticated-redirect: a logout arrives here *before* the session
/// clears (see `LogoutButton`), and an authenticated visitor pressing "Sign in" is
/// harmless.
#[component]
pub fn LoginPage() -> impl IntoView {
    let auth = use_auth();
    let config = LocalResource::new(move || async move { auth.config_cached().await });

    // COLLIERY-T-1836: the Aurora `CenterScreen` + `AuthCard`.
    view! {
        <CenterScreen>
            <AuthCard
                title="Kairos"
                sub="flight levels, running"
                brand=Box::new(|| view! { <BrandMark/> }.into_any())
            >
                <Stack gap="sm" center=true>
                    {move || match config.get() {
                        None => view! { <Loading label="Checking how to sign in…"/> }.into_any(),
                        // A failed /api/config still offers the provider button: it is
                        // the path that worked before this page could ask, and an error
                        // with no way forward is worse than a button that might work.
                        Some(Err(message)) => view! {
                            <Stack gap="sm" center=true>
                                <ProviderButton/>
                                <Text size="xs" dimmed=true>{message}</Text>
                            </Stack>
                        }.into_any(),
                        Some(Ok(loaded)) => {
                            let sso = loaded.can_sso();
                            let local = loaded.local_auth;
                            view! {
                                {sso.then(|| view! { <ProviderButton/> })}
                                {(sso && local).then(|| view! {
                                    <Text size="xs" dimmed=true>"or"</Text>
                                })}
                                {local.then(|| view! { <PasswordForm/> })}
                                {(!sso && !local).then(|| view! {
                                    <Text size="sm" dimmed=true>
                                        "This deployment has no way to sign in configured. \
                                         An operator needs to set an OIDC issuer or turn on \
                                         local accounts."
                                    </Text>
                                })}
                            }.into_any()
                        }
                    }}
                </Stack>
            </AuthCard>
        </CenterScreen>
    }
}

/// The "sign in with your identity provider" button: starts PKCE.
#[component]
fn ProviderButton() -> impl IntoView {
    let auth = use_auth();
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let sign_in = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        leptos::task::spawn_local(async move {
            let return_to = auth::take_return_to();
            if let Err(message) = auth::begin_login(auth, &return_to).await {
                error.set(Some(message));
                busy.set(false);
            }
        });
    };

    view! {
        <Stack gap="xs" center=true>
            <Text dimmed=true size="sm">
                "Sign in with your organization's identity provider."
            </Text>
            <Button loading=busy loading_label="Redirecting…" on_click=Callback::new(sign_in)>
                "Sign in"
            </Button>
            {move || error.get().map(|message| view! {
                <Text size="xs" dimmed=true>{message}</Text>
            })}
        </Stack>
    }
}

/// The email/password form, for a deployment with local accounts
/// (KAIROS-T-0205, KAIROS-T-0203).
///
/// COLLIERY-T-1836: Aurora 0.4 inputs have the props that a login form
/// needs (`input_type`, `autocomplete`, `name`, `required`), so the form
/// uses `TextInput` and a `Button` with `button_type="submit"` in a real
/// `<form>`, so that Enter submits and a password manager recognises it.
/// Each label points to its control, so `getByLabel` finds the fields.
#[component]
fn PasswordForm() -> impl IntoView {
    let auth = use_auth();
    let navigate = leptos_router::hooks::use_navigate();
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let submit = move |event: leptos::ev::SubmitEvent| {
        // The browser would navigate and lose the SPA otherwise. A real form is still
        // the right element: it is what makes Enter work and what a password manager
        // looks for.
        event.prevent_default();
        if busy.get() {
            return;
        }
        let navigate = navigate.clone();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result =
                auth::password_login(auth, &email.get_untracked(), &password.get_untracked()).await;
            match result {
                Ok(()) => {
                    // Clear the password from the signal before leaving the page. It is
                    // in memory either way, but there is no reason for it to outlive the
                    // request that used it.
                    password.set(String::new());
                    navigate(&auth::take_return_to(), Default::default());
                }
                Err(message) => {
                    // Rendered VERBATIM (KAIROS-T-0203): the server's 401 is one message
                    // for a wrong password, an unknown email and an OIDC-only account.
                    // Guessing "no account with that email" here would rebuild the
                    // enumeration oracle the endpoint was careful to avoid.
                    error.set(Some(message));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <form class="kairos-login-form" on:submit=submit>
            <Stack gap="xs">
                <TextInput
                    label="Email"
                    value=email
                    input_type="email"
                    name="email"
                    autocomplete="email"
                    required=true
                    spellcheck=false
                />
                <TextInput
                    label="Password"
                    value=password
                    input_type="password"
                    name="password"
                    autocomplete="current-password"
                    required=true
                />
                // The endpoint is intentionally slow — argon2 — so a form with no
                // feedback reads as broken rather than as working.
                //
                // "Log in", NOT "Sign in" and not "Sign in with password": on a
                // deployment with both paths there would otherwise be two buttons
                // whose accessible names overlap, which is ambiguous for a person
                // choosing between them and for anything addressing the page by role
                // and name. It broke two existing e2e specs, which is how it was
                // found — and "with password" was not enough, because accessible-name
                // matching is substring by default. "Log in" also pairs with the
                // header's "Log out".
                <Button button_type="submit" loading=busy loading_label="Logging in…">
                    "Log in"
                </Button>
                {move || error.get().map(|message| view! {
                    <Text size="xs" dimmed=true>{message}</Text>
                })}
            </Stack>
        </form>
    }
}

/// PKCE redirect target: exchanges the code (via the server relay), then
/// forwards to wherever the user was originally headed.
#[component]
pub fn CallbackPage() -> impl IntoView {
    let auth = use_auth();
    let exchange = LocalResource::new(move || auth::complete_login(auth));
    view! {
        <CenterScreen>
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
        </CenterScreen>
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

// Repositories (KAIROS-I-0010, A-0019): the shared repository data layer
// (`repositories::api`) that boards, item detail, team pages and
// `admin::api` all consume. No route of its own yet.
pub(crate) mod repositories;

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

// The agent keys of the signed-in person (KAIROS-T-0359): make, list and
// revoke the keys that let an agent act as the person.
mod agent_keys;
pub use agent_keys::AgentKeysPage;

/// Router fallback.
#[component]
pub fn NotFoundPage() -> impl IntoView {
    view! {
        <CenterScreen>
            <Stack gap="sm" center=true>
                <Text bright=true bold=true>"Nothing here"</Text>
                <Text dimmed=true size="sm">"That page does not exist."</Text>
                <Anchor href="/boards">"Back to boards"</Anchor>
            </Stack>
        </CenterScreen>
    }
}
