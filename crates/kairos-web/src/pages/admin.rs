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
//!   or `administer_members`, incl. globs like `configure_*`/`manage_*`/`*`) on
//!   at least one board now reach the admin section too, but only its
//!   per-board configuration surface (`/admin/boards` + `/admin/boards/:id`).
//!   The org-admin-only surfaces (teams, streams, org members, templates,
//!   metadata, and board create/delete) stay hidden from them.
//! - **Holders of `administer_members` on a board** also see the Members tab
//!   (KAIROS-T-0413, [`gating::may_assign_members`]): the list of the
//!   organization members, with no org-admin control, and a link to the
//!   grants page of each person (`/admin/members/:user`). That page shows
//!   them only the boards that they administer.
//! - **Everyone else** (a plain member with no grants) gets the graceful
//!   "admin access required" panel instead of a broken screen.
//!
//! The nav entry ([`AdminNavLink`]) follows the same [`gating::can_access`]
//! rule. Any server 403 that slips through (e.g. a grant revoked mid-session)
//! still renders per-section via `ErrorState`/[`MutationNotice`] — the GUI
//! gate is UX, the server is the authority (A-0006).

use aurora_dark::components::{
    Alert, Anchor, ErrorState, Group, Loading, PageHeader, Panel, SimpleGrid, Stack, Table, Text,
};
use aurora_dark::frame::{TabItem, Tabs};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::components::Outlet;
use leptos_router::hooks::use_location;

pub(crate) mod gating;

mod agent_keys;
pub(crate) mod api;
mod board_policy;
mod boards;
mod capabilities;
mod code_index;
mod dispatch;
mod member_grants;
mod members;
mod metadata;
mod repositories;
mod service_accounts;
mod streams;
mod teams;
mod templates;

pub use boards::{AdminBoardPage, AdminBoardsPage};

/// The id of the members panel in the admin page of a board
/// (KAIROS-T-0412). The board page links to `/admin/boards/<id>#members`.
pub(crate) const MEMBERS_ANCHOR: &str = "members";
pub use code_index::AdminCodeIndexPage;
pub use member_grants::AdminMemberGrantsPage;
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
    let whoami = crate::app::use_whoami();
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
                let assigns_members = gating::may_assign_members(&me);
                view! {
                    <Stack gap="md">
                        <SectionTabs is_admin assigns_members/>
                        <Outlet/>
                    </Stack>
                }.into_any()
            }
        }}
    }
}

