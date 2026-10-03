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
//!
//! `link` and `unlink` write the relationship `impacts` from a document or
//! an ADR to a repository (COLLIERY-T-0269). The link says what the item
//! is about, and it gives no right. `bind` is for a task, and it says
//! where the code of the task is. `get` lists the documents and the ADRs
//! that impact the repository.
//!
//! `credential set|remove|check` manage the read token that the builder of
//! the code index gives to git (COLLIERY-T-3105). `set` reads the token from
//! standard input or from a prompt with no echo, never from an argument.
//! `get` (alias `show`) prints the status of the token, never the token.

use kairos_client::EntityKind;
use kairos_client::types_repositories::{
    CreateRepositoryRequest, ImpactingItem, Repository, UpdateRepositoryRequest,
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
    /// Show one repository: owner, the owner's board, how-to-work-here,
    /// the status of its read token, the documents and ADRs that impact it,
    /// in-flight PRs
    #[command(visible_alias = "show")]
    Get {
        /// Repository slug (or UUID)
        repository: String,
        /// Also list the archived documents and ADRs that impact the
        /// repository, marked `[archived]`
        #[arg(long)]
        include_deleted: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Register a repository under its owning team (org admin, or a member
    /// of that team)
    Create {
        /// `github|gitlab|other`
        #[arg(long)]
        forge: String,
        /// The name on the forge, as the forge sends it in webhooks: 2 parts
        /// for github (owner/repo), 2 or more for gitlab, 1 or more for
        /// other. No space, and no .git at the end
        #[arg(long, value_name = "FULL_NAME")]
        name: String,
        /// Browser URL of the repository: an absolute http or https URL,
        /// with no user name and no password
        #[arg(long = "repo-url", value_name = "URL")]
        repo_url: String,
        /// Owning team (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: String,
        /// Slug (defaults to one derived from --name)
        #[arg(long)]
        slug: Option<String>,
        /// Default branch: a branch name that git accepts (defaults to main)
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
        /// New browser URL: an absolute http or https URL, with no user
        /// name and no password
        #[arg(long = "repo-url", value_name = "URL")]
        repo_url: Option<String>,
        /// New default branch: a branch name that git accepts
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
    /// Say that a document or an ADR impacts a repository (the
    /// relationship `impacts`). It can be any repository, of any team. You
    /// must be able to edit the document or the ADR. The link gives no
    /// right
    Link {
        /// Short code of the document or of the ADR
        short_code: String,
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove an `impacts` link of a document or of an ADR
    Unlink {
        /// Short code of the document or of the ADR
        short_code: String,
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Set, remove or check the read token of a repository. The builder of
    /// the code index gives the token to git to fetch a private repository.
    /// Kairos keeps the token encrypted, and no command shows it
    Credential {
        #[command(subcommand)]
        command: CredentialCommand,
    },
}

/// The read token of a repository (COLLIERY-T-3105). An organization admin
/// or a member of the owner team uses these commands.
#[derive(clap::Subcommand, Debug)]
pub enum CredentialCommand {
    /// Set or replace the read token. The command reads the token from
    /// standard input, or asks for it on the terminal. It does not take the
    /// token as an argument, so the token is not in the shell history. Use
    /// a GitHub fine-grained token with only "Contents: read" on the one
    /// repository
    Set {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove the read token. The next fetch has no credential
    Remove {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Test the read token with `git ls-remote` on the server. The command
    /// fails when git cannot read the repository with the token
    Check {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
}

impl CredentialCommand {
    async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Set { repository, common } => {
                let token = crate::password::read_secret(
                    &format!("Read token for the repository {repository}: "),
                    "token",
                )?;
                let client = client(&common)?;
                let status = client
                    .set_repository_credential(&repository, &token)
                    .await?;
                if common.json {
                    return print_json(&status);
                }
                println!(
                    "Kairos keeps the read token of the repository {repository}, encrypted. \
                     To test it, run: kairos repos credential check {repository}"
                );
                Ok(())
            }
            Self::Remove { repository, common } => {
                let client = client(&common)?;
                let status = client.remove_repository_credential(&repository).await?;
                if common.json {
                    return print_json(&status);
                }
                println!("Kairos removed the read token of the repository {repository}.");
                Ok(())
            }
            Self::Check { repository, common } => {
                let client = client(&common)?;
                let status = client.check_repository_credential(&repository).await?;
                if common.json {
                    print_json(&status)?;
                } else {
                    println!("read token: {}", status.summary());
                }
                match status.last_check_ok {
                    Some(true) => Ok(()),
                    _ => Err(CliError::Failure(format!(
                        "git cannot read the repository {repository} with the token. Make \
                         sure that the token can read the repository, and set it again."
                    ))),
                }
            }
        }
    }
}

/// The entity family of a short code, from its type letter
/// (`ACME-D-0001` is a document). The server has the rule on which family
/// can impact a repository, and it refuses the others with the reason.
fn family_of(short_code: &str) -> Result<EntityKind, CliError> {
    let mut parts = short_code.rsplit('-');
    let (_number, letter) = (parts.next(), parts.next());
    match letter {
        Some("S") => Ok(EntityKind::Strategy),
        Some("I") => Ok(EntityKind::Initiative),
        Some("T") => Ok(EntityKind::Task),
        Some("D") => Ok(EntityKind::Document),
        Some("A") => Ok(EntityKind::Adr),
        _ => Err(CliError::Failure(format!(
            "{short_code:?} is not a short code. A short code has the form ACME-D-0001."
        ))),
    }
}

/// The lines of `get` for the documents and the ADRs that impact the
/// repository (COLLIERY-T-0269).
fn impacted_by_lines(items: &[ImpactingItem]) -> Vec<String> {
    if items.is_empty() {
        return vec!["  (none)".to_string()];
    }
    items
        .iter()
        .map(|item| {
            let kind = match (&item.document_type, item.entity_type.as_str()) {
                (Some(document_type), _) => format!("document ({document_type})"),
                (None, entity_type) => entity_type.to_string(),
            };
            let state = item
                .lifecycle
                .as_deref()
                .or(item.column.as_deref())
                .unwrap_or("-");
            format!(
                "  {}{} [{state}] {} — {kind}",
                item.short_code,
                if item.archived_at.is_some() {
                    " [archived]"
                } else {
                    ""
                },
                item.title
            )
        })
        .collect()
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
            Self::Get {
                repository,
                include_deleted,
                common,
            } => {
                let client = client(&common)?;
                let detail = if include_deleted {
                    client.get_repository_with_archived(&repository).await?
                } else {
                    client.get_repository(&repository).await?
                };
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
                println!("read token:     {}", detail.repository.credential.summary());
                println!("\nHow to work here:");
                if detail.repository.description.trim().is_empty() {
                    println!("  (no description yet)");
                } else {
                    for line in detail.repository.description.lines() {
                        println!("  {line}");
                    }
                }
                println!("\nDocuments and ADRs that impact this repository:");
                for line in impacted_by_lines(&detail.impacted_by) {
                    println!("{line}");
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
                    "Kairos added the repository {} ({} {}) for the team {}. The delivery \
                     board of that team is {}.",
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
                        "The command has no change. Use --slug, --repo-url, --default-branch, \
                         --team or --description."
                            .to_string(),
                    ));
                }
                let client = client(&common)?;
                // COLLIERY-T-0267: a PATCH with the values of the
                // repository changes nothing, and the response is a normal
                // 200. `updated_at` tells the two apart.
                let before = client.get_repository(&repository).await?;
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
                if repo.updated_at == before.repository.updated_at {
                    println!(
                        "Kairos did not change the repository {}. It has these values \
                         already.",
                        repo.slug
                    );
                    return Ok(());
                }
                println!(
                    "Kairos changed the repository {} (owner: {}).",
                    repo.slug, repo.team.slug
                );
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
                println!("Kairos deleted the repository {}.", response.id);
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
                    "Kairos set the repository of {} to {}.",
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
                println!("Kairos removed the repository of {}.", task.short_code);
                Ok(())
            }
            Self::Link {
                short_code,
                repository,
                common,
            } => {
                let kind = family_of(&short_code)?;
                let client = client(&common)?;
                let impact = client.add_impact(kind, &short_code, &repository).await?;
                if common.json {
                    return print_json(&impact);
                }
                println!(
                    "Kairos made the link: {short_code} impacts the repository {}.",
                    impact.repository.slug
                );
                Ok(())
            }
            Self::Unlink {
                short_code,
                repository,
                common,
            } => {
                let kind = family_of(&short_code)?;
                let client = client(&common)?;
                let removed = client.remove_impact(kind, &short_code, &repository).await?;
                if common.json {
                    return print_json(&removed);
                }
                println!(
                    "Kairos removed the link: {short_code} does not impact the repository {}.",
                    removed.repository
                );
                Ok(())
            }
            Self::Credential { command } => command.run().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COLLIERY-T-0269: the family of `link` and `unlink` comes from the
    /// type letter of the short code.
    #[test]
    fn the_family_comes_from_the_short_code() {
        assert_eq!(family_of("ACME-D-0001").unwrap(), EntityKind::Document);
        assert_eq!(family_of("ACME-A-0012").unwrap(), EntityKind::Adr);
        // The letter is the part before the number.
        assert_eq!(family_of("ACME-CO-D-0001").unwrap(), EntityKind::Document);
        // The server refuses a task, with the reason.
        assert_eq!(family_of("ACME-T-0001").unwrap(), EntityKind::Task);
        let err = family_of("fidius").expect_err("not a short code");
        assert_eq!(
            err.to_string(),
            "\"fidius\" is not a short code. A short code has the form ACME-D-0001."
        );
    }

    #[test]
    fn the_items_that_impact_a_repository() {
        assert_eq!(impacted_by_lines(&[]), vec!["  (none)".to_string()]);
        let lines = impacted_by_lines(&[
            ImpactingItem {
                short_code: "ACME-D-0004".into(),
                title: "The vision of fidius".into(),
                entity_type: "document".into(),
                document_type: Some("vision".into()),
                lifecycle: Some("published".into()),
                column: None,
                archived_at: None,
            },
            ImpactingItem {
                short_code: "ACME-A-0002".into(),
                title: "Plugins are dynamic libraries".into(),
                entity_type: "adr".into(),
                document_type: None,
                lifecycle: None,
                column: Some("Decided".into()),
                archived_at: Some("2026-09-01T00:00:00Z".into()),
            },
        ]);
        assert_eq!(
            lines,
            vec![
                "  ACME-D-0004 [published] The vision of fidius — document (vision)".to_string(),
                "  ACME-A-0002 [archived] [Decided] Plugins are dynamic libraries — adr"
                    .to_string(),
            ]
        );
    }
}
