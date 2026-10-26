//! The base index in Kairos, for `kairos index update` (COLLIERY-T-1854,
//! COLLIERY-I-0264 "The flow").
//!
//! - **The connection.** The URL: `--url`, else `KAIROS_URL`, else
//!   `deployment_url` in `.claude/kairos.local.md` of the checkout, else the
//!   credential cache of `kairos login`. A worktree has no such file (git
//!   excludes it): the CLI then reads the file of its main clone
//!   ([`settings_path`], KAIROS-T-0646), as the plugin hooks do. The token: `KAIROS_KEY`, else
//!   `KAIROS_MCP_KEY` (an agent key or a service-account key, as the plugin
//!   and `scripts/render-references.sh` use), else the credential cache.
//!   [`crate::auth`] resolves both, as for each other command.
//! - **The repository.** `--repo`, else `repository` in
//!   `.claude/kairos.local.md`, else the repository of Kairos whose
//!   `repo_url` is the `origin` remote of the checkout.
//! - **The branch point.** `git merge-base HEAD origin/<default branch>`
//!   (then the local default branch), else HEAD. Kairos gives the nearest
//!   indexed commit at or below it. The CLI also asks for the nearest index
//!   of `origin/<default branch>`, and takes the base with the smaller diff
//!   (KAIROS-T-0516): a clone that is only behind gets the index of the tip.
//! - **The changed files.** `git diff --name-only <base>` (the changes that
//!   are not committed too) and the files that git does not track and does
//!   not ignore.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kairos_client::{Error as ApiError, KairosClient};

use super::git_output;
use crate::auth::{self, Source};
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
    /// The tenant, when the token is from `kairos login`. Default: `tenant` in .claude/kairos.local.md, else the tenant of the login
    #[arg(long)]
    pub tenant: Option<String>,
    /// The repository in Kairos. Default: `repository` in .claude/kairos.local.md, else the repository of the `origin` remote. The old name --repository still works
    #[arg(long = "repo", alias = "repository", value_name = "REPOSITORY")]
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
    /// The commit that the CLI asked for: the indexed commit is at or
    /// below it (KAIROS-T-0517).
    pub from: String,
    /// The commits from the indexed commit to `from`, as Kairos gave them.
    pub distance: usize,
    /// The commits from HEAD to `origin/<default>`, when HEAD is an
    /// ancestor of it and not the same commit (KAIROS-T-0517).
    pub behind: Option<usize>,
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
    let settings = bound_settings(root);
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
    let repository = match pick_repository(root, args, &settings, &repositories) {
        Ok(repository) => repository,
        Err(why) => return Found::NoBase(why),
    };

    let point = branch_point(root, &repository.default_branch);
    let Some(point) = point else {
        return Found::NoBase("the checkout has no git commit".to_string());
    };
    // KAIROS-T-0516: the nearest index of the branch point, and of the tip
    // of `origin/<default>`. A clone that is only behind has HEAD as its
    // branch point, and the nearest index below it can be old. The index of
    // the tip is a descendant, and the diff works in each direction.
    let first = nearest_of(&client, &repository.slug, root, &point).await;
    let tip = git_output(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!(
                "refs/remotes/origin/{}^{{commit}}",
                repository.default_branch
            ),
        ],
    );
    let behind = tip.as_deref().and_then(|tip| behind_by(root, tip));
    let second = match (&first, tip) {
        (Asked::Unreachable(_), _) => None,
        (Asked::Found(n), _) if n.changed == 0 => None,
        (_, Some(tip)) if tip != point => {
            Some(nearest_of(&client, &repository.slug, root, &tip).await)
        }
        _ => None,
    };
    let nearest = match (first, second) {
        (Asked::Found(a), Some(Asked::Found(b))) => choose(a, b),
        (Asked::Found(a), _) | (_, Some(Asked::Found(a))) => a,
        (Asked::Unreachable(why), _) => return unreachable(why),
        (Asked::NoBase(why), _) => return Found::NoBase(why),
    };
    Found::Base(Base {
        client,
        repository: repository.slug.clone(),
        branch: repository.default_branch.clone(),
        commit: nearest.commit,
        changed: nearest.changed,
        summaries: nearest.summaries,
        from: nearest.from,
        distance: nearest.distance,
        behind,
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

/// The repository of the checkout at `root` in `repositories`: `--repo`,
/// else `repository` in `.claude/kairos.local.md`, else the repository
/// whose `repo_url` is the `origin` remote. The error says why none is.
fn pick_repository<'a>(
    root: &Path,
    args: &RemoteArgs,
    settings: &HashMap<String, String>,
    repositories: &'a [kairos_client::types_repositories::Repository],
) -> Result<&'a kairos_client::types_repositories::Repository, String> {
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
    repository.ok_or_else(|| match wanted {
        Some(slug) => format!("Kairos has no repository {slug:?}"),
        None => "no repository in Kairos has the `origin` remote of this checkout. \
                 Give `--repo`"
            .to_string(),
    })
}

