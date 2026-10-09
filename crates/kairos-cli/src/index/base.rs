//! The base index in Kairos, for `kairos index update` (COLLIERY-T-1854,
//! COLLIERY-I-0264 "The flow").
//!
//! - **The connection.** The URL: `--url`, else `KAIROS_URL`, else
//!   `deployment_url` in `.claude/kairos.local.md` of the checkout, else the
//!   credential cache of `kairos login`. The token: `KAIROS_KEY`, else
//!   `KAIROS_MCP_KEY` (a service-account key, as the plugin and
//!   `scripts/render-references.sh` use), else the credential cache.
//! - **The repository.** `--repository`, else `repository` in
//!   `.claude/kairos.local.md`, else the repository of Kairos whose
//!   `repo_url` is the `origin` remote of the checkout.
//! - **The branch point.** `git merge-base HEAD origin/<default branch>`
//!   (then the local default branch), else HEAD. Kairos gives the nearest
//!   indexed commit at or below it.
//! - **The changed files.** `git diff --name-only <base>` (the changes that
//!   are not committed too) and the files that git does not track and does
//!   not ignore.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kairos_client::{Error as ApiError, KairosClient};

use super::git_output;
use crate::credentials;
use crate::provider::CachedTokenProvider;

/// The settings file of the plugin in a checkout (KAIROS-A-0014).
const SETTINGS_FILE: &str = ".claude/kairos.local.md";

/// How long a short request to Kairos can take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the download of an index can take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// The flags that choose the Kairos deployment and the repository.
#[derive(clap::Args, Debug, Default)]
pub struct RemoteArgs {
    /// The Kairos deployment. Default: KAIROS_URL, else `deployment_url` in .claude/kairos.local.md, else the only deployment of `kairos login`
    #[arg(long)]
    pub url: Option<String>,
    /// The tenant, when the token is from `kairos login`. Default: the tenant of the login
    #[arg(long)]
    pub tenant: Option<String>,
    /// The repository in Kairos. Default: `repository` in .claude/kairos.local.md, else the repository of the `origin` remote
    #[arg(long)]
    pub repository: Option<String>,
}

/// The base index that Kairos has for the checkout.
pub struct Base {
    pub client: KairosClient,
    pub repository: String,
    /// The default branch of the repository.
    pub branch: String,
    /// The indexed commit.
    pub commit: String,
    /// The files of the checkout that differ from the commit.
    pub changed: usize,
    /// The summaries that the index uses: a full build makes about as many.
    pub summaries: usize,
    /// The repository makes its summaries on Kairos, with a hosted
    /// provider (KAIROS-T-0343): the CLI links the pool and makes none.
    pub hosted: bool,
}

impl Base {
    pub fn short(&self) -> &str {
        short(&self.commit)
    }

    /// Download the index file of the base commit.
    pub async fn download(&self) -> Result<Vec<u8>, String> {
        match tokio::time::timeout(
            DOWNLOAD_TIMEOUT,
            self.client
                .download_code_index(&self.repository, &self.commit),
        )
        .await
        {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!(
                "the download did not end in {} s",
                DOWNLOAD_TIMEOUT.as_secs()
            )),
        }
    }
}

/// What Kairos gave for the checkout.
pub enum Found {
    Base(Base),
    /// No deployment is set: no URL and no `kairos login`.
    NotSet,
    /// The CLI could not connect to the deployment at the URL.
    Unreachable {
        url: String,
        why: String,
    },
    /// Kairos answered, but it has no base index for the checkout. The text
    /// says why.
    NoBase(String),
}

pub fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

