//! `kairos self-update` and the daily version check (KAIROS-T-0432).
//!
//! The server tells its version at `GET /version` (open). The CLI compares
//! it with its own. `self-update` downloads the release tarball of this
//! platform, checks it, and swaps the binary.
//!
//! The trust rule (KAIROS-A-0080, which amends KAIROS-A-0058): the CLI
//! trusts the server that it uses. It downloads the tarball and its
//! `.sha256` from the links of `GET /version`, so an upgrade needs no
//! access to github.com. The checksum finds a damaged download. A link
//! that is a path resolves against the deployment. When the user sets
//! `KAIROS_RELEASES_URL`, the checksum comes from there, as before.
//! `--version` and `--latest` use the published release. A version older
//! than this CLI needs `--version` or `--allow-downgrade`.
//!
//! The swap writes the new binary to a new file next to the old one and
//! renames it over the old path. Writing over the old file in place gets
//! the binary killed at launch on macOS (exit 137): the kernel keeps the
//! code signature of that file. The old binary stays as `<name>.bak`.
//! A failed download or a bad checksum leaves the old binary in place.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{self, Source};
use crate::credentials;
use crate::error::{CliError, EXIT_FAILURE, EXIT_OUTDATED};

/// The version of this CLI.
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the release assets are. `KAIROS_RELEASES_URL` overrides it (tests,
/// a mirror).
const RELEASES_URL: &str = "https://github.com/colliery-io/kairos/releases";

/// The variable that replaces [`RELEASES_URL`].
const RELEASES_URL_VAR: &str = "KAIROS_RELEASES_URL";

/// The latest release, for `--latest`.
const LATEST_API: &str = "https://api.github.com/repos/colliery-io/kairos/releases/latest";

/// The variable that turns the daily check into an update without asking.
const AUTO_UPDATE_VAR: &str = "KAIROS_AUTO_UPDATE";

/// The variable that turns the daily check off (`0`).
const UPDATE_CHECK_VAR: &str = "KAIROS_UPDATE_CHECK";

/// The file of the daily check, in the config directory.
const CHECK_FILE: &str = "version-check.json";

/// One check a day at most.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// `kairos self-update`.
#[derive(clap::Args, Debug, Default)]
pub struct SelfUpdateArgs {
    /// Deployment base URL whose version to install (defaults to KAIROS_URL,
    /// then the only cached deployment)
    #[arg(long)]
    pub url: Option<String>,
    /// Install this version instead of the version of the server (e.g. 0.12.0). It can be older than this CLI
    #[arg(long, conflicts_with = "latest")]
    pub version: Option<String>,
    /// Install the latest release instead of the version of the server
    #[arg(long)]
    pub latest: bool,
    /// Only say which version the server has and which this CLI has. The exit
    /// code is 3 when the server is newer, else 0
    #[arg(long, conflicts_with_all = ["version", "latest", "allow_downgrade"])]
    pub check: bool,
    /// Install the version of the server, or of --latest, also when it is older than this CLI
    #[arg(long)]
    pub allow_downgrade: bool,
    /// Print one JSON object: the versions and the action that the command took or refused
    #[arg(long)]
    pub json: bool,
}

/// What `self-update` did, or refused to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// `--check`: the command changed nothing.
    Checked,
    /// The CLI has the version already.
    Unchanged,
    /// A newer version is in place.
    Updated,
    /// An older version is in place (`--version` or `--allow-downgrade`).
    Installed,
    /// The version is older than this CLI and no option allows it.
    Refused,
}

/// The result of `self-update`: the JSON object of `--json`. `message` is
/// the line that the command prints without `--json`.
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub cli_version: String,
    /// The version of the server, when the command asked the server.
    pub server_version: Option<String>,
    /// The deployment, when the command asked the server.
    pub deployment: Option<String>,
    /// The version that the command installs, or refuses to install.
    pub version: Option<String>,
    pub action: Action,
    pub message: String,
    /// The path of the binary, after a swap.
    pub binary: Option<String>,
    /// The path of the old binary, after a swap.
    pub backup: Option<String>,
}

impl Outcome {
    fn new(action: Action, message: String) -> Self {
        Self {
            cli_version: CLI_VERSION.to_string(),
            server_version: None,
            deployment: None,
            version: None,
            action,
            message,
            binary: None,
            backup: None,
        }
    }

    /// 3 for `--check` when the server is newer (KAIROS-T-0588), 1 for a
    /// refusal, else 0.
    fn exit_code(&self) -> ExitCode {
        let newer = || {
            self.server_version
                .as_deref()
                .is_some_and(|server| compare(CLI_VERSION, server).is_lt())
        };
        match self.action {
            Action::Checked if newer() => ExitCode::from(EXIT_OUTDATED),
            Action::Refused => ExitCode::from(EXIT_FAILURE),
            _ => ExitCode::SUCCESS,
        }
    }
}

