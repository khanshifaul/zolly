#!/usr/bin/env python3
"""Installer integration tests: fresh install, repeat, force, collisions.

Each test builds a real throwaway git repository with a bare remote and a
throwaway HOME, so no machine-level state is touched. `--no-binary` and
`--no-systemd` keep the run offline and side-effect-free.
"""

import os
import subprocess
import tempfile
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
        return subprocess.run(
            [str(INSTALL), str(repo), "--no-binary", "--no-systemd", "--founder", "Test", *args],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=60,
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
            [str(INSTALL), str(repo), "--no-binary", "--founder", "Test"],
            env=self.env, capture_output=True, text=True, timeout=60,
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
            [str(INSTALL), str(repo), "--no-binary", "--founder", "Test"],
            env=env, capture_output=True, text=True, timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        cred_dir = self.home / ".config" / "zolly" / "app"
        cred_file = cred_dir / "credentials"
        self.assertEqual(stat.S_IMODE(cred_dir.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(cred_file.stat().st_mode), 0o600)
        self.assertTrue(
            (self.home / ".config" / "systemd" / "user" / "zolly@app.service").is_file()
        )

    def test_frequency_collision_is_refused(self):
        first = self.make_repo("app")
        second = self.make_repo("App")
        self.assertEqual(self.install(first).returncode, 0)
        clash = self.install(second)
        self.assertNotEqual(clash.returncode, 0)
        self.assertIn("already belongs", clash.stderr)


if __name__ == "__main__":
    unittest.main()
