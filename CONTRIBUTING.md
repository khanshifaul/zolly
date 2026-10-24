# Contributing to zolly

Issues and pull requests are welcome. Open an issue before sending a large
change so the design can be agreed first.

## Automatic issue reports — opt-in only

The installer asks separately whether this office may send automatic reports to
the zolly repository, `khanshifaul/zolly`. The default is **off**. `--yes` does
not turn it on; only an explicit interactive approval or `--contribute` does.

When enabled, zolly may create a public GitHub issue containing:

- the zolly version;
- the command name (`doctor`, `close`, `verify`, …);
- OS and architecture;
- a timestamp;
- a sanitized error or internal failure detail.

It never sends command arguments, task briefs, prompts, project files, working
directories, environment variables, tokens, cookies, or customer data. Home and
office paths are replaced before anything is sent. Reports are deduplicated by
a local fingerprint, capped at five per day, and skipped when `gh` is missing or
not authenticated.

Create public issues only for defects in zolly itself — not for a project's
configuration, test failure, missing tool, or expected refusal. The full rule
is `.agents/rules/issue-contract.md` in an installed office.

Turn it off at any time by setting `contribute = false` in
`.agents/zolly/config.toml`, or re-run the installer with `--no-contribute`.
Turning it off does not delete issues already created.

Automatic reporting is a publishing action. Do not enable it where the error
text itself is confidential. A report is public the moment `gh issue create`
succeeds.
