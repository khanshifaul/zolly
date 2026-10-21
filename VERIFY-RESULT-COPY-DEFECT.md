# Defect report: `zolly verify` writes its evidence to the copy the reviewer never reads

Repo: `/home/khan/Projects/zolly` (the office-kit source; the `zolly` binary at
`/home/khan/.local/bin/zolly` is built from `tool/` here).
Symptom observed in: `/home/khan/Projects/zylosync`, 2026-10-22.

## The claim in the rules

`.agents/rules/result-contract.md` says the reviewer reads the **branch** copy
of the result file, never the main-checkout copy:

```
- **The single authoritative place:** `.agents/results/<task-id>.md` on the
  **task branch**, not the main checkout.
```

`.agents/rules/workflows.md` says the same about the worker:

> Worker writes result to `.agents/results/<task-id>.md` **on the task branch**
> (in `.agents/worktrees/<task-id>/`), committed there before review. The
> main-checkout copy is never authoritative.

And `.agents/rules/workflows.md`, under *Reviewing (reviewer)*:

> Run `zolly verify <task-id>` before the verdict: it runs the project's
> `test-command` in the task worktree and rewrites `## Verify` with the
> command, exit code and tail.

## What the code does

`tool/src/main.rs:967` `verify_cmd`:

```rust
let worktree = root.join(".agents/worktrees").join(task_id);
...
let result_path = root.join(".agents/results").join(format!("{task_id}.md"));
...
let updated = replace_section(&document, "Verify", &block);
std::fs::write(&result_path, updated)
```

It runs the command in `root/.agents/worktrees/<task-id>/` — the worktree — and
writes the `## Verify` section to `root/.agents/results/<task-id>.md` — the
**main checkout**, not the worktree's copy.

## The consequence, reproduced

Scratch repo, one task `t-x`, a branch `foreman/t-x` whose result file carries
a `## Verify` section written by the worker, and a `master` checkout with the
same file carrying `none yet`:

```console
$ cd /tmp/vtest && git checkout master
$ zolly verify t-x
verify: passed — echo HELLO_FROM_VERIFY

$ grep -A3 '## Verify' .agents/results/t-x.md      # the MAIN checkout
## Verify

- command: `echo HELLO_FROM_VERIFY`
- exit: 0

$ git show foreman/t-x:.agents/results/t-x.md | grep -A2 '## Verify'   # the BRANCH copy
## Verify

WORKER WROTE THIS
```

The branch copy — the one the reviewer reads, and the one the contract calls
authoritative — is **untouched**. The evidence lands somewhere nobody reviews.

## Why this compounds with the approval defect

These two are the same shape of bug: the tool writes to the wrong copy of the
same file.

- `handle_approve` writes `approved: yes` into `.agents/queue/`, while the
  brief reads `.agents/results/`. (See `APPROVAL-PATH-DEFECT.md`.)
- `verify_cmd` writes `## Verify` into the main checkout, while the reviewer
  reads the branch copy.

Both produce the same founder-visible failure: the office believes it has
recorded something it did not record where it matters.

## A second, sharper consequence: the task that installs the test command
cannot verify itself

`verify_cmd` reads the test command from `root/.agents/rules/project.md` —
the **main** checkout — via `find_test_command(root)` (`tool/src/main.rs:931`).

```rust
fn find_test_command(root: &std::Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".agents/rules/project.md")).ok()?;
    parse_test_command(&text)
}
```

The first task in a project that has no `test-command:` line yet is therefore
unverifiable by construction: the command it installs lives on its branch, and
`verify` reads the main checkout, which does not have it. The tool reports:

```
Error: no test command — add `test-command: <cmd>` to .agents/rules/project.md
```

In `/home/khan/Projects/zylosync` this is the live state — `.agents/rules/project.md`
is still the install template with 22 `TODO:` placeholders and no
`test-command:` line — so **no PR can currently merge**, because the reviewer
has no verify command to run.

## What a fix looks like

`verify_cmd` should write the `## Verify` section to the **worktree's** copy
of the result file — `worktree.join(".agents/results").join(...)` — which is the
branch copy the reviewer reads and the contract calls authoritative. Reading
the test command from the same place makes the tool self-consistent: a task
that installs the command can then be verified, because the command is read
from the branch that carries it.

That change also removes the need for the two-PR split this defect currently
forces (PR-1 adds the command line so PR-2 can be verified).

If the main-checkout copy must also be updated for any reason, write it as a
**second** write, after the branch copy — never instead of it.

A test for this: a scratch repo with a worktree on `foreman/<id>`, a branch
result file, a `test-command` in the **branch's** `.agents/rules/project.md`,
then assert that after `verify_cmd` the **branch** copy carries the command,
the exit code and the timestamp, and that the main-checkout copy is not the
only place the evidence landed.

## Evidence to reproduce

```sh
# in the kit repo
sed -n '967,1010p' tool/src/main.rs       # verify_cmd — note result_path vs worktree
sed -n '931,935p'  tool/src/main.rs       # find_test_command — reads root, not the worktree
```

## Note on scope

This report is documentation only; it ships no code change. It is filed
alongside `APPROVAL-PATH-DEFECT.md`, which describes the other half of the
same bug shape. The two fixes belong in the same pass, but they are
independent changes and neither requires the other.

The repo also has an unrelated uncommitted change in `tool/src/herdr.rs` (a
`launch_args` parameter threaded through `agent_start`). It is deliberately
not swept into this branch.
