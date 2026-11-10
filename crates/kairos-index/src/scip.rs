//! The Rust references of a Cargo workspace, from `rust-analyzer scip`
//! (COLLIERY-I-0264, "The call graph").
//!
//! **The run builds nothing.** `rust-analyzer scip` reads no `procMacro` or
//! `cargo.buildScripts.enable` setting: it always runs the build-script
//! command and starts the proc-macro server of the sysroot. So the run
//! gives it a configuration that turns both into nothing:
//!
//! - `cargo.buildScripts.overrideCommand = ["true"]`: the build-script
//!   command is `true`, so no `cargo check` runs and no build script runs.
//! - `cargo.sysroot` is a temporary folder with links to the `bin/` and
//!   `lib/` folders of the toolchain and no `libexec/`. rust-analyzer finds
//!   std there, but it finds no proc-macro server, so it starts none.
//! - `cargo.targetDir` and `CARGO_TARGET_DIR` point into the temporary
//!   folder, never to the `target/` of the repository. `TMPDIR` points into
//!   it too, so the copy of `Cargo.lock` that rust-analyzer makes is there.
//! - rust-analyzer runs `cargo` through the rustup proxy, with
//!   `RUSTUP_TOOLCHAIN` set to the temporary sysroot. The `cargo` of that
//!   sysroot is a short script: `cargo metadata` for the repository gives
//!   metadata that this module prepared, `check`, `build` and the other
//!   build commands do nothing, and the rest goes to the real `cargo`.
//!
//! **The rust-analyzer** is the pinned standalone release of
//! [`crate::rust_analyzer`], with the sysroot of the toolchain of the
//! repository and the pinned std source (`cargo.sysrootSrc`, COLLIERY-T-1860).
//! The `rust-src` of the toolchain is not used. The log must name the
//! pinned std source; the run is refused if not.
//!
//! **A file in 2 crates.** rust-analyzer 1.93.0 `scip` panics when one file
//! is a module of 2 crates ("Invariant violation: file emitted multiple
//! times"), for example `tests/common/mod.rs` of 2 integration tests. The
//! pinned release does not (COLLIERY-T-1858). Only after that panic, the run
//! starts again with prepared metadata that leaves out each target that
//! shares a module file with an earlier target (library first, then
//! binaries, then the rest by name). The calls of a left-out target get name
//! classes.
//!
//! **A symbol in 2 crates.** A SCIP symbol names the package and the path,
//! not the crate. So `fn pin` at the root of 2 test crates of one package
//! is one symbol with 2 definitions. The run gives the crate (target) of
//! each module file, and `edges` uses it to choose the definition.
//!
//! The temporary folder is deleted at the end of the run. The log of
//! rust-analyzer is read to confirm that the build-script command was
//! `true` and that no proc-macro server started; the run is refused if not.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::Mutex;
use std::time::Instant;

use kairos_narsil::parser::LanguageParser;
use serde_json::Value;

use crate::{IndexError, ScipRun};

