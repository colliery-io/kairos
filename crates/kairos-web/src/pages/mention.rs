//! The `@` picker of the note box and the content editor (KAIROS-T-0665).
//!
//! A mention is `@` and a user name (`kairos_core::notifications::mentions`
//! on the server). The picker shows the people of the board of the item
//! after `@` and some letters: their name and their user name. A choice
//! puts `@<user name> ` in the text. The arrow keys move the choice, Enter
//! or Tab takes it, Escape closes the list.
//!
//! The people of a board are the grants of `GET /api/boards/{id}/members`
//! and the members of the team of the board (`GET /api/teams/{id}/members`):
//! the people that MCP `list_members board=` gives. The item page reads
//! them at the first `@`, and gives them in a [`MentionSource`] context. A
//! page with no context has no picker.
//!
//! The selection of a textarea counts UTF-16 code units, so the logic here
//! counts them too.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// The most people that the list shows.
const LIMIT: usize = 8;

/// A person that the picker can give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub user_name: String,
    pub display_name: String,
}

/// The people of the board, for the picker of the item page. `people` is
/// `None` until the read is done. `want` starts the read.
#[derive(Clone, Copy)]
pub struct MentionSource {
    pub people: Signal<Option<Vec<Person>>>,
    pub want: Callback<()>,
}

/// The `@` and the letters before the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// The place of the `@` (UTF-16 code units).
    pub start: usize,
    /// The letters after the `@`, up to the caret.
    pub text: String,
}

/// A character of a user name, as the server reads a mention.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+')
}

/// The mention that the caret is in: an `@` at the start of the text or
/// after a character that is not a letter, a digit, `_`, `.`, `@` or `` ` ``
/// (an email address is not a mention), then name characters up to the
/// caret. `caret` counts UTF-16 code units. Pure, host-tested.
pub fn query_at(value: &str, caret: usize) -> Option<Query> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let before = String::from_utf16_lossy(&units[..caret.min(units.len())]);
    let chars: Vec<char> = before.chars().collect();
    let mut at = chars.len();
    while at > 0 && is_name_char(chars[at - 1]) {
        at -= 1;
    }
    if at == 0 || chars[at - 1] != '@' {
        return None;
    }
    let sign = at - 1;
    if sign > 0 {
        let c = chars[sign - 1];
        if c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '`') {
            return None;
        }
    }
    let start = chars[..sign].iter().map(|c| c.len_utf16()).sum();
    Some(Query {
        start,
        text: chars[at..].iter().collect(),
    })
}

/// The people that `query` matches, the best first, [`LIMIT`] at most: a
/// user name that starts with the letters, then a word of the name that
/// starts with them, then a user name or a name that has them. Case does
/// not count. No letters: each person. In a rank, the order of `people`
/// (the order of the user names, [`board_people`]). No sort: the bundle
/// has a size budget. Pure, host-tested.
pub fn matches(people: &[Person], query: &str) -> Vec<Person> {
    let query = query.to_lowercase();
    let rank = |person: &Person| -> Option<u8> {
        let user = person.user_name.to_lowercase();
        let name = person.display_name.to_lowercase();
        if person.user_name.is_empty() {
            None
        } else if user.starts_with(&query) {
            Some(0)
        } else if name.split_whitespace().any(|word| word.starts_with(&query)) {
            Some(1)
        } else if user.contains(&query) || name.contains(&query) {
            Some(2)
        } else {
            None
        }
    };
    let ranks: Vec<Option<u8>> = people.iter().map(rank).collect();
    let mut out = Vec::new();
    for want in 0..3 {
        for (person, rank) in people.iter().zip(&ranks) {
            if *rank == Some(want) && out.len() < LIMIT {
                out.push(person.clone());
            }
        }
    }
    out
}

/// The text with `@<user name> ` in place of the query, and the caret
/// after it (UTF-16 code units). Pure, host-tested.
pub fn insert(value: &str, query: &Query, caret: usize, user_name: &str) -> (String, usize) {
    let units: Vec<u16> = value.encode_utf16().collect();
    let end = caret.min(units.len());
    let start = query.start.min(end);
    let mention: Vec<u16> = format!("@{user_name} ").encode_utf16().collect();
    let mut out = Vec::with_capacity(units.len() + mention.len());
    out.extend_from_slice(&units[..start]);
    out.extend_from_slice(&mention);
    out.extend_from_slice(&units[end..]);
    (String::from_utf16_lossy(&out), start + mention.len())
}

