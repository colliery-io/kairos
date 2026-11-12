//! The Watching lane of the "for me" page (KAIROS-T-0763, decision 7 of
//! COLLIERY-I-0674): each item that the person watches, from
//! `GET /api/me/watches`.
//!
//! A swim lane as the other lanes of the page, with four columns: Todo,
//! Active, Blocked and Done. The server decides the state of each item
//! (`state`); an unknown state goes to Todo. Each card links to the item and
//! names its board, its column, the claim of a task and the time of its last
//! change. In each column the newest change is first. An archived item does
//! not show.

use aurora_dark::components::{Group, Pill, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;

use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types_notifications::WatchedItem` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct WatchedItem {
    pub short_code: String,
    pub entity_type: String,
    pub title: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub board: Option<String>,
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub claim: Option<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub state: String,
}

/// mirror of: `kairos_client::types_notifications::WatchList`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub(super) struct WatchList {
    pub items: Vec<WatchedItem>,
}

const WATCHES: &str = "/api/me/watches";

async fn watches(auth: Auth) -> Result<WatchList, ApiError> {
    crate::api::get_json(auth, WATCHES).await
}

/// The rows of the lane: no archived item, the newest change first, then
/// by code. The times are RFC 3339 in UTC, so the text order is the time
/// order. Pure, host-tested.
pub(super) fn rows(items: &[WatchedItem]) -> Vec<WatchedItem> {
    let mut out: Vec<WatchedItem> = items.iter().filter(|w| !w.archived).cloned().collect();
    out.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.short_code.cmp(&b.short_code))
    });
    out
}

/// The columns of the lane: `(state, heading)`, in the order of the page.
pub(super) const COLUMNS: [(&str, &str); 4] = [
    ("todo", "Todo"),
    ("active", "Active"),
    ("blocked", "Blocked"),
    ("done", "Done"),
];

/// The rows in each column, in the order of [`COLUMNS`]; the order of
/// [`rows`] in each. An unknown state goes to Todo. Pure, host-tested.
pub(super) fn columns_of(items: &[WatchedItem]) -> [Vec<WatchedItem>; 4] {
    let mut out: [Vec<WatchedItem>; 4] = Default::default();
    for item in rows(items) {
        let index = COLUMNS
            .iter()
            .position(|(state, _)| *state == item.state)
            .unwrap_or(0);
        out[index].push(item);
    }
    out
}

/// Where the item is: "board / column", the board only, or nothing. Pure,
/// host-tested.
pub(super) fn place(item: &WatchedItem) -> Option<String> {
    match (&item.board, &item.column) {
        (Some(board), Some(column)) => Some(format!("{board} / {column}")),
        (Some(board), None) => Some(board.clone()),
        _ => None,
    }
}

/// The time of the last change: the date and the time to the minute, UTC.
/// Pure, host-tested.
pub(super) fn changed(rfc3339: &str) -> String {
    match rfc3339.split_once('T') {
        Some((date, time)) => format!("changed {date} {} UTC", time.get(..5).unwrap_or(time)),
        None => format!("changed {rfc3339}"),
    }
}

/// One row: the code (the link), the type, the title, the place, the claim
/// and the last change.
#[component]
fn WatchRow(item: WatchedItem) -> impl IntoView {
    let where_ = place(&item);
    let when = changed(&item.updated_at);
    let WatchedItem {
        short_code,
        entity_type,
        title,
        claim,
        ..
    } = item;
    let href = format!("/items/{short_code}");
    let code_attr = short_code.clone();
    let code_for_copy = short_code.clone();
    view! {
        <article class="kairos-card" data-testid="watching-item" data-code=code_attr>
            <Group justify="between" wrap=true>
                <Group gap="xs">
                    <a class="cl-mono kairos-card__code" href=href>{short_code}</a>
                    <super::copy_link::CopyLinkButton code=code_for_copy/>
                </Group>
                <Group gap="xs" wrap=true>
                    {claim.map(|name| view! {
                        <span class="kairos-card__claim" data-testid="watching-claim">
                            <Pill color=token::OK>{name}</Pill>
                        </span>
                    })}
                    <Pill color=token::MUTED>{entity_type}</Pill>
                </Group>
            </Group>
            <span class="kairos-card__title">{title}</span>
            <Group gap="xs" wrap=true>
                {where_.map(|text| view! {
                    <span data-testid="watching-place">
                        <Text dimmed=true size="xs">{text}</Text>
                    </span>
                })}
                <span data-testid="watching-changed">
                    <Text dimmed=true size="xs">{when}</Text>
                </span>
            </Group>
        </article>
    }
}

/// The text of the lane with no row.
pub(super) const NO_WATCH: &str = "You watch no item. Open an item and click Watch to see it here.";

/// The Watching lane. It reads the watches again when `reload` changes.
#[component]
pub(super) fn WatchingLane(reload: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let list = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        async move { watches(auth).await }
    });
    view! {
        <section data-testid="for-me-watching">
            <Stack gap="xs">
                {move || {
                    let items = match list.get() {
                        Some(Ok(list)) => Some(list.items.into_iter().filter(|w| !w.archived).collect::<Vec<_>>()),
                        Some(Err(_)) => None,
                        None => Some(Vec::new()),
                    };
                    let count = items.as_ref().map(Vec::len).unwrap_or(0).to_string();
                    view! {
                        <Group gap="xs">
                            <Text bright=true bold=true>"Watching"</Text>
                            <Text dimmed=true size="xs">{count}</Text>
                        </Group>
                        {match items {
                            None => view! {
                                <Text dimmed=true size="xs">"Your watches did not load."</Text>
                            }.into_any(),
                            Some(items) if items.is_empty() => view! {
                                <span data-testid="watching-empty">
                                    <Text dimmed=true size="xs">{NO_WATCH}</Text>
                                </span>
                            }.into_any(),
                            Some(items) => view! {
                                <div class="kairos-board">
                                    {COLUMNS.iter().zip(columns_of(&items)).map(|((state, heading), cards)| {
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
                                                            <WatchRow item/>
                                                        }).collect_view()}
                                                    </Stack>
                                                </div>
                                            </section>
                                        }
                                    }).collect_view()}
                                </div>
                            }.into_any(),
                        }}
                    }
                }}
            </Stack>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: &str, updated_at: &str) -> WatchedItem {
        WatchedItem {
            short_code: code.into(),
            entity_type: "task".into(),
            title: "A title".into(),
            archived: false,
            board: Some("Web".into()),
            column: Some("Active".into()),
            claim: None,
            updated_at: updated_at.into(),
            state: "active".into(),
        }
    }

    #[test]
    fn each_state_has_its_column_and_an_unknown_one_goes_to_todo() {
        let mut todo = item("WEB-T-0001", "2026-11-10T00:00:00Z");
        todo.state = "todo".into();
        let active = item("WEB-T-0002", "2026-11-10T00:00:00Z");
        let mut blocked = item("WEB-T-0003", "2026-11-10T00:00:00Z");
        blocked.state = "blocked".into();
        let mut done = item("WEB-T-0004", "2026-11-10T00:00:00Z");
        done.state = "done".into();
        let mut odd = item("WEB-T-0005", "2026-11-11T00:00:00Z");
        odd.state = "elsewhere".into();
        let mut archived = item("WEB-T-0006", "2026-11-12T00:00:00Z");
        archived.archived = true;
        let out = columns_of(&[todo, active, blocked, done, odd, archived]);
        let codes: Vec<Vec<&str>> = out
            .iter()
            .map(|c| c.iter().map(|w| w.short_code.as_str()).collect())
            .collect();
        assert_eq!(
            codes,
            vec![
                vec!["WEB-T-0005", "WEB-T-0001"],
                vec!["WEB-T-0002"],
                vec!["WEB-T-0003"],
                vec!["WEB-T-0004"],
            ]
        );
    }

    #[test]
    fn an_empty_list_has_no_row() {
        let list: WatchList =
            serde_json::from_value(serde_json::json!({"items": []})).expect("parses");
        assert!(rows(&list.items).is_empty());
        assert!(NO_WATCH.starts_with("You watch no item."));
    }

    #[test]
    fn the_newest_change_is_first_and_an_archived_item_does_not_show() {
        let mut archived = item("WEB-T-0004", "2026-11-12T00:00:00Z");
        archived.archived = true;
        let out = rows(&[
            item("WEB-T-0001", "2026-11-10T00:00:00Z"),
            item("WEB-T-0003", "2026-11-11T09:00:00Z"),
            archived,
            item("WEB-T-0002", "2026-11-11T09:00:00Z"),
        ]);
        let codes: Vec<&str> = out.iter().map(|w| w.short_code.as_str()).collect();
        assert_eq!(codes, vec!["WEB-T-0002", "WEB-T-0003", "WEB-T-0001"]);
    }

    #[test]
    fn a_row_names_the_place_and_the_change() {
        let task = item("WEB-T-0001", "2026-11-11T09:30:12Z");
        assert_eq!(place(&task).as_deref(), Some("Web / Active"));
        let mut document = item("WEB-D-0001", "2026-11-11T09:30:12Z");
        document.column = None;
        assert_eq!(place(&document).as_deref(), Some("Web"));
        document.board = None;
        assert_eq!(place(&document), None);
        assert_eq!(changed(&task.updated_at), "changed 2026-11-11 09:30 UTC");
    }

    #[test]
    fn the_answer_of_the_server_parses() {
        let list: WatchList = serde_json::from_value(serde_json::json!({
            "items": [{
                "short_code": "WEB-T-0001", "entity_type": "task", "title": "T",
                "archived": false, "with_children": false,
                "created_at": "2026-11-10T00:00:00Z",
                "board": "Web", "column": "Active", "claim": "Robin",
                "updated_at": "2026-11-11T00:00:00Z", "state": "done"
            }]
        }))
        .expect("parses");
        assert_eq!(list.items[0].claim.as_deref(), Some("Robin"));
        assert_eq!(rows(&list.items).len(), 1);
        assert_eq!(columns_of(&list.items)[3].len(), 1);
    }
}
