# Queue Item Contract

Canonical shape of `.agents/queue/<task-id>.md` — the brief the manager files
and the foreman dispatches from. Every queued task has one.

`close-the-day` and `workflows.md` both read the `status:` and `gate:` lines
from this file. Without a fixed shape, "the queue says the work exists" is not a
claim anyone can check.

## Template

```markdown
---
id: <task-id>
title: <short title>
role: generic            # or a role hr.py hired
gate: human | auto
status: queued | dispatched | blocked | done | failed | escalated
---

## Task

What must be true when this is done, in one or two sentences.

## Acceptance criteria

- <observable result 1>
- <observable result 2>

## Context

What the worker needs that is not obvious from the tree: the defect, the file,
the command that reproduces it, the decision behind it.

A brief is not private: it is the instruction that produces a result file that
may be published. Do not put personal facts in the context — health, family,
home, contacts, credentials — and do not instruct a worker to write one, even
as an example of what to avoid. Name the decision and the paths, and stop.
```

## Status vocabulary

| Value | Means |
|---|---|
| `queued` | filed, not dispatched |
| `dispatched` | a worker is running it |
| `blocked` | cannot proceed without a founder decision (`gate: human`) |
| `done` | the result file says `status: done` and the PR is merged |
| `failed` | the worker gave up; not finished |
| `escalated` | needs a decision or a retry; **not finished** |

`failed` and `escalated` are the opposite of done. They require either a retry
(`<task-id>-r2`) or a founder decision; never merge a branch because it has
commits. A reviewer OBJECTION must file the retry, or the chain dies at round 1.

## Who moves it

| Transition | Who | When |
|---|---|---|
| `queued` → `dispatched` | foreman | worktree opened, worker started |
| `dispatched` → `blocked` | worker | needs a founder decision; sets `gate: human` |
| `dispatched` → `done` | worker writes the result; reviewer merges | PR merged **and** the result says `done` |
| `dispatched` → `failed` / `escalated` | worker / foreman | gave up, or needs a decision |
| `failed` / `escalated` → `queued` | foreman | files `<task-id>-r2`; never reuses the attempt |
| any → `cancelled` | founder only | a named decision; an agent may not cancel |

A claim is not a completion. `dispatched` means one worker owns **that attempt**;
`done` requires both the result file and the merge. If a worker dies after
claiming, the foreman marks the attempt `failed` and files the retry — the task
is never silently picked up by a second worker.

## The gate

`gate: human` is the founder's. It is copied into the result file and must be
approved there (`approved: yes` immediately after the gate line, committed on
the task branch). An agent never passes it.

## Path

`.agents/queue/<task-id>.md`. Never `results/`, never `reports/queue/`.