/// What a key does while the list is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
    Choose,
    Close,
}

/// The key of a `KeyboardEvent.key`, when the list uses it. Pure,
/// host-tested.
pub fn key_of(key: &str) -> Option<Key> {
    match key {
        "ArrowDown" => Some(Key::Down),
        "ArrowUp" => Some(Key::Up),
        "Enter" | "Tab" => Some(Key::Choose),
        "Escape" => Some(Key::Close),
        _ => None,
    }
}

/// The next choice in a list of `len`: the arrows go round. Pure,
/// host-tested.
pub fn step(selected: usize, len: usize, key: Key) -> usize {
    match (key, len) {
        (_, 0) => 0,
        (Key::Down, _) => (selected + 1) % len,
        (Key::Up, _) => (selected + len - 1) % len,
        _ => selected.min(len - 1),
    }
}

/// The people of a board: the people with a grant and the members of the
/// team of the board, each one time, in the order of their user names.
/// Pure, host-tested.
pub fn board_people(grants: Vec<Person>, team: Vec<Person>) -> Vec<Person> {
    let by_name: std::collections::BTreeMap<String, String> = grants
        .into_iter()
        .chain(team)
        .filter(|person| !person.user_name.is_empty())
        .map(|person| (person.user_name, person.display_name))
        .collect();
    by_name
        .into_iter()
        .map(|(user_name, display_name)| Person {
            user_name,
            display_name,
        })
        .collect()
}

/// The people of the board `slug` and of its team (`team_id`), for the
/// picker. A read that fails gives no people for its part.
pub async fn load_board_people(
    auth: crate::auth::Auth,
    slug: &str,
    team_id: Option<&str>,
) -> Vec<Person> {
    let grants = crate::pages::admin::api::board_members(auth, slug)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| Person {
            user_name: m.user_name,
            display_name: m.display_name,
        })
        .collect();
    let team = match team_id {
        Some(team_id) => crate::pages::teams::api::team_members(auth, team_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| Person {
                user_name: m.user_name,
                display_name: m.display_name,
            })
            .collect(),
        None => Vec::new(),
    };
    board_people(grants, team)
}

/// `@name` for a user name; empty for no user name (an older server).
/// Pure, host-tested.
pub fn handle(user_name: &str) -> String {
    if user_name.is_empty() {
        String::new()
    } else {
        format!("@{user_name}")
    }
}

/// A name and its handle: `Robin @robin`; the name alone with no user
/// name. Pure, host-tested.
pub fn with_handle(display_name: &str, user_name: &str) -> String {
    if user_name.is_empty() {
        display_name.to_string()
    } else {
        format!("{display_name} @{user_name}")
    }
}

/// The list as HTML: one option for each person, the choice marked. The
/// list is one `inner_html`, not a view for each option: the bundle has a
/// size budget. Pure, host-tested.
pub fn list_html(people: &[Person], selected: usize) -> String {
    let mut html = String::new();
    for (i, person) in people.iter().enumerate() {
        let on = i == selected;
        html.push_str(&format!(
            "<li role=\"option\" data-i=\"{i}\" aria-selected=\"{on}\" class=\"kairos-mention__option{}\">{} <span class=\"kairos-mention__handle\">{}</span></li>",
            if on { " kairos-mention__option--on" } else { "" },
            escape(&person.display_name),
            escape(&handle(&person.user_name)),
        ));
    }
    html
}

/// Text as HTML text.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The textarea of an event.
fn textarea_of(event: &web_sys::Event) -> Option<web_sys::HtmlTextAreaElement> {
    event
        .target()?
        .dyn_into::<web_sys::HtmlTextAreaElement>()
        .ok()
}

/// The caret of a textarea (UTF-16 code units).
fn caret_of(element: &web_sys::HtmlTextAreaElement) -> usize {
    element.selection_start().ok().flatten().unwrap_or(0) as usize
}

/// The index of the option of an event (`data-i`).
fn option_of(event: &web_sys::Event) -> Option<usize> {
    let element = event.target()?.dyn_into::<web_sys::Element>().ok()?;
    element
        .closest("[data-i]")
        .ok()??
        .get_attribute("data-i")?
        .parse()
        .ok()
}

