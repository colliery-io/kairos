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
//! - After a full check, the size and the modified time of the file go into
//!   `.kairos-index-checked` in the folder (COLLIERY-T-2529). At the next
//!   start, a file whose size and modified time did not change, and whose
//!   pin did not change, is not hashed again: about 11 s for the 2.7 GB of
//!   files. A file that changed is hashed in full.
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

/// The Rust release of the toolchain. The SCIP run reads `cargo metadata`
/// and the sysroot of it, and builds nothing. It is the release of the
/// pinned std source ([`rust_analyzer::RUST_SRC_VERSION`]): rustc, cargo,
/// rust-std and rust-src of one release (KAIROS-T-0747). A test checks it.
pub const TOOLCHAIN_VERSION: &str = "1.99.0";

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
        rustc: "6780ebb1b8ae66cc5d37ebfdc70b18fe775b1083c997f4e2c963d45a7eec59bc",
        cargo: "f8b85e84b0514b69e17e3bad1c2dec7526f4a40cec565d233bbdcb66484dd9e0",
        rust_std: "cdffa7100206633ccfd2514ea7781b4211813bf04c831a509240e1cda5308512",
    },
    ToolchainPin {
        target: "x86_64-unknown-linux-gnu",
        rustc: "238e72b8617f79bc96f27a5bfeb1a208b5755fe1a48f8bc190fe403ef6d54ab8",
        cargo: "c2b8ba1f59e7a230aa5522684f3f7aa620b98e5ad37683a5e5cf9b41f534fa1e",
        rust_std: "1dcaa01beb6bc78fdb13815b4f15214719167c58e3d79fbdf214591f8c195ee1",
    },
    ToolchainPin {
        target: "aarch64-apple-darwin",
        rustc: "2f6b9c9cdc9c2e87ab2e27335e8bb0c4dbe4daea486e39030b66980058f74353",
        cargo: "318b85c276b43134102fb49bbb54bfd0757e5cce47a772461149aeab5adea2ab",
        rust_std: "3589b3a6c6470d590aa5aa7fa1a4af0101103847761a5372a7300b0aa85699c6",
    },
    ToolchainPin {
        target: "x86_64-apple-darwin",
        rustc: "4b909ba0153d4a67eb83a31ef11157b7c54e5c04a323d80e46436b778c18df52",
        cargo: "ec5bd1c782e5b9d58cfcafbc4131d19fd7ca2ec6b1a197f2fd1a1f953802a26a",
        rust_std: "7321ff86554726b8ab7b0a5c0d9ba0a3e763824f5d09b0c997e36bd72b80fa06",
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
    /// The summary model (a GGUF file). None for a server with no embedded
    /// model (KAIROS-T-0344): it downloads none.
    pub model: Option<Download>,
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
            model: Some(Download::new(MODEL_URL, MODEL_SHA256)),
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

    /// The same set with no summary model (KAIROS-T-0344): for a server
    /// that has no `llama` feature, so that it does not download the 2.5 GB
    /// file it cannot run.
    pub fn without_model(mut self) -> Self {
        self.model = None;
        self
    }

    /// Each file to download, in the order of [`prepare`].
    pub fn downloads(&self) -> impl Iterator<Item = &Download> {
        self.model
            .iter()
            .chain([&self.rust_analyzer, &self.rust_src])
            .chain(self.toolchain.iter())
    }
}

/// The checked tools, ready for the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// The summary model file. None when the set has none (KAIROS-T-0344).
    pub model: Option<PathBuf>,
    /// The rust-analyzer binary.
    pub rust_analyzer: PathBuf,
    /// The `rust-src` archive. The SCIP run unpacks it next to itself.
    pub rust_src: PathBuf,
    /// The unpacked toolchain: the sysroot of the SCIP run.
    pub sysroot: PathBuf,
    /// The files that this call downloaded.
    pub downloaded: Vec<String>,
    /// The files that this call read again to hash them in full: the files
    /// that are not in the record of full checks, or that changed since
    /// their check. A download is hashed as it comes and is not in this list.
    pub hashed: Vec<String>,
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
            ..Default::default()
        }
    }
}

/// The file that marks a complete unpack, with the checksums of its
/// archives.
const UNPACKED_MARK: &str = ".kairos-index-unpacked";

/// The record of the full checks of the files in the tools folder.
const CHECKED: &str = ".kairos-index-checked";

/// The size and the modified time (in ns after the epoch) of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified_ns: u128,
}

