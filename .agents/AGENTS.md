# `.agents/` — the agent layer

Everything an agent *is*: its rules, its memory, its prompts, and the skills it
runs. One layer, one place.

The harness is a different thing: `.omp/` holds tool commands and persona files —
the programs agents run inside. It is not the agent layer.

This tree is **the template**. `install.sh <target-dir>` materialises it into
any git project. Everything here is project-independent; everything
project-specific lives in `.agents/rules/project.md` and
`.agents/memory/founder.md`, which the installer fills in.

## Layout

```
.agents/
├── AGENTS.md              # this file — the entry point
├── rules/
│   ├── system.md          # identity, bounds, definitions — immutable
│   ├── project.md         # THIS project's truths: architecture, layout, thresholds
│   ├── learned.md         # how a learning becomes a rule (store: memory/learnings.md)
│   ├── workflows.md       # SOPs: shift, hand-off, triage, settle, merge
│   ├── safety.md          # destructive commands, secrets, external systems, gates
│   ├── csv-conventions.md # how CSVs are written and handed over here
│   ├── result-contract.md # what a results/<task>.md has to contain
│   └── queue-contract.md  # what a queue/<task>.md brief has to contain
├── prompts/
│   ├── task_triage.md     # small / large / gate — decide, then act
│   └── post_task_reflect.md   # what did I learn that the next agent would re-derive?
├── memory/
│   ├── README.md          # the split, and why (read this first)
│   ├── founder.md         # who the founder is, and how to address them
│   ├── decisions.md       # owned by shared/memory
│   ├── learnings.md       # owned by shared/memory
│   ├── current.md         # owned by shared/memory
│   ├── goal.md            # the office's goal — loaded at session start
│   ├── personality.md     # Zolly's voice traits
│   ├── entity-store.schema.md  # the shape of the optional entity index
│   └── archive/           # rolled-off memory (.gitkeep keeps the dir in git)
├── worker/
│   ├── generic.md         # the default worker persona
│   └── <role>.md          # starter personas (examples; a hire replaces them)
├── shared/
│   └── memory             # the bounded, atomic, secret-refusing memory CLI
├── hr/
│   ├── hr.py              # hire request -> worker persona + receipt
│   └── rules/extensions.md
├── zolly/
│   ├── agent.md           # the manager persona
│   ├── config.toml        # frequency, founder, base branch, remote
│   ├── rules/extensions.md
│   ├── mcp/
│   │   ├── zolly_mcp.py   # the MCP server that reaches Zolly as a tool
│   │   ├── test_mcp.py    # a stdio client: handshake, status, say
│   │   ├── install.sh     # deploys it to ~/.local/bin/zolly-mcp
│   │   └── README.md      # the mcp.json entry to register
│   └── skills/
│       ├── close-the-day/SKILL.md
│       └── recap/SKILL.md
├── foreman/
│   ├── agent.md           # the team leader persona
│   ├── rules/extensions.md
│   └── boot/
│       ├── foreman.md     # the opening prompt for the foreman seat
│       └── pr-reviewer.md # the opening prompt for the reviewer seat
└── reviewer/
    ├── agent.md           # the reviewer persona — the only door to main
    └── rules/extensions.md
```

Depth matters: every agent script resolves the repo root as **two levels up**
(shell `cd "$HERE/../.."`, python `parents[2]`). A file at
`.agents/<dir>/<file>` gets the repo root. That is why `shared/memory` (at
`.agents/shared/memory`) and `hr/hr.py` (at `.agents/hr/hr.py`) both resolve
the root the same way.

### Skills are real files here, never symlinks

`.agents/zolly/skills/<name>/SKILL.md` is a **real file** in this tree. A
symlinked skill is a known hazard: two copies of the same skill drift apart and
only one is tracked, so a fresh clone loses it. One copy, on disk, tracked.

## Read order for any agent

1. `.agents/rules/system.md` — who you are and what you may not do
2. `.agents/rules/safety.md` — the rails
3. `.agents/rules/workflows.md` — how work actually moves here
4. `.agents/rules/project.md` — what is true about *this* project
5. `.agents/memory/README.md` — where memory lives and why

Only then: `.agents/prompts/` for the specific job, and the persona's own
`rules/extensions.md`.

## Instruction precedence

When two loaded files disagree, the earlier row wins:

