"""
Test management tasks for Kairos.

Implements the KAIROS-A-0012 test tiers as angreal tasks so agents and CI
share a single entry point:

  tier 1  unit         -> `angreal test unit`
  tier 2+3 integration -> `angreal test integration`
  tier 4  e2e smoke    -> `angreal test e2e`   (golden path + MCP + GUI Playwright)
  tier 5  soak         -> `angreal test soak`  (workforce driver, KAIROS-T-0046)
  tier 6  uat          -> `angreal test uat`   (persona journeys, KAIROS-I-0011)

plus the static gate that precedes them (CI gates 1+2, KAIROS-T-0114):

  fmt + clippy         -> `angreal test lint`  (whole workspace, -D warnings)
"""

import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import angreal  # type: ignore

# The KAIROS-T-0045 GUI leg calls `angreal web build` in-process. Both task
# files are already loaded by angreal, so this is the cached module (no
# double registration) — the same direct-call pattern `test all` uses for
# `unit()`/`integration()`.
from task_web import build as _web_build

from utils import docker_up, docker_down, run_cargo_command, PROJECT_ROOT

test = angreal.command_group(name="test", about="commands for running tests")

# Soak stack wiring (KAIROS-T-0046): a dedicated port so neither a dev
# server (41080) nor the e2e stack (41188) collides. The soak DRIVER
# (crates/kairos-soak) always targets an already-running deployment via
# --url; this task is the boot-and-seed choreography around it, mirroring
# the e2e pattern: compose up -> build -> seed-demo --force -> serve ->
# drive -> stop the server. Unlike e2e it does NOT tear down the compose
# services afterwards (soak is nightly-scale; the dev stack stays up).
SOAK_PORT = int(os.environ.get("KAIROS_SOAK_PORT", "41189"))
SOAK_BASE_URL = f"http://127.0.0.1:{SOAK_PORT}"
SOAK_BIN = PROJECT_ROOT / "target" / "debug" / "kairos-soak"
SOAK_REPORT = os.environ.get(
    "KAIROS_SOAK_REPORT", str(PROJECT_ROOT / "target" / "soak-report.json")
)
# alice's OIDC sub (kairos-db/src/seed.rs DEMO_USERS): made a deployment
# admin so the driver can provision the bystander tenant used by the
# tenant-isolation invariant.
SOAK_DEPLOYMENT_ADMIN = "CiQwOGE4Njg0Yi1kYjg4LTRiNzMtOTBhOS0zY2QxNjYxZjU0NjYSBWxvY2Fs"

# E2E stack wiring (KAIROS-T-0035): dedicated port so a dev server on the
# default 8080 never collides; everything else matches the compose stack
# defaults (.angreal/task_db.py, .angreal/dex/config.yaml).
E2E_PORT = int(os.environ.get("KAIROS_E2E_PORT", "41188"))
E2E_BASE_URL = f"http://127.0.0.1:{E2E_PORT}"
E2E_DATABASE_URL = os.environ.get(
    "DATABASE_URL", "postgres://kairos:kairos@localhost:41432/kairos"
)
E2E_ISSUER = "http://localhost:41558/dex"
SERVER_BIN = PROJECT_ROOT / "target" / "debug" / "kairos-server"
E2E_RUNNER_BIN = PROJECT_ROOT / "target" / "debug" / "examples" / "e2e_golden_path"

# GUI smoke leg (KAIROS-T-0045): the Leptos SPA is served on :41080 because
# that is the ONLY redirect_uri Dex registers for the `kairos-web` public
# client (.angreal/dex/config.yaml), so the Playwright suite does REAL
# in-browser PKCE with no interception. Distinct from the golden-path
# server's :8188 — the two run side by side against the same compose stack.
E2E_GUI_PORT = int(os.environ.get("KAIROS_E2E_GUI_PORT", "41080"))
E2E_GUI_BASE_URL = f"http://localhost:{E2E_GUI_PORT}"
E2E_DIR = PROJECT_ROOT / "e2e"
WEB_DIST = PROJECT_ROOT / "crates" / "kairos-web" / "dist"

# UAT tier (KAIROS-I-0011, A-0012 tier 6): persona journeys in uat/ against
# the same :41080 GUI server the e2e leg boots (real in-browser PKCE), or
# against any deployment via --server. alice is made a deployment admin so
# the onboarding journey can prove tenant provisioning (compose only).
UAT_DIR = PROJECT_ROOT / "uat"
CLI_BIN = PROJECT_ROOT / "target" / "debug" / "kairos"


