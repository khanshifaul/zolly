#!/usr/bin/env python3
"""
hr.py — HR. Turns an approved hiring request into a real worker persona.

The founder files a hiring request as `.agents/hr/<role>.md`. That request waits
on the founder's approval (`hr.py approve <role>`, or `approve` in Telegram). This
is the step that turns the yes into a file. Until it exists, a hire is paperwork.

    hr.py list                        # every role: on paper, and its state
    hr.py check                       # is a hire waiting to be processed?
    hr.py approve <role>              # the founder's yes — stamps the request
    hr.py hire <role>                 # write .agents/worker/<role>.md from .agents/hr/<role>.md
    hr.py hire --all                  # process every approved request
    hr.py hire --dry-run <role>       # print what would be written; change nothing

Two outputs, and the second matters more than it looks:

  .agents/worker/<role>.md   the persona. Governs SUBAGENT dispatch in omp.
  .agents/hr/<role>.hired   the receipt. Records that the persona was written,
                         and when. `hr.py list` reads this, so a role is only
                         ever "on staff" once this file exists.

Why the receipt: a persona file is not proof a hire happened. Without it, the
only way to know is to stat a file and hope — and a persona someone wrote by
hand once would look identical to one HR produced.

The kit ships starter personas (marked `<!-- kit starter persona:`) for common
roles. They are examples, not staff: an approved hire replaces the starter, and
a real hire no longer carries the marker, so later re-hires still leave it
alone.

## The reviewer seat is hand-authored

The reviewer is the one seat with a hand-written persona
(`.agents/reviewer/agent.md`), because it carries merge authority — the one
authority the office grants a worker, and one that does not go through HR. Every
other role is a worker, and a worker gets its persona from `hr.py hire <role>`.


## Never

- Write a persona for a request that is not `status: approved`. Approving is the
  founder's.
- Invent a role nobody asked for.
- Grant a worker authority over money, production, public publishing, or a
  customer. Those stay `gate: human` at every level.

Persona format and the identity-block contract: `.agents/hr/rules/extensions.md`.
"""

from __future__ import annotations

import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
# The hire requests and receipts live in the agent layer; the personas they
# produce live in `.agents/worker/`. One copy on disk — the drift that a skills
# symlink allowed.
HR = REPO / ".agents" / "hr"
PERSONAS = REPO / ".agents" / "worker"

# The kit ships starter personas for roles nobody has hired yet. This marker
# separates "replaceable example" from "hand-edited persona": a real hire does
# not carry it, so re-hiring without --force still leaves that file alone.
STARTER_MARKER = "<!-- kit starter persona:"

# The persona frontmatter. omp reads `name` and `description` when it lists
# agents; everything else is body prose the agent reads on invocation.
PERSONA_DESCRIPTION = {
    # A role the office does not ship a persona for falls back to a description
    # built from the hire request itself, so any role works without a code
    # change. Only the roles with a hand-written persona in the kit are listed
    # here.
    "reviewer": (
        "Reviews a finished task's result against the task it was given and the "
        "diff it produced. The only path to main: merges the approved PR and may "
        "push to main (founder decision, 2026-10-12); no other agent does. "
        "Never edits files or writes result files. Returns PASS or a specific "
        "objection."
    ),
}


def now() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def read_request(path: Path) -> dict:
    """Parse a .agents/hr/<role>.md request. Returns {} when it is not one."""
    if not path.is_file():
        return {}
    text = path.read_text(encoding="utf-8", errors="replace")
    fm: dict[str, str] = {}
    if text.startswith("---"):
        block = text.split("---", 2)
        if len(block) >= 3:
            for line in block[1].splitlines():
                if ":" in line:
                    k, v = line.split(":", 1)
                    fm[k.strip()] = v.strip()
    return fm


def requests() -> list[tuple[str, Path, dict]]:
    """Every hiring request on disk, as (role, path, frontmatter)."""
    if not HR.is_dir():
        return []
    out = []
    for p in sorted(HR.glob("*.md")):
        out.append((p.stem, p, read_request(p)))
    return out


def is_hired(role: str) -> bool:
    return (HR / f"{role}.hired").is_file()


def persona_path(role: str) -> Path:
    return PERSONAS / f"{role}.md"


def why_block(req: dict, path: Path) -> str:
    """The request's stated reason, for the persona body.

    This is the point of the whole loop: a worker whose persona says why it
    exists reads the same justification the founder approved.
    """
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    m = re.search(
        r"^##\s*Why .*?\n(.*?)(?=^##\s|\Z)", text, re.M | re.S
    )
    return m.group(1).strip() if m else ""



