#!/bin/sh
# install.sh — install a Zolly office into any git project on this machine.
#
# One machine-wide binary, N per-project skeletons:
#
#   ~/.local/bin/zolly      built from the Rust tool source (or --no-binary)
#   <project>/.agents/      the office kit, materialized from this template
#
# The office is per-project: its radio frequency is the project's name, its
# seats are named `<seat>-<frequency>` so two offices never collide on the
# herdr server (herdr agent names are unique server-wide), and its secrets
# live in ~/.config/zolly/<frequency>/credentials — never in the project tree.
#
# Usage:
#   ./install.sh [target-dir] [options]
#   curl -fsSL https://raw.githubusercontent.com/khanshifaul/zolly/main/install.sh \
#     | bash -s -- [target-dir] [options]
#
# The target defaults to the current directory. The install is per project:
# run it once per repository; each gets its own office.
#
# Options:
#   --name <freq>       radio frequency (default: target directory name)
#   --founder <name>    founder's display name (default: prompt; "" for office-only)
#   --base-branch <b>   integration branch (default: main)
#   --remote <r>        git remote (default: origin)
#   --no-systemd        do not write a systemd user unit
#   --no-binary         do not install the shared binary
#   --force             install even if the target already has .agents/
#   --dry-run           print what would happen, change nothing
#
# Env:
#   ZOLLY_TELEGRAM_TOKEN   bot token (prompted if absent and Telegram wanted)
#   ZOLLY_ALLOWED_IDS      comma-separated founder chat ids (prompted if absent)
#   ZOLLY_FOUNDER          founder name, non-interactive
#   ZOLLY_KIT_URL          tarball of this kit (piped only; default: GitHub main)
#   ZOLLY_REF              branch or tag to fetch when piped (default: main)
#   ZOLLY_MCP_DEST         where to deploy the MCP server (default ~/.local/bin)
#
# Idempotent: re-running updates the skeleton and leaves the rest alone.
set -eu

# If a later step fails after the old office was moved aside, say where it is.
BACKUP=""
trap 'rc=$?; if [ "$rc" -ne 0 ] && [ -n "$BACKUP" ]; then printf "\ninstall.sh: FAILED. The previous office is preserved at:\n  %s\n" "$BACKUP" >&2; fi' EXIT

# ---------------------------------------------------------------------------
# piped install: `curl -fsSL <url>/install.sh | bash -s -- [target] [options]`
# ---------------------------------------------------------------------------
# When the script is piped, $0 is the shell, not a file, so the template tree
# is not beside it. Fetch a tarball of the kit, then run the real installer from
# it. The install is still per-project: the target is the argument (or the
# current directory), and nothing is installed machine-wide except the binary.
if [ ! -f "$0" ]; then
  # The script cannot know its own URL, so the kit's default home is embedded.
  # ZOLLY_REF pins a branch or tag; ZOLLY_KIT_URL overrides the tarball entirely.
  ZOLLY_REF="${ZOLLY_REF:-main}"
  : "${ZOLLY_KIT_URL:=https://github.com/khanshifaul/zolly/archive/$ZOLLY_REF.tar.gz}"
  _tmp=$(mktemp -d)
  trap 'rm -rf "$_tmp"' EXIT INT TERM
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$ZOLLY_KIT_URL" | tar -xz -C "$_tmp" || { echo "install.sh: could not fetch $ZOLLY_KIT_URL" >&2; exit 1; }
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$ZOLLY_KIT_URL" | tar -xz -C "$_tmp" || { echo "install.sh: could not fetch $ZOLLY_KIT_URL" >&2; exit 1; }
  else
    echo "install.sh: need curl or wget to install from a pipe" >&2
    exit 1
  fi
  _kit=$(find "$_tmp" -maxdepth 3 -name install.sh -type f | head -1)
  [ -n "$_kit" ] || { echo "install.sh: the archive has no install.sh" >&2; exit 1; }
  if sh "$_kit" "$@"; then _rc=0; else _rc=$?; fi
  exit "$_rc"
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TEMPLATE="$SCRIPT_DIR/.agents"

