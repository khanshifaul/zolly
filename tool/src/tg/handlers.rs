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

/// Tasks currently gated on the founder: `gate: human`, not approved,
/// not escalated, not settled.
fn pending_gates(root: &Path) -> Result<Vec<(std::path::PathBuf, String)>> {
    Ok(queue::read_queue(root)?
        .into_iter()
        .filter(|(_, c)| queue::is_pending_gate(&queue::parse_frontmatter(c)))
        .collect())
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

/// `haa` / `ha` / `yes` writes `approved: yes` into every unsettled
/// `gate: human` task. Never dispatches, commits, or pushes.
pub fn handle_approve(root: &Path) -> Result<String> {
    let mut approved = Vec::new();
    for (path, content) in pending_gates(root)? {
        let new_content = content.replace("gate: human", "gate: human\napproved: yes");
        fs::write(&path, new_content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        approved.push(queue::display_name(&path));
    }

    if approved.is_empty() {
        Ok("No pending gate: human tasks to approve.".to_string())
    } else {
        Ok(format!("Approved: {}", approved.join(", ")))
    }
}

/// `na` / `no` writes `status: escalated` plus a `reject:` reason. Neither
/// ever dispatches, commits, or pushes.
pub fn handle_decline(root: &Path) -> Result<String> {
    let mut declined = Vec::new();
    for (path, content) in pending_gates(root)? {
        let new_content = content.replace(
            "gate: human",
            "gate: human\nstatus: escalated\nreject: declined by founder",
        );
        fs::write(&path, new_content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        declined.push(queue::display_name(&path));
    }

    if declined.is_empty() {
        Ok("No pending gate: human tasks to decline.".to_string())
    } else {
        Ok(format!("Declined: {}", declined.join(", ")))
    }
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

    // Check for existing sessions
    let list_output = tokio::process::Command::new("omp")
        .args(["collab", "list", "--json"])
        .output()
        .await
        .context("Failed to run omp collab list")?;

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let instance_id = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&list_stdout) {
        if let Some(hosts) = json.get("hosts").and_then(|v| v.as_array()) {
            if let Some(first) = hosts.first() {
                first.get("instanceId").and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

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
                .output()
                .await
                .context("Failed to run omp collab list after /collab")?;

            let list_stdout = String::from_utf8_lossy(&list_output.stdout);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&list_stdout) {
                if let Some(hosts) = json.get("hosts").and_then(|v| v.as_array()) {
                    if let Some(first) = hosts.first() {
                        first.get("instanceId").and_then(|v| v.as_str()).map(|s| s.to_string())
                            .ok_or_else(|| anyhow::anyhow!("Sent /collab but no collab session found"))?
                    } else {
                        anyhow::bail!("Sent /collab but no collab session found");
                    }
                } else {
                    anyhow::bail!("Sent /collab but no hosts in collab list");
                }
            } else {
                anyhow::bail!("Failed to parse collab list after /collab");
            }
        }
    };

    // Control link by default; --view asks for read-only access.
    let mut cmd = tokio::process::Command::new("omp");
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
