//! Deterministic signals for an idle-curiosity pass.
//!
//! The manager asks the questions; this module only measures. Every field is
//! something the tree can prove, so an inquiry can cite `file:line` instead of
//! a hunch. No model call, no network.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// Enough dirty files to see the shape, not a second `git status`.
const DIRTY_LIMIT: usize = 25;
/// Recent commits are orientation, not a changelog.
const COMMIT_LIMIT: usize = 5;
/// A TODO list is a prompt, not a backlog dump.
const TODO_LIMIT: usize = 25;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Curiosity {
    /// Task files whose frontmatter is not settled.
    pub queue_pending: usize,
    /// `git status --porcelain` entries, capped.
    pub dirty: Vec<String>,
    /// `git log` subjects, newest first.
    pub recent_commits: Vec<String>,
    /// `path:line: text` for TODO/FIXME/HACK in source files.
    pub code_todos: Vec<String>,
    /// `TODO` occurrences in `.agents/rules/project.md` (unfilled truths).
    pub project_todos: usize,
    /// Lines beginning with `- ` in `OPEN-QUESTIONS.md`.
    pub open_questions: usize,
    pub readme: bool,
    /// `test-command:` from project.md, when it is not a TODO.
    pub test_command: Option<String>,
}

/// Inspect `root`. Missing files and a non-git tree are empty signals, not
/// errors: the manager should still be able to ask something.
pub fn inspect(root: &Path) -> Result<Curiosity> {
    let project = std::fs::read_to_string(root.join(".agents/rules/project.md")).unwrap_or_default();
    let project_todos = project.matches("TODO").count();
    let test_command = project
        .lines()
        .find_map(|line| line.strip_prefix("test-command:"))
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "TODO")
        .map(str::to_string);

    let questions = std::fs::read_to_string(root.join("OPEN-QUESTIONS.md")).unwrap_or_default();
    let open_questions = questions
        .lines()
        .filter(|line| line.trim_start().starts_with("- "))
        .count();

    Ok(Curiosity {
        queue_pending: pending_queue(root),
        dirty: git_lines(root, &["status", "--porcelain"], DIRTY_LIMIT),
        recent_commits: git_lines(root, &["log", "-n", "5", "--format=%h %s"], COMMIT_LIMIT),
        code_todos: code_todos(root),
        project_todos,
        open_questions,
        readme: root.join("README.md").is_file(),
        test_command,
    })
}

fn pending_queue(root: &Path) -> usize {
    let dir = root.join(".agents/queue");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .filter(|entry| {
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                return true;
            };
            !crate::queue::parse_frontmatter(&content).settled
        })
        .count()
}

fn code_todos(root: &Path) -> Vec<String> {
    git_lines(
        root,
        &[
            "grep",
            "-n",
            "-I",
            "-E",
            "TODO|FIXME|HACK",
            "--",
            "*.rs",
            "*.py",
            "*.ts",
            "*.tsx",
            "*.js",
            "*.go",
            "*.sh",
        ],
        TODO_LIMIT,
    )
}

fn git_lines(root: &Path, args: &[&str], limit: usize) -> Vec<String> {
    let Ok(output) = Command::new("git").args(args).current_dir(root).output() else {
        return Vec::new();
    };
    if !output.status.success() && output.stdout.is_empty() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(limit)
        .map(str::to_string)
        .collect()
}

pub fn render(curiosity: &Curiosity) -> String {
    let mut out = format!(
        "curiosity: queue={} dirty={} commits={} code_todos={} project_todos={} open_questions={}\n",
        curiosity.queue_pending,
        curiosity.dirty.len(),
        curiosity.recent_commits.len(),
        curiosity.code_todos.len(),
        curiosity.project_todos,
        curiosity.open_questions,
    );
    match &curiosity.test_command {
        Some(command) => out.push_str(&format!("  test-command: {command}\n")),
        None => out.push_str("  test-command: missing or TODO\n"),
    }
    if !curiosity.readme {
        out.push_str("  README: missing\n");
    }
    for line in &curiosity.dirty {
        out.push_str(&format!("  dirty: {line}\n"));
    }
    for line in &curiosity.code_todos {
        out.push_str(&format!("  todo: {line}\n"));
    }
    for line in &curiosity.recent_commits {
        out.push_str(&format!("  commit: {line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "zolly-curious-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn measures_queue_todos_and_project_truths() {
        let dir = temp_repo("signals");
        std::fs::write(dir.join("README.md"), "# app\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "// TODO: handle retries\n").unwrap();
        std::fs::create_dir_all(dir.join(".agents/queue")).unwrap();
        std::fs::write(
            dir.join(".agents/queue/todo.md"),
            "---\nid: todo\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".agents/queue/done.md"),
            "---\nid: done\nstatus: done\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join(".agents/rules")).unwrap();
        std::fs::write(
            dir.join(".agents/rules/project.md"),
            "test-command: cargo test\n\n# TODO: architecture\n",
        )
        .unwrap();
        std::fs::write(dir.join("OPEN-QUESTIONS.md"), "- [ ] one\n- [x] two\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "seed"]);

        let signals = inspect(&dir).unwrap();
        assert_eq!(signals.queue_pending, 1);
        assert_eq!(signals.project_todos, 1);
        assert_eq!(signals.open_questions, 2);
        assert_eq!(signals.test_command.as_deref(), Some("cargo test"));
        assert!(signals.readme);
        assert!(signals.code_todos.iter().any(|line| line.contains("src/a.rs")));
        assert!(!signals.recent_commits.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_non_git_tree_is_empty_not_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "zolly-curious-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let signals = inspect(&dir).unwrap();
        assert_eq!(signals.queue_pending, 0);
        assert!(signals.dirty.is_empty());
        assert!(signals.code_todos.is_empty());
        assert_eq!(signals.test_command, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
