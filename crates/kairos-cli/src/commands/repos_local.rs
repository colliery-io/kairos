//! `kairos repos local [<FOLDER>]` (KAIROS-T-0646, COLLIERY-I-0638): the
//! clones and worktrees of a folder, with the repository of each.
//!
//! A board folder holds the clones of the repositories of a team, and the
//! worktrees of the work. The verb lists each immediate subfolder that has
//! a `.git` (a folder: a clone; a file: a worktree). A folder that is
//! itself a checkout gives that checkout only. For each checkout it gives
//! the branch, the `origin` remote as `host/path` (never a user or a
//! token), the repository of Kairos whose `repo_url` is that remote, and
//! the count of commits behind the default branch.
//!
//! The count comes from the local refs: the verb runs no fetch. The default
//! branch is the one of the repository in Kairos, when the clone has its
//! ref. Else it is `origin/HEAD`, else `origin/main`, else `origin/master`.
//!
//! The deployment: `--url`, else `KAIROS_URL`, else `deployment` of the
//! binding of the folder (`kairos config show`), else the deployment of
//! `kairos login`. The session hook of the plugin runs the verb in a board
//! folder.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::context::{Common, client_for};
use crate::error::CliError;
use crate::index::{git_output, normalized_remote};
use crate::listing::{Paging, fetch, print_whole};
use crate::table::Table;

/// One clone or worktree of the folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checkout {
    /// The name of the subfolder, or `.` for the folder itself.
    pub folder: String,
    /// The absolute path of the checkout.
    pub path: String,
    /// True when `.git` is a file: a worktree.
    pub worktree: bool,
    /// The folder name of the main clone of a worktree.
    pub clone: Option<String>,
    /// The branch, or `None` for a detached HEAD.
    pub branch: Option<String>,
    /// The `origin` remote as `host/path`, or `None` with no `origin`.
    pub remote: Option<String>,
    /// The slug of the repository in Kairos whose `repo_url` is the remote.
    pub repository: Option<String>,
    /// The default branch that `behind` counts against.
    pub default_branch: Option<String>,
    /// The commits of `origin/<default_branch>` that HEAD does not have,
    /// from the local refs.
    pub behind: Option<u64>,
}

/// A repository of Kairos, as the match needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub slug: String,
    /// The `repo_url`, normalized.
    pub remote: String,
    pub default_branch: String,
}

/// The checkouts in `folder`, sorted by name: the folder itself when it is
/// a checkout, else each subfolder with a `.git`. Hidden folders are
/// skipped. A folder that cannot be read is a failure that names it.
pub fn checkouts_in(folder: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    if !folder.is_dir() {
        return Err(CliError::Failure(format!(
            "The folder {} does not exist, or it is not a folder.",
            folder.display()
        )));
    }
    if folder.join(".git").exists() {
        return Ok(vec![(".".to_string(), folder.to_path_buf())]);
    }
    let entries = std::fs::read_dir(folder).map_err(|e| {
        CliError::Failure(format!(
            "The CLI cannot read the folder {}: {e}.",
            folder.display()
        ))
    })?;
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            (!name.starts_with('.') && path.is_dir() && path.join(".git").exists())
                .then_some((name, path))
        })
        .collect();
    found.sort();
    Ok(found)
}

/// The default branch of `origin` in the local refs: `origin/HEAD`, else
/// `main`, else `master`.
fn origin_default(path: &Path) -> Option<String> {
    if let Some(head) = git_output(
        path,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(name) = head.strip_prefix("origin/")
    {
        return Some(name.to_string());
    }
    ["main", "master"]
        .into_iter()
        .find(|name| has_origin_ref(path, name))
        .map(str::to_string)
}

fn has_origin_ref(path: &Path, branch: &str) -> bool {
    git_output(
        path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}^{{commit}}"),
        ],
    )
    .is_some()
}

/// The repository of `remote` (normalized) in `known`.
pub fn repository_of<'a>(remote: &str, known: &'a [Known]) -> Option<&'a Known> {
    known.iter().find(|k| k.remote == remote)
}

