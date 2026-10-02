//! The bare clone of a repository that the base code index needs
//! (COLLIERY-T-1853): to find the nearest indexed commit below a commit,
//! and to get the tree of a new commit for the builder.
//!
//! The `git` program does the work. Each command runs with no terminal
//! prompt, so a remote that asks for a password fails at once and does not
//! wait.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An error of a git command.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("The git command did not start: {0}.")]
    Start(std::io::Error),
    #[error("git {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
    #[error("Cannot write {path}: {source}.")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, GitError> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .output()
        .map_err(GitError::Start)
}

fn checked(dir: &Path, args: &[&str]) -> Result<Output, GitError> {
    let output = run(dir, args)?;
    if output.status.success() {
        return Ok(output);
    }
    Err(GitError::Failed {
        command: args.first().copied().unwrap_or_default().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// Make the bare clone at `path` if it is not there, and fetch the branches
/// and the tags of `remote` into it.
pub fn fetch(path: &Path, remote: &str) -> Result<(), GitError> {
    if !path.join("HEAD").is_file() {
        std::fs::create_dir_all(path).map_err(|source| GitError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        checked(path, &["init", "--bare", "--quiet"])?;
    }
    checked(
        path,
        &[
            "-c",
            "http.lowSpeedLimit=1000",
            "-c",
            "http.lowSpeedTime=60",
            "fetch",
            "--quiet",
            "--prune",
            "--no-write-fetch-head",
            remote,
            "+refs/heads/*:refs/heads/*",
            "+refs/tags/*:refs/tags/*",
        ],
    )?;
    Ok(())
}

/// Whether the clone has the commit `commit`.
pub fn has_commit(path: &Path, commit: &str) -> bool {
    path.join("HEAD").is_file()
        && run(path, &["cat-file", "-e", &format!("{commit}^{{commit}}")])
            .is_ok_and(|o| o.status.success())
}

/// The commit at the head of `branch`, if the clone has the branch.
pub fn branch_head(path: &Path, branch: &str) -> Result<Option<String>, GitError> {
    let output = run(
        path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

/// `commit` and the commits below it, nearest first, at most `limit`.
pub fn ancestors(path: &Path, commit: &str, limit: usize) -> Result<Vec<String>, GitError> {
    let output = checked(
        path,
        &[
            "rev-list",
            "--topo-order",
            &format!("--max-count={limit}"),
            commit,
        ],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

/// Write the tree of `commit` into the folder `to`.
pub fn checkout_tree(path: &Path, commit: &str, to: &Path) -> Result<(), GitError> {
    let output = checked(path, &["archive", "--format=tar", commit])?;
    tar::Archive::new(output.stdout.as_slice())
        .unpack(to)
        .map_err(|source| GitError::Io {
            path: to.to_path_buf(),
            source,
        })
}
