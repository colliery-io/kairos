//! The writes of the Relationships panel (KAIROS-T-0580): add an edge,
//! and set or remove the parent of an initiative or a task.
//!
//! Who may write: the link rule of the server (COLLIERY-T-0228). A person
//! may add or remove an edge when they may edit one end of it. The page
//! knows the edit rule for this item only; the other end is not known
//! until the server reads it. So the panel shows the controls to a person
//! who may edit this item, or who may edit some item ([`may_link`]). The
//! server stays the authority: a refusal shows in the panel.
//!
//! The parent goes through the reparent of the server (KAIROS-T-0572):
//! one write replaces the old parent edge with the new one. An external
//! reference (a URI) has its own panel on the page.

use aurora_dark::components::{Alert, Button, Group, Select, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;

use super::api::{self, Family};
use super::code_picker::CodePicker;
use crate::auth::use_auth;
use crate::pages::search::data;

const WORKFLOW: &[&str] = &["strategy", "initiative", "task"];
const WRITINGS: &[&str] = &["document", "adr"];
const WRITINGS_AND_TASKS: &[&str] = &["document", "adr", "task"];
const STRATEGY: &[&str] = &["strategy"];
const INITIATIVE: &[&str] = &["initiative"];
const TASK: &[&str] = &["task"];
const ADR: &[&str] = &["adr"];

/// One choice of the Relationship select: the edge from the side of this
/// item, and the types of the item at the other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Choice {
    /// The relationship of the server.
    pub kind: &'static str,
    /// `true`: this item is the source of the edge.
    pub outgoing: bool,
    /// The words: "This item {label} the other item".
    pub label: &'static str,
    /// The entity types that the other end can have.
    pub other: &'static [&'static str],
}

impl Choice {
    /// The value of the choice in the select.
    pub fn value(self) -> String {
        format!("{}:{}", self.kind, if self.outgoing { "out" } else { "in" })
    }
}

/// The edges that an item of `family` can have, from its side: the type
/// rules of the server. The parent of an initiative or a task is not
/// here: the Parent picker sets it. Pure, host-tested.
pub(super) fn choices(family: Family) -> Vec<Choice> {
    let choice = |kind, outgoing, label, other| Choice {
        kind,
        outgoing,
        label,
        other,
    };
    let mut out = Vec::new();
    match family {
        Family::Strategy => out.push(choice("parent", true, "is the parent of", INITIATIVE)),
        Family::Initiative => out.push(choice("parent", true, "is the parent of", TASK)),
        _ => {}
    }
    if family.is_workflow() {
        out.push(choice("blocks", true, "blocks", WORKFLOW));
        out.push(choice("blocks", false, "is blocked by", WORKFLOW));
        out.push(choice("supports", true, "is supported by", WRITINGS));
        let informed_by = if family == Family::Task {
            WRITINGS_AND_TASKS
        } else {
            WRITINGS
        };
        out.push(choice("informs", false, "is informed by", informed_by));
        if family == Family::Task {
            out.push(choice("informs", true, "informs", TASK));
        }
    } else {
        out.push(choice("supports", false, "supports", WORKFLOW));
        out.push(choice("informs", true, "informs", WORKFLOW));
    }
    if family == Family::Adr {
        out.push(choice("supersedes", true, "supersedes", ADR));
        out.push(choice("supersedes", false, "is superseded by", ADR));
        out.push(choice("amends", true, "amends", ADR));
        out.push(choice("amends", false, "is amended by", ADR));
    }
    out
}

/// The body of `POST /api/relationships` for a choice: this item at the
/// end that the choice names. `None` for an empty code. Pure,
/// host-tested.
pub(super) fn edge_request(
    choice: Choice,
    this: &str,
    other: &str,
) -> Option<data::CreateRelationship> {
    let other = other.trim();
    if other.is_empty() {
        return None;
    }
    let (source, target) = if choice.outgoing {
        (this, other)
    } else {
        (other, this)
    };
    Some(data::CreateRelationship {
        source_short_code: source.to_string(),
        target_short_code: target.to_string(),
        relationship: choice.kind.to_string(),
    })
}

/// The type of the parent of an item of `family`: a strategy for an
/// initiative, an initiative for a task. `None`: the item has no parent.
/// Pure, host-tested.
pub(super) fn parent_types(family: Family) -> Option<&'static [&'static str]> {
    match family {
        Family::Initiative => Some(STRATEGY),
        Family::Task => Some(INITIATIVE),
        _ => None,
    }
}

