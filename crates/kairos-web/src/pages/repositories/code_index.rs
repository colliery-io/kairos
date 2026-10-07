//! The code index of a repository in the GUI (KAIROS-T-0333,
//! COLLIERY-I-0610): the newest index, the last runs of the builder with
//! their result, the indexed commits, and a button that asks for a new
//! index.
//!
//! The panel is under each repository on the team page and on the admin
//! page Repositories, closed until the reader opens it. Open, it reads
//! `GET …/code-indexes` and `GET …/code-indexes/builds?limit=5`, and it
//! listens to the tenant stream: a `code_index_build_changed` event whose
//! short code is the slug of the repository makes it read again, so a run
//! that starts or ends shows with no reload.
//!
//! The panel has `data-code-index-repo`, not `data-repo`: the row of the
//! repository has `data-repo` already, and the e2e specs select it alone.
//!
//! The button "Build again" is for a person with the right to change the
//! repository: an organization admin, or a member of the owner team
//! ([`can_rebuild`]). It is off while a run is active. A refusal of the
//! server is shown with its text.

use aurora_dark::components::{
    Alert, Button, Empty, ErrorState, Group, Loading, Pill, Stack, Text,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;

use crate::api::{self as shell_api, encode_component, get_json, post_empty};
use crate::auth::{Auth, use_auth};
use crate::pages::boards::live;
use crate::pages::item::api::error_text;

/// The runs that the panel shows.
const RUNS_SHOWN: i64 = 5;

/// The indexed commits that the panel shows before "Show all".
const INDEXES_SHOWN: usize = 10;

/// mirror of: `kairos_client::types_code_index::CodeIndex` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndex {
    pub commit: String,
    #[serde(default)]
    pub r#ref: Option<String>,
    /// `upload` or `build`.
    pub source: String,
    pub structure_bytes: i64,
    pub updated_at: String,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexBuild` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndexBuild {
    pub id: String,
    #[serde(default)]
    pub commit: Option<String>,
    /// `push`, `first`, `request` or `upload`.
    pub trigger: String,
    /// `running`, `ok` or `failed`.
    pub outcome: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub files: Option<i32>,
    #[serde(default)]
    pub symbols: Option<i32>,
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexBuildList`
/// (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndexBuildList {
    pub items: Vec<CodeIndexBuild>,
    pub total: i64,
}

/// What the panel shows: the 2 reads.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeIndexState {
    /// The indexed commits, newest write first.
    pub indexes: Vec<CodeIndex>,
    /// The runs, newest first.
    pub builds: CodeIndexBuildList,
}

impl CodeIndexState {
    /// Whether a run is active: the newest run is `running`.
    pub fn running(&self) -> bool {
        self.builds
            .items
            .first()
            .is_some_and(|run| run.outcome == "running")
    }
}

async fn read_state(auth: Auth, slug: &str) -> Result<CodeIndexState, ApiError> {
    let base = format!("/api/repositories/{}/code-indexes", encode_component(slug));
    let indexes = get_json(auth, &base).await?;
    let builds = get_json(auth, &format!("{base}/builds?limit={RUNS_SHOWN}")).await?;
    Ok(CodeIndexState { indexes, builds })
}

/// `POST /api/repositories/{slug}/code-indexes/rebuild`: the answer is the
/// `running` run.
async fn rebuild(auth: Auth, slug: &str) -> Result<CodeIndexBuild, ApiError> {
    post_empty(
        auth,
        &format!(
            "/api/repositories/{}/code-indexes/rebuild",
            encode_component(slug)
        ),
    )
    .await
}

/// Whether the person can ask for a build: an organization admin, or a
/// member of the owner team of the repository. The server has the same
/// gate; the button only hides a call that the server would refuse. Pure,
/// host-tested.
pub(crate) fn can_rebuild(me: &shell_api::Whoami, owner_team: &str) -> bool {
    me.organization.role == "admin" || me.teams.iter().any(|team| team.slug == owner_team)
}

