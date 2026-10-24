# rules/project.md — this project's truths

# Generated at install. Edit freely: this file is yours, not the template's.

Everything in this file is **specific to this project**. The universal rules
live in `system.md`, `safety.md` and `workflows.md`; this file is the only
place that may name a directory, a service, a threshold or a command that
belongs to this codebase.

The installer fills the placeholder tokens below. Anything left as a `TODO:` is
a question you must answer from the tree before you plan anything.

## Architecture

<!-- TODO: replace this with the truth, verified against the tree on disk. -->

**{{PROJECT_NAME}}** — TODO: one sentence on what it is and what it is for.

- Application shape: TODO — one process, or several? Name the services.
- Persistence layer: TODO — what stores the data, and where does it live?
- Client side: TODO — what does a user actually talk to?
- Test command: TODO — the exact command that runs the suite.

test-command: TODO

`zolly verify <task-id>` runs that line in the task worktree and rewrites the
result's `## Verify` section. Leave it as `TODO` until you know the command;
verify then refuses rather than guessing.

Before writing a plan, spec, or import: **check the tree first.** Never write
against a service that is not on disk. If this file disagrees with the tree,
the tree is right and this file is a bug — fix it here.

## Where things live

| Concern | Path |
|---|---|
| Application code | TODO |
| Tests | TODO |
| Migrations | TODO |
| Documentation | TODO |
| CI config | TODO |

- TODO: note any layout that surprises a reader — a directory that does *not*
  exist, or a service that lives somewhere unexpected.
- TODO: name the directories that are generated and must never be edited by hand.

## Size thresholds

These are the numbers the foreman and worker use when scoping a change. They
are **not** read by `zolly route`: that command is a fixed instruction
heuristic (45 words, 3 paths, keyword lists), not a diff-size judgement.

| Threshold | Value |
|---|---|
| A change is LARGE when it touches | TODO |
| A change is LARGE when it spans more than | TODO files |
| A change is LARGE when it is more than | TODO |
| A diff is small when it is under | TODO lines |

## Divisions / teams

| Actor | Does |
|---|---|
| TODO | TODO |

## Conventions this project has that the office does not

- TODO: a command that is not the obvious one (a linter, a typecheck, a runner).
- TODO: a rule a newcomer would get wrong on their first day.
- TODO: anything the base branch, remote or frequency implies for this project.

Project wiring the office reads from `.agents/zolly/config.toml`:

- frequency: `{{FREQUENCY}}`
- base branch: `{{BASE_BRANCH}}`
- remote: `{{REMOTE}}`

## Optional tooling

gitnexus: off

Set that to `on` after `node .gitnexus/run.cjs analyze`. `zolly doctor` then
fails when the index no longer matches HEAD, and the workflow tells foreman,
worker and reviewer which graph queries to use. Zolly never requires it.
