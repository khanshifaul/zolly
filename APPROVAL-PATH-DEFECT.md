# Defect report: the founder's `haa` cannot clear the one gate the office reports as waiting

Repo: `/home/khan/Projects/zolly` (the office-kit source; the `zolly` binary at
`/home/khan/.local/bin/zolly` is built from `tool/` here).
Symptom observed in: `/home/khan/Projects/zylosync`, 2026-10-22.

## The two facts

**1. The gate lives in `.agents/results/`, not in `.agents/queue/`.**

`.agents/rules/result-contract.md` says the authoritative place for an approval
is the result file on the task branch:

```
## `approved:` marker — where an approval lives
- **The single authoritative place:** `.agents/results/<task-id>.md` on the
  **task branch**, not the main checkout.
- **Exact line:** `approved: yes`
```

The brief reads gates from the same place — `tool/src/brief.rs:199`
`result_gates()` scans `dir` = `.agents/results/`, and `tool/src/brief.rs:214`
skips a file only when `fm.gate_human` is false or `fm.approved` is true.

**2. The `haa` handler only scans `.agents/queue/`.**

`tool/src/tg/handlers.rs:46`:

```rust
fn pending_gates(root: &Path) -> Result<Vec<(std::path::PathBuf, String)>> {
    Ok(queue::read_queue(root)?
        .into_iter()
        .filter(|(_, c)| queue::is_pending_gate(&queue::parse_frontmatter(c)))
        .collect())
}
```

`queue::read_queue` (`tool/src/queue.rs:251`) reads
`root.join(".agents/queue")` and nothing else. `handle_approve`
(`tool/src/tg/handlers.rs:68`) writes `approved: yes` into exactly the files
that function returns.

Measured in zylosync right now:

```console
$ find .agents/queue -type f | wc -l
0
$ ls .agents/results | wc -l
106
$ zolly brief --json | jq -r '.gates'
["t-k3s-deploy-prisma-rerun.md"]
```

So `handle_approve` iterates an empty list. It reports
`"No pending gate: human tasks to approve."` and writes nothing, while the
brief simultaneously reports one gate waiting.

Reproduced in isolation (mirroring `pending_gates` exactly, in a scratch repo
with one result file carrying `gate: human` and an empty `.agents/queue/`):

```
pending_gates sees: []
approved: []
```

The result file was unchanged afterwards. The founder's `haa` cannot clear the
gate.

## A second, independent blocker in the same path

Even if `handle_approve` were pointed at `.agents/results/`, it would still
miss this gate. `queue::is_pending_gate` (`tool/src/queue.rs:125`) is:

```rust
pub fn is_pending_gate(fm: &Frontmatter) -> bool {
    fm.gate_human && !fm.approved && !fm.escalated && !fm.settled
}
```

`fm.settled` is true when `status:` is one of done / settled / failed /
escalated / complete / completed (`tool/src/queue.rs:111`). The waiting gate
`.agents/results/t-k3s-deploy-prisma-rerun.md` carries `status: done` with
`gate: human` — and `rules/result-contract.md` says explicitly:

> Settled status does NOT clear a gate: `status: done` plus an unapproved gate
> is exactly the "finished but founder never signed off" state this check
> exists for.

`tool/src/queue.rs:163` (`is_unresolved_gate`) has the correct rule —
`is_waiting_gate(content, false)` — and `brief.rs` uses the merge-aware
`is_waiting_gate`. But the Telegram approve path uses `is_pending_gate`,
which drops settled files. The two predicates disagree about the same file.

So there are two independent reasons the founder's `haa` cannot clear
`t-k3s-deploy-prisma-rerun.md`:

1. the handler scans the wrong directory (`.agents/queue/` instead of
   `.agents/results/`), and
2. the predicate it uses (`is_pending_gate`) excludes settled-status files,
   which is every finished-but-unapproved gate.

## What a fix looks like

The approve/decline path needs to:

- read from the same place the brief reads from (`.agents/results/`, and the
  **branch copy** when the task is in flight — the result contract says the
  main-checkout copy is not authoritative);
- use the same predicate the brief uses (`is_waiting_gate` / merge-aware
  gate detection), not a third one;
- write `approved: yes` immediately after the `gate: human` line, which is
  what `result-contract.md` prescribes and what `brief.rs` parses.

Note the merge-aware subtlety: `is_waiting_gate(content, merged)` returns
false for a **bare** `gate: human` on a merged task — the merge is treated as
the review. But a **qualified** gate (`gate: human — live cutover only`)
stays waiting even when merged, because the qualifier names a decision a merge
cannot make. The approve path must preserve that distinction, or approving a
merged qualified gate will silently do nothing.

`handle_status` (`tool/src/tg/handlers.rs:52`) has the same directory bug:
the founder's `/status` on Telegram reports "No tasks waiting on your
approval." while `zolly brief` reports one gate. Two different answers to the
same question, from two different directories.

## Evidence to reproduce

In `/home/khan/Projects/zylosync`:

```sh
find .agents/queue -type f | wc -l          # 0
zolly brief --json | jq '.gates'             # ["t-k3s-deploy-prisma-rerun.md"]
grep -c 'approved: yes' .agents/results/t-k3s-deploy-prisma-rerun.md   # 0
grep -m1 '^status:' .agents/results/t-k3s-deploy-prisma-rerun.md      # status: done
grep -m1 '^gate:'   .agents/results/t-k3s-deploy-prisma-rerun.md      # gate: human — no k3s/kubectl access from this host
```

In `/home/khan/Projects/zolly`:

```sh
sed -n '40,80p' tool/src/tg/handlers.rs      # pending_gates + handle_approve
sed -n '120,135p' tool/src/queue.rs          # is_pending_gate
sed -n '160,175p' tool/src/queue.rs          # is_unresolved_gate — the right rule
sed -n '195,225p' tool/src/brief.rs          # result_gates — where the gate really lives
```

## Why this is the office's worst kind of bug

The founder is told a gate is waiting. He answers `haa`. The office says
"Approved: ..." or "No pending gate: human tasks to approve." Either way,
**the gate does not clear**, and the brief keeps reporting it. He is then
told the same gate is waiting again. From his side the office ignores him.

The gate in question is the one only he can close — the k3s deploy and the
Cloudflare DNS for `zollybot.shifaul.dev`. Both of those are already done in
reality: I verified today that `POST https://zollybot.shifaul.dev/` with the
configured secret returns `200 {"ok":true}`, that the listener is on
`127.0.0.1:8082`, and that the k3s deploy is recorded as verified in the
result file. The office cannot record that because the approval path is
broken.

## Suggested scope for a fix

- `tool/src/tg/handlers.rs`: point `pending_gates` at the results directory
  (with the branch-copy rule honoured), and use the brief's predicate.
- `tool/src/tg/handlers.rs`: `handle_approve` / `handle_decline` must write
  `approved: yes` / `status: escalated` in the format `result-contract.md`
  prescribes, into the file the brief will actually read.
- Add a test that reproduces the exact state in this report: one result file
  with `status: done` + `gate: human`, an empty `.agents/queue/`, then assert
  that `handle_approve` clears it and that `brief.gates` drops to empty.
  The current test suite has no coverage for the approve path against the
  results directory.

Note the repo also has an uncommitted change in `tool/src/herdr.rs` (a
`launch_args` parameter threaded through `agent_start`). Keep that intact and
land the fix without sweeping it in.
