//! The typed metadata panel (KAIROS-T-0041, per KAIROS-A-0003; scoping per
//! KAIROS-T-0065): the panel shows editors only for fields the item
//! ACTUALLY CARRIES — values stamped at creation from its template's
//! declared fields, or added here explicitly. The full definition catalog
//! never renders wholesale (that put a "Document Type" editor on every
//! task); unstamped definitions sit behind an "add a field" picker
//! instead. Editors match the field type — enum → dropdown of the
//! definition's options, date → a date input, string → text. Clearing a
//! field sends `null` (the A-0003 "null clears" contract); saving PATCHes
//! only the fields that changed. Metadata is not versioned (A-0004):
//! last-write-wins, no conflict UI here.

use std::collections::BTreeMap;

use aurora_dark::components::{Alert, Empty, ErrorState, Group, Loading, Panel, Pill, Text};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use wasm_bindgen::JsCast;

use super::api::{self, Family, MetadataDefinition, MetadataValue};
use crate::auth::use_auth;

/// One field's editing state: its definition, the live draft, and the
/// value the server had when the panel loaded.
#[derive(Clone)]
struct FieldRow {
    definition: MetadataDefinition,
    draft: RwSignal<String>,
    original: String,
}

/// The metadata panel: definitions + values fetched together, typed
/// editors, one save for all changed fields.
#[component]
pub fn MetadataPanel(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);

    let data = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        async move {
            let definitions = api::fetch_definitions(auth, family).await?;
            let values = api::fetch_metadata(auth, family, code.get_value()).await?;
            Ok::<_, aurora_dark::tokens::ApiError>((definitions, values))
        }
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    view! {
        <Panel title="Metadata" caption="typed fields (A-0003)">
            {move || match data.get() {
                None => view! { <Loading label="Loading metadata…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok((definitions, _))) if definitions.is_empty() => view! {
                    <Empty message="No metadata definitions yet — add them from Admin."/>
                }.into_any(),
                Some(Ok((definitions, values))) => view! {
                    <MetadataForm
                        family
                        code=code.get_value()
                        definitions
                        values
                        on_saved=Callback::new(move |_| reload.update(|n| *n += 1))
                    />
                }.into_any(),
            }}
        </Panel>
    }
}

/// The "add a field" picker's no-choice option.
const ADD_PLACEHOLDER: &str = "(add a field…)";