def _integration_test_targets():
    """Return the workspace's integration test targets (tests/ directories).

    Returns a list of "package::target" strings, or None if cargo metadata
    itself failed.
    """
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=str(PROJECT_ROOT),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr)
        return None

    metadata = json.loads(result.stdout)
    targets = []
    for package in metadata.get("packages", []):
        for target in package.get("targets", []):
            if "test" in target.get("kind", []):
                targets.append(f"{package['name']}::{target['name']}")
    return targets


# The clippy gate covers the WHOLE workspace — every crate, every target,
# warnings denied. kairos-web is a plain workspace member (its lib target
# compiles natively, docs/gui-conventions.md §1), so it is in scope here
# exactly like the server: no per-crate carve-outs (KAIROS-T-0114 retired
# the last pre-existing kairos-web backlog).
CLIPPY_ARGS = ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]


@test()
@angreal.command(
    name="lint",
    about="static gate: cargo fmt --check + cargo clippy --workspace -D warnings",
    tool=angreal.ToolDescription(
        """
        The static gate that precedes the KAIROS-A-0012 tiers (CI gates 1
        and 2 in .github/workflows/ci.yml, run locally as one task):

        1. `cargo fmt --all --check`
        2. `cargo clippy --workspace --all-targets -- -D warnings`

        The clippy run covers every workspace crate including kairos-web
        (its lib target compiles natively). No services needed.

        ## When to use
        - Before committing (part of the mechanical completion gate)
        - After touching any crate, to catch warnings CI would reject

        ## Related tasks
        - `web lint` - the kairos-web token-rule grep (separate, also a gate)
        - `test unit` - tier 1 unit tests

        ## Output
        Standard cargo output. Exit code 0 iff both steps pass; the first
        failing step's exit code is propagated otherwise.
        """,
        risk_level="safe",
    ),
)
def lint():
    """Run rustfmt in check mode, then clippy across the whole workspace."""
    code = run_cargo_command(["fmt", "--all", "--check"])
    if code:
        return code
    return run_cargo_command(CLIPPY_ARGS)


@test()
@angreal.command(
    name="unit",
    about="run unit tests (workspace lib/bin targets, no services needed)",
    tool=angreal.ToolDescription(
        """
        Run KAIROS-A-0012 tier 1: `cargo test --workspace --lib --bins`.
        Pure-logic unit and smoke tests only; requires no Docker services
        and no database.

        ## When to use
        - On every agent loop iteration for fast feedback
        - Before committing changes (part of the mechanical completion gate)

        ## Related tasks
        - `test integration` - tiers 2+3 against real Postgres/Dex (slower)
        - `test all` - unit followed by integration

        ## Output
        Standard cargo test output. Exit code 0 iff every test passes;
        cargo's exit code is propagated otherwise.
        """,
        risk_level="safe",
    ),
)
def unit():
    """Run unit tests for all workspace crates, then the plugin hook's
    stdlib unit tests (KAIROS-T-0108)."""
    code = run_cargo_command(["test", "--workspace", "--lib", "--bins"])
    if code:
        return code
    hook_tests = os.path.join(PROJECT_ROOT, "plugin", "hooks", "test_session_start.py")
    return subprocess.run(
        [sys.executable, "-m", "unittest", hook_tests], cwd=PROJECT_ROOT, check=False
    ).returncode


@test()
@angreal.command(
    name="integration",
    about="run integration tests (spins up compose stack, runs tests/ targets)",
    tool=angreal.ToolDescription(
        """
        Run KAIROS-A-0012 tiers 2+3: workspace integration test targets
        (each crate's tests/ directory) against the real compose stack
        (Postgres 16 + Dex). The database is never mocked.

        Lifecycle: compose up --wait, `cargo test --workspace --test '*'`,
        compose down -v (unless --keep-running).

        If the workspace has no integration test targets yet, the task
        prints an explicit "no integration test targets found" notice and
        exits 0 - that is a pass, not a silent no-op.

        ## When to use
        - Before completing any Metis task (mechanical completion gate)
        - After changing SQL, tenant isolation, auth, or search code

        ## Preconditions
        - Docker available and able to pull pgvector/pgvector:pg16 / dexidp/dex

        ## Examples
        ```
        angreal test integration
        angreal test integration --keep-running   # leave services up for debugging
        ```

        ## Output
        Cargo test output; exit code propagated. With --keep-running the
        compose stack stays up (`angreal services down -v` to stop).
        """,
        risk_level="safe",
    ),
)
@angreal.argument(
    name="keep_running",
    long="keep-running",
    short="k",
    help="keep services running after tests complete",
    takes_value=False,
    is_flag=True,
)
def integration(keep_running=False):
    """Run integration tests against the compose stack.

    1. Starts docker services (postgres, dex)
    2. Runs every workspace integration test target (tests/ directories)
    3. Tears down services (unless --keep-running)
    """
    print("Starting docker services...")
    exit_code = docker_up()
    if exit_code != 0:
        return exit_code

    targets = _integration_test_targets()
    if targets is None:
        print("Failed to enumerate workspace test targets.", file=sys.stderr)
        test_exit_code = 1
    elif not targets:
        print(
            "No integration test targets found in the workspace "
            "(no crate has a tests/ directory yet) - nothing to run. PASS."
        )
        test_exit_code = 0
    else:
        print(f"Running {len(targets)} integration test target(s): {', '.join(targets)}")
        result = subprocess.run(
            ["cargo", "test", "--workspace", "--test", "*"],
            cwd=str(PROJECT_ROOT),
        )
        test_exit_code = result.returncode

    if not keep_running:
        print("Tearing down services...", flush=True)
        docker_down(remove_volumes=True)
    else:
        print("Services kept running. Use 'angreal services down -v' to stop them.")

    return test_exit_code


