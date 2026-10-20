# The founder

Committed memory, so every agent reads the same person rather than guessing.
Source of truth for name, language, and how to address them. Edit through this
file, not through code.

## Who

**{{FOUNDER}}.** Founder and owner of {{PROJECT_NAME}}. The installer fills
the name; everything below it is yours to correct.

If you do not know the answer to something in this file, do not guess it —
write "not logged" and ask once. A wrong founder file is worse than an empty
one: every agent will read it and act on the fiction.

## How to address them

**Address them as `{{FOUNDER}}`.** The installer fills the short, spoken form
here. Use it when greeting, when answering directly, and at the start of a
brief. Drop it in running text; repeating it in every paragraph reads as a
formality and is not warmth.

Not the full name in every sentence, not a title ("Mr", "Sir", "Dr" — formal
distance between an assistant and the person it works for), and not a job
description ("the founder" is a job title, not a name — an agent that says
"the founder" is talking about them in the third person to their face).

TODO: if the short form is not the same string the installer filled in, replace
the line above with the actual short form, and say what the surname is, so an
agent does not have to guess which part to drop.

Legal documents and external signatures keep the formal form: the full name,
or the name as a counterparty would write it. That is a different register, and
this file does not override it.

## Language

TODO: the language they read and write in.

The rule, whatever the language is: prose in the language they use; numbers,
code identifiers, file paths, commit hashes, SQL and product names stay in
their original form and are never translated.

If they ask for a different language later, change it here — one line — and
every agent picks it up.

## How they read reports

Recorded behaviour, not a guess:

- They want **the exact defect and the row count**, not a characterisation.
- They push back on wording that overstates, but act on the number the moment
  they are given it.
- They **verify what actually reaches them**, not what the logs claim.
- They ask direct operational questions mid-run rather than waiting for a
  report.

## What this file is not

Not a personality to perform, and not permission to flatter them. The voice
traits live in `.agents/memory/personality.md` and are adopted only from
recorded, consistent evidence. A warm tone is not a licence to invent good
news.
