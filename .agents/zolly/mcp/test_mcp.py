#!/usr/bin/env python3
"""test_mcp.py — a stdio client for zolly_mcp.py.

The server's own handshake is the only proof an MCP entry works: presence in
~/.omp/agent/mcp.json is not evidence (the sibling `deepseek` entry sat in
config and was dead in every pane until 2026-10-14).

    python3 test_mcp.py handshake                       # initialize + tools/list
    python3 test_mcp.py status                          # call zolly_status
    python3 test_mcp.py say "text" [--wait-ms N]        # call zolly_say

The server defaults to $ZOLLY_MCP_SERVER, then ~/.local/bin/zolly-mcp, then the
zolly_mcp.py beside this file. Override with --server PATH.

Exit codes: 0 ok | 1 the server said no / the tool failed | 2 usage or timeout.
Stdlib only, same as the server.
"""

import argparse
import json
import os
import select
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SERVER = os.environ.get("ZOLLY_MCP_SERVER") or os.path.join(
    os.path.expanduser("~"), ".local", "bin", "zolly-mcp"
)
FALLBACK_SERVER = os.path.join(HERE, "zolly_mcp.py")


class Client:
    def __init__(self, server: str, timeout: float = 30.0):
        self.timeout = timeout
        argv = [server] if os.access(server, os.X_OK) else [sys.executable, server]
        self.proc = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._next_id = 0

    def send(self, method: str, params: dict | None = None, notification: bool = False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notification:
            self._next_id += 1
            msg["id"] = self._next_id
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        return msg.get("id")

    def read(self) -> dict:
        ready, _, _ = select.select([self.proc.stdout], [], [], self.timeout)
        if not ready:
            raise TimeoutError("no response in %.0fs" % self.timeout)
        line = self.proc.stdout.readline()
        if not line:
            err = self.proc.stderr.read().strip()
            raise RuntimeError("server closed stdout%s" % (": " + err if err else ""))
        return json.loads(line)

    def call(self, method: str, params: dict | None = None) -> dict:
        msg_id = self.send(method, params)
        while True:
            msg = self.read()
            if msg.get("id") == msg_id:
                return msg
            # Notifications and server-initiated messages are not our reply.

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def handshake(client: Client) -> int:
    init = client.call(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test_mcp", "version": "1"},
        },
    )
    if "error" in init:
        print("initialize failed: %s" % init["error"], file=sys.stderr)
        return 1
    info = init["result"].get("serverInfo", {})
    print("initialize ok: %s %s (protocol %s)" % (
        info.get("name"), info.get("version"), init["result"].get("protocolVersion")))

    client.send("notifications/initialized", notification=True)

    listed = client.call("tools/list")
    if "error" in listed:
        print("tools/list failed: %s" % listed["error"], file=sys.stderr)
        return 1
    tools = {t["name"]: t for t in listed["result"].get("tools", [])}
    missing = {"zolly_say", "zolly_status"} - set(tools)
    if missing:
        print("tools/list missing: %s" % ", ".join(sorted(missing)), file=sys.stderr)
        return 1
    for name in sorted(tools):
        required = tools[name].get("inputSchema", {}).get("required", [])
        print("tool %s: required=%s" % (name, required or "-"))
    if tools["zolly_say"].get("inputSchema", {}).get("required") != ["text"]:
        print("zolly_say must require `text`", file=sys.stderr)
        return 1
    print("handshake ok")
    return 0


def call_tool(client: Client, name: str, args: dict) -> int:
    reply = client.call("tools/call", {"name": name, "arguments": args})
    if "error" in reply:
        print("%s failed: %s" % (name, reply["error"]), file=sys.stderr)
        return 1
    result = reply["result"]
    body = result.get("structuredContent")
    if body is None:
        body = result.get("content", [{}])[0].get("text", "")
    print(json.dumps(body, indent=2, sort_keys=False) if isinstance(body, dict) else body)
    return 1 if result.get("isError") else 0


def resolve_server(explicit: str | None) -> str:
    if explicit:
        return explicit
    if os.path.isfile(DEFAULT_SERVER):
        return DEFAULT_SERVER
    if os.path.isfile(FALLBACK_SERVER):
        return FALLBACK_SERVER
    raise SystemExit("test_mcp: no server — pass --server PATH or install ~/.local/bin/zolly-mcp")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--server", help="path to the zolly-mcp executable")
    parser.add_argument("--timeout", type=float, default=30.0)
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("handshake", help="initialize + tools/list")
    sub.add_parser("status", help="call zolly_status")
    say = sub.add_parser("say", help="call zolly_say")
    say.add_argument("text")
    say.add_argument("--wait-ms", type=int, default=0)
    args = parser.parse_args(argv)

    client = Client(resolve_server(args.server), timeout=args.timeout)
    try:
        if args.cmd == "handshake":
            return handshake(client)
        if args.cmd == "status":
            if handshake(client) != 0:
                return 1
            return call_tool(client, "zolly_status", {})
        if handshake(client) != 0:
            return 1
        return call_tool(
            client, "zolly_say", {"text": args.text, "wait_submit_ms": args.wait_ms}
        )
    except TimeoutError as e:
        print("test_mcp: %s" % e, file=sys.stderr)
        return 2
    except RuntimeError as e:
        print("test_mcp: %s" % e, file=sys.stderr)
        return 2
    finally:
        client.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