def _e2e_phase(name, exit_code):
    """Attribute a failed phase loudly; return the exit code unchanged. A
    phase named `UAT: …` is attributed to the UAT tier."""
    if exit_code != 0:
        tier = "UAT" if name.startswith("UAT") else "E2E"
        print(f"{tier} FAILED at phase: {name} (exit {exit_code})", file=sys.stderr)
    return exit_code


def _wait_for_url(url, timeout_seconds=30):
    """Poll `url` until it answers 200 or the timeout passes."""
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2) as resp:
                if resp.status == 200:
                    return True
        except OSError:
            pass
        time.sleep(0.5)
    return False


def _wait_for_healthz(timeout_seconds=30):
    """Poll {E2E_BASE_URL}/healthz until it answers or the timeout passes."""
    return _wait_for_url(f"{E2E_BASE_URL}/healthz", timeout_seconds)


def _ensure_playwright(package_dir=E2E_DIR):
    """Make a Playwright package (e2e/ or uat/) runnable from a clean
    checkout: install the npm deps if node_modules is absent, then ensure
    the chromium browser (npx playwright install is idempotent). Returns an
    exit code."""
    if shutil.which("npm") is None or shutil.which("npx") is None:
        print(
            "node/npm not found — the Playwright tiers need Node.js (npm + npx) "
            "on PATH. Install Node 18+ and re-run.",
            file=sys.stderr,
        )
        return 1
    if not (package_dir / "node_modules").exists():
        print(f"Installing {package_dir.name} npm dependencies (first run)...", flush=True)
        code = subprocess.run(["npm", "install"], cwd=str(package_dir)).returncode
        if code != 0:
            return code
    print("Ensuring the Playwright chromium browser...", flush=True)
    return subprocess.run(
        ["npx", "playwright", "install", "chromium"], cwd=str(package_dir)
    ).returncode


def _prepare_gui_stack(env, phase):
    """Shared by the e2e GUI leg and the UAT tier: build the SPA and reseed
    a clean demo fixture. Returns an exit code (0 on success)."""
    print("Building the kairos-web bundle (angreal web build)...", flush=True)
    code = _web_build()
    if code != 0:
        return _e2e_phase(f"{phase}: web build", code)

    print(f"Reseeding the demo tenant for {phase}...", flush=True)
    code = subprocess.run(
        [str(SERVER_BIN), "seed-demo", "--force"],
        cwd=str(PROJECT_ROOT),
        env=env,
    ).returncode
    if code != 0:
        return _e2e_phase(f"{phase}: seed-demo", code)
    return 0


def _gui_server_env(env, extra=None):
    """Environment for the :41080 GUI server (the only redirect_uri Dex
    registers for kairos-web, so in-browser PKCE is real)."""
    gui_env = env.copy()
    gui_env.update({
        "KAIROS_BIND_ADDR": f"127.0.0.1:{E2E_GUI_PORT}",
        "OIDC_ISSUER_URL": E2E_ISSUER,
        "OIDC_AUDIENCE": "kairos-web",
        "KAIROS_SINGLE_TENANT": "demo",
        "KAIROS_BASE_DOMAIN": "kairos.test",
        "KAIROS_WEB_DIST": str(WEB_DIST),
        # KAIROS-T-0102: the forge spec registers a repo and delivers
        # signed webhooks, so the GUI leg needs the integration configured
        # (without these the connection endpoints answer 501).
        "KAIROS_PUBLIC_URL": E2E_GUI_BASE_URL,
        "KAIROS_WEBHOOK_SIGNING_KEY": "e2e-webhook-signing-key",
        # KAIROS-T-0193: retrieval is a tested surface, so the test server runs
        # the real local model rather than degrading to lexical. Without the
        # cache the provider declines to start (by design — an image is built
        # with it populated), and `related_work` would answer "not enabled on
        # this deployment" while every assertion about proposals quietly passed
        # for the wrong reason. Downloading is allowed HERE and nowhere else:
        # this is a test harness on a developer machine or a CI runner, not a
        # deployment.
        "KAIROS_EMBED_CACHE": str(PROJECT_ROOT / "target" / "embed-cache"),
        "KAIROS_EMBED_ALLOW_DOWNLOAD": "1",
        # Two seconds rather than the default ten, and deliberately NOT zero:
        # embedding happens off the write path by design, so a journey that
        # created an item and immediately expected a vector would be testing a
        # mechanism the product does not have. The journey waits, as a real
        # caller would.
        "KAIROS_EMBED_REFRESH_SECS": "2",
    })
    if extra:
        gui_env.update(extra)
    return gui_env


