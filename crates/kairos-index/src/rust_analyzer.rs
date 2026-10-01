//! The pinned rust-analyzer of the SCIP run (COLLIERY-T-1858).
//!
//! The index does not use the rust-analyzer of the toolchain: rust-analyzer
//! 1.93.0 `scip` panics when 2 crates share a module file. It uses one
//! standalone release, pinned here with the sha256 of its archive and of its
//! binary for each platform.
//!
//! - The binary is kept in `~/.cache/kairos-index/bin/`, outside the
//!   repository and outside `target/`. `KAIROS_INDEX_RUST_ANALYZER` can give
//!   another path.
//! - On first need, the index downloads the archive from the GitHub release,
//!   checks its sha256, unpacks it and checks the sha256 of the binary.
//! - Each run checks the sha256 of the binary again, and refuses a binary
//!   that is not the pinned one.
//!
//! `angreal dev fetch-rust-analyzer` downloads it before a test run; the
//! tests never download.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::IndexError;

/// The tag of the pinned release on github.com/rust-lang/rust-analyzer.
pub const RELEASE: &str = "2026-07-13";

/// What `rust-analyzer --version` of the pinned release prints.
pub const VERSION: &str = "rust-analyzer 0.3.2971-standalone (ffcdbbd906 2026-07-13)";

/// The pinned release for one platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// The target triple, as in the name of the release asset.
    pub target: &'static str,
    /// The sha256 of `rust-analyzer-<target>.gz`.
    pub archive_sha256: &'static str,
    /// The sha256 of the binary in it.
    pub binary_sha256: &'static str,
}

/// The pinned release for each platform that can build an index.
pub const PINS: &[Pin] = &[
    Pin {
        target: "aarch64-apple-darwin",
        archive_sha256: "9c6b3ebf06480e2c95a7b01750fa68d77834bffa34da81e4eb00cef3cdff4613",
        binary_sha256: "1ac9d9ae6cfc472744fa981420037fcb98f3803cb7bae05e910c7723908ff53a",
    },
    Pin {
        target: "x86_64-apple-darwin",
        archive_sha256: "b8832accb9f163214e63ccc989bb2161d52f19270eafb136da0fb16093185041",
        binary_sha256: "9e14add6fb840d7ee3b8c6bb877d4c14bba5d4f2e1e0f350c72c91c3547ae133",
    },
    Pin {
        target: "x86_64-unknown-linux-gnu",
        archive_sha256: "5ee1754afa7a1eb7f56606847b61328e6fac2f316e40ebf314dcefb30263df4d",
        binary_sha256: "73dc265a58f78a29d80d67319fe84c6a8f0b377bb161ffff2846e60a35620457",
    },
    Pin {
        target: "aarch64-unknown-linux-gnu",
        archive_sha256: "d30c3ac726f93ae7cb57c6e16cd2d2b5460c9893ccdd38b6d3ae9300c72852ab",
        binary_sha256: "88a2be2f3999dbc7f81422b9926c9e29ceb8f485fd2237b45335ffdfbfc6e53c",
    },
];

/// The target triple of this build.
fn this_target() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-gnu",
        other => other,
    };
    format!("{}-{os}", std::env::consts::ARCH)
}

/// The pinned release for this platform.
pub fn pin() -> Result<&'static Pin, IndexError> {
    let target = this_target();
    PINS.iter()
        .find(|p| p.target == target)
        .ok_or(IndexError::RustAnalyzerPlatform(target))
}

/// Where the pinned binary is: `KAIROS_INDEX_RUST_ANALYZER` if it is set,
/// else `~/.cache/kairos-index/bin/rust-analyzer-<release>`.
pub fn binary_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KAIROS_INDEX_RUST_ANALYZER") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".cache/kairos-index/bin")
            .join(format!("rust-analyzer-{RELEASE}")),
    )
}

/// Which rust-analyzer a SCIP run uses.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// The binary to use in place of the one at [`binary_path`]. It must
    /// have the pinned sha256 too.
    pub rust_analyzer: Option<PathBuf>,
    /// Download the pinned release if the binary is not there. The tests
    /// turn this off.
    pub download: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            rust_analyzer: None,
            download: true,
        }
    }
}

