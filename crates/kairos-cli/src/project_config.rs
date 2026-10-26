//! The config of the person (#270): `config.toml` in the config directory
//! of the CLI (`KAIROS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/kairos`, else
//! `~/.config/kairos`, as `credentials.json`). It binds a repository to a
//! deployment and to the boards of a team with no Kairos file in the
//! repository:
//!
//! ```toml
//! [default]
//! deployment = "https://kairos.example.com"
//! tenant     = "example"
//! team_board = "team-delivery"
//!
//! [[project]]
//! remote     = "github.com/example-org/example-repo"
//! team_board = "other-delivery"
//! ```
//!
//! A `[[project]]` is found by the `origin` remote of the checkout, as
//! `host/path` ([`normalized_remote`]): each clone and each worktree of a
//! repository has the same one. A repository with no remote is found by
//! `path`, the folder of the checkout (or a folder above it).
//!
//! KAIROS-T-0707: a subfolder of a checkout has the binding of the top of
//! the checkout ([`checkout_top`]). A board folder, which holds clones and
//! is no checkout, is bound by a `[[project]]` with `path`: `kairos config
//! project` writes one in a folder with no `origin` remote.
//!
//! The order is the same for each command, for the hooks of the plugin
//! (`kairos config show --json`) and for `kairos mcp-proxy`: a flag, then
//! the environment (`KAIROS_URL`), then the `[[project]]` of the checkout,
//! then `.claude/kairos.local.md` of the checkout, then `[default]`. A login
//! gives the last deployment and tenant ([`crate::auth::resolve`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::credentials;
use crate::error::CliError;
use crate::index::{git_output, normalized_remote, settings, settings_path};

/// The name of the file in the config directory.
pub const CONFIG_FILE: &str = "config.toml";

/// The keys of a binding. Each one has the name of the same key in
/// `.claude/kairos.local.md`, but `deployment` is `deployment_url` there.
pub const KEYS: [&str; 7] = [
    "deployment",
    "tenant",
    "repository",
    "team_board",
    "initiative_board",
    "adr_board",
    "delivery_stream",
];

/// The values of `[default]` or of one `[[project]]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Values {
    pub deployment: Option<String>,
    pub tenant: Option<String>,
    pub repository: Option<String>,
    pub team_board: Option<String>,
    pub initiative_board: Option<String>,
    pub adr_board: Option<String>,
    pub delivery_stream: Option<String>,
}

impl Values {
    /// The values that are set, by key ([`KEYS`]). A blank value is not set.
    fn pairs(&self) -> Vec<(&'static str, &str)> {
        [
            ("deployment", &self.deployment),
            ("tenant", &self.tenant),
            ("repository", &self.repository),
            ("team_board", &self.team_board),
            ("initiative_board", &self.initiative_board),
            ("adr_board", &self.adr_board),
            ("delivery_stream", &self.delivery_stream),
        ]
        .into_iter()
        .filter_map(|(key, value)| {
            let value = value.as_deref()?.trim();
            (!value.is_empty()).then_some((key, value))
        })
        .collect()
    }
}

/// One `[[project]]`: `remote` or `path`, and its values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// The `origin` remote, in any form that git takes; it is compared as
    /// `host/path`.
    pub remote: Option<String>,
    /// The folder of a checkout with no remote. `~/` is the home folder.
    pub path: Option<String>,
    pub deployment: Option<String>,
    pub tenant: Option<String>,
    pub repository: Option<String>,
    pub team_board: Option<String>,
    pub initiative_board: Option<String>,
    pub adr_board: Option<String>,
    pub delivery_stream: Option<String>,
}

impl Project {
    fn values(&self) -> Values {
        Values {
            deployment: self.deployment.clone(),
            tenant: self.tenant.clone(),
            repository: self.repository.clone(),
            team_board: self.team_board.clone(),
            initiative_board: self.initiative_board.clone(),
            adr_board: self.adr_board.clone(),
            delivery_stream: self.delivery_stream.clone(),
        }
    }

