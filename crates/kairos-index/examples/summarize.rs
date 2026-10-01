//! Build the index of a tree and summarize it with the real model: a
//! measurement tool for COLLIERY-T-1850 and COLLIERY-T-1851 until
//! `kairos index` exists (COLLIERY-T-1852).
//!
//! ```text
//! cargo run --release -p kairos-index --features llama --example summarize -- \
//!     <repository> <index.sqlite> [--under <path prefix>]... [--max <symbols>]
//!     [--skip-build | --update [--rust-edges] | --structure-only]
//! ```
//!
//! `--update` updates the index from the tree: the structure is built again
//! with the SCIP edges of the index for the Rust functions that did not change
//! (or with a SCIP run, with `--rust-edges`), and only the keys that the pool
//! does not have are summarized. `--structure-only` builds the structure,
//! prints the SCIP run and stops: no model is loaded.
//!
//! The model file comes from `KAIROS_INDEX_MODEL` or the default path (see
//! `kairos_index::model_path`). The vectors use the local
//! `bge-small-en-v1.5` from `KAIROS_EMBED_CACHE`, else `target/embed-cache`
//! (`angreal dev fetch-model`). Nothing is downloaded.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use kairos_embed::local::{LocalConfig, LocalProvider};
use kairos_index::{
    Level, LlamaModelFile, SummarizeOptions, UpdateOptions, build_structure, model_path, summarize,
    update_structure,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut options = SummarizeOptions::default();
    let mut skip_build = false;
    let mut update = false;
    let mut rust_edges = false;
    let mut structure_only = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Use the structure of an earlier run, so that the peak memory
            // of this process is the memory of the summaries only.
            "--skip-build" => skip_build = true,
            "--update" => update = true,
            "--rust-edges" => rust_edges = true,
            "--structure-only" => structure_only = true,
            "--under" => options
                .under
                .push(args.next().ok_or("--under needs a value")?),
            "--max" => {
                let value = args.next().ok_or("--max needs a value")?;
                options.max_new_symbols = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--max: {value} is not a number"))?,
                );
            }
            flag if flag.starts_with("--") => return Err(format!("Unknown option {flag}.")),
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    let [root, db] = positional.as_slice() else {
        return Err("Give the repository and the index file.".into());
    };
    if rust_edges && !update {
        return Err("--rust-edges needs --update.".into());
    }
    if update && skip_build {
        return Err("--update and --skip-build cannot go together.".into());
    }
    if structure_only && (update || skip_build) {
        return Err("--structure-only cannot go with --update or --skip-build.".into());
    }

    let started = Instant::now();
    if update {
        let built = update_structure(
            root,
            db,
            &UpdateOptions {
                rust_edges,
                summarize: options.clone(),
                ..UpdateOptions::default()
            },
        )
        .map_err(|e| e.to_string())?;
        println!(
            "structure update: {} files, {} symbols, SCIP run: {}, SCIP edges kept: {}, \
             marked for SCIP: {}, macro-text: {}, {:.1} s",
            built.files,
            built.symbols,
            built.scip.is_some(),
            built.edges.kept_scip,
            built.edges.pending,
            built.edges.macro_text,
            started.elapsed().as_secs_f64()
        );
    } else if !skip_build {
        let built = build_structure(root, db).map_err(|e| e.to_string())?;
        println!(
            "structure: {} files, {} symbols, {:.1} s",
            built.files,
            built.symbols,
            started.elapsed().as_secs_f64()
        );
        if let Some(scip) = &built.scip {
            println!(
                "SCIP run: {}, {:.1} s, {} documents, {} occurrences, {} targets left out",
                scip.rust_analyzer,
                scip.elapsed.as_secs_f64(),
                scip.documents,
                scip.occurrences,
                scip.left_out_targets.len()
            );
        }
        println!("edges: {:?}", built.edges);
        if structure_only {
            return Ok(());
        }
    }

    let cache = std::env::var_os("KAIROS_EMBED_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/embed-cache"));
    let embedder = LocalProvider::new(&LocalConfig {
        cache_dir: cache,
        allow_download: false,
    })
    .map_err(|e| e.to_string())?;

    let model_file = model_path().ok_or("No model path: set KAIROS_INDEX_MODEL.")?;
    let loading = Instant::now();
    let model = LlamaModelFile::load(&model_file).map_err(|e| e.to_string())?;
    let mut summarizer = model.summarizer().map_err(|e| e.to_string())?;
    println!("model load: {:.1} s", loading.elapsed().as_secs_f64());

    let summarizing = Instant::now();
    let report =
        summarize(root, db, &mut summarizer, &embedder, &options).map_err(|e| e.to_string())?;
    let wall = summarizing.elapsed();

    for level in [Level::Symbol, Level::File, Level::Module] {
        let mut times: Vec<Duration> = report
            .calls
            .iter()
            .filter(|c| c.level == level)
            .map(|c| c.elapsed)
            .collect();
        if times.is_empty() {
            continue;
        }
        times.sort();
        let total: Duration = times.iter().sum();
        let pick = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize].as_secs_f64();
        println!(
            "{}: {} calls, total {:.0} s, mean {:.2} s, median {:.2} s, p95 {:.2} s",
            level.as_str(),
            times.len(),
            total.as_secs_f64(),
            total.as_secs_f64() / times.len() as f64,
            pick(0.5),
            pick(0.95)
        );
    }
    println!(
        "counts: symbols {:?}, files {:?}, modules {:?}",
        report.symbols, report.files, report.modules
    );
    println!(
        "vectors: {} in {:.1} s; summary run {:.0} s; whole run {:.0} s",
        report.vectors,
        report.embed_elapsed.as_secs_f64(),
        wall.as_secs_f64(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
