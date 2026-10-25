//! The bare clone of a repository that the base code index needs
//! (COLLIERY-T-1853): to find the nearest indexed commit below a commit,
//! and to get the tree of a new commit for the builder.
//!
//! The `git` program does the work. Each command runs with no terminal
//! prompt, so a remote that asks for a password fails at once and does not
//! wait.
//!
//! # The read token (COLLIERY-T-3105)
//!
//! [`fetch`] and [`ls_remote`] take an optional [`GitToken`]. The token goes
//! to git through a `GIT_ASKPASS` helper, and only through it:
//!
//! - The helper is a small `/bin/sh` script in a new temporary folder (mode
//!   0700) that is removed after the command. The script holds no secret. It
//!   prints [`TOKEN_USER`] for the user name prompt, and the value of the
//!   environment variable [`TOKEN_ENV`] for the password prompt.
//! - Only the one git child process (and the helper that git starts) gets
//!   [`TOKEN_ENV`]. The server process does not set it on itself.
//! - The token is never in the remote URL, the arguments, the config of the
//!   clone, or a file.
//! - Each command runs with `credential.helper` cleared, so that a
//!   credential helper of the host does not store the token or give a
//!   different one.
//! - [`GitError`] texts have the token replaced with `[token]`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use kairos_client::types_auth::Secret;

/// The user name that git sends with a token. GitHub takes any user name
/// with a token over HTTPS; this is the one that its documentation gives.
pub const TOKEN_USER: &str = "x-access-token";

/// The environment variable of the git child process that holds the token
/// for the askpass helper.
pub const TOKEN_ENV: &str = "KAIROS_GIT_TOKEN";

/// What the token is replaced with in an error text.
const REDACTED: &str = "[token]";

/// The askpass helper. It holds no secret: it reads the token from the
/// environment of the git process that runs it.
const ASKPASS_SCRIPT: &str = "#!/bin/sh\n\
case \"$1\" in\n\
  [Uu]sername*) printf '%s\\n' 'x-access-token' ;;\n\
  *) printf '%s\\n' \"$KAIROS_GIT_TOKEN\" ;;\n\
esac\n";

/// The read token of a repository. Its `Debug` shows no part of it.
pub type GitToken = Secret;

/// An error of a git command.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// KAIROS-T-0697: the error of the system in Kairos words.
    #[error("The git command did not start: {}. Is git installed on the server?", crate::error::io_words(.0))]
    Start(std::io::Error),
    #[error("git {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
    #[error("Cannot write {path}: {}.", crate::error::io_words(.source))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// `text` with each occurrence of the token replaced.
pub fn scrub(text: &str, token: Option<&GitToken>) -> String {
    match token {
        Some(token) if !token.is_empty() => text.replace(token.expose(), REDACTED),
        _ => text.to_string(),
    }
}

/// The askpass helper of one command, and the folder that holds it. The
/// folder goes away when this is dropped.
struct Askpass {
    _dir: tempfile::TempDir,
    script: PathBuf,
}

impl Askpass {
    fn new() -> Result<Self, GitError> {
        let dir = tempfile::Builder::new()
            .prefix("kairos-askpass-")
            .tempdir()
            .map_err(|source| GitError::Io {
                path: std::env::temp_dir(),
                source,
            })?;
        let script = dir.path().join("askpass.sh");
        let io = |source| GitError::Io {
            path: script.clone(),
            source,
        };
        // KAIROS-T-0511: the script is written under a draft name, closed,
        // made executable, and then renamed into place. Nothing holds the
        // final path open for write.
        let draft = dir.path().join("askpass.sh.draft");
        std::fs::write(&draft, ASKPASS_SCRIPT).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(io)?;
            std::fs::set_permissions(&draft, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
        }
        std::fs::rename(&draft, &script).map_err(io)?;
        Ok(Askpass { _dir: dir, script })
    }
}

/// How many times a spawn that fails with ETXTBSY is tried.
const ETXTBSY_TRIES: u32 = 5;

/// The wait after the first ETXTBSY; each later wait is one step longer.
const ETXTBSY_WAIT: Duration = Duration::from_millis(20);

