//! Download the pinned rust-analyzer release into the cache of the index
//! (COLLIERY-T-1858): `angreal dev fetch-rust-analyzer`.
//!
//! The binary goes to `~/.cache/kairos-index/bin/` (or to
//! `KAIROS_INDEX_RUST_ANALYZER`). Its sha256 is checked. If it is there
//! already, only the check runs.

use std::process::ExitCode;

use kairos_index::rust_analyzer;

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        eprintln!("Unknown argument {arg}. This command has no arguments.");
        return ExitCode::FAILURE;
    }
    match rust_analyzer::fetch() {
        Ok(path) => {
            println!("{} is at {}.", rust_analyzer::VERSION, path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