/// The messages of a SCIP index that the edges use. prost skips the other
/// fields. The tags are those of `scip.proto` (github.com/sourcegraph/scip).
pub mod proto {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Index {
        #[prost(message, repeated, tag = "2")]
        pub documents: Vec<Document>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Document {
        #[prost(string, tag = "1")]
        pub relative_path: String,
        #[prost(message, repeated, tag = "2")]
        pub occurrences: Vec<Occurrence>,
        #[prost(int32, tag = "6")]
        pub position_encoding: i32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Occurrence {
        /// `[start line, start column, end line, end column]`, or 3 numbers
        /// when the range is on one line. From 0.
        #[prost(int32, repeated, tag = "1")]
        pub range: Vec<i32>,
        #[prost(string, tag = "2")]
        pub symbol: String,
        #[prost(int32, tag = "3")]
        pub symbol_roles: i32,
    }

    /// `SymbolRole.Definition`.
    pub const ROLE_DEFINITION: i32 = 0x1;
    /// `PositionEncoding.UTF8CodeUnitOffsetFromLineStart`: the encoding of
    /// rust-analyzer.
    pub const ENCODING_UTF8: i32 = 1;
    /// `PositionEncoding.UTF16CodeUnitOffsetFromLineStart`.
    pub const ENCODING_UTF16: i32 = 2;
    /// `PositionEncoding.UTF32CodeUnitOffsetFromLineStart`.
    pub const ENCODING_UTF32: i32 = 3;
}

/// The log targets that tell what rust-analyzer ran. The rest of its info
/// log is large and is not needed.
const RA_LOG: &str =
    "warn,load_cargo=info,project_model::build_dependencies=info,project_model::workspace=info";

/// The panic of rust-analyzer 1.93.0 `scip` on a file in 2 crates.
const SHARED_FILE_PANIC: &str = "file emitted multiple times";

/// One rust-analyzer SCIP run at a time in a process (KAIROS-T-0747). A run
/// of a repository of the size of kairos needs about 4.7 GiB at its peak, so
/// 2 builds at the same time (for example 2 hosted builds) need 2 times
/// that. A build waits here for the run of the other build.
static SCIP_RUN: Mutex<()> = Mutex::new(());

/// The lines of output that a failed run gives in its error.
const FAILURE_LINES: usize = 8;

/// The crate of each module file of the run, from the repository root. A
/// crate is a target of a workspace member.
#[derive(Debug, Default)]
pub struct Crates {
    pub file_crate: HashMap<String, usize>,
    /// The crates that are library targets.
    pub libraries: HashSet<usize>,
}

/// Run `rust-analyzer scip` on the Cargo workspace at `root`, with the
/// build turned off, and read its index.
pub fn run(
    root: &Path,
    options: &crate::BuildOptions,
) -> Result<(ScipRun, proto::Index, Crates), IndexError> {
    let started = Instant::now();
    if let Some(need) = options.min_memory_bytes {
        check_memory_limit(memory_limit(Path::new(CGROUP_ROOT)), need)?;
    }
    let root = fs::canonicalize(root).map_err(|source| IndexError::Io {
        path: root.to_path_buf(),
        source,
    })?;

    // The pinned binary, with its sha256 checked.
    let program = crate::rust_analyzer::resolve(options)?;
    let version = Command::new(&program)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .ok_or_else(|| IndexError::Scip(format!("{} --version did not run", program.display())))?;
    let rust_analyzer = String::from_utf8_lossy(&version.stdout).trim().to_string();
    // The given toolchain (the tools folder of the server), else the one
    // that the rustup proxy chooses for the repository.
    let sysroot = match &options.sysroot {
        Some(sysroot) => sysroot.clone(),
        None => Command::new("rustc")
            .args(["--print", "sysroot"])
            .current_dir(&root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
            .ok_or_else(|| IndexError::Scip("rustc --print sysroot did not run".into()))?,
    };
    // The pinned std source, with the sha256 of its archive checked.
    let std_source = crate::rust_analyzer::resolve_std_source(options)?;

    let work = tempfile::Builder::new()
        .prefix("kairos-scip-")
        .tempdir()
        .map_err(|source| IndexError::Io {
            path: std::env::temp_dir(),
            source,
        })?;
    let work_dir = work.path().to_path_buf();
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| IndexError::Io { path, source }
    };

    let target_dir = work_dir.join("target");
    let tmp_dir = work_dir.join("tmp");
    fs::create_dir(&tmp_dir).map_err(io(&tmp_dir))?;

    // The metadata of the workspace.
    let manifest = root.join("Cargo.toml");
    let metadata = cargo_metadata(&root, &manifest, &sysroot, &work_dir)?;
    let metadata_path = work_dir.join("metadata.json");

    // The sysroot of the run: lib/ as it is, bin/ with each tool but cargo,
    // and no libexec/.
    let fake_sysroot = work_dir.join("sysroot");
    let fake_bin = fake_sysroot.join("bin");
    fs::create_dir_all(&fake_bin).map_err(io(&fake_bin))?;
    link(&sysroot.join("lib"), &fake_sysroot.join("lib")).map_err(io(&fake_sysroot))?;
    let real_bin = sysroot.join("bin");
    for entry in fs::read_dir(&real_bin).map_err(io(&real_bin))? {
        let entry = entry.map_err(io(&real_bin))?;
        if entry.file_name() != "cargo" {
            link(&entry.path(), &fake_bin.join(entry.file_name())).map_err(io(&fake_bin))?;
        }
    }
    let cargo_script = fake_bin.join("cargo");
    write_cargo_script(
        &cargo_script,
        &manifest,
        &metadata_path,
        &real_bin.join("cargo"),
    )
    .map_err(io(&cargo_script))?;
    let config = serde_json::json!({
        "cargo": {
            "buildScripts": { "enable": false, "overrideCommand": ["true"] },
            "sysroot": fake_sysroot,
            "sysrootSrc": std_source,
            "targetDir": target_dir,
        },
        "procMacro": { "enable": false },
    });
    let config_path = work_dir.join("rust-analyzer.json");
    fs::write(&config_path, config.to_string()).map_err(io(&config_path))?;

    let log_path = work_dir.join("rust-analyzer.log");
    let output_path = work_dir.join("index.scip");
    // First with each target. Only a rust-analyzer with the defect of 1.93.0
    // panics on a file in 2 crates; then again, with the shared targets left
    // out.
    let mut leave_out = false;
    let running = SCIP_RUN.lock().unwrap_or_else(|e| e.into_inner());
    let (left_out_targets, crates) = loop {
        let (prepared, left_out_targets, crates) = prepare_metadata(&root, &metadata, leave_out)?;
        fs::write(&metadata_path, prepared.to_string()).map_err(io(&metadata_path))?;
        let output = with_toolchain(&mut Command::new(&program), &fake_bin)
            .arg("--log-file")
            .arg(&log_path)
            .arg("scip")
            .arg(&root)
            .arg("--output")
            .arg(&output_path)
            .arg("--config-path")
            .arg(&config_path)
            .current_dir(&root)
            .env("RA_LOG", RA_LOG)
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("RUSTUP_TOOLCHAIN", &fake_sysroot)
            .env("TMPDIR", &tmp_dir)
            .output()
            .map_err(|e| IndexError::Scip(e.to_string()))?;
        if output.status.success() {
            break (left_out_targets, crates);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !leave_out && stderr.contains(SHARED_FILE_PANIC) {
            leave_out = true;
            continue;
        }
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        return Err(IndexError::Scip(failure_text(
            "rust-analyzer",
            output.status,
            &stderr,
            &log,
        )));
    };
    drop(running);

    let log = fs::read_to_string(&log_path).unwrap_or_default();
    let build_script_command = build_script_command(&log);
    let proc_macro_server_started = proc_macro_server_started(&log);
    if build_script_command.as_deref().is_some_and(|c| c != "true") {
        return Err(IndexError::BuildNotOff(format!(
            "the build-script command was {}",
            build_script_command.unwrap_or_default()
        )));
    }
    if proc_macro_server_started == Some(true) {
        return Err(IndexError::BuildNotOff(
            "a proc-macro server started".into(),
        ));
    }
    let logged = std_sources(&log);
    if logged.is_empty() || logged.iter().any(|p| !same_path(p, &std_source)) {
        return Err(IndexError::Scip(format!(
            "rust-analyzer did not read the pinned std source {}. Its log names {}",
            std_source.display(),
            if logged.is_empty() {
                "no std source".to_string()
            } else {
                logged.join(", ")
            }
        )));
    }

    let bytes = fs::read(&output_path).map_err(io(&output_path))?;
    let index = <proto::Index as prost::Message>::decode(bytes.as_slice())
        .map_err(|e| IndexError::Scip(format!("the SCIP file is not valid: {e}")))?;
    drop(bytes);
    work.close().map_err(io(&work_dir))?;

    let run = ScipRun {
        work_dir,
        elapsed: started.elapsed(),
        build_script_command,
        proc_macro_server_started,
        rust_analyzer,
        std_source: logged.first().map(PathBuf::from),
        left_out_targets,
        documents: index.documents.len(),
        occurrences: index.documents.iter().map(|d| d.occurrences.len()).sum(),
    };
    Ok((run, index, crates))
}

/// The `cargo` of the temporary sysroot: the prepared metadata for the
/// repository, nothing for a build command, the real `cargo` for the rest.
fn write_cargo_script(
    path: &Path,
    manifest: &Path,
    metadata: &Path,
    real_cargo: &Path,
) -> std::io::Result<()> {
    let quote = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    let script = format!(
        "#!/bin/sh\n\
         # Written by kairos-index for one rust-analyzer scip run. It builds nothing.\n\
         case \"$1\" in\n\
         \x20 metadata)\n\
         \x20   for arg in \"$@\"; do\n\
         \x20     if [ \"$arg\" = {manifest} ]; then exec cat {metadata}; fi\n\
         \x20   done ;;\n\
         \x20 build|check|run|test|bench|rustc|doc|install|fix|clippy) exit 0 ;;\n\
         esac\n\
         exec {cargo} \"$@\"\n",
        manifest = quote(manifest),
        metadata = quote(metadata),
        cargo = quote(real_cargo),
    );
    fs::write(path, script)?;
    make_executable(path)
}

/// Run `command` with the tools of `bin`: `RUSTC` is its `rustc`, and `PATH`
/// starts with it. Cargo finds `rustc` through `RUSTC` or `PATH`, and the
/// server image has no `rustc` on `PATH`: its toolchain is in the tools
/// folder (COLLIERY-T-3107).
fn with_toolchain<'c>(command: &'c mut Command, bin: &Path) -> &'c mut Command {
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    command.env("RUSTC", bin.join("rustc"));
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    command
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

/// `cargo metadata` of the workspace. Like rust-analyzer, it gives cargo a
/// copy of `Cargo.lock`, so the repository is not changed.
fn cargo_metadata(
    root: &Path,
    manifest: &Path,
    sysroot: &Path,
    work_dir: &Path,
) -> Result<Value, IndexError> {
    let lock_dir = work_dir.join("lock");
    fs::create_dir(&lock_dir).map_err(|source| IndexError::Io {
        path: lock_dir.clone(),
        source,
    })?;
    let lock_copy = lock_dir.join("Cargo.lock");
    let lock = root.join("Cargo.lock");
    if lock.is_file() {
        fs::copy(&lock, &lock_copy).map_err(|source| IndexError::Io { path: lock, source })?;
    }
    // Cargo up to about 1.95 takes the unstable `-Zunstable-options
    // --lockfile-path <path>`. Later cargo (1.96 tested) refuses that flag and
    // takes `-Zlockfile-path --config resolver.lockfile-path=<path>` instead.
    // A repository can pin either, so try the old form and fall back.
    // Each feature on (KAIROS-A-0083): rust-analyzer takes the active
    // features of each package from `resolve`, so code behind a feature gets
    // SCIP data. A workspace whose features cannot be on together falls back
    // to the default features.
    let run = |form: LockfileForm, all_features: bool| {
        let mut command = Command::new(sysroot.join("bin/cargo"));
        with_toolchain(&mut command, &sysroot.join("bin"))
            .arg("metadata")
            .args(["--format-version", "1"])
            .args(all_features.then_some("--all-features"))
            .arg("--manifest-path")
            .arg(manifest)
            .args(lockfile_args(form, &lock_copy))
            .current_dir(root)
            // The same switch as rust-analyzer, for the unstable lockfile path.
            .env("__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS", "nightly")
            .env("CARGO_TARGET_DIR", work_dir.join("target"));
        command
            .output()
            .map_err(|e| IndexError::Scip(format!("cargo metadata did not run: {e}")))
    };
    let attempt = |all_features: bool| {
        let output = run(LockfileForm::Flag, all_features)?;
        if !output.status.success()
            && refuses_lockfile_flag(&String::from_utf8_lossy(&output.stderr))
        {
            return run(LockfileForm::Config, all_features);
        }
        Ok(output)
    };
    let mut output = attempt(true)?;
    if !output.status.success() {
        output = attempt(false)?;
    }
    if !output.status.success() {
        return Err(IndexError::Scip(failure_text(
            "cargo metadata",
            output.status,
            &String::from_utf8_lossy(&output.stderr),
            "",
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| IndexError::Scip(format!("cargo metadata is not valid JSON: {e}")))
}

/// The metadata of one run, the root files of the left-out targets and the
/// crate of each module file, from `root`. With `leave_out`, the targets
/// that share a module file with an earlier target are left out.
fn prepare_metadata(
    root: &Path,
    metadata: &Value,
    leave_out: bool,
) -> Result<(Value, Vec<String>, Crates), IndexError> {
    let mut metadata = metadata.clone();
    let parser = LanguageParser::new().map_err(|e| IndexError::Parser(e.to_string()))?;
    let shared = shared_targets(&mut metadata, &parser, leave_out);
    let relative = |p: &Path| {
        p.strip_prefix(root)
            .map_or_else(|_| p.to_string_lossy(), |r| r.to_string_lossy())
            .replace('\\', "/")
    };
    let left_out = shared
        .left_out
        .iter()
        .map(|p| relative(Path::new(p)))
        .collect();
    let crates = Crates {
        file_crate: shared
            .file_crate
            .iter()
            .map(|(file, c)| (relative(file), *c))
            .collect(),
        libraries: shared.libraries,
    };
    Ok((metadata, left_out, crates))
}

/// What `shared_targets` found.
struct SharedTargets {
    /// The root files of the left-out targets.
    left_out: Vec<String>,
    /// The first kept target of each module file.
    file_crate: HashMap<PathBuf, usize>,
    libraries: HashSet<usize>,
}

/// The crate of each module file: the first target (library first, then
/// binaries, then the rest by name) whose module tree has it. With
/// `leave_out`, remove from the workspace members each target that shares a
/// module file with an earlier target.
fn shared_targets(metadata: &mut Value, parser: &LanguageParser, leave_out: bool) -> SharedTargets {
    let members: HashSet<String> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str().map(str::to_string))
        .collect();
    let mut out = SharedTargets {
        left_out: Vec::new(),
        file_crate: HashMap::new(),
        libraries: HashSet::new(),
    };
    let mut next_crate = 0;
    let Some(packages) = metadata["packages"].as_array_mut() else {
        return out;
    };
    for package in packages {
        if !package["id"]
            .as_str()
            .is_some_and(|id| members.contains(id))
        {
            continue;
        }
        let Some(targets) = package["targets"].as_array_mut() else {
            continue;
        };
        let rank = |t: &Value| {
            let kinds: Vec<&str> = t["kind"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let order = if kinds.iter().any(|k| {
                matches!(
                    *k,
                    "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                )
            }) {
                0
            } else if kinds.contains(&"bin") {
                1
            } else {
                2
            };
            (order, t["name"].as_str().unwrap_or_default().to_string())
        };
        let mut order: Vec<usize> = (0..targets.len()).collect();
        order.sort_by_key(|&i| rank(&targets[i]));
        let mut drop = HashSet::new();
        for i in order {
            let Some(src) = targets[i]["src_path"].as_str() else {
                continue;
            };
            let files = module_files(parser, Path::new(src));
            if leave_out && files.iter().any(|f| out.file_crate.contains_key(f)) {
                drop.insert(i);
                out.left_out.push(src.to_string());
            } else {
                if rank(&targets[i]).0 == 0 {
                    out.libraries.insert(next_crate);
                }
                for f in files {
                    out.file_crate.entry(f).or_insert(next_crate);
                }
                next_crate += 1;
            }
        }
        let mut i = 0;
        targets.retain(|_| {
            let keep = !drop.contains(&i);
            i += 1;
            keep
        });
    }
    out.left_out.sort();
    out
}

/// The files of the module tree of a crate root: the root and each file
/// that a `mod name;` names, at `name.rs`, `name/mod.rs` or a
/// `#[path = "…"]`.
fn module_files(parser: &LanguageParser, root_file: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    // (file, whether its child modules are in its own folder)
    let mut stack = vec![(root_file.to_path_buf(), true)];
    while let Some((file, owns_dir)) = stack.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        out.push(file.clone());
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(tree) = parser.parse_to_tree(&file, &text) else {
            continue;
        };
        let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
        let base = if owns_dir || file.file_name().is_some_and(|n| n == "mod.rs") {
            dir.clone()
        } else {
            dir.join(file.file_stem().unwrap_or_default())
        };
        child_modules(tree.root_node(), text.as_bytes(), &base, &dir, &mut stack);
    }
    out
}

fn child_modules(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    base: &Path,
    dir: &Path,
    stack: &mut Vec<(PathBuf, bool)>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "mod_item" {
            continue;
        }
        let Some(name) = child
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
        else {
            continue;
        };
        if let Some(body) = child.child_by_field_name("body") {
            child_modules(body, source, &base.join(name), dir, stack);
            continue;
        }
        if let Some(path) = path_attribute(child, source) {
            stack.push((dir.join(path), true));
            continue;
        }
        let flat = base.join(format!("{name}.rs"));
        let nested = base.join(name).join("mod.rs");
        if flat.is_file() {
            stack.push((flat, false));
        } else if nested.is_file() {
            stack.push((nested, true));
        }
    }
}

/// The value of a `#[path = "…"]` attribute of a `mod` item.
fn path_attribute(item: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut sibling = item.prev_sibling();
    while let Some(s) = sibling {
        match s.kind() {
            "attribute_item" => {
                let text = s.utf8_text(source).ok()?;
                let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                if let Some(rest) = compact.strip_prefix("#[path=\"") {
                    return rest.strip_suffix("\"]").map(str::to_string);
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return None,
        }
        sibling = s.prev_sibling();
    }
    None
}

/// How `cargo metadata` is told to write its lock file to a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockfileForm {
    /// `-Zunstable-options --lockfile-path <path>`: cargo up to about 1.95.
    Flag,
    /// `-Zlockfile-path --config resolver.lockfile-path=<path>`: later cargo.
    Config,
}

fn lockfile_args(form: LockfileForm, lock_copy: &Path) -> Vec<std::ffi::OsString> {
    match form {
        LockfileForm::Flag => vec![
            "-Zunstable-options".into(),
            "--lockfile-path".into(),
            lock_copy.as_os_str().to_owned(),
        ],
        LockfileForm::Config => {
            // A TOML string; the path is quoted and its backslashes and quotes escaped.
            let path = lock_copy
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            vec![
                "-Zlockfile-path".into(),
                "--config".into(),
                format!("resolver.lockfile-path=\"{path}\"").into(),
            ]
        }
    }
}

/// True when cargo refused the old `--lockfile-path` flag (cargo 1.96 and
/// later), so the run must use [`LockfileForm::Config`].
fn refuses_lockfile_flag(stderr: &str) -> bool {
    stderr.contains("unexpected argument '--lockfile-path'")
}

/// The panic message of rust-analyzer, or the last lines of its output.
/// Why `program` failed, for the error of the run (KAIROS-T-0747): how it
/// ended (its exit status, or the signal that stopped it), then the panic
/// of its output if it has one, else the last lines of its output and of
/// its log. The first lines ("Loading cargo metadata") tell nothing.
fn failure_text(program: &str, status: ExitStatus, stderr: &str, log: &str) -> String {
    let mut text = format!("{program} {}", ended(status));
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    if let Some(i) = lines.iter().position(|l| l.contains("panicked at")) {
        text.push_str(". Its panic: ");
        text.push_str(&lines[i..lines.len().min(i + 2)].join(" "));
        return text;
    }
    let last = |lines: &[&str]| lines[lines.len().saturating_sub(FAILURE_LINES)..].join("\n");
    if !lines.is_empty() {
        text.push_str(". The last lines of its output:\n");
        text.push_str(&last(&lines));
    }
    let log: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
    if !log.is_empty() {
        text.push_str("\nThe last lines of its log:\n");
        text.push_str(&last(&log));
    }
    text
}

/// How a process ended: "exited with status 101", or "was stopped by
/// signal 9 (SIGKILL): out of memory?".
fn ended(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exited with status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return match signal {
                9 => "was stopped by signal 9 (SIGKILL): out of memory? The kernel stops \
                      the largest process when a container is at its memory limit"
                    .to_string(),
                15 => "was stopped by signal 15 (SIGTERM)".to_string(),
                6 => "was stopped by signal 6 (SIGABRT)".to_string(),
                11 => "was stopped by signal 11 (SIGSEGV)".to_string(),
                n => format!("was stopped by signal {n}"),
            };
        }
    }
    format!("ended with {status}")
}

/// The root of the cgroup file system.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// The memory limit of the cgroup of this process, in bytes, under the
/// cgroup file system at `root`: `memory.max` (cgroup v2) or
/// `memory/memory.limit_in_bytes` (cgroup v1). None when there is no
/// limit, or no cgroup file system (macOS).
fn memory_limit(root: &Path) -> Option<u64> {
    let parse = |text: &str| -> Option<u64> {
        let value: u64 = text.trim().parse().ok()?;
        // cgroup v1 gives a number near 2^63 for "no limit".
        (value < 1 << 62).then_some(value)
    };
    // cgroup v2: the folder of this process ("0::/path" in /proc/self/cgroup),
    // else the root, which is the folder of the container in a cgroup
    // namespace.
    let own = fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("0::").map(|p| p.trim().to_string()))
        })
        .map(|p| root.join(p.trim_start_matches('/')));
    for dir in own.iter().map(PathBuf::as_path).chain([root]) {
        if let Ok(text) = fs::read_to_string(dir.join("memory.max")) {
            return parse(&text);
        }
    }
    fs::read_to_string(root.join("memory/memory.limit_in_bytes"))
        .ok()
        .and_then(|t| parse(&t))
}

