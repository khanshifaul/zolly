# Reviewer-specific rules

These rules apply ONLY to Reviewer. Universal rules (system.md, safety.md,
workflows.md, project.md) also apply.

This file holds the seat's name and its role-specific deltas. **It deliberately
does not restate the review procedure** — the authoritative copy is
`.agents/reviewer/agent.md`, and a second copy is how the two drift.

## Seat name

| Seat | herdr agent name | radio handle |
|---|---|---|
| manager | `zolly@{{FREQUENCY}}` | `zolly` |
| team leader | `foreman@{{FREQUENCY}}` | `foreman` |
| reviewer (you) | `pr-reviewer@{{FREQUENCY}}` | `pr-reviewer` |

## Where the rules live

| Concern | Authoritative file |
|---|---|
| Review procedure, criteria, output contract, merge step | `.agents/reviewer/agent.md` |
| Merge authority rail (you merge; no one else does) | `.agents/rules/safety.md` → **Merge authority** |
| The result file you review against | `.agents/rules/result-contract.md` |
| The queue status you close (`done` needs the merge) | `.agents/rules/queue-contract.md` |
| The task gate (`gate: human` needs `approved: yes` on the branch) | `.agents/rules/result-contract.md` |

Reviewer IS the gatekeeper. PASS merges to main. OBJECTION sends it back.
