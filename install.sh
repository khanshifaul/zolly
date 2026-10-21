#!/bin/sh
# install.sh — install a Zolly office into any git project on this machine.
#
# One machine-wide binary, N per-project skeletons:
#
#   ~/.local/bin/zolly      built from the Rust tool source (or --no-binary)
#   <project>/.agents/      the office kit, materialized from this template
#
# The office is per-project: its radio frequency is the project's name, its
# seats are named `<seat>@<frequency>` so two offices never collide on the
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
#   --no-mcp            do not deploy or register the MCP server
#   --no-telegram       do not configure Telegram
#   --yes, -y           accept every default; ask nothing
#   --force             install even if the target already has .agents/
#   --dry-run           print what would happen, change nothing
#
# The install asks before each optional feature — the shared binary, the MCP
# server, Telegram and the systemd unit. Answer no to skip one. With no
# terminal (CI, a pipe) every default is used and nothing is asked; --yes
# takes the defaults without asking. Color is used only when stdout is a
# terminal; NO_COLOR=1 or TERM=dumb keeps the log plain.
#
# Env:
#   ZOLLY_TELEGRAM_TOKEN   bot token (prompted if absent and Telegram wanted)
#   ZOLLY_ALLOWED_IDS      comma-separated founder chat ids; when absent the
#                          installer asks Telegram (getUpdates) for the chats
#                          that have messaged the bot, then prompts as a fallback
#   ZOLLY_FOUNDER_USERNAME founder's @handle, without the @ (discovered from the
#                          same getUpdates call; empty when they have none)
#   ZOLLY_TELEGRAM_API     Bot API base (default https://api.telegram.org)
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
DO_MCP=1
DO_TELEGRAM=1           # 0 = --no-telegram: no credentials, no listener
ASSUME_YES=0
BIN_STATE="skipped"
MCP_STATE="skipped"
SD_STATE="skipped"
FORCE=0
DRY_RUN=0

BIN_DEST="${ZOLLY_BIN_DEST:-$HOME/.local/bin/zolly}"
TG_API="${ZOLLY_TELEGRAM_API:-https://api.telegram.org}"
TG_API=${TG_API%/}
CRED_ROOT="$HOME/.config/zolly"
SERVICE_DIR="$HOME/.config/systemd/user"

# ---------------------------------------------------------------------------
# presentation — color is a terminal affordance: pipes, logs and CI keep the
# plain text they always had. NO_COLOR (no-color.org) and TERM=dumb turn it
# off too, and stdout not being a terminal is enough on its own.
# ---------------------------------------------------------------------------
COLOR=0
C_RESET= C_BOLD= C_DIM= C_ACCENT= C_OK= C_WARN= C_ERR=
init_color() {
  [ -t 1 ] || return 0
  [ -z "${NO_COLOR:-}" ] || return 0
  [ "${TERM:-}" != dumb ] || return 0
  COLOR=1
  C_RESET=$(printf '\033[0m')
  C_BOLD=$(printf '\033[1m')
  C_DIM=$(printf '\033[2m')
  C_ACCENT=$(printf '\033[1;36m')
  C_OK=$(printf '\033[32m')
  C_WARN=$(printf '\033[33m')
  C_ERR=$(printf '\033[1;31m')
}
init_color

die() {
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s✖%s install.sh: %s\n' "$C_ERR" "$C_RESET" "$1" >&2
  else
    printf '  ✖ install.sh: %s\n' "$1" >&2
  fi
  exit 1
}
say() { printf '%s\n' "$1"; }

# One icon, one column. Every status line starts at the same place in a
# terminal and in a piped log; the `skip ` word stays contiguous for greps.
glyph() {
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s%s%s %s\n' "$2" "$1" "$C_RESET" "$3"
  else
    printf '  %s %s\n' "$1" "$3"
  fi
}
ok()   { glyph '✔' "$C_OK" "$1"; }
warn() { glyph '⚠' "$C_WARN" "$1"; }
skip() { glyph '→' "$C_DIM" "skip $1"; }
note() { glyph '·' "$C_DIM" "$1"; }
# Section headings were removed on request: the flat list reads top to bottom.
step() { :; }
heading() {
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s%s%s\n' "$C_BOLD" "$1" "$C_RESET"
  else
    printf '  %s\n' "$1"
  fi
}

