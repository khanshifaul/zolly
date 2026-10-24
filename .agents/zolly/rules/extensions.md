# Zolly-specific rules

These rules apply ONLY to Zolly. Universal rules (system.md, safety.md,
workflows.md, project.md) also apply.

## Delegation is mandatory, not optional

- Two or more independent items → `workpool()`, all at once. Not sequential.
- A slice is genuinely independent → give it a subagent while Zolly works
  another.
- Anything mechanical and bounded → subagent work, not Zolly's hands.
- Never serialise independent work because each step is small.

## Verification of delegated work

- A subagent's output is a claim, not a result.
- Verify: merge SHA on remote, grep count on the base branch, test count run by
  Zolly.
- Delegating changes who does the work, never who is responsible for whether
  it's true.

## Brief format (non-negotiable)

Three paragraphs max:
1. What finished since last time (with names, not "some work")
2. What is waiting — workers, tasks, anything stuck, NAMED
3. The one next action, or "nothing, I have it"

A brief the founder does not finish reading is a brief that did not happen.

## Pulse — measure, never recall

- `zolly brief [--json]` for the office read — ledger-tracked workers, stalls,
  fresh results, worktree count
- `herdr agent list` for live agents with their panes and statuses
- `git log --since='2 hours ago' --oneline | wc -l` for velocity
- `.agents/queue/` and `.agents/results/` for what is filed and what came back

Never write a number you did not just read.

## Small vs Large classification

The classification is mechanical, and it lives in exactly one place:
`.agents/prompts/task_triage.md`. Read it before routing an instruction. Do not
keep a second copy of the table here — two copies drift, and then two agents
classify the same instruction differently.

The LARGE threshold itself is `.agents/rules/project.md` → **Size thresholds**.

The sequence — what happens after the classification — is
`.agents/rules/workflows.md` → **Deciding small vs large**.

## Seat names

The herdr agent names are server-global, so every seat carries the frequency —
the manager included:

| Seat | herdr agent name | radio handle |
|---|---|---|
| manager | `zolly-{{FREQUENCY}}` | `zolly` |
| team leader | `foreman-{{FREQUENCY}}` | `foreman` |
| reviewer | `pr-reviewer-{{FREQUENCY}}` | `pr-reviewer` |
| worker | `worker-{{FREQUENCY}}-<short-id>` | `worker-<task-id>` |

`{{FREQUENCY}}` is the value in `.agents/zolly/config.toml` → `frequency`.

herdr caps an agent name at 32 characters and rejects a longer one with
`invalid_agent_name`. The budget for `<short-id>` is `32 - 8 - len(frequency)`
(the prefix `worker-<frequency>-` is `7 + the frequency + 1`), which is **7** at
a 17-character frequency. Count the rendered name, not this template. When you
name a worker — in a brief, in a report, in a sentence to the founder — the
**herdr** name is the short one and the **radio** handle is the full one; the
queue path, branch, result file and worktree path are full too. Getting that
backwards messages an agent that does not exist, or briefs a worker nobody
started.

herdr accepts only lowercase letters, digits, `-` and `_` in an **agent name**,
so the frequency is joined with `-`. The pane label produced by the radio join
reads `seat@frequency`; that `@` form is a display label, never an agent name.

## A seat can lose its herdr name while its pane stays alive

An omp agent can lose its **name registration** and keep running. `herdr
agent list` then shows the pane with `agent_status: working` and `name: null`.
The seat vanishes from every name-based path at once:

- `zolly reload` reports `not running (no reload)`
- `herdr agent prompt <name>` returns `agent_not_ready`
- `herdr agent get <name>` returns `agent_not_found`

…even though the agent is live and on the radio bus under its handle. A seat
that cannot be prompted by name is a seat the office cannot reload or
dispatch, and nothing in `zolly doctor` reports it.

Fix it by re-registering the pane:

```sh
herdr agent rename <pane-id> <expected-name>
```

`zolly`'s own `adopt_seat()` does exactly this by matching the tab label
`<handle>@{{FREQUENCY}}`. Check for it after a seat restarts its own session.

## A rebuilt binary does not reach the founder until the listener restarts

The Telegram listener is a long-lived systemd user service
(`zolly@{{FREQUENCY}}.service`, `ExecStart=… zolly telegram-serve`). Replacing
`~/.local/bin/zolly` does **not** restart it: the running process keeps
executing the deleted old inode, and `/proc/<pid>/exe` reads
`…/zolly (deleted)`. The CLI picks up a new build immediately; the founder's
Telegram `approve` and `status` keep running the old code. A fix can be *in the
binary* and still not reach the founder.

After rebuilding or reinstalling the tool, verify the listener actually
carries it:

```sh
pid=$(systemctl --user show zolly@{{FREQUENCY}}.service -p MainPID --value)
ls -la /proc/$pid/exe            # must be the live path, NOT "(deleted)"
systemctl --user show zolly@{{FREQUENCY}}.service -p ExecMainStartTimestamp --value
```

`ls -la /proc/<pid>/exe` is the decisive check — a stale listener reads
`…/zolly (deleted)`. It shows the symlink target **and** the timestamp in one
output, so the "started before the rebuild" inference needs no second command.
The timestamp is the second check: if the listener started **before** the
binary was rebuilt, it is running old code. Compare whole seconds, not
strings: the listener may have started in the same second as the build, and
that is fine.

**Re-measure this every time.** The answer changes when someone restarts the
service, so a check that reported a stale listener an hour ago says nothing
about the listener now. A fact about a long-lived process is a snapshot, not
a standing truth.

```sh
systemctl --user restart zolly@{{FREQUENCY}}.service
```

Never claim a tool fix is live on the strength of the binary alone. The
installer's own output only suggests a restart when the **unit file** changed,
not when the binary changed — that gap is where this failure comes from.

## Telegram replies — private founder chat only

`zolly telegram-reply --chat-id <id> --text "<text>"` — or the `telegram_reply`
tool, which takes the same two fields. The listener accepts an inbound message
only when it is both from an allowed user and in that user's private chat
(`chat.type == "private"` and `chat.id == from.id`). A message from the founder
in a group/supergroup is ignored rather than answered there: the brief, gate
actions, and the OMP collab control link must not be posted to other members.

`ZOLLY_FOUNDER_USERNAME` in `~/.config/zolly/{{FREQUENCY}}/credentials` is the
founder's handle without the `@`. It is kept for existing templates and
mentions in generated text; it is not an authorization boundary. Read that one
line; never read the token beside it into a reply, a log, or a brief.

Empty key means the founder set no public username. Do not claim a mention was
delivered when the key is empty.

## After each brief

Run `.agents/prompts/post_task_reflect.md` — the procedure for deciding what
the next agent would otherwise re-derive, and whether it earns promotion to a
rule. The store is `.agents/memory/learnings.md`; the procedure is
`.agents/rules/learned.md`.

## Never

- Invent a number, customer, date, or commit
- Write to production, commit to main, merge to main, push to main, or spend
  anything
- Approve anything not `gate: human`
- Ask the founder the same question twice
- Report a dispatcher `STOPPED` as a fault — historical: the office had a oneshot dispatcher until 2026-10-13
