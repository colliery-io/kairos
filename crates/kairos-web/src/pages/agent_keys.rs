//! `/agent-keys` (KAIROS-T-0359, KAIROS-A-0024): the agent keys of the
//! signed-in person. Make a key (the secret shows once), list the keys, and
//! revoke one. An agent key acts as the person, with the capabilities of the
//! person.
//!
//! The data layer is here: three calls on `/api/me/agent-keys`, through the
//! `crate::api` helpers (envelope-aware errors, the 401 hook).

use aurora_dark::components::{
    Alert, Button, Code, CopyButton, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel,
    Pill, Stack, Text, TextInput,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;

use super::admin::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types_service_accounts::ApiKey`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AgentKey {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// mirror of: `kairos_client::types_service_accounts::ApiKeyList`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct AgentKeyList {
    items: Vec<AgentKey>,
}

/// mirror of: `kairos_client::types_service_accounts::ApiKeyCreated` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct AgentKeyCreated {
    name: String,
    key: String,
}

const PATH: &str = "/api/me/agent-keys";

async fn list_agent_keys(auth: Auth) -> Result<Vec<AgentKey>, ApiError> {
    crate::api::get_json::<AgentKeyList>(auth, PATH)
        .await
        .map(|list| list.items)
}

async fn create_agent_key(
    auth: Auth,
    name: &str,
    expires_at: Option<&str>,
) -> Result<AgentKeyCreated, ApiError> {
    let body = match expires_at {
        Some(expires_at) => json!({ "name": name, "expires_at": expires_at }),
        None => json!({ "name": name }),
    };
    crate::api::post_json(auth, PATH, &body).await
}

async fn revoke_agent_key(auth: Auth, key_id: &str) -> Result<serde_json::Value, ApiError> {
    crate::api::delete_json(auth, &format!("{PATH}/{key_id}")).await
}

/// The date part of an RFC 3339 timestamp, or "never".
fn day(value: Option<&str>) -> String {
    value
        .map(|ts| ts.split('T').next().unwrap_or(ts).to_string())
        .unwrap_or_else(|| "never".to_string())
}

/// `/agent-keys`.
#[component]
pub fn AgentKeysPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    // The secret of the key made last. It shows once, on this page only.
    let made: RwSignal<Option<(String, String)>> = RwSignal::new(None);

    let keys = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        list_agent_keys(auth)
    });

    let name = RwSignal::new(String::new());
    let expires_at = RwSignal::new(String::new());

    let on_create = move |_| {
        if busy.get_untracked() {
            return;
        }
        let key_name = name.get_untracked().trim().to_string();
        let expiry = expires_at.get_untracked().trim().to_string();
        busy.set(true);
        made.set(None);
        leptos::task::spawn_local(async move {
            let expiry = (!expiry.is_empty()).then_some(expiry);
            let result = create_agent_key(auth, &key_name, expiry.as_deref()).await;
            busy.set(false);
            match result {
                Ok(created) => {
                    made.set(Some((created.name, created.key)));
                    outcome.set(None);
                    name.set(String::new());
                    expires_at.set(String::new());
                    reload.update(|n| *n += 1);
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };

    view! {
        <PageHeader title="Agent keys" sub="An agent key lets an agent act as you. The agent \
                                            gets your capabilities, and the history shows \
                                            your name."/>
        <Stack gap="md">
            <MutationNotice outcome/>
            {move || made.get().map(|(key_name, key)| {
                let copied = key.clone();
                view! {
                    <Alert title=format!("The key {key_name}") color=token::OK>
                        <Stack gap="xs">
                            <Text size="sm">
                                "Copy the key now. Kairos does not show it again."
                            </Text>
                            <Group gap="sm" wrap=true>
                                <Code>{key}</Code>
                                <CopyButton value=copied label="Copy key"/>
                            </Group>
                            <Text size="sm" dimmed=true>
                                "Put the key in the settings of your agent. For Claude Code, \
                                 set the environment variable KAIROS_MCP_KEY. Do not put the \
                                 key in a repository."
                            </Text>
                            <Group justify="end">
                                <Button variant="subtle" size="xs"
                                    on_click=Callback::new(move |_| made.set(None))>
                                    "Hide the key"
                                </Button>
                            </Group>
                        </Stack>
                    </Alert>
                }
            })}
            <Panel title="Make an agent key"
                caption="Give each agent its own key. Then you can revoke one key and keep \
                         the others.">
                <Group align="end" gap="sm" wrap=true>
                    <TextInput label="Name" value=name placeholder="laptop-claude-code"/>
                    <TextInput label="Expires at (optional)" value=expires_at
                        placeholder="2027-01-01T00:00:00Z"/>
                    <Button on_click=Callback::new(on_create)>"Make key"</Button>
                </Group>
            </Panel>
            <Panel title="Your agent keys"
                caption="Kairos keeps the start of each key only. A revoked key stays in the \
                         list.">
                {move || match keys.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message="You have no agent keys."/>
                    }.into_any(),
                    Some(Ok(list)) => list.into_iter().map(|key| {
                        let key_id = StoredValue::new(key.id.clone());
                        let revoked = key.revoked_at.is_some();
                        let revoked_name = key.name.clone();
                        let on_revoke = move |_| {
                            let key_id = key_id.get_value();
                            let revoked_name = revoked_name.clone();
                            run_mutation(
                                busy, outcome, reload,
                                format!("Kairos revoked the key {revoked_name}."),
                                async move { revoke_agent_key(auth, &key_id).await.map(|_| ()) },
                            );
                        };
                        view! {
                            <Stack gap="xs">
                                <Group justify="between" wrap=true>
                                    <Group gap="sm" wrap=true>
                                        <Text bright=true>{key.name.clone()}</Text>
                                        <Code>{key.prefix.clone()}</Code>
                                        {if revoked {
                                            view! { <Pill color=token::BAD>"revoked"</Pill> }.into_any()
                                        } else {
                                            view! { <Pill color=token::OK>"active"</Pill> }.into_any()
                                        }}
                                    </Group>
                                    {(!revoked).then(|| view! {
                                        <Button variant="default" size="xs" bad=true
                                            on_click=Callback::new(on_revoke)>
                                            "Revoke"
                                        </Button>
                                    })}
                                </Group>
                                <Text dimmed=true size="sm">
                                    {format!(
                                        "Made {}. Expires {}. Last used {}.",
                                        day(Some(&key.created_at)),
                                        day(key.expires_at.as_deref()),
                                        day(key.last_used_at.as_deref()),
                                    )}
                                </Text>
                                <Divider/>
                            </Stack>
                        }
                    }).collect_view().into_any(),
                }}
            </Panel>
        </Stack>
    }
}

#[cfg(test)]
mod tests {
    use super::day;

    #[test]
    fn day_shows_the_date_or_never() {
        assert_eq!(day(Some("2027-01-01T00:00:00.000000Z")), "2027-01-01");
        assert_eq!(day(None), "never");
    }
}
