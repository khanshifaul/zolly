---
name: close-the-day
description: "Use when the founder says goodnight, wraps up, calls it a day, asks what is left, or hands over to tomorrow. Runs a bounded close: reconcile what actually landed, verify each claim against repo state rather than reports, queue or escalate what is unfinished, and leave a resume point. Also use when asked for a day summary, a handover, or 'what did we do today'. Examples: \"Good night\", \"Let's wrap\", \"Wrap up the day\", \"What is left\", \"Anything I need to do\", \"Commit everything\", \"Handover for tomorrow\""
---

# Close the day

Bounded, measured, and honest. This skill produces a **verified** state, not a
narration of what the session felt like it accomplished.

## The failure this exists to prevent

An agent that wraps up by summarising intentions reads like a status report but
is fiction. Three shapes of that failure showed up on 2026-10-03 alone:

- A worker's result file said "landed". The branch was never merged. The
  summary repeated the claim because the claim was written down.
- A peer's report said four regression checks failed. They had passed; the
  agent read a truncated mid-run tail and reported the intermediate state.
- The manager attributed a read-only filesystem to a full disk, then told the
  founder to free space. The real cause was one line in a systemd unit.

So the rule is narrow and absolute: **every number and every status in the
closing report is read from repo or process state during this run, or it is
labelled `not logged`.** Never carried over from an earlier report, including
one written by another agent.

## Read-only until step 4

Steps 1–3 change nothing. Do not write, commit, or push while measuring — a
measurement that mutates the thing it measures is not a measurement.

```
git log --oneline -N {{BASE_BRANCH}}
git status --porcelain
git rev-list --count {{BASE_BRANCH}}..{{REMOTE}}/{{BASE_BRANCH}}
zolly brief
herdr agent list
git log --since='2 hours ago' --oneline | wc -l
ls .agents/runs/*.run
```

The base branch is the one named in `.agents/zolly/config.toml`
(`base_branch`), and the remote is `remote` in the same file.

## 1. Reconcile

Produce three lists, each verified against the commands above:

- **landed** — the PR is merged (`gh pr view <n> --json state,mergedAt`), or the
  content is on the base branch. A squash merge leaves no branch commit in the
  ancestry, so `--is-ancestor` is not the test; see step 2.
- **unmerged** — commits on a branch that is not an ancestor of the base branch
- **uncommitted** — working-tree changes, including untracked files
- **not started** — work identified during the day that was never filed: a defect
  named in chat, a deliverable waiting on the founder's input, a decision
  deferred. This is the list that gets lost, because it has no branch and no
  result file to leave a trace. Every item on it must leave this run as a
  `.agents/queue/<id>.md` carrying a `gate:` line, shaped by
  `rules/queue-contract.md`. An item that stays in chat is an item that did not
  survive the day.

  Measured 2026-10-16: a close reported "the queue is empty of unstarted work"
  from `for f in .agents/queue/*.md; [ -f .agents/results/$id.md ]` and every
  word of it was true — all 14 queue items had results. It was also useless,
  because the queue held no entry for the two defects and the video blocker
  that the day had named in chat over and over. **Absence of unstarted work in
  the queue is not absence of unstarted work.** Grep the day's own
  conversations for named-but-unfiled items before concluding nothing is
  outstanding.

For every unmerged branch, read the task's `status:` from `queue/<id>.md`.
`failed` and `escalated` are **not** finished work — they are the opposite, and
merging them because they have commits is how bad code reaches the base branch.
Say which they are; never lump them with landed work.

## 2. Verify each claim that will be repeated

For every statement already made today that you are about to repeat:

| Claim | How to check |
|---|---|
| a test passed | run it now; a truncated tail is not a result |
| a build passed | run it now |
| a file exists | `ls` it now |
| work is merged | `merge-base --is-ancestor` **only for true merge commits**. A **squash-merged** PR leaves no branch commit in the ancestry, so that check returns false for work that merged cleanly — verify by content instead: `git show {{REMOTE}}/{{BASE_BRANCH}}:<path>`, or `gh pr view <n> --json state,mergedAt`. Measured 2026-10-16: `--is-ancestor 033ffbec origin/main` said "not in main" for PR #131, which had merged as `b75ef9f4` a minute earlier. Reporting that false negative would have claimed the founder's ruling never landed. |
| a page renders | load it; a passing build is not a rendering |

