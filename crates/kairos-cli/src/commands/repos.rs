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
//!
//! `local` lists the clones and worktrees of a folder with their
//! repositories ([`super::repos_local`], KAIROS-T-0646).
//!
//! `indexes` lists the indexed commits of a repository, newest first
//! (KAIROS-T-0518). It shows why `fetch_index` takes an old base: the
//! nearest index can be far from the checkout.

use kairos_client::EntityKind;
use kairos_client::types_code_index::CodeIndex;
use kairos_client::types_repositories::{
    CreateRepositoryRequest, ImpactingItem, Repository, UpdateRepositoryRequest,
};

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::listing::{Paging, fetch};
use crate::table::Table;

/// Operations on repositories.
#[derive(clap::Subcommand, Debug)]
pub enum ReposCommand {
    /// List repositories (optionally one team's)
    List {
        /// Only this team's repositories (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: Option<String>,
        /// Page size (default 50, max 200)
        #[arg(long)]
        limit: Option<i64>,
        /// Rows to skip
        #[arg(long)]
        offset: Option<i64>,
        /// Read each page: the CLI asks for one page after the other, and
        /// prints each row. Not with --limit or --offset
        #[arg(long, conflicts_with_all = ["limit", "offset"])]
        all: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show one repository: owner, the owner's board, how-to-work-here,
    /// the status of its read token, the documents and ADRs that impact it,
    /// in-flight PRs
    #[command(alias = "get")]
    Show {
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
        /// Default branch: a branch name that git accepts. With it, the
        /// branch is set by hand. Without it, the code index builder
        /// follows the default branch of the forge
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
        /// New default branch: a branch name that git accepts. The branch
        /// is then set by hand: the code index builder indexes it, whatever
        /// the forge says
        #[arg(
            long = "default-branch",
            value_name = "BRANCH",
            conflicts_with = "follow_forge"
        )]
        default_branch: Option<String>,
        /// Clear the branch set by hand: the code index builder follows the
        /// default branch of the forge again
        #[arg(long = "follow-forge")]
        follow_forge: bool,
        /// New owning team (slug or UUID)
        #[arg(long, value_name = "TEAM")]
        team: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Whether the code index builder works on the repository: on or
        /// off. With off, the builder makes no index of it
        #[arg(long = "code-index-build", value_name = "ON|OFF", value_parser = ["on", "off"])]
        code_index_build: Option<String>,
        /// Where the summaries of the code index are made: embedded (the
        /// model in the server), hosted (the provider of the organization;
        /// the code of each changed symbol leaves the host) or organization
        /// (the default of the organization)
        #[arg(long = "code-index-summaries", value_name = "EMBEDDED|HOSTED|ORGANIZATION", value_parser = ["embedded", "hosted", "organization"])]
        code_index_summaries: Option<String>,
        /// Set the include pattern of a forge filter field (org admin),
        /// for example `--include pr_head_branch='^feature/'`. Fields:
        /// branch_name, pr_head_branch, pr_title, pr_author, issue_title,
        /// issue_author. Use it again for more fields
        #[arg(long = "include", value_name = "FIELD=PATTERN")]
        include: Vec<String>,
        /// Set the exclude pattern of a forge filter field (org admin),
        /// for example `--exclude 'pr_author=^dependabot\[bot\]$'`. An
        /// empty pattern excludes nothing
        #[arg(long = "exclude", value_name = "FIELD=PATTERN")]
        exclude: Vec<String>,
        /// How often the repository takes its open issues, with no webhook
        /// (org admin): off, or a number with a unit: 90s, 15m, 1h or 1d.
        /// The shortest interval is 1m and the longest is 30d. The
        /// repository needs a read token
        #[arg(long = "import-interval", value_name = "OFF|INTERVAL")]
        import_interval: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Import the open issues of a repository once (org admin). Each one
    /// becomes a support task on the delivery board of the team, as an
    /// opened issue does. A second run skips what the first took. Needs a read
    /// token with Issues: read (GitHub) or read_api (GitLab), and no webhook
    /// connection. The import is a job of the server: the command
    /// starts it and shows it. While an import of the repository waits or
    /// runs, the command shows that import
    ImportIssues {
        /// Repository slug (or UUID)
        repository: String,
        /// Wait until the import ends, then show its counts
        #[arg(long)]
        wait: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show the import of the open issues of a repository (org admin): the
    /// interval, the last complete run, the next run, the counts and the
    /// last error, then the last job
    ImportStatus {
        /// Repository slug (or UUID)
        repository: String,
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
    /// Set, remove or show the write token of the write-back. With it,
    /// Kairos writes a comment and the commit status `kairos` on each GitHub
    /// pull request that links to an item. It also comments on an issue
    /// that became a task, and closes or opens it with the task. No command
    /// shows the token
    #[command(name = "write-token")]
    WriteToken {
        #[command(subcommand)]
        command: WriteTokenCommand,
    },
    /// Connect, list, rotate or remove the webhook of a repository on
    /// GitHub or GitLab. Add and rotate print the secret one time
    Webhook {
        #[command(subcommand)]
        command: crate::commands::webhooks::WebhookCommand,
    },
    /// Ask Kairos to build the code index of a repository again. The builder
    /// of the server makes a full index of the head of the default branch.
    /// The command shows the run; `kairos repos builds` follows it
    Reindex {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// List the indexed commits of a repository, newest first: the commit,
    /// its branch or tag, the schema version, the size and the times
    Indexes {
        /// Repository slug (or UUID)
        repository: String,
        /// Page size (default 50, max 200)
        #[arg(long)]
        limit: Option<i64>,
        /// Rows to skip
        #[arg(long)]
        offset: Option<i64>,
        /// Read each page: the CLI asks for one page after the other, and
        /// prints each row. Not with --limit or --offset
        #[arg(long, conflicts_with_all = ["limit", "offset"])]
        all: bool,
        #[command(flatten)]
        common: Common,
    },
    /// List the clones and worktrees of a folder: the repository of each
    /// (its `origin` remote matched with the repositories of Kairos), its
    /// branch, and how far it is behind its default branch. The counts are
    /// from the local refs: the command runs no fetch
    Local {
        /// The folder: a board folder with clones, or a checkout. Default:
        /// the current folder
        folder: Option<std::path::PathBuf>,
        #[command(flatten)]
        common: Common,
    },
    /// Show the runs of the code index builder for a repository, newest
    /// first: a build after a push, a first build, a build on request, or
    /// an upload, with its result
    Builds {
        /// Repository slug (or UUID)
        repository: String,
        /// The most runs to show: 20 when not given, 100 at most
        #[arg(long)]
        limit: Option<i64>,
        #[command(flatten)]
        common: Common,
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
    /// Remove the read token (needs --confirm). The next fetch has no
    /// credential
    Remove {
        /// Repository slug (or UUID)
        repository: String,
        /// Required: without it, the command deletes nothing
        #[arg(long)]
        confirm: bool,
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
            Self::Remove {
                repository,
                confirm,
                common,
            } => {
                require_confirm(
                    confirm,
                    &format!("the read token of the repository {repository}"),
                )?;
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

/// The write token of the write-back (KAIROS-T-0386, KAIROS-T-0575).
#[derive(clap::Subcommand, Debug)]
pub enum WriteTokenCommand {
    /// Set or replace the write token. The command reads the token from
    /// standard input, or asks for it on the terminal. On GitHub, use a
    /// fine-grained token with "Pull requests: write", "Commit statuses:
    /// write" and "Issues: write" on the one repository. On GitLab, use a
    /// project access token with the scope "api"
    Set {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove the write token (needs --confirm). Kairos writes nothing more
    /// to the forge
    Remove {
        /// Repository slug (or UUID)
        repository: String,
        /// Required: without it, the command deletes nothing
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show the status of the write-back: on or off, and the last write
    Status {
        /// Repository slug (or UUID)
        repository: String,
        #[command(flatten)]
        common: Common,
    },
}

impl WriteTokenCommand {
    async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Set { repository, common } => {
                let token = crate::password::read_secret(
                    &format!("Write token for the repository {repository}: "),
                    "token",
                )?;
                let client = client(&common)?;
                let status = client
                    .set_repository_write_token(&repository, &token)
                    .await?;
                if common.json {
                    return print_json(&status);
                }
                println!(
                    "Kairos keeps the write token of the repository {repository}, encrypted. \
                     The write-back is on."
                );
                Ok(())
            }
            Self::Remove {
                repository,
                confirm,
                common,
            } => {
                require_confirm(
                    confirm,
                    &format!("the write token of the repository {repository}"),
                )?;
                let client = client(&common)?;
                let status = client.remove_repository_write_token(&repository).await?;
                if common.json {
                    return print_json(&status);
                }
                println!(
                    "Kairos removed the write token of the repository {repository}. The \
                     write-back is off."
                );
                Ok(())
            }
            Self::Status { repository, common } => {
                let client = client(&common)?;
                let status = client.get_repository_write_back(&repository).await?;
                if common.json {
                    return print_json(&status);
                }
                println!("write-back: {}", status.summary());
                Ok(())
            }
        }
    }
}

/// The entity family of a short code, from its type letter
/// (`ACME-D-0001` is a document). The server has the rule on which family
/// can impact a repository, and it refuses the others with the reason.
pub(crate) fn family_of(short_code: &str) -> Result<EntityKind, CliError> {
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

/// The `forge_filters` of `--include FIELD=PATTERN` and `--exclude
/// FIELD=PATTERN` (KAIROS-T-0479), or `None` when neither is given. An
/// unknown field is refused, and the error names it.
fn forge_filters_arg(
    include: &[String],
    exclude: &[String],
) -> Result<Option<kairos_client::types_repositories::ForgeFilters>, CliError> {
    use kairos_client::types_repositories::{ForgeFilterRule, ForgeFilters};
    if include.is_empty() && exclude.is_empty() {
        return Ok(None);
    }
    let mut filters = ForgeFilters::default();
    for (flag, values) in [("--include", include), ("--exclude", exclude)] {
        for value in values {
            let Some((field, pattern)) = value.split_once('=') else {
                return Err(CliError::Failure(format!(
                    "The value {value:?} of {flag} has no `=`. Write FIELD=PATTERN."
                )));
            };
            if !ForgeFilters::FIELDS.contains(&field) {
                return Err(CliError::Failure(format!(
                    "The field {field:?} of {flag} is not a forge filter field. Use one of: {}.",
                    ForgeFilters::FIELDS.join(", ")
                )));
            }
            let mut rule = filters
                .get(field)
                .cloned()
                .unwrap_or(ForgeFilterRule::default());
            if flag == "--include" {
                rule.include = Some(pattern.to_string());
            } else {
                rule.exclude = Some(pattern.to_string());
            }
            filters.set(field, rule);
        }
    }
    Ok(Some(filters))
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

/// The table of the indexed commits (KAIROS-T-0518). Pure.
fn index_table(indexes: &[CodeIndex]) -> Table {
    let mut table = Table::new(&[
        "COMMIT",
        "REF",
        "SCHEMA",
        "SIZE",
        "SUMMARIES",
        "CREATED",
        "UPDATED",
    ]);
    for index in indexes {
        table.row(vec![
            index.commit.chars().take(12).collect(),
            refs_text(index),
            index
                .schema_version
                .map_or_else(|| "-".to_string(), |v| v.to_string()),
            size_text(index.structure_bytes),
            index.summary_keys.to_string(),
            minute_text(&index.created_at),
            minute_text(&index.updated_at),
        ]);
    }
    table
}

/// The refs that point at an indexed commit (KAIROS-T-0750), joined; else
/// the ref that the index was for; else `-`. Pure.
fn refs_text(index: &CodeIndex) -> String {
    if !index.refs.is_empty() {
        return index
            .refs
            .iter()
            .map(|r| r.text())
            .collect::<Vec<_>>()
            .join(", ");
    }
    index.r#ref.clone().unwrap_or_else(|| "-".to_string())
}

/// A size in bytes as B, KiB or MiB. Pure.
fn size_text(bytes: i64) -> String {
    const KIB: i64 = 1024;
    const MIB: i64 = 1024 * 1024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < MIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    }
}

/// An RFC 3339 time as `YYYY-MM-DD HH:MM` in UTC; another text as it is.
/// Pure.
fn minute_text(time: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(time).map_or_else(
        |_| time.to_string(),
        |t| {
            t.with_timezone(&chrono::Utc)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

impl ReposCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List {
                team,
                limit,
                offset,
                all,
                common,
            } => {
                let client = client(&common)?;
                let paging = Paging { limit, offset, all };
                let envelope = fetch(paging, |page| {
                    client.list_repositories_page(team.as_deref(), page)
                })
                .await?;
                if common.json {
                    return envelope.print_json();
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", repo_table(&envelope.items).render());
                }
                envelope.print_footer();
                Ok(())
            }
            Self::Show {
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
                println!(
                    "default branch: {} ({})",
                    detail.repository.default_branch,
                    detail.repository.default_branch_summary()
                );
                println!(
                    "webhook:        {}",
                    detail.connection_id.as_deref().unwrap_or("not connected")
                );
                println!("read token:     {}", detail.repository.credential.summary());
                println!("write-back:     {}", detail.repository.write_back.summary());
                println!("index builder:  {}", detail.repository.code_index_build);
                println!(
                    "index breaker:  {}",
                    detail.repository.code_index_breaker.summary()
                );
                for warning in &detail.repository.code_index_scip_warnings {
                    println!("index edges:    {}", warning.note());
                }
                println!(
                    "summaries:      {} (now: {})",
                    detail.repository.code_index_summaries,
                    detail.repository.code_index_summaries_resolved
                );
                println!(
                    "forge filters:  {}",
                    detail.repository.forge_filters.summary()
                );
                println!(
                    "issue import:   {}",
                    detail.repository.issue_import_summary()
                );
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
                // KAIROS-T-0480: open branches and pull requests with no item.
                println!("\nNot linked to an item:");
                let unlinked: Vec<_> = detail.refs.iter().filter(|r| !r.linked).collect();
                if unlinked.is_empty() {
                    println!("  (none)");
                }
                for r in unlinked {
                    println!(
                        "  {} {} [{}] {} by {} ({})",
                        r.kind, r.external_id, r.state, r.title, r.author, r.url
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
                follow_forge,
                team,
                description,
                code_index_build,
                code_index_summaries,
                include,
                exclude,
                import_interval,
                common,
            } => {
                if slug.is_none()
                    && repo_url.is_none()
                    && default_branch.is_none()
                    && !follow_forge
                    && team.is_none()
                    && description.is_none()
                    && code_index_build.is_none()
                    && code_index_summaries.is_none()
                    && include.is_empty()
                    && exclude.is_empty()
                    && import_interval.is_none()
                {
                    return Err(CliError::Failure(
                        "The command has no change. Use --slug, --repo-url, --default-branch, \
                         --follow-forge, --team, --description, --code-index-build, \
                         --code-index-summaries, --include, --exclude or --import-interval."
                            .to_string(),
                    ));
                }
                let forge_filters = forge_filters_arg(&include, &exclude)?;
                // clap accepts only on and off.
                let code_index_build = code_index_build
                    .as_deref()
                    .and_then(kairos_client::types_repositories::CodeIndexBuild::parse);
                let code_index_summaries = code_index_summaries
                    .as_deref()
                    .and_then(kairos_client::types_repositories::CodeIndexSummaries::parse);
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
                            default_branch_source: follow_forge.then_some(
                                kairos_client::types_repositories::DefaultBranchSource::Forge,
                            ),
                            team,
                            description,
                            code_index_build,
                            code_index_summaries,
                            // The site switch has its own command (KAIROS-T-0408).
                            site_enabled: None,
                            forge_filters,
                            issue_import_interval: import_interval,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&repo);
                }
                if repo.updated_at == before.repository.updated_at
                    && repo.forge_filters == before.repository.forge_filters
                {
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
            Self::ImportIssues {
                repository,
                wait,
                common,
            } => {
                let client = client(&common)?;
                let mut import = client.import_issues(&repository).await?;
                while wait && !matches!(import.state.as_str(), "done" | "failed") {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    import = client.issue_import(&repository).await?;
                }
                if common.json {
                    return print_json(&import);
                }
                print_import(&import);
                Ok(())
            }
            Self::ImportStatus { repository, common } => {
                let client = client(&common)?;
                let import = client.issue_import(&repository).await?;
                if common.json {
                    return print_json(&import);
                }
                print_import(&import);
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
                println!("Kairos deleted the repository {repository}.");
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
            Self::WriteToken { command } => command.run().await,
            Self::Webhook { command } => command.run().await,
            Self::Reindex { repository, common } => {
                let client = client(&common)?;
                let run = client.rebuild_code_index(&repository).await?;
                if common.json {
                    return print_json(&run);
                }
                println!(
                    "Kairos builds the code index of the repository {repository} again (run {}, \
                     started {}). To follow it, run: kairos repos builds {repository}",
                    run.id, run.started_at
                );
                Ok(())
            }
            Self::Indexes {
                repository,
                limit,
                offset,
                all,
                common,
            } => {
                let client = client(&common)?;
                let paging = Paging { limit, offset, all };
                let listing =
                    fetch(paging, |page| client.code_indexes_page(&repository, page)).await?;
                if common.json {
                    return listing.print_json();
                }
                if listing.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", index_table(&listing.items).render());
                }
                listing.print_footer();
                Ok(())
            }
            Self::Local { folder, common } => {
                crate::commands::repos_local::run(folder, common).await
            }
            Self::Builds {
                repository,
                limit,
                common,
            } => {
                let client = client(&common)?;
                let list = client
                    .list_code_index_builds(
                        &repository,
                        &kairos_client::types_code_index::CodeIndexBuildListQuery {
                            limit,
                            offset: None,
                        },
                    )
                    .await?;
                if common.json {
                    return print_json(&list);
                }
                // KAIROS-T-0394: a deployment with no builder says so.
                if let Some(note) = &list.builder {
                    println!("{note}");
                }
                if list.items.is_empty() {
                    println!("The repository {repository} has no run of the code index builder.");
                    return Ok(());
                }
                println!(
                    "{:<25} {:<8} {:<8} {:<12} {:<9} DETAIL",
                    "STARTED", "TRIGGER", "OUTCOME", "COMMIT", "SYMBOLS"
                );
                for run in &list.items {
                    let commit = run.commit.as_deref().map_or("-", |c| &c[..c.len().min(12)]);
                    let symbols = run
                        .symbols
                        .map_or_else(|| "-".to_string(), |n| n.to_string());
                    let mut detail = match (&run.error, &run.finished_at) {
                        (Some(error), _) => error.clone(),
                        (None, Some(finished)) => format!("ended {finished}"),
                        (None, None) => "running".to_string(),
                    };
                    if let Some(model) = &run.model {
                        detail.push_str(&format!(" · {model}"));
                    }
                    // KAIROS-T-0495: the next build makes them.
                    if let Some(missing) = run.summaries_missing.filter(|n| *n > 0) {
                        detail.push_str(&format!(" · {missing} summaries missing"));
                    }
                    println!(
                        "{:<25} {:<8} {:<8} {:<12} {:<9} {}",
                        run.started_at, run.trigger, run.outcome, commit, symbols, detail
                    );
                    // KAIROS-T-0478: the languages with no SCIP index.
                    for w in &run.scip_warnings {
                        println!(
                            "{:<25} warning: no SCIP index for {}: {}. Its edges are by name.",
                            "", w.language, w.reason
                        );
                    }
                }
                if list.total > list.items.len() as i64 {
                    println!(
                        "{} of {} runs. Use --limit for more (100 at most).",
                        list.items.len(),
                        list.total
                    );
                }
                Ok(())
            }
        }
    }
}

/// Show an import of the open issues (KAIROS-T-0487).
fn print_import(import: &kairos_client::types_repositories::IssueImport) {
    let state = match (import.state.as_str(), &import.waiting_until) {
        ("waiting", Some(until)) => {
            format!("waiting for the rate limit of the forge until {until}")
        }
        (state, _) => state.to_string(),
    };
    if import.job.is_empty() {
        println!(
            "Repository {}: no import job has run yet.",
            import.repository
        );
    } else {
        println!(
            "Import {} of {}: {state} ({} page(s) read).",
            import.job, import.repository, import.pages
        );
        println!("{}", import.result.message);
    }
    println!("Interval: {}", import.interval);
    for line in import.sync.lines("  ") {
        println!("{line}");
    }
    if let Some(error) = &import.error {
        println!("Error: {error}");
    }
    for code in &import.result.items {
        println!("  {code}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(commit: &str, r#ref: Option<&str>, version: Option<i64>, bytes: i64) -> CodeIndex {
        CodeIndex {
            commit: commit.to_string(),
            r#ref: r#ref.map(str::to_string),
            refs: Vec::new(),
            source: "build".to_string(),
            structure_bytes: bytes,
            summary_keys: 7,
            schema_version: version,
            vector_model: None,
            created_at: "2026-10-16T21:57:18.123456+00:00".to_string(),
            updated_at: "2026-10-18T01:24:06+00:00".to_string(),
        }
    }

    /// KAIROS-T-0518: one line for each indexed commit, with the short
    /// commit, the ref, the schema version, the size and the times.
    #[test]
    fn the_index_table_has_one_line_for_each_commit() {
        let mut tagged = index(&"c".repeat(40), Some("main"), Some(8), 900);
        tagged.refs = vec![
            kairos_client::types_code_index::CodeIndexRef {
                name: "main".into(),
                kind: "branch".into(),
            },
            kairos_client::types_code_index::CodeIndexRef {
                name: "v1.0.0".into(),
                kind: "tag".into(),
            },
        ];
        let rows = [
            index(&"a".repeat(40), Some("main"), Some(6), 2_500_000),
            index(&"b".repeat(40), None, None, 900),
            tagged,
        ];
        // KAIROS-T-0750: the REF column shows each ref that points at the
        // commit with its kind, else the ref of the build.
        assert_eq!(
            index_table(&rows).render(),
            "COMMIT        REF                 SCHEMA  SIZE     SUMMARIES  CREATED           UPDATED\n\
             aaaaaaaaaaaa  main                6       2.4 MiB  7          2026-10-16 21:57  2026-10-18 01:24\n\
             bbbbbbbbbbbb  -                   -       900 B    7          2026-10-16 21:57  2026-10-18 01:24\n\
             cccccccccccc  main, v1.0.0 (tag)  8       900 B    7          2026-10-16 21:57  2026-10-18 01:24\n"
        );
    }

    #[test]
    fn sizes_and_times_are_short() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1536), "1.5 KiB");
        assert_eq!(size_text(22 * 1024 * 1024), "22.0 MiB");
        assert_eq!(minute_text("2026-10-18T03:24:06+02:00"), "2026-10-18 01:24");
        assert_eq!(minute_text("not a time"), "not a time");
    }

    /// COLLIERY-T-0269: the family of `link` and `unlink` comes from the
    /// type letter of the short code.
    #[test]
    fn forge_filter_flags_build_the_patch() {
        let filters = forge_filters_arg(
            &["pr_head_branch=^feature/".to_string()],
            &[
                "pr_author=dependabot\\[bot\\]".to_string(),
                "pr_head_branch=wip".to_string(),
            ],
        )
        .unwrap()
        .unwrap();
        let head = filters.pr_head_branch.as_ref().unwrap();
        assert_eq!(head.include.as_deref(), Some("^feature/"));
        assert_eq!(head.exclude.as_deref(), Some("wip"));
        let author = filters.pr_author.as_ref().unwrap();
        assert_eq!(author.include, None);
        assert_eq!(author.exclude.as_deref(), Some("dependabot\\[bot\\]"));
        assert!(filters.issue_title.is_none());
        assert!(forge_filters_arg(&[], &[]).unwrap().is_none());
        let err = forge_filters_arg(&["issue_body=x".to_string()], &[]).unwrap_err();
        assert!(err.to_string().contains("issue_body"), "{err}");
        assert!(forge_filters_arg(&["pr_title".to_string()], &[]).is_err());
    }

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
