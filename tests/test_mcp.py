#!/usr/bin/env python3
"""Bounds and project binding in zolly_mcp.py.

These are the findings a security review can verify without a live herdr:
oversized input is refused before a subprocess starts, wait values are
integers in range, diagnostics are redacted and bounded, and a project's
config cannot be overridden into another office by an inherited environment.

Run from the kit root: `./test.sh` (or `python3 -m unittest discover -s tests`).
"""

import contextlib
import importlib.util
import io
import os
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location(
    "zolly_mcp", ROOT / ".agents" / "zolly" / "mcp" / "zolly_mcp.py"
)
mcp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mcp)


class McpLimitTests(unittest.TestCase):
    def assert_refused(self, result, needle):
        self.assertFalse(result["ok"])
        self.assertFalse(result["delivered"])
        self.assertIn(needle, result["error"])

    def test_oversized_text_is_refused(self):
        result = mcp.zolly_say("x" * (mcp.MAX_TEXT_BYTES + 1))
        self.assert_refused(result, "exceeds")

    def test_non_integer_wait_is_refused(self):
        for value in (True, 1.5, "2000"):
            with self.subTest(value=value):
                self.assert_refused(mcp.zolly_say("hello", value), "integer")

    def test_out_of_range_wait_is_refused(self):
        for value in (-1, mcp.MAX_WAIT_MS + 1):
            with self.subTest(value=value):
                self.assert_refused(mcp.zolly_say("hello", value), "0-%d" % mcp.MAX_WAIT_MS)

    def test_diagnostics_redact_and_truncate(self):
        text = mcp._diag("ZOLLY_TELEGRAM_TOKEN=supersecretvalue")
        self.assertNotIn("supersecretvalue", text)
        self.assertIn("[redacted]", text)
        self.assertLessEqual(len(mcp._diag("x" * 10000)), mcp.MAX_DIAG)


class McpBindingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        cfg = Path(self.tmp.name) / ".agents" / "zolly"
        cfg.mkdir(parents=True)
        (cfg / "config.toml").write_text('frequency = "office-a"\n', encoding="utf-8")
        self._saved = {k: os.environ.get(k) for k in ("ZOLLY_ROOT", "ZOLLY_MCP_AGENT")}
        os.environ["ZOLLY_ROOT"] = self.tmp.name

    def tearDown(self):
        for key, value in self._saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        self.tmp.cleanup()

    def test_project_config_wins_over_a_conflicting_env_override(self):
        os.environ["ZOLLY_MCP_AGENT"] = "zolly-other-office"
        with contextlib.redirect_stderr(io.StringIO()) as err:
            name = mcp._agent_name()
        self.assertEqual(name, "zolly-office-a")
        self.assertIn("ignoring ZOLLY_MCP_AGENT", err.getvalue())

    def test_env_override_is_used_only_when_no_project_resolves(self):
        os.environ.pop("ZOLLY_ROOT")
        os.environ["ZOLLY_MCP_AGENT"] = "zolly-explicit"
        with tempfile.TemporaryDirectory() as empty:
            cwd = os.getcwd()
            os.chdir(empty)
            try:
                self.assertEqual(mcp._agent_name(), "zolly-explicit")
            finally:
                os.chdir(cwd)


if __name__ == "__main__":
    unittest.main()
