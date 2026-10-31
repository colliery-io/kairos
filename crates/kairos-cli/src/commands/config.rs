//! `kairos config` (#270): the config of the person, `config.toml` in the
//! config directory. `show` gives the binding of a folder (the deployment,
//! the tenant, the repository and the boards, and where each one comes
//! from); the hooks of the plugin read it with `--json`. `project` adds a
//! `[[project]]` for the `origin` remote of a checkout, or for the path of
//! a folder with no remote: a board folder (KAIROS-T-0707). `migrate` moves
//! the values of the retired `.claude/kairos.local.md` to a `[[project]]`
//! (KAIROS-T-0710). `migrate` is the one command that reads that file: no
//! other command and no hook does (KAIROS-T-0742). `path` prints the path
//! of `config.toml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::auth;
use crate::context::print_json;
use crate::credentials;
use crate::error::CliError;
use crate::index::git_output;
use crate::project_config::{self, Added, Binding, Key, Origin, Values};

/// The retired wiring file of a checkout (KAIROS-A-0014, KAIROS-T-0710).
/// Only `config migrate` reads it.
const WIRING_FILE: &str = ".claude/kairos.local.md";

#[derive(clap::Subcommand, Debug)]
pub enum ConfigCommand {
    /// The binding of a folder: the deployment, the tenant, the repository
    /// and the boards, and where each value comes from
    Show(ShowArgs),
    /// Add a [[project]] to config.toml: for the origin remote of a
    /// checkout, else for the path of the folder (a board folder)
    Project(ProjectArgs),
    /// Move the values of the retired .claude/kairos.local.md to a
    /// [[project]] of config.toml
    Migrate(MigrateArgs),
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
    /// The checkout whose origin remote the entry names, or the folder
    /// whose path it names when it has no origin remote (default: the
    /// current folder)
    pub folder: Option<PathBuf>,
    /// The remote of the entry, in place of the origin of the folder
    #[arg(long, conflicts_with = "path")]
    pub remote: Option<String>,
    /// Name the folder by its path, also when it has an origin remote
    #[arg(long)]
    pub path: bool,
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

#[derive(clap::Args, Debug)]
pub struct MigrateArgs {
    /// The checkout of the file (default: the current folder)
    pub folder: Option<PathBuf>,
    /// The remote of the entry, in place of the origin of the folder
    #[arg(long)]
    pub remote: Option<String>,
    /// Print one JSON object
    #[arg(long)]
    pub json: bool,
}

impl ConfigCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            ConfigCommand::Show(args) => show(args),
            ConfigCommand::Project(args) => project(args),
            ConfigCommand::Migrate(args) => migrate(args),
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

/// The wiring file of the checkout at `root` (KAIROS-T-0646): `WIRING_FILE`
/// in `root`, else the same file in the main clone, the folder above `git
/// rev-parse --git-common-dir` (git excluded the file, so a worktree has
/// none). `None` when neither exists. A folder with no `.git` gets no git
/// call.
fn wiring_path(root: &Path) -> Option<PathBuf> {
    let own = root.join(WIRING_FILE);
    if own.is_file() {
        return Some(own);
    }
    if !root.join(".git").exists() {
        return None;
    }
    let common = git_output(root, &["rev-parse", "--git-common-dir"])?;
    let common = std::fs::canonicalize(root.join(common)).ok()?;
    let clone = common.parent()?.join(WIRING_FILE);
    clone.is_file().then_some(clone)
}

/// The `key: value` lines of the front matter of the wiring file: an
/// indented line, a blank line and a `#` line are not keys; one pair of
/// matching quotes is removed from a value; with no quotes, a ` #` comment
/// is cut; an empty value is not set.
fn front_matter(text: &str) -> HashMap<String, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return HashMap::new();
    }
    lines
        .take_while(|line| line.trim() != "---")
        .filter(|line| !line.starts_with(char::is_whitespace) && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let value = front_value(value);
            (!value.is_empty()).then(|| (key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// One value of a front matter line ([`front_matter`]).
fn front_value(raw: &str) -> &str {
    let value = raw.trim();
    let quoted = value.len() >= 2
        && (value.starts_with('"') && value.ends_with('"')
            || value.starts_with('\'') && value.ends_with('\''));
    if quoted {
        return &value[1..value.len() - 1];
    }
    value.find(" #").map_or(value, |cut| value[..cut].trim())
}

/// The values of the binding that the wiring file gives. The file names
/// `deployment` as `deployment_url`.
fn wiring_values(front: &HashMap<String, String>) -> Values {
    let get = |key: &str| front.get(key).cloned();
    Values {
        deployment: get("deployment_url"),
        tenant: get("tenant"),
        repository: get("repository"),
        team_board: get("team_board"),
        initiative_board: get("initiative_board"),
        adr_board: get("adr_board"),
        delivery_stream: get("delivery_stream"),
    }
}

/// What `config migrate` did: the file, the remote, and the result of
/// [`project_config::add_project`]. No file, or a file with no value:
/// nothing to move.
#[derive(Debug, PartialEq)]
struct Migrated {
    file: Option<String>,
    remote: Option<String>,
    added: Option<Added>,
}

/// Move the values of the wiring file of `folder` into a `[[project]]` of
/// the config at `config`. Nothing when the folder has no file, or the
/// file gives no value.
fn migrate_in(folder: &Path, config: &Path, remote: Option<String>) -> Result<Migrated, CliError> {
    let nothing = Migrated {
        file: None,
        remote: None,
        added: None,
    };
    // A subfolder of a checkout has the file of the top (KAIROS-T-0707).
    let top = project_config::checkout_top(folder);
    let folder = top.as_deref().unwrap_or(folder);
    let Some(path) = wiring_path(folder) else {
        return Ok(nothing);
    };
    let text = std::fs::read(&path)
        .map_err(|e| CliError::Failure(format!("The CLI cannot read {}: {e}.", path.display())))?;
    let values = wiring_values(&front_matter(&String::from_utf8_lossy(&text)));
    if values == Values::default() {
        return Ok(nothing);
    }
    let file = path.display().to_string();
    let remote = remote.or_else(|| project_config::origin_of(folder));
    let Some(remote) = remote else {
        return Err(CliError::Failure(format!(
            "The folder {} has no origin remote. Give --remote, or add a [[project]] with \
             `path` to {} by hand.",
            folder.display(),
            config.display()
        )));
    };
    let added = project_config::add_project(
        config,
        &project_config::Key::Remote(remote.clone()),
        &values,
    )?;
    Ok(Migrated {
        file: Some(file),
        remote: Some(crate::index::normalized_remote(&remote)),
        added: Some(added),
    })
}

fn migrate(args: MigrateArgs) -> Result<(), CliError> {
    let folder = folder_or_here(args.folder)?;
    let config = project_config::config_path()?;
    let done = migrate_in(&folder, &config, args.remote)?;
    if args.json {
        return print_json(&json!({
            "config": config.display().to_string(),
            "file": done.file,
            "remote": done.remote,
            "result": done.added,
        }));
    }
    let (Some(file), Some(remote), Some(added)) = (done.file, done.remote, done.added) else {
        println!(
            "Nothing to migrate: {} has no {WIRING_FILE} with a value. Nothing changed.",
            folder.display()
        );
        return Ok(());
    };
    match added {
        Added::Added => println!(
            "Added a [[project]] for {remote} to {} with the values of {file}.",
            config.display()
        ),
        Added::Same => println!(
            "{} has these values for {remote} already. Nothing changed.",
            config.display()
        ),
    }
    println!("Check it with `kairos config show`, then remove {file}. The CLI does not read it.");
    Ok(())
}

fn project(args: ProjectArgs) -> Result<(), CliError> {
    let folder = folder_or_here(args.folder)?;
    let key = entry_key(&folder, args.remote, args.path)?;
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
    let added = project_config::add_project(&config, &key, &values)?;
    // The key as the file has it.
    let added_key = key.normalized();
    let label = added_key.label();
    if args.json {
        let (remote, path) = match &added_key {
            Key::Remote(remote) => (Some(remote.clone()), None),
            Key::Path(path) => (None, Some(path.display().to_string())),
        };
        return print_json(&json!({
            "config": config.display().to_string(),
            "remote": remote,
            "path": path,
            "result": added,
        }));
    }
    match added {
        Added::Added => println!("Added a [[project]] for {label} to {}.", config.display()),
        Added::Same => println!(
            "{} has this [[project]] for {label} already. Nothing changed.",
            config.display()
        ),
    }
    Ok(())
}

/// What the new `[[project]]` is found by (KAIROS-T-0707): `--remote`;
/// else the path of the folder with `--path`; else the `origin` remote of
/// the checkout of the folder; else the path of the folder. A subfolder of
/// a checkout names the top of the checkout. A board folder (no checkout)
/// and a checkout with no remote get a `path` entry.
fn entry_key(
    folder: &std::path::Path,
    remote: Option<String>,
    path: bool,
) -> Result<Key, CliError> {
    if let Some(remote) = remote {
        return Ok(Key::Remote(remote));
    }
    if !folder.is_dir() {
        return Err(CliError::Failure(format!(
            "The folder {} does not exist.",
            folder.display()
        )));
    }
    let top = project_config::checkout_top(folder);
    if !path && let Some(remote) = project_config::origin_of(folder) {
        return Ok(Key::Remote(remote));
    }
    let folder = top.unwrap_or_else(|| folder.to_path_buf());
    let folder = folder.canonicalize().map_err(|e| {
        CliError::Failure(format!(
            "The CLI cannot read the folder {}: {e}.",
            folder.display()
        ))
    })?;
    Ok(Key::Path(folder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::Resolved;

    fn git(dir: &Path, args: &[&str]) {
        // A CI runner has no git identity: a commit needs one.
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_AUTHOR_NAME", "Robin")
            .env("GIT_AUTHOR_EMAIL", "robin@example.test")
            .env("GIT_COMMITTER_NAME", "Robin")
            .env("GIT_COMMITTER_EMAIL", "robin@example.test")
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A checkout with an origin remote and a wiring file.
    fn wired_checkout(dir: &Path) -> PathBuf {
        let clone = dir.join("app");
        std::fs::create_dir_all(clone.join(".claude")).expect("make .claude");
        git(&clone, &["init", "-q"]);
        git(
            &clone,
            &["remote", "add", "origin", "git@github.com:acme/app.git"],
        );
        std::fs::write(
            clone.join(".claude/kairos.local.md"),
            "---\ndeployment_url: https://kairos.example.com\ntenant: acme\nteam_board: app-delivery\n---\n",
        )
        .expect("write the wiring");
        clone
    }

    /// KAIROS-T-0710: `config migrate` writes the values of the file to a
    /// [[project]]. KAIROS-T-0742: no other command reads the file, so the
    /// binding has none of its values before the migrate.
    #[test]
    fn migrate_moves_the_values_of_the_file_to_a_project() {
        let dir = tempfile::tempdir().expect("a folder");
        let clone = wired_checkout(dir.path());
        let config = dir.path().join("config/config.toml");

        let before = project_config::binding_with(&clone, &config).expect("a binding");
        assert_eq!(before.get("tenant"), None, "the file is not read");
        assert!(!before.is_bound());

        let done = migrate_in(&clone, &config, None).expect("migrate");
        assert_eq!(done.added, Some(Added::Added));
        assert_eq!(done.remote.as_deref(), Some("github.com/acme/app"));
        let text = std::fs::read_to_string(&config).expect("the config");
        assert!(text.contains("remote = \"github.com/acme/app\""), "{text}");
        assert!(
            text.contains("deployment = \"https://kairos.example.com\""),
            "{text}"
        );
        assert!(text.contains("team_board = \"app-delivery\""), "{text}");

        let after = project_config::binding_with(&clone, &config).expect("a binding");
        assert!(after.is_bound());
        assert_eq!(after.get("tenant"), Some("acme"));
        // A second run has the same values: nothing changes.
        assert_eq!(
            migrate_in(&clone, &config, None).expect("migrate").added,
            Some(Added::Same)
        );
    }

    /// KAIROS-T-0742: with no file, or a file with no value, there is
    /// nothing to migrate.
    #[test]
    fn migrate_with_no_file_or_no_value_has_nothing_to_move() {
        let dir = tempfile::tempdir().expect("a folder");
        let clone = wired_checkout(dir.path());
        let config = dir.path().join("config.toml");
        let nothing = Migrated {
            file: None,
            remote: None,
            added: None,
        };
        std::fs::write(clone.join(WIRING_FILE), "---\nrepository:\n---\n").expect("write");
        assert_eq!(migrate_in(&clone, &config, None).expect("migrate"), nothing);
        std::fs::remove_file(clone.join(WIRING_FILE)).expect("remove");
        assert_eq!(migrate_in(&clone, &config, None).expect("migrate"), nothing);
        assert!(!config.exists());
    }

    /// KAIROS-T-0646: git excluded the file, so a worktree has none;
    /// `migrate` in a worktree reads the file of its clone.
    #[test]
    fn migrate_in_a_worktree_reads_the_file_of_its_clone() {
        let dir = tempfile::tempdir().expect("a folder");
        let clone = wired_checkout(dir.path());
        git(&clone, &["commit", "-q", "--allow-empty", "-m", "A"]);
        let worktree = dir.path().join("wt-app");
        let path = worktree.display().to_string();
        git(&clone, &["worktree", "add", "-q", "-b", "work", &path]);
        assert!(!worktree.join(".claude").exists());
        assert_eq!(
            wiring_path(&worktree).expect("the file of the clone"),
            clone.canonicalize().expect("the clone").join(WIRING_FILE)
        );
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).expect("make the folder");
        assert_eq!(wiring_path(&plain), None);
        let config = dir.path().join("config.toml");
        let done = migrate_in(&worktree, &config, None).expect("migrate");
        assert_eq!(done.added, Some(Added::Added));
        assert_eq!(done.remote.as_deref(), Some("github.com/acme/app"));
    }

    /// The front matter rule of the retired file: an indented line, a
    /// comment line and an empty value are not keys; one pair of quotes is
    /// removed; a ` #` comment after a value with no quotes is cut.
    #[test]
    fn the_front_matter_of_the_file_is_read_by_one_rule() {
        let front = front_matter(
            "---\n# tenant: commented\n  tenant: indented\nrepository:\n\
             team_board: \"delivery\" \ntenant: acme # the tenant\nadr_board: 'a # b'\n---\n\n\
             repository: not this\n",
        );
        assert_eq!(front["team_board"], "delivery");
        assert_eq!(front["tenant"], "acme");
        assert_eq!(front["adr_board"], "a # b");
        assert_eq!(front.len(), 3);
        assert!(front_matter("tenant: acme\n").is_empty());
        let values = wiring_values(&front);
        assert_eq!(values.deployment, None);
        assert_eq!(values.tenant.as_deref(), Some("acme"));
    }

    #[test]
    fn migrate_with_no_remote_is_refused_and_says_what_to_give() {
        let dir = tempfile::tempdir().expect("a folder");
        let clone = wired_checkout(dir.path());
        git(&clone, &["remote", "remove", "origin"]);
        let config = dir.path().join("config.toml");
        let err = migrate_in(&clone, &config, None).expect_err("no remote");
        assert!(err.to_string().contains("--remote"), "{err}");
        assert!(!config.exists());
        git(
            &clone,
            &["remote", "add", "origin", "git@github.com:acme/app.git"],
        );
        let done = migrate_in(&clone, &config, Some("https://github.com/acme/app".into()))
            .expect("migrate with --remote");
        assert_eq!(done.added, Some(Added::Added));
    }

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
