//! `kairos config` (#270): the config of the person, `config.toml` in the
//! config directory. `show` gives the binding of a folder (the deployment,
//! the tenant, the repository and the boards, and where each one comes
//! from); the hooks of the plugin read it with `--json`. `project` adds a
//! `[[project]]` for the `origin` remote of a checkout. `path` prints the
//! path of the file.

use std::path::PathBuf;

use serde_json::json;

use crate::auth;
use crate::context::print_json;
use crate::credentials;
use crate::error::CliError;
use crate::project_config::{self, Added, Binding, Origin, Values};

#[derive(clap::Subcommand, Debug)]
pub enum ConfigCommand {
    /// The binding of a folder: the deployment, the tenant, the repository
    /// and the boards, and where each value comes from
    Show(ShowArgs),
    /// Add a [[project]] for the origin remote of a checkout to config.toml
    Project(ProjectArgs),
    /// Print the path of config.toml
    Path,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The folder (default: the current folder)
    pub folder: Option<PathBuf>,
    /// Print one JSON object
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug)]
pub struct ProjectArgs {
    /// The checkout whose origin remote the entry names (default: the
    /// current folder)
    pub folder: Option<PathBuf>,
    /// The remote of the entry, in place of the origin of the folder
    #[arg(long)]
    pub remote: Option<String>,
    /// The deployment URL of the project
    #[arg(long)]
    pub deployment: Option<String>,
    /// The tenant of the project
    #[arg(long)]
    pub tenant: Option<String>,
    /// The repository in Kairos, by its slug
    #[arg(long)]
    pub repository: Option<String>,
    /// The delivery board of the team
    #[arg(long)]
    pub team_board: Option<String>,
    /// The initiative board
    #[arg(long)]
    pub initiative_board: Option<String>,
    /// The ADR board
    #[arg(long)]
    pub adr_board: Option<String>,
    /// The delivery stream
    #[arg(long)]
    pub delivery_stream: Option<String>,
    /// Print one JSON object
    #[arg(long)]
    pub json: bool,
}

impl ConfigCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            ConfigCommand::Show(args) => show(args),
            ConfigCommand::Project(args) => project(args),
            ConfigCommand::Path => {
                println!("{}", project_config::config_path()?.display());
                Ok(())
            }
        }
    }
}

fn folder_or_here(folder: Option<PathBuf>) -> Result<PathBuf, CliError> {
    match folder {
        Some(folder) => Ok(folder),
        None => std::env::current_dir().map_err(|e| {
            CliError::Failure(format!("The CLI cannot read the current folder: {e}."))
        }),
    }
}

fn show(args: ShowArgs) -> Result<(), CliError> {
    let folder = folder_or_here(args.folder)?;
    let config = project_config::config_path()?;
    let binding = project_config::binding_with(&folder, &config)?;
    // KAIROS_URL comes before the binding (#270): say so.
    let env_url = auth::Env::from_process().url;
    if args.json {
        return print_json(&json!({
            "config": config.display().to_string(),
            "folder": binding.folder,
            "remote": binding.remote,
            "project": binding.project,
            "bound": binding.is_bound(),
            "values": binding.values,
            "env_url": env_url,
            "login_url": login_url(),
        }));
    }
    print!(
        "{}",
        show_text(&config.display().to_string(), &binding, env_url.as_deref())
    );
    Ok(())
}

/// The deployment of the only login in the credential cache, or None
/// (no login, more than one, or a cache that cannot be read). A command
/// with no deployment in the binding uses it (KAIROS-T-0708); the hooks
/// name it as the source of the deployment.
fn login_url() -> Option<String> {
    let store = credentials::load(&credentials::credentials_path().ok()?).ok()?;
    only_login(&store)
}