/// One checkout, matched against the repositories of Kairos.
pub fn describe(folder: &str, path: &Path, known: &[Known]) -> Checkout {
    let worktree = path.join(".git").is_file();
    let clone = worktree
        .then(|| git_output(path, &["rev-parse", "--git-common-dir"]))
        .flatten()
        .and_then(|common| std::fs::canonicalize(path.join(common)).ok())
        .and_then(|common| {
            common
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
        });
    let branch = git_output(path, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let remote = git_output(path, &["config", "--get", "remote.origin.url"])
        .map(|url| normalized_remote(&url));
    let matched = remote.as_deref().and_then(|r| repository_of(r, known));
    let default_branch = match matched {
        Some(k) if has_origin_ref(path, &k.default_branch) => Some(k.default_branch.clone()),
        _ if remote.is_some() => origin_default(path),
        _ => None,
    };
    let behind = default_branch.as_deref().and_then(|branch| {
        git_output(
            path,
            &[
                "rev-list",
                "--count",
                &format!("HEAD..refs/remotes/origin/{branch}"),
            ],
        )
        .and_then(|count| count.parse().ok())
    });
    Checkout {
        folder: folder.to_string(),
        path: path.display().to_string(),
        worktree,
        clone,
        branch,
        remote,
        repository: matched.map(|k| k.slug.clone()),
        default_branch,
        behind,
    }
}

/// The table of the checkouts. Pure.
pub fn local_table(checkouts: &[Checkout]) -> Table {
    let mut table = Table::new(&["FOLDER", "REPOSITORY", "KIND", "BRANCH", "BEHIND", "REMOTE"]);
    let dash = || "-".to_string();
    for c in checkouts {
        let kind = match (c.worktree, &c.clone) {
            (true, Some(clone)) => format!("worktree of {clone}/"),
            (true, None) => "worktree".to_string(),
            (false, _) => "clone".to_string(),
        };
        let behind = match (&c.default_branch, c.behind) {
            (Some(branch), Some(n)) => format!("{n} (origin/{branch})"),
            _ => dash(),
        };
        table.row(vec![
            c.folder.clone(),
            c.repository.clone().unwrap_or_else(dash),
            kind,
            c.branch.clone().unwrap_or_else(|| "(detached)".to_string()),
            behind,
            c.remote.clone().unwrap_or_else(dash),
        ]);
    }
    table
}

/// `kairos repos local`.
pub async fn run(folder: Option<PathBuf>, common: Common) -> Result<(), CliError> {
    let folder = match folder {
        Some(folder) => folder,
        None => std::env::current_dir().map_err(|e| {
            CliError::Failure(format!("The CLI cannot read the current folder: {e}."))
        })?,
    };
    let found = checkouts_in(&folder)?;
    // #270: the deployment and the tenant of the folder, not of the current
    // folder of the command.
    let binding = crate::project_config::binding(&folder)?;
    let (client, _) = client_for(&common, &binding)?;
    let paging = Paging {
        all: true,
        ..Paging::default()
    };
    let repositories = fetch(paging, |page| client.list_repositories_page(None, page)).await?;
    let known: Vec<Known> = repositories
        .items
        .iter()
        .map(|r| Known {
            slug: r.slug.clone(),
            remote: normalized_remote(&r.repo_url),
            default_branch: r.default_branch.clone(),
        })
        .collect();
    let checkouts: Vec<Checkout> = found
        .iter()
        .map(|(name, path)| describe(name, path, &known))
        .collect();
    if common.json {
        return print_whole(&checkouts);
    }
    if checkouts.is_empty() {
        println!("(none)");
    } else {
        print!("{}", local_table(&checkouts).render());
    }
    println!("total: {}", checkouts.len());
    println!("BEHIND counts from the local refs, as of the last fetch.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::test_git as git;

    fn known() -> Vec<Known> {
        vec![Known {
            slug: "payments".into(),
            remote: "github.com/acme/payments".into(),
            default_branch: "main".into(),
        }]
    }

    /// A board folder: a bare origin with 3 commits, a clone 2 commits
    /// behind it, a worktree of the clone, a plain folder, a file and a
    /// hidden checkout.
    fn board() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a folder");
        let origin = dir.path().join("origin.git");
        let seed = dir.path().join("seed");
        std::fs::create_dir_all(&seed).expect("seed");
        git(&seed, &["init", "-q", "-b", "main"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "A"]);
        let origin_path = origin.display().to_string();
        git(dir.path(), &["clone", "-q", "--bare", "seed", &origin_path]);
        let board = dir.path().join("board");
        std::fs::create_dir_all(&board).expect("board");
        git(&board, &["clone", "-q", &origin_path, "payments"]);
        git(&seed, &["remote", "add", "origin", &origin_path]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "B"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "C"]);
        git(&seed, &["push", "-q", "origin", "main"]);
        let clone = board.join("payments");
        git(&clone, &["fetch", "-q"]);
        git(
            &clone,
            &[
                "remote",
                "set-url",
                "origin",
                "https://robin:secret@github.com/acme/payments.git",
            ],
        );
        let worktree = board.join("wt-payments").display().to_string();
        git(
            &clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "work",
                &worktree,
                "origin/main",
            ],
        );
        std::fs::create_dir_all(board.join("notes")).expect("notes");
        std::fs::write(board.join("README"), "x").expect("file");
        git(&board, &["init", "-q", ".hidden"]);
        dir
    }

    #[test]
    fn each_clone_and_worktree_is_listed_and_nothing_else() {
        let dir = board();
        let found = checkouts_in(&dir.path().join("board")).expect("the list");
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["payments", "wt-payments"]);
    }

    #[test]
    fn a_clone_has_its_repository_branch_and_count_behind() {
        let dir = board();
        let clone = dir.path().join("board/payments");
        let c = describe("payments", &clone, &known());
        assert_eq!(c.repository.as_deref(), Some("payments"));
        assert_eq!(c.remote.as_deref(), Some("github.com/acme/payments"));
        assert_eq!(c.branch.as_deref(), Some("main"));
        assert_eq!(c.default_branch.as_deref(), Some("main"));
        assert_eq!(c.behind, Some(2));
        assert!(!c.worktree);
        assert_eq!(c.clone, None);
        // The user and the token of the URL are never in the output.
        let json = serde_json::to_string(&c).expect("json");
        assert!(
            !json.contains("secret") && !json.contains("robin"),
            "{json}"
        );
    }

    #[test]
    fn a_worktree_names_its_clone_and_its_branch() {
        let dir = board();
        let path = dir.path().join("board/wt-payments");
        let c = describe("wt-payments", &path, &known());
        assert!(c.worktree);
        assert_eq!(c.clone.as_deref(), Some("payments"));
        assert_eq!(c.branch.as_deref(), Some("work"));
        assert_eq!(c.repository.as_deref(), Some("payments"));
        assert_eq!(c.behind, Some(0));
    }

    #[test]
    fn an_unknown_remote_has_no_repository_and_still_counts() {
        let dir = board();
        let clone = dir.path().join("board/payments");
        let c = describe("payments", &clone, &[]);
        assert_eq!(c.repository, None);
        // No repository: origin/HEAD of the clone gives the default branch.
        assert_eq!(c.default_branch.as_deref(), Some("main"));
        assert_eq!(c.behind, Some(2));
    }

    #[test]
    fn a_folder_that_is_a_checkout_lists_itself() {
        let dir = board();
        let clone = dir.path().join("board/payments");
        let found = checkouts_in(&clone).expect("the list");
        assert_eq!(found, vec![(".".to_string(), clone)]);
    }

    #[test]
    fn a_missing_folder_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("a folder");
        let missing = dir.path().join("none");
        let err = checkouts_in(&missing).expect_err("refused");
        assert!(err.to_string().contains("none"), "{err}");
    }

    #[test]
    fn the_table_names_the_kind_and_the_count() {
        let rows = [
            Checkout {
                folder: "payments".into(),
                path: "/tmp/b/payments".into(),
                worktree: false,
                clone: None,
                branch: Some("main".into()),
                remote: Some("github.com/acme/payments".into()),
                repository: Some("payments".into()),
                default_branch: Some("main".into()),
                behind: Some(2),
            },
            Checkout {
                folder: "wt-x".into(),
                path: "/tmp/b/wt-x".into(),
                worktree: true,
                clone: Some("payments".into()),
                branch: None,
                remote: None,
                repository: None,
                default_branch: None,
                behind: None,
            },
        ];
        let text = local_table(&rows).render();
        assert!(text.contains("2 (origin/main)"), "{text}");
        assert!(text.contains("worktree of payments/"), "{text}");
        assert!(text.contains("(detached)"), "{text}");
    }
}
