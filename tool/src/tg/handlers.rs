//! Keyword and subcommand handlers: the approval loop (status/approve/
//! decline), pulse/gates/stalls reads, watch alerts, day journaling, the
//! hourly report, and OMP collab links. Everything that mutates the queue
//! goes through `queue::parse_frontmatter`, so approve/decline/status can
//! never disagree about what counts as pending.

use anyhow::{Context, Result};
use clap::Subcommand;
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::config::{get_allowed_ids, get_token};
use super::format::{hoist_decision, strip_markdown, KEYWORD_HELP};
use super::send::send_message;
use crate::brief;
use crate::queue;
use crate::render;

#[derive(Subcommand)]
pub enum DayAction {
    /// Open a new day with a goal
    Open {
        goal: String,
        #[arg(long)]
        force: bool,
    },
    /// Add a note to the current day
    Note { text: String },
    /// Close the current day
    Close {
        #[arg(long)]
        done: bool,
    },
}

/// How often `zolly telegram-watch` wakes up (loop mode).
const WATCH_INTERVAL_SECS: u64 = 60;

// ---------------------------------------------------------------------------
// Approval loop (queue-backed)
// ---------------------------------------------------------------------------

/// Tasks currently gated on the founder, read from the canonical result
/// file: the branch copy when a worktree exists, otherwise the main copy.
/// The queue file is a brief, not a gate store.
fn pending_gates(root: &Path) -> Result<Vec<(std::path::PathBuf, String)>> {
    let mut out = Vec::new();
    for path in queue::result_files(root) {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        // Settled status does not clear a gate. Use the merge-aware
        // unresolved predicate with merged=false: the approve path is, by
        // definition, for work that is not merged.
        if queue::is_unresolved_gate(&content)
            && !queue::parse_frontmatter(&content).escalated
        {
            out.push((path, content));
        }
    }
    Ok(out)
}

pub fn handle_status(root: &Path) -> Result<String> {
    let waiting: Vec<String> = pending_gates(root)?
        .into_iter()
        .map(|(p, _)| queue::display_name(&p))
        .collect();

    if waiting.is_empty() {
        Ok("No tasks waiting on your approval.".to_string())
    } else {
        Ok(format!("Waiting on you:\n  {}", waiting.join("\n  ")))
    }
}

/// Insert `approved: yes` into a result file's frontmatter, as its OWN line.
///
/// The marker must not disturb the `gate:` value. A gate value may be
/// QUALIFIED — `gate: human — live cutover only` — and the qualifier names a
/// decision a merge cannot make, which is exactly why a bare gate and a
/// qualified gate are treated differently. Replacing the text
/// `gate: human` in place splits such a value:
///
/// ```text
/// gate: human — no k3s/kubectl access from this host
/// ```
///
/// becomes
///
/// ```text
/// gate: human
/// approved: yes — no k3s/kubectl access from this host
/// ```
///
/// The gate is now bare, and the reason it needed the founder has been
/// demoted to a trailing remark on the approval line. The approval registers
/// either way, so nothing looks broken — but the qualifier is gone, and a
/// qualified gate is precisely the one a merge must not settle.
///
/// So the marker goes on a new line, immediately after the `gate:` line, and
/// the gate value is left byte-for-byte intact.
fn mark_approved(content: &str) -> String {
    let mut out = String::with_capacity(content.len() + 16);
    let mut inserted = false;
    let mut in_frontmatter = false;
    let mut saw_opening = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                saw_opening = true;
            }
            out.push_str(line);
            continue;
        }
        if in_frontmatter && !inserted && trimmed.to_lowercase().starts_with("gate:") {
            out.push_str(line);
            out.push_str("approved: yes\n");
            inserted = true;
            continue;
        }
        if in_frontmatter && saw_opening && trimmed == "---" {
            in_frontmatter = false;
        }
        out.push_str(line);
    }
    if !inserted {
        // No `gate:` line to mark — nothing to approve. Leave the file alone
        // rather than inventing a gate.
        return content.to_string();
    }
    out
}

