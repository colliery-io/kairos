//! The relationships explorer (KAIROS-T-0042): one item's place in the
//! KAIROS-A-0001 graph — parent-chain breadcrumb, children, blockers in
//! both directions, supporting documents/ADRs — plus org-admin link /
//! unlink affordances that surface the T-0020 typed 422s
//! (`RELATIONSHIP_RULE`, `CYCLE_DETECTED`, `ALREADY_LINKED`) through the
//! conventions' `ApiError` pattern.
//!
//! Routing: mounted at `/search/relationships/:code` (T-0042's path
//! segment, per docs/gui-conventions.md §3 — `/items/:code` belongs to
//! T-0041). [`RelationshipsView`] itself is standalone-by-design so the
//! item-detail task can embed it on `/items/:code` later; the handoff is
//! recorded in KAIROS-T-0042's status updates.

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill,
    SegmentedControl, Select, Stack, Text, TextInput,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::{RELATIONSHIPS, data, entity_color};
use crate::api;
use crate::auth::use_auth;

/// `/search/relationships/:code` — route wrapper; re-mounts the view when
/// the `:code` param changes (explorer links navigate item → item).
#[component]
pub fn RelationshipsPage() -> impl IntoView {
    let params = use_params_map();
    view! {
        {move || {
            let code = params
                .read()
                .get("code")
                .unwrap_or_default()
                .to_uppercase();
            view! { <RelationshipsView short_code=code/> }
        }}
    }
}