    /// The name of the entry in a message: its remote, or its path.
    fn label(&self) -> String {
        match (&self.remote, &self.path) {
            (Some(remote), _) => format!("remote = {remote:?}"),
            (None, Some(path)) => format!("path = {path:?}"),
            (None, None) => "with no remote and no path".to_string(),
        }
    }
}

/// The parsed `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    #[serde(default)]
    pub default: Values,
    #[serde(default, rename = "project")]
    pub projects: Vec<Project>,
}

/// `config.toml` in the config directory.
pub fn config_path() -> Result<PathBuf, CliError> {
    Ok(credentials::config_dir()?.join(CONFIG_FILE))
}

/// The config at `path`. A missing file is an empty config. A file that
/// does not parse, or a `[[project]]` with no `remote` and no `path`, is a
/// failure that names the file: a typo must not send work to the wrong
/// deployment.
pub fn load(path: &Path) -> Result<UserConfig, CliError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(UserConfig::default()),
        Err(e) => {
            return Err(CliError::Failure(format!(
                "The CLI cannot read {}: {e}.",
                path.display()
            )));
        }
    };
    let config: UserConfig = toml::from_str(&text).map_err(|e| {
        CliError::Failure(format!(
            "The file {} is not correct: {}",
            path.display(),
            e.to_string().trim_end()
        ))
    })?;
    if let Some((index, bad)) = config
        .projects
        .iter()
        .enumerate()
        .find(|(_, p)| p.remote.is_none() == p.path.is_none())
    {
        let line = project_line(&text, index)
            .map(|line| format!(" at line {line}"))
            .unwrap_or_default();
        return Err(CliError::Failure(format!(
            "The [[project]]{line} of {} has {}. Give it `remote` or `path`, not both and not neither.",
            path.display(),
            if bad.remote.is_some() {
                "both `remote` and `path`"
            } else {
                "no `remote` and no `path`"
            }
        )));
    }
    Ok(config)
}

/// The line (from 1) of the header of the `[[project]]` at `index` in
/// `text`, for the message of a refusal (KAIROS-T-0706).
fn project_line(text: &str, index: usize) -> Option<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim();
            line.strip_prefix("[[")
                .and_then(|rest| rest.split_once("]]"))
                .is_some_and(|(name, after)| {
                    name.trim() == "project" && {
                        let after = after.trim();
                        after.is_empty() || after.starts_with('#')
                    }
                })
        })
        .nth(index)
        .map(|(n, _)| n + 1)
}

/// Where a value comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Origin {
    /// The `[[project]]` of the checkout.
    Project { file: String, entry: String },
    /// `.claude/kairos.local.md` of the checkout or of its clone.
    Wiring { file: String },
    /// `[default]`.
    Default { file: String },
}

/// One resolved value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub value: String,
    #[serde(flatten)]
    pub origin: Origin,
}

/// The binding of the checkout at `folder`, with no flag and no
/// environment: `[[project]]`, then `.claude/kairos.local.md`, then
/// `[default]`. The keys are those of [`KEYS`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Binding {
    /// The checkout, or the folder when it is not one.
    pub folder: String,
    /// The `origin` remote as `host/path`.
    pub remote: Option<String>,
    /// The `[[project]]` of the folder, by its remote or its path.
    pub project: Option<String>,
    pub values: BTreeMap<String, Resolved>,
}

impl Binding {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|r| r.value.as_str())
    }

    /// The values with the names of `.claude/kairos.local.md`
    /// (`deployment_url`), for the code that reads that file.
    pub fn as_settings(&self) -> std::collections::HashMap<String, String> {
        self.values
            .iter()
            .map(|(key, r)| (wiring_key(key).to_string(), r.value.clone()))
            .collect()
    }

    /// True when a `[[project]]` or `.claude/kairos.local.md` binds the
    /// folder. `[default]` alone binds nothing: the hooks of the plugin
    /// stay silent in a repository of no project.
    pub fn is_bound(&self) -> bool {
        self.project.is_some()
            || self
                .values
                .values()
                .any(|r| matches!(r.origin, Origin::Wiring { .. }))
    }
}

/// The name of `key` in `.claude/kairos.local.md`.
fn wiring_key(key: &str) -> &str {
    if key == "deployment" {
        "deployment_url"
    } else {
        key
    }
}

