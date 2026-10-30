//! The Notes panel of the item page (KAIROS-T-0579), on the notes rows of
//! KAIROS-T-0630 (KAIROS-A-0054).
//!
//! - The list: each note with its id as an anchor (`#n-3`), its date and
//!   author, its text as markdown, and the item that a promote made from
//!   it. The note of the URL hash has a highlight, and the page scrolls to
//!   it when the notes render.
//! - "Add note": `POST /api/{family}/{code}/notes`. The page then reads the
//!   item again, and the new row shows. A note of another writer comes
//!   with the live read of `note_appended` (the page takes the notes at
//!   once: [`takes_notes`]).
//! - "Promote": `POST /api/{family}/{code}/notes/{id}/promote` with a kind,
//!   a title and a target board. The form reads the boards only when it
//!   opens (KAIROS-T-0618). A CONFLICT shows the message of the server.
//!
//! Both writes need the edit rule of the item: a person who may not edit
//! sees the reason, and no control.

use aurora_dark::components::{
    Alert, Anchor, Button, Group, Panel, Select, Stack, Text, TextInput, Textarea,
};
use aurora_dark::frame::use_toaster;
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::use_location;
use serde::{Deserialize, Serialize};

use super::api::{self, Family, Note};
use super::markdown;
use super::owner::owner_board_targets;
use crate::auth::use_auth;
use crate::pages::boards;

/// The kinds that a note can become, as `(value, label)`.
const KINDS: [(&str, &str); 2] = [("adr", "ADR"), ("document", "Document")];

/// mirror of: `kairos_client::types::AppendNoteRequest`.
#[derive(Serialize)]
struct AppendBody {
    text: String,
}

/// mirror of: `kairos_client::types::AppendNoteResponse` (partial).
#[derive(Deserialize)]
struct Appended {
    #[serde(default)]
    note_id: String,
}

