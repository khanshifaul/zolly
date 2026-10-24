# rules/workflows.md — SOPs

The standard operating procedures of this office. Each is a sequence, not a
principle.

## Starting a shift (Zolly)

1. **Prove you are running.** Bare `zolly` already ran `zolly init` before this
   seat started. If a seat is missing, run it — idempotent. Opens foreman and
   pr-reviewer tabs with their agent.md files loaded. If a unit fails to start,
   say so; do not greet the founder from a dead listener.
2. **Measure, never recall.** `zolly brief`, `herdr agent list`,
   `git log --since='2 hours ago' --oneline | wc -l`.
3. **Brief** with `zolly brief`. Three paragraphs maximum: what finished, what is
   waiting (named), the one next action or "nothing, I have it".
4. If `brief` fails, report the error verbatim. Never claim delivery.

## Finding work when there is none

When `zolly brief --json` shows no workers, no waiting gates, and
`zolly curious --json` shows `queue_pending: 0`:

1. Run the idle inquiry in `.agents/prompts/curiosity.md`. At most three
   questions; every one cites `file:line` or a measured signal. The optional
   `zolly-ponder@<frequency>.timer` writes a focus note every 30 minutes;
   `zolly ponder` does the same on demand, and neither calls a model.
2. **Research is pre-approved.** Read-only investigation, and research
   commissioned through `zolly foreman` with `gate: auto`, may start without
   the founder. Foreman may use a worker. A research task may read, run tests,
   search, and write only its result file on a branch — no product changes.
3. **Building is not.** A change still goes through `zolly route` and the
   normal gate. Money, production, publishing and customers are always
   `gate: human`.
4. Ask the founder only for the decision that changes what gets built. One
   question, five lines, two options, a recommendation (§2a of the manager
   persona).
5. Record what changes a future decision; append unresolved choices to
   `OPEN-QUESTIONS.md`; never repeat a question.

Do not manufacture work to fill an idle slot. "No questions" is a valid result.

## Filing work (Zolly → foreman)

```sh
zolly foreman "<task description with clear acceptance criteria>"
```

Opens a worktree, spawns a worker, runs review, and reports back. Zolly does
not dispatch workers directly; the foreman is the only thing that opens a
worktree.

Seat-to-seat messages go through `zolly bus <handle> <kind> <task> "<text>"`:
the receiver gets `[kind] task=<id> gate=<auto|human>`, not prose.

## Deciding small vs large

Run `zolly route "<instruction>"` first: it prints SMALL or LARGE with the words
that decided it; override only with a stated reason. The classification table
lives in exactly one place:
`.agents/prompts/task_triage.md`. Read it; do not carry a second copy here.

`route` is a **fixed instruction heuristic** (45 words, 3 paths, and its keyword
lists) — it is a first pass, not the project's change-size threshold. The
project's **Size thresholds** in `rules/project.md` govern how the foreman and
worker scope a change; they are not read by `zolly route`.

Two rules that hold regardless of the table:

- money, ad spend, production, publishing, or a customer is **never** — write
  `gate: human`;
- an instruction that is still ambiguous after reading the table gets **one
  question**, not a guess.

## Classifying a bulk queue

When the queue is long and the question is the same for every entry, do not read
it one file at a time.

1. **Freeze the question first.** Bucket, yes-no, or score. Labels must be
   exhaustive, mutually exclusive, and observable. Include a catch-all.
2. **Pre-filter** deterministically (path prefix, size, pure deletions). Log how
   many it removed.
3. **Judge the bulk** in one pass. Read only what it flags.
4. **Escalate and confirm by reading the code** — a verdict is evidence, not
   truth. Judge output on the highest-stakes item is the first thing to check.
5. **Report** counts, pre-filter removals, and the flagged items with evidence.

Below ~20 items, or a question needing cross-item reasoning, read directly.

## Working a task (worker)

1. Do the work on the task branch. The office keeps no separate in-flight state
   file: the task branch and the result file are the state. The brief itself is
   `.agents/queue/<id>.md`, shaped by `rules/queue-contract.md`.
