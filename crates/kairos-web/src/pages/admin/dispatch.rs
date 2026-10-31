//! The agent dispatch section of the board settings (KAIROS-T-0384,
//! KAIROS-T-0385): the webhook targets (make one and see its secret once,
//! turn one on again, send a test, remove), the dispatch rules (make,
//! remove), and the delivery log with each try.
//!
//! The delivery log has pages of [`LOG_PAGE`] deliveries (KAIROS-T-0584).
//! "Older" sends the id of the last delivery of the page as `before`.
//! The page asks for one delivery more than it shows, so it knows the
//! last page. "Newer" goes back one page.
//!
//! The server decides the rights: a board manager writes, a person who
//! belongs to the board reads. A refusal shows in the notice of the page.

use aurora_dark::components::{
    Alert, Button, Code, CopyButton, Divider, ErrorState, Group, Loading, Panel, Pill, Select,
    Stack, Switch, Table, Text, TextInput,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{MutationOutcome, run_mutation};
use crate::api::{delete_json, get_json, post_empty, post_json, put_json};
use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types_dispatch::WebhookTarget` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub consecutive_failures: i32,
    pub disabled_reason: Option<String>,
}

/// mirror of: `kairos_client::types_dispatch::CreatedWebhookTarget`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Created {
    target: Target,
    secret: String,
}

/// mirror of: `kairos_client::types_dispatch::DispatchRule` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Rule {
    pub id: String,
    /// `entered_column` or `unblocked` (KAIROS-T-0427).
    pub trigger: String,
    /// `None`: an `unblocked` rule with no column.
    pub column: Option<String>,
    pub target: String,
    pub task_type: Option<String>,
    pub metadata_slug: Option<String>,
    pub metadata_value: Option<String>,
    pub repository: Option<String>,
}

/// mirror of: `kairos_client::types_dispatch::WebhookDeliveryTry` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Try {
    pub try_number: i32,
    pub at: String,
    pub ok: bool,
    pub response_code: Option<i32>,
    pub error: Option<String>,
}

/// mirror of: `kairos_client::types_dispatch::WebhookDelivery` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub event: String,
    pub short_code: Option<String>,
    pub status: String,
    pub tries: i32,
    pub response_code: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub log: Vec<Try>,
}

/// The task types of a rule; the first is no filter.
const TASK_TYPES: [&str; 5] = ["any type", "task", "bug", "tech_debt", "support"];

/// The triggers of a rule, in words: the first is `entered_column`.
const TRIGGERS: [&str; 2] = ["enters the column", "is unblocked"];

/// The column option of an `unblocked` rule with no column.
const ANY_COLUMN: &str = "any column";

/// When a rule fires, in words. Pure, host-tested.
fn when_words(rule: &Rule) -> String {
    match (rule.trigger.as_str(), rule.column.as_deref()) {
        ("unblocked", Some(column)) => format!("in {column} that is unblocked"),
        ("unblocked", None) => "that is unblocked".to_string(),
        (_, column) => format!("that enters {}", column.unwrap_or("?")),
    }
}

/// The filter of a rule in words. Pure, host-tested.
fn filter_words(rule: &Rule) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &rule.task_type {
        parts.push(format!("type {t}"));
    }
    if let (Some(slug), Some(value)) = (&rule.metadata_slug, &rule.metadata_value) {
        parts.push(format!("{slug} = {value}"));
    }
    if let Some(repo) = &rule.repository {
        parts.push(format!("repository {repo}"));
    }
    if parts.is_empty() {
        "each task".to_string()
    } else {
        parts.join(", ")
    }
}

/// The body of a new rule. `metadata` is `slug=value` or empty. Pure,
/// host-tested.
fn rule_body(
    trigger: &str,
    column: &str,
    target_id: &str,
    task_type: &str,
    metadata: &str,
    repository: &str,
) -> Result<Value, String> {
    let mut body = json!({ "target_id": target_id });
    if trigger == TRIGGERS[1] {
        body["trigger"] = json!("unblocked");
    }
    if column != ANY_COLUMN {
        body["column"] = json!(column);
    } else if trigger != TRIGGERS[1] {
        return Err("A rule that fires when a task enters a column needs a column.".to_string());
    }
    if task_type != TASK_TYPES[0] && !task_type.is_empty() {
        body["task_type"] = json!(task_type);
    }
    let metadata = metadata.trim();
    if !metadata.is_empty() {
        match metadata.split_once('=') {
            Some((slug, value)) if !slug.trim().is_empty() && !value.trim().is_empty() => {
                body["metadata_slug"] = json!(slug.trim());
                body["metadata_value"] = json!(value.trim());
            }
            _ => {
                return Err(format!(
                    "The metadata filter {metadata:?} is not slug=value. Write for example \
                     priority=high."
                ));
            }
        }
    }
    if !repository.trim().is_empty() {
        body["repository"] = json!(repository.trim());
    }
    Ok(body)
}

/// The color of a status. Pure.
fn status_color(status: &str) -> &'static str {
    match status {
        "delivered" => token::OK,
        "failed" => token::BAD,
        "pending" => token::GOLD,
        _ => token::MUTED,
    }
}

async fn targets(auth: Auth, board: &str) -> Result<Vec<Target>, ApiError> {
    get_json(auth, &format!("/api/boards/{board}/webhook-targets")).await
}

async fn rules(auth: Auth, board: &str) -> Result<Vec<Rule>, ApiError> {
    get_json(auth, &format!("/api/boards/{board}/dispatch-rules")).await
}

/// The deliveries of a page of the log.
const LOG_PAGE: usize = 50;

/// The pages of the log that the person went through (KAIROS-T-0584): the
/// cursor of each page before the current one. Empty: the first page.
#[derive(Clone, Debug, Default, PartialEq)]
struct LogPages {
    cursors: Vec<String>,
}

impl LogPages {
    /// The `before` of the current page.
    fn before(&self) -> Option<&str> {
        self.cursors.last().map(String::as_str)
    }

    /// The number of the current page, from 1.
    fn number(&self) -> usize {
        self.cursors.len() + 1
    }

    /// Go to the next page: the deliveries older than `last_id`.
    fn older(&mut self, last_id: String) {
        self.cursors.push(last_id);
    }

    /// Go back one page.
    fn newer(&mut self) {
        self.cursors.pop();
    }
}

/// The URL of a page of the log: one delivery more than [`LOG_PAGE`], so
/// the page knows when it is the last. Pure, host-tested.
fn log_url(board: &str, before: Option<&str>) -> String {
    let mut url = format!(
        "/api/boards/{board}/webhook-deliveries?limit={}",
        LOG_PAGE + 1
    );
    if let Some(before) = before {
        url.push_str(&format!("&before={}", crate::api::encode_component(before)));
    }
    url
}

/// A page of the log, and whether an older page is there.
#[derive(Clone, Debug, PartialEq)]
struct LogPage {
    deliveries: Vec<Delivery>,
    more: bool,
}

/// The page from the answer to [`log_url`]: the first [`LOG_PAGE`]
/// deliveries. Pure, host-tested.
fn log_page(mut deliveries: Vec<Delivery>) -> LogPage {
    let more = deliveries.len() > LOG_PAGE;
    deliveries.truncate(LOG_PAGE);
    LogPage { deliveries, more }
}

async fn deliveries(auth: Auth, board: &str, before: Option<String>) -> Result<LogPage, ApiError> {
    get_json(auth, &log_url(board, before.as_deref()))
        .await
        .map(log_page)
}

/// mirror of: `kairos_client::types_dispatch::AgentTriage` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct AgentTriage {
    agents_decide: bool,
}

async fn agent_triage(auth: Auth, board: &str) -> Result<AgentTriage, ApiError> {
    get_json(auth, &format!("/api/boards/{board}/agent-triage")).await
}

/// The data of the section, in one load.
#[derive(Clone, Debug, PartialEq)]
struct Data {
    targets: Vec<Target>,
    rules: Vec<Rule>,
    /// KAIROS-T-0428: an agent decides the triage of intake.
    agents_decide: bool,
}

async fn load(auth: Auth, board: String) -> Result<Data, ApiError> {
    Ok(Data {
        targets: targets(auth, &board).await?,
        rules: rules(auth, &board).await?,
        agents_decide: agent_triage(auth, &board).await?.agents_decide,
    })
}

/// The agent dispatch section of a board.
///
/// Its changes reload only this section, with a reload signal of its own
/// (KAIROS-T-0387). A reload of the page makes a new section, and the
/// secret of a new target, which shows one time, went with the old one.
#[component]
pub fn DispatchPanel(
    board_id: String,
    column_names: Vec<String>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
) -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let board = StoredValue::new(board_id);
    let data = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        load(auth, board.get_value())
    });
    // KAIROS-T-0584: the log has its own pages. A change of the section
    // reads the current page again.
    let log_pages = RwSignal::new(LogPages::default());
    let log = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        let before = log_pages.with(|pages| pages.before().map(str::to_string));
        let board_id = board.get_value();
        async move { deliveries(auth, &board_id, before).await }
    });
    // The secret of the target made last. It shows once.
    let made: RwSignal<Option<(String, String)>> = RwSignal::new(None);

    let target_name = RwSignal::new(String::new());
    let target_url = RwSignal::new(String::new());
    let on_add_target = move |_| {
        if busy.get_untracked() {
            return;
        }
        let board_id = board.get_value();
        let body = json!({
            "name": target_name.get_untracked().trim(),
            "url": target_url.get_untracked().trim(),
        });
        busy.set(true);
        made.set(None);
        leptos::task::spawn_local(async move {
            let result: Result<Created, ApiError> = post_json(
                auth,
                &format!("/api/boards/{board_id}/webhook-targets"),
                &body,
            )
            .await;
            busy.set(false);
            match result {
                Ok(created) => {
                    made.set(Some((created.target.name, created.secret)));
                    outcome.set(None);
                    target_name.set(String::new());
                    target_url.set(String::new());
                    reload.update(|n| *n += 1);
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };

    let rule_trigger = RwSignal::new(TRIGGERS[0].to_string());
    let rule_column = RwSignal::new(column_names.first().cloned().unwrap_or_default());
    let mut column_names = column_names;
    column_names.push(ANY_COLUMN.to_string());
    let column_names = StoredValue::new(column_names);
    let rule_target = RwSignal::new(String::new());
    let rule_type = RwSignal::new(TASK_TYPES[0].to_string());
    let rule_metadata = RwSignal::new(String::new());
    let rule_repository = RwSignal::new(String::new());

    view! {
        <Panel title="Agent dispatch"
            caption="When a task enters the column of a rule (or its last open blocker \
                     closes, for a rule on unblocked) and matches its filter, Kairos \
                     sends one signed delivery to the target of the rule. The runner of the \
                     target starts the agent.">
            <div data-testid="dispatch-panel">
            <Stack gap="sm">
                {move || made.get().map(|(name, secret)| {
                    let copied = secret.clone();
                    view! {
                        <Alert title=format!("The secret of {name}") color=token::OK>
                            <Stack gap="xs">
                                <Text size="sm">
                                    "Copy the secret now. Kairos does not show it again. The \
                                     runner uses it to check X-Kairos-Signature."
                                </Text>
                                <Group gap="sm" wrap=true>
                                    <Code>{secret}</Code>
                                    <CopyButton value=copied label="Copy secret"/>
                                </Group>
                                <Group justify="end">
                                    <Button variant="subtle" size="xs"
                                        on_click=Callback::new(move |_| made.set(None))>
                                        "Hide the secret"
                                    </Button>
                                </Group>
                            </Stack>
                        </Alert>
                    }
                })}
                {move || match data.get() {
                    None => view! { <Loading label="Loading the dispatch…"/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(data)) => {
                        let target_names: Vec<String> =
                            data.targets.iter().map(|t| t.name.clone()).collect();
                        if rule_target.get_untracked().is_empty()
                            || !target_names.contains(&rule_target.get_untracked())
                        {
                            rule_target.set(target_names.first().cloned().unwrap_or_default());
                        }
                        let ids = StoredValue::new(
                            data.targets
                                .iter()
                                .map(|t| (t.name.clone(), t.id.clone()))
                                .collect::<Vec<_>>(),
                        );
                        let on_add_rule = move |_| {
                            let board_id = board.get_value();
                            let target_id = ids
                                .get_value()
                                .iter()
                                .find(|(name, _)| *name == rule_target.get_untracked())
                                .map(|(_, id)| id.clone())
                                .unwrap_or_default();
                            let body = match rule_body(
                                &rule_trigger.get_untracked(),
                                &rule_column.get_untracked(),
                                &target_id,
                                &rule_type.get_untracked(),
                                &rule_metadata.get_untracked(),
                                &rule_repository.get_untracked(),
                            ) {
                                Ok(body) => body,
                                Err(message) => {
                                    outcome.set(Some(Err(ApiError::Unknown(message))));
                                    return;
                                }
                            };
                            run_mutation(
                                busy,
                                outcome,
                                reload,
                                "Kairos made the dispatch rule.".to_string(),
                                async move {
                                    post_json::<_, Value>(
                                        auth,
                                        &format!("/api/boards/{board_id}/dispatch-rules"),
                                        &body,
                                    )
                                    .await
                                    .map(|_| ())
                                },
                            );
                        };
                        view! {
                            <TargetRows targets=data.targets.clone() board busy outcome reload/>
                            <Group align="end" gap="sm" wrap=true>
                                <TextInput label="Target name" value=target_name
                                    placeholder="e.g. Agent runner"/>
                                <TextInput label="URL" value=target_url
                                    placeholder="https://runner.example.com/kairos"/>
                                <Button on_click=Callback::new(on_add_target)>"Add target"</Button>
                            </Group>
                            <Divider/>
                            <TriageSetting agents_decide=data.agents_decide board busy outcome reload/>
                            <Divider/>
                            <RuleRows rules=data.rules.clone() board busy outcome reload/>
                            <Group align="end" gap="sm" wrap=true>
                                <Select label="A task that"
                                    options={TRIGGERS.iter().map(|t| t.to_string()).collect::<Vec<_>>()}
                                    value=rule_trigger/>
                                <Select label="Column" options=column_names.get_value() value=rule_column/>
                                <Select label="Target" options=target_names value=rule_target/>
                                <Select label="Task type"
                                    options={TASK_TYPES.iter().map(|t| t.to_string()).collect::<Vec<_>>()}
                                    value=rule_type/>
                                <TextInput label="Metadata (slug=value)" value=rule_metadata
                                    placeholder="priority=high"/>
                                <TextInput label="Repository" value=rule_repository
                                    placeholder="slug"/>
                                <Button on_click=Callback::new(on_add_rule)>"Add rule"</Button>
                            </Group>
                            <Divider/>
                            <DeliveryLog log pages=log_pages reload/>
                        }.into_any()
                    }
                }}
            </Stack>
            </div>
        </Panel>
    }
}

#[component]
fn TargetRows(
    targets: Vec<Target>,
    board: StoredValue<String>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    if targets.is_empty() {
        return view! {
            <Text size="sm" dimmed=true>"The board has no webhook target."</Text>
        }
        .into_any();
    }
    targets
        .into_iter()
        .map(|target| {
            let id = StoredValue::new(target.id.clone());
            let name = target.name.clone();
            let action = move |path: &'static str, done: String| {
                let board_id = board.get_value();
                let target_id = id.get_value();
                run_mutation(busy, outcome, reload, done, async move {
                    let url = format!("/api/boards/{board_id}/webhook-targets/{target_id}{path}");
                    if path.is_empty() {
                        delete_json::<Value>(auth, &url).await.map(|_| ())
                    } else {
                        post_empty::<Value>(auth, &url).await.map(|_| ())
                    }
                });
            };
            let (on_test, on_enable, on_redeliver, on_remove) = {
                let (a, b, c, d) = (name.clone(), name.clone(), name.clone(), name.clone());
                (
                    move |_| action("/test", format!("Kairos queued a test delivery to {a}.")),
                    // KAIROS-T-0577: turning on sends the skipped deliveries.
                    move |_| {
                        action(
                            "/enable",
                            format!(
                                "Kairos turned on {b}, and queued its skipped deliveries again."
                            ),
                        )
                    },
                    move |_| {
                        action(
                            "/redeliver",
                            format!("Kairos queued the skipped deliveries of {c} again."),
                        )
                    },
                    move |_| action("", format!("Kairos removed {d}, with its rules.")),
                )
            };
            let state = if target.enabled {
                view! { <Pill color=token::OK>"on"</Pill> }.into_any()
            } else {
                view! { <Pill color=token::BAD>"off"</Pill> }.into_any()
            };
            view! {
                <Group justify="between" wrap=true>
                    <Group gap="sm" wrap=true>
                        <Text bright=true size="sm">{target.name.clone()}</Text>
                        {state}
                        <Text mono=true size="xs" dimmed=true>{target.url.clone()}</Text>
                        <Text size="xs" dimmed=true>
                            {format!("{} failed in a row", target.consecutive_failures)}
                        </Text>
                        {target.disabled_reason.clone().map(|reason| view! {
                            <Text size="xs">{reason}</Text>
                        })}
                    </Group>
                    <Group gap="xs">
                        <Button variant="default" size="xs" on_click=Callback::new(on_test)>
                            "Send test"
                        </Button>
                        {(!target.enabled).then(|| view! {
                            <Button variant="default" size="xs" on_click=Callback::new(on_enable)>
                                "Turn on"
                            </Button>
                        })}
                        {target.enabled.then(|| view! {
                            <Button variant="default" size="xs"
                                on_click=Callback::new(on_redeliver)>
                                "Send skipped again"
                            </Button>
                        })}
                        <Button variant="default" size="xs" bad=true
                            on_click=Callback::new(on_remove)>
                            "Remove"
                        </Button>
                    </Group>
                </Group>
            }
            .into_any()
        })
        .collect_view()
        .into_any()
}

#[component]
fn RuleRows(
    rules: Vec<Rule>,
    board: StoredValue<String>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    if rules.is_empty() {
        return view! {
            <Text size="sm" dimmed=true>"The board has no dispatch rule."</Text>
        }
        .into_any();
    }
    rules
        .into_iter()
        .map(|rule| {
            let id = StoredValue::new(rule.id.clone());
            let on_remove = move |_| {
                let board_id = board.get_value();
                let rule_id = id.get_value();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    "Kairos removed the dispatch rule.".to_string(),
                    async move {
                        delete_json::<Value>(
                            auth,
                            &format!("/api/boards/{board_id}/dispatch-rules/{rule_id}"),
                        )
                        .await
                        .map(|_| ())
                    },
                );
            };
            view! {
                <Group justify="between" wrap=true>
                    <Text size="sm" attr:data-testid="dispatch-rule">
                        {format!(
                            "{} {} goes to {}",
                            filter_words(&rule),
                            when_words(&rule),
                            rule.target
                        )}
                    </Text>
                    <Button variant="default" size="xs" bad=true
                        on_click=Callback::new(on_remove)>
                        "Remove"
                    </Button>
                </Group>
            }
        })
        .collect_view()
        .into_any()
}

/// Who decides the triage of intake (KAIROS-T-0428): off, an agent
/// proposes with a note and a person decides; on, an agent decides.
#[component]
fn TriageSetting(
    agents_decide: bool,
    board: StoredValue<String>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let checked = RwSignal::new(agents_decide);
    let on_save = move |_| {
        let board_id = board.get_value();
        let decide = checked.get_untracked();
        run_mutation(
            busy,
            outcome,
            reload,
            "Kairos saved the triage setting.".to_string(),
            async move {
                put_json::<_, Value>(
                    auth,
                    &format!("/api/boards/{board_id}/agent-triage"),
                    &json!({ "agents_decide": decide }),
                )
                .await
                .map(|_| ())
            },
        );
    };
    view! {
        <Group align="end" gap="sm" wrap=true attr:data-testid="agent-triage">
            <Switch checked label="An agent decides the triage of intake (support lane)"/>
            <Button variant="default" size="xs" on_click=Callback::new(on_save)>"Save"</Button>
        </Group>
        <Text size="xs" dimmed=true>
            "Off: an agent writes its proposal as a note, and a person moves the task."
        </Text>
    }
}

/// The delivery log: a page, and the buttons Newer and Older.
#[component]
fn DeliveryLog(
    log: LocalResource<Result<LogPage, ApiError>>,
    pages: RwSignal<LogPages>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let more = move || matches!(log.get(), Some(Ok(page)) if page.more);
    let on_older = move |_| {
        let Some(Ok(page)) = log.get_untracked() else {
            return;
        };
        if let Some(last) = page.deliveries.last() {
            pages.update(|pages| pages.older(last.id.clone()));
        }
    };
    view! {
        <div data-testid="delivery-log">
            <Group justify="between">
                <Text bright=true bold=true size="sm">"Delivery log, newest first"</Text>
                <Button variant="subtle" size="xs"
                    on_click=Callback::new(move |_| reload.update(|n| *n += 1))>
                    "Refresh"
                </Button>
            </Group>
            {move || match log.get() {
                None => view! { <Loading label="Loading the log…"/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(page)) => view! { <DeliveryRows log=page.deliveries/> }.into_any(),
            }}
            <div data-testid="delivery-log-pager">
                <Group justify="end" gap="sm">
                    <Button variant="default" size="xs"
                        disabled=Signal::derive(move || pages.with(|p| p.number() == 1))
                        on_click=Callback::new(move |_| pages.update(LogPages::newer))>
                        "Newer"
                    </Button>
                    <Text size="sm" dimmed=true>
                        {move || format!("Page {}", pages.with(LogPages::number))}
                    </Text>
                    <Button variant="default" size="xs"
                        disabled=Signal::derive(move || !more())
                        on_click=Callback::new(on_older)>
                        "Older"
                    </Button>
                </Group>
            </div>
        </div>
    }
}

/// The rows of a page of the log.
#[component]
fn DeliveryRows(log: Vec<Delivery>) -> impl IntoView {
    let empty = log.is_empty();
    let rows = log
        .into_iter()
        .map(|d| {
            let tries = d
                .log
                .iter()
                .map(|t| {
                    format!(
                        "#{} {} {}{}",
                        t.try_number,
                        t.at,
                        if t.ok { "ok" } else { "failed" },
                        t.response_code
                            .map(|c| format!(" ({c})"))
                            .or_else(|| t.error.clone().map(|e| format!(" ({e})")))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            view! {
                <tr>
                    <td>{d.created_at.clone()}</td>
                    <td>{d.event.clone()}</td>
                    <td>{d.short_code.clone().unwrap_or_else(|| "-".into())}</td>
                    <td><Pill color=status_color(&d.status)>{d.status.clone()}</Pill></td>
                    <td class="cl-num">{d.tries}</td>
                    <td class="cl-num">{d.response_code.map(|c| c.to_string()).unwrap_or_default()}</td>
                    <td>{d.last_error.clone().unwrap_or_default()}</td>
                    <td title=tries.clone()>{tries.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    view! {
        {empty.then(|| view! { <Text size="sm" dimmed=true>"No delivery yet."</Text> })}
        <Table label="Delivery log">
            <thead><tr><th>"Time"</th><th>"Event"</th><th>"Task"</th><th>"Status"</th>
                <th class="cl-num">"Tries"</th><th class="cl-num">"Code"</th>
                <th>"Error"</th><th>"Each try"</th></tr></thead>
            <tbody>{rows}</tbody>
        </Table>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        Rule {
            id: "r".into(),
            trigger: "entered_column".into(),
            column: Some("Todo".into()),
            target: "Runner".into(),
            task_type: None,
            metadata_slug: None,
            metadata_value: None,
            repository: None,
        }
    }

    #[test]
    fn the_filter_reads_in_words() {
        assert_eq!(filter_words(&rule()), "each task");
        let r = Rule {
            task_type: Some("bug".into()),
            metadata_slug: Some("priority".into()),
            metadata_value: Some("high".into()),
            repository: Some("site".into()),
            ..rule()
        };
        assert_eq!(
            filter_words(&r),
            "type bug, priority = high, repository site"
        );
    }

    #[test]
    fn when_a_rule_fires_reads_in_words() {
        assert_eq!(when_words(&rule()), "that enters Todo");
        let unblocked = Rule {
            trigger: "unblocked".into(),
            ..rule()
        };
        assert_eq!(when_words(&unblocked), "in Todo that is unblocked");
        let anywhere = Rule {
            column: None,
            ..unblocked
        };
        assert_eq!(when_words(&anywhere), "that is unblocked");
    }

    #[test]
    fn the_body_of_an_unblocked_rule_names_its_trigger() {
        assert_eq!(
            rule_body("is unblocked", "any column", "t1", "any type", "", "").unwrap(),
            json!({ "trigger": "unblocked", "target_id": "t1" })
        );
        assert_eq!(
            rule_body("is unblocked", "Todo", "t1", "any type", "", "").unwrap(),
            json!({ "trigger": "unblocked", "column": "Todo", "target_id": "t1" })
        );
        assert!(rule_body("enters the column", "any column", "t1", "any type", "", "").is_err());
    }

    #[test]
    fn the_body_of_a_rule_has_only_the_parts_that_are_set() {
        assert_eq!(
            rule_body("enters the column", "Todo", "t1", "any type", "", "").unwrap(),
            json!({ "column": "Todo", "target_id": "t1" })
        );
        assert_eq!(
            rule_body(
                "enters the column",
                "Todo",
                "t1",
                "bug",
                " priority = high ",
                "site"
            )
            .unwrap(),
            json!({
                "column": "Todo", "target_id": "t1", "task_type": "bug",
                "metadata_slug": "priority", "metadata_value": "high", "repository": "site",
            })
        );
        assert!(rule_body("enters the column", "Todo", "t1", "bug", "priority", "").is_err());
    }

    fn delivery(id: &str) -> Delivery {
        Delivery {
            id: id.into(),
            event: "ping".into(),
            short_code: None,
            status: "delivered".into(),
            tries: 1,
            response_code: Some(200),
            last_error: None,
            created_at: "2026-10-17T09:00:00Z".into(),
            log: Vec::new(),
        }
    }

    /// KAIROS-T-0584: the page asks for one delivery more than it shows.
    #[test]
    fn a_page_of_the_log_knows_the_last_page() {
        assert_eq!(
            log_url("b1", None),
            "/api/boards/b1/webhook-deliveries?limit=51"
        );
        assert_eq!(
            log_url("b1", Some("d 9")),
            "/api/boards/b1/webhook-deliveries?limit=51&before=d%209"
        );
        let full: Vec<Delivery> = (0..=LOG_PAGE).map(|n| delivery(&n.to_string())).collect();
        let page = log_page(full);
        assert!(page.more, "one more than the page: an older page is there");
        assert_eq!(page.deliveries.len(), LOG_PAGE);
        assert_eq!(page.deliveries.last().map(|d| d.id.as_str()), Some("49"));
        let last = log_page(vec![delivery("a"), delivery("b")]);
        assert!(!last.more, "the last page");
        assert_eq!(last.deliveries.len(), 2);
    }

    /// KAIROS-T-0584: Older keeps the cursor of each page; Newer goes back.
    #[test]
    fn the_pager_of_the_log_goes_older_and_back() {
        let mut pages = LogPages::default();
        assert_eq!((pages.number(), pages.before()), (1, None));
        pages.older("d50".into());
        pages.older("d100".into());
        assert_eq!((pages.number(), pages.before()), (3, Some("d100")));
        pages.newer();
        assert_eq!((pages.number(), pages.before()), (2, Some("d50")));
        pages.newer();
        pages.newer();
        assert_eq!((pages.number(), pages.before()), (1, None));
    }

    #[test]
    fn each_status_has_a_color() {
        assert_eq!(status_color("delivered"), token::OK);
        assert_eq!(status_color("failed"), token::BAD);
        assert_eq!(status_color("skipped"), token::MUTED);
    }
}
