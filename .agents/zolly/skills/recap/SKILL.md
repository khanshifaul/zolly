---
name: recap
description: "Use when starting cold in this repo with no memory of prior sessions, or when asked what the current state is, what is in flight, what is blocked, or what just landed. Replaces reading HANDOFF.md end-to-end. Examples: \"Recap the repo\", \"What is in flight?\", \"What is blocked?\", \"Where did we leave off?\", \"Orient me\""
---

# Repo Recap

A fast, bounded orientation of this repo's current state, emitted in a fixed shape
so a human can scan it in five seconds and an agent can rely on the same four
headings every time.

## This skill is read-only

It reports; it does not change. While running a recap you must **not**:

- write, append to, or reformat `HANDOFF.md`, `AGENTS.md`, or `OPEN-QUESTIONS.md`
- create, edit, or delete any file in the repo
- run a migration, seed, or any command with a side effect (a database
  migration, a deploy, `git commit`, `git push`, `git checkout`, `git reset`, `rm`)
- contact a person or a service (no posting, DM, email, or comment)

Read-only commands only: `tail`, `sed`, `grep`, `awk`, `wc`, `ls`, `git log`,
`git status`, `git diff`.
If the task in front of you needs a change, finish the recap first, then do the
change under its own workflow.

## Why this exists: HANDOFF.md grows without bound

`HANDOFF.md` is an append-only log — in the office this skill was written for it
had reached ~6,000 lines / ~430KB with 100+ `##` headings and no index. A
cold-starting agent that reads it end-to-end spends a large share of its context
on history before it understands anything, and the oldest ~90% of it is
superseded by the tail.

So: **do not read it end-to-end.** Read the newest one or two sessions from the
tail. The bounded read below is roughly 600 lines — well under a tenth of the
file, and a small fraction of a context window.

Newest sessions are at the **tail**, because the file is append-only. The head is
the oldest material in the repo.

Treat every size quoted here as a snapshot; `wc -l HANDOFF.md` is the live
number. What matters is the ratio, and that only ever gets worse.

## Read order

Front-loads a small amount of high-value material. Stop as soon as you can answer
`NOW`, `BLOCKED` and `RECENT` — steps 1–4 are the recap; step 5 is only for the task
you are about to do.

### 1. Latest sessions in `HANDOFF.md` (tail, ~200 lines)

```
grep -n "^## " HANDOFF.md | tail -2
sed -n '<line number of the EARLIER one>,$p' HANDOFF.md
```

**Take the read length from the heading locator, not from a fixed line count.** A hardcoded
`tail -200` rots silently as the file grows: session blocks are ~100–200 lines, so once the
last block grew past that, `tail -200` stopped reaching the second-to-last heading and
quietly returned **one** session instead of two — a stale `NOW`, with nothing to signal the
truncation. Anchor on the heading line numbers and the same command returns two sessions
today and keeps returning two as the file doubles.

**Match `^## `, not `^## Session`.** Only about half the session headings use the
`## Session …` prefix; the rest are bare `## <date> — <subject>`. Grepping for
`## Session` skips every bare-headed entry, so the "latest" session it reports can be
days older than the actual tail. When unsure, trust the tail over any heading count.

Read from the **second-to-last** heading onward. Session blocks usually carry
`**Completed**`, `**Remaining**`, and `**Open**` subheadings — `**Open**` is
normally the highest-value line in the whole file, because that is where a human
decision and a blocked task get named. If the tail is mid-session, read to EOF.

### 2. This project's own rules (`.agents/rules/project.md`)

```
sed -n '/^## Architecture/,/^## Size thresholds/p' .agents/rules/project.md
sed -n '/^## Where things live/,/^## Size thresholds/p' .agents/rules/project.md
```

Read the standards tables **only** if the task you are about to do touches them. Do
not read the file top to bottom. The universal office rules
(`.agents/rules/system.md`, `safety.md`, `workflows.md`) are short and are read
in full — they are the same in every project.

### 3. `OPEN-QUESTIONS.md` — unresolved only (~100 lines)

Answers are recorded by *deleting* the entry, but in practice resolved blocks often
linger with a marker, so filter rather than assume:

```
grep -n "^## Q" OPEN-QUESTIONS.md
grep -n -A6 "^## Q" OPEN-QUESTIONS.md
```

