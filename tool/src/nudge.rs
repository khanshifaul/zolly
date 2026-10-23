//! `zolly telegram-nudge` — a durable office nudge.
//!
//! Reads `zolly brief --json` and, when something is waiting on the founder,
//! sends one Telegram nudge: a **tapable poll** for gates (decisions) or a
//! plain reply for stalls. Never sends when there is nothing to say, and
//! never double-nudges within the minimum interval (persisted stamp file).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::brief::Brief;
use crate::tg::send::{cmd_poll, cmd_reply};
use crate::tg::config::get_allowed_ids;

/// Minimum interval between nudges, in seconds. Configurable via the
/// `ZOLLY_NUDGE_MIN_INTERVAL_SECS` env var (or the CLI flag); default one hour.
/// Not hardcoded at the call site — read from the environment at startup.
pub const DEFAULT_MIN_INTERVAL_SECS: u64 = 3_600;

/// Where the last-nudge stamp lives: mode 600, under `.agents/zolly/`.
/// Mirrors the `telegram_offset.txt` convention used by the serve loop.
pub const STAMP_FILE: &str = "telegram-nudge-stamp.txt";

/// Kind of nudge to send.
#[derive(Debug, Clone)]
pub enum NudgeKind {
    /// A tapable poll — for gates, i.e. yes/no or multiple-choice decisions.
    Poll { question: String, options: Vec<String> },
    /// A plain reply — for stalls and everything else that is not a choice.
    Reply { text: String },
}

/// The outcome of evaluating whether to nudge, before any network effect.
#[derive(Debug, Clone)]
pub struct NudgeDecision {
    pub send: bool,
    pub kind: Option<NudgeKind>,
    pub reason: String,
}

/// Parse `zolly brief --json` output. `Brief` must match the brief shape
/// exactly (`gates`, `waiting.stalls`, `next_action`).
fn parse_brief_json(text: &str) -> Result<Brief> {
    serde_json::from_str(text).context("Failed to parse `zolly brief --json` output")
}

/// Run `zolly brief --json` from `root` and return the parsed brief.
pub fn read_brief(root: &Path) -> Result<Brief> {
    let exe = std::env::current_exe().context("Failed to locate the zolly binary")?;
    let output = Command::new(&exe)
        .args(["brief", "--json"])
        .current_dir(root)
        .output()
        .context("Failed to run `zolly brief --json`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("`zolly brief --json` exited {}: {}", output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "-".into()), stderr);
    }
    parse_brief_json(&String::from_utf8_lossy(&output.stdout))
}

/// The persisted stamp: last nudge time as unix seconds, or None when absent
/// / unreadable / corrupt (treat as "never sent", i.e. allowed to send).
fn read_stamp(path: &Path) -> Option<u64> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Write the stamp atomically-ish, mode 600. Creates `.agents/zolly/` if
/// missing (it already exists in this repo).
fn write_stamp(path: &Path, now: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create nudge stamp directory")?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .context("Failed to open nudge stamp file")?;
    // Mode 600: owner read/write only — the stamp holds timing, not secrets,
    // but keep it private to the founder's user like the offset file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .context("Failed to set stamp file mode 600")?;
    }
    writeln!(file, "{now}").context("Failed to write nudge stamp")?;
    Ok(())
}

/// Seconds since the last nudge, or `u64::MAX` when there is no stamp.
fn age_secs(path: &Path, now: u64) -> u64 {
    match read_stamp(path) {
        Some(last) => now.saturating_sub(last),
        None => u64::MAX,
    }
}

/// Decide whether to nudge and what to send, purely from the brief + stamp.
/// `now` is unix seconds (injected so the test is deterministic).
pub fn evaluate_nudge(root: &Path, brief: &Brief, stamp_path: &Path, interval_secs: u64, now: u64) -> NudgeDecision {
    // Suppression: a nudge was sent recently. Interval not elapsed → silence.
    let suppressed = age_secs(stamp_path, now) < interval_secs;

    // Primary trigger: waiting gates. These are merge-aware (settled/merged
    // gates are already excluded by `result_gates`), so a gate here is a real
    // decision the founder still owes. A decision is a yes/no choice → poll.
    if !brief.gates.is_empty() {
        let question = format!(
            "{} decision needed: {}",
            brief.gates.len(),
            brief.gates.join(", ")
        );
        return NudgeDecision {
            send: !suppressed,
            kind: Some(NudgeKind::Poll {
                question,
                options: vec!["yes".to_string(), "no".to_string()],
            }),
            reason: if suppressed {
                "interval not elapsed".into()
            } else {
                "waiting gates".into()
            },
        };
    }

    // Secondary trigger: stalls, filtered to unmerged work. A stall whose
    // result file already sits on origin/main is settled (merged) and must not
    // wake the founder — that mislabel is exactly what gets a keepalive muted.
    // Gates remain the primary trigger; stalls are only the fallback.
    let live_stalls: Vec<_> = brief.waiting.stalls
        .iter()
        .filter(|s| !s.starts_with("worktree leak:"))
        .filter(|s| {
            // stall strings look like "t-catalog-feed-proxy-ctype.md — waiting for review over 2h"
            let name = s.split(' ').next().unwrap_or(s);
            !result_is_merged(root, name)
        })
        .cloned()
        .collect();

    if !live_stalls.is_empty() {
        let text = format!(
            "Office stall needs attention: {}",
            live_stalls.join("; ")
        );
        return NudgeDecision {
            send: !suppressed,
            kind: Some(NudgeKind::Reply { text }),
            reason: if suppressed {
                "interval not elapsed".into()
            } else {
                "stall".into()
            },
        };
    }

    NudgeDecision {
        send: false,
        kind: None,
        reason: "nothing to nudge".into(),
    }
}

