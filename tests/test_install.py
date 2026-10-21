#!/usr/bin/env python3
"""Installer integration tests: fresh install, repeat, force, collisions.

Each test builds a real throwaway git repository with a bare remote and a
throwaway HOME, so no machine-level state is touched. `--no-binary` and
`--no-systemd` keep the run offline and side-effect-free; `--yes` keeps every
run non-interactive, and one test drives the prompts on a real pty.
"""

import json
import os
import pty
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INSTALL = ROOT / "install.sh"

# A getUpdates body in the real shape: the same private chat twice (dedupe) and
# a supergroup with a negative id (must not be allow-listed as a person).
UPDATES = (
    '{"ok":true,"result":['
    '{"update_id":373388186,"message":{"message_id":64,'
    '"from":{"id":5530013864,"is_bot":false,"first_name":"Shifaul",'
    '"username":"khanshifaul"},"chat":{"id":5530013864,'
    '"first_name":"Shifaul","type":"private"},"date":1792567855,"text":"hi"}},'
    '{"update_id":373388187,"message":{"message_id":65,'
    '"from":{"id":5530013864,"is_bot":false,"first_name":"Shifaul",'
    '"username":"khanshifaul"},"chat":{"id":5530013864,'
    '"first_name":"Shifaul","type":"private"},"date":1792567860,"text":"again"}},'
    '{"update_id":373388188,"message":{"message_id":66,'
    '"from":{"id":999,"is_bot":false,"first_name":"Other"},'
    '"chat":{"id":-1001234567890,'
    '"title":"group","type":"supergroup"},"date":1792567870,"text":"hi"}}'
    "]}"
)

