#!/usr/bin/env python3
"""Installer integration tests: fresh install, repeat, force, collisions.

Each test builds a real throwaway git repository with a bare remote and a
throwaway HOME, so no machine-level state is touched. `--no-binary` and
`--no-systemd` keep the run offline and side-effect-free; `--yes` keeps every
run non-interactive, and one test drives the prompts on a real pty.
"""

import os
import pty
import select
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INSTALL = ROOT / "install.sh"


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = dict(os.environ, HOME=str(self.home))

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
        self.assertIn("Deploy the MCP server", text)
        self.assertIn("Configure Telegram for this office?", text)
        self.assertIn("skip MCP (declined)", text)
        self.assertIn("skip Telegram (declined)", text)
        self.assertFalse((self.home / ".local" / "bin" / "zolly-mcp").exists())
        self.assertFalse((self.home / ".config" / "zolly" / "app" / "credentials").exists())


    def install_with_pasted_token(self, name, payload):
        repo = self.make_repo(name)
        # MCP no, Telegram yes, token payload, allowed ids.
        answers = b"n\ny\n" + payload + b"1,2\n"
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
        self.assertIn("characters entered", text)

    def test_pasted_token_split_by_a_clipboard_newline_is_used_verbatim(self):
        # A clipboard that ends in a newline splits the paste across two reads.
        self.install_with_pasted_token(
            "paste-two", b"\x1b[200~123456:ABCdef\n\x1b[201~\n"
        )


if __name__ == "__main__":
    unittest.main()
