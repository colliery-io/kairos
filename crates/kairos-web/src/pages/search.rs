//! Unified search (KAIROS-T-0042, contract per KAIROS-A-0007 / S-0005):
//! one text query + a structured filter builder + an optional graph
//! traversal, composed into a single `POST /api/search`, with results
//! grouped by entity type and paginated over the combined set.
//!
//! KAIROS-T-0583: the form offers each filter of the API, and the sort.
//! The URL query is the source of truth ([`url`]): the Search button and
//! the pager write the URL, and the page reads the URL into the form and
//! the request. A reload, Back and a shared link give the same search.
//! The top bar has a search box ([`search_box`]) that opens this page.
//!
//! The relationships explorer lives in [`relationships`] and mounts at
//! `/search/relationships/:code` — under this task's path segment per
//! docs/gui-conventions.md §3 ("sub-routes under your own path segment
//! only"; `/items/:code` belongs to T-0041). Its `RelationshipsView`
//! component is standalone so the item-detail task can also embed it.

pub mod data;
pub mod graph;
pub mod graph_model;
pub mod relationships;
pub mod search_box;
pub mod url;

use std::collections::HashMap;

use aurora_dark::components::{
    ActionIcon, Alert, Anchor, Button, Chip, Divider, Empty, ErrorState, Group, Loading,
    NumberInput, PageHeader, Panel, Pill, SegmentedControl, Select, Stack, Switch, Table, Text,
    TextInput,
};
use aurora_dark::data::{Pagination, RelativeTime};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::auth::use_auth;

pub use relationships::RelationshipsPage;
pub use search_box::ShellSearchBox;

/// The entity-type vocabulary (S-0005 `filter.entity_type`).
const ENTITY_TYPES: [&str; 5] = ["strategy", "initiative", "task", "document", "adr"];

/// The task-type vocabulary (S-0005 `filter.task_type`).
const TASK_TYPES: [&str; 4] = ["task", "bug", "tech_debt", "support"];

/// The lane vocabulary (`filter.work_class`, KAIROS-T-0077).
const WORK_CLASSES: [&str; 2] = ["planned", "support"];

/// The relationship vocabulary (S-0005 `traverse.relationships`).
const RELATIONSHIPS: [&str; 6] = [
    "parent",
    "supports",
    "informs",
    "supersedes",
    "amends",
    "blocks",
];

/// The sort choices: `field:order`, and the label. The empty value is the
/// default of the server.
const SORTS: [(&str, &str); 8] = [
    ("", "Best match with a text query, else newest first"),
    ("relevance:desc", "Best match"),
    ("created_at:desc", "Newest first"),
    ("created_at:asc", "Oldest first"),
    ("updated_at:desc", "Changed most recently"),
    ("updated_at:asc", "Changed least recently"),
    ("title:asc", "Title, A to Z"),
    ("title:desc", "Title, Z to A"),
];

/// The page sizes.
const PAGE_SIZES: [&str; 5] = ["5", "10", "25", "50", "100"];

/// The page size of the server when the request has no `limit`. The URL
/// has no `limit` for it.
const DEFAULT_PAGE_SIZE: i64 = 25;

/// A data-driven accent per entity type (token constants only).
pub(crate) fn entity_color(entity_type: &str) -> &'static str {
    match entity_type {
        "strategy" => token::VIOLET,
        "initiative" => token::ICE,
        "task" => token::TEAL,
        "document" => token::GOLD,
        "adr" => token::SKIP,
        _ => token::MUTED,
    }
}

/// One dynamic metadata `key = value` filter row.
#[derive(Clone, Copy, PartialEq)]
struct MetaRow {
    id: usize,
    key: RwSignal<String>,
    value: RwSignal<String>,
}

/// The choices of a select: `("", any)` first, then `rest`, then the
/// current value when the list does not have it (a link can name a board
/// that the person cannot see, or a person who left).
fn choices(any: &str, rest: Vec<(String, String)>, current: &str) -> Vec<(String, String)> {
    let mut all = vec![(String::new(), any.to_string())];
    all.extend(rest);
    if !current.is_empty() && !all.iter().any(|(value, _)| value == current) {
        all.push((current.to_string(), current.to_string()));
    }
    all
}

