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
//! The runs have pages of [`RUNS_SHOWN`] (KAIROS-T-0584): the pager under
//! them sends `offset`. On a page past the first, the panel reads the
//! newest run too, so "Build again" stays off while a run is active.
//!
//! The panel has `data-code-index-repo`, not `data-repo`: the row of the
//! repository has `data-repo` already, and the e2e specs select it alone.
//!
//! A run with a SCIP warning (KAIROS-T-0478) has one line for each language
//! that had no SCIP index, with the reason: its edges are by name. For an
//! organization admin, the panel ends with the disk use of the builder of
//! the server: the tools by language, the dependency caches and their cap,
//! and the free space.
//!
//! The button "Build again" is for a person with the right to change the
//! repository: an organization admin, or a member of the owner team
//! ([`can_rebuild`]). It is off while a run is active. A refusal of the
//! server is shown with its text.

use aurora_dark::components::{
    Alert, Button, Empty, ErrorState, Group, Loading, Pill, Stack, Text,
};
use aurora_dark::data::Pagination;
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;

use crate::api::{self as shell_api, encode_component, get_json, post_empty};
use crate::auth::{Auth, use_auth};
use crate::pages::boards::live;
use crate::pages::item::api::error_text;

/// The runs of a page of the panel.
const RUNS_SHOWN: usize = 5;

/// The indexed commits that the panel shows before "Show all".
const INDEXES_SHOWN: usize = 10;

