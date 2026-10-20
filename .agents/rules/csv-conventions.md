# CSV Conventions

Shared rules for every CSV handed over in this office. Every worker that writes
one loads this file.

A CSV is a **handover format**: the founder reads it, edits it, and acts on it.
It is not a scratch file, and it is not a database.

## Publish-state vocabulary

Every handover CSV's `status` column uses these values, in this order:

| Value | Meaning |
|-------|---------|
| `drafted` | Worker wrote the row. Founder has not reviewed. |
| `approved` | Founder reviewed and approved. Ready to act on. |
| `sent` | Founder sent the message or published the content. |
| `rejected` | Founder reviewed and declined. Do not re-draft. |
| `done` | Action completed (e.g. asset generated, article published). |

Workers only write `drafted`. The founder moves rows forward. No worker reads
`approved`, `sent`, `rejected`, or `done` to decide what to do next — workers
only skip rows that are not `drafted`.

## Handing a CSV over

The founder reviews and edits a CSV **in place, on the branch** — in the web
interface of the repository, or in whatever editor they use. The worker commits
`drafted` rows; the founder edits `status` and any post-publish columns and
commits back.

That means:

- The CSV must be readable **without** running the project's code. No build
  step, no script, no generated intermediate file.
- Column headers are stable text on the first row, so the review interface can
  parse them.
- One row per item, no merged cells, no blank separator rows inside the table.
- Encoding is UTF-8. Line endings are LF.
- The file is committed on the task branch, so the founder reviews exactly what
  the worker wrote.

Post-publish columns are **project-specific**: `rules/project.md` names them for
this project. Workers never write them; the founder fills them after the action.

## Worker output columns

Every worker's CSV has these columns. Workers write to them; the founder may
update them after the fact.

| Column | Who writes | When |
|--------|-----------|------|
| `status` | Worker writes `drafted`; founder moves it forward | On draft, on review |
| `date_drafted` or `date_created` | Worker | On draft |
| `date_published` or `date_posted` or `date_launched` | Founder | After publishing |
| `notes` | Worker and founder | Any time |

## Data source rule

A worker only writes a column if it has a data source for it. If the worker
cannot measure or observe the value, the column does not exist in that worker's
CSV. Never invent a value. Never leave a column in the schema that the
worker's tools cannot fill.

## Cross-worker coordination

Workers that act on the same subject read the shared contact log before
drafting for anyone. The log's path is project-specific and named in
`rules/project.md` → **Where things live**; a worker that cannot find it asks
once rather than drafting blind. The log records what has been sent to whom and
when. If a subject already has a pending draft from another worker, the new
worker notes the conflict in `notes` and does not draft — the founder resolves
it.

The shared log's columns: `customer_name`, `date`, `channel`, `worker`,
`action`, `status`, `notes`.

## Shared IDs

Where two CSVs need to join, they share an ID column. If the strings do not
match exactly, the worker logs the mismatch in `notes` and skips — it does not
fuzzy-match or invent a join.

## Never

- Never put a secret, a token, or a password in a CSV. Record **where** the
  secret lives, in the repo's secret store, and put only the key name here.
- Never overwrite a row the founder has moved forward. Append; never rewrite a
  row whose `status` is no longer `drafted`.
- Never delete rows. A row that is no longer wanted becomes `rejected`.
