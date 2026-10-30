//! The stored check of the typed sections of each document (KAIROS-T-0731,
//! COLLIERY-I-0664 decision 3).
//!
//! An MCP write that does not pass the typed sections of its template is
//! refused (KAIROS-T-0730). Old content, and a REST write, is not refused:
//! it gets a mark. The mark is a row of `section_checks`: one row for each
//! document whose template has typed sections and that the server checked,
//! with the number of violations (0: the document passes) and the failing
//! sections as JSON ([`StoredFailure`]).
//!
//! - Each content write of a document checks the whole new content in its
//!   transaction ([`on_document_write`]): `items::create_document`, the
//!   content writes of `items` (REST and MCP update and edit, rollback) and
//!   `items::set_document_board`. So a fix clears the mark at once.
//! - A write of the typed sections of a template ([`mark_template_stale`])
//!   sets `stale` on the checks of its documents. A change of the
//!   dictionary of the organization ([`mark_dictionary_stale`],
//!   [`mark_dictionaries_stale`]) sets `stale` on each check: each document
//!   uses that dictionary (KAIROS-A-0079). They check nothing: the number of
//!   documents can be large.
//! - The periodic pass `section-checks` ([`pass`]) checks, a page at a
//!   time, each document with typed sections that has no check (the old
//!   documents: the backfill) or a stale check. It removes the stale check
//!   of a document whose template has no typed section now.
//!
//! The check of a newer version wins: a write of an older version
//! changes no row ([`store`]).

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Integer, Nullable, Text, Uuid as SqlUuid};
use kairos_core::sections::{SectionFailure, SectionProblem, SectionType, check_sections};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The documents that one run of [`pass`] checks in one organization, at
/// most. The next run checks the next ones.
pub const PASS_LIMIT: i64 = 500;

/// The documents that one page of [`check_pending`] reads.
pub const PAGE: i64 = 100;

/// One rule that a section breaks, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredViolation {
    pub rule: String,
    pub text: String,
    pub fix: String,
}

/// A section that does not pass, as stored in `section_checks.failures`
/// and as the refusal of an MCP write gives it in `details.sections`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFailure {
    pub heading: String,
    /// `free` or `plain`.
    #[serde(rename = "type")]
    pub section_type: String,
    /// `missing`, `empty` or `violations`.
    pub problem: String,
    #[serde(default)]
    pub violations: Vec<StoredViolation>,
}

impl From<&SectionFailure> for StoredFailure {
    fn from(failure: &SectionFailure) -> Self {
        let (problem, violations) = match &failure.problem {
            SectionProblem::Missing => ("missing", Vec::new()),
            SectionProblem::Empty => ("empty", Vec::new()),
            SectionProblem::Violations(violations) => (
                "violations",
                violations
                    .iter()
                    .map(|v| StoredViolation {
                        rule: v.rule.to_string(),
                        text: v.text.clone(),
                        fix: v.fix.clone(),
                    })
                    .collect(),
            ),
        };
        StoredFailure {
            heading: failure.heading.clone(),
            section_type: failure.section_type.as_str().to_string(),
            problem: problem.to_string(),
            violations,
        }
    }
}

impl StoredFailure {
    /// The number of problems of the section: 1 for a missing or an empty
    /// section, else each violation.
    pub fn count(&self) -> usize {
        self.violations.len().max(1)
    }

    /// One line of text for each problem of the section, as the refusal of
    /// an MCP write and `get_item` give them.
    pub fn lines(&self) -> Vec<String> {
        let name = format!("Section {:?} ({})", self.heading, self.section_type);
        match self.problem.as_str() {
            "missing" => vec![format!(
                "{name}: the content does not have this section. Add the heading \"## {}\" \
                 and text under it.",
                self.heading
            )],
            "empty" => vec![format!(
                "{name}: the section has no text. Write text under the heading."
            )],
            _ => self
                .violations
                .iter()
                .map(|v| format!("{name}, {}: {:?}. {}", v.rule, v.text, v.fix))
                .collect(),
        }
    }
}

/// The number of problems of `failures`.
pub fn violation_count(failures: &[StoredFailure]) -> usize {
    failures.iter().map(StoredFailure::count).sum()
}

/// The stored check of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCheck {
    /// The version of the document that the server checked.
    pub version: i32,
    pub violations: i32,
    pub failures: Vec<StoredFailure>,
    /// The template sections or a dictionary changed after the check.
    pub stale: bool,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

impl StoredCheck {
    /// The document passes its typed sections.
    pub fn passes(&self) -> bool {
        self.violations == 0
    }
}