# ---------------------------------------------------------------------------
# defaults
# ---------------------------------------------------------------------------
TARGET=""
FREQ=""
FOUNDER="${ZOLLY_FOUNDER:-}"
BASE_BRANCH="main"
EXPLICIT_BASE=0
REMOTE="origin"
DO_SYSTEMD=1
DO_BINARY=1
FORCE=0
DRY_RUN=0

BIN_DEST="${ZOLLY_BIN_DEST:-$HOME/.local/bin/zolly}"
CRED_ROOT="$HOME/.config/zolly"
SERVICE_DIR="$HOME/.config/systemd/user"

die() { printf 'install.sh: %s\n' "$1" >&2; exit 1; }
say() { printf '%s\n' "$1"; }
step() { printf '\n== %s\n' "$1"; }

usage() {
  sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

# ---------------------------------------------------------------------------
# argument parsing
# ---------------------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --name)         FREQ="${2:-}"; shift 2 ;;
    --founder)      FOUNDER="${2:-}"; shift 2 ;;
    --base-branch)  BASE_BRANCH="${2:-}"; EXPLICIT_BASE=1; shift 2 ;;
    --remote)       REMOTE="${2:-}"; shift 2 ;;
    --no-systemd)   DO_SYSTEMD=0; shift ;;
    --no-binary)    DO_BINARY=0; shift ;;
    --force)        FORCE=1; shift ;;
    --dry-run)      DRY_RUN=1; shift ;;
    -h|--help)      usage 0 ;;
    -*)             die "unknown option: $1 (try --help)" ;;
    *)              [ -z "$TARGET" ] || die "more than one target directory given"
                    TARGET="$1"; shift ;;
  esac
done

# The target defaults to the current directory, so `cd project && ... | bash`
# installs into that project without repeating its path.
[ -n "$TARGET" ] || TARGET="."
TARGET=$(CDPATH= cd -- "$TARGET" 2>/dev/null && pwd) || die "target directory does not exist: $TARGET"
[ -d "$TEMPLATE" ] || die "template not found at $TEMPLATE — run install.sh from the zolly kit directory"

# ---------------------------------------------------------------------------
# token guard: the template must never carry a secret
# ---------------------------------------------------------------------------
stray_env=$(find "$TEMPLATE" -name '.env' -o -name '*.env' | head -20)
[ -z "$stray_env" ] || {
  printf 'install.sh: refusing to install — the template contains a secret file:\n' >&2
  printf '  %s\n' "$stray_env" >&2
  exit 1
}

# ---------------------------------------------------------------------------
# preconditions
# ---------------------------------------------------------------------------
step "preconditions"

git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1 || die "target is not a git repository: $TARGET
  run: cd $TARGET && git init -b $BASE_BRANCH"
say "ok   git repository: $TARGET"

if [ -e "$TARGET/.agents" ] && [ "$FORCE" -ne 1 ]; then
  die "$TARGET/.agents already exists — re-run with --force to overwrite it"
fi

# A fresh `git init` has an unborn branch: no ref exists, so neither the base
# branch nor a worktree base can be resolved. Name the exact commands.
if ! git -C "$TARGET" rev-parse --verify --quiet HEAD >/dev/null 2>&1; then
  die "the repository has no commits yet, so '$BASE_BRANCH' does not exist.
  Run:
    cd $TARGET
    git config user.name \"Your Name\"
    git config user.email \"you@example.com\"
    git commit --allow-empty -m init
    git branch -M $BASE_BRANCH
  then re-run this installer."
fi

# No --base-branch given: use the branch the repo is on. `git init` can create
# `master`, and hardcoding `main` would fail a repository that works fine.
if [ "$EXPLICIT_BASE" -eq 0 ]; then
  HEAD_BRANCH=$(git -C "$TARGET" symbolic-ref --quiet --short HEAD 2>/dev/null || true)
  if [ -n "$HEAD_BRANCH" ] && [ "$HEAD_BRANCH" != "$BASE_BRANCH" ] \
     && ! git -C "$TARGET" rev-parse --verify --quiet "$BASE_BRANCH" >/dev/null 2>&1; then
    say "note no '$BASE_BRANCH'; using the current branch '$HEAD_BRANCH'"
    BASE_BRANCH="$HEAD_BRANCH"
  fi
