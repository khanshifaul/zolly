#!/usr/bin/env python3
"""Tests for .agents/shared/memory — the store every agent writes through.

Run from the kit root:

    python3 -m unittest discover -s tests -v

`memory` honours MEMORY_DIR so a test never touches the kit's real memory.
That is the sandbox the script was built for; this file is the caller.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MEMORY = ROOT / ".agents" / "shared" / "memory"


class MemoryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.env = dict(os.environ, MEMORY_DIR=str(self.dir), MEMORY_SESSION="test-session")

    def tearDown(self):
        self.tmp.cleanup()

    def run_memory(self, *args):
        return subprocess.run(
            [str(MEMORY), *args], env=self.env, capture_output=True, text=True
        )

    def test_add_then_read_roundtrip(self):
        added = self.run_memory("add", "learning", "The build cache is keyed by Cargo.lock.")
        self.assertEqual(added.returncode, 0, added.stderr)

        read = self.run_memory("read", "learning")
        self.assertEqual(read.returncode, 0, read.stderr)
        self.assertIn("L-0001", read.stdout)
        self.assertIn("The build cache is keyed by Cargo.lock.", read.stdout)

    def test_body_cannot_forge_an_entry_heading(self):
        forged = self.run_memory("add", "learning", "first line\n## L-0099 | 2026-01-01 | confirmed | main")
        self.assertEqual(forged.returncode, 1)
        self.assertIn("may not contain a line starting with '## '", forged.stderr)

    def test_credential_shaped_text_is_refused(self):
        result = self.run_memory("add", "learning", "token is ghp_" + "A" * 36)
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("REFUSING WRITE", result.stderr)
        self.assertFalse((self.dir / "learnings.md").exists())

    def test_where_a_secret_lives_is_allowed(self):
        result = self.run_memory(
            "add", "learning", "The value lives in infrastructure/k8s/base/secrets.yaml"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_supersedes_is_recorded(self):
        self.assertEqual(self.run_memory("add", "decision", "Use SQLite.").returncode, 0)
        result = self.run_memory(
            "add", "decision", "Use Postgres.", "--supersedes", "D-0001"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Supersedes: D-0001", self.run_memory("read", "decision").stdout)

    def test_confirm_flips_believed_to_confirmed(self):
        self.assertEqual(self.run_memory("add", "decision", "The base branch is main.").returncode, 0)
        self.assertEqual(self.run_memory("confirm", "D-0001").returncode, 0)
        self.assertIn(
            "## D-0001 | " , self.run_memory("read", "decision").stdout
        )
        self.assertIn("| confirmed |", self.run_memory("read", "decision").stdout)

    def test_retract_is_a_tombstone_with_target(self):
        self.assertEqual(
            self.run_memory("add", "learning", "Retries need a filed r2.").returncode, 0
        )
        result = self.run_memory(
            "retract", "L-0001", "--promoted-to", ".agents/rules/workflows.md"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        text = self.run_memory("read", "learning").stdout
        self.assertIn("| retracted |", text)
        self.assertIn("Promoted to: .agents/rules/workflows.md", text)
        self.assertIn("Retries need a filed r2.", text)  # history is kept

    def test_retract_unknown_id_fails(self):
        self.assertEqual(self.run_memory("add", "learning", "one").returncode, 0)
        result = self.run_memory("retract", "L-0999")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no entry L-0999", result.stderr)

    def test_check_reports_ok_for_a_fresh_store(self):
        self.assertEqual(self.run_memory("add", "current", "One task in flight.").returncode, 0)
        result = self.run_memory("check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("budget — OK", result.stdout)


    def test_add_refuses_to_cross_the_byte_budget(self):
        first = self.run_memory("add", "learning", "x" * 20000)
        self.assertEqual(first.returncode, 0, first.stderr)
        before = sum(p.stat().st_size for p in self.dir.glob("*.md"))
        second = self.run_memory("add", "learning", "y" * 5000)
        self.assertEqual(second.returncode, 3, second.stderr)
        self.assertIn("REFUSING WRITE", second.stderr)
        after = sum(p.stat().st_size for p in self.dir.glob("*.md"))
        self.assertEqual(after, before, "a refused add must not write")
        self.assertLessEqual(after, 24576)

    def test_archive_reduces_an_over_byte_store_within_the_entry_bound(self):
        # Three entries, well under BOUND_LEARNING=30, but over the byte
        # budget. The entry-count roll is not enough; archive must drop the
        # oldest entry until the live files fit.
        body = "# Learnings\n\n"
        for i in range(1, 4):
            body += (
                f"## L-{i:04d} | 2026-01-0{i} | believed | main\n\n"
                + ("z" * 9000)
                + "\n\n"
            )
        (self.dir / "learnings.md").write_text(body, encoding="utf-8")
        self.assertGreater(
            sum(p.stat().st_size for p in self.dir.glob("*.md")), 24576
        )
        check = self.run_memory("check")
        self.assertEqual(check.returncode, 3)
        archived = self.run_memory("archive")
        self.assertEqual(archived.returncode, 0, archived.stderr)
        live = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertLessEqual(
            sum(p.stat().st_size for p in self.dir.glob("*.md")), 24576
        )
        self.assertNotIn("L-0001", live, "the oldest entry should have rolled")
        self.assertIn("L-0003", live, "the newest entries stay live")
        self.assertTrue((self.dir / "archive" / "learnings-2026-01.md").is_file())

    def test_search_returns_the_entry_id(self):
        self.assertEqual(
            self.run_memory("add", "learning", "The lock is fd 9.").returncode, 0
        )
        found = self.run_memory("search", "lock is")
        self.assertEqual(found.returncode, 0, found.stderr)
        self.assertIn("L-0001", found.stdout)
        self.assertIn("The lock is fd 9.", found.stdout)

    def test_second_identical_add_is_recurrence(self):
        self.assertEqual(
            self.run_memory("add", "learning", "The lock is fd 9.").returncode, 0
        )
        again = self.run_memory("add", "learning", "The lock is fd 9.")
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertIn("already recorded as L-0001", again.stderr)
        self.assertIn("recurrence", again.stderr)
        read = self.run_memory("read", "learning")
        self.assertEqual(read.stdout.count("## L-"), 1)


if __name__ == "__main__":
    unittest.main()
