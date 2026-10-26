//! `kairos index`: the code index of a checkout (COLLIERY-I-0264,
//! COLLIERY-T-1852).
//!
//! - `kairos index build` builds the structure (files, symbols, call edges)
//!   with a SCIP run for the Rust edges, then the summaries.
//! - `kairos index update` builds the structure again from the tree and
//!   summarizes only what changed. It keeps the Rust edges of the index for
//!   each function whose code did not change, and runs no SCIP unless
//!   `--rust-edges` (or `--scip`) is given. A SCIP run covers each language
//!   whose indexer is on the computer: rust-analyzer, scip-go,
//!   scip-typescript and scip-python (KAIROS-T-0473). A language whose
//!   indexer is missing or fails gets its edges by name, with a warning. With `--link-only`, it runs no model: it links
//!   the summaries of the pool and makes no new ones (COLLIERY-T-2529, the
//!   background update of the plugin). It starts from the nearest base index in
//!   Kairos when the checkout has no index, or when the base is nearer to
//!   the tree than the local index ([`base`], COLLIERY-T-1854). When the
//!   local index is as near, it keeps its structure; if the base has more
//!   summaries than the local index links, the CLI takes the pool of the
//!   base first (KAIROS-T-0355, [`take_base_summaries`]). If more
//!   files than the limit changed since the base, it builds nothing and
//!   tells the developer to rebase, or to run `kairos index --full`. The
//!   default limit applies only when a model runs ([`limit_of`],
//!   KAIROS-T-0515).
//! - `kairos index --full` builds the index again with no base: the same as
//!   `kairos index build`.
//! - `kairos index status` gives the counts.
//! - `kairos index duplicates` finds repeated code: exact copies, near copies
//!   and the same idea in other code (COLLIERY-T-1857). It gives the text of the
//!   MCP tool `duplicates`.
//! - `kairos index mcp` serves the code tools to an agent over stdio
//!   ([`mcp`]).
//! - `kairos index upload` sends the index of the checkout to Kairos as the
//!   index of its HEAD commit ([`upload`], KAIROS-T-0592).
//!
//! The index is `.kairos/index.db` in the checkout. Git must not track it,
//! so a build adds it to `.git/info/exclude` when git does not ignore it
//! already. That file is local to the checkout and is not tracked, so the
//! CLI changes no file of the repository, and each repository that it
//! indexes is covered with no commit.
//!
//! Summaries need the feature `llama` (llama.cpp and the local vector
//! model) and the model file. Without them, a build or an update makes the
//! structure and the edges, and says that it made no summaries and why.

mod arguments;
mod base;
mod error;
mod mcp;
mod query;
mod report;
mod upload;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Subcommand;
use kairos_index::{
    BuildOptions, BuildReport, DuplicateKind, DuplicateOptions, Index, UpdateOptions,
};

pub use error::IndexCommandError;

// KAIROS-T-0646: `kairos repos local` reads the settings and the remotes as
// `kairos index` does.
pub(crate) use base::{normalized_remote, settings, settings_path};

use crate::context::print_json;
use crate::error::CliError;

/// The default limit of the files changed since the base index, when a
/// model runs (COLLIERY-I-0264, "The flow"). An update that runs no model
/// has no default limit (KAIROS-T-0515): it parses the changed files and
/// links the summaries of the pool, about 0.1 s for each file.
pub const DEFAULT_MAX_CHANGED: usize = 200;

/// The limit of the files changed since the base index: `max_changed`, else
/// [`DEFAULT_MAX_CHANGED`] when a model runs, else no limit (KAIROS-T-0515).
pub(super) fn limit_of(max_changed: Option<usize>, model_runs: bool) -> Option<usize> {
    max_changed.or(model_runs.then_some(DEFAULT_MAX_CHANGED))
}

/// `kairos index`: a subcommand, or `--full`.
#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true, arg_required_else_help = true)]
pub struct IndexArgs {
    /// Build the whole index of the checkout again, with no base index: the same as `kairos index build`
    #[arg(long)]
    full: bool,
    #[command(flatten)]
    root: RootArg,
    /// Print one JSON object with the result of the build, in place of the text
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: Option<IndexCommand>,
}

impl IndexArgs {
    /// The command is `kairos index mcp`: a long stdio session, with no
    /// daily version check after it (KAIROS-T-0595).
    pub fn serves_mcp(&self) -> bool {
        matches!(self.command, Some(IndexCommand::Mcp { .. }))
    }

    pub async fn run(self) -> Result<(), CliError> {
        match self.command {
            Some(command) => command.run().await,
            None if self.full => build(&resolve_root(self.root)?, self.json),
            None => Err(CliError::Failure(
                "Give a subcommand of `kairos index`, or `--full`. See `kairos index --help`."
                    .into(),
            )),
        }
    }
}

