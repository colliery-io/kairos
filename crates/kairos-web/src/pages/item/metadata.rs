//! The typed metadata panel (KAIROS-T-0041, per KAIROS-A-0003): every
//! tenant metadata definition renders with an editor matched to its type —
//! enum → dropdown of the definition's options, date → a date input,
//! string → text — with the item's current values filled in. Clearing a
//! field sends `null` (the A-0003 "null clears" contract); saving PATCHes
//! only the fields that changed. Metadata is not versioned (A-0004):
//! last-write-wins, no conflict UI here.

use std::collections::BTreeMap;

use aurora_dark::components::{Alert, Empty, ErrorState, Group, Loading, Panel, Pill, Text};
use aurora_dark::tokens::token;
use leptos::prelude::*;

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
            let definitions = api::fetch_definitions(auth).await?;
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

/// The editors + save button, built fresh per fetch (drafts start at the
/// server's values).
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

    let rows: Vec<FieldRow> = definitions
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
        .collect();
    let rows = StoredValue::new(rows);

    let dirty =
        move || rows.with_value(|rows| rows.iter().any(|row| row.draft.get() != row.original));

    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        // Changed fields only; "" means clear (A-0003 null-clears).
        let changed: BTreeMap<String, Option<String>> = rows.with_value(|rows| {
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
            {rows.with_value(|rows| {
                rows.iter().cloned().map(|row| view! { <FieldEditor row/> }).collect_view()
            })}
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
