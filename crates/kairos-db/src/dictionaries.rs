//! Dictionaries (KAIROS-T-0733, COLLIERY-I-0664 decisions 6 and 7).
//!
//! A dictionary is a document made with the system template `dictionary`.
//! Its owner board says whose dictionary it is:
//!
//! - the dictionary of a team is owned by the delivery board of the team,
//! - the dictionary of the organization is owned by the board `strategy`.
//!
//! A board owns at most one live dictionary. `items::create_document`,
//! `items::set_document_board` and `items::restore_item` refuse a second one
//! and name the first ([`live_dictionary_of_board`]).
//!
//! [`team_dictionary`] gives the terms of a team: the organization entries,
//! then the team entries, and the team entry wins for the same word (case
//! ignored, `kairos_plain::resolve`). A bad row does not stop the read: it
//! is in [`TeamDictionary::problems`] with its document and its line.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use kairos_plain::{DictionaryProblem, ParsedDictionary, Term, Vocabulary, parse_dictionary};

use crate::models::enums::BoardLevel;

/// The slug of the system template of a dictionary.
pub const DICTIONARY_TEMPLATE_SLUG: &str = "dictionary";

/// The slug of the board that owns the dictionary of the organization.
pub const ORGANIZATION_BOARD_SLUG: &str = "strategy";

/// An error of [`team_dictionary`].
#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    /// No live team with this id.
    #[error("The team {0} does not exist.")]
    TeamNotFound(Uuid),
    /// The team has more than one live delivery board, so its dictionary
    /// is not known.
    #[error(
        "The team {team} has {count} delivery boards. A team has one delivery board, which \
         owns the dictionary of the team."
    )]
    SeveralDeliveryBoards { team: Uuid, count: usize },
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// The dictionary document of one board, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardDictionary {
    pub document_id: Uuid,
    pub short_code: String,
    pub parsed: ParsedDictionary,
}

/// A problem of a dictionary, with the code of its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentProblem {
    pub short_code: String,
    pub problem: DictionaryProblem,
}

/// The resolved dictionary of a team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamDictionary {
    /// The code of the organization dictionary, when there is one.
    pub organization: Option<String>,
    /// The code of the dictionary of the team, when there is one.
    pub team: Option<String>,
    /// Each term, sorted by the word (case ignored). For a word that is in
    /// both dictionaries, the entry of the team.
    pub terms: Vec<Term>,
    /// The bad rows of the two documents, the organization first.
    pub problems: Vec<DocumentProblem>,
}

impl TeamDictionary {
    /// The words of the terms, for `kairos_plain::Vocabulary::new`.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.terms.iter().map(|t| t.entry.word.as_str())
    }

    /// The vocabulary of the `base` word list and the terms of the team.
    pub fn vocabulary<B>(&self, base: B) -> Vocabulary
    where
        B: IntoIterator,
        B::Item: AsRef<str>,
    {
        Vocabulary::new(base, self.words())
    }
}

/// Whether the template `template_id` is the dictionary template.
pub fn is_dictionary_template(
    conn: &mut PgConnection,
    template_id: Uuid,
) -> Result<bool, DieselError> {
    use crate::schema::templates::dsl;
    let found: Option<Uuid> = dsl::templates
        .filter(dsl::id.eq(template_id))
        .filter(dsl::slug.eq(DICTIONARY_TEMPLATE_SLUG))
        .select(dsl::id)
        .first(conn)
        .optional()?;
    Ok(found.is_some())
}