/// "board · column" for a hit, from the names that the page has. Pure.
fn place_label(board: Option<&str>, column: Option<&str>) -> String {
    match (board, column) {
        (Some(board), Some(column)) => format!("{board} · {column}"),
        (Some(board), None) => board.to_string(),
        (None, Some(column)) => column.to_string(),
        (None, None) => String::new(),
    }
}

/// The person of a hit: the claim of a task, or the owner of a strategy or
/// an initiative. Pure.
fn person_label(hit: &data::Hit) -> String {
    if let Some(claim) = &hit.claim {
        if claim.agent {
            return format!("{} (agent)", claim.display_name);
        }
        return claim.display_name.clone();
    }
    hit.owner
        .as_ref()
        .map(|owner| format!("{} (owner)", owner.display_name))
        .unwrap_or_default()
}

/// The vocabulary words whose chip is on.
fn chosen(labels: &[&str], flags: &[RwSignal<bool>]) -> Vec<String> {
    labels
        .iter()
        .zip(flags)
        .filter(|(_, flag)| flag.get_untracked())
        .map(|(name, _)| (*name).to_string())
        .collect()
}

/// Sets each chip from a list of words.
fn set_chips(labels: &[&str], flags: &[RwSignal<bool>], words: &[String]) {
    for (label, flag) in labels.iter().zip(flags) {
        flag.set(words.iter().any(|word| word == label));
    }
}

