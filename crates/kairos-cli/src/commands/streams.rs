//! `kairos streams` — delivery streams and their team membership (S-0005
//! `/api/delivery-streams`).

use kairos_client::types_org::{
    AddStreamTeamRequest, CreateStreamRequest, DeliveryStream, UpdateStreamRequest,
};

use crate::commands::entities::{ListArgs, require_confirm};
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Operations on delivery streams.
#[derive(clap::Subcommand, Debug)]
pub enum StreamsCommand {
    /// List delivery streams (paginated)
    List(ListArgs),
    /// Show one delivery stream
    Get {
        /// Stream id (UUID)
        stream_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Create a delivery stream
    Create {
        /// Stream name
        #[arg(long)]
        name: String,
        /// Stream slug
        #[arg(long)]
        slug: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Update a stream's name, slug, and/or description
    Update {
        /// Stream id (UUID)
        stream_id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New slug
        #[arg(long)]
        slug: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Soft-delete a delivery stream (requires --confirm)
    Delete {
        /// Stream id (UUID)
        stream_id: String,
        /// Actually delete
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Teams belonging to a stream
    #[command(subcommand)]
    Teams(StreamTeamsCommand),
}

/// Team membership of one delivery stream.
#[derive(clap::Subcommand, Debug)]
pub enum StreamTeamsCommand {
    /// List the teams in a stream
    List {
        /// Stream id (UUID)
        stream_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Add a team to a stream
    Add {
        /// Stream id (UUID)
        stream_id: String,
        /// Team id (UUID)
        #[arg(long, value_name = "TEAM_ID")]
        team: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a team from a stream
    Remove {
        /// Stream id (UUID)
        stream_id: String,
        /// Team id (UUID)
        #[arg(long, value_name = "TEAM_ID")]
        team: String,
        #[command(flatten)]
        common: Common,
    },
}

fn stream_table(streams: &[DeliveryStream]) -> Table {
    let mut table = Table::new(&["ID", "NAME", "SLUG", "DESCRIPTION"]);
    for stream in streams {
        table.row(vec![
            stream.id.clone(),
            stream.name.clone(),
            stream.slug.clone(),
            stream
                .description
                .clone()
                .unwrap_or_else(|| "-".to_string()),
        ]);
    }
    table
}

impl StreamsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List(args) => {
                let client = client(&args.common)?;
                let envelope = client.list_streams(args.page()).await?;
                if args.common.json {
                    return print_json(&envelope);
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", stream_table(&envelope.items).render());
                }
                println!(
                    "total: {} (limit {}, offset {})",
                    envelope.total, envelope.limit, envelope.offset
                );
                Ok(())
            }
            Self::Get { stream_id, common } => {
                let client = client(&common)?;
                let stream = client.get_stream(&stream_id).await?;
                if common.json {
                    return print_json(&stream);
                }
                print!("{}", stream_table(std::slice::from_ref(&stream)).render());
                Ok(())
            }
            Self::Create {
                name,
                slug,
                description,
                common,
            } => {
                let client = client(&common)?;
                let stream = client
                    .create_stream(&CreateStreamRequest {
                        name,
                        slug,
                        description,
                    })
                    .await?;
                if common.json {
                    return print_json(&stream);
                }
                println!(
                    "Created delivery stream {} ({}, id {})",
                    stream.name, stream.slug, stream.id
                );
                Ok(())
            }
            Self::Update {
                stream_id,
                name,
                slug,
                description,
                common,
            } => {
                if name.is_none() && slug.is_none() && description.is_none() {
                    return Err(CliError::Failure(
                        "nothing to update: pass --name, --slug, or --description".to_string(),
                    ));
                }
                let client = client(&common)?;
                let stream = client
                    .update_stream(
                        &stream_id,
                        &UpdateStreamRequest {
                            name,
                            slug,
                            description,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&stream);
                }
                println!(
                    "Updated delivery stream {} ({}, id {})",
                    stream.name, stream.slug, stream.id
                );
                Ok(())
            }
            Self::Delete {
                stream_id,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("delivery stream {stream_id}"))?;
                let client = client(&common)?;
                let response = client.delete_stream(&stream_id).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Deleted delivery stream {}", response.id);
                Ok(())
            }
            Self::Teams(command) => command.run().await,
        }
    }
}

impl StreamTeamsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List { stream_id, common } => {
                let client = client(&common)?;
                let teams = client.list_stream_teams(&stream_id).await?;
                if common.json {
                    return print_json(&teams);
                }
                if teams.is_empty() {
                    println!("(none)");
                } else {
                    let mut table = Table::new(&["ID", "NAME", "SLUG", "TYPE"]);
                    for team in &teams {
                        table.row(vec![
                            team.id.clone(),
                            team.name.clone(),
                            team.slug.clone(),
                            team.team_type.clone(),
                        ]);
                    }
                    print!("{}", table.render());
                }
                Ok(())
            }
            Self::Add {
                stream_id,
                team,
                common,
            } => {
                let client = client(&common)?;
                let response = client
                    .add_stream_team(&stream_id, &AddStreamTeamRequest { team_id: team })
                    .await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Added team {} to stream {stream_id}", response.id);
                Ok(())
            }
            Self::Remove {
                stream_id,
                team,
                common,
            } => {
                let client = client(&common)?;
                let response = client.remove_stream_team(&stream_id, &team).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Removed team {} from stream {stream_id}", response.id);
                Ok(())
            }
        }
    }
}
