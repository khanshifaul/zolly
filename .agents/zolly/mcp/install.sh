#!/bin/sh
# Deploy zolly-mcp to a stable path and print the mcp.json entry to add.
#
# Deployed as a copy (not a symlink) so the registered command survives the
# foreman worktree being garbage-collected after the PR merges. Re-run this
# after editing zolly_mcp.py; the running server is the file at DEST.
#
# Usage: install.sh [DEST] [--frequency <freq>] [--root <repo>]
#   --frequency adds ZOLLY_MCP_AGENT=zolly-<freq> to the printed entry, so the
#   server reaches this office's manager rather than the first bare `zolly`.
#   --root adds ZOLLY_ROOT, so the server resolves `zolly pulse` without
#   depending on the MCP client's working directory.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DEST="$HOME/.local/bin/zolly-mcp"
FREQ=""
ROOT=""

while [ $# -gt 0 ]; do
  case "$1" in
    --frequency) FREQ="${2:-}"; shift 2 ;;
    --root)      ROOT="${2:-}"; shift 2 ;;
    -*)          echo "usage: install.sh [DEST] [--frequency <freq>] [--root <repo>]" >&2; exit 1 ;;
    *)           DEST="$1"; shift ;;
  esac
done

mkdir -p "$(dirname "$DEST")"
install -m 0755 "$HERE/zolly_mcp.py" "$DEST"
echo "installed: $DEST"

ENV_JSON=""
if [ -n "$FREQ" ]; then
  ENV_JSON=",
      \"env\": { \"ZOLLY_MCP_AGENT\": \"zolly-$FREQ\", \"ZOLLY_ROOT\": \"${ROOT:-.}\" }"
fi

echo
echo "Add this to ~/.omp/agent/mcp.json under \"mcpServers\" (one entry;"
echo "leave the existing \"deepseek\" entry untouched):"
echo
cat <<JSON
    "zolly": {
      "type": "stdio",
      "command": "$DEST",
      "args": [],
      "timeout": 120000,
      "instructions": true$ENV_JSON
    }
JSON