/// Ask Kairos for the nearest base index of the checkout at `root`.
pub async fn find(root: &Path, args: &RemoteArgs) -> Found {
    let settings = settings(root);
    let (client, url) = match client(args, &settings) {
        Ok(Some(found)) => found,
        Ok(None) => return Found::NotSet,
        Err(why) => return Found::NoBase(why),
    };
    let unreachable = |why: String| Found::Unreachable {
        url: url.clone(),
        why,
    };

    let repositories = match within(client.list_repositories(None)).await {
        Ok(list) => list,
        Err(Failure::Unreachable(why)) => return unreachable(why),
        Err(Failure::Refused(why)) => {
            return Found::NoBase(format!("Kairos refused the request ({why})"));
        }
    };
    let wanted = args
        .repository
        .clone()
        .or_else(|| settings.get("repository").cloned());
    let repository = match &wanted {
        Some(slug) => repositories
            .iter()
            .find(|r| &r.slug == slug || &r.id == slug),
        None => git_output(root, &["remote", "get-url", "origin"]).and_then(|origin| {
            let origin = normalized_remote(&origin);
            repositories
                .iter()
                .find(|r| normalized_remote(&r.repo_url) == origin)
        }),
    };
    let Some(repository) = repository else {
        return Found::NoBase(match wanted {
            Some(slug) => format!("Kairos has no repository {slug:?}"),
            None => "no repository in Kairos has the `origin` remote of this checkout. \
                     Give `--repository`"
                .to_string(),
        });
    };

    let point = branch_point(root, &repository.default_branch);
    let Some(point) = point else {
        return Found::NoBase("the checkout has no git commit".to_string());
    };
    let nearest = match within(client.nearest_code_index(&repository.slug, &point)).await {
        Ok(nearest) => nearest,
        Err(Failure::Unreachable(why)) => return unreachable(why),
        Err(Failure::Refused(why)) => {
            return Found::NoBase(format!(
                "Kairos has no index at or below commit {} ({why})",
                short(&point)
            ));
        }
    };
    let Some(changed) = changed_since(root, &nearest.index.commit) else {
        return Found::NoBase(format!(
            "the checkout does not have commit {}",
            short(&nearest.index.commit)
        ));
    };
    Found::Base(Base {
        client,
        repository: repository.slug.clone(),
        branch: repository.default_branch.clone(),
        commit: nearest.index.commit,
        changed,
        summaries: usize::try_from(nearest.index.summary_keys).unwrap_or(0),
        // KAIROS-T-0358: the value that the server resolved (a repository
        // can follow the organization). A server from before it sends no
        // resolved value, so its own `hosted` counts too.
        hosted: [
            &repository.code_index_summaries_resolved,
            &repository.code_index_summaries,
        ]
        .contains(&&kairos_client::types_repositories::CodeIndexSummaries::Hosted),
    })
}

/// The client of the Kairos deployment of the checkout at `root`: the
/// connection of [`find`], with no flags (KAIROS-T-0360). `None` when no
/// deployment is set or the credentials do not resolve. It sends no
/// request.
pub fn connection(root: &Path) -> Option<KairosClient> {
    client(&RemoteArgs::default(), &settings(root))
        .ok()
        .flatten()
        .map(|(client, _)| client)
}

enum Failure {
    Unreachable(String),
    Refused(String),
}

/// A request with [`REQUEST_TIMEOUT`]. A transport error or a timeout is
/// `Unreachable`; an answer of the server that is an error is `Refused`.
async fn within<T>(
    request: impl std::future::Future<Output = Result<T, ApiError>>,
) -> Result<T, Failure> {
    match tokio::time::timeout(REQUEST_TIMEOUT, request).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(ApiError::Transport(e))) => Err(Failure::Unreachable(e.to_string())),
        Ok(Err(e)) => Err(Failure::Refused(e.to_string())),
        Err(_) => Err(Failure::Unreachable(format!(
            "no answer in {} s",
            REQUEST_TIMEOUT.as_secs()
        ))),
    }
}

/// The client and its URL, or `None` when no deployment is set.
fn client(
    args: &RemoteArgs,
    settings: &HashMap<String, String>,
) -> Result<Option<(KairosClient, String)>, String> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let url = args
        .url
        .clone()
        .or_else(|| env("KAIROS_URL"))
        .or_else(|| settings.get("deployment_url").cloned());
    let key = env("KAIROS_KEY").or_else(|| env("KAIROS_MCP_KEY"));
    let path = credentials::credentials_path().map_err(|e| e.to_string())?;
    let store = credentials::load(&path).map_err(|e| e.to_string())?;

    if let Some(key) = key {
        let url = match url {
            Some(url) => credentials::normalize_url(&url),
            None => match credentials::resolve_deployment(&store, None) {
                Ok(url) => url,
                Err(_) => return Ok(None),
            },
        };
        let mut client = KairosClient::with_static_token(&url, key);
        if let Some(tenant) = &args.tenant {
            client = client.with_tenant(tenant);
        }
        return Ok(Some((client, url)));
    }
    let deployment = match credentials::resolve_deployment(&store, url.as_deref()) {
        Ok(deployment) => deployment,
        Err(_) if url.is_none() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let entry = credentials::entry_for(&store, &deployment).map_err(|e| {
        format!(
            "{} Or set KAIROS_KEY to a service-account key",
            e.to_string().replace('\n', " ")
        )
    })?;
    let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
    let mut client = KairosClient::new(&deployment, provider);
    if let Some(tenant) = args.tenant.clone().or(entry.tenant) {
        client = client.with_tenant(tenant);
    }
    Ok(Some((client, deployment)))
}

