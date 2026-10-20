#!/usr/bin/env python3
"""Tests for .agents/hr/hr.py — the approval gate and the hire receipt.

Run from the kit root:

    python3 -m unittest discover -s tests -v

The invariants under test are the ones the design claims, not the wording of
the persona:

- a request that is not `approved` produces no persona (HR cannot approve);
- an approved request produces both a persona and a `.hired` receipt;
- re-hiring without `--force` is a no-op, so a hand edit is not overwritten;
- the reviewer persona alone carries merge authority.
"""

import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location("hr", ROOT / ".agents" / "hr" / "hr.py")
hr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(hr)


class HrTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.hr_dir = self.root / ".agents" / "hr"
        self.hr_dir.mkdir(parents=True)
        self.worker_dir = self.root / ".agents" / "worker"

        self._saved = (hr.REPO, hr.HR, hr.PERSONAS)
        hr.REPO = self.root
        hr.HR = self.hr_dir
        hr.PERSONAS = self.worker_dir

    def tearDown(self):
        hr.REPO, hr.HR, hr.PERSONAS = self._saved
        self.tmp.cleanup()

    def file_request(self, role="researcher", status="pending", why="We need market research."):
        path = self.hr_dir / f"{role}.md"
        path.write_text(
            "---\n"
            f"title: {role.title()}\n"
            f"status: {status}\n"
            "reports_to: foreman\n"
            "---\n"
            "\n## Why this role\n\n"
            f"{why}\n",
            encoding="utf-8",
        )
        return path

    def hire(self, role="researcher"):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return hr.do_hire(role)

    def test_unapproved_request_produces_no_persona(self):
        self.file_request(status="pending")
        self.assertEqual(self.hire(), 1)
        self.assertFalse((self.worker_dir / "researcher.md").exists())
        self.assertFalse((self.hr_dir / "researcher.hired").exists())

    def test_approved_request_writes_persona_and_receipt(self):
        self.file_request(status="approved", why="We need a second pair of eyes.")
        self.assertEqual(self.hire(), 0)

        persona = self.worker_dir / "researcher.md"
        self.assertTrue(persona.is_file())
        text = persona.read_text(encoding="utf-8")
        self.assertIn("ROLE: researcher", text)
        self.assertIn("We need a second pair of eyes.", text)
        self.assertIn("You may not commit to main or push to main.", text)

        receipt = self.hr_dir / "researcher.hired"
        self.assertTrue(receipt.is_file())
        self.assertIn("persona: .agents/worker/researcher.md", receipt.read_text(encoding="utf-8"))

    def test_rehire_is_a_noop_without_force(self):
        self.file_request(status="approved")
        self.assertEqual(self.hire(), 0)
        persona = self.worker_dir / "researcher.md"
        persona.write_text("hand edit\n", encoding="utf-8")

        self.assertEqual(self.hire(), 0)
        self.assertEqual(persona.read_text(encoding="utf-8"), "hand edit\n")

    def test_deleted_persona_is_rewritten(self):
        # The receipt is the record that a hire happened; a missing persona is
        # recoverable from the request, so `hire` recreates it.
        self.file_request(status="approved")
        self.assertEqual(self.hire(), 0)
        persona = self.worker_dir / "researcher.md"
        persona.unlink()
        self.assertEqual(self.hire(), 0)
        self.assertTrue(persona.is_file())

    def test_dry_run_writes_nothing(self):
        self.file_request(status="approved")
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(hr.do_hire("researcher", dry_run=True), 0)
        self.assertFalse((self.worker_dir / "researcher.md").exists())
        self.assertFalse((self.hr_dir / "researcher.hired").exists())
        self.assertIn("would write", out.getvalue())

    def test_reviewer_persona_carries_the_merge_exception(self):
        self.file_request(role="reviewer", status="approved")
        self.assertEqual(self.hire("reviewer"), 0)
        text = (self.worker_dir / "reviewer.md").read_text(encoding="utf-8")
        self.assertIn("you merge approved PRs to main", text)
        self.assertNotIn("You may not commit to main or push to main.", text)


if __name__ == "__main__":
    unittest.main()
