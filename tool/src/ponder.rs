//! The idle "subconscious": pick one domain to think about, write evidence.
//!
//! This module never calls a model and never acts. It is a salience filter:
//! when the office is genuinely idle, it ranks the four inquiry domains from
//! measured signals and drops a focus note in `.agents/inbox/`. The manager
//! (or a cheap task-role subagent) reads the note and generates candidates.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::curious::{self, Curiosity};

const DEDUPE_SECS: u64 = 2 * 60 * 60;

#[derive(Debug, Serialize)]
pub struct Ponder {
    pub focus: String,
    pub reasons: Vec<String>,
    pub signals: Curiosity,
    /// None when the office is busy or the same focus was written recently.
    pub written: Option<String>,
    pub skipped: Option<String>,
}

#[derive(Debug, Clone)]
struct Focus {
    name: &'static str,
    score: usize,
    reasons: Vec<String>,
}

pub fn run(root: &Path, force: bool) -> Result<Ponder> {
    let signals = curious::inspect(root)?;
    let brief = crate::brief::generate_brief(root).context("Failed to read the brief")?;
    let busy = signals.queue_pending > 0
        || brief.waiting.workers > 0
        || !brief.gates.is_empty()
        || !brief.waiting.stalled_results.is_empty();
    if busy && !force {
        return Ok(Ponder {
            focus: "none".into(),
            reasons: vec![
                format!("queue_pending={}", signals.queue_pending),
                format!("workers={}", brief.waiting.workers),
                format!("gates={}", brief.gates.len()),
                format!("stalled_results={}", brief.waiting.stalled_results.len()),
            ],
            signals,
            written: None,
            skipped: Some("office is not idle".into()),
        });
    }

    let now = now_secs();
    let focus = pick_focus(&signals, now / 1800);
    if !force && recent_focus(root, &focus.name, now) {
        return Ok(Ponder {
            focus: focus.name.into(),
            reasons: focus.reasons,
            signals,
            written: None,
            skipped: Some("same focus written in the last 2h".into()),
        });
    }

    let inbox = root.join(".agents/inbox");
    std::fs::create_dir_all(&inbox).context("Failed to create the inbox")?;
    let path = inbox.join(format!("ponder-{now}.md"));
    let body = format!(
        "# Ponder — {}\n\n\
         generated: {now}\n\n\
         ## Why this domain\n\n{}\n\n\
         ## Signals\n\n```\n{}\n```\n\n\
         ## Next\n\n\
         The manager reads this note when idle and runs \
         `.agents/prompts/curiosity.md` against this focus. The note is a hint, \
         not a finding; verify every candidate against the tree.\n",
        focus.name,
        focus
            .reasons
            .iter()
            .map(|reason| format!("- {reason}"))
            .collect::<Vec<_>>()
            .join("\n"),
        curious::render(&signals),
    );
    std::fs::write(&path, body).context("Failed to write the ponder note")?;
    Ok(Ponder {
        focus: focus.name.into(),
        reasons: focus.reasons,
        signals,
        written: Some(path.display().to_string()),
        skipped: None,
    })
}

pub fn render(ponder: &Ponder) -> String {
    match (&ponder.written, &ponder.skipped) {
        (Some(path), _) => format!("ponder: {} — wrote {path}\n", ponder.focus),
        (None, Some(reason)) => format!("ponder: skipped ({reason})\n"),
        (None, None) => "ponder: no note\n".to_string(),
    }
}

fn pick_focus(signals: &Curiosity, tick: u64) -> Focus {
    let mut candidates = vec![
        Focus {
            name: "project truths",
            score: if signals.project_todos > 0 {
                20 + signals.project_todos
            } else {
                0
            },
            reasons: if signals.project_todos > 0 {
                vec![format!(
                    "project.md still has {} TODO truths",
                    signals.project_todos
                )]
            } else {
                Vec::new()
            },
        },
        Focus {
            name: "tests",
            score: if signals.test_command.is_none() { 15 } else { 0 },
            reasons: if signals.test_command.is_none() {
                vec!["project.md has no concrete test-command".into()]
            } else {
                Vec::new()
            },
        },
        Focus {
            name: "technical debt",
            score: signals.code_todos.len().min(10) * 2,
            reasons: if signals.code_todos.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{} code TODO/FIXME/HACK lines are tracked",
                    signals.code_todos.len()
                )]
            },
        },
        Focus {
            name: "business & domain",
            score: signals.recent_commits.len().min(5),
            reasons: if signals.recent_commits.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{} recent commits; the domain rules behind them may be unstated",
                    signals.recent_commits.len()
                )]
            },
        },
        Focus {
            name: "operations",
            score: if signals.readme { 0 } else { 8 },
            reasons: if signals.readme {
                Vec::new()
            } else {
                vec!["no README: how to run, test and deploy is unstated".into()]
            },
        },
        Focus {
            name: "architecture",
            score: signals.dirty.len().min(6),
            reasons: if signals.dirty.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{} uncommitted paths; an unfinished boundary may be hiding",
                    signals.dirty.len()
                )]
            },
        },
    ];

    let top = candidates.iter().map(|focus| focus.score).max().unwrap_or(0);
    let mut tied: Vec<Focus> = candidates
        .drain(..)
        .filter(|focus| focus.score == top)
        .collect();
    if top == 0 {
        for focus in tied.iter_mut() {
            focus.reasons = vec!["no strong signal; rotating by clock".into()];
        }
    }
    tied[(tick as usize) % tied.len()].clone()
}

fn recent_focus(root: &Path, focus: &str, now: u64) -> bool {
    let Ok(entries) = std::fs::read_dir(root.join(".agents/inbox")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return false;
        };
        let Some(stamp) = stem.strip_prefix("ponder-") else {
            return false;
        };
        let Ok(then) = stamp.parse::<u64>() else {
            return false;
        };
        if now.saturating_sub(then) > DEDUPE_SECS {
            return false;
        }
        std::fs::read_to_string(&path)
            .map(|body| body.lines().next() == Some(&format!("# Ponder — {focus}")))
            .unwrap_or(false)
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_project_truths_outranks_everything() {
        let signals = Curiosity {
            queue_pending: 0,
            dirty: vec!["M x".into()],
            recent_commits: vec!["abc subject".into()],
            code_todos: vec!["src/a.rs:1:// TODO: x".into()],
            project_todos: 22,
            open_questions: 0,
            readme: true,
            test_command: Some("cargo test".into()),
        };
        let focus = pick_focus(&signals, 0);
        assert_eq!(focus.name, "project truths");
        assert!(focus.score >= 20);
    }

    #[test]
    fn a_missing_test_command_beats_technical_debt() {
        let signals = Curiosity {
            queue_pending: 0,
            dirty: Vec::new(),
            recent_commits: Vec::new(),
            code_todos: vec!["src/a.rs:1:// TODO: x".into()],
            project_todos: 0,
            open_questions: 0,
            readme: true,
            test_command: None,
        };
        assert_eq!(pick_focus(&signals, 0).name, "tests");
    }

    #[test]
    fn the_same_focus_is_deduped_within_two_hours() {
        let root = std::env::temp_dir().join(format!(
            "zolly-ponder-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(root.join(".agents/inbox")).unwrap();
        let now = now_secs();
        std::fs::write(
            root.join(format!(".agents/inbox/ponder-{now}.md")),
            "# Ponder — tests\n\nbody\n",
        )
        .unwrap();
        assert!(recent_focus(&root, "tests", now));
        assert!(!recent_focus(&root, "architecture", now));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
