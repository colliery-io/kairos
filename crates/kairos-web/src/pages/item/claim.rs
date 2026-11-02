//! The claim of a task on the item page (KAIROS-T-0369, KAIROS-T-0359,
//! KAIROS-T-0535).
//!
//! The claim says who works on a task in a claims column (Active). A
//! person who moves the task to Active gets it. [`ClaimPanel`] shows the
//! claim and gives three controls:
//!
//! - Claim: take the claim of a task with no claim
//!   (`POST /api/tasks/{short_code}/claim`, KAIROS-T-0528). It needs
//!   `transition_items` on the board (KAIROS-A-0030 as amended).
//! - Hand off: give the claim to a person who holds `transition_items` on
//!   the board (KAIROS-T-0530), with or without a claim now. The list comes
//!   from `GET /api/tasks/{short_code}/hand-off-targets`. The picker starts
//!   with no person, and a dialog asks before the hand-off.
//! - Release: end the claim. The task stays in Active with no claim.
//!
//! [`claim_actions`] mirrors the rules of the server. The server stays the
//! authority: a refusal shows in the panel. After an action the page reads
//! the item again, so the panel shows the new claim.

use aurora_dark::components::{Alert, Button, Group, Panel, Select, Stack, Text};
use aurora_dark::frame::ConfirmDialog;
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;

use super::api::{self, BoardInfo, Claim};
use crate::auth::use_auth;
use crate::pages::boards;

/// mirror of: `kairos_client::types::HandOffTarget`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Member {
    user_id: String,
    email: String,
    display_name: String,
}

/// The text of the claim: who has it, and since when. Pure, host-tested.
pub(crate) fn claim_text(claim: &Claim) -> String {
    // KAIROS-T-0665: the user name to write after `@`.
    let name = crate::pages::mention::with_handle(&claim.display_name, &claim.user_name);
    let who = if claim.agent {
        format!("{name} (agent)")
    } else {
        name
    };
    let since = claim.claimed_at.get(..10).unwrap_or(&claim.claimed_at);
    if since.is_empty() {
        format!("{who} has the claim.")
    } else {
        format!("{who} has the claim since {since}.")
    }
}

/// The text when the task is in a claims column with no claim.
pub(crate) const NO_CLAIM: &str =
    "No claim. A person who can move the task can claim it or hand it off.";

/// The first choice of the hand-off picker: no person.
const CHOOSE: &str = "Choose a person";

/// May the signed-in person hand off or release the claim? The holder
/// may, and a person who can move the task may. Pure, host-tested.
pub(crate) fn may_change_claim(me: &str, holder: &str, can_move: bool) -> bool {
    can_move || (!me.is_empty() && me == holder)
}

/// The controls of the claim panel that the signed-in person may use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClaimActions {
    pub claim: bool,
    pub hand_off: bool,
    pub release: bool,
}

/// The rules of the server (`kairos_server::claims`), mirrored. Claim: the
/// task has no claim and the person can move it. Hand off: the holder, or
/// a person who can move the task, with or without a claim. Release: the
/// task has a claim, and the person may change it. Pure, host-tested.
pub(crate) fn claim_actions(me: &str, holder: Option<&str>, can_move: bool) -> ClaimActions {
    if me.is_empty() {
        return ClaimActions::default();
    }
    let may_change = may_change_claim(me, holder.unwrap_or_default(), can_move);
    ClaimActions {
        claim: holder.is_none() && can_move,
        hand_off: may_change,
        release: holder.is_some() && may_change,
    }
}

/// The persons a claim can go to: each hand-off target that does not
/// have the claim, as `(user id, label)`. `holder` is empty when the task
/// has no claim. Pure, host-tested.
fn hand_off_options(members: &[Member], holder: &str) -> Vec<(String, String)> {
    members
        .iter()
        .filter(|m| m.user_id != holder)
        .map(|m| {
            (
                m.user_id.clone(),
                format!("{} ({})", m.display_name, m.email),
            )
        })
        .collect()
}

/// The question of the hand-off dialog for the chosen person `to`.
/// Pure, host-tested.
fn hand_off_question(members: &[Member], to: &str) -> String {
    match members.iter().find(|m| m.user_id == to) {
        Some(m) => format!(
            "Kairos gives the claim to {} ({}).",
            m.display_name, m.email
        ),
        None => "Choose a person first.".to_string(),
    }
}