/// The graceful denied path: what a plain `member` with no board-config
/// grants sees on a direct visit to any `/admin` route (the nav never shows
/// Admin to them). Holders of `configure_boards`/`administer_members` no longer
/// land here — they get the boards surface (KAIROS-T-0052).
#[component]
fn NotAdminGate(role: String) -> impl IntoView {
    view! {
        <PageHeader title="Admin" sub="organization administration"/>
        <Panel title="Admin access required" caption="You do not have access.">
            <Stack gap="sm">
                <Text>
                    "To administer the organization, you must have the role "<b>"admin"</b>
                    ", or the grant "<b>"configure_boards"</b>" or "
                    <b>"administer_members"</b>" on a board. Your role is "
                    <b>{role}</b>", and you have no such grant."
                </Text>
                <Text dimmed=true size="sm">
                    "To configure a board here, ask an organization admin for a
                     capability on that board."
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
    let whoami = crate::app::use_whoami();
    view! {
        {move || match whoami.get() {
            Some(Ok(me)) if gating::can_access(&me) => view! {
                <crate::app::NavLink href="/admin" label="Admin"/>
            }.into_any(),
            // Loading, error, or no admin access: no admin nav entry.
            _ => ().into_any(),
        }}
    }
}

/// The section tabs of the admin area: Aurora route `Tabs`
/// (COLLIERY-T-1836; links with `role="tab"`, the route selects the tab).
/// Non-admins with a board-config grant see only Overview + Boards; the
/// org-admin-only tabs (teams, streams, repositories, code index, members,
/// templates, metadata)
/// render for admins only (KAIROS-T-0052). A holder of
/// `administer_members` on a board also sees Members (KAIROS-T-0413): the
/// way to the grants page of a person.
#[component]
fn SectionTabs(is_admin: bool, assigns_members: bool) -> impl IntoView {
    let pathname = use_location().pathname;
    let mut sections: Vec<(&'static str, &'static str)> =
        vec![("/admin", "Overview"), ("/admin/boards", "Boards")];
    if is_admin {
        sections.extend([
            ("/admin/teams", "Teams"),
            ("/admin/streams", "Streams"),
            ("/admin/repositories", "Repositories"),
            ("/admin/code-index", "Code index"),
            ("/admin/members", "Members"),
            ("/admin/templates", "Templates"),
            ("/admin/metadata", "Metadata"),
        ]);
    } else if assigns_members {
        sections.push(("/admin/members", "Members"));
    }
    let hrefs: Vec<&'static str> = sections.iter().map(|(href, _)| *href).collect();
    let current = move || {
        let path = pathname.get();
        hrefs
            .iter()
            .rev()
            .find(|href| {
                if **href == "/admin" {
                    path == "/admin"
                } else {
                    path == **href || path.starts_with(&format!("{href}/"))
                }
            })
            .map(|href| href.to_string())
            .unwrap_or_default()
    };
    let value = RwSignal::new(current());
    Effect::new(move |_| value.set(current()));
    let tabs = sections
        .into_iter()
        .map(|(href, label)| TabItem::new(href, label).href(href))
        .collect();
    view! { <Tabs tabs value label="Admin sections"/> }
}

/// `/admin` — the overview: one card per admin surface. Non-admins with a
/// board-config grant see only the Boards card; the org-admin-only cards
/// render for admins (KAIROS-T-0052). The Boards card blurb adapts to the
/// caller's reach.
#[component]
pub fn AdminHomePage() -> impl IntoView {
    let whoami = crate::app::use_whoami();
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
                        "The delivery streams, and the teams of each stream.")}
                    {card("/admin/repositories", "Repositories",
                        "The codebases that tasks link to — one owning team each — \
                         and their forge webhooks.")}
                    {card("/admin/code-index", "Code index",
                        "Where the summaries and the vectors of the code index are made: \
                         the embedded model, Ollama Cloud or Bedrock.")}
                    {card("/admin/members", "Organization members",
                        "The members of this organization, its admins, and the agent keys of \
                         each person.")}
                    {card("/admin/templates", "Templates",
                        "The document templates, and the metadata fields that they give.")}
                    {card("/admin/metadata", "Metadata definitions",
                        "The typed metadata fields (string, enum, date) that an item can have.")}
                </>
            });
            view! {
                {is_admin.then(|| view! { <RetentionPanel/> })}
                {is_admin.then(|| view! { <JobsPanel/> })}
                <SimpleGrid cols=2>
                    {card("/admin/boards", "Boards", boards_blurb)}
                    {admin_only}
                </SimpleGrid>
            }
        }}
    }
}

/// mirror of: `kairos_client::types_org::RetentionStatus` (partial).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct RetentionStatus {
    mode: String,
    #[serde(default)]
    archive_target: Option<String>,
    status: String,
    #[serde(default)]
    warning: Option<String>,
    #[serde(default)]
    last_result: Option<String>,
    #[serde(default)]
    last_run_at: Option<String>,
}

/// The line of the overview for the retention settings (KAIROS-T-0631).
fn retention_line(status: &RetentionStatus) -> String {
    let target = status.archive_target.as_deref().unwrap_or("none");
    let last = match (&status.last_result, &status.last_run_at) {
        (Some(result), Some(at)) => format!("Last pass {at}: {result}."),
        (Some(result), None) => format!("Last pass: {result}."),
        _ => "No pass ended yet.".to_string(),
    };
    format!("Mode {}, archive target {target}. {last}", status.mode)
}

