//! The tools of the builder of the base index in the server
//! (COLLIERY-T-2525): the summary model, rust-analyzer, the std source and a
//! minimal Rust toolchain.
//!
//! The server image does not hold them. On each start, the server calls
//! [`prepare`] on the folder `tools/` of `KAIROS_CODE_INDEX_DIR`:
//!
//! - A file that is there is checked against its pinned sha256. A wrong
//!   checksum is refused and named. It is not downloaded again: the operator
//!   removes the file.
//! - A file that is not there is downloaded, checked, and only then put in
//!   the folder. So a stopped download leaves no file.
//! - rust-analyzer is unpacked from its archive, and the 3 archives of the
//!   toolchain are unpacked into one folder, which is the sysroot of the
//!   SCIP run. A mark with the checksums of the archives tells that an
//!   unpack is complete.
//!
//! An operator with no network puts the files of [`ToolSet::pinned`] in the
//! folder first, with the names of their URLs. Then nothing is downloaded.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::IndexError;
use crate::rust_analyzer;

/// The Rust release of the toolchain: the release of this repository
/// (`rust-toolchain.toml`). The SCIP run reads `cargo metadata` and the
/// sysroot of it, and builds nothing.
pub const TOOLCHAIN_VERSION: &str = "1.93.0";

/// The summary model: Qwen3-4B-Instruct-2507, GGUF Q4_K_M from bartowski, at
/// a fixed revision.
pub const MODEL_URL: &str = "https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF/resolve/ae44f08e1392f39c0e474af10c3ff8355c8b6688/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

/// The sha256 of the file at [`MODEL_URL`].
pub const MODEL_SHA256: &str = "2fde00ce69dd4899c70d020845e2638353015bba0fdf161b3eb965f2bca4464e";

/// The sha256 of the archives of the toolchain for one platform, from the
/// `.sha256` files of static.rust-lang.org.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolchainPin {
    /// The target triple.
    pub target: &'static str,
    /// The sha256 of `rustc-<version>-<target>.tar.gz`.
    pub rustc: &'static str,
    /// The sha256 of `cargo-<version>-<target>.tar.gz`.
    pub cargo: &'static str,
    /// The sha256 of `rust-std-<version>-<target>.tar.gz`.
    pub rust_std: &'static str,
}

/// The toolchain archives for each platform that can build an index.
pub const TOOLCHAIN_PINS: &[ToolchainPin] = &[
    ToolchainPin {
        target: "aarch64-unknown-linux-gnu",
        rustc: "f8609d27c42de42ab655c7072c894cebe1091eb481c0aaaccbad7dccfa8b06fd",
        cargo: "21c823462acf538d9e35bc36a39f1690e1a809cf07048506bf7bb8dfd7be6df8",
        rust_std: "e10aaf81c552ffe09f21e01492daa4d0b15217b31b4b7c1d4195cf98a3184380",
    },
    ToolchainPin {
        target: "x86_64-unknown-linux-gnu",
        rustc: "9e35d0d8251db1fff243fe36e417263f7dd48c9ec7c61f8c13560f7e21a37f46",
        cargo: "c4ad6f857c3b72f1a515adc7d02fc1edd376a1313bb4edfdaffc1835cc920e37",
        rust_std: "8e276f68c7793bbc18a2856a501dd6b296af2296d6482407762e3ce79a2221ff",
    },
    ToolchainPin {
        target: "aarch64-apple-darwin",
        rustc: "4b5cf30c0d552ac8a292854e7d96ee65e0f1c6fdce22e30ee6295558bccbd16b",
        cargo: "904b6638bb2dcc274b03aaae322ce5a98704f8333f5343f351cfbd7cf12df869",
        rust_std: "154c3f22a7d4ce6a47f02e2d7817115f11b1f8c0e191b55b59099324d402eed2",
    },
    ToolchainPin {
        target: "x86_64-apple-darwin",
        rustc: "99b91f9c93686cb2591a4e23d233e719dd05f428da3c4ab579cf60a6dba69f5d",
        cargo: "2d3ff96858dd1d7200fd1d19a48f5832643aab81dea06b9da8a542d7379f2240",
        rust_std: "ef43d48f140291ad8889f06d89ec17288f5c02854ca92bc76c6bc1775946815e",
    },
];