/// The checked binary for a SCIP run.
pub(crate) fn resolve(options: &BuildOptions) -> Result<PathBuf, IndexError> {
    let pin = pin()?;
    let path = match &options.rust_analyzer {
        Some(path) => path.clone(),
        None => binary_path().ok_or_else(|| {
            IndexError::Scip("HOME and KAIROS_INDEX_RUST_ANALYZER are not set".into())
        })?,
    };
    if !path.is_file() {
        if options.download && options.rust_analyzer.is_none() {
            return fetch_to(&path, pin);
        }
        return Err(IndexError::RustAnalyzerMissing(path));
    }
    check(&path, pin)?;
    Ok(path)
}

/// Download the pinned release to [`binary_path`] if it is not there, and
/// check it. Returns its path.
pub fn fetch() -> Result<PathBuf, IndexError> {
    resolve(&BuildOptions::default())
}

/// Refuse a binary whose sha256 is not the pinned value.
fn check(path: &Path, pin: &Pin) -> Result<(), IndexError> {
    let io = |source| IndexError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(io)?;
    let found = hex(&hasher.finalize());
    if found != pin.binary_sha256 {
        return Err(IndexError::RustAnalyzerChecksum {
            path: path.to_path_buf(),
            found,
            expected: pin.binary_sha256.to_string(),
        });
    }
    Ok(())
}

/// Download, check and unpack the pinned release to `path`.
fn fetch_to(path: &Path, pin: &Pin) -> Result<PathBuf, IndexError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| IndexError::Io { path: p, source }
    };
    fs::create_dir_all(dir).map_err(io(dir))?;
    let url = format!(
        "https://github.com/rust-lang/rust-analyzer/releases/download/{RELEASE}/rust-analyzer-{}.gz",
        pin.target
    );
    let mut archive = Vec::new();
    ureq::get(&url)
        .call()
        .map_err(|e| IndexError::RustAnalyzerDownload(format!("{url}: {e}")))?
        .into_body()
        .into_reader()
        .read_to_end(&mut archive)
        .map_err(|e| IndexError::RustAnalyzerDownload(format!("{url}: {e}")))?;
    let found = hex(&Sha256::digest(&archive));
    if found != pin.archive_sha256 {
        return Err(IndexError::RustAnalyzerChecksum {
            path: PathBuf::from(url),
            found,
            expected: pin.archive_sha256.to_string(),
        });
    }
    let mut binary = Vec::new();
    flate2::read::GzDecoder::new(archive.as_slice())
        .read_to_end(&mut binary)
        .map_err(|e| IndexError::RustAnalyzerDownload(format!("{url}: {e}")))?;
    // Write next to the target, check, then rename: a stopped download
    // leaves no binary at `path`.
    let mut partial = tempfile::NamedTempFile::new_in(dir).map_err(io(dir))?;
    partial.write_all(&binary).map_err(io(partial.path()))?;
    check(partial.path(), pin)?;
    make_executable(partial.path()).map_err(io(partial.path()))?;
    partial.persist(path).map_err(|e| IndexError::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(path.to_path_buf())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_has_one_pin() {
        let mut targets: Vec<_> = PINS.iter().map(|p| p.target).collect();
        targets.sort();
        targets.dedup();
        assert_eq!(targets.len(), PINS.len());
        for p in PINS {
            assert_eq!(p.archive_sha256.len(), 64, "{p:?}");
            assert_eq!(p.binary_sha256.len(), 64, "{p:?}");
        }
    }

    #[test]
    fn a_wrong_binary_is_refused_with_its_path_and_the_pinned_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rust-analyzer");
        fs::write(&path, b"not rust-analyzer").unwrap();
        let options = BuildOptions {
            rust_analyzer: Some(path.clone()),
            download: true,
        };
        let text = resolve(&options).unwrap_err().to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains(pin().unwrap().binary_sha256), "{text}");
    }

    #[test]
    fn a_missing_given_binary_is_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rust-analyzer");
        let options = BuildOptions {
            rust_analyzer: Some(path.clone()),
            download: true,
        };
        assert!(matches!(
            resolve(&options),
            Err(IndexError::RustAnalyzerMissing(p)) if p == path
        ));
        assert!(!path.exists());
    }
}
