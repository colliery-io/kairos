#!/usr/bin/env python3
"""Tests of the `kairos-code` wrapper (COLLIERY-T-2529, KAIROS-T-0513): the
version comparison, and the server end to end with a fake `kairos` on PATH
that is older than, equal to or newer than the plugin, or absent. Stdlib
only."""

import importlib.machinery
import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "bin", "kairos-code")
LOADER = importlib.machinery.SourceFileLoader("kairos_code", SCRIPT)
SPEC = importlib.util.spec_from_loader("kairos_code", LOADER)
code = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(code)

INITIALIZE = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                         "params": {"protocolVersion": "2025-06-18"}}) + "\n"


class Versions(unittest.TestCase):
    def test_an_older_cli_is_older(self):
        self.assertTrue(code.is_older("0.13.0", "0.15.0"))
        self.assertTrue(code.is_older("kairos 0.13.0".split()[-1], "0.15.0"))
        self.assertTrue(code.is_older("0.9.9", "0.15.0"))

    def test_an_equal_or_newer_cli_is_not_older(self):
        self.assertFalse(code.is_older("0.15.0", "0.15.0"))
        self.assertFalse(code.is_older("0.16.0", "0.15.0"))
        self.assertFalse(code.is_older("1.0.0", "0.15.0"))

    def test_an_unknown_version_counts_as_current(self):
        self.assertFalse(code.is_older(None, "0.15.0"))
        self.assertFalse(code.is_older("0.13.0", None))
        self.assertFalse(code.is_older("dev", "0.15.0"))
        self.assertFalse(code.is_older("0.13.0", "main"))

    def test_a_prerelease_compares_by_its_numbers(self):
        self.assertTrue(code.is_older("0.14.0-rc1", "0.15.0"))
        self.assertFalse(code.is_older("v0.15.0-rc1", "0.15.0"))

    def test_the_plugin_version_is_read_from_the_manifest(self):
        with tempfile.TemporaryDirectory() as root:
            os.makedirs(os.path.join(root, ".claude-plugin"))
            with open(os.path.join(root, ".claude-plugin", "plugin.json"), "w") as f:
                json.dump({"name": "kairos", "version": "0.15.0"}, f)
            self.assertEqual(code.plugin_version(root), "0.15.0")
            self.assertIsNone(code.plugin_version(os.path.join(root, "nowhere")))

    def test_the_real_manifest_gives_a_version(self):
        self.assertRegex(code.plugin_version(), r"^\d+\.\d+\.\d+")

    def test_the_old_cli_instructions_name_both_versions_and_the_command(self):
        text = code.old_cli("0.13.0", "0.15.0")
        self.assertIn("0.13.0", text)
        self.assertIn("0.15.0", text)
        self.assertIn("kairos self-update", text)
        self.assertIn("module_map", text)