impl SelfUpdateArgs {
    /// The exit code of the command: 0, 3 for `--check` when the server is
    /// newer than the CLI (KAIROS-T-0588), or 1 for a refused downgrade.
    /// Without `--json`, a refusal is an error line on stderr. With
    /// `--json`, each outcome is one object on stdout.
    pub async fn run(self) -> Result<ExitCode, CliError> {
        let http = http()?;
        let json = self.json;
        let outcome = self.outcome(&http).await?;
        if json {
            let text = serde_json::to_string_pretty(&outcome).map_err(|err| {
                CliError::Failure(format!("The CLI cannot write the JSON: {err}."))
            })?;
            println!("{text}");
        } else if outcome.action == Action::Refused {
            return Err(CliError::Failure(outcome.message));
        } else {
            println!("{}", outcome.message);
        }
        Ok(outcome.exit_code())
    }

    async fn outcome(self, http: &reqwest::Client) -> Result<Outcome, CliError> {
        if self.check {
            let deployment = deployment(self.url.as_deref())?;
            let server = server_version(http, &deployment).await?.version;
            let mut outcome = Outcome::new(Action::Checked, check_line(&deployment, &server));
            outcome.server_version = Some(server);
            outcome.deployment = Some(deployment);
            return Ok(outcome);
        }
        let explicit = self.version.is_some();
        let (release, asked) = match (self.version, self.latest) {
            (Some(version), _) => (Release::published(version.trim_start_matches('v'))?, None),
            (None, true) => (Release::published(&latest_version(http).await?)?, None),
            (None, false) => {
                let deployment = deployment(self.url.as_deref())?;
                let server = server_version(http, &deployment)
                    .await
                    .map_err(|err| match err {
                        CliError::Failure(message) => CliError::Failure(format!(
                            "{message} Name one with --version or use --latest."
                        )),
                        other => other,
                    })?;
                let version = server.version.clone();
                (server.release(&deployment)?, Some((deployment, version)))
            }
        };
        let mut outcome = match downgrade_refusal(&release.version, explicit, self.allow_downgrade)
        {
            Some(message) => Outcome::new(Action::Refused, message),
            None => update_to(http, &release).await?,
        };
        outcome.version = Some(release.version);
        if let Some((deployment, server)) = asked {
            outcome.deployment = Some(deployment);
            outcome.server_version = Some(server);
        }
        Ok(outcome)
    }
}

/// The refusal of a version older than this CLI, or None. `--version`
/// names the version, so it is not refused; `--allow-downgrade` allows the
/// version of the server or of `--latest` (KAIROS-T-0595).
fn downgrade_refusal(version: &str, explicit: bool, allowed: bool) -> Option<String> {
    if explicit || allowed || !compare(CLI_VERSION, version).is_gt() {
        return None;
    }
    Some(format!(
        "The version {version} is older than this CLI ({CLI_VERSION}). Nothing changed. \
         To install it, use --allow-downgrade or --version {version}."
    ))
}

/// The line of `--check` and its exit code: 3 when the server is newer than
/// this CLI, else 0 (KAIROS-T-0588).
fn check_line(deployment: &str, server: &str) -> String {
    if compare(CLI_VERSION, server).is_lt() {
        format!(
            "The CLI is {CLI_VERSION}; the server {deployment} is {server}. \
             Run `kairos self-update`."
        )
    } else {
        format!("The CLI is {CLI_VERSION}; the server {deployment} is {server}.")
    }
}

/// Download, check and install a release. The same version is no change.
/// `self-update` prints the message on stdout, the daily check on stderr,
/// so the output of a command stays what it is (KAIROS-T-0460).
async fn update_to(http: &reqwest::Client, release: &Release) -> Result<Outcome, CliError> {
    let version = &release.version;
    if version == CLI_VERSION {
        return Ok(Outcome::new(
            Action::Unchanged,
            format!("The CLI is {CLI_VERSION} already: nothing changed."),
        ));
    }
    let asset = release.url.rsplit('/').next().unwrap_or("the tarball");
    let archive = download(http, &release.url).await?;
    let checksum = download(http, &release.sha256_url).await?;
    let binary = verified_binary(&archive, &String::from_utf8_lossy(&checksum), asset)?;
    let exe = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|err| CliError::Failure(format!("The CLI cannot find its own path: {err}.")))?;
    let backup = install(&exe, &binary)?;
    let action = match compare(CLI_VERSION, version) {
        std::cmp::Ordering::Greater => Action::Installed,
        _ => Action::Updated,
    };
    let mut outcome = Outcome::new(action, installed_line(version, &exe, &backup));
    outcome.binary = Some(exe.display().to_string());
    outcome.backup = Some(backup.display().to_string());
    Ok(outcome)
}

/// The line after a swap. A version older than this CLI is installed, not
/// an update.
fn installed_line(version: &str, exe: &Path, backup: &Path) -> String {
    let what = match compare(CLI_VERSION, version) {
        std::cmp::Ordering::Greater => format!("Installed {version} in place of {CLI_VERSION}"),
        _ => format!("Updated the CLI from {CLI_VERSION} to {version}"),
    };
    format!(
        "{what}: {}. The old binary is {}.",
        exe.display(),
        backup.display()
    )
}

/// One tarball of the CLI to install, and its checksum.
#[derive(Debug, Clone, PartialEq)]
struct Release {
    version: String,
    url: String,
    sha256_url: String,
}

