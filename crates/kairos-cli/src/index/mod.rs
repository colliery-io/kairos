//! `kairos index`: the code index of a checkout (COLLIERY-I-0264,
//! COLLIERY-T-1852).
//!
//! - `kairos index build` builds the structure (files, symbols, call edges)
//!   with a SCIP run for the Rust edges, then the summaries.
//! - `kairos index update` builds the structure again from the tree and
//!   summarizes only what changed. It keeps the Rust edges of the index for
//!   each function whose code did not change, and runs no SCIP unless
//!   `--rust-edges` is given. With `--link-only`, it runs no model: it links
//!   the summaries of the pool and makes no new ones (COLLIERY-T-2529, the
//!   background update of the plugin). It starts from the nearest base index in
//!   Kairos when the checkout has no index, or when the base is nearer to
//!   the tree than the local index ([`base`], COLLIERY-T-1854). If more
//!   files than the limit changed since the base, it builds nothing and
//!   tells the developer to rebase, or to run `kairos index --full`.
//! - `kairos index --full` builds the index again with no base: the same as
//!   `kairos index build`.
//! - `kairos index status` gives the counts.
//! - `kairos index duplicates` finds repeated code: exact copies, near copies
//!   and the same idea in other code (COLLIERY-T-1857). It gives the text of the
//!   MCP tool `duplicates`.
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
mod base;
mod error;
mod mcp;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::Subcommand;
use kairos_embed::EmbeddingProvider;
use kairos_index::{
    BuildOptions, BuildReport, DuplicateKind, DuplicateOptions, Index, UpdateOptions,
};

pub use error::IndexCommandError;

use crate::error::CliError;

/// The default limit of the files changed since the base index
/// (COLLIERY-I-0264, "The flow").
pub const DEFAULT_MAX_CHANGED: usize = 200;

/// `kairos index`: a subcommand, or `--full`.
#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true, arg_required_else_help = true)]
pub struct IndexArgs {
    /// Build the whole index of the checkout again, with no base index: the same as `kairos index build`
    #[arg(long)]
    full: bool,
    #[command(flatten)]
    root: RootArg,
    #[command(subcommand)]
    command: Option<IndexCommand>,
}

