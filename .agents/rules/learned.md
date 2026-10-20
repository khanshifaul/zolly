# rules/learned.md — self-improving rules

## This file is a procedure, not a store

The learned facts live in **`.agents/memory/learnings.md`**, owned by
`.agents/shared/memory`. That command enforces the bound (30 live entries,
overflow rolls into `.agents/memory/archive/`), writes atomically
(`tmp → sync → mv`, so a power cut never tears a file), and refuses any entry
matching a credential pattern.

**Do not write a second learning store here.** Two stores is the exact failure
`.agents/memory/README.md` documents: "Four overlapping memory files is the
problem, not the solution."

## How a learning becomes a rule

```
observe  →  .agents/shared/memory add learning "…"
   ↓
confirm  →  .agents/shared/memory confirm L-0007      # founder-verified
   ↓
promote  →  if it is now always true, move the rule into
            system.md (identity/bounds), project.md (this project's
            truths), workflows.md (a sequence),
            or safety.md (a rail). Then retract the entry:
            .agents/shared/memory retract L-0007 --promoted-to rules/<file>.md
            A retracted entry is a tombstone, not a deletion: the history stays.
```

A learning is **`believed` until confirmed**. A believed entry is a claim an
agent made, not a verified fact. Corrections never delete: a new entry
`Supersedes:` the old one, so the next agent can see the question was already
answered.

## What earns promotion to a rule

Promote when the learning is:

- **recurring** — it happened more than once, or
- **expensive** — one occurrence already cost real time or money.

Leave it as a learning when it is situational. A rules file that accumulates
every one-off stops being read, which defeats the purpose — the same reason
`OPEN-QUESTIONS.md` deletes answered entries instead of archiving them.

## Worked examples (real, from this office)

| Learning | Promoted to | Why |
|---|---|---|
| A status that says one thing while git says another is the same bug three times over | `system.md` law 2 | Recurring, and the cost was a founder making a decision from fiction |
| A dispatcher settles without filing a retry, so chains die at round 1 | `workflows.md` § Settling | Recurring: two tasks stalled terminally |
| A task marked `unverified` was actually on `main` | `workflows.md` § Working a task | Expensive: nearly caused a pile of stranded commits to be "recovered" |
| A linter version upgrade broke a one-line command | (kept as a learning) | Situational — one command, one workaround |

## The trap this file keeps falling into

A file in `.agents/` being committed **proves nothing about whether any session
reads it.** A goal file sat in git for a full session with a test scoring 10/10
on its vocabulary and no session-load path at all.

So: if you add a rule, **add the line that loads it**, and assert that line
resolves. The load-line register in `.agents/AGENTS.md` is that assertion made
visible — a shipped file with no row in that table is a file nobody opens.

The procedure that drives this file is `.agents/prompts/post_task_reflect.md` —
it is what an agent runs when it asks whether a learning has earned promotion.
Read it before you write anything into `learnings.md`.
