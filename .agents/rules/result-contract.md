# Result File Contract

Canonical format for `.agents/results/<task-id>.md`. Every worker writes this.

## Template

```markdown
---
id: <task-id>
title: <short title>
status: done | failed
gate: human | auto
---

## Summary

<what was done, in 3-5 lines>

## Files changed

- `path/to/file1.md`
- `path/to/file2.csv`

## Verify

<commands run, with output>

## Commit

<commit SHA>

## Blockers

<what stopped this, or "none">
```

## Fields

| Field | Required | Notes |
|-------|----------|-------|
| `id` | yes | task ID |
| `title` | yes | short title |
| `status` | yes | `done` or `failed` |
| `gate` | yes | `human` = founder must review before merge; `auto` = safe to merge automatically |
| `## Summary` | yes | 3-5 lines |
| `## Files changed` | yes | list of paths |
| `## Verify` | yes | commands run + output. If no commands, say "none — file-only change" |
| `## Commit` | yes | commit SHA, or "not committed" |
| `## Blockers` | yes | what stopped this, or "none" |

## `## Verify` section rules

- List every command you ran, with its output
- If a test exists and you did not run it, say so
- If you cannot verify something, say so — do not claim it passes
- The reviewer reads this section. If it is missing, the reviewer OBJECTIONs.
- `zolly verify <task-id>` runs the project's `test-command` and rewrites this
  section with the command, exit code, timestamp and output tail. A non-zero
  exit is an OBJECTION; never overwrite it by hand.
- If `## Verify` claims tests it cannot reproduce, the reviewer OBJECTIONs.

## `gate` field

- `human` — the founder must review before merge (default for all workers)
- `auto` — safe to merge without founder review (only for docs, CSV updates, no-behavior changes)

**Both directions:**

- **(i) Never claim `auto` for a behaviour change.** A behaviour change that claims `auto` is the error the reviewer will object to. Use `human`.
- **(ii) Claim `auto` when the change is docs-content-only — ALL documentation-content-only changes are `auto`, including published and user-facing docs.** Do not default to `human` out of caution — a needless human gate parks the founder on work the contract already declared safe.

The reviewer never approves `gate: human`. That is the founder's call.

## `approved:` marker — where an approval lives

- **The single authoritative place:** `.agents/results/<task-id>.md` on the
  **task branch**, not the main checkout.
- **Exact line:** `approved: yes`
- **Position:** immediately AFTER the `gate: human` line in the frontmatter:

```
gate: human
approved: yes
```

- **When:** it must be **committed on the branch** BEFORE review is requested.
- **Not authoritative:** an approval that exists only in the main checkout copy of the result file does not satisfy the gate. The reviewer reads the branch copy; the main-checkout copy is invisible to it.

## Path

All workers write to `.agents/results/<task-id>.md`. Never `results/`, never
`reports/results/`.