/// The `key: value` lines of the front matter of `.claude/kairos.local.md`.
fn settings(root: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(root.join(SETTINGS_FILE)) else {
        return HashMap::new();
    };
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return HashMap::new();
    }
    lines
        .take_while(|line| line.trim() != "---")
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let value = value.trim().trim_matches('"');
            (!value.is_empty()).then(|| (key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// A remote URL as `host/path`, in lower case, with no scheme, user,
/// `.git` or `/` at the end: `git@github.com:a/b.git` and
/// `https://github.com/a/b` give `github.com/a/b`.
fn normalized_remote(url: &str) -> String {
    let url = url.trim();
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest.to_string(),
        // scp form: user@host:path
        None => url.replacen(':', "/", 1),
    };
    let rest = rest.rsplit_once('@').map_or(rest.as_str(), |(_, r)| r);
    rest.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_lowercase()
}

/// The commit that the checkout branched from: the merge base of HEAD and
/// the default branch, else HEAD.
fn branch_point(root: &Path, branch: &str) -> Option<String> {
    for reference in [
        format!("refs/remotes/origin/{branch}"),
        format!("refs/heads/{branch}"),
    ] {
        if let Some(base) = git_output(root, &["merge-base", "HEAD", &reference]) {
            return Some(base);
        }
    }
    git_output(root, &["rev-parse", "HEAD"])
}

/// The count of the files of the checkout that differ from `commit`: the
/// changed files, also those not committed, and the files that git does not
/// track and does not ignore. `None` if git cannot compare.
fn changed_since(root: &Path, commit: &str) -> Option<usize> {
    let diff = git_output(root, &["diff", "--name-only", "--no-renames", commit, "--"])?;
    let untracked = git_output(root, &["ls-files", "--others", "--exclude-standard"])?;
    let files: BTreeSet<&str> = diff
        .lines()
        .chain(untracked.lines())
        .filter(|l| !l.is_empty())
        .collect();
    Some(files.len())
}

/// The count of the files that git knows in the checkout, and those that it
/// does not track and does not ignore.
pub fn file_count(root: &Path) -> usize {
    git_output(
        root,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    )
    .map_or(0, |out| out.lines().filter(|l| !l.is_empty()).count())
}

/// The seconds of the structure for each file (`COLLIERY-T-1852`: 962
/// files of Kairos in 95 s, with the SCIP run).
const SECONDS_PER_FILE: f64 = 0.1;

/// The seconds of one summary with llama.cpp on Metal (`COLLIERY-T-1853`:
/// 3,145 summaries of Kairos in 5,614 s on an M1 Max).
#[cfg(feature = "llama")]
const SECONDS_PER_SUMMARY_METAL: f64 = 1.8;

/// The seconds of one summary with llama.cpp on a CPU (the benchmark of
/// 2026-09-30: about 12 h for 3,288 model calls, with 4 threads).
#[cfg(feature = "llama")]
const SECONDS_PER_SUMMARY_CPU: f64 = 13.0;

/// The estimated time of a full build on this machine, and what it counts.
pub fn full_build_estimate(files: usize, summaries: usize) -> String {
    let structure = files as f64 * SECONDS_PER_FILE;
    #[cfg(feature = "llama")]
    {
        let per_summary = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            SECONDS_PER_SUMMARY_METAL
        } else {
            SECONDS_PER_SUMMARY_CPU
        };
        let total = structure + summaries as f64 * per_summary;
        format!(
            "about {} on this machine: {files} files and about {summaries} summaries",
            duration(total)
        )
    }
    #[cfg(not(feature = "llama"))]
    {
        let _ = summaries;
        format!(
            "about {} on this machine: {files} files. This kairos binary has no summarizer, \
             so it makes no summaries",
            duration(structure)
        )
    }
}

fn duration(seconds: f64) -> String {
    let seconds = seconds.round().max(1.0) as u64;
    match (seconds / 3600, seconds % 3600 / 60, seconds % 60) {
        (0, 0, s) => format!("{s} s"),
        (0, m, _) => format!("{m} min"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_compare_with_no_scheme_user_or_suffix() {
        let https = normalized_remote("https://github.com/colliery-io/kairos");
        assert_eq!(https, "github.com/colliery-io/kairos");
        assert_eq!(
            normalized_remote("git@github.com:colliery-io/kairos.git"),
            https
        );
        assert_eq!(
            normalized_remote("ssh://git@GitHub.com/colliery-io/kairos/"),
            https
        );
        assert_ne!(
            normalized_remote("https://github.com/colliery-io/weir"),
            https
        );
    }

    #[test]
    fn the_settings_are_the_front_matter() {
        let dir = tempfile::tempdir().expect("a folder");
        std::fs::create_dir_all(dir.path().join(".claude")).expect("make .claude");
        std::fs::write(
            dir.path().join(SETTINGS_FILE),
            "---\ndeployment_url: https://kairos.example\ntenant: colliery\nrepository: kairos\n---\n\nrepository: not this\n",
        )
        .expect("write");
        let settings = settings(dir.path());
        assert_eq!(settings["deployment_url"], "https://kairos.example");
        assert_eq!(settings["repository"], "kairos");
        assert_eq!(settings.len(), 3);
        assert!(super::settings(&dir.path().join("none")).is_empty());
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(duration(0.2), "1 s");
        assert_eq!(duration(95.0), "1 min");
        assert_eq!(duration(5614.0), "1 h 33 min");
    }
}
