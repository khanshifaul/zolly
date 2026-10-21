# prompts/curiosity.md — the idle inquiry

The office is never idle in the sense of "nothing to know". When the queue is
empty, find the next decision with evidence — do not invent work to look busy.

## Trigger

Run only when **all** are true:

- `zolly brief --json` shows no active workers and no waiting gate.
- `zolly curious --json` shows `queue_pending == 0`.
- no instruction from the founder is in flight.

If any work exists, stop: the queue is the priority. This pass is for the gap,
not for displacing work.

## Gather — research is pre-approved

- `zolly curious --json` (queue, dirty files, recent commits, TODOs,
  `project.md` truths, open questions).
- Read the files named in `code_todos`, `dirty`, and the last commits.
- `OPEN-QUESTIONS.md`, `memory/decisions.md`, `memory/learnings.md`.
- `herdr agent list` — check whether a seat is already on this question.

Read-only investigation does not need the founder. You may commission it:

```sh
zolly foreman "research: <question>; read-only; write only .agents/results/<id>.md"
```

with `gate: auto`, and foreman may use a worker. A research task may read code,
run tests, search the web, and write its result file on a branch. It may not
change product code, spend, publish, contact a customer, or touch production.
Those stay `gate: human`.

## Ask

At most **3** questions, grouped by category:

1. **Business & domain logic** — an unwritten rule the code/tests do not state.
2. **Architecture & design** — a boundary, state owner, or failure mode that is
   implicit.
3. **Technical debt & safety** — a real bottleneck, missing check, or TODO with
   no plan.
4. **Operations & environment** — secrets, flags, monitoring, or deploy truth.

Every question has all three:

- **Observation:** `file:line` or a measured signal. A hunch is not an
  observation.
- **Question:** one decision, not a survey.
- **Why it matters:** the risk or cost of leaving it for another week.

Skip anything the tree already answers. Skip anything in `OPEN-QUESTIONS.md` or
`memory/decisions.md`. If nothing survives, say so in one line and name what you
checked.

## Speak to the founder

The founder may be context-switching and has ADHD. That is a design constraint,
not a joke.

- One decision per message. Five lines maximum.
- Plain words, no jargon, no file paths unless the path is the decision.
- Two options and a recommendation; say what happens if we do nothing.
- Label it `🔍 idle inquiry` so it is not mistaken for a status report.
- Never make them read a report, remember yesterday's task, or choose between
  six things.

## After

- Append each unresolved decision to `OPEN-QUESTIONS.md`, one line, exactly as
  asked.
- Record a learning only if it changes a future decision:
  `.agents/shared/memory remember learning "..."`.
- Never ask the same question twice. If the founder declined, it is settled.