/// The first 12 characters of a commit. Pure, host-tested.
pub(crate) fn short_commit(commit: &str) -> &str {
    let end = commit
        .char_indices()
        .nth(12)
        .map_or(commit.len(), |(index, _)| index);
    &commit[..end]
}

/// A size in bytes as text: `2.1 MB`, `640 kB`, `12 B`. Pure, host-tested.
pub(crate) fn size_text(bytes: i64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{} kB", bytes / 1_000)
    } else {
        format!("{bytes} B")
    }
}

/// One line for an indexed commit: the ref, the commit, the size, the
/// source and when it was written. Pure, host-tested.
pub(crate) fn index_line(index: &CodeIndex) -> String {
    format!(
        "{} · {} · {} · {} · {}",
        index.r#ref.as_deref().unwrap_or("no ref"),
        short_commit(&index.commit),
        size_text(index.structure_bytes),
        index.source,
        index.updated_at
    )
}

/// One line for a run: the trigger, when it started, its commit and the
/// symbols of the index it wrote. The outcome is the pill next to it, and
/// the error text is its own line. Pure, host-tested.
pub(crate) fn run_line(run: &CodeIndexBuild) -> String {
    let mut parts = vec![run.trigger.clone(), run.started_at.clone()];
    if let Some(commit) = &run.commit {
        parts.push(short_commit(commit).to_string());
    }
    if let Some(symbols) = run.symbols {
        parts.push(format!("{symbols} symbols"));
    }
    parts.join(" · ")
}

/// The color of the pill of an outcome. Pure, host-tested.
pub(crate) fn outcome_color(outcome: &str) -> &'static str {
    match outcome {
        "ok" => token::OK,
        "failed" => token::BAD,
        _ => token::GOLD,
    }
}

