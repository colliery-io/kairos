//! The notifications of the signed-in person in the GUI (KAIROS-T-0383):
//!
//! - [`NotificationBell`]: a bell in the top bar of the shell with the
//!   unread count. The count changes live: the bell listens on
//!   `/ws/events` for the events of the person (`notification_created`,
//!   `notifications_read`), which the server sends to the sockets of that
//!   person only. A click opens a drawer with the list.
//! - `/notifications` ([`NotificationsPage`]): the list, and the settings
//!   that turn a kind off.
//!
//! Each row links to its item and has "Mark read". "Mark all read" marks
//! the newest unread one of the list and each older one (`up_to`), so a
//! notification that came after the list stays unread (KAIROS-T-0550).
//! A notification marked read here is read for the person only: each agent
//! key of the person keeps its own read state.
//!
//! A notice from an agent key (KAIROS-T-0737) names the key on its row,
//! with "Mute this key": after it, a notice from the key makes no
//! notification for the person. The settings list the muted keys, each with
//! "Unmute".
//!
//! The page `/notifications` has "Show older" (KAIROS-T-0584). The first
//! read gives up to 100 unread notifications and the 20 newest read ones.
//! "Show older" adds the next page: the older unread ones (`before`), and
//! after them the older read ones (`read_before`). A mark or an event of
//! the person reads the first page again.

use aurora_dark::components::{
    Anchor, Button, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Stack,
    Switch, Text,
};
use aurora_dark::frame::Drawer;
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;

use super::admin::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::{Auth, use_auth};

/// mirror of: `kairos_client::types_notifications::Notification`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Notification {
    pub id: String,
    pub kind: String,
    pub short_code: Option<String>,
    pub text: String,
    pub created_at: String,
    pub read_at: Option<String>,
    #[serde(default)]
    pub agent_key_id: Option<String>,
    #[serde(default)]
    pub agent_key_name: Option<String>,
}

/// mirror of: `kairos_client::types_notifications::NotificationList`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NotificationList {
    pub unread: i64,
    pub items: Vec<Notification>,
}

/// mirror of: `kairos_client::types_notifications::NotificationSettings`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct NotificationSettings {
    hand_off: bool,
    blocker_cleared: bool,
    blocked_again: bool,
    cancelled: bool,
    blocker_cancelled: bool,
    mention: bool,
    p0_bug: bool,
    delivery_failing: bool,
    agent_key_revoked: bool,
    claim_dropped: bool,
    claim_stale: bool,
    key_expiring: bool,
    proposal_decided: bool,
    notice: bool,
    #[serde(default)]
    muted_keys: Vec<MutedKey>,
}

/// mirror of: `kairos_client::types_notifications::MutedKey` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct MutedKey {
    id: String,
    name: String,
    prefix: String,
    owner_name: String,
}

/// The body of a mute or an unmute of one agent key (KAIROS-T-0737).
/// Pure, host-tested.
fn mute_body(key_id: &str, mute: bool) -> serde_json::Value {
    if mute {
        json!({ "mute_keys": [key_id] })
    } else {
        json!({ "unmute_keys": [key_id] })
    }
}

async fn set_mute(auth: Auth, key_id: String, mute: bool) -> Result<(), ApiError> {
    crate::api::put_json::<_, NotificationSettings>(auth, SETTINGS, &mute_body(&key_id, mute))
        .await
        .map(|_| ())
}

/// mirror of: `kairos_client::types_notifications::MarkNotificationsReadResponse`
/// (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Marked {
    unread: i64,
}

const LIST: &str = "/api/me/notifications";
const READ: &str = "/api/me/notifications/read";
const SETTINGS: &str = "/api/me/notification-settings";

async fn list(auth: Auth) -> Result<NotificationList, ApiError> {
    crate::api::get_json(auth, LIST).await
}

/// The most unread notifications in a page (mirror of
/// `kairos_server::api::notifications::LIST_UNREAD`).
const UNREAD_PAGE: usize = 100;
/// The read notifications in the first page (mirror of `LIST_READ`).
const READ_FIRST: usize = 20;
/// The most read notifications in a page of `read_before` (mirror of
/// `LIST_READ_PAGE`).
const READ_PAGE: usize = 100;

