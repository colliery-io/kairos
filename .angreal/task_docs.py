"""
Documentation book tasks — KAIROS-T-0167 (KAIROS-I-0016), KAIROS-T-0389.

The book is an Astro Starlight site in docs/, with the Colliery theme
starlight-aurora. `angreal docs build` builds it into docs/dist; `angreal docs
serve` runs the live-reloading dev server. Both need Node 22 or later, and
install the npm dependencies on the first run. `angreal docs images` makes the
screenshots of the book from a new demo seed (COLLIERY-T-0252).

The published site is built by .github/workflows/docs.yml on pushes touching
docs/**, deliberately decoupled from release tags so documentation ships
independently of binaries and images.
"""

import os
import shutil
import subprocess
import sys

import angreal  # type: ignore

# `angreal docs images` uses the start, seed and stop code of the GUI leg of
# `angreal test e2e` (COLLIERY-T-0252). The import is here, at load time, as
# task_test.py does with task_web: an import in the command does not return.
import task_test as gui

from utils import PROJECT_ROOT, TARGET_DIR, docker_down, docker_up

docs = angreal.command_group(name="docs", about="documentation book commands")

BOOK_ROOT = PROJECT_ROOT / "docs"
BOOK_OUT = BOOK_ROOT / "dist"
IMAGES = BOOK_ROOT / "src" / "assets" / "images"


def _npm(*args):
    """Run npm in docs/. Install the dependencies first if they are missing."""
    npm = shutil.which("npm")
    if not npm:
        print("npm not found: install Node 22 or later", file=sys.stderr)
        return 1
    if not (BOOK_ROOT / "node_modules").exists():
        result = subprocess.run(
            [npm, "install", "--no-audit", "--no-fund"], cwd=str(BOOK_ROOT)
        )
        if result.returncode != 0:
            return result.returncode
    return subprocess.run([npm, *args], cwd=str(BOOK_ROOT)).returncode


@docs()
@angreal.command(
    name="build",
    about="build the documentation book into docs/dist",
    tool=angreal.ToolDescription(
        "Builds the Starlight book in docs/ into docs/dist. Installs the npm "
        "dependencies on the first run. Fails when a page does not build.",
        risk_level="safe",
    ),
)
def docs_build():
    result = _npm("run", "build")
    if result == 0:
        print(f"book written to {BOOK_OUT}")
    return result


@docs()
@angreal.command(
    name="ste",
    about="check procedural docs against KAIROS-S-0009 (Simplified Technical English)",
    tool=angreal.ToolDescription(
        """
        Check docs/src/content/docs/{tutorials,how-to,reference} against the mechanical half of
        KAIROS-S-0009 (ASD-STE100): sentence length, paragraph length, passive voice,
        gerund chains, and the banned domain synonyms in section 4.1.

        BASELINED. scripts/ste-baseline.json records the violation count per file and
        the gate fails only when a file gets WORSE. A new file must be clean. The
        baseline is a ceiling that only goes down.

        NOT CHECKED: STE-V1, the approved vocabulary. It needs ASD's word list as
        data and ASD owns its copyright, so this repository ships no copy. A clean run
        is NOT STE conformance.

        Out of scope entirely: explanation/, ADRs, Status Updates, commit messages.
        STE is hostile to argument and those exist to argue.

        --code (COLLIERY-T-0258) checks the texts of errors in the Rust code in place
        of the book: each `#[error("...")]` attribute of kairos-core, kairos-db and
        the server, and each text that is a direct argument of an `ApiError`
        constructor. It reads the source as text and needs no Rust build. Its baseline
        is scripts/ste-code-baseline.json. It adds the rules of a text of an error: a
        capital at the start, a period at the end, no contraction, no semicolon, no
        dash or arrow as punctuation, no Latin abbreviation, no plural in parentheses.
        It CANNOT see a text that the code puts in a variable first, the result texts
        of the MCP tools, the CLI texts and the GUI texts.

        --refusals (KAIROS-T-0690) runs scripts/refusal-check.py: no refusal of the
        server or of the CLI MCP servers is built from the text of a library (serde,
        sqlx, reqwest, diesel, a foreign error, HTML), and each VALIDATION or
        NOT_FOUND refusal has a `field` detail. Its baseline is
        scripts/refusal-baseline.json. With --list it prints each finding; with
        --next, the refusals that name no next call (for review, not a failure).

        --list      print every violation with its rule ID
        --baseline  rewrite the baseline from the current tree (lock in improvements)
        --code      check the texts of errors in the code, not the book
        --refusals  check the refusals of the code (library text, `field`)
        --next      with --refusals: list the refusals that name no next call
        """,
        risk_level="read_only",
    ),
)
@angreal.argument(
    name="list_all",
    long="list",
    takes_value=False,
    is_flag=True,
    help="print every violation with its rule ID",
)
@angreal.argument(
    name="baseline",
    long="baseline",
    takes_value=False,
    is_flag=True,
    help="rewrite scripts/ste-baseline.json from the current tree",
)
@angreal.argument(
    name="code",
    long="code",
    takes_value=False,
    is_flag=True,
    help="check the texts of errors in the Rust code, not the book",
)
@angreal.argument(
    name="refusals",
    long="refusals",
    takes_value=False,
    is_flag=True,
    help="check the refusals of the code: no library text, a field (KAIROS-T-0690)",
)
@angreal.argument(
    name="next_call",
    long="next",
    takes_value=False,
    is_flag=True,
    help="with --refusals: list the refusals that name no next call",
)
def docs_ste(list_all=False, baseline=False, code=False, refusals=False, next_call=False):
    """Run scripts/ste-check.py. Pure Python, no services, no build."""
    if refusals:
        args = [sys.executable, str(PROJECT_ROOT / "scripts" / "refusal-check.py")]
        if list_all:
            args.append("--list")
        if baseline:
            args.append("--baseline")
        if next_call:
            args.append("--next")
        return subprocess.run(args, cwd=str(PROJECT_ROOT)).returncode
    args = [sys.executable, str(PROJECT_ROOT / "scripts" / "ste-check.py")]
    if code:
        args.append("--code")
    if list_all:
        args.append("--list")
    if baseline:
        args.append("--baseline")
    return subprocess.run(args, cwd=str(PROJECT_ROOT)).returncode