fi

# The office merges into the integration branch, so the branch has to exist
# locally and match the remote. Same guard `zolly foreman` enforces.
if git -C "$TARGET" rev-parse --verify --quiet "$BASE_BRANCH" >/dev/null 2>&1; then
  say "ok   branch: $BASE_BRANCH"
else
  die "branch '$BASE_BRANCH' does not exist in $TARGET — create it (git branch $BASE_BRANCH), or pass --base-branch <existing>"
fi

if git -C "$TARGET" rev-parse --verify --quiet "$REMOTE/$BASE_BRANCH" >/dev/null 2>&1; then
  say "ok   remote ref: $REMOTE/$BASE_BRANCH"
else
  say "warn no remote ref '$REMOTE/$BASE_BRANCH' — the office will merge locally only"
fi

# ---------------------------------------------------------------------------
# names
# ---------------------------------------------------------------------------
if [ -z "$FREQ" ]; then
  FREQ=$(basename "$TARGET")
fi
# The frequency lands in a radio scope and in a herdr agent name; both are
# lowercase and restricted. Sanitize rather than reject — a directory named
# "My Project" should still install.
FREQ=$(printf '%s' "$FREQ" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9_-]/-/g; s/^-*//; s/-*$//')
[ -n "$FREQ" ] || FREQ="zolly"
# A herdr agent name is at most 32 chars and must start with a letter. The
# seat names are "<seat>-<freq>", so the frequency has to leave room.
if [ "${#FREQ}" -gt 24 ]; then
  FREQ=$(printf '%s' "$FREQ" | cut -c1-24 | sed 's/-*$//')
fi
PROJECT_NAME=$(basename "$TARGET")

# A frequency is an identity: seat names, the credentials directory and the
# systemd unit all derive from it. Two projects must never silently share one.
PROJECT_ID=$(CDPATH= cd -- "$TARGET" && pwd -P)
if [ -f "$CRED_ROOT/$FREQ/project" ]; then
  OWNER=$(cat "$CRED_ROOT/$FREQ/project")
  [ "$OWNER" = "$PROJECT_ID" ] || die "frequency '$FREQ' already belongs to $OWNER — re-run with --name <distinct>"
fi
if [ "$DRY_RUN" -ne 1 ]; then
  ( umask 077; mkdir -p "$CRED_ROOT/$FREQ"; printf '%s\n' "$PROJECT_ID" > "$CRED_ROOT/$FREQ/project" )
fi

say "ok   frequency: $FREQ"
say "ok   project:   $PROJECT_NAME"

if [ -z "$FOUNDER" ]; then
  printf '\nFounder name (the office greets them by it; blank for an office with no Telegram): '
  read -r FOUNDER || FOUNDER=""
  printf '\n'
fi
[ -n "$FOUNDER" ] || say "warn no founder name — the office runs without Telegram greetings"

# ---------------------------------------------------------------------------
# materialize the skeleton
# ---------------------------------------------------------------------------
step "materialize office kit"

AGENTS_DIR="$TARGET/.agents"
if [ "$DRY_RUN" -eq 1 ]; then
  say "would copy $TEMPLATE -> $AGENTS_DIR"