impl Release {
    /// The tarball of `version` for this platform on the published releases
    /// (GitHub, or `KAIROS_RELEASES_URL`). The version must look like a
    /// release version: it goes into the path of the URL of the checksum.
    fn published(version: &str) -> Result<Self, CliError> {
        if !is_release_version(version) {
            return Err(CliError::Failure(format!(
                "`{version}` is not a release version (e.g. 0.12.0). Nothing changed."
            )));
        }
        let base = std::env::var(RELEASES_URL_VAR)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| RELEASES_URL.to_string());
        let target = target_triple()?;
        let url = format!(
            "{}/download/v{version}/kairos-{version}-{target}.tar.gz",
            base.trim_end_matches('/')
        );
        Ok(Self {
            version: version.to_string(),
            sha256_url: format!("{url}.sha256"),
            url,
        })
    }
}

/// A version of the form `1.2.3`, with an optional `-pre.1` part. Only
/// digits, letters, `.` and `-`: a `/` or `..` cannot move the URL of the
/// checksum to another place.
fn is_release_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    let core_ok = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    let pre_ok = pre.is_none_or(|pre| {
        pre.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    });
    core_ok && pre_ok
}

/// The body of `GET /version`: the version of the server and the tarballs
/// of the CLI of that version, each with the link of its `.sha256`.
#[derive(Debug, Deserialize)]
struct ServerVersion {
    version: String,
    #[serde(default)]
    cli: Vec<CliDownload>,
}

#[derive(Debug, Deserialize)]
struct CliDownload {
    target: String,
    url: String,
    #[serde(default)]
    sha256_url: Option<String>,
}

/// True when the user names the release source (`KAIROS_RELEASES_URL`):
/// the checksum then comes from there, not from the server.
fn own_release_source() -> bool {
    std::env::var(RELEASES_URL_VAR).is_ok_and(|value| !value.is_empty())
}

/// A link of `GET /version` as a URL: a path resolves against the
/// deployment (a server with no `KAIROS_PUBLIC_URL`, KAIROS-T-0745).
fn resolve(deployment: &str, link: &str) -> Result<String, CliError> {
    let base = reqwest::Url::parse(deployment).map_err(|err| {
        CliError::Failure(format!("The deployment {deployment} is not a URL: {err}."))
    })?;
    base.join(link).map(String::from).map_err(|err| {
        CliError::Failure(format!(
            "The server sent the link `{link}`, not a URL: {err}."
        ))
    })
}

impl ServerVersion {
    /// The tarball for this platform that the server names, and its
    /// checksum (KAIROS-A-0080).
    fn release(self, deployment: &str) -> Result<Release, CliError> {
        self.release_with(deployment, own_release_source())
    }

    /// The tarball and the checksum that the server names. With
    /// `own_source` (`KAIROS_RELEASES_URL`), the checksum of that release
    /// source. A server with no links (an older one) gives the published
    /// release of its version. A server whose links have no tarball for
    /// this platform is refused, and the message names the platforms it
    /// lists (KAIROS-T-0460): a mirror that lacks one is a gap to fix, not
    /// a reason to go to GitHub.
    fn release_with(self, deployment: &str, own_source: bool) -> Result<Release, CliError> {
        let target = target_triple()?;
        let published = Release::published(&self.version)?;
        if self.cli.is_empty() {
            return Ok(published);
        }
        let listed: Vec<String> = self.cli.iter().map(|d| d.target.clone()).collect();
        match self
            .cli
            .into_iter()
            .find(|download| download.target == target)
        {
            Some(download) => {
                let url = resolve(deployment, &download.url)?;
                let sha256_url = match (own_source, download.sha256_url) {
                    (true, _) => published.sha256_url,
                    (false, Some(link)) => resolve(deployment, &link)?,
                    (false, None) => format!("{url}.sha256"),
                };
                Ok(Release {
                    version: published.version,
                    url,
                    sha256_url,
                })
            }
            None => Err(CliError::Failure(format!(
                "The server lists the CLI {} for {}, not for this platform ({target}). \
                 Use --version {} or --latest for the published release.",
                self.version,
                listed.join(", "),
                self.version
            ))),
        }
    }
}

/// The daily check, run after a command that succeeded. It asks the
/// deployment of the command (else the default deployment) for its
/// version, at most once a day for each deployment. When the server is
/// newer, it prints one line on stderr. With `KAIROS_AUTO_UPDATE=1` it
/// updates instead, but not from an `http://` deployment. It never fails
/// the command: a check that cannot run says nothing.
pub async fn daily_check() {
    // KAIROS-T-0460: a script or CI turns the check off.
    if std::env::var(UPDATE_CHECK_VAR).is_ok_and(|value| value.trim() == "0") {
        return;
    }
    let Ok(dir) = credentials::config_dir() else {
        return;
    };
    let Some(deployment) = check_deployment(auth::deployment_in_use(), || deployment(None).ok())
    else {
        return;
    };
    let auto = std::env::var(AUTO_UPDATE_VAR).is_ok_and(|value| value == "1");
    if let Some(line) = check_once(&dir, &deployment, credentials::unix_now(), auto).await {
        // A closed stderr is no reason to fail the command.
        let _ = writeln!(std::io::stderr(), "{line}");
    }
}

