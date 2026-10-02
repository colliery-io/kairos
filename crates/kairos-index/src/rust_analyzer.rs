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
//! **The std source** is pinned too (COLLIERY-T-1860). Since release
//! 2026-07-20 (rust-lang/rust-analyzer#22784), rust-analyzer finds the std
//! macros only through the prelude, as std does since Rust 1.94. With an
//! older std source, `assert_eq!`, `format!`, `vec!` and `println!` do not
//! expand, and the calls in them get no SCIP reference (COLLIERY-T-1859). So
//! the index does not use the `rust-src` of the toolchain of the repository:
//!
//! - The `rust-src` archive of one stable Rust release is kept in
//!   `~/.cache/kairos-index/rust-src/`. `KAIROS_INDEX_RUST_SRC` can give
//!   another path.
//! - Each run checks the sha256 of the archive. The `library/` folder of the
//!   archive is unpacked once, next to it, and the SCIP run gives it to
//!   rust-analyzer as `cargo.sysrootSrc`.
//!
//! `angreal dev fetch-rust-analyzer` downloads both before a test run; the
//! tests never download.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::IndexError;

/// The tag of the pinned release on github.com/rust-lang/rust-analyzer.
pub const RELEASE: &str = "2026-09-28";

/// What `rust-analyzer --version` of the pinned release prints.
pub const VERSION: &str = "rust-analyzer 0.3.3065-standalone (03fcb77246 2026-09-27)";

/// The Rust release of the pinned std source. It must be 1.94 or later.
pub const RUST_SRC_VERSION: &str = "1.99.0";

/// Where the pinned `rust-src` archive is, from the stable channel manifest
/// of static.rust-lang.org.
pub const RUST_SRC_URL: &str =
    "https://static.rust-lang.org/dist/2026-10-01/rust-src-1.99.0.tar.gz";

/// The sha256 of the archive at [`RUST_SRC_URL`].
pub const RUST_SRC_SHA256: &str =
    "82b978093b33c71bcbe69005b92181bfb76d3d0e434ef303e637812e21badcd7";

/// The folder of the std source in the archive.
const RUST_SRC_PREFIX: &str = "rust-src-1.99.0/rust-src/lib/rustlib/src/rust/";

/// The file that marks a complete unpack, with the sha256 of its archive.
const UNPACKED_MARK: &str = ".kairos-index-unpacked";

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
        archive_sha256: "54ec873d8996e2c127d758bf45d4eacb6d3371dae4f6f6d5d3f05cedbae5fd59",
        binary_sha256: "9b5877a14acf82e1d4dc86e5406423a0dd4e7227fa0d9cc6ccf5989946bcccc2",
    },
    Pin {
        target: "x86_64-apple-darwin",
        archive_sha256: "d032c0eb75e4597cc8ffc35ea4cdbd9eecc8341936b6edac6749e679fc3f0682",
        binary_sha256: "8df394553d85b3ca7d03df5b7ee4be01df4332bfe7b6d8591e07748d0e3bfee5",
    },
    Pin {
        target: "x86_64-unknown-linux-gnu",
        archive_sha256: "23f711d86b5f826e22886f01d7355dc01e0f4c1357dafa29710a95b903b48c85",
        binary_sha256: "d9c4fb5836f3367e530b28cdb0054ad30ccf1bdc96b1624003fb8ecb5d5fb0f5",
    },
    Pin {
        target: "aarch64-unknown-linux-gnu",
        archive_sha256: "03bad9c3dabb0f07a2678d5f9f8f1575a3742ea141506e14b3a26b42a1f896f3",
        binary_sha256: "12f81af1ec5487946adeb3c3cb9197e39d16f718c13bd175ea5f06c182294cee",
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

/// Where the pinned std source archive is: `KAIROS_INDEX_RUST_SRC` if it is
/// set, else `~/.cache/kairos-index/rust-src/rust-src-<version>.tar.gz`.
pub fn std_source_archive_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KAIROS_INDEX_RUST_SRC") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".cache/kairos-index/rust-src")
            .join(format!("rust-src-{RUST_SRC_VERSION}.tar.gz")),
    )
}