/// The editors + save button, built fresh per fetch (drafts start at the
/// server's values). Only STAMPED fields render editors (KAIROS-T-0065);
/// the rest of the catalog waits behind the add-a-field picker.
#[component]
fn MetadataForm(
    family: Family,
    #[prop(into)] code: String,
    definitions: Vec<MetadataDefinition>,
    values: Vec<MetadataValue>,
    on_saved: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let saving = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    // EVERY definition gets its row (and its draft signal) up front, at
    // render time; `visible` decides which rows show. The picker only
    // pushes a slug into `visible` — it never creates signals. Two prior
    // shapes of this code broke on the add path (T-0078 review): an
    // Effect resetting the signal it watched re-entered itself and
    // panicked the reactive runtime, and creating the draft signal inside
    // the change handler (no reactive owner) produced a dead editor whose
    // writes never propagated.
    let all_rows: StoredValue<Vec<FieldRow>> = StoredValue::new(
        definitions
            .into_iter()
            .map(|definition| {
                let original = values
                    .iter()
                    .find(|value| value.slug == definition.slug)
                    .map(|value| value.value.clone())
                    .unwrap_or_default();
                FieldRow {
                    definition,
                    draft: RwSignal::new(original.clone()),
                    original,
                }
            })
            .collect(),
    );
    let visible: RwSignal<Vec<String>> =
        RwSignal::new(values.iter().map(|value| value.slug.clone()).collect());

    // Choosing a definition in the picker reveals its (empty) editor row;
    // the value only reaches the server on save.
    let on_pick = move |e: web_sys::Event| {
        let choice = event_target_value(&e);
        if choice == ADD_PLACEHOLDER {
            return;
        }
        let slug = all_rows.with_value(|rows| {
            rows.iter()
                .find(|row| row.definition.name == choice)
                .map(|row| row.definition.slug.clone())
        });
        if let Some(slug) = slug {
            visible.update(|shown| {
                if !shown.contains(&slug) {
                    shown.push(slug);
                }
            });
        }
        // Reset the picker to its placeholder so the next add is a fresh
        // change event (the node survives the option-list re-render).
        if let Some(select) = e
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlSelectElement>().ok())
        {
            select.set_value(ADD_PLACEHOLDER);
        }
    };

    // Hidden rows are never dirty (draft == original == server value).
    let dirty = move || {
        all_rows.with_value(|rows| rows.iter().any(|row| row.draft.get() != row.original))
    };

    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        // Changed fields only; "" means clear (A-0003 null-clears).
        let changed: BTreeMap<String, Option<String>> = all_rows.with_value(|rows| {
            rows.iter()
                .filter(|row| row.draft.get_untracked() != row.original)
                .map(|row| {
                    let draft = row.draft.get_untracked();
                    let value = (!draft.is_empty()).then_some(draft);
                    (row.definition.slug.clone(), value)
                })
                .collect()
        });
        if changed.is_empty() {
            return;
        }
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::update_metadata(auth, family, &code.get_value(), changed).await;
            saving.set(false);
            match result {
                Ok(_) => on_saved.run(()),
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    };

    view! {
        <div class="kairos-metadata">
            {move || error.get().map(|message| view! {
                <Alert title="Metadata save failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
            {move || {
                // Render in VISIBLE order (stamped first, then picks in
                // the order made), so a freshly added row lands at the
                // bottom, right above the picker — in catalog order an
                // add could insert above existing rows, out of view of
                // the picker the user just used.
                let shown = visible.get();
                let current: Vec<FieldRow> = all_rows.with_value(|rows| {
                    shown
                        .iter()
                        .filter_map(|slug| {
                            rows.iter().find(|row| &row.definition.slug == slug)
                        })
                        .cloned()
                        .collect()
                });
                if current.is_empty() {
                    view! {
                        <Text size="sm" dimmed=true>
                            "No metadata on this item — its template declared none. Add a field below if one applies."
                        </Text>
                    }.into_any()
                } else {
                    current
                        .into_iter()
                        .map(|row| view! { <FieldEditor row/> })
                        .collect_view()
                        .into_any()
                }
            }}
            {move || {
                let shown = visible.get();
                let available: Vec<String> = all_rows.with_value(|rows| {
                    rows.iter()
                        .filter(|row| !shown.contains(&row.definition.slug))
                        .map(|row| row.definition.name.clone())
                        .collect()
                });
                (!available.is_empty()).then(|| {
                    let mut options = vec![ADD_PLACEHOLDER.to_string()];
                    options.extend(available);
                    view! {
                        <div class="cl-field">
                            <select class="cl-input cl-select" on:change=on_pick>
                                {options.into_iter().map(|option| {
                                    let label = option.clone();
                                    view! { <option value=option>{label}</option> }
                                }).collect_view()}
                            </select>
                        </div>
                    }
                })
            }}
            <Group justify="between">
                <Text size="xs" dimmed=true>"Blank clears a field. Last write wins (not versioned, A-0004)."</Text>
                <button
                    class="cl-btn cl-btn--filled cl-btn--xs"
                    disabled=move || saving.get() || !dirty()
                    on:click=save
                >
                    {move || if saving.get() { "Saving…" } else { "Save metadata" }}
                </button>
            </Group>
        </div>
    }
}

/// One typed editor row: label + the editor its `field_type` calls for.
#[component]
fn FieldEditor(row: FieldRow) -> impl IntoView {
    let FieldRow {
        definition, draft, ..
    } = row;
    let type_pill = match definition.field_type.as_str() {
        "enum" => ("enum", token::VIOLET),
        "date" => ("date", token::TEAL),
        _ => ("string", token::ICE),
    };
    let editor = match definition.field_type.as_str() {
        "enum" => {
            // "" is the explicit unset choice; the rest come from the
            // definition, in its display order.
            let options: Vec<String> = std::iter::once(String::new())
                .chain(definition.enum_options.iter().cloned())
                .collect();
            let options_view = options
                .into_iter()
                .map(|option| {
                    let label = if option.is_empty() { "—".to_string() } else { option.clone() };
                    view! { <option value=option.clone() selected=move || draft.get() == option>{label}</option> }
                })
                .collect_view();
            view! {
                <select
                    class="cl-input cl-select"
                    prop:value=move || draft.get()
                    on:change=move |e| draft.set(event_target_value(&e))
                >
                    {options_view}
                </select>
            }
            .into_any()
        }
        "date" => view! {
            <input
                class="cl-input"
                type="date"
                prop:value=move || draft.get()
                on:input=move |e| draft.set(event_target_value(&e))
            />
        }
        .into_any(),
        _ => view! {
            <input
                class="cl-input"
                type="text"
                placeholder="—"
                prop:value=move || draft.get()
                on:input=move |e| draft.set(event_target_value(&e))
            />
        }
        .into_any(),
    };
    view! {
        <div class="kairos-metadata__field">
            <Group justify="between">
                <label class="cl-field__label">{definition.name.clone()}</label>
                <Pill color=type_pill.1>{type_pill.0}</Pill>
            </Group>
            {editor}
        </div>
    }
}