/// Whether the document `document_id` (live or archived) is a dictionary.
pub fn is_dictionary_document(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<bool, DieselError> {
    use crate::schema::{documents, templates};
    let found: Option<Uuid> = documents::table
        .inner_join(templates::table)
        .filter(documents::id.eq(document_id))
        .filter(templates::slug.eq(DICTIONARY_TEMPLATE_SLUG))
        .select(documents::id)
        .first(conn)
        .optional()?;
    Ok(found.is_some())
}

/// The live dictionary document of `board_id`, as its id and its code.
///
/// The row of the board is locked (`FOR UPDATE`) to the end of the
/// transaction, so two writes that each add a dictionary to the same board
/// do not both see none. Call it in the transaction of the write.
pub fn live_dictionary_of_board(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Option<(Uuid, String)>, DieselError> {
    use crate::schema::{boards, documents, templates};
    boards::table
        .filter(boards::id.eq(board_id))
        .select(boards::id)
        .for_update()
        .first::<Uuid>(conn)
        .optional()?;
    documents::table
        .inner_join(templates::table)
        .filter(documents::board_id.eq(board_id))
        .filter(documents::deleted_at.is_null())
        .filter(templates::slug.eq(DICTIONARY_TEMPLATE_SLUG))
        .order(documents::short_code.asc())
        .select((documents::id, documents::short_code))
        .first(conn)
        .optional()
}

/// The live dictionary of `board_id`, read. `None`: the board has none.
pub fn board_dictionary(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Option<BoardDictionary>, DieselError> {
    use crate::schema::{documents, templates};
    let row: Option<(Uuid, String, String)> = documents::table
        .inner_join(templates::table)
        .filter(documents::board_id.eq(board_id))
        .filter(documents::deleted_at.is_null())
        .filter(templates::slug.eq(DICTIONARY_TEMPLATE_SLUG))
        .order(documents::short_code.asc())
        .select((documents::id, documents::short_code, documents::content))
        .first(conn)
        .optional()?;
    Ok(
        row.map(|(document_id, short_code, content)| BoardDictionary {
            document_id,
            short_code,
            parsed: parse_dictionary(&content),
        }),
    )
}

/// The dictionary of the organization: the live dictionary of the live
/// board `strategy`.
pub fn organization_dictionary(
    conn: &mut PgConnection,
) -> Result<Option<BoardDictionary>, DieselError> {
    use crate::schema::boards::dsl;
    let board: Option<Uuid> = dsl::boards
        .filter(dsl::slug.eq(ORGANIZATION_BOARD_SLUG))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()?;
    match board {
        Some(board) => board_dictionary(conn, board),
        None => Ok(None),
    }
}

/// The terms of the team `team_id`: the organization dictionary, then the
/// dictionary of the delivery board of the team. The team entry wins for
/// the same word. A team with no delivery board has the organization terms
/// only.
pub fn team_dictionary(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<TeamDictionary, DictionaryError> {
    use crate::schema::{boards, teams};
    let team: Option<Uuid> = teams::table
        .filter(teams::id.eq(team_id))
        .filter(teams::deleted_at.is_null())
        .select(teams::id)
        .first(conn)
        .optional()?;
    if team.is_none() {
        return Err(DictionaryError::TeamNotFound(team_id));
    }
    let delivery: Vec<Uuid> = boards::table
        .filter(boards::team_id.eq(team_id))
        .filter(boards::board_level.eq(BoardLevel::Delivery))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .load(conn)?;
    let team_board = match delivery.as_slice() {
        [] => None,
        [one] => Some(*one),
        several => {
            return Err(DictionaryError::SeveralDeliveryBoards {
                team: team_id,
                count: several.len(),
            });
        }
    };

    let organization = organization_dictionary(conn)?;
    let team = match team_board {
        Some(board) => board_dictionary(conn, board)?,
        None => None,
    };
    let entries = |d: &Option<BoardDictionary>| {
        d.as_ref()
            .map(|d| d.parsed.entries.clone())
            .unwrap_or_default()
    };
    let terms = kairos_plain::resolve(&entries(&organization), &entries(&team));
    let problems = [&organization, &team]
        .into_iter()
        .flatten()
        .flat_map(|d| {
            d.parsed.problems.iter().map(|problem| DocumentProblem {
                short_code: d.short_code.clone(),
                problem: problem.clone(),
            })
        })
        .collect();
    Ok(TeamDictionary {
        organization: organization.map(|d| d.short_code),
        team: team.map(|d| d.short_code),
        terms,
        problems,
    })
}