/// The cursor of the next page (KAIROS-T-0584).
#[derive(Clone, Debug, PartialEq)]
enum Cursor {
    /// The unread notifications older than this id.
    Before(String),
    /// The read notifications read before this id.
    ReadBefore(String),
}

impl Cursor {
    /// The URL of the page. Pure, host-tested.
    fn url(&self) -> String {
        match self {
            Cursor::Before(id) => format!("{LIST}?before={}", crate::api::encode_component(id)),
            Cursor::ReadBefore(id) => {
                format!("{LIST}?read_before={}", crate::api::encode_component(id))
            }
        }
    }
}

/// The notifications that the page shows: each page that the person read,
/// the unread ones first (KAIROS-T-0584).
#[derive(Clone, Debug, Default, PartialEq)]
struct Pages {
    unread: Vec<Notification>,
    read: Vec<Notification>,
    /// The last page of unread ones was full: an older one can be there.
    unread_full: bool,
    /// The last page of read ones was full.
    read_full: bool,
}

impl Pages {
    /// The pages after the first read. Pure, host-tested.
    fn first(list: &NotificationList) -> Self {
        let (unread, read): (Vec<_>, Vec<_>) = list
            .items
            .iter()
            .cloned()
            .partition(|n| n.read_at.is_none());
        Pages {
            unread_full: unread.len() >= UNREAD_PAGE,
            read_full: read.len() >= READ_FIRST,
            unread,
            read,
        }
    }

    /// The cursor of the next page: the older unread ones first, then the
    /// older read ones. `None`: the page shows each notification. Pure,
    /// host-tested.
    fn next(&self) -> Option<Cursor> {
        if self.unread_full {
            return self.unread.last().map(|n| Cursor::Before(n.id.clone()));
        }
        if self.read_full {
            return self.read.last().map(|n| Cursor::ReadBefore(n.id.clone()));
        }
        None
    }

    /// Add the page of `cursor`. Pure, host-tested.
    fn add(&mut self, cursor: &Cursor, page: NotificationList) {
        match cursor {
            Cursor::Before(_) => {
                self.unread_full = page.items.len() >= UNREAD_PAGE;
                self.unread.extend(page.items);
            }
            Cursor::ReadBefore(_) => {
                self.read_full = page.items.len() >= READ_PAGE;
                self.read.extend(page.items);
            }
        }
    }

    /// Each notification of the pages: the unread ones, then the read ones.
    fn items(&self) -> Vec<Notification> {
        self.unread.iter().chain(&self.read).cloned().collect()
    }
}

/// What a mark sends: one id, or all up to the newest unread id of the
/// list (KAIROS-T-0550).
#[derive(Clone, Debug, PartialEq)]
enum MarkWhat {
    One(String),
    AllUpTo(String),
}

/// The body of a mark. Pure, host-tested.
fn mark_body(what: &MarkWhat) -> serde_json::Value {
    match what {
        MarkWhat::One(id) => json!({ "ids": [id] }),
        MarkWhat::AllUpTo(id) => json!({ "all": true, "up_to": id }),
    }
}

/// The newest unread notification of a list: the `up_to` of "Mark all
/// read". The list has the unread ones first, newest first. Pure,
/// host-tested.
fn newest_unread(list: &NotificationList) -> Option<String> {
    list.items
        .iter()
        .find(|n| n.read_at.is_none())
        .map(|n| n.id.clone())
}

async fn mark(auth: Auth, what: MarkWhat) -> Result<Marked, ApiError> {
    crate::api::post_json(auth, READ, &mark_body(&what)).await
}

/// The words of a kind. Pure, host-tested.
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "hand_off" => "Hand-off",
        "blocker_cleared" => "Blocker cleared",
        "blocked_again" => "Blocked again",
        "cancelled" => "Cancelled",
        "blocker_cancelled" => "Blocker cancelled",
        "mention" => "Mention",
        "p0_bug" => "P0 bug",
        "delivery_failing" => "Delivery failing",
        "agent_key_revoked" => "Agent key revoked",
        "claim_dropped" => "Claim dropped",
        "claim_stale" => "Claim stale",
        "key_expiring" => "Key expiring",
        "proposal_decided" => "Proposal decided",
        "notice" => "Notice",
        _ => "Notification",
    }
}

