---
description: Triage — decide whether a request is small, large, or a founder gate, then act
---

You are triaging one instruction from the founder. Decide its size, then act or
hand it off. Do not deliberate about the classification — decide and move.

## Read first

`.agents/rules/system.md` (identity, bounds, definitions),
`.agents/rules/safety.md` (destructive commands, secrets, external systems),
and `.agents/rules/project.md` (this project's thresholds and layout).

## Decide size

| The instruction is… | Action |
|---|---|
| a question, status check, or recorded decision | answer now |
| a check, count, diff, or state of anything | answer now, with the number you just read |
| one file, read-only, or a draft inside a report | **do it yourself**, then say what you did |
| meets the project's LARGE threshold (see `rules/project.md`) | **hand to foreman** |
| money, ad spend, production, publishing, customer contact | **never** — write `gate: human` |
| ambiguous | **ask one question.** Do not guess |

When you are unsure it is large: a wrong large costs ten seconds of waiting, a
wrong small costs a broken tree.

## Before acting, prove the premise

The most expensive failure in this office is acting on a status that disagrees
with reality. Before you report or build on a claim, **measure it**:

- A branch is landed only if its PR is merged: `gh pr view <n> --json state,mergedAt`.
  Do **not** use `git merge-base --is-ancestor` as the test — a squash merge
  leaves no branch commit in the ancestry and returns false for work that landed.
- A task is done only if `.agents/results/<id>.md` exists **and** its work is on
  {{BASE_BRANCH}}.
- A service is live only if the production service serves it. A merged branch is
  merged; that is all it means.
- A number you did not just read is not a number. Say "not logged."

## Then hand off

```sh
zolly foreman "<title> — <instruction>; done means <acceptance criteria>"
```

If nothing is dispatchable it says so — report that honestly.

## Close

State, in one line: what you did, or what you handed off and to whom. If you hit
a `gate: human`, name it and stop. Do not ask the founder a question they have
already answered.