/// `haa` / `ha` / `yes` writes `approved: yes` into every unsettled
/// `gate: human` result. The branch copy is canonical, so the write is
/// committed on the task branch; it never dispatches, pushes, or merges.
pub fn handle_approve(root: &Path) -> Result<String> {
    let mut approved = Vec::new();
    for (path, content) in pending_gates(root)? {
        let old = content.clone();
        let new_content = mark_approved(&content);
        fs::write(&path, &new_content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        let id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Err(error) = commit_marker(root, &path, &format!("approve: {id}")) {
            let _ = fs::write(&path, old);
            return Err(error);
        }
        approved.push(queue::display_name(&path));
    }

    if approved.is_empty() {
        Ok("No pending gate: human tasks to approve.".to_string())
    } else {
        Ok(format!("Approved: {}", approved.join(", ")))
    }
}

/// `na` / `no` writes `status: escalated` plus a `reject:` reason into the
/// canonical result and commits it on the task branch. It never dispatches,
/// pushes, or merges.
pub fn handle_decline(root: &Path) -> Result<String> {
    let mut declined = Vec::new();
    for (path, content) in pending_gates(root)? {
        let old = content.clone();
        let new_content = content.replacen(
            "gate: human",
            "gate: human\nstatus: escalated\nreject: declined by founder",
            1,
        );
        fs::write(&path, &new_content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        let id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Err(error) = commit_marker(root, &path, &format!("decline: {id}")) {
            let _ = fs::write(&path, old);
            return Err(error);
        }
        declined.push(queue::display_name(&path));
    }

    if declined.is_empty() {
        Ok("No pending gate: human tasks to decline.".to_string())
    } else {
        Ok(format!("Declined: {}", declined.join(", ")))
    }
}

/// Commit a marker in the task's worktree. A main-checkout result has no
/// branch to commit to; that copy is canonical only when no worktree exists.
fn commit_marker(root: &Path, path: &Path, message: &str) -> Result<()> {
    let id = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let worktree = root.join(".agents/worktrees").join(&id);
    if !worktree.is_dir() || !path.starts_with(&worktree) {
        return Ok(());
    }
    let relative = path
        .strip_prefix(&worktree)
        .context("result path is not inside the task worktree")?;
    let add = std::process::Command::new("git")
        .args(["add", "--"])
        .arg(relative)
        .current_dir(&worktree)
        .output()
        .context("Failed to stage the approval")?;
    if !add.status.success() {
        anyhow::bail!(
            "failed to stage the approval: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        );
    }
    let commit = std::process::Command::new("git")
        .args(["commit", "-m", message, "--no-verify"])
        .current_dir(&worktree)
        .output()
        .context("Failed to commit the approval")?;
    if !commit.status.success() {
        anyhow::bail!(
            "failed to commit the approval: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Read-only status text for chat keywords
// ---------------------------------------------------------------------------

pub fn handle_pulse(root: &Path) -> Result<String> {
    let b = brief::generate_brief(root).context("Failed to generate brief")?;
    let s = render::sections(root, &b);
    let mut output = String::new();

    output.push_str(&format!("Workers: {} running\n", s.workers.len()));
    for (name, pane) in &s.workers {
        output.push_str(&format!("  - {} (pane: {})\n", name, pane));
    }

    output.push_str(&format!("\nRecent results: {}\n", s.results.len()));
    for r in &s.results {
        output.push_str(&format!("  - {}\n", r));
    }

    if !s.unverified.is_empty() {
        output.push_str(&format!(
            "\nUNVERIFIED (a crashed review is not a pass): {}\n",
            s.unverified.join(", ")
        ));
    }

    // A file pulse cannot read is named with its reason and never counted as
    // a review the office is waiting on.
    if !s.unparseable.is_empty() {
        output.push_str(&format!(
            "\n⚠ {} pulse cannot read (NOT counted as waiting) — fix the frontmatter:\n",
            s.unparseable.len()
        ));
        for u in &s.unparseable {
            output.push_str(&format!("  - {}\n", u));
        }
    }

    if !s.gates.is_empty() {
        output.push_str("\nGates waiting on you:\n");
        for g in &s.gates {
            output.push_str(&format!("  - {}\n", g));
        }
    }

    if !s.historical_gates.is_empty() {
        output.push_str(&format!(
            "\nSettled gates (task merged — historical, not waiting): {}\n",
            s.historical_gates.join(", ")
        ));
    }

    output.push_str(&format!("\nCommits (2h): {}\n", s.commits));

    if s.stalls.is_empty() {
        output.push_str("\nStalls: none\n");
    } else {
        output.push_str("\nStalls:\n");
        for st in &s.stalls {
            output.push_str(&format!("  - {}\n", st));
        }
    }

    Ok(output)
}

/// `/gates` — just the gates, so a decision request is not buried.
pub fn handle_gates(root: &Path) -> Result<String> {
    let b = brief::generate_brief(root).context("Failed to generate brief")?;
    let mut out = if b.gates.is_empty() {
        "No gates waiting on you.".to_string()
    } else {
        let mut waiting = format!("⏳ {} waiting on you:\n", b.gates.len());
        for g in &b.gates {
            waiting.push_str(&format!("  - {}\n", g));
        }
        waiting.push_str("\nReply `haa` to approve all pending gates, `na` to decline.");
        waiting
    };
    if !b.historical_gates.is_empty() {
        out.push_str(&format!(
            "\n{} settled by merge (historical, not waiting): {}",
            b.historical_gates.len(),
            b.historical_gates.join(", ")
        ));
    }
    Ok(out)
}

/// `/stalls` — stalled reviews and worktree leaks.
pub fn handle_stalls(root: &Path) -> Result<String> {
    let b = brief::generate_brief(root).context("Failed to generate brief")?;
    if b.waiting.stalls.is_empty() {
        return Ok("No stalls. All clear.".to_string());
    }
    let mut out = format!("⚠ {} stalled:\n", b.waiting.stalls.len());
    for s in &b.waiting.stalls {
        out.push_str(&format!("  - {}\n", s));
    }
    Ok(out)
}

/// `/help` — the keyword table, one line per command.
pub fn handle_help() -> String {
    let mut out = String::from("Available commands:\n");
    for (cmd, desc) in KEYWORD_HELP {
        out.push_str(&format!("  {} — {}\n", cmd, desc));
    }
    out.push_str("  Free text — ask zolly anything");
    out
}

// ---------------------------------------------------------------------------
// Watch
// ---------------------------------------------------------------------------

/// Single watch pass. Returns the alert text (or `None` if all clear).
async fn watch_once(root: &Path) -> Result<Option<String>> {
    let b = brief::generate_brief(root).context("Failed to generate brief")?;
    let s = render::sections(root, &b);
    let mut alerts = Vec::new();

    for id in &s.unverified {
        alerts.push(format!("UNVERIFIED: {}", id));
    }

    if !s.stalls.is_empty() {
        alerts.push(format!("STALLS: {}", s.stalls.join("; ")));
    }

    if !s.gates.is_empty() {
        alerts.push(format!("GATES: {} waiting on the founder", s.gates.len()));
    }

    if s.workers.is_empty() && !s.results.is_empty() {
        alerts.push("DISPATCHABLE: work waiting, no workers running".to_string());
    }

    if alerts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("⚠ Zolly Watch Alert:\n{}", alerts.join("\n"))))
    }
}

pub async fn cmd_watch(root: &Path, dry: bool, once: bool) -> Result<()> {
    let token = if dry { None } else { Some(get_token()?) };
    let target_chat = if dry {
        None
    } else {
        Some(get_allowed_ids()?[0])
    };

    loop {
        match watch_once(root).await {
            Ok(None) => {
                if dry {
                    println!("All clear — no alert would be sent");
                }
            }
            Ok(Some(message)) => {
                if dry {
                    println!("DRY RUN — would send:\n{}", message);
                } else {
                    let token = token.as_deref().unwrap();
                    let chat = target_chat.unwrap();
                    send_message(token, chat, &message).await?;
                    println!("Watch alert sent");
                }
            }
            Err(e) => {
                eprintln!("Watch pass failed: {:#}", e);
            }
        }

        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(WATCH_INTERVAL_SECS)).await;
    }
}

// ---------------------------------------------------------------------------
// Day journal
// ---------------------------------------------------------------------------

pub fn cmd_day(root: &Path, action: DayAction) -> Result<()> {
    match action {
        DayAction::Open { goal, force } => {
            let day_path = root.join(".agents/days/current.json");
            if day_path.exists() && !force {
                anyhow::bail!("A day is already open. Use --force to override.");
            }
            if goal.trim().is_empty() {
                anyhow::bail!("Goal cannot be empty");
            }

            let day = serde_json::json!({
                "goal": goal,
                "opened_at": chrono::Utc::now().to_rfc3339(),
                "notes": [],
            });
            fs::write(&day_path, serde_json::to_string_pretty(&day)?)?;
            println!("Day opened: {}", goal);
        }
        DayAction::Note { text } => {
            let day_path = root.join(".agents/days/current.json");
            if !day_path.exists() {
                anyhow::bail!("No day is open. Run `zolly telegram-day open <goal>` first.");
            }
            let mut day: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&day_path)?)?;
            let notes = day
                .get_mut("notes")
                .and_then(|v| v.as_array_mut())
                .context("current.json is missing a `notes` array")?;
            notes.push(serde_json::json!({
                "at": chrono::Utc::now().to_rfc3339(),
                "text": text,
            }));
            fs::write(&day_path, serde_json::to_string_pretty(&day)?)?;
            println!("Note added");
        }
        DayAction::Close { done } => {
            let day_path = root.join(".agents/days/current.json");
            if !day_path.exists() {
                anyhow::bail!("No day is open.");
            }
            let day: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&day_path)?)?;
            let stamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
            let archive_path = root.join(format!(".agents/days/{}.json", stamp));
            fs::copy(&day_path, &archive_path)?;
            fs::remove_file(&day_path)?;
            println!("Day closed and archived to {}", archive_path.display());
            if done {
                println!("Goal completed: {}", day["goal"]);
            } else {
                println!("Goal not completed: {}", day["goal"]);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Report & collab
// ---------------------------------------------------------------------------

/// Render the founder brief as the Telegram text (personality signature
/// included). Shared by `telegram-report` and the in-chat `brief` keyword.
pub fn report_text(root: &Path) -> Result<String> {
    let b = brief::generate_brief(root).context("Failed to generate brief")?;
    let s = render::sections(root, &b);
    let out = render::render_telegram(root, &s);
    Ok(strip_markdown(&hoist_decision(&out)))
}

pub async fn cmd_report(root: &Path) -> Result<()> {
    let token = get_token()?;
    let target_chat = get_allowed_ids()?[0];

    let output = report_text(root)?;
    send_message(&token, target_chat, &output).await?;
    println!("Report sent to chat {}", target_chat);
    Ok(())
}

/// The office's Collab host, if one is running.
///
/// `omp collab list` is machine-wide: every local OMP session registers, so
/// the first entry may belong to another project on the same machine. Take the
/// host whose cwd is the office root; a host inside the office (a worktree or
/// a nested session) is the fallback, matching `workspace_for_path`.
fn collab_host_for_office(json: &serde_json::Value, root: &Path) -> Option<String> {
    let hosts = json.get("hosts")?.as_array()?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut nested = None;
    for host in hosts {
        let Some(cwd) = host.get("cwd").and_then(|v| v.as_str()) else {
            continue;
        };
        let cwd = Path::new(cwd);
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let instance = host
            .get("instanceId")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if cwd == root {
            return instance;
        }
        if nested.is_none() && cwd.starts_with(&root) {
            nested = instance;
        }
    }
    nested
}

/// Find or start an OMP collab session and send the URL to Telegram.
/// Control link by default (the founder can drive the session from
/// chat); pass `view = true` for a view-only link.
pub async fn cmd_collab(
    root: &Path,
    _list: bool,
    _link: bool,
    _view: bool,
    chat_id: Option<i64>,
) -> Result<()> {
    let token = get_token()?;
    let target_chat = super::send::resolve_chat(chat_id)?;

    // Check for existing sessions. The listing is machine-wide, so it is
    // filtered to this office before a host is chosen.
    let list_output = tokio::process::Command::new("omp")
        .args(["collab", "list", "--json"])
        .current_dir(root)
        .output()
        .await
        .context("Failed to run omp collab list")?;

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let instance_id = serde_json::from_str::<serde_json::Value>(&list_stdout)
        .ok()
        .and_then(|json| collab_host_for_office(&json, root));

    let instance_id = match instance_id {
        Some(id) => id,
        None => {
            // No active session — send /collab slash command to existing zolly OMP agent
            let herdr = crate::herdr::HerdrClient::new(root.to_path_buf());

            herdr
                .agent_prompt_only(&super::config::zolly_agent(root), "/collab")
                .context("Failed to send /collab to zolly agent")?;
            
            // Give it a moment to create the session
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            
            // Re-list to get the new session's instanceId
            let list_output = tokio::process::Command::new("omp")
                .args(["collab", "list", "--json"])
                .current_dir(root)
                .output()
                .await
                .context("Failed to run omp collab list after /collab")?;

            let list_stdout = String::from_utf8_lossy(&list_output.stdout);
            match serde_json::from_str::<serde_json::Value>(&list_stdout) {
                Ok(json) => collab_host_for_office(&json, root).ok_or_else(|| {
                    anyhow::anyhow!("Sent /collab but no collab session found for this office")
                })?,
                Err(_) => anyhow::bail!("Failed to parse collab list after /collab"),
            }
        }
    };

    // Control link by default; --view asks for read-only access.
    let mut cmd = tokio::process::Command::new("omp");
    cmd.current_dir(root);
    let args: Vec<String> = if _view {
        vec!["collab".to_string(), "link".to_string(), instance_id.clone(), "--view".to_string()]
    } else {
        vec!["collab".to_string(), "link".to_string(), instance_id.clone()]
    };
    cmd.args(&args);

    let output = cmd
        .output()
        .await
        .context("Failed to run omp collab link")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        anyhow::bail!("omp collab link failed: {}", stderr);
    }

    let label = if _view { "Collab view URL (read-only)" } else { "Collab control link (full access — anyone with it can drive the session)" };
    send_message(&token, target_chat, &format!("{}\n\n{}", label, stdout.trim_end())).await?;
    println!("Collab {} sent to chat {}", if _view { "view URL" } else { "control link" }, target_chat);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn git(dir: &Path, args: &[&str]) -> std::process::Output {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must run");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    /// A pending gate lives in the worktree branch copy. The queue file and
    /// the main-checkout result both exist but are not authoritative. The
    /// frontmatter deliberately has no `status:`: a settled status is not a
    /// pending gate.
    fn fixture() -> (PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "zolly-gate-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".agents/queue")).unwrap();
        std::fs::create_dir_all(root.join(".agents/results")).unwrap();
        std::fs::write(
            root.join(".agents/queue/t-gate.md"),
            "---\nid: t-gate\ngate: human\n---\n\nbrief\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".agents/results/t-gate.md"),
            "---\nid: t-gate\ngate: human\n---\n\nmain copy — not authoritative\n",
        )
        .unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["add", ".agents/queue", ".agents/results"]);
        git(&root, &["commit", "-q", "-m", "seed"]);

        let worktree = root.join(".agents/worktrees/t-gate");
        let worktree_str = worktree.to_string_lossy().into_owned();
        git(&root, &["worktree", "add", "-q", &worktree_str, "-b", "foreman/t-gate"]);
        std::fs::create_dir_all(worktree.join(".agents/results")).unwrap();
        std::fs::write(
            worktree.join(".agents/results/t-gate.md"),
            "---\nid: t-gate\ngate: human\nstatus: done\n---\n\nbranch copy — authoritative\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".agents/results/t-done.md"),
            "---\nid: t-done\nstatus: done\n---\n\nno gate at all\n",
        )
        .unwrap();
        git(&worktree, &["add", ".agents/results/t-gate.md"]);
        git(&worktree, &["commit", "-q", "-m", "result"]);
        (root, worktree)
    }

    #[test]
    fn approve_writes_and_commits_the_branch_copy() {
        let (root, worktree) = fixture();
        let branch_path = worktree.join(".agents/results/t-gate.md");
        let main_path = root.join(".agents/results/t-gate.md");
        let queue_path = root.join(".agents/queue/t-gate.md");
        let main_before = std::fs::read_to_string(&main_path).unwrap();
        let queue_before = std::fs::read_to_string(&queue_path).unwrap();

        let before = handle_status(&root).unwrap();
        assert!(before.contains("t-gate.md"), "{before}");
        assert!(!before.contains("t-done"), "{before}");
        handle_approve(&root).expect("approve must succeed");

        let branch = std::fs::read_to_string(&branch_path).unwrap();
        assert!(branch.contains("approved: yes"), "{branch}");
        assert!(branch.contains("gate: human\napproved: yes"), "{branch}");
        assert_eq!(
            std::fs::read_to_string(&main_path).unwrap(),
            main_before,
            "the main copy must not be touched"
        );
        assert_eq!(
            std::fs::read_to_string(&queue_path).unwrap(),
            queue_before,
            "the queue file is a brief, not the gate store"
        );

        let subject = git(&worktree, &["log", "-1", "--format=%s"]);
        assert!(
            String::from_utf8_lossy(&subject.stdout).contains("approve: t-gate"),
            "the approval must be committed on the task branch"
        );

        let (waiting, historical) = crate::brief::result_gates(&root).unwrap();
        assert!(waiting.is_empty(), "approved gate still waiting: {waiting:?}");
        assert_eq!(historical, vec!["t-gate.md".to_string()]);
        assert!(handle_status(&root).unwrap().contains("No tasks waiting"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A gate that is SETTLED but never signed off is still a gate.
    ///
    /// The approve test above deliberately uses a fixture with no `status:`
    /// line, so the settled-status question never arises there — a settled
    /// gate is a different case, and this is it. `status: done` plus an
    /// unapproved `gate: human` is the finished-but-never-signed-off state
    /// `queue::is_unresolved_gate` exists for.
    #[test]
    fn settled_gate_is_still_approvable() {
        let content = "---\nid: t-settled\ngate: human\nstatus: done\n---\n\nbody\n";
        let marked = mark_approved(content);
        assert!(
            marked.contains("approved: yes"),
            "the settled gate must still be approvable: {marked}"
        );
        assert!(
            marked.contains("gate: human"),
            "the gate value must survive the approval: {marked}"
        );
    }

    /// A QUALIFIED gate keeps its qualifier when it is approved.
    ///
    /// The qualifier names a decision a merge cannot make, which is the
    /// whole reason a bare gate and a qualified gate are treated differently.
    /// Injecting the marker in place would split the value and leave a bare
    /// gate behind — the approval would register and the qualifier would be
    /// gone, so nothing would look broken.
    #[test]
    fn qualified_gate_keeps_its_qualifier() {
        let content =
            "---\nid: t-qualified\ngate: human — no k3s/kubectl access from this host\n---\n\nbody\n";
        let marked = mark_approved(content);

        let fm = crate::queue::parse_frontmatter(&marked);
        assert!(
            fm.approved,
            "the approval must register: {marked}"
        );
        assert_eq!(
            fm.gate_value.as_deref(),
            Some("human — no k3s/kubectl access from this host"),
            "the gate value must be byte-for-byte intact: {marked}"
        );
        assert!(
            !crate::queue::gate_is_bare(fm.gate_value.as_deref().unwrap_or("")),
            "a qualified gate must not become bare: {marked}"
        );
        // And it must not be read as settled by a merge. An APPROVED gate is
        // not waiting by definition — that is the point of the approval. The
        // qualifier's job is to survive it, so check the value, not the
        // waiting state: an approved BARE gate would also read as not
        // waiting, and that is the shape the in-place injection produced.
        assert!(
            !crate::queue::gate_is_bare(fm.gate_value.as_deref().unwrap_or("")),
            "an approved qualified gate must still read as qualified: {marked}"
        );
    }

    /// The approval marker is its own line, and the gate line is untouched.
    #[test]
    fn approval_marker_is_its_own_line() {
        let content = "---\nid: t-x\ngate: human\nstatus: done\n---\n\nbody\n";
        let marked = mark_approved(content);
        let gate_line = marked
            .lines()
            .find(|l| l.trim().to_lowercase().starts_with("gate:"))
            .unwrap();
        assert_eq!(gate_line.trim(), "gate: human", "{marked}");
        assert!(
            marked.lines().any(|l| l.trim() == "approved: yes"),
            "the marker must be a line of its own: {marked}"
        );
    }

    /// `omp collab list` is machine-wide. The office must pick its own host,
    /// not whichever project registered first.
    #[test]
    fn collab_host_is_scoped_to_the_office() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "zolly-collab-{}-{nanos}",
            std::process::id()
        ));
        let office = base.join("office");
        let other = base.join("other");
        let nested = office.join(".agents/worktrees/t-x");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&other).unwrap();

        let list = |entries: &[(&str, &Path)]| {
            serde_json::json!({
                "version": 1,
                "hosts": entries
                    .iter()
                    .map(|(id, cwd)| serde_json::json!({
                        "instanceId": id,
                        "cwd": cwd.to_string_lossy(),
                    }))
                    .collect::<Vec<_>>(),
            })
        };

        // Another project registered first; the office's host is second.
        let json = list(&[("other", &other), ("office", &office)]);
        assert_eq!(
            collab_host_for_office(&json, &office).as_deref(),
            Some("office")
        );

        // No exact host: a session nested inside the office still counts.
        let json = list(&[("other", &other), ("nested", &nested)]);
        assert_eq!(
            collab_host_for_office(&json, &office).as_deref(),
            Some("nested")
        );

        // Nothing in the office: no host, rather than the first machine-wide one.
        let json = list(&[("other", &other)]);
        assert_eq!(collab_host_for_office(&json, &office), None);
    }
}
