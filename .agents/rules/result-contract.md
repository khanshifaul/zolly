# Result File Contract

Canonical format for `.agents/results/<task-id>.md`. Every worker writes this.

## Template

```markdown
---
id: <task-id>
title: <short title>
status: done | failed
gate: human | auto
---

## Summary

<what was done, in 3-5 lines>

## Files changed

- `path/to/file1.md`
- `path/to/file2.csv`

## Verify

<commands run, with output>

## Commit

<commit SHA>

## Blockers

<what stopped this, or "none">
```

## Fields

| Field | Required | Notes |
|-------|----------|-------|
| `id` | yes | task ID |
| `title` | yes | short title |
| `status` | yes | `done` or `failed` |
| `gate` | yes | `human` = founder must review before merge; `auto` = safe to merge automatically |
| `## Summary` | yes | 3-5 lines |
| `## Files changed` | yes | list of paths |
| `## Verify` | yes | commands run + output. If no commands, say "none — file-only change" |
| `## Commit` | yes | commit SHA, or "not committed" |
| `## Blockers` | yes | what stopped this, or "none" |

## `## Verify` section rules

- List every command you ran, with its output
- If a test exists and you did not run it, say so
- If you cannot verify something, say so — do not claim it passes
- The reviewer reads this section. If it is missing, the reviewer OBJECTIONs.
- `zolly verify <task-id>` runs the project's `test-command` and rewrites this
  section with the command, exit code, timestamp and output tail. A non-zero
  exit is an OBJECTION; never overwrite it by hand.
- If `## Verify` claims tests it cannot reproduce, the reviewer OBJECTIONs.

## What a result file must not contain

A result file is **committed to the task branch**, and on some projects the
branch reaches a **public** repo. A result file states the **decision** and
the **paths**. It never states the personal content behind them.

Measured 2026-10-22, guzia-high-school: a result file that recorded *why* the
office layer was untracked wrote the founder's personal health detail into
the file. The repo is public. The file was merged through a correct review —
one path in the diff, tests green, checks green — and published a detail the
founder's own decision was about keeping private. The gate checked the diff
and the tests; nothing checked the **content** of a file the review added.

So:

- Name the **decision** and the **paths**. "The office layer carries the
  founder's personal notes and working preferences, so `.agents/` is not
  published" is correct. Naming *which* personal detail is being protected is
  what publishes it.
- Never quote a personal detail, a health or accessibility note, a family or
  home detail, a private contact, or a credential — even obliquely, even
  "several files mention X".
- If the reason for a rule cannot be stated without the personal content,
  state the rule and the paths, and stop there.
- The same applies to anything else a review adds: a diff, a commit message,
  a doc. The screen is the content, not the file type.

### Three places a copy survives — and the three checks

Screening the repo is not the whole check. A copy survives in three places,
and only one of them is reviewed before it lands:

| Surface | Where it lives | Gitignored? |
|---|---|---|
| the tree | tracked files, and `.agents/results/` once merged | yes, by decision |
| the notes | `.agents/inbox/` | yes — machine-local |
| the bus | the radio ledger, `~/.local/share/herdr-radio/radio.db` | **no** — it is in no git repository at all |

Run all three, as written:

```bash
grep -ril "<the founder-sensitive term>" .agents/ \
  --exclude-dir=trash --exclude-dir=node_modules --exclude-dir=target \
  --exclude='*.rlib' --exclude='*.rmeta' --exclude='*.so'          # the tree
grep -ril "<the founder-sensitive term>" .agents/inbox/            # the notes
radio log --limit 500 | grep -ci "<the founder-sensitive term>"   # the bus
```

**The tree check excludes, and the exclusions are not optional.** Without them
the check **cries wolf**: on a clean office, measured 2026-10-22, it returned
**19 hits — and not one of them was a real file.** Sixteen were compiled
artefacts (`.rlib`, `.rmeta` — Rust dependencies, where the term appears as a
coincidental byte run inside a binary), and the rest were markdown in
`.agents/trash/`, a gitignored scratch folder. The **published** surface was
clean the whole time: `git grep -n -w "<the founder-sensitive term>" origin/main`
returns nothing.

A privacy check that reports hits on a clean office teaches its reader to
distrust the check, and then the check stops being run. This rule already warns
about the case-insensitive false positive and the token-plus-phrase false
positive; the artefact trap is the third of the family, and it is the one an
agent hits first because it greps a directory and gets a wall of paths.

**Match the check to the definition.** The table above defines the tree as
"tracked files, and `.agents/results/` once merged". The exclusions make the
command match that definition: what is left is files a person could read and a
reader could see. If you want the published surface exactly, use
`git grep -n -w "<the founder-sensitive term>" origin/main` — that one has no
artefacts in it by construction, because git only searches tracked files.

