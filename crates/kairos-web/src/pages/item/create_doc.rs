//! Create-from-template (KAIROS-T-0041, per KAIROS-A-0003): from a
//! workflow item, create a supporting document stamped from a template.
//! The picker previews the selected template's starter content (rendered
//! markdown) and lists its declared metadata fields (type, defaults,
//! required) — exactly what `POST /api/documents` will stamp. The new
//! document attaches to this item via the `supports` edge (the T-0018
//! parent contract) and the page navigates to its detail route.

use aurora_dark::components::{
    Alert, Button, Empty, ErrorState, Group, Loading, Panel, Pill, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use super::api::{self, CreateDocumentBody, TemplateSummary};
use super::markdown;
use crate::auth::use_auth;

/// The "New document" dialog. `open` is owned by the page header button.
#[component]
pub fn CreateDocumentDialog(
    /// Short code of the workflow item that will parent the document.
    #[prop(into)]
    parent_code: String,
    open: RwSignal<bool>,
) -> impl IntoView {
    let parent_code = StoredValue::new(parent_code);

    view! {
        {move || open.get().then(|| view! {
            <div class="kairos-dialog__backdrop" on:click=move |_| open.set(false)></div>
            <div class="kairos-dialog" role="dialog" aria-modal="true">
                <div class="kairos-dialog__box">
                    <Group justify="between">
                        <Text bright=true bold=true>
                            {format!("New document supporting {}", parent_code.get_value())}
                        </Text>
                        <Button variant="default" size="xs" on_click=Callback::new(move |_| open.set(false))>
                            "Close"
                        </Button>
                    </Group>
                    <TemplatePicker parent_code=parent_code.get_value()/>
                </div>
            </div>
        })}
    }
}

/// Template list + preview + create form (own component so its resources
/// only exist while the dialog is open).
#[component]
fn TemplatePicker(#[prop(into)] parent_code: String) -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let templates = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_templates(auth)
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    let parent_code = StoredValue::new(parent_code);

    view! {
        {move || match templates.get() {
            None => view! { <Loading label="Loading templates…"/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
            Some(Ok(templates)) if templates.is_empty() => view! {
                <Empty message="No templates yet — create them from Admin."/>
            }.into_any(),
            Some(Ok(templates)) => view! {
                <TemplateForm templates parent_code=parent_code.get_value()/>
            }.into_any(),
        }}
    }
}

/// The picker itself: select a template, preview it, name the document,
/// create.
#[component]
fn TemplateForm(
    templates: Vec<TemplateSummary>,
    #[prop(into)] parent_code: String,
) -> impl IntoView {
    let auth = use_auth();
    let parent_code = StoredValue::new(parent_code);
    let title = RwSignal::new(String::new());
    let selected_id = RwSignal::new(templates[0].id.clone());
    let creating = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    // The declared-fields + content preview for the selected template.
    let detail = LocalResource::new(move || {
        let _ = auth.token();
        api::fetch_template_detail(auth, selected_id.get())
    });

    let options_view = templates
        .iter()
        .map(|template| {
            let id = template.id.clone();
            let id_for_selected = template.id.clone();
            view! {
                <option value=id selected=move || selected_id.get() == id_for_selected>
                    {template.name.clone()}
                </option>
            }
        })
        .collect_view();

    let navigate = use_navigate();
    let create = move |_| {
        if creating.get_untracked() {
            return;
        }
        let document_title = title.get_untracked().trim().to_string();
        if document_title.is_empty() {
            error.set(Some("Give the document a title.".to_string()));
            return;
        }
        creating.set(true);
        error.set(None);
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let body = CreateDocumentBody {
                title: document_title,
                template_id: selected_id.get_untracked(),
                parent_short_code: parent_code.get_value(),
            };
            match api::create_document(auth, &body).await {
                Ok(created) => navigate(
                    &format!("/items/{}", created.short_code),
                    Default::default(),
                ),
                Err(e) => {
                    creating.set(false);
                    error.set(Some(api::error_text(&e)));
                }
            }
        });
    };

    view! {
        <div class="kairos-template-form">
            {move || error.get().map(|message| view! {
                <Alert title="Could not create the document" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
            <Group gap="sm" top=true>
                <div class="kairos-template-form__controls">
                    <label class="cl-field__label">"Template"</label>
                    <select
                        class="cl-input cl-select"
                        prop:value=move || selected_id.get()
                        on:change=move |e| selected_id.set(event_target_value(&e))
                    >
                        {options_view}
                    </select>
                    <TextInput label="Document title" placeholder="e.g. PRD: Portal sign-up flow" value=title/>
                    <button
                        class="cl-btn cl-btn--filled"
                        disabled=move || creating.get()
                        on:click=create
                    >
                        {move || if creating.get() { "Creating…" } else { "Create document" }}
                    </button>
                    <Text size="xs" dimmed=true>
                        "Starter content is copied in; declared fields are stamped as metadata (defaults applied) — A-0003."
                    </Text>
                </div>
                <div class="kairos-template-form__preview">
                    {move || match detail.get() {
                        None => view! { <Loading label="Loading template…"/> }.into_any(),
                        Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                        Some(Ok(template)) => {
                            let content_html = markdown::to_html(&template.content);
                            let fields = template.metadata;
                            view! {
                                <Panel title="Declared fields" caption=template.name>
                                    {if fields.is_empty() {
                                        view! { <Empty message="This template declares no metadata fields."/> }.into_any()
                                    } else {
                                        fields.into_iter().map(|field| view! {
                                            <Group gap="sm" justify="between">
                                                <Text size="sm">{field.name}</Text>
                                                <Group gap="sm">
                                                    {field.default_value.map(|default| view! {
                                                        <Text size="xs" dimmed=true>{format!("default: {default}")}</Text>
                                                    })}
                                                    {field.required.then(|| view! {
                                                        <Pill color=token::GOLD>"required"</Pill>
                                                    })}
                                                    <Pill color=token::VIOLET>{field.field_type}</Pill>
                                                </Group>
                                            </Group>
                                        }).collect_view().into_any()
                                    }}
                                </Panel>
                                <Panel title="Content preview" caption="starter markdown">
                                    <div
                                        class="kairos-markdown kairos-markdown--preview"
                                        inner_html=content_html
                                    ></div>
                                </Panel>
                            }.into_any()
                        }
                    }}
                </div>
            </Group>
        </div>
    }
}