/// The retention settings of the deployment on the overview
/// (KAIROS-T-0631): an alert when the pass removes nothing or fails by its
/// settings, else one line.
#[component]
fn RetentionPanel() -> impl IntoView {
    let auth = crate::auth::use_auth();
    let retention = LocalResource::new(move || {
        auth.track_session();
        crate::api::get_json::<RetentionStatus>(auth, "/api/org/retention")
    });
    view! {
        {move || match retention.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(e)) => view! { <ErrorState error=e/> }.into_any(),
            Some(Ok(status)) => {
                let line = retention_line(&status);
                match status.warning.clone() {
                    Some(warning) => view! {
                        <Alert title="Retention removes nothing" color=token::GOLD>
                            <Stack gap="xs">
                                <Text size="sm">{warning}</Text>
                                <Text dimmed=true size="sm">{line}</Text>
                            </Stack>
                        </Alert>
                    }.into_any(),
                    None => view! {
                        <Panel title="Retention">
                            <Text dimmed=true size="sm">{format!("{line} Status {}.", status.status)}</Text>
                        </Panel>
                    }.into_any(),
                }
            }
        }}
    }
}

/// mirror of: `kairos_client::types_org::JobKindCounts`.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct JobKindCounts {
    kind: String,
    queued: i64,
    leased: i64,
    done: i64,
    failed: i64,
}

/// mirror of: `kairos_client::types_org::JobFailure` (partial).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct JobFailure {
    kind: String,
    #[serde(default)]
    tenant: Option<String>,
    tries: i32,
    max_tries: i32,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    finished_at: Option<String>,
}

/// mirror of: `kairos_client::types_org::JobQueueStatus`.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct JobQueueStatus {
    kinds: Vec<JobKindCounts>,
    failures: Vec<JobFailure>,
}

/// The head line of a failed job on the overview (KAIROS-T-0576): the
/// time, the kind, the organization and the try.
fn failure_line(failure: &JobFailure) -> String {
    format!(
        "{}, {}, {}, try {} of {}",
        failure.finished_at.as_deref().unwrap_or("no end time"),
        failure.kind,
        failure.tenant.as_deref().unwrap_or("deployment"),
        failure.tries,
        failure.max_tries,
    )
}

