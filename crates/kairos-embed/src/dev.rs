//! The folders of the tests and the dev tools (KAIROS-T-0712).
//!
//! A test that caches the local model, or writes the OpenAPI spec, puts the
//! file in the cargo target folder. That folder is `CARGO_TARGET_DIR` when
//! it is set, else the `target/` folder of the workspace. The caller gives
//! the workspace root (from its own `CARGO_MANIFEST_DIR`), so no build path
//! goes into a product binary.

use std::path::{Path, PathBuf};

/// The cargo target folder: `CARGO_TARGET_DIR`, else `<workspace>/target`.
/// A relative value is relative to the workspace root, as for a cargo
/// command in the root.
pub fn target_dir(workspace: &Path) -> PathBuf {
    target_dir_from(workspace, std::env::var_os("CARGO_TARGET_DIR"))
}

fn target_dir_from(workspace: &Path, configured: Option<std::ffi::OsString>) -> PathBuf {
    match configured.filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                workspace.join(path)
            }
        }
        None => workspace.join("target"),
    }
}

/// The cache of the local model for the tests and the dev tools:
/// `KAIROS_EMBED_CACHE`, else `embed-cache` in [`target_dir`]. The folder
/// is made. `angreal dev fetch-model` fills the same folder.
pub fn model_cache(workspace: &Path) -> PathBuf {
    let cache = std::env::var_os("KAIROS_EMBED_CACHE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| target_dir(workspace).join("embed-cache"));
    let _ = std::fs::create_dir_all(&cache);
    cache
}

/// The workspace root of a crate in `crates/<name>`, from the
/// `CARGO_MANIFEST_DIR` of that crate.
pub fn workspace_of(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir).join("../..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_folder_follows_cargo_target_dir() {
        let workspace = Path::new("/w");
        assert_eq!(target_dir_from(workspace, None), Path::new("/w/target"));
        assert_eq!(
            target_dir_from(workspace, Some("".into())),
            Path::new("/w/target")
        );
        assert_eq!(
            target_dir_from(workspace, Some("/elsewhere/t".into())),
            Path::new("/elsewhere/t")
        );
        assert_eq!(
            target_dir_from(workspace, Some("build/t".into())),
            Path::new("/w/build/t")
        );
    }
}