/// Which rust-analyzer and which std source a SCIP run uses.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// The binary to use in place of the one at [`binary_path`]. It must
    /// have the pinned sha256 too.
    pub rust_analyzer: Option<PathBuf>,
    /// The std source archive to use in place of the one at
    /// [`std_source_archive_path`]. It must have the pinned sha256 too.
    pub std_source: Option<PathBuf>,
    /// The sysroot of the Rust toolchain, in place of `rustc --print sysroot`
    /// in the repository. The server gives the toolchain of its tools folder
    /// (COLLIERY-T-2525).
    pub sysroot: Option<PathBuf>,
    /// Download the pinned release and the pinned std source if they are
    /// not there. The tests turn this off.
    pub download: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            rust_analyzer: None,
            std_source: None,
            sysroot: None,
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

/// Download the pinned release to [`binary_path`] and the pinned std source
/// to [`std_source_archive_path`] if they are not there, check them, and
/// unpack the std source. Returns the path of the binary and of the std
/// source (its `library/` folder).
pub fn fetch() -> Result<(PathBuf, PathBuf), IndexError> {
    let options = BuildOptions::default();
    Ok((resolve(&options)?, resolve_std_source(&options)?))
}

/// The checked and unpacked std source for a SCIP run: the canonical path
/// of its `library/` folder.
pub(crate) fn resolve_std_source(options: &BuildOptions) -> Result<PathBuf, IndexError> {
    let archive = match &options.std_source {
        Some(path) => path.clone(),
        None => std_source_archive_path()
            .ok_or_else(|| IndexError::Scip("HOME and KAIROS_INDEX_RUST_SRC are not set".into()))?,
    };
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| IndexError::Io { path: p, source }
    };
    let bytes = if archive.is_file() {
        let bytes = fs::read(&archive).map_err(io(&archive))?;
        check_std_source(&archive, &bytes)?;
        bytes
    } else if options.download && options.std_source.is_none() {
        download_std_source(&archive)?
    } else {
        return Err(IndexError::StdSourceMissing(archive));
    };

    let dir = archive.parent().unwrap_or(Path::new("."));
    let dest = dir.join(format!("rust-src-{RUST_SRC_VERSION}"));
    let unpacked = |dest: &Path| {
        fs::read_to_string(dest.join(UNPACKED_MARK)).is_ok_and(|s| s.trim() == RUST_SRC_SHA256)
    };
    if !unpacked(&dest) {
        // Unpack next to the target, mark, then rename: a stopped unpack
        // leaves no marked folder at `dest`.
        let partial = tempfile::Builder::new()
            .prefix(".rust-src-")
            .tempdir_in(dir)
            .map_err(io(dir))?;
        unpack_std_source(&bytes, partial.path(), &archive)?;
        let mark = partial.path().join(UNPACKED_MARK);
        fs::write(&mark, RUST_SRC_SHA256).map_err(io(&mark))?;
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(io(&dest))?;
        }
        if let Err(e) = fs::rename(partial.path(), &dest) {
            // Another run unpacked it at the same time.
            if !unpacked(&dest) {
                return Err(IndexError::Io {
                    path: dest,
                    source: e,
                });
            }
        }
    }
    let library = dest.join("library");
    fs::canonicalize(&library).map_err(io(&library))
}

/// Refuse a std source archive whose sha256 is not the pinned value.
fn check_std_source(path: &Path, bytes: &[u8]) -> Result<(), IndexError> {
    let found = hex(&Sha256::digest(bytes));
    if found != RUST_SRC_SHA256 {
        return Err(IndexError::StdSourceChecksum {
            path: path.to_path_buf(),
            found,
            expected: RUST_SRC_SHA256.to_string(),
        });
    }
    Ok(())
}

