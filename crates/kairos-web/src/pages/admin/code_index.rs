//! `/admin/code-index` — where the code index summaries and vectors of the
//! organization are made (KAIROS-T-0339, COLLIERY-I-0611): the embedded
//! model, Ollama Cloud (an OpenAI-compatible endpoint) or Bedrock for the
//! summaries; the embedded model or a remote endpoint for the vectors.
//!
//! A secret field that is empty keeps the stored secret, as the read token
//! form does; the button "Remove" sends an empty secret, which removes it.
//! The page never shows a secret: the server gives a status (set by whom,
//! when).

use aurora_dark::components::{
    Button, ErrorState, Group, Loading, PageHeader, Panel, Select, Stack, Text, TextInput,
};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::api::{get_json, put_json};
use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types_code_index::SecretStatus`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct SecretStatus {
    pub set: bool,
    #[serde(default)]
    pub set_by: Option<String>,
    #[serde(default)]
    pub set_at: Option<String>,
}

/// mirror of: `kairos_client::types_code_index::SummaryProviderSettings`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SummaryProvider {
    pub provider: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub secret: SecretStatus,
}

/// mirror of: `kairos_client::types_code_index::VectorProviderSettings`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct VectorProvider {
    pub provider: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub secret: SecretStatus,
}

/// mirror of: `kairos_client::types_code_index::CodeIndexSettings`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CodeIndexSettings {
    pub summary: SummaryProvider,
    pub vectors: VectorProvider,
    pub concurrency: i32,
    #[serde(default)]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// mirror of: `kairos_client::types_code_index::PutCodeIndexSettings`.
#[derive(Debug, Serialize)]
struct PutSettings {
    summary: PutSummary,
    vectors: PutVectors,
    concurrency: i32,
}

#[derive(Debug, Serialize)]
struct PutSummary {
    provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

#[derive(Debug, Serialize)]
struct PutVectors {
    provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

async fn load_settings(auth: Auth) -> Result<CodeIndexSettings, aurora_dark::tokens::ApiError> {
    get_json(auth, "/api/org/code-index-settings").await
}

/// The text of a secret status: who set it and when, or "none". Pure,
/// host-tested.
pub(crate) fn secret_text(status: &SecretStatus) -> String {
    match (&status.set_by, &status.set_at) {
        (Some(by), Some(at)) => format!("Secret: set by {by} at {at}."),
        _ if status.set => "Secret: set.".to_string(),
        _ => "Secret: none.".to_string(),
    }
}

/// An empty field is `None`: the server then takes the field as not given.
/// Pure, host-tested.
pub(crate) fn given(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

const SUMMARY_HINT: &str = "ollama-cloud needs the base URL (for example https://ollama.com/v1), \
                            the model (for example gemma4:31b) and the API key. bedrock needs \
                            the region, the model id and the credentials as <access key \
                            id>:<secret access key>. An empty secret field keeps the stored \
                            secret.";
const VECTOR_HINT: &str = "remote needs the base URL of an OpenAI-compatible embeddings endpoint \
                           and the model. A local Ollama needs no secret. A change of the vector \
                           model gives the code index new vectors over time.";
const OPT_IN_HINT: &str = "A repository uses the hosted provider only after a person opts it \
                           in, on the page Repositories. Until then its summaries come from the \
                           embedded model.";

/// `/admin/code-index`.
#[component]
pub fn AdminCodeIndexPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let settings = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        load_settings(auth)
    });

    view! {
        <PageHeader title="Code index" sub="Where the summaries and the vectors of the code \
                                            index of this organization are made."/>
        <Stack gap="md">
            <MutationNotice outcome/>
            {move || match settings.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(current)) => view! {
                    <SettingsForm current busy outcome reload/>
                }.into_any(),
            }}
        </Stack>
    }
}

#[component]
fn SettingsForm(
    current: CodeIndexSettings,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let summary_provider = RwSignal::new(current.summary.provider.clone());
    let summary_url = RwSignal::new(current.summary.base_url.clone().unwrap_or_default());
    let summary_model = RwSignal::new(current.summary.model.clone().unwrap_or_default());
    let summary_region = RwSignal::new(current.summary.region.clone().unwrap_or_default());
    let summary_secret = RwSignal::new(String::new());
    let vector_provider = RwSignal::new(current.vectors.provider.clone());
    let vector_url = RwSignal::new(current.vectors.base_url.clone().unwrap_or_default());
    let vector_model = RwSignal::new(current.vectors.model.clone().unwrap_or_default());
    let vector_secret = RwSignal::new(String::new());
    let concurrency = RwSignal::new(current.concurrency.to_string());
    let summary_secret_text = secret_text(&current.summary.secret);
    let vector_secret_text = secret_text(&current.vectors.secret);
    let summary_secret_set = current.summary.secret.set;
    let vector_secret_set = current.vectors.secret.set;
    let state_line = match (&current.updated_by, &current.updated_at) {
        (Some(by), Some(at)) => format!("Set by {by} at {at}."),
        _ => "Not set: the organization uses the embedded model for both.".to_string(),
    };

    // `remove`: which secret the save removes (an empty secret in the body).
    let save = move |remove_summary: bool, remove_vectors: bool| {
        let body = PutSettings {
            summary: PutSummary {
                provider: summary_provider.get_untracked(),
                base_url: given(&summary_url.get_untracked()),
                model: given(&summary_model.get_untracked()),
                region: given(&summary_region.get_untracked()),
                secret: if remove_summary {
                    Some(String::new())
                } else {
                    given(&summary_secret.get_untracked())
                },
            },
            vectors: PutVectors {
                provider: vector_provider.get_untracked(),
                base_url: given(&vector_url.get_untracked()),
                model: given(&vector_model.get_untracked()),
                secret: if remove_vectors {
                    Some(String::new())
                } else {
                    given(&vector_secret.get_untracked())
                },
            },
            concurrency: concurrency
                .get_untracked()
                .trim()
                .parse()
                .unwrap_or(current.concurrency),
        };
        run_mutation(
            busy,
            outcome,
            reload,
            "Kairos set the providers of the code index.".to_string(),
            async move {
                let _: serde_json::Value =
                    put_json(auth, "/api/org/code-index-settings", &body).await?;
                summary_secret.set(String::new());
                vector_secret.set(String::new());
                Ok(())
            },
        );
    };

    view! {
        <Panel title="Summaries" caption="the model that writes the summaries of the code">
            <Stack gap="sm" attr:data-testid="code-index-summary-settings">
                <Group gap="sm" wrap=true top=true>
                    <Select label="Provider" value=summary_provider
                        options=vec!["embedded".to_string(), "ollama-cloud".to_string(), "bedrock".to_string()]/>
                    <TextInput label="Base URL" value=summary_url placeholder="https://ollama.com/v1"/>
                    <TextInput label="Model" value=summary_model placeholder="gemma4:31b"/>
                    <TextInput label="Region (bedrock)" value=summary_region placeholder="us-east-1"/>
                </Group>
                <Group gap="sm" wrap=true align="end">
                    <TextInput label="Secret" value=summary_secret input_type="password"
                        autocomplete="new-password" spellcheck=false name="summary-secret"/>
                    {summary_secret_set.then(|| view! {
                        <Button variant="default" size="xs" bad=true
                            on_click=Callback::new(move |_| save(true, false))>
                            "Remove the secret"
                        </Button>
                    })}
                </Group>
                <Text dimmed=true size="xs" attr:data-testid="summary-secret-status">{summary_secret_text}</Text>
                <Text dimmed=true size="xs">{SUMMARY_HINT}</Text>
                <Text dimmed=true size="xs">{OPT_IN_HINT}</Text>
            </Stack>
        </Panel>
        <Panel title="Vectors" caption="the model that makes the vectors of the summaries">
            <Stack gap="sm" attr:data-testid="code-index-vector-settings">
                <Group gap="sm" wrap=true top=true>
                    <Select label="Provider" value=vector_provider
                        options=vec!["embedded".to_string(), "remote".to_string()]/>
                    <TextInput label="Base URL" value=vector_url placeholder="http://ollama:11434/v1"/>
                    <TextInput label="Model" value=vector_model placeholder="nomic-embed-text"/>
                </Group>
                <Group gap="sm" wrap=true align="end">
                    <TextInput label="Secret" value=vector_secret input_type="password"
                        autocomplete="new-password" spellcheck=false name="vector-secret"/>
                    {vector_secret_set.then(|| view! {
                        <Button variant="default" size="xs" bad=true
                            on_click=Callback::new(move |_| save(false, true))>
                            "Remove the secret"
                        </Button>
                    })}
                </Group>
                <Text dimmed=true size="xs" attr:data-testid="vector-secret-status">{vector_secret_text}</Text>
                <Text dimmed=true size="xs">{VECTOR_HINT}</Text>
            </Stack>
        </Panel>
        <Panel title="Builder" caption="how the builder uses a hosted provider">
            <Stack gap="sm">
                <Group gap="sm" wrap=true align="end">
                    <TextInput label="Requests at a time (1 to 32)" value=concurrency/>
                    <Button on_click=Callback::new(move |_| save(false, false))
                        attr:data-testid="code-index-settings-save">
                        "Save"
                    </Button>
                </Group>
                <Text dimmed=true size="xs" attr:data-testid="code-index-settings-state">{state_line}</Text>
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_status_reads_as_a_line() {
        assert_eq!(secret_text(&SecretStatus::default()), "Secret: none.");
        assert_eq!(
            secret_text(&SecretStatus {
                set: true,
                set_by: Some("u1".into()),
                set_at: Some("2026-10-07T08:00:00+00:00".into()),
            }),
            "Secret: set by u1 at 2026-10-07T08:00:00+00:00."
        );
    }

    #[test]
    fn an_empty_field_is_not_given() {
        assert_eq!(given("  "), None);
        assert_eq!(given(" gemma4:31b "), Some("gemma4:31b".to_string()));
    }

    #[test]
    fn the_settings_decode() {
        let settings: CodeIndexSettings = serde_json::from_str(
            r#"{"summary":{"provider":"ollama-cloud","base_url":"https://ollama.com/v1",
                "model":"gemma4:31b","secret":{"set":true,"set_by":"u1","set_at":"t"}},
               "vectors":{"provider":"embedded","secret":{"set":false}},
               "concurrency":4,"updated_by":"u1","updated_at":"t"}"#,
        )
        .expect("decodes");
        assert_eq!(settings.summary.model.as_deref(), Some("gemma4:31b"));
        assert!(settings.summary.secret.set);
        assert!(!settings.vectors.secret.set);
    }
}