/// mirror of: `kairos_client::types::PromoteNoteRequest`.
#[derive(Serialize)]
struct PromoteBody {
    kind: String,
    board: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

/// mirror of: `kairos_client::types::PromoteNoteResponse` (partial).
#[derive(Deserialize)]
struct Promoted {
    note_id: String,
    promoted_to: String,
}

/// The path of the append. Pure, host-tested.
pub(super) fn notes_path(family: Family, code: &str) -> String {
    format!("/api/{}/{code}/notes", family.api_family())
}

/// The path of a promote. Pure, host-tested.
pub(super) fn promote_path(family: Family, code: &str, note_id: &str) -> String {
    format!("{}/{note_id}/promote", notes_path(family, code))
}

/// The id of the note that the URL hash names: `#n-3` → `n-3`. Any other
/// hash names no note. Pure, host-tested.
pub(super) fn note_target(hash: &str) -> Option<&str> {
    let id = hash.strip_prefix('#')?;
    let digits = id.strip_prefix("n-")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The text that "Add note" sends: the draft without the spaces at its
/// ends. `None` for a draft with no text. Pure, host-tested.
pub(super) fn note_text(draft: &str) -> Option<String> {
    let text = draft.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The title that a promote sends. `None`: the server takes the first
/// line of the note. Pure, host-tested.
pub(super) fn promote_title(input: &str) -> Option<String> {
    note_text(input)
}

/// The boards that can get the new item, as `(slug, name)`: an ADR board
/// with `manage_adrs` for an ADR, a board with `manage_documents` for a
/// document (its owner board). Pure, host-tested.
pub(super) fn promote_targets(
    me: &crate::api::Whoami,
    list: &[boards::data::Board],
    kind: &str,
) -> Vec<(String, String)> {
    if kind == "document" {
        return owner_board_targets(me, list, None);
    }
    list.iter()
        .filter(|board| board.board_level == "adr")
        .filter(|board| {
            boards::holds_capability(
                me,
                Some(&board.slug),
                board.team_id.as_deref(),
                "manage_adrs",
            )
        })
        .map(|board| (board.slug.clone(), board.name.clone()))
        .collect()
}

/// Does the page take the notes of a live read at once? Notes only grow,
/// and no draft holds them, so the panel takes a new list. It does not take
/// a shorter list: that read is older than the list on the page. Pure,
/// host-tested.
pub(super) fn takes_notes(shown: Option<&[Note]>, read: Option<&[Note]>) -> bool {
    match (shown, read) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(shown), Some(read)) => read.len() >= shown.len() && read != shown,
    }
}

/// The line next to the id of a note: `2026-10-17 · Robin`. Pure,
/// host-tested.
pub(super) fn note_byline(created_at: &str, author: &str) -> String {
    let date = created_at.get(..10).unwrap_or(created_at);
    format!("{date} · {author}")
}

/// Is the refusal a CONFLICT (409)?
fn is_conflict(error: &ApiError) -> bool {
    matches!(error, ApiError::Http { status: 409, .. })
}

/// The Notes panel. `on_changed` reads the item again after a write.
#[component]
pub(super) fn NotesPanel(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] notes: Signal<Vec<Note>>,
    #[prop(into)] read_only: Signal<bool>,
    #[prop(into)] read_only_reason: Signal<String>,
    on_changed: Callback<()>,
    /// The terms of the dictionary of the team (KAIROS-T-0734): each note
    /// links them, with the meaning on hover.
    #[prop(into)]
    terms: Signal<Vec<markdown::TermLink>>,
) -> impl IntoView {
    let code = StoredValue::new(code);
    let hash = use_location().hash;
    let target = Memo::new(move |_| hash.with(|hash| note_target(hash).map(str::to_string)));
    // The router scrolls to the hash before the notes render. Scroll once
    // when the note of the hash is on the page.
    let scrolled = StoredValue::new(None::<String>);
    Effect::new(move |_| {
        let Some(id) = target.get() else {
            return;
        };
        let present = notes.with(|notes| notes.iter().any(|note| note.id == id));
        if !present || scrolled.with_value(|done| done.as_deref() == Some(id.as_str())) {
            return;
        }
        scrolled.set_value(Some(id.clone()));
        request_animation_frame(move || {
            if let Some(element) = document().get_element_by_id(&id) {
                element.scroll_into_view();
            }
        });
    });
    // The note whose promote form is open.
    let promoting = RwSignal::new(None::<String>);

    let row = move |note: Note| {
        let id = StoredValue::new(note.id.clone());
        let promoted = Memo::new(move |_| {
            notes.with(|notes| {
                id.with_value(|id| {
                    notes
                        .iter()
                        .find(|note| note.id == *id)
                        .and_then(|note| note.promoted_to.clone())
                })
            })
        });
        let is_target = move || target.with(|t| id.with_value(|id| t.as_ref() == Some(id)));
        let form_open =
            move || promoting.with(|open| id.with_value(|id| open.as_ref() == Some(id)));
        let byline = note_byline(&note.created_at, &note.author);
        let text = note.text.clone();
        let html = move || terms.with(|terms| markdown::to_html_with_terms(&text, terms));
        let href = format!("#{}", note.id);
        let label = note.id.clone();
        view! {
            <section
                class="kairos-note"
                class:kairos-note--target=is_target
                id=note.id
                data-testid="item-note"
            >
                <Group justify="between">
                    <Group gap="sm">
                        <a class="kairos-note-link" href=href>
                            <Text mono=true size="sm">{label}</Text>
                        </a>
                        <Text dimmed=true size="xs">{byline}</Text>
                    </Group>
                    {move || match promoted.get() {
                        Some(made) => view! {
                            <Text size="xs">
                                "Promoted to "
                                <Anchor href=format!("/items/{made}")>{made.clone()}</Anchor>
                            </Text>
                        }.into_any(),
                        None if !read_only.get() && !form_open() => view! {
                            <Button variant="default" size="xs"
                                on_click=Callback::new(move |_| promoting.set(Some(id.get_value())))>
                                "Promote"
                            </Button>
                        }.into_any(),
                        None => ().into_any(),
                    }}
                </Group>
                <div class="kairos-markdown" inner_html=html></div>
                {move || form_open().then(|| view! {
                    <PromoteForm
                        family
                        code=code.get_value()
                        note_id=id.get_value()
                        on_close=Callback::new(move |_| promoting.set(None))
                        on_changed
                    />
                })}
            </section>
        }
    };

    view! {
        <Panel title="Notes" caption="the oldest note first">
            <Stack gap="sm">
                {move || notes.with(Vec::is_empty).then(|| view! {
                    <Text size="sm" dimmed=true>"This item has no notes."</Text>
                })}
                <For each=move || notes.get() key=|note| note.id.clone() children=row/>
                {move || if read_only.get() {
                    view! { <Text size="xs" dimmed=true>{read_only_reason.get()}</Text> }.into_any()
                } else {
                    view! { <AddNote family code=code.get_value() on_changed/> }.into_any()
                }}
            </Stack>
        </Panel>
    }
}

