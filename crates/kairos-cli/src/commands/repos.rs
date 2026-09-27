//! `kairos repos` — the repository directory (KAIROS-T-0107, decision
//! KAIROS-A-0019): where the code is.
//! Every repository has exactly one owning team. `bind` sets the repository
//! a task links to and changes nothing else: the task stays on its board,
//! with its team (COLLIERY-T-0217, COLLIERY-A-0023). Until then `bind`
//! accepted only a repository of the team whose board the task was on.
//!
//! `get` does not print a line about stale tasks (COLLIERY-T-0219). That
//! line counted the linked tasks on a board of a team that does not own the
//! repository. COLLIERY-A-0023 makes those tasks normal work, and the `OPEN`
//! column counts them with all the others. For the same reason the board
//! column is `OWNER_BOARD`: it is the delivery board of the owner, not where
//! tasks go.

use kairos_client::types_repositories::{
    CreateRepositoryRequest, Repository, UpdateRepositoryRequest,
};

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Operations on repositories.
#[derive(clap::Subcommand, Debug)]
pub enum ReposCommand {
    /// List repositories (optionally one team's)
    List {
        /// Only this team's repositories (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Show one repository: owner, the owner's board, how-to-work-here, in-flight PRs
    Get {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Register a repository under its owning team (org admin, or a member
    /// of that team)
    Create {
        /// `github|gitlab|other`
        #[arg(long)]
        forge: String,
        /// `owner/repo` — must match what the forge sends in webhooks
        #[arg(long, value_name = "FULL_NAME")]
        name: String,
        /// Browser URL of the repository
        #[arg(long = "repo-url", value_name = "URL")]
        repo_url: String,
        /// Owning team (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: String,
        /// Slug (defaults to one derived from --name)
        #[arg(long)]
        slug: Option<String>,
        /// Default branch (defaults to main)
        #[arg(long = "default-branch", value_name = "BRANCH")]
        default_branch: Option<String>,
        /// Short "how to work here" blurb for agents
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Edit a repository (same gate as create; --team re-homes it)
    Update {
        /// Repository slug (or UUID)
        repository: String,
        #[arg(long)]
        slug: Option<String>,
        #[arg(long = "repo-url", value_name = "URL")]
        repo_url: Option<String>,
        #[arg(long = "default-branch", value_name = "BRANCH")]
        default_branch: Option<String>,
        /// New owning team (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a repository (org admin; refused while tasks or a webhook
    /// connection reference it; requires --confirm)
    Delete {
        /// Repository slug (or UUID)
        repository: String,
        /// Actually delete
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Bind a task to a repository. It can be any repository, of any team.
    /// The board and the team of the task do not change
    Bind {
        /// Task short code
        short_code: String,
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Clear a task's repository binding
    Unbind {
        /// Task short code
        short_code: String,
        #[command(flatten)]
        common: Common,
    },
}

fn repo_table(repos: &[Repository]) -> Table {
    let mut table = Table::new(&[
        "SLUG",
        "FORGE",
        "NAME",
        "TEAM",
        "OWNER_BOARD",
        "OPEN",
        "WEBHOOK",
    ]);
    for repo in repos {
        table.row(vec![
            repo.slug.clone(),
            repo.forge.clone(),
            repo.repo_full_name.clone(),
            repo.team.slug.clone(),
            repo.delivery_board_id
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            repo.open_tasks.to_string(),
            if repo.has_webhook { "yes" } else { "no" }.to_string(),
        ]);
    }
    table
}

impl ReposCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List { team, common } => {
                let client = client(&common)?;
                let repos = client.list_repositories(team.as_deref()).await?;
                if common.json {
                    return print_json(&repos);
                }
                if repos.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", repo_table(&repos).render());
                }
                Ok(())
            }
            Self::Get { repository, common } => {
                let client = client(&common)?;
                let detail = client.get_repository(&repository).await?;
                if common.json {
                    return print_json(&detail);
                }
                print!(
                    "{}",
                    repo_table(std::slice::from_ref(&detail.repository)).render()
                );
                println!("url:            {}", detail.repository.repo_url);
                println!("default branch: {}", detail.repository.default_branch);
                println!(
                    "webhook:        {}",
                    detail.connection_id.as_deref().unwrap_or("not connected")
                );
                println!("\nHow to work here:");
                if detail.repository.description.trim().is_empty() {
                    println!("  (no description yet)");
                } else {
                    for line in detail.repository.description.lines() {
                        println!("  {line}");
                    }
                }
                println!("\nIn flight:");
                if detail.in_flight.is_empty() {
                    println!("  (nothing open)");
                }
                for link in &detail.in_flight {
                    println!(
                        "  {} {} [{}] {} — {} ({})",
                        link.kind,
                        link.external_id,
                        link.state,
                        link.title,
                        link.item_short_code,
                        link.url
                    );
                }
                Ok(())
            }
            Self::Create {
                forge,
                name,
                repo_url,
                team,
                slug,
                default_branch,
                description,
                common,
            } => {
                let client = client(&common)?;
                let repo = client
                    .create_repository(&CreateRepositoryRequest {
                        slug,
                        forge,
                        repo_full_name: name,
                        repo_url,
                        default_branch,
                        team,
                        description,
                    })
                    .await?;
                if common.json {
                    return print_json(&repo);
                }
                println!(
                    "Registered {} ({} {}) under team {}; the delivery board of that team is {}",
                    repo.slug,
                    repo.forge,
                    repo.repo_full_name,
                    repo.team.slug,
                    repo.delivery_board_id.as_deref().unwrap_or("-")
                );
                Ok(())
            }
            Self::Update {
                repository,
                slug,
                repo_url,
                default_branch,
                team,
                description,
                common,
            } => {
                if slug.is_none()
                    && repo_url.is_none()
                    && default_branch.is_none()
                    && team.is_none()
                    && description.is_none()
                {
                    return Err(CliError::Failure(
                        "nothing to update: pass --slug, --repo-url, --default-branch, --team, \
                         or --description"
                            .to_string(),
                    ));
                }
                let client = client(&common)?;
                let repo = client
                    .update_repository(
                        &repository,
                        &UpdateRepositoryRequest {
                            slug,
                            repo_url,
                            default_branch,
                            team,
                            description,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&repo);
                }
                println!("Updated {} (owner: {})", repo.slug, repo.team.slug);
                Ok(())
            }
            Self::Delete {
                repository,
                confirm,
                common,
            } => {
                require_confirm(confirm, &repository)?;
                let client = client(&common)?;
                let response = client.delete_repository(&repository).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Deleted repository {}", response.id);
                Ok(())
            }
            Self::Bind {
                short_code,
                repository,
                common,
            } => {
                let client = client(&common)?;
                let task = client
                    .set_task_repository(&short_code, Some(&repository))
                    .await?;
                if common.json {
                    return print_json(&task);
                }
                println!(
                    "{} bound to {}",
                    task.short_code,
                    task.repository.as_ref().map_or("-", |r| r.slug.as_str())
                );
                Ok(())
            }
            Self::Unbind { short_code, common } => {
                let client = client(&common)?;
                let task = client.set_task_repository(&short_code, None).await?;
                if common.json {
                    return print_json(&task);
                }
                println!("{} unbound", task.short_code);
                Ok(())
            }
        }
    }
}
