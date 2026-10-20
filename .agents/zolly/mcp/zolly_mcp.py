#!/usr/bin/env python3
"""zolly-mcp — a stdio MCP server that reaches Zolly in her pane.

Two tools:

  zolly_say(text)   deliver `text` to Zolly's pane via `herdr agent prompt zolly-<frequency>`
                    and report what actually happened (delivered / the exact
                    error), fire-and-forget — no wait for a reply turn.
  zolly_status()    the office read: `zolly pulse --json`, shape-identical.

Why this exists: nothing exposed Zolly as a callable interface. The only path
into her pane was a raw shell call, `herdr agent prompt zolly-<frequency> "<text>"`, which
has no contract — on 2026-10-14 a timed-out `zolly foreman` returned a pane
fragment instead of a failure, and a caller could not tell "the agent replied"
from "the agent is busy" (t-foreman-reply-capture). This is a typed,
discoverable wrapper over the same primitive.

Stdlib only, no third-party imports. That is deliberate: the sibling MCP entry
(`deepseek`) was dead in every pane until 2026-10-14 because the interpreter it
named was missing `requests` + `wasmtime`. A stdlib-only server has nothing to
be missing, so the only requirement is a working `python3`.

Source of truth: `.agents/zolly/mcp/zolly_mcp.py` in the project that installed
this office. Deploy: `.agents/zolly/mcp/install.sh` (copies this file to
~/.local/bin/zolly-mcp). Registered in `~/.omp/agent/mcp.json` — that file is
outside the repo, see README.md in this directory.

Environment overrides (all optional):
  ZOLLY_MCP_AGENT       herdr agent name to reach   (default: zolly-<frequency>,
                                                      read from .agents/zolly/config.toml)
  ZOLLY_BIN             path to the `zolly` binary         (default: which zolly,
                                                          else ~/.local/bin/zolly)
  HERDR_BIN             path to the `herdr` binary         (default: which herdr)
  ZOLLY_ROOT            repo root for `zolly pulse`        (default: nearest
                                                          ancestor of cwd with
                                                          .agents/ + AGENTS.md)
  ZOLLY_MCP_PROMPT_TIMEOUT_S  seconds allowed for `herdr agent prompt`  (default: 20)
  ZOLLY_MCP_STATUS_TIMEOUT_S  seconds allowed for `zolly pulse --json`  (default: 30)
"""

import json
import os
import re
import shutil
import subprocess
import sys
import time

SERVER_NAME = "zolly-mcp"
SERVER_VERSION = "0.1.0"

# Bounds. The server is a local stdio process, but a client (or a bug) can
# still send an enormous message or a polling interval that parks the loop.
# A serial protocol loop must bound every input it reads.
MAX_TEXT_BYTES = 64 * 1024
MAX_WAIT_MS = 5000
MAX_LINE_BYTES = 1024 * 1024
MAX_DIAG = 2000


def _diag(text, limit=MAX_DIAG):
    """A diagnostic string safe to return over MCP: redacted and bounded."""
    text = _text(text)
    text = re.sub(
        r'(?i)((?:[A-Za-z_]*TOKEN|secret|password)\s*[=:]\s*)\S+',
        r'\1[redacted]',
        text,
    )
    text = re.sub(r'\b(ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9]{20,}\b', r'\1[redacted]', text)
    text = re.sub(r'\bsk-[A-Za-z0-9]{16,}\b', 'sk-[redacted]', text)
    return text[:limit]

DEFAULT_PROTOCOL = "2024-11-05"
KNOWN_PROTOCOLS = ("2024-11-05", "2025-03-26", "2025-06-18")

def _read_frequency(root):
    """The office frequency for `root`, or None if no office is found."""
    while True:
        cfg = os.path.join(root, ".agents", "zolly", "config.toml")
        try:
            with open(cfg, encoding="utf-8") as f:
                for line in f:
                    m = re.match(r'\s*frequency\s*=\s*"([^"]+)"', line)
                    if m and "{{" not in m.group(1):
                        return m.group(1)
        except OSError:
            pass
        parent = os.path.dirname(root)
        if parent == root:
            return None
        root = parent


