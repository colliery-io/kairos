//! The mark of a document that does not pass the typed sections of its
//! template (KAIROS-T-0731, COLLIERY-I-0664 decision 3).
//!
//! The server stores the check of each document whose template has typed
//! sections, and `GET /api/documents/{short_code}` gives it as
//! `section_check`. The page shows a badge next to the version
//! ([`SectionsBadge`]) and the list of the problems ([`SectionsPanel`]).
//! Nothing is blocked: a person can save free text. A save that fixes the
//! sections clears the mark.

use aurora_dark::components::{Pill, Stack, Text};
use aurora_dark::tokens::token;
use aurora_dark::widgets::Banner;
use leptos::prelude::*;

use super::api::SectionCheck;

/// The text of the badge, when the document does not pass. Pure.
pub fn badge_text(check: &SectionCheck) -> Option<String> {
    if check.passes {
        return None;
    }
    let noun = if check.violations == 1 {
        "violation"
    } else {
        "violations"
    };
    Some(format!("does not pass: {} {noun}", check.violations))
}

/// One line for each problem of the check, in the order of the template.
/// Pure.
pub fn problem_lines(check: &SectionCheck) -> Vec<String> {
    let mut lines = Vec::new();
    for section in &check.sections {
        let name = format!("{} ({})", section.heading, section.section_type);
        match section.problem.as_str() {
            "missing" => lines.push(format!(
                "{name}: the section is not in the document. Add the heading and text under it."
            )),
            "empty" => lines.push(format!("{name}: the section has no text.")),
            _ => {
                for violation in &section.violations {
                    lines.push(format!(
                        "{name}, {}: \"{}\". {}",
                        violation.rule, violation.text, violation.fix
                    ));
                }
            }
        }
    }
    lines
}

/// The badge next to the version, when the document does not pass.
#[component]
pub fn SectionsBadge(check: Option<SectionCheck>) -> impl IntoView {
    check.as_ref().and_then(badge_text).map(|text| {
        view! {
            <span class="kairos-sections-badge" data-testid="sections-badge">
                <Pill color=token::BAD>{text}</Pill>
            </span>
        }
    })
}

/// The list of the problems, when the document does not pass.
#[component]
pub fn SectionsPanel(check: Option<SectionCheck>) -> impl IntoView {
    let check = check.filter(|check| !check.passes)?;
    let lines = problem_lines(&check);
    let stale = check.stale.then(|| {
        view! {
            <Text size="xs" dimmed=true attr:data-testid="sections-stale">
                "The template or a dictionary changed. Kairos checks this document again soon."
            </Text>
        }
    });
    Some(view! {
        <div class="kairos-item__sections" data-testid="sections-panel">
            <Banner color=token::BAD icon="!">
                <Stack gap="xs">
                    <Text bright=true bold=true>
                        "This document does not pass the typed sections of its template."
                    </Text>
                    <ul class="kairos-item__sections-list" data-testid="sections-list">
                        {lines
                            .into_iter()
                            .map(|line| view! {
                                <li data-testid="sections-problem"><Text size="sm">{line}</Text></li>
                            })
                            .collect_view()}
                    </ul>
                    {stale}
                    <Text size="xs" dimmed=true>
                        "Correct each section and save. The mark goes when the document passes."
                    </Text>
                </Stack>
            </Banner>
        </div>
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::item::api::{SectionCheckFailure, SectionCheckViolation};

    fn check(passes: bool) -> SectionCheck {
        SectionCheck {
            passes,
            violations: if passes { 0 } else { 2 },
            stale: false,
            sections: if passes {
                Vec::new()
            } else {
                vec![
                    SectionCheckFailure {
                        heading: "Summary".into(),
                        section_type: "free".into(),
                        problem: "missing".into(),
                        violations: Vec::new(),
                    },
                    SectionCheckFailure {
                        heading: "Goal".into(),
                        section_type: "plain".into(),
                        problem: "violations".into(),
                        violations: vec![SectionCheckViolation {
                            rule: "STE-V1".into(),
                            text: "utilize".into(),
                            fix: "Write \"use\".".into(),
                        }],
                    },
                ]
            },
        }
    }

    #[test]
    fn a_passing_document_has_no_badge() {
        assert_eq!(badge_text(&check(true)), None);
        assert!(problem_lines(&check(true)).is_empty());
    }

    #[test]
    fn a_failing_document_has_the_count_and_one_line_for_each_problem() {
        assert_eq!(
            badge_text(&check(false)).as_deref(),
            Some("does not pass: 2 violations")
        );
        assert_eq!(
            problem_lines(&check(false)),
            vec![
                "Summary (free): the section is not in the document. Add the heading and text \
                 under it."
                    .to_string(),
                "Goal (plain), STE-V1: \"utilize\". Write \"use\".".to_string(),
            ]
        );
    }
}