/// `/search` — the unified search page.
#[component]
pub fn SearchPage() -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();

    // ---- the URL is the state --------------------------------------------
    let query = use_query_map();
    let parsed = Memo::new(move |_| query.with(|map| url::SearchState::from_pairs(map)));

    // ---- form state (a copy of the URL until Search) ----------------------
    let q = RwSignal::new(String::new());
    let entity_flags: [RwSignal<bool>; 5] = std::array::from_fn(|_| RwSignal::new(false));
    let task_flags: [RwSignal<bool>; TASK_TYPES.len()] =
        std::array::from_fn(|_| RwSignal::new(false));
    let work_flags: [RwSignal<bool>; WORK_CLASSES.len()] =
        std::array::from_fn(|_| RwSignal::new(false));
    let board_sel = RwSignal::new(String::new());
    let column_sel = RwSignal::new(String::new());
    let team_sel = RwSignal::new(String::new());
    let repo_sel = RwSignal::new(String::new());
    let claimed_sel = RwSignal::new(String::new());
    let owned_sel = RwSignal::new(String::new());
    let creator_sel = RwSignal::new(String::new());
    let bucket_sel = RwSignal::new(String::new());
    let created_after = RwSignal::new(String::new());
    let created_before = RwSignal::new(String::new());
    let updated_after = RwSignal::new(String::new());
    let updated_before = RwSignal::new(String::new());
    let meta_rows: RwSignal<Vec<MetaRow>> = RwSignal::new(Vec::new());
    let next_meta_id = StoredValue::new(0usize);
    let missing_input = RwSignal::new(String::new());
    // ADR-20 rule 3: default listings hide archived work, so this starts
    // OFF and the page sends nothing about it until someone asks.
    let include_put_away = RwSignal::new(false);
    let traverse_on = RwSignal::new(false);
    let t_from = RwSignal::new(String::new());
    let t_rel_flags: [RwSignal<bool>; 5] = std::array::from_fn(|i| RwSignal::new(i == 0));
    let t_direction = RwSignal::new("outbound".to_string());
    let t_depth = RwSignal::new(3.0_f64);
    let sort_sel = RwSignal::new(String::new());
    let page_size = RwSignal::new(DEFAULT_PAGE_SIZE.to_string());
    let form_error = RwSignal::new(None::<String>);

    let new_meta_row = move |key: String, value: String| {
        let id = next_meta_id.get_value();
        next_meta_id.set_value(id + 1);
        MetaRow {
            id,
            key: RwSignal::new(key),
            value: RwSignal::new(value),
        }
    };

    // URL → form: on the first load, on Back, and after a Search.
    Effect::new(move |_| {
        let state = parsed.get().state;
        q.set(state.q.clone());
        set_chips(&ENTITY_TYPES, &entity_flags, &state.entity_type);
        set_chips(&TASK_TYPES, &task_flags, &state.task_type);
        set_chips(&WORK_CLASSES, &work_flags, &state.work_class);
        board_sel.set(state.board.clone());
        column_sel.set(state.column.clone());
        team_sel.set(state.team.clone());
        repo_sel.set(state.repository.clone());
        claimed_sel.set(state.claimed_by.clone());
        owned_sel.set(state.owned_by.clone());
        creator_sel.set(state.created_by.clone());
        bucket_sel.set(match state.is_bucket {
            Some(true) => "true".to_string(),
            Some(false) => "false".to_string(),
            None => String::new(),
        });
        created_after.set(state.created_after.clone());
        created_before.set(state.created_before.clone());
        updated_after.set(state.updated_after.clone());
        updated_before.set(state.updated_before.clone());
        meta_rows.set(
            state
                .metadata
                .iter()
                .map(|(key, value)| new_meta_row(key.clone(), value.clone()))
                .collect(),
        );
        missing_input.set(state.metadata_missing.join(", "));
        include_put_away.set(state.include_deleted);
        let traverse = state.traverse.clone();
        traverse_on.set(traverse.is_some());
        let traverse = traverse.unwrap_or_default();
        t_from.set(traverse.from.clone());
        set_chips(&RELATIONSHIPS, &t_rel_flags, &traverse.relationships);
        t_direction.set(traverse.direction.clone());
        t_depth.set(f64::from(traverse.depth));
        sort_sel.set(
            state
                .sort
                .as_ref()
                .map(|(field, order)| format!("{field}:{order}"))
                .unwrap_or_default(),
        );
        page_size.set(state.limit.unwrap_or(DEFAULT_PAGE_SIZE).to_string());
        form_error.set(None);
    });

    // Form → state, for the Search button.
    let read_form = move || -> url::SearchState {
        let traverse = traverse_on.get_untracked().then(|| url::TraverseState {
            from: t_from.get_untracked().trim().to_string(),
            relationships: chosen(&RELATIONSHIPS, &t_rel_flags),
            direction: t_direction.get_untracked(),
            depth: (t_depth.get_untracked() as u32).clamp(1, 10),
        });
        url::SearchState {
            q: q.get_untracked().trim().to_string(),
            entity_type: chosen(&ENTITY_TYPES, &entity_flags),
            task_type: chosen(&TASK_TYPES, &task_flags),
            work_class: chosen(&WORK_CLASSES, &work_flags),
            board: board_sel.get_untracked(),
            column: column_sel.get_untracked(),
            team: team_sel.get_untracked(),
            repository: repo_sel.get_untracked(),
            claimed_by: claimed_sel.get_untracked(),
            owned_by: owned_sel.get_untracked(),
            created_by: creator_sel.get_untracked(),
            is_bucket: match bucket_sel.get_untracked().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            metadata: meta_rows
                .get_untracked()
                .iter()
                .map(|row| {
                    (
                        row.key.get_untracked().trim().to_string(),
                        row.value.get_untracked().trim().to_string(),
                    )
                })
                .filter(|(key, value)| !key.is_empty() && !value.is_empty())
                .collect(),
            metadata_missing: missing_input
                .get_untracked()
                .split(',')
                .map(str::trim)
                .filter(|slug| !slug.is_empty())
                .map(str::to_string)
                .collect(),
            created_after: created_after.get_untracked().trim().to_string(),
            created_before: created_before.get_untracked().trim().to_string(),
            updated_after: updated_after.get_untracked().trim().to_string(),
            updated_before: updated_before.get_untracked().trim().to_string(),
            include_deleted: include_put_away.get_untracked(),
            traverse,
            sort: sort_sel
                .get_untracked()
                .split_once(':')
                .map(|(field, order)| (field.to_string(), order.to_string())),
            limit: page_size
                .get_untracked()
                .parse::<i64>()
                .ok()
                .filter(|size| *size != DEFAULT_PAGE_SIZE),
            offset: 0,
        }
    };

    let go_to_state = {
        let navigate = navigate.clone();
        move |state: &url::SearchState| {
            navigate(&format!("/search{}", state.to_query()), Default::default());
        }
    };

    // ---- the request of the URL --------------------------------------------
    let request = Memo::new(move |_| {
        let state = parsed.get().state;
        (!state.asks_nothing()).then(|| state.to_request())
    });

    // ---- option sources ---------------------------------------------------
    let boards = LocalResource::new(move || {
        auth.track_session();
        data::boards(auth)
    });
    let columns = LocalResource::new(move || {
        auth.track_session();
        let slug = board_sel.get();
        let boards_now = boards.get();
        async move {
            let Some(Ok(list)) = boards_now else {
                return Ok(Vec::new());
            };
            match list.iter().find(|b| b.slug == slug) {
                Some(board) => data::board_columns(auth, &board.id).await,
                None => Ok(Vec::new()),
            }
        }
    });
    let teams = LocalResource::new(move || {
        auth.track_session();
        data::teams(auth)
    });
    let repositories = LocalResource::new(move || {
        auth.track_session();
        data::repositories(auth)
    });
    let members = LocalResource::new(move || {
        auth.track_session();
        data::members(auth)
    });

    // ---- results ----------------------------------------------------------
    let results = LocalResource::new(move || {
        auth.track_session();
        let request = request.get();
        async move {
            match request {
                Some(Ok(request)) => data::search(auth, &request).await.map(Some),
                _ => Ok(None),
            }
        }
    });
    // The names of the boards and the columns of the hits.
    let board_names = Memo::new(move |_| match boards.get() {
        Some(Ok(list)) => list
            .into_iter()
            .map(|board| (board.id, board.slug))
            .collect::<HashMap<_, _>>(),
        _ => HashMap::new(),
    });
    let column_names = LocalResource::new(move || {
        let mut board_ids: Vec<String> = match results.get() {
            Some(Ok(Some(response))) => {
                let groups = response.results;
                groups
                    .strategies
                    .iter()
                    .chain(&groups.initiatives)
                    .chain(&groups.tasks)
                    .chain(&groups.adrs)
                    .filter(|hit| hit.column_id.is_some())
                    .filter_map(|hit| hit.board_id.clone())
                    .collect()
            }
            _ => Vec::new(),
        };
        board_ids.sort();
        board_ids.dedup();
        async move {
            let mut names = HashMap::new();
            for id in board_ids {
                if let Ok(columns) = data::board_columns(auth, &id).await {
                    names.extend(columns.into_iter().map(|column| (column.id, column.name)));
                }
            }
            names
        }
    });
    let column_names = Signal::derive(move || column_names.get().unwrap_or_default());
    let board_names = Signal::from(board_names);

    // ---- submit ------------------------------------------------------------
    let submit = {
        let go_to_state = go_to_state.clone();
        move || {
            let state = read_form();
            match state.to_request() {
                Err(message) => form_error.set(Some(message)),
                // The same URL does not change the query, so Search runs
                // the search again itself.
                Ok(_) if state == parsed.get_untracked().state => {
                    form_error.set(None);
                    results.refetch();
                }
                Ok(_) => {
                    form_error.set(None);
                    go_to_state(&state);
                }
            }
        }
    };

    // ---- chips -------------------------------------------------------------
    let chip_row = |labels: &'static [&'static str], flags: &[RwSignal<bool>]| {
        labels
            .iter()
            .zip(flags.iter().copied())
            .map(|(label, flag)| {
                view! {
                    <Chip
                        label=*label
                        active=flag
                        on_click=Callback::new(move |_| flag.update(|v| *v = !*v))
                    />
                }
            })
            .collect_view()
    };
    let entity_chips = chip_row(&ENTITY_TYPES, &entity_flags);
    let task_chips = chip_row(&TASK_TYPES, &task_flags);
    let work_chips = chip_row(&WORK_CLASSES, &work_flags);
    // Built lazily — the traverse block re-renders on every toggle.
    let rel_chips = move || chip_row(&RELATIONSHIPS, &t_rel_flags);

    // A person select: anyone, me, then each member by email.
    let person_select = move |label: &'static str, value: RwSignal<String>| {
        move || {
            let mut people = vec![("me".to_string(), "Me".to_string())];
            if let Some(Ok(list)) = members.get() {
                people.extend(list.into_iter().map(|member| {
                    let text = format!("{} ({})", member.display_name, member.email);
                    (member.email, text)
                }));
            }
            let options = choices("Anyone", people, &value.get());
            view! { <Select label=label option_pairs=options value=value/> }
        }
    };

    let page_go = Callback::new(move |(to, _): (usize, usize)| {
        let mut state = parsed.get_untracked().state;
        state.offset = i64::try_from(to).unwrap_or(0);
        go_to_state(&state);
    });

    let sort_options: Vec<(String, String)> = SORTS
        .iter()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect();
    let page_sizes: Vec<String> = PAGE_SIZES.iter().map(|size| size.to_string()).collect();

    view! {
        <PageHeader title="Search" sub="Find work by text, by filters, and by the relationships of an item."/>
        <Stack gap="md">
            <Panel title="Query" caption="An item must match each part of the search.">
                <Stack gap="sm">
                    <Group align="end" gap="sm" wrap=true>
                        <TextInput
                            label="Text query"
                            placeholder="e.g. sign-up, \"magic link\", auth OR billing"
                            value=q
                        />
                        <Select
                            label="Sort"
                            option_pairs=sort_options
                            value=sort_sel
                        />
                        <Select
                            label="Page size"
                            options=page_sizes
                            value=page_size
                        />
                        <Button on_click=Callback::new(move |_| submit())>
                            "Search"
                        </Button>
                    </Group>
                    <Divider/>
                    // KAIROS-T-0163 / ADR-20: the audit switch. It sits
                    // above the filters, labelled, because "visible for
                    // audit" means discoverable by someone who never read
                    // the docs — a URL-only parameter would not be.
                    //
                    // The copy says "put away", never a bare "archived":
                    // a document's editorial `lifecycle: archived`
                    // (KAIROS-T-0078) is an unrelated state that can land
                    // in these same results while the document is
                    // perfectly live. Nothing is renamed here — this
                    // comment is the record of the collision for whoever
                    // eventually does (KAIROS-A-0020, Neutral).
                    <div data-testid="include-put-away">
                        <Stack gap="xs">
                            <Switch
                                checked=include_put_away
                                label="Include work that has been put away"
                            />
                            <Text dimmed=true size="xs">
                                "This is off by default. Then boards, queues and search do \
                                 not show put-away (archived) work. You can still open it. \
                                 The results mark it."
                            </Text>
                        </Stack>
                    </div>
                    <Stack gap="xs">
                        <Text dimmed=true size="xs">"Entity types"</Text>
                        <Group gap="xs" wrap=true>{entity_chips}</Group>
                    </Stack>
                    <Group gap="sm" wrap=true top=true>
                        {move || {
                            let rest = match boards.get() {
                                Some(Ok(list)) => list
                                    .iter()
                                    .map(|b| (b.slug.clone(), b.slug.clone()))
                                    .collect(),
                                _ => Vec::new(),
                            };
                            let options = choices("Any", rest, &board_sel.get());
                            // A new board makes the column choice stale.
                            let reset_column = Callback::new(move |_| column_sel.set(String::new()));
                            view! {
                                <Select
                                    label="Board"
                                    option_pairs=options
                                    value=board_sel
                                    on_change=reset_column
                                />
                            }
                        }}
                        {move || {
                            let rest = match columns.get() {
                                Some(Ok(cols)) => cols
                                    .iter()
                                    .map(|c| (c.name.clone(), c.name.clone()))
                                    .collect(),
                                _ => Vec::new(),
                            };
                            let options = choices("Any", rest, &column_sel.get());
                            view! { <Select label="Column" option_pairs=options value=column_sel/> }
                        }}
                        {move || {
                            let rest = match teams.get() {
                                Some(Ok(list)) => list
                                    .into_iter()
                                    .map(|team| (team.slug, team.name))
                                    .collect(),
                                _ => Vec::new(),
                            };
                            let options = choices("Any", rest, &team_sel.get());
                            view! { <Select label="Team" option_pairs=options value=team_sel/> }
                        }}
                        {move || {
                            let rest = match repositories.get() {
                                Some(Ok(list)) => list
                                    .into_iter()
                                    .map(|repo| (repo.slug.clone(), repo.slug))
                                    .collect(),
                                _ => Vec::new(),
                            };
                            let options = choices("Any", rest, &repo_sel.get());
                            view! { <Select label="Repository" option_pairs=options value=repo_sel/> }
                        }}
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        {person_select("Claimed by", claimed_sel)}
                        {person_select("Owned by", owned_sel)}
                        {person_select("Created by", creator_sel)}
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Created after" input_type="date" value=created_after/>
                        <TextInput label="Created before" input_type="date" value=created_before/>
                        <TextInput label="Changed after" input_type="date" value=updated_after/>
                        <TextInput label="Changed before" input_type="date" value=updated_before/>
                    </Group>
                    <Group gap="md" wrap=true top=true>
                        <Stack gap="xs">
                            <Text dimmed=true size="xs">"Task types"</Text>
                            <Group gap="xs" wrap=true>{task_chips}</Group>
                        </Stack>
                        <Stack gap="xs">
                            <Text dimmed=true size="xs">"Lanes"</Text>
                            <Group gap="xs" wrap=true>{work_chips}</Group>
                        </Stack>
                        <Select
                            label="Initiative buckets"
                            option_pairs=vec![
                                (String::new(), "Any".to_string()),
                                ("true".to_string(), "Only buckets".to_string()),
                                ("false".to_string(), "No buckets".to_string()),
                            ]
                            value=bucket_sel
                        />
                    </Group>
                    <Stack gap="xs">
                        <Group gap="sm">
                            <Text dimmed=true size="xs">"Metadata values (an item must match each row)"</Text>
                            <Button
                                variant="default"
                                size="xs"
                                on_click=Callback::new(move |_| {
                                    let row = new_meta_row(String::new(), String::new());
                                    meta_rows.update(|rows| rows.push(row));
                                })
                            >
                                "+ add"
                            </Button>
                        </Group>
                        <For
                            each=move || meta_rows.get()
                            key=|row| row.id
                            children=move |row| view! {
                                <Group gap="sm">
                                    <TextInput placeholder="key (definition slug)" value=row.key/>
                                    <TextInput placeholder="value (trailing * glob)" value=row.value/>
                                    <ActionIcon
                                        title="Remove this metadata filter"
                                        on_click=Callback::new(move |_| {
                                            meta_rows.update(|rows| rows.retain(|r| r.id != row.id));
                                        })
                                    >
                                        "×"
                                    </ActionIcon>
                                </Group>
                            }
                        />
                        <TextInput
                            label="Metadata with no value"
                            placeholder="e.g. priority, estimate"
                            value=missing_input
                        />
                    </Stack>
                    <Divider/>
                    // KAIROS-T-0090: this is result SCOPING, not the graph
                    // story — the graph view lives on each item.
                    <Switch checked=traverse_on label="Limit results to items reachable from…"/>
                    {move || traverse_on.get().then(|| view! {
                        <Group gap="sm" wrap=true top=true>
                            <TextInput
                                label="From (short code)"
                                placeholder="DEMO-S-0001"
                                value=t_from
                            />
                            <Stack gap="xs">
                                <Text dimmed=true size="xs">"Relationships"</Text>
                                <Group gap="xs" wrap=true>{rel_chips()}</Group>
                            </Stack>
                            <Stack gap="xs">
                                <Text dimmed=true size="xs">"Direction"</Text>
                                <SegmentedControl
                                    options=vec![
                                        "outbound".to_string(),
                                        "inbound".to_string(),
                                        "both".to_string(),
                                    ]
                                    value=t_direction
                                />
                            </Stack>
                            <NumberInput label="Depth (1–10)" value=t_depth/>
                        </Group>
                    })}
                    {move || {
                        let problems = parsed.get().problems;
                        (!problems.is_empty()).then(|| view! {
                            <Alert title="The page cannot use a part of the link" color=token::GOLD>
                                <Stack gap="xs">
                                    {problems
                                        .into_iter()
                                        .map(|problem| view! { <Text size="sm">{problem}</Text> })
                                        .collect_view()}
                                </Stack>
                            </Alert>
                        })
                    }}
                    {move || {
                        let message = form_error.get().or_else(|| match request.get() {
                            Some(Err(message)) => Some(message),
                            _ => None,
                        });
                        message.map(|message| view! {
                            <Alert title="Nothing to search" color=token::GOLD>
                                <Text size="sm">{message}</Text>
                            </Alert>
                        })
                    }}
                </Stack>
            </Panel>

            {move || match results.get() {
                None => view! { <Loading label="Searching…"/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState
                        error=error
                        on_retry=Callback::new(move |_| results.refetch())
                    />
                }.into_any(),
                Some(Ok(None)) => view! {
                    <Empty message="Search the strategies, initiatives, tasks, documents and ADRs. Type a text query, or add a filter or a traversal. Then click Search."/>
                }.into_any(),
                Some(Ok(Some(response))) if response.total == 0 => view! {
                    <Empty message="No matches. Remove a filter or change the query."/>
                }.into_any(),
                Some(Ok(Some(response))) => {
                    let (total, limit, offset) =
                        (response.total, response.limit.max(1), response.offset);
                    // COLLIERY-T-0258: the results have no bound, so the
                    // page shows a part of them and says which part.
                    let groups = response.results;
                    // COLLIERY-T-1836: the Aurora `Pagination`. A change of
                    // page writes the new offset into the URL.
                    let page_offset = RwSignal::new(usize::try_from(offset).unwrap_or(0));
                    let page_limit = RwSignal::new(usize::try_from(limit).unwrap_or(1));
                    let page_total = usize::try_from(total).unwrap_or(0);
                    view! {
                        <Stack gap="xs">
                            <Text dimmed=true size="sm">
                                "The groups are the types of the items."
                            </Text>
                            // KAIROS-T-0326: the pager says the range.
                            <Pagination
                                offset=page_offset
                                limit=page_limit
                                total=page_total
                                on_change=page_go
                                range_label=Callback::new(|range| {
                                    crate::api::page_range_text(&range)
                                })
                            />
                        </Stack>
                        <ResultGroup entity="strategy" title="Strategies" hits=groups.strategies board_names column_names/>
                        <ResultGroup entity="initiative" title="Initiatives" hits=groups.initiatives board_names column_names/>
                        <ResultGroup entity="task" title="Tasks" hits=groups.tasks board_names column_names/>
                        <ResultGroup entity="document" title="Documents" hits=groups.documents board_names column_names/>
                        <ResultGroup entity="adr" title="ADRs" hits=groups.adrs board_names column_names/>
                    }.into_any()
                }
            }}
        </Stack>
    }
}