/// The top of the checkout that holds `folder` (KAIROS-T-0707): `git
/// rev-parse --show-toplevel`. `None` when no folder from `folder` up has
/// a `.git`: then there is no git call.
pub fn checkout_top(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find(|dir| dir.join(".git").exists())?;
    git_output(folder, &["rev-parse", "--show-toplevel"])
        .filter(|top| !top.is_empty())
        .map(PathBuf::from)
}

/// The `origin` remote of the checkout that holds `folder`, as
/// `host/path`. A subfolder has the remote of its checkout.
pub fn origin_of(folder: &Path) -> Option<String> {
    remote_of_top(&checkout_top(folder)?)
}

/// The `origin` remote of the checkout at its top folder `top`.
fn remote_of_top(top: &Path) -> Option<String> {
    git_output(top, &["config", "--get", "remote.origin.url"])
        .map(|url| normalized_remote(&url))
        .filter(|r| !r.is_empty())
}

/// `~/` in `path` as the home folder.
fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The `[[project]]` of the folder: the first one whose remote is the
/// origin of the folder, else the first one whose path holds the folder.
fn project_for<'a>(
    config: &'a UserConfig,
    folder: &Path,
    remote: Option<&str>,
) -> Option<&'a Project> {
    if let Some(remote) = remote
        && let Some(p) = config
            .projects
            .iter()
            .find(|p| p.remote.as_deref().map(normalized_remote).as_deref() == Some(remote))
    {
        return Some(p);
    }
    let folder = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    config.projects.iter().find(|p| {
        p.path.as_deref().is_some_and(|path| {
            let path = expand(path);
            let path = path.canonicalize().unwrap_or(path);
            folder.starts_with(&path)
        })
    })
}

/// The binding of `folder` with the config at `config_file`.
/// A subfolder of a checkout has the binding of the top of the checkout
/// (KAIROS-T-0707): its remote, its `.claude/kairos.local.md`. A `path` of
/// a `[[project]]` holds `folder` or a folder above it.
pub fn binding_with(folder: &Path, config_file: &Path) -> Result<Binding, CliError> {
    let config = load(config_file)?;
    let top = checkout_top(folder);
    let root = top.as_deref().unwrap_or(folder);
    let remote = top.as_deref().and_then(remote_of_top);
    let file = config_file.display().to_string();
    let mut values = BTreeMap::new();
    for (key, value) in config.default.pairs() {
        values.insert(
            key.to_string(),
            Resolved {
                value: value.to_string(),
                origin: Origin::Default { file: file.clone() },
            },
        );
    }
    if let Some(wiring) = settings_path(root) {
        let wiring = wiring.display().to_string();
        let found = settings(root);
        for key in KEYS {
            if let Some(value) = found.get(wiring_key(key)) {
                values.insert(
                    key.to_string(),
                    Resolved {
                        value: value.clone(),
                        origin: Origin::Wiring {
                            file: wiring.clone(),
                        },
                    },
                );
            }
        }
    }
    let project = project_for(&config, folder, remote.as_deref());
    if let Some(project) = project {
        let entry = project.label();
        for (key, value) in project.values().pairs() {
            values.insert(
                key.to_string(),
                Resolved {
                    value: value.to_string(),
                    origin: Origin::Project {
                        file: file.clone(),
                        entry: entry.clone(),
                    },
                },
            );
        }
    }
    Ok(Binding {
        folder: root.display().to_string(),
        remote,
        project: project.map(Project::label),
        values,
    })
}

/// The binding of `folder` with the config of the person.
pub fn binding(folder: &Path) -> Result<Binding, CliError> {
    binding_with(folder, &config_path()?)
}

/// The folder of the command: `CLAUDE_PROJECT_DIR` when a hook or an MCP
/// server of the plugin runs it, else the current folder.
pub fn folder_here() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
}

