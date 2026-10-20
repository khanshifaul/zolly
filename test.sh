#!/bin/sh
# One command for the kit's own checks. Run from anywhere.
set -eu
cd "$(dirname "$0")"

echo "== syntax"
sh -n install.sh
sh -n .agents/zolly/mcp/install.sh
bash -n .agents/shared/memory
python3 -m py_compile \
  .agents/hr/hr.py \
  .agents/zolly/mcp/zolly_mcp.py \
  .agents/zolly/mcp/test_mcp.py \
  tests/*.py
echo "ok"

echo "== unit tests"
python3 -m unittest discover -s tests -v

echo "== mcp handshake"
python3 .agents/zolly/mcp/test_mcp.py handshake
