# zolly-mcp

A stdio MCP server that reaches Zolly in her herdr pane, so any omp seat can
hand her a message through a typed tool instead of a raw shell call.

```
zolly_say(text)    → herdr agent prompt zolly-<frequency> <text>   (fire-and-forget; reports what happened)
zolly_status()     → zolly pulse --json                (the office read, same shape as the CLI)
```

## Why a typed interface

The only path into Zolly's pane was `herdr agent prompt zolly-<frequency> "<text>"` — used by
the Telegram listener and implemented as `HerdrClient::agent_prompt_only` in
the zolly Rust tool. A shell call has no contract: on 2026-10-14 a timed-out
`zolly foreman` returned a *pane fragment* (`TODO` + box-drawing) instead of a
failure, so a caller could not tell "the agent replied" from "the agent is
busy". This server wraps the same primitive and reports its outcome —
`delivered`, the exact error, and the pane revision before/after.

## Reading a `zolly_say` result

The authoritative field is **`delivered`** (with `herdr_result_type:
"agent_prompted"`): it is herdr's own ack that the text was submitted. The rest
are observations made inside the optional `wait_submit_ms` window, so a `false`
means "not seen in that window", never "did not happen":

|Field|Means|
|---|---|
|`delivered`, `herdr_result_type`|herdr accepted the submission (`agent_prompted`) — the ack|
|`working_before`|Zolly was already mid-turn when the text was sent|
|`revision_before/after`, `revision_changed`|the pane emitted output inside the window|
|`working_glyph_seen`|the working glyph appeared inside the window|
|`text_seen_on_screen`|the text appeared on the pane's screen inside the window|
|`notes`|anything the caller should not misread (mid-turn, unobserved glyph)|

Pass `wait_submit_ms: 0` for a pure fire-and-forget that only reports the ack.

## Files

|File|What|
|---|---|
|`zolly_mcp.py`|the server — stdlib only, no third-party imports|
|`test_mcp.py`|a stdio client: `handshake`, `status`, `say "text"`|
|`install.sh`|copies `zolly_mcp.py` to `~/.local/bin/zolly-mcp` and prints the mcp.json entry|

## Bounds and binding

The server is local stdio, but it still bounds every input:

| Limit | Value |
|---|---|
| `zolly_say` text | 64 KiB |
| `wait_submit_ms` | integer, 0–5000 (booleans and floats are refused) |
| JSON-RPC line | 1 MiB |
| returned diagnostics | 2 KiB, credential-shaped strings redacted |

The agent is **bound to the project**: when `.agents/zolly/config.toml` resolves
from `ZOLLY_ROOT` (or the cwd), the manager is `zolly-<frequency>`. A conflicting
`ZOLLY_MCP_AGENT` is ignored with a warning, so an inherited environment cannot
point this server at another office. The override is honoured only when no
project config resolves.

## Install

```sh
.agents/zolly/mcp/install.sh          # installs to ~/.local/bin/zolly-mcp
```

Then add the printed entry to `~/.omp/agent/mcp.json` under `mcpServers`.
That file is harness config **shared by every omp pane** — read it first, add one
entry, leave the other entries untouched, keep it valid JSON. The entry is keyed
per office (`zolly-<frequency>`), because one unqualified `zolly` entry can only
reach one office; a second project needs its own entry under its own key.

## Verify a handshake (not just "the entry exists")

```sh
python3 .agents/zolly/mcp/test_mcp.py handshake
python3 .agents/zolly/mcp/test_mcp.py status
python3 .agents/zolly/mcp/test_mcp.py say "MCP reach probe — no action required; do not act on this."
```

`test_mcp.py` defaults to `~/.local/bin/zolly-mcp` and falls back to the
`zolly_mcp.py` beside it. Pass `--server PATH` to test an uninstalled copy.

The sibling `deepseek` entry was present in config but **dead in every pane** until
2026-10-14 (the interpreter it names lacked `requests` + `wasmtime`). Presence in
config is not evidence; an `initialize` + `tools/list` round trip is.

## Why stdlib-only

So there is nothing to be missing. The server needs only `python3` and the
`herdr` / `zolly` binaries that are already on the box. `zolly_mcp.py` is the
source of truth; `install.sh` deploys a copy, because a copy survives the foreman
worktree being garbage-collected after the PR merges, while a symlink into it
would not. Re-run `install.sh` after editing the source.

## Deliberately not wired into the Telegram webhook path

The Telegram webhook receiver (`t-telegram-webhook-transport`) delivers through
`handle_update`, which already reaches Zolly via `herdr`. Routing that through
MCP would add a hop to the path the founder is trying to make *faster*. This
server is for agents and other callers, not for the receiver's hot path.

## Mid-turn sends (measured 2026-10-14)

Sending while Zolly is mid-turn does **not** refuse and does **not** wedge the
pane. `herdr agent prompt` acks with `agent_prompted` either way; omp's TUI then
queues the text for the turn already running (two probes sent mid-turn were both
received, in order and intact, and acknowledged together) and the input box is
left empty. `herdr agent prompt` refuses with `agent_blocked` only when the agent
is *blocked*, not when it is merely working.

Full evidence: the zolly Rust tool's own test suite, and the office's
`.agents/results/` entries from the sessions that produced this server.