/// Run `attempt`, and run it again after a short wait when it fails with
/// ETXTBSY ("Text file busy", `ExecutableFileBusy`), up to [`ETXTBSY_TRIES`]
/// times in all (KAIROS-T-0511).
///
/// On Linux, the exec of a file that a process holds open for write fails
/// with ETXTBSY. A child forked by a different thread while the askpass
/// helper is written inherits the open descriptor, and holds it until its
/// own exec: a transient state, so a short wait and a new try clear it.
fn retry_text_file_busy<T>(mut attempt: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut tries = 1;
    loop {
        match attempt() {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy && tries < ETXTBSY_TRIES => {
                std::thread::sleep(ETXTBSY_WAIT * tries);
                tries += 1;
            }
            result => return result,
        }
    }
}

/// `command.output()`, tried again on ETXTBSY (KAIROS-T-0511).
fn output(command: &mut Command) -> io::Result<Output> {
    retry_text_file_busy(|| command.output())
}

/// The git command for `args` in `dir`, with no prompt and no credential
/// helper. With a token, the askpass helper and the token in the
/// environment of this one child.
fn command(dir: &Path, args: &[&str], token: Option<(&GitToken, &Askpass)>) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(["-c", "credential.helper="])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove(TOKEN_ENV);
    match token {
        Some((token, askpass)) => {
            command
                .env("GIT_ASKPASS", &askpass.script)
                .env(TOKEN_ENV, token.expose());
        }
        None => {
            command.env("GIT_ASKPASS", "true");
        }
    }
    command
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, GitError> {
    output(&mut command(dir, args, None)).map_err(GitError::Start)
}

/// Run `args`, and fail with the standard error when git fails. The token
/// is not in the error.
fn checked_with(dir: &Path, args: &[&str], token: Option<&GitToken>) -> Result<Output, GitError> {
    let token = token.filter(|t| !t.is_empty());
    let askpass = token.map(|_| Askpass::new()).transpose()?;
    let output =
        output(&mut command(dir, args, token.zip(askpass.as_ref()))).map_err(GitError::Start)?;
    if output.status.success() {
        return Ok(output);
    }
    let command = args
        .iter()
        .find(|a| !a.starts_with('-') && !a.contains('='))
        .copied()
        .unwrap_or_default();
    Err(GitError::Failed {
        command: scrub(command, token),
        stderr: scrub(String::from_utf8_lossy(&output.stderr).trim(), token),
    })
}

fn checked(dir: &Path, args: &[&str]) -> Result<Output, GitError> {
    checked_with(dir, args, None)
}

