//! `/admin/templates` — document template CRUD (KAIROS-T-0043, A-0003):
//! starter markdown content plus the metadata fields a template stamps at
//! create-from-template time (associations reference metadata-definition
//! slugs; the association list is replaced wholesale on save).

use aurora_dark::components::{
    Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Switch, Text, TextInput, Textarea,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

/// One editable metadata-association row (definition slug + default +
/// required). Signals per field so rows edit independently.
#[derive(Clone, Copy)]
struct EntryRow {
    slug: RwSignal<String>,
    default_value: RwSignal<String>,
    required: RwSignal<bool>,
}

impl EntryRow {
    fn new(slug: &str, default_value: &str, required: bool) -> Self {
        Self {
            slug: RwSignal::new(slug.to_string()),
            default_value: RwSignal::new(default_value.to_string()),
            required: RwSignal::new(required),
        }
    }

    fn to_entry(self) -> api::TemplateMetadataEntry {
        let default_value = self.default_value.get_untracked();
        api::TemplateMetadataEntry {
            definition_slug: self.slug.get_untracked(),
            default_value: (!default_value.is_empty()).then_some(default_value),
            required: self.required.get_untracked(),
        }
    }
}

/// The metadata-association editor: rows of definition-slug pickers.
#[component]
fn AssociationsEditor(
    rows: RwSignal<Vec<EntryRow>>,
    definition_slugs: Vec<String>,
) -> impl IntoView {
    let slugs = StoredValue::new(definition_slugs);
    view! {
        <Stack gap="xs">
            <Text dimmed=true size="sm">
                "Metadata fields stamped when a document is created from this template:"
            </Text>
            {move || rows.get().into_iter().enumerate().map(|(index, row)| {
                let mut options = slugs.get_value();
                options.insert(0, String::new());
                view! {
                    <Group gap="sm" wrap=true top=true>
                        <Select label="Definition" options value=row.slug/>
                        <TextInput label="Default (optional)" value=row.default_value/>
                        <Switch checked=row.required label="required"/>
                        <Button variant="subtle" size="xs"
                            on_click=Callback::new(move |_| {
                                rows.update(|list| { list.remove(index); });
                            })>
                            "Remove field"
                        </Button>
                    </Group>
                }
            }).collect_view()}
            <Group>
                <Button variant="default" size="xs"
                    on_click=Callback::new(move |_| {
                        rows.update(|list| list.push(EntryRow::new("", "", false)));
                    })>
                    "Add metadata field"
                </Button>
            </Group>
        </Stack>
    }
}

/// `/admin/templates`.
#[component]
pub fn AdminTemplatesPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);

    let templates = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_templates(auth)
    });
    let definitions = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_definitions(auth)
    });
    let definition_slugs = move || {
        definitions
            .get()
            .and_then(|result| result.ok())
            .map(|list| {
                list.into_iter()
                    .map(|definition| definition.slug)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    // Create form.
    let name = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let entries = RwSignal::new(Vec::<EntryRow>::new());

    let on_create = move |_| {
        let (n, s, c) = (
            name.get_untracked(),
            slug.get_untracked(),
            content.get_untracked(),
        );
        let metadata: Vec<api::TemplateMetadataEntry> = entries
            .get_untracked()
            .into_iter()
            .map(EntryRow::to_entry)
            .filter(|entry| !entry.definition_slug.is_empty())
            .collect();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Template \"{n}\" created."),
            async move {
                api::create_template(auth, &n, &s, &c, &metadata)
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Templates" sub="starter content + stamped metadata for new documents"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="Templates">
                {move || match templates.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message="No templates yet — create one below."/>
                    }.into_any(),
                    Some(Ok(list)) => {
                        let slugs = definition_slugs();
                        list.into_iter().map(|template| {
                            view! {
                                <TemplateRow template busy outcome reload
                                    definition_slugs=slugs.clone()/>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </Panel>
            <Panel title="Create template">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Name" value=name placeholder="e.g. PRD"/>
                        <TextInput label="Slug" value=slug placeholder="e.g. prd"/>
                    </Group>
                    <Textarea label="Starter content (markdown)" value=content rows=6/>
                    {move || view! {
                        <AssociationsEditor rows=entries definition_slugs=definition_slugs()/>
                    }}
                    <Group>
                        <Button on_click=Callback::new(on_create)>"Create template"</Button>
                    </Group>
                </Stack>
            </Panel>
        </Stack>
    }
}

/// One template row: identity, edit (loads the full detail on open),
/// delete.
#[component]
fn TemplateRow(
    template: api::Template,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    definition_slugs: Vec<String>,
) -> impl IntoView {
    let auth = use_auth();
    let template_id = StoredValue::new(template.id.clone());
    // StoredValue keeps the Show children `Fn` (nothing owned moves in).
    let slugs = StoredValue::new(definition_slugs);
    let editing = RwSignal::new(false);
    let edit_name = RwSignal::new(template.name.clone());
    let edit_slug = RwSignal::new(template.slug.clone());
    let edit_content = RwSignal::new(String::new());
    let edit_entries = RwSignal::new(Vec::<EntryRow>::new());
    let deleted_name = template.name.clone();

    // Edit opens after the full detail (content + associations) loads —
    // the list endpoint only carries the identity fields.
    let on_edit = move |_| {
        if editing.get_untracked() {
            editing.set(false);
            return;
        }
        let id = template_id.get_value();
        leptos::task::spawn_local(async move {
            match api::template_detail(auth, &id).await {
                Ok(detail) => {
                    edit_name.set(detail.name);
                    edit_slug.set(detail.slug);
                    edit_content.set(detail.content);
                    edit_entries.set(
                        detail
                            .metadata
                            .iter()
                            .map(|field| {
                                EntryRow::new(
                                    &field.slug,
                                    field.default_value.as_deref().unwrap_or(""),
                                    field.required,
                                )
                            })
                            .collect(),
                    );
                    editing.set(true);
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };

    let on_save = move |_| {
        let id = template_id.get_value();
        let (n, s, c) = (
            edit_name.get_untracked(),
            edit_slug.get_untracked(),
            edit_content.get_untracked(),
        );
        let metadata: Vec<api::TemplateMetadataEntry> = edit_entries
            .get_untracked()
            .into_iter()
            .map(EntryRow::to_entry)
            .filter(|entry| !entry.definition_slug.is_empty())
            .collect();
        editing.set(false);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Template \"{n}\" updated."),
            async move {
                api::update_template(auth, &id, &n, &s, &c, &metadata)
                    .await
                    .map(|_| ())
            },
        );
    };
    let on_delete = move |_| {
        let id = template_id.get_value();
        let deleted_name = deleted_name.clone();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Template \"{deleted_name}\" deleted."),
            async move { api::delete_template(auth, &id).await.map(|_| ()) },
        );
    };

    view! {
        <Stack gap="xs">
            <Group justify="between" wrap=true>
                <Group gap="sm">
                    <Text bright=true>{template.name.clone()}</Text>
                    <Code>{template.slug.clone()}</Code>
                    {template.is_system_default.then(|| view! {
                        <Pill color=token::GOLD>"system default"</Pill>
                    })}
                </Group>
                <Group gap="xs">
                    <Button variant="default" size="xs" on_click=Callback::new(on_edit)>
                        {move || if editing.get() { "Close" } else { "Edit" }}
                    </Button>
                    <Button variant="default" size="xs" bad=true on_click=Callback::new(on_delete)>
                        "Delete"
                    </Button>
                </Group>
            </Group>
            <Show when=move || editing.get()>
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Name" value=edit_name/>
                        <TextInput label="Slug" value=edit_slug/>
                    </Group>
                    <Textarea label="Content (markdown)" value=edit_content rows=8/>
                    <AssociationsEditor rows=edit_entries
                        definition_slugs=slugs.get_value()/>
                    <Group>
                        <Button size="xs" on_click=Callback::new(on_save)>"Save template"</Button>
                    </Group>
                </Stack>
            </Show>
            <Divider/>
        </Stack>
    }
}
