//! `kairos boards` — list boards and render one board's items grouped by
//! column (KAIROS-A-0015: "board views per level with column layout";
//! `GET /api/boards` + `GET /api/boards/{id}/items`).

use crate::commands::entities::{EntityView, ListArgs};
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

use kairos_client::types_org::BoardItemsResponse;

/// Operations on boards.
#[derive(clap::Subcommand, Debug)]
pub enum BoardsCommand {
    /// List boards (paginated)
    List(ListArgs),
    /// Show a board's live items grouped by column
    Show {
        /// Board id (UUID)
        board_id: String,
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
            Self::Show { board_id, common } => {
                let client = client(&common)?;
                let items = client.board_items(&board_id).await?;
                if common.json {
                    return print_json(&items);
                }
                print_board_items(&items);
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
            println!("   {}  [strategy] {}", item.short_code(), item.title());
        }
        for item in &column.initiatives {
            println!("   {}  [initiative] {}", item.short_code(), item.title());
        }
        for item in &column.tasks {
            println!(
                "   {}  [{}] {}",
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