# What a worker writes when it finishes. Not cosmetic: settling marks a
# task settled by results/<id>.md existing, so a worker told to write elsewhere
# leaves the task unsettled, and a worker told to write results/ when it must
# not settles the review with its own objection. The reviewer gets the opposite
# rule, stated where it will read it.
DEFAULT_REPORTING = [
    "Write your outcome to `.agents/results/<task-id>.md` with a `## Verify` section",
    "naming the command you ran and what it printed. A result with no",
    "verification is treated as a failure when the task is settled.",
]

PERSONA_REPORTING = {
    "reviewer": [
        "**You write no files at all.** Not `.agents/results/`, not `queue/`, not source.",
        "",
        "The reviewer harness runs you and parses what you print. Your entire output",
        "is one line, exactly one of:",
        "",
        "    PASS: merged <commit-sha> — <pr-url>",
        "",
        "or an objection that names what is wrong and where:",
        "",
        "    OBJECTION: <what is wrong> — <file:line, or the specific claim>",
        "",
        "Then stop. Editing files, committing directly to main, and pushing a",
        "feature branch are forbidden to you; writing any file that marks the task",
        "settled would defeat the review. Merging the approved PR into main — and",
        "pushing to main — is yours alone (founder decision, 2026-10-12).",
    ],
}

def render_persona(role: str, req: dict, req_path: Path) -> str:
    """Build the persona file body.

    Deliberately a template, not a model. HR's output here is a small, checkable
    document; letting a model write it would make "what does this worker do"
    unanswerable by reading the repo.
    """
    desc = PERSONA_DESCRIPTION.get(
        role,
        f"The {role} worker for this project. Works tasks whose frontmatter names "
        f"role: {role}.",
    )
    reports_to = req.get("reports_to", "foreman")
    title = req.get("title", role)
    why = why_block(req, req_path)
    return "\n".join(
        [
            "---",
            f"name: {role}",
            f"description: {desc}",
            "---",
            "",
            f"# {title}",
            "",
            f"ROLE: {role}",
            "",
            f"You report to {reports_to}. You were hired to do one job: "
            f"{title}.",
            "",
        ]
        + ([f"## Why you exist\n\n{why}\n"] if why else [])
        + [
            "## Guardrails",
            "",
            "- You may not spend money, touch production, publish anything publicly,",
            "  or contact a customer. Those are `gate: human` at every level.",
            *(
                [
                    "- You are the only path to main: you merge approved PRs to main",
                    "  and may push to main (founder decision, 2026-10-12).",
                    "  No other agent merges or pushes to main.",
                ]
                if role == "reviewer"
                else [
                    "- You may not commit to main or push to main. Your work is reviewed first."
                ]
            ),
            "- You may not invent a number, a customer, a date, or a commit.",
            "- If you cannot finish, say what is missing. A blocker you report is",
            "  worth more than a result you cannot defend.",
            "",
            "## Reporting",
            "",
            *PERSONA_REPORTING.get(role, DEFAULT_REPORTING),
        ]
    )


def do_hire(role: str, *, force: bool = False, dry_run: bool = False) -> int:
    req_path = HR / f"{role}.md"
    if not req_path.is_file():
        print(f"hr: no hiring request at {req_path}", file=sys.stderr)
        return 1
    req = read_request(req_path)
    status = req.get("status", "")
    if status != "approved" and not force:
        # The whole point of the gate. HR does not get to approve its own hire.
        print(
            f"hr: {role} is '{status}', not 'approved'. Approval is the founder's —\n"
            f"    run 'hr.py approve {role}' or send 'approve' in Telegram.",
            file=sys.stderr,
        )
        return 1

    PERSONAS.mkdir(parents=True, exist_ok=True)
    target = persona_path(role)
    if target.exists() and not force:
        existing = target.read_text(encoding="utf-8")
        if STARTER_MARKER not in existing:
            print(f"hr: {target} already exists — {role} is already on staff")
            return 0
        print(f"hr: replacing the starter persona at {target.relative_to(REPO)}")

    if dry_run:
        print(f"hr: would write {target.relative_to(REPO)}")
        print(f"hr: would write receipt .agents/hr/{role}.hired")
        return 0

    target.write_text(render_persona(role, req, req_path), encoding="utf-8")

    # The receipt. A persona alone is not proof a hire was processed.
    (HR / f"{role}.hired").write_text(
        "\n".join(
            [
                f"# hired: {role}",
                "",
                f"persona: {target.relative_to(REPO)}",
                f"requested: {req.get('requested', 'unknown')}",
                f"approved: {req.get('approved', 'unknown')}",
                f"processed: {now()}",
                "",
                "Written by .agents/hr/hr.py from .agents/hr/"
                f"{role}.md. Format: .agents/hr/rules/extensions.md",
                "",
            ]
        ),
        encoding="utf-8",
    )
    print(f"hr: hired {role} -> {target.relative_to(REPO)}")
    print(f"hr: receipt  -> .agents/hr/{role}.hired")
    print(f"hr: it staffs on its own now — foreman dispatches tasks naming "
          f"'role: {role}'.")
    return 0


