"""
Utility functions for Kairos development.
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import angreal  # type: ignore

PROJECT_ROOT = Path(angreal.get_root()).parent
DOCKER_COMPOSE_FILE = Path(angreal.get_root()) / "docker-compose.yaml"
MIGRATIONS_DIR = PROJECT_ROOT / "crates" / "kairos-db" / "migrations"


def docker_up():
    """Start docker containers for local development."""
    try:
        subprocess.run(
            ["docker", "compose", "-f", str(DOCKER_COMPOSE_FILE), "up", "-d", "--wait"],
            check=True
        )
        print("Docker services started successfully.")
        return 0
    except subprocess.CalledProcessError as e:
        print(f"Error starting Docker services: {e}", file=sys.stderr)
        return 1


def docker_down(remove_volumes=False):
    """Stop docker containers for local development."""
    try:
        cmd = ["docker", "compose", "-f", str(DOCKER_COMPOSE_FILE), "down"]
        if remove_volumes:
            cmd.append("-v")
        subprocess.run(cmd, check=True)
        print("Docker services stopped successfully.")
        return 0
    except subprocess.CalledProcessError as e:
        print(f"Error stopping Docker services: {e}", file=sys.stderr)
        return 1


def docker_clean():
    """Remove docker volumes for clean restart."""
    try:
        subprocess.run(
            ["docker", "compose", "-f", str(DOCKER_COMPOSE_FILE), "down", "-v"],
            check=True
        )
        print("Docker services and volumes cleaned successfully.")
        return 0
    except subprocess.CalledProcessError as e:
        print(f"Error cleaning Docker volumes: {e}", file=sys.stderr)
        return 1


def run_psql(sql: str, database: str = "kairos") -> int:
    """Run SQL against the local PostgreSQL container."""
    try:
        subprocess.run(
            [
                "docker", "exec", "kairos-dev-postgres",
                "psql", "-U", "kairos", "-d", database, "-c", sql
            ],
            check=True
        )
        return 0
    except subprocess.CalledProcessError as e:
        print(f"Error running SQL: {e}", file=sys.stderr)
        return e.returncode


def run_psql_file(sql_file: Path, database: str = "kairos") -> int:
    """Run SQL file against the local PostgreSQL container."""
    try:
        with open(sql_file) as f:
            sql = f.read()
        subprocess.run(
            [
                "docker", "exec", "-i", "kairos-dev-postgres",
                "psql", "-U", "kairos", "-d", database
            ],
            input=sql,
            text=True,
            check=True
        )
        return 0
    except subprocess.CalledProcessError as e:
        print(f"Error running SQL file {sql_file}: {e}", file=sys.stderr)
        return e.returncode


# Build artefacts older than this are removed before the first cargo command of
# a task. Without it target/ grew to 360 GiB in a week of gate runs and filled
# the disk of the host that also runs the live deployment (2026-10-01).
SWEEP_DAYS = float(os.environ.get("KAIROS_SWEEP_DAYS", "3"))

# The directories under a cargo profile that collect one entry per build of a
# crate. Old entries are never read again: cargo rebuilds whatever it needs.
_SWEPT_DIRS = {"deps", "incremental", "build", ".fingerprint"}

_swept = False


def sweep_target(days: float = SWEEP_DAYS) -> tuple:
    """Remove build artefacts in target/ not modified in `days` days.

    The same idea as `cargo sweep --time`, with no tool to install. It walks
    target/<profile>/ and target/<triple>/<profile>/ and removes each entry of
    deps/, incremental/, build/ and .fingerprint/ that is older than the cutoff.
    Returns (entries removed, bytes freed).
    """
    target = PROJECT_ROOT / "target"
    if not target.is_dir():
        return 0, 0
    cutoff = time.time() - days * 86400
    removed, freed = 0, 0
    # target/<profile>/<dir> and target/<triple>/<profile>/<dir>, listed level
    # by level: a glob over target/ would stat every file in it.
    profiles = [p for p in target.iterdir() if p.is_dir()]
    profiles += [p for t in list(profiles) for p in t.iterdir() if p.is_dir()]
    for swept in (p / name for p in profiles for name in _SWEPT_DIRS):
        if not swept.is_dir():
            continue
        for entry in swept.iterdir():
            try:
                if entry.lstat().st_mtime >= cutoff:
                    continue
                if entry.is_dir() and not entry.is_symlink():
                    size = sum(f.lstat().st_size for f in entry.rglob("*") if f.is_file())
                    shutil.rmtree(entry)
                else:
                    size = entry.lstat().st_size
                    entry.unlink()
            except FileNotFoundError:
                continue
            removed += 1
            freed += size
    return removed, freed


def sweep_target_once() -> None:
    """Sweep target/ once per angreal process, before the first cargo command."""
    global _swept
    if _swept or SWEEP_DAYS <= 0:
        return
    _swept = True
    removed, freed = sweep_target()
    if removed:
        print(f"Removed {removed} build artefacts older than {SWEEP_DAYS:g} days "
              f"({freed / 2**30:.1f} GiB).")


def run_cargo_command(command_args: list, cwd: Path = None) -> int:
    """Run a cargo command."""
    sweep_target_once()
    try:
        result = subprocess.run(
            ["cargo"] + command_args,
            cwd=str(cwd) if cwd else str(PROJECT_ROOT),
            check=True
        )
        return result.returncode
    except subprocess.CalledProcessError as e:
        print(f"Cargo command failed: {e}", file=sys.stderr)
        return e.returncode