/// The deployment of the daily check: the one that the command used
/// (`--url`, `KAIROS_URL`, the checkout settings or the cache), else the
/// default deployment (KAIROS-T-0595).
fn check_deployment(
    in_use: Option<&str>,
    default: impl FnOnce() -> Option<String>,
) -> Option<String> {
    in_use.map(str::to_string).or_else(default)
}

/// One daily check of `deployment`: the line to print, or None. Only a
/// server that answers is recorded: an unreachable server is asked again by
/// the next command, so its warning is not muted for a day.
async fn check_once(dir: &Path, deployment: &str, now: u64, auto: bool) -> Option<String> {
    let path = dir.join(CHECK_FILE);
    let mut state = CheckState::load(&path);
    if state.is_recent(deployment, now) {
        return None;
    }
    let probe = http_with_timeout(Duration::from_secs(2)).ok()?;
    let server = server_version(&probe, deployment).await.ok()?;
    let version = server.version.clone();
    state.record(deployment, now, &version);
    state.save(dir, &path);
    if !compare(CLI_VERSION, &version).is_lt() {
        return None;
    }
    if !auto {
        return Some(newer_line(deployment, &version));
    }
    if is_plain_http(deployment) {
        return Some(format!(
            "{} {AUTO_UPDATE_VAR} does not update from an http:// deployment.",
            newer_line(deployment, &version)
        ));
    }
    let result = match (http(), server.release(deployment)) {
        (Ok(http), Ok(release)) => update_to(&http, &release).await,
        (Err(err), _) | (_, Err(err)) => Err(err),
    };
    Some(match result {
        Ok(outcome) => outcome.message,
        Err(err) => format!("warning: the CLI could not update itself to {version}: {err}"),
    })
}

/// The warning of the daily check. It names `--check` and the update.
fn newer_line(deployment: &str, server: &str) -> String {
    format!(
        "warning: the CLI is {CLI_VERSION} and the server {deployment} is {server}. \
         Run `kairos self-update` to install {server}, or `kairos self-update --check` \
         to compare the versions."
    )
}

/// The deployment has the scheme `http`: no TLS.
fn is_plain_http(deployment: &str) -> bool {
    deployment
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
}

/// The line for a code tools server older than its deployment
/// (KAIROS-T-0526), or None: no deployment, no answer in 2 s, or the
/// CLI is not older. The MCP server `kairos-code` is this binary, started
/// once per session: a `kairos self-update` does not change the running
/// server, so the line says to restart the session.
pub async fn behind_server_line() -> Option<String> {
    let deployment = deployment(None).ok()?;
    let probe = http_with_timeout(Duration::from_secs(2)).ok()?;
    let server = server_version(&probe, &deployment).await.ok()?;
    behind_line(CLI_VERSION, &server.version)
}

/// The text of [`behind_server_line`] (pure; testable).
fn behind_line(cli: &str, server: &str) -> Option<String> {
    if server.is_empty() || compare(cli, server) != std::cmp::Ordering::Less {
        return None;
    }
    Some(format!(
        "The kairos CLI is {cli} and the server is {server}. `kairos-code` runs this old \
         binary: after `kairos self-update`, restart the session."
    ))
}

/// What the daily check keeps between runs: the last answer of each
/// deployment. A file of an older CLI reads as no state.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CheckState {
    #[serde(default)]
    deployments: BTreeMap<String, Checked>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Checked {
    checked_at: u64,
    server_version: String,
}

impl CheckState {
    fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn is_recent(&self, deployment: &str, now: u64) -> bool {
        self.deployments
            .get(deployment)
            .is_some_and(|checked| now.saturating_sub(checked.checked_at) < CHECK_INTERVAL_SECS)
    }

    fn record(&mut self, deployment: &str, now: u64, server_version: &str) {
        self.deployments.insert(
            deployment.to_string(),
            Checked {
                checked_at: now,
                server_version: server_version.to_string(),
            },
        );
    }

    fn save(&self, dir: &Path, path: &Path) {
        if let Ok(text) = serde_json::to_string(self) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(path, text);
        }
    }
}

fn deployment(url: Option<&str>) -> Result<String, CliError> {
    let path = credentials::credentials_path()?;
    let source = auth::resolve(url, &auth::Env::from_process(), || credentials::load(&path))?;
    Ok(match source {
        Source::Key { deployment, .. } | Source::Cache { deployment, .. } => deployment,
    })
}

fn http() -> Result<reqwest::Client, CliError> {
    http_with_timeout(Duration::from_secs(300))
}

fn http_with_timeout(timeout: Duration) -> Result<reqwest::Client, CliError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("kairos-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| CliError::Failure(format!("The CLI cannot make an HTTP client: {err}.")))
}