/// `2026-10-12T09:15:00Z` → `2026-10-12 09:15 UTC`. Pure, host-tested.
fn when(value: &str) -> String {
    match value.split_once('T') {
        Some((date, time)) => format!("{date} {} UTC", time.get(..5).unwrap_or(time)),
        None => value.to_string(),
    }
}

/// The text of the count of the bell: empty for 0, `99+` above 99. Pure,
/// host-tested.
fn count_text(unread: i64) -> String {
    match unread {
        n if n <= 0 => String::new(),
        n if n > 99 => "99+".to_string(),
        n => n.to_string(),
    }
}

/// A bell, drawn with the token colors.
#[component]
fn BellIcon() -> impl IntoView {
    view! {
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
                d="M6 16 V11 a6 6 0 0 1 12 0 V16 l1.5 2 H4.5 Z"
                style="stroke:var(--ice);"
                stroke-width="1.8"
                stroke-linejoin="round"
            />
            <path d="M10 20.5 a2 2 0 0 0 4 0" style="stroke:var(--ice);" stroke-width="1.8"/>
        </svg>
    }
}

/// The rows of a list of notifications, with "Mark read" on each unread
/// row. `on_marked` runs after a mark, with the error when it failed.
#[component]
fn NotificationRows(
    items: Vec<Notification>,
    busy: RwSignal<bool>,
    on_marked: Callback<Result<(), ApiError>>,
) -> impl IntoView {
    let auth = use_auth();
    if items.is_empty() {
        return view! { <Empty message="You have no notifications."/> }.into_any();
    }
    items
        .into_iter()
        .map(|n| {
            let id = StoredValue::new(n.id.clone());
            let unread = n.read_at.is_none();
            let key = n.agent_key_id.clone().map(|key_id| {
                let name = n.agent_key_name.clone().unwrap_or_else(|| key_id.clone());
                (key_id, name)
            });
            let on_mute = move |key_id: String| {
                if busy.get_untracked() {
                    return;
                }
                busy.set(true);
                leptos::task::spawn_local(async move {
                    let result = set_mute(auth, key_id, true).await;
                    busy.set(false);
                    on_marked.run(result);
                });
            };
            let on_mark = move |_| {
                if busy.get_untracked() {
                    return;
                }
                busy.set(true);
                let id = id.get_value();
                leptos::task::spawn_local(async move {
                    let result = mark(auth, MarkWhat::One(id)).await.map(|_| ());
                    busy.set(false);
                    on_marked.run(result);
                });
            };
            let kind = n.kind.clone();
            let label = kind_label(&n.kind);
            let key_pill = key.clone().map(|(_, name)| {
                view! {
                    <span data-testid="notice-agent-key">
                        <Pill color=token::MUTED>{format!("Agent key {name}")}</Pill>
                    </span>
                }
            });
            let mute_button = key.filter(|_| n.kind == "notice").map(|(key_id, _)| {
                let key_id = StoredValue::new(key_id);
                view! {
                    <Button variant="subtle" size="xs"
                        title="A notice from this agent key makes no notification for you"
                        attr:data-testid="notice-mute"
                        on_click=Callback::new(move |_| on_mute(key_id.get_value()))>
                        "Mute this key"
                    </Button>
                }
            });
            view! {
                <div data-testid="notification" data-unread=unread.to_string()
                    data-kind=kind>
                    <Stack gap="xs">
                        <Group justify="between" wrap=true>
                            <Group gap="sm" wrap=true>
                                <Pill color=if unread { token::GOLD } else { token::MUTED }>
                                    {label}
                                </Pill>
                                {n.short_code.clone().map(|code| view! {
                                    <Anchor href=format!("/items/{code}")>{code.clone()}</Anchor>
                                })}
                                <Text dimmed=true size="sm">{when(&n.created_at)}</Text>
                                {key_pill}
                            </Group>
                            <Group gap="xs">
                                {mute_button}
                                {unread.then(|| view! {
                                    <Button variant="default" size="xs"
                                        on_click=Callback::new(on_mark)>
                                        "Mark read"
                                    </Button>
                                })}
                            </Group>
                        </Group>
                        <Text size="sm" bright=unread>{n.text.clone()}</Text>
                        <Divider/>
                    </Stack>
                </div>
            }
        })
        .collect_view()
        .into_any()
}

