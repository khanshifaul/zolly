# Issue Contract

How an agent files a GitHub issue. An issue is a **public report**, not an
office task and not a queue item. Internal work stays in `.agents/queue/`.

The shape matters because an issue is read by people who do not have the
office: no context from the chat, the worktree, or the session that produced
it.

## Automatic zolly reports

There is one exception to the no-autonomous-issue rule: when the installer's
contribution policy is approved (`contribute = true` in
`.agents/zolly/config.toml`), the `zolly` runtime may create a public issue in
`khanshifaul/zolly` for a defect in zolly itself. It reports only the zolly
version, command name, OS/architecture, timestamp, and sanitized error or
internal self-check detail. The reporter never sends project files, command arguments,
prompts, environment variables, credentials, or customer data, and it
deduplicates and rate-limits reports.

That is the runtime's job, not an agent's. An agent must not hand-create an
issue for the same error; if a report was skipped, say why (no `gh`, no auth,
rate limit) and let the founder decide.

## When to file

File only when all three are true:

1. The report belongs to a GitHub repository, not to this office's queue.
2. It describes one problem: a reproducible defect, a missing capability with a
   concrete use, or a question that needs a durable public record.
3. The founder has named the issue creation, or the task brief carries
   `gate: human` and that gate is approved.

A public issue is publishing. Do not create one on your own initiative. Prepare
the body and ask; a direct founder instruction naming the issue is the
approval. Never file an issue for an internal office task, a secret, a customer,
or a production incident whose details are not already public.

## Before creating

Search both issues and pull requests first:

```sh
repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
gh issue list --repo "$repo" --state all --search "<distinctive words>" --limit 20
gh pr list   --repo "$repo" --state all --search "<distinctive words>" --limit 20
```

If an open issue already covers it, comment only when the new evidence is
material; otherwise report the existing issue URL. Do not open a duplicate.

Check the target repository. For the zolly kit it is `khanshifaul/zolly`; for an
installed project, use the project's own GitHub remote. Never file a project
issue in the kit repository by accident.

## Body shape

Use a body file, never a long `--body` argument:

```sh
body=$(mktemp)
cat >"$body" <<'EOF'
## Problem

One paragraph. What is wrong, for whom, and why it matters.

## Reproduction

The smallest sequence that shows it. Exact command, input, or URL.
`none` is valid for a capability request.

## Expected

What should happen.

## Actual

What happens instead. Include the error text, not a paraphrase.

## Scope

What the fix may touch. Name the files, service, or API if known.
Write `unknown` when it is not known; do not invent a scope.

## Evidence

Commands, versions, logs, or links. Redact tokens, customer data, and private
paths before pasting.
EOF

gh issue create --repo "$repo" \
  --title "<short, specific title>" \
  --body-file "$body"
rm -f "$body"
```

Use only labels that already exist and that the founder named. Do not create a
label, milestone, assignee, or project card as a side effect. If no label is
named, create the issue without one and say so.

## Rules

- One issue, one problem. Split unrelated defects.
- The body must stand alone after the chat is gone.
- Never include a token, cookie, private key, customer record, private URL, or
  an unredacted log line.
- Personal facts do not belong in an issue. Say what is wrong, not who reported
  it.
- Do not close an issue from a PR unless the change is verified to fix it;
  otherwise use `Refs #<n>`. `Closes #<n>` is a promise, not a convenience.
- Record the issue URL in the task result, and report it through `zolly bus`
  (`radio pm` first, herdr fallback) when the task expects a report.
- If `gh` is unavailable or unauthenticated, stop and report that. Do not use
  the web UI as a fallback unless the founder names that action too.
