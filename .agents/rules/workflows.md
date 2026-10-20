# rules/workflows.md — SOPs

The standard operating procedures of this office. Each is a sequence, not a
principle.

## Starting a shift (Zolly)

1. **Prove you are running.** `zolly init` — idempotent. Opens foreman and
   pr-reviewer tabs with their agent.md files loaded. If a unit fails to start,
   say so; do not greet the founder from a dead listener.
2. **Measure, never recall.** `zolly pulse`, `herdr agent list`,
   `git log --since='2 hours ago' --oneline | wc -l`.
3. **Brief** with `zolly brief`. Three paragraphs maximum: what finished, what is
   waiting (named), the one next action or "nothing, I have it".
4. If `brief` fails, report the error verbatim. Never claim delivery.

## Filing work (Zolly → foreman)

```sh
zolly foreman "<task description with clear acceptance criteria>"
```

Opens a worktree, spawns a worker, runs review, and reports back. Zolly does
not dispatch workers directly; the foreman is the only thing that opens a
worktree.

## Deciding small vs large

The classification table lives in exactly one place:
`.agents/prompts/task_triage.md`. Read it; do not carry a second copy here.
The "large" threshold is a property of the project, not of the office: read it
from `rules/project.md` → **Size thresholds**.

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
   result), `commit` (sha or none), and `blockers`, in the format
   `rules/result-contract.md` fixes. **A task without a result file is
   unfinished**, whatever the queue status says.
5. Commit the result file on the task branch, push the branch, and open the PR
   (`gh pr create --base {{BASE_BRANCH}} --head foreman/<id>`). The reviewer
   reads the branch copy and merges the PR; without both, the task cannot close.
6. If genuinely blocked, write `gate: human` and say so plainly. Do not approve
   it yourself.

## Reviewing (reviewer)

Verdict is **PASS** or a specific objection. Never edit files, never write result
files, never push a feature branch. On PASS the reviewer squash-merges the PR
into `main` and may push to `main` while doing it — that is the only door to
`main` (founder decision, 2026-10-12). No other agent merges or pushes to
`main`.

Check that the diff actually does what `.agents/results/<id>.md` claims, that
the diff matches the task, and that no file outside the task's scope is swept
in. A reviewer that never rejects is not a reviewer. If everything is green,
that is a finding to report upward, not a success.

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
