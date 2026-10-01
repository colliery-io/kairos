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
//! **A file in 2 crates.** rust-analyzer 1.93.0 `scip` panics when one file
//! is a module of 2 crates ("Invariant violation: file emitted multiple
//! times"), for example `tests/common/mod.rs` of 2 integration tests. The
//! prepared metadata leaves out each target that shares a module file with
//! an earlier target (library first, then binaries, then the rest by name).
//! The calls of a left-out target get name classes.
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
use std::process::Command;
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
    /// `PositionEncoding.UTF16CodeUnitOffsetFromLineStart`.
    pub const ENCODING_UTF16: i32 = 2;
    /// `PositionEncoding.UTF32CodeUnitOffsetFromLineStart`.
    pub const ENCODING_UTF32: i32 = 3;
}

/// The log targets that tell what rust-analyzer ran. The rest of its info
/// log is large and is not needed.
const RA_LOG: &str = "warn,load_cargo=info,project_model::build_dependencies=info";

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
pub fn run(root: &Path) -> Result<(ScipRun, proto::Index, Crates), IndexError> {
    let started = Instant::now();
    let root = fs::canonicalize(root).map_err(|source| IndexError::Io {
        path: root.to_path_buf(),
        source,
    })?;

    // The rustup proxies choose the toolchain of the repository.
    let version = Command::new("rust-analyzer")
        .arg("--version")
        .current_dir(&root)
        .output();
    if !version.is_ok_and(|o| o.status.success()) {
        return Err(IndexError::MissingComponent("rust-analyzer"));
    }
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .ok_or_else(|| IndexError::Scip("rustc --print sysroot did not run".into()))?;
    if !sysroot.join("lib/rustlib/src/rust/library").is_dir() {
        return Err(IndexError::MissingComponent("rust-src"));
    }

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

    // The metadata of the workspace, with the targets left out that share a
    // module file with an earlier target.
    let manifest = root.join("Cargo.toml");
    let (metadata, left_out_targets, crates) =
        prepared_metadata(&root, &manifest, &sysroot, &work_dir)?;
    let metadata_path = work_dir.join("metadata.json");
    fs::write(&metadata_path, metadata.to_string()).map_err(io(&metadata_path))?;

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
            "targetDir": target_dir,
        },
        "procMacro": { "enable": false },
    });
    let config_path = work_dir.join("rust-analyzer.json");
    fs::write(&config_path, config.to_string()).map_err(io(&config_path))?;

    let log_path = work_dir.join("rust-analyzer.log");
    let output_path = work_dir.join("index.scip");
    // The rust-analyzer of the toolchain, or the one on the PATH.
    let tool = sysroot.join("bin/rust-analyzer");
    let program = if tool.is_file() {
        tool
    } else {
        PathBuf::from("rust-analyzer")
    };
    let output = Command::new(program)
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
    if !output.status.success() {
        return Err(IndexError::Scip(failure_text(&String::from_utf8_lossy(
            &output.stderr,
        ))));
    }

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

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// `cargo metadata` of the workspace, with the targets left out that share
/// a module file with an earlier target. Like rust-analyzer, it gives cargo
/// a copy of `Cargo.lock`, so the repository is not changed. Returns the
/// metadata, the root files of the left-out targets and the crate of each
/// module file, from `root`.
fn prepared_metadata(
    root: &Path,
    manifest: &Path,
    sysroot: &Path,
    work_dir: &Path,
) -> Result<(Value, Vec<String>, Crates), IndexError> {
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
    let output = Command::new(sysroot.join("bin/cargo"))
        .arg("metadata")
        .args(["--format-version", "1", "-Zunstable-options"])
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--lockfile-path")
        .arg(&lock_copy)
        .current_dir(root)
        // The same switch as rust-analyzer, for the unstable --lockfile-path.
        .env("__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS", "nightly")
        .env("CARGO_TARGET_DIR", work_dir.join("target"))
        .output()
        .map_err(|e| IndexError::Scip(format!("cargo metadata did not run: {e}")))?;
    if !output.status.success() {
        return Err(IndexError::Scip(format!(
            "cargo metadata failed: {}",
            failure_text(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    let mut metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| IndexError::Scip(format!("cargo metadata is not valid JSON: {e}")))?;
    let parser = LanguageParser::new().map_err(|e| IndexError::Parser(e.to_string()))?;
    let shared = leave_out_shared_targets(&mut metadata, &parser);
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

/// What `leave_out_shared_targets` found.
struct SharedTargets {
    /// The root files of the left-out targets.
    left_out: Vec<String>,
    /// The kept target of each module file.
    file_crate: HashMap<PathBuf, usize>,
    libraries: HashSet<usize>,
}

/// Remove from the workspace members each target that shares a module file
/// with an earlier target.
fn leave_out_shared_targets(metadata: &mut Value, parser: &LanguageParser) -> SharedTargets {
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
            if files.iter().any(|f| out.file_crate.contains_key(f)) {
                drop.insert(i);
                out.left_out.push(src.to_string());
            } else {
                if rank(&targets[i]).0 == 0 {
                    out.libraries.insert(next_crate);
                }
                out.file_crate
                    .extend(files.into_iter().map(|f| (f, next_crate)));
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

/// The panic message of rust-analyzer, or the last lines of its output.
fn failure_text(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    if let Some(i) = lines.iter().position(|l| l.contains("panicked at")) {
        return lines[i..lines.len().min(i + 2)].join(" ");
    }
    let start = lines.len().saturating_sub(5);
    lines[start..].join("\n")
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
/// or module that holds it, then the name (`Vec::push`, `mem::swap`,
/// `Shape::area`). `None` if the symbol is not a function.
pub fn function_name(descriptors: &[Descriptor]) -> Option<(String, String)> {
    let (Descriptor::Method(name), rest) = descriptors.split_last()? else {
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
            (Descriptor::Type(t) | Descriptor::Namespace(t), _) => Some(t.clone()),
            _ => None,
        })
        .map(|owner| owner.split('<').next().unwrap_or_default().to_string())
        .filter(|owner| !owner.is_empty());
    let short = match owner {
        Some(owner) => format!("{owner}::{name}"),
        None => name.clone(),
    };
    Some((name.clone(), short))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(symbol: &str) -> Option<String> {
        function_name(&descriptors(symbol)?).map(|(_, s)| s)
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
    fn a_target_that_shares_a_module_is_left_out() {
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
        let shared = leave_out_shared_targets(&mut metadata, &parser);
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
}