# ---------------------------------------------------------------------------
# prompts — read from the terminal, never stdin (when piped, stdin is the
# script). No terminal (CI) means the default answer, so a prompt can never
# hang a non-interactive run.
# ---------------------------------------------------------------------------
ESC=$(printf '\033')

# A pasted value can arrive wrapped in bracketed-paste markers
# (ESC[200~ … ESC[201~) and, when the clipboard ends in a newline, split across
# two reads. Strip every CSI sequence and CR/LF/TAB so the value is the value.
clean_input() {
  printf '%s' "$1" \
    | sed -e "s/${ESC}\[[0-9;?]*[A-Za-z~]//g" \
    | tr -d '\015\012\011' \
    | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

can_ask() {
  [ "$ASSUME_YES" -eq 0 ] || return 1
  # stderr is redirected first: a missing or unusable /dev/tty would print a
  # shell error into the install log.
  tty 2>/dev/null >/dev/null </dev/tty
}

# tty_mode — canonical input and CR→NL, echo on, saved and restored around the
# read. A terminal a TUI left in raw mode otherwise never delivers the Enter
# that ends the line.
tty_mode() {
  tty_saved=$(stty -g </dev/tty 2>/dev/null || true)
  [ -n "$tty_saved" ] || return 0
  stty icanon icrnl echo </dev/tty 2>/dev/null || true
}
tty_restore() {
  [ -z "${tty_saved:-}" ] || stty "$tty_saved" </dev/tty 2>/dev/null || true
  tty_saved=""
}

# ask_yn <question> <y|n> — prints y or n. The default is used when there is
# no terminal or when the answer is empty.
ask_yn() {
  ask_q="$1"; ask_def="$2"
  if ! can_ask; then printf '%s\n' "$ask_def"; return 0; fi
  if [ "$ask_def" = y ]; then ask_sfx='[Y/n]'; else ask_sfx='[y/N]'; fi
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s?%s %s%s%s %s%s%s ' "$C_ACCENT" "$C_RESET" "$C_BOLD" "$ask_q" "$C_RESET" "$C_DIM" "$ask_sfx" "$C_RESET" >/dev/tty
  else
    printf '  ? %s %s ' "$ask_q" "$ask_sfx" >/dev/tty
  fi
  tty_mode
  if read -r ask_ans </dev/tty 2>/dev/null; then ask_ans=$(clean_input "$ask_ans"); else ask_ans=""; fi
  tty_restore
  [ -n "$ask_ans" ] || ask_ans="$ask_def"
  case "$ask_ans" in y|Y|yes|YES) printf 'y\n' ;; *) printf 'n\n' ;; esac
}

# ask_text <question> [default] — prints one line read from the terminal.
ask_text() {
  ask_q="$1"; ask_def="${2:-}"
  if ! can_ask; then printf '%s\n' "$ask_def"; return 0; fi
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s?%s %s%s%s ' "$C_ACCENT" "$C_RESET" "$C_BOLD" "$ask_q" "$C_RESET" >/dev/tty
  else
    printf '  ? %s ' "$ask_q" >/dev/tty
  fi
  tty_mode
  if read -r ask_val </dev/tty 2>/dev/null; then ask_val=$(clean_input "$ask_val"); else ask_val=""; fi
  tty_restore
  [ -n "$ask_val" ] || ask_val="$ask_def"
  printf '%s\n' "$ask_val"
}

# ask_secret <question> — prints the line read with echo on. The founder
# pastes the token here; seeing it is the point. It still never reaches shell
# history (read, not executed) and is not written to any log.
ask_secret() {
  ask_q="$1"
  if ! can_ask; then printf '\n'; return 0; fi
  if [ "$COLOR" -eq 1 ]; then
    printf '  %s?%s %s%s%s ' "$C_ACCENT" "$C_RESET" "$C_BOLD" "$ask_q" "$C_RESET" >/dev/tty
  else
    printf '  ? %s ' "$ask_q" >/dev/tty
  fi
  tty_mode
  if read -r ask_val </dev/tty 2>/dev/null; then :; else ask_val=""; fi
  # Bracketed paste is atomic, but a clipboard newline ends the read early.
  # Keep reading until the closing marker arrives (bounded, so no hang).
  ask_rounds=0
  while [ "$ask_rounds" -lt 8 ]; do
    case "$ask_val" in
      *"${ESC}[201~"*) break ;;
      *"${ESC}[200~"*)
        if read -r ask_more </dev/tty 2>/dev/null; then ask_val="$ask_val$ask_more"; else break; fi
        ask_rounds=$((ask_rounds + 1)) ;;
      *) break ;;
    esac
  done
  tty_restore
  ask_val=$(clean_input "$ask_val")
  printf '%s\n' "$ask_val"
}