/// The binding of the folder of the command ([`folder_here`]).
/// KAIROS-T-0706: a `config.toml` that does not parse is an error that
/// names the file and the line. It is never ignored: before, the command
/// used `[default]` or the login, which can be another deployment. With no
/// folder or no config directory there is no binding.
pub fn binding_here() -> Result<Binding, CliError> {
    let (Some(folder), Ok(config)) = (folder_here(), config_path()) else {
        return Ok(Binding::default());
    };
    binding_with(&folder, &config)
}

/// What a new `[[project]]` is found by: the `origin` remote, or the path
/// of a folder with no remote (KAIROS-T-0707: a board folder).
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Remote(String),
    Path(PathBuf),
}

impl Key {
    /// The key as it is written and compared: the remote as `host/path`,
    /// the path as an absolute folder.
    pub fn normalized(&self) -> Key {
        match self {
            Key::Remote(remote) => Key::Remote(normalized_remote(remote)),
            Key::Path(path) => Key::Path(path.canonicalize().unwrap_or_else(|_| path.clone())),
        }
    }

    /// True when `project` is the entry of this key.
    fn names(&self, project: &Project) -> bool {
        match self {
            Key::Remote(remote) => {
                project.remote.as_deref().map(normalized_remote).as_deref() == Some(remote.as_str())
            }
            Key::Path(path) => project.path.as_deref().is_some_and(|p| {
                let p = expand(p);
                p.canonicalize().unwrap_or(p) == *path
            }),
        }
    }

    /// The key in a message: the remote, or the path.
    pub fn label(&self) -> String {
        match self {
            Key::Remote(remote) => remote.clone(),
            Key::Path(path) => path.display().to_string(),
        }
    }
}

/// The text of a new `[[project]]` for `key`, with the values that are
/// set. The values are TOML basic strings.
fn project_block(key: &Key, values: &Values) -> String {
    let head = match key {
        Key::Remote(remote) => format!("remote = {}", toml_string(remote)),
        Key::Path(path) => format!("path = {}", toml_string(&path.display().to_string())),
    };
    let mut out = format!("\n[[project]]\n{head}\n");
    for (key, value) in values.pairs() {
        out.push_str(&format!("{key} = {}\n", toml_string(value)));
    }
    out
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

/// What [`add_project`] did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Added {
    /// The file has a new `[[project]]`.
    Added,
    /// The file has this `[[project]]` with these values already.
    Same,
}

/// Add a `[[project]]` for `key` to the config at `path` (#270: what
/// `bootstrap` writes). The CLI appends the text and keeps the rest of the
/// file as it is, with its comments. A `[[project]]` of the same key with
/// other values is a refusal: the person edits the file.
pub fn add_project(path: &Path, key: &Key, values: &Values) -> Result<Added, CliError> {
    let key = key.normalized();
    let label = key.label();
    let config = load(path)?;
    if let Some(existing) = config.projects.iter().find(|p| key.names(p)) {
        let mut asked = existing.values();
        for (key, value) in values.pairs() {
            set(&mut asked, key, value);
        }
        if asked.pairs() == existing.values().pairs() {
            return Ok(Added::Same);
        }
        return Err(CliError::Failure(format!(
            "{} has a [[project]] for {label} with other values. Edit that entry in the file, \
             or remove it and run the command again.",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::Failure(format!("The CLI cannot make {}: {e}.", parent.display()))
        })?;
    }
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&project_block(&key, values));
    // The new text must parse: a refusal here keeps the old file.
    toml::from_str::<UserConfig>(&text).map_err(|e| {
        CliError::Failure(format!("The new {} would not parse: {e}", path.display()))
    })?;
    std::fs::write(path, text)
        .map_err(|e| CliError::Failure(format!("The CLI cannot write {}: {e}.", path.display())))?;
    Ok(Added::Added)
}

