# zolly — an installable agent office for any git project

One machine-wide binary, N per-project offices. The binary is built from the
Rust tool source, which is distributed separately from this kit; every project
on the machine consumes the same binary plus a small per-project skeleton.

```sh
./install.sh ~/Projects/my-app --founder "Your Name"
```

That writes `~/Projects/my-app/.agents/` — the office kit — and prints the
follow-ups it will not do for you (Telegram credentials, the systemd unit, the
seat command).

## Install with one command

```sh
curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
  | bash -s -- ~/Projects/my-app --founder "Your Name"
```

Run it **inside** a project and omit the path:

```sh
cd ~/Projects/my-app
curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
  | bash -s -- --founder "Your Name"
```

The target defaults to the current directory. The pipe only downloads the kit;
everything it writes belongs to that project: `.agents/`, the frequency (the
directory name), `~/.config/zolly/<freq>/credentials`, and the systemd unit. A
second project gets a second office; the binary is the only machine-wide piece,
installed once.

The target must be a git repository. `git init` is enough — no commit is
required:

```sh
mkdir -p ~/Projects/my-app && cd ~/Projects/my-app
git init -b main
curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
  | bash -s -- --founder "Your Name"
```

A directory that is not a repository is refused and left untouched. If the repo
is on `master`, the installer uses `master` unless you pass `--base-branch`.

Installation needs no commit. The **first task** does: task branches are cut
from the base branch, so make the project's first commit when there is
something to commit.

To pin a release, use a tag in the script URL and tell the installer to fetch
the matching archive:

```sh
ZOLLY_REF=v0.1.0
curl -fsSL "https://raw.githubusercontent.com/khanshifaul/zolly/$ZOLLY_REF/install.sh" \
  | ZOLLY_REF="$ZOLLY_REF" bash -s -- ~/Projects/my-app --founder "Your Name"
```

The command installs the office **and** the shared `zolly` binary. On a machine
without the Rust tool source (or a preinstalled binary), add `--no-binary`; the
office skeleton still installs, and the binary can be added later with
`ZOLLY_TOOL_DIR=<crate> ./install.sh ...`.

`ZOLLY_KIT_URL=<tarball>` overrides the download entirely (a fork, a mirror, or
a local file). The clone path still works: `./install.sh <target>` — same
installer, no download.

## What you get

| Path | What it is |
|---|---|
| `~/.local/bin/zolly` | the shared binary, installed once |
| `<project>/.agents/` | the office kit: rules, personas, memory, prompts, skills |
| `~/.config/zolly/<freq>/credentials` | the Telegram token, allowed ids and the founder's handle — machine-local, never in the repo |
| `~/.config/systemd/user/zolly-<freq>.service` | the Telegram listener for that project |

## Isolation between offices

Two offices on one machine must not collide. Three mechanisms do that, and they
are deliberately different:

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

### Where two offices can still collide

1. **Telegram token.** The unit is per frequency; the bot token is not. Two
   offices configured with the *same* token both call `getUpdates`, and
   Telegram answers `409 Conflict` — only one receives updates. Use a distinct
   bot per office, or run one listener and route.
2. **MCP registration.** There is one deployed `zolly-mcp`, but the client
   entry must be per office: `ZOLLY_MCP_AGENT=zolly-<freq>`,
   `ZOLLY_ROOT=<project>`. The installer prints the entry keyed
   `zolly-<freq>`; register one entry per office under a distinct key. A single
   unqualified `zolly` entry reaches only one office.

Frequency collisions (e.g. `guzia-high-school` vs `Guzia High School`) are
refused by the installer, so unit and credential paths cannot silently override
each other.

## Trust model

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

## Configuration

`<project>/.agents/zolly/config.toml` is the single source. Precedence is
**environment variable > config.toml > default**.

```toml
frequency   = "my-app"     # default: the repo directory name
founder     = "Your Name"  # required — the office greets the founder by it
base_branch = "main"       # the branch work integrates into
remote      = "origin"     # the remote that branch is tracked on
voice       = false        # voice replies; only true where piper is installed
project     = "My App"     # display name (the installer writes the directory name)
```

Env overrides: `ZOLLY_FREQUENCY`, `ZOLLY_FOUNDER`, `ZOLLY_BASE_BRANCH`,
`ZOLLY_REMOTE`, `ZOLLY_VOICE`, `ZOLLY_PROJECT`.

`frequency` is the machine identifier; `project` is what the founder calls the
thing. The installer sets the frequency from `--name` (default: the directory
name) and the project name to the directory name, so they differ only when
you pass a different `--name` or edit the file.