/// Refuse the run when the memory limit is below `need` (KAIROS-T-0747).
/// The kernel would stop rust-analyzer near the end of the run, after
/// minutes of work, with no message.
fn check_memory_limit(limit: Option<u64>, need: u64) -> Result<(), IndexError> {
    match limit {
        Some(limit) if limit < need => Err(IndexError::ScipMemory {
            limit_mib: limit / (1024 * 1024),
            need_mib: need / (1024 * 1024),
        }),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(not(unix))]
fn link(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "the SCIP run needs symbolic links (Linux or macOS)",
    ))
}

/// The build-script command in the log: `Running build scripts: cd "…" &&
/// KEY="v" "true"` gives `true`.
fn build_script_command(log: &str) -> Option<String> {
    let line = log.lines().find(|l| l.contains("Running build scripts:"))?;
    let command = line.rsplit_once("&& ").map_or(line, |(_, c)| c);
    let words: Vec<String> = command
        .split_whitespace()
        .filter(|w| !w.contains('=') || w.starts_with('"'))
        .map(|w| w.trim_matches('"').to_string())
        .collect();
    Some(words.join(" "))
}

/// The std sources in the log: `Using sysroot … src_root=Some(AbsPath("…"))`
/// gives the path in the quotes.
fn std_sources(log: &str) -> Vec<String> {
    const START: &str = "src_root=Some(AbsPath(\"";
    log.lines()
        .filter(|l| l.contains("Using sysroot"))
        .filter_map(|l| {
            let rest = &l[l.find(START)? + START.len()..];
            Some(rest[..rest.find("\"))")?].to_string())
        })
        .collect()
}

