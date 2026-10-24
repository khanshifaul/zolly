#!/usr/bin/env python3
"""Check the load-line register in `.agents/AGENTS.md`.

The register is the kit's claim that every rule, prompt and skill has a line
that loads it. This test enforces the mechanical half of that claim:

- every path named in the register exists;
- every file under `rules/`, `prompts/` and `zolly/skills/*/SKILL.md` has a row;
- no path is registered twice.

It cannot prove that a loader actually opens the file — the "Mechanism" column
is prose, not an executable hook. It does catch the common failure: a new rule
or skill that nobody registered, and a register row that points at a file that
has since moved.
"""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AGENTS = ROOT / ".agents"
REGISTER = AGENTS / "AGENTS.md"
MARKER = "## Load lines — every file here has one"


def register_paths():
    """The first backticked path on each table row under the load-line marker."""
    text = REGISTER.read_text(encoding="utf-8")
    if MARKER not in text:
        raise AssertionError(f"missing marker in {REGISTER}: {MARKER}")
    section = text.split(MARKER, 1)[1].split("\n## ", 1)[0]
    paths = []
    for line in section.splitlines():
        match = re.match(r"^\| `([^`]+)` \|", line)
        if not match:
            continue
        path = match.group(1)
        # `worker/<role>.md` is a pattern row, not a file.
        if "<" in path or ">" in path:
            continue
        paths.append(path)
    return paths


class LoadLineTests(unittest.TestCase):
    def test_every_registered_path_exists(self):
        missing = [path for path in register_paths() if not (AGENTS / path).exists()]
        self.assertEqual(missing, [], f"register points at missing files: {missing}")

    def test_every_rule_prompt_and_skill_is_registered(self):
        registered = set(register_paths())
        required = []
        for directory in ("rules", "prompts"):
            required.extend(sorted((AGENTS / directory).glob("*.md")))
        required.extend(sorted((AGENTS / "zolly" / "skills").glob("*/SKILL.md")))

        missing = [
            str(path.relative_to(AGENTS))
            for path in required
            if str(path.relative_to(AGENTS)) not in registered
        ]
        self.assertEqual(missing, [], f"unregistered files: {missing}")

    def test_no_duplicate_registered_paths(self):
        paths = register_paths()
        duplicates = sorted({path for path in paths if paths.count(path) > 1})
        self.assertEqual(duplicates, [], f"duplicate register rows: {duplicates}")


if __name__ == "__main__":
    unittest.main()
