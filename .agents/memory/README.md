# Agent memory

Durable memory that survives a session — and a machine switch.

## This directory is committed. Its siblings are not.

That looks inconsistent. It is deliberate, and it is the whole point.

| Path | In git? | Why |
|---|---|---|
| `.agents/memory/` | **yes** | Knowledge that must outlive the machine. It is the backup. |
| `.agents/results/` | no on `main` | Task outcomes. Committed on the **task branch** so the reviewer can read the branch copy; never committed to `main`. |
| `.agents/queue/` | no | Task briefs the foreman dispatches from. |
| `.agents/worktrees/` | no | Git worktrees, already managed by git. |
| `.agents/nudge.stamp` | no | Idempotence stamp for the Telegram nudge. |
| `.agents/memory/.work/`, `.lock` | no | Transient write artefacts. The live file is only ever replaced by an atomic rename. |

**If you were about to add `.agents/` wholesale to `.gitignore`, don't.** That
would delete the only thing in the repo that carries memory across a fresh
clone. The split is per-file and deliberate; see the table above.

## Not everything here is read by `memory`

`decisions.md`, `learnings.md` and `current.md` are the three files `memory` owns.
The rest of this directory is read some other way, and how matters more than it
looks:

| File | Read by | Mechanism |
|---|---|---|
| `goal.md` | the session harness, at session start | the root context file carries `@.agents/memory/goal.md`; `@` tokens inside a context file are expanded before injection |
| `personality.md` | the manager, when it renders a report | named in `.agents/zolly/agent.md` |
| `founder.md` | the manager | named in `.agents/zolly/agent.md` |
| `entity-store.schema.md` | whoever builds an entity index | the shape, not the index |

The trap this directory keeps falling into: **a file in here is committed, and
that proves nothing about whether any session reads it.** A goal file sat in git
for a full session with a test scoring 10/10 on its vocabulary, and no
session-load path at all — so the rule changed nothing. If you add a file here,
add the line that loads it (the register in `.agents/AGENTS.md`), and assert
that line resolves.

## Usage

```sh
.agents/shared/memory add decision "…"
.agents/shared/memory add learning  "…"
.agents/shared/memory add current   "…"
.agents/shared/memory add decision  "…" --supersedes D-0002
.agents/shared/memory confirm D-0002      # founder-verified
.agents/shared/memory retract L-0007 --promoted-to .agents/rules/workflows.md
.agents/shared/memory read                # all three live files, one shot
.agents/shared/memory read archive        # the rolled-off history
.agents/shared/memory search "telegram"   # live + archive, with entry ids
.agents/shared/memory check               # bound + secret scan
```

A second **identical** `add` is recurrence, not a new fact: it writes nothing,
prints the existing id, and points at the promotion command. Promote it into
`.agents/rules/` instead of duplicating it.

Scope an entry to one agent by naming the agent in the text — the store is
shared:

```sh
.agents/shared/memory add learning "Zolly: <what was learned>"
```

The CLI also has an `--agent <name>` flag, which writes to
`.agents/<name>/memory/learnings.md` instead. **Do not use it here.** It opens
a second store — and four overlapping memory files is the problem this file
exists to solve, not the solution.

Every write is `tmp -> sync -> mv`, so a power cut leaves the old file or the new
one, never a torn one. The live files are bounded (20 decisions, 30 learnings,
5 current; 24576 bytes total) and overflow rolls into
`archive/<file>-<YYYY-MM>.md` automatically on every write.

## How this differs from the other memory files

Four overlapping memory files is the problem, not the solution. The split is by
**question**, and by load cost:

| File | Question | Fate |
|---|---|---|
| `HANDOFF.md` | *What happened, chronologically?* | permanent log; read by search, never loaded in full |
| `.agents/queue/` | *What is the office working on right now?* | machine-written, dies with the machine by design |
| `OPEN-QUESTIONS.md` | *What can only the founder decide?* | **deleted** when answered |
| **`.agents/memory/`** | *What do I already know that I would otherwise re-derive?* | bounded, committed, read in one shot |

Rule that keeps this from rotting: **a decision that needs the founder goes in
`OPEN-QUESTIONS.md` and is *not* also written here.** One register, one place.

## Trust

Every entry is `believed` until `memory confirm <id>` is run. A believed entry
is a claim an agent made, not a verified fact. Corrections never delete — a new
entry `Supersedes:` the old one, so the next agent can see the question was
already answered.

## Never commit a secret

A secret here is in the permanent git history forever. `memory` **refuses** the
write (exit 2) if the text matches a credential pattern. Record *where* a secret
lives, never its value:

```
good: The API signing secret lives in config/secrets.yaml under API_SIGNING_SECRET.
bad:  The API signing secret is [____]        (refused, exit 2)
```

The scan is a shape check, not a proof. `AGENTS.md` remains the real control.