/// A box around a textarea with the picker under it. `value` is the signal
/// of the textarea. With no [`MentionSource`] in the context, the box
/// shows the textarea only.
#[component]
pub fn MentionBox(value: RwSignal<String>, children: Children) -> impl IntoView {
    let source = use_context::<MentionSource>();
    let query = RwSignal::new(None::<Query>);
    let selected = RwSignal::new(0usize);
    let element = StoredValue::new_local(None::<web_sys::HtmlTextAreaElement>);
    // The people of the list now. A plain function, not a memo: the
    // bundle has a size budget.
    let shown = move || -> Vec<Person> {
        let (Some(source), Some(query)) = (source, query.get()) else {
            return Vec::new();
        };
        source
            .people
            .with(|people| people.as_deref().map(|people| matches(people, &query.text)))
            .unwrap_or_default()
    };
    let read = move |event: &web_sys::Event| {
        let (Some(source), Some(el)) = (source, textarea_of(event)) else {
            return;
        };
        let found = query_at(&el.value(), caret_of(&el));
        if found.is_some() {
            source.want.run(());
        }
        if found != query.get_untracked() {
            selected.set(0);
        }
        query.set(found);
        element.set_value(Some(el));
    };
    let choose = move |index: usize| {
        let pick = untrack(shown).get(index).map(|p| p.user_name.clone());
        let (Some(user_name), Some(el), Some(found)) =
            (pick, element.get_value(), query.get_untracked())
        else {
            return;
        };
        let (text, caret) = insert(&el.value(), &found, caret_of(&el), &user_name);
        el.set_value(&text);
        let _ = el.set_selection_range(caret as u32, caret as u32);
        let _ = el.focus();
        value.set(text);
        query.set(None);
    };
    let on_key = move |event: web_sys::KeyboardEvent| {
        let key = key_of(&event.key());
        let len = untrack(shown).len();
        let Some(key) = key.filter(|_| len > 0) else {
            if key.is_none() && event.type_() == "keyup" {
                // A caret move of the arrows, Home or End.
                read(&event);
            }
            return;
        };
        if event.type_() != "keydown" {
            return;
        }
        event.prevent_default();
        match key {
            Key::Close => {
                event.stop_propagation();
                query.set(None);
            }
            Key::Choose => choose(step(selected.get_untracked(), len, key)),
            Key::Down | Key::Up => selected.update(|i| *i = step(*i, len, key)),
        }
    };
    view! {
        <div
            class="kairos-mention"
            on:input=move |e| read(&e)
            on:keydown=on_key
            on:keyup=on_key
            on:focusout=move |_| query.set(None)
        >
            {children()}
            <ul
                class="kairos-mention__list"
                role="listbox"
                data-testid="mention-picker"
                inner_html=move || list_html(&shown(), selected.get())
                on:mousedown=move |e| {
                    // Keep the focus in the textarea.
                    e.prevent_default();
                    if let Some(index) = option_of(&e) {
                        choose(index);
                    }
                }
            ></ul>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(user_name: &str, display_name: &str) -> Person {
        Person {
            user_name: user_name.into(),
            display_name: display_name.into(),
        }
    }

    fn names(list: Vec<Person>) -> Vec<String> {
        list.into_iter().map(|p| p.user_name).collect()
    }

    #[test]
    fn an_at_sign_after_a_space_or_the_start_opens_a_query() {
        assert_eq!(
            query_at("@ro", 3),
            Some(Query {
                start: 0,
                text: "ro".into()
            })
        );
        assert_eq!(
            query_at("Ask @sam", 8),
            Some(Query {
                start: 4,
                text: "sam".into()
            })
        );
        assert_eq!(
            query_at("(@", 2),
            Some(Query {
                start: 1,
                text: String::new()
            })
        );
        // The caret is in the name: the letters up to the caret.
        assert_eq!(query_at("@robin x", 3).map(|q| q.text), Some("ro".into()));
    }

    #[test]
    fn an_email_code_or_a_space_is_no_query() {
        assert_eq!(query_at("robin@example", 13), None);
        assert_eq!(query_at("`@ro", 4), None);
        assert_eq!(query_at("@ro x", 5), None);
        assert_eq!(query_at("no mention", 10), None);
        assert_eq!(query_at("", 0), None);
    }

    #[test]
    fn the_place_counts_utf16_units() {
        // "é" is one unit, "😀" is two.
        let text = "é😀 @ja";
        let caret = text.encode_utf16().count();
        assert_eq!(
            query_at(text, caret),
            Some(Query {
                start: 4,
                text: "ja".into()
            })
        );
    }

    #[test]
    fn a_user_name_prefix_comes_first_then_a_word_of_the_name() {
        // In the order of `board_people`: the user names.
        let people = vec![
            person("jamie", "Jamie Robinson"),
            person("old", ""),
            person("robin", "Robin Ash"),
            person("sam", "Sam Lee"),
        ];
        assert_eq!(names(matches(&people, "ro")), ["robin", "jamie"]);
        assert_eq!(names(matches(&people, "RO")), ["robin", "jamie"]);
        assert_eq!(names(matches(&people, "lee")), ["sam"]);
        assert_eq!(names(matches(&people, "mi")), ["jamie"]);
        assert_eq!(
            names(matches(&people, "")),
            ["jamie", "old", "robin", "sam"]
        );
        assert!(matches(&people, "zz").is_empty());
    }

    #[test]
    fn a_person_with_no_user_name_and_the_limit() {
        let mut people: Vec<Person> = (0..12).map(|i| person(&format!("p{i:02}"), "")).collect();
        people.push(person("", "No Name"));
        let shown = matches(&people, "");
        assert_eq!(shown.len(), LIMIT);
        assert!(shown.iter().all(|p| !p.user_name.is_empty()));
        assert!(matches(&people, "no").is_empty());
    }

    #[test]
    fn a_choice_puts_the_user_name_and_a_space_in_place_of_the_query() {
        let text = "Ask @ro about it";
        let query = query_at(text, 7).expect("query");
        assert_eq!(
            insert(text, &query, 7, "robin"),
            ("Ask @robin  about it".to_string(), 11)
        );
        let text = "@";
        let query = query_at(text, 1).expect("query");
        assert_eq!(insert(text, &query, 1, "sam"), ("@sam ".to_string(), 5));
        let text = "😀 @j";
        let query = query_at(text, 5).expect("query");
        assert_eq!(
            insert(text, &query, 5, "jamie"),
            ("😀 @jamie ".to_string(), 10)
        );
    }

    #[test]
    fn the_keys_move_take_and_close() {
        assert_eq!(key_of("ArrowDown"), Some(Key::Down));
        assert_eq!(key_of("ArrowUp"), Some(Key::Up));
        assert_eq!(key_of("Enter"), Some(Key::Choose));
        assert_eq!(key_of("Tab"), Some(Key::Choose));
        assert_eq!(key_of("Escape"), Some(Key::Close));
        assert_eq!(key_of("a"), None);
        assert_eq!(step(0, 3, Key::Down), 1);
        assert_eq!(step(2, 3, Key::Down), 0);
        assert_eq!(step(0, 3, Key::Up), 2);
        assert_eq!(step(5, 3, Key::Choose), 2);
        assert_eq!(step(0, 0, Key::Down), 0);
    }

    #[test]
    fn the_people_of_a_board_are_the_grants_and_the_team_one_time_each() {
        let grants = vec![person("sam", "Sam"), person("", "Old")];
        let team = vec![person("robin", "Robin"), person("sam", "Sam")];
        assert_eq!(names(board_people(grants, team)), ["robin", "sam"]);
    }

    #[test]
    fn the_list_marks_the_choice_and_escapes_the_names() {
        let people = vec![person("robin", "Robin <Ash>"), person("sam", "Sam")];
        let html = list_html(&people, 1);
        assert_eq!(html.matches("<li ").count(), 2);
        assert!(
            html.contains("data-i=\"0\" aria-selected=\"false\""),
            "{html}"
        );
        assert!(html.contains("data-i=\"1\" aria-selected=\"true\" class=\"kairos-mention__option kairos-mention__option--on\""), "{html}");
        assert!(html.contains("Robin &lt;Ash&gt;"), "{html}");
        assert!(html.contains(">@sam<"), "{html}");
    }

    #[test]
    fn a_handle_is_the_user_name_after_an_at_sign() {
        assert_eq!(handle("robin"), "@robin");
        assert_eq!(handle(""), "");
        assert_eq!(with_handle("Robin", "robin"), "Robin @robin");
        assert_eq!(with_handle("Robin", ""), "Robin");
    }
}