/// Whether `logged` is the folder `path`, after links are followed.
fn same_path(logged: &str, path: &Path) -> bool {
    fs::canonicalize(logged).is_ok_and(|p| p == path)
}

fn proc_macro_server_started(log: &str) -> Option<bool> {
    if log.contains("Proc-macro server started") {
        Some(true)
    } else if log.contains("Failed to start proc-macro server")
        || log.contains("No proc-macro server started")
    {
        Some(false)
    } else {
        None
    }
}

/// One descriptor of a SCIP symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Descriptor {
    Namespace(String),
    Type(String),
    Term(String),
    Method(String),
    TypeParameter(String),
    Parameter(String),
    Meta(String),
    Macro(String),
}

/// The descriptors of a global SCIP symbol (`<scheme> <manager> <package>
/// <version> <descriptors>`). `None` for a local symbol or a symbol that is
/// not valid.
pub fn descriptors(symbol: &str) -> Option<Vec<Descriptor>> {
    if symbol.starts_with("local ") {
        return None;
    }
    let mut rest = symbol;
    for _ in 0..4 {
        rest = skip_field(rest)?;
    }
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        match chars[i] {
            '[' => {
                let (name, next) = read_name(&chars, i + 1)?;
                (chars.get(next) == Some(&']')).then_some(())?;
                out.push(Descriptor::TypeParameter(name));
                i = next + 1;
            }
            '(' => {
                let (name, next) = read_name(&chars, i + 1)?;
                (chars.get(next) == Some(&')')).then_some(())?;
                out.push(Descriptor::Parameter(name));
                i = next + 1;
            }
            _ => {
                let (name, next) = read_name(&chars, i)?;
                let (descriptor, next) = match chars.get(next)? {
                    '/' => (Descriptor::Namespace(name), next + 1),
                    '#' => (Descriptor::Type(name), next + 1),
                    '.' => (Descriptor::Term(name), next + 1),
                    ':' => (Descriptor::Meta(name), next + 1),
                    '!' => (Descriptor::Macro(name), next + 1),
                    '(' => {
                        let close = (next..chars.len()).find(|&j| chars[j] == ')')?;
                        (chars.get(close + 1) == Some(&'.')).then_some(())?;
                        (Descriptor::Method(name), close + 2)
                    }
                    _ => return None,
                };
                out.push(descriptor);
                i = next;
            }
        }
    }
    Some(out)
}