/// The client and the slug of the repository of the checkout at `root`,
/// for `kairos index upload` (KAIROS-T-0592). The connection and the
/// repository come as for [`find`]. The error says what is missing.
pub async fn repository_of(
    root: &Path,
    args: &RemoteArgs,
) -> Result<(KairosClient, String), String> {
    let settings = bound_settings(root);
    let (client, url) = client(args, &settings)?.ok_or_else(|| {
        format!(
            "The CLI does not know the deployment. Use --url, set {}, or run `kairos login`",
            auth::URL_VAR
        )
    })?;
    let repositories = match within(client.list_repositories(None)).await {
        Ok(list) => list,
        Err(Failure::Unreachable(why)) => {
            return Err(format!("The CLI cannot reach Kairos at {url} ({why})"));
        }
        Err(Failure::Refused(why)) => return Err(format!("Kairos refused the request ({why})")),
    };
    let slug = pick_repository(root, args, &settings, &repositories)?
        .slug
        .clone();
    Ok((client, slug))
}

/// One nearest index that Kairos gave, and the files of the checkout that
/// differ from it.
#[derive(Debug, Clone, PartialEq)]
struct Nearest {
    commit: String,
    changed: usize,
    summaries: usize,
    from: String,
    distance: usize,
}

/// The answer of Kairos to one request of the nearest index.
enum Asked {
    Found(Nearest),
    Unreachable(String),
    /// No base: the text says why.
    NoBase(String),
}

/// Ask Kairos for the nearest index at or below `from`, and count the files
/// of the checkout that differ from it.
async fn nearest_of(client: &KairosClient, slug: &str, root: &Path, from: &str) -> Asked {
    let nearest = match within(client.nearest_code_index(slug, from)).await {
        Ok(nearest) => nearest,
        Err(Failure::Unreachable(why)) => return Asked::Unreachable(why),
        Err(Failure::Refused(why)) => {
            return Asked::NoBase(format!(
                "Kairos has no index at or below commit {} ({why})",
                short(from)
            ));
        }
    };
    let Some(changed) = changed_since(root, &nearest.index.commit) else {
        return Asked::NoBase(format!(
            "the checkout does not have commit {}",
            short(&nearest.index.commit)
        ));
    };
    Asked::Found(Nearest {
        commit: nearest.index.commit,
        changed,
        summaries: usize::try_from(nearest.index.summary_keys).unwrap_or(0),
        from: from.to_string(),
        distance: nearest.distance,
    })
}

/// The commits from HEAD to `tip`, when HEAD is an ancestor of `tip` and
/// not the same commit: the checkout is only behind (KAIROS-T-0517).
fn behind_by(root: &Path, tip: &str) -> Option<usize> {
    git_output(root, &["merge-base", "--is-ancestor", "HEAD", tip])?;
    let count = git_output(root, &["rev-list", "--count", &format!("HEAD..{tip}")])?;
    count.parse().ok().filter(|n| *n > 0)
}

/// The base with the smaller diff (KAIROS-T-0516): `of_point`, the index of
/// the branch point, on a tie.
fn choose(of_point: Nearest, of_tip: Nearest) -> Nearest {
    if of_tip.changed < of_point.changed {
        of_tip
    } else {
        of_point
    }
}