/// Make the bare clone at `path` if it is not there, and fetch the branches
/// and the tags of `remote` into it, with `token` when it is given.
pub fn fetch(path: &Path, remote: &str, token: Option<&GitToken>) -> Result<(), GitError> {
    if !path.join("HEAD").is_file() {
        std::fs::create_dir_all(path).map_err(|source| GitError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        checked(path, &["init", "--bare", "--quiet"])?;
    }
    checked_with(
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
        token,
    )?;
    Ok(())
}

/// Run `git ls-remote <flags> <remote> <patterns>` in a temporary folder,
/// with `token` when it is given.
fn ls_remote_with(
    remote: &str,
    flags: &[&str],
    patterns: &[&str],
    token: Option<&GitToken>,
) -> Result<Output, GitError> {
    let dir = tempfile::Builder::new()
        .prefix("kairos-ls-remote-")
        .tempdir()
        .map_err(|source| GitError::Io {
            path: std::env::temp_dir(),
            source,
        })?;
    let mut args = vec![
        "-c",
        "http.lowSpeedLimit=1000",
        "-c",
        "http.lowSpeedTime=60",
        "ls-remote",
    ];
    args.extend_from_slice(flags);
    args.push(remote);
    args.extend_from_slice(patterns);
    checked_with(dir.path(), &args, token)
}

/// Whether git can read `remote` with `token`: `git ls-remote --heads`, in
/// a temporary folder. The access check of a token (COLLIERY-T-3105).
pub fn ls_remote(remote: &str, token: Option<&GitToken>) -> Result<(), GitError> {
    ls_remote_with(remote, &["--heads"], &[], token)?;
    Ok(())
}

/// The default branch of `remote` (KAIROS-T-0430): the branch that its
/// `HEAD` names, from `git ls-remote --symref <remote> HEAD`, with `token`
/// when it is given. `None` when the remote gives no `HEAD` that names a
/// branch (for example, a `HEAD` that names a branch that does not exist).
pub fn remote_head(remote: &str, token: Option<&GitToken>) -> Result<Option<String>, GitError> {
    let output = ls_remote_with(remote, &["--symref"], &["HEAD"], token)?;
    Ok(head_branch(&String::from_utf8_lossy(&output.stdout)))
}

/// The branch in the output of `git ls-remote --symref <remote> HEAD`: the
/// line `ref: refs/heads/<branch>\tHEAD`.
fn head_branch(listing: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let (target, name) = line.strip_prefix("ref: ")?.split_once('\t')?;
        (name.trim() == "HEAD")
            .then(|| target.strip_prefix("refs/heads/"))
            .flatten()
            .filter(|branch| !branch.is_empty())
            .map(str::to_string)
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_branch_is_read_from_the_symref_line() {
        let listing = "ref: refs/heads/development\tHEAD\n\
                       0123456789abcdef0123456789abcdef01234567\tHEAD\n";
        assert_eq!(head_branch(listing).as_deref(), Some("development"));
        assert_eq!(
            head_branch("ref: refs/heads/release/2.x\tHEAD\n").as_deref(),
            Some("release/2.x")
        );
        // No symref line: the remote gives no default branch.
        assert_eq!(
            head_branch("0123456789abcdef0123456789abcdef01234567\tHEAD\n"),
            None
        );
        assert_eq!(head_branch(""), None);
        // A HEAD that names no branch.
        assert_eq!(head_branch("ref: refs/tags/v1\tHEAD\n"), None);
    }

    #[test]
    fn the_head_branch_of_a_bare_repository_follows_its_head() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let bare = dir.path().join("bare.git");
        std::fs::create_dir_all(&work).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@kairos.test")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@kairos.test")
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}: {output:?}");
        };
        git(&work, &["init", "-q", "-b", "main"]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "A"]);
        git(&work, &["branch", "development"]);
        git(dir.path(), &["clone", "-q", "--bare", "work", "bare.git"]);
        let remote = bare.display().to_string();
        assert_eq!(remote_head(&remote, None).unwrap().as_deref(), Some("main"));
        git(&bare, &["symbolic-ref", "HEAD", "refs/heads/development"]);
        assert_eq!(
            remote_head(&remote, None).unwrap().as_deref(),
            Some("development")
        );
        // A HEAD that names a branch that does not exist gives no branch.
        git(&bare, &["symbolic-ref", "HEAD", "refs/heads/gone"]);
        assert_eq!(remote_head(&remote, None).unwrap(), None);
    }

    #[test]
    fn scrub_removes_each_occurrence_of_the_token() {
        let token = GitToken::new("ghp_s3cret");
        assert_eq!(
            scrub("a ghp_s3cret b ghp_s3cret", Some(&token)),
            "a [token] b [token]"
        );
        assert_eq!(scrub("nothing here", Some(&token)), "nothing here");
        assert_eq!(scrub("ghp_s3cret", None), "ghp_s3cret");
        // An empty token replaces nothing.
        assert_eq!(scrub("abc", Some(&GitToken::new(""))), "abc");
    }

    /// The askpass helper gives the user name and the token from its
    /// environment, and the script file holds no token.
    #[cfg(unix)]
    #[test]
    fn the_askpass_helper_reads_the_token_from_its_environment() {
        use std::os::unix::fs::PermissionsExt;
        let askpass = Askpass::new().expect("helper");
        let mode = std::fs::metadata(askpass.script.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        let ask = |prompt: &str| {
            let out = output(
                Command::new(&askpass.script)
                    .arg(prompt)
                    .env(TOKEN_ENV, "ghp_from_env"),
            )
            .expect("runs");
            String::from_utf8(out.stdout).unwrap()
        };
        // The draft is renamed into place: the folder holds the script only.
        let names: Vec<_> = std::fs::read_dir(askpass.script.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["askpass.sh"]);
        assert_eq!(
            ask("Username for 'https://github.com': "),
            "x-access-token\n"
        );
        assert_eq!(
            ask("Password for 'https://x-access-token@github.com': "),
            "ghp_from_env\n"
        );
        let script = std::fs::read_to_string(&askpass.script).unwrap();
        assert!(!script.contains("ghp_from_env"));
        let dir = askpass.script.parent().unwrap().to_path_buf();
        drop(askpass);
        assert!(!dir.exists(), "the folder of the helper is removed");
    }

    fn text_file_busy() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::ExecutableFileBusy, "Text file busy")
    }

    /// KAIROS-T-0511: a spawn that fails with ETXTBSY is tried again, and
    /// succeeds when the file is free. The error is injected: macOS does
    /// not give ETXTBSY.
    #[test]
    fn a_text_file_busy_spawn_is_tried_again() {
        let mut calls = 0;
        let result = retry_text_file_busy(|| {
            calls += 1;
            if calls < 3 {
                Err(text_file_busy())
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result.unwrap(), 3);
        assert_eq!(calls, 3);
    }

    /// The retry gives up after `ETXTBSY_TRIES` tries, with the error.
    #[test]
    fn a_spawn_that_stays_busy_fails_after_the_last_try() {
        let mut calls = 0;
        let err = retry_text_file_busy(|| -> std::io::Result<()> {
            calls += 1;
            Err(text_file_busy())
        })
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::ExecutableFileBusy);
        assert_eq!(calls, ETXTBSY_TRIES);
    }

    /// A different error is not tried again.
    #[test]
    fn a_spawn_that_fails_for_another_reason_is_not_tried_again() {
        let mut calls = 0;
        let err = retry_text_file_busy(|| -> std::io::Result<()> {
            calls += 1;
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        })
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(calls, 1);
    }

    /// KAIROS-T-0511, the race itself: helpers written and run on 8 threads
    /// at once, beside a thread that forks all the time. On Linux, a child
    /// forked while a helper is open for write holds its descriptor until
    /// its exec, and the exec of that helper gives ETXTBSY without the
    /// retry. Ignored: it takes seconds. Run it with `--ignored`.
    #[cfg(unix)]
    #[test]
    #[ignore = "stress test, seconds long"]
    fn the_askpass_helper_runs_beside_concurrent_spawns() {
        const THREADS: usize = 8;
        const ROUNDS: usize = 50;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let spawner = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    Command::new("true").output().expect("true runs");
                }
            })
        };
        let workers: Vec<_> = (0..THREADS)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..ROUNDS {
                        let askpass = Askpass::new().expect("helper");
                        let out = output(
                            Command::new(&askpass.script)
                                .arg("Password: ")
                                .env(TOKEN_ENV, "ghp_stress"),
                        )
                        .expect("runs");
                        assert_eq!(String::from_utf8(out.stdout).unwrap(), "ghp_stress\n");
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("a worker panicked");
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        spawner.join().unwrap();
    }

    /// A git error that has the token in it (here: the token is a part of
    /// the remote) has `[token]` in its place.
    #[test]
    fn a_git_error_has_no_token() {
        let work = tempfile::tempdir().unwrap();
        let token = GitToken::new("tok_ABC123xyz");
        let remote = work.path().join("no-repo-tok_ABC123xyz");
        let err = fetch(
            &work.path().join("clone.git"),
            remote.to_str().unwrap(),
            Some(&token),
        )
        .expect_err("no such remote");
        let text = err.to_string();
        assert!(!text.contains("tok_ABC123xyz"), "{text}");
        assert!(text.contains("[token]"), "{text}");
        let err = ls_remote(remote.to_str().unwrap(), Some(&token)).expect_err("no remote");
        assert!(!err.to_string().contains("tok_ABC123xyz"), "{err}");
    }
}