/// Skip one space-ended field of the symbol header. 2 spaces are one space
/// in the field.
fn skip_field(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            if bytes.get(i + 1) == Some(&b' ') {
                i += 2;
                continue;
            }
            return Some(&s[i + 1..]);
        }
        i += 1;
    }
    None
}

/// A simple name, or a name in backticks (2 backticks are one).
fn read_name(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    if chars.get(start) == Some(&'`') {
        let mut i = start + 1;
        loop {
            match chars.get(i)? {
                '`' if chars.get(i + 1) == Some(&'`') => {
                    name.push('`');
                    i += 2;
                }
                '`' => return Some((name, i + 1)),
                c => {
                    name.push(*c);
                    i += 1;
                }
            }
        }
    }
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        if c.is_alphanumeric() || matches!(c, '_' | '+' | '-' | '$') {
            name.push(c);
            i += 1;
        } else {
            break;
        }
    }
    Some((name, i))
}

/// The method name of a function symbol and a short name for it: the type
/// or module that holds it, then the name. In Rust, `Vec::push`,
/// `mem::swap`, `Shape::area`; in the other languages `Cart.total`,
/// `greet.Hello` (the last part of a Go package path), `report.load` (the
/// last part of a Python module), `queue.enqueue` (a TypeScript file with no
/// extension). `None` if the symbol is not a function.
pub fn function_name(descriptors: &[Descriptor], language: &str) -> Option<(String, String)> {
    match descriptors.last()? {
        Descriptor::Method(_) => owned_name(descriptors, language),
        _ => None,
    }
}