usage() {
  # Everything from line 2 to `set -eu` is the header; no hardcoded range.
  sed -n '2,/^set -eu$/p' "$0" | sed -e 's/^# \{0,1\}//' -e '/^set -eu$/d'
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
    --no-mcp)       DO_MCP=0; shift ;;
    --no-telegram)  DO_TELEGRAM=0; shift ;;
    --yes|-y)       ASSUME_YES=1; shift ;;
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

if [ "$COLOR" -eq 1 ]; then
  printf '\n  %szolly%s %s·%s office installer  %s%s%s\n' \
    "$C_ACCENT" "$C_RESET" "$C_DIM" "$C_RESET" "$C_DIM" "$TARGET" "$C_RESET"
fi

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

if ! git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1; then
  die "target is not a git repository: $TARGET
  run: cd $TARGET && git init -b $BASE_BRANCH
  (a commit is not required)"
fi
ok "git repository: $TARGET"

if [ -e "$TARGET/.agents" ] && [ "$FORCE" -ne 1 ]; then
  die "$TARGET/.agents already exists — re-run with --force to overwrite it"
fi

# Unborn branch: `git init` is enough. There is no ref to verify, so take the
# branch git is on, and honour an explicit --base-branch by renaming the
# pending branch (safe before the first commit).
UNBORN=0
git -C "$TARGET" rev-parse --verify --quiet HEAD >/dev/null 2>&1 || UNBORN=1

if [ "$UNBORN" -eq 1 ]; then
  HEAD_BRANCH=$(git -C "$TARGET" symbolic-ref --quiet --short HEAD 2>/dev/null || true)
  [ -n "$HEAD_BRANCH" ] || HEAD_BRANCH="$BASE_BRANCH"
  if [ "$EXPLICIT_BASE" -eq 1 ] && [ "$HEAD_BRANCH" != "$BASE_BRANCH" ]; then
    git -C "$TARGET" symbolic-ref HEAD "refs/heads/$BASE_BRANCH" \
      || die "could not switch the unborn branch to $BASE_BRANCH"
    HEAD_BRANCH="$BASE_BRANCH"
  fi
  if [ "$HEAD_BRANCH" != "$BASE_BRANCH" ]; then
    BASE_BRANCH="$HEAD_BRANCH"
  fi
  ok "branch: $BASE_BRANCH (no commits yet)"
else
  # Commits exist. With no --base-branch, use the branch the repo is on.
  if [ "$EXPLICIT_BASE" -eq 0 ]; then
    HEAD_BRANCH=$(git -C "$TARGET" symbolic-ref --quiet --short HEAD 2>/dev/null || true)
    if [ -n "$HEAD_BRANCH" ] && [ "$HEAD_BRANCH" != "$BASE_BRANCH" ] \
       && ! git -C "$TARGET" rev-parse --verify --quiet "$BASE_BRANCH" >/dev/null 2>&1; then
      note "using the current branch '$HEAD_BRANCH'"
      BASE_BRANCH="$HEAD_BRANCH"
    fi
  fi
  if git -C "$TARGET" rev-parse --verify --quiet "$BASE_BRANCH" >/dev/null 2>&1; then
    ok "branch: $BASE_BRANCH"
  else
    die "branch '$BASE_BRANCH' does not exist in $TARGET — create it (git branch $BASE_BRANCH), or pass --base-branch <existing>"
  fi
fi

if git -C "$TARGET" rev-parse --verify --quiet "$REMOTE/$BASE_BRANCH" >/dev/null 2>&1; then
  ok "remote ref: $REMOTE/$BASE_BRANCH"
