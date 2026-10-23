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

## Cite a rule by its phrase, not by its line number

When a result file records **where a rule lives** — "the load line is in
`foreman/agent.md:14`", "the screen is at `result-contract.md:69`" — quote the
**phrase** that identifies it, not a line number. A line number is a
coordinate in a file that is still being edited; a phrase is a claim about
content, and content moves without the claim becoming false.

Measured 2026-10-22, guzia-high-school: a reviewer quoted seven line numbers
for the rules in `reviewer/agent.md`, checked them, and **four were wrong** —
they were right when quoted and wrong by the time the message was read, because
the file had grown in between. They corrected the map, and the corrected map
was *also* wrong, for the same reason: two were blank lines, and three landed
on real text that was simply not the text meant. **A citation that lands on
something plausible is worse than one that lands nowhere**, because a reader
who checks it finds a sentence and assumes it is the one you meant.

So:

- **Quote the phrase.** "`Three dots for the diff, two for the log`" survives
  every edit that adds a line above it. A line number does not.
- **A line number is a convenience, not a citation.** Give it for the reader
  who is opening the file right now, but always beside the phrase, so the
  record still resolves when the file has moved.
- **A `file:line` record of a *change* is fine** — "the diff replaced the
  invocation at `reviewer/agent.md:64`" is a statement about a specific edit
  at a specific moment, and the moment is what makes it checkable. It is the
  record of where a rule **lives** that must be a phrase.

## A defect report is a snapshot — name the commit it was measured against

Every claim about a defect is true **of some moment**. The repo moves under
the report, and a report that does not say what it measured against will be
read against a tree it never saw.

Measured 2026-10-22, guzia-high-school: a reviewer's defect report described
the tool's `main` accurately, and by the time the message landed the fix had
already landed — so the report was stale, not wrong. Several claims that
session were stale the same way, and answering each one cost a round trip.

So a defect claim names its subject:

- **the commit or ref it was measured against** — "on `5cbeda3`,
  `pending_gates` calls `queue::read_queue`" — not "the tool currently does".
- **the command that produced the reading**, so a reader can re-run it.
- **when the reading was taken**, when the answer can change under it.

A report that names its subject stays readable after the fix lands: it becomes
the record of what was true, and of when. One that does not becomes a claim
about "now" that nobody can check.

The same holds in the other direction: when a fix lands, say what it
supersedes, so the old report is closed rather than contradicted.

## `## Verify` section rules

- List every command you ran, with its output
- **Put the command's OUTPUT in the file, not a count you computed.** When a
  claim is a measured value - a length, a count, a size - the reader must be
  able to re-run the command and see the same number. Timestamps and other
  metadata are exempt: they record when the command ran, not a value it
  measured, so a later run cannot reproduce them. A number that only exists in
  the prose is a number nobody can check, and one that contradicts its own
  command is worse than no number at all. Measured 2026-10-22, guzia-high-school:
  a message quoted a command that printed `2` next to a sentence claiming `45` -
  the command had been mangled by shell quoting, and the prose was right. An
  agent running it would have seen a number contradicting the claim and had to
  decide which to believe, which is the state this whole contract exists to end.
- **Count strings by running them.** A length, a count, a size - take it from
  the command, not from arithmetic done in your head. The same report counted
  the placeholder `worker-<frequency>-` instead of the rendered name:

  ```
  $ printf '%s' 'worker-<frequency>-' | wc -c
  19
  $ printf '%s' 'worker-zylosync-t-project-truths' | wc -c
  32
  ```

  The placeholder says 19; the rendered name is 32, exactly at herdr's
  32-character cap. Arithmetic on the placeholder answers a question nobody
  asked.
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
office layer was untracked wrote a personal detail about the founder into the
file. The repo is public. The file was merged through a correct review — one
path in the diff, tests green, checks green — and published a detail the
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
- **This file travels; a decision file does not.** A rule is copied into every
  office that installs the kit, so a category in a rule must be the **widest
  one that still says what the rule needs to say**. A decision record may
  name the category it was about — it lives on one machine and records one
  decision — but a rule that names a narrow category republishes that category
  to every future office, and the narrowing is the part that leaks. Measured
  2026-10-22: this file named a narrow category in its own worked example
  while the bullet two lines below said naming *which* personal detail is what
  publishes it — the rule file contradicting its own rule, which is the same
  shape as a procedure that pairs a command with a comment that cannot both
  be right. The example is left here at the wide category deliberately: even
  a **before-snippet** in a rule file is a copy that travels, so the rule
  quotes nothing narrower than it needs.