#[derive(QueryableByName)]
struct CheckRow {
    #[diesel(sql_type = Integer)]
    version: i32,
    #[diesel(sql_type = Integer)]
    violations: i32,
    #[diesel(sql_type = Text)]
    failures: String,
    #[diesel(sql_type = Bool)]
    stale: bool,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    checked_at: chrono::DateTime<chrono::Utc>,
}

/// The stored check of the document `document_id`, when there is one.
pub fn of_document(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<Option<StoredCheck>, DieselError> {
    let row: Option<CheckRow> = sql_query(
        "SELECT version, violations, failures::text AS failures, stale, checked_at \
           FROM section_checks WHERE document_id = $1",
    )
    .bind::<SqlUuid, _>(document_id)
    .get_result(conn)
    .optional()?;
    row.map(|row| {
        Ok(StoredCheck {
            version: row.version,
            violations: row.violations,
            failures: parse_failures(&row.failures)?,
            stale: row.stale,
            checked_at: row.checked_at,
        })
    })
    .transpose()
}

/// The failures of `section_checks.failures`, as text.
pub fn parse_failures(json: &str) -> Result<Vec<StoredFailure>, DieselError> {
    serde_json::from_str(json).map_err(|e| DieselError::DeserializationError(Box::new(e)))
}

/// The terms of the dictionary of the organization, for a document on any
/// board (KAIROS-A-0079). With no dictionary, none.
pub fn dictionary_terms(conn: &mut PgConnection) -> Result<Vec<String>, DieselError> {
    let dictionary = crate::dictionaries::organization_dictionary(conn)?;
    Ok(dictionary.terms.into_iter().map(|e| e.word).collect())
}

/// What the check of one document reads: one statement.
#[derive(QueryableByName)]
struct Inputs {
    #[diesel(sql_type = Integer)]
    version: i32,
    #[diesel(sql_type = Nullable<Text>)]
    template_slug: Option<String>,
    /// Empty when the caller has the content.
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Array<Text>)]
    headings: Vec<String>,
    #[diesel(sql_type = Array<Text>)]
    types: Vec<String>,
}

impl Inputs {
    /// The typed sections. A type that this server does not know (a later
    /// server wrote it) gets the check of `free`.
    fn sections(&self) -> Vec<(String, SectionType)> {
        self.headings
            .iter()
            .zip(&self.types)
            .map(|(heading, t)| (heading.clone(), t.parse().unwrap_or(SectionType::Free)))
            .collect()
    }
}

fn read_inputs(
    conn: &mut PgConnection,
    document_id: Uuid,
    with_content: bool,
) -> Result<Option<Inputs>, DieselError> {
    sql_query(
        "SELECT d.version, t.slug AS template_slug, \
                CASE WHEN $2 THEN d.content ELSE '' END AS content, \
                ARRAY(SELECT ts.heading FROM template_sections ts \
                       WHERE ts.template_id = d.template_id \
                       ORDER BY ts.position, ts.heading) AS headings, \
                ARRAY(SELECT ts.section_type FROM template_sections ts \
                       WHERE ts.template_id = d.template_id \
                       ORDER BY ts.position, ts.heading) AS types \
           FROM documents d LEFT JOIN templates t ON t.id = d.template_id \
          WHERE d.id = $1",
    )
    .bind::<SqlUuid, _>(document_id)
    .bind::<Bool, _>(with_content)
    .get_result(conn)
    .optional()
}

/// Write the check of `version` of a document. A stored check of a newer
/// version stays. One statement.
fn store(
    conn: &mut PgConnection,
    document_id: Uuid,
    version: i32,
    failures: &[StoredFailure],
) -> Result<(), DieselError> {
    let count = i32::try_from(violation_count(failures)).unwrap_or(i32::MAX);
    let json = serde_json::to_string(failures)
        .map_err(|e| DieselError::SerializationError(Box::new(e)))?;
    sql_query(
        "INSERT INTO section_checks (document_id, version, violations, failures, stale, checked_at) \
         VALUES ($1, $2, $3, $4::jsonb, false, now()) \
         ON CONFLICT (document_id) DO UPDATE \
            SET version = EXCLUDED.version, violations = EXCLUDED.violations, \
                failures = EXCLUDED.failures, stale = false, checked_at = now() \
          WHERE section_checks.version <= EXCLUDED.version",
    )
    .bind::<SqlUuid, _>(document_id)
    .bind::<Integer, _>(version)
    .bind::<Integer, _>(count)
    .bind::<Text, _>(json)
    .execute(conn)?;
    Ok(())
}