def _agent_name():
    """The manager's herdr name, bound to the project this server runs in.

    herdr agent names are server-global, so every seat carries the office's
    frequency. When a project config is found it wins: an inherited or
    misconfigured ZOLLY_MCP_AGENT must not be able to point this server at
    another office. The override is honoured only when no project resolves
    (the single-office compatibility case).
    """
    root = os.path.realpath(os.environ.get("ZOLLY_ROOT") or os.getcwd())
    freq = _read_frequency(root)
    override = os.environ.get("ZOLLY_MCP_AGENT")
    if freq:
        expected = "zolly-" + freq
        if override and override != expected:
            sys.stderr.write(
                "zolly-mcp: ignoring ZOLLY_MCP_AGENT=%r; this project is %s\n"
                % (override, expected)
            )
        return expected
    return override or "zolly"


AGENT = _agent_name()
PROMPT_TIMEOUT_S = float(os.environ.get("ZOLLY_MCP_PROMPT_TIMEOUT_S", "20"))
STATUS_TIMEOUT_S = float(os.environ.get("ZOLLY_MCP_STATUS_TIMEOUT_S", "30"))

# The mid-turn status bar of an omp agent, measured against this office's seats
# (2026-10-14; `is_working_status_bar` and `screen_shows_working` in
# `.agents/zolly/tool/src/herdr.rs`):
#   at rest   `π > ◔ DeepSeek V4.1 Flash > 📁 <project> > ⑂ main …`
#   mid-turn  `⠏ 12s > ◔ DeepSeek V4.1 Flash > 📁 <project> > ⑂ main …`
# Only a braille spinner (U+2800–U+28FF) plus an elapsed-time token flips, and
# the trailing `>` keeps ordinary conversation text from matching. `herdr agent
# list` reports `agent_status: idle` for the whole turn, so the status bar is
# the only honest mid-turn signal.
WORKING_BAR_RE = re.compile(r"^\s*[\u2800-\u28ff]\s+(?:\d+[hms])+\s*>")

WORKING_BEFORE_NOTE = (
    "sent while the agent was mid-turn (working glyph visible on its status "
    "bar). Measured 2026-10-14: herdr accepts the submission anyway (it does "
    "not refuse — refusal is only for a blocked agent), and omp's TUI queues "
    "the text for the turn already running rather than dropping it or holding "
    "it unsubmitted. The pane does not wedge: the queued text is delivered to "
    "the running turn and the input box is left empty. See "
    ".agents/results/t-zolly-mcp-reach.md."
)

# `herdr agent prompt` is the delivery ack: a successful submission returns a
# result of type `agent_prompted`. Surfaced verbatim as `herdr_result_type`.
PROMPT_ACK_TYPE = "agent_prompted"


# ---------------------------------------------------------------------------
# Locating things
# ---------------------------------------------------------------------------


def tool_path(env_key, name, fallback=None):
    """Resolve an external binary: env override, then PATH, then a fixed path."""
    override = os.environ.get(env_key)
    if override:
        return override
    found = shutil.which(name)
    if found:
        return found
    return fallback


def find_main_root():
    """Repo root, for `zolly pulse`: env, else the nearest ancestor of cwd that
    holds `.agents/` and `AGENTS.md`, else cwd. `zolly` itself takes its root
    from the cwd (main.rs:96), so we hand it one explicitly."""
    override = os.environ.get("ZOLLY_ROOT")
    if override and os.path.isdir(os.path.join(override, ".agents")):
        return override
    d = os.getcwd()
    while True:
        if os.path.isdir(os.path.join(d, ".agents")) and os.path.isfile(
            os.path.join(d, "AGENTS.md")
        ):
            return d
        parent = os.path.dirname(d)
        if parent == d:
            return os.getcwd()
        d = parent


def herdr_bin():
    return tool_path("HERDR_BIN", "herdr", "/usr/bin/herdr")


def zolly_bin():
    return tool_path(
        "ZOLLY_BIN", "zolly", os.path.expanduser("~/.local/bin/zolly")
    )


