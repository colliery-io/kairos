"""
Local development server — KAIROS-T-0174 (KAIROS-I-0016).

`angreal dev serve` boots kairos-server against the compose stack with the
environment that stack actually needs: the Dex issuer on :41558, the `demo`
tenant pinned, and the GUI bundle served from crates/kairos-web/dist. It binds
:41080 because that is the only `redirect_uri` the dev Dex registers for the
`kairos-web` client, so browser login works rather than half-working.

Why this exists: before it, running Kairos locally meant setting six
environment variables by hand, and the `tutorials/run-kairos-locally.md`
lesson could not be written to the Diátaxis tutorial contract because of it —
T2 says a tutorial offers the learner no choices, and T6 says every step must
be repeatable. A six-variable copy-paste is not a choice, but it is a step
that fails opaquely when one variable is wrong, which is the same problem.
Every other workflow in this repo has an angreal entry point; this one was
missing.
"""

import os
import subprocess
import sys
from pathlib import Path

import angreal  # type: ignore

from utils import PROJECT_ROOT, SWEEP_DAYS, sweep_target

dev = angreal.command_group(name="dev", about="local development commands")

WEB_DIST = PROJECT_ROOT / "crates" / "kairos-web" / "dist"

# Matches the ports .angreal/docker-compose.yaml publishes and the
# redirect_uri .angreal/dex/config.yaml registers for kairos-web. Changing
# either of those means changing these.
DATABASE_URL = "postgres://kairos:kairos@localhost:41432/kairos"
ISSUER = "http://localhost:41558/dex"
BIND_ADDR = "127.0.0.1:41080"


@dev()
@angreal.command(
    name="serve",
    about="run kairos-server against the compose stack on :41080",
)
@angreal.argument(
    name="release", long="release", takes_value=False, is_flag=True,
    help="build and run in release mode (slower to build, faster to run)",
)
def dev_serve(release=False):
    """Boot the server. Ctrl-C stops it; the compose stack keeps running."""
    if not WEB_DIST.exists():
        print(
            f"The GUI bundle is missing from {WEB_DIST}.\n"
            "Build it first:  angreal web build",
            file=sys.stderr,
        )
        return 1

    env = os.environ.copy()
    env.update({
        "DATABASE_URL": env.get("DATABASE_URL", DATABASE_URL),
        "OIDC_ISSUER_URL": ISSUER,
        # A comma-separated allow-list (KAIROS-T-0055): the GUI presents a
        # kairos-web token and the CLI a kairos-cli one, so a dev server that
        # names only kairos-web rejects every CLI call with InvalidAudience.
        "OIDC_AUDIENCE": "kairos-web,kairos-cli,kairos-svc",
        # Pinned rather than wildcard-subdomain, so http://localhost:41080
        # resolves to a tenant without any /etc/hosts editing.
        "KAIROS_SINGLE_TENANT": "demo",
        "KAIROS_BASE_DOMAIN": "kairos.test",
        "KAIROS_WEB_DIST": str(WEB_DIST),
        "KAIROS_BIND_ADDR": BIND_ADDR,
        "KAIROS_LOG_LEVEL": env.get("KAIROS_LOG_LEVEL", "info"),
    })

    cmd = ["cargo", "run", "-p", "kairos-server"]
    if release:
        cmd.append("--release")
    cmd += ["--", "serve"]

    print(f"Serving http://localhost:41080 (tenant `demo`, issuer {ISSUER})")
    print("Sign in as alice@kairos.test / alice-password. Ctrl-C to stop.")
    try:
        return subprocess.run(cmd, cwd=str(PROJECT_ROOT), env=env).returncode
    except KeyboardInterrupt:
        return 0


@dev()
@angreal.command(
    name="sweep",
    about="remove build artefacts in target/ older than N days",
    tool=angreal.ToolDescription(
        """
        Remove old build artefacts from target/ (the idea of `cargo sweep
        --time`, with no tool to install).

        Each cargo build of a crate leaves a new entry in deps/, incremental/,
        build/ and .fingerprint/, and cargo never removes the old ones. A week
        of gate runs grew target/ to 360 GiB and filled the disk of the host
        that also runs the live deployment (2026-10-01). Every task that runs
        cargo through `run_cargo_command` already sweeps once at its start, with
        KAIROS_SWEEP_DAYS (default 3; 0 turns it off). This task is the manual
        entry point.

        ## When to use
        - When target/ is large and you want the space back now
        - With --days 0 to remove every artefact (like `cargo clean`, but the
          embedding model cache and the GUI bundle stay)

        ## Output
        The number of entries removed and the space freed. Cargo rebuilds any
        removed artefact that it needs, so the cost is a slower next build.
        """,
        risk_level="safe",
    ),
)
@angreal.argument(
    name="days", long="days", takes_value=True,
    help=f"remove artefacts not modified in this many days (default {SWEEP_DAYS:g})",
)
def dev_sweep(days=None):
    """Remove old build artefacts from target/."""
    days = float(days) if days is not None else SWEEP_DAYS
    removed, freed = sweep_target(days)
    print(f"Removed {removed} build artefacts older than {days:g} days "
          f"({freed / 2**30:.1f} GiB).")
    return 0


