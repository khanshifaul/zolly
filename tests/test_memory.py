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

    def test_retract_twice_is_idempotent_and_does_not_grow(self):
        self.assertEqual(
            self.run_memory("add", "learning", "Retries need a filed r2.").returncode, 0
        )
        first = self.run_memory(
            "retract", "L-0001", "--promoted-to", ".agents/rules/workflows.md"
        )
        self.assertEqual(first.returncode, 0, first.stderr)
        before = (self.dir / "learnings.md").stat().st_size

        second = self.run_memory(
            "retract", "L-0001", "--promoted-to", ".agents/rules/workflows.md"
        )
        self.assertEqual(second.returncode, 0, second.stderr)

        after = (self.dir / "learnings.md").stat().st_size
        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertEqual(after, before, "a second retract must not grow the store")
        self.assertEqual(text.count("Promoted to: "), 1, "one pointer per entry")
        self.assertIn("already retracted in learnings.md", second.stdout)
        self.assertIn("unchanged", second.stdout)
        self.assertNotIn("retracted from", second.stdout)

        # retract must not create work for compact: no duplicate pointer to
        # reclaim. compact may still reclaim the body.
        compacted = self.run_memory("compact")
        self.assertEqual(compacted.returncode, 0, compacted.stderr)
        self.assertNotIn("duplicate", compacted.stdout)
        self.assertEqual(
            (self.dir / "learnings.md")
            .read_text(encoding="utf-8")
            .count("Promoted to: "),
            1,
        )

    def test_retract_reports_a_replaced_target(self):
        self.assertEqual(self.run_memory("add", "learning", "one").returncode, 0)
        self.assertEqual(
            self.run_memory(
                "retract", "L-0001", "--promoted-to", ".agents/rules/a.md"
            ).returncode,
            0,
        )
        result = self.run_memory(
            "retract", "L-0001", "--promoted-to", ".agents/rules/b.md"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertEqual(text.count("Promoted to: "), 1)
        self.assertIn("Promoted to: .agents/rules/b.md", text)
        self.assertNotIn(".agents/rules/a.md", text)
        self.assertIn("already retracted in learnings.md", result.stdout)
        self.assertIn(
            "promotion target replaced: .agents/rules/a.md -> .agents/rules/b.md",
            result.stdout,
        )

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

    def write_learnings(self, text):
        (self.dir / "learnings.md").write_text(text, encoding="utf-8")

    def test_compact_drops_a_retracted_body_and_keeps_its_header(self):
        self.write_learnings(
            "# Learnings\n\nPreamble stays.\n\n"
            "## L-0001 | 2026-01-01 | believed | main\n\nKeep this body.\n\n"
            "## L-0002 | 2026-01-02 | retracted | main\n"
            "Promoted to: .agents/rules/workflows.md\n\nDrop this body.\n\n"
        )
        before_text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        before = (self.dir / "learnings.md").stat().st_size

        result = self.run_memory("compact")
        self.assertEqual(result.returncode, 0, result.stderr)

        after = (self.dir / "learnings.md").stat().st_size
        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        # Preamble + believed entry must be byte-for-byte identical.
        self.assertEqual(
            before_text.split("## L-0002")[0], text.split("## L-0002")[0]
        )
        self.assertIn("Keep this body.", text)
        self.assertNotIn("Drop this body.", text)
        self.assertIn("## L-0002 | 2026-01-02 | retracted | main", text)
        self.assertIn("Promoted to: .agents/rules/workflows.md", text)
        self.assertIn(f"reclaimed {before - after}B", result.stdout)
        self.assertIn("kept 2 entries", result.stdout)

    def test_compact_keeps_a_retracted_entry_without_a_target(self):
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n\nNo home yet.\n\n"
        )
        result = self.run_memory("compact")
        self.assertEqual(result.returncode, 1)
        self.assertIn("LEFT IN PLACE", result.stderr)
        self.assertIn("L-0001", result.stderr)
        self.assertIn(
            "No home yet.", (self.dir / "learnings.md").read_text(encoding="utf-8")
        )

    def test_compact_is_idempotent(self):
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n"
            "Promoted to: .agents/rules/a.md\n\nbody\n\n"
        )
        first = self.run_memory("compact")
        self.assertEqual(first.returncode, 0, first.stderr)
        snapshot = (self.dir / "learnings.md").read_bytes()

        second = self.run_memory("compact")
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual((self.dir / "learnings.md").read_bytes(), snapshot)
        self.assertIn("nothing to compact", second.stdout)

    def test_compact_recovers_an_over_budget_store(self):
        # Under the entry-count bound (3 of 30) but over the byte budget.
        body = "# Learnings\n\n"
        body += "## L-0001 | 2026-01-01 | believed | main\n\n" + "z" * 9000 + "\n\n"
        body += (
            "## L-0002 | 2026-01-02 | retracted | main\n"
            "Promoted to: .agents/rules/a.md\n\n" + "y" * 9000 + "\n\n"
        )
        body += (
            "## L-0003 | 2026-01-03 | retracted | main\n"
            "Promoted to: .agents/rules/b.md\n\n" + "x" * 9000 + "\n\n"
        )
        self.write_learnings(body)
        self.assertEqual(self.run_memory("check").returncode, 3)

        result = self.run_memory("compact")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLessEqual(
            sum(p.stat().st_size for p in self.dir.glob("*.md")), 24576
        )
        self.assertEqual(self.run_memory("check").returncode, 0)
        # Compaction is not an archive roll: all three ids stay live.
        live = (self.dir / "learnings.md").read_text(encoding="utf-8")
        for n in (1, 2, 3):
            self.assertIn(f"## L-000{n} |", live)
        self.assertFalse((self.dir / "archive").exists())

    def test_compact_removes_a_duplicate_tombstone(self):
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n"
            "Promoted to: .agents/rules/a.md\n"
            "Promoted to: .agents/rules/a.md\n\nbody\n\n"
        )
        result = self.run_memory("compact")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertEqual(text.count("Promoted to: .agents/rules/a.md"), 1)
        self.assertIn("removed 1 duplicate", result.stdout)

    def test_compact_keeps_the_most_recent_of_two_targets(self):
        # retract inserts the newest pointer immediately after the header, so
        # the first line is the latest intent.
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n"
            "Promoted to: .agents/rules/new.md\n"
            "Promoted to: .agents/rules/old.md\n\nbody\n\n"
        )
        result = self.run_memory("compact")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertIn("Promoted to: .agents/rules/new.md", text)
        self.assertNotIn(".agents/rules/old.md", text)
        self.assertIn("discarded .agents/rules/old.md", result.stdout)

    def test_compact_only_named_ids(self):
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n"
            "Promoted to: .agents/rules/a.md\n\nBODY ONE\n\n"
            "## L-0002 | 2026-01-02 | retracted | main\n"
            "Promoted to: .agents/rules/b.md\n\nBODY TWO\n\n"
        )
        result = self.run_memory("compact", "L-0002")
        self.assertEqual(result.returncode, 0, result.stderr)
        text = (self.dir / "learnings.md").read_text(encoding="utf-8")
        self.assertIn("BODY ONE", text)
        self.assertNotIn("BODY TWO", text)

    def test_compact_unknown_id_fails_without_writing(self):
        self.write_learnings(
            "# L\n\n"
            "## L-0001 | 2026-01-01 | retracted | main\n"
            "Promoted to: .agents/rules/a.md\n\nbody\n\n"
        )
        snapshot = (self.dir / "learnings.md").read_bytes()
        result = self.run_memory("compact", "L-9999")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no entry L-9999", result.stderr)
        self.assertEqual((self.dir / "learnings.md").read_bytes(), snapshot)

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
