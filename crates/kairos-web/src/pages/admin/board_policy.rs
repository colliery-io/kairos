//! The claim flags of a column and the policies of a board, on
//! `/admin/boards/:board` (KAIROS-T-0587, KAIROS-T-0652).
//!
//! - [`ColumnFlagSwitch`]: the switches `claims` and `keeps_claims` of a
//!   column row (KAIROS-T-0359, KAIROS-A-0034), beside the done flag and
//!   the WIP limit.
//! - [`BoardPolicyPanel`]: the policies of the board by its level:
//!   `parent_gate` (KAIROS-A-0040) and `blockers_gate` (KAIROS-A-0053) on a
//!   delivery board, `owner_gate` on an initiative board (KAIROS-A-0042).
//!   The other levels have no policy, and the panel is not there.
//!
//! A switch sends one field (`PATCH`); a refusal turns the switch back.

use aurora_dark::components::{Button, Group, Panel, Stack, Switch, Text, TextInput};
use leptos::prelude::*;

use super::api::{self, BoardPolicy, ColumnFlag};
use super::{MutationOutcome, run_mutation};
use crate::auth::use_auth;

/// The label of the switch of a column flag. Pure, host-tested.
pub(crate) fn flag_label(flag: ColumnFlag) -> &'static str {
    match flag {
        ColumnFlag::Claims => "Gives claims",
        ColumnFlag::KeepsClaims => "Keeps claims",
    }
}

/// What the page says after a change of a column flag. Pure, host-tested.
pub(crate) fn flag_message(flag: ColumnFlag, on: bool) -> String {
    match (flag, on) {
        (ColumnFlag::Claims, true) => "The column gives claims now. A person who moves a free \
                                       task into the column gets its claim."
            .to_string(),
        (ColumnFlag::Claims, false) => "The column gives no claims now. Kairos ended the claims \
                                        of the tasks in the column, unless the column keeps \
                                        claims."
            .to_string(),
        (ColumnFlag::KeepsClaims, true) => "The column keeps claims now. A task keeps its claim \
                                            in the column."
            .to_string(),
        (ColumnFlag::KeepsClaims, false) => "The column does not keep claims now. Kairos ended \
                                             the claims of the tasks in the column, unless the \
                                             column gives claims."
            .to_string(),
    }
}

/// The title, the caption and the switch label of the panel of a policy.
/// Pure, host-tested.
pub(crate) fn policy_words(policy: BoardPolicy) -> (&'static str, &'static str, &'static str) {
    match policy {
        BoardPolicy::Parent => (
            "Board policy: parent gate",
            "When the policy is on, a task moves into a column that gives claims only while \
             its initiative is active. An initiative in Active or Monitoring is active. A \
             bucket initiative is always active. A task with no initiative moves freely.",
            "A task needs an active initiative",
        ),
        BoardPolicy::Owner => (
            "Board policy: owner gate",
            "When the policy is on, an initiative moves to Active only with an owner. The \
             owner of an initiative in Active or Monitoring cannot release it. A hand-off \
             changes the owner.",
            "An initiative needs an owner",
        ),
        BoardPolicy::Blockers => (
            "Board policy: blockers gate",
            "When the policy is on, Kairos refuses a move of a task with an open blocker. \
             The rule applies to a column that gives claims and to a done column. A cancelled \
             blocker is open. When the policy is off, Kairos records the move and gives a \
             warning.",
            "A task needs completed blockers",
        ),
    }
}

/// What the page says after a change of a policy. Pure, host-tested.
pub(crate) fn policy_message(policy: BoardPolicy, on: bool) -> String {
    let name = match policy {
        BoardPolicy::Parent => "parent gate",
        BoardPolicy::Owner => "owner gate",
        BoardPolicy::Blockers => "blockers gate",
    };
    let state = if on { "on" } else { "off" };
    format!("Kairos turned the {name} of the board {state}.")
}

/// One switch of a column flag. The switch shows the value of the column;
/// a click sends the new value, and a refusal turns the switch back.
#[component]
pub(crate) fn ColumnFlagSwitch(
    board_id: String,
    column_id: String,
    flag: ColumnFlag,
    on: bool,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let checked = RwSignal::new(on);
    let ids = StoredValue::new((board_id, column_id));
    let on_change = Callback::new(move |value: bool| {
        let (board_id, column_id) = ids.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            flag_message(flag, value),
            async move {
                let result = api::set_column_flag(auth, &board_id, &column_id, flag, value)
                    .await
                    .map(|_| ());
                if result.is_err() {
                    checked.set(!value);
                }
                result
            },
        );
    });
    let testid = match flag {
        ColumnFlag::Claims => "column-claims",
        ColumnFlag::KeepsClaims => "column-keeps-claims",
    };
    view! {
        <span data-testid=testid>
            <Switch checked label=flag_label(flag) disabled=busy on_change/>
        </span>
    }
}

/// The policies of the board, by its level: one panel per policy. Nothing
/// for a level with no policy.
#[component]
pub(crate) fn BoardPolicyPanel(
    board: api::Board,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let size = board.size_warning.map(|size| {
        view! { <SizeWarningField board_id=board.id.clone() size busy outcome reload/> }
    });
    view! {
        {BoardPolicy::for_level(&board.board_level)
            .iter()
            .map(|&policy| {
                view! {
                    <PolicySwitch board_id=board.id.clone() policy on=policy.of(&board)
                        busy outcome reload/>
                }
            })
            .collect_view()}
        {size}
    }
}

