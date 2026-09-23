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

from utils import PROJECT_ROOT

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
