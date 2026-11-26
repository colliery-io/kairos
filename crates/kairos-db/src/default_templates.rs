//! The default templates of an organization (KAIROS-T-0790, COLLIERY-I-0683,
//! KAIROS-A-0087): one for each item type and task type, and the default
//! document types. The content of each one is a file in
//! `crates/kairos-db/default_templates/`. Each heading has italic guidance:
//! plain text that says how to fill in the section.
//!
//! - A new organization gets them: [`crate::tenant::seed_system_defaults`]
//!   writes them to `public.system_templates`, and the provisioning copies
//!   them.
//! - The migrations `system_templates_with_guidance` (public) and
//!   `templates_with_guidance` (tenant) upgrade what exists: a template with
//!   the content of [`DefaultTemplate::replaces`] (nobody changed it) gets the
//!   new content; a changed one stays; a missing type is added. The tests of
//!   this module check that the migrations have the same text as the files.

/// One default template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultTemplate {
    pub slug: &'static str,
    pub name: &'static str,
    /// `strategy`, `initiative`, `task`, `document` or `adr`.
    pub item_type: &'static str,
    /// The task type of a task template.
    pub task_type: Option<&'static str>,
    /// When to use the type.
    pub use_when: &'static str,
    /// The content: the starter markdown with its guidance.
    pub content: &'static str,
    /// The content of the seed before KAIROS-T-0790, for a template that
    /// existed then. The upgrade replaces only a template with this content.
    pub replaces: Option<&'static str>,
}