/// The job queue on the overview (KAIROS-T-0576): the jobs of each kind in
/// each state, and the last failures with their error
/// (`GET /api/admin/jobs`).
#[component]
fn JobsPanel() -> impl IntoView {
    let auth = crate::auth::use_auth();
    let jobs = LocalResource::new(move || {
        auth.track_session();
        crate::api::get_json::<JobQueueStatus>(auth, "/api/admin/jobs")
    });
    view! {
        {move || match jobs.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(e)) => view! { <ErrorState error=e/> }.into_any(),
            Some(Ok(status)) => {
                let kinds = if status.kinds.is_empty() {
                    view! { <Text dimmed=true size="sm">"No jobs."</Text> }.into_any()
                } else {
                    let rows = status
                        .kinds
                        .into_iter()
                        .map(|kind| view! {
                            <tr>
                                <td><Text mono=true size="sm">{kind.kind}</Text></td>
                                <td>{kind.queued}</td>
                                <td>{kind.leased}</td>
                                <td>{kind.done}</td>
                                <td>{kind.failed}</td>
                            </tr>
                        })
                        .collect_view();
                    view! {
                        <Table label="Jobs by kind and state">
                            <thead>
                                <tr>
                                    <th>"Kind"</th>
                                    <th>"Queued"</th>
                                    <th>"Leased"</th>
                                    <th>"Done"</th>
                                    <th>"Failed"</th>
                                </tr>
                            </thead>
                            <tbody>{rows}</tbody>
                        </Table>
                    }
                    .into_any()
                };
                let failures = if status.failures.is_empty() {
                    view! { <Text dimmed=true size="sm">"No failed jobs."</Text> }.into_any()
                } else {
                    status
                        .failures
                        .into_iter()
                        .map(|failure| {
                            let line = failure_line(&failure);
                            let error = failure
                                .error
                                .unwrap_or_else(|| "No error recorded.".to_string());
                            view! {
                                <Stack gap="xs">
                                    <Text size="sm">{line}</Text>
                                    <Text mono=true dimmed=true size="xs">{error}</Text>
                                </Stack>
                            }
                        })
                        .collect_view()
                        .into_any()
                };
                view! {
                    <Panel title="Job queue">
                        <Stack gap="sm">
                            {kinds}
                            <Text size="sm">"Last failures, newest first"</Text>
                            {failures}
                        </Stack>
                    </Panel>
                }
                .into_any()
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

/// The rule of the slug of a board, a team or a delivery stream, below the
/// field `Slug` of a form (COLLIERY-T-0265). A mirror of
/// `kairos_core::slug::is_valid_slug`: this crate does not depend on
/// `kairos-core`.
pub(crate) const SLUG_HINT: &str = "Slug: 2 to 63 characters. The first character is a lowercase \
                                    letter. Each other character is a lowercase letter, a \
                                    digit, - or _. The slug cannot have the form of a UUID.";

/// The rule of the slug of a repository (COLLIERY-T-0265). A mirror of
/// `kairos_core::repositories::is_valid_slug`. The slug comes from the
/// name on the forge, and that name can start with a digit.
pub(crate) const REPOSITORY_SLUG_HINT: &str = "Slug: 2 to 63 characters. The first character is \
                                               a lowercase letter or a digit. Each other \
                                               character is a lowercase letter, a digit or -. \
                                               The slug cannot have the form of a UUID.";

/// The fields of a form that has a slug only.
pub(crate) const SLUG_FIELD: [&str; 1] = ["slug"];

/// The rule of the short-code prefix of a board, below the field `Prefix`
/// of a form (COLLIERY-T-3099). A mirror of
/// `kairos_core::short_code::is_valid_prefix`.
pub(crate) const PREFIX_HINT: &str = "Prefix: 2 to 10 characters. The first character is a \
                                      capital letter. Each other character is a capital letter \
                                      or a digit. Each item on the board gets a code with the \
                                      prefix, for example SKADI-T-0001. The prefix does not \
                                      change later.";

/// The fields of a form that makes a board: the slug and the prefix
/// (COLLIERY-T-3099).
pub(crate) const BOARD_FORM_FIELDS: [&str; 2] = ["slug", "code_prefix"];

/// Whether a form with the fields `fields` shows `refusal` below one of
/// them (COLLIERY-T-0265). Each other refusal goes to the notice of the
/// page. Pure, host-tested.
pub(crate) fn is_form_refusal(refusal: &api::Refusal, fields: &[&str]) -> bool {
    fields
        .iter()
        .any(|field| refusal.message_for(field).is_some())
}

/// [`run_mutation`] for a form (COLLIERY-T-0267, COLLIERY-T-0265). A
/// refusal about a field of `fields` goes to `refused`, and the form shows
/// it below the field. Each other refusal goes to `outcome`.
pub(crate) fn run_form_mutation<F>(
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    refused: RwSignal<Option<api::Refusal>>,
    fields: &'static [&'static str],
    success: String,
    fut: F,
) where
    F: std::future::Future<Output = Result<(), api::Refusal>> + 'static,
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
                refused.set(None);
                outcome.set(Some(Ok(success)));
                reload.update(|n| *n += 1);
            }
            Err(refusal) if is_form_refusal(&refusal, fields) => {
                outcome.set(None);
                refused.set(Some(refusal));
            }
            Err(refusal) => {
                refused.set(None);
                outcome.set(Some(Err(refusal.error)));
            }
        }
    });
}