Skip an entry when **any** of these hold:

- its heading or body carries `RESOLVED` / `Resolved:` — note that an entry marked
  *Resolved* whose *To resolve* line is struck through is still resolved;
- it has a "Default while unanswered" / "Assumed default" line — that is **not**
  resolved. The default is in force and a human can still be asked, so carry it
  forward, but mark it as "default in force, not blocking" rather than a hard block.

**The common case is a third pattern: answered in code, entry never deleted.** A
question gets settled by a commit, and nobody goes back to remove the block. The
`RESOLVED` filter misses these entirely, which is what inflates `BLOCKED` with
non-blockers. Before carrying an entry forward, check whether it was already decided:

```
git log --oneline -8 -- OPEN-QUESTIONS.md .agents/rules/
```

If a recent commit plainly implements the decision the question asks about, report it
as resolved-by-commit (naming the sha) rather than as an open blocker. When genuinely
unsure, carry it forward but say the decision may already be settled — do not assert
a blocker you have not verified.

### 4. Recent commits (15 lines)

```
git log --oneline -15
git status --short
```

The log tells you what actually landed; the tail of `HANDOFF.md` tells you what was
*intended* to land. When they disagree, trust the log. Uncommitted changes in
`git status` mean another agent is mid-flight — say so under `NOW` and do not touch
their files.

### 5. The task's own spec (only if one applies)

```
ls specs/ 2>/dev/null
```

If the task implements a feature and **this project's `project.md` says it is
spec-first**, find the matching `specs/**/<feature>.spec.md` and read its
**Goal, Acceptance Criteria, API Contract, DB Changes, Test Plan** headings.
Skip this step entirely if the project keeps no specs; do not read `specs/`
wholesale to find out.

## Output shape

Emit exactly these four headings, in this order, every time. Keep each to a handful of
lines. An empty section says `none` — never drop a heading, because the shape is what
makes the recap scannable.

```
NOW
- <what is in flight, and in which file/app>
- <uncommitted work by another agent, if git status shows it>

BLOCKED
- <Q-id>: <one line> — blocks <what>

RECENT
- <sha> <subject>          (git log --oneline -15, newest first)

RULES THAT BITE
- Never commit to {{BASE_BRANCH}}; never push without being asked
- A decision that is the human's goes in OPEN-QUESTIONS.md — do not guess
- <the project's own standing rules, taken from .agents/rules/project.md>
```

`RULES THAT BITE` is the standing set — reproduce it verbatim from the project's
own rules. It is the short list of rules that are violated most often, and it is
what a cold agent needs most because none of them are inferable from the code.

## Checklist

```
- [ ] HANDOFF.md read from the TAIL via `grep -n "^## "` (NOT `^## Session`), newest 1–2 only,
      anchored on the second-to-last heading's line number — not a fixed `tail -N`
- [ ] .agents/rules/project.md extracted by section, not read whole
- [ ] OPEN-QUESTIONS.md filtered to unresolved entries, incl. the resolved-in-code check
- [ ] git log --oneline -15 and git status --short
- [ ] Spec read only if the task implements a feature
- [ ] Four headings emitted in order, none dropped
- [ ] Nothing written, migrated, or sent
```

## Known limitation

**`HANDOFF.md` should be split, and this skill does not do it.** It is a log of
history; a recap that tails it will always miss older context, and `grep`-ing it for a
decision made long ago is worse than either extreme. The recommended end state:

- `HANDOFF.md` — current state only, rewritten in place at each session, target
  **under ~300 lines**. Append-only stops being a virtue at this size; the log has
  stopped being readable, which is the entire problem.
- `handoff/archive/YYYY-MM.md` — the existing history, moved verbatim, never edited.
  Nothing is lost, and `grep` still reaches it when a session turns out to matter.

The split is a founder decision — it is thousands of lines of history, it changes what
every agent reads on cold start, and it rewrites the file the project points at. It is
deliberately **not** performed here. Until it happens, this skill's bounded tail-read
is the mitigation.

Second limitation: because answers are supposed to be recorded by *deleting* the
entry, `OPEN-QUESTIONS.md` still carries resolved blocks — some marked `RESOLVED`,
some answered by a commit and never touched. Step 3 filters the first kind and
git-checks the second, but the file will keep growing until the deletions are made.
