//! The "for me" page at `/` (KAIROS-T-0490, COLLIERY-I-0624): the work of
//! the signed-in person, from `GET /api/me/work`.
//!
//! Two lanes: Initiatives (the strategies and the initiatives that the
//! person owns) and Tasks (the tasks that the person or an agent key of the
//! person claims, the tasks under the initiatives of the person, and the
//! tasks that these block). Each lane has three columns: Todo, Active and
//! Blocked. The server decides the state of each item; the page only puts
//! each card in its column.
//!
//! A card links to its item, and names its board, its claimant or owner,
//! and its open blockers. The page reads the work again on each item event
//! of the organization (`/ws/events`) and on each reconnect.

use aurora_dark::components::{
    Button, Empty, ErrorState, Group, Loading, PageHeader, Pill, Stack, Text,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;

use super::copy_link;
use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types::TaskClaim` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkClaim {
    pub display_name: String,
    #[serde(default)]
    pub agent: bool,
}

/// mirror of: `kairos_client::types::ItemOwner` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkOwner {
    pub display_name: String,
}

/// mirror of: `kairos_client::types_work::WorkBoard` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkBoard {
    pub slug: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_work::WorkBlocker`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkBlocker {
    pub short_code: String,
    pub title: String,
}

/// mirror of: `kairos_client::types_work::WorkItem` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkItem {
    pub short_code: String,
    pub title: String,
    pub entity_type: String,
    pub state: String,
    pub board: WorkBoard,
    pub column_name: String,
    #[serde(default)]
    pub claim: Option<WorkClaim>,
    #[serde(default)]
    pub owner: Option<WorkOwner>,
    #[serde(default)]
    pub blockers: Vec<WorkBlocker>,
}

/// mirror of: `kairos_client::types_work::MyWork`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct MyWork {
    pub initiatives: Vec<WorkItem>,
    pub tasks: Vec<WorkItem>,
}

const WORK: &str = "/api/me/work";

async fn work(auth: Auth) -> Result<MyWork, ApiError> {
    crate::api::get_json(auth, WORK).await
}

/// The columns of a lane: `(state, heading)`, in the order of the page.
const COLUMNS: [(&str, &str); 3] = [
    ("todo", "Todo"),
    ("active", "Active"),
    ("blocked", "Blocked"),
];

/// The items of a lane in each column, in the order of [`COLUMNS`]. An item
/// with a state that the page does not know goes to Todo. Pure,
/// host-tested.
fn columns_of(items: &[WorkItem]) -> [Vec<WorkItem>; 3] {
    let mut out: [Vec<WorkItem>; 3] = Default::default();
    for item in items {
        let index = COLUMNS
            .iter()
            .position(|(state, _)| *state == item.state)
            .unwrap_or(0);
        out[index].push(item.clone());
    }
    out
}

/// The chip of who has the item: the claimant of a task ("(agent)" when an
/// agent key made the claim), or the owner. Pure, host-tested.
fn who(item: &WorkItem) -> Option<String> {
    if let Some(claim) = &item.claim {
        return Some(if claim.agent {
            format!("{} (agent)", claim.display_name)
        } else {
            claim.display_name.clone()
        });
    }
    item.owner
        .as_ref()
        .map(|owner| format!("owner: {}", owner.display_name))
}

/// The words of the type of an item. Pure, host-tested.
fn kind_label(entity_type: &str) -> &'static str {
    match entity_type {
        "strategy" => "Strategy",
        "initiative" => "Initiative",
        _ => "Task",
    }
}

/// One card of the page: the code (the link to the item), the type, the
/// board and the column, who has it, the title, and the open blockers.
#[component]
fn WorkCard(item: WorkItem) -> impl IntoView {
    let holder = who(&item);
    let WorkItem {
        short_code,
        title,
        entity_type,
        state,
        board,
        column_name,
        blockers,
        ..
    } = item;
    let href = format!("/items/{short_code}");
    let code_attr = short_code.clone();
    let code_for_copy = short_code.clone();
    let place = format!("{} / {column_name}", board.name);
    view! {
        <article class="kairos-card" data-testid="work-card"
            data-code=code_attr data-state=state>
            <Group justify="between" wrap=true>
                <Group gap="xs">
                    <a class="cl-mono kairos-card__code" href=href>{short_code}</a>
                    <copy_link::CopyLinkButton code=code_for_copy/>
                </Group>
                <Group gap="xs" wrap=true>
                    {holder.map(|label| view! {
                        <span class="kairos-card__claim" data-testid="work-holder">
                            <Pill color=token::OK>{label}</Pill>
                        </span>
                    })}
                    <Pill color=token::MUTED>{kind_label(&entity_type)}</Pill>
                </Group>
            </Group>
            <span class="kairos-card__title">{title}</span>
            <span data-testid="work-board" data-board=board.slug>
                <Text dimmed=true size="xs">{place}</Text>
            </span>
            {(!blockers.is_empty()).then(|| view! {
                <Stack gap="xs">
                    {blockers.into_iter().map(|blocker| {
                        let href = format!("/items/{}", blocker.short_code);
                        view! {
                            <a class="kairos-card__blocks" data-testid="work-blocker"
                                href=href title=blocker.title>
                                <Pill color=token::GOLD>
                                    {format!("blocked by {}", blocker.short_code)}
                                </Pill>
                            </a>
                        }
                    }).collect_view()}
                </Stack>
            })}
        </article>
    }
}