/// The message of the refusal in `refused` for `field`, or an empty text.
pub(crate) fn refusal_for(
    refused: RwSignal<Option<api::Refusal>>,
    field: &'static str,
) -> Signal<String> {
    // KAIROS-T-0326: Aurora 0.4.1 takes `error` as a signal, so a field
    // shows a new refusal with no re-render of its own.
    Signal::derive(move || {
        refused
            .with(|refusal| refusal.as_ref().and_then(|r| r.message_for(field)))
            .unwrap_or_default()
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_dark::tokens::ApiError;

    fn refusal(status: u16, field: Option<&str>) -> api::Refusal {
        api::Refusal {
            error: ApiError::Http {
                status,
                message: "The team slug \"Road Map\" is not correct.".to_string(),
                code: Some("VALIDATION".to_string()),
            },
            field: field.map(str::to_string),
        }
    }

    /// KAIROS-T-0631: the line names the mode, the target and the last pass.
    #[test]
    fn the_retention_line_names_the_mode_the_target_and_the_last_pass() {
        let status: RetentionStatus = serde_json::from_value(serde_json::json!({
            "mode": "archive",
            "status": "no_target",
            "warning": "KAIROS_ARCHIVE_TARGET is not set.",
            "history_hot_days": 90,
            "history_keep_latest": 5,
            "activity_retention_days": 365,
            "last_result": "skipped: no target",
            "last_run_at": "2026-10-17T12:00:00+00:00"
        }))
        .expect("the wire shape decodes");
        assert_eq!(status.status, "no_target");
        assert_eq!(
            retention_line(&status),
            "Mode archive, archive target none. Last pass 2026-10-17T12:00:00+00:00: \
             skipped: no target."
        );
    }

    /// KAIROS-T-0576: the wire shape of `GET /api/admin/jobs` decodes, and
    /// the line of a failure names the time, the kind, the organization
    /// and the try.
    #[test]
    fn the_jobs_decode_and_a_failure_line_names_its_job() {
        let status: JobQueueStatus = serde_json::from_value(serde_json::json!({
            "kinds": [
                {"kind": "periodic", "queued": 1, "leased": 0, "done": 4, "failed": 0}
            ],
            "failures": [
                {
                    "id": "00000000-0000-0000-0000-000000000001",
                    "kind": "webhook_delivery",
                    "tenant": "acme",
                    "tries": 5,
                    "max_tries": 5,
                    "error": "The target answered 500.",
                    "finished_at": "2026-10-17T12:00:00+00:00"
                },
                {"id": "00000000-0000-0000-0000-000000000002", "kind": "periodic",
                 "tries": 1, "max_tries": 3}
            ]
        }))
        .expect("the wire shape decodes");
        assert_eq!(status.kinds[0].done, 4);
        assert_eq!(
            failure_line(&status.failures[0]),
            "2026-10-17T12:00:00+00:00, webhook_delivery, acme, try 5 of 5"
        );
        assert_eq!(
            failure_line(&status.failures[1]),
            "no end time, periodic, deployment, try 1 of 3"
        );
    }

    /// COLLIERY-T-0265: a refusal about the slug shows below the field
    /// `Slug`. Each other refusal goes to the notice of the page.
    #[test]
    fn a_refusal_about_the_slug_goes_to_the_field() {
        let about_slug = refusal(422, Some("slug"));
        assert!(is_form_refusal(&about_slug, &SLUG_FIELD));
        assert_eq!(
            about_slug.message_for("slug").as_deref(),
            Some("The team slug \"Road Map\" is not correct.")
        );
        // A conflict names no field: the slug of a live team.
        assert!(!is_form_refusal(&refusal(409, None), &SLUG_FIELD));
        // A field that the form does not have.
        assert!(!is_form_refusal(
            &refusal(422, Some("team_type")),
            &SLUG_FIELD
        ));
        assert!(is_form_refusal(
            &refusal(422, Some("slug")),
            &["repo_url", "slug"]
        ));
        // An error with no message of the server.
        let network = api::Refusal {
            error: ApiError::Network,
            field: Some("slug".to_string()),
        };
        assert!(!is_form_refusal(&network, &SLUG_FIELD));
    }

    /// COLLIERY-T-0265: the two rules of a slug are different, and each
    /// form gives the rule of its slug. The texts are mirrors of
    /// `kairos_core::slug` and `kairos_core::repositories`.
    #[test]
    fn each_form_gives_the_rule_of_its_slug() {
        assert!(SLUG_HINT.contains("The first character is a lowercase letter."));
        assert!(SLUG_HINT.contains("a digit, - or _."));
        assert!(REPOSITORY_SLUG_HINT.contains("a lowercase letter or a digit."));
        assert!(REPOSITORY_SLUG_HINT.contains("a digit or -."));
        assert!(!REPOSITORY_SLUG_HINT.contains('_'));
        for hint in [SLUG_HINT, REPOSITORY_SLUG_HINT] {
            assert!(hint.contains("2 to 63 characters"));
            assert!(hint.contains("cannot have the form of a UUID"));
        }
    }
}