/// The bell of the shell: the unread count, live, and a drawer with the
/// list.
#[component]
pub fn NotificationBell() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let notifications = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        list(auth)
    });

    // The count changes live: each event of the person reads the list
    // again, and so does each reconnect.
    let guard = super::boards::live::subscribe_person_events(auth, move || {
        reload.try_update(|n| *n += 1);
    });
    let live_guard: StoredValue<Option<super::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    let unread = Signal::derive(move || match notifications.get() {
        Some(Ok(list)) => list.unread,
        _ => 0,
    });
    let on_marked = Callback::new(move |result: Result<(), ApiError>| {
        error.set(result.err());
        reload.update(|n| *n += 1);
    });
    let on_mark_all = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Some(Ok(list)) = notifications.get_untracked() else {
            return;
        };
        let Some(up_to) = newest_unread(&list) else {
            return;
        };
        busy.set(true);
        leptos::task::spawn_local(async move {
            let result = mark(auth, MarkWhat::AllUpTo(up_to)).await.map(|_| ());
            busy.set(false);
            on_marked.run(result);
        });
    };

    view! {
        <span data-testid="notification-bell">
            <Button
                variant="subtle"
                size="xs"
                aria_label="Notifications"
                title="Notifications"
                on_click=Callback::new(move |_| open.set(true))
            >
                <Group gap="xs">
                    <BellIcon/>
                    {move || {
                        let text = count_text(unread.get());
                        (!text.is_empty()).then(|| view! {
                            <span data-testid="notification-count">
                                <Pill color=token::GOLD>{text}</Pill>
                            </span>
                        })
                    }}
                </Group>
            </Button>
        </span>
        <Drawer open=open title="Notifications">
            <div data-testid="notification-drawer">
                <Stack gap="sm">
                    <Group justify="between" wrap=true>
                        <Text size="sm" dimmed=true>
                            {move || format!("{} unread", unread.get())}
                        </Text>
                        <Group gap="sm">
                            <Button variant="default" size="xs"
                                disabled=Signal::derive(move || unread.get() == 0)
                                on_click=Callback::new(on_mark_all)>
                                "Mark all read"
                            </Button>
                            <Button variant="subtle" size="xs" href="/notifications"
                                on_click=Callback::new(move |_| open.set(false))>
                                "Settings"
                            </Button>
                        </Group>
                    </Group>
                    {move || error.get().map(|error| view! { <ErrorState error/> })}
                    {move || match notifications.get() {
                        None => view! { <Loading/> }.into_any(),
                        Some(Err(error)) => view! {
                            <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                        }.into_any(),
                        Some(Ok(list)) => view! {
                            <NotificationRows items=list.items busy on_marked/>
                        }.into_any(),
                    }}
                </Stack>
            </div>
        </Drawer>
    }
}