def _stop_process(proc, label):
    print(f"Stopping {label}...", flush=True)
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _boot_gui_server(gui_env, log_path=None):
    """Boot kairos-server on :41080 and wait for healthz. Returns
    `(process, exit_code)`; a non-zero code means the boot failed (and the
    process, if it started, has been stopped). With `log_path` the server
    writes there instead of inheriting this shell's stdout — required when
    it is meant to outlive the task (--keep-running), or the caller's
    pipe never closes."""
    print(f"Booting the GUI server on {E2E_GUI_BASE_URL}...", flush=True)
    output = open(log_path, "ab") if log_path else None
    if output:
        print(f"(server output -> {log_path})", flush=True)
    gui_server = subprocess.Popen(
        [str(SERVER_BIN), "serve"],
        cwd=str(PROJECT_ROOT),
        env=gui_env,
        stdout=output,
        stderr=subprocess.STDOUT if output else None,
    )
    if not _wait_for_url(f"{E2E_GUI_BASE_URL}/healthz"):
        state = gui_server.poll()
        _stop_process(gui_server, "the GUI server")
        return None, _e2e_phase(
            f"GUI server boot (healthz never answered; process state: {state})",
            1,
        )
    # `healthz` answering is NOT proof that OUR server answered it: if
    # the port was already taken (a stray dev server), our process
    # exits with "Address already in use" and the suite silently runs
    # against whatever else is listening — usually a stale binary,
    # producing a pile of baffling failures. Fail loudly instead.
    if gui_server.poll() is not None:
        return None, _e2e_phase(
            f"GUI server exited immediately (code {gui_server.returncode}) — "
            f"something else is already listening on {E2E_GUI_BASE_URL}; "
            "stop it and re-run",
            1,
        )
    return gui_server, 0


def _run_gui_smoke(env):
    """The KAIROS-T-0045 GUI leg: build the SPA, reseed a clean demo
    fixture, serve it on :41080 (the only redirect_uri Dex registers for
    kairos-web → real in-browser PKCE), and run the Playwright smoke suite
    headless. Returns an exit code; the caller attributes the phase."""
    code = _prepare_gui_stack(env, "GUI")
    if code != 0:
        return code
    code = _ensure_playwright(E2E_DIR)
    if code != 0:
        return _e2e_phase("GUI: playwright install", code)
    gui_server, code = _boot_gui_server(_gui_server_env(env))
    if code != 0:
        return code
    try:
        print("Running the Playwright GUI smoke suite...", flush=True)
        pw_env = os.environ.copy()
        pw_env.update({
            "E2E_GUI_BASE_URL": E2E_GUI_BASE_URL,
            "E2E_ISSUER": E2E_ISSUER,
        })
        return _e2e_phase(
            "GUI: playwright test",
            subprocess.run(
                # `@docs` is the documentation-screenshot capture script
                # (KAIROS-T-0185). It is not a test: it needs state a caller
                # sets up around it (an archived item matching its query) and
                # it writes PNGs into the book. Its own header claimed it was
                # excluded here and nothing did the excluding, so `angreal test
                # e2e` failed on it from the day it landed. Run it deliberately:
                #   cd e2e && npx playwright test capture-docs-images --grep @docs
                # which still works, because this exclusion lives here and not
                # in the config where it would fight that `--grep`.
                ["npx", "playwright", "test", "--grep-invert", "@docs"],
                cwd=str(E2E_DIR),
                env=pw_env,
            ).returncode,
        )
    finally:
        _stop_process(gui_server, "the GUI server")