/// One file to download, check and keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// The name of the file in the tools folder.
    pub file: String,
    /// Where to download it from.
    pub url: String,
    /// The pinned sha256 of the file.
    pub sha256: String,
}

impl Download {
    /// A download whose file name is the last part of `url`.
    pub fn new(url: impl Into<String>, sha256: impl Into<String>) -> Self {
        let url = url.into();
        let file = url.rsplit('/').next().unwrap_or(&url).to_string();
        Download {
            file,
            url,
            sha256: sha256.into(),
        }
    }
}

/// The tools of the builder, with their pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSet {
    /// The summary model (a GGUF file).
    pub model: Download,
    /// The rust-analyzer release: one gzip file with the binary in it.
    pub rust_analyzer: Download,
    /// The sha256 of the binary in [`ToolSet::rust_analyzer`].
    pub rust_analyzer_binary_sha256: String,
    /// The `rust-src` archive of the pinned std source.
    pub rust_src: Download,
    /// The archives of the toolchain (`rustc`, `cargo`, `rust-std`). Each
    /// has one top folder, which has one folder for each component.
    pub toolchain: Vec<Download>,
    /// The folder in the tools folder that the toolchain is unpacked into.
    pub toolchain_dir: String,
}

impl ToolSet {
    /// The pinned tools for this platform: the downloads from Hugging Face,
    /// GitHub and static.rust-lang.org.
    pub fn pinned() -> Result<ToolSet, IndexError> {
        let ra = rust_analyzer::pin()?;
        let target = ra.target;
        let tc = TOOLCHAIN_PINS
            .iter()
            .find(|p| p.target == target)
            .ok_or_else(|| IndexError::RustAnalyzerPlatform(target.to_string()))?;
        let dist = |component: &str, sha256: &str| {
            Download::new(
                format!(
                    "https://static.rust-lang.org/dist/{component}-{TOOLCHAIN_VERSION}-{target}.tar.gz"
                ),
                sha256,
            )
        };
        Ok(ToolSet {
            model: Download::new(MODEL_URL, MODEL_SHA256),
            rust_analyzer: Download::new(
                format!(
                    "https://github.com/rust-lang/rust-analyzer/releases/download/{}/rust-analyzer-{target}.gz",
                    rust_analyzer::RELEASE
                ),
                ra.archive_sha256,
            ),
            rust_analyzer_binary_sha256: ra.binary_sha256.to_string(),
            rust_src: Download::new(rust_analyzer::RUST_SRC_URL, rust_analyzer::RUST_SRC_SHA256),
            toolchain: vec![
                dist("rustc", tc.rustc),
                dist("cargo", tc.cargo),
                dist("rust-std", tc.rust_std),
            ],
            toolchain_dir: format!("rust-{TOOLCHAIN_VERSION}"),
        })
    }

    /// Each file to download, in the order of [`prepare`].
    pub fn downloads(&self) -> impl Iterator<Item = &Download> {
        [&self.model, &self.rust_analyzer, &self.rust_src]
            .into_iter()
            .chain(self.toolchain.iter())
    }
}

/// The checked tools, ready for the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// The summary model file.
    pub model: PathBuf,
    /// The rust-analyzer binary.
    pub rust_analyzer: PathBuf,
    /// The `rust-src` archive. The SCIP run unpacks it next to itself.
    pub rust_src: PathBuf,
    /// The unpacked toolchain: the sysroot of the SCIP run.
    pub sysroot: PathBuf,
    /// The files that this call downloaded.
    pub downloaded: Vec<String>,
}

impl Tools {
    /// The options of a SCIP run that uses these tools and downloads
    /// nothing.
    pub fn build_options(&self) -> rust_analyzer::BuildOptions {
        rust_analyzer::BuildOptions {
            rust_analyzer: Some(self.rust_analyzer.clone()),
            std_source: Some(self.rust_src.clone()),
            sysroot: Some(self.sysroot.clone()),
            download: false,
        }
    }
}