else
  warn "no $REMOTE/$BASE_BRANCH — local merges only"
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
# seat names are "<seat>@<freq>", so the frequency has to leave room.
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

ok "frequency: $FREQ"
ok "project:   $PROJECT_NAME"

if [ -z "$FOUNDER" ]; then
  FOUNDER=$(ask_text "Founder name:")
fi
[ -n "$FOUNDER" ] || warn "no founder name — the office runs without Telegram greetings"

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
    # office state) and runs are transient. The tool's target/ is build output
    # and can be gigabytes, so it is excluded too. Everything else is
    # recoverable.
    ( cd "$AGENTS_DIR" && tar -cf - --exclude=./worktrees --exclude=./runs \
        --exclude=./zolly/tool/target . ) \
      | ( cd "$BACKUP" && tar -xf - )
    ok "previous office saved to $BACKUP"
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
    # Work in flight and work finished are not template files. A reinstall that
    # dropped them silently destroyed 106 results and 33 tracked files on
    # 2026-10-21: the backup had them, the restore list did not name them.
    for d in results queue; do
      [ -d "$BACKUP/$d" ] || continue
      mkdir -p "$AGENTS_DIR/$d"
      ( cd "$BACKUP/$d" && tar -cf - . ) | ( cd "$AGENTS_DIR/$d" && tar -xf - )
    done
    ok "restored memory, hires, worker personas and project.md"
    # A project that keeps the Rust tool source under .agents/zolly/tool (the
    # factory checkout) must not lose it to --force: it is not part of the
    # template, and the fresh tree has no copy.
    if [ -d "$BACKUP/zolly/tool" ]; then
      mkdir -p "$AGENTS_DIR/zolly/tool"
      ( cd "$BACKUP/zolly/tool" && tar -cf - --exclude=./target . ) \
        | ( cd "$AGENTS_DIR/zolly/tool" && tar -xf - )
      ok "restored the tool source"
    fi
  fi
  ok "copied office kit"
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
  ok "placeholders substituted"
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
  ok "$AGENTS_DIR/zolly/config.toml"
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
      warn "$ROOT_CONTEXT has no token for:$missing"
      say "     the goal and those rulebooks will NOT be in context at session start."
      if grep -qF '@.agents/memory/zolly-goal.md' "$ROOT_CONTEXT" 2>/dev/null; then
        say "     note: it still imports @.agents/memory/zolly-goal.md; that file"
        say "           was renamed to goal.md. Update the line."
      fi
      say "     Add the @-token lines — this installer will not edit your file."
    else
      ok "$ROOT_CONTEXT already carries the goal and rulebook tokens"
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
    ok "$ROOT_CONTEXT (goal + rulebooks imported)"
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
    ok "$TARGET/OPEN-QUESTIONS.md"
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
    ok "$TARGET/HANDOFF.md"
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
    ok "runtime gitignore entries"
  else
    ok "runtime gitignore entries already present"
  fi
  # omp's project MCP entry names this machine's zolly-mcp binary and this
  # office's seat, so it is per machine, like the credentials — never committed.
  if ! grep -qxF '.omp/mcp.json' "$GITIGNORE" 2>/dev/null; then
    cat >> "$GITIGNORE" <<'EOF'

# Zolly: omp project MCP registration (absolute zolly-mcp path, per office).
.omp/mcp.json
EOF
    ok ".omp/mcp.json ignored"
  fi
  ok "runtime directories created"
fi

# ---------------------------------------------------------------------------
# the shared binary
# ---------------------------------------------------------------------------
step "shared binary"

if [ "$DO_BINARY" -eq 0 ]; then
  skip "binary (--no-binary)"
elif [ -x "$BIN_DEST" ]; then
  ok "already installed: $BIN_DEST"
  BIN_STATE="installed"