async fn server_version(
    http: &reqwest::Client,
    deployment: &str,
) -> Result<ServerVersion, CliError> {
    let url = format!("{}/version", deployment.trim_end_matches('/'));
    let response = http
        .get(&url)
        .send()
        .await
        .map_err(|err| CliError::Failure(format!("The CLI cannot reach {url}: {err}.")))?;
    if !response.status().is_success() {
        return Err(CliError::Failure(format!(
            "{url} answered {}. An older server does not tell its version.",
            response.status()
        )));
    }
    let body: ServerVersion = response.json().await.map_err(|_| {
        CliError::Failure(format!(
            "{url} sent no version. An older server does not tell its version."
        ))
    })?;
    Ok(body)
}

async fn latest_version(http: &reqwest::Client) -> Result<String, CliError> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let response = http
        .get(LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| CliError::Failure(format!("The CLI cannot reach GitHub: {err}.")))?;
    if !response.status().is_success() {
        return Err(CliError::Failure(format!(
            "GitHub answered {} for the latest release.",
            response.status()
        )));
    }
    let release: Release = response
        .json()
        .await
        .map_err(|err| CliError::Failure(format!("GitHub sent no release: {err}.")))?;
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, CliError> {
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|err| CliError::Failure(format!("The CLI cannot download {url}: {err}.")))?;
    if !response.status().is_success() {
        return Err(CliError::Failure(format!(
            "The download of {url} answered {}. Nothing changed.",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| CliError::Failure(format!("The download of {url} failed: {err}.")))?;
    Ok(bytes.to_vec())
}

/// The release target of this build, as the release assets name it.
fn target_triple() -> Result<&'static str, CliError> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Ok("aarch64-apple-darwin"),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin"),
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Ok("aarch64-unknown-linux-gnu"),
        (arch, os) => Err(CliError::Failure(format!(
            "No release of the CLI is built for {arch} {os}."
        ))),
    }
}

/// Compare two versions `a.b.c` by number. A part that is not a number
/// counts as 0.
fn compare(left: &str, right: &str) -> std::cmp::Ordering {
    let parts = |version: &str| -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(left).cmp(&parts(right))
}

