//! The check of the typed sections on an MCP write (KAIROS-T-0730,
//! COLLIERY-I-0664 decisions 1 and 4).
//!
//! `create_item`, `update_item` and `edit_item` call [`refuse_failing`] with
//! the content that they are about to write. When the template of the
//! document has typed sections and the content does not pass them, the
//! write is refused with `SECTIONS_NOT_PASSED` and nothing is saved. The
//! REST routes do not call this: a person in the web may write free text
//! (decision 1). The pure check is `kairos_core::sections::check_sections`.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_core::sections::{SectionFailure, SectionProblem, SectionType, check_sections};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::ApiError;

/// The most violations that the message of a refusal lists. `details` has
/// each one.
const MESSAGE_LINES: usize = 40;

/// The typed sections of a template: each heading and its type, in the
/// order of the template.
type Typed = Vec<(String, SectionType)>;

/// The typed sections of a template, in the order of the template.
pub(super) fn of_template(conn: &mut PgConnection, template_id: Uuid) -> Result<Typed, ApiError> {
    use kairos_db::schema::template_sections;
    let rows: Vec<(String, String)> = template_sections::table
        .filter(template_sections::template_id.eq(template_id))
        .order(template_sections::position.asc())
        .select((template_sections::heading, template_sections::section_type))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(typed(rows))
}