else
  # The crate ships beside this script in tool/. A checkout that keeps it
  # elsewhere overrides with ZOLLY_TOOL_DIR; no other location is guessed.
  TOOL_DIR="${ZOLLY_TOOL_DIR:-$SCRIPT_DIR/tool}"
  if [ -n "$TOOL_DIR" ] && [ -f "$TOOL_DIR/Cargo.toml" ]; then
    if ! command -v cargo >/dev/null 2>&1; then
      skip "binary (cargo not found) — install it at $BIN_DEST later"
    elif [ "$(ask_yn "Install the shared zolly binary now? (built from $TOOL_DIR)" y)" = n ]; then
      skip "binary (declined)"
    elif [ "$DRY_RUN" -eq 1 ]; then
      say "would build $TOOL_DIR and install to $BIN_DEST"
      BIN_STATE="planned"
    else
      say "building the tool (this takes a minute)..."
      ( cd "$TOOL_DIR" && cargo build --release ) || die "cargo build failed in $TOOL_DIR"
      mkdir -p "$(dirname "$BIN_DEST")"
      install -m 0755 "$TOOL_DIR/target/release/zolly" "$BIN_DEST"
      # Voice transcription resolves stt.py beside the binary first; the
      # office-local fallback no longer exists now that the crate lives in
      # the kit. Best-effort: voice is optional and config-gated.
      [ -f "$TOOL_DIR/stt.py" ] && install -m 0644 "$TOOL_DIR/stt.py" "$(dirname "$BIN_DEST")/stt.py"
      ok "installed $BIN_DEST"
      BIN_STATE="installed"
    fi
  else
    die "no Rust tool source found at $TOOL_DIR — set ZOLLY_TOOL_DIR to the crate, or install the binary at $BIN_DEST and re-run with --no-binary"
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
MCP_PROJECT_FILE="$TARGET/.omp/mcp.json"

if [ "$DRY_RUN" -eq 1 ]; then
  # Nothing was copied, so check the template rather than the target.
  MCP_SRC="$TEMPLATE/zolly/mcp/zolly_mcp.py"
  MCP_INSTALLER="$TEMPLATE/zolly/mcp/install.sh"
fi

if [ ! -f "$MCP_SRC" ] || [ ! -f "$MCP_INSTALLER" ]; then
  skip "MCP (missing $MCP_SRC or its install.sh)"
elif [ "$DO_MCP" -ne 1 ]; then
  skip "MCP (--no-mcp)"
elif [ -f "$MCP_DEST" ] && cmp -s "$MCP_SRC" "$MCP_DEST"; then
  ok "MCP server already deployed and identical: $MCP_DEST"
  MCP_STATE="installed"
  # The registration is per project, so it is ensured even when the binary is
  # current — a re-run is what upgrades an office installed before this file.
  if [ "$DRY_RUN" -eq 1 ]; then
    say "would write $MCP_PROJECT_FILE"
  else
    sh "$MCP_INSTALLER" "$MCP_DEST" --frequency "$FREQ" --root "$TARGET" --write "$TARGET" >/dev/null
    ok "MCP registered for this project: $MCP_PROJECT_FILE"
  fi
elif [ "$(ask_yn "Deploy zolly-mcp?" y)" = n ]; then
  skip "MCP (declined)"
elif [ "$DRY_RUN" -eq 1 ]; then
  say "would deploy the MCP server to $MCP_DEST"
  say "would write $MCP_PROJECT_FILE"
  MCP_STATE="planned"
else
  # One deployer, not two. The component owns the entry's shape and merges it
  # into the project file; a copy of that JSON here is how the two drift apart.
  sh "$MCP_INSTALLER" "$MCP_DEST" --frequency "$FREQ" --root "$TARGET" --write "$TARGET"
  MCP_STATE="installed"
fi

# ---------------------------------------------------------------------------
# credentials — machine-local, never in the project tree
# ---------------------------------------------------------------------------
step "credentials"

CRED_DIR="$CRED_ROOT/$FREQ"
CRED_FILE="$CRED_DIR/credentials"

# tg_updates <token> — the raw getUpdates body, or nothing. The chat id is not
# a second secret to collect: the token already reaches it, and so does the
# founder's @username. getUpdates is a read — no offset is confirmed, so the
# listener still sees every update afterwards. A failure (no network, a bad
# token, a 409 from another listener) prints nothing and the caller prompts.
tg_updates() {
  _tg_url="$TG_API/bot$1/getUpdates"
  if command -v curl >/dev/null 2>&1; then
    curl -fsS --connect-timeout 5 --max-time 10 "$_tg_url" 2>/dev/null || :
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- --timeout=10 "$_tg_url" 2>/dev/null || :
  fi
}