impl Stamp {
    fn of(path: &Path) -> Option<Stamp> {
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let modified_ns = modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        Some(Stamp {
            size: meta.len(),
            modified_ns,
        })
    }
}

/// The record of the full checks (COLLIERY-T-2529): for each file name, its
/// stamp at the check and the sha256 that it had. One line for each file:
/// `name<TAB>size<TAB>modified_ns<TAB>sha256`.
struct Checked {
    path: PathBuf,
    entries: std::collections::BTreeMap<String, (Stamp, String)>,
    changed: bool,
}

impl Checked {
    /// The record in `dir`. A missing or bad record is empty: each file is
    /// then hashed in full.
    fn load(dir: &Path) -> Checked {
        let path = dir.join(CHECKED);
        let mut entries = std::collections::BTreeMap::new();
        for line in fs::read_to_string(&path).unwrap_or_default().lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if let [name, size, modified_ns, sha256] = parts[..]
                && let (Ok(size), Ok(modified_ns)) = (size.parse(), modified_ns.parse())
            {
                let stamp = Stamp { size, modified_ns };
                entries.insert(name.to_string(), (stamp, sha256.to_string()));
            }
        }
        Checked {
            path,
            entries,
            changed: false,
        }
    }

    /// Whether the file `name` at `path` had the sha256 `expected` at its
    /// last full check, and its size and modified time did not change.
    fn is_checked(&self, name: &str, path: &Path, expected: &str) -> bool {
        match (self.entries.get(name), Stamp::of(path)) {
            (Some((stamp, sha256)), Some(now)) => *stamp == now && sha256 == expected,
            _ => false,
        }
    }

    fn record(&mut self, name: &str, path: &Path, sha256: &str) {
        match Stamp::of(path) {
            Some(stamp) => {
                self.entries
                    .insert(name.to_string(), (stamp, sha256.to_string()));
            }
            None => {
                self.entries.remove(name);
            }
        }
        self.changed = true;
    }

    fn forget(&mut self, name: &str) {
        self.changed |= self.entries.remove(name).is_some();
    }

    /// Write the record when it changed: to a file next to it, then a
    /// rename. A record that is not written costs only a full check at the
    /// next start, so an error here is not an error of [`prepare`].
    fn save(&self) {
        if !self.changed {
            return;
        }
        let text: String = self
            .entries
            .iter()
            .map(|(name, (stamp, sha256))| {
                format!("{name}\t{}\t{}\t{sha256}\n", stamp.size, stamp.modified_ns)
            })
            .collect();
        let dir = self.path.parent().unwrap_or(Path::new("."));
        if let Ok(mut partial) = tempfile::NamedTempFile::new_in(dir)
            && partial.write_all(text.as_bytes()).is_ok()
        {
            let _ = partial.persist(&self.path);
        }
    }

    /// Check the file `name` at `path` against `expected`: with no hash
    /// when the record has it, else in full. A file that is wrong leaves the
    /// record.
    fn check(
        &mut self,
        name: &str,
        path: &Path,
        expected: &str,
        hashed: &mut Vec<String>,
    ) -> Result<(), IndexError> {
        if self.is_checked(name, path, expected) {
            return Ok(());
        }
        hashed.push(name.to_string());
        match check_file(path, expected) {
            Ok(()) => {
                self.record(name, path, expected);
                Ok(())
            }
            Err(e) => {
                self.forget(name);
                Err(e)
            }
        }
    }
}

/// The longest wait for a connection to a download server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Check each tool in `dir`, download each one that is not there, and unpack
/// rust-analyzer and the toolchain. `on_download` is called before each
/// download, with the file and its URL. A file that the record of full
/// checks has, with the same size, modified time and pin, is not hashed
/// again.
pub fn prepare(
    dir: &Path,
    set: &ToolSet,
    on_download: &mut dyn FnMut(&Download),
) -> Result<Tools, IndexError> {
    fs::create_dir_all(dir).map_err(io(dir))?;
    let mut checked = Checked::load(dir);
    let result = prepare_checked(dir, set, on_download, &mut checked);
    // Also after an error: the files checked before it keep their record.
    checked.save();
    result
}