else
  BACKUP=""
  if [ -e "$AGENTS_DIR" ]; then
    # --force must never be a silent downgrade of an existing office. The
    # template's memory files are empty scaffolds; the target's are the only
    # copy of decisions, learnings and current state.
    BACKUP="$TARGET/.agents-backup-$(date -u +%Y%m%dT%H%M%SZ)"
    mkdir -p "$BACKUP"
    # Keep everything except worktrees and runs: worktrees are checkouts (not
    # office state) and runs are transient. Everything else is recoverable.
    ( cd "$AGENTS_DIR" && tar -cf - --exclude=./worktrees --exclude=./runs . ) \
      | ( cd "$BACKUP" && tar -xf - )
    say "ok   previous office saved to $BACKUP"
  fi

  rm -rf "$AGENTS_DIR"
  mkdir -p "$AGENTS_DIR"
  # `cp -R` keeps the template's own directory layout. The template holds no
  # symlinks (verified at build time), so nothing can point outside the tree.
  cp -R "$TEMPLATE/." "$AGENTS_DIR/"
  # Never ship a stale interpreter cache into a project. A local template tree
  # can have one after tests; a git archive cannot.
  find "$AGENTS_DIR" -type d -name __pycache__ -prune -exec rm -rf {} + 2>/dev/null || true
  find "$AGENTS_DIR" -type f -name '*.pyc' -exec rm -f {} + 2>/dev/null || true

  if [ -n "$BACKUP" ]; then
    # Restore what the office cannot regenerate. The template's copies are
    # empty preambles; the target's are the state that has to survive.
    find "$BACKUP/memory" -maxdepth 1 -type f -name '*.md' -exec cp {} "$AGENTS_DIR/memory/" \;
    [ -d "$BACKUP/memory/archive" ] && cp -R "$BACKUP/memory/archive/." "$AGENTS_DIR/memory/archive/"
    [ -f "$BACKUP/rules/project.md" ] && cp "$BACKUP/rules/project.md" "$AGENTS_DIR/rules/project.md"
    find "$BACKUP/hr" -maxdepth 1 -type f \( -name '*.md' -o -name '*.hired' \) -exec cp {} "$AGENTS_DIR/hr/" \; 2>/dev/null || true
    find "$BACKUP/worker" -maxdepth 1 -type f -name '*.md' ! -name 'generic.md' -exec cp {} "$AGENTS_DIR/worker/" \; 2>/dev/null || true
    say "ok   restored memory, hires, worker personas and project.md"
  fi
  say "ok   copied office kit"
fi

# ---------------------------------------------------------------------------
# substitute the placeholders
# ---------------------------------------------------------------------------
step "configure"