/// The claim panel of a task. It shows when the task has a claim, or when
/// the task is in a column that gives a claim.
#[component]
pub fn ClaimPanel(
    #[prop(into)] code: String,
    /// The claim of the task. A signal (KAIROS-T-0618): the page stays
    /// mounted when the claim changes.
    #[prop(into)]
    claim: Signal<Option<Claim>>,
    /// The page's shared board read.
    board: LocalResource<Result<Option<BoardInfo>, ApiError>>,
    #[prop(into)] column_id: Signal<Option<String>>,
    #[prop(into)] archived: Signal<bool>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let code = StoredValue::new(code);
    // Show the panel when the task has a claim, or sits in a column that
    // gives one.
    let shown = Memo::new(move |_| {
        if claim.with(Option::is_some) {
            return true;
        }
        let Some(Ok(Some(board))) = board.get() else {
            return false;
        };
        column_id.with(|column| {
            board
                .columns
                .iter()
                .any(|c| Some(&c.id) == column.as_ref() && c.claims)
        })
    });
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let holder = Memo::new(move |_| claim.with(|c| c.as_ref().map(|c| c.user_id.clone())));
    let actions = Memo::new(move |_| {
        let Some(me) = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
        else {
            return ClaimActions::default();
        };
        let can_move = match board.get() {
            Some(Ok(Some(board))) => {
                boards::board_powers(&me, &board.slug, board.team_id.as_deref(), None).transition
            }
            _ => false,
        };
        claim_actions(&me.user.id, holder.get().as_deref(), can_move)
    });
    view! {
        {move || shown.get().then(|| view! {
            <Panel title="Claim" caption="who works on the task">
                <div class="kairos-item__claim" data-testid="claim-panel">
                    <Stack gap="sm">
                        {move || match claim.with(|c| c.as_ref().map(claim_text)) {
                            Some(text) => view! {
                                <Text size="sm" bright=true>{text}</Text>
                            }.into_any(),
                            None => view! {
                                <Text size="sm" dimmed=true>{NO_CLAIM}</Text>
                            }.into_any(),
                        }}
                        {move || (actions.get() != ClaimActions::default() && !archived.get()).then(|| view! {
                            <ClaimControls
                                code=code.get_value()
                                holder=holder.get().unwrap_or_default()
                                actions=actions.get()
                                on_moved
                            />
                        })}
                    </Stack>
                </div>
            </Panel>
        })}
    }
}