/// One lane: a heading and the three columns of [`COLUMNS`].
#[component]
pub(super) fn WorkLane(
    #[prop(into)] title: String,
    testid: &'static str,
    items: Vec<WorkItem>,
) -> impl IntoView {
    let count = items.len();
    let columns = columns_of(&items);
    view! {
        <section data-testid=testid>
            <Stack gap="xs">
                <Group gap="xs">
                    <Text bright=true bold=true>{title}</Text>
                    <Text dimmed=true size="xs">{count.to_string()}</Text>
                </Group>
                <div class="kairos-board">
                    {COLUMNS.iter().zip(columns).map(|((state, heading), cards)| {
                        let size = cards.len().to_string();
                        view! {
                            <section class="kairos-board__column" data-column=*state>
                                <header class="kairos-board__column-head">
                                    <Group gap="xs">
                                        <Text bright=true bold=true size="sm">{*heading}</Text>
                                        <Text dimmed=true size="xs">{size}</Text>
                                    </Group>
                                </header>
                                <div class="kairos-board__cards">
                                    <Stack gap="xs">
                                        {cards.is_empty().then(|| view! {
                                            <Text dimmed=true size="xs">"No items in this column."</Text>
                                        })}
                                        {cards.into_iter().map(|item| view! {
                                            <WorkCard item/>
                                        }).collect_view()}
                                    </Stack>
                                </div>
                            </section>
                        }
                    }).collect_view()}
                </div>
            </Stack>
        </section>
    }
}

/// The page when the person has no work: what to do next.
#[component]
fn NoWork() -> impl IntoView {
    view! {
        <div data-testid="for-me-empty">
            <Stack gap="sm">
                <Empty
                    message="You have no work in Kairos now."
                    hint="Open the board of your team and move a task to Active: the task is \
                          then yours. Or claim an initiative that has no owner. Your agent's \
                          claims show here too."
                />
                <Group gap="sm">
                    <Button variant="default" size="xs" href="/boards">"Open the boards"</Button>
                    <Button variant="subtle" size="xs" href="/search">"Search the work"</Button>
                </Group>
            </Stack>
        </div>
    }
}

/// How long the page waits after an event before it reads again
/// (KAIROS-T-0617): a burst of events is one read.
pub(super) const DEBOUNCE_MS: u64 = 250;

/// The debounce of the live refetch (KAIROS-T-0617): `arm` says whether to
/// start the timer (false while one runs), `fire` ends it. Pure, host-tested.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Debounce {
    armed: bool,
}

impl Debounce {
    /// An event came. `true`: start the timer. `false`: one runs already.
    pub(super) fn arm(&mut self) -> bool {
        !std::mem::replace(&mut self.armed, true)
    }

    /// The timer fired: the next event starts a new one.
    pub(super) fn fire(&mut self) {
        self.armed = false;
    }
}

