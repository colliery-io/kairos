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


clean()
