//! `zolly close` removes the worktree, deletes the branch, and closes only the
//! workspace attached to that worktree. Fake herdr keeps the test off the real
//! server; a temp git repo keeps it off every real office.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zolly-close-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).expect("write fake executable");
    let mut perms = fs::metadata(&path).expect("stat fake executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod fake executable");
}

#[test]
fn close_removes_worktree_branch_and_workspace() {
    let base = temp_dir("workspace");
    let repo = base.join("repo");
    let remote = base.join("remote.git");
    let bin = base.join("bin");
    let home = base.join("home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();

    git(&base, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join(".gitignore"), ".agents/worktrees/\n").unwrap();
    fs::create_dir_all(repo.join(".agents/results")).unwrap();
    fs::write(
        repo.join(".agents/results/t1.md"),
        "---\nid: t1\ntitle: test\nstatus: done\ngate: auto\n---\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);

    let worktree = repo.join(".agents/worktrees/t1");
    fs::create_dir_all(repo.join(".agents/worktrees")).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "foreman/t1",
            worktree.to_str().unwrap(),
            "main",
        ],
    );

    let other = base.join("other-office");
    fs::create_dir_all(other.join(".agents/worktrees/t1")).unwrap();
    let json = format!(
        "{{\"result\":{{\"worktrees\":[{{\"path\":{:?},\"branch\":\"foreman/t1\",\"open_workspace_id\":\"wX\",\"repo_root\":{:?}}},{{\"path\":{:?},\"branch\":\"foreman/t1\",\"open_workspace_id\":\"wOTHER\",\"repo_root\":{:?}}}]}}}}",
        worktree.to_string_lossy().to_string(),
        repo.to_string_lossy().to_string(),
        worktree.to_string_lossy().to_string(),
        other.to_string_lossy().to_string()
    );
    write_script(
        &bin,
        "herdr",
        "#!/bin/sh\ncase \"$1 $2\" in\n  'worktree list') printf '%s\\n' \"$WORKTREE_JSON\" ;;\n  'workspace close') printf '%s\\n' \"$3\" >> \"$CLOSE_LOG\" ;;\n  'agent list') printf '%s\\n' '{\"result\":{\"agents\":[]}}' ;;\n  *) exit 0 ;;\nesac\n",
    );

    let out = Command::new(env!("CARGO_BIN_EXE_zolly"))
        .args(["close", "t1"])
        .current_dir(&repo)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("WORKTREE_JSON", &json)
        .env("CLOSE_LOG", base.join("closed.log"))
        .output()
        .expect("run zolly close");

    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!worktree.exists(), "worktree should be gone");
    let branches = Command::new("git")
        .args(["branch", "--list", "foreman/t1"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
        "branch should be deleted"
    );
    let closed = fs::read_to_string(base.join("closed.log")).expect("workspace close called");
    assert!(closed.contains("wX"), "workspace close log: {closed}");
    assert!(
        !closed.contains("wOTHER"),
        "another office's workspace was closed: {closed}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("workspace wX closed"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let _ = fs::remove_dir_all(base);
}
