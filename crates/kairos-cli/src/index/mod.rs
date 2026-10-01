//! `kairos index`: the code index of a checkout (COLLIERY-I-0264,
//! COLLIERY-T-1852).
//!
//! - `kairos index build` builds the structure (files, symbols, call edges)
//!   with a SCIP run for the Rust edges, then the summaries.
//! - `kairos index update` builds the structure again from the tree and
//!   summarizes only what changed. It keeps the Rust edges of the index for
//!   each function whose code did not change, and runs no SCIP unless
//!   `--rust-edges` is given.
//! - `kairos index status` gives the counts.
//! - `kairos index mcp` serves the code tools to an agent over stdio
//!   ([`mcp`]).
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
mod error;
mod mcp;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::Subcommand;
use kairos_embed::EmbeddingProvider;
use kairos_index::{BuildOptions, BuildReport, Index, UpdateOptions};

pub use error::IndexCommandError;

use crate::error::CliError;

#[derive(Subcommand)]
pub enum IndexCommand {
    /// Build the index of the checkout: the structure, the call edges and the summaries
    Build {
        #[command(flatten)]
        root: RootArg,
    },
    /// Update the index for the changes in the checkout, also those not committed
    Update {
        #[command(flatten)]
        root: RootArg,
        /// Run rust-analyzer again for the Rust edges of the changed code
        #[arg(long)]
        rust_edges: bool,
    },
    /// Show the counts of the index: files, symbols, edges and summaries
    Status {
        #[command(flatten)]
        root: RootArg,
    },
    /// Serve the code tools of the index to an agent: MCP over stdio
    Mcp {
        #[command(flatten)]
        root: RootArg,
    },
}

#[derive(clap::Args)]
pub struct RootArg {
    /// The root of the checkout. Default: the root of the git repository of the current folder
    #[arg(long)]
    root: Option<PathBuf>,
}

