"""
Release tasks — cutting a version, and checking one before it is cut.

`.github/workflows/release.yml` guards the tag against THREE independent
version declarations: the workspace `Cargo.toml`, the chart's `appVersion`, and
the binary's own `--version`. Those guards run per job, so a version that
disagrees in one place does not fail the release — it fails *part* of it, after
other jobs have already pushed artefacts to GHCR. A half-published release is
not something a re-tag fixes.

So the bump is mechanical here, and `angreal release check` applies the same
guards locally, before the tag exists.

## Push main before the tag

The workflow triggers do not overlap. `ci.yml` is `push: branches: [main]`;
`e2e.yml` and `release.yml` are `push: tags: ["v*"]`. So no single push runs
every gate, and `release.yml` runs no gates of its own — it publishes. Tagging
before main lands means the tarballs, the image and the chart are all built
from code `ci.yml` has never run, on commits no branch can reach.

v0.2.0 was pushed in the wrong order for exactly this reason, on the argument
that `docs.yml` would otherwise publish a book naming a chart version not yet
up. It does — for about as long as the image job takes. That is worth much
less than a gate.

## Why an explicit table rather than a search-and-replace

Some version strings in this repository are HISTORY and must not move. The
README says the image is multi-arch "from v0.1.1", which is a fact about v0.1.1
and stays true for ever; `.metis/` is a work record full of version references
that describe what was true when they were written. A blanket rewrite would
corrupt both. Every site below is therefore named, with a pattern narrow enough
that it cannot match the historical mentions.

Adding a version reference somewhere new means adding it here. `check` is what
tells you that you forgot: it fails on a site whose version disagrees, but it
cannot know about a site nobody listed.
"""

import re
import subprocess
import sys
from pathlib import Path

import angreal  # type: ignore

from utils import PROJECT_ROOT

release = angreal.command_group(name="release", about="release cutting commands")

SEMVER = r"(\d+\.\d+\.\d+)"

# (path, pattern, what it is). Each pattern must contain exactly one capture
# group per version occurrence, and must match every occurrence in the file
# that ought to move.
VERSION_SITES = [
    (
        "Cargo.toml",
        rf'^version = "{SEMVER}"$',
        "the workspace version, which every crate inherits and which "
        "`kairos --version` is compiled from (CARGO_PKG_VERSION)",
    ),
    (
        "deploy/helm/kairos/Chart.yaml",
        rf"^version: {SEMVER}$",
        "the chart's own version — what the pushed OCI artefact is tagged with",
    ),
    (
        "deploy/helm/kairos/Chart.yaml",
        rf'^appVersion: "{SEMVER}"$',
        "the Kairos release the chart deploys; image.tag defaults to it",
    ),
    (
        "README.md",
        rf"releases/download/v{SEMVER}/kairos-{SEMVER}-",
        "the CLI download URLs",
    ),
    (
        "README.md",
        rf"--version {SEMVER}",
        "the helm install example",
    ),
    (
        "deploy/helm/kairos/README.md",
        rf"--version {SEMVER}",
        "the chart README's install example",
    ),
    (
        "docs/src/how-to/install-with-helm.md",
        rf"Get a Kairos {SEMVER} deployment",
        "the how-to's stated goal",
    ),
    (
        "docs/src/how-to/install-with-helm.md",
        rf"--version {SEMVER}",
        "the how-to's install command",
    ),
    (
        "docs/src/tutorials/deploy-to-kubernetes.md",
        rf"--version {SEMVER}",
        "the tutorial's install command",
    ),
    (
        "docs/src/tutorials/deploy-to-kubernetes.md",
        rf"^Pulled: ghcr\.io/colliery-io/charts/kairos:{SEMVER}$",
        "the output the tutorial promises the reader will see",
    ),
    (
        "docs/src/reference/rest-api.md",
        rf"OpenAPI 3\.1\.0, Kairos {SEMVER}\.",
        "the generated REST reference header (regenerate with "
        "`angreal docs api`; patched here so `check` agrees either way)",
    ),
]

# Apache-2.0 section 4 asks a redistributor to pass these along, and
# release.yml now hard-fails the tarball step without them rather than
# silently shipping a build with no terms attached.
REQUIRED_FILES = ["LICENSE", "NOTICE"]


def _read(rel: str) -> str:
    return (PROJECT_ROOT / rel).read_text()


def _found(rel: str, pattern: str):
    """Every version captured by `pattern` in `rel`, with line numbers."""
    hits = []
    for n, line in enumerate(_read(rel).splitlines(), start=1):
        for m in re.finditer(pattern, line, flags=re.MULTILINE):
            hits.extend((n, g) for g in m.groups())
    return hits


def _workspace_version() -> str:
    hits = _found("Cargo.toml", rf'^version = "{SEMVER}"$')
    if len(hits) != 1:
        raise SystemExit(
            f"Cargo.toml: expected exactly one workspace version, found {len(hits)}"
        )
    return hits[0][1]


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=str(PROJECT_ROOT),
        capture_output=True, text=True, check=True,
    ).stdout.strip()