# ---------------------------------------------------------------------------
# Subprocess plumbing
# ---------------------------------------------------------------------------


def _text(value):
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    return value or ""


def run(cmd, timeout_s, cwd=None):
    """Run a command, never raise. Returns a dict of what happened."""
    try:
        p = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            cwd=cwd,
        )
        return {
            "exit_code": p.returncode,
            "stdout": p.stdout or "",
            "stderr": p.stderr or "",
            "timed_out": False,
        }
    except subprocess.TimeoutExpired as e:
        # On POSIX, TimeoutExpired.stdout/stderr are bytes, not str.
        return {
            "exit_code": None,
            "stdout": _text(e.stdout),
            "stderr": _text(e.stderr),
            "timed_out": True,
        }
    except FileNotFoundError as e:
        return {
            "exit_code": None,
            "stdout": "",
            "stderr": "%s: not found (%s)" % (cmd[0], e),
            "timed_out": False,
        }
    except OSError as e:
        return {
            "exit_code": None,
            "stdout": "",
            "stderr": "%s: %s" % (cmd[0], e),
            "timed_out": False,
        }


def agent_entry(name):
    """The `herdr agent list` entry for `name`, or None."""
    res = run([herdr_bin(), "agent", "list"], timeout_s=10)
    if res["exit_code"] != 0:
        return None, res
    try:
        payload = json.loads(res["stdout"])
    except (ValueError, TypeError):
        return None, res
    agents = (payload.get("result") or {}).get("agents") or []
    for a in agents:
        if a.get("name") == name:
            return a, res
    return None, res


def agent_screen(name, lines=12):
    res = run(
        [herdr_bin(), "agent", "read", name, "--lines", str(lines), "--format", "text"],
        timeout_s=10,
    )
    if res["exit_code"] != 0:
        return ""
    return res["stdout"]


def is_working(screen):
    return any(WORKING_BAR_RE.match(line) for line in screen.splitlines())


def normalize(text):
    return re.sub(r"\s+", " ", text).strip()


# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------