`radio log`, **never** `radio show <id>`: `radio show` reads one message and
**consumes** it, so it cannot be a check — using it as an audit destroys what it
audits. `radio log` is read-only and prints message bodies to any seat.

**The bus check is a SCREEN, not a census, and its number is a floor.** The
log rendering **truncates message bodies**, so the count it reports is lower
than the number of occurrences in the ledger. Measured 2026-10-22: the log
reported 38 where the raw table held 47 — and an hour earlier the same two
numbers were 37 and 43, so **the gap widens as messages are added**. Read the
number as "at least this many", never as "exactly this many". A rule that says
"run this and you will know" is wrong by the size of the truncation, and a
reader who trusts it will conclude the bus is clean when it is not.

For the **exact** count, the ledger itself is a SQLite database:
```bash
sqlite3 ~/.local/share/herdr-radio/radio.db \
  "SELECT count(*) FROM messages WHERE text LIKE '%<the founder-sensitive term>%';"
```
Use that when the number matters and the log's truncation would mislead.

**One wrinkle to know about this command.** Written literally, the placeholder
in the pattern matches the placeholder itself — and several messages
legitimately contain the placeholder, because a report that correctly used
`<the founder-sensitive term>` instead of the term IS the behaviour this rule
asks for. Measured 2026-10-22: that query returned **4**, all of them
correctly-written reports, and **0** real leaks. So a non-zero result here is
usually the rule working, not the rule failing. **Read the matching message
ids, not just the count** — `SELECT id, from_handle FROM messages WHERE …` —
and judge each one. The screen is the screen; the census needs a human.

**The term must not appear in a command either, not only in prose.** The
deceptive copy is a verification command quoted in a message, a note or a commit
message: the agent that quotes it did what looks like diligence, and the copy
still outlives the tree. **The canonical form of a privacy check in any message
or file is the placeholder, never the term itself** — the three lines above are
written that way on purpose.

**The bus is shared across offices.** The ledger holds other offices' messages,
so a clean result on this office's frequency is not a clean bus; the check
covers every frequency in it.

**The ledger is a record and is not ours to scrub.** It holds messages that are
not this office's, so the fix is the rule going forward — the placeholder above
— not a deletion. Report a bus hit; do not edit the ledger.

The rule is stated as a **category** — "a personal detail about the founder" —
and never as the content behind it. A category is what a reader can apply;
naming the content would be the leak the rule exists to stop.

The reviewer checks all three before merging. If a file a review adds carries
personal content, that is an **OBJECTION** — and the fix is to state the
decision neutrally, not to delete the file. A hit on the bus is reported upward,
not deleted.

### The same rule, one layer up: the brief

A brief is not private. It is the instruction that produces a file that may
be published, and a worker writes what the brief says. Measured 2026-10-22:
the leak above did not start in the result file — it started in the brief,
which told the worker to record the reason for a decision and worded that
instruction to include the personal fact. The worker wrote it faithfully. A
rule about result files would not have stopped it, because the sentence was
authored one layer earlier.

So a brief must not instruct a worker to write a personal fact into a result
file — **not even as an example of the kind of thing to avoid**, and not even
in a brief that says not to. When a brief needs to say why something is
untracked, it says: name the decision, name the paths, and stop.

A brief is written by a manager or a foreman, and both are bound by this rule
before the task is dispatched. The brief's own shape is fixed in
`.agents/rules/queue-contract.md`.

## `gate` field

- `human` — the founder must review before merge (default for all workers)
- `auto` — safe to merge without founder review (only for docs, CSV updates, no-behavior changes)

**Both directions:**

- **(i) Never claim `auto` for a behaviour change.** A behaviour change that claims `auto` is the error the reviewer will object to. Use `human`.
- **(ii) Claim `auto` when the change is docs-content-only — ALL documentation-content-only changes are `auto`, including published and user-facing docs.** Do not default to `human` out of caution — a needless human gate parks the founder on work the contract already declared safe.

The reviewer never approves `gate: human`. That is the founder's call.

## `approved:` marker — where an approval lives

- **The single authoritative place:** `.agents/results/<task-id>.md` on the
  **task branch**, not the main checkout.
- **Exact line:** `approved: yes`
- **Position:** immediately AFTER the `gate: human` line in the frontmatter:

```
gate: human
approved: yes
```

- **When:** it must be **committed on the branch** BEFORE review is requested.
- **Not authoritative:** an approval that exists only in the main checkout copy of the result file does not satisfy the gate. The reviewer reads the branch copy; the main-checkout copy is invisible to it.

## Path

All workers write to `.agents/results/<task-id>.md`. Never `results/`, never
`reports/results/`.