/// Check the archive against the `.sha256` text (`<hex>  <name>`, as
/// `shasum -a 256` writes it) and take the `kairos` binary out of it.
fn verified_binary(archive: &[u8], checksum: &str, asset: &str) -> Result<Vec<u8>, CliError> {
    let expected = checksum
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let actual: String = Sha256::digest(archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if expected != actual {
        return Err(CliError::Failure(format!(
            "The checksum of {asset} does not match: expected {expected}, got {actual}. \
             Nothing changed."
        )));
    }
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let entries = tar
        .entries()
        .map_err(|err| CliError::Failure(format!("{asset} is not a tarball: {err}.")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|err| CliError::Failure(format!("{asset} is damaged: {err}.")))?;
        let is_binary = entry
            .path()
            .ok()
            .and_then(|path| path.file_name().map(|name| name == "kairos"))
            .unwrap_or(false);
        if is_binary {
            let mut binary = Vec::new();
            entry
                .read_to_end(&mut binary)
                .map_err(|err| CliError::Failure(format!("{asset} is damaged: {err}.")))?;
            return Ok(binary);
        }
    }
    Err(CliError::Failure(format!(
        "{asset} holds no `kairos` binary. Nothing changed."
    )))
}

/// Put `binary` at `exe`: write a new file next to it, keep the old one as
/// `<exe>.bak`, and rename the new file over the path. Returns the backup.
fn install(exe: &Path, binary: &[u8]) -> Result<PathBuf, CliError> {
    let failure = |what: &str, err: std::io::Error| {
        CliError::Failure(format!("The CLI cannot {what}: {err}. Nothing changed."))
    };
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "kairos".to_string());
    let staged = exe.with_file_name(format!(".{name}.new-{}", std::process::id()));
    let backup = exe.with_file_name(format!("{name}.bak"));
    std::fs::write(&staged, binary).map_err(|err| failure("write the new binary", err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        {
            let _ = std::fs::remove_file(&staged);
            return Err(failure("make the new binary executable", err));
        }
    }
    if let Err(err) = std::fs::copy(exe, &backup) {
        let _ = std::fs::remove_file(&staged);
        return Err(failure("keep a backup of the old binary", err));
    }
    if let Err(err) = std::fs::rename(&staged, exe) {
        let _ = std::fs::remove_file(&staged);
        return Err(failure("put the new binary in place", err));
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A local server that answers each request with `status` and `body`,
    /// and counts the requests.
    fn stub(status: u16, body: impl Into<String>) -> (String, Arc<AtomicUsize>) {
        let body: String = body.into();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counter.fetch_add(1, SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (url, hits)
    }

    /// A URL where nothing listens.
    fn dead_url() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        url
    }

    /// KAIROS-T-0588: `--check` exits 3 when the server is newer, 0 when
    /// the CLI is the same version or newer.
    #[test]
    fn check_exits_3_when_the_server_is_newer() {
        let line = check_line("https://k.test", "999.0.0");
        assert!(line.contains("Run `kairos self-update`"), "{line}");
        let line = check_line("https://k.test", CLI_VERSION);
        assert!(!line.contains("self-update"), "{line}");

        let checked = |server: &str| {
            let mut outcome = Outcome::new(Action::Checked, String::new());
            outcome.server_version = Some(server.to_string());
            outcome.exit_code()
        };
        assert_eq!(checked("999.0.0"), ExitCode::from(EXIT_OUTDATED));
        assert_eq!(checked(CLI_VERSION), ExitCode::SUCCESS);
        assert_eq!(checked("0.0.1"), ExitCode::SUCCESS);
    }

    /// KAIROS-T-0595: an older version needs `--version` or
    /// `--allow-downgrade`, and the refusal names both.
    #[test]
    fn a_downgrade_needs_version_or_allow_downgrade() {
        let refusal = downgrade_refusal("0.0.1", false, false).expect("refused");
        assert!(refusal.contains("--allow-downgrade"), "{refusal}");
        assert!(refusal.contains("--version 0.0.1"), "{refusal}");
        assert!(refusal.contains("Nothing changed"), "{refusal}");
        assert_eq!(downgrade_refusal("0.0.1", true, false), None);
        assert_eq!(downgrade_refusal("0.0.1", false, true), None);
        assert_eq!(downgrade_refusal("999.0.0", false, false), None);
        assert_eq!(downgrade_refusal(CLI_VERSION, false, false), None);
        let refused = Outcome::new(Action::Refused, refusal);
        assert_eq!(refused.exit_code(), ExitCode::from(EXIT_FAILURE));
    }

    /// KAIROS-T-0595: `--json` is one object with the versions and the
    /// action.
    #[test]
    fn the_json_object_names_the_versions_and_the_action() {
        let mut outcome = Outcome::new(Action::Refused, "no".into());
        outcome.server_version = Some("0.0.1".into());
        outcome.version = Some("0.0.1".into());
        outcome.deployment = Some("https://k.test".into());
        let value = serde_json::to_value(&outcome).unwrap();
        assert_eq!(value["cli_version"], CLI_VERSION);
        assert_eq!(value["server_version"], "0.0.1");
        assert_eq!(value["version"], "0.0.1");
        assert_eq!(value["deployment"], "https://k.test");
        assert_eq!(value["action"], "refused");
        assert_eq!(value["message"], "no");
        assert!(value["binary"].is_null() && value["backup"].is_null());
        for (action, name) in [
            (Action::Checked, "checked"),
            (Action::Unchanged, "unchanged"),
            (Action::Updated, "updated"),
            (Action::Installed, "installed"),
        ] {
            assert_eq!(serde_json::to_value(action).unwrap(), name);
        }
    }

    #[test]
    fn behind_line_names_the_restart() {
        let line = behind_line("0.13.0", "0.15.0").expect("older");
        assert!(line.contains("0.13.0") && line.contains("0.15.0"), "{line}");
        assert!(line.contains("restart the session"), "{line}");
        assert_eq!(behind_line("0.15.0", "0.15.0"), None);
        assert_eq!(behind_line("0.16.0", "0.15.0"), None);
        assert_eq!(behind_line("0.13.0", ""), None);
    }

    #[test]
    fn versions_compare_by_number() {
        use std::cmp::Ordering::*;
        assert_eq!(compare("0.9.0", "0.12.0"), Less);
        assert_eq!(compare("0.12.0", "0.12.0"), Equal);
        assert_eq!(compare("v0.12.1", "0.12.0"), Greater);
        assert_eq!(compare("1.0.0", "0.99.99"), Greater);
    }

    /// KAIROS-T-0595: the version goes into the URL of the checksum, so a
    /// server cannot move that URL with `/` or `..`.
    #[test]
    fn only_a_release_version_makes_a_url() {
        for good in ["0.12.0", "1.2.3", "0.13.0-rc.1", "10.0.0-beta-2"] {
            assert!(is_release_version(good), "{good}");
        }
        for bad in [
            "",
            "0.12",
            "0.12.0.1",
            "v0.12.0",
            "0.12.x",
            "0.12.0-",
            "0.12.0-rc..1",
            "0.12.0/../../../evil/repo/releases/download/v1",
            "../0.12.0",
            "0.12.0?x=1",
        ] {
            assert!(!is_release_version(bad), "{bad}");
        }
        let err = Release::published("1.0.0/../../x").unwrap_err().to_string();
        assert!(err.contains("not a release version"), "{err}");
    }

    #[test]
    fn a_good_archive_gives_the_binary() {
        let archive = tarball(&[
            ("./LICENSE", b"license"),
            ("./kairos", b"new binary"),
            ("./NOTICE", b"notice"),
        ]);
        let checksum = format!("{}  kairos-0.13.0-x.tar.gz\n", sha(&archive));
        let binary = verified_binary(&archive, &checksum, "kairos-0.13.0-x.tar.gz").unwrap();
        assert_eq!(binary, b"new binary");
    }

    #[test]
    fn a_bad_checksum_is_refused() {
        let archive = tarball(&[("./kairos", b"new binary")]);
        let err = verified_binary(&archive, &"0".repeat(64), "a.tar.gz").unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[test]
    fn an_archive_with_no_binary_is_refused() {
        let archive = tarball(&[("./LICENSE", b"license")]);
        let err = verified_binary(&archive, &sha(&archive), "a.tar.gz").unwrap_err();
        assert!(err.to_string().contains("no `kairos` binary"), "{err}");
    }

    #[test]
    fn install_renames_the_new_binary_over_the_old_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("kairos");
        std::fs::write(&exe, b"old").unwrap();
        let backup = install(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        assert_eq!(backup, dir.path().join("kairos.bak"));
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "no staged file is left: {left:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    /// KAIROS-A-0080: the server names the tarball and the checksum.
    #[test]
    fn the_server_names_the_tarball_and_the_checksum() {
        let target = target_triple().unwrap();
        let server: ServerVersion = serde_json::from_value(serde_json::json!({
            "version": "9.9.9",
            "cli": [
                {"target": target, "url": "https://mirror/k.tar.gz", "sha256_url": "https://mirror/k.tar.gz.sha256"},
            ],
        }))
        .unwrap();
        let release = server
            .release_with("https://kairos.example.com", false)
            .unwrap();
        assert_eq!(release.url, "https://mirror/k.tar.gz");
        assert_eq!(release.sha256_url, "https://mirror/k.tar.gz.sha256");
    }

    /// KAIROS-T-0745: a server with no public URL sends paths; they
    /// resolve against the deployment.
    #[test]
    fn a_path_resolves_against_the_deployment() {
        let target = target_triple().unwrap();
        let file = format!("kairos-9.9.9-{target}.tar.gz");
        let server: ServerVersion = serde_json::from_value(serde_json::json!({
            "version": "9.9.9",
            "cli": [
                {"target": target, "url": format!("/cli/{file}"), "sha256_url": format!("/cli/{file}.sha256")},
            ],
        }))
        .unwrap();
        let release = server
            .release_with("https://kairos.example.com", false)
            .unwrap();
        assert_eq!(
            release.url,
            format!("https://kairos.example.com/cli/{file}")
        );
        assert_eq!(
            release.sha256_url,
            format!("https://kairos.example.com/cli/{file}.sha256")
        );
    }

    /// KAIROS-T-0595: with `KAIROS_RELEASES_URL`, the server can name a
    /// mirror for the tarball, but the checksum is the one of that release
    /// source, not the server's.
    #[test]
    fn with_an_own_release_source_the_server_names_the_tarball_only() {
        let target = target_triple().unwrap();
        let server: ServerVersion = serde_json::from_value(serde_json::json!({
            "version": "9.9.9",
            "cli": [
                {"target": "other", "url": "https://x/other", "sha256_url": "https://x/other.sha256"},
                {"target": target, "url": "https://mirror/k.tar.gz", "sha256_url": "https://mirror/k.tar.gz.sha256"},
            ],
        }))
        .unwrap();
        let release = server
            .release_with("https://kairos.example.com", true)
            .unwrap();
        let published = Release::published("9.9.9").unwrap();
        assert_eq!(release.url, "https://mirror/k.tar.gz");
        assert_eq!(release.sha256_url, published.sha256_url);
        assert!(
            release.sha256_url.ends_with(&format!(
                "/download/v9.9.9/kairos-9.9.9-{target}.tar.gz.sha256"
            )),
            "{release:?}"
        );
        assert!(!release.sha256_url.contains("mirror"), "{release:?}");
    }

    /// A server that sends a version that is not a release version is
    /// refused before any download.
    #[test]
    fn a_server_with_a_bad_version_is_refused() {
        let server: ServerVersion =
            serde_json::from_value(serde_json::json!({"version": "1.0.0/../../x"})).unwrap();
        let err = server
            .release_with("https://kairos.example.com", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a release version"), "{err}");
    }

    #[test]
    fn a_server_with_no_links_gives_the_published_release() {
        let server: ServerVersion =
            serde_json::from_value(serde_json::json!({"version": "9.9.9"})).unwrap();
        let release = server
            .release_with("https://kairos.example.com", false)
            .unwrap();
        assert!(
            release.url.contains("/download/v9.9.9/kairos-9.9.9-"),
            "{release:?}"
        );
        assert_eq!(release.sha256_url, format!("{}.sha256", release.url));
    }

    #[test]
    fn a_server_that_lists_other_platforms_only_is_refused() {
        // KAIROS-T-0460: a mirror with a gap, not a silent trip to GitHub.
        let server: ServerVersion = serde_json::from_value(serde_json::json!({
            "version": "9.9.9",
            "cli": [
                {"target": "a-b-c", "url": "https://x/a", "sha256_url": "https://x/a.sha256"},
                {"target": "d-e-f", "url": "https://x/d", "sha256_url": "https://x/d.sha256"},
            ],
        }))
        .unwrap();
        let err = server
            .release_with("https://kairos.example.com", false)
            .unwrap_err()
            .to_string();
        let target = target_triple().unwrap();
        assert!(err.contains("a-b-c, d-e-f"), "{err}");
        assert!(err.contains(target), "{err}");
        assert!(err.contains("--version 9.9.9"), "{err}");
    }

    #[test]
    fn a_downgrade_says_installed_and_an_upgrade_says_updated() {
        let exe = Path::new("/usr/local/bin/kairos");
        let backup = Path::new("/usr/local/bin/kairos.bak");
        let newer = installed_line("999.0.0", exe, backup);
        let updated = format!("Updated the CLI from {CLI_VERSION} to 999.0.0");
        assert!(newer.starts_with(updated.as_str()), "{newer}");
        let older = installed_line("0.0.1", exe, backup);
        let installed = format!("Installed 0.0.1 in place of {CLI_VERSION}");
        assert!(older.starts_with(installed.as_str()), "{older}");
        assert!(
            older.ends_with("The old binary is /usr/local/bin/kairos.bak."),
            "{older}"
        );
    }

    #[test]
    fn this_platform_has_a_release_target() {
        assert!(target_triple().is_ok());
    }

    /// KAIROS-T-0595: the daily check asks the deployment of the command;
    /// the default deployment only when the command used none.
    #[test]
    fn the_check_uses_the_deployment_of_the_command() {
        let default = || Some("https://default.test".to_string());
        assert_eq!(
            check_deployment(Some("https://command.test"), default).as_deref(),
            Some("https://command.test")
        );
        assert_eq!(
            check_deployment(None, default).as_deref(),
            Some("https://default.test")
        );
        assert_eq!(check_deployment(None, || None), None);
    }

    #[test]
    fn plain_http_is_found() {
        assert!(is_plain_http("http://k.test"));
        assert!(is_plain_http("HTTP://k.test"));
        assert!(!is_plain_http("https://k.test"));
        assert!(!is_plain_http("http"));
    }

    /// KAIROS-T-0595: a server that does not answer is not recorded, so the
    /// next command asks it again.
    #[tokio::test]
    async fn an_unreachable_server_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let dead = dead_url();
        assert_eq!(check_once(dir.path(), &dead, 1_000, false).await, None);
        assert!(!dir.path().join(CHECK_FILE).exists());

        // A server that answers with no version is not recorded either.
        let (broken, hits) = stub(503, "{}");
        assert_eq!(check_once(dir.path(), &broken, 1_000, false).await, None);
        assert_eq!(check_once(dir.path(), &broken, 1_001, false).await, None);
        assert_eq!(hits.load(SeqCst), 2, "asked again");
        assert!(!dir.path().join(CHECK_FILE).exists());
    }

    /// KAIROS-T-0595: the warning names the deployment and `--check`; an
    /// answer is recorded for a day, for that deployment only.
    #[tokio::test]
    async fn a_newer_server_gives_one_warning_a_day_for_each_deployment() {
        let dir = tempfile::tempdir().unwrap();
        let (newer, hits) = stub(200, r#"{"version": "999.0.0"}"#);
        let line = check_once(dir.path(), &newer, 1_000, false)
            .await
            .expect("a warning");
        assert!(line.starts_with("warning:"), "{line}");
        assert!(line.contains(&newer), "{line}");
        assert!(line.contains("999.0.0"), "{line}");
        assert!(line.contains("`kairos self-update --check`"), "{line}");
        assert!(line.contains("`kairos self-update`"), "{line}");

        // Recorded: no second request within a day.
        assert_eq!(check_once(dir.path(), &newer, 2_000, false).await, None);
        assert_eq!(hits.load(SeqCst), 1);
        let state = CheckState::load(&dir.path().join(CHECK_FILE));
        assert_eq!(state.deployments[&newer].server_version, "999.0.0");

        // Another deployment has a check of its own.
        let (same, same_hits) = stub(200, format!(r#"{{"version": "{CLI_VERSION}"}}"#));
        assert_eq!(check_once(dir.path(), &same, 2_000, false).await, None);
        assert_eq!(same_hits.load(SeqCst), 1);
        let state = CheckState::load(&dir.path().join(CHECK_FILE));
        assert_eq!(state.deployments.len(), 2);

        // A day later the first deployment is asked again.
        let later = 1_000 + CHECK_INTERVAL_SECS;
        assert!(check_once(dir.path(), &newer, later, false).await.is_some());
        assert_eq!(hits.load(SeqCst), 2);
    }

    /// The state file of an older CLI reads as no state.
    #[test]
    fn an_old_state_file_reads_as_no_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECK_FILE);
        std::fs::write(
            &path,
            r#"{"deployment":"https://k.test","checked_at":5,"server_version":"0.1.0"}"#,
        )
        .unwrap();
        assert!(!CheckState::load(&path).is_recent("https://k.test", 6));
    }

    /// KAIROS-T-0595: `KAIROS_AUTO_UPDATE` does not install from an
    /// `http://` deployment: the check asks for the version only, and the
    /// line says why it did not update.
    #[tokio::test]
    async fn auto_update_refuses_an_http_deployment() {
        let dir = tempfile::tempdir().unwrap();
        let (newer, hits) = stub(200, r#"{"version": "999.0.0"}"#);
        let line = check_once(dir.path(), &newer, 1_000, true)
            .await
            .expect("a line");
        assert!(line.contains("KAIROS_AUTO_UPDATE"), "{line}");
        assert!(line.contains("http://"), "{line}");
        assert!(line.contains("--check"), "{line}");
        assert_eq!(hits.load(SeqCst), 1, "no download");
    }
}