A missing `config.toml` is not an error — every field has a default — but
`zolly doctor` **fails** when the founder is unset. An office that greets the
founder by the wrong name is worse than one that says nothing.

## The installer

```sh
install.sh [target-dir] [options]
```

**Interactive by default.** The installer asks before each optional feature —
the shared binary, the MCP server, Telegram, and the systemd unit — and any
answer of "no" skips just that feature. Questions are read from the terminal
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
| `--no-binary` | do not install the shared binary |
| `--no-mcp` | do not deploy the MCP server |
| `--no-telegram` | do not configure Telegram for this office |
| `--yes`, `-y` | accept every default; ask nothing |
| `--force` | install even if the target already has `.agents/` |
| `--dry-run` | print what would happen, change nothing |

Defaults per feature: binary **yes**, MCP **yes**, Telegram **no** — or **yes**
when `ZOLLY_TELEGRAM_TOKEN` is set — systemd **yes** when credentials exist.
`--yes` therefore installs the office and the binary, and turns Telegram on
only when a token is configured. The chat id is not a second thing to look up:
the installer asks Telegram (`getUpdates`) for the chats that have messaged the
bot and offers those ids, group chats excluded. `ZOLLY_ALLOWED_IDS` overrides
the discovery; with no update yet (nobody has messaged the bot) the installer
falls back to the prompt. `getUpdates` names every chat that has messaged the
bot, so on a bot strangers can reach, set `ZOLLY_ALLOWED_IDS` explicitly rather
than accepting the list. The same call records the founder's `@handle` as
`ZOLLY_FOUNDER_USERNAME` — the manager prefixes a reply into a group with it,
because only a mention notifies there; a DM already does. Pass `--founder` with
`--yes` — `zolly doctor` fails on an office with no founder name.

Env: `ZOLLY_TELEGRAM_TOKEN`, `ZOLLY_ALLOWED_IDS`, `ZOLLY_FOUNDER_USERNAME`,
`ZOLLY_TELEGRAM_API`, `ZOLLY_FOUNDER`, `ZOLLY_BIN_DEST`, `ZOLLY_TOOL_DIR`,
`ZOLLY_MCP_DEST`, `ZOLLY_KIT_URL`, `ZOLLY_REF`.

It refuses to run when the target is not a git repository, when the base
branch does not exist, when the target already has `.agents/` (without
`--force`), when the template tree contains a `.env`, or when the frequency is
already registered to a different project path. **The token guard is the
point:** a tree copy that shipped the bot token would leak it into every
project it touched.

It never edits `~/.zshrc` and never touches systemd state. It prints the
follow-ups and lets you run them.

## After installing

```sh
cd <target-dir>
zolly doctor      # the acceptance test: all green
zolly init        # start the foreman and pr-reviewer seats
```

`zolly init` opens two seats, joins them to the project's radio frequency, and
delivers each seat's boot prompt followed by its persona.

Then sit down. In the project, bare `zolly` joins the office's bus and starts
`omp` with the manager persona (`@.agents/zolly/agent.md`). `zolly <command>`
still runs the CLI, and bare `zolly` outside an office says so instead of
seating.

The installer no longer prints a shell block for `~/.zshrc`: the seat lives in
the binary, so it works in any shell, in a script, and in a fresh terminal.

## What the office needs from your project

The kit ships `rules/project.md` empty on purpose. It is the one file that
holds **your** truths, and no agent can infer them:

- the architecture and the services that make it up
- where things live on disk
- the size thresholds that make a change LARGE
- the divisions, if you have any

Fill it in, or the office will plan against a codebase that is not on disk.

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
| `tests/test_memory.py` | secret refusal, supersession, confirm, retract, the bound check |
| `tests/test_mcp.py` | input bounds, diagnostics redaction, project-bound agent selection |
| `tests/test_install.py` | fresh install, repeat refusal, force-preserves memory, frequency collisions |

`memory` honours `MEMORY_DIR`, so the suite runs in a temp directory and never
touches `.agents/memory/`. The MCP server has its own client:
`python3 .agents/zolly/mcp/test_mcp.py handshake`.

## Where things live

```
zolly/
├── install.sh          # the installer
├── test.sh             # syntax + unit + MCP handshake
├── README.md           # this file
├── tests/              # the kit's own tests
└── .agents/            # the template — the office kit, project-agnostic
```

The Rust tool source is not part of this repository. A project installs the
shared binary once (`ZOLLY_TOOL_DIR=<crate>`, or a prebuilt `~/.local/bin/zolly`).
`zolly close`'s self-rebuild step then **skips** rather than fails: installed
projects have no local tool source, so they use the shared binary.
