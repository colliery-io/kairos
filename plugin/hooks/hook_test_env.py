"""The clean environment of the hook tests (KAIROS-T-0712).

Each test file of the hooks imports this module first. At import, it
removes each `KAIROS_*` variable and the variables of a Claude Code session
from the environment of the test process, and it sets a temp `HOME`, a temp
config folder and a temp cache folder. Thus a test that copies
`os.environ` for a hook does not give the hook the deployment, the key or
the login of the person, and no test writes into the real home folder.

The temp folder is removed when the process ends. The work runs once per
process. A test that needs a variable sets it itself. Python stdlib only.
"""

import atexit
import json
import os
import shutil
import tempfile

SESSION_VARS = ("CLAUDE_PROJECT_DIR", "CLAUDE_ENV_FILE", "CLAUDE_PLUGIN_ROOT")
MARK = "KAIROS_HOOK_TEST_HOME"


def clean():
    """Make the environment of this process clean. Idempotent."""
    if os.environ.get(MARK):
        return
    for name in list(os.environ):
        if name.startswith("KAIROS_") or name in SESSION_VARS:
            del os.environ[name]
    home = tempfile.mkdtemp(prefix="kairos-hook-tests-")
    atexit.register(shutil.rmtree, home, True)
    os.environ["HOME"] = home
    os.environ["XDG_CONFIG_HOME"] = os.path.join(home, ".config")
    os.environ["XDG_CACHE_HOME"] = os.path.join(home, ".cache")
    os.environ["KAIROS_CONFIG_DIR"] = os.path.join(home, ".config", "kairos")
    # The git of a test has no global config of the person: give it an
    # identity for a commit.
    os.environ.setdefault("GIT_AUTHOR_NAME", "Robin")
    os.environ.setdefault("GIT_AUTHOR_EMAIL", "robin@example.test")
    os.environ.setdefault("GIT_COMMITTER_NAME", "Robin")
    os.environ.setdefault("GIT_COMMITTER_EMAIL", "robin@example.test")
    os.environ[MARK] = home


# The hooks read the wiring of a folder from `kairos config show --json`
# alone (KAIROS-T-0742: no hook reads `.claude/kairos.local.md`). A test
# that wires a folder puts a stub `kairos` on PATH: `stub_kairos` writes
# one, and `write_binding` writes the JSON that the stub prints for
# `config show`. The stub reads the JSON file that STUB_BINDING names;
# with none, no folder is bound. With STUB_BOUND (folders, `:` between
# them), only those folders and the folders below them are bound: the
# folder is the last argument of `config show --json <folder>`.

# The start of a stub `kairos` (a sh script): the `config` verb.
STUB_CONFIG_SH = '''if [ "$1" = "config" ]; then
  bound=1
  if [ -n "$STUB_BOUND" ]; then
    bound=0
    eval "asked=\${$#}"
    old_ifs=$IFS
    IFS=:
    for folder in $STUB_BOUND; do
      case "$asked" in "$folder"|"$folder"/*) bound=1 ;; esac
    done
    IFS=$old_ifs
  fi
  if [ "$bound" = 1 ] && [ -n "$STUB_BINDING" ] && [ -f "$STUB_BINDING" ]; then
    cat "$STUB_BINDING"
  else
    echo '{"bound": false, "values": {}}'
  fi
  exit 0
fi
'''

# The keys of the hooks, and the keys of the binding of the CLI.
BINDING_KEYS = {
    "deployment_url": "deployment",
    "tenant": "tenant",
    "repository": "repository",
    "delivery_stream": "delivery_stream",
    "team_board": "team_board",
    "initiative_board": "initiative_board",
    "adr_board": "adr_board",
}


def binding_json(values, origin="project", **extra):
    """The JSON of `kairos config show --json` for a bound folder with
    `values` (the keys of the hooks: `deployment_url`, `tenant`, ...), each
    from `origin`. `extra` adds fields such as `env_url` or `login_url`."""
    data = {
        "bound": True,
        "values": {
            BINDING_KEYS[key]: {"value": value, "from": origin, "file": "/c/config.toml"}
            for key, value in values.items()
        },
    }
    data.update(extra)
    return json.dumps(data)


def write_binding(path, values, **extra):
    """Write the binding of `values` to `path`, the file of STUB_BINDING."""
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(binding_json(values, **extra))


def stub_kairos(bin_dir, body="exit 64\n"):
    """Write a stub `kairos` into `bin_dir` (made when needed): the `config`
    verb of STUB_CONFIG_SH, then `body` for each other verb (by default the
    usage exit code of an older CLI). Returns its path."""
    os.makedirs(bin_dir, exist_ok=True)
    path = os.path.join(bin_dir, "kairos")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("#!/bin/sh\n" + STUB_CONFIG_SH + body)
    os.chmod(path, 0o755)
    return path


clean()