/// The number of `size_warning`, or why it is not one. Pure, host-tested.
pub(crate) fn parse_size(text: &str) -> Result<i32, String> {
    match text.trim().parse::<i32>() {
        Ok(size) if (1..=1000).contains(&size) => Ok(size),
        _ => Err("Type a number of tasks from 1 to 1000.".to_string()),
    }
}

/// The setting `size_warning` of an initiative board (KAIROS-T-0779): past
/// this many tasks, Kairos asks whether an initiative is too big.
#[component]
fn SizeWarningField(
    board_id: String,
    size: i32,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let value = RwSignal::new(size.to_string());
    let error = RwSignal::new(None::<String>);
    let board_id = StoredValue::new(board_id);
    let save = Callback::new(move |_| {
        let size = match parse_size(&value.get_untracked()) {
            Ok(size) => size,
            Err(why) => {
                error.set(Some(why));
                return;
            }
        };
        error.set(None);
        let board_id = board_id.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Kairos asks whether an initiative is too big past {size} tasks."),
            async move {
                api::set_size_warning(auth, &board_id, size)
                    .await
                    .map(|_| ())
            },
        );
    });
    view! {
        <Panel title="Initiative size" caption="Past this many tasks, Kairos asks whether an \
            initiative is too big, and where it can split. It is a warning only. The card of \
            the initiative shows its size.">
            <Stack gap="xs">
                <Group align="end" gap="sm">
                    <span data-testid="size_warning">
                        <TextInput label="Tasks" value=value/>
                    </span>
                    <Button size="xs" disabled=busy on_click=save>"Save"</Button>
                </Group>
                {move || error.get().map(|why| view! { <Text size="xs" dimmed=true>{why}</Text> })}
            </Stack>
        </Panel>
    }
}

/// The panel of one policy. The switch shows the value of the board; a
/// click sends the new value, and a refusal turns the switch back.
#[component]
fn PolicySwitch(
    board_id: String,
    policy: BoardPolicy,
    on: bool,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let (title, caption, label) = policy_words(policy);
    let checked = RwSignal::new(on);
    let board_id = StoredValue::new(board_id);
    let on_change = Callback::new(move |value: bool| {
        let board_id = board_id.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            policy_message(policy, value),
            async move {
                let result = api::set_board_policy(auth, &board_id, policy, value)
                    .await
                    .map(|_| ());
                if result.is_err() {
                    checked.set(!value);
                }
                result
            },
        );
    });
    view! {
        <Panel title=title caption=caption>
            <Stack gap="xs">
                <span data-testid=policy.field()>
                    <Switch checked label=label disabled=busy on_change/>
                </span>
                <Text dimmed=true size="xs">
                    "An agent and the command line set the same policy."
                </Text>
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0779: the setting takes 1 to 1000.
    #[test]
    fn the_size_takes_a_number_from_1_to_1000() {
        assert_eq!(parse_size(" 15 "), Ok(15));
        assert!(parse_size("0").is_err());
        assert!(parse_size("1001").is_err());
        assert!(parse_size("many").is_err());
    }

    #[test]
    fn each_flag_has_its_label_and_its_messages() {
        assert_eq!(flag_label(ColumnFlag::Claims), "Gives claims");
        assert_eq!(flag_label(ColumnFlag::KeepsClaims), "Keeps claims");
        assert!(flag_message(ColumnFlag::Claims, true).starts_with("The column gives claims now."));
        assert!(flag_message(ColumnFlag::Claims, false).contains("Kairos ended the claims"));
        assert!(
            flag_message(ColumnFlag::KeepsClaims, true).starts_with("The column keeps claims now.")
        );
        assert!(flag_message(ColumnFlag::KeepsClaims, false).contains("unless the column gives"));
    }

    #[test]
    fn each_policy_has_its_words() {
        assert_eq!(
            policy_words(BoardPolicy::Parent).0,
            "Board policy: parent gate"
        );
        assert_eq!(
            policy_words(BoardPolicy::Owner).0,
            "Board policy: owner gate"
        );
        let (title, caption, label) = policy_words(BoardPolicy::Blockers);
        assert_eq!(title, "Board policy: blockers gate");
        assert!(caption.contains("Kairos refuses a move of a task with an open blocker."));
        assert!(caption.contains("A cancelled blocker is open."));
        assert_eq!(label, "A task needs completed blockers");
        assert_eq!(
            policy_message(BoardPolicy::Blockers, true),
            "Kairos turned the blockers gate of the board on."
        );
        assert_eq!(
            policy_message(BoardPolicy::Blockers, false),
            "Kairos turned the blockers gate of the board off."
        );
        assert_eq!(
            policy_message(BoardPolicy::Parent, false),
            "Kairos turned the parent gate of the board off."
        );
        assert_eq!(
            policy_message(BoardPolicy::Owner, true),
            "Kairos turned the owner gate of the board on."
        );
    }

    /// The texts keep the rule of the GUI: sentences of 20 words or fewer.
    #[test]
    fn the_texts_have_short_sentences() {
        let mut texts: Vec<String> = Vec::new();
        for flag in [ColumnFlag::Claims, ColumnFlag::KeepsClaims] {
            texts.push(flag_message(flag, true));
            texts.push(flag_message(flag, false));
        }
        for policy in [
            BoardPolicy::Parent,
            BoardPolicy::Owner,
            BoardPolicy::Blockers,
        ] {
            let (title, caption, label) = policy_words(policy);
            texts.extend([title.to_string(), caption.to_string(), label.to_string()]);
            texts.push(policy_message(policy, true));
            texts.push(policy_message(policy, false));
        }
        for text in texts {
            for sentence in text.split(". ") {
                let words = sentence.split_whitespace().count();
                assert!(words <= 20, "{words} words: {sentence}");
            }
        }
    }
}
