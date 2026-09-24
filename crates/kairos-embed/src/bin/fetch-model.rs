//! Populate the local model cache (KAIROS-T-0189).
//!
//! Run at **image build time** so the runtime image carries the weights and
//! never reaches huggingface.co to serve a request — the finding that made
//! runtime download unacceptable: a stateless container that must call out
//! before it can answer is not "local by default", and it breaks air-gapped
//! deployments outright.
//!
//! It works by asking [`LocalProvider`] to load with downloading enabled, so the
//! cache layout is fastembed's own rather than something reproduced here from
//! observation — a hand-rolled `curl` of the HuggingFace paths would be a second
//! copy of a layout this crate does not own.
//!
//!     fetch-model <cache-dir>
//!
//! Idempotent: a populated cache makes this a load and a probe.
use std::path::PathBuf;

use kairos_embed::EmbeddingProvider;
use kairos_embed::local::{LocalConfig, LocalProvider};

fn main() -> std::process::ExitCode {
    let dir = match std::env::args().nth(1) {
        Some(d) => PathBuf::from(d),
        None => {
            eprintln!("usage: fetch-model <cache-dir>");
            return std::process::ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        return std::process::ExitCode::FAILURE;
    }

    let config = LocalConfig {
        cache_dir: dir.clone(),
        allow_download: true,
    };
    match LocalProvider::new(&config) {
        Ok(provider) => {
            // Prove it works here, where a failure is a build failure, rather
            // than at first request in production.
            match provider.embed_one("model fetch verification") {
                Ok(v) => {
                    println!(
                        "{} ready in {} ({} dimensions)",
                        provider.model_id(),
                        dir.display(),
                        v.len()
                    );
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("model loaded but could not embed: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("could not fetch the model into {}: {e}", dir.display());
            std::process::ExitCode::FAILURE
        }
    }
}