/// The failures of `content` against `sections`, with the dictionary of
/// the organization. `vocabulary` keeps the vocabulary for a pass: one
/// read of the dictionary.
fn check(
    conn: &mut PgConnection,
    sections: &[(String, SectionType)],
    content: &str,
    vocabulary: &mut Option<kairos_plain::Vocabulary>,
) -> Result<Vec<StoredFailure>, DieselError> {
    // A `free` section reads no vocabulary: no read of the dictionary.
    let free_only;
    let vocabulary = if sections.iter().any(|(_, t)| *t == SectionType::Plain) {
        match vocabulary {
            Some(vocabulary) => vocabulary,
            None => vocabulary.insert(kairos_plain::Vocabulary::standard(dictionary_terms(conn)?)),
        }
    } else {
        free_only = kairos_plain::Vocabulary::default();
        &free_only
    };
    Ok(check_sections(content, sections, vocabulary)
        .iter()
        .map(StoredFailure::from)
        .collect())
}

/// The check of a content write of a document, in the transaction of the
/// write: `content` is the new content, at `version`. A document whose
/// template has typed sections gets its check written. A document of the
/// template `dictionary` sets `stale` on each check
/// ([`mark_dictionary_stale`]). Other documents: one read, no write.
pub fn on_document_write(
    conn: &mut PgConnection,
    document_id: Uuid,
    version: i32,
    content: &str,
) -> Result<(), DieselError> {
    let Some(inputs) = read_inputs(conn, document_id, false)? else {
        return Ok(());
    };
    if inputs.template_slug.as_deref() == Some(crate::dictionaries::DICTIONARY_TEMPLATE_SLUG) {
        mark_dictionary_stale(conn)?;
    }
    let sections = inputs.sections();
    if sections.is_empty() {
        return Ok(());
    }
    let failures = check(conn, &sections, content, &mut None)?;
    store(conn, document_id, version, &failures)
}

/// Check the document `document_id` as it is now, and write its check.
/// Gives `false` when the document is not there or has no typed section.
pub fn check_document(
    conn: &mut PgConnection,
    document_id: Uuid,
    vocabulary: &mut Option<kairos_plain::Vocabulary>,
) -> Result<bool, DieselError> {
    let Some(inputs) = read_inputs(conn, document_id, true)? else {
        return Ok(false);
    };
    let sections = inputs.sections();
    if sections.is_empty() {
        return Ok(false);
    }
    let failures = check(conn, &sections, &inputs.content, vocabulary)?;
    store(conn, document_id, inputs.version, &failures)?;
    Ok(true)
}

/// The typed sections of the template `template_id` changed: each check of
/// a document of the template is stale. One statement.
pub fn mark_template_stale(
    conn: &mut PgConnection,
    template_id: Uuid,
) -> Result<usize, DieselError> {
    sql_query(
        "UPDATE section_checks SET stale = true \
          WHERE NOT stale \
            AND document_id IN (SELECT id FROM documents WHERE template_id = $1)",
    )
    .bind::<SqlUuid, _>(template_id)
    .execute(conn)
}

/// The dictionary of the organization changed (written, archived or
/// restored): each check is stale, because each document uses that
/// dictionary (KAIROS-A-0079). One statement.
pub fn mark_dictionary_stale(conn: &mut PgConnection) -> Result<usize, DieselError> {
    sql_query("UPDATE section_checks SET stale = true WHERE NOT stale").execute(conn)
}