/// The typed sections of the template of a document, and the owner board
/// of the document. No sections when the document has no template, or its
/// template has no typed section. One statement.
pub(super) fn of_document(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<(Typed, Option<Uuid>), ApiError> {
    use kairos_db::schema::{documents, template_sections};
    let rows: Vec<(String, String, Uuid)> = template_sections::table
        .inner_join(
            documents::table
                .on(documents::template_id.eq(template_sections::template_id.nullable())),
        )
        .filter(documents::id.eq(document_id))
        .order(template_sections::position.asc())
        .select((
            template_sections::heading,
            template_sections::section_type,
            documents::board_id,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    let board = rows.first().map(|(_, _, board)| *board);
    let sections = typed(rows.into_iter().map(|(h, t, _)| (h, t)).collect());
    Ok((sections, board))
}

/// The stored rows as typed sections. A type that this server does not know
/// (a later server wrote it) gets the check of `free`: the section must be
/// there and have text.
fn typed(rows: Vec<(String, String)>) -> Typed {
    rows.into_iter()
        .map(|(heading, section_type)| {
            let section_type = section_type.parse().unwrap_or(SectionType::Free);
            (heading, section_type)
        })
        .collect()
}

/// The terms of the dictionary for a document of the owner board `board`
/// (COLLIERY-I-0664 decision 6). The seam for the dictionary of a team
/// (KAIROS-T-0733): until that task gives the terms of a team, there are
/// none, and the vocabulary is the vendored word list only.
fn dictionary_terms(
    _conn: &mut PgConnection,
    _board: Option<Uuid>,
) -> Result<Vec<String>, ApiError> {
    Ok(Vec::new())
}

/// Refuse `content` when it does not pass `sections`: `SECTIONS_NOT_PASSED`,
/// with each section and each violation named. `Ok` when there is no typed
/// section, or the content passes.
pub(super) fn refuse_failing(
    conn: &mut PgConnection,
    short_code: Option<&str>,
    sections: &[(String, SectionType)],
    board: Option<Uuid>,
    content: &str,
) -> Result<(), ApiError> {
    if sections.is_empty() {
        return Ok(());
    }
    let terms = if sections.iter().any(|(_, t)| *t == SectionType::Plain) {
        dictionary_terms(conn, board)?
    } else {
        Vec::new()
    };
    let vocabulary = kairos_plain::Vocabulary::standard(terms);
    let failures = check_sections(content, sections, &vocabulary);
    if failures.is_empty() {
        return Ok(());
    }
    Err(refusal(short_code, &failures))
}

/// The refusal of content that does not pass its typed sections.
pub(crate) fn refusal(short_code: Option<&str>, failures: &[SectionFailure]) -> ApiError {
    let subject = match short_code {
        Some(code) => format!("The content of {code}"),
        None => "The content".to_string(),
    };
    let mut message = format!(
        "{subject} does not pass the typed sections of its template. The server saved \
         nothing. Correct each section, and then send the write again."
    );
    let mut lines = 0usize;
    let mut not_listed = 0usize;
    for failure in failures {
        let name = format!("Section {:?} ({})", failure.heading, failure.section_type);
        let texts: Vec<String> = match &failure.problem {
            SectionProblem::Missing => vec![format!(
                "{name}: the content does not have this section. Add the heading \"## {}\" \
                 and text under it.",
                failure.heading
            )],
            SectionProblem::Empty => vec![format!(
                "{name}: the section has no text. Write text under the heading."
            )],
            SectionProblem::Violations(violations) => violations
                .iter()
                .map(|v| format!("{name}, {}: {:?}. {}", v.rule, v.text, v.fix))
                .collect(),
        };
        for text in texts {
            if lines < MESSAGE_LINES {
                message.push_str("\n- ");
                message.push_str(&text);
                lines += 1;
            } else {
                not_listed += 1;
            }
        }
    }
    if not_listed > 0 {
        message.push_str(&format!(
            "\n- The list stops here. The details give the {not_listed} other violations."
        ));
    }
    ApiError::unprocessable("SECTIONS_NOT_PASSED", message).with_details(json!({
        "field": "content",
        "sections": failures.iter().map(failure_json).collect::<Vec<Value>>(),
    }))
}

fn failure_json(failure: &SectionFailure) -> Value {
    let (problem, violations) = match &failure.problem {
        SectionProblem::Missing => ("missing", Vec::new()),
        SectionProblem::Empty => ("empty", Vec::new()),
        SectionProblem::Violations(violations) => (
            "violations",
            violations
                .iter()
                .map(|v| json!({ "rule": v.rule, "text": v.text, "fix": v.fix }))
                .collect(),
        ),
    };
    json!({
        "heading": failure.heading,
        "type": failure.section_type.as_str(),
        "problem": problem,
        "violations": violations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_core::sections::SectionViolation;

    #[test]
    fn the_refusal_names_each_section_and_each_violation() {
        let failures = vec![
            SectionFailure {
                heading: "Summary".into(),
                section_type: SectionType::Free,
                problem: SectionProblem::Missing,
            },
            SectionFailure {
                heading: "Goal".into(),
                section_type: SectionType::Plain,
                problem: SectionProblem::Violations(vec![SectionViolation {
                    rule: "STE-V1",
                    text: "utilize".into(),
                    fix: "\"utilize\" is not in the vocabulary. Write \"use\", not \"utilize\"."
                        .into(),
                }]),
            },
        ];
        let error = refusal(Some("ACME-D-0001"), &failures);
        assert_eq!(error.code, "SECTIONS_NOT_PASSED");
        assert_eq!(error.status.as_u16(), 422);
        assert!(
            error
                .message
                .starts_with("The content of ACME-D-0001 does not pass")
        );
        assert!(error.message.contains(
            "\n- Section \"Summary\" (free): the content does not have this section. Add the \
             heading \"## Summary\" and text under it."
        ));
        assert!(error.message.contains(
            "\n- Section \"Goal\" (plain), STE-V1: \"utilize\". \"utilize\" is not in the \
             vocabulary."
        ));
        assert_eq!(error.details["field"], "content");
        assert_eq!(error.details["sections"][0]["problem"], "missing");
        assert_eq!(
            error.details["sections"][1]["violations"][0]["rule"],
            "STE-V1"
        );
        assert_eq!(
            error.details["sections"][1]["violations"][0]["text"],
            "utilize"
        );
    }

    #[test]
    fn a_long_list_stops_in_the_message_and_not_in_the_details() {
        let violations: Vec<SectionViolation> = (0..50)
            .map(|i| SectionViolation {
                rule: "STE-V1",
                text: format!("w{i}"),
                fix: "Fix it.".into(),
            })
            .collect();
        let failures = vec![SectionFailure {
            heading: "Goal".into(),
            section_type: SectionType::Plain,
            problem: SectionProblem::Violations(violations),
        }];
        let error = refusal(None, &failures);
        assert_eq!(error.message.matches("\n- Section").count(), MESSAGE_LINES);
        assert!(error.message.contains("the 10 other violations"));
        assert_eq!(
            error.details["sections"][0]["violations"]
                .as_array()
                .unwrap()
                .len(),
            50
        );
    }

    #[test]
    fn an_unknown_stored_type_is_checked_as_free() {
        let sections = typed(vec![("Goal".into(), "ace".into())]);
        assert_eq!(sections, vec![("Goal".to_string(), SectionType::Free)]);
    }
}
