#!/usr/bin/env python3
"""Unit tests for the clean environment of the hook tests (KAIROS-T-0712).
The live env of the person (KAIROS_URL, a key, CLAUDE_PROJECT_DIR) must not
reach a hook in a test, and no test may write into the real home folder.
Stdlib only."""

import os
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))

CHILD = """
import os, sys
sys.path.insert(0, sys.argv[1])
import hook_test_env
names = sorted(n for n in os.environ if n.startswith("KAIROS_") or n in hook_test_env.SESSION_VARS)
print(os.environ["HOME"])
print(",".join(names))
"""


class CleanEnv(unittest.TestCase):
    def test_the_live_env_does_not_reach_a_test(self):
        env = dict(
            os.environ,
            KAIROS_URL="https://live.example.com",
            KAIROS_MCP_KEY="live-key",
            KAIROS_KEY="live-key",
            CLAUDE_PROJECT_DIR="/somewhere",
            HOME="/real/home",
        )
        env.pop("KAIROS_HOOK_TEST_HOME", None)
        out = subprocess.run(
            [sys.executable, "-I", "-c", CHILD, HERE],
            capture_output=True,
            text=True,
            env=env,
            check=True,
        )
        home, names = out.stdout.splitlines()
        self.assertIn("kairos-hook-tests-", home)
        self.assertNotEqual(home, "/real/home")
        self.assertFalse(os.path.exists(home), "the temp HOME is removed at the end")
        self.assertEqual(names.split(","), ["KAIROS_CONFIG_DIR", "KAIROS_HOOK_TEST_HOME"])


if __name__ == "__main__":
    unittest.main()