/// `/notifications`: the list, and the settings of the kinds.
#[component]
pub fn NotificationsPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let notifications = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        list(auth)
    });
    let guard = super::boards::live::subscribe_person_events(auth, move || {
        reload.try_update(|n| *n += 1);
    });
    let live_guard: StoredValue<Option<super::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    // KAIROS-T-0584: the pages that the person read. A new first page
    // starts them again.
    let pages = RwSignal::new(Pages::default());
    let older_busy = RwSignal::new(false);
    let older_error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    Effect::new(move |_| {
        if let Some(Ok(list)) = notifications.get() {
            pages.set(Pages::first(&list));
            older_error.set(None);
        }
    });
    let on_older = move |_| {
        if older_busy.get_untracked() {
            return;
        }
        let Some(cursor) = pages.with_untracked(Pages::next) else {
            return;
        };
        older_busy.set(true);
        leptos::task::spawn_local(async move {
            let page: Result<NotificationList, ApiError> =
                crate::api::get_json(auth, &cursor.url()).await;
            older_busy.set(false);
            match page {
                Ok(page) => pages.update(|pages| pages.add(&cursor, page)),
                Err(error) => older_error.set(Some(error)),
            }
        });
    };

    // The switches of the settings, filled when the settings arrive.
    let hand_off = RwSignal::new(true);
    let blocker_cleared = RwSignal::new(true);
    let blocked_again = RwSignal::new(true);
    let cancelled = RwSignal::new(true);
    let blocker_cancelled = RwSignal::new(true);
    let mention = RwSignal::new(true);
    let p0_bug = RwSignal::new(true);
    let delivery_failing = RwSignal::new(true);
    let agent_key_revoked = RwSignal::new(true);
    let claim_dropped = RwSignal::new(true);
    let claim_stale = RwSignal::new(true);
    let key_expiring = RwSignal::new(true);
    let proposal_decided = RwSignal::new(true);
    let notice = RwSignal::new(true);
    let settings_reload = RwSignal::new(0u32);
    let settings = LocalResource::new(move || {
        auth.track_session();
        settings_reload.get();
        async move { crate::api::get_json::<NotificationSettings>(auth, SETTINGS).await }
    });
    Effect::new(move |_| {
        if let Some(Ok(s)) = settings.get() {
            hand_off.set(s.hand_off);
            blocker_cleared.set(s.blocker_cleared);
            blocked_again.set(s.blocked_again);
            cancelled.set(s.cancelled);
            blocker_cancelled.set(s.blocker_cancelled);
            mention.set(s.mention);
            p0_bug.set(s.p0_bug);
            delivery_failing.set(s.delivery_failing);
            agent_key_revoked.set(s.agent_key_revoked);
            claim_dropped.set(s.claim_dropped);
            claim_stale.set(s.claim_stale);
            key_expiring.set(s.key_expiring);
            proposal_decided.set(s.proposal_decided);
            notice.set(s.notice);
        }
    });
    let on_save = move |_| {
        let body = json!({
            "hand_off": hand_off.get_untracked(),
            "blocker_cleared": blocker_cleared.get_untracked(),
            "blocked_again": blocked_again.get_untracked(),
            "cancelled": cancelled.get_untracked(),
            "blocker_cancelled": blocker_cancelled.get_untracked(),
            "mention": mention.get_untracked(),
            "p0_bug": p0_bug.get_untracked(),
            "delivery_failing": delivery_failing.get_untracked(),
            "agent_key_revoked": agent_key_revoked.get_untracked(),
            "claim_dropped": claim_dropped.get_untracked(),
            "claim_stale": claim_stale.get_untracked(),
            "key_expiring": key_expiring.get_untracked(),
            "proposal_decided": proposal_decided.get_untracked(),
            "notice": notice.get_untracked(),
        });
        run_mutation(
            busy,
            outcome,
            settings_reload,
            "Kairos saved your notification settings.".to_string(),
            async move {
                crate::api::put_json::<_, NotificationSettings>(auth, SETTINGS, &body)
                    .await
                    .map(|_| ())
            },
        );
    };
    let on_marked = Callback::new(move |result: Result<(), ApiError>| {
        if let Err(error) = result {
            outcome.set(Some(Err(error)));
        }
        reload.update(|n| *n += 1);
    });
    let on_mark_all = move |_| {
        let Some(Ok(list)) = notifications.get_untracked() else {
            return;
        };
        let Some(up_to) = newest_unread(&list) else {
            return;
        };
        run_mutation(
            busy,
            outcome,
            reload,
            "Kairos marked each notification of the list read.".to_string(),
            async move { mark(auth, MarkWhat::AllUpTo(up_to)).await.map(|_| ()) },
        );
    };

    view! {
        <PageHeader title="Notifications" sub="Kairos tells you when a person hands off a task \
                                               to you, when a blocker of your task clears, when \
                                               a person cancels a task that you created, when a \
                                               person writes your user name, when a P0 bug \
                                               lands on the board of your team, when Kairos \
                                               turns off a webhook target that you own, and \
                                               when a person or an agent sends you a notice."/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="Your notifications"
                caption="A notification that you mark read here is read for you. Each agent key keeps its own read state.">
                <Stack gap="sm">
                    <Group justify="end">
                        <Button variant="default" size="xs" on_click=Callback::new(on_mark_all)>
                            "Mark all read"
                        </Button>
                    </Group>
                    {move || match notifications.get() {
                        None => view! { <Loading/> }.into_any(),
                        Some(Err(error)) => view! {
                            <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                        }.into_any(),
                        Some(Ok(list)) => {
                            // Before the effect fills the pages, the first
                            // read is the list.
                            let items = pages.with(|p| {
                                if *p == Pages::default() {
                                    Pages::first(&list).items()
                                } else {
                                    p.items()
                                }
                            });
                            view! { <NotificationRows items busy on_marked/> }.into_any()
                        }
                    }}
                    {move || older_error.get().map(|error| view! { <ErrorState error/> })}
                    {move || pages.with(|p| p.next().is_some()).then(|| view! {
                        <Group justify="center">
                            <Button variant="default" size="xs"
                                disabled=Signal::derive(move || older_busy.get())
                                on_click=Callback::new(on_older)
                                attr:data-testid="notifications-older">
                                "Show older"
                            </Button>
                        </Group>
                    })}
                </Stack>
            </Panel>
            <Panel title="Settings"
                caption="Turn a kind off, and Kairos makes no notification of that kind for you.">
                {move || match settings.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                    Some(Ok(s)) => view! {
                        <div data-testid="notification-settings">
                            <Stack gap="sm">
                                <Switch checked=hand_off label="A person hands off a task to you"/>
                                <Switch checked=blocker_cleared
                                    label="A blocker of your claimed task is done, archived or unlinked"/>
                                <Switch checked=blocked_again
                                    label="A done blocker of your task is open again"/>
                                <Switch checked=cancelled
                                    label="A person cancels a task that you created"/>
                                <Switch checked=blocker_cancelled
                                    label="A person cancels a blocker of your task"/>
                                <Switch checked=mention label="A person writes your user name"/>
                                <Switch checked=p0_bug label="A P0 bug lands on the board of your team"/>
                                <Switch checked=delivery_failing
                                    label="Kairos turns off a webhook target that you own after failed deliveries"/>
                                <Switch checked=agent_key_revoked
                                    label="An organization admin revokes an agent key of yours"/>
                                <Switch checked=claim_dropped
                                    label="A person leaves, and their claim on a task of your team ends"/>
                                <Switch checked=claim_stale
                                    label="A claim of yours, or on a task of your initiative, has no write for days"/>
                                <Switch checked=key_expiring
                                    label="An agent key of yours, or a key of a service account, expires within 7 days"/>
                                <Switch checked=proposal_decided
                                    label="A person confirms or rejects an edge proposal that you made"/>
                                <Switch checked=notice
                                    label="A person or an agent key sends a notice to you or to your team"/>
                                <MutedKeys keys=s.muted_keys busy outcome settings_reload/>
                                <Group justify="end">
                                    <Button on_click=Callback::new(on_save)>"Save"</Button>
                                </Group>
                            </Stack>
                        </div>
                    }.into_any(),
                }}
            </Panel>
        </Stack>
    }
}