def zolly_say(text, wait_submit_ms=2000):
    """Deliver `text` to Zolly's pane. Fire-and-forget over `herdr agent prompt`.

    Returns what actually happened: whether herdr accepted the submission, the
    exact error if it did not, and the pane revision before/after so a caller
    can see the pane reacted. `wait_submit_ms` only bounds a look for the
    working glyph — it is not a wait for a reply turn.
    """
    if not isinstance(text, str) or not text.strip():
        return {
            "ok": False,
            "agent": AGENT,
            "delivered": False,
            "error": "zolly_say requires non-empty text",
        }
    if len(text.encode("utf-8", "replace")) > MAX_TEXT_BYTES:
        return {
            "ok": False,
            "agent": AGENT,
            "delivered": False,
            "error": "text exceeds %d bytes" % MAX_TEXT_BYTES,
        }
    if isinstance(wait_submit_ms, bool) or not isinstance(wait_submit_ms, int):
        return {
            "ok": False,
            "agent": AGENT,
            "delivered": False,
            "error": "wait_submit_ms must be an integer in 0-%d" % MAX_WAIT_MS,
        }
    if wait_submit_ms < 0 or wait_submit_ms > MAX_WAIT_MS:
        return {
            "ok": False,
            "agent": AGENT,
            "delivered": False,
            "error": "wait_submit_ms must be in 0-%d" % MAX_WAIT_MS,
        }

    wait_ms = wait_submit_ms

    before, _ = agent_entry(AGENT)
    screen_before = agent_screen(AGENT, 8)
    working_before = is_working(screen_before)

    res = run(
        [herdr_bin(), "agent", "prompt", AGENT, text],
        timeout_s=PROMPT_TIMEOUT_S,
    )
    delivered = res["exit_code"] == 0 and not res["timed_out"]

    # A short, bounded look for the submission to land. Not a reply wait.
    working_glyph_seen = None
    text_seen_on_screen = None
    revision_after = None
    waited_ms = 0
    if wait_ms > 0:
        steps = max(1, wait_ms // 250)
        needle = normalize(text)[:200]
        for _ in range(steps):
            time.sleep(0.25)
            waited_ms += 250
            screen = agent_screen(AGENT, 12)
            if working_glyph_seen is None and is_working(screen):
                working_glyph_seen = True
            if needle and needle in normalize(screen):
                text_seen_on_screen = True
            # The pane's revision is the pane reacting at all. Track it during
            # the window: reading it once, right after the prompt, misses the
            # change by a beat and reports a delivery that landed as if the
            # pane never moved.
            entry, _ = agent_entry(AGENT)
            if entry and entry.get("revision") is not None:
                revision_after = entry["revision"]
            if working_glyph_seen and text_seen_on_screen:
                break
        if working_glyph_seen is None:
            working_glyph_seen = False
        if text_seen_on_screen is None:
            text_seen_on_screen = False

    after, _ = agent_entry(AGENT)
    revision_before = (before or {}).get("revision")
    if revision_after is None:
        revision_after = (after or {}).get("revision")

    ack_type = None
    try:
        ack_type = (
            (json.loads(res["stdout"]).get("result") or {}).get("type")
            if res["stdout"].strip()
            else None
        )
    except (ValueError, TypeError):
        ack_type = None

    notes = []
    if working_before:
        notes.append(WORKING_BEFORE_NOTE)
    if delivered and working_glyph_seen is False:
        notes.append(
            "no working glyph was observed within the %dms look window; delivery "
            "is still confirmed by herdr (%s), but a turn may start after the "
            "window — this flag is not proof of failure." % (waited_ms, ack_type or "exit 0")
        )

    out = {
        "ok": delivered,
        "agent": AGENT,
        "delivered": delivered,
        "exit_code": res["exit_code"],
        "timed_out": res["timed_out"],
        "stderr": _diag(res["stderr"]),
        "herdr_result_type": ack_type,
        "herdr_stdout": _diag(res["stdout"]),
        "pane_id": (after or before or {}).get("pane_id"),
        "revision_before": revision_before,
        "revision_after": revision_after,
        "revision_changed": (
            revision_before is not None
            and revision_after is not None
            and revision_before != revision_after
        ),
        "working_before": working_before,
        "working_glyph_seen": working_glyph_seen,
        "text_seen_on_screen": text_seen_on_screen,
        "waited_ms": waited_ms,
        "notes": notes,
    }
    if res["timed_out"]:
        out["error"] = "herdr agent prompt timed out after %ss" % PROMPT_TIMEOUT_S
    elif not delivered:
        out["error"] = "herdr agent prompt failed: %s" % (_diag(res["stderr"]) or ("exit %s" % res["exit_code"]))
    return out


def zolly_status():
    """The office read: parsed `zolly pulse --json`."""
    root = find_main_root()
    bins = zolly_bin()
    res = run([bins, "pulse", "--json"], timeout_s=STATUS_TIMEOUT_S, cwd=root)
    out = {
        "ok": res["exit_code"] == 0 and not res["timed_out"],
        "command": [bins, "pulse", "--json"],
        "cwd": root,
        "exit_code": res["exit_code"],
        "timed_out": res["timed_out"],
        "stderr": _diag(res["stderr"]),
    }
    if res["timed_out"]:
        out["error"] = "zolly pulse --json timed out after %ss" % STATUS_TIMEOUT_S
        return out
    if res["exit_code"] != 0:
        out["error"] = "zolly pulse --json failed: %s" % (_diag(res["stderr"]) or ("exit %s" % res["exit_code"]))
        return out
    try:
        out["json"] = json.loads(res["stdout"])
    except (ValueError, TypeError) as e:
        out["ok"] = False
        out["error"] = "zolly pulse --json returned unparseable JSON: %s" % e
        out["stdout"] = _diag(res["stdout"])
    return out


# ---------------------------------------------------------------------------
# MCP protocol (JSON-RPC 2.0, newline-delimited over stdio)
# ---------------------------------------------------------------------------

TOOLS = [
    {
        "name": "zolly_say",
        "description": (
            "Deliver a message to Zolly (the hourly operational seat) in her "
            "live herdr pane, via `herdr agent prompt zolly-<frequency>`. Fire-and-forget: "
            "it returns as soon as the text is submitted and does NOT wait for "
            "Zolly's reply turn. Returns what actually happened — whether the "
            "submission was accepted (herdr acks with an `agent_prompted` "
            "result), the exact error if not, the pane revision before and "
            "after, and whether Zolly was already mid-turn. Sending while she is "
            "mid-turn is safe and measured: herdr accepts it and omp's TUI "
            "queues the text for the turn already running; nothing is dropped "
            "and the pane does not wedge. Use this to hand Zolly an instruction "
            "or a note; read `delivered` before assuming it landed. It does not "
            "carry a reply — read the office with zolly_status() instead."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The message to deliver. Non-empty.",
                },
                "wait_submit_ms": {
                    "type": "integer",
                    "description": (
                        "How long (ms) to look for the submission to land "
                        "before returning, default 2000, 0 to skip the look "
                        "entirely. This is not a wait for a reply turn."
                    ),
                },
            },
            "required": ["text"],
            "additionalProperties": False,
        },
    },
    {
        "name": "zolly_status",
        "description": (
            "The office read, without prompting Zolly: runs `zolly pulse --json` "
            "and returns its JSON verbatim under `json` (same shape as the CLI). "
            "Use it for a cheap liveness/queue check before deciding whether to "
            "say anything to Zolly at all."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": False,
        },
    },
]