#[derive(Subcommand)]
pub enum IndexCommand {
    /// Build the index of the checkout: the structure, the call edges and the summaries
    Build {
        #[command(flatten)]
        root: RootArg,
        /// Print one JSON object with the result, in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Update the index for the changes in the checkout, also those not committed. With no index, start from the nearest base index in Kairos
    Update {
        #[command(flatten)]
        root: RootArg,
        /// Run the SCIP indexers again (rust-analyzer, scip-go, scip-typescript, scip-python) for the precise edges of the changed code
        #[arg(long, visible_alias = "scip")]
        rust_edges: bool,
        /// Run no model: link the summaries of the pool and make no new ones
        #[arg(long)]
        link_only: bool,
        /// The most files that can change since the base index. Above it, the CLI builds nothing. Default: 200 when a model runs, no limit with --link-only or on a hosted repository
        #[arg(long)]
        max_changed: Option<usize>,
        #[command(flatten)]
        remote: base::RemoteArgs,
        /// Print one JSON object with the result, in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Show the counts of the index: files, symbols, edges and summaries
    Status {
        #[command(flatten)]
        root: RootArg,
        /// Print one JSON object with the result, in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Find repeated code: exact copies, near copies and the same idea in other code
    Duplicates {
        #[command(flatten)]
        root: RootArg,
        /// Only this kind of repeated code
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
        /// The smallest function to compare, in lines
        #[arg(long, default_value_t = kairos_index::DEFAULT_MIN_LINES, value_parser = clap::value_parser!(u32).range(1..))]
        min_lines: u32,
        /// Also compare test code
        #[arg(long)]
        tests: bool,
        /// Only the functions in files under this path, from the root of the checkout
        #[arg(long)]
        under: Option<String>,
        /// The most groups to show, from 1 to 200
        #[arg(long, default_value_t = mcp::DEFAULT_GROUPS as u64, value_parser = clap::value_parser!(u64).range(1..=mcp::MAX_GROUPS as u64))]
        limit: u64,
        /// Print the groups as the JSON list envelope (items, total, next_offset), in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Show each use of a symbol: in Rust, the uses as a type, a field, an import and a call; in the other languages, the calls. Give an ADR short code to show each place that cites the ADR
    References {
        #[command(flatten)]
        root: RootArg,
        /// The symbol: a name, `Type::name` or `Type.name`, or a candidate `name (file:line)`; or an ADR short code, as `KAIROS-A-0065`
        symbol: String,
        /// The file of the symbol, when more than one symbol has the name
        #[arg(long)]
        file: Option<String>,
        /// A line of the symbol, when more than one symbol of one file has the name
        #[arg(long)]
        line: Option<u32>,
        /// Also show the possible calls: calls by a name that more than one function has
        #[arg(long)]
        possible: bool,
        /// The most uses to show, from 1 to 1000
        #[arg(long, default_value_t = mcp::DEFAULT_USES as u64, value_parser = clap::value_parser!(u64).range(1..=mcp::MAX_USES as u64))]
        limit: u64,
        /// Print the uses as the JSON list envelope (items, total, next_offset), in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Send the index of the checkout to Kairos as the index of its HEAD commit, for example from CI. The tree must have no changes that are not committed
    Upload {
        #[command(flatten)]
        root: RootArg,
        #[command(flatten)]
        remote: base::RemoteArgs,
        /// The branch, the pull request or the tag of the index, for example main, pull/12 or v1.0.0. Default: the branch of the checkout
        #[arg(long = "ref", value_name = "REF")]
        r#ref: Option<String>,
        /// Print the JSON of the stored index, in place of the text
        #[arg(long)]
        json: bool,
    },
    /// Serve the code tools of the index to an agent: MCP over stdio
    Mcp {
        #[command(flatten)]
        root: RootArg,
    },
}

/// A kind of repeated code, as a CLI value.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum KindArg {
    Exact,
    Near,
    SameIdea,
}

#[derive(clap::Args)]
pub struct RootArg {
    /// The root of the checkout. Default: the root of the git repository of the current folder
    #[arg(long)]
    root: Option<PathBuf>,
}

impl IndexCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            IndexCommand::Build { root, json } => build(&resolve_root(root)?, json),
            IndexCommand::Update {
                root,
                rust_edges,
                link_only,
                max_changed,
                remote,
                json,
            } => {
                let root = resolve_root(root)?;
                let mut run = report::Run::new("update", json);
                update(&root, rust_edges, link_only, max_changed, &remote, &mut run).await?;
                run.finish()
            }
            IndexCommand::Status { root, json } => status(&resolve_root(root)?, json),
            IndexCommand::Duplicates {
                root,
                kind,
                min_lines,
                tests,
                under,
                limit,
                json,
            } => {
                let root = resolve_root(root)?;
                let kinds = match kind {
                    Some(KindArg::Exact) => vec![DuplicateKind::Exact],
                    Some(KindArg::Near) => vec![DuplicateKind::Near],
                    Some(KindArg::SameIdea) => vec![DuplicateKind::SameIdea],
                    None => DuplicateKind::ALL.to_vec(),
                };
                let options = DuplicateOptions {
                    kinds,
                    min_lines,
                    include_tests: tests,
                    under,
                    ..DuplicateOptions::default()
                };
                duplicates(&root, &options, limit as usize, json)
            }
            IndexCommand::References {
                root,
                symbol,
                file,
                line,
                possible,
                limit,
                json,
            } => references(
                &resolve_root(root)?,
                &symbol,
                file.as_deref(),
                line,
                possible,
                limit as usize,
                json,
            ),
            IndexCommand::Upload {
                root,
                remote,
                r#ref,
                json,
            } => upload::upload(&resolve_root(root)?, &remote, r#ref, json).await,
            IndexCommand::Mcp { root } => Ok(mcp::serve(resolve_root(root)?).await?),
        }
    }
}

/// The root of the checkout: `--root`, else the top of the git repository
/// of the current folder, else the current folder.
fn resolve_root(arg: RootArg) -> Result<PathBuf, IndexCommandError> {
    let root = match arg.root {
        Some(root) => root,
        None => {
            let here = std::env::current_dir().map_err(|source| IndexCommandError::Write {
                path: PathBuf::from("."),
                source,
            })?;
            git_output(&here, &["rev-parse", "--show-toplevel"])
                .map(PathBuf::from)
                .unwrap_or(here)
        }
    };
    root.canonicalize()
        .map_err(|_| IndexCommandError::NoRoot(root))
}

pub(super) fn db_path(root: &Path) -> PathBuf {
    root.join(kairos_index::INDEX_FILE)
}

/// The trimmed stdout of a git command that succeeded.
pub(crate) fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A git command in `dir` for a test, with a fixed identity.
#[cfg(test)]
pub(crate) fn test_git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Robin")
        .env("GIT_AUTHOR_EMAIL", "robin@example.test")
        .env("GIT_COMMITTER_NAME", "Robin")
        .env("GIT_COMMITTER_EMAIL", "robin@example.test")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Make git ignore the index: if git does not ignore `.kairos/index.db`,
/// add it to `.git/info/exclude`. Returns the file that was changed. A root
/// that is not in a git repository needs nothing.
fn ensure_ignored(root: &Path) -> Result<Option<PathBuf>, IndexCommandError> {
    let Some(exclude) = git_output(root, &["rev-parse", "--git-path", "info/exclude"]) else {
        return Ok(None);
    };
    let ignored = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", kairos_index::INDEX_FILE])
        .status()
        .map_err(|e| IndexCommandError::Git {
            path: root.to_path_buf(),
            message: e.to_string(),
        })?;
    if ignored.success() {
        return Ok(None);
    }
    let exclude = root.join(exclude);
    let write = |source| IndexCommandError::Write {
        path: exclude.clone(),
        source,
    };
    if let Some(folder) = exclude.parent() {
        std::fs::create_dir_all(folder).map_err(write)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .map_err(write)?;
    // The pattern also covers the journal files of SQLite.
    writeln!(
        file,
        "\n# The code index of `kairos index` (COLLIERY-T-1852).\n/{}*",
        kairos_index::INDEX_FILE
    )
    .map_err(write)?;
    Ok(Some(exclude))
}

/// The path of the index file, with its folder made and `.kairos/` ignored
/// by git. The exclude file is given back when this call added the line,
/// so that the caller can say so: the MCP server must not print
/// (KAIROS-T-0348).
pub(super) fn prepare_quiet(root: &Path) -> Result<(PathBuf, Option<PathBuf>), IndexCommandError> {
    let db = db_path(root);
    if let Some(folder) = db.parent() {
        std::fs::create_dir_all(folder).map_err(|source| IndexCommandError::Write {
            path: folder.to_path_buf(),
            source,
        })?;
    }
    let exclude = ensure_ignored(root)?;
    Ok((db, exclude))
}

/// The lock of the index of a checkout, `.kairos/index.db.lock`: one process
/// at a time fetches, builds or updates the index. Until KAIROS-T-0357, 2
/// agents that opened one checkout at the same time wrote the same download
/// file and the same tables, and one got "disk I/O error" or "no such
/// table". The lock waits for the other process; it ends when the file is
/// dropped.
pub(super) fn lock_index(db: &Path) -> Result<std::fs::File, IndexCommandError> {
    let path = db.with_extension("db.lock");
    let write = |source| IndexCommandError::Write {
        path: path.clone(),
        source,
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(write)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(write)?;
    file.lock().map_err(write)?;
    Ok(file)
}

/// `kairos index build` and `kairos index --full`: the text as the build
/// goes, or one JSON object at its end (KAIROS-T-0654).
fn build(root: &Path, json: bool) -> Result<(), CliError> {
    let mut run = report::Run::new("build", json);
    let (db, exclude) = prepare_quiet(root)?;
    run.excluded(exclude);
    let _lock = lock_index(&db)?;
    run.index(&db);
    let started = Instant::now();
    let report = kairos_index::build_structure_with(root, &db, &BuildOptions::default())
        .map_err(IndexCommandError::from)?;
    run.structure(&report, started.elapsed());
    run.summaries(summaries::run(root, &db, None)?);
    run.finish()
}

/// The local index of the checkout, before an update.
enum Local {
    Missing,
    /// An index that this CLI cannot update: another schema version.
    Old,
    /// The count of the files of the tree that differ from it.
    Current(usize),
}

async fn update(
    root: &Path,
    rust_edges: bool,
    link_only: bool,
    max_changed: Option<usize>,
    remote: &base::RemoteArgs,
    run: &mut report::Run,
) -> Result<(), IndexCommandError> {
    let db = db_path(root);
    // The exclude line before the lock file: else git counts the lock file
    // as a change of the tree when the CLI compares it with a base index
    // (KAIROS-T-0357).
    let (_, exclude) = prepare_quiet(root)?;
    run.excluded(exclude);
    let _lock = lock_index(&db)?;
    let local = if !db.is_file() {
        Local::Missing
    } else {
        match kairos_index::changed_files(root, &db) {
            Ok(changed) => Local::Current(changed.len()),
            Err(_) => Local::Old,
        }
    };
    let local_changed = match local {
        Local::Current(n) => Some(n),
        Local::Missing | Local::Old => None,
    };

    // Where the update starts: the local index, or the base from Kairos.
    let mut download = None;
    let found = base::find(root, remote).await;
    // KAIROS-T-0343: a repository on a hosted provider makes its summaries
    // on Kairos. The CLI then links the pool and runs no model, whatever
    // KAIROS_INDEX_SUMMARIZE says. With no answer from Kairos, the CLI
    // cannot know, and it runs as before.
    let hosted = matches!(&found, base::Found::Base(b) if b.hosted);
    let limit = limit_of(max_changed, !(link_only || hosted));
    match (found, &local) {
        (base::Found::Base(b), Local::Current(n)) if *n <= b.changed => {
            if let Some(limit) = limit
                && *n > limit
            {
                return Err(too_far(root, &b, *n, limit, true, error::Caller::Cli));
            }
            run.say(&format!(
                "Kairos: the local index is nearer to the tree than the index of {} \
                 ({n} changed files, against {}). The CLI updates the local index.",
                b.short(),
                b.changed
            ));
            let taken = take_base_summaries(&db, &b).await;
            if let Some(line) = &taken {
                run.say(line);
            }
            run.start(report::Start {
                from: "local",
                base: Some(report::base(&b)),
                local_changed,
                no_base: None,
                summaries_taken: taken,
            });
        }
        (base::Found::Base(b), _) => {
            if let Some(limit) = limit
                && b.changed > limit
            {
                return Err(too_far(
                    root,
                    &b,
                    b.changed,
                    limit,
                    false,
                    error::Caller::Cli,
                ));
            }
            download = Some(b);
        }
        (found, Local::Current(_)) => {
            let why = reason(&found);
            run.say(&format!(
                "Kairos: no base index, because {why}. The CLI updates the local index."
            ));
            run.start(report::Start {
                from: "local",
                base: None,
                local_changed,
                no_base: Some(why),
                summaries_taken: None,
            });
        }
        (found, Local::Missing | Local::Old) => {
            return Err(IndexCommandError::NoStart {
                path: db,
                why: reason(&found),
            });
        }
    }

    if let Some(b) = download {
        let bytes = b
            .download()
            .await
            .map_err(|why| IndexCommandError::NoStart {
                path: db.clone(),
                why: format!("the download of the index of {} failed ({why})", b.short()),
            })?;
        if let Some(warning) = install_quiet(&db, &bytes, local_changed.is_some())? {
            run.warn(warning);
        }
        run.say(&format!(
            "Kairos: downloaded the index of {}, {} files changed since it.",
            b.short(),
            b.changed
        ));
        run.start(report::Start {
            from: "base",
            base: Some(report::base(&b)),
            local_changed,
            no_base: None,
            summaries_taken: None,
        });
    }
    run.index(&db);
    let started = Instant::now();
    let options = UpdateOptions {
        rust_edges,
        ..UpdateOptions::default()
    };
    let report = kairos_index::update_structure(root, &db, &options)?;
    run.structure(&report, started.elapsed());
    let no_model = if hosted {
        Some(summaries::HOSTED)
    } else if link_only {
        Some(summaries::LINK_ONLY)
    } else {
        None
    };
    run.summaries(summaries::run(root, &db, no_model)?);
    Ok(())
}

/// Take the summaries of the base index `b` into the local index at `db`,
/// when the base has more summaries than the local index links
/// (KAIROS-T-0355). The local index is nearer to the tree, so it keeps its
/// structure: the CLI downloads the base, copies its pool into the local
/// index, and the link step that follows links the new summaries. The
/// local index records the commit and the count of the pool that it took,
/// so the next update does not download the same pool again.
///
/// The line to show: what the CLI took, or why it took nothing after a
/// try. `None` when the local index has the summaries already.
pub(super) async fn take_base_summaries(db: &Path, b: &base::Base) -> Option<String> {
    let linked = kairos_index::store::keys_of(db).ok()?.len();
    let taken = format!("{}:{}", b.commit, b.summaries);
    if b.summaries <= linked
        || kairos_index::store::taken_base_pool(db).ok().flatten() == Some(taken.clone())
    {
        return None;
    }
    let bytes = match b.download().await {
        Ok(bytes) => bytes,
        Err(why) => {
            return Some(format!(
                "Kairos: the summaries of the index of {} are not taken: the download failed \
                 ({why}).",
                b.short()
            ));
        }
    };
    let db = db.to_path_buf();
    let short = b.short().to_string();
    let summaries = b.summaries;
    let copied = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let download = db.with_extension("db.base");
        std::fs::write(&download, &bytes).map_err(|e| e.to_string())?;
        let copied = Index::open(&download)
            .map_err(|e| e.to_string())
            .and_then(|_| kairos_index::store::copy_pool(&download, &db).map_err(|e| e.to_string()))
            .and_then(|()| {
                kairos_index::store::set_taken_base_pool(&db, &taken).map_err(|e| e.to_string())
            });
        let _ = std::fs::remove_file(&download);
        copied
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    Some(match copied {
        Ok(()) => format!(
            "Kairos: the index of {short} has {summaries} summaries, and the local index links \
             {linked}. The update took the summaries from Kairos and keeps the local structure."
        ),
        Err(why) => format!("Kairos: the summaries of the index of {short} are not taken: {why}"),
    })
}

/// Why the CLI got no base index from Kairos, as a part of a sentence.
pub(super) fn reason(found: &base::Found) -> String {
    match found {
        base::Found::Base(b) => format!("the index of {} is not used", b.short()),
        base::Found::NotSet => {
            "no Kairos deployment is set (KAIROS_URL, a [[project]] of config.toml or `kairos login`)"
                .to_string()
        }
        base::Found::Unreachable { url, why } => {
            format!("the CLI could not reach Kairos at {url} ({why})")
        }
        base::Found::NoBase(why) => why.clone(),
    }
}

/// The refusal: `changed` files, more than `limit`, changed since the
/// index that the update starts from: the base, or the local index when it
/// is nearer (`local`, KAIROS-T-0515). The text says what to do
/// (KAIROS-T-0517): pull when the checkout is only behind, else rebase or
/// build again; and the limit to give.
pub(super) fn too_far(
    root: &Path,
    b: &base::Base,
    changed: usize,
    limit: usize,
    local: bool,
    caller: error::Caller,
) -> IndexCommandError {
    let commit = b.short().to_string();
    let since = if local {
        error::Since::Local { changed, commit }
    } else if b.distance > 0 {
        error::Since::Below {
            changed,
            commit,
            distance: b.distance,
            from: base::short(&b.from).to_string(),
        }
    } else {
        error::Since::Base { changed, commit }
    };
    let (stop, raise) = match caller {
        error::Caller::Cli => (error::Stop::Cli(limit), error::Raise::Cli(changed)),
        error::Caller::Tool => (error::Stop::Tool(limit), error::Raise::Tool(changed)),
    };
    let advice = match b.behind {
        Some(commits) => error::Advice::Behind {
            commits,
            branch: b.branch.clone(),
            raise,
        },
        None => error::Advice::Rebase {
            branch: b.branch.clone(),
            estimate: base::full_build_estimate(base::file_count(root), b.summaries),
            raise,
        },
    };
    IndexCommandError::TooFar(Box::new(error::TooFar {
        since,
        stop,
        advice,
    }))
}

/// Put the downloaded index at `db`. With `keep_pool`, the summaries of the
/// local index go into it first, so that none is lost. The warning is given
/// back, not printed (KAIROS-T-0348): the MCP server must not print.
pub(super) fn install_quiet(
    db: &Path,
    bytes: &[u8],
    keep_pool: bool,
) -> Result<Option<String>, IndexCommandError> {
    let download = db.with_extension("db.download");
    let write = |source| IndexCommandError::Write {
        path: download.clone(),
        source,
    };
    std::fs::write(&download, bytes).map_err(write)?;
    if let Err(e) = Index::open(&download) {
        let _ = std::fs::remove_file(&download);
        return Err(e.into());
    }
    let mut warning = None;
    if keep_pool && let Err(e) = kairos_index::store::copy_pool(db, &download) {
        warning = Some(format!(
            "The summaries of the local index are not kept: {e}"
        ));
    }
    std::fs::rename(&download, db).map_err(|source| IndexCommandError::Write {
        path: db.to_path_buf(),
        source,
    })?;
    Ok(warning)
}

fn print_structure(report: &BuildReport, elapsed: Duration) {
    let e = &report.edges;
    let kept = if e.kept_scip > 0 {
        format!(" ({} kept from the index)", e.kept_scip)
    } else {
        String::new()
    };
    let cached = if report.cached_files > 0 {
        format!(", {} of them from the parse cache", report.cached_files)
    } else {
        String::new()
    };
    println!(
        "Structure: {} files ({} parsed{cached}), {} symbols, {} edges{kept}, in {:.1} s.",
        report.files,
        report.parsed_files,
        report.symbols,
        e.total(),
        elapsed.as_secs_f64()
    );
    match &report.scip {
        Some(scip) => println!(
            "Rust edges: from rust-analyzer scip, {} certain and {} external, in {:.1} s.",
            e.certain_scip,
            e.external_scip,
            scip.elapsed.as_secs_f64()
        ),
        None if e.kept_scip > 0 || e.pending > 0 => println!(
            "Rust edges: {} kept from the index. {} edges are by name until the next SCIP run \
             (`kairos index update --rust-edges`).",
            e.kept_scip, e.pending
        ),
        None => {}
    }
    for run in report.scip_runs.iter().filter(|r| r.language != "rust") {
        println!(
            "{} edges: from SCIP, {} files, in {:.1} s.",
            run.language,
            run.documents,
            run.elapsed.as_secs_f64()
        );
    }
    for w in &report.scip_warnings {
        println!(
            "Warning: no SCIP index for {}: {}. Its edges are by name.",
            w.language, w.reason
        );
    }
    println!(
        "Token vectors: {} made, {} reused from the index.",
        report.token_vectors.made, report.token_vectors.reused
    );
}

fn print_summaries(outcome: summaries::Outcome) {
    let linked =
        |r: &kairos_index::SummaryReport| r.symbols.reused + r.files.reused + r.modules.reused;
    match outcome {
        summaries::Outcome::Linked(r) => println!(
            "Summaries: {} linked from the pool. No model ran.",
            linked(&r)
        ),
        summaries::Outcome::NotMade(r, why) => println!(
            "Summaries: {} linked from the pool. Not made: {}, {} and {}. {why}",
            linked(&r),
            mcp::plural(r.symbols.left, "symbol", "symbols"),
            mcp::plural(r.files.left, "file", "files"),
            mcp::plural(r.modules.left, "module", "modules"),
        ),
        #[cfg(feature = "llama")]
        summaries::Outcome::Made(r, elapsed) => {
            println!(
                "Summaries: {} symbols, {} files and {} modules made, {} reused, in {:.1} s.",
                r.symbols.summarized,
                r.files.summarized,
                r.modules.summarized,
                r.symbols.reused + r.files.reused + r.modules.reused,
                elapsed.as_secs_f64()
            );
            // KAIROS-T-0495: a failed summary does not stop the build.
            if let Some(failure) = &r.failure {
                println!(
                    "Missing summaries: {}. The next build makes them. First error: {}: {}",
                    r.missing(),
                    failure.name,
                    failure.message
                );
            }
        }
    }
}

fn status(root: &Path, json: bool) -> Result<(), CliError> {
    let db = db_path(root);
    if !db.is_file() {
        return Err(IndexCommandError::NoIndex(db).into());
    }
    let c = Index::open(&db)
        .and_then(|index| index.counts())
        .map_err(IndexCommandError::from)?;
    if json {
        return print_json(&report::status(&db, &c));
    }
    println!("Index: {}", db.display());
    println!("Files: {} ({} parsed)", c.files, c.parsed_files);
    println!("Symbols: {}", c.symbols);
    println!(
        "Edges: {} (certain {}, possible {}, external {}; {} by name until the next SCIP run)",
        c.edges, c.certain, c.possible, c.external, c.pending
    );
    println!(
        "Summaries: {} (symbols {} of {}, files {}, modules {}; {} in the pool)",
        c.summaries(),
        c.symbol_summaries,
        c.summarizable,
        c.file_summaries,
        c.module_summaries,
        c.pool
    );
    if let Some(model) = &c.summary_model {
        println!("Summaries model: {model}");
    }
    if let Some(model) = &c.vector_model {
        println!("Vectors: {model}");
    }
    Ok(())
}

fn duplicates(
    root: &Path,
    options: &DuplicateOptions,
    limit: usize,
    json: bool,
) -> Result<(), CliError> {
    let index = open(root)?;
    if json {
        let found = index.duplicates(options).map_err(IndexCommandError::from)?;
        return print_json(&report::duplicates(&found, options, limit));
    }
    let text = mcp::duplicates_text(&index, options, limit).map_err(IndexCommandError::from)?;
    println!("{text}");
    Ok(())
}

/// The index of the checkout, or the error that no index is there.
fn open(root: &Path) -> Result<Index, IndexCommandError> {
    let db = db_path(root);
    if !db.is_file() {
        return Err(IndexCommandError::NoIndex(db));
    }
    Ok(Index::open(&db)?)
}

/// `kairos index references` (KAIROS-T-0419): the text of the MCP tool.
fn references(
    root: &Path,
    symbol: &str,
    file: Option<&str>,
    line: Option<u32>,
    possible: bool,
    limit: usize,
    json: bool,
) -> Result<(), CliError> {
    let index = open(root)?;
    // KAIROS-T-0703: an ADR short code gives the places that cite it.
    if kairos_index::is_adr_code(symbol) {
        let extra = [
            ("file", file.is_some()),
            ("line", line.is_some()),
            ("possible", possible),
        ]
        .into_iter()
        .find(|(_, given)| *given);
        if let Some((field, _)) = extra {
            return Err(
                IndexCommandError::Symbol(Box::new(error::ToolError::AdrCodeArgument {
                    symbol: symbol.to_string(),
                    field: field.to_string(),
                    others: "`--file`, `--line` or `--possible`".to_string(),
                }))
                .into(),
            );
        }
        if json {
            let found =
                report::citations(&index, symbol, limit).map_err(IndexCommandError::from)?;
            return print_json(&found);
        }
        let text = mcp::citations_text(&index, symbol, limit).map_err(IndexCommandError::from)?;
        println!("{text}");
        return Ok(());
    }
    let s = mcp::find(&index, symbol, file, line, mcp::SYMBOL)
        .map_err(|e| IndexCommandError::Symbol(Box::new(e)))?;
    if json {
        let found =
            report::references(&index, &s, possible, limit).map_err(IndexCommandError::from)?;
        return print_json(&found);
    }
    let text =
        mcp::references_text(&index, &s, possible, limit).map_err(IndexCommandError::from)?;
    println!("{text}");
    Ok(())
}

/// The summarizer of a build or an update.
mod summaries {
    use std::path::Path;

    use super::IndexCommandError;

    use kairos_index::{SummarizeOptions, SummaryReport};

    pub enum Outcome {
        /// Each key was in the pool: the link pass did all, with no model.
        Linked(SummaryReport),
        /// The link pass, and why no model made the summaries that it left.
        NotMade(SummaryReport, String),
        #[cfg(feature = "llama")]
        Made(SummaryReport, std::time::Duration),
    }

    /// Why `--link-only` made no summaries.
    pub const LINK_ONLY: &str =
        "The update ran with --link-only. Run `kairos index update` to make them.";

    /// Why a hosted repository made no summaries (KAIROS-T-0343).
    pub const HOSTED: &str = "This repository makes its summaries on Kairos, with the hosted \
                              provider of the organization. The next push updates them.";

    /// Link each key that the pool has, with no model (COLLIERY-T-1854). The
    /// summarizer runs only if a key is not in the pool, and not when
    /// `no_model` gives the reason that no model runs: `--link-only`
    /// (COLLIERY-T-2529) or a hosted repository (KAIROS-T-0343).
    pub fn run(
        root: &Path,
        db: &Path,
        no_model: Option<&str>,
    ) -> Result<Outcome, IndexCommandError> {
        let linked = kairos_index::link(root, db, &SummarizeOptions::default())?;
        if linked.symbols.left + linked.files.left + linked.modules.left == 0 {
            return Ok(Outcome::Linked(linked));
        }
        if let Some(why) = no_model {
            return Ok(Outcome::NotMade(linked, why.into()));
        }
        summarize(root, db, linked)
    }

    #[cfg(not(feature = "llama"))]
    fn summarize(
        _root: &Path,
        _db: &Path,
        linked: SummaryReport,
    ) -> Result<Outcome, IndexCommandError> {
        Ok(Outcome::NotMade(
            linked,
            "This kairos binary has no summarizer. Build it with the feature `llama`.".into(),
        ))
    }

    /// The local vector model: `KAIROS_EMBED_CACHE`, else
    /// `~/.cache/kairos-index/embed`. The first run downloads it there.
    #[cfg(feature = "vectors")]
    pub fn local_embedder() -> Result<kairos_embed::local::LocalProvider, String> {
        use kairos_embed::local::{LocalConfig, LocalProvider};
        let cache = std::env::var_os("KAIROS_EMBED_CACHE")
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".cache/kairos-index/embed"))
            })
            .ok_or("HOME and KAIROS_EMBED_CACHE are not set.")?;
        LocalProvider::new(&LocalConfig {
            cache_dir: cache,
            allow_download: true,
        })
        .map_err(|e| format!("The vector model did not start: {e}."))
    }

