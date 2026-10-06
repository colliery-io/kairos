//! `kairos teams` — teams and their membership (S-0005 `/api/teams`;
//! creating a team also creates its delivery board, KAIROS-A-0002).

use kairos_client::types_org::{
    AddTeamMemberRequest, CreateTeamRequest, ItemTeam, Team, TeamMember, UpdateTeamRequest,
};

use crate::commands::entities::{ListArgs, require_confirm};
use crate::commands::repos::family_of;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Operations on teams.
#[derive(clap::Subcommand, Debug)]
pub enum TeamsCommand {
    /// List teams (paginated)
    List(ListArgs),
    /// Show one team
    Get {
        /// Team id (UUID)
        team_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Create a team (also creates its delivery board)
    Create {
        /// Team name
        #[arg(long)]
        name: String,
        /// Team slug (the delivery board becomes `{slug}-delivery`)
        #[arg(long)]
        slug: String,
        /// stream_aligned|platform|enabling|complicated_subsystem
        /// (defaults to stream_aligned)
        #[arg(long = "type", value_name = "TEAM_TYPE")]
        team_type: Option<String>,
        /// The short-code prefix of the delivery board, for example SKADI
        /// (a capital letter, then 1 to 9 capital letters or digits). It
        /// does not change later.
        #[arg(long = "code-prefix", value_name = "PREFIX")]
        code_prefix: String,
        #[command(flatten)]
        common: Common,
    },
    /// Update a team's name, slug, and/or type
    Update {
        /// Team id (UUID)
        team_id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New slug
        #[arg(long)]
        slug: Option<String>,
        /// New type: stream_aligned|platform|enabling|complicated_subsystem
        #[arg(long = "type", value_name = "TEAM_TYPE")]
        team_type: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Soft-delete a team (requires --confirm)
    Delete {
        /// Team id (UUID)
        team_id: String,
        /// Actually delete
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Team membership
    #[command(subcommand)]
    Members(TeamMembersCommand),
    /// Show the teams of an initiative or a strategy: the teams of the
    /// boards of its tasks (two levels down for a strategy), and the teams
    /// set on it by hand
    Of {
        /// Short code of the initiative or of the strategy
        short_code: String,
        #[command(flatten)]
        common: Common,
    },
    /// Set a team on an initiative or a strategy by hand, for example
    /// before it has tasks. The team stays when tasks come. You must be
    /// able to edit the item
    Set {
        /// Short code of the initiative or of the strategy
        short_code: String,
        /// Team slug (or UUID)
        team: String,
        #[command(flatten)]
        common: Common,
    },
    /// Clear a team that is set on an initiative or a strategy by hand. A
    /// team from its tasks stays
    Clear {
        /// Short code of the initiative or of the strategy
        short_code: String,
        /// Team slug (or UUID)
        team: String,
        #[command(flatten)]
        common: Common,
    },
}

/// How an item has a team (KAIROS-T-0321).
fn team_source(team: &ItemTeam) -> &'static str {
    match (team.from_tasks, team.set_by_hand) {
        (true, true) => "from tasks, set by hand",
        (true, false) => "from tasks",
        _ => "set by hand",
    }
}

/// Membership of one team.
#[derive(clap::Subcommand, Debug)]
pub enum TeamMembersCommand {
    /// List a team's members
    List {
        /// Team id (UUID)
        team_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Add a user to a team
    Add {
        /// Team id (UUID)
        team_id: String,
        /// User id (UUID; see `kairos members list`)
        #[arg(long, value_name = "USER_ID")]
        user: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a user from a team
    Remove {
        /// Team id (UUID)
        team_id: String,
        /// User id (UUID)
        #[arg(long, value_name = "USER_ID")]
        user: String,
        #[command(flatten)]
        common: Common,
    },
}

/// The shared team table (list + single-row `get`).
fn team_table(teams: &[Team]) -> Table {
    let mut table = Table::new(&["ID", "NAME", "SLUG", "TYPE", "DELIVERY_BOARD"]);
    for team in teams {
        table.row(vec![
            team.id.clone(),
            team.name.clone(),
            team.slug.clone(),
            team.team_type.clone(),
            team.delivery_board_id
                .clone()
                .unwrap_or_else(|| "-".to_string()),
        ]);
    }
    table
}

fn member_table(members: &[TeamMember]) -> Table {
    let mut table = Table::new(&["USER_ID", "EMAIL", "NAME", "JOINED"]);
    for member in members {
        table.row(vec![
            member.user_id.clone(),
            member.email.clone(),
            member.display_name.clone(),
            member.joined_at.clone(),
        ]);
    }
    table
}

impl TeamsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List(args) => {
                let client = client(&args.common)?;
                let envelope = client.list_teams(args.page()).await?;
                if args.common.json {
                    return print_json(&envelope);
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", team_table(&envelope.items).render());
                }
                println!(
                    "total: {} (limit {}, offset {})",
                    envelope.total, envelope.limit, envelope.offset
                );
                Ok(())
            }
            Self::Get { team_id, common } => {
                let client = client(&common)?;
                let team = client.get_team(&team_id).await?;
                if common.json {
                    return print_json(&team);
                }
                print!("{}", team_table(std::slice::from_ref(&team)).render());
                Ok(())
            }
            Self::Create {
                name,
                slug,
                team_type,
                code_prefix,
                common,
            } => {
                let client = client(&common)?;
                let team = client
                    .create_team(&CreateTeamRequest {
                        name,
                        slug,
                        team_type,
                        code_prefix,
                    })
                    .await?;
                if common.json {
                    return print_json(&team);
                }
                println!(
                    "Kairos made the team {} ({}, id {}). The delivery board is {}.",
                    team.name,
                    team.slug,
                    team.id,
                    team.delivery_board_id.as_deref().unwrap_or("-")
                );
                Ok(())
            }
            Self::Update {
                team_id,
                name,
                slug,
                team_type,
                common,
            } => {
                if name.is_none() && slug.is_none() && team_type.is_none() {
                    return Err(CliError::Failure(
                        "The command has no change. Use --name, --slug or --type.".to_string(),
                    ));
                }
                let client = client(&common)?;
                let team = client
                    .update_team(
                        &team_id,
                        &UpdateTeamRequest {
                            name,
                            slug,
                            team_type,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&team);
                }
                println!(
                    "Kairos changed the team {} ({}, id {}).",
                    team.name, team.slug, team.id
                );
                Ok(())
            }
            Self::Delete {
                team_id,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("the team {team_id}"))?;
                let client = client(&common)?;
                let response = client.delete_team(&team_id).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Kairos deleted the team {}.", response.id);
                Ok(())
            }
            Self::Members(command) => command.run().await,
            Self::Of { short_code, common } => {
                let kind = family_of(&short_code)?;
                let client = client(&common)?;
                let response = client.item_teams(kind, &short_code).await?;
                if common.json {
                    return print_json(&response);
                }
                if response.teams.is_empty() {
                    println!(
                        "{short_code} has no team: no task on a team board, and no team set \
                         by hand."
                    );
                    return Ok(());
                }
                let mut table = Table::new(&["SLUG", "NAME", "SOURCE"]);
                for team in &response.teams {
                    table.row(vec![
                        team.slug.clone(),
                        team.name.clone(),
                        team_source(team).to_string(),
                    ]);
                }
                print!("{}", table.render());
                Ok(())
            }
            Self::Set {
                short_code,
                team,
                common,
            } => {
                let kind = family_of(&short_code)?;
                let client = client(&common)?;
                let set = client.set_item_team(kind, &short_code, &team).await?;
                if common.json {
                    return print_json(&set);
                }
                println!("Kairos set the team {} on {short_code} by hand.", set.slug);
                Ok(())
            }
            Self::Clear {
                short_code,
                team,
                common,
            } => {
                let kind = family_of(&short_code)?;
                let client = client(&common)?;
                let cleared = client.clear_item_team(kind, &short_code, &team).await?;
                if common.json {
                    return print_json(&cleared);
                }
                println!(
                    "Kairos cleared the team {} that was set on {short_code} by hand.",
                    cleared.team
                );
                Ok(())
            }
        }
    }
}

impl TeamMembersCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List { team_id, common } => {
                let client = client(&common)?;
                let members = client.list_team_members(&team_id).await?;
                if common.json {
                    return print_json(&members);
                }
                if members.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", member_table(&members).render());
                }
                Ok(())
            }
            Self::Add {
                team_id,
                user,
                common,
            } => {
                let client = client(&common)?;
                let member = client
                    .add_team_member(&team_id, &AddTeamMemberRequest { user_id: user })
                    .await?;
                if common.json {
                    return print_json(&member);
                }
                println!(
                    "Kairos added {} ({}) to the team {team_id}.",
                    member.display_name, member.email
                );
                Ok(())
            }
            Self::Remove {
                team_id,
                user,
                common,
            } => {
                let client = client(&common)?;
                let response = client.remove_team_member(&team_id, &user).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Kairos removed the user {user} from the team {team_id}.");
                Ok(())
            }
        }
    }
}