def tool_result(obj, is_error=False):
    text = obj if isinstance(obj, str) else json.dumps(obj, indent=2, sort_keys=False)
    result = {"content": [{"type": "text", "text": text}]}
    if isinstance(obj, dict):
        result["structuredContent"] = obj
    if is_error:
        result["isError"] = True
    return result


def handle_call(params):
    name = (params or {}).get("name")
    args = (params or {}).get("arguments") or {}
    try:
        if name == "zolly_say":
            return tool_result(zolly_say(args.get("text", ""), args.get("wait_submit_ms", 2000)))
        if name == "zolly_status":
            out = zolly_status()
            if "json" in out:
                return tool_result(json.dumps(out["json"], indent=2))
            return tool_result(out, is_error=True)
        return tool_result({"error": "unknown tool: %r" % (name,)}, is_error=True)
    except Exception as e:  # noqa: BLE001 - a tool must never kill the server
        return tool_result({"error": _diag("%s: %s" % (type(e).__name__, e))}, is_error=True)


def respond(msg_id, result=None, error=None):
    out = {"jsonrpc": "2.0", "id": msg_id}
    if error is not None:
        out["error"] = error
    else:
        out["result"] = result if result is not None else {}
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()


def handle(msg):
    if not isinstance(msg, dict):
        return
    method = msg.get("method")
    msg_id = msg.get("id")
    params = msg.get("params") or {}

    # Notifications carry no id and get no response.
    if msg_id is None:
        return

    if method == "initialize":
        requested = params.get("protocolVersion")
        version = requested if requested in KNOWN_PROTOCOLS else DEFAULT_PROTOCOL
        respond(
            msg_id,
            {
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                "instructions": (
                    "Reach Zolly in her pane. zolly_say(text) delivers a message "
                    "and reports whether it landed; zolly_status() reads the "
                    "office without prompting her."
                ),
            },
        )
    elif method == "ping":
        respond(msg_id, {})
    elif method == "tools/list":
        respond(msg_id, {"tools": TOOLS})
    elif method == "tools/call":
        respond(msg_id, handle_call(params))
    elif method in ("resources/list", "prompts/list"):
        # Advertised capabilities are tools-only; answer empty rather than error.
        key = method.split("/")[0]
        respond(msg_id, {key: []})
    else:
        respond(msg_id, error={"code": -32601, "message": "Method not found: %s" % method})


def main():
    for line in sys.stdin:
        if len(line.encode("utf-8", "replace")) > MAX_LINE_BYTES:
            respond(None, error={"code": -32600, "message": "request too large"})
            continue
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError as e:
            respond(None, error={"code": -32700, "message": "Parse error: %s" % e})
            continue
        handle(msg)


if __name__ == "__main__":
    main()