fn prepare_checked(
    dir: &Path,
    set: &ToolSet,
    on_download: &mut dyn FnMut(&Download),
    checked: &mut Checked,
) -> Result<Tools, IndexError> {
    let mut downloaded = Vec::new();
    let mut hashed = Vec::new();
    let mut get = |d: &Download| -> Result<PathBuf, IndexError> {
        let path = dir.join(&d.file);
        if path.exists() {
            checked.check(&d.file, &path, &d.sha256, &mut hashed)?;
        } else {
            on_download(d);
            download(d, &path)?;
            checked.record(&d.file, &path, &d.sha256);
            downloaded.push(d.file.clone());
        }
        Ok(path)
    };
    let model = set.model.as_ref().map(&mut get).transpose()?;
    let ra_archive = get(&set.rust_analyzer)?;
    let rust_src = get(&set.rust_src)?;
    let archives = set
        .toolchain
        .iter()
        .map(&mut get)
        .collect::<Result<Vec<_>, _>>()?;

    let rust_analyzer = dir.join("rust-analyzer");
    let ra_sha256 = &set.rust_analyzer_binary_sha256;
    if checked
        .check("rust-analyzer", &rust_analyzer, ra_sha256, &mut hashed)
        .is_err()
    {
        gunzip_binary(&ra_archive, &rust_analyzer, ra_sha256)?;
        checked.record("rust-analyzer", &rust_analyzer, ra_sha256);
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
        hashed,
    })
}

pub(crate) fn io(path: &Path) -> impl Fn(std::io::Error) -> IndexError + '_ {
    move |source| IndexError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The sha256 of the file at `path`, read in parts.
pub(crate) fn sha256_of(path: &Path) -> Result<String, IndexError> {
    let mut file = fs::File::open(path).map_err(io(path))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(io(path))?;
    Ok(hex(&hasher.finalize()))
}

/// Refuse a file whose sha256 is not `expected`.
pub(crate) fn check_file(path: &Path, expected: &str) -> Result<(), IndexError> {
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
pub(crate) fn download(d: &Download, path: &Path) -> Result<(), IndexError> {
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

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0747: rustc, cargo, rust-std and rust-src of the tools
    /// folder are of one Rust release. The live worker had rust-1.93.0 with
    /// rust-src-1.99.0.
    #[test]
    fn the_toolchain_and_the_std_source_are_of_one_release() {
        assert_eq!(TOOLCHAIN_VERSION, rust_analyzer::RUST_SRC_VERSION);
        let set = ToolSet::pinned().expect("this platform has pins");
        let named = |file: &str| {
            file.contains(&format!("-{TOOLCHAIN_VERSION}-"))
                || file.ends_with(&format!("-{TOOLCHAIN_VERSION}.tar.gz"))
        };
        assert_eq!(set.toolchain.len(), 3);
        for d in set.toolchain.iter().chain([&set.rust_src]) {
            assert!(
                named(&d.file),
                "{} is not of Rust {TOOLCHAIN_VERSION}",
                d.file
            );
            assert!(d.url.ends_with(&d.file), "{}", d.url);
        }
        for (d, component) in set.toolchain.iter().zip(["rustc", "cargo", "rust-std"]) {
            assert!(d.file.starts_with(&format!("{component}-")), "{}", d.file);
        }
        assert_eq!(set.toolchain_dir, format!("rust-{TOOLCHAIN_VERSION}"));
    }

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
        assert_eq!(
            set.model.as_ref().unwrap().file,
            crate::summary::MODEL_FILE_NAME
        );
        assert_eq!(set.rust_src.file, "rust-src-1.99.0.tar.gz");
        assert_eq!(set.downloads().count(), 6);
        // KAIROS-T-0344: a server with no embedded model downloads 5 files.
        let hosted = ToolSet::pinned().unwrap().without_model();
        assert_eq!(hosted.model, None);
        assert_eq!(hosted.downloads().count(), 5);
        for d in set.downloads() {
            assert!(d.url.ends_with(&d.file), "{d:?}");
            assert!(d.url.starts_with("https://"), "{d:?}");
        }
    }

    #[test]
    fn a_tool_with_a_wrong_checksum_is_refused_and_named() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = ToolSet::pinned().unwrap();
        let model = set.model.as_mut().unwrap();
        model.url = "http://127.0.0.1:1/model.gguf".into();
        let path = dir.path().join(&model.file);
        fs::write(&path, b"not the model").unwrap();
        let text = prepare(dir.path(), &set, &mut |_| panic!("no download"))
            .unwrap_err()
            .to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains(MODEL_SHA256), "{text}");
        assert_eq!(fs::read(&path).unwrap(), b"not the model");
    }
}