class EndToEnd(unittest.TestCase):
    """The wrapper as Claude Code runs it, with a fake `kairos` on PATH."""

    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.bin = os.path.join(self.root.name, "bin")
        os.makedirs(self.bin)
        plugin = os.path.join(self.root.name, "plugin")
        os.makedirs(os.path.join(plugin, ".claude-plugin"))
        with open(os.path.join(plugin, ".claude-plugin", "plugin.json"), "w") as f:
            json.dump({"name": "kairos", "version": "0.15.0"}, f)
        self.env = {k: v for k, v in os.environ.items()
                    if k not in ("KAIROS_URL", "KAIROS_MCP_KEY")}
        self.env["PATH"] = self.bin
        self.env["CLAUDE_PLUGIN_ROOT"] = plugin

    def fake_kairos(self, version):
        """A `kairos` that prints its version, and echoes `index mcp`."""
        path = os.path.join(self.bin, "kairos")
        with open(path, "w") as f:
            f.write(
                "#!/bin/sh\n"
                f'if [ "$1" = "--version" ]; then echo "kairos {version}"; exit 0; fi\n'
                'echo "ran: $*"\n'
            )
        os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)

    def run_wrapper(self):
        proc = subprocess.run([sys.executable, SCRIPT, "--extra"], input=INITIALIZE,
                              capture_output=True, text=True, env=self.env, timeout=30)
        self.assertEqual(proc.stderr, "")
        return proc.stdout

    def test_an_older_cli_gets_the_server_with_no_tools(self):
        self.fake_kairos("0.13.0")
        reply = json.loads(self.run_wrapper().splitlines()[0])
        instructions = reply["result"]["instructions"]
        self.assertIn("kairos CLI is 0.13.0", instructions)
        self.assertIn("plugin is 0.15.0", instructions)
        self.assertIn("kairos self-update", instructions)
        self.assertEqual(reply["result"]["capabilities"], {"tools": {}})

    def test_an_equal_cli_runs_index_mcp(self):
        self.fake_kairos("0.15.0")
        self.assertEqual(self.run_wrapper().strip(), "ran: index mcp --extra")

    def test_a_newer_cli_runs_index_mcp(self):
        self.fake_kairos("0.16.0")
        self.assertEqual(self.run_wrapper().strip(), "ran: index mcp --extra")

    def test_a_cli_with_no_readable_version_runs(self):
        self.fake_kairos("dev")
        self.assertEqual(self.run_wrapper().strip(), "ran: index mcp --extra")

    def test_no_cli_gets_the_server_with_no_tools(self):
        out = self.run_wrapper()
        reply = json.loads(out.splitlines()[0])
        self.assertIn("not on PATH", reply["result"]["instructions"])
        self.assertEqual(reply["result"]["capabilities"], {"tools": {}})

    def test_the_server_with_no_tools_lists_none_and_refuses_the_rest(self):
        lines = INITIALIZE + "\n".join([
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json.dumps({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json.dumps({"jsonrpc": "2.0", "id": 4, "method": "tools/call"}),
        ]) + "\n"
        proc = subprocess.run([sys.executable, SCRIPT], input=lines, capture_output=True,
                              text=True, env=self.env, timeout=30)
        replies = [json.loads(line) for line in proc.stdout.splitlines()]
        self.assertEqual([r["id"] for r in replies], [1, 2, 3, 4])
        self.assertEqual(replies[1]["result"], {"tools": []})
        self.assertEqual(replies[2]["result"], {})
        self.assertEqual(replies[3]["error"]["code"], -32601)


if __name__ == "__main__":
    unittest.main()


MCP_SCRIPT = os.path.join(HERE, "..", "bin", "kairos-mcp")


class KairosMcp(unittest.TestCase):
    """#270: the `kairos` server of the plugin runs `kairos mcp-proxy`, with
    the fallback of `kairos-code` for an older CLI or none."""

    setUp = EndToEnd.setUp
    fake_kairos = EndToEnd.fake_kairos

    def run_wrapper(self):
        proc = subprocess.run([sys.executable, MCP_SCRIPT, "--extra"], input=INITIALIZE,
                              capture_output=True, text=True, env=self.env, timeout=30)
        self.assertEqual(proc.stderr, "")
        return proc.stdout

    def test_a_current_cli_runs_the_proxy(self):
        self.fake_kairos("0.15.0")
        self.assertEqual(self.run_wrapper().strip(), "ran: mcp-proxy --extra")

    def test_an_older_cli_gets_the_server_with_no_tools_and_the_reason(self):
        self.fake_kairos("0.13.0")
        reply = json.loads(self.run_wrapper().splitlines()[0])
        self.assertEqual(reply["result"]["serverInfo"]["name"], "kairos")
        instructions = reply["result"]["instructions"]
        self.assertIn("kairos CLI is 0.13.0 and the plugin is 0.15.0", instructions)
        self.assertIn("`kairos mcp-proxy`", instructions)
        self.assertIn("kairos self-update", instructions)

    def test_no_cli_gets_the_server_with_no_tools(self):
        reply = json.loads(self.run_wrapper().splitlines()[0])
        self.assertEqual(reply["result"]["serverInfo"]["name"], "kairos")
        self.assertIn("`kairos` is not on PATH", reply["result"]["instructions"])

    def test_the_manifest_starts_the_launcher(self):
        with open(os.path.join(HERE, "..", "..", ".claude-plugin", "plugin.json")) as f:
            server = json.load(f)["mcpServers"]["kairos"]
        self.assertEqual(server["type"], "stdio")
        self.assertEqual(server["command"], "${CLAUDE_PLUGIN_ROOT}/plugin/bin/kairos-mcp")
