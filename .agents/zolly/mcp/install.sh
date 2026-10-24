#!/bin/sh
# Deploy zolly-mcp to a stable path and register it for a project.
#
# Deployed as a copy (not a symlink) so the registered command survives the
# foreman worktree being garbage-collected after the PR merges. Re-run this
# after editing zolly_mcp.py; the running server is the file at DEST.
#
# Usage: install.sh [DEST] [--frequency <freq>] [--root <repo>] [--write <dir>]
#   --frequency adds ZOLLY_MCP_AGENT=zolly-<freq> to the entry, so the server
#   reaches this office's manager rather than the first bare `zolly`.
#   --root adds ZOLLY_ROOT, so the server resolves `zolly pulse` without
#   depending on the MCP client's working directory.
#   --write merges the entry into <dir>/.omp/mcp.json — omp's project-scoped
#   config, which the project precedes the user scope in discovery. Without it
#   the entry is printed for a manual merge into ~/.omp/agent/mcp.json.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DEST="$HOME/.local/bin/zolly-mcp"
FREQ=""
ROOT=""
WRITE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --frequency) FREQ="${2:-}"; shift 2 ;;
    --root)      ROOT="${2:-}"; shift 2 ;;
    --write)     WRITE="${2:-}"; shift 2 ;;
    -*)          echo "usage: install.sh [DEST] [--frequency <freq>] [--root <repo>] [--write <dir>]" >&2; exit 1 ;;
    *)           DEST="$1"; shift ;;
  esac
done

mkdir -p "$(dirname "$DEST")"
install -m 0755 "$HERE/zolly_mcp.py" "$DEST"
echo "  ✔ installed: $DEST"

ENV_JSON=""
ENTRY_KEY="zolly"
if [ -n "$FREQ" ]; then
  ENTRY_KEY="zolly-$FREQ"
fi
if [ -n "$FREQ" ] || [ -n "$ROOT" ]; then
  ENV_PARTS=""
  if [ -n "$FREQ" ]; then
    ENV_PARTS="\"ZOLLY_MCP_AGENT\": \"zolly-$FREQ\""
  fi
  if [ -n "$ROOT" ]; then
    [ -n "$ENV_PARTS" ] && ENV_PARTS="$ENV_PARTS, "
    ENV_PARTS="$ENV_PARTS\"ZOLLY_ROOT\": \"${ROOT:-.}\""
  fi
  ENV_JSON=",
      \"env\": { $ENV_PARTS }"
fi

if [ -n "$WRITE" ]; then
  command -v python3 >/dev/null 2>&1 || {
    echo "zolly-mcp: python3 is required to merge $WRITE/.omp/mcp.json" >&2
    exit 1
  }
  mkdir -p "$WRITE/.omp"
  python3 - "$WRITE/.omp/mcp.json" "$ENTRY_KEY" "$DEST" "$ROOT" "$FREQ" <<'PY'
import json
import os
import sys

SCHEMA = ("https://raw.githubusercontent.com/can1357/oh-my-pi/main/"
          "packages/coding-agent/src/config/mcp-schema.json")

path, key, dest, root, freq = sys.argv[1:6]
created = not os.path.exists(path)
data = {}
if not created:
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except json.JSONDecodeError as exc:
        sys.exit(f"zolly-mcp: {path} is not valid JSON ({exc}); merge by hand")
    if not isinstance(data, dict):
        sys.exit(f"zolly-mcp: {path} is not a JSON object; merge by hand")

servers = data.get("mcpServers")
if servers is None:
    servers = data["mcpServers"] = {}
elif not isinstance(servers, dict):
    sys.exit(f"zolly-mcp: {path}: mcpServers is not an object; merge by hand")

entry = {"type": "stdio", "command": dest, "args": [], "timeout": 120000,
         "instructions": True}
env = {}
if freq:
    env["ZOLLY_MCP_AGENT"] = f"zolly-{freq}"
    root = root or "."
if root:
    env["ZOLLY_ROOT"] = root
if env:
    entry["env"] = env
servers[key] = entry
if created and "$schema" not in data:
    data = {"$schema": SCHEMA, **data}

tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
os.replace(tmp, path)
PY
  echo "  ✔ registered: $WRITE/.omp/mcp.json ($ENTRY_KEY)"
else
  echo
  echo "Add this to ~/.omp/agent/mcp.json under \"mcpServers\" — one entry per"
  echo "office, under its own key; leave the other entries untouched:"
  echo
  cat <<JSON
    "$ENTRY_KEY": {
      "type": "stdio",
      "command": "$DEST",
      "args": [],
      "timeout": 120000,
      "instructions": true$ENV_JSON
    }
JSON
fi