/// The default templates, in the order of the list of the web.
pub const DEFAULT_TEMPLATES: [DefaultTemplate; 18] = [
    DefaultTemplate {
        slug: "task",
        name: "Task",
        item_type: "task",
        task_type: Some("task"),
        use_when: "Use when the team can do the work in one piece, with one result that a person can check.",
        content: include_str!("../default_templates/task.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "bug",
        name: "Bug",
        item_type: "task",
        task_type: Some("bug"),
        use_when: "Use when the product does not do what it should do.",
        content: include_str!("../default_templates/bug.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "tech_debt",
        name: "Tech debt",
        item_type: "task",
        task_type: Some("tech_debt"),
        use_when: "Use when the code works, but a part of it makes the next change slow, risky or hard to read.",
        content: include_str!("../default_templates/tech_debt.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "support",
        name: "Support",
        item_type: "task",
        task_type: Some("support"),
        use_when: "Use when a person or a team asks for help, and nobody has decided what to do yet.",
        content: include_str!("../default_templates/support.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "initiative",
        name: "Initiative",
        item_type: "initiative",
        task_type: None,
        use_when: "Use when one objective needs more than one task, or the work of more than one person or team.",
        content: include_str!("../default_templates/initiative.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "strategy",
        name: "Strategy",
        item_type: "strategy",
        task_type: None,
        use_when: "Use when the organization bets on a direction for many months, and many initiatives serve it.",
        content: include_str!("../default_templates/strategy.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "adr",
        name: "ADR",
        item_type: "adr",
        task_type: None,
        use_when: "Use when you make a decision that is hard to change later, or that other teams must follow.",
        content: include_str!("../default_templates/adr.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "design_doc",
        name: "Design Doc",
        item_type: "document",
        task_type: None,
        use_when: "Use when you design a new system or a large feature. Start here: this document says which other documents the design needs.",
        content: include_str!("../default_templates/design_doc.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "prd",
        name: "PRD",
        item_type: "document",
        task_type: None,
        use_when: "Use when you must say what a system must do, so that a team can build it and test it.",
        content: include_str!("../default_templates/prd.md"),
        replaces: Some(
            "# Product Requirements Document\n\n## Problem\n\n## Goals\n\n## Non-Goals\n\n## Requirements\n\n## Open Questions\n",
        ),
    },
    DefaultTemplate {
        slug: "system_context",
        name: "System Context",
        item_type: "document",
        task_type: None,
        use_when: "Use when you must show who uses a system and which other systems it depends on.",
        content: include_str!("../default_templates/system_context.md"),
        replaces: Some(
            "# System Context\n\n## Overview\n\n## Actors\n\n## External Systems\n\n## Data Flows\n",
        ),
    },
    DefaultTemplate {
        slug: "architecture_framing",
        name: "Architecture Framing",
        item_type: "document",
        task_type: None,
        use_when: "Use when a design has open decisions, and the team must agree on each question before it chooses.",
        content: include_str!("../default_templates/architecture_framing.md"),
        replaces: Some(
            "# Architecture Framing\n\n## Context\n\n## Constraints\n\n## Options Considered\n\n## Decision Drivers\n",
        ),
    },
    DefaultTemplate {
        slug: "runbook",
        name: "Runbook",
        item_type: "document",
        task_type: None,
        use_when: "Use when a service runs in production and a person on call must keep it working.",
        content: include_str!("../default_templates/runbook.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "postmortem",
        name: "Postmortem",
        item_type: "document",
        task_type: None,
        use_when: "Use when an incident is over. Learn from it and track the fixes. Write it without blame.",
        content: include_str!("../default_templates/postmortem.md"),
        replaces: None,
    },
    DefaultTemplate {
        slug: "team_charter",
        name: "Team Charter",
        item_type: "document",
        task_type: None,
        use_when: "Use when a team starts, or its mission or its responsibilities change.",
        content: include_str!("../default_templates/team_charter.md"),
        replaces: Some(
            "# Team Charter\n\n## Mission\n\n## Scope\n\n## Ways of Working\n\n## Success Measures\n",
        ),
    },
    DefaultTemplate {
        slug: "social_contract",
        name: "Social Contract",
        item_type: "document",
        task_type: None,
        use_when: "Use when a team agrees how its members work together.",
        content: include_str!("../default_templates/social_contract.md"),
        replaces: Some(
            "# Social Contract\n\n## Values\n\n## Agreements\n\n## Communication\n\n## Conflict Resolution\n",
        ),
    },
    DefaultTemplate {
        slug: "company_vision",
        name: "Company Vision",
        item_type: "document",
        task_type: None,
        use_when: "Use when the organization says where it wants to be in some years. The organization has one.",
        content: include_str!("../default_templates/company_vision.md"),
        replaces: Some(
            "# Company Vision\n\n## Where We Are Going\n\n## Why It Matters\n\n## How We Will Know\n",
        ),
    },
    DefaultTemplate {
        slug: "product_vision",
        name: "Product Vision",
        item_type: "document",
        task_type: None,
        use_when: "Use when you say what a product, a repository or a capability is for, and where it goes. It is not the vision of the organization.",
        content: include_str!("../default_templates/product_vision.md"),
        replaces: Some(
            "# Product Vision\n\n## Purpose\n\n## Who It Is For\n\n## Current State\n\n## Future State\n\n## Principles\n\n## What It Is Not\n",
        ),
    },
    DefaultTemplate {
        slug: "dictionary",
        name: "Dictionary",
        item_type: "document",
        task_type: None,
        use_when: "Use when you define a term of the organization. The organization has one dictionary, on the board strategy.",
        content: include_str!("../default_templates/dictionary.md"),
        replaces: Some(
            "# Dictionary\n\nThe terms of the organization. Write one row for each term. The organization has one dictionary, on the board strategy.\n\n| Word | Part of speech | Meaning |\n|---|---|---|\n",
        ),
    },
];

/// The SQL literal of a text, as the migrations of KAIROS-T-0790 write it:
/// `E'…'`, with `\`, `'` and the newline escaped. Pure.
pub fn sql_literal(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n");
    format!("E'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT_UP: &str =
        include_str!("../migrations/tenant/2026-11-25-000200_templates_with_guidance/up.sql");
    const PUBLIC_UP: &str = include_str!(
        "../migrations/public/2026-11-25-000200_system_templates_with_guidance/up.sql"
    );

    /// One template for each fixed type, and each fixed type has one.
    #[test]
    fn each_fixed_type_has_one_template() {
        let fixed = [
            ("task", Some("task")),
            ("task", Some("bug")),
            ("task", Some("tech_debt")),
            ("task", Some("support")),
            ("initiative", None),
            ("strategy", None),
            ("adr", None),
        ];
        for (item_type, task_type) in fixed {
            let count = DEFAULT_TEMPLATES
                .iter()
                .filter(|t| t.item_type == item_type && t.task_type == task_type)
                .count();
            assert_eq!(count, 1, "{item_type} {task_type:?}");
        }
        let mut slugs: Vec<&str> = DEFAULT_TEMPLATES.iter().map(|t| t.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), DEFAULT_TEMPLATES.len(), "each slug one time");
    }

    /// The shape of each template: guidance under each `##` heading (or a
    /// `###` heading that has it), no title (the item has one), no emoji,
    /// and a "use when" that starts so.
    #[test]
    fn each_template_has_guidance_and_no_title_or_emoji() {
        for template in DEFAULT_TEMPLATES {
            let slug = template.slug;
            assert!(template.use_when.starts_with("Use when "), "{slug}");
            assert!(
                !template.content.lines().any(|l| l.starts_with("# ")),
                "{slug}: a title"
            );
            assert!(
                !template
                    .content
                    .chars()
                    .any(|c| ('\u{2600}'..='\u{27BF}').contains(&c) || c >= '\u{1F000}'),
                "{slug}: an emoji"
            );
            let lines: Vec<&str> = template.content.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if !line.starts_with("## ") {
                    continue;
                }
                let next = lines[index + 1..]
                    .iter()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| panic!("{slug}: nothing under {line}"));
                let guidance = next.starts_with('*') && !next.starts_with("**");
                assert!(
                    guidance || next.starts_with("### "),
                    "{slug}: no guidance under {line}"
                );
            }
        }
    }

    /// The guidance and the "use when" pass the plain-language check
    /// (KAIROS-S-0009).
    #[test]
    fn the_guidance_is_plain_language() {
        let mut failures = Vec::new();
        for template in DEFAULT_TEMPLATES {
            let mut texts = vec![template.use_when.to_string()];
            texts.extend(
                template
                    .content
                    .lines()
                    .filter(|l| l.starts_with('*') && !l.starts_with("**") && l.ends_with('*'))
                    .map(|l| l.trim_matches('*').to_string()),
            );
            for text in texts {
                for violation in kairos_plain::check_text(&text) {
                    failures.push(format!(
                        "{}: {} {}: {text:?}",
                        template.slug, violation.rule, violation.message
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The migrations have the text of the files: a change of a file needs
    /// a new migration (the old ones are released and do not change).
    #[test]
    fn the_migrations_have_the_text_of_the_files() {
        for template in DEFAULT_TEMPLATES {
            let content = sql_literal(template.content);
            let use_when = sql_literal(template.use_when);
            for (name, migration) in [("tenant", TENANT_UP), ("public", PUBLIC_UP)] {
                assert!(
                    migration.contains(&content),
                    "{name} migration: the content of {}",
                    template.slug
                );
                assert!(
                    migration.contains(&use_when),
                    "{name} migration: the use_when of {}",
                    template.slug
                );
                if let Some(old) = template.replaces {
                    assert!(
                        migration.contains(&sql_literal(old)),
                        "{name} migration: the old content of {}",
                        template.slug
                    );
                }
            }
        }
    }
}
