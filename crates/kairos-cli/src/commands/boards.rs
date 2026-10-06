//! `kairos boards` — list boards and render one board's items grouped by
//! column (KAIROS-A-0015: "board views per level with column layout";
//! `GET /api/boards` + `GET /api/boards/{id}/items`).

use crate::commands::entities::{EntityView, ListArgs};
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

use kairos_client::types_org::{BoardItemsQuery, BoardItemsResponse};

/// Operations on boards.
#[derive(clap::Subcommand, Debug)]
pub enum BoardsCommand {
    /// List boards (paginated)
    List(ListArgs),
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
        #[command(flatten)]
        common: Common,
    },
}

impl BoardsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
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
                println!(
                    "total: {} (limit {}, offset {})",
                    envelope.total, envelope.limit, envelope.offset
                );
                Ok(())
            }
            Self::Show {
                board_id,
                limit,
                offset,
                team,
                no_team,
                common,
            } => {
                let client = client(&common)?;
                // COLLIERY-T-0261: one page of the board.
                let query = BoardItemsQuery {
                    limit,
                    offset,
                    team,
                    no_team,
                    ..BoardItemsQuery::default()
                };
                let items = client.board_items(&board_id, &query).await?;
                if common.json {
                    return print_json(&items);
                }
                print_board_items(&items);
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
        }
    }
}

/// The human board rendering: one section per column (in position order),
/// every live item listed with its kind, short code, and title.
fn print_board_items(items: &BoardItemsResponse) {
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
            "== {} ({count} items, id {})",
            column.column.name, column.column.id
        );
        if count == 0 {
            println!("   (empty)");
            continue;
        }
        for item in &column.strategies {
            println!(
                "   {}  [strategy] {}{}",
                item.short_code(),
                item.title(),
                teams_suffix(items, item.short_code())
            );
        }
        for item in &column.initiatives {
            println!(
                "   {}  [initiative] {}{}",
                item.short_code(),
                item.title(),
                teams_suffix(items, item.short_code())
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
                "   {}  [{}]{lane} {}",
                item.short_code(),
                item.task_type,
                item.title()
            );
        }
        for item in &column.adrs {
            println!("   {}  [adr] {}", item.short_code(), item.title());
        }
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