1. `rules/system.md` — identity and bounds; immutable
2. `rules/safety.md` — the rails
3. `rules/workflows.md` — the sequences
4. `rules/project.md` — this project's truths and thresholds
5. the seat's `agent.md`
6. the seat's `rules/extensions.md`
7. the prompt or skill currently running

`rules/learned.md` is a procedure, not a policy: it never overrides anything
above it. A prompt that contradicts a rule is a bug in the prompt — fix it
there, not by adding an exception to the rule.

## What a file is called, and why

Three kinds of file, three naming rules:

| Kind | Name | Example |
|---|---|---|
| A **seat** — a long-lived pane that a boot prompt starts | `agent.md` | `foreman/agent.md` |
| A **worker role** — a persona HR renders per hire | `<role>.md` | `worker/generic.md` |
| A **tool** — a program an agent runs | its own filename | `hr/hr.py`, `shared/memory` |

A seat gets `agent.md` because there is exactly one seat per directory and only
the boot prompt varies. A worker gets `<role>.md` because many roles share one
directory (`generic.md`, the starter personas, then whatever `hr.py hire`
writes). `hr/` has no `agent.md` on purpose: HR is a tool, not a seat — nothing
boots it. `shared/` holds tools no single agent owns; its one file is the memory
CLI, which is why it is executable and has no `.sh` suffix — agents call it by
name.

The kit ships starter personas for common sales and marketing roles. They are
examples, not staff: `.agents/hr/<role>.md` plus the `.hired` receipt is still
what puts a role on staff, and `hr.py hire` replaces the starter with the
project-specific version. A starter carries
`<!-- kit starter persona: … -->`; a hired persona does not, so later re-hires
leave hand edits alone.

## Load lines — every file here has one

**A committed file proves nothing about being read.** If you add a rule, add the
line that loads it and assert that line resolves. The table is the load-line
register; a file with no row is a file nobody opens.

| File | Loaded by | Mechanism |
|---|---|---|
| `rules/system.md` | every agent | named in each persona's rule block |
| `rules/safety.md` | every agent | named in each persona's rule block |
| `rules/workflows.md` | every agent | named in each persona's rule block |
| `rules/project.md` | every agent | named in `rules/workflows.md` and `rules/system.md` |
| `rules/learned.md` | any agent promoting a learning | named in `prompts/post_task_reflect.md` |
| `rules/csv-conventions.md` | any worker writing a CSV | named in `rules/workflows.md` and `worker/generic.md` |
| `rules/result-contract.md` | every worker and reviewer | named in `rules/workflows.md`, `worker/generic.md`, `reviewer/agent.md` |
| `rules/queue-contract.md` | the manager and the foreman | named in `rules/workflows.md`, the `zolly` and `foreman` personas, and `zolly/skills/close-the-day/SKILL.md` |
| `prompts/task_triage.md` | the manager, on each instruction | named in `zolly/agent.md` |
| `prompts/post_task_reflect.md` | any agent closing a unit of work | named in `zolly/rules/extensions.md` and `worker/generic.md` |
| `prompts/curiosity.md` | the manager, when the queue is empty | named in `rules/workflows.md` and `zolly/agent.md` |
| `memory/README.md` | every agent | step 5 of the read order above |
| `memory/founder.md` | the manager | named in `zolly/agent.md` |
| `memory/decisions.md` | every agent | `shared/memory read decision` |
| `memory/learnings.md` | every agent | `shared/memory read learning` |
| `memory/current.md` | every agent | `shared/memory read current` |
| `memory/goal.md` | the manager, at session start | the `@` token below |
| `memory/personality.md` | the manager | named in `zolly/agent.md` and `memory/goal.md` |
| `memory/entity-store.schema.md` | whoever builds an entity index | named in `memory/README.md` |
| `memory/archive/.gitkeep` | git | keeps `memory/archive/` in the tree so the overflow directory exists on a fresh clone |
| `shared/memory` | every agent that remembers | named in `rules/learned.md` and `memory/README.md` |
| `hr/hr.py` | HR, and anyone filing a hire | self-describing `--help`; the command block in `hr/rules/extensions.md` |
| `hr/rules/extensions.md` | whoever runs HR | named in every `.hired` receipt `hr.py` writes |
| `zolly/agent.md` | the manager seat | loaded by `zolly init` |
| `zolly/rules/extensions.md` | the manager | named in `zolly/agent.md` |
| `zolly/config.toml` | the zolly binary | read at startup for the frequency, founder, base branch and remote |
| `zolly/skills/close-the-day/SKILL.md` | the manager, on a day close | named in `zolly/agent.md` |
| `zolly/skills/recap/SKILL.md` | the manager, on a cold start | named in `zolly/agent.md` |
| `zolly/mcp/zolly_mcp.py` | any MCP client | deployed by the installer to `~/.local/bin/zolly-mcp`; registered in `~/.omp/agent/mcp.json` |
| `zolly/mcp/test_mcp.py` | whoever verifies the install | run by hand: `python3 zolly/mcp/test_mcp.py handshake`; named in `zolly/mcp/README.md` |
| `zolly/mcp/install.sh` | the installer | deploys `zolly_mcp.py` to `~/.local/bin/zolly-mcp`; invoked by the root `install.sh` |
| `zolly/mcp/README.md` | whoever registers the server | names the `mcp.json` entry |
| `foreman/agent.md` | the foreman seat | loaded by `zolly init` |
| `foreman/rules/extensions.md` | the foreman | named in `foreman/agent.md` |
| `foreman/boot/foreman.md` | the foreman seat | the opening prompt at seat start; named in `foreman/agent.md` |
| `foreman/boot/pr-reviewer.md` | the reviewer seat | the opening prompt at seat start; named in `reviewer/agent.md` |
| `reviewer/agent.md` | the reviewer seat | loaded by `zolly init` |
| `reviewer/rules/extensions.md` | the reviewer | named in `reviewer/agent.md` |
| `worker/generic.md` | a worker seat | named in `foreman/agent.md` when a worker is spawned |
| `worker/<role>.md` | a worker with a named role | `foreman/agent.md`: "if the brief names a role, load that persona instead" |