substitute() {
  # sed cannot portably take a value containing a slash or an ampersand from a
  # variable, so every value is escaped once here.
  esc() { printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'; }
  _f=$(esc "$FREQ")
  _n=$(esc "$PROJECT_NAME")
  _b=$(esc "$BASE_BRANCH")
  _r=$(esc "$REMOTE")
  _d=$(esc "$FOUNDER")

  find "$AGENTS_DIR" -type f \
    \( -name '*.md' -o -name '*.toml' -o -name '*.sh' -o -name '*.py' \) \
    -exec sed -i \
        -e "s|{{FREQUENCY}}|$_f|g" \
        -e "s|{{PROJECT_NAME}}|$_n|g" \
        -e "s|{{BASE_BRANCH}}|$_b|g" \
        -e "s|{{REMOTE}}|$_r|g" \
        -e "s|{{FOUNDER}}|$_d|g" \
        {} +
}

if [ "$DRY_RUN" -eq 1 ]; then
  say "would substitute {{FREQUENCY}}=${FREQ} {{PROJECT_NAME}}=${PROJECT_NAME} {{BASE_BRANCH}}=${BASE_BRANCH} {{REMOTE}}=${REMOTE} {{FOUNDER}}=${FOUNDER:-<unset>}"
else
  substitute
  say "ok   placeholders substituted"
fi

# ---------------------------------------------------------------------------
# config.toml — the file every later change reads
# ---------------------------------------------------------------------------
if [ "$DRY_RUN" -ne 1 ]; then
  mkdir -p "$AGENTS_DIR/zolly"
  cat > "$AGENTS_DIR/zolly/config.toml" <<EOF
# Zolly office configuration for this project.
#
# Precedence: environment variable > this file > default.
#   ZOLLY_FREQUENCY, ZOLLY_FOUNDER, ZOLLY_BASE_BRANCH, ZOLLY_REMOTE
#
# The installer writes this file; edit it directly any time — nothing else
# reads the values, this file does.
frequency = "$FREQ"
founder = "$FOUNDER"
base_branch = "$BASE_BRANCH"
remote = "$REMOTE"
voice = false
# The project's display name, used in the office's own greetings.
project = "$PROJECT_NAME"
EOF
  say "ok   $AGENTS_DIR/zolly/config.toml"
fi

# ---------------------------------------------------------------------------
# root context file — the `@` tokens that put the goal and the rulebooks in
# context at session start
# ---------------------------------------------------------------------------
# The office's core mechanism is an `@path` token in the repo's root context
# file: omp expands `@` inside a context file before injecting it. Without this
# the goal and the four rulebooks sit on disk and are never read — a file
# nobody reads is a comment.
ROOT_CONTEXT="$TARGET/AGENTS.md"
if [ "$DRY_RUN" -eq 1 ]; then
  say "would write the @-token block into $ROOT_CONTEXT"
else
  if [ -f "$ROOT_CONTEXT" ]; then
    # The file exists — do not touch a file the founder owns. But do check
    # that the tokens are actually in it: an office whose rulebooks sit on
    # disk and are never read is a comment, not a rule.
    missing=""
    for tok in '@.agents/memory/goal.md' '@.agents/rules/system.md' '@.agents/rules/safety.md' '@.agents/rules/workflows.md' '@.agents/rules/project.md'; do
      grep -qF "$tok" "$ROOT_CONTEXT" 2>/dev/null || missing="$missing $tok"
    done
    if [ -n "$missing" ]; then
      say "warn $ROOT_CONTEXT has no token for:$missing"
      say "     the goal and those rulebooks will NOT be in context at session start."
      if grep -qF '@.agents/memory/zolly-goal.md' "$ROOT_CONTEXT" 2>/dev/null; then
        say "     note: it still imports @.agents/memory/zolly-goal.md; that file"
        say "           was renamed to goal.md. Update the line."
      fi
      say "     Add the @-token lines — this installer will not edit your file."
    else
      say "ok   $ROOT_CONTEXT already carries the goal and rulebook tokens"
    fi
  else
    cat > "$ROOT_CONTEXT" <<EOF
# $PROJECT_NAME — agent context

The objective is imported below, so it is in context at session start. Do not
go looking for it, and do not re-derive it.

@.agents/memory/goal.md

The agent layer is \`.agents/\`, and its rulebooks are imported here so they are
in context at session start rather than merely present on disk:

@.agents/rules/system.md
@.agents/rules/safety.md
@.agents/rules/workflows.md
@.agents/rules/project.md

\`.agents/rules/learned.md\` is deliberately NOT imported: it is the procedure for
promoting a learning, and the facts it governs live in \`.agents/memory/learnings.md\`.
\`.agents/AGENTS.md\` is the map of the whole agent layer.
EOF
    say "ok   $ROOT_CONTEXT (goal + rulebooks imported)"
  fi
fi

# ---------------------------------------------------------------------------
# runtime directories + gitignore entries
# ---------------------------------------------------------------------------
step "runtime state"

RUNTIME_DIRS="results queue inbox runs worktrees trash events days memory/.work"

if [ "$DRY_RUN" -ne 1 ]; then
  for d in $RUNTIME_DIRS; do
    mkdir -p "$AGENTS_DIR/$d"
  done
  # .gitkeep so an empty runtime dir still exists in a fresh clone.
  : > "$AGENTS_DIR/results/.gitkeep"

  # The register of decisions only the founder can make. The `recap` skill
  # reads it at a cold start; a skill that reads a file that does not exist
  # fails at step 1 of a fresh install.
  if [ ! -f "$TARGET/OPEN-QUESTIONS.md" ]; then
    cat > "$TARGET/OPEN-QUESTIONS.md" <<'EOF'
# Open Questions

The register of decisions only the founder can make. It exists because the
failure mode it prevents is silent: an agent hits an ambiguity, picks a
plausible default, and the assumption is then buried in a change nobody re-reads.

Each entry has a short question, why it matters, the options and their costs,
and a blank Answer line.

An entry is deleted when it is answered — the answer goes into a rule or a
memory file, not into a register of things still open.
EOF
    say "ok   $TARGET/OPEN-QUESTIONS.md"
  fi

  # The handover point. The `recap` skill reads it from the tail on a cold
  # start; the `close-the-day` skill writes it at the end of a day.
  if [ ! -f "$TARGET/HANDOFF.md" ]; then
    cat > "$TARGET/HANDOFF.md" <<'EOF'
# Handoff

The resume point for the next session. Read from the **tail**: the newest
session is the last one.

## What is in flight

## What is blocked

## The single next action

(No handoff yet. The `close-the-day` skill writes this file at the end of a
working day.)
EOF
    say "ok   $TARGET/HANDOFF.md"
  fi

  GITIGNORE="$TARGET/.gitignore"
  if [ ! -f "$GITIGNORE" ]; then
    : > "$GITIGNORE"
  fi
  if ! grep -q '^# Zolly office runtime' "$GITIGNORE" 2>/dev/null; then
    cat >> "$GITIGNORE" <<'EOF'

# Zolly office runtime — machine-local state, NOT memory.
# .agents/memory/ IS committed on purpose: it is the knowledge that has to
# survive a machine switch. Everything below is high-churn runtime that is
# worthless on another box. Do NOT add `.agents/` wholesale.
# .agents/results/ is deliberately NOT ignored: a worker commits its result
# file on the task branch, and the reviewer reads it there. It is not
# committed to main, but git has no per-branch ignore.
.agents/STATE.md
.agents/day.json
.agents/queue/
.agents/runs/
.agents/inbox/
.agents/worktrees/
.agents/trash/
.agents/days/
.agents/events/
.agents/monitor/
.agents/memory/.work/
.agents/memory/.lock
.agents/memory/*.tmp
.agents-backup-*/
.agents/foreman.pid
.agents/foreman-loop.pid
.agents/foreman-settle.pid
.agents/LAST_PROMPT.md
.agents/foreman-nudged.stamp
.agents/zolly/telegram_offset.txt
.agents/zolly/telegram_serve.lock
.agents/zolly/task_ledger.json
.agents/zolly/task_ledger.json.tmp
.agents/zolly/dead_letter.jsonl
.agents/nudge.stamp
EOF
    say "ok   runtime gitignore entries"
  else
    say "ok   runtime gitignore entries already present"
  fi
  say "ok   runtime directories created"
fi

# ---------------------------------------------------------------------------
# the shared binary
# ---------------------------------------------------------------------------
step "shared binary"

if [ "$DO_BINARY" -eq 0 ]; then
  say "skip binary (--no-binary)"
elif [ -x "$BIN_DEST" ]; then
  say "ok   already installed: $BIN_DEST"
else
  # The Rust source is not part of this kit. ZOLLY_TOOL_DIR is the only
  # location the installer assumes; a sibling checkout is not guessed.
  if [ -n "${ZOLLY_TOOL_DIR:-}" ]; then
    TOOL_DIR="$ZOLLY_TOOL_DIR"
  else
    TOOL_DIR=""
  fi
  if [ -n "$TOOL_DIR" ] && [ -f "$TOOL_DIR/Cargo.toml" ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
      say "would build $TOOL_DIR and install to $BIN_DEST"
    else
      say "building the tool (this takes a minute)..."
      ( cd "$TOOL_DIR" && cargo build --release ) || die "cargo build failed in $TOOL_DIR"
      mkdir -p "$(dirname "$BIN_DEST")"
      install -m 0755 "$TOOL_DIR/target/release/zolly" "$BIN_DEST"
      say "ok   installed $BIN_DEST"
    fi
  else
    die "no Rust tool source found — set ZOLLY_TOOL_DIR to the crate, or install the binary at $BIN_DEST and re-run with --no-binary"
  fi
fi

# ---------------------------------------------------------------------------
# the MCP server — one copy per machine, not per project
# ---------------------------------------------------------------------------
# `zolly-mcp` exposes Zolly as a typed MCP server. It is machine-wide: the
# binary reaches whatever office it runs in, so there is one deployed copy.
MCP_SRC="$AGENTS_DIR/zolly/mcp/zolly_mcp.py"
MCP_INSTALLER="$AGENTS_DIR/zolly/mcp/install.sh"
MCP_DEST="${ZOLLY_MCP_DEST:-$HOME/.local/bin/zolly-mcp}"

if [ "$DRY_RUN" -eq 1 ]; then
  # Nothing was copied, so check the template rather than the target.
  MCP_SRC="$TEMPLATE/zolly/mcp/zolly_mcp.py"
  MCP_INSTALLER="$TEMPLATE/zolly/mcp/install.sh"
fi

if [ ! -f "$MCP_SRC" ] || [ ! -f "$MCP_INSTALLER" ]; then
  say "skip MCP (missing $MCP_SRC or its install.sh)"
elif [ -f "$MCP_DEST" ] && cmp -s "$MCP_SRC" "$MCP_DEST"; then
  say "ok   MCP server already deployed and identical: $MCP_DEST"
elif [ "$DRY_RUN" -eq 1 ]; then
  say "would deploy the MCP server to $MCP_DEST"
else
  # One deployer, not two. The component prints the canonical mcp.json entry;
  # a copy of that text here is how the two installers drift apart.
  sh "$MCP_INSTALLER" "$MCP_DEST" --frequency "$FREQ" --root "$TARGET"
fi

# ---------------------------------------------------------------------------
# credentials — machine-local, never in the project tree
# ---------------------------------------------------------------------------
step "credentials"

CRED_DIR="$CRED_ROOT/$FREQ"
CRED_FILE="$CRED_DIR/credentials"

TOKEN="${ZOLLY_TELEGRAM_TOKEN:-}"
IDS="${ZOLLY_ALLOWED_IDS:-}"

if [ -n "$FOUNDER" ] && { [ -z "$TOKEN" ] || [ -z "$IDS" ]; }; then
  if [ -f "$CRED_FILE" ]; then
    say "ok   credentials already exist: $CRED_FILE"
    TOKEN=$(sed -n 's/^ZOLLY_TELEGRAM_TOKEN=//p' "$CRED_FILE")
    IDS=$(sed -n 's/^ZOLLY_ALLOWED_IDS=//p' "$CRED_FILE")
  else
    if [ -t 0 ]; then
      printf '\nTelegram bot token (from @BotFather; blank to skip Telegram): '
      read -r TOKEN || TOKEN=""
      printf 'Allowed Telegram user ids (comma separated; blank to skip): '
      read -r IDS || IDS=""
      printf '\n'
    else
      say "warn no Telegram token — the office runs without Telegram"
    fi
  fi
fi

if [ -n "$TOKEN" ] && [ -n "$IDS" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    say "would write $CRED_FILE"
  else
    # umask before mkdir: the directory must be 0700 from the start, not
    # however the caller's umask happened to leave it. Write the file
    # atomically so a failed update cannot destroy a valid token.
    ( umask 077
      mkdir -p "$CRED_DIR"
      chmod 700 "$CRED_DIR"
      tmp="$CRED_DIR/.credentials.tmp.$$"
      trap 'rm -f "$tmp"' EXIT HUP INT TERM
      cat > "$tmp" <<EOF
ZOLLY_TELEGRAM_TOKEN=$TOKEN
ZOLLY_ALLOWED_IDS=$IDS
EOF
      chmod 600 "$tmp"
      mv -f "$tmp" "$CRED_FILE"
      trap - EXIT HUP INT TERM
    )
    say "ok   credentials written: $CRED_FILE"
  fi
else
  say "skip credentials (no token) — add one later at $CRED_FILE"
fi

# ---------------------------------------------------------------------------
# systemd user unit
# ---------------------------------------------------------------------------
step "systemd"

UNIT_FILE="$SERVICE_DIR/zolly@$FREQ.service"

if [ "$DO_SYSTEMD" -ne 1 ]; then
  say "skip systemd (--no-systemd)"
elif [ "$DRY_RUN" -eq 1 ]; then
  say "would write $UNIT_FILE"
else
  mkdir -p "$SERVICE_DIR"
  cat > "$UNIT_FILE" <<EOF
[Unit]
Description=Zolly Telegram listener ($FREQ)
After=network.target

[Service]
Type=simple
ExecStart=$BIN_DEST telegram-serve
Restart=always
RestartSec=10
EnvironmentFile=-$CRED_FILE
WorkingDirectory=$TARGET

[Install]
WantedBy=default.target
EOF
  say "ok   unit written: $UNIT_FILE"
  say ""
  say "To run it:"
  say "  systemctl --user daemon-reload"
  say "  systemctl --user enable --now zolly@$FREQ.service"
fi

# ---------------------------------------------------------------------------
# summary + the follow-ups the founder has to do by hand
# ---------------------------------------------------------------------------
step "done"

if [ "$DRY_RUN" -eq 1 ]; then
  say "dry run — nothing was changed"
  exit 0
fi

say "office installed: $AGENTS_DIR"
say ""
say "Next steps (printed, not run — they touch your shell and your services):"
say ""
say "1. Check the office:"
say "     cd $TARGET && $BIN_DEST doctor"
say ""
say "2. Start the seats (foreman + pr-reviewer):"
say "     cd $TARGET && $BIN_DEST init"
say ""
say "3. Bind your own pane to the bus. Add this to ~/.zshrc:"
say ""
say "     zolly-seat() {"
say "       # Reads the frequency and project name from the office's own config,"
say "       # so one function serves every project on this machine."
say "       local freq=\"$FREQ\""
say "       local project=\"$PROJECT_NAME\""
say "       if [ -f \".agents/zolly/config.toml\" ]; then"
say "         freq=\$(sed -n 's/^[[:space:]]*frequency[[:space:]]*=[[:space:]]*\"\\([^\"]*\)\".*/\\1/p' \".agents/zolly/config.toml\" | head -1)"
say "         project=\$(sed -n 's/^[[:space:]]*project[[:space:]]*=[[:space:]]*\"\\([^\"]*\)\".*/\\1/p' \".agents/zolly/config.toml\" | head -1)"
say "         [ -n \"\$freq\" ] || freq=\"$FREQ\""
say "         [ -n \"\$project\" ] || project=\"\$freq\""
say "       fi"
say "       local pane=\"\${HERDR_PANE_ID:-}\""
say "       local role=\"Manager of the \$project office. Routes founder instructions: small work herself, large work to foreman. Never merges.\""
say "       local out=\"\""
say "       if ! command -v radio >/dev/null 2>&1; then"
say "         out=\"radio is not installed\""
say "       elif [ -n \"\$pane\" ]; then"
say "         out=\"\$(radio join zolly --pane \"\$pane\" --no-launch --frequency \"\$freq\" --role \"\$role\" 2>&1)\" && out=\"\""
say "       else"
say "         out=\"\$(radio join zolly --no-launch --frequency \"\$freq\" --role \"\$role\" 2>&1)\" && out=\"\""
say "       fi"
say "       [ -z \"\$out\" ] || printf 'zolly-seat: %s — starting omp without the Radio join\\n' \"\${out//\$\'\\n\'/ }\" >&2"
say "       omp \"\$@\""
say "     }"
say ""
say "4. Write this project's truths into $AGENTS_DIR/rules/project.md —"
say "   the architecture, where things live, the size thresholds. The template"
say "   shipped that file empty on purpose: it is yours."
say ""
say "5. If you want Telegram, put the token in $CRED_FILE"
say "   (ZOLLY_TELEGRAM_TOKEN=... and ZOLLY_ALLOWED_IDS=...), then run"
say "   systemctl --user daemon-reload && systemctl --user enable --now zolly@$FREQ.service"
