//! `kairos boards dispatch` — the webhook targets, the agent dispatch rules
//! and the delivery log of a board (KAIROS-T-0384, KAIROS-T-0385).

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

use kairos_client::types_dispatch::{
    CreateDispatchRuleRequest, CreateWebhookTargetRequest, DispatchRule, RedeliverQuery,
    WebhookDeliveriesQuery, WebhookDelivery, WebhookTarget,
};

/// The dispatch of a board to agent runners.
#[derive(clap::Subcommand, Debug)]
pub enum DispatchCommand {
    /// Show the dispatch rules and the webhook targets of a board
    List {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        #[command(flatten)]
        common: Common,
    },
    /// Make a webhook target (needs configure_boards). The command shows the
    /// secret this one time
    #[command(name = "add-target")]
    AddTarget {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// A name for people, e.g. "Agent runner"
        #[arg(long)]
        name: String,
        /// The http:// or https:// URL of the receiver
        #[arg(long = "hook-url", value_name = "URL")]
        hook_url: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a webhook target, with its rules and its log (needs
    /// configure_boards and --confirm)
    #[command(name = "remove-target")]
    RemoveTarget {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The id of the target
        #[arg(value_name = "TARGET")]
        target: String,
        /// Required: without it, the command deletes nothing
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Turn a webhook target on again after Kairos turned it off, and send
    /// its skipped deliveries of the last 30 days (needs configure_boards)
    #[command(name = "enable-target")]
    EnableTarget {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The id of the target
        #[arg(value_name = "TARGET")]
        target: String,
        #[command(flatten)]
        common: Common,
    },
    /// Send the skipped deliveries of a webhook target that is on again,
    /// oldest first (needs configure_boards)
    Redeliver {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The id of the target
        #[arg(value_name = "TARGET")]
        target: String,
        /// Only the events at or after this time (RFC 3339). Without it:
        /// each skipped delivery of the last 30 days
        #[arg(long, value_name = "TIME")]
        since: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Send a test delivery (the event ping) to a webhook target (needs
    /// configure_boards)
    #[command(name = "test-target")]
    TestTarget {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The id of the target
        #[arg(value_name = "TARGET")]
        target: String,
        #[command(flatten)]
        common: Common,
    },
    /// Make a dispatch rule: a task that enters COLUMN and matches the
    /// filter sends one signed delivery to the target (needs
    /// configure_boards)
    #[command(name = "add-rule")]
    AddRule {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// When the rule fires: entered_column (a task enters --column),
        /// unblocked (the last open blocker of a task closes) or
        /// review_changed (the review state of a task changes)
        #[arg(long, default_value = "entered_column")]
        trigger: String,
        /// Column name or id (UUID). Required for entered_column; for
        /// unblocked and review_changed, the column the task must be in
        #[arg(long)]
        column: Option<String>,
        /// The id of a webhook target of the board
        #[arg(long)]
        target: String,
        /// Only tasks of this kind: task, bug, tech_debt or support. The old
        /// name --task-type still works
        #[arg(long = "kind", alias = "task-type", value_name = "KIND")]
        task_type: Option<String>,
        /// Only tasks with this metadata value, as SLUG=VALUE (e.g.
        /// priority=high)
        #[arg(long, value_name = "SLUG=VALUE")]
        metadata: Option<String>,
        /// Only tasks of this repository (slug or UUID). The old name
        /// --repository still works
        #[arg(long = "repo", alias = "repository", value_name = "REPOSITORY")]
        repository: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Show or set who decides the triage of intake (KAIROS-T-0428): with
    /// --on an agent decides; with --off an agent proposes with a note and
    /// a person decides. Setting needs configure_boards
    #[command(name = "agent-triage")]
    AgentTriage {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// An agent decides
        #[arg(long, conflicts_with = "off")]
        on: bool,
        /// An agent proposes, a person decides
        #[arg(long)]
        off: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a dispatch rule (needs configure_boards and --confirm). Its
    /// deliveries stay in the log
    #[command(name = "remove-rule")]
    RemoveRule {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The id of the rule
        #[arg(value_name = "RULE")]
        rule: String,
        /// Required: without it, the command deletes nothing
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show the delivery log of a board, newest first, with each try
    Log {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// Only the deliveries of this target (id)
        #[arg(long)]
        target: Option<String>,
        /// Only the deliveries of this rule (id)
        #[arg(long)]
        rule: Option<String>,
        /// The newest deliveries to show, 1 to 200 (50)
        #[arg(long)]
        limit: Option<i64>,
        /// Show the deliveries older than this delivery (id): the next page
        #[arg(long)]
        before: Option<String>,
        #[command(flatten)]
        common: Common,
    },
}

/// `SLUG=VALUE` of `--metadata`. Pure.
pub fn split_metadata(raw: &str) -> Result<(String, String), CliError> {
    match raw.split_once('=') {
        Some((slug, value)) if !slug.trim().is_empty() && !value.trim().is_empty() => {
            Ok((slug.trim().to_string(), value.trim().to_string()))
        }
        _ => Err(CliError::Failure(format!(
            "--metadata {raw:?} is not SLUG=VALUE. Send for example --metadata priority=high."
        ))),
    }
}

/// What `enable-target` and `redeliver` queued again (KAIROS-T-0577).
/// Pure.
pub fn redelivered_text(target: &WebhookTarget) -> String {
    match target.redelivered.unwrap_or(0) {
        0 => format!(
            "Kairos had no skipped delivery of {} to send again.",
            target.name
        ),
        1 => format!("Kairos queued 1 skipped delivery of {} again.", target.name),
        n => format!(
            "Kairos queued {n} skipped deliveries of {} again, oldest first.",
            target.name
        ),
    }
}

/// The filter of a rule in words. Pure.
/// When a rule fires (KAIROS-T-0427).
fn when_text(rule: &DispatchRule) -> String {
    match (rule.trigger.as_str(), rule.column.as_deref()) {
        ("unblocked", Some(column)) => format!("in {column} that is unblocked"),
        ("unblocked", None) => "that is unblocked".to_string(),
        ("review_changed", Some(column)) => format!("in {column} whose review state changes"),
        ("review_changed", None) => "whose review state changes".to_string(),
        (_, column) => format!("that enters {}", column.unwrap_or("?")),
    }
}

fn filter_text(rule: &DispatchRule) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &rule.task_type {
        parts.push(format!("type {t}"));
    }
    if let (Some(slug), Some(value)) = (&rule.metadata_slug, &rule.metadata_value) {
        parts.push(format!("{slug}={value}"));
    }
    if let Some(repo) = &rule.repository {
        parts.push(format!("repository {repo}"));
    }
    if parts.is_empty() {
        "each task".to_string()
    } else {
        parts.join(", ")
    }
}

fn target_state(target: &WebhookTarget) -> String {
    if target.enabled {
        format!("on ({} failed in a row)", target.consecutive_failures)
    } else {
        "off".to_string()
    }
}

fn delivery_rows(deliveries: &[WebhookDelivery]) -> Table {
    let mut table = Table::new(&[
        "CREATED", "DELIVERY", "EVENT", "TASK", "STATUS", "TRIES", "CODE", "ERROR",
    ]);
    for d in deliveries {
        table.row(vec![
            d.created_at.clone(),
            d.id.clone(),
            d.event.clone(),
            d.short_code.clone().unwrap_or_else(|| "-".into()),
            d.status.clone(),
            d.tries.to_string(),
            d.response_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".into()),
            d.last_error.clone().unwrap_or_default(),
        ]);
    }
    table
}

impl DispatchCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List { board, common } => {
                let client = client(&common)?;
                let rules = client.list_dispatch_rules(&board).await?;
                let targets = client.list_webhook_targets(&board).await?;
                if common.json {
                    return print_json(&serde_json::json!({
                        "rules": rules,
                        "targets": targets,
                    }));
                }
                let mut table = Table::new(&["RULE", "TRIGGER", "COLUMN", "FILTER", "TARGET"]);
                for rule in &rules {
                    table.row(vec![
                        rule.id.clone(),
                        rule.trigger.clone(),
                        rule.column.clone().unwrap_or_else(|| "-".into()),
                        filter_text(rule),
                        rule.target.clone(),
                    ]);
                }
                print!("{}", table.render());
                println!();
                let mut table = Table::new(&["TARGET", "NAME", "URL", "STATE"]);
                for target in &targets {
                    table.row(vec![
                        target.id.clone(),
                        target.name.clone(),
                        target.url.clone(),
                        target_state(target),
                    ]);
                }
                print!("{}", table.render());
                Ok(())
            }
            Self::AddTarget {
                board,
                name,
                hook_url,
                common,
            } => {
                let client = client(&common)?;
                let created = client
                    .create_webhook_target(
                        &board,
                        &CreateWebhookTargetRequest {
                            name,
                            url: hook_url,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&created);
                }
                println!(
                    "Made the webhook target {} ({}) for {}.",
                    created.target.name, created.target.id, created.target.url
                );
                println!("secret: {}", created.secret);
                println!(
                    "Kairos shows the secret this one time. Keep it in the receiver: \
                     X-Kairos-Signature is sha256=<hex HMAC-SHA256 of the body with this secret>."
                );
                Ok(())
            }
            Self::RemoveTarget {
                board,
                target,
                confirm,
                common,
            } => {
                require_confirm(
                    confirm,
                    &format!(
                        "the webhook target {target} of the board {board} (with its rules and its log)"
                    ),
                )?;
                let client = client(&common)?;
                let removed = client.delete_webhook_target(&board, &target).await?;
                if common.json {
                    return print_json(&removed);
                }
                println!(
                    "Removed the webhook target {} ({}), with its rules and its log.",
                    removed.name, removed.id
                );
                Ok(())
            }
            Self::EnableTarget {
                board,
                target,
                common,
            } => {
                let client = client(&common)?;
                let target = client.enable_webhook_target(&board, &target).await?;
                if common.json {
                    return print_json(&target);
                }
                println!("The webhook target {} ({}) is on.", target.name, target.id);
                println!("{}", redelivered_text(&target));
                Ok(())
            }
            Self::Redeliver {
                board,
                target,
                since,
                common,
            } => {
                let client = client(&common)?;
                let target = client
                    .redeliver_webhook_target(&board, &target, &RedeliverQuery { since })
                    .await?;
                if common.json {
                    return print_json(&target);
                }
                println!("{}", redelivered_text(&target));
                Ok(())
            }
            Self::TestTarget {
                board,
                target,
                common,
            } => {
                let client = client(&common)?;
                let delivery = client.test_webhook_target(&board, &target).await?;
                if common.json {
                    return print_json(&delivery);
                }
                println!(
                    "Queued the test delivery {} ({}). `kairos boards dispatch log {board}` \
                     shows its result.",
                    delivery.id, delivery.status
                );
                Ok(())
            }
            Self::AddRule {
                board,
                trigger,
                column,
                target,
                task_type,
                metadata,
                repository,
                common,
            } => {
                let (metadata_slug, metadata_value) = match metadata.as_deref() {
                    Some(raw) => {
                        let (slug, value) = split_metadata(raw)?;
                        (Some(slug), Some(value))
                    }
                    None => (None, None),
                };
                let client = client(&common)?;
                let rule = client
                    .create_dispatch_rule(
                        &board,
                        &CreateDispatchRuleRequest {
                            trigger: Some(trigger),
                            column,
                            target_id: target,
                            task_type,
                            metadata_slug,
                            metadata_value,
                            repository,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&rule);
                }
                println!(
                    "Made the dispatch rule {}: {} {} goes to {}.",
                    rule.id,
                    filter_text(&rule),
                    when_text(&rule),
                    rule.target
                );
                Ok(())
            }
            Self::AgentTriage {
                board,
                on,
                off,
                common,
            } => {
                let client = client(&common)?;
                let setting = if on || off {
                    client.set_agent_triage(&board, on).await?
                } else {
                    client.get_agent_triage(&board).await?
                };
                if common.json {
                    return print_json(&setting);
                }
                println!(
                    "Triage of intake on {board}: {}.",
                    if setting.agents_decide {
                        "an agent decides"
                    } else {
                        "an agent proposes with a note, a person decides"
                    }
                );
                Ok(())
            }
            Self::RemoveRule {
                board,
                rule,
                confirm,
                common,
            } => {
                require_confirm(
                    confirm,
                    &format!("the dispatch rule {rule} of the board {board}"),
                )?;
                let client = client(&common)?;
                let removed = client.delete_dispatch_rule(&board, &rule).await?;
                if common.json {
                    return print_json(&removed);
                }
                println!(
                    "Removed the dispatch rule {} ({}).",
                    removed.id,
                    when_text(&removed)
                );
                Ok(())
            }
            Self::Log {
                board,
                target,
                rule,
                limit,
                before,
                common,
            } => {
                let client = client(&common)?;
                let deliveries = client
                    .list_webhook_deliveries(
                        &board,
                        &WebhookDeliveriesQuery {
                            target_id: target,
                            rule_id: rule,
                            limit,
                            before,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&deliveries);
                }
                print!("{}", delivery_rows(&deliveries).render());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metadata_is_slug_equals_value() {
        assert_eq!(
            split_metadata("priority=high").unwrap(),
            ("priority".to_string(), "high".to_string())
        );
        assert_eq!(
            split_metadata(" area = web ").unwrap(),
            ("area".to_string(), "web".to_string())
        );
        for bad in ["priority", "=high", "priority="] {
            assert!(split_metadata(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_answer_counts_the_deliveries_queued_again() {
        let target = |redelivered| WebhookTarget {
            id: "t1".into(),
            board_id: "b1".into(),
            name: "Runner".into(),
            url: "https://hooks.example.com/run".into(),
            enabled: true,
            consecutive_failures: 0,
            disabled_at: None,
            disabled_reason: None,
            created_by: "u1".into(),
            created_at: "2026-10-17T09:00:00Z".into(),
            redelivered,
        };
        assert_eq!(
            redelivered_text(&target(Some(0))),
            "Kairos had no skipped delivery of Runner to send again."
        );
        assert_eq!(
            redelivered_text(&target(None)),
            "Kairos had no skipped delivery of Runner to send again."
        );
        assert_eq!(
            redelivered_text(&target(Some(1))),
            "Kairos queued 1 skipped delivery of Runner again."
        );
        assert_eq!(
            redelivered_text(&target(Some(3))),
            "Kairos queued 3 skipped deliveries of Runner again, oldest first."
        );
    }
}
