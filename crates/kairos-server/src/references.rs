//! One lookup per kind of reference (KAIROS-T-0689, pass 2 of
//! KAIROS-D-0016).
//!
//! A tool argument, a REST field or a filter that names a team, a board, a
//! person, a repository, a column or an item comes here, so that each kind
//! has one set of forms in each tool and route. The caller keeps its own
//! refusal (its field, its status); the forms are here.
//!
//! | Kind | The forms |
//! |---|---|
//! | team | [`TEAM_FORMS`] |
//! | board | [`BOARD_FORMS`] |
//! | repository | [`REPOSITORY_FORMS`] |
//! | column | [`COLUMN_FORMS`] |
//! | person | [`PERSON_FORMS`] |
//! | item | [`ITEM_FORMS`] |
//!
//! Each lookup first removes the blanks at the two ends of the reference
//! ([`text`]): no slug, UUID, name, email or short code starts or ends
//! with a blank. Each other form is refused (the rule "refuse unknown
//! input"): a team takes no code prefix and no name (KAIROS-T-0685).

use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_db::models::User;
use kairos_db::models::boards::Board;
use kairos_db::models::teams::Team;
use uuid::Uuid;

use crate::error::ApiError;

/// The forms of a team reference.
pub const TEAM_FORMS: &[&str] = &["UUID", "slug"];
/// The forms of a board reference.
pub const BOARD_FORMS: &[&str] = &["UUID", "slug"];
/// The forms of a repository reference.
pub const REPOSITORY_FORMS: &[&str] = &["UUID", "slug"];
/// The forms of a column reference: the name in any case.
pub const COLUMN_FORMS: &[&str] = &["UUID", "name"];
/// The forms of a person reference: `me` in any case is the caller. A
/// service account has no user name: its name is its display name.
pub const PERSON_FORMS: &[&str] = &["me", "UUID", "email", "user name", "service-account name"];
/// The forms of an item reference: the short code in any case.
pub const ITEM_FORMS: &[&str] = &["short code"];

/// The reference without the blanks at its two ends.
pub fn text(reference: &str) -> &str {
    reference.trim()
}

/// A short code as the directory has it: in capitals (KAIROS-T-0560).
pub fn short_code(reference: &str) -> String {
    text(reference).to_ascii_uppercase()
}

/// The live team with the UUID or the slug `reference`.
pub fn team(conn: &mut PgConnection, reference: &str) -> Result<Option<Team>, ApiError> {
    use kairos_db::schema::teams::dsl;
    let reference = text(reference);
    let mut query = dsl::teams.filter(dsl::deleted_at.is_null()).into_boxed();
    query = match Uuid::parse_str(reference) {
        Ok(id) => query.filter(dsl::id.eq(id)),
        Err(_) => query.filter(dsl::slug.eq(reference)),
    };
    query
        .select(Team::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)
}

/// The live board with the UUID or the slug `reference`.
pub fn board(conn: &mut PgConnection, reference: &str) -> Result<Option<Board>, ApiError> {
    use kairos_db::schema::boards::dsl;
    let reference = text(reference);
    let mut query = dsl::boards.filter(dsl::deleted_at.is_null()).into_boxed();
    query = match Uuid::parse_str(reference) {
        Ok(id) => query.filter(dsl::id.eq(id)),
        Err(_) => query.filter(dsl::slug.eq(reference)),
    };
    query
        .select(Board::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)
}

/// The column of `columns` (id, name) with the UUID or the name
/// `reference`. A name matches in any case.
pub fn column<'a>(
    columns: impl IntoIterator<Item = (Uuid, &'a str)>,
    reference: &str,
) -> Option<Uuid> {
    let reference = text(reference);
    let by_id = Uuid::parse_str(reference).ok();
    columns
        .into_iter()
        .find(|(id, name)| Some(*id) == by_id || name.eq_ignore_ascii_case(reference))
        .map(|(id, _)| id)
}

