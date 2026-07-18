"""
Utility functions for Kairos development.
"""

import subprocess
import sys
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
                "docker", "exec", "kairos-postgres",
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
                "docker", "exec", "-i", "kairos-postgres",
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


def run_cargo_command(command_args: list, cwd: Path = None) -> int:
    """Run a cargo command."""
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
