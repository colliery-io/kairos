//! KAIROS-T-0326: the trunk `pre_build` hook of `kairos-web` runs this to
//! write `style/aurora.css`, which `index.html` links as a normal
//! stylesheet. The page then has the Aurora styles at its first paint,
//! with no flash (the README of Aurora 0.4.1, "Linked stylesheet").
//!
//! Usage: `kairos-web-css <dir>`. Trunk runs it with the source directory
//! of the web crate in `TRUNK_SOURCE_DIR`; a relative `<dir>` is read from
//! there.

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "style".into()));
    let dir = match std::env::var_os("TRUNK_SOURCE_DIR") {
        Some(source) if dir.is_relative() => PathBuf::from(source).join(dir),
        _ => dir,
    };
    aurora_dark::write_css(&dir)?;
    Ok(())
}