/// The code index of one repository: closed until the reader opens it.
#[component]
pub fn CodeIndexPanel(
    /// The slug of the repository.
    #[prop(into)]
    slug: String,
    /// The slug of the owner team of the repository: its members can ask
    /// for a build.
    #[prop(into)]
    owner_team: String,
) -> impl IntoView {
    let auth = use_auth();
    let slug = StoredValue::new(slug);
    let owner_team = StoredValue::new(owner_team);
    let open = RwSignal::new(false);
    let show_all = RwSignal::new(false);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    // `(ok, text)`: the answer of the last "Build again".
    let notice: RwSignal<Option<(bool, String)>> = RwSignal::new(None);
    let whoami = use_context::<LocalResource<Result<shell_api::Whoami, ApiError>>>();
    let allowed = Signal::derive(move || match whoami.and_then(|me| me.get()) {
        Some(Ok(me)) => can_rebuild(&me, &owner_team.get_value()),
        _ => false,
    });

    let state = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        let wanted = open.get();
        async move {
            if wanted {
                read_state(auth, &slug.get_value()).await.map(Some)
            } else {
                Ok(None)
            }
        }
    });
    let running =
        Signal::derive(move || matches!(state.get(), Some(Ok(Some(state))) if state.running()));

    // The live refresh: the tenant stream, while the panel is open. An
    // event about this repository, or a reconnect, reads again.
    let live_guard: StoredValue<Option<live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(None);
    Effect::new(move |_| {
        if !open.get() {
            live_guard.set_value(None);
            return;
        }
        if live_guard.with_value(|guard| guard.is_some()) {
            return;
        }
        let guard = live::subscribe_all_events(auth, move |event_code| {
            if event_code.is_none_or(|code| code == slug.get_value()) {
                reload.update(|n| *n += 1);
            }
        });
        live_guard.set_value(Some(guard));
    });
    on_cleanup(move || live_guard.set_value(None));

    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    let on_rebuild = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        notice.set(None);
        leptos::task::spawn_local(async move {
            let answer = rebuild(auth, &slug.get_value()).await;
            notice.set(Some(match answer {
                Ok(_) => (
                    true,
                    "Kairos builds the index again. The run is in the list, and it \
                     updates when the build ends."
                        .to_string(),
                ),
                Err(error) => (false, error_text(&error)),
            }));
            busy.set(false);
            reload.update(|n| *n += 1);
        });
    };

    view! {
        <Stack gap="xs" attr:data-testid="code-index-panel" attr:data-code-index-repo=slug.get_value()>
            <Group gap="xs">
                <Button variant="default" size="xs"
                    on_click=Callback::new(move |_| open.update(|open| *open = !*open))>
                    {move || if open.get() { "Hide code index" } else { "Show code index" }}
                </Button>
                <Show when=move || open.get() && allowed.get()>
                    <Button size="xs"
                        disabled=Signal::derive(move || busy.get() || running.get())
                        on_click=Callback::new(on_rebuild)
                        attr:data-testid="code-index-rebuild">
                        "Build again"
                    </Button>
                </Show>
            </Group>
            {move || notice.get().map(|(ok, text)| {
                let (title, color) = if ok {
                    ("Build asked", token::ICE)
                } else {
                    ("Kairos refused the build", token::BAD)
                };
                view! {
                    <Alert title=title color=color>
                        <Text size="sm" dimmed=true attr:data-testid="code-index-notice">{text}</Text>
                    </Alert>
                }
            })}
            <Show when=move || open.get()>
                {move || match state.get() {
                    None | Some(Ok(None)) => view! { <Loading label="Loading the code index…"/> }.into_any(),
                    Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                    Some(Ok(Some(state))) => {
                        let newest = state.indexes.first().map(index_line);
                        let count = state.indexes.len();
                        let runs = state.builds.items.clone();
                        let indexes = state.indexes.clone();
                        view! {
                            <Stack gap="xs">
                                {match newest {
                                    None => view! {
                                        <Text size="xs" dimmed=true attr:data-testid="code-index-newest">
                                            "No index yet."
                                        </Text>
                                    }.into_any(),
                                    Some(line) => view! {
                                        <Text size="xs" attr:data-testid="code-index-newest">
                                            {format!("Newest index: {line}")}
                                        </Text>
                                    }.into_any(),
                                }}
                                <Text size="xs" dimmed=true>"Last runs of the builder"</Text>
                                {if runs.is_empty() {
                                    view! {
                                        <Empty message="No run of the builder yet. Kairos builds the index \
                                                        after a push to the default branch, or when a person \
                                                        uses the button Build again."/>
                                    }.into_any()
                                } else {
                                    view! {
                                        <Stack gap="xs" attr:data-testid="code-index-runs">
                                            {runs.into_iter().map(|run| {
                                                let line = run_line(&run);
                                                let color = outcome_color(&run.outcome);
                                                let outcome_attr = run.outcome.clone();
                                                let outcome_text = run.outcome.clone();
                                                let error = run.error.clone();
                                                view! {
                                                    <Stack gap="xs" attr:data-run=run.id.clone() attr:data-outcome=outcome_attr>
                                                        <Group gap="sm" wrap=true>
                                                            <Pill color=color>{outcome_text}</Pill>
                                                            <Text size="xs">{line}</Text>
                                                        </Group>
                                                        {error.map(|text| view! {
                                                            <Text size="xs" dimmed=true attr:data-testid="code-index-run-error">{text}</Text>
                                                        })}
                                                    </Stack>
                                                }
                                            }).collect_view()}
                                        </Stack>
                                    }.into_any()
                                }}
                                {(count > 0).then(|| view! {
                                    <Text size="xs" dimmed=true>{format!("Indexed commits ({count})")}</Text>
                                    <Stack gap="xs" attr:data-testid="code-index-commits">
                                        {move || {
                                            let shown = if show_all.get() { count } else { INDEXES_SHOWN.min(count) };
                                            indexes.iter().take(shown).map(|index| {
                                                let line = index_line(index);
                                                view! {
                                                    <Text size="xs" attr:data-commit=index.commit.clone()>{line}</Text>
                                                }
                                            }).collect_view()
                                        }}
                                    </Stack>
                                    {(count > INDEXES_SHOWN).then(|| view! {
                                        <Button variant="default" size="xs"
                                            on_click=Callback::new(move |_| show_all.update(|all| *all = !*all))>
                                            {move || if show_all.get() { "Show fewer" } else { "Show all" }}
                                        </Button>
                                    })}
                                })}
                            </Stack>
                        }.into_any()
                    }
                }}
            </Show>
        </Stack>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(role: &str, teams: &[&str]) -> shell_api::Whoami {
        serde_json::from_value(serde_json::json!({
            "user": {"id": "u1", "display_name": "Alice", "email": "alice@kairos.test"},
            "organization": {"slug": "acme", "role": role},
            "teams": teams.iter().map(|slug| serde_json::json!({
                "id": format!("t-{slug}"), "slug": slug, "name": slug,
            })).collect::<Vec<_>>(),
        }))
        .expect("a whoami")
    }

    /// The gate of the button is the gate of the server: an organization
    /// admin, or a member of the owner team.
    #[test]
    fn an_admin_or_a_member_of_the_owner_team_can_ask() {
        assert!(can_rebuild(&me("admin", &[]), "platform"));
        assert!(can_rebuild(&me("member", &["platform"]), "platform"));
        assert!(!can_rebuild(&me("member", &["web"]), "platform"));
        assert!(!can_rebuild(&me("member", &[]), "platform"));
    }

    #[test]
    fn the_mirrors_decode_the_wire_shape() {
        let list: CodeIndexBuildList = serde_json::from_str(
            r#"{"items":[{"id":"r1","commit":"0123456789abcdef0123456789abcdef01234567",
                "trigger":"push","outcome":"ok","files":803,"symbols":7964,"edges":66896,
                "summaries_made":12,"started_at":"2026-10-07T06:00:00+00:00",
                "finished_at":"2026-10-07T06:01:00+00:00"},
               {"id":"r0","trigger":"request","outcome":"running",
                "requested_by":"u1","started_at":"2026-10-07T05:00:00+00:00"}],
              "total":2,"limit":5}"#,
        )
        .expect("the list decodes");
        assert_eq!(list.total, 2);
        assert_eq!(list.items[1].commit, None);
        assert_eq!(list.items[1].finished_at, None);
        let index: CodeIndex = serde_json::from_str(
            r#"{"commit":"0123456789abcdef0123456789abcdef01234567","ref":"main",
                "source":"build","structure_bytes":2100000,"summary_keys":3730,
                "created_at":"2026-10-07T06:00:00+00:00","updated_at":"2026-10-07T06:01:00+00:00"}"#,
        )
        .expect("the index decodes");
        assert_eq!(
            index_line(&index),
            "main · 0123456789ab · 2.1 MB · build · 2026-10-07T06:01:00+00:00"
        );
        assert_eq!(
            run_line(&list.items[0]),
            "push · 2026-10-07T06:00:00+00:00 · 0123456789ab · 7964 symbols"
        );
        assert_eq!(
            run_line(&list.items[1]),
            "request · 2026-10-07T05:00:00+00:00"
        );
        let state = CodeIndexState {
            indexes: vec![index],
            builds: list,
        };
        assert!(!state.running(), "the newest run is ok");
    }

    #[test]
    fn sizes_and_commits_are_short() {
        assert_eq!(size_text(12), "12 B");
        assert_eq!(size_text(640_000), "640 kB");
        assert_eq!(size_text(2_149_000), "2.1 MB");
        assert_eq!(short_commit("abc"), "abc");
        assert_eq!(short_commit("0123456789abcdef"), "0123456789ab");
        assert_eq!(outcome_color("ok"), token::OK);
        assert_eq!(outcome_color("failed"), token::BAD);
        assert_eq!(outcome_color("running"), token::GOLD);
    }
}