/// mirror of: `kairos_client::types_code_index::CodeIndex` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndex {
    pub commit: String,
    #[serde(default)]
    pub r#ref: Option<String>,
    /// Each ref that points at the commit now (KAIROS-T-0750).
    #[serde(default)]
    pub refs: Vec<CodeIndexRef>,
    /// `upload` or `build`.
    pub source: String,
    pub structure_bytes: i64,
    pub updated_at: String,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexRef`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndexRef {
    pub name: String,
    /// `branch`, `tag` or `pull`.
    pub kind: String,
}

impl CodeIndexRef {
    /// The name, with `(tag)` after a tag (KAIROS-T-0750).
    fn text(&self) -> String {
        if self.kind == "tag" {
            format!("{} (tag)", self.name)
        } else {
            self.name.clone()
        }
    }
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
    /// `provider/model` of the summaries of the run.
    #[serde(default)]
    pub model: Option<String>,
    /// The languages with no SCIP index in the run (KAIROS-T-0478).
    #[serde(default)]
    pub scip_warnings: Vec<ScipWarning>,
    /// The summaries that the index of the run does not have
    /// (KAIROS-T-0495). The next build makes them.
    #[serde(default)]
    pub summaries_missing: Option<i32>,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexScipWarning`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ScipWarning {
    pub language: String,
    pub reason: String,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexDiskUse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DiskUse {
    #[serde(default)]
    pub tools: Vec<ToolUse>,
    #[serde(default)]
    pub caches: Vec<CacheUse>,
    pub caches_bytes: u64,
    pub caches_cap_bytes: u64,
    #[serde(default)]
    pub free_bytes: Option<u64>,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexToolUse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ToolUse {
    pub name: String,
    #[serde(default)]
    pub language: Option<String>,
    pub bytes: u64,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexCacheUse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CacheUse {
    pub language: String,
    pub repositories: u32,
    pub bytes: u64,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexBuildList`
/// (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndexBuildList {
    pub items: Vec<CodeIndexBuild>,
    pub total: i64,
    /// The runs before the page (KAIROS-T-0584).
    #[serde(default)]
    pub offset: i64,
    /// Set when no code index builder runs (KAIROS-T-0394).
    #[serde(default)]
    pub builder: Option<String>,
    /// The disk use of the tools and the caches (KAIROS-T-0478).
    #[serde(default)]
    pub disk: Option<DiskUse>,
}

/// What the panel shows: the 2 reads.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeIndexState {
    /// The indexed commits, newest write first.
    pub indexes: Vec<CodeIndex>,
    /// A page of the runs, newest first.
    pub builds: CodeIndexBuildList,
    /// The newest run of the repository, on any page (KAIROS-T-0584).
    pub newest_run: Option<CodeIndexBuild>,
}

impl CodeIndexState {
    /// Whether a run is active: the newest run is `running`.
    pub fn running(&self) -> bool {
        self.newest_run
            .as_ref()
            .is_some_and(|run| run.outcome == "running")
    }
}

/// The URL of a page of the runs. Pure, host-tested.
fn runs_url(base: &str, offset: usize) -> String {
    if offset == 0 {
        format!("{base}/builds?limit={RUNS_SHOWN}")
    } else {
        format!("{base}/builds?limit={RUNS_SHOWN}&offset={offset}")
    }
}

async fn read_state(auth: Auth, slug: &str, offset: usize) -> Result<CodeIndexState, ApiError> {
    let base = format!("/api/repositories/{}/code-indexes", encode_component(slug));
    let indexes = get_json(auth, &base).await?;
    let builds: CodeIndexBuildList = get_json(auth, &runs_url(&base, offset)).await?;
    let newest_run = if offset == 0 {
        builds.items.first().cloned()
    } else {
        get_json::<CodeIndexBuildList>(auth, &format!("{base}/builds?limit=1"))
            .await?
            .items
            .into_iter()
            .next()
    };
    Ok(CodeIndexState {
        indexes,
        builds,
        newest_run,
    })
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

/// A size in bytes as text: `1.4 GB`, `2.1 MB`, `640 kB`, `12 B`. Pure,
/// host-tested.
pub(crate) fn size_text(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
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
    let refs = if index.refs.is_empty() {
        index.r#ref.clone().unwrap_or_else(|| "no ref".to_string())
    } else {
        index
            .refs
            .iter()
            .map(CodeIndexRef::text)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} · {} · {} · {} · {}",
        refs,
        short_commit(&index.commit),
        size_text(index.structure_bytes),
        index.source,
        index.updated_at
    )
}

/// One line for a run: the trigger, when it started, its commit, the
/// symbols of the index it wrote, the model and the summaries that the
/// index does not have (KAIROS-T-0495). The outcome is the pill next to it, and
/// the error text is its own line. Pure, host-tested.
pub(crate) fn run_line(run: &CodeIndexBuild) -> String {
    let mut parts = vec![run.trigger.clone(), run.started_at.clone()];
    if let Some(commit) = &run.commit {
        parts.push(short_commit(commit).to_string());
    }
    if let Some(symbols) = run.symbols {
        parts.push(format!("{symbols} symbols"));
    }
    if let Some(model) = &run.model {
        parts.push(model.clone());
    }
    if let Some(missing) = run.summaries_missing.filter(|n| *n > 0) {
        parts.push(format!("{missing} summaries missing"));
    }
    parts.join(" · ")
}

/// The line of a SCIP warning of a run (KAIROS-T-0478). Pure, host-tested.
pub(crate) fn warning_line(warning: &ScipWarning) -> String {
    format!(
        "No SCIP index for {}: {}. Its edges are by name.",
        warning.language, warning.reason
    )
}

fn size_of_u64(bytes: u64) -> String {
    size_text(i64::try_from(bytes).unwrap_or(i64::MAX))
}

/// The lines of the disk use of the builder (KAIROS-T-0478): the tools by
/// language, the dependency caches by language and their cap, and the free
/// space. Pure, host-tested.
pub(crate) fn disk_lines(disk: &DiskUse) -> Vec<String> {
    let mut by_language: std::collections::BTreeMap<&str, (u64, Vec<&str>)> = Default::default();
    for tool in &disk.tools {
        let language = tool.language.as_deref().unwrap_or("other files");
        let entry = by_language.entry(language).or_default();
        entry.0 += tool.bytes;
        if tool.name != "other" {
            entry.1.push(&tool.name);
        }
    }
    let mut lines: Vec<String> = by_language
        .into_iter()
        .map(|(language, (bytes, names))| {
            if names.is_empty() {
                format!("Tools, {language}: {}", size_of_u64(bytes))
            } else {
                format!(
                    "Tools, {language}: {} ({})",
                    size_of_u64(bytes),
                    names.join(", ")
                )
            }
        })
        .collect();
    let caches: Vec<String> = disk
        .caches
        .iter()
        .map(|c| {
            let repos = if c.repositories == 1 {
                "1 repository".to_string()
            } else {
                format!("{} repositories", c.repositories)
            };
            format!("{} {} in {repos}", c.language, size_of_u64(c.bytes))
        })
        .collect();
    let mut cache_line = format!(
        "Dependency caches: {} of {}",
        size_of_u64(disk.caches_bytes),
        size_of_u64(disk.caches_cap_bytes)
    );
    if !caches.is_empty() {
        cache_line.push_str(&format!(" ({})", caches.join(", ")));
    }
    lines.push(cache_line);
    if let Some(free) = disk.free_bytes {
        lines.push(format!("Free disk space: {}", size_of_u64(free)));
    }
    lines
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
    // KAIROS-T-0584: the page of the runs.
    let runs_offset = RwSignal::new(0usize);
    let runs_limit = RwSignal::new(RUNS_SHOWN);
    let busy = RwSignal::new(false);
    // `(ok, text)`: the answer of the last "Build again".
    let notice: RwSignal<Option<(bool, String)>> = RwSignal::new(None);
    let whoami = use_context::<LocalResource<Result<shell_api::Whoami, ApiError>>>();
    let allowed = Signal::derive(move || match whoami.and_then(|me| me.get()) {
        Some(Ok(me)) => can_rebuild(&me, &owner_team.get_value()),
        _ => false,
    });

    let state = LocalResource::new(move || {
        auth.track_session();
        let _ = reload.get();
        let wanted = open.get();
        let offset = runs_offset.get();
        async move {
            if wanted {
                read_state(auth, &slug.get_value(), offset).await.map(Some)
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
                        let runs_total = usize::try_from(state.builds.total).unwrap_or(0);
                        let indexes = state.indexes.clone();
                        let builder = state.builds.builder.clone();
                        // The disk use is for an organization admin.
                        let is_admin = matches!(
                            whoami.and_then(|me| me.get()),
                            Some(Ok(me)) if me.organization.role == "admin"
                        );
                        let disk = state.builds.disk.clone().filter(|_| is_admin);
                        view! {
                            <Stack gap="xs">
                                {builder.map(|note| view! {
                                    <Text size="xs" attr:data-testid="code-index-no-builder">{note}</Text>
                                })}
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
                                <Text size="xs" dimmed=true>"Runs of the builder, newest first"</Text>
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
                                                let warnings = run.scip_warnings.clone();
                                                view! {
                                                    <Stack gap="xs" attr:data-run=run.id.clone() attr:data-outcome=outcome_attr>
                                                        <Group gap="sm" wrap=true>
                                                            <Pill color=color>{outcome_text}</Pill>
                                                            <Text size="xs">{line}</Text>
                                                        </Group>
                                                        {error.map(|text| view! {
                                                            <Text size="xs" dimmed=true attr:data-testid="code-index-run-error">{text}</Text>
                                                        })}
                                                        {warnings.into_iter().map(|w| {
                                                            let language = w.language.clone();
                                                            view! {
                                                                <Text size="xs" attr:data-testid="code-index-run-warning" attr:data-language=language>
                                                                    {warning_line(&w)}
                                                                </Text>
                                                            }
                                                        }).collect_view()}
                                                    </Stack>
                                                }
                                            }).collect_view()}
                                        </Stack>
                                    }.into_any()
                                }}
                                {(runs_total > RUNS_SHOWN).then(|| view! {
                                    <div data-testid="code-index-runs-pager">
                                        <Pagination offset=runs_offset limit=runs_limit
                                            total=runs_total
                                            range_label=Callback::new(|range| shell_api::page_range_text(&range))/>
                                    </div>
                                })}
                                {disk.map(|disk| view! {
                                    <Text size="xs" dimmed=true>"Disk use of the builder"</Text>
                                    <Stack gap="xs" attr:data-testid="code-index-disk">
                                        {disk_lines(&disk).into_iter().map(|line| view! {
                                            <Text size="xs">{line}</Text>
                                        }).collect_view()}
                                    </Stack>
                                })}
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

    /// KAIROS-T-0478: a run with a SCIP warning, and the disk use, decode
    /// and give their lines.
    #[test]
    fn the_warnings_and_the_disk_use_decode_and_read() {
        let list: CodeIndexBuildList = serde_json::from_str(
            r#"{"items":[{"id":"r1","trigger":"push","outcome":"ok","started_at":"t",
                "scip_warnings":[{"language":"go","reason":"go mod download failed"}]}],
               "total":1,"limit":5,
               "disk":{"tools":[{"name":"go-1.27.2","language":"go","bytes":250000000},
                                {"name":"scip-go-0.2.7","language":"go","bytes":13000000},
                                {"name":"other","bytes":2000}],
                       "caches":[{"language":"go","repositories":2,"bytes":120000000}],
                       "caches_bytes":120000000,"caches_cap_bytes":10737418240,
                       "free_bytes":70000000000}}"#,
        )
        .expect("a list");
        let warning = &list.items[0].scip_warnings[0];
        assert_eq!(
            warning_line(warning),
            "No SCIP index for go: go mod download failed. Its edges are by name."
        );
        assert_eq!(
            disk_lines(list.disk.as_ref().unwrap()),
            [
                "Tools, go: 263.0 MB (go-1.27.2, scip-go-0.2.7)",
                "Tools, other files: 2 kB",
                "Dependency caches: 120.0 MB of 10.7 GB (go 120.0 MB in 2 repositories)",
                "Free disk space: 70.0 GB",
            ]
        );
        // A run of an older server has no warnings.
        let old: CodeIndexBuild =
            serde_json::from_str(r#"{"id":"r0","trigger":"push","outcome":"ok","started_at":"t"}"#)
                .unwrap();
        assert!(old.scip_warnings.is_empty());
    }

    #[test]
    fn the_note_of_no_builder_decodes() {
        let list: CodeIndexBuildList = serde_json::from_str(
            r#"{"items":[],"total":0,"limit":5,"builder":"No code index builder runs."}"#,
        )
        .expect("the list decodes");
        assert_eq!(list.builder.as_deref(), Some("No code index builder runs."));
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
        assert_eq!(list.builder, None);
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
        // KAIROS-T-0495: a run with missing summaries says how many.
        let missing: CodeIndexBuild = serde_json::from_str(
            r#"{"id":"r2","trigger":"first","outcome":"ok","started_at":"t","symbols":10,
                "model":"ollama-cloud/m","summaries_missing":7}"#,
        )
        .unwrap();
        assert_eq!(
            run_line(&missing),
            "first · t · 10 symbols · ollama-cloud/m · 7 summaries missing"
        );
        let newest_run = list.items.first().cloned();
        let state = CodeIndexState {
            indexes: vec![index],
            builds: list.clone(),
            newest_run,
        };
        assert!(!state.running(), "the newest run is ok");
        // KAIROS-T-0584: on a page past the first, the newest run decides.
        let past_first = CodeIndexState {
            newest_run: list.items.get(1).cloned(),
            ..state
        };
        assert!(past_first.running(), "the newest run is running");
    }

    /// KAIROS-T-0584: a page of the runs sends its offset.
    #[test]
    fn a_page_of_the_runs_sends_its_offset() {
        let base = "/api/repositories/site/code-indexes";
        assert_eq!(runs_url(base, 0), format!("{base}/builds?limit=5"));
        assert_eq!(runs_url(base, 5), format!("{base}/builds?limit=5&offset=5"));
        let page: CodeIndexBuildList =
            serde_json::from_str(r#"{"items":[],"total":12,"limit":5,"offset":10}"#)
                .expect("a page decodes");
        assert_eq!((page.total, page.offset), (12, 10));
        assert_eq!(
            shell_api::page_range_text(&aurora_dark::data::page_range(10, 5, 12)),
            "The list shows items 11 to 12 of 12."
        );
    }

    /// KAIROS-T-0750: the line shows each ref of the commit with its kind,
    /// else the ref of the build, else "no ref".
    #[test]
    fn the_index_line_shows_the_refs() {
        let mut index = CodeIndex {
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            r#ref: Some("main".into()),
            refs: vec![
                CodeIndexRef {
                    name: "main".into(),
                    kind: "branch".into(),
                },
                CodeIndexRef {
                    name: "v1.0.0".into(),
                    kind: "tag".into(),
                },
            ],
            source: "build".into(),
            structure_bytes: 12,
            updated_at: "2026-11-07T00:00:00Z".into(),
        };
        assert_eq!(
            index_line(&index),
            "main, v1.0.0 (tag) · 0123456789ab · 12 B · build · 2026-11-07T00:00:00Z"
        );
        index.refs.clear();
        assert!(index_line(&index).starts_with("main · "));
        index.r#ref = None;
        assert!(index_line(&index).starts_with("no ref · "));
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
