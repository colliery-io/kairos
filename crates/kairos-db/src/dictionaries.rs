//! The dictionary of the organization (KAIROS-T-0733, COLLIERY-I-0664
//! decision 7, KAIROS-A-0079).
//!
//! A dictionary is a document made with the system template `dictionary`.
//! An organization has one dictionary, and the board `strategy` owns it
//! (KAIROS-A-0079). There are no team dictionaries:
//! `items::create_document`, `items::set_document_board` and
//! `items::restore_item` refuse a dictionary on any other board and name
//! the board `strategy`. The board `strategy` owns at most one live
//! dictionary: the same writes refuse a second one and name the first
//! ([`live_dictionary_of_board`]).
//!
//! [`organization_dictionary`] gives its terms and its bad rows. Each
//! reader uses it and nothing else: the check of the typed sections on an
//! MCP write and the stored mark (`crate::section_checks`), and the block
//! "Terms used" of `get_item` (KAIROS-T-0735), which reads the content in
//! the statement of the other facts of the item
//! ([`organization_dictionary_sql`]) and builds the terms with
//! [`OrganizationDictionary::from_document`].

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use kairos_plain::{DictionaryEntry, DictionaryProblem, Vocabulary, parse_dictionary};

/// The slug of the system template of a dictionary.
pub const DICTIONARY_TEMPLATE_SLUG: &str = "dictionary";

/// The slug of the board that owns the dictionary of the organization: the
/// only board that can own a dictionary (KAIROS-A-0079).
pub const ORGANIZATION_BOARD_SLUG: &str = "strategy";

/// The dictionary of the organization, read. With no dictionary, it has no
/// code, no term and no problem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrganizationDictionary {
    /// The code of the dictionary document, when there is one.
    pub short_code: Option<String>,
    /// Each entry, sorted by the word (case ignored).
    pub terms: Vec<DictionaryEntry>,
    /// The bad rows of the document, each with its line.
    pub problems: Vec<DictionaryProblem>,
}

impl OrganizationDictionary {
    /// The dictionary of the document `short_code` with the text `content`.
    /// `None`: the organization has no dictionary.
    pub fn from_document(short_code: Option<String>, content: Option<&str>) -> Self {
        let Some(content) = content else {
            return Self::default();
        };
        let parsed = parse_dictionary(content);
        let mut terms = parsed.entries;
        terms.sort_by_cached_key(|entry| entry.word.to_lowercase());
        Self {
            short_code,
            terms,
            problems: parsed.problems,
        }
    }

    /// The words of the terms, for `kairos_plain::Vocabulary::new`.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.terms.iter().map(|t| t.word.as_str())
    }

    /// The vocabulary of the `base` word list and the terms.
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

/// The dictionary of the organization: the live dictionary of the live
/// board `strategy` (KAIROS-A-0079). One statement.
pub fn organization_dictionary(
    conn: &mut PgConnection,
) -> Result<OrganizationDictionary, DieselError> {
    use crate::schema::{boards, documents, templates};
    let row: Option<(String, String)> = documents::table
        .inner_join(templates::table)
        .inner_join(boards::table)
        .filter(boards::slug.eq(ORGANIZATION_BOARD_SLUG))
        .filter(boards::deleted_at.is_null())
        .filter(documents::deleted_at.is_null())
        .filter(templates::slug.eq(DICTIONARY_TEMPLATE_SLUG))
        .order(documents::short_code.asc())
        .select((documents::short_code, documents::content))
        .first(conn)
        .optional()?;
    Ok(match row {
        Some((short_code, content)) => {
            OrganizationDictionary::from_document(Some(short_code), Some(&content))
        }
        None => OrganizationDictionary::default(),
    })
}

/// An SQL expression (nullable `text`) of the content of the dictionary of
/// the organization, as [`organization_dictionary`] reads it, for a
/// statement that reads other facts too (`get_item`, KAIROS-T-0735).
/// `NULL`: the organization has no dictionary.
pub fn organization_dictionary_sql() -> String {
    format!(
        "(SELECT dd.content FROM documents dd \
            JOIN templates dtp ON dtp.id = dd.template_id \
                              AND dtp.slug = '{DICTIONARY_TEMPLATE_SLUG}' \
            JOIN boards db ON db.id = dd.board_id AND db.deleted_at IS NULL \
           WHERE db.slug = '{ORGANIZATION_BOARD_SLUG}' AND dd.deleted_at IS NULL \
           ORDER BY dd.short_code LIMIT 1)"
    )
}

/// The terms of `terms` that the prose of `texts` uses
/// (`kairos_plain::terms_used`: case ignored, the longest term first at
/// each word, code is not prose), in the order of `terms`.
pub fn terms_used_by<X>(texts: X, terms: &[DictionaryEntry]) -> Vec<&DictionaryEntry>
where
    X: IntoIterator,
    X::Item: AsRef<str>,
{
    let words: Vec<&str> = terms.iter().map(|t| t.word.as_str()).collect();
    kairos_plain::terms_used(texts, &words)
        .into_iter()
        .map(|index| &terms[index])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terms_are_sorted_and_a_bad_row_is_a_problem() {
        let dictionary = OrganizationDictionary::from_document(
            Some("X-D-0001".to_string()),
            Some(
                "| Word | Part of speech | Meaning |\n|---|---|---|\n\
                 | production | noun | Code that runs for the users. |\n\
                 | Board | noun | A set of columns. |\n\
                 | lane | noun |\n",
            ),
        );
        let words: Vec<&str> = dictionary.words().collect();
        assert_eq!(words, ["Board", "production"]);
        assert_eq!(dictionary.problems.len(), 1, "{:?}", dictionary.problems);
        assert_eq!(dictionary.problems[0].line, 5);
    }

    #[test]
    fn no_document_is_an_empty_dictionary() {
        assert_eq!(
            OrganizationDictionary::from_document(None, None),
            OrganizationDictionary::default()
        );
    }

    #[test]
    fn the_sql_reads_the_board_strategy_and_the_template_dictionary() {
        let sql = organization_dictionary_sql();
        assert!(sql.contains("db.slug = 'strategy'"), "{sql}");
        assert!(sql.contains("dtp.slug = 'dictionary'"), "{sql}");
    }
}