fn only_login(store: &credentials::CredentialStore) -> Option<String> {
    let mut deployments = store.deployments.keys();
    match (deployments.next(), deployments.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

/// The text of `kairos config show`.
fn show_text(config: &str, binding: &Binding, env_url: Option<&str>) -> String {
    let mut out = format!("Folder: {}\n", binding.folder);
    out.push_str(&format!(
        "Remote: {}\n",
        binding.remote.as_deref().unwrap_or("none")
    ));
    match &binding.project {
        Some(entry) => out.push_str(&format!(
            "Project: the [[project]] with {entry} in {config}\n"
        )),
        None => out.push_str(&format!("Project: none in {config}\n")),
    }
    if binding.values.is_empty() {
        out.push_str("No value is set for this folder.\n");
    }
    for (key, resolved) in &binding.values {
        let from = match &resolved.origin {
            Origin::Project { .. } => "project".to_string(),
            Origin::Wiring { file } => file.clone(),
            Origin::Default { .. } => "default".to_string(),
        };
        out.push_str(&format!("{key}: {} ({from})\n", resolved.value));
    }
    if let Some(url) = env_url {
        out.push_str(&format!(
            "{} is set to {url}: commands use it in place of the deployment above.\n",
            auth::URL_VAR
        ));
    }
    out
}

fn project(args: ProjectArgs) -> Result<(), CliError> {
    let folder = folder_or_here(args.folder)?;
    let remote = match args.remote {
        Some(remote) => remote,
        None => project_config::origin_of(&folder).ok_or_else(|| {
            CliError::Failure(format!(
                "The folder {} has no origin remote. Give --remote, or add a [[project]] with \
                 `path` to the config by hand.",
                folder.display()
            ))
        })?,
    };
    let values = Values {
        deployment: args.deployment,
        tenant: args.tenant,
        repository: args.repository,
        team_board: args.team_board,
        initiative_board: args.initiative_board,
        adr_board: args.adr_board,
        delivery_stream: args.delivery_stream,
    };
    let config = project_config::config_path()?;
    let added = project_config::add_project(&config, &remote, &values)?;
    let remote = crate::index::normalized_remote(&remote);
    if args.json {
        return print_json(&json!({
            "config": config.display().to_string(),
            "remote": remote,
            "result": added,
        }));
    }
    match added {
        Added::Added => println!("Added a [[project]] for {remote} to {}.", config.display()),
        Added::Same => println!(
            "{} has this [[project]] for {remote} already. Nothing changed.",
            config.display()
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::Resolved;

    /// KAIROS-T-0708: the only login gives the deployment of a binding with
    /// none; no login or two logins give none.
    #[test]
    fn the_login_url_is_the_only_login() {
        let entry = credentials::DeploymentCredentials::local_session(
            &kairos_client::types_auth::Secret::new("not-a-secret"),
            0,
            "robin@example.com".into(),
            None,
        );
        let mut store = credentials::CredentialStore::default();
        assert_eq!(only_login(&store), None);
        store
            .deployments
            .insert("https://kairos.example.com".into(), entry.clone());
        assert_eq!(
            only_login(&store).as_deref(),
            Some("https://kairos.example.com")
        );
        store
            .deployments
            .insert("https://other.example.com".into(), entry);
        assert_eq!(only_login(&store), None);
    }

    #[test]
    fn show_names_the_origin_of_each_value_and_kairos_url() {
        let mut binding = Binding {
            folder: "/work/app".into(),
            remote: Some("github.com/acme/app".into()),
            project: Some("remote = \"github.com/acme/app\"".into()),
            ..Default::default()
        };
        binding.values.insert(
            "deployment".into(),
            Resolved {
                value: "https://kairos.example.com".into(),
                origin: Origin::Default {
                    file: "/c/config.toml".into(),
                },
            },
        );
        binding.values.insert(
            "team_board".into(),
            Resolved {
                value: "other-delivery".into(),
                origin: Origin::Project {
                    file: "/c/config.toml".into(),
                    entry: "remote = \"github.com/acme/app\"".into(),
                },
            },
        );
        assert_eq!(
            show_text("/c/config.toml", &binding, Some("https://elsewhere")),
            "Folder: /work/app\n\
             Remote: github.com/acme/app\n\
             Project: the [[project]] with remote = \"github.com/acme/app\" in /c/config.toml\n\
             deployment: https://kairos.example.com (default)\n\
             team_board: other-delivery (project)\n\
             KAIROS_URL is set to https://elsewhere: commands use it in place of the deployment above.\n"
        );
    }
}