fn set(values: &mut Values, key: &str, value: &str) {
    let slot = match key {
        "deployment" => &mut values.deployment,
        "tenant" => &mut values.tenant,
        "repository" => &mut values.repository,
        "team_board" => &mut values.team_board,
        "initiative_board" => &mut values.initiative_board,
        "adr_board" => &mut values.adr_board,
        "delivery_stream" => &mut values.delivery_stream,
        _ => return,
    };
    *slot = Some(value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn clone_with_origin(dir: &Path, remote: &str) -> PathBuf {
        let clone = dir.join("clone");
        std::fs::create_dir_all(&clone).unwrap();
        git(&clone, &["init", "-q"]);
        git(&clone, &["remote", "add", "origin", remote]);
        clone
    }

    fn wire(folder: &Path, front: &str) {
        std::fs::create_dir_all(folder.join(".claude")).unwrap();
        std::fs::write(
            folder.join(".claude/kairos.local.md"),
            format!("---\n{front}---\n\nWiring.\n"),
        )
        .unwrap();
    }

    const CONFIG: &str = r#"
# The deployment of the company.
[default]
deployment = "https://kairos.example.com"
tenant     = "example"
team_board = "team-delivery"

[[project]]
remote     = "git@github.com:Example-Org/example-repo.git"
team_board = "other-delivery"
"#;

    /// #270: the project of the remote, then the wiring, then [default].
    #[test]
    fn a_project_is_found_by_its_remote_and_wins_over_the_wiring_and_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, CONFIG).unwrap();
        let clone = clone_with_origin(dir.path(), "https://github.com/example-org/example-repo");
        wire(
            &clone,
            "tenant: from-wiring\nteam_board: wired-board\nadr_board: adrs\n",
        );

        let b = binding_with(&clone, &config).unwrap();
        assert_eq!(
            b.remote.as_deref(),
            Some("github.com/example-org/example-repo")
        );
        assert!(b.project.is_some());
        assert!(b.is_bound());
        assert_eq!(b.get("deployment"), Some("https://kairos.example.com"));
        assert!(matches!(
            b.values["deployment"].origin,
            Origin::Default { .. }
        ));
        assert_eq!(b.get("tenant"), Some("from-wiring"));
        assert!(matches!(b.values["tenant"].origin, Origin::Wiring { .. }));
        assert_eq!(b.get("team_board"), Some("other-delivery"));
        assert!(matches!(
            b.values["team_board"].origin,
            Origin::Project { .. }
        ));
        assert_eq!(b.get("adr_board"), Some("adrs"));
        assert_eq!(
            b.as_settings().get("deployment_url").map(String::as_str),
            Some("https://kairos.example.com")
        );
    }

    /// A worktree has the remote of its clone, so the same project.
    #[test]
    fn a_worktree_finds_the_project_of_its_clone() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, CONFIG).unwrap();
        let clone = clone_with_origin(dir.path(), "github.com:example-org/example-repo.git");
        git(
            &clone,
            &[
                "-c",
                "user.name=R",
                "-c",
                "user.email=r@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "first",
            ],
        );
        let worktree = dir.path().join("wt");
        git(
            &clone,
            &["worktree", "add", "-q", worktree.to_str().unwrap()],
        );
        let b = binding_with(&worktree, &config).unwrap();
        assert_eq!(b.get("team_board"), Some("other-delivery"));
    }

    /// [default] alone gives values, but binds no folder.
    #[test]
    fn the_default_alone_binds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, CONFIG).unwrap();
        let other = clone_with_origin(dir.path(), "https://github.com/someone/else");
        let b = binding_with(&other, &config).unwrap();
        assert_eq!(b.project, None);
        assert!(!b.is_bound());
        assert_eq!(b.get("team_board"), Some("team-delivery"));
    }

    /// A repository with no remote is found by the path of its folder.
    #[test]
    fn a_project_is_found_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("scratch");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            format!(
                "[[project]]\npath = {:?}\ntenant = \"local\"\n",
                folder.display().to_string()
            ),
        )
        .unwrap();
        let b = binding_with(&folder.join("sub"), &config).unwrap();
        assert_eq!(b.get("tenant"), Some("local"));
        assert!(b.is_bound());
    }

    /// No file is an empty config; a bad file is a failure that names it.
    #[test]
    fn a_bad_config_is_refused_and_named() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        assert_eq!(load(&config).unwrap(), UserConfig::default());
        std::fs::write(&config, "[default]\ndeploymnet = \"x\"\n").unwrap();
        let err = load(&config).unwrap_err().to_string();
        assert!(
            err.contains("config.toml is not correct")
                && err.contains("line 2")
                && err.contains("deploymnet"),
            "{err}"
        );
        std::fs::write(
            &config,
            "[[project]]\nremote = \"a/b\"\n\n# two\n[[project]] # no remote\ntenant = \"x\"\n",
        )
        .unwrap();
        let err = load(&config).unwrap_err().to_string();
        assert!(
            err.contains("no `remote` and no `path`") && err.contains("at line 5"),
            "{err}"
        );
    }

    fn remote(remote: &str) -> Key {
        Key::Remote(remote.to_string())
    }

    /// KAIROS-T-0707: a subfolder of a checkout has the binding of the top
    /// of the checkout: its remote and its `.claude/kairos.local.md`.
    #[test]
    fn a_subfolder_has_the_binding_of_the_top_of_its_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, CONFIG).unwrap();
        let clone = clone_with_origin(dir.path(), "https://github.com/example-org/example-repo");
        wire(&clone, "adr_board: adrs\n");
        let sub = clone.join("crates").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        let b = binding_with(&sub, &config).unwrap();
        assert_eq!(
            b.remote.as_deref(),
            Some("github.com/example-org/example-repo")
        );
        assert!(b.is_bound());
        assert_eq!(b.get("team_board"), Some("other-delivery"));
        assert_eq!(b.get("adr_board"), Some("adrs"));
        assert_eq!(
            Path::new(&b.folder).canonicalize().unwrap(),
            clone.canonicalize().unwrap(),
            "the folder of the binding is the top of the checkout"
        );
        assert_eq!(
            origin_of(&sub).as_deref(),
            Some("github.com/example-org/example-repo")
        );
    }

    /// KAIROS-T-0707: a board folder (clones, no checkout) is bound by a
    /// `[[project]]` with `path`, also from a clone under it that has no
    /// entry of its own.
    #[test]
    fn a_board_folder_is_bound_by_a_path_entry() {
        let dir = tempfile::tempdir().unwrap();
        let board = dir.path().join("board");
        std::fs::create_dir_all(&board).unwrap();
        let clone = clone_with_origin(&board, "https://github.com/someone/else");
        let config = dir.path().join("config.toml");
        let values = Values {
            deployment: Some("https://kairos.example.com".into()),
            team_board: Some("team-delivery".into()),
            ..Default::default()
        };
        assert_eq!(
            add_project(&config, &Key::Path(board.clone()), &values).unwrap(),
            Added::Added
        );
        assert_eq!(
            add_project(&config, &Key::Path(board.clone()), &values).unwrap(),
            Added::Same
        );
        let b = binding_with(&board, &config).unwrap();
        assert!(b.is_bound(), "{b:?}");
        assert_eq!(b.remote, None);
        assert_eq!(b.get("team_board"), Some("team-delivery"));
        let c = binding_with(&clone, &config).unwrap();
        assert!(c.is_bound());
        assert_eq!(c.get("deployment"), Some("https://kairos.example.com"));
    }

    /// `add_project` appends and keeps the comments; the same values again
    /// change nothing; other values are refused.
    #[test]
    fn add_project_appends_and_keeps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("kairos").join("config.toml");
        let values = Values {
            team_board: Some("team-delivery".into()),
            ..Default::default()
        };
        assert_eq!(
            add_project(&config, &remote("git@github.com:Acme/App.git"), &values).unwrap(),
            Added::Added
        );
        std::fs::write(
            &config,
            format!("# mine\n{}", std::fs::read_to_string(&config).unwrap()),
        )
        .unwrap();
        assert_eq!(
            add_project(&config, &remote("https://github.com/acme/app"), &values).unwrap(),
            Added::Same
        );
        let other = Values {
            team_board: Some("elsewhere".into()),
            ..Default::default()
        };
        let err = add_project(&config, &remote("github.com/acme/app"), &other)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("has a [[project]] for github.com/acme/app"),
            "{err}"
        );
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# mine\n"), "{text}");
        assert!(
            text.contains("remote = \"github.com/acme/app\"\nteam_board = \"team-delivery\"\n"),
            "{text}"
        );
        assert_eq!(load(&config).unwrap().projects.len(), 1);
    }
}