2. Read the task. If it says to redo something that already stands, do not.
3. Do the work. Do not commit to `{{BASE_BRANCH}}`; work on the task branch.
4. Write `.agents/results/<id>.md` with `files_changed`, `verify` (command +
   result), `commit` (sha or none), `blockers`, and `reflection` (the answer to
   `.agents/prompts/post_task_reflect.md`; "nothing to record" is valid), in the
   format `rules/result-contract.md` fixes. **A task without a result file is
   unfinished**, whatever the queue status says.
5. Commit the result file on the task branch, push the branch, and open the PR
   (`gh pr create --base {{BASE_BRANCH}} --head foreman/<id>`). The reviewer
   reads the branch copy and merges the PR; without both, the task cannot close.
6. If genuinely blocked, write `gate: human` and say so plainly. Do not approve
   it yourself.

## Reviewing (reviewer)

Verdict is **PASS** or a specific objection. Never edit files, never write result
files, never push a feature branch — the one exception is the
`results-index.md` row appended on a PASS, committed and pushed on the branch
under review. On PASS the reviewer squash-merges the PR into `main` and may push
to `main` while doing it — that is the only door to `main` (founder decision,
2026-10-12). No other agent merges or pushes to `main`.

On PASS, append one row to `.agents/memory/results-index.md` on the task branch
before merging: `| YYYY-MM-DD | <task-id> | <pr-url> | <one-line outcome> |`.
Commit it, push the branch, then merge.

Check that the diff actually does what `.agents/results/<id>.md` claims, that
the diff matches the task, and that no file outside the task's scope is swept
in. A reviewer that never rejects is not a reviewer. If everything is green,
that is a finding to report upward, not a success.

Run `zolly verify <task-id>` before the verdict: it runs the project's
`test-command` in the task worktree and rewrites `## Verify` with the command,
exit code and tail. A non-zero exit is an objection. If it refuses (no
`test-command` in `project.md`), say so — an unverified result does not merge.

## Code graph (optional)

Only when `rules/project.md` says `gitnexus: on`:

- **foreman:** `impact <symbol> --direction upstream` for a LARGE scope;
  `context` and `processes` for the plan. The graph is evidence, not a verdict.
- **worker:** `detect-changes --scope all` before writing the result;
  `trace`/`context` while editing. Cite the symbol or route you checked.
- **reviewer:** read the worker's graph diff; run
  `detect-changes --scope compare --base-ref <base>` yourself when it is
  missing. `api_impact`/`shape_check` for route changes.

The index is derived and machine-local. `zolly doctor` fails when it predates
HEAD; uncommitted work needs `node .gitnexus/run.cjs analyze` before you trust
graph results for it. A missing finding is not proof of safety — the PDG/taint
layers model what they model.

## Settling

Settling records the outcome and releases the slot. On a reviewer objection it
must file the next retry task — the chain dies terminally at round 1 if it does
not. If you find a task that stays `escalated` with no `-r2` filed, that is the
bug.

## Merging

Merge authority is settled (founder decision, 2026-10-12): **the reviewer
merges a branch into main after recording PASS, and may push to main.** No other
agent merges or pushes to main. Zolly coordinates and stops a bad merge; she
does not merge herself. Nothing merges on a task's own say-so, and nothing
merges unreviewed.

## Writing a CSV

Any CSV handed over for review follows `rules/csv-conventions.md`: a fixed
column set, a fixed status vocabulary, and no secrets. Read it before writing
the first row.

## Choosing a browse tool

Pick the tool before reaching for one; the same job done with a different tool
each time is the failure this rule exists to stop.

| situation | tool |
|---|---|
| one known URL, static or SSR | `read` — no browser at all |
| search, many pages, structured bulk extraction | a bulk fetch/search tool |
| needs the founder's real logged-in session (supplier portal, payment gateway, admin) | the founder's own browser session, opened only for that task |
| a page shown beside the human, or a session of its own | a terminal browser |
| *why* did it break — network, console, perf | a devtools protocol client |
| native OS window or canvas | an OS automation tool |

- **Vision is unreliable on some machines** — screenshot/vision credits can fail
  with a 402. Confirm from the DOM, never from a screenshot.
- **The founder's real accounts are the founder's.** Never navigate or act on a
  tab you were not pointed at, and confirm before anything consequential.
- Tool versions and local ports change. Read the tool's own `--help` before
  relying on a recipe; a hardcoded version number is a claim that rots silently.