/// The "Add note" box.
#[component]
fn AddNote(family: Family, #[prop(into)] code: String, on_changed: Callback<()>) -> impl IntoView {
    let auth = use_auth();
    let toaster = use_toaster();
    let path = StoredValue::new(notes_path(family, &code));
    let draft = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let add = move |_: ()| {
        if busy.get_untracked() {
            return;
        }
        let Some(text) = note_text(&draft.get_untracked()) else {
            error.set(Some("Write the text of the note.".to_string()));
            return;
        };
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result: Result<Appended, ApiError> =
                crate::api::post_json(auth, &path.get_value(), &AppendBody { text }).await;
            let _ = busy.try_set(false);
            match result {
                Ok(added) => {
                    let _ = draft.try_set(String::new());
                    toaster.success(format!("Kairos added the note {}.", added.note_id));
                    on_changed.run(());
                }
                Err(e) => {
                    let _ = error.try_set(Some(api::error_text(&e)));
                }
            }
        });
    };
    view! {
        <Stack gap="xs">
            <Textarea
                label="New note"
                id="kairos-note-text"
                placeholder="Write the note in markdown."
                value=draft
                rows=3
            />
            <Text size="xs" dimmed=true>
                "A note does not change the content or its version."
            </Text>
            {move || error.get().map(|message| view! {
                <Alert title="Kairos did not add the note" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
            <Group>
                <Button size="xs" loading=busy loading_label="Adding…" on_click=Callback::new(add)>
                    "Add note"
                </Button>
            </Group>
        </Stack>
    }
}

/// The promote form of one note. It reads the boards when it opens.
#[component]
fn PromoteForm(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] note_id: String,
    on_close: Callback<()>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let toaster = use_toaster();
    let path = StoredValue::new(promote_path(family, &code, &note_id));
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let all_boards = LocalResource::new(move || {
        auth.track_session();
        boards::data::list_boards(auth)
    });
    let kind = RwSignal::new(KINDS[0].0.to_string());
    let board = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let targets: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(Ok(list))) = (me, all_boards.get()) else {
            return Vec::new();
        };
        kind.with(|kind| promote_targets(&me, &list, kind))
    });
    let kinds: Vec<(String, String)> = KINDS
        .iter()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect();
    let promote = move |_: ()| {
        if busy.get_untracked() {
            return;
        }
        let target = board.get_untracked();
        if target.is_empty() {
            error.set(Some("Select the target board.".to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        let body = PromoteBody {
            kind: kind.get_untracked(),
            board: target,
            title: promote_title(&title.get_untracked()),
        };
        leptos::task::spawn_local(async move {
            let result: Result<Promoted, ApiError> =
                crate::api::post_json(auth, &path.get_value(), &body).await;
            let _ = busy.try_set(false);
            match result {
                Ok(done) => {
                    toaster.success(format!(
                        "The note {} is now {}.",
                        done.note_id, done.promoted_to
                    ));
                    on_changed.run(());
                    on_close.run(());
                }
                Err(e) => {
                    // A CONFLICT: the note became an item already. Show it.
                    if is_conflict(&e) {
                        on_changed.run(());
                    }
                    let _ = error.try_set(Some(api::error_text(&e)));
                }
            }
        });
    };
    view! {
        <div class="kairos-note__promote" data-testid="note-promote">
            <Stack gap="xs">
                <Select
                    label="Kind"
                    option_pairs=kinds
                    value=kind
                    on_change=Callback::new(move |_| board.set(String::new()))
                />
                <TextInput
                    label="Title"
                    placeholder="Empty: the first line of the note"
                    value=title
                />
                {move || match all_boards.get() {
                    None => view! { <Text size="sm" dimmed=true>"Kairos reads the boards…"</Text> }.into_any(),
                    Some(Err(e)) => view! {
                        <Text size="sm" dimmed=true>{api::error_text(&e)}</Text>
                    }.into_any(),
                    Some(Ok(_)) if targets.with(Vec::is_empty) => view! {
                        <Text size="sm" dimmed=true>
                            "You cannot create this kind on a board. Ask an organization admin \
                             for the capability."
                        </Text>
                    }.into_any(),
                    Some(Ok(_)) => view! {
                        <Select
                            label="Target board"
                            option_pairs=targets
                            value=board
                            placeholder="Select the target board"
                        />
                    }.into_any(),
                }}
                <Text size="xs" dimmed=true>
                    "The new item gets the text of the note. The note then shows the code of the \
                     new item."
                </Text>
                {move || error.get().map(|message| view! {
                    <Alert title="Kairos did not promote the note" color=token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}
                <Group gap="sm">
                    <Button
                        size="xs"
                        loading=busy
                        loading_label="Promoting…"
                        disabled=Signal::derive(move || board.with(String::is_empty))
                        on_click=Callback::new(promote)
                    >
                        "Promote"
                    </Button>
                    <Button variant="default" size="xs" on_click=on_close>
                        "Close"
                    </Button>
                </Group>
            </Stack>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Whoami, WhoamiBoardCapabilities, WhoamiOrganization, WhoamiUser};
    use crate::pages::boards::data::Board;

    fn note(id: &str, promoted_to: Option<&str>) -> Note {
        Note {
            id: id.to_string(),
            author: "Robin".to_string(),
            created_at: "2026-10-17T09:30:00+00:00".to_string(),
            text: "One.".to_string(),
            promoted_to: promoted_to.map(str::to_string),
        }
    }

    fn me(role: &str, grants: &[(&str, &str)]) -> Whoami {
        Whoami {
            user: WhoamiUser {
                id: "u-1".into(),
                email: "robin@kairos.test".into(),
                display_name: "Robin".into(),
            },
            organization: WhoamiOrganization {
                slug: "acme".into(),
                role: role.into(),
            },
            teams: Vec::new(),
            capabilities: grants
                .iter()
                .map(|(board, grant)| WhoamiBoardCapabilities {
                    board_slug: board.to_string(),
                    grants: vec![grant.to_string()],
                    implied: Vec::new(),
                })
                .collect(),
            implicit: Vec::new(),
        }
    }

    fn board(slug: &str, level: &str) -> Board {
        Board {
            id: format!("b-{slug}"),
            name: slug.to_uppercase(),
            slug: slug.into(),
            board_level: level.into(),
            team_id: None,
        }
    }

    fn slugs(targets: Vec<(String, String)>) -> Vec<String> {
        targets.into_iter().map(|(slug, _)| slug).collect()
    }

    #[test]
    fn the_paths_name_the_family_the_item_and_the_note() {
        assert_eq!(notes_path(Family::Task, "A-T-1"), "/api/tasks/A-T-1/notes");
        assert_eq!(
            promote_path(Family::Initiative, "A-I-2", "n-3"),
            "/api/initiatives/A-I-2/notes/n-3/promote"
        );
    }

    #[test]
    fn only_a_note_id_in_the_hash_names_a_note() {
        assert_eq!(note_target("#n-3"), Some("n-3"));
        assert_eq!(note_target("#n-12"), Some("n-12"));
        assert_eq!(note_target(""), None);
        assert_eq!(note_target("#members"), None);
        assert_eq!(note_target("#n-"), None);
        assert_eq!(note_target("#n-3a"), None);
        assert_eq!(note_target("n-3"), None);
    }

    #[test]
    fn a_blank_draft_or_title_sends_nothing() {
        assert_eq!(note_text("  \n "), None);
        assert_eq!(
            note_text("\n## Done\n\nText.\n"),
            Some("## Done\n\nText.".to_string())
        );
        assert_eq!(promote_title(" "), None);
        assert_eq!(promote_title(" Use rows "), Some("Use rows".to_string()));
    }

    #[test]
    fn a_note_names_its_date_and_its_author() {
        assert_eq!(
            note_byline("2026-10-17T09:30:00+00:00", "Robin"),
            "2026-10-17 · Robin"
        );
        assert_eq!(note_byline("", "Sam"), " · Sam");
    }

    /// An ADR goes to an ADR board with `manage_adrs`; a document goes to
    /// a board with `manage_documents`, of any level.
    #[test]
    fn the_targets_follow_the_kind_and_the_capability() {
        let list = vec![
            board("adrs", "adr"),
            board("web-adrs", "adr"),
            board("web-delivery", "delivery"),
        ];
        let robin = me(
            "member",
            &[
                ("adrs", "manage_adrs"),
                ("web-delivery", "manage_adrs"),
                ("web-delivery", "manage_documents"),
            ],
        );
        assert_eq!(slugs(promote_targets(&robin, &list, "adr")), ["adrs"]);
        assert_eq!(
            slugs(promote_targets(&robin, &list, "document")),
            ["web-delivery"]
        );
        let sam = me("member", &[]);
        assert!(promote_targets(&sam, &list, "adr").is_empty());
        let admin = me("admin", &[]);
        assert_eq!(
            slugs(promote_targets(&admin, &list, "adr")),
            ["adrs", "web-adrs"]
        );
    }

    /// The page takes a new or a longer list of notes, and a promote mark.
    /// It keeps its list against a read with no notes or fewer notes.
    #[test]
    fn the_page_takes_new_notes_and_no_older_list() {
        let one = vec![note("n-1", None)];
        let two = vec![note("n-1", None), note("n-2", None)];
        let marked = vec![note("n-1", Some("A-A-1"))];
        assert!(takes_notes(None, Some(&one)));
        assert!(takes_notes(Some(&one), Some(&two)));
        assert!(takes_notes(Some(&one), Some(&marked)));
        assert!(!takes_notes(Some(&one), Some(&one)));
        assert!(!takes_notes(Some(&two), Some(&one)));
        assert!(!takes_notes(Some(&one), None));
    }

    #[test]
    fn a_conflict_is_a_409() {
        let conflict = ApiError::Http {
            status: 409,
            message: "The note n-1 on A-T-1 became A-A-1 already.".to_string(),
            code: Some("CONFLICT".to_string()),
        };
        assert!(is_conflict(&conflict));
        assert!(!is_conflict(&ApiError::Network));
    }
}
