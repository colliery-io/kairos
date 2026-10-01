//! Download the pinned rust-analyzer release and the pinned std source into
//! the cache of the index (COLLIERY-T-1858, COLLIERY-T-1860):
//! `angreal dev fetch-rust-analyzer`.
//!
//! The binary goes to `~/.cache/kairos-index/bin/` (or to
//! `KAIROS_INDEX_RUST_ANALYZER`), the `rust-src` archive to
//! `~/.cache/kairos-index/rust-src/` (or to `KAIROS_INDEX_RUST_SRC`), and its
//! `library/` folder is unpacked next to it. Both sha256 values are checked.
//! If both are there already, only the checks run.

use std::process::ExitCode;

use kairos_index::rust_analyzer;

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        eprintln!("Unknown argument {arg}. This command has no arguments.");
        return ExitCode::FAILURE;
    }
    match rust_analyzer::fetch() {
        Ok((binary, std_source)) => {
            println!("{} is at {}.", rust_analyzer::VERSION, binary.display());
            println!(
                "The std source of Rust {} is at {}.",
                rust_analyzer::RUST_SRC_VERSION,
                std_source.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