# tg_chat_ids <body> — the private-chat ids that have messaged this bot, comma
# separated. Negative ids are group chats, and a group is not a person to
# allow: only the private chats are offered.
tg_chat_ids() {
  printf '%s' "$1" |
    tr -d '\n' |
    grep -oE '"chat" *: *\{"id" *: *-?[0-9]+' |
    sed -e 's/.*: *//' |
    awk '!/^-/ && !seen[$0]++' |
    paste -sd, -
}

# tg_username <body> <user-id> — the handle of that sender, without the @.
# Empty when the sender set no public username. The `from` object is flat, so
# it is matched up to its closing brace; a `}` inside a display name would cut
# the match short and only cost the mention.
tg_username() {
  printf '%s' "$1" |
    tr -d '\n' |
    grep -oE '"from" *: *\{"id" *: *'"$2"',[^}]*"username" *: *"[A-Za-z0-9_]+"' |
    sed -e 's/.*"username" *: *"//' -e 's/"$//' |
    head -1
}

TOKEN="${ZOLLY_TELEGRAM_TOKEN:-}"
IDS="${ZOLLY_ALLOWED_IDS:-}"
USERNAME="${ZOLLY_FOUNDER_USERNAME:-}"
USERNAME=${USERNAME#@}
HAVE_CREDS=0
TG_STATE="off"

# Credentials already on disk belong to this frequency — the collision check
# binds a frequency to a project — so reuse them before asking. One bot token
# per office: two listeners sharing a token both call getUpdates and Telegram
# answers 409, so a second office needs its own bot.
if [ -z "$TOKEN" ] || [ -z "$IDS" ] || [ -z "$USERNAME" ]; then
  if [ -f "$CRED_FILE" ]; then
    ok "credentials already exist: $CRED_FILE"
    [ -n "$TOKEN" ] || TOKEN=$(sed -n 's/^ZOLLY_TELEGRAM_TOKEN=//p' "$CRED_FILE")
    [ -n "$IDS" ] || IDS=$(sed -n 's/^ZOLLY_ALLOWED_IDS=//p' "$CRED_FILE")
    if [ -z "$USERNAME" ]; then
      USERNAME=$(sed -n 's/^ZOLLY_FOUNDER_USERNAME=//p' "$CRED_FILE")
      USERNAME=${USERNAME#@}
    fi
  fi
fi

WANT_TELEGRAM=0
if [ "$DO_TELEGRAM" -eq 0 ]; then
  skip "Telegram (--no-telegram)"
else
  if [ -n "$TOKEN" ] && [ -n "$IDS" ]; then
    WANT_TELEGRAM=1
  else
    # A token alone is a configured office: the chat id is derived from it.
    # With no token the default stays no, as it always was.
    tg_default=n
    [ -n "$TOKEN" ] && tg_default=y
    if [ "$(ask_yn "Configure Telegram? (one bot per office)" "$tg_default")" = y ]; then
      WANT_TELEGRAM=1
    else
      skip "Telegram (declined)"
    fi
  fi
  if [ "$WANT_TELEGRAM" -eq 1 ]; then
    [ -n "$TOKEN" ] || TOKEN=$(ask_secret "Telegram bot token (blank to skip):")
    TG_JSON=""
    if [ -n "$TOKEN" ] && { [ -z "$IDS" ] || [ -z "$USERNAME" ]; }; then
      TG_JSON=$(tg_updates "$TOKEN")
    fi
    if [ -z "$IDS" ]; then
      IDS=$(tg_chat_ids "$TG_JSON")
      if [ -n "$IDS" ]; then
        say "chat ids: $IDS"
        [ "$(ask_yn "Use these ids?" y)" = y ] || IDS=""
      elif [ -n "$TOKEN" ]; then
        note "no chat id yet — send the bot a message, or paste the id below"
      fi
    fi
    [ -n "$IDS" ] || IDS=$(ask_text "Telegram user ids (comma separated):")
    if [ -n "$IDS" ] && [ -z "$USERNAME" ] && [ -n "$TG_JSON" ]; then
      USERNAME=$(tg_username "$TG_JSON" "${IDS%%,*}")
      [ -z "$USERNAME" ] || say "handle: @$USERNAME"
    fi
    if [ -n "$IDS" ] && [ -z "$USERNAME" ]; then
      say "no @handle — group replies will not mention the founder"
      say "  set ZOLLY_FOUNDER_USERNAME in $CRED_FILE to add one"
    fi
  fi
fi

if [ "$WANT_TELEGRAM" -eq 1 ] && [ -n "$TOKEN" ]; then
  if [ -n "$IDS" ]; then
    HAVE_CREDS=1
    TG_STATE="configured"
  else
    # Keep the token the founder just typed. The chat id arrives with the
    # first message, and a --force re-run discovers it; discarding a token
    # here made them hunt down BotFather again. No listener is written until
    # both are present, so a token alone is inert.
    TG_STATE="token-only"
  fi
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
ZOLLY_FOUNDER_USERNAME=$USERNAME
EOF
      chmod 600 "$tmp"
      mv -f "$tmp" "$CRED_FILE"
      trap - EXIT HUP INT TERM
    )
    if [ "$HAVE_CREDS" -eq 1 ]; then
      ok "credentials written: $CRED_FILE"
    else
      note "token saved — no chat id yet"
    fi
  fi
elif [ "$WANT_TELEGRAM" -eq 1 ]; then
  skip "credentials (no token) — add them later at $CRED_FILE"
fi

# ---------------------------------------------------------------------------
# systemd user unit
# ---------------------------------------------------------------------------
step "systemd"

UNIT_FILE="$SERVICE_DIR/zolly@$FREQ.service"

if [ "$DO_SYSTEMD" -ne 1 ]; then
  skip "systemd (--no-systemd)"
elif [ "$DO_TELEGRAM" -eq 0 ]; then
  skip "systemd (Telegram is off)"
elif [ "$HAVE_CREDS" -ne 1 ]; then
  # A listener with no token would restart-loop. Do not create a unit that
  # cannot work; the summary says how to finish the credentials.
  if [ "$TG_STATE" = "token-only" ]; then
    skip "systemd (no chat id yet)"
  else
    skip "systemd (no Telegram credentials)"
  fi
elif [ "$(ask_yn "Write the listener unit zolly@$FREQ.service?" y)" = n ]; then
  skip "systemd (declined)"
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
  ok "unit written: $UNIT_FILE"
  SD_STATE="written"
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

if [ "$COLOR" -eq 1 ]; then
  printf '\n  %s✔%s %soffice installed%s  %s%s%s\n' \
    "$C_OK" "$C_RESET" "$C_BOLD" "$C_RESET" "$C_DIM" "$AGENTS_DIR" "$C_RESET"
  printf '  %s·%s features' "$C_DIM" "$C_RESET"
  for _pair in "binary:$BIN_STATE" "MCP:$MCP_STATE" "Telegram:$TG_STATE" "systemd:$SD_STATE"; do
    _label=${_pair%%:*}; _value=${_pair#*:}
    case "$_value" in
      installed|configured|written) _col=$C_OK ;;
      *) _col=$C_DIM ;;
    esac
    printf '  %s%s%s %s%s%s' "$C_DIM" "$_label" "$C_RESET" "$_col" "$_value" "$C_RESET"
  done
  printf '\n'
else
  ok "office installed  $AGENTS_DIR"
  note "features: binary $BIN_STATE, MCP $MCP_STATE, Telegram $TG_STATE, systemd $SD_STATE"
fi
say ""
heading "Next steps:"
note "cd $TARGET"
note "zolly doctor    # optional check"
note "zolly           # manager + foreman + pr-reviewer"
if [ "$UNBORN" -eq 1 ]; then
  say ""
  note "no commits yet — commit before dispatching (branches cut from '$BASE_BRANCH')."
fi
say ""
note "Fill .agents/rules/project.md: architecture, layout, thresholds."
if [ "$HAVE_CREDS" -eq 1 ]; then
  note "Telegram: configured."
elif [ "$TG_STATE" = "token-only" ]; then
  warn "Telegram: token saved — message the bot, then re-run --force."
else
  warn "Telegram: off — token+ids in $CRED_FILE, then re-run --force."
fi
if [ "$BIN_STATE" != "installed" ] && [ "$DO_BINARY" -eq 1 ]; then
  warn "Binary: $BIN_STATE — build tool/ or drop it at $BIN_DEST."
fi