/// The name of a term symbol at a call site, as [`function_name`] gives
/// it: a method of a Go interface (`Shape#Area.`), a function in a
/// variable (`const f = () => …`). `None` if the symbol is not a term.
pub fn term_name(descriptors: &[Descriptor], language: &str) -> Option<(String, String)> {
    match descriptors.last()? {
        Descriptor::Term(_) => owned_name(descriptors, language),
        _ => None,
    }
}

/// The name of the last descriptor, and the short name with its owner.
fn owned_name(descriptors: &[Descriptor], language: &str) -> Option<(String, String)> {
    let (last, rest) = descriptors.split_last()?;
    let (Descriptor::Method(name) | Descriptor::Term(name)) = last else {
        return None;
    };
    let owner = rest
        .iter()
        .rposition(|d| matches!(d, Descriptor::Type(_) | Descriptor::Namespace(_)))
        .and_then(|i| match (&rest[i], rest.get(i + 1)) {
            // `impl#[Self][Trait]`: the type of the impl.
            (Descriptor::Type(t), Some(Descriptor::TypeParameter(self_type))) if t == "impl" => {
                Some(self_type.clone())
            }
            (Descriptor::Type(t), _) => Some(t.clone()),
            (Descriptor::Namespace(t), _) if language == "rust" => Some(t.clone()),
            (Descriptor::Namespace(t), _) => Some(module_name(t, language).to_string()),
            _ => None,
        })
        .map(|owner| owner.split('<').next().unwrap_or_default().to_string())
        .filter(|owner| !owner.is_empty());
    let separator = if language == "rust" { "::" } else { "." };
    let short = match owner {
        Some(owner) => format!("{owner}{separator}{name}"),
        None => name.clone(),
    };
    Some((name.clone(), short))
}

/// The short name of a module namespace of a SCIP symbol that is not Rust.
fn module_name<'a>(namespace: &'a str, language: &str) -> &'a str {
    let last = namespace.rsplit('/').next().unwrap_or(namespace);
    match language {
        "python" => last.rsplit('.').next().unwrap_or(last),
        "typescript" | "tsx" | "javascript" => [
            ".d.ts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs",
        ]
        .iter()
        .find_map(|ext| last.strip_suffix(ext))
        .unwrap_or(last),
        _ => last,
    }
}