def cmd_list(_args: list[str]) -> int:
    reqs = requests()
    if not reqs:
        print("hr: no hiring requests filed. Create .agents/hr/<role>.md to file one.")
        return 0
    print(f"{'role':<18} {'request':<10} {'persona':<10} on staff")
    for role, path, fm in reqs:
        req_state = fm.get("status", "?")
        persona_state = "yes" if persona_path(role).is_file() else "-"
        staff = "yes" if is_hired(role) else "no"
        if req_state == "approved" and staff == "no":
            staff = "PENDING — run: hr.py hire " + role
        print(f"{role:<18} {req_state:<10} {persona_state:<10} {staff}")
    return 0


def cmd_approve(args: list[str]) -> int:
    """Set a hire request status to approved."""
    if not args:
        print("hr: approve needs a role", file=sys.stderr)
        return 1
    role = args[0]
    req_path = HR / f"{role}.md"
    if not req_path.is_file():
        print(f"hr: no hiring request at {req_path}", file=sys.stderr)
        return 1
    text = req_path.read_text(encoding="utf-8", errors="replace")
    if not text.startswith("---"):
        print(f"hr: {req_path} has no frontmatter", file=sys.stderr)
        return 1
    lines = text.splitlines()
    in_fm = False
    fm_end = 0
    for i, line in enumerate(lines):
        if line == "---":
            if not in_fm:
                in_fm = True
            else:
                fm_end = i
                break
    if fm_end == 0:
        print(f"hr: malformed frontmatter in {req_path}", file=sys.stderr)
        return 1
    # Update status and add approved timestamp
    new_lines = []
    for i, line in enumerate(lines):
        if i <= fm_end and line.startswith("status:"):
            new_lines.append("status: approved")
        elif i <= fm_end and line.startswith("approved:"):
            # already has approved, update it
            new_lines.append(f"approved: {now()}")
        elif i == fm_end - 1 and not any(l.startswith("approved:") for l in lines[:fm_end]):
            # no approved line, add it before closing ---
            new_lines.append(f"approved: {now()}")
            new_lines.append(line)
        else:
            new_lines.append(line)
    req_path.write_text("\n".join(new_lines), encoding="utf-8")
    print(f"hr: {role} approved at {now()}")
    return 0

def cmd_check(_args: list[str]) -> int:
    """Anything needing the founder, or an approved hire to process.

    Exit 1 if either. Two states, deliberately distinguished in the output:
    `pending` means they still have a decision to make; `approved` means they made it
    and HR has not done the paperwork yet, which is a stall on our side, not theirs.
    Reporting only the second was how a request sat unactioned while every check
    said the queue was clear.
    """
    waiting_founder = [
        role for role, _, fm in requests()
        if fm.get("status") == "pending"
    ]
    ready = [
        role for role, _, fm in requests()
        if fm.get("status") == "approved" and not is_hired(role)
    ]
    if waiting_founder:
        print("hr: awaiting YOUR approval: " + ", ".join(waiting_founder)
              + "  → 'hr.py approve <role>'")
    if ready:
        print("hr: approved, not yet staffed: " + ", ".join(ready)
              + "  → 'hr.py hire <role>'")
    if waiting_founder or ready:
        return 1
    print("hr: nothing waiting.")
    return 0


def cmd_hire(args: list[str]) -> int:
    dry_run = "--dry-run" in args
    force = "--force" in args
    if "--all" in args:
        todo = [
            role for role, _, fm in requests()
            if fm.get("status") == "approved" and not is_hired(role)
        ]
        if not todo:
            print("hr: nothing to process.")
            return 0
        rc = 0
        for role in todo:
            rc |= do_hire(role, force=force, dry_run=dry_run)
        return rc
    roles = [a for a in args if not a.startswith("-")]
    if len(roles) != 1:
        print('usage: hr.py hire <role> | hr.py hire --all [--force] [--dry-run]', file=sys.stderr)
        return 1
    return do_hire(roles[0], force=force, dry_run=dry_run)


def main(argv: list[str]) -> int:
    handlers = {
        "list": cmd_list, "check": cmd_check, "hire": cmd_hire, "approve": cmd_approve,
    }
    if len(argv) < 2 or argv[1] in {"-h", "--help"}:
        print(__doc__)
        return 0
    verb = argv[1]
    if verb not in handlers:
        print(f"hr: unknown verb {verb!r}. Try: {', '.join(handlers)}", file=sys.stderr)
        return 2
    return handlers[verb](argv[2:])


if __name__ == "__main__":
    sys.exit(main(sys.argv))