/// One item's relationships, explorable. Standalone on purpose — T-0041's
/// item detail can mount it as-is (`short_code` is its only input; auth
/// and data flow through context + the module's own fetchers).
#[component]
pub fn RelationshipsView(#[prop(into)] short_code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(short_code);
    let reload = RwSignal::new(0u32);

    // The item header + both relationship directions + the ancestor chain,
    // fetched together: the view renders whole or not at all (§5 states).
    let bundle = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        let code = code.get_value();
        async move {
            let item = data::item_summary(auth, &code).await?;
            let rels = data::relationships(auth, &code).await?;
            let chain = data::parent_chain(auth, &code).await?;
            Ok::<_, ApiError>((item, rels, chain))
        }
    });

    // Org-admin gating comes from /api/whoami (conventions §6: roles from
    // whoami data, never token introspection).
    let whoami = LocalResource::new(move || {
        let _ = auth.token();
        api::whoami(auth)
    });
    let is_admin = move || matches!(whoami.get(), Some(Ok(me)) if me.organization.role == "admin");

    // Link/unlink outcome, section-scoped (§5: Alert for notices,
    // ErrorState for the typed ApiError — 422 reason codes included).
    let feedback = RwSignal::new(None::<Result<String, ApiError>>);

    // ---- link form (org admin) --------------------------------------------
    let role = RwSignal::new("source".to_string());
    let other = RwSignal::new(String::new());
    let rel_kind = RwSignal::new("parent".to_string());
    let busy = RwSignal::new(false);

    let create_link = move || {
        let this = code.get_value();
        let other_code = other.get().trim().to_uppercase();
        if other_code.is_empty() {
            feedback.set(Some(Err(ApiError::Unknown(
                "Enter the other item's short code.".to_string(),
            ))));
            return;
        }
        let (source, target) = if role.get() == "source" {
            (this, other_code)
        } else {
            (other_code, this)
        };
        let request = data::CreateRelationship {
            source_short_code: source,
            target_short_code: target,
            relationship: rel_kind.get(),
        };
        busy.set(true);
        leptos::task::spawn_local(async move {
            match data::create_relationship(auth, &request).await {
                Ok(_) => {
                    feedback.set(Some(Ok(format!(
                        "Linked {} \u{2014}{}\u{2192} {}.",
                        request.source_short_code, request.relationship, request.target_short_code
                    ))));
                    reload.update(|n| *n += 1);
                }
                Err(error) => feedback.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };

    // ---- row renderer -------------------------------------------------------
    // `label` prefixes the row for mixed sections ("supports →" etc.);
    // empty for single-relationship sections.
    let related_rows = move |items: Vec<(String, data::RelatedItem)>, empty: &'static str| {
        if items.is_empty() {
            return view! { <Empty message=empty/> }.into_any();
        }
        items
            .into_iter()
            .map(|(label, item)| {
                let detail = format!("/items/{}", item.short_code);
                let explore = format!("/search/relationships/{}", item.short_code);
                let edge_id = item.relationship_id.clone();
                let edge_desc = format!("{} ({label})", item.short_code);
                view! {
                    <Group justify="between" wrap=true>
                        <Group gap="sm" wrap=true>
                            {(!label.is_empty()).then(|| view! {
                                <Pill color=token::MUTED>{label.clone()}</Pill>
                            })}
                            <Pill color=entity_color(&item.entity_type)>
                                {item.entity_type.clone()}
                            </Pill>
                            <Anchor href=detail>
                                <span class="cl-mono">{item.short_code.clone()}</span>
                            </Anchor>
                            <Text size="sm">{item.title.clone()}</Text>
                            <Anchor href=explore>"explore"</Anchor>
                        </Group>
                        {move || is_admin().then(|| {
                            let edge_id = edge_id.clone();
                            let edge_desc = edge_desc.clone();
                            view! {
                                <Button
                                    variant="default"
                                    size="xs"
                                    bad=true
                                    on_click=Callback::new(move |_| {
                                        let edge_id = edge_id.clone();
                                        let edge_desc = edge_desc.clone();
                                        leptos::task::spawn_local(async move {
                                            match data::delete_relationship(auth, &edge_id).await {
                                                Ok(_) => {
                                                    feedback.set(Some(Ok(format!(
                                                        "Unlinked {edge_desc}."
                                                    ))));
                                                    reload.update(|n| *n += 1);
                                                }
                                                Err(error) => {
                                                    feedback.set(Some(Err(error)));
                                                }
                                            }
                                        });
                                    })
                                >
                                    "Unlink"
                                </Button>
                            }
                        })}
                    </Group>
                }
            })
            .collect_view()
            .into_any()
    };

    let plain = |items: Vec<data::RelatedItem>| -> Vec<(String, data::RelatedItem)> {
        items.into_iter().map(|i| (String::new(), i)).collect()
    };

    view! {
        <PageHeader
            title=code.get_value()
            sub="relationships — parent chain, children, blockers, supporting material"
        />
        <Stack gap="md">
            {move || feedback.get().map(|outcome| match outcome {
                Ok(message) => view! {
                    <Alert title="Done" color=token::OK>
                        <Text size="sm">{message}</Text>
                    </Alert>
                }.into_any(),
                Err(error) => view! { <ErrorState error=error/> }.into_any(),
            })}

            {move || match bundle.get() {
                None => view! { <Loading label="Walking the graph…"/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState
                        error=error
                        on_retry=Callback::new(move |_| reload.update(|n| *n += 1))
                    />
                }.into_any(),
                Some(Ok((item, rels, chain))) => {
                    let top_level = chain.is_empty();
                    // Breadcrumb: root ancestor → … → this item.
                    let crumbs = chain
                        .into_iter()
                        .map(|crumb| {
                            let href = format!("/search/relationships/{}", crumb.short_code);
                            view! {
                                <Group gap="xs">
                                    <Pill color=entity_color(&crumb.entity_type)>
                                        {crumb.entity_type.clone()}
                                    </Pill>
                                    <Anchor href=href>
                                        <span class="cl-mono">{crumb.short_code.clone()}</span>
                                    </Anchor>
                                    <Text dimmed=true size="sm">{crumb.title.clone()}</Text>
                                    <Text dimmed=true>"›"</Text>
                                </Group>
                            }
                        })
                        .collect_view();

                    let children = rels.group("parent", true);
                    let blocked_by = rels.group("blocks", false);
                    let blocking = rels.group("blocks", true);
                    let mut supporting: Vec<(String, data::RelatedItem)> = Vec::new();
                    for relationship in ["supports", "informs", "supersedes"] {
                        for item in rels.group(relationship, true) {
                            supporting.push((format!("{relationship} \u{2192}"), item));
                        }
                        for item in rels.group(relationship, false) {
                            supporting.push((format!("\u{2190} {relationship}"), item));
                        }
                    }

                    let detail_href = format!("/items/{}", item.short_code);
                    let item_code = item.short_code;
                    let item_title = item.title;
                    view! {
                        <Panel title="Lineage" caption="parent chain (incoming parent edges)">
                            <Group gap="sm" wrap=true>
                                {crumbs}
                                {top_level.then(|| view! {
                                    <Text dimmed=true size="sm">
                                        "top level (no parent) —"
                                    </Text>
                                })}
                                <Text bright=true bold=true>
                                    <span class="cl-mono">{item_code}</span>
                                </Text>
                                <Text size="sm">{item_title}</Text>
                                <Anchor href=detail_href>
                                    "open detail"
                                </Anchor>
                            </Group>
                        </Panel>
                        <Panel title="Children" caption="outgoing parent edges">
                            <Stack gap="xs">
                                {related_rows(
                                    plain(children),
                                    "No children — link one below (org admin) or from a create flow.",
                                )}
                            </Stack>
                        </Panel>
                        <Panel title="Blocked by" caption="incoming blocks edges">
                            <Stack gap="xs">
                                {related_rows(
                                    plain(blocked_by),
                                    "Nothing blocks this item.",
                                )}
                            </Stack>
                        </Panel>
                        <Panel title="Blocks" caption="outgoing blocks edges">
                            <Stack gap="xs">
                                {related_rows(
                                    plain(blocking),
                                    "This item blocks nothing.",
                                )}
                            </Stack>
                        </Panel>
                        <Panel
                            title="Supporting material"
                            caption="documents & ADRs — supports · informs · supersedes, both directions"
                        >
                            <Stack gap="xs">
                                {related_rows(
                                    supporting,
                                    "No supporting documents or ADRs are linked.",
                                )}
                            </Stack>
                        </Panel>
                    }.into_any()
                }
            }}

            {move || is_admin().then(|| view! {
                <Panel
                    title="Link items"
                    caption="org admin — the server enforces the A-0001 rule matrix and cycle checks"
                >
                    <Stack gap="sm">
                        <Group gap="sm" wrap=true top=true>
                            <Stack gap="xs">
                                <Text dimmed=true size="xs">
                                    {format!("Role of {} in the new edge", code.get_value())}
                                </Text>
                                <SegmentedControl
                                    options=vec!["source".to_string(), "target".to_string()]
                                    value=role
                                />
                            </Stack>
                            <TextInput
                                label="Other item (short code)"
                                placeholder="DEMO-T-0001"
                                value=other
                            />
                            <Select
                                label="Relationship"
                                options=RELATIONSHIPS.iter().map(|r| r.to_string()).collect()
                                value=rel_kind
                            />
                            {move || view! {
                                <Button
                                    disabled=busy.get()
                                    on_click=Callback::new(move |_| create_link())
                                >
                                    {if busy.get() { "Linking…" } else { "Create link" }}
                                </Button>
                            }}
                        </Group>
                        <Text dimmed=true size="xs">
                            "Rejections surface the API's typed 422 reason (RELATIONSHIP_RULE, CYCLE_DETECTED, ALREADY_LINKED) above."
                        </Text>
                    </Stack>
                </Panel>
            })}
        </Stack>
    }
}
