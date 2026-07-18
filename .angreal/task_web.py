"""
GUI (kairos-web) build tasks — KAIROS-T-0039 (A-0015: Leptos CSR via trunk).

`angreal web build` is the one entry point CI, the release image, and dev
flows share: it ensures the wasm32 target + trunk exist (cargo-native, no
Homebrew — trunk installs into target/tools/ unless already on PATH) and
runs `trunk build` in crates/kairos-web. `angreal web lint` is the
conventions grep from docs/gui-conventions.md (tokens only, no hardcoded
colors).
"""

import re
import shutil
import subprocess
import sys
from pathlib import Path

import angreal  # type: ignore

from utils import PROJECT_ROOT

web = angreal.command_group(name="web", about="GUI (kairos-web) build commands")

WEB_CRATE = PROJECT_ROOT / "crates" / "kairos-web"
WEB_DIST = WEB_CRATE / "dist"
TOOLS_ROOT = PROJECT_ROOT / "target" / "tools"
# Pinned like the diesel CLI in task_db.py: upgrades are deliberate.
TRUNK_VERSION = "0.21.14"
WASM_TARGET = "wasm32-unknown-unknown"

# The docs/gui-conventions.md token rule: no raw colors anywhere in the
# GUI sources. Aurora tokens arrive as `token::*` constants or
# `var(--…)` CSS references, never as literals in this crate.
COLOR_LITERAL = re.compile(
    r"#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(", re.IGNORECASE
)
LINT_GLOBS = ("src/**/*.rs", "*.css", "index.html")


def _ensure_wasm_target() -> int:
    """`rustup target add` is idempotent and quiet when already present."""
    result = subprocess.run(
        ["rustup", "target", "add", WASM_TARGET], cwd=str(PROJECT_ROOT)
    )
    return result.returncode


def _acquire_trunk():
    """Locate or install trunk (cargo-native, no Homebrew).

    Preference order (mirrors task_db.py's diesel acquisition):
    1. project-local install at target/tools/bin/trunk
    2. a `trunk` already on PATH
    3. `cargo install trunk` (pinned) into target/tools

    Returns the binary path, or None on failure.
    """
    local_trunk = TOOLS_ROOT / "bin" / "trunk"
    if local_trunk.exists():
        return str(local_trunk)

    path_trunk = shutil.which("trunk")
    if path_trunk:
        return path_trunk

    print(
        f"No trunk found; installing trunk {TRUNK_VERSION} into {TOOLS_ROOT} "
        f"via cargo (first run only; this takes a few minutes)..."
    )
    result = subprocess.run(
        [
            "cargo", "install", "trunk",
            "--version", TRUNK_VERSION,
            "--root", str(TOOLS_ROOT),
            "--locked",
        ],
        cwd=str(PROJECT_ROOT),
    )
    if result.returncode != 0:
        print("cargo install trunk failed", file=sys.stderr)
        return None
    return str(local_trunk) if local_trunk.exists() else None


@web()
@angreal.command(
    name="build",
    about="build the Leptos CSR bundle (trunk) into crates/kairos-web/dist",
    tool=angreal.ToolDescription(
        """
        Build the kairos-web wasm bundle (KAIROS-T-0039, A-0015: Leptos
        CSR built with trunk). Ensures the wasm32-unknown-unknown target
        (rustup) and trunk itself (PATH, or a pinned cargo install into
        target/tools/ - no Homebrew), then runs `trunk build` in
        crates/kairos-web. Output: crates/kairos-web/dist/.

        Serving the result (docs/gui-conventions.md):
        - dev: run the server with KAIROS_WEB_DIST=crates/kairos-web/dist
        - release: rebuild kairos-server with `--features embed-web`
          after `angreal web build --release` (A-0013 single artifact)

        ## Examples
        ```
        angreal web build            # dev profile (fast, unoptimized)
        angreal web build --release  # wasm-opt'd release bundle
        ```
        """,
        risk_level="safe",
    ),
)
@angreal.argument(
    name="release",
    long="release",
    takes_value=False,
    is_flag=True,
    help="release profile (opt-level z + wasm-opt; what ships)",
)
def build(release=False):
    """Ensure wasm tooling, then `trunk build` in crates/kairos-web."""
    exit_code = _ensure_wasm_target()
    if exit_code != 0:
        print("rustup target add wasm32-unknown-unknown failed", file=sys.stderr)
        return exit_code

    trunk = _acquire_trunk()
    if trunk is None:
        return 1

    cmd = [trunk, "build"]
    if release:
        cmd.append("--release")
    print(f"Building kairos-web ({'release' if release else 'dev'} profile)...")
    result = subprocess.run(cmd, cwd=str(WEB_CRATE))
    if result.returncode != 0:
        print("trunk build failed", file=sys.stderr)
        return result.returncode
    print(f"GUI bundle written to {WEB_DIST}")
    return 0


@web()
@angreal.command(
    name="lint",
    about="conventions grep: no hardcoded colors in kairos-web (tokens only)",
    tool=angreal.ToolDescription(
        """
        The docs/gui-conventions.md token rule, mechanized: scan
        crates/kairos-web (src/**/*.rs, *.css, index.html) for raw color
        literals (#hex, rgb()/rgba(), hsl()/hsla()). Aurora Dark tokens
        must arrive as `token::*` constants or `var(--...)` references -
        never literals. Exit 0 when clean; non-zero listing offending
        lines otherwise. Read-only.
        """,
        risk_level="safe",
    ),
)
def lint():
    """Grep kairos-web sources for raw color literals."""
    offenders = []
    for glob in LINT_GLOBS:
        for path in sorted(WEB_CRATE.glob(glob)):
            for number, line in enumerate(
                path.read_text(encoding="utf-8").splitlines(), start=1
            ):
                if COLOR_LITERAL.search(line):
                    relative = path.relative_to(PROJECT_ROOT)
                    offenders.append(f"{relative}:{number}: {line.strip()}")
    if offenders:
        print(
            "hardcoded colors in kairos-web (use aurora tokens: `token::*` "
            "in Rust, `var(--…)` in CSS — docs/gui-conventions.md):",
            file=sys.stderr,
        )
        for offender in offenders:
            print(f"  {offender}", file=sys.stderr)
        return 1
    print("kairos-web token rule: clean (no raw color literals)")
    return 0
