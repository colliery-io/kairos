//! `/admin/metadata` — metadata definition CRUD (KAIROS-T-0043, A-0003):
//! typed fields (`string` / `enum` / `date`) items can carry; enum
//! definitions get an option-list editor (the server requires non-empty
//! options for enums and forbids them otherwise — 422 `VALIDATION`).

use aurora_dark::components::{
    Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

const FIELD_TYPES: [&str; 3] = ["string", "enum", "date"];

/// A reusable enum-option list editor over one `RwSignal<Vec<String>>`.
#[component]
fn EnumOptionsEditor(options: RwSignal<Vec<String>>) -> impl IntoView {
    let draft = RwSignal::new(String::new());
    let on_add = move |_| {
        let option = draft.get_untracked().trim().to_string();
        if option.is_empty() {
            return;
        }
        options.update(|list| {
            if !list.contains(&option) {
                list.push(option);
            }
        });
        draft.set(String::new());
    };
    view! {
        <Stack gap="xs">
            <Text dimmed=true size="sm">"Allowed values (display order):"</Text>
            <Group gap="xs" wrap=true>
                {move || options.get().into_iter().enumerate().map(|(index, option)| {
                    view! {
                        <Group gap="xs">
                            <Pill color=token::TEAL>{option}</Pill>
                            <Button variant="subtle" size="xs"
                                on_click=Callback::new(move |_| {
                                    options.update(|list| { list.remove(index); });
                                })>
                                "×"
                            </Button>
                        </Group>
                    }
                }).collect_view()}
            </Group>
            <Group gap="sm">
                <TextInput value=draft placeholder="New option"/>
                <Button variant="default" size="xs" on_click=Callback::new(on_add)>
                    "Add option"
                </Button>
            </Group>
        </Stack>
    }
}

/// `/admin/metadata`.
#[component]
pub fn AdminMetadataPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);

    let definitions = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_definitions(auth)
    });

    // Create form.
    let name = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let field_type = RwSignal::new("string".to_string());
    let enum_options = RwSignal::new(Vec::<String>::new());

    let on_create = move |_| {
        let (n, s, t) = (
            name.get_untracked(),
            slug.get_untracked(),
            field_type.get_untracked(),
        );
        let options = if t == "enum" {
            enum_options.get_untracked()
        } else {
            Vec::new()
        };
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Definition \"{n}\" ({t}) created."),
            async move {
                api::create_definition(auth, &n, &s, &t, &options)
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Metadata definitions" sub="typed fields items can carry (A-0003)"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="Definitions">
                {move || match definitions.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message="No metadata definitions yet — create one below."/>
                    }.into_any(),
                    Some(Ok(list)) => list.into_iter().map(|definition| {
                        view! { <DefinitionRow definition busy outcome reload/> }
                    }).collect_view().into_any(),
                }}
            </Panel>
            <Panel title="Create definition"
                caption="enum definitions need at least one allowed value; string/date take none">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Name" value=name placeholder="e.g. Priority"/>
                        <TextInput label="Slug" value=slug placeholder="e.g. priority"/>
                        <Select label="Type"
                            options=FIELD_TYPES.iter().map(|t| t.to_string()).collect()
                            value=field_type/>
                    </Group>
                    <Show when=move || field_type.get() == "enum">
                        <EnumOptionsEditor options=enum_options/>
                    </Show>
                    <Group>
                        <Button on_click=Callback::new(on_create)>"Create definition"</Button>
                    </Group>
                </Stack>
            </Panel>
        </Stack>
    }
}

/// One definition row with inline edit (name/slug, and the option list for
/// enums) and delete.
#[component]
fn DefinitionRow(
    definition: api::MetadataDefinition,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let definition_id = StoredValue::new(definition.id.clone());
    let is_enum = definition.field_type == "enum";
    let editing = RwSignal::new(false);
    let edit_name = RwSignal::new(definition.name.clone());
    let edit_slug = RwSignal::new(definition.slug.clone());
    let edit_options = RwSignal::new(definition.enum_options.clone());
    let deleted_name = definition.name.clone();

    let on_save = move |_| {
        let id = definition_id.get_value();
        let (n, s) = (edit_name.get_untracked(), edit_slug.get_untracked());
        let options = is_enum.then(|| edit_options.get_untracked());
        editing.set(false);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Definition \"{n}\" updated."),
            async move {
                api::update_definition(auth, &id, &n, &s, options.as_deref())
                    .await
                    .map(|_| ())
            },
        );
    };
    let on_delete = move |_| {
        let id = definition_id.get_value();
        let deleted_name = deleted_name.clone();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Definition \"{deleted_name}\" deleted."),
            async move { api::delete_definition(auth, &id).await.map(|_| ()) },
        );
    };

    view! {
        <Stack gap="xs">
            <Group justify="between" wrap=true>
                <Group gap="sm" wrap=true>
                    <Text bright=true>{definition.name.clone()}</Text>
                    <Code>{definition.slug.clone()}</Code>
                    <Pill color=token::VIOLET>{definition.field_type.clone()}</Pill>
                    {is_enum.then(|| view! {
                        <Group gap="xs" wrap=true>
                            {definition.enum_options.clone().into_iter().map(|option| view! {
                                <Pill color=token::TEAL>{option}</Pill>
                            }).collect_view()}
                        </Group>
                    })}
                    {definition.is_system_default.then(|| view! {
                        <Pill color=token::GOLD>"system default"</Pill>
                    })}
                </Group>
                <Group gap="xs">
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| editing.update(|open| *open = !*open))>
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
                    {is_enum.then(|| view! {
                        <EnumOptionsEditor options=edit_options/>
                    })}
                    <Group>
                        <Button size="xs" on_click=Callback::new(on_save)>"Save"</Button>
                    </Group>
                </Stack>
            </Show>
            <Divider/>
        </Stack>
    }
}