/// The agent keys that the person muted (KAIROS-T-0737), each with
/// "Unmute".
#[component]
fn MutedKeys(
    keys: Vec<MutedKey>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    settings_reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    if keys.is_empty() {
        return view! {
            <Text size="sm" dimmed=true>"You muted no agent key."</Text>
        }
        .into_any();
    }
    let rows = keys
        .into_iter()
        .map(|key| {
            let id = StoredValue::new(key.id.clone());
            let on_unmute = move |_| {
                let id = id.get_value();
                run_mutation(
                    busy,
                    outcome,
                    settings_reload,
                    "Kairos unmuted the agent key.".to_string(),
                    async move { set_mute(auth, id, false).await },
                );
            };
            view! {
                <div data-testid="muted-key">
                    <Group justify="between" wrap=true>
                        <Text size="sm">
                            {format!("{} ({}) of {}", key.name, key.prefix, key.owner_name)}
                        </Text>
                        <Button variant="default" size="xs"
                            attr:data-testid="unmute-key"
                            on_click=Callback::new(on_unmute)>
                            "Unmute"
                        </Button>
                    </Group>
                </div>
            }
        })
        .collect_view();
    view! {
        <Stack gap="xs">
            <Text size="sm" dimmed=true>
                "Muted agent keys: a notice from one of these keys makes no notification for you."
            </Text>
            {rows}
        </Stack>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{
        Cursor, MarkWhat, Notification, NotificationList, Pages, READ_FIRST, READ_PAGE,
        UNREAD_PAGE, count_text, kind_label, mark_body, mute_body, newest_unread, when,
    };

    fn note(id: String, read: bool) -> Notification {
        Notification {
            id,
            kind: "mention".into(),
            short_code: None,
            text: "t".into(),
            created_at: "2026-10-17T09:00:00Z".into(),
            read_at: read.then(|| "2026-10-17T10:00:00Z".into()),
            agent_key_id: None,
            agent_key_name: None,
        }
    }

    fn page(prefix: &str, count: usize, read: bool) -> NotificationList {
        NotificationList {
            unread: 0,
            items: (0..count)
                .map(|n| note(format!("{prefix}{n}"), read))
                .collect(),
        }
    }

    /// KAIROS-T-0584: "Show older" reads the older unread pages first, then
    /// the older read pages, and stops at the last page.
    #[test]
    fn show_older_goes_through_the_unread_then_the_read_pages() {
        let mut first = page("u", UNREAD_PAGE, false);
        first.items.extend(page("r", READ_FIRST, true).items);
        let mut pages = Pages::first(&first);
        let next = pages.next().expect("a full unread page: more");
        assert_eq!(next, Cursor::Before("u99".into()));
        assert_eq!(next.url(), "/api/me/notifications?before=u99");
        pages.add(&next, page("v", 3, false));
        let next = pages.next().expect("the read ones are next");
        assert_eq!(next, Cursor::ReadBefore("r19".into()));
        assert_eq!(next.url(), "/api/me/notifications?read_before=r19");
        pages.add(&next, page("s", READ_PAGE, true));
        let next = pages.next().expect("a full read page: more");
        assert_eq!(next, Cursor::ReadBefore(format!("s{}", READ_PAGE - 1)));
        pages.add(&next, page("t", 4, true));
        assert_eq!(pages.next(), None, "the last page");
        let items = pages.items();
        assert_eq!(items.len(), UNREAD_PAGE + 3 + READ_FIRST + READ_PAGE + 4);
        assert_eq!(items[UNREAD_PAGE + 2].id, "v2", "the unread ones first");
        assert!(items[UNREAD_PAGE + 3].read_at.is_some());
    }

    #[test]
    fn a_short_first_page_has_no_older_page() {
        let mut first = page("u", 2, false);
        first.items.extend(page("r", 5, true).items);
        assert_eq!(Pages::first(&first).next(), None);
    }

    #[test]
    fn the_body_marks_one_or_all_up_to_the_newest() {
        assert_eq!(
            mark_body(&MarkWhat::One("x".into())),
            serde_json::json!({"ids": ["x"]})
        );
        assert_eq!(
            mark_body(&MarkWhat::AllUpTo("y".into())),
            serde_json::json!({"all": true, "up_to": "y"})
        );
    }

    #[test]
    fn mark_all_takes_the_newest_unread_id() {
        let row = |id: &str, read: bool| Notification {
            id: id.into(),
            kind: "mention".into(),
            short_code: None,
            text: "t".into(),
            created_at: "2026-10-17T09:00:00Z".into(),
            read_at: read.then(|| "2026-10-17T10:00:00Z".into()),
            agent_key_id: None,
            agent_key_name: None,
        };
        let list = NotificationList {
            unread: 2,
            items: vec![row("a", false), row("b", false), row("c", true)],
        };
        assert_eq!(newest_unread(&list).as_deref(), Some("a"));
        let read = NotificationList {
            unread: 0,
            items: vec![row("c", true)],
        };
        assert_eq!(newest_unread(&read), None);
    }

    #[test]
    fn a_mute_and_an_unmute_name_one_key() {
        assert_eq!(
            mute_body("k1", true),
            serde_json::json!({ "mute_keys": ["k1"] })
        );
        assert_eq!(
            mute_body("k1", false),
            serde_json::json!({ "unmute_keys": ["k1"] })
        );
    }

    #[test]
    fn each_kind_has_words() {
        for kind in [
            "hand_off",
            "blocker_cleared",
            "blocked_again",
            "cancelled",
            "blocker_cancelled",
            "mention",
            "p0_bug",
            "delivery_failing",
            "agent_key_revoked",
            "claim_dropped",
            "claim_stale",
            "key_expiring",
            "proposal_decided",
            "notice",
        ] {
            assert_ne!(kind_label(kind), "Notification", "{kind}");
        }
        assert_eq!(kind_label("other"), "Notification");
    }

    #[test]
    fn the_count_and_the_time_read_short() {
        assert_eq!(count_text(0), "");
        assert_eq!(count_text(7), "7");
        assert_eq!(count_text(150), "99+");
        assert_eq!(when("2026-10-12T09:15:00Z"), "2026-10-12 09:15 UTC");
    }
}
