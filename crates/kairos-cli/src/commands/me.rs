//! `kairos me` — the work of the caller (KAIROS-T-0489,
//! `GET /api/me/work`): the initiatives that you own, and your tasks, each
//! with its state (Todo, Active, Blocked).

use clap::Args;

use kairos_client::types_work::{MyWork, WorkItem};

use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Your work: the initiatives you own, and the tasks you or your agent keys
/// claim, the tasks under your initiatives, and the tasks these block.
#[derive(Args, Debug, Default)]
pub struct MeArgs {
    #[command(flatten)]
    pub common: Common,
}

impl MeArgs {
    pub async fn run(self) -> Result<(), CliError> {
        let client = client(&self.common)?;
        let work = client.my_work().await?;
        if self.common.json {
            return print_json(&work);
        }
        print!("{}", render(&work));
        Ok(())
    }
}

/// The states in the order of the page.
const STATES: [(&str, &str); 3] = [
    ("todo", "Todo"),
    ("active", "Active"),
    ("blocked", "Blocked"),
];

/// Who has the item: the claimant of a task, or the owner.
fn holder(item: &WorkItem) -> String {
    if let Some(claim) = &item.claim {
        return if claim.agent {
            format!("{} (agent)", claim.display_name)
        } else {
            claim.display_name.clone()
        };
    }
    if let Some(owner) = &item.owner {
        return owner.display_name.clone();
    }
    "-".to_string()
}

/// The human rendering: one section for each lane, one table for each
/// state.
pub fn render(work: &MyWork) -> String {
    let mut out = String::new();
    for (lane, items) in [("Initiatives", &work.initiatives), ("Tasks", &work.tasks)] {
        out.push_str(&format!("{lane} ({}):\n", items.len()));
        if items.is_empty() {
            out.push_str("  (none)\n");
            continue;
        }
        for (state, heading) in STATES {
            let in_state: Vec<&WorkItem> = items.iter().filter(|i| i.state == state).collect();
            if in_state.is_empty() {
                continue;
            }
            out.push_str(&format!("  {heading}:\n"));
            let mut table =
                Table::new(&["    CODE", "TITLE", "BOARD", "COLUMN", "WHO", "BLOCKED BY"]);
            for item in in_state {
                let blockers: Vec<&str> = item
                    .blockers
                    .iter()
                    .map(|b| b.short_code.as_str())
                    .collect();
                table.row(vec![
                    format!("    {}", item.short_code),
                    item.title.clone(),
                    item.board.slug.clone(),
                    item.column_name.clone(),
                    holder(item),
                    if blockers.is_empty() {
                        "-".to_string()
                    } else {
                        blockers.join(", ")
                    },
                ]);
            }
            out.push_str(&table.render());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_client::types::TaskClaim;
    use kairos_client::types_work::{WorkBlocker, WorkBoard};

    fn item(code: &str, state: &str) -> WorkItem {
        WorkItem {
            id: "00000000-0000-0000-0000-000000000000".into(),
            short_code: code.into(),
            title: format!("Title of {code}"),
            entity_type: "task".into(),
            state: state.into(),
            reasons: vec!["claimed".into()],
            board: WorkBoard {
                id: "00000000-0000-0000-0000-000000000000".into(),
                slug: "web".into(),
                name: "Web".into(),
            },
            column_id: "00000000-0000-0000-0000-000000000000".into(),
            column_name: "Active".into(),
            claim: None,
            owner: None,
            blockers: Vec::new(),
        }
    }

    #[test]
    fn the_lanes_group_by_state() {
        let mut blocked = item("WEB-T-0002", "blocked");
        blocked.blockers.push(WorkBlocker {
            short_code: "WEB-T-0009".into(),
            title: "The blocker".into(),
        });
        let mut active = item("WEB-T-0001", "active");
        active.claim = Some(TaskClaim {
            user_id: "x".into(),
            display_name: "Robin".into(),
            user_name: "robin".into(),
            agent: true,
            claimed_at: "2026-10-15T00:00:00Z".into(),
        });
        let text = render(&MyWork {
            initiatives: Vec::new(),
            tasks: vec![active, blocked],
        });
        assert!(text.contains("Initiatives (0):\n  (none)\n"), "{text}");
        let active_at = text.find("  Active:").expect("Active");
        let blocked_at = text.find("  Blocked:").expect("Blocked");
        assert!(active_at < blocked_at, "{text}");
        assert!(text.contains("Robin (agent)"), "{text}");
        assert!(text.contains("WEB-T-0009"), "{text}");
        assert!(!text.contains("  Todo:"), "{text}");
    }
}