# Serves argv[1] on an ephemeral port and writes that port to argv[2]. A
# process, not a thread: the pty tests fork, and a forked child must not
# inherit a running server thread.
STUB = """
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

body = open(sys.argv[1], "rb").read()

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass

server = HTTPServer(("127.0.0.1", 0), Handler)
tmp = sys.argv[2] + ".tmp"
with open(tmp, "w") as f:
    f.write(str(server.server_address[1]))
os.replace(tmp, sys.argv[2])
server.serve_forever()
"""


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = dict(os.environ, HOME=str(self.home))
        # Never reach api.telegram.org from a test: a closed local port fails
        # the lookup in milliseconds, so the fallback prompt is deterministic.
        self.env["ZOLLY_TELEGRAM_API"] = "http://127.0.0.1:1"

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args, cwd=None):
        subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)

    def make_repo(self, name: str, branch: str = "main", commit: bool = True) -> Path:
        repo = self.root / name
        remote = self.root / (name + ".git")
        subprocess.run(["git", "init", "-q", "--bare", str(remote)], check=True)
        subprocess.run(["git", "init", "-q", "-b", branch, str(repo)], check=True)
        self.git("remote", "add", "origin", str(remote), cwd=repo)
        if commit:
            self.git("config", "user.email", "t@example.com", cwd=repo)
            self.git("config", "user.name", "Test", cwd=repo)
            self.git("commit", "-q", "--allow-empty", "-m", "init", cwd=repo)
            self.git("push", "-q", "-u", "origin", branch, cwd=repo)
        return repo

    def install(self, repo: Path, *args):
        # --yes: the suite must never block on a prompt when it runs from a
        # terminal; start_new_session drops /dev/tty as well.
        return subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--yes", "--founder", "Test", *args],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=60,
            start_new_session=True,
        )

    def test_fresh_install_creates_the_kit(self):
        repo = self.make_repo("app")
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((repo / ".agents" / "rules" / "queue-contract.md").is_file())
        config = (repo / ".agents" / "zolly" / "config.toml").read_text(encoding="utf-8")
        self.assertIn('frequency = "app"', config)
        self.assertIn('founder = "Test"', config)

    def test_second_install_without_force_refuses(self):
        repo = self.make_repo("app")
        self.assertEqual(self.install(repo).returncode, 0)
        second = self.install(repo)
        self.assertNotEqual(second.returncode, 0)
        self.assertIn("already exists", second.stderr)

    def test_force_preserves_memory_and_keeps_a_backup(self):
        repo = self.make_repo("app")
        self.assertEqual(self.install(repo).returncode, 0)

        added = subprocess.run(
            [str(repo / ".agents" / "shared" / "memory"), "add", "learning",
             "The office kept this entry across a reinstall."],
            cwd=repo, env=self.env, capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(added.returncode, 0, added.stderr)

        forced = self.install(repo, "--force")
        self.assertEqual(forced.returncode, 0, forced.stderr)
        learnings = (repo / ".agents" / "memory" / "learnings.md").read_text(encoding="utf-8")
        self.assertIn("kept this entry across a reinstall", learnings)
        self.assertTrue(list(repo.glob(".agents-backup-*")), "no backup directory was kept")

    def test_force_preserves_results_and_queue(self):
        repo = self.make_repo("app")
        self.assertEqual(self.install(repo).returncode, 0)
        (repo / ".agents" / "results" / "t-keep.md").write_text(
            "---\nstatus: done\n---\n", encoding="utf-8"
        )
        (repo / ".agents" / "queue" / "t-keep.md").write_text(
            "a queued task\n", encoding="utf-8"
        )
        (repo / ".agents" / "days" / "2026-10-21.json").write_text(
            "{}", encoding="utf-8"
        )
        forced = self.install(repo, "--force")
        self.assertEqual(forced.returncode, 0, forced.stderr)
        self.assertTrue(
            (repo / ".agents" / "results" / "t-keep.md").is_file(),
            "a reinstall must not drop finished work",
        )
        self.assertTrue(
            (repo / ".agents" / "queue" / "t-keep.md").is_file(),
            "a reinstall must not drop queued work",
        )
        self.assertTrue(
            (repo / ".agents" / "days" / "2026-10-21.json").is_file(),
            "a reinstall must not drop the day records",
        )

    def test_binary_source_is_found_in_the_kit(self):
        # The crate ships in tool/. A dry run proves the installer found it
        # without paying for a real build.
        repo = self.make_repo("app")
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-systemd", "--yes", "--founder", "Test", "--dry-run"],
            env=self.env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"would build {ROOT / 'tool'}", result.stdout)

    def test_token_without_chat_id_is_kept(self):
        # A token typed before the bot has ever been messaged used to be
        # thrown away, so the founder had to fetch it from BotFather again.
        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="123456:ABCdef")
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("token saved — no chat id yet", result.stdout)
        creds = (self.home / ".config" / "zolly" / "app" / "credentials").read_text(encoding="utf-8")
        self.assertIn("ZOLLY_TELEGRAM_TOKEN=123456:ABCdef\n", creds)
        self.assertIn("ZOLLY_ALLOWED_IDS=\n", creds)
        units = self.home / ".config" / "systemd" / "user"
        self.assertFalse(units.exists(), "no listener without a chat id")

    def test_empty_folder_without_git_is_refused(self):
        repo = self.root / "blank"
        repo.mkdir()
        result = self.install(repo)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a git repository", result.stderr)
        self.assertFalse((repo / ".git").exists(), "the installer must not create a repo")
        self.assertFalse((repo / ".agents").exists())

    def test_unborn_repo_installs_without_a_commit(self):
        repo = self.make_repo("fresh", commit=False)
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((repo / ".agents").is_dir())
        head = subprocess.run(
            ["git", "rev-parse", "--verify", "HEAD"], cwd=repo, capture_output=True
        )
        self.assertNotEqual(head.returncode, 0)

    def test_nonempty_folder_that_is_not_a_repo_is_refused(self):
        repo = self.root / "loose"
        repo.mkdir()
        (repo / "notes.txt").write_text("someone's files\n", encoding="utf-8")
        result = self.install(repo)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a git repository", result.stderr)
        self.assertFalse((repo / ".git").exists())

    def test_install_uses_the_branch_the_repo_is_on(self):
        repo = self.make_repo("legacy", branch="master")
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        config = (repo / ".agents" / "zolly" / "config.toml").read_text(encoding="utf-8")
        self.assertIn('base_branch = "master"', config)

    def test_no_credentials_skips_the_listener_unit(self):
        repo = self.make_repo("app")
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test"],
            env=self.env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("skip systemd (no Telegram credentials)", result.stdout)
        self.assertFalse(
            (self.home / ".config" / "systemd" / "user" / "zolly@app.service").exists()
        )

    def test_credentials_write_the_unit_with_restricted_modes(self):
        import stat

        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        cred_dir = self.home / ".config" / "zolly" / "app"
        cred_file = cred_dir / "credentials"
        self.assertEqual(stat.S_IMODE(cred_dir.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(cred_file.stat().st_mode), 0o600)
        self.assertTrue(
            (self.home / ".config" / "systemd" / "user" / "zolly@app.service").is_file()
        )
        unit = (self.home / ".config" / "systemd" / "user" / "zolly@app.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("telegram-serve\n", unit)
        self.assertNotIn("--webhook", unit)
        # The poll choice is recorded so a re-run does not ask for a webhook
        # URL again (and an empty value is a valid, remembered answer).
        creds = cred_file.read_text(encoding="utf-8")
        self.assertIn("ZOLLY_TELEGRAM_WEBHOOK_URL=\n", creds)

    def test_webhook_mode_wires_url_and_secret(self):
        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        bind = f"127.0.0.1:{self.free_port()}"
        result = subprocess.run(
            [
                str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test",
                "--webhook-url", "https://office.example.com/tg",
                "--webhook-bind", bind,
            ],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        creds = (
            self.home / ".config" / "zolly" / "app" / "credentials"
        ).read_text(encoding="utf-8")
        secret = [
            line.split("=", 1)[1]
            for line in creds.splitlines()
            if line.startswith("ZOLLY_TELEGRAM_WEBHOOK_SECRET=")
        ][0]
        self.assertTrue(secret, "a secret must be generated")
        unit = (self.home / ".config" / "systemd" / "user" / "zolly@app.service").read_text(
            encoding="utf-8"
        )
        self.assertIn(f"telegram-serve --webhook --bind {bind}", unit)
        self.assertIn(
            "telegram-set-webhook --url https://office.example.com/tg", result.stdout
        )
        self.assertIn("point the TLS proxy", result.stdout)
        self.assertIn(
            "ZOLLY_TELEGRAM_WEBHOOK_URL=https://office.example.com/tg\n", creds
        )

        # A re-run must keep the secret Telegram already has; rotating it here
        # would silently break every delivery.
        again = subprocess.run(
            [
                str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test",
                "--force", "--webhook-url", "https://office.example.com/tg",
                "--webhook-bind", bind,
            ],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(again.returncode, 0, again.stderr)
        creds_again = (
            self.home / ".config" / "zolly" / "app" / "credentials"
        ).read_text(encoding="utf-8")
        secret_again = [
            line.split("=", 1)[1]
            for line in creds_again.splitlines()
            if line.startswith("ZOLLY_TELEGRAM_WEBHOOK_SECRET=")
        ][0]
        self.assertEqual(secret_again, secret)

    def test_busy_webhook_bind_is_refused(self):
        import socket

        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        sock.listen(1)
        bind = f"127.0.0.1:{sock.getsockname()[1]}"
        try:
            result = subprocess.run(
                [
                    str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--yes",
                    "--founder", "Test",
                    "--webhook-url", "https://office.example.com/tg",
                    "--webhook-bind", bind,
                ],
                env=env, capture_output=True, text=True, timeout=60,
                start_new_session=True,
            )
        finally:
            sock.close()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already in use", result.stderr)

    def test_webhook_url_must_be_https(self):
        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        result = subprocess.run(
            [
                str(INSTALL), str(repo), "--no-binary", "--yes", "--founder", "Test",
                "--webhook-url", "http://office.example.com/tg",
            ],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must start with https", result.stderr)

    def test_invalid_webhook_url_reprompts_instead_of_aborting(self):
        repo = self.make_repo("app")
        # MCP no, Telegram yes, token, ids, a bad URL, then blank for poll.
        code, text = self.run_in_pty(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test"],
            b"n\ny\n123456:ABCdef\n1,2\nhttp://x\n\n",
        )
        self.assertEqual(code, 0, text)
        self.assertIn("must start with https", text)
        creds = (
            self.home / ".config" / "zolly" / "app" / "credentials"
        ).read_text(encoding="utf-8")
        self.assertIn("ZOLLY_TELEGRAM_WEBHOOK_URL=\n", creds)
        self.assertFalse(
            (self.home / ".config" / "systemd" / "user" / "zolly@app.service").exists()
        )

    def test_poll_choice_is_not_asked_again(self):
        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        first = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--yes", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(first.returncode, 0, first.stderr)
        # A re-run must read the remembered poll choice, not prompt again.
        code, text = self.run_in_pty(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test", "--force"],
            b"",
        )
        self.assertEqual(code, 0, text)
        self.assertNotIn("Webhook URL", text)

    def test_force_preserves_custom_skills_and_root_files(self):
        repo = self.make_repo("app")
        self.assertEqual(self.install(repo).returncode, 0)
        skill = repo / ".agents" / "skills" / "custom" / "SKILL.md"
        skill.parent.mkdir(parents=True)
        skill.write_text("custom skill\n", encoding="utf-8")
        helper = repo / ".agents" / "helper.py"
        helper.write_text("print(1)\n", encoding="utf-8")
        forced = self.install(repo, "--force")
        self.assertEqual(forced.returncode, 0, forced.stderr)
        self.assertTrue(skill.is_file(), "a reinstall must not drop office skills")
        self.assertTrue(helper.is_file(), "a reinstall must not drop office helpers")

    def test_force_preserves_a_project_local_tool_source(self):
        repo = self.make_repo("app")
        self.assertEqual(self.install(repo).returncode, 0)
        tool = repo / ".agents" / "zolly" / "tool"
        (tool / "src").mkdir(parents=True)
        (tool / "Cargo.toml").write_text('[package]\nname = "probe"\n', encoding="utf-8")
        (tool / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        result = self.install(repo, "--force")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((tool / "Cargo.toml").is_file())
        self.assertTrue((tool / "src" / "main.rs").is_file())
        self.assertIn("restored the tool source", result.stdout)

    def test_frequency_collision_is_refused(self):
        first = self.make_repo("app")
        second = self.make_repo("App")
        self.assertEqual(self.install(first).returncode, 0)
        clash = self.install(second)
        self.assertNotEqual(clash.returncode, 0)
        self.assertIn("already belongs", clash.stderr)


    def test_no_mcp_flag_skips_the_deployment(self):
        repo = self.make_repo("app")
        result = self.install(repo, "--no-mcp")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("skip MCP (--no-mcp)", result.stdout)
        self.assertFalse((self.home / ".local" / "bin" / "zolly-mcp").exists())

    def test_no_telegram_flag_skips_credentials_and_unit(self):
        repo = self.make_repo("app")
        env = dict(self.env, ZOLLY_TELEGRAM_TOKEN="dummy", ZOLLY_ALLOWED_IDS="1")
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-telegram", "--yes", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("skip Telegram (--no-telegram)", result.stdout)
        self.assertIn("skip systemd (Telegram is off)", result.stdout)
        self.assertFalse((self.home / ".config" / "zolly" / "app" / "credentials").exists())
        self.assertFalse(
            (self.home / ".config" / "systemd" / "user" / "zolly@app.service").exists()
        )

    def start_updates_stub(self):
        """Serve UPDATES from a child process; return its base URL."""
        body = self.root / "updates.json"
        body.write_text(UPDATES, encoding="utf-8")
        script = self.root / "stub_updates.py"
        script.write_text(STUB, encoding="utf-8")
        port_file = self.root / "stub_port"
        proc = subprocess.Popen([sys.executable, str(script), str(body), str(port_file)])
        self.addCleanup(proc.wait, 5)
        self.addCleanup(proc.terminate)
        deadline = time.time() + 5
        while not port_file.exists() and time.time() < deadline:
            time.sleep(0.02)
        self.assertTrue(port_file.exists(), "the getUpdates stub did not start")
        return "http://127.0.0.1:%s" % port_file.read_text(encoding="utf-8").strip()

    def test_chat_id_is_derived_from_the_token(self):
        # The token already reaches getUpdates, which names the chat. With no
        # ZOLLY_ALLOWED_IDS the installer must ask Telegram, not the founder.
        repo = self.make_repo("app")
        self.env["ZOLLY_TELEGRAM_API"] = self.start_updates_stub()
        self.env["ZOLLY_TELEGRAM_TOKEN"] = "123456:ABCdef"
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--yes", "--founder", "Test"],
            env=self.env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("chat ids: 5530013864", result.stdout)
        self.assertIn("handle: @khanshifaul", result.stdout)
        creds = (self.home / ".config" / "zolly" / "app" / "credentials").read_text(encoding="utf-8")
        self.assertIn("ZOLLY_ALLOWED_IDS=5530013864\n", creds)
        self.assertIn("ZOLLY_FOUNDER_USERNAME=khanshifaul\n", creds)

    def test_discovered_chat_id_is_confirmed_interactively(self):
        repo = self.make_repo("app")
        self.env["ZOLLY_TELEGRAM_API"] = self.start_updates_stub()
        self.env["ZOLLY_TELEGRAM_TOKEN"] = "123456:ABCdef"
        # MCP no, Telegram yes (the token makes the default yes), ids yes.
        code, text = self.run_in_pty(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test"],
            b"n\ny\ny\n\n",
        )
        self.assertEqual(code, 0, text)
        self.assertIn("[Y/n]", text)
        self.assertIn("Use these ids?", text)
        creds = (self.home / ".config" / "zolly" / "app" / "credentials").read_text(encoding="utf-8")
        self.assertIn("ZOLLY_ALLOWED_IDS=5530013864\n", creds)
        self.assertIn("ZOLLY_FOUNDER_USERNAME=khanshifaul\n", creds)
        self.assertNotIn("-1001234567890", creds)

    def test_explicit_founder_handle_wins_and_loses_the_at(self):
        repo = self.make_repo("app")
        env = dict(
            self.env,
            ZOLLY_TELEGRAM_TOKEN="dummy",
            ZOLLY_ALLOWED_IDS="1",
            ZOLLY_FOUNDER_USERNAME="@khanshifaul",
        )
        result = subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--yes", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
            start_new_session=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        creds = (self.home / ".config" / "zolly" / "app" / "credentials").read_text(encoding="utf-8")
        self.assertIn("ZOLLY_FOUNDER_USERNAME=khanshifaul\n", creds)

    def test_mcp_is_registered_in_the_project(self):
        # omp reads .omp/mcp.json at the project scope, so the installer
        # writes the entry instead of asking the founder to paste it into
        # ~/.omp/agent/mcp.json — and the entry is machine-local, so ignored.
        repo = self.make_repo("app")
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads((repo / ".omp" / "mcp.json").read_text(encoding="utf-8"))
        entry = config["mcpServers"]["zolly-app"]
        self.assertEqual(entry["command"], str(self.home / ".local" / "bin" / "zolly-mcp"))
        self.assertEqual(entry["env"]["ZOLLY_MCP_AGENT"], "zolly-app")
        self.assertEqual(entry["env"]["ZOLLY_ROOT"], str(repo))
        self.assertIn(".omp/mcp.json", (repo / ".gitignore").read_text(encoding="utf-8"))

    def test_mcp_registration_merges_into_an_existing_file(self):
        repo = self.make_repo("app")
        (repo / ".omp").mkdir()
        (repo / ".omp" / "mcp.json").write_text(
            json.dumps({"mcpServers": {"other": {"type": "stdio", "command": "x"}}}),
            encoding="utf-8",
        )
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads((repo / ".omp" / "mcp.json").read_text(encoding="utf-8"))
        self.assertIn("other", config["mcpServers"])
        self.assertIn("zolly-app", config["mcpServers"])

    def test_no_mcp_skips_project_registration(self):
        repo = self.make_repo("app")
        result = self.install(repo, "--no-mcp")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((repo / ".omp" / "mcp.json").exists())

    def test_yes_installs_with_defaults_and_asks_nothing(self):
        repo = self.make_repo("app")
        result = self.install(repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        # No prompt text may reach the log when the defaults are taken.
        self.assertNotIn("[Y/n]", result.stdout)
        self.assertNotIn("[y/N]", result.stdout)
        # MCP is a default-yes feature; Telegram is a default-no feature.
        self.assertTrue((self.home / ".local" / "bin" / "zolly-mcp").is_file())
        self.assertFalse((self.home / ".config" / "zolly" / "app" / "credentials").exists())
        self.assertIn("skip Telegram (declined)", result.stdout)

    def free_port(self):
        import socket

        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        sock.close()
        return port

    def run_in_pty(self, argv, answers, timeout=60):
        """Run argv on a pty so /dev/tty is a terminal; feed it `answers`."""
        pid, fd = pty.fork()
        if pid == 0:
            try:
                os.execvpe(argv[0], argv, self.env)
            finally:
                os._exit(127)
        out = b""
        status = None
        try:
            os.write(fd, answers)
            deadline = time.time() + timeout
            while True:
                if time.time() > deadline:
                    os.kill(pid, signal.SIGKILL)
                    self.fail("interactive install did not finish in time")
                ready, _, _ = select.select([fd], [], [], 0.2)
                if not ready:
                    continue
                try:
                    chunk = os.read(fd, 4096)
                except OSError:
                    break
                if not chunk:
                    break
                out += chunk
            _, status = os.waitpid(pid, 0)
        finally:
            os.close(fd)
        code = os.WEXITSTATUS(status) if status is not None and os.WIFEXITED(status) else -1
        return code, out.decode("utf-8", "replace")

    def test_interactive_answers_can_skip_features(self):
        repo = self.make_repo("app")
        code, text = self.run_in_pty(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test"],
            b"n\nn\n",
        )
        self.assertEqual(code, 0, text)
        self.assertIn("Deploy zolly-mcp?", text)
        self.assertIn("Configure Telegram?", text)
        self.assertIn("skip MCP (declined)", text)
        self.assertIn("skip Telegram (declined)", text)
        self.assertFalse((self.home / ".local" / "bin" / "zolly-mcp").exists())
        self.assertFalse((self.home / ".config" / "zolly" / "app" / "credentials").exists())


    def install_with_pasted_token(self, name, payload):
        repo = self.make_repo(name)
        # MCP no, Telegram yes, token payload, allowed ids.
        answers = b"n\ny\n" + payload + b"1,2\n\n"
        code, text = self.run_in_pty(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test"],
            answers,
        )
        self.assertEqual(code, 0, text)
        creds = (self.home / ".config" / "zolly" / name / "credentials").read_text(encoding="utf-8")
        self.assertIn("ZOLLY_TELEGRAM_TOKEN=123456:ABCdef\n", creds)
        self.assertIn("ZOLLY_ALLOWED_IDS=1,2\n", creds)
        return text

    def test_pasted_token_is_used_verbatim(self):
        # Modern terminals wrap a paste in ESC[200~ … ESC[201~; the markers are
        # part of the line a plain read() sees, and must not reach the file.
        text = self.install_with_pasted_token("paste-one", b"\x1b[200~123456:ABCdef\x1b[201~\n")
        self.assertIn("123456:ABCdef", text)
        self.assertNotIn("characters entered", text)

    def test_pasted_token_split_by_a_clipboard_newline_is_used_verbatim(self):
        # A clipboard that ends in a newline splits the paste across two reads.
        self.install_with_pasted_token(
            "paste-two", b"\x1b[200~123456:ABCdef\n\x1b[201~\n"
        )


if __name__ == "__main__":
    unittest.main()