@release()
@angreal.command(
    name="check",
    about="verify every version reference agrees, before the tag exists",
    tool=angreal.ToolDescription(
        """
        Apply release.yml's guards locally. Read-only — touches nothing.

        Checks, in order:
        - every site in VERSION_SITES declares the same version as the
          workspace Cargo.toml
        - LICENSE and NOTICE exist (release.yml fails the tarball without them)
        - no tag already exists for this version
        - the working tree is clean

        ## Why this exists
        The three guards in release.yml run in SEPARATE JOBS. A mismatch fails
        one job while the others publish, leaving a partly-released version that
        re-tagging does not undo.

        ## Output
        A table of sites and their versions; non-zero exit naming each
        disagreement. Exit 0 means the tag is safe to push.
        """,
        risk_level="read_only",
    ),
)
def release_check():
    expected = _workspace_version()
    print(f"workspace version: {expected}\n")

    problems = []
    for rel, pattern, what in VERSION_SITES:
        hits = _found(rel, pattern)
        if not hits:
            problems.append(f"{rel}: no match for {what} — pattern {pattern!r}")
            continue
        bad = [(n, v) for n, v in hits if v != expected]
        mark = "✗" if bad else "✓"
        seen = ",".join(sorted({v for _, v in hits}))
        print(f"  {mark} {rel}: {seen}  ({len(hits)}×) — {what}")
        for n, v in bad:
            problems.append(f"{rel}:{n}: {v}, expected {expected} — {what}")

    print()
    for name in REQUIRED_FILES:
        if (PROJECT_ROOT / name).exists():
            print(f"  ✓ {name}")
        else:
            problems.append(f"{name} is missing; release.yml will fail the tarball step")

    tag = f"v{expected}"
    if tag in _git("tag", "--list").splitlines():
        print(f"  • {tag} already exists locally")
    else:
        print(f"  • {tag} does not exist yet")

    dirty = _git("status", "--porcelain")
    if dirty:
        problems.append(
            "the working tree is dirty; the tag would not describe what you tested:\n"
            + dirty
        )

    sys.stdout.flush()
    if problems:
        print("\nNOT ready to release:", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1

    print(f"\n{expected} is consistent everywhere. Next:")
    print("    git push origin main       # ci.yml gates it")
    print("    # wait for ci.yml to go green")
    print(f"    git push origin {tag}      # e2e.yml + release.yml publish")
    print(
        "\nMAIN FIRST, and not for taste. No single push runs every gate:\n"
        "  ci.yml       push: branches: [main]  fmt, clippy, unit, integration\n"
        "  e2e.yml      push: tags: ['v*']      the end-to-end smoke\n"
        "  release.yml  push: tags: ['v*']      publishes, and gates NOTHING\n"
        "                                      but the three version guards\n"
        "\nTag first and the artefacts publish from code ci.yml has never seen,\n"
        "on commits no branch can reach. Pushing main first costs only this:\n"
        "docs.yml publishes the book immediately, so for as long as the image\n"
        "job runs it names a chart version that is not up yet. That is a\n"
        "cosmetic window on one page, and the cheaper of the two."
    )
    return 0


@release()
@angreal.command(
    name="version",
    about="set the release version everywhere it is declared",
    tool=angreal.ToolDescription(
        """
        Rewrite every site in VERSION_SITES to `--to`, then verify with the
        same logic as `angreal release check`.

        Edits tracked files and nothing else — no commit, no tag, no push. Run
        `git diff` afterwards; the prose around a bumped version often needs a
        human (a sentence that said "this version does not need pgvector yet"
        was true at 0.1.1 and a lie at 0.2.0).

        ## When to use
        - Cutting a release, before committing and tagging

        ## Examples
        ```
        angreal release version --to 0.3.0
        angreal docs api            # regenerate the REST reference properly
        angreal release check
        ```

        ## Output
        One line per file changed, then the check. Non-zero if a site did not
        match — which means the reference moved and the table needs updating.
        """,
        # safe rather than destructive: it rewrites tracked files and nothing
        # else, so `git checkout` undoes it completely. angreal has no middle
        # value — the set is safe | read_only | destructive.
        risk_level="safe",
    ),
)
@angreal.argument(
    name="to",
    long="to",
    takes_value=True,
    required=True,
    help="the version to set, e.g. 0.3.0 (no leading v)",
)
def release_version(to: str):
    if not re.fullmatch(SEMVER, to):
        print(f"{to!r} is not a bare semver like 0.3.0", file=sys.stderr)
        return 1

    changed = {}
    missed = []
    for rel, pattern, what in VERSION_SITES:
        path = PROJECT_ROOT / rel
        before = path.read_text()

        def sub(m):
            out = m.group(0)
            # Replace each captured group, rightmost first, so earlier spans
            # stay valid as the string shortens or grows.
            for g in reversed(range(1, (m.lastindex or 0) + 1)):
                s, e = m.span(g)
                out = out[: s - m.start()] + to + out[e - m.start():]
            return out

        after, n = re.subn(pattern, sub, before, flags=re.MULTILINE)
        if n == 0:
            missed.append(f"{rel}: no match for {what} — pattern {pattern!r}")
            continue
        if after != before:
            path.write_text(after)
            changed[rel] = changed.get(rel, 0) + n

    for rel, n in sorted(changed.items()):
        print(f"  {rel}: {n} occurrence(s) -> {to}")
    if not changed:
        print(f"  nothing to change; already {to}")

    if missed:
        print("\nsites that did not match — the table is stale:", file=sys.stderr)
        for m in missed:
            print(f"  - {m}", file=sys.stderr)
        return 1

    print()
    return release_check()