/// Claim, Hand off and Release.
#[component]
fn ClaimControls(
    code: String,
    /// The user id of the holder; empty when the task has no claim.
    holder: String,
    actions: ClaimActions,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let title = format!("Hand off {code}?");
    let code = StoredValue::new(code);
    let holder = StoredValue::new(holder);
    // KAIROS-T-0535: the picker starts with no person.
    let to = RwSignal::new(String::new());
    let asking = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);
    // KAIROS-T-0530: only a person who holds transition_items on the
    // board can get the claim; the server lists them.
    let members = LocalResource::new(move || {
        auth.track_session();
        let path = format!("/api/tasks/{}/hand-off-targets", code.get_value());
        async move {
            if actions.hand_off {
                crate::api::get_json::<Vec<Member>>(auth, &path).await
            } else {
                Ok(Vec::new())
            }
        }
    });
    let claim: Callback<()> = Callback::new(move |()| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::claim_task(auth, &code.get_value()).await {
                Ok(_) => on_moved.run("You have the claim.".to_string()),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    let hand_off: Callback<()> = Callback::new(move |()| {
        asking.set(false);
        let person = to.get_untracked();
        if person.is_empty() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::hand_off_task(auth, &code.get_value(), &person).await {
                Ok(task) => {
                    let name = task
                        .claim
                        .map(|c| c.display_name)
                        .unwrap_or_else(|| "the person".to_string());
                    on_moved.run(format!("Kairos gave the claim to {name}."));
                }
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    let release: Callback<()> = Callback::new(move |()| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::release_task(auth, &code.get_value()).await {
                Ok(_) => on_moved.run(
                    "Kairos released the claim. The task stays in Active with no claim."
                        .to_string(),
                ),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    let question = move || match members.get() {
        Some(Ok(list)) => hand_off_question(&list, &to.get()),
        _ => String::new(),
    };
    view! {
        <Stack gap="xs">
            {actions.claim.then(|| view! {
                <Group gap="sm">
                    <Button size="xs" disabled=busy on_click=claim>"Claim"</Button>
                </Group>
            })}
            {move || actions.hand_off.then(|| match members.get() {
                None => view! { <Text size="xs" dimmed=true>"Loading the members of the board…"</Text> }.into_any(),
                Some(Err(_)) => view! {
                    <Text size="xs" dimmed=true>"The members of the board are not available."</Text>
                }.into_any(),
                Some(Ok(list)) => {
                    let options = holder.with_value(|holder| hand_off_options(&list, holder));
                    view! {
                        <Group align="end" gap="sm" wrap=true>
                            <Select label="Hand off to" placeholder=CHOOSE option_pairs=options
                                value=to/>
                            <Button size="xs"
                                disabled=move || busy.get() || to.with(String::is_empty)
                                on_click=Callback::new(move |_| asking.set(true))>
                                "Hand off"
                            </Button>
                        </Group>
                    }.into_any()
                }
            })}
            {actions.release.then(|| view! {
                <Group gap="sm">
                    <Button variant="default" size="xs" disabled=busy on_click=release>
                        "Release"
                    </Button>
                </Group>
            })}
            {move || error.get().map(|e| view! {
                <Alert title="Could not change the claim" color=token::BAD>
                    <Text size="sm">{api::error_text(&e)}</Text>
                </Alert>
            })}
            <ConfirmDialog
                open=asking
                title=title
                confirm_label="Hand off"
                danger=false
                busy=busy
                on_confirm=hand_off
            >
                <Text size="sm">{question}</Text>
            </ConfirmDialog>
        </Stack>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(agent: bool) -> Claim {
        Claim {
            user_id: "u1".to_string(),
            display_name: "Alice".to_string(),
            user_name: String::new(),
            agent,
            claimed_at: "2026-10-09T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn the_text_names_the_holder_and_the_day() {
        assert_eq!(
            claim_text(&claim(false)),
            "Alice has the claim since 2026-10-09."
        );
        assert_eq!(
            claim_text(&claim(true)),
            "Alice (agent) has the claim since 2026-10-09."
        );
        let named = Claim {
            user_name: "alice".to_string(),
            ..claim(true)
        };
        assert_eq!(
            claim_text(&named),
            "Alice @alice (agent) has the claim since 2026-10-09."
        );
    }

    #[test]
    fn the_holder_or_a_mover_may_change_the_claim() {
        assert!(may_change_claim("u1", "u1", false));
        assert!(may_change_claim("u2", "u1", true));
        assert!(!may_change_claim("u2", "u1", false));
        assert!(!may_change_claim("", "", false));
    }

    #[test]
    fn a_mover_claims_a_task_with_no_claim() {
        let actions = claim_actions("u2", None, true);
        assert!(actions.claim);
        assert!(actions.hand_off);
        assert!(!actions.release);
    }

    #[test]
    fn a_person_who_cannot_move_the_task_gets_no_control_without_a_claim() {
        assert_eq!(claim_actions("u2", None, false), ClaimActions::default());
    }

    #[test]
    fn nobody_claims_a_task_with_a_claim() {
        // The holder releases and hands off, but does not claim again.
        let holder = claim_actions("u1", Some("u1"), false);
        assert!(!holder.claim);
        assert!(holder.hand_off);
        assert!(holder.release);
        // A mover hands off and releases the claim of a different person.
        let mover = claim_actions("u2", Some("u1"), true);
        assert!(!mover.claim);
        assert!(mover.hand_off);
        assert!(mover.release);
        // A different person who cannot move the task gets nothing.
        assert_eq!(
            claim_actions("u2", Some("u1"), false),
            ClaimActions::default()
        );
    }

    #[test]
    fn no_signed_in_person_gets_no_control() {
        assert_eq!(claim_actions("", None, true), ClaimActions::default());
        assert_eq!(claim_actions("", Some(""), true), ClaimActions::default());
    }

    fn members() -> Vec<Member> {
        vec![
            Member {
                user_id: "u1".to_string(),
                email: "a@example.test".to_string(),
                display_name: "Alice".to_string(),
            },
            Member {
                user_id: "u2".to_string(),
                email: "b@example.test".to_string(),
                display_name: "Bob".to_string(),
            },
        ]
    }

    #[test]
    fn the_options_leave_out_the_holder() {
        assert_eq!(
            hand_off_options(&members(), "u1"),
            vec![("u2".to_string(), "Bob (b@example.test)".to_string())]
        );
    }

    #[test]
    fn with_no_claim_the_options_list_each_target() {
        assert_eq!(
            hand_off_options(&members(), ""),
            vec![
                ("u1".to_string(), "Alice (a@example.test)".to_string()),
                ("u2".to_string(), "Bob (b@example.test)".to_string()),
            ]
        );
    }

    #[test]
    fn the_question_names_the_chosen_person() {
        assert_eq!(
            hand_off_question(&members(), "u2"),
            "Kairos gives the claim to Bob (b@example.test)."
        );
        assert_eq!(hand_off_question(&members(), ""), "Choose a person first.");
    }
}
