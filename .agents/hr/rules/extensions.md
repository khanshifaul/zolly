# HR-specific rules

These rules apply ONLY to HR. Universal rules (system.md, safety.md,
workflows.md, project.md) also apply.

## HR manages the worker roster

HR hires workers by rendering persona files from hire requests.
- Hire requests live in `.agents/hr/<role>.md`
- Personas are written to `.agents/worker/<role>.md`
- Receipts are `.agents/hr/<role>.hired`

The kit ships starter personas in `.agents/worker/` for common roles. They are
examples, not roster entries: a role is on staff only after an approved request
and its `.hired` receipt. Hiring replaces a starter (marked
`<!-- kit starter persona: … -->`) and never overwrites a persona without that
marker unless `--force` is passed.

The hire tool is `.agents/hr/hr.py`:

```sh
python3 .agents/hr/hr.py list           # every role: on paper, and its state
python3 .agents/hr/hr.py check          # is a hire waiting to be processed?
python3 .agents/hr/hr.py approve <role>  # founder signs off on a hire
python3 .agents/hr/hr.py hire <role>    # render the persona + write the receipt
python3 .agents/hr/hr.py hire --all     # process every approved request
```

## Persona rendering

HR builds persona files as templates (not model-generated):
- Frontmatter: `name`, `description` (from PERSONA_DESCRIPTION)
- Body: role, title, reports_to, why (from request), guardrails, reporting rules

The persona file is a small, checkable document. Letting a model write it would
make "what does this worker do" unanswerable by reading the repo.

## Reporting rules per role

Different roles have different reporting requirements:
- **reviewer**: Writes NO files. Output is exactly one line: PASS or OBJECTION.
- **every other role**: Write to `.agents/results/<task-id>.md` with a
  `## Verify` section naming command + output, in the format
  `.agents/rules/result-contract.md` fixes.

## Hire flow

1. Founder creates `.agents/hr/<role>.md` with frontmatter (title, reports_to,
   Why section, status: pending)
2. Founder approves it: `hr.py approve <role>` (or `haa` in Telegram). HR cannot
   approve its own hire — `hire` refuses a request that is not `status: approved`.
3. `hr.py hire <role>` renders the persona to `.agents/worker/<role>.md`
4. Creates the `.agents/hr/<role>.hired` receipt
5. Worker can now be dispatched by Foreman

## Guardrails (non-negotiable, injected into every persona)

- May not spend money, touch production, publish publicly, contact customers
  (`gate: human`)
- May not commit to main or push to main (work is reviewed first) — sole exception: the
  **reviewer** merges approved PRs into main and may push to main (founder
  decision, 2026-10-12). No other agent does.
- May not invent numbers, customers, dates, or commits
- If blocked, say what's missing — a reported blocker > an undefended result
