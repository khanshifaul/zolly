---
description: Post-task reflection — record what was learned and whether it earns promotion to a rule
---

You are finishing a unit of work. Before you report it done, reflect — then
write down only what will still matter to the next agent.

## Read first

`.agents/rules/learned.md`. It is the procedure; `.agents/memory/learnings.md`
is the store.

## The question

**What did I learn that the next agent would otherwise re-derive?**

Not a summary of what you did — `.agents/results/<id>.md` and the commit already
hold that. A summary is not a learning.

A learning is a **correction**: something was believed and turned out false, or
something is true in a way that is not obvious from reading the code.

## The bar

Record a learning only if it is **recurring** (happened more than once) or
**expensive** (one occurrence already cost real time or money). Otherwise it is
noise and the next agent will skip the file.

Before writing, ask the falsifiable form: *could I write a test that fails if
this were untrue?* If not, it is an opinion, not a learning.

## Write it

```sh
.agents/shared/memory add learning "<what was wrong, and what is actually true>"
```

The command enforces the bound, writes atomically, and refuses secrets.

The shared store is the only one. A per-agent note is a **second store** — two
stores is the failure this kit exists to prevent, not the fix:

```sh
.agents/shared/memory add learning "<…>"
```

That writes to `.agents/memory/learnings.md`. If you want a note scoped to one
agent, put the agent's name in the entry text; do not open a second file.

Then **confirm what the founder actually did**, not what you assume they felt.
Record behaviour, never a guess about mood. The voice traits the manager uses
live in `.agents/memory/personality.md`; a trait is only adopted when
there is consistent recorded evidence for it, and adopting one is the
founder's call.

## Promote or leave it

If the learning is now **always** true, it belongs in a rule, not in the log:

- identity or a bound → `system.md`
- a fact about this project → `project.md`
- a sequence of steps → `workflows.md`
- something no agent may do → `safety.md`

Promote it there, then retract it from `learnings.md`:

```sh
.agents/shared/memory retract L-0007 --promoted-to .agents/rules/<file>.md
```

Retraction is a tombstone, not a deletion: the entry stays visible as
`retracted` with the file it became, so the next agent can see the promotion
happened. Do not leave the rule in both places.

## Verify the loop

If you touched `.agents/memory/`:

```sh
.agents/shared/memory check      # bound + secret scan
```

A file being committed proves nothing about whether any session reads it. If you
added a rule, add the line that loads it — the register in `.agents/AGENTS.md` —
and assert that line resolves.