If a claim cannot be re-checked in a minute, mark it `not verified` rather than
repeating it. **Correcting your own earlier claim is a success, not a failure** —
report the correction plainly, with what changed the answer.

## 3. Report, in this order

1. **Landed** — with commit shas
2. **Unmerged** — branch, commit count, and why it is not landed
3. **Uncommitted** — the exact files
4. **Not verified** — anything you could not check, named plainly
5. **Errors you made today** — specific and unhedged

Item 5 is not optional. A handover that hides its own mistakes is worse than no
handover, because tomorrow inherits them as facts.

## 4. Close out

Only now, and only what is genuinely complete:

- **Send the day brief to the founder's channel as part of the close — before
  the seats are stopped.** The brief otherwise exists in the chat pane only, and
  the founder has to ask for it. Send it with:

  ```
  zolly telegram-reply --chat-id <id> --text "<brief>"
  → Reply sent to chat <id>   (rc 0)
  ```

  The chat id is the `ZOLLY_ALLOWED_IDS` value in
  `~/.config/zolly/{{FREQUENCY}}/credentials` — read the id there, never echo the token that
  sits in the same file. Do not use `zolly telegram-collab --list` / `--link`
  for this: each sends a real message to the chat (`--view` is the read-only
  variant).
- Commit work that is finished and verified **to its task branch** — never to
  the base branch. Do not commit a file whose test you could not run.
- There is **no day-close command**. `zolly close <TASK_ID>` closes one *task*;
  the day close is this skill's procedure, run by hand. Verified 2026-10-16:
  `zolly --help` lists `close` (per-task) and no `telegram-day` at all. If you
  find yourself typing `zolly telegram-day progress …`, that command does not
  exist — record the day in the handoff instead.
- **Suspend anything that can message the founder overnight, and say that you
  did.** A watcher unit that sends a tapable poll when `pulse` reports waiting
  gates or stalls will keep a "closed" office talking — a promise kept in the
  wrong direction. Confirm the watcher's state with
  `systemctl --user list-timers`, then stop it and state it in the close report
  — an unnamed assumption is not a guarantee.
- Write `HANDOFF.md` at the repo root — that is where it already lives. It must
  carry the date, what landed, what is in flight, what is blocked, **the
  not-started list from step 1**, and the single next action. It also carries a
  suggested order for tomorrow, because "here is what is left" is far less useful
  than "do this first, then this". That is the entire point of a resume point.
- Do **not** park the handover in `.agents/memory/current.md`. Its live window is
  the newest few entries and older ones roll to an archive, so the resume point
  rotates out of view on the very next memory write. Give `current.md` a pointer
  entry; give the handover a file.
- State plainly which state dies with the session. Armed watchers, background
  jobs, and anything held only in a pane's scrollback do not survive; only
  committed files do. A resume point that assumes otherwise is a fiction.
- **Create `tomorrow_schedule.md` at the repo root.** `HANDOFF.md` records what
  *happened*; the schedule records what *should happen next*, in order, so the
  next session can open by proposing work instead of asking what to do. One
  entry per item, each with: the order, the item, **who owns it** (a worker, the
  founder, or the next Zolly), and the `gate:`. Two rules make it useful rather
  than decorative:
  - **Ordered by what unblocks the most**, not by what is easiest. The founder's
    one-line answer that starts a deliverable outranks a tidy cleanup.
  - **Nothing on it that is not also a `queue/` entry or a named founder
    decision.** A schedule item with no artifact behind it is a wish.
  Keep it to roughly a screen. A schedule that runs past a screen is a backlog,
  and a backlog is not a morning plan.
- Anything unfinished becomes a `gate:` line in `queue/` — never left as prose
  in a summary, because prose does not get dispatched.

## 5. The founder's list

End with only decisions that are genuinely theirs, each with the cost of the
options. Exclude anything already recorded in `OPEN-QUESTIONS.md` unless the
answer has changed.

A decision list padded with things you can decide yourself is noise. So is one
that omits something only they can answer.

## Never

- Report a number you did not measure this run
- Trust a result file as evidence that work landed
- Merge — the reviewer merges; the founder deploys
- Leave an unfinished task as a bullet in a summary instead of a queue entry
- Say "everything is done" when a branch is unmerged

## Related skills

| Situation | Skill |
|---|---|
| cold-starting tomorrow | `recap` |
| handing work to another agent or worktree | the foreman, via `zolly foreman` |
| reviewing an open PR before merge | the reviewer seat, `pr-reviewer` |