@docs()
@angreal.command(
    name="api",
    about="regenerate the REST reference pages from the OpenAPI spec",
)
def docs_api():
    """Render docs/src/content/docs/reference/rest{-api.md,/*.md} from the spec.

    The spec comes from the same pure test CI uses (no services needed), so
    this works on a laptop with nothing running.
    """
    spec = TARGET_DIR / "openapi.json"
    print("producing the OpenAPI spec...")
    produced = subprocess.run(
        [
            "cargo", "test", "-q",
            "-p", "kairos-server",
            "--test", "openapi",
            "write_spec_artifact",
        ],
        cwd=str(PROJECT_ROOT),
    )
    if produced.returncode != 0 or not spec.exists():
        print("could not produce target/openapi.json", file=sys.stderr)
        return 1
    return subprocess.run(
        [sys.executable, str(PROJECT_ROOT / "scripts" / "render-openapi.py")],
        cwd=str(PROJECT_ROOT),
    ).returncode


@docs()
@angreal.command(
    name="serve",
    about="serve the book locally with live reload",
    tool=angreal.ToolDescription(
        "Serves the book at http://localhost:4321 (or --port) and reloads on "
        "each change. Runs until stopped.",
        risk_level="safe",
    ),
)
@angreal.argument(
    name="port", long="port", takes_value=True,
    help="port to serve on (default 4321)",
)
def docs_serve(port=None):
    args = ["run", "dev"]
    if port:
        args += ["--", "--port", str(port)]
    return _npm(*args)


def _images_phase(name, exit_code):
    """Name the phase that failed; return the exit code unchanged."""
    if exit_code != 0:
        print(
            f"DOCS IMAGES FAILED at phase: {name} (exit {exit_code})",
            file=sys.stderr,
        )
    return exit_code


@docs()
@angreal.command(
    name="images",
    about="make the screenshots of the book from a new demo seed",
    tool=angreal.ToolDescription(
        """
        Make the images of docs/src/assets/images (COLLIERY-T-0252). The images come from
        e2e/tests/capture-docs-images.spec.ts, which `angreal test e2e` does not run.

        The task uses the start, seed and stop code of the GUI leg of
        `angreal test e2e`:

        1. compose up (Postgres + Dex, --wait), project `kairos-dev`
        2. cargo build kairos-server; `angreal web build`
        3. `kairos-server seed-demo --force`, so the images show the demo seed
        4. boot kairos-server on :41080
        5. `npx playwright test capture-docs-images --grep @docs` in e2e/
        6. stop the server, compose down -v

        The task writes the PNG files in docs/src/assets/images. Look at each image
        and at the text near it in the book before you commit.
        docs/src/assets/images/README.md lists the images and the pages that show
        them.

        DESTRUCTIVE for the `demo` tenant of the dev database (seed --force), and it
        removes the compose volumes of the dev stack at the end. Port 41080 must be
        free: stop a dev server first.

        ## When to use
        - A change of the demo seed or of the GUI makes an image of the book stale
        """,
        risk_level="destructive",
    ),
)
def docs_images():
    """Compose up -> seed-demo --force -> serve -> capture spec -> down."""
    print("Starting docker services for the images of the book...", flush=True)
    exit_code = _images_phase("compose up", docker_up())
    if exit_code != 0:
        return exit_code

    server = None
    try:
        print("Building kairos-server...", flush=True)
        exit_code = _images_phase(
            "cargo build",
            subprocess.run(
                [
                    "cargo", "build", "--quiet",
                    "-p", "kairos-server", "--bin", "kairos-server",
                ],
                cwd=str(PROJECT_ROOT),
            ).returncode,
        )
        if exit_code != 0:
            return exit_code

        env = os.environ.copy()
        env.setdefault("DATABASE_URL", gui.E2E_DATABASE_URL)
        exit_code = gui._prepare_gui_stack(env, "docs images")
        if exit_code != 0:
            return _images_phase("web build and seed", exit_code)
        exit_code = _images_phase(
            "playwright install", gui._ensure_playwright(gui.E2E_DIR)
        )
        if exit_code != 0:
            return exit_code
        server, exit_code = gui._boot_gui_server(gui._gui_server_env(env))
        if exit_code != 0:
            return _images_phase("GUI server boot", exit_code)

        print("Running the capture spec...", flush=True)
        pw_env = os.environ.copy()
        pw_env.update({
            "E2E_GUI_BASE_URL": gui.E2E_GUI_BASE_URL,
            "E2E_ISSUER": gui.E2E_ISSUER,
        })
        exit_code = _images_phase(
            "capture spec",
            subprocess.run(
                # No retry: a second attempt would write the images of a
                # tenant that the first attempt changed.
                [
                    "npx", "playwright", "test", "capture-docs-images",
                    "--grep", "@docs", "--retries=0",
                ],
                cwd=str(gui.E2E_DIR),
                env=pw_env,
            ).returncode,
        )
        if exit_code == 0:
            print(f"images written to {IMAGES}", flush=True)
        return exit_code
    finally:
        if server is not None:
            gui._stop_process(server, "the GUI server")
        print("Tearing down services...", flush=True)
        docker_down(remove_volumes=True)