/// The accounts that the person reference `reference` names, before the
/// check of membership. `me` is `caller`. A UUID is the id, a text with
/// `@` is the email, each other text is the user name, or the display
/// name of a service account when no user has that user name. Two
/// service accounts can have one name: the caller refuses more than one.
pub fn people(
    conn: &mut PgConnection,
    caller: Uuid,
    reference: &str,
) -> Result<Vec<User>, ApiError> {
    use kairos_db::schema::users;
    let reference = text(reference);
    let select = users::table.select(User::as_select());
    let found = if reference.eq_ignore_ascii_case("me") {
        select.filter(users::id.eq(caller)).load(conn)
    } else if let Ok(id) = reference.parse::<Uuid>() {
        select.filter(users::id.eq(id)).load(conn)
    } else if reference.contains('@') {
        select.filter(users::email.eq(reference)).load(conn)
    } else {
        let by_user_name = users::table
            .select(User::as_select())
            .filter(users::user_name.eq(reference))
            .load::<User>(conn)
            .map_err(ApiError::internal)?;
        if !by_user_name.is_empty() {
            return Ok(by_user_name);
        }
        select
            .filter(users::display_name.eq(reference))
            .filter(users::kind.eq(kairos_db::models::public::USER_KIND_SERVICE_ACCOUNT))
            .load(conn)
    };
    found.map_err(ApiError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind of reference of each argument name, and the words of its
    /// forms that the description of the argument must have. A name that
    /// has more than one meaning (KAIROS-T-0688) has its tools.
    const KINDS: &[(&str, &[&str], &str, &[&str])] = &[
        ("team", &[], "team", &["slug", "UUID"]),
        ("from_team", &[], "team", &["slug", "UUID"]),
        ("board", &[], "board", &["slug", "UUID"]),
        ("to_board", &[], "board", &["slug", "UUID"]),
        ("repository", &[], "repository", &["slug", "UUID"]),
        ("column", &[], "column", &["name", "UUID"]),
        ("to_column", &[], "column", &["name", "UUID"]),
        ("from_column", &[], "column", &["name", "UUID"]),
        ("user", &[], "person", &["me", "id", "email", "user name"]),
        (
            "to",
            &["hand_off_item"],
            "person",
            &["me", "id", "email", "user name"],
        ),
        (
            "claimed_by",
            &[],
            "person",
            &["me", "id", "email", "user name"],
        ),
        (
            "created_by",
            &[],
            "person",
            &["me", "id", "email", "user name"],
        ),
        (
            "owned_by",
            &[],
            "person",
            &["me", "id", "email", "user name"],
        ),
        ("short_code", &[], "item", &["short code"]),
        ("source", &[], "item", &["short code"]),
        ("target", &[], "item", &["short code"]),
        (
            "parent",
            &["create_item", "set_parent"],
            "item",
            &["short code"],
        ),
        ("from", &["search"], "item", &["short code"]),
    ];

    /// The words of a text, in lower case.
    fn words_of(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    }

    /// `text` has the words of `phrase`, one after the other.
    fn has_words(text: &str, phrase: &str) -> bool {
        let text = words_of(text);
        let phrase = words_of(phrase);
        text.windows(phrase.len()).any(|w| w == phrase.as_slice())
    }

    #[test]
    fn a_phrase_is_whole_words() {
        assert!(has_words("a user id, an email or a user name", "user name"));
        assert!(!has_words("the name of a service account", "me"));
        assert!(!has_words("by slug or UUID", "id"));
        assert!(has_words("`me`, a user id", "me"));
    }

    /// KAIROS-T-0689: the description of each argument that names a team,
    /// a board, a repository, a column, a person or an item states the
    /// forms of [`TEAM_FORMS`] and the others.
    #[test]
    fn each_reference_argument_states_its_forms() {
        use serde_json::Value;
        let forms_of = |kind: &str| match kind {
            "team" => TEAM_FORMS,
            "board" => BOARD_FORMS,
            "repository" => REPOSITORY_FORMS,
            "column" => COLUMN_FORMS,
            "person" => PERSON_FORMS,
            _ => ITEM_FORMS,
        };
        let mut faults = Vec::new();
        let mut seen = 0;
        for tool in crate::mcp::all_tools() {
            let schema = &tool.input_schema;
            let mut objects = vec![schema.as_ref().clone()];
            if let Some(Value::Object(defs)) = schema.get("$defs") {
                objects.extend(defs.values().filter_map(|d| d.as_object().cloned()));
            }
            for object in objects {
                let Some(Value::Object(properties)) = object.get("properties") else {
                    continue;
                };
                for (name, property) in properties {
                    let Some((_, _, kind, words)) = KINDS.iter().find(|(n, tools, _, _)| {
                        n == name && (tools.is_empty() || tools.contains(&tool.name.as_ref()))
                    }) else {
                        continue;
                    };
                    seen += 1;
                    let description = property
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let first = description.split("\nOther names").next().unwrap_or("");
                    let missing: Vec<&&str> = words
                        .iter()
                        .filter(|phrase| !has_words(first, phrase))
                        .collect();
                    if !missing.is_empty() {
                        faults.push(format!(
                            "{}.{name} ({kind}, forms {:?}): the description does not say {:?}: \
                             {first:?}",
                            tool.name,
                            forms_of(kind),
                            missing
                        ));
                    }
                }
            }
        }
        assert!(seen > 90, "{seen} arguments");
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    #[test]
    fn a_reference_has_no_blanks_at_its_ends() {
        assert_eq!(text("  web \n"), "web");
        assert_eq!(short_code(" acme-t-0012 "), "ACME-T-0012");
    }

    #[test]
    fn a_column_by_id_or_by_name_in_any_case() {
        let todo = Uuid::new_v4();
        let active = Uuid::new_v4();
        let columns = [(todo, "Todo"), (active, "In Progress")];
        for reference in ["In Progress", "in progress", " IN PROGRESS "] {
            assert_eq!(column(columns, reference), Some(active), "{reference}");
        }
        assert_eq!(column(columns, &todo.to_string()), Some(todo));
        assert_eq!(column(columns, &format!(" {todo} ")), Some(todo));
        assert_eq!(column(columns, "Done"), None);
        assert_eq!(column(columns, &Uuid::new_v4().to_string()), None);
    }
}
