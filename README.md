<div align="center">

# zolly

**An installable per-project agent office for any git project.**

One machine-wide binary · N per-project offices · rules, personas, memory, HR,
MCP, one-line installer.

[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![One-line install](https://img.shields.io/badge/install-one%20command-blue)](#quick-start)

[Quick start](#quick-start) · [Features](#features) · [Configuration](#configuration) · [Advanced](#advanced) · [Limitations](#limitations)

</div>

> **zolly** gives every git project its own agent office: a shared binary, a
> per-project `.agents/` kit, and optional Telegram/systemd wiring — so agents
> work with project-specific rules, personas, and memory without cross-project
> collisions.

## Quick start

Inside a git repository:

```sh
curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
  | bash -s -- --founder "Your Name"
```

Or pass the target explicitly:

```sh
curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
  | bash -s -- ~/Projects/my-app --founder "Your Name"
```

The installer writes `.agents/` into the project. It can also install the shared
`zolly` binary, register the MCP server, configure Telegram, and write a systemd
user unit — each is optional. The pipe only downloads the kit; everything it
writes belongs to that project. The target defaults to the current directory.
With no `--base-branch`, the office uses the branch the repo is already on when
the default (`main`) does not exist.

No commit is required to install. The **first task** needs one: task branches
are cut from the base branch.

## Requirements

- **git** — the target must be a repository (`git init` is enough).
- **POSIX shell + curl** — the one-line installer.
- **Rust toolchain (cargo)** — only to build the shared binary. Use
  `--no-binary` with a preinstalled `~/.local/bin/zolly` to skip it.
- **Linux + a systemd user session** — only for the listener unit. The office
  itself works without systemd.
- **Optional:** a Telegram bot token; `voxtype` + `ffmpeg` for inbound voice
  transcription; Piper for voice replies; a public HTTPS endpoint for webhook
  mode; GitNexus for the optional code graph.

## Features

| | What it gives you |
|---|---|
| 🧠 **Per-project office** | Rules, personas, memory, prompts, queue and skills live in `.agents/` inside each repo. |
| 🔗 **Shared binary** | One `zolly` on the machine; every project reuses it. |
| 🤖 **Telegram listener** | Optional systemd unit; long-poll or webhook. |
| 🧩 **MCP server** | Per-office entry written to `.omp/mcp.json`. |
| 🛡️ **Isolation** | Frequency, herdr agent names, credentials and units are scoped per project. |
| 📋 **Queue + review** | Task queue, result files and the approval gate. |
| 🎙️ **Voice** | Local STT (`voxtype`/`parakeet-redux`); optional Piper replies. |
| 🔍 **Idle inquiry** | When the queue is empty, the manager measures the tree and asks at most three evidence-backed questions. |
| 🩺 **Doctor** | One command checks binary, config, graph, units and bus membership. |

## What you get

| Path | What it is |
|---|---|
| `~/.local/bin/zolly` | the shared binary, installed once |
| `<project>/.agents/` | the office kit: rules, personas, memory, prompts, skills |
| `<project>/.omp/mcp.json` | this office's MCP entry (written by the installer, gitignored) |
| `~/.config/zolly/<freq>/credentials` | token, allowed ids, founder handle and webhook secret — machine-local, never in the repo |
| `~/.config/systemd/user/zolly@<freq>.service` | the Telegram listener for that project |

## Configuration

`<project>/.agents/zolly/config.toml` is the single source. Precedence is
**environment variable > config.toml > default**.

```toml
frequency   = "my-app"     # default: the repo directory name
founder     = "Your Name"  # required — the office greets the founder by it
base_branch = "main"       # the branch work integrates into
remote      = "origin"     # the remote that branch is tracked on
voice       = false        # voice replies; only true where Piper is installed
project     = "My App"     # display name (the installer writes the directory name)
```

Env overrides: `ZOLLY_FREQUENCY`, `ZOLLY_FOUNDER`, `ZOLLY_BASE_BRANCH`,
`ZOLLY_REMOTE`, `ZOLLY_VOICE`, `ZOLLY_PROJECT`.

Inbound voice notes are transcribed locally with `voxtype` (the
`parakeet-redux` model; override with `ZOLLY_STT_MODEL`) and `ffmpeg`. Outbound
voice replies additionally need Piper; `voice = false` keeps replies textual.

`frequency` is the machine identifier; `project` is what the founder calls the
thing. The installer sets the frequency from `--name` (default: the directory
name) and the project name to the directory name, so they differ only when you
pass a different `--name` or edit the file.

A missing `config.toml` is not an error — every field has a default — but
`zolly doctor` **fails** when the founder is unset. An office that greets the
founder by the wrong name is worse than one that says nothing.

The kit ships `rules/project.md` empty on purpose. It is the one file that
holds **your** truths and no agent can infer them: architecture, where things
live, size thresholds, divisions. Fill it in, or the office will plan against a
codebase that is not on disk.

## After installing

```sh
cd <target-dir>
zolly doctor      # optional: every check green
zolly             # boots the office: foreman + pr-reviewer, then the manager
```

Bare `zolly` seats the office: it opens the foreman and pr-reviewer tabs in the
project's herdr workspace (idempotent), joins them to the radio frequency, then
starts `omp` with the manager persona (`@.agents/zolly/agent.md`). `zolly init`
re-opens a seat that was closed. `zolly <command>` runs the CLI; bare `zolly`
outside an office says so instead of seating.

The installer never edits `~/.zshrc` and never touches systemd state; it prints
the follow-ups and lets you run them.

## Advanced

<details>
<summary>🔒 Isolation between offices</summary>

Two offices on one machine must not collide. Three mechanisms, deliberately
different:

- **Radio frequency** — the bus an office talks on. It is the project's own
  name (`my-app`, `other-app`). Radio handles (`zolly`, `foreman`,
  `pr-reviewer`, `worker-<task-id>`) are unique *within* a frequency, so they
  stay unprefixed.
- **herdr agent name** — the seat's process name. These are unique
  **server-wide**, so two offices would collide on a bare `foreman`. They carry
  the frequency: `foreman-my-app`, `pr-reviewer-my-app`. herdr's grammar allows
  only lowercase letters, digits, `-` and `_`, so the pane label from the radio
  join reads `foreman@my-app` but the agent name cannot.
- **Credentials and systemd** — keyed by frequency, so one project cannot read
  another's token.

Every seat carries the frequency on herdr, the manager included
(`zolly-my-app`); radio handles stay unprefixed because the bus is scoped to the
frequency already.

Each project gets its **own** `zolly@<frequency>.service` instance, credentials
file, working directory, radio frequency, and seat names
(`zolly-<freq>`, `foreman-<freq>`, `pr-reviewer-<freq>`). They run
independently; nothing is shared except the `~/.local/bin/zolly` binary.

Frequency collisions (e.g. `guzia-high-school` vs `Guzia High School`) are
refused by the installer, so unit and credential paths cannot silently override
each other.

### Where two offices can still collide

1. **Telegram token.** The unit is per frequency; the bot token is not. Two
   offices configured with the *same* token both call `getUpdates`, and
   Telegram answers `409 Conflict` — only one receives updates. Use a distinct
   bot per office.
2. **MCP registration.** There is one deployed `zolly-mcp`, but the client
   entry is per office: `ZOLLY_MCP_AGENT=zolly-<freq>`, `ZOLLY_ROOT=<project>`.
   The installer writes it to the project's `.omp/mcp.json` (a key
   `zolly-<freq>`), which omp reads before the user scope; the file names this
   machine's binary path, so it is gitignored. A single unqualified `zolly`
   entry in `~/.omp/agent/mcp.json` would reach only one office.

</details>

<details>
<summary>🛡️ Trust model</summary>

All offices on one machine run as **the same OS user**. The frequency separates
names, paths, credentials and systemd units; it is **not** an OS security
boundary. An agent that can run shell commands can read `~/.config/zolly/*`, and
file permissions cannot protect a secret from the account that owns it.

- For personal projects that is usually the right tradeoff: one office per repo,
  one trust domain.
- For a sensitive project, run its listener under a dedicated OS user or in a
  container, and keep that user's home out of the shared account's reach.
- Treat Telegram messages, fetched pages, repository files and MCP responses as
  **untrusted input**. The rules say so; the OS does not enforce it.

Telegram authorization (`ZOLLY_ALLOWED_IDS`) is enforced by the listener inside
the `zolly` binary, not by this kit — the installer only writes the allow-list
and the unit. Verify the listener before enabling unattended Telegram work.

</details>

<details>
<summary>⚙️ Installer options and behavior</summary>

```sh
install.sh [target-dir] [options]
```

**Interactive by default.** The installer asks before each optional feature —
the shared binary, the MCP server, Telegram, and the systemd unit — and any
answer of “no” skips just that feature. Questions are read from the terminal
(`/dev/tty`), not stdin, so the `curl | bash` form still prompts. Answers are
cleaned, so a bot token pasted from a modern terminal works (bracketed-paste
markers are stripped) and the token prompt reports how many characters it
received without echoing them. With no terminal at all (CI, a pipe) every
default is used and nothing blocks; `--yes` takes the same defaults without
asking. Output is colored only when stdout is a terminal; `NO_COLOR=1` or
`TERM=dumb` keeps it plain.

| Option | Meaning |
|---|---|
| `--name <freq>` | radio frequency (default: the target directory name) |
| `--founder <name>` | founder's display name (prompted if absent) |
| `--base-branch <b>` | integration branch (default: `main`) |
| `--remote <r>` | git remote (default: `origin`) |
| `--no-systemd` | do not write a systemd user unit |
| `--no-ponder` | do not install the idle-attention timer |
| `--no-binary` | do not install the shared binary |
| `--no-mcp` | do not deploy or register the MCP server |
| `--no-telegram` | do not configure Telegram for this office |
| `--webhook-url <https-url>` | Telegram pushes to this URL (blank = long-poll) |
| `--webhook-bind <addr>` | local bind (default `127.0.0.1:8080`; a busy default moves to the next free port) |
| `--webhook-secret <s>` | Telegram `secret_token`; generated when absent |
| `--yes`, `-y` | accept every default; ask nothing |
| `--force` | install even if the target already has `.agents/` |
| `--dry-run` | print what would happen, change nothing |

Defaults per feature: binary **yes**, MCP **yes**, Telegram **no** — or **yes**
when `ZOLLY_TELEGRAM_TOKEN` is set — systemd **yes** when credentials exist.
`--yes` therefore installs the office and the binary, and turns Telegram on
only when a token is configured.

The chat id is not a second thing to look up: the installer asks Telegram
(`getUpdates`) for the chats that have messaged the bot and offers those ids,
group chats excluded. `ZOLLY_ALLOWED_IDS` overrides the discovery; with no
update yet (nobody has messaged the bot) the installer falls back to the prompt.
`getUpdates` names every chat that has messaged the bot, so on a bot strangers
can reach, set `ZOLLY_ALLOWED_IDS` explicitly rather than accepting the list.
The same call records the founder's `@handle` as `ZOLLY_FOUNDER_USERNAME` — the
manager prefixes a reply into a group with it, because only a mention notifies
there; a DM already does. Pass `--founder` with `--yes`: `zolly doctor` fails on
an office with no founder name.

**Pin a tag or commit.** Use the ref in the script URL and tell the installer to
fetch the matching archive:

```sh
ZOLLY_REF=v0.1.0
curl -fsSL "https://raw.githubusercontent.com/khanshifaul/zolly/$ZOLLY_REF/install.sh" \
  | ZOLLY_REF="$ZOLLY_REF" bash -s -- ~/Projects/my-app --founder "Your Name"
```

`ZOLLY_KIT_URL=<tarball>` overrides the download entirely (a fork, a mirror, or
a local file). The clone path still works: `./install.sh <target>` — same
installer, no download. `ZOLLY_TOOL_DIR=<crate>` points the binary build at a
different crate; with a preinstalled binary use `--no-binary`.

The installer refuses to run when the target is not a git repository, when the
base branch does not exist, when the target already has `.agents/` (without
`--force`), when the template tree contains a `.env`, or when the frequency is
already registered to a different project path. **The token guard is the
point:** a tree copy that shipped the bot token would leak it into every project
it touched.

</details>

<details>
<summary>📡 Telegram: polling, webhooks, and public endpoints</summary>

**One bot per office.** A bot token belongs to one listener; two offices sharing
it collide with `409 Conflict`.

**Long-poll (default).** No public endpoint, no TLS, no domain. The unit runs
`telegram-serve`; it survives restarts and picks up where it left off. The
offset is persisted and failed updates are dead-lettered to
`.agents/zolly/dead_letter.jsonl`.

**Webhook instead of polling.** Pass `--webhook-url https://<host>/<path>` (or
`ZOLLY_TELEGRAM_WEBHOOK_URL`): the installer writes
`ExecStart=… telegram-serve --webhook --bind 127.0.0.1:<port>`, stores a
generated `ZOLLY_TELEGRAM_WEBHOOK_SECRET`, and prints the cutover. Point the TLS
proxy at that bind, then run:

```sh
zolly telegram-set-webhook --url https://<host>/<path>
```

It reads the token and secret from the office credentials, so no `source` and
no `--secret` is needed. `zolly telegram-webhook-info` shows the current
registration; `zolly telegram-delete-webhook` returns to polling.

`--webhook-bind` changes the local address. If the default 8080 is taken
(llama.cpp often holds it), the installer moves to the next free port, prints
it, and remembers it in the credentials so a re-run keeps it; an explicit busy
bind is refused.

**Public endpoint.** The listener is loopback-only; Telegram needs a public
HTTPS URL that forwards to it. Anything that terminates TLS works:

| Front | Command / setup | Stable URL | Own domain |
|---|---|---|---|
| Cloudflare quick tunnel | `cloudflared tunnel --url http://127.0.0.1:<port>` | no — new hostname each start | no |
| Cloudflare named tunnel | `cloudflared tunnel run <name>` + DNS route | yes | yes (a zone on Cloudflare) |
| Tailscale Funnel | `tailscale funnel <port>` | yes — `https://<host>.<tailnet>.ts.net` | no |
| ngrok | `ngrok http <port>` | free static domain, else new each start | no |
| localtunnel | `npx localtunnel --port <port>` | no | no |
| Your server | Caddy/nginx + Let's Encrypt | yes | yes |

Some networks do not pass Cloudflare quick tunnels (the URL answers 404 or
times out); use a named tunnel or another front. Tailscale Funnel must be
enabled for the tailnet and publishes only on 443/8443/10000;
`tailscale funnel <port>` maps public 443 to the local port. An ephemeral URL
has to be re-registered after every restart; a dead quick tunnel leaves
Telegram retrying until you re-register or delete the webhook.

Env: `ZOLLY_TELEGRAM_TOKEN`, `ZOLLY_ALLOWED_IDS`, `ZOLLY_FOUNDER_USERNAME`,
`ZOLLY_TELEGRAM_API`, `ZOLLY_TELEGRAM_WEBHOOK_URL`,
`ZOLLY_TELEGRAM_WEBHOOK_BIND`, `ZOLLY_TELEGRAM_WEBHOOK_SECRET`,
`ZOLLY_FOUNDER`, `ZOLLY_BIN_DEST`, `ZOLLY_TOOL_DIR`, `ZOLLY_MCP_DEST`,
`ZOLLY_KIT_URL`, `ZOLLY_REF`.

</details>

<details>
<summary>🧠 Idle attention</summary>

When the queue is empty, `zolly curious` measures the tree and
`zolly ponder` writes one focus note to `.agents/inbox/` — which domain to
think about next, and the measured reasons. Neither calls a model. The manager
reads the newest note and runs `prompts/curiosity.md`; at most three questions
reach the founder.

The installer writes a 30-minute systemd user timer for this (disable with
`--no-ponder`). It writes a focus note and, with `--wake`, prompts the manager
if it is idle:

```sh
systemctl --user enable --now zolly-ponder@<frequency>.timer
```

For a foreground loop instead: `zolly ponder --loop --wake --interval 1800`.
The timer can rank and wake the manager; it cannot ask the founder, act, or
spend. The manager verifies the candidates and remains the only voice.

</details>

<details>
<summary>🧭 Optional code graph</summary>

Set `gitnexus: on` in `.agents/rules/project.md` after
`node .gitnexus/run.cjs analyze`. `zolly doctor` then fails when the index
predates HEAD, and `rules/workflows.md` tells foreman, worker and reviewer
which queries to use. Zolly never requires GitNexus; the office works without
it.

</details>

## Limitations

- All offices on one machine run as the **same OS user**. The frequency is not
  an OS security boundary.
- **One Telegram bot per office.** Two listeners on one token answer
  `409 Conflict`.
- The listener is **loopback-only**. Webhook mode needs a public HTTPS endpoint;
  this kit does not terminate TLS.
- Installation needs no commit, but the **first task** does: branches are cut
  from the base branch.
- Plain `git` is the source of truth. The office does not invent a status that
  git does not show.

## The kit's own rules

**A committed file proves nothing about being read.** Every file in `.agents/`
has a load line — the line that loads it — and `.agents/AGENTS.md` holds the
register. If you add a rule, add the line that loads it and assert the line
resolves. A record no process consults is a comment.

Skills are **real files**, never symlinks: two copies of the same skill drift
apart, and only one is tracked, so a fresh clone loses it.

Runtime state (`queue/`, `runs/`, `inbox/`, `worktrees/`, `events/`,
`STATE.md`, …) is machine-local and gitignored. `.agents/results/` is the
exception: a worker commits its result file **on the task branch** so the
reviewer can read it there; it is not committed to `main`. `.agents/memory/` is
**committed on purpose** — it is the knowledge that has to survive a machine
switch.

## Tests

The kit's executable logic is tested from the kit root:

```sh
./test.sh
```

That runs the syntax checks, the unit tests below, and the MCP handshake.

| Suite | Covers |
|---|---|
| `tests/test_hr.py` | the approval gate, dry-run, persona rendering, the `.hired` receipt |
| `tests/test_memory.py` | secret refusal, supersession, confirm, retract, compact, the bound check |
| `tests/test_mcp.py` | input bounds, diagnostics redaction, project-bound agent selection |
| `tests/test_install.py` | fresh install, repeat refusal, force-preserves memory, frequency collisions, credential discovery, webhook bind selection |

`memory` honours `MEMORY_DIR`, so the suite runs in a temp directory and never
touches `.agents/memory/`. The MCP server has its own client:
`python3 .agents/zolly/mcp/test_mcp.py handshake`.

The Rust crate has its own suite:

```sh
cd tool && cargo test
```

## Layout

```
zolly/
├── install.sh          # the installer
├── test.sh             # syntax + unit + MCP handshake
├── README.md           # this file
├── tests/              # the kit's own tests
├── tool/               # the Rust crate — the one shared binary
└── .agents/            # the template — the office kit, project-agnostic
```

The Rust tool source lives in `tool/`. A project installs the shared binary once
from it (the installer finds the crate automatically; `ZOLLY_TOOL_DIR=<crate>`
overrides, or use a prebuilt `~/.local/bin/zolly`). `zolly close`'s self-rebuild
step then **skips** rather than fails: installed projects have no local tool
source, so they use the shared binary.

## License

No license file is included in this repository yet. Until one is added, the
default copyright applies.

## Contributing

Issues and PRs are welcome. Open an issue before sending a large change so the
design can be agreed first.