/// [`mark_dictionary_stale`] when one of the documents `ids` is a
/// dictionary (an archive or a restore of items). One statement; nothing
/// when no id is a dictionary.
pub fn mark_dictionaries_stale(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<usize, DieselError> {
    sql_query(
        "UPDATE section_checks SET stale = true \
          WHERE NOT stale \
            AND EXISTS (SELECT 1 FROM documents dd \
                          JOIN templates dt ON dt.id = dd.template_id \
                         WHERE dd.id = ANY($1) AND dt.slug = $2)",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .bind::<Text, _>(crate::dictionaries::DICTIONARY_TEMPLATE_SLUG)
    .execute(conn)
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// The outcome of [`check_pending`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingOutcome {
    /// The documents checked.
    pub checked: usize,
    /// The stale checks removed: their template has no typed section now.
    pub removed: usize,
}

/// Check up to `limit` documents of this organization that have typed
/// sections and no check, or a stale check, a page of [`PAGE`] at a time,
/// each page in a transaction of its own. First, remove each stale check of
/// a document whose template has no typed section now. `progress` runs
/// after each page.
pub fn check_pending(
    conn: &mut PgConnection,
    limit: i64,
    progress: &mut dyn FnMut(),
) -> Result<PendingOutcome, DieselError> {
    let removed = sql_query(
        "DELETE FROM section_checks c \
          WHERE c.stale AND NOT EXISTS ( \
                SELECT 1 FROM documents d \
                  JOIN template_sections ts ON ts.template_id = d.template_id \
                 WHERE d.id = c.document_id)",
    )
    .execute(conn)?;
    let mut outcome = PendingOutcome {
        checked: 0,
        removed,
    };
    let mut vocabulary = None;
    let mut after = Uuid::nil();
    while (outcome.checked as i64) < limit {
        let page = PAGE.min(limit - outcome.checked as i64);
        let ids: Vec<IdRow> = sql_query(
            "SELECT d.id FROM documents d \
              WHERE d.id > $1 AND d.template_id IS NOT NULL \
                AND EXISTS (SELECT 1 FROM template_sections ts \
                             WHERE ts.template_id = d.template_id) \
                AND NOT EXISTS (SELECT 1 FROM section_checks c \
                                 WHERE c.document_id = d.id AND NOT c.stale) \
              ORDER BY d.id LIMIT $2",
        )
        .bind::<SqlUuid, _>(after)
        .bind::<BigInt, _>(page)
        .load(conn)?;
        let Some(last) = ids.last() else {
            break;
        };
        after = last.id;
        let full = ids.len() as i64 == page;
        conn.transaction::<_, DieselError, _>(|conn| {
            for row in &ids {
                if check_document(conn, row.id, &mut vocabulary)? {
                    outcome.checked += 1;
                }
            }
            Ok(())
        })?;
        progress();
        if !full {
            break;
        }
    }
    Ok(outcome)
}

/// One run of the periodic pass `section-checks`: [`check_pending`] in
/// each organization, up to `limit` documents each. The number of
/// documents checked and of checks removed.
pub fn pass(
    conn: &mut PgConnection,
    limit: i64,
    progress: &mut dyn FnMut(),
) -> Result<PendingOutcome, crate::tenant::TenantError> {
    let mut total = PendingOutcome::default();
    for tenant_info in crate::tenant::list_tenants(conn)? {
        progress();
        if !tenant_info.schema_exists {
            continue;
        }
        let schema = crate::tenant::tenant_schema_name(&tenant_info.slug);
        sql_query(format!("SET search_path TO \"{schema}\", public")).execute(conn)?;
        let result = check_pending(conn, limit, progress);
        let reset = sql_query("SET search_path TO DEFAULT").execute(conn);
        let one = result?;
        reset?;
        total.checked += one.checked;
        total.removed += one.removed;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_core::sections::SectionViolation;

    fn failures() -> Vec<SectionFailure> {
        vec![
            SectionFailure {
                heading: "Summary".into(),
                section_type: SectionType::Free,
                problem: SectionProblem::Missing,
            },
            SectionFailure {
                heading: "Goal".into(),
                section_type: SectionType::Plain,
                problem: SectionProblem::Violations(vec![
                    SectionViolation {
                        rule: "STE-V1",
                        text: "utilize".into(),
                        fix: "Write \"use\".".into(),
                    },
                    SectionViolation {
                        rule: "STE-G1",
                        text: "is checked".into(),
                        fix: "Passive.".into(),
                    },
                ]),
            },
        ]
    }

    #[test]
    fn a_missing_section_counts_one_and_each_violation_counts_one() {
        let stored: Vec<StoredFailure> = failures().iter().map(StoredFailure::from).collect();
        assert_eq!(violation_count(&stored), 3);
        assert_eq!(stored[0].problem, "missing");
        assert_eq!(stored[1].violations[1].rule, "STE-G1");
    }

    #[test]
    fn the_stored_json_has_the_shape_of_the_refusal_details() {
        let stored: Vec<StoredFailure> = failures().iter().map(StoredFailure::from).collect();
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json[0]["type"], "free");
        assert_eq!(json[0]["problem"], "missing");
        assert_eq!(json[0]["violations"], serde_json::json!([]));
        assert_eq!(json[1]["violations"][0]["text"], "utilize");
        let back = parse_failures(&json.to_string()).unwrap();
        assert_eq!(back, stored);
    }

    #[test]
    fn each_problem_has_one_line() {
        let stored: Vec<StoredFailure> = failures().iter().map(StoredFailure::from).collect();
        assert_eq!(
            stored[0].lines(),
            vec![
                "Section \"Summary\" (free): the content does not have this section. Add the \
                 heading \"## Summary\" and text under it."
                    .to_string()
            ]
        );
        assert_eq!(
            stored[1].lines()[0],
            "Section \"Goal\" (plain), STE-V1: \"utilize\". Write \"use\"."
        );
        assert_eq!(stored[1].lines().len(), 2);
    }
}