    #[cfg(feature = "llama")]
    fn summarize(
        root: &Path,
        db: &Path,
        linked: SummaryReport,
    ) -> Result<Outcome, IndexCommandError> {
        use kairos_index::{LlamaModelFile, MODEL_FILE_NAME, model_path};
        let Some(model) = model_path().filter(|p| p.is_file()) else {
            return Ok(Outcome::NotMade(
                linked,
                format!(
                    "The model file is not on disk. Put {MODEL_FILE_NAME} in \
                     ~/.cache/kairos-index/models/, or set KAIROS_INDEX_MODEL."
                ),
            ));
        };
        let embedder = match local_embedder() {
            Ok(embedder) => embedder,
            Err(why) => return Ok(Outcome::NotMade(linked, why)),
        };
        let started = std::time::Instant::now();
        let model = LlamaModelFile::load(&model)?;
        let mut summarizer = model.summarizer()?;
        let report = kairos_index::summarize(
            root,
            db,
            &mut summarizer,
            &embedder,
            &SummarizeOptions::default(),
        )?;
        Ok(Outcome::Made(report, started.elapsed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(clap::Parser)]
    struct Kairos {
        #[command(subcommand)]
        command: Top,
    }

    #[derive(clap::Subcommand)]
    enum Top {
        Index(IndexArgs),
    }

    fn parse(args: &[&str]) -> Result<IndexArgs, clap::Error> {
        use clap::Parser;
        Kairos::try_parse_from(args).map(|k| match k.command {
            Top::Index(args) => args,
        })
    }

    #[test]
    fn full_is_a_flag_of_index_and_not_of_a_subcommand() {
        let args = parse(&["kairos", "index", "--full"]).expect("--full");
        assert!(args.full && args.command.is_none());
        let args = parse(&[
            "kairos",
            "index",
            "update",
            "--max-changed",
            "5",
            "--repository",
            "kairos",
        ])
        .expect("update");
        match args.command {
            Some(IndexCommand::Update {
                max_changed,
                remote,
                ..
            }) => {
                assert_eq!(max_changed, Some(5));
                assert_eq!(remote.repository.as_deref(), Some("kairos"));
            }
            _ => panic!("not update"),
        }
        let default = parse(&["kairos", "index", "update"]).expect("update");
        assert!(matches!(
            default.command,
            Some(IndexCommand::Update {
                max_changed: None,
                ..
            })
        ));
        assert!(parse(&["kairos", "index", "--full", "update"]).is_err());
        assert!(parse(&["kairos", "index", "update", "--full"]).is_err());
        assert!(parse(&["kairos", "index", "update", "--depth", "3"]).is_err());
    }

    /// KAIROS-T-0654: each command that prints a result takes `--json`.
    #[test]
    fn json_is_a_flag_of_each_command_with_a_result() {
        let args = parse(&["kairos", "index", "--full", "--json"]).expect("--full --json");
        assert!(args.full && args.json);
        for command in [
            &["build"][..],
            &["update"],
            &["status"],
            &["duplicates"],
            &["references", "Queue"],
        ] {
            let mut argv = vec!["kairos", "index"];
            argv.extend_from_slice(command);
            argv.push("--json");
            let args = parse(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            let json = match args.command {
                Some(IndexCommand::Build { json, .. })
                | Some(IndexCommand::Update { json, .. })
                | Some(IndexCommand::Status { json, .. })
                | Some(IndexCommand::Duplicates { json, .. })
                | Some(IndexCommand::References { json, .. }) => json,
                _ => panic!("{argv:?}: another command"),
            };
            assert!(json, "{argv:?}");
        }
        assert!(parse(&["kairos", "index", "mcp", "--json"]).is_err());
        assert!(parse(&["kairos", "index", "--json", "status"]).is_err());
    }

    /// KAIROS-T-0515: an update that runs no model has no default limit.
    #[test]
    fn the_default_limit_applies_only_when_a_model_runs() {
        assert_eq!(limit_of(None, true), Some(DEFAULT_MAX_CHANGED));
        assert_eq!(limit_of(None, false), None);
        assert_eq!(limit_of(Some(7), false), Some(7));
        assert_eq!(limit_of(Some(900), true), Some(900));
    }

    #[test]
    fn link_only_is_a_flag_of_update() {
        let args = parse(&["kairos", "index", "update", "--link-only"]).expect("update");
        assert!(matches!(
            args.command,
            Some(IndexCommand::Update {
                link_only: true,
                ..
            })
        ));
        let args = parse(&["kairos", "index", "update"]).expect("update");
        assert!(matches!(
            args.command,
            Some(IndexCommand::Update {
                link_only: false,
                ..
            })
        ));
        assert!(parse(&["kairos", "index", "build", "--link-only"]).is_err());
        assert!(parse(&["kairos", "index", "--link-only"]).is_err());
    }

    #[test]
    fn link_only_runs_no_model() {
        let dir = tempfile::tempdir().expect("a folder");
        let root = dir.path();
        std::fs::write(
            root.join("tool.py"),
            "def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n    return a + b\n",
        )
        .expect("write");
        let db = root.join("index.db");
        kairos_index::build_structure(root, &db).expect("structure");
        match summaries::run(root, &db, Some(summaries::LINK_ONLY)).expect("run") {
            summaries::Outcome::NotMade(report, why) => {
                assert_eq!(why, summaries::LINK_ONLY);
                assert!(report.symbols.left > 0, "the symbol has no summary");
            }
            _ => panic!("a model ran, or each summary was in the pool"),
        }
        // KAIROS-T-0343: a hosted repository gives its own reason.
        match summaries::run(root, &db, Some(summaries::HOSTED)).expect("run") {
            summaries::Outcome::NotMade(_, why) => assert_eq!(why, summaries::HOSTED),
            _ => panic!("a model ran, or each summary was in the pool"),
        }
    }

    /// KAIROS-T-0357: 8 processes build the index of one checkout at the
    /// same time, as agents that open one shared clone do. Each build waits
    /// for the lock of the index, and each succeeds.
    #[test]
    fn builds_of_one_checkout_at_the_same_time_all_succeed() {
        let dir = tempfile::tempdir().expect("a folder");
        let root = dir.path().to_path_buf();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["init", "-q"])
                .status()
                .expect("git")
                .success()
        );
        for n in 0..20 {
            std::fs::write(
                root.join(format!("m{n}.py")),
                format!("def f{n}(x):\n    return g{n}(x)\n\ndef g{n}(x):\n    return x\n"),
            )
            .expect("a file");
        }
        let builds: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                std::thread::spawn(move || build(&root, false).map_err(|e| e.to_string()))
            })
            .collect();
        for b in builds {
            b.join().expect("the thread").expect("the build");
        }
        let index = Index::open(&db_path(&root)).expect("the index opens");
        assert_eq!(index.counts().expect("counts").symbols, 40);
    }

    #[test]
    fn a_build_makes_git_ignore_the_index() {
        let dir = tempfile::tempdir().expect("a folder");
        let root = dir.path();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["init", "-q"])
                .status()
                .expect("git")
                .success()
        );
        let changed = ensure_ignored(root).expect("ignore");
        assert_eq!(changed, Some(root.join(".git/info/exclude")));
        // A second time, git ignores it already.
        assert_eq!(ensure_ignored(root).expect("ignore"), None);
        let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).expect("read");
        assert_eq!(exclude.matches("/.kairos/index.db*").count(), 1);
        assert!(!root.join(".gitignore").exists());
    }

    #[test]
    fn a_folder_with_no_git_needs_nothing() {
        let dir = tempfile::tempdir().expect("a folder");
        // A temporary folder is not in a git repository on a normal machine.
        if git_output(dir.path(), &["rev-parse", "--git-dir"]).is_none() {
            assert_eq!(ensure_ignored(dir.path()).expect("ignore"), None);
        }
    }
}