/// May the person add or remove an edge from the page of this item? Yes
/// when they may edit this item, or may edit some item: an org admin, a
/// member of a team (the team edits the work of its board), or a person
/// with a `manage_` grant or with membership of a board. The server
/// decides each edge. Pure, host-tested.
pub(super) fn may_link(me: &crate::api::Whoami, may_edit_this: bool) -> bool {
    may_edit_this
        || me.organization.role == "admin"
        || !me.teams.is_empty()
        || me.capabilities.iter().any(|board| {
            !board.implied.is_empty()
                || board
                    .grants
                    .iter()
                    .any(|grant| grant == "*" || grant.starts_with("manage_"))
        })
}

/// The message after a reparent. Pure, host-tested.
pub(super) fn parent_message(outcome: &api::SetParentOutcome) -> String {
    match (&outcome.parent, outcome.changed) {
        (_, false) => "The parent did not change.".to_string(),
        (Some(parent), true) => format!("The parent is now {parent}."),
        (None, true) => "The item has no parent now.".to_string(),
    }
}

/// The add form and the Parent picker, under the list of the panel.
/// `on_changed` runs with the message of a write that the server took.
#[component]
pub(super) fn RelationshipEditor(
    family: Family,
    #[prop(into)] code: String,
    /// The item has a parent now.
    has_parent: bool,
    on_changed: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let options = choices(family);
    let first = options.first().map(|c| c.value()).unwrap_or_default();
    let options = StoredValue::new(options);
    let kind = RwSignal::new(first);
    let other = RwSignal::new(String::new());
    let parent = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    let chosen = Memo::new(move |_| {
        let value = kind.get();
        options.with_value(|all| all.iter().copied().find(|c| c.value() == value))
    });
    let other_types = Signal::derive(move || chosen.get().map_or(WORKFLOW, |c| c.other));

    let done = move |result: Result<String, ApiError>| match result {
        Ok(message) => {
            let _ = error.try_set(None);
            on_changed.run(message);
        }
        Err(e) => {
            let _ = error.try_set(Some(e));
        }
    };
    let add = Callback::new(move |()| {
        let Some(request) = chosen
            .get_untracked()
            .and_then(|c| edge_request(c, &code.get_value(), &other.get_untracked()))
        else {
            return;
        };
        busy.set(true);
        leptos::task::spawn_local(async move {
            let result = data::create_relationship(auth, &request).await.map(|_| {
                format!(
                    "Kairos added the edge {} {} {}.",
                    request.source_short_code, request.relationship, request.target_short_code
                )
            });
            let _ = busy.try_set(false);
            done(result);
        });
    });
    let reparent = move |to: Option<String>| {
        busy.set(true);
        leptos::task::spawn_local(async move {
            let result = api::set_parent(auth, family, &code.get_value(), to)
                .await
                .map(|outcome| parent_message(&outcome));
            let _ = busy.try_set(false);
            done(result);
        });
    };

    view! {
        <Stack gap="sm">
            {move || error.get().map(|e| view! {
                <Alert title="Kairos did not change the relationships" color=token::BAD>
                    <Text size="sm">{api::error_text(&e)}</Text>
                </Alert>
            })}
            {parent_types(family).map(|types| view! {
                <div data-testid="parent-picker">
                    <Stack gap="xs">
                        <CodePicker label="Parent" value=parent types exclude=code.get_value()
                            disabled=busy/>
                        <Group gap="sm">
                            {move || view! {
                                <Button size="xs"
                                    disabled=busy.get() || parent.get().trim().is_empty()
                                    on_click=Callback::new(move |_| {
                                        reparent(Some(parent.get_untracked().trim().to_string()));
                                    })>
                                    "Set parent"
                                </Button>
                            }}
                            {has_parent.then(|| view! {
                                <Button variant="default" size="xs" bad=true disabled=busy
                                    on_click=Callback::new(move |_| reparent(None))>
                                    "Remove parent"
                                </Button>
                            })}
                        </Group>
                    </Stack>
                </div>
            })}
            <div data-testid="add-relationship">
                <Stack gap="xs">
                    <Select
                        label="This item"
                        option_pairs=options.with_value(|all| {
                            all.iter().map(|c| (c.value(), c.label.to_string())).collect::<Vec<_>>()
                        })
                        value=kind
                    />
                    <CodePicker label="Other item" value=other types=other_types
                        exclude=code.get_value() disabled=busy/>
                    {move || view! {
                        <Button size="xs"
                            disabled=busy.get() || other.get().trim().is_empty()
                            on_click=add>
                            "Add relationship"
                        </Button>
                    }}
                </Stack>
            </div>
            <Text size="xs" dimmed=true>
                "To link to a URI, use the External references panel."
            </Text>
        </Stack>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(family: Family) -> Vec<String> {
        choices(family).into_iter().map(Choice::value).collect()
    }

    /// The select offers the edges that the type rules permit, from the
    /// side of the item. The parent of a child goes through the picker.
    #[test]
    fn the_choices_follow_the_type_rules() {
        assert_eq!(
            kinds(Family::Strategy),
            [
                "parent:out",
                "blocks:out",
                "blocks:in",
                "supports:out",
                "informs:in"
            ]
        );
        assert_eq!(
            kinds(Family::Task),
            [
                "blocks:out",
                "blocks:in",
                "supports:out",
                "informs:in",
                "informs:out"
            ]
        );
        assert_eq!(kinds(Family::Document), ["supports:in", "informs:out"]);
        assert_eq!(
            kinds(Family::Adr),
            [
                "supports:in",
                "informs:out",
                "supersedes:out",
                "supersedes:in",
                "amends:out",
                "amends:in"
            ]
        );
        let initiative = choices(Family::Initiative);
        assert_eq!(initiative[0].other, TASK, "an initiative parents tasks");
        assert!(initiative.iter().all(|c| c.kind != "parent" || c.outgoing));
    }

    /// The item of the page is the source of an outgoing choice and the
    /// target of an incoming one; an empty code sends nothing.
    #[test]
    fn the_edge_puts_this_item_at_its_end() {
        let blocks = choices(Family::Task)[0];
        let blocked_by = choices(Family::Task)[1];
        let out = edge_request(blocks, "DEMO-T-0001", " DEMO-T-0002 ").expect("an edge");
        assert_eq!(out.source_short_code, "DEMO-T-0001");
        assert_eq!(out.target_short_code, "DEMO-T-0002");
        assert_eq!(out.relationship, "blocks");
        let incoming = edge_request(blocked_by, "DEMO-T-0001", "DEMO-T-0002").expect("an edge");
        assert_eq!(incoming.source_short_code, "DEMO-T-0002");
        assert_eq!(incoming.target_short_code, "DEMO-T-0001");
        assert_eq!(edge_request(blocks, "DEMO-T-0001", "  "), None);
    }

    /// An initiative takes a strategy, a task an initiative; the others
    /// have no parent.
    #[test]
    fn the_parent_type_follows_the_family() {
        assert_eq!(parent_types(Family::Initiative), Some(STRATEGY));
        assert_eq!(parent_types(Family::Task), Some(INITIATIVE));
        assert_eq!(parent_types(Family::Strategy), None);
        assert_eq!(parent_types(Family::Document), None);
        assert_eq!(parent_types(Family::Adr), None);
    }

    fn me(role: &str, teams: bool, grants: &[&str], implied: &[&str]) -> crate::api::Whoami {
        serde_json::from_value(serde_json::json!({
            "user": {"id": "u-1", "display_name": "Sam", "email": "sam@example.test"},
            "organization": {"slug": "demo", "role": role},
            "teams": if teams {
                serde_json::json!([{"id": "t1", "slug": "web", "name": "Web"}])
            } else {
                serde_json::json!([])
            },
            "capabilities": [{"board_slug": "b", "grants": grants, "implied": implied}],
        }))
        .expect("test whoami")
    }

    /// The controls show for a person who may edit this item or some
    /// other item, and not for a person who may edit nothing.
    #[test]
    fn the_controls_show_for_a_person_who_may_edit_an_end() {
        let nobody = me("member", false, &[], &[]);
        assert!(!may_link(&nobody, false));
        assert!(may_link(&nobody, true), "the editor of this item");
        assert!(may_link(&me("admin", false, &[], &[]), false));
        assert!(may_link(&me("member", true, &[], &[]), false), "a team");
        assert!(may_link(
            &me("member", false, &["manage_tasks"], &[]),
            false
        ));
        assert!(may_link(&me("member", false, &["*"], &[]), false));
        assert!(may_link(
            &me("member", false, &[], &["manage_initiatives"]),
            false
        ));
        assert!(!may_link(
            &me("member", false, &["transition_items"], &[]),
            false
        ));
    }

    /// The body of the reparent always names `parent`: `null` removes it.
    #[test]
    fn the_reparent_body_always_names_the_parent() {
        let body = serde_json::to_value(api::SetParentBody { parent: None }).expect("json");
        assert_eq!(body, serde_json::json!({"parent": null}));
        let body = serde_json::to_value(api::SetParentBody {
            parent: Some("DEMO-I-0001".to_string()),
        })
        .expect("json");
        assert_eq!(body, serde_json::json!({"parent": "DEMO-I-0001"}));
    }

    #[test]
    fn the_reparent_message_says_what_changed() {
        let outcome = |parent: Option<&str>, changed| api::SetParentOutcome {
            parent: parent.map(str::to_string),
            changed,
        };
        assert_eq!(
            parent_message(&outcome(Some("DEMO-I-0002"), true)),
            "The parent is now DEMO-I-0002."
        );
        assert_eq!(
            parent_message(&outcome(None, true)),
            "The item has no parent now."
        );
        assert_eq!(
            parent_message(&outcome(Some("DEMO-I-0002"), false)),
            "The parent did not change."
        );
    }
}