/// The client of the Kairos deployment of the checkout at `root`: the
/// connection of [`find`], with no flags (KAIROS-T-0360). `None` when no
/// deployment is set or the credentials do not resolve. It sends no
/// request.
pub fn connection(root: &Path) -> Option<KairosClient> {
    client(&RemoteArgs::default(), &bound_settings(root))
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
    let env = auth::Env::from_process();
    let url = args
        .url
        .clone()
        .or_else(|| env.url.clone())
        .or_else(|| settings.get("deployment_url").cloned());
    let path = credentials::credentials_path().map_err(|e| e.to_string())?;
    let source = match auth::resolve(url.as_deref(), &env, || credentials::load(&path)) {
        Ok(source) => source,
        Err(_) if url.is_none() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    // KAIROS-T-0595: the daily version check asks this deployment.
    match &source {
        Source::Key { deployment, .. } | Source::Cache { deployment, .. } => {
            auth::note_deployment(deployment)
        }
    }
    let (deployment, store) = match source {
        Source::Key {
            deployment,
            var,
            key,
        } => {
            let mut client = auth::key_client(&deployment, var, key);
            if let Some(tenant) = &args.tenant {
                client = client.with_tenant(tenant);
            }
            return Ok(Some((client, deployment)));
        }
        Source::Cache { deployment, store } => (deployment, store),
    };
    let entry = credentials::entry_for(&store, &deployment).map_err(|e| {
        format!(
            "{} Or set KAIROS_KEY to an agent key or a service-account key",
            e.to_string().replace('\n', " ")
        )
    })?;
    let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
    let mut client = KairosClient::new(&deployment, provider);
    if let Some(tenant) = login_tenant(args, settings, entry.tenant) {
        client = client.with_tenant(tenant);
    }
    Ok(Some((client, deployment)))
}

/// The tenant of a token from `kairos login` (KAIROS-T-0594): `--tenant`,
/// else `tenant` in `.claude/kairos.local.md`, else the tenant of the login.
/// A key carries its tenant, so a key does not use the settings file.
fn login_tenant(
    args: &RemoteArgs,
    settings: &HashMap<String, String>,
    cached: Option<String>,
) -> Option<String> {
    args.tenant
        .clone()
        .or_else(|| settings.get("tenant").cloned())
        .or(cached)
}

/// The settings of the checkout at `root` with the config of the person
/// (#270): its `[[project]]`, then `.claude/kairos.local.md`, then
/// `[default]`, with the key names of the settings file. A `config.toml`
/// that does not parse gives the settings file alone; `kairos config show`
/// names the error.
fn bound_settings(root: &Path) -> HashMap<String, String> {
    crate::project_config::binding(root)
        .map(|b| b.as_settings())
        .unwrap_or_else(|_| settings(root))
}

/// The settings file of the checkout at `root` (KAIROS-T-0646):
/// `.claude/kairos.local.md` in `root`, else the same file in the main
/// clone. Git excludes the file, so a worktree has none. The main clone is
/// the folder above `git rev-parse --git-common-dir`, as in
/// `session_start.wiring_path` of the plugin. `None` when neither exists.
/// A folder with no `.git` gets no git call.
pub(crate) fn settings_path(root: &Path) -> Option<PathBuf> {
    let own = root.join(SETTINGS_FILE);
    if own.is_file() {
        return Some(own);
    }
    if !root.join(".git").exists() {
        return None;
    }
    let common = git_output(root, &["rev-parse", "--git-common-dir"])?;
    let common = std::fs::canonicalize(root.join(common)).ok()?;
    let clone = common.parent()?.join(SETTINGS_FILE);
    clone.is_file().then_some(clone)
}

/// The `key: value` lines of the front matter of the settings file of the
/// checkout at `root` ([`settings_path`]).
pub(crate) fn settings(root: &Path) -> HashMap<String, String> {
    let Some(text) = settings_path(root).and_then(|path| std::fs::read(path).ok()) else {
        return HashMap::new();
    };
    front_matter(&String::from_utf8_lossy(&text))
}

/// The `key: value` lines of a front matter. The rule is also the rule of
/// `read_frontmatter` of the plugin hooks (KAIROS-T-0708): an indented
/// line, a blank line and a `#` line are not keys; one pair of matching
/// quotes is removed from a value; with no quotes, a ` #` comment is cut;
/// an empty value is not set.
pub(crate) fn front_matter(text: &str) -> HashMap<String, String> {
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

/// A remote URL as `host/path`, in lower case, with no scheme, user,
/// `.git` or `/` at the end: `git@github.com:a/b.git` and
/// `https://github.com/a/b` give `github.com/a/b`.
pub(crate) fn normalized_remote(url: &str) -> String {
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

    /// KAIROS-T-0708: the CLI and the plugin hooks read each front matter
    /// to the same values (the cases are shared with the hook tests).
    #[test]
    fn the_front_matter_rule_is_the_rule_of_the_hooks() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../plugin/hooks/frontmatter_cases.json"
        ))
        .expect("the cases parse");
        for case in cases["cases"].as_array().expect("a list of cases") {
            let text = case["text"].as_str().expect("a text");
            let want: HashMap<String, String> =
                serde_json::from_value(case["values"].clone()).expect("the values");
            assert_eq!(front_matter(text), want, "{}", case["name"]);
        }
    }

    /// KAIROS-T-0646: a worktree has no settings file (git excludes it).
    /// The CLI reads the file of its main clone. A file in the worktree
    /// comes first.
    #[test]
    fn a_worktree_reads_the_settings_file_of_its_clone() {
        let dir = tempfile::tempdir().expect("a folder");
        let clone = dir.path().join("payments");
        std::fs::create_dir_all(clone.join(".claude")).expect("make .claude");
        crate::index::test_git(&clone, &["init", "-q", "-b", "main"]);
        crate::index::test_git(&clone, &["commit", "-q", "--allow-empty", "-m", "A"]);
        std::fs::write(
            clone.join(SETTINGS_FILE),
            "---\ndeployment_url: https://kairos.example\nrepository: payments\n---\n",
        )
        .expect("write");
        let worktree = dir.path().join("wt-payments");
        let path = worktree.display().to_string();
        crate::index::test_git(&clone, &["worktree", "add", "-q", "-b", "work", &path]);

        let settings = settings(&worktree);
        assert_eq!(settings["deployment_url"], "https://kairos.example");
        assert_eq!(settings["repository"], "payments");

        std::fs::create_dir_all(worktree.join(".claude")).expect("make .claude");
        std::fs::write(worktree.join(SETTINGS_FILE), "---\nrepository: own\n---\n").expect("write");
        assert_eq!(super::settings(&worktree)["repository"], "own");

        // A folder that is no checkout has no settings, and git is not asked.
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).expect("make the folder");
        assert!(settings_path(&plain).is_none());
    }

    /// KAIROS-T-0516: the base with the smaller diff, the branch point on a
    /// tie.
    #[test]
    fn the_base_with_the_smaller_diff_is_chosen() {
        let of = |commit: &str, changed| Nearest {
            commit: commit.into(),
            changed,
            summaries: 0,
            from: commit.into(),
            distance: 0,
        };
        assert_eq!(choose(of("a", 250), of("b", 1)).commit, "b");
        assert_eq!(choose(of("a", 3), of("b", 40)).commit, "a");
        assert_eq!(choose(of("a", 5), of("b", 5)).commit, "a");
    }

    /// KAIROS-T-0594: the tenant of a login is `--tenant`, else `tenant`
    /// in the settings file, else the tenant of the login.
    #[test]
    fn the_tenant_of_a_login_reads_the_settings_file() {
        let settings: HashMap<String, String> =
            [("tenant".to_string(), "colliery".to_string())].into();
        let none = RemoteArgs::default();
        let flag = RemoteArgs {
            tenant: Some("flag".into()),
            ..RemoteArgs::default()
        };
        let cached = || Some("cached".to_string());
        assert_eq!(
            login_tenant(&flag, &settings, cached()).as_deref(),
            Some("flag")
        );
        assert_eq!(
            login_tenant(&none, &settings, cached()).as_deref(),
            Some("colliery")
        );
        assert_eq!(
            login_tenant(&none, &HashMap::new(), cached()).as_deref(),
            Some("cached")
        );
        assert_eq!(login_tenant(&none, &HashMap::new(), None), None);
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(duration(0.2), "1 s");
        assert_eq!(duration(95.0), "1 min");
        assert_eq!(duration(5614.0), "1 h 33 min");
    }
}
