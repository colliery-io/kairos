//! Unified search (KAIROS-T-0042, contract per KAIROS-A-0007 / S-0005):
//! one text query + a structured filter builder + an optional graph
//! traversal, composed into a single `POST /api/search`, with results
//! grouped by entity type and paginated over the combined set.
//!
//! The relationships explorer lives in [`relationships`] and mounts at
//! `/search/relationships/:code` — under this task's path segment per
//! docs/gui-conventions.md §3 ("sub-routes under your own path segment
//! only"; `/items/:code` belongs to T-0041). Its `RelationshipsView`
//! component is standalone so the item-detail task can also embed it.

pub mod data;
pub mod graph;
pub mod graph_layout;
pub mod relationships;

use aurora_dark::components::{
    ActionIcon, Alert, Anchor, Button, Chip, Divider, Empty, ErrorState, Group, Loading,
    NumberInput, PageHeader, Panel, Pill, SegmentedControl, Select, Stack, Switch, Table, Text,
    TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use crate::auth::use_auth;

pub use relationships::RelationshipsPage;

/// The entity-type vocabulary (S-0005 `filter.entity_type`).
const ENTITY_TYPES: [&str; 5] = ["strategy", "initiative", "task", "document", "adr"];

/// The task-type vocabulary (S-0005 `filter.task_type`).
const TASK_TYPES: [&str; 3] = ["task", "bug", "tech_debt"];

/// The relationship vocabulary (S-0005 `traverse.relationships`).
const RELATIONSHIPS: [&str; 5] = ["parent", "supports", "informs", "supersedes", "blocks"];

/// The "no board/column selected" option.
const ANY: &str = "any";

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

/// `/search` — the unified search page.
#[component]
pub fn SearchPage() -> impl IntoView {
    let auth = use_auth();

    // ---- form state ------------------------------------------------------
    let q = RwSignal::new(String::new());
    let entity_flags: [RwSignal<bool>; 5] = std::array::from_fn(|_| RwSignal::new(false));
    let task_flags: [RwSignal<bool>; 3] = std::array::from_fn(|_| RwSignal::new(false));
    let board_sel = RwSignal::new(ANY.to_string());
    let column_sel = RwSignal::new(ANY.to_string());
    let created_after = RwSignal::new(String::new());
    let created_before = RwSignal::new(String::new());
    let meta_rows: RwSignal<Vec<MetaRow>> = RwSignal::new(Vec::new());
    let next_meta_id = StoredValue::new(0usize);
    // ADR-20 rule 3: default listings hide archived work, so this starts
    // OFF and the page sends nothing about it until someone asks.
    let include_put_away = RwSignal::new(false);
    let traverse_on = RwSignal::new(false);
    let t_from = RwSignal::new(String::new());
    let t_rel_flags: [RwSignal<bool>; 5] = std::array::from_fn(|i| RwSignal::new(i == 0));
    let t_direction = RwSignal::new("outbound".to_string());
    let t_depth = RwSignal::new(3.0_f64);
    let page_size = RwSignal::new("10".to_string());
    let form_error = RwSignal::new(None::<String>);

    // The submitted request drives the results resource; pagination edits
    // its offset in place (server state is the source of truth — every
    // page is a fresh POST /api/search).
    let submitted = RwSignal::new(None::<data::SearchRequest>);

    // ---- option sources ---------------------------------------------------
    let boards = LocalResource::new(move || {
        let _ = auth.token();
        data::boards(auth)
    });
    let columns = LocalResource::new(move || {
        let _ = auth.token();
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
    // Changing the board invalidates the column choice.
    Effect::new(move |_| {
        let _ = board_sel.get();
        column_sel.set(ANY.to_string());
    });

    // ---- results ----------------------------------------------------------
    let results = LocalResource::new(move || {
        let _ = auth.token();
        let request = submitted.get();
        async move {
            match request {
                None => Ok(None),
                Some(request) => data::search(auth, &request).await.map(Some),
            }
        }
    });

    // ---- submit ------------------------------------------------------------
    let build_and_submit = move || {
        let text = q.get().trim().to_string();

        let mut filter = data::SearchFilter::default();
        let entity_type: Vec<String> = ENTITY_TYPES
            .iter()
            .zip(entity_flags)
            .filter(|(_, flag)| flag.get())
            .map(|(name, _)| (*name).to_string())
            .collect();
        if !entity_type.is_empty() {
            filter.entity_type = Some(entity_type);
        }
        let task_type: Vec<String> = TASK_TYPES
            .iter()
            .zip(task_flags)
            .filter(|(_, flag)| flag.get())
            .map(|(name, _)| (*name).to_string())
            .collect();
        if !task_type.is_empty() {
            filter.task_type = Some(task_type);
        }
        if board_sel.get() != ANY
            && let Some(Ok(list)) = boards.get()
            && let Some(board) = list.iter().find(|b| b.slug == board_sel.get())
        {
            filter.board_id = Some(board.id.clone());
        }
        if column_sel.get() != ANY
            && let Some(Ok(cols)) = columns.get()
            && let Some(col) = cols.iter().find(|c| c.name == column_sel.get())
        {
            filter.column_id = Some(col.id.clone());
        }
        let after = created_after.get().trim().to_string();
        if !after.is_empty() {
            filter.created_after = Some(format!("{after}T00:00:00Z"));
        }
        let before = created_before.get().trim().to_string();
        if !before.is_empty() {
            // Inclusive end date ("strictly before" server-side).
            filter.created_before = Some(format!("{before}T23:59:59Z"));
        }
        let metadata: std::collections::BTreeMap<String, String> = meta_rows
            .get()
            .iter()
            .filter_map(|row| {
                let key = row.key.get().trim().to_string();
                let value = row.value.get().trim().to_string();
                (!key.is_empty() && !value.is_empty()).then_some((key, value))
            })
            .collect();
        if !metadata.is_empty() {
            filter.metadata = Some(metadata);
        }
        // KAIROS-T-0163 / ADR-20 rule 2: archived work stays searchable
        // when asked for explicitly, composing with everything above —
        // and since KAIROS-T-0157 the flag constrains on its own, so
        // "just show me what's been put away" is a complete search.
        filter.include_deleted = include_put_away.get();

        let traverse = if traverse_on.get() {
            let from = t_from.get().trim().to_uppercase();
            if from.is_empty() {
                form_error.set(Some(
                    "Traversal is on but has no starting short code.".to_string(),
                ));
                return;
            }
            let rels: Vec<String> = RELATIONSHIPS
                .iter()
                .zip(t_rel_flags)
                .filter(|(_, flag)| flag.get())
                .map(|(name, _)| (*name).to_string())
                .collect();
            if rels.is_empty() {
                form_error.set(Some(
                    "Traversal needs at least one relationship type.".to_string(),
                ));
                return;
            }
            Some(data::SearchTraverse {
                from: data::SearchTraverseFrom { short_code: from },
                relationships: rels,
                direction: t_direction.get(),
                depth: (t_depth.get() as u32).clamp(1, 10),
            })
        } else {
            None
        };

        let request = data::SearchRequest {
            q: (!text.is_empty()).then_some(text),
            filter: (!filter.is_empty()).then_some(filter),
            traverse,
            limit: page_size.get().parse::<i64>().ok(),
            offset: Some(0),
        };
        if request.q.is_none() && request.filter.is_none() && request.traverse.is_none() {
            form_error.set(Some(
                "Add a text query, at least one filter, or a traversal.".to_string(),
            ));
            return;
        }
        form_error.set(None);
        submitted.set(Some(request));
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
    // Built lazily — the traverse block re-renders on every toggle.
    let rel_chips = move || chip_row(&RELATIONSHIPS, &t_rel_flags);

    view! {
        <PageHeader title="Search" sub="text, filters, and graph traversal — composed (A-0007)"/>
        <Stack gap="md">
            <Panel title="Query" caption="capabilities AND together">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true>
                        <TextInput
                            label="Text query"
                            placeholder="e.g. sign-up, \"magic link\", auth OR billing"
                            value=q
                        />
                        <Select
                            label="Page size"
                            options=vec![
                                "5".to_string(),
                                "10".to_string(),
                                "25".to_string(),
                                "50".to_string(),
                            ]
                            value=page_size
                        />
                        <Button on_click=Callback::new(move |_| build_and_submit())>
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
                                "Off by default: put-away (archived) work is hidden from \
                                 boards, queues and search until it is asked for. It is \
                                 still readable — hits are marked below."
                            </Text>
                        </Stack>
                    </div>
                    <Stack gap="xs">
                        <Text dimmed=true size="xs">"Entity types"</Text>
                        <Group gap="xs" wrap=true>{entity_chips}</Group>
                    </Stack>
                    <Group gap="sm" wrap=true top=true>
                        {move || {
                            let options = match boards.get() {
                                Some(Ok(list)) => std::iter::once(ANY.to_string())
                                    .chain(list.iter().map(|b| b.slug.clone()))
                                    .collect::<Vec<_>>(),
                                _ => vec![ANY.to_string()],
                            };
                            view! { <Select label="Board" options=options value=board_sel/> }
                        }}
                        {move || {
                            let options = match columns.get() {
                                Some(Ok(cols)) => std::iter::once(ANY.to_string())
                                    .chain(cols.iter().map(|c| c.name.clone()))
                                    .collect::<Vec<_>>(),
                                _ => vec![ANY.to_string()],
                            };
                            view! { <Select label="Column" options=options value=column_sel/> }
                        }}
                        <TextInput
                            label="Created after"
                            placeholder="YYYY-MM-DD"
                            value=created_after
                        />
                        <TextInput
                            label="Created before"
                            placeholder="YYYY-MM-DD"
                            value=created_before
                        />
                    </Group>
                    <Stack gap="xs">
                        <Text dimmed=true size="xs">"Task types"</Text>
                        <Group gap="xs" wrap=true>{task_chips}</Group>
                    </Stack>
                    <Stack gap="xs">
                        <Group gap="sm">
                            <Text dimmed=true size="xs">"Metadata (key = value, AND)"</Text>
                            <Button
                                variant="default"
                                size="xs"
                                on_click=Callback::new(move |_| {
                                    let id = next_meta_id.get_value();
                                    next_meta_id.set_value(id + 1);
                                    meta_rows.update(|rows| rows.push(MetaRow {
                                        id,
                                        key: RwSignal::new(String::new()),
                                        value: RwSignal::new(String::new()),
                                    }));
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
                    {move || form_error.get().map(|message| view! {
                        <Alert title="Nothing to search" color=token::GOLD>
                            <Text size="sm">{message}</Text>
                        </Alert>
                    })}
                </Stack>
            </Panel>

            {move || match results.get() {
                None => view! { <Loading label="Searching…"/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState
                        error=error
                        on_retry=Callback::new(move |_| submitted.update(|_| {}))
                    />
                }.into_any(),
                Some(Ok(None)) => view! {
                    <Empty message="Search across strategies, initiatives, tasks, documents, and ADRs — add a query, filters, or a traversal, then press Search."/>
                }.into_any(),
                Some(Ok(Some(response))) if response.total == 0 => view! {
                    <Empty message="No matches — loosen the query or the filters."/>
                }.into_any(),
                Some(Ok(Some(response))) => {
                    let (total, limit, offset) =
                        (response.total, response.limit.max(1), response.offset);
                    let start = offset + 1;
                    let end = (offset + limit).min(total);
                    let groups = response.results;
                    view! {
                        <Group justify="between">
                            <Text dimmed=true size="sm">
                                {format!("Showing {start}–{end} of {total} (grouped by type)")}
                            </Text>
                            <Group gap="sm">
                                <Button
                                    variant="default"
                                    size="xs"
                                    disabled={offset == 0}
                                    on_click=Callback::new(move |_| submitted.update(|r| {
                                        if let Some(r) = r {
                                            r.offset = Some((offset - limit).max(0));
                                        }
                                    }))
                                >
                                    "‹ Prev"
                                </Button>
                                <Button
                                    variant="default"
                                    size="xs"
                                    disabled={offset + limit >= total}
                                    on_click=Callback::new(move |_| submitted.update(|r| {
                                        if let Some(r) = r {
                                            r.offset = Some(offset + limit);
                                        }
                                    }))
                                >
                                    "Next ›"
                                </Button>
                            </Group>
                        </Group>
                        <ResultGroup entity="strategy" title="Strategies" hits=groups.strategies/>
                        <ResultGroup entity="initiative" title="Initiatives" hits=groups.initiatives/>
                        <ResultGroup entity="task" title="Tasks" hits=groups.tasks/>
                        <ResultGroup entity="document" title="Documents" hits=groups.documents/>
                        <ResultGroup entity="adr" title="ADRs" hits=groups.adrs/>
                    }.into_any()
                }
            }}
        </Stack>
    }
}

/// One entity-type result group: a panel with linked rows (omitted when
/// the group is empty, mirroring the wire shape).
#[component]
fn ResultGroup(entity: &'static str, title: &'static str, hits: Vec<data::Hit>) -> impl IntoView {
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
                let created = hit
                    .created_at
                    .get(..10)
                    .map(str::to_string)
                    .unwrap_or_else(|| hit.created_at.clone());
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
                        </td>
                        <td><Text dimmed=true size="sm">{created}</Text></td>
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
                <Table>
                    <thead>
                        <tr>
                            <th>"Code"</th>
                            <th>"Title"</th>
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
