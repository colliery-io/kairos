"""
Documentation book tasks — KAIROS-T-0167 (KAIROS-I-0016).

`angreal docs build` renders the mdBook at docs/ into docs/book; `angreal
docs serve` runs the live-reloading dev server. mdBook is acquired the same
way task_web.py acquires trunk and task_db.py acquires diesel: a pinned
project-local install under target/tools, an existing binary on PATH, or a
cargo install as a last resort — cargo-native, no Homebrew.

The published site is built by .github/workflows/docs.yml on pushes touching
docs/**, deliberately decoupled from release tags so documentation ships
independently of binaries and images.
"""

import shutil
import subprocess
import sys
from pathlib import Path

import angreal  # type: ignore

from utils import PROJECT_ROOT

docs = angreal.command_group(name="docs", about="documentation book commands")

BOOK_ROOT = PROJECT_ROOT / "docs"
BOOK_OUT = BOOK_ROOT / "book"
TOOLS_ROOT = PROJECT_ROOT / "target" / "tools"
# Pinned to the version .github/workflows/docs.yml installs, so a local
# build and CI render the same book.
MDBOOK_VERSION = "0.5.2"


def _acquire_mdbook():
    """Locate or install mdbook. Returns the binary path, or None."""
    local = TOOLS_ROOT / "bin" / "mdbook"
    if local.exists():
        return str(local)

    on_path = shutil.which("mdbook")
    if on_path:
        return on_path

    print(
        f"No mdbook found; installing mdbook {MDBOOK_VERSION} into "
        f"{TOOLS_ROOT} via cargo (first run only)..."
    )
    result = subprocess.run(
        [
            "cargo", "install", "mdbook",
            "--version", MDBOOK_VERSION,
            "--root", str(TOOLS_ROOT),
            "--locked",
        ],
        cwd=str(PROJECT_ROOT),
    )
    if result.returncode != 0:
        return None
    return str(local) if local.exists() else shutil.which("mdbook")


@docs()
@angreal.command(
    name="build", about="render the documentation book into docs/book"
)
def docs_build():
    mdbook = _acquire_mdbook()
    if not mdbook:
        print("could not acquire mdbook", file=sys.stderr)
        return 1
    result = subprocess.run([mdbook, "build"], cwd=str(BOOK_ROOT))
    if result.returncode != 0:
        return result.returncode
    print(f"book written to {BOOK_OUT}")
    return 0


@docs()
@angreal.command(
    name="serve", about="serve the book locally with live reload"
)
@angreal.argument(
    name="port", long="port", takes_value=True,
    help="port to serve on (default 3000)",
)
def docs_serve(port=None):
    mdbook = _acquire_mdbook()
    if not mdbook:
        print("could not acquire mdbook", file=sys.stderr)
        return 1
    cmd = [mdbook, "serve"]
    if port:
        cmd += ["--port", str(port)]
    return subprocess.run(cmd, cwd=str(BOOK_ROOT)).returncode