/// The name of a type symbol, for a call that makes an object (`Report()`,
/// `new Queue()`): the name and the same name as the short name. `None` if
/// the symbol is not a type.
pub fn type_name(descriptors: &[Descriptor]) -> Option<(String, String)> {
    match descriptors.last()? {
        Descriptor::Type(name) => {
            let name = name.split('<').next().unwrap_or(name).to_string();
            Some((name.clone(), name))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0747: the error names how rust-analyzer ended, and gives the
    /// last lines of its output and its log, not the first.
    #[cfg(unix)]
    #[test]
    fn a_failed_run_names_the_signal_and_the_last_lines() {
        use std::os::unix::process::ExitStatusExt;
        let stderr: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let killed = failure_text(
            "rust-analyzer",
            ExitStatus::from_raw(9),
            &stderr,
            "WARN a\nWARN b\n",
        );
        assert!(
            killed.starts_with("rust-analyzer was stopped by signal 9 (SIGKILL): out of memory?"),
            "{killed}"
        );
        assert!(killed.contains("line 20"), "{killed}");
        assert!(killed.contains("line 13"), "{killed}");
        assert!(!killed.contains("line 12\n"), "{killed}");
        assert!(killed.ends_with("WARN a\nWARN b"), "{killed}");

        let exited = failure_text("cargo metadata", ExitStatus::from_raw(101 << 8), "", "");
        assert_eq!(exited, "cargo metadata exited with status 101");

        let panic = failure_text(
            "rust-analyzer",
            ExitStatus::from_raw(101 << 8),
            "Loading\nthread 'main' panicked at src/x.rs:1:1:\nfile emitted multiple times\nmore\n",
            "",
        );
        assert_eq!(
            panic,
            "rust-analyzer exited with status 101. Its panic: thread 'main' panicked at \
             src/x.rs:1:1: file emitted multiple times"
        );
    }

    /// KAIROS-T-0747: the memory limit comes from cgroup v2, else v1; "max"
    /// and the v1 value for no limit are no limit.
    #[test]
    fn the_memory_limit_comes_from_the_cgroup() {
        let v2 = tempfile::tempdir().unwrap();
        fs::write(v2.path().join("memory.max"), "8589934592\n").unwrap();
        assert_eq!(memory_limit(v2.path()), Some(8 << 30));
        fs::write(v2.path().join("memory.max"), "max\n").unwrap();
        assert_eq!(memory_limit(v2.path()), None);

        let v1 = tempfile::tempdir().unwrap();
        fs::create_dir(v1.path().join("memory")).unwrap();
        fs::write(v1.path().join("memory/memory.limit_in_bytes"), "4294967296").unwrap();
        assert_eq!(memory_limit(v1.path()), Some(4 << 30));
        fs::write(
            v1.path().join("memory/memory.limit_in_bytes"),
            "9223372036854771712",
        )
        .unwrap();
        assert_eq!(memory_limit(v1.path()), None);

        let none = tempfile::tempdir().unwrap();
        assert_eq!(memory_limit(none.path()), None);
    }

    /// KAIROS-T-0747: a limit below the need refuses the run with a text
    /// that names both and the setting; no limit or enough memory runs.
    #[test]
    fn a_memory_limit_below_the_need_refuses_the_run() {
        let need = 6144 << 20;
        let err = check_memory_limit(Some(4096 << 20), need).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("4096 MiB"), "{text}");
        assert!(text.contains("6144 MiB"), "{text}");
        assert!(text.contains("KAIROS_CODE_INDEX_SCIP_MEMORY_MIB"), "{text}");
        assert!(check_memory_limit(Some(8192 << 20), need).is_ok());
        assert!(check_memory_limit(Some(need), need).is_ok());
        assert!(check_memory_limit(None, need).is_ok());
    }

    /// KAIROS-T-0747: the check runs before the tools are read, so a
    /// refused run needs no rust-analyzer. On a host with no limit (macOS,
    /// most CI runners) the run goes on and fails on the missing binary.
    #[test]
    fn the_memory_check_runs_first() {
        let options = crate::BuildOptions {
            rust_analyzer: Some(PathBuf::from("/nonexistent/rust-analyzer")),
            download: false,
            min_memory_bytes: Some(u64::MAX),
            ..Default::default()
        };
        let err = run(Path::new("."), &options).unwrap_err();
        match memory_limit(Path::new(CGROUP_ROOT)) {
            Some(_) => assert!(matches!(err, IndexError::ScipMemory { .. }), "{err}"),
            None => assert!(matches!(err, IndexError::RustAnalyzerMissing(_)), "{err}"),
        }
    }

    #[test]
    fn the_lock_file_copy_has_a_form_for_old_and_new_cargo() {
        let lock = Path::new("/tmp/w x/Cargo.lock");
        assert_eq!(
            lockfile_args(LockfileForm::Flag, lock),
            [
                "-Zunstable-options",
                "--lockfile-path",
                "/tmp/w x/Cargo.lock"
            ]
            .map(std::ffi::OsString::from)
        );
        assert_eq!(
            lockfile_args(LockfileForm::Config, lock),
            [
                "-Zlockfile-path",
                "--config",
                "resolver.lockfile-path=\"/tmp/w x/Cargo.lock\"",
            ]
            .map(std::ffi::OsString::from)
        );
        // The refusal of cargo 1.96.1 (weir pins it), and not another failure.
        assert!(refuses_lockfile_flag(
            "error: unexpected argument '--lockfile-path' found\n\n  tip: a similar argument exists: '--locked'"
        ));
        assert!(!refuses_lockfile_flag("error: failed to parse manifest"));
    }

    /// COLLIERY-T-3107: in the server image, `rustc` is not on `PATH`. The
    /// metadata step must use the `rustc` of the given toolchain. The test
    /// toolchain has a `rustc` that records each call, so a `rustc` found on
    /// `PATH` does not make the test pass.
    /// KAIROS-T-0760: the metadata has each feature on, so rust-analyzer
    /// gives SCIP data for the code behind a feature.
    #[test]
    fn cargo_metadata_turns_on_each_feature() {
        let Some(sysroot) = Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        else {
            eprintln!("no rustc on this computer; the test does nothing");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("crate");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             [features]\nextra = []\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        let work = dir.path().join("work");
        fs::create_dir(&work).unwrap();
        let metadata = cargo_metadata(&root, &root.join("Cargo.toml"), &sysroot, &work).unwrap();
        assert_eq!(
            metadata["resolve"]["nodes"][0]["features"],
            serde_json::json!(["extra"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn cargo_metadata_uses_the_rustc_of_the_toolchain() {
        let Some(real) = Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        else {
            eprintln!("no rustc on this computer; the test does nothing");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("toolchain/bin");
        fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(real.join("bin/cargo"), bin.join("cargo")).unwrap();
        let marker = dir.path().join("rustc-called");
        fs::write(
            bin.join("rustc"),
            format!(
                "#!/bin/sh\ntouch '{}'\nexec '{}' \"$@\"\n",
                marker.display(),
                real.join("bin/rustc").display()
            ),
        )
        .unwrap();
        make_executable(&bin.join("rustc")).unwrap();

        let root = dir.path().join("crate");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        let work = dir.path().join("work");
        fs::create_dir(&work).unwrap();

        let metadata = cargo_metadata(
            &root,
            &root.join("Cargo.toml"),
            &dir.path().join("toolchain"),
            &work,
        )
        .unwrap();
        assert_eq!(metadata["packages"][0]["name"], "t");
        assert!(
            marker.exists(),
            "cargo did not run the rustc of the toolchain"
        );
    }

    fn short(symbol: &str) -> Option<String> {
        function_name(&descriptors(symbol)?, "rust").map(|(_, s)| s)
    }

    #[test]
    fn function_names_of_rust_analyzer_symbols() {
        let cases = [
            (
                "rust-analyzer cargo polyglot 0.0.0 queue/impl#[Queue]push().",
                "Queue::push",
            ),
            (
                "rust-analyzer cargo polyglot 0.0.0 shapes/Shape#area().",
                "Shape::area",
            ),
            (
                "rust-analyzer cargo polyglot 0.0.0 shapes/impl#[Square][Shape]area().",
                "Square::area",
            ),
            (
                "rust-analyzer cargo polyglot 0.0.0 enqueue_all().",
                "enqueue_all",
            ),
            (
                "rust-analyzer cargo alloc https://github.com/rust-lang/rust/library/alloc collections/vec_deque/impl#[`VecDeque<T, A>`]len().",
                "VecDeque::len",
            ),
            (
                "rust-analyzer cargo core https://github.com/rust-lang/rust/library/core slice/impl#[`[T]`]iter().",
                "[T]::iter",
            ),
            ("rust-analyzer cargo core 1.0 mem/swap().", "mem::swap"),
        ];
        for (symbol, want) in cases {
            assert_eq!(short(symbol).as_deref(), Some(want), "{symbol}");
        }
    }

    /// KAIROS-T-0473: the short names of the symbols of scip-go,
    /// scip-typescript and scip-python.
    #[test]
    fn function_names_of_other_indexers() {
        let cases = [
            (
                "scip-go gomod example.com/m v1.0.0 `example.com/m/greet`/Hello().",
                "go",
                "greet.Hello",
            ),
            (
                "scip-go gomod example.com/m v1.0.0 `example.com/m/greet`/Greeter#Greet().",
                "go",
                "Greeter.Greet",
            ),
            (
                "scip-typescript npm web 1.0.0 src/`queue.ts`/enqueue().",
                "typescript",
                "queue.enqueue",
            ),
            (
                "scip-typescript npm web 1.0.0 src/`queue.ts`/Queue#push().",
                "typescript",
                "Queue.push",
            ),
            (
                "scip-python python polyglot 0.1.0 `polyglot.report`/load().",
                "python",
                "report.load",
            ),
            (
                "scip-python python polyglot 0.1.0 `polyglot.report`/Report#render().",
                "python",
                "Report.render",
            ),
        ];
        for (symbol, language, want) in cases {
            let got = function_name(&descriptors(symbol).unwrap(), language).map(|(_, s)| s);
            assert_eq!(got.as_deref(), Some(want), "{symbol}");
        }
        let class =
            descriptors("scip-python python polyglot 0.1.0 `polyglot.report`/Report#").unwrap();
        assert_eq!(function_name(&class, "python"), None);
        assert_eq!(type_name(&class), Some(("Report".into(), "Report".into())));
    }

    #[test]
    fn other_symbols_are_not_functions() {
        for symbol in [
            "local 12",
            "rust-analyzer cargo polyglot 0.0.0 queue/Queue#",
            "rust-analyzer cargo polyglot 0.0.0 stack/Stack#items.",
            "rust-analyzer cargo core 1.0 macros/assert_eq!",
        ] {
            assert_eq!(short(symbol), None, "{symbol}");
        }
    }

    #[test]
    fn a_target_that_shares_a_module_is_left_out_only_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/inner")).unwrap();
        fs::create_dir_all(root.join("tests/common")).unwrap();
        fs::write(root.join("src/lib.rs"), "mod inner;\nmod flat;\n").unwrap();
        fs::write(root.join("src/inner/mod.rs"), "mod deep;\n").unwrap();
        fs::write(root.join("src/inner/deep.rs"), "").unwrap();
        fs::write(root.join("src/flat.rs"), "").unwrap();
        fs::write(root.join("tests/common/mod.rs"), "").unwrap();
        fs::write(root.join("tests/b.rs"), "mod common;\n").unwrap();
        fs::write(root.join("tests/a.rs"), "mod common;\n").unwrap();
        fs::write(
            root.join("tests/c.rs"),
            "#[path = \"common/mod.rs\"]\nmod shared;\n",
        )
        .unwrap();
        let src = |p: &str| root.join(p).to_string_lossy().into_owned();
        let target = |name: &str, kind: &str, path: &str| serde_json::json!({ "name": name, "kind": [kind], "src_path": src(path) });
        let mut metadata = serde_json::json!({
            "workspace_members": ["p"],
            "packages": [{
                "id": "p",
                "targets": [
                    target("b", "test", "tests/b.rs"),
                    target("p", "lib", "src/lib.rs"),
                    target("c", "test", "tests/c.rs"),
                    target("a", "test", "tests/a.rs"),
                ],
            }],
        });
        let parser = LanguageParser::new().unwrap();
        let files = module_files(&parser, &root.join("src/lib.rs"));
        assert_eq!(files.len(), 4, "{files:?}");
        // With no defect, no target is left out, and a shared file is in the
        // first crate that has it.
        let mut each = metadata.clone();
        let all = shared_targets(&mut each, &parser, false);
        assert!(all.left_out.is_empty());
        assert_eq!(each, metadata);
        assert_eq!(
            all.file_crate[&root.join("tests/common/mod.rs")],
            all.file_crate[&root.join("tests/a.rs")]
        );
        let shared = shared_targets(&mut metadata, &parser, true);
        assert_eq!(shared.left_out, [src("tests/b.rs"), src("tests/c.rs")]);
        assert_eq!(shared.libraries.len(), 1);
        let lib = shared.file_crate[&root.join("src/inner/deep.rs")];
        assert!(shared.libraries.contains(&lib));
        assert_ne!(shared.file_crate[&root.join("tests/a.rs")], lib);
        let kept: Vec<&str> = metadata["packages"][0]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(kept, ["p", "a"]);
    }

    #[test]
    fn an_escaped_space_stays_in_the_header() {
        let d = descriptors("scheme man my  pkg 1.0 f().").unwrap();
        assert_eq!(d, [Descriptor::Method("f".into())]);
    }

    #[test]
    fn the_log_gives_the_build_script_command_and_the_server_state() {
        let log = "2026 INFO Running build scripts: cd \"/r\" && RUSTUP_AUTO_INSTALL=\"0\" \"true\"\n\
                   2026 INFO Failed to start proc-macro server manifest=/r/Cargo.toml e=cannot find\n";
        assert_eq!(build_script_command(log).as_deref(), Some("true"));
        assert_eq!(proc_macro_server_started(log), Some(false));
        assert_eq!(build_script_command(""), None);
        assert_eq!(
            proc_macro_server_started("INFO Proc-macro server started path=/x"),
            Some(true)
        );
    }

    #[test]
    fn the_log_gives_the_std_source() {
        let log = "2026 INFO Using sysroot workspace=/r/Cargo.toml \
                   src_root=Some(AbsPath(\"/c/rust-src-1.99.0/library\")) root=Some(AbsPath(\"/s\"))\n\
                   2026 INFO Loaded sysroot src_root=Some(AbsPath(\"/other\"))\n";
        assert_eq!(std_sources(log), ["/c/rust-src-1.99.0/library"]);
        assert!(std_sources("2026 INFO Using sysroot src_root=None").is_empty());
    }
}
