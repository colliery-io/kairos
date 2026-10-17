//! `kairos boards` — list boards and render one board's items grouped by
//! column (KAIROS-A-0015: "board views per level with column layout";
//! `GET /api/boards` + `GET /api/boards/{id}/items`).

use crate::commands::entities::ListArgs;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::{Table, print_page_footer};

use kairos_client::types_org::{
    BoardItemsQuery, BoardItemsResponse, CreateBoardRequest, UpdateBoardRequest,
    UpdateColumnRequest,
};

/// Operations on boards.
#[derive(clap::Subcommand, Debug)]
pub enum BoardsCommand {
    /// List boards (paginated)
    List(ListArgs),
    /// Create a board: an ADR board, a strategy or an initiative board, or the
    /// delivery board of a team that has none (KAIROS-T-0337)
    Create {
        /// Board name
        #[arg(long)]
        name: String,
        /// Board slug (^[a-z][a-z0-9_-]{1,62}$)
        #[arg(long)]
        slug: String,
        /// strategy|initiative|delivery|adr
        #[arg(long)]
        level: String,
        /// The owner team, by slug or id (UUID). A delivery board needs one. An
        /// ADR board with a team is the ADR board of that team. A strategy or
        /// initiative board with no team gets its own empty team
        #[arg(long)]
        team: Option<String>,
        /// The short-code prefix of the board, for example OPS (a capital
        /// letter, then 1 to 9 capital letters or digits). The ADR board of a
        /// team takes the prefix of the team. It does not change later.
        #[arg(long = "code-prefix", value_name = "PREFIX")]
        code_prefix: String,
        #[command(flatten)]
        common: Common,
    },
    /// Show a board's live items grouped by column (one page)
    Show {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board_id: String,
        /// Page size (server default 200, max 1000)
        #[arg(long)]
        limit: Option<i64>,
        /// Items to skip
        #[arg(long)]
        offset: Option<i64>,
        /// Show only the strategies and the initiatives of this team (slug
        /// or UUID), from tasks or set by hand. Tasks and ADRs are not
        /// changed
        #[arg(long, conflicts_with = "no_team")]
        team: Option<String>,
        /// Show only the strategies and the initiatives that have no team
        #[arg(long)]
        no_team: bool,
        /// Show more on each item: the date of the last change, the claim
        /// of a task and the metadata values, e.g. {updated 2026-10-08;
        /// priority: high}
        #[arg(long)]
        details: bool,
        #[command(flatten)]
        common: Common,
    },
    /// The flow metrics of a board for a range of days: cycle time,
    /// throughput per week (cancelled apart), the age of the work in
    /// progress, and the cumulative flow by column
    Flow {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// The first day, YYYY-MM-DD (default: 89 days before --to)
        #[arg(long)]
        from: Option<String>,
        /// The last day, YYYY-MM-DD (default: today, UTC)
        #[arg(long)]
        to: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Set or remove the WIP limit of a column (needs configure_boards). A
    /// move into a column over its limit is allowed, with a warning
    #[command(name = "wip-limit")]
    WipLimit {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// Column name or id (UUID)
        #[arg(value_name = "COLUMN")]
        column: String,
        /// The limit, a number above 0
        #[arg(long, conflicts_with = "none", required_unless_present = "none")]
        limit: Option<i32>,
        /// Remove the limit
        #[arg(long)]
        none: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show or set the policies of a board (KAIROS-T-0641, KAIROS-T-0643).
    /// Setting needs configure_boards. parent_gate (a delivery board): a task
    /// moves to a claims column only while its parent initiative is active.
    /// owner_gate (an initiative board): an initiative moves to Active only
    /// with an owner
    Policy {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// on or off
        #[arg(long, value_name = "on|off", value_parser = ["on", "off"])]
        parent_gate: Option<String>,
        /// on or off
        #[arg(long, value_name = "on|off", value_parser = ["on", "off"])]
        owner_gate: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// The columns of a board: list, add, update, remove (KAIROS-T-0443)
    Columns {
        #[command(subcommand)]
        command: crate::commands::columns::ColumnsCommand,
    },
    /// The allowed moves between the columns of a board: list, add, remove
    Transitions {
        #[command(subcommand)]
        command: crate::commands::columns::TransitionsCommand,
    },
    /// Send tasks to agent runners: the webhook targets, the dispatch rules
    /// and the delivery log of a board. A task that enters the column of a
    /// rule and matches its filter sends one signed delivery
    Dispatch {
        #[command(subcommand)]
        command: crate::commands::dispatch::DispatchCommand,
    },
    /// Archive each task in the done columns of a delivery board, cancelled
    /// tasks too (requires --confirm). You can restore each task
    #[command(name = "archive-completed")]
    ArchiveCompleted {
        /// Board slug or id (UUID)
        #[arg(value_name = "BOARD")]
        board: String,
        /// Archive the tasks
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
}

impl BoardsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Create {
                name,
                slug,
                level,
                team,
                code_prefix,
                common,
            } => {
                let client = client(&common)?;
                // The body takes the id of the team. A slug is read first.
                let team_id = match team {
                    Some(team) if looks_like_uuid(&team) => Some(team),
                    Some(team) => Some(client.get_team_by_slug(&team).await?.id),
                    None => None,
                };
                let detail = client
                    .create_board(&CreateBoardRequest {
                        name,
                        slug,
                        board_level: level,
                        team_id,
                        code_prefix,
                    })
                    .await?;
                if common.json {
                    return print_json(&detail);
                }
                let columns: Vec<&str> = detail.columns.iter().map(|c| c.name.as_str()).collect();
                println!(
                    "Created board {} ({}, {}, prefix {}). Columns: {}.",
                    detail.board.slug,
                    detail.board.name,
                    detail.board.board_level,
                    detail.board.code_prefix,
                    columns.join(", ")
                );
                Ok(())
            }
            Self::List(args) => {
                let client = client(&args.common)?;
                let envelope = client.list_boards(args.page()).await?;
                if args.common.json {
                    return print_json(&envelope);
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    let mut table = Table::new(&["ID", "NAME", "SLUG", "LEVEL", "TEAM"]);
                    for board in &envelope.items {
                        table.row(vec![
                            board.id.clone(),
                            board.name.clone(),
                            board.slug.clone(),
                            board.board_level.clone(),
                            board.team_id.clone().unwrap_or_else(|| "-".to_string()),
                        ]);
                    }
                    print!("{}", table.render());
                }
                print_page_footer(
                    envelope.total,
                    envelope.limit,
                    envelope.offset,
                    envelope.items.len(),
                );
                Ok(())
            }
            Self::Show {
                board_id,
                limit,
                offset,
                team,
                no_team,
                details,
                common,
            } => {
                let client = client(&common)?;
                // COLLIERY-T-0261: one page of the board.
                let query = BoardItemsQuery {
                    limit,
                    offset,
                    team,
                    no_team,
                    with_metadata: details,
                    ..BoardItemsQuery::default()
                };
                let items = client.board_items(&board_id, &query).await?;
                if common.json {
                    return print_json(&items);
                }
                print_board_items(&items, details);
                println!();
                println!(
                    "total: {} (limit {}, offset {})",
                    items.total, items.limit, items.offset
                );
                // The page is a part of the board: say so, and say how to
                // read the next part.
                if let Some(note) = items.incomplete_note("use --offset") {
                    println!("{note}");
                }
                Ok(())
            }
            Self::Dispatch { command } => command.run().await,
            Self::Columns { command } => command.run().await,
            Self::Transitions { command } => command.run().await,
            Self::Flow {
                board,
                from,
                to,
                common,
            } => {
                let client = client(&common)?;
                let flow = client
                    .board_flow(
                        &board,
                        &kairos_client::types_org::BoardFlowQuery { from, to },
                    )
                    .await?;
                if common.json {
                    return print_json(&flow);
                }
                print!("{}", flow_report(&flow));
                Ok(())
            }
            Self::WipLimit {
                board,
                column,
                limit,
                none,
                common,
            } => {
                let client = client(&common)?;
                let detail = client.get_board(&board).await?;
                let target = detail
                    .columns
                    .iter()
                    .find(|c| c.id == column || c.name.eq_ignore_ascii_case(&column))
                    .ok_or_else(|| {
                        CliError::Failure(format!(
                            "The board {board} has no column {column:?}. The columns are: {}.",
                            detail
                                .columns
                                .iter()
                                .map(|c| c.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })?;
                let wip_limit = if none { None } else { limit };
                let updated = client
                    .update_column(
                        &detail.board.id,
                        &target.id,
                        &UpdateColumnRequest {
                            name: None,
                            position: None,
                            is_done: None,
                            claims: None,
                            wip_limit: Some(wip_limit),
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&updated);
                }
                match updated.wip_limit {
                    Some(limit) => println!(
                        "The column {} of {} has the WIP limit {limit}.",
                        updated.name, detail.board.slug
                    ),
                    None => println!(
                        "The column {} of {} has no WIP limit.",
                        updated.name, detail.board.slug
                    ),
                }
                Ok(())
            }
            Self::Policy {
                board,
                parent_gate,
                owner_gate,
                common,
            } => {
                let client = client(&common)?;
                let mut current = client.get_board(&board).await?.board;
                let parent_gate = parent_gate.map(|v| v == "on");
                let owner_gate = owner_gate.map(|v| v == "on");
                if parent_gate.is_some() || owner_gate.is_some() {
                    current = client
                        .update_board(
                            &current.id,
                            &UpdateBoardRequest {
                                parent_gate,
                                owner_gate,
                                ..UpdateBoardRequest::default()
                            },
                        )
                        .await?;
                }
                if common.json {
                    return print_json(&current);
                }
                println!("{}", policy_text(&current));
                Ok(())
            }
            Self::ArchiveCompleted {
                board,
                confirm,
                common,
            } => {
                if !confirm {
                    return Err(CliError::Failure(format!(
                        "The archive of the completed tasks of {board} needs --confirm. Run \
                         the command again with --confirm."
                    )));
                }
                let client = client(&common)?;
                let response = client.archive_completed(&board).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("{}", archived_completed_text(&board, &response));
                Ok(())
            }
        }
    }
}

/// The text of `boards policy` (KAIROS-T-0641). Pure.
fn policy_text(board: &kairos_client::types_org::Board) -> String {
    let on_off = |on: bool| if on { "on" } else { "off" };
    let mut lines = Vec::new();
    if let Some(on) = board.parent_gate {
        lines.push(format!(
            "parent_gate: {} (a task moves to a claims column only while its initiative is \
             active)",
            on_off(on)
        ));
    }
    if let Some(on) = board.owner_gate {
        lines.push(format!(
            "owner_gate: {} (an initiative moves to Active only with an owner)",
            on_off(on)
        ));
    }
    if lines.is_empty() {
        return format!("The board {} has no policy.", board.slug);
    }
    format!("Policies of {}:\n  {}", board.slug, lines.join("\n  "))
}

/// The human board rendering: one section per column (in position order),
/// every live item listed with its kind, short code, and title.
fn print_board_items(items: &BoardItemsResponse, details: bool) {
    // KAIROS-T-0368: with --details, the date of the last change, the
    // claim of a task and the metadata values.
    let more = |code: &str, updated_at: &str, claim: Option<&kairos_client::types::TaskClaim>| {
        if !details {
            return String::new();
        }
        let mut parts = vec![format!(
            "updated {}",
            updated_at.get(..10).unwrap_or(updated_at)
        )];
        if let Some(claim) = claim {
            parts.push(format!(
                "claimed by {}{}",
                claim.display_name,
                if claim.agent { " (agent)" } else { "" }
            ));
        }
        if let Some(values) = items.item_metadata.as_ref().and_then(|m| m.get(code)) {
            parts.extend(
                values
                    .iter()
                    .map(|(slug, value)| format!("{slug}: {value}")),
            );
        }
        format!("  {{{}}}", parts.join("; "))
    };
    println!(
        "Board: {} (slug {}, level {}, id {})",
        items.board.name, items.board.slug, items.board.board_level, items.board.id
    );
    for column in &items.columns {
        let count = column.strategies.len()
            + column.initiatives.len()
            + column.tasks.len()
            + column.adrs.len();
        println!();
        println!(
            "== {} ({count} items, id {}){}",
            column.column.name,
            column.column.id,
            wip_text(column.item_count, column.column.wip_limit)
        );
        if count == 0 {
            println!("   (empty)");
            continue;
        }
        for item in &column.strategies {
            println!(
                "   {}  [strategy] {}{}{}",
                item.short_code.as_str(),
                item.title,
                teams_suffix(items, item.short_code.as_str()),
                more(item.short_code.as_str(), &item.updated_at, None)
            );
        }
        for item in &column.initiatives {
            println!(
                "   {}  [initiative] {}{}{}",
                item.short_code.as_str(),
                item.title,
                teams_suffix(items, item.short_code.as_str()),
                more(item.short_code.as_str(), &item.updated_at, None)
            );
        }
        for item in &column.tasks {
            // KAIROS-T-0077: the Support lane rides on the type tag;
            // Planned stays unmarked as the default lane.
            let lane = if item.work_class == "support" {
                " (support lane)"
            } else {
                ""
            };
            println!(
                "   {}  [{}]{lane} {}{}",
                item.short_code.as_str(),
                item.task_type,
                item.title,
                more(
                    item.short_code.as_str(),
                    &item.updated_at,
                    item.claim.as_ref()
                )
            );
        }
        for item in &column.adrs {
            println!(
                "   {}  [adr] {}{}",
                item.short_code.as_str(),
                item.title,
                more(item.short_code.as_str(), &item.updated_at, None)
            );
        }
    }
}

/// The text of `boards flow` (KAIROS-T-0372). Pure, host-tested.
fn flow_report(flow: &kairos_client::types_org::BoardFlow) -> String {
    let days = |value: Option<f64>| value.map_or_else(|| "-".to_string(), |v| format!("{v}"));
    let mut out = format!(
        "Flow of {} from {} to {}\nStart columns: {}. Done columns: {}.\nDone: {}. Cancelled: {} \
         (counted apart). Done with no start: {}.\n",
        flow.board_slug,
        flow.from,
        flow.to,
        if flow.start_columns.is_empty() {
            "none (no cycle time)".to_string()
        } else {
            flow.start_columns.join(", ")
        },
        flow.done_columns.join(", "),
        flow.done_count,
        flow.cancelled_count,
        flow.done_without_start
    );
    let cycle = &flow.cycle_time;
    out.push_str(&format!(
        "\nCycle time (days): count {}, mean {}, median {}, 85th percentile {}\n",
        cycle.count,
        days(cycle.mean_days),
        days(cycle.median_days),
        days(cycle.p85_days)
    ));
    let mut weeks = Table::new(&["WEEK", "DONE", "CANCELLED"]);
    for week in &flow.throughput {
        weeks.row(vec![
            week.week_start.clone(),
            week.done.to_string(),
            week.cancelled.to_string(),
        ]);
    }
    out.push_str("\nThroughput per week\n");
    out.push_str(&weeks.render());
    out.push_str("\nWork in progress (oldest first)\n");
    if flow.ageing.is_empty() {
        out.push_str("(none)\n");
    } else {
        let mut ageing = Table::new(&["CODE", "COLUMN", "AGE_DAYS", "TITLE"]);
        for item in &flow.ageing {
            ageing.row(vec![
                item.short_code.clone(),
                item.column.clone(),
                item.age_days.to_string(),
                item.title.clone(),
            ]);
        }
        out.push_str(&ageing.render());
    }
    let cfd = &flow.cumulative_flow;
    let mut header = vec!["DATE"];
    header.extend(cfd.columns.iter().map(String::as_str));
    let mut table = Table::new(&header);
    for day in cfd.days.iter().skip(cfd.days.len().saturating_sub(14)) {
        let mut row = vec![day.date.clone()];
        row.extend(day.counts.iter().map(i64::to_string));
        table.row(row);
    }
    out.push_str("\nCumulative flow (last 14 days; --json has each day)\n");
    out.push_str(&table.render());
    out
}

/// The WIP limit of a column after its name (KAIROS-T-0371):
/// ` [WIP 2/3]`, ` [WIP 4/3, over the limit]`, or nothing. Pure.
fn wip_text(count: i64, wip_limit: Option<i32>) -> String {
    match wip_limit {
        None => String::new(),
        Some(limit) if count > i64::from(limit) => {
            format!(" [WIP {count}/{limit}, over the limit]")
        }
        Some(limit) => format!(" [WIP {count}/{limit}]"),
    }
}

/// The teams of a strategy or an initiative, after its title
/// (KAIROS-T-0321): `  [teams: a, b]`, or nothing when it has no team.
fn teams_suffix(items: &BoardItemsResponse, short_code: &str) -> String {
    match items.item_teams.get(short_code) {
        Some(teams) if !teams.is_empty() => format!(
            "  [teams: {}]",
            teams
                .iter()
                .map(|team| team.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => String::new(),
    }
}

/// The text of `boards archive-completed` (KAIROS-T-0363).
fn archived_completed_text(
    board: &str,
    response: &kairos_client::types::ArchiveCompletedResponse,
) -> String {
    if response.count == 0 {
        return format!("The done columns of {board} have no tasks. Kairos archived nothing.");
    }
    format!(
        "Kairos archived {} completed task{} of {board}: {}.\nTo get one back, run `kairos \
         tasks restore <code>`.",
        response.count,
        if response.count == 1 { "" } else { "s" },
        response.short_codes.join(", ")
    )
}

/// Whether `value` has the form of a UUID (8-4-4-4-12 hex digits). A team
/// slug cannot have that form (COLLIERY-T-0260).
fn looks_like_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod archive_completed_tests {
    use super::*;

    /// KAIROS-T-0363: the text gives the count and the codes, and says
    /// how to restore.
    #[test]
    fn the_text_gives_the_count() {
        let response = |codes: &[&str]| kairos_client::types::ArchiveCompletedResponse {
            board_id: "b".into(),
            count: codes.len() as i64,
            short_codes: codes.iter().map(|c| c.to_string()).collect(),
        };
        assert_eq!(
            archived_completed_text("acme", &response(&[])),
            "The done columns of acme have no tasks. Kairos archived nothing."
        );
        let text = archived_completed_text("acme", &response(&["ACME-T-0001", "ACME-T-0002"]));
        assert!(
            text.starts_with(
                "Kairos archived 2 completed tasks of acme: ACME-T-0001, ACME-T-0002."
            ),
            "{text}"
        );
        assert!(text.contains("tasks restore"), "{text}");
    }
}

#[cfg(test)]
mod wip_tests {
    use super::wip_text;

    /// KAIROS-T-0371.
    #[test]
    fn the_wip_text_names_the_count_and_the_limit() {
        assert_eq!(wip_text(2, None), "");
        assert_eq!(wip_text(2, Some(3)), " [WIP 2/3]");
        assert_eq!(wip_text(3, Some(3)), " [WIP 3/3]");
        assert_eq!(wip_text(4, Some(3)), " [WIP 4/3, over the limit]");
    }

    /// KAIROS-T-0372.
    #[test]
    fn the_flow_report_has_the_four_parts() {
        use kairos_client::types_org::*;
        let flow = BoardFlow {
            board_id: "b".into(),
            board_slug: "web-delivery".into(),
            from: "2026-10-01".into(),
            to: "2026-10-11".into(),
            start_columns: vec!["Active".into()],
            done_columns: vec!["Done".into()],
            done_count: 1,
            cancelled_count: 1,
            cycle_time: FlowCycleTime {
                count: 1,
                mean_days: Some(2.0),
                median_days: Some(2.0),
                p85_days: Some(2.0),
                items: vec![],
            },
            done_without_start: 0,
            throughput: vec![FlowWeek {
                week_start: "2026-09-28".into(),
                done: 1,
                cancelled: 1,
            }],
            ageing: vec![FlowAgeingItem {
                short_code: "WEB-T-0002".into(),
                title: "Two".into(),
                column: "Active".into(),
                started_at: "2026-10-01T00:00:00Z".into(),
                age_days: 10.0,
            }],
            cumulative_flow: CumulativeFlow {
                columns: vec!["Active".into(), "Done".into()],
                days: vec![CumulativeFlowDay {
                    date: "2026-10-11".into(),
                    counts: vec![1, 2],
                }],
            },
        };
        let text = super::flow_report(&flow);
        assert!(
            text.contains("Done: 1. Cancelled: 1 (counted apart)."),
            "{text}"
        );
        assert!(text.contains("median 2, 85th percentile 2"), "{text}");
        assert!(text.contains("2026-09-28"), "{text}");
        assert!(text.contains("WEB-T-0002"), "{text}");
        assert!(text.contains("2026-10-11"), "{text}");
    }
}