/// `/`: the work of the signed-in person.
#[component]
pub fn ForMePage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    // KAIROS-T-0617: one read at a time, and a burst of events is one read.
    // The gate (the board's, `data::RefetchGate`) merges the events that
    // come during a read into one more read; the debounce waits 250 ms
    // after an event before it asks, so N events in a burst ask once.
    let gate = StoredValue::new(super::boards::data::RefetchGate::default());
    let debounce = StoredValue::new(Debounce::default());
    let my_work = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        async move {
            gate.update_value(|g| g.started());
            let result = work(auth).await;
            if gate.try_update_value(|g| g.finished()).unwrap_or(false) {
                reload.try_update(|n| *n += 1);
            }
            result
        }
    });
    let ask = move || {
        let now = gate.try_update_value(|g| g.request()).unwrap_or(false);
        if now {
            reload.try_update(|n| *n += 1);
        }
    };
    // Live: each item event of the organization, and each reconnect, reads
    // the work again, through the debounce and the gate.
    let guard = super::boards::live::subscribe_all_events(auth, move |_| {
        if debounce.try_update_value(Debounce::arm).unwrap_or(false) {
            set_timeout(
                move || {
                    debounce.try_update_value(Debounce::fire);
                    ask();
                },
                std::time::Duration::from_millis(DEBOUNCE_MS),
            );
        }
    });
    let live_guard: StoredValue<Option<super::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    view! {
        <PageHeader title="For me" sub="The initiatives that you own, and the tasks that you or \
                                        your agents claim, the tasks under your initiatives, and \
                                        the tasks that these block. Done items do not show."/>
        <div data-testid="for-me">
            {move || match my_work.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(work)) if work.initiatives.is_empty() && work.tasks.is_empty() => {
                    view! { <NoWork/> }.into_any()
                }
                Some(Ok(work)) => view! {
                    <Stack gap="lg">
                        <WorkLane title="Initiatives" testid="for-me-initiatives"
                            items=work.initiatives/>
                        <WorkLane title="Tasks" testid="for-me-tasks" items=work.tasks/>
                    </Stack>
                }.into_any(),
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_burst_of_events_arms_one_timer() {
        let mut debounce = super::Debounce::default();
        assert!(debounce.arm(), "the first event starts the timer");
        assert!(!debounce.arm(), "the second does not");
        assert!(!debounce.arm());
        debounce.fire();
        assert!(debounce.arm(), "after the timer, the next event starts one");
    }

    use super::*;

    fn item(code: &str, state: &str) -> WorkItem {
        WorkItem {
            short_code: code.into(),
            title: "A title".into(),
            entity_type: "task".into(),
            state: state.into(),
            board: WorkBoard {
                slug: "web-delivery".into(),
                name: "Web".into(),
            },
            column_name: "Todo".into(),
            claim: None,
            owner: None,
            blockers: Vec::new(),
        }
    }

    #[test]
    fn each_state_has_its_column() {
        let [todo, active, blocked] = columns_of(&[
            item("WEB-T-0001", "active"),
            item("WEB-T-0002", "blocked"),
            item("WEB-T-0003", "todo"),
            item("WEB-T-0004", "later"),
        ]);
        let codes = |items: &[WorkItem]| -> Vec<String> {
            items.iter().map(|i| i.short_code.clone()).collect()
        };
        assert_eq!(codes(&todo), vec!["WEB-T-0003", "WEB-T-0004"]);
        assert_eq!(codes(&active), vec!["WEB-T-0001"]);
        assert_eq!(codes(&blocked), vec!["WEB-T-0002"]);
    }

    #[test]
    fn the_chip_names_the_claimant_or_the_owner() {
        let mut task = item("WEB-T-0001", "active");
        assert_eq!(who(&task), None);
        task.claim = Some(WorkClaim {
            display_name: "Robin".into(),
            agent: true,
        });
        assert_eq!(who(&task).as_deref(), Some("Robin (agent)"));
        let mut initiative = item("DEMO-I-0001", "todo");
        initiative.owner = Some(WorkOwner {
            display_name: "Sam".into(),
        });
        assert_eq!(who(&initiative).as_deref(), Some("owner: Sam"));
        assert_eq!(kind_label("initiative"), "Initiative");
        assert_eq!(kind_label("task"), "Task");
    }

    #[test]
    fn the_answer_of_the_server_parses() {
        let work: MyWork = serde_json::from_value(serde_json::json!({
            "initiatives": [],
            "tasks": [{
                "id": "x", "short_code": "WEB-T-0001", "title": "T", "entity_type": "task",
                "state": "blocked", "reasons": ["claimed"],
                "board": {"id": "b", "slug": "web-delivery", "name": "Web"},
                "column_id": "c", "column_name": "Active",
                "claim": {"user_id": "u", "display_name": "Robin", "agent": false,
                          "claimed_at": "2026-10-15T00:00:00Z"},
                "blockers": [{"short_code": "WEB-T-0002", "title": "B"}]
            }]
        }))
        .expect("parses");
        assert_eq!(work.tasks[0].blockers[0].short_code, "WEB-T-0002");
        assert_eq!(work.tasks[0].claim.as_ref().map(|c| c.agent), Some(false));
    }
}