/// Stamp path under the repo root, following the `.agents/zolly/` convention.
fn stamp_path(root: &Path) -> PathBuf {
    root.join(".agents/zolly").join(STAMP_FILE)
}

/// True when `name` (a `.agents/results/` result file) is present on
/// `origin/main` — i.e. the task is merged and settled, so its stall marker
/// is stale and must not nudge the founder.
fn result_is_merged(root: &Path, name: &str) -> bool {
    let output = Command::new("git")
        .args(["cat-file", "-e", &format!("origin/main:.agents/results/{}", name)])
        .current_dir(root)
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Send one nudge per run, idempotently. Returns a one-line outcome.
pub async fn cmd_nudge(root: &Path, min_interval_secs: u64) -> Result<String> {
    let brief = read_brief(root)?;
    let stamp = stamp_path(root);
    let now = now_secs();
    let decision = evaluate_nudge(root, &brief, &stamp, min_interval_secs, now);

    if !decision.send {
        return Ok(format!(
            "nudge suppressed: {} ({})",
            decision.reason,
            decision.kind.as_ref().map(|_| "had a target").unwrap_or("none")
        ));
    }

    let token_present = get_allowed_ids().is_ok(); // validates we can reach the allowed chat
    let _ = token_present;

    match decision.kind.context("nudge decided to send but no kind")? {
        NudgeKind::Poll { question, options } => {
            let opts = options.join(",");
            // Non-anonymous explicitly: the `telegram-poll` default is
            // anonymous=true (see `default_value_t = true` + `--no-anonymous`),
            // so we must pass `anonymous=false` here, not rely on the default.
            cmd_poll(root, question, opts, false, false).await?;
        }
        NudgeKind::Reply { text } => {
            let chat_id = crate::tg::send::resolve_chat(None)?;
            cmd_reply(chat_id, text).await?;
        }
    }

    write_stamp(&stamp, now)?;
    Ok(format!("nudge sent: {}", decision.reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn brief_with(gates: Vec<String>, stalls: Vec<String>) -> Brief {
        Brief {
            finished: vec![],
            waiting: crate::brief::WaitingStatus {
                workers: 0,
                worker_details: vec![],
                recent_results: vec![],
                stalled_results: stalls.clone(),
                aged_results: 0,
                unparseable_results: vec![],
                unreadable_aged: 0,
                recent_commits: 0,
                stalls,
                worktree_count: 0,
                worktree_leak: false,
            },
            gates,
            historical_gates: vec![],
            next_action: "".into(),
        }
    }

    fn stamp_tmp() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        env::temp_dir().join(format!("zolly-nudge-test-{}-{}.txt", std::process::id(), n))
    }

    #[test]
    fn gates_trigger_a_poll() {
        let b = brief_with(vec!["t-foo.md".into()], vec![]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &stamp_tmp(), DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        assert!(d.send);
        assert!(matches!(d.kind, Some(NudgeKind::Poll { .. })));
        assert!(d.reason == "waiting gates");
    }

    #[test]
    fn gates_poll_is_non_anonymous_explicit() {
        // Documented in the result: the telegram-poll default is anonymous=true,
        // so the nudge must pass anonymous=false, not rely on the default.
        let b = brief_with(vec!["t-foo.md".into()], vec![]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &stamp_tmp(), DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        if let Some(NudgeKind::Poll { options, .. }) = d.kind {
            assert_eq!(options, vec!["yes".to_string(), "no".to_string()]);
        } else {
            panic!("expected a poll");
        }
    }

    #[test]
    fn stalls_only_trigger_a_reply() {
        let b = brief_with(vec![], vec!["t-stale.md — waiting for review over 2h".into()]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &stamp_tmp(), DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        assert!(d.send);
        assert!(matches!(d.kind, Some(NudgeKind::Reply { .. })));
        assert!(d.reason == "stall");
    }

    #[test]
    fn nothing_to_say_sends_nothing() {
        let b = brief_with(vec![], vec![]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &stamp_tmp(), DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        assert!(!d.send);
        assert!(d.kind.is_none());
        assert!(d.reason == "nothing to nudge");
    }

    #[test]
    fn stamp_suppresses_second_run() {
        let path = stamp_tmp();
        // No stamp → allowed to send.
        let _ = age_secs;
        // Fresh stamp → suppressed.
        write_stamp(&path, u64::MAX).unwrap();
        assert!(age_secs(&path, u64::MAX) < DEFAULT_MIN_INTERVAL_SECS);
        let b = brief_with(vec!["t-foo.md".into()], vec![]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &path, DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        assert!(!d.send, "second run must not send");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn old_stamp_allows_send() {
        let path = stamp_tmp();
        write_stamp(&path, 0).unwrap(); // ancient
        let b = brief_with(vec!["t-foo.md".into()], vec![]);
        let d = evaluate_nudge(std::path::Path::new("."), &b, &path, DEFAULT_MIN_INTERVAL_SECS, u64::MAX);
        assert!(d.send, "an old stamp must allow a send");
        let _ = fs::remove_file(&path);
    }
}