/// One entity-type result group: a panel with linked rows (omitted when
/// the group is empty, mirroring the wire shape).
#[component]
fn ResultGroup(
    entity: &'static str,
    title: &'static str,
    hits: Vec<data::Hit>,
    /// Board id → slug.
    board_names: Signal<HashMap<String, String>>,
    /// Column id → name, for the boards of the hits.
    column_names: Signal<HashMap<String, String>>,
) -> impl IntoView {
    (!hits.is_empty()).then(|| {
        let count = hits.len();
        // KAIROS-T-0163: say how much of this page is put-away work
        // before the reader scans the rows, not after.
        let put_away = hits.iter().filter(|hit| hit.archived_at.is_some()).count();
        let caption = if put_away > 0 {
            format!("{count} on this page · {put_away} put away")
        } else {
            format!("{count} on this page")
        };
        let rows = hits
            .into_iter()
            .map(|hit| {
                let detail = format!("/items/{}", hit.short_code);
                let explore = format!("/search/relationships/{}", hit.short_code);
                let created = hit.created_at.clone();
                let task_pill = hit.task_type.clone().filter(|t| t != "task").map(|t| {
                    let color = if t == "bug" { token::BAD } else { token::GOLD };
                    view! { <Pill color=color>{t}</Pill> }
                });
                let bucket_pill = (hit.is_bucket == Some(true))
                    .then(|| view! { <Pill color=token::MUTED>"bucket"</Pill> });
                // The ADR-20 state, in the words the item page uses
                // (KAIROS-T-0164). Never a bare "archived": a document
                // hit can be editorially `lifecycle: archived` and still
                // be live work (KAIROS-T-0078) — two different states,
                // and this list shows documents.
                let put_away_badge = hit.archived_at.clone().map(|when| {
                    let title = format!("Put away on {}", crate::pages::item::put_away_when(&when));
                    view! {
                        <span class="kairos-archived-badge" title=title>
                            <Pill color=token::GOLD>"put away"</Pill>
                        </span>
                    }
                });
                let is_put_away = hit.archived_at.is_some();
                // KAIROS-T-0602: the hit has a snippet, not the content.
                let snippet = hit
                    .snippet
                    .clone()
                    .map(|text| view! { <Text dimmed=true size="xs">{text}</Text> });
                // KAIROS-T-0583: where the hit is, and who has it.
                let (board_id, column_id) = (hit.board_id.clone(), hit.column_id.clone());
                let place = move || {
                    let board = board_id
                        .as_ref()
                        .and_then(|id| board_names.with(|names| names.get(id).cloned()));
                    let column = column_id
                        .as_ref()
                        .and_then(|id| column_names.with(|names| names.get(id).cloned()));
                    place_label(board.as_deref(), column.as_deref())
                };
                let person = person_label(&hit);
                view! {
                    <tr class:kairos-search__hit--put-away=is_put_away>
                        <td>
                            <Anchor href=detail>
                                <span class="cl-mono">{hit.short_code.clone()}</span>
                            </Anchor>
                        </td>
                        <td>
                            <Group gap="sm">
                                <Text size="sm">{hit.title.clone()}</Text>
                                {put_away_badge}
                                {task_pill}
                                {bucket_pill}
                            </Group>
                            {snippet}
                        </td>
                        <td data-testid="search-hit-place"><Text dimmed=true size="sm">{place}</Text></td>
                        <td data-testid="search-hit-person"><Text dimmed=true size="sm">{person}</Text></td>
                        <td><Text dimmed=true size="sm"><RelativeTime iso=created/></Text></td>
                        <td><Anchor href=explore>"relationships"</Anchor></td>
                    </tr>
                }
            })
            .collect_view();
        view! {
            <Panel title=title caption=caption>
                <Group gap="sm">
                    <Pill color=entity_color(entity)>{entity}</Pill>
                </Group>
                // The same widths in each group, so the tables line up.
                <Table
                    label=title
                    fixed=true
                    widths=vec![
                        "13%".into(),
                        "37%".into(),
                        "16%".into(),
                        "12%".into(),
                        "9%".into(),
                        "13%".into(),
                    ]
                >
                    <thead>
                        <tr>
                            <th>"Code"</th>
                            <th>"Title"</th>
                            <th>"Place"</th>
                            <th>"Person"</th>
                            <th>"Created"</th>
                            <th>"Explore"</th>
                        </tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </Table>
            </Panel>
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A select offers "any" first and keeps a value from the link that
    /// the list does not have.
    #[test]
    fn a_select_keeps_the_value_of_the_link() {
        let rest = vec![("kairos".to_string(), "kairos".to_string())];
        let options = choices("Any", rest.clone(), "gone");
        assert_eq!(options[0], (String::new(), "Any".to_string()));
        assert_eq!(
            options.last(),
            Some(&("gone".to_string(), "gone".to_string()))
        );
        assert_eq!(choices("Any", rest.clone(), "kairos").len(), 2);
        assert_eq!(choices("Any", rest, "").len(), 2);
    }

    /// The place of a hit is "board · column", or the part that is known.
    #[test]
    fn the_place_of_a_hit() {
        assert_eq!(place_label(Some("kairos"), Some("Todo")), "kairos · Todo");
        assert_eq!(place_label(Some("kairos"), None), "kairos");
        assert_eq!(place_label(None, None), "");
    }

    /// The person of a hit: the claim, marked for an agent, or the owner.
    #[test]
    fn the_person_of_a_hit() {
        let hit = |claim: Option<data::HitPerson>, owner: Option<data::HitPerson>| data::Hit {
            short_code: "DEMO-T-0001".into(),
            title: "t".into(),
            task_type: None,
            is_bucket: None,
            created_at: "2026-07-01T10:00:00Z".into(),
            archived_at: None,
            snippet: None,
            board_id: None,
            column_id: None,
            claim,
            owner,
        };
        let person = |name: &str, agent: bool| data::HitPerson {
            display_name: name.into(),
            agent,
        };
        assert_eq!(
            person_label(&hit(Some(person("Robin", false)), None)),
            "Robin"
        );
        assert_eq!(
            person_label(&hit(Some(person("Robin", true)), None)),
            "Robin (agent)"
        );
        assert_eq!(
            person_label(&hit(None, Some(person("Sam", false)))),
            "Sam (owner)"
        );
        assert_eq!(person_label(&hit(None, None)), "");
    }
}