### The boot prompts

`foreman/boot/foreman.md` and `foreman/boot/pr-reviewer.md` are the **opening
prompts** for the two non-manager seats. They are short, and they exist so the
seat's *first* turn says the right thing:

- the boot prompt names the seat, says who it reports to, and points at the
  full persona (`.agents/foreman/agent.md` / `.agents/reviewer/agent.md`);
- the persona holds the verbs, the rails, and the output contract.

Both live under `foreman/boot/` for historical reasons. The reviewer's is **not
foreman-owned**: `zolly init` delivers both. `pr-reviewer.md` stays at this path
because the installed binary and `reviewer/agent.md` reference it — moving it
means updating both.

`zolly init` delivers the boot prompt when the seat is created, then the
persona. A seat booted on the persona alone starts warm; a seat booted on the
boot prompt alone has no rules. Both, in that order.

### The goal is injected, not opened

The installed repo's **root** context file carries this token — omp expands
`@path` inside a context file before injecting it:

```
@.agents/memory/goal.md
```

If that token is not in the root context file, the goal is a comment, not
knowledge. A record no process consults is a comment.

## The rules that are load-bearing

- **`system.md` is immutable.** It changes when the founder says so, not when an
  agent finds it inconvenient.
- **`project.md` is the project's own.** Everything that used to be hardcoded
  about one codebase lives there now, so `system.md` stays true everywhere.
- **`learned.md` holds no learnings.** It is the *procedure*. The store is
  `.agents/memory/learnings.md`, owned by `.agents/shared/memory` (bounded,
  atomic, secret-refusing). Two stores is the failure, not the fix.
- **A committed file proves nothing about being read.** If you add a rule, add
  the line that loads it and assert that line resolves.

## Adding to this directory

| You want to add | Put it here | Not here |
|---|---|---|
| an identity or a hard bound | `rules/system.md` | a new file |
| a fact about *this* project | `rules/project.md` | `rules/system.md` |
| a sequence of steps | `rules/workflows.md` | a skill |
| something no agent may do | `rules/safety.md` | `AGENTS.md` at root |
| a template sub-prompt | `prompts/` | `.omp/commands/` unless it is a *harness* command |
| a fact learned | `.agents/memory/learnings.md` via `.agents/shared/memory` | `rules/learned.md` |
| a founder decision | `OPEN-QUESTIONS.md`, deleted when answered | anywhere here |

## Runtime state is not memory

`queue/`, `runs/`, `inbox/`, `worktrees/`, `results/`, `days/`, `trash/`,
`events/` and `nudge.stamp` are machine-local and deliberately uncommitted —
see the table in `.agents/memory/README.md`. Committing them is churn; losing
`.agents/memory/` is losing the only thing that survives a fresh clone.