/// The file that marks a complete unpack, with the checksums of its
/// archives.
const UNPACKED_MARK: &str = ".kairos-index-unpacked";

/// The longest wait for a connection to a download server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Check each tool in `dir`, download each one that is not there, and unpack
/// rust-analyzer and the toolchain. `on_download` is called before each
/// download, with the file and its URL.
pub fn prepare(
    dir: &Path,
    set: &ToolSet,
    on_download: &mut dyn FnMut(&Download),
) -> Result<Tools, IndexError> {
    fs::create_dir_all(dir).map_err(io(dir))?;
    let mut downloaded = Vec::new();
    let mut get = |d: &Download| -> Result<PathBuf, IndexError> {
        let path = dir.join(&d.file);
        if path.exists() {
            check_file(&path, &d.sha256)?;
        } else {
            on_download(d);
            download(d, &path)?;
            downloaded.push(d.file.clone());
        }
        Ok(path)
    };
    let model = get(&set.model)?;
    let ra_archive = get(&set.rust_analyzer)?;
    let rust_src = get(&set.rust_src)?;
    let archives = set
        .toolchain
        .iter()
        .map(&mut get)
        .collect::<Result<Vec<_>, _>>()?;

    let rust_analyzer = dir.join("rust-analyzer");
    if check_file(&rust_analyzer, &set.rust_analyzer_binary_sha256).is_err() {
        gunzip_binary(
            &ra_archive,
            &rust_analyzer,
            &set.rust_analyzer_binary_sha256,
        )?;
    }

    let sysroot = dir.join(&set.toolchain_dir);
    let mark: String = set
        .toolchain
        .iter()
        .map(|d| d.sha256.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let unpacked = |dest: &Path| {
        fs::read_to_string(dest.join(UNPACKED_MARK)).is_ok_and(|s| s.trim() == mark.trim())
    };
    if !unpacked(&sysroot) {
        // Unpack next to the target, mark, then rename: a stopped unpack
        // leaves no marked folder.
        let partial = tempfile::Builder::new()
            .prefix(".toolchain-")
            .tempdir_in(dir)
            .map_err(io(dir))?;
        for archive in &archives {
            unpack_component(archive, partial.path())?;
        }
        for tool in ["rustc", "cargo"] {
            let bin = partial.path().join("bin").join(tool);
            if !bin.is_file() {
                return Err(IndexError::Io {
                    path: sysroot.clone(),
                    source: std::io::Error::other(format!("the toolchain has no bin/{tool}")),
                });
            }
        }
        let mark_path = partial.path().join(UNPACKED_MARK);
        fs::write(&mark_path, &mark).map_err(io(&mark_path))?;
        if sysroot.exists() {
            fs::remove_dir_all(&sysroot).map_err(io(&sysroot))?;
        }
        fs::rename(partial.path(), &sysroot).map_err(io(&sysroot))?;
    }

    Ok(Tools {
        model,
        rust_analyzer,
        rust_src,
        sysroot,
        downloaded,
    })
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> IndexError + '_ {
    move |source| IndexError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The sha256 of the file at `path`, read in parts.
fn sha256_of(path: &Path) -> Result<String, IndexError> {
    let mut file = fs::File::open(path).map_err(io(path))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(io(path))?;
    Ok(hex(&hasher.finalize()))
}

/// Refuse a file whose sha256 is not `expected`.
fn check_file(path: &Path, expected: &str) -> Result<(), IndexError> {
    let found = sha256_of(path)?;
    if found != expected {
        return Err(IndexError::ToolChecksum {
            path: path.to_path_buf(),
            found,
            expected: expected.to_string(),
        });
    }
    Ok(())
}

/// Download `d` to a file next to `path`, with its sha256 checked as it
/// comes, then rename it to `path`.
fn download(d: &Download, path: &Path) -> Result<(), IndexError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let failed = |e: &dyn std::fmt::Display| IndexError::ToolDownload(format!("{}: {e}", d.url));
    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .build()
        .new_agent();
    let response = agent.get(&d.url).call().map_err(|e| failed(&e))?;
    let mut reader = response.into_body().into_reader();
    let mut partial = tempfile::NamedTempFile::new_in(dir).map_err(io(dir))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf).map_err(|e| failed(&e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        partial.write_all(&buf[..n]).map_err(io(partial.path()))?;
    }
    let found = hex(&hasher.finalize());
    if found != d.sha256 {
        return Err(IndexError::ToolChecksum {
            path: PathBuf::from(&d.url),
            found,
            expected: d.sha256.clone(),
        });
    }
    partial.persist(path).map_err(|e| IndexError::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

/// Unpack the gzip file `archive` to the executable `to`, and check it.
fn gunzip_binary(archive: &Path, to: &Path, sha256: &str) -> Result<(), IndexError> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let file = fs::File::open(archive).map_err(io(archive))?;
    let mut partial = tempfile::NamedTempFile::new_in(dir).map_err(io(dir))?;
    std::io::copy(&mut flate2::read::GzDecoder::new(file), &mut partial).map_err(io(archive))?;
    partial.flush().map_err(io(partial.path()))?;
    check_file(partial.path(), sha256)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(partial.path(), fs::Permissions::from_mode(0o755))
            .map_err(io(partial.path()))?;
    }
    partial.persist(to).map_err(|e| IndexError::Io {
        path: to.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

/// Unpack the component folders of one toolchain archive into `to`. An
/// entry `<top>/<component>/<path>` goes to `to/<path>`; the files at the top
/// of a component folder (`manifest.in`) and of the archive are left out.
fn unpack_component(archive: &Path, to: &Path) -> Result<(), IndexError> {
    let bad = io(archive);
    let file = fs::File::open(archive).map_err(&bad)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(&bad)? {
        let mut entry = entry.map_err(&bad)?;
        let path = entry.path().map_err(&bad)?.into_owned();
        let parts: Vec<Component> = path.components().collect();
        if parts.len() < 4 || !parts.iter().all(|c| matches!(c, Component::Normal(_))) {
            continue;
        }
        let rel: PathBuf = parts[2..].iter().collect();
        let target = to.join(&rel);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(&bad)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(&bad)?;
        }
        entry.unpack(&target).map_err(&bad)?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_of_rust_analyzer_has_a_toolchain_pin() {
        for pin in rust_analyzer::PINS {
            let tc = TOOLCHAIN_PINS.iter().find(|t| t.target == pin.target);
            let tc = tc.unwrap_or_else(|| panic!("no toolchain pin for {}", pin.target));
            for sha in [tc.rustc, tc.cargo, tc.rust_std] {
                assert_eq!(sha.len(), 64, "{tc:?}");
            }
        }
        assert_eq!(MODEL_SHA256.len(), 64);
    }

    #[test]
    fn the_pinned_set_names_each_file_by_its_url() {
        let set = ToolSet::pinned().unwrap();
        assert_eq!(set.model.file, crate::summary::MODEL_FILE_NAME);
        assert_eq!(set.rust_src.file, "rust-src-1.99.0.tar.gz");
        assert_eq!(set.downloads().count(), 6);
        for d in set.downloads() {
            assert!(d.url.ends_with(&d.file), "{d:?}");
            assert!(d.url.starts_with("https://"), "{d:?}");
        }
    }

    #[test]
    fn a_tool_with_a_wrong_checksum_is_refused_and_named() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = ToolSet::pinned().unwrap();
        set.model.url = "http://127.0.0.1:1/model.gguf".into();
        let path = dir.path().join(&set.model.file);
        fs::write(&path, b"not the model").unwrap();
        let text = prepare(dir.path(), &set, &mut |_| panic!("no download"))
            .unwrap_err()
            .to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains(MODEL_SHA256), "{text}");
        assert_eq!(fs::read(&path).unwrap(), b"not the model");
    }
}