@test()
@angreal.command(
    name="e2e",
    about="run the E2E smoke (compose up -> seed -> API + MCP + GUI Playwright)",
    tool=angreal.ToolDescription(
        """
        KAIROS-A-0012 tier 4 E2E smoke (implemented in KAIROS-T-0035):

        1. compose up (Postgres + Dex, --wait)
        2. build kairos-server + the golden-path runner
        3. `kairos-server seed-demo --force` (applies migrations, then
           drops + reseeds the `demo` tenant on the dev database)
        4. boot `kairos-server serve` on 127.0.0.1:8188 (override with
           KAIROS_E2E_PORT) against the compose stack
        5. run the golden-path runner (kairos-server example
           `e2e_golden_path`): healthz -> real Dex token -> REST whoami ->
           create strategy -> initiative -> decompose 2 tasks with a
           blocks edge -> transition -> search finds them -> MCP session
           smoke (initialize/whoami/board_items over streamable HTTP)
        6. stop the server, compose down -v

        7. GUI smoke leg (KAIROS-T-0045): `angreal web build` (the CSR
           bundle), reseed the demo tenant, boot a SECOND kairos-server on
           :41080 (OIDC_AUDIENCE=kairos-web, KAIROS_SINGLE_TENANT=demo,
           KAIROS_WEB_DIST=dist — :41080 is the only redirect_uri Dex
           registers for the kairos-web client, so PKCE is real), then run
           the Playwright suite in e2e/ headless (`npx playwright test`):
           real Dex login -> board list -> platform-delivery items in
           columns -> create item -> transition via the move menu -> a live
           /ws/events update from a second (API) writer reflected without a
           reload -> item edit + save + a forced 409 merge path -> logout.
           node_modules/chromium install is guarded (installed on a clean
           checkout). Exit code propagates.

        Exits 0 ONLY when every step passes; any failure names its phase
        (and, inside the runner, its step) on stderr.

        DESTRUCTIVE for the dev database's `demo` tenant (seed --force,
        run twice — once for the API path, once for the GUI leg) and tears
        down compose volumes afterwards.

        ## When to use
        - Gating releases and milestone acceptance, not per-task loops
        """,
        risk_level="destructive",
    ),
)
def e2e():
    """Compose up -> seed-demo --force -> serve -> golden-path runner -> down."""
    print("Starting docker services for E2E smoke...", flush=True)
    exit_code = _e2e_phase("compose up", docker_up())
    if exit_code != 0:
        return exit_code

    server = None
    try:
        print("Building kairos-server and the golden-path runner...", flush=True)
        exit_code = _e2e_phase(
            "cargo build",
            subprocess.run(
                [
                    "cargo", "build", "--quiet", "-p", "kairos-server",
                    "--bin", "kairos-server", "--example", "e2e_golden_path",
                ],
                cwd=str(PROJECT_ROOT),
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        env = os.environ.copy()
        env.setdefault("DATABASE_URL", E2E_DATABASE_URL)

        print("Seeding the demo tenant (kairos-server seed-demo --force)...", flush=True)
        exit_code = _e2e_phase(
            "seed-demo",
            subprocess.run(
                [str(SERVER_BIN), "seed-demo", "--force"],
                cwd=str(PROJECT_ROOT),
                env=env,
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        print(f"Booting kairos-server serve on {E2E_BASE_URL}...", flush=True)
        server_env = env.copy()
        server_env.update({
            "KAIROS_BIND_ADDR": f"127.0.0.1:{E2E_PORT}",
            "OIDC_ISSUER_URL": E2E_ISSUER,
            "OIDC_AUDIENCE": "kairos-cli",
            "KAIROS_BASE_DOMAIN": "kairos.test",
        })
        server = subprocess.Popen(
            [str(SERVER_BIN), "serve"],
            cwd=str(PROJECT_ROOT),
            env=server_env,
        )
        if not _wait_for_healthz():
            server_state = server.poll()
            return _e2e_phase(
                "server boot (healthz never answered; "
                f"server process state: {server_state})",
                1,
            )

        print("Running the golden-path runner...", flush=True)
        runner_env = env.copy()
        runner_env.update({
            "KAIROS_E2E_BASE_URL": E2E_BASE_URL,
            "KAIROS_E2E_ISSUER": E2E_ISSUER,
        })
        exit_code = _e2e_phase(
            "golden-path runner",
            subprocess.run(
                [str(E2E_RUNNER_BIN)],
                cwd=str(PROJECT_ROOT),
                env=runner_env,
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        print(
            "API golden path + MCP smoke PASSED; running the GUI smoke leg "
            "(KAIROS-T-0045)...",
            flush=True,
        )
        gui_exit = _run_gui_smoke(env)
        if gui_exit != 0:
            return gui_exit

        print("E2E PASSED (API golden path + MCP + GUI smoke).", flush=True)
        return 0
    finally:
        if server is not None:
            print("Stopping kairos-server...", flush=True)
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
        print("Tearing down services...", flush=True)
        docker_down(remove_volumes=True)


@test()
@angreal.command(
    name="soak",
    about="run the KAIROS-A-0012 tier-5 workforce soak (boots server, drives kairos-soak)",
    tool=angreal.ToolDescription(
        """
        KAIROS-A-0012 tier 5 workforce soak (implemented in KAIROS-T-0046):

        1. compose up (Postgres + Dex, --wait; idempotent if already up)
        2. build kairos-server + the kairos-soak driver
        3. `kairos-server seed-demo --force` on DATABASE_URL
        4. boot `kairos-server serve` on 127.0.0.1:8189 (override with
           KAIROS_SOAK_PORT) with KAIROS_DEPLOYMENT_ADMINS set to alice's
           sub, so the driver can provision the bystander tenant
        5. run `kairos-soak --url ... --duration ... [--config ...]`:
           alice/bob/carol + svc agent workers execute the tier-5 mix
           (creates, edits with deliberate 409 collisions, transitions,
           searches/traversals, MCP sessions, WS subscribers) at a
           sustained rate, with continuous assertions (error rate, p95
           vs the 50ms budget, /metrics pool stability when the endpoint
           exists, item_history boundedness, bystander-tenant isolation)
        6. stop the server; the compose services are LEFT RUNNING
           (soak is nightly-scale; `angreal services down -v` to stop)

        Exit codes: 0 = all assertions held; 1 = breaches (attributed in
        the report); 2 = fatal tenant-isolation breach (run stopped
        early); anything else = a boot/setup phase failed. The JSON run
        report lands at target/soak-report.json (KAIROS_SOAK_REPORT to
        override); a summary is printed on stdout.

        Profiles (KAIROS-T-0046): pre-release smoke = `--duration 10m`;
        nightly = the 4h default (hours-scale). KAIROS_SOAK_RATE tunes
        the sustained ops/second (default 8).

        DESTRUCTIVE for the dev database's `demo` tenant (seed --force);
        also creates/reuses a `soak-bystander` tenant.

        ## When to use
        - Nightly and pre-release only - NEVER part of the per-task
          completion gate (KAIROS-A-0012)

        ## Examples
        ```
        angreal test soak --duration 10m                 # smoke profile
        angreal test soak                                # default 4h nightly
        angreal test soak --duration 8h --config soak.toml
        ```
        """,
        risk_level="destructive",
    ),
)
@angreal.argument(
    name="duration",
    long="duration",
    help="how long to run the soak (e.g. 30m, 4h); default 4h",
    takes_value=True,
    required=False,
)
@angreal.argument(
    name="config",
    long="config",
    help="path to a kairos-soak TOML profile (optional)",
    takes_value=True,
    required=False,
)
def soak(duration=None, config=None):
    """Boot compose + server + seed, then drive the kairos-soak workforce."""
    duration = duration or "4h"

    def phase(name, exit_code):
        if exit_code != 0:
            print(f"SOAK FAILED at phase: {name} (exit {exit_code})", file=sys.stderr)
        return exit_code

    print(f"Soak run: duration={duration}, config={config or '<defaults>'}", flush=True)
    print("Starting docker services for the soak stack...", flush=True)
    exit_code = phase("compose up", docker_up())
    if exit_code != 0:
        return exit_code

    server = None
    try:
        print("Building kairos-server and the kairos-soak driver...", flush=True)
        exit_code = phase(
            "cargo build",
            subprocess.run(
                [
                    "cargo", "build", "--quiet",
                    "-p", "kairos-server", "--bin", "kairos-server",
                    "-p", "kairos-soak", "--bin", "kairos-soak",
                ],
                cwd=str(PROJECT_ROOT),
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        env = os.environ.copy()
        env.setdefault("DATABASE_URL", E2E_DATABASE_URL)

        print("Seeding the demo tenant (kairos-server seed-demo --force)...", flush=True)
        exit_code = phase(
            "seed-demo",
            subprocess.run(
                [str(SERVER_BIN), "seed-demo", "--force"],
                cwd=str(PROJECT_ROOT),
                env=env,
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        print(f"Booting kairos-server serve on {SOAK_BASE_URL}...", flush=True)
        server_env = env.copy()
        server_env.update({
            "KAIROS_BIND_ADDR": f"127.0.0.1:{SOAK_PORT}",
            "OIDC_ISSUER_URL": E2E_ISSUER,
            "OIDC_AUDIENCE": "kairos-cli",
            "KAIROS_BASE_DOMAIN": "kairos.test",
            "KAIROS_DEPLOYMENT_ADMINS": SOAK_DEPLOYMENT_ADMIN,
        })
        # Hours-scale runs at INFO would stream one JSON span per request;
        # keep the soak server quiet unless the caller says otherwise.
        server_env.setdefault("KAIROS_LOG_LEVEL", "warn")
        server = subprocess.Popen(
            [str(SERVER_BIN), "serve"],
            cwd=str(PROJECT_ROOT),
            env=server_env,
        )
        deadline = time.monotonic() + 30
        healthy = False
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(f"{SOAK_BASE_URL}/healthz", timeout=2) as resp:
                    if resp.status == 200:
                        healthy = True
                        break
            except OSError:
                pass
            time.sleep(0.5)
        if not healthy:
            return phase(
                f"server boot (healthz never answered; process state: {server.poll()})",
                1,
            )

        print(f"Running the workforce driver for {duration}...", flush=True)
        driver_cmd = [
            str(SOAK_BIN),
            "--url", SOAK_BASE_URL,
            "--issuer", E2E_ISSUER,
            "--duration", duration,
            "--report", SOAK_REPORT,
        ]
        if os.environ.get("KAIROS_SOAK_RATE"):
            driver_cmd += ["--rate", os.environ["KAIROS_SOAK_RATE"]]
        if config:
            driver_cmd += ["--config", config]
        exit_code = phase(
            "kairos-soak driver",
            subprocess.run(driver_cmd, cwd=str(PROJECT_ROOT), env=env).returncode,
        )
        if exit_code == 0:
            print("SOAK PASSED (all continuous assertions held).", flush=True)
        return exit_code
    finally:
        if server is not None:
            print("Stopping kairos-server...", flush=True)
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
        print(
            "Compose services left running (soak is nightly-scale; "
            "use 'angreal services down -v' to stop them).",
            flush=True,
        )


def _run_uat_suite(server_url, mode, journeys, headed, report_dir):
    """Run the uat/ Playwright journeys against `server_url`. Returns the
    Playwright exit code; the report path is printed by the reporter."""
    code = _ensure_playwright(UAT_DIR)
    if code != 0:
        return _e2e_phase("UAT: playwright install", code)
    pw_env = os.environ.copy()
    pw_env.update({
        "UAT_SERVER": server_url,
        "UAT_ISSUER": os.environ.get("UAT_ISSUER", E2E_ISSUER),
        "UAT_MODE": mode,
        "UAT_KAIROS_BIN": str(CLI_BIN),
    })
    if headed:
        pw_env["UAT_HEADED"] = "1"
    if report_dir:
        pw_env["UAT_REPORT_DIR"] = str(Path(report_dir).resolve())
    args = ["npx", "playwright", "test"]
    if journeys:
        tags = "|".join(f"@{j.strip()}$" for j in journeys.split(",") if j.strip())
        args += ["--grep", tags]
    print(f"Running the UAT journeys against {server_url} ({mode} mode)...", flush=True)
    return _e2e_phase(
        "UAT: playwright test",
        subprocess.run(args, cwd=str(UAT_DIR), env=pw_env).returncode,
    )


@test()
@angreal.command(
    name="uat",
    about="run the user-acceptance journeys (compose stack by default, or --server URL)",
    tool=angreal.ToolDescription(
        """
        KAIROS-A-0012 tier 6 — user-acceptance journeys (KAIROS-I-0011):
        persona-driven stories in uat/ (an agent over MCP + the real
        `kairos` CLI, engineers in the browser, admins over the CLI/API)
        that end in a readable report: uat/reports/<run>/report.md +
        report.json (override with --report-dir).

        Default (compose mode — DESTRUCTIVE to the dev database's `demo`
        tenant, like `test e2e`):
          1. compose up (Postgres + Dex, --wait)
          2. cargo build kairos-server + the `kairos` CLI; angreal web build
          3. `kairos-server seed-demo --force`
          4. boot kairos-server on :41080 (the only redirect_uri Dex
             registers for kairos-web → real in-browser PKCE) with
             KAIROS_SINGLE_TENANT=demo and alice as a deployment admin
          5. `npx playwright test` in uat/
          6. stop the server, compose down -v (unless --keep-running, which
             leaves the stack + server up for a hand-run or a --server run)

        --server URL: NON-destructive. No compose, no seed, no server boot;
        the journeys run against URL with UAT_PERSONA_<NAME>_EMAIL/_PASSWORD
        (defaults: the seed users), UAT_TENANT (demo) and UAT_ISSUER. Every
        object a journey creates is named `uat-<run>-…` and deleted in
        teardown; steps that need a fresh tenant / a deployment-admin token
        are skipped and the report says so. The deployment must register
        `<URL>/callback` as a redirect_uri for the `kairos-web` client.

        --journey a,b   run only these journey ids (smoke, onboarding,
                        planning, agent-loop, cross-team)
        --headed        show the browser
        --report-dir    where to write the report (default uat/reports/<run>)

        Exit code: 0 only when every selected journey passes. Not part of
        `test all` — this is a release/milestone gate, not a per-task one.

        ## When to use
        - Accepting a release or a milestone from the user's point of view
        - Checking a deployment after an upgrade (--server)
        """,
        risk_level="destructive",
    ),
)
@angreal.argument(
    name="server",
    long="server",
    help="run against this deployment URL instead of booting the compose stack",
    takes_value=True,
    required=False,
)
@angreal.argument(
    name="journey",
    long="journey",
    help="comma-separated journey ids to run (default: all)",
    takes_value=True,
    required=False,
)
@angreal.argument(
    name="keep_running",
    long="keep-running",
    short="k",
    help="compose mode: leave the stack and the :41080 server running afterwards",
    takes_value=False,
    is_flag=True,
)
@angreal.argument(
    name="headed",
    long="headed",
    help="show the browser",
    takes_value=False,
    is_flag=True,
)
@angreal.argument(
    name="report_dir",
    long="report-dir",
    help="directory for report.md/report.json (default uat/reports/<run>)",
    takes_value=True,
    required=False,
)
def uat(server=None, journey=None, keep_running=False, headed=False, report_dir=None):
    """Persona journeys: compose up -> seed -> serve -> uat/ -> report."""
    if server:
        return _run_uat_suite(server.rstrip("/"), "server", journey, headed, report_dir)

    print("Starting docker services for the UAT run...", flush=True)
    exit_code = _e2e_phase("compose up", docker_up())
    if exit_code != 0:
        return exit_code

    gui_server = None
    try:
        print("Building kairos-server and the kairos CLI...", flush=True)
        exit_code = _e2e_phase(
            "cargo build",
            subprocess.run(
                [
                    "cargo", "build", "--quiet",
                    "-p", "kairos-server", "--bin", "kairos-server",
                    "-p", "kairos-cli", "--bin", "kairos",
                ],
                cwd=str(PROJECT_ROOT),
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        env = os.environ.copy()
        env.setdefault("DATABASE_URL", E2E_DATABASE_URL)
        exit_code = _prepare_gui_stack(env, "UAT")
        if exit_code != 0:
            return exit_code

        gui_server, exit_code = _boot_gui_server(
            _gui_server_env(env, {"KAIROS_DEPLOYMENT_ADMINS": SOAK_DEPLOYMENT_ADMIN}),
            log_path=(PROJECT_ROOT / "target" / "uat-server.log") if keep_running else None,
        )
        if exit_code != 0:
            return exit_code

        exit_code = _run_uat_suite(E2E_GUI_BASE_URL, "compose", journey, headed, report_dir)
        if exit_code == 0:
            print("UAT PASSED.", flush=True)
        return exit_code
    finally:
        if keep_running:
            print(
                f"--keep-running: the compose stack and the server on {E2E_GUI_BASE_URL} "
                "are still up (stop with `angreal services down`; the server logs to "
                "target/uat-server.log — `pkill -f 'kairos-server serve'` to stop it).",
                flush=True,
            )
        else:
            if gui_server is not None:
                _stop_process(gui_server, "the GUI server")
            print("Tearing down services...", flush=True)
            docker_down(remove_volumes=True)


@test()
@angreal.command(
    name="all",
    about="run the static gate and all implemented tests (lint + unit + integration)",
    tool=angreal.ToolDescription(
        """
        Run `test lint`, then `test unit`, then `test integration` in
        sequence, stopping at the first failure. Does NOT include the e2e
        tier (release/milestone gate - run `angreal test e2e` explicitly)
        or the soak tier (nightly/pre-release only).

        ## When to use
        - Full pre-completion validation in one command

        ## Output
        Exit code of the first failing step, or 0 if all pass.
        """,
        risk_level="safe",
    ),
)
def all_tests():
    """Run the static gate, then all implemented tests."""
    print("Running the static gate (fmt + clippy)...")
    exit_code = lint()
    if exit_code != 0:
        print("Static gate failed!")
        return exit_code

    print("\nRunning unit tests...")
    exit_code = unit()
    if exit_code != 0:
        print("Unit tests failed!")
        return exit_code

    print("\nRunning integration tests...")
    return integration()