impl IndexCommand {
    pub async fn run(self) -> Result<(), CliError> {
        let result = match self {
            IndexCommand::Build { root } => resolve_root(root).and_then(|root| build(&root)),
            IndexCommand::Update { root, rust_edges } => {
                resolve_root(root).and_then(|root| update(&root, rust_edges))
            }
            IndexCommand::Status { root } => resolve_root(root).and_then(|root| status(&root)),
            IndexCommand::Mcp { root } => match resolve_root(root) {
                Ok(root) => mcp::serve(root).await,
                Err(e) => Err(e),
            },
        };
        Ok(result?)
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

fn db_path(root: &Path) -> PathBuf {
    root.join(kairos_index::INDEX_FILE)
}

/// The trimmed stdout of a git command that succeeded.
fn git_output(root: &Path, args: &[&str]) -> Option<String> {
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

fn prepare(root: &Path) -> Result<PathBuf, IndexCommandError> {
    let db = db_path(root);
    if let Some(folder) = db.parent() {
        std::fs::create_dir_all(folder).map_err(|source| IndexCommandError::Write {
            path: folder.to_path_buf(),
            source,
        })?;
    }
    if let Some(exclude) = ensure_ignored(root)? {
        println!(
            "Git ignores {} now: the CLI added it to {}.",
            kairos_index::INDEX_FILE,
            exclude.display()
        );
    }
    Ok(db)
}

fn build(root: &Path) -> Result<(), IndexCommandError> {
    let db = prepare(root)?;
    println!("Index: {}", db.display());
    let started = Instant::now();
    let report = kairos_index::build_structure_with(root, &db, &BuildOptions::default())?;
    print_structure(&report, started);
    print_summaries(summaries::run(root, &db)?);
    Ok(())
}

fn update(root: &Path, rust_edges: bool) -> Result<(), IndexCommandError> {
    let db = db_path(root);
    if !db.is_file() {
        return Err(IndexCommandError::NoIndex(db));
    }
    prepare(root)?;
    println!("Index: {}", db.display());
    let started = Instant::now();
    let options = UpdateOptions {
        rust_edges,
        ..UpdateOptions::default()
    };
    let report = kairos_index::update_structure(root, &db, &options)?;
    print_structure(&report, started);
    print_summaries(summaries::run(root, &db)?);
    Ok(())
}

fn print_structure(report: &BuildReport, started: Instant) {
    let e = &report.edges;
    let edges =
        e.certain_scip + e.external_scip + e.certain_name + e.possible_name + e.external_name;
    println!(
        "Structure: {} files ({} parsed), {} symbols, {} edges, in {:.1} s.",
        report.files,
        report.parsed_files,
        report.symbols,
        edges,
        started.elapsed().as_secs_f64()
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
}

fn print_summaries(outcome: summaries::Outcome) {
    match outcome {
        summaries::Outcome::NotMade(why) => println!("Summaries: not made. {why}"),
        #[cfg(feature = "llama")]
        summaries::Outcome::Made(r, elapsed) => println!(
            "Summaries: {} symbols, {} files and {} modules made, {} reused, in {:.1} s.",
            r.symbols.summarized,
            r.files.summarized,
            r.modules.summarized,
            r.symbols.reused + r.files.reused + r.modules.reused,
            elapsed.as_secs_f64()
        ),
    }
}

fn status(root: &Path) -> Result<(), IndexCommandError> {
    let db = db_path(root);
    if !db.is_file() {
        return Err(IndexCommandError::NoIndex(db));
    }
    let c = Index::open(&db)?.counts()?;
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
    if let Some(model) = &c.vector_model {
        println!("Vectors: {model}");
    }
    Ok(())
}

/// The provider of the query vectors for the model of the pool
/// (`provider/model/dimension`), or why this binary has none.
pub(crate) fn query_embedder(model: &str) -> Result<Box<dyn EmbeddingProvider>, String> {
    let mut parts = model.splitn(3, '/');
    let (provider, name, dimension) = (parts.next(), parts.next(), parts.next());
    if let (Some("deterministic"), Some(dimension)) = (provider, dimension)
        && let Ok(dimension) = dimension.parse::<usize>()
    {
        let deterministic = kairos_embed::DeterministicProvider::new(dimension);
        if Some(deterministic.model_id().model.as_str()) == name {
            return Ok(Box::new(deterministic));
        }
    }
    #[cfg(feature = "llama")]
    if provider == Some("local") {
        let local = summaries::local_embedder()?;
        if local.model_id().model.as_str() == name.unwrap_or_default() {
            return Ok(Box::new(local));
        }
    }
    Err(format!(
        "This kairos binary has no provider for the vectors of the model {model}."
    ))
}

/// The summarizer of a build or an update.
mod summaries {
    use std::path::Path;

    use super::IndexCommandError;

    pub enum Outcome {
        /// Why the run made no summaries.
        NotMade(String),
        #[cfg(feature = "llama")]
        Made(kairos_index::SummaryReport, std::time::Duration),
    }

    #[cfg(not(feature = "llama"))]
    pub fn run(_root: &Path, _db: &Path) -> Result<Outcome, IndexCommandError> {
        Ok(Outcome::NotMade(
            "This kairos binary has no summarizer. Build it with the feature `llama`.".into(),
        ))
    }

    /// The local vector model: `KAIROS_EMBED_CACHE`, else
    /// `~/.cache/kairos-index/embed`. The first run downloads it there.
    #[cfg(feature = "llama")]
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
    pub fn run(root: &Path, db: &Path) -> Result<Outcome, IndexCommandError> {
        use kairos_index::{LlamaModelFile, MODEL_FILE_NAME, SummarizeOptions, model_path};
        let Some(model) = model_path().filter(|p| p.is_file()) else {
            return Ok(Outcome::NotMade(format!(
                "The model file is not on disk. Put {MODEL_FILE_NAME} in \
                 ~/.cache/kairos-index/models/, or set KAIROS_INDEX_MODEL."
            )));
        };
        let embedder = match local_embedder() {
            Ok(embedder) => embedder,
            Err(why) => return Ok(Outcome::NotMade(why)),
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

    #[test]
    fn the_deterministic_vectors_have_a_query_provider() {
        let provider = query_embedder("deterministic/sha256-384/384").expect("a provider");
        assert_eq!(provider.model_id().dimension, 384);
        let err = query_embedder("other/model/3").err().expect("no provider");
        assert_eq!(
            err,
            "This kairos binary has no provider for the vectors of the model other/model/3."
        );
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