/// Download and check the pinned std source archive, and write it to `path`.
fn download_std_source(path: &Path) -> Result<Vec<u8>, IndexError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| IndexError::Io { path: p, source }
    };
    fs::create_dir_all(dir).map_err(io(dir))?;
    let mut bytes = Vec::new();
    ureq::get(RUST_SRC_URL)
        .call()
        .map_err(|e| IndexError::StdSourceDownload(format!("{RUST_SRC_URL}: {e}")))?
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| IndexError::StdSourceDownload(format!("{RUST_SRC_URL}: {e}")))?;
    check_std_source(Path::new(RUST_SRC_URL), &bytes)?;
    let mut partial = tempfile::NamedTempFile::new_in(dir).map_err(io(dir))?;
    partial.write_all(&bytes).map_err(io(partial.path()))?;
    partial.persist(path).map_err(|e| IndexError::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(bytes)
}

/// Unpack the `library/` folder of the std source archive into `to`.
fn unpack_std_source(bytes: &[u8], to: &Path, archive: &Path) -> Result<(), IndexError> {
    let bad = |e: std::io::Error| IndexError::Io {
        path: archive.to_path_buf(),
        source: e,
    };
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in tar.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let path = entry.path().map_err(bad)?.into_owned();
        let Ok(rel) = path.strip_prefix(RUST_SRC_PREFIX) else {
            continue;
        };
        if !rel.starts_with("library")
            || !rel.components().all(|c| matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let target = to.join(rel);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target).map_err(bad)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(bad)?;
        }
        entry.unpack(&target).map_err(bad)?;
    }
    if !to.join("library/core").is_dir() {
        return Err(IndexError::Io {
            path: archive.to_path_buf(),
            source: std::io::Error::other("the archive has no library/core folder"),
        });
    }
    Ok(())
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
            std_source: None,
            sysroot: None,
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
            std_source: None,
            sysroot: None,
            download: true,
        };
        assert!(matches!(
            resolve(&options),
            Err(IndexError::RustAnalyzerMissing(p)) if p == path
        ));
        assert!(!path.exists());
    }

    #[test]
    fn the_std_source_is_from_rust_1_94_or_later() {
        let mut parts = RUST_SRC_VERSION
            .split('.')
            .map(|p| p.parse::<u32>().unwrap());
        let (major, minor) = (parts.next().unwrap(), parts.next().unwrap());
        assert!((major, minor) >= (1, 94), "{RUST_SRC_VERSION}");
        assert!(RUST_SRC_URL.ends_with(&format!("rust-src-{RUST_SRC_VERSION}.tar.gz")));
        assert!(RUST_SRC_PREFIX.starts_with(&format!("rust-src-{RUST_SRC_VERSION}/")));
        assert_eq!(RUST_SRC_SHA256.len(), 64);
    }

    #[test]
    fn a_wrong_std_source_is_refused_with_its_path_and_the_pinned_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rust-src.tar.gz");
        fs::write(&path, b"not rust-src").unwrap();
        let options = BuildOptions {
            rust_analyzer: None,
            std_source: Some(path.clone()),
            sysroot: None,
            download: true,
        };
        let text = resolve_std_source(&options).unwrap_err().to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains(RUST_SRC_SHA256), "{text}");
        // Nothing is unpacked.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_missing_given_std_source_is_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rust-src.tar.gz");
        let options = BuildOptions {
            rust_analyzer: None,
            std_source: Some(path.clone()),
            sysroot: None,
            download: true,
        };
        assert!(matches!(
            resolve_std_source(&options),
            Err(IndexError::StdSourceMissing(p)) if p == path
        ));
        assert!(!path.exists());
    }

    #[test]
    fn only_the_library_folder_is_unpacked() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut add = |path: &str, text: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, text).unwrap();
        };
        add(
            &format!("{RUST_SRC_PREFIX}library/core/src/lib.rs"),
            b"//! core",
        );
        add(&format!("{RUST_SRC_PREFIX}src/llvm-project/x.c"), b"no");
        add("rust-src-1.99.0/manifest.in", b"no");
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        let bytes = gz.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        unpack_std_source(&bytes, dir.path(), Path::new("a.tar.gz")).unwrap();
        let mut found = Vec::new();
        for entry in ignore::WalkBuilder::new(dir.path()).hidden(false).build() {
            let entry = entry.unwrap();
            if entry.file_type().is_some_and(|t| t.is_file()) {
                let rel = entry.path().strip_prefix(dir.path()).unwrap();
                found.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        assert_eq!(found, ["library/core/src/lib.rs"]);
    }
}