### Three places a copy survives — and the three checks

Screening the repo is not the whole check. A copy survives in three places,
and only one of them is reviewed before it lands:

| Surface | Where it lives | Gitignored? |
|---|---|---|
| the tree | tracked files, and `.agents/results/` once merged | yes, by decision |
| the notes | `.agents/inbox/` | yes — machine-local |
| the bus | the radio ledger, `~/.local/share/herdr-radio/radio.db` | **no** — it is in no git repository at all |

Run the three, with the term held in a local shell variable — never in a file
or a command you commit:

```bash
printf 'sensitive term: ' >&2; read -r -s term; echo >&2
: "${term:?empty term — refusing to match every line}"

grep -rlFw -- "$term" .agents/ \
  --exclude-dir=trash --exclude-dir=node_modules --exclude-dir=target \
  --exclude='*.rlib' --exclude='*.rmeta' --exclude='*.so'          # the tree
grep -rlFw -- "$term" .agents/inbox/                              # the notes
sqlite3 ~/.local/share/herdr-radio/radio.db <<SQL
.parameter set @term "$term"
SELECT id, from_handle FROM messages WHERE instr(text, @term) > 0;
SQL
```

**The tree check excludes, and the exclusions are not optional.** Without them
the check **cries wolf**: on a clean office, measured 2026-10-22, it returned
**19 hits — and not one of them was a real file.** Sixteen were compiled
artefacts (`.rlib`, `.rmeta` — Rust dependencies, where the term appears as a
coincidental byte run inside a binary), and the rest were markdown in
`.agents/trash/`, a gitignored scratch folder. The **published** surface was
clean the whole time: `git grep -nwF -- "$term" origin/main`
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
`git grep -nwF -- "$term" origin/main` — that one has no
artefacts in it by construction, because git only searches tracked files.

`radio log`, **never** `radio show <id>`. `radio show` is not a read — it is an
**acknowledgement**. The bus's own source says so: reading a message as its
addressed recipient "records the delivery — that is the pull path's
acknowledgement", and it ends with `UPDATE deliveries SET status='delivered'`.
So an audit written against `radio show` does not merely consume what it
checks — it **silently marks the inbox delivered**, so the audit destroys the
inbox state it is auditing, and the next reader sees the messages as already
read. A check that consumes the thing it checks is not a check. `radio log` is
read-only and prints message bodies to any seat.

**The bus check is a SCREEN over YOUR office's traffic, not a census of the
ledger — and the difference is SCOPE, not rendering.** `radio log` prints each
message body in full (it renders `row["text"]` verbatim), so it does not
truncate and clip a term out of a long line. What it does is **select**:
inside a pane it selects `WHERE from_ws = ? OR to_ws = ?` with the current
scope, so it shows **only your own frequency's traffic**. Outside herdr it
shows the whole ledger. Measured 2026-10-22 on a shared ledger: the raw table
held 58 messages carrying the term across two offices, and the in-pane log
showed 40 — the other office's 18 were not in scope, and the log gave no
indication of that.

So read the number as **"this many in this office"**, never as "this many
in the ledger" — and never as "the ledger is clean". The ledger is **shared
across offices**, so a clean log is not a clean bus. A rule that names the
log as THE check lets a dirty ledger pass because the dirty messages belong
to somebody else.

The SQLite query reads the **whole shared ledger**, not just this office's
scope. It is the gate; `radio log` remains a lower-bound, human-readable
screen, never the census. The query is a literal substring match and returns
message ids, so read each row and judge it — the census still needs a human.

**The term must not appear in a command either, not only in prose.** The
deceptive copy is a verification command quoted in a message, a note or a commit
message: the agent that quotes it did what looks like diligence, and the copy
still outlives the tree. The commands above read the value into a local `term`
variable; the term itself never appears in this file or in shell history.

**The bus is shared across offices.** The ledger holds other offices' messages,
so a clean result on this office's frequency is not a clean bus; the check
covers every frequency in it.

**The ledger is a record and is not ours to scrub.** It holds messages that are
not this office's, so the fix is the rule going forward — the local `term` value above
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