impl IndexArgs {
    pub async fn run(self) -> Result<(), CliError> {
        match self.command {
            Some(command) => command.run().await,
            None if self.full => Ok(resolve_root(self.root).and_then(|root| build(&root))?),
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
    },
    /// Update the index for the changes in the checkout, also those not committed. With no index, start from the nearest base index in Kairos
    Update {
        #[command(flatten)]
        root: RootArg,
        /// Run rust-analyzer again for the Rust edges of the changed code
        #[arg(long)]
        rust_edges: bool,
        /// Run no model: link the summaries of the pool and make no new ones
        #[arg(long)]
        link_only: bool,
        /// The most files that can change since the base index. Above it, the CLI builds nothing
        #[arg(long, default_value_t = DEFAULT_MAX_CHANGED)]
        max_changed: usize,
        #[command(flatten)]
        remote: base::RemoteArgs,
    },
    /// Show the counts of the index: files, symbols, edges and summaries
    Status {
        #[command(flatten)]
        root: RootArg,
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
        let result = match self {
            IndexCommand::Build { root } => resolve_root(root).and_then(|root| build(&root)),
            IndexCommand::Update {
                root,
                rust_edges,
                link_only,
                max_changed,
                remote,
            } => match resolve_root(root) {
                Ok(root) => update(&root, rust_edges, link_only, max_changed, &remote).await,
                Err(e) => Err(e),
            },
            IndexCommand::Status { root } => resolve_root(root).and_then(|root| status(&root)),
            IndexCommand::Duplicates {
                root,
                kind,
                min_lines,
                tests,
                under,
                limit,
            } => resolve_root(root).and_then(|root| {
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
                duplicates(&root, &options, limit as usize)
            }),
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
    print_summaries(summaries::run(root, &db, None)?);
    Ok(())
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
    max_changed: usize,
    remote: &base::RemoteArgs,
) -> Result<(), IndexCommandError> {
    let db = db_path(root);
    let local = if !db.is_file() {
        Local::Missing
    } else {
        match kairos_index::changed_files(root, &db) {
            Ok(changed) => Local::Current(changed.len()),
            Err(_) => Local::Old,
        }
    };

    // Where the update starts: the local index, or the base from Kairos.
    let mut download = None;
    let found = base::find(root, remote).await;
    // KAIROS-T-0343: a repository on a hosted provider makes its summaries
    // on Kairos. The CLI then links the pool and runs no model, whatever
    // KAIROS_INDEX_SUMMARIZE says. With no answer from Kairos, the CLI
    // cannot know, and it runs as before.
    let hosted = matches!(&found, base::Found::Base(b) if b.hosted);
    match (found, &local) {
        (base::Found::Base(b), Local::Current(n)) if *n <= b.changed => {
            if *n > max_changed {
                return Err(too_far(root, &b, max_changed));
            }
            println!(
                "Kairos: the local index is nearer to the tree than the index of {} \
                 ({n} changed files, against {}). The CLI updates the local index.",
                b.short(),
                b.changed
            );
        }
        (base::Found::Base(b), _) => {
            if b.changed > max_changed {
                return Err(too_far(root, &b, max_changed));
            }
            download = Some(b);
        }
        (found, Local::Current(_)) => println!(
            "Kairos: no base index, because {}. The CLI updates the local index.",
            reason(&found)
        ),
        (found, Local::Missing | Local::Old) => {
            return Err(IndexCommandError::NoStart {
                path: db,
                why: reason(&found),
            });
        }
    }

    prepare(root)?;
    if let Some(b) = download {
        let bytes = b
            .download()
            .await
            .map_err(|why| IndexCommandError::NoStart {
                path: db.clone(),
                why: format!("the download of the index of {} failed ({why})", b.short()),
            })?;
        install(&db, &bytes, matches!(local, Local::Current(_)))?;
        println!(
            "Kairos: downloaded the index of {}, {} files changed since it.",
            b.short(),
            b.changed
        );
    }
    println!("Index: {}", db.display());
    let started = Instant::now();
    let options = UpdateOptions {
        rust_edges,
        ..UpdateOptions::default()
    };
    let report = kairos_index::update_structure(root, &db, &options)?;
    print_structure(&report, started);
    let no_model = if hosted {
        Some(summaries::HOSTED)
    } else if link_only {
        Some(summaries::LINK_ONLY)
    } else {
        None
    };
    print_summaries(summaries::run(root, &db, no_model)?);
    Ok(())
}

/// Why the CLI got no base index from Kairos, as a part of a sentence.
fn reason(found: &base::Found) -> String {
    match found {
        base::Found::Base(b) => format!("the index of {} is not used", b.short()),
        base::Found::NotSet => {
            "no Kairos deployment is set (KAIROS_URL, .claude/kairos.local.md or `kairos login`)"
                .to_string()
        }
        base::Found::Unreachable { url, why } => {
            format!("the CLI could not reach Kairos at {url} ({why})")
        }
        base::Found::NoBase(why) => why.clone(),
    }
}

/// The rebase answer: more than `limit` files changed since the base.
fn too_far(root: &Path, b: &base::Base, limit: usize) -> IndexCommandError {
    IndexCommandError::TooFar {
        changed: b.changed,
        commit: b.short().to_string(),
        limit,
        branch: b.branch.clone(),
        estimate: base::full_build_estimate(base::file_count(root), b.summaries),
    }
}

/// Put the downloaded index at `db`. With `keep_pool`, the summaries of the
/// local index go into it first, so that none is lost.
fn install(db: &Path, bytes: &[u8], keep_pool: bool) -> Result<(), IndexCommandError> {
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
    if keep_pool && let Err(e) = kairos_index::store::copy_pool(db, &download) {
        println!("The summaries of the local index are not kept: {e}");
    }
    std::fs::rename(&download, db).map_err(|source| IndexCommandError::Write {
        path: db.to_path_buf(),
        source,
    })
}

fn print_structure(report: &BuildReport, started: Instant) {
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
) -> Result<(), IndexCommandError> {
    let db = db_path(root);
    if !db.is_file() {
        return Err(IndexCommandError::NoIndex(db));
    }
    let index = Index::open(&db)?;
    println!("{}", mcp::duplicates_text(&index, options, limit)?);
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
                assert_eq!(max_changed, 5);
                assert_eq!(remote.repository.as_deref(), Some("kairos"));
            }
            _ => panic!("not update"),
        }
        let default = parse(&["kairos", "index", "update"]).expect("update");
        assert!(matches!(
            default.command,
            Some(IndexCommand::Update {
                max_changed: DEFAULT_MAX_CHANGED,
                ..
            })
        ));
        assert!(parse(&["kairos", "index", "--full", "update"]).is_err());
        assert!(parse(&["kairos", "index", "update", "--full"]).is_err());
        assert!(parse(&["kairos", "index", "update", "--depth", "3"]).is_err());
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