# Where `angreal dev` puts the local embedding model. Matches the integration
# test's cache (crates/kairos-embed/tests/local_model.rs) so a developer who has
# run the tests already has it, and sits under target/ so `cargo clean` and CI's
# cache treat it like any other build artefact.
EMBED_CACHE = PROJECT_ROOT / "target" / "embed-cache"


@dev()
@angreal.command(
    name="fetch-model",
    about="download the local embedding model into target/embed-cache",
    tool=angreal.ToolDescription(
        """
        Populate the local embedding model cache (KAIROS-T-0189, A-0021 rule 1).

        The container image bakes the model in at build time, so a deployment
        never downloads it. A developer running `kairos-server` from source has
        no such layer, and the provider refuses to download on its own — an
        operator who deployed an image should get an error naming the directory,
        not a surprise 65 MB fetch. This task is the deliberate fetch.

        ## When to use
        - Before running retrieval locally from a source build
        - After `cargo clean`, which removes target/embed-cache with everything else

        ## Related tasks
        - `dev serve` - run the server; set KAIROS_EMBED_CACHE to this directory
        - `test integration` - the crate's own tests fetch into the same cache

        ## Output
        The model id and dimension on success. Idempotent: an already-populated
        cache just loads and verifies. About 65 MB (bge-small-en-v1.5,
        statically quantized).
        """,
        risk_level="safe",
    ),
)
def dev_fetch_model():
    """Fetch the local embedding model into target/embed-cache."""
    EMBED_CACHE.mkdir(parents=True, exist_ok=True)
    print(f"Fetching the local embedding model into {EMBED_CACHE}...", flush=True)
    return subprocess.run(
        [
            "cargo", "run", "--release",
            "-p", "kairos-embed", "--bin", "fetch-model",
            "--", str(EMBED_CACHE),
        ],
        cwd=str(PROJECT_ROOT),
    ).returncode


@dev()
@angreal.command(
    name="fetch-rust-analyzer",
    about="download the pinned rust-analyzer and std source of the code index into ~/.cache/kairos-index",
    tool=angreal.ToolDescription(
        """
        Populate the rust-analyzer cache of the code index (COLLIERY-T-1858,
        COLLIERY-T-1860).

        kairos-index runs `rust-analyzer scip` with one pinned standalone
        release, not the rust-analyzer of the toolchain, and with one pinned
        std source (the rust-src archive of a stable Rust release, 1.94 or
        later), not the rust-src of the toolchain. The index downloads both
        on first need, but the kairos-index scenarios never download: they
        fail and name this task if one is not there. This task is the
        deliberate fetch, for a test machine and for CI.

        ## When to use
        - Before `test integration` on a new machine or in CI
        - After the pinned release or std source changes
          (crates/kairos-index/src/rust_analyzer.rs)

        ## Related tasks
        - `test integration` - runs the kairos-index scenarios with the binary

        ## Output
        The version and the path of the binary and of the std source.
        Idempotent: if both are there, only their sha256 values are checked.
        The binary: about 16 MB to download, 40 MB on disk, in
        ~/.cache/kairos-index/bin (or KAIROS_INDEX_RUST_ANALYZER). The std
        source: 9.4 MB to download, unpacked once next to the archive in
        ~/.cache/kairos-index/rust-src (or KAIROS_INDEX_RUST_SRC).
        """,
        risk_level="safe",
    ),
)
def dev_fetch_rust_analyzer():
    """Fetch the pinned rust-analyzer and std source into ~/.cache/kairos-index."""
    return subprocess.run(
        [
            "cargo", "run", "-p", "kairos-index",
            "--example", "fetch_rust_analyzer",
        ],
        cwd=str(PROJECT_ROOT),
    ).returncode
