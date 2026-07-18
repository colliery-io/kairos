//! `/admin/members` — organization membership (KAIROS-T-0043): list,
//! add-by-email, role changes, removal. The server guards the org's last
//! admin (422 `LAST_ADMIN`) and unknown emails (users are JIT-provisioned
//! at first login, so adding one that never logged in is a 404 with that
//! guidance) — both render through [`super::MutationNotice`].

use aurora_dark::components::{
    Button, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select, Stack,
    Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

/// `/admin/members`.
#[component]
pub fn AdminMembersPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);

    let members = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_org_members(auth)
    });

    let add_email = RwSignal::new(String::new());
    let add_role = RwSignal::new("member".to_string());

    let on_add = move |_| {
        let email = add_email.get_untracked();
        let role = add_role.get_untracked();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("{email} added as {role}."),
            async move { api::add_org_member(auth, &email, &role).await.map(|_| ()) },
        );
    };

    view! {
        <PageHeader title="Organization members" sub="who belongs here, and who administers it"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="Members"
                caption="an organization must always retain at least one admin (LAST_ADMIN guard)">
                {move || match members.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message="No members — which should be impossible while you can see this."/>
                    }.into_any(),
                    Some(Ok(list)) => list.into_iter().map(|member| {
                        let user_id = StoredValue::new(member.user_id);
                        let is_admin = member.role == "admin";
                        let role_color = if is_admin { token::GOLD } else { token::ICE };
                        let email = member.email.clone();
                        let toggled_email = email.clone();
                        let removed_email = email.clone();
                        let on_toggle_role = move |_| {
                            let user_id = user_id.get_value();
                            let new_role = if is_admin { "member" } else { "admin" };
                            let toggled_email = toggled_email.clone();
                            run_mutation(
                                busy, outcome, reload,
                                format!("{toggled_email} is now a {new_role}."),
                                async move {
                                    api::set_org_member_role(auth, &user_id, new_role)
                                        .await
                                        .map(|_| ())
                                },
                            );
                        };
                        let on_remove = move |_| {
                            let user_id = user_id.get_value();
                            let removed_email = removed_email.clone();
                            run_mutation(
                                busy, outcome, reload,
                                format!("{removed_email} removed from the organization."),
                                async move {
                                    api::remove_org_member(auth, &user_id).await.map(|_| ())
                                },
                            );
                        };
                        view! {
                            <Stack gap="xs">
                                <Group justify="between" wrap=true>
                                    <Group gap="sm">
                                        <Text bright=true>{member.display_name.clone()}</Text>
                                        <Text dimmed=true size="sm">{email.clone()}</Text>
                                        <Pill color=role_color>
                                            {member.role.clone()}
                                        </Pill>
                                    </Group>
                                    <Group gap="xs">
                                        <Button variant="default" size="xs"
                                            on_click=Callback::new(on_toggle_role)>
                                            {if is_admin { "Make member" } else { "Make admin" }}
                                        </Button>
                                        <Button variant="default" size="xs" bad=true
                                            on_click=Callback::new(on_remove)>
                                            "Remove"
                                        </Button>
                                    </Group>
                                </Group>
                                <Divider/>
                            </Stack>
                        }
                    }).collect_view().into_any(),
                }}
            </Panel>
            <Panel title="Add member by email"
                caption="users are provisioned at first login — an email that has never signed in cannot be added yet">
                <Group gap="sm" wrap=true top=true>
                    <TextInput label="Email" value=add_email placeholder="someone@example.com"/>
                    <Select label="Role"
                        options=vec!["member".to_string(), "admin".to_string()]
                        value=add_role/>
                    <Button on_click=Callback::new(on_add)>"Add member"</Button>
                </Group>
            </Panel>
        </Stack>
    }
}
