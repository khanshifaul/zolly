use anyhow::{Context, Result};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Only results younger than this are reported as "waiting for review".
/// Older files still exist on disk, but a six-day-old result is not
/// waiting — it was either reviewed and never deleted, or abandoned.
/// Either way it is not an action item for the founder right now.
const RESULT_FRESH_SECS: u64 = 86_400; // 24h

/// A result older than this but younger than RESULT_FRESH_SECS is
/// stalled: it has been waiting for review too long. Two hours is the
/// point at which a review should have happened in normal operation.
const STALL_SECS: u64 = 7_200; // 2h

/// Above this many non-main worktrees, the brief reports a leak.
const WORKTREE_LEAK_THRESHOLD: usize = 8;

/// How many lines of an agent's pane to read when deciding whether it is a
/// live worker. The status bar is the pane's last line; 8 is the same window
/// `herdr::agent_is_working` reads, so both consumers of the bar see the same
/// bytes. Exactly one read per candidate — this is a bound, not a rate.
const WORKER_SCREEN_LINES: usize = 8;

/// Manager/observer agent names that are infrastructure, never counted as
/// task workers no matter what their herdr status says. An unnamed agent
/// (name=None) is also excluded — that is the opencode session, not a
/// worker.
const MANAGER_NAMES: &[&str] = &["zolly", "foreman", "pr-reviewer", "reviewer"];

#[derive(Debug, Serialize, Deserialize)]
pub struct Brief {
    pub finished: Vec<String>,
    pub waiting: WaitingStatus,
    /// Gates still waiting on the founder — the only ones that are.
    pub gates: Vec<String>,
    /// Gates on merged work whose value is a bare `human`: settled by the
    /// merge, kept visible as history. Never counted as waiting, never
    /// erased — a gate the view drops is a gate nobody can audit.
    pub historical_gates: Vec<String>,
    pub next_action: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WaitingStatus {
    pub workers: usize,
    pub worker_details: Vec<WorkerInfo>,
    pub recent_results: Vec<String>,
    pub stalled_results: Vec<String>,
    pub aged_results: usize,
    /// Result files the brief cannot read a status from, fresh enough that they
    /// would otherwise have been reported as waiting. Each entry is
    /// `name — reason` (`queue::frontmatter_defect`). Loud, never a wait.
    pub unparseable_results: Vec<String>,
    /// Of `aged_results`, how many carry equally unreadable frontmatter.
    /// Counted rather than named: 30-plus historical names would be exactly
    /// the crying-wolf list this defect exists to end, but dropping them from
    /// the population silently would be the same lie one age bucket over.
    pub unreadable_aged: usize,
    pub recent_commits: usize,
    pub stalls: Vec<String>,
    pub worktree_count: usize,
    pub worktree_leak: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub name: String,
    pub pane_id: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_manager(name: &str) -> bool {
    // Agent names carry the frequency (`foreman-guzia-high-school`); the bare
    // handles in MANAGER_NAMES are the radio handles, not the herdr names.
    MANAGER_NAMES
        .iter()
        .any(|m| *m == name || name.starts_with(&format!("{m}-")))
}

/// Is this herdr agent a live worker *right now*?
///
/// The screen decides, and `agent_status` may only ever *add* liveness. The
/// shipped predicate here was `a.agent_status == "working"`, which is
/// **unsatisfiable for the only worker kind this office runs**: `herdr agent
/// list` reports `agent_status: "idle"` for the *whole* turn of an omp agent
/// (`fallback_reason: default_known_agent_idle_fallback`, measured
/// 2026-10-14/16, `herdr.rs`). So `zolly brief` printed "No workers running."
/// over an agent demonstrably mid-turn.
///
/// The screen half is `herdr::screen_shows_working` — the corrected primitive
/// the `Free`-predicate defect already fixed, and the same fn
/// `agent_is_working` consumes. Reused deliberately, not copied: a second
/// "is this agent working" decision is the drift that shipped the broken one.
pub(crate) fn worker_is_live(agent_status: &str, screen: &str) -> bool {
    agent_status == "working" || crate::herdr::screen_shows_working(screen)
}

fn mtime_secs(path: &Path) -> Option<u64> {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn worktree_stats(root: &Path) -> (usize, bool) {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root)
        .output();
    let Ok(output) = output else { return (0, false) };
    if !output.status.success() {
        return (0, false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let total = stdout.lines().filter(|l| l.starts_with("worktree ")).count();
    let linked = total.saturating_sub(1);
    (linked, linked > WORKTREE_LEAK_THRESHOLD)
}


/// Run `git` in `root`; `true` only on a clean exit. Merge detection never
/// fails the brief — an unanswerable question is handled by the caller as
/// "not merged", which keeps the gate reported rather than hiding it.
fn git_ok(root: &Path, args: &[&str]) -> bool {
    matches!(
        Command::new("git").args(args).current_dir(root).output(),
        Ok(o) if o.status.success()
    )
}

/// The ref whose tree decides whether a task is merged. `origin/main` is the
/// published integration branch — the reviewer records PASS, merges and
/// pushes there, so a result file present on it is merged for real. Local
/// `main` is the fallback for a repo without a remote (the test fixtures, a
/// fresh local-only checkout). When neither resolves, nothing counts as
/// merged: the conservative direction, because a gate wrongly retired is a
/// gate masked, while a gate wrongly kept is only today's behaviour.
fn integration_ref(root: &Path) -> Option<&'static str> {
    ["origin/main", "main"]
        .into_iter()
        .find(|r| git_ok(root, &["rev-parse", "--verify", "--quiet", &format!("{r}^{{commit}}")]))
}

/// Squash-safe merge signal: the task's result file is present on the
/// integration ref. Branch ancestry is worthless here — these PRs are
/// squash-merged and `gh pr merge --delete-branch` deletes the head branch,
/// so `git merge-base --is-ancestor foreman/<task-id> main` answers
/// "not merged" for work that is merged.
fn merged_into_integration(root: &Path, refname: &str, result_name: &str) -> bool {
    git_ok(
        root,
        &[
            "cat-file",
            "-e",
            &format!("{refname}:.agents/results/{result_name}"),
        ],
    )
}

/// Split the gates in `.agents/results/` into `(waiting, historical)`.
///
/// Reads the result files from disk at `dir`. When called from
/// `generate_brief`, `dir` is the branch's own `.agents/results/` in the
/// worktree checkout — so this reads the **branch copy**, never the main
/// checkout's copy. An `approved: yes` that exists only in the main checkout
/// is invisible here and never counts.
///
/// Historical means: the gate is present and unapproved, its value is a bare
/// provider, and the task's result file is already on the integration ref —
/// so the merge satisfied it and nobody is waiting. Everything else with a
/// gate is waiting: unmerged work whatever its value, and merged work whose
/// value carries a qualifier naming a decision a merge cannot make.
pub(crate) fn result_gates(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let mut waiting = Vec::new();
    let mut historical = Vec::new();
    let refname = integration_ref(root);
    for path in crate::queue::result_files(root) {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fm = crate::queue::parse_frontmatter(&content);
        if !fm.gate_human {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if fm.approved {
            historical.push(name);
            continue;
        }
        let merged = refname
            .map(|r| merged_into_integration(root, r, &name))
            .unwrap_or(false);
        if crate::queue::is_waiting_gate(&content, merged) {
            waiting.push(name);
        } else {
            historical.push(name);
        }
    }
    Ok((waiting, historical))
}

pub fn generate_brief(root: &Path) -> Result<Brief> {
    let herdr = crate::herdr::HerdrClient::new(root.to_path_buf());
    // Only this office's agents. `herdr agent list` is server-wide, and the
    // unfiltered read made the test office report zylosync's foreman and
    // reviewer as its own workers (wJ:p1, 2026-10-21). The workspace is the
    // boundary: every seat and worker lives in the office's workspace.
    let agents = herdr
        .agents_for_office(root)
        .context("Failed to list this office's agents")?;

    // Workers = any named, non-manager herdr agent currently live.
    // The ledger is gone; herdr is the source of truth for *who exists*, and
    // the pane's status bar is the source of truth for whether they are
    // working (`worker_is_live`) — `agent_status` never reads `working` for
    // this office's omp agents.
    //
    // Cost, deliberately bounded: one pane read per candidate, and the
    // managers are filtered out *before* any read. A normal office — 4 agents
    // (zolly, foreman, pr-reviewer, one worker) — costs exactly 1 read. No
    // polling loop, nothing per-poll; `brief` sits on the founder's read
    // path, so the bound is the agent list, never a rate.
    let mut working: Vec<WorkerInfo> = Vec::new();
    for a in &agents {
        let Some(name) = a.name.as_deref() else {
            continue;
        };
        if is_manager(name) {
            continue;
        }
        // A failed read yields an empty screen, which falls back to
        // `agent_status` alone — exactly the shipped behaviour. A failed read
        // can neither invent a worker nor hide one the status already named.
        let screen = herdr
            .agent_read(name, WORKER_SCREEN_LINES)
            .unwrap_or_default();
        if worker_is_live(&a.agent_status, &screen) {
            working.push(WorkerInfo {
                name: name.to_string(),
                pane_id: a.pane_id.clone(),
            });
        }
    }

    // Scan results dir once for freshness, stalls and gates.
    let results_dir = root.join(".agents/results");
    let now = now_secs();
    let mut recent_results: Vec<String> = Vec::new();
    let mut stalled_results: Vec<String> = Vec::new();
    let mut aged_results = 0usize;
    let mut unparseable_results: Vec<String> = Vec::new();
    let mut unreadable_aged = 0usize;

    if results_dir.exists() {
        let mut entries: Vec<_> = std::fs::read_dir(&results_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by(|a, b| {
            let ma = mtime_secs(&a.path());
            let mb = mtime_secs(&b.path());
            mb.cmp(&ma).then_with(|| b.file_name().cmp(&a.file_name()))
        });

        for e in &entries {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let age = mtime_secs(&path)
                .map(|m| now.saturating_sub(m))
                .unwrap_or(u64::MAX);

            // A file the brief cannot read a status from is a defect, not a
            // waiting review. Reported as "waiting" it becomes a phantom —
            // twelve merged, closed tasks were listed that way on
            // 2026-10-16, two of them as "stalled over 2h".
            //
            // Loud inside the fresh window, because that is where an action
            // item lives; an aged-out file is not an action item, so it is
            // counted (see `unreadable_aged`) rather than named. A read error
            // is the same finding and gets the same treatment.
            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(err) => {
                    if age > RESULT_FRESH_SECS {
                        aged_results += 1;
                        unreadable_aged += 1;
                    } else {
                        unparseable_results.push(format!("{name} — cannot be read: {err}"));
                    }
                    continue;
                }
            };
            if let Some(reason) = crate::queue::frontmatter_defect(&content) {
                if age > RESULT_FRESH_SECS {
                    aged_results += 1;
                    unreadable_aged += 1;
                } else {
                    unparseable_results.push(format!("{name} — {reason}"));
                }
                continue;
            }

            // A settled result (status: done/settled/failed/...) is not
            // "waiting for review" no matter how old it is — it finished.
            // Only an *open* result that has sat past STALL_SECS is stalled.
            let settled = crate::queue::result_is_settled(&content);

            if settled {
                // finished: not recent, not stalled, not aged-out waiting
                continue;
            }

            if age > RESULT_FRESH_SECS {
                aged_results += 1;
            } else if age > STALL_SECS {
                stalled_results.push(name.clone());
            } else {
                if recent_results.len() < 5 {
                    recent_results.push(name.clone());
                }
            }
        }
    }
    // Scan order is mtime-desc, which is not stable across checkouts; the
    // loud list must read the same on every run.
    unparseable_results.sort();

    // Gates. One canonical file per task: the branch copy when a worktree
    // exists, otherwise the main result. The queue file is a brief, not a
    // gate store; reading both is how an approval in one copy and a gate in
    // the other never meet.
    let (mut gates, mut historical_gates) = result_gates(root)?;
    gates.sort();
    historical_gates.sort();

    // Recent commits: real data or fail loudly.
    let output = Command::new("git")
        .args(["log", "--since=2 hours ago", "--oneline"])
        .current_dir(root)
        .output()
        .context("Failed to run git log")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // An empty repository is a state, not an error: a fresh office has
        // zero commits and must still answer. Dying here made every brief and
        // brief on a template checkout look broken (wJ:p1, 2026-10-21).
        if !stderr.contains("does not have any commits yet") {
            anyhow::bail!("git log failed: {}", stderr);
        }
    }
    let commit_lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    let recent_commits = commit_lines.len();
    let finished: Vec<String> = commit_lines.into_iter().take(5).collect();

    // Stalls: stalled results + worktree leak.
    let mut stalls: Vec<String> = stalled_results
        .iter()
        .map(|r| format!("{} — waiting for review over 2h", r))
        .collect();

    let (worktree_count, worktree_leak) = worktree_stats(root);
    if worktree_leak {
        stalls.push(format!(
            "worktree leak: {} open (threshold {})",
            worktree_count, WORKTREE_LEAK_THRESHOLD
        ));
    }

    // Next action priority: unreadable frontmatter > gates > stalls > results
    // > workers > nothing. The defect ranks first on purpose: while a result
    // file cannot be read, the rest of this picture is not trustworthy — a
    // gate behind an unreadable file is invisible, which is how twelve merged
    // tasks read as outstanding.
    // Only *waiting* gates count — a gate the merge already settled must not
    // be what today's brief asks the founder for.
    let next_action = if !unparseable_results.is_empty() {
        "repair the unparseable result frontmatter".to_string()
    } else if !gates.is_empty() {
        "resolve the waiting gates".to_string()
    } else if !stalls.is_empty() {
        "unstall the stalled tasks".to_string()
    } else if !recent_results.is_empty() {
        "review results".to_string()
    } else if !working.is_empty() {
        "nothing, I have it".to_string()
    } else if aged_results > 0 {
        "nothing — older results are aged out".to_string()
    } else {
        "nothing, I have it".to_string()
    };

    Ok(Brief {
        finished,
        waiting: WaitingStatus {
            workers: working.len(),
            worker_details: working,
            recent_results,
            stalled_results,
            aged_results,
            unparseable_results,
            unreadable_aged,
            recent_commits,
            stalls,
            worktree_count,
            worktree_leak,
        },
        gates,
        historical_gates,
        next_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_names_are_recognised_with_the_frequency_suffix() {
        assert!(is_manager("zolly"));
        assert!(is_manager("foreman"));
        assert!(is_manager("pr-reviewer"));
        assert!(is_manager("zolly-zylosync"));
        assert!(is_manager("foreman-guzia-high-school"));
        assert!(is_manager("pr-reviewer-pijunctionbd"));
        assert!(!is_manager("worker-fix-1"));
        assert!(!is_manager("worker-zylosync-t-project-truths"));
        assert!(!is_manager("weld"));
    }

    #[test]
    fn result_freshness_window_is_24h() {
        assert_eq!(RESULT_FRESH_SECS, 86_400);
    }

    #[test]
    fn stall_threshold_is_2h() {
        assert_eq!(STALL_SECS, 7_200);
    }

    #[test]
    fn leak_threshold_is_reasonable() {
        assert!(WORKTREE_LEAK_THRESHOLD >= 4);
        assert!(WORKTREE_LEAK_THRESHOLD <= 32);
    }
    #[test]
    fn settled_result_is_never_reported_as_stalled() {
        // Regression: a done+merged result must not show up as
        // "waiting for review over 2h" just because its mtime is old.
        // The stall classifier skips settled results outright, so a
        // directory holding only settled results yields no stalls.
        let root = std::env::temp_dir().join(format!(
            "zolly-stall-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let results = root.join(".agents/results");
        std::fs::create_dir_all(&results).unwrap();
        // generate_brief runs `git log` in root; give it a repo
        // with one commit so the log succeeds.
        std::fs::write(root.join("README.md"), "test\n").unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "init"])
            .current_dir(&root)
            .output();

        // done + gate:human + commit — a settled, merged result
        std::fs::write(
            results.join("t-done-merged.md"),
            "---\nid: t-done-merged\nstatus: done\ngate: human\ncommit: abc123\n---\n\nBody",
        )
        .unwrap();
        // failed — also settled
        std::fs::write(
            results.join("t-failed.md"),
            "---\nid: t-failed\nstatus: failed\n---\n\nBody",
        )
        .unwrap();
        // escalated — settled, not a stall
        std::fs::write(
            results.join("t-escalated.md"),
            "---\nid: t-escalated\nstatus: escalated\n---\n\nBody",
        )
        .unwrap();

        let b = generate_brief(&root).unwrap();
        assert!(
            b.waiting.stalls.is_empty(),
            "settled results must never be stalls, got: {:?}",
            b.waiting.stalls
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unparseable_frontmatter_is_named_not_reported_as_waiting() {
        // Measured 2026-10-16: twelve merged, closed tasks were listed as
        // "waiting for review" / "stalled over 2h" because the brief could not
        // read their frontmatter and fell through to the age buckets. One
        // fixture per offender shape. Delete the `frontmatter_defect` arm
        // from `generate_brief` and every assertion here goes red: the three
        // files come back as recent_results, and next_action asks for a
        // review instead of naming the defect.
        let root = gated_repo(
            "unparseable",
            &[],
            &[
                // t-docs-verify-b6 shape: key/value header, no `---` fences.
                (
                    "t-nofence.md",
                    "id: t-nofence\ntitle: x\nstatus: complete\nbranch: foreman/t-docs-verify\n\nbody\n",
                ),
                // fenced, but no `status:` line at all.
                ("t-nostatus.md", "---\nid: t-nostatus\ngate: human\n---\n\nbody\n"),
                // t-k3s-deploy-prisma-rerun's committed shape: fenced, with a
                // status value no reader can act on.
                (
                    "t-blocked.md",
                    "---\nid: t-blocked\nstatus: blocked\ngate: human — no kubectl here\n---\n\nbody\n",
                ),
                // A well-formed, settled result must stay quiet.
                ("t-done.md", "---\nid: t-done\nstatus: done\ngate: auto\n---\n\nbody\n"),
            ],
        );

        let b = generate_brief(&root).unwrap();

        assert!(
            b.waiting.recent_results.is_empty(),
            "an unreadable file must not be a review waiting: {:?}",
            b.waiting.recent_results
        );
        assert!(
            b.waiting.stalled_results.is_empty(),
            "an unreadable file must not be a stall: {:?}",
            b.waiting.stalled_results
        );
        assert!(b.waiting.stalls.is_empty(), "{:?}", b.waiting.stalls);
        assert_eq!(
            b.next_action, "repair the unparseable result frontmatter",
            "the defect outranks a review request — the picture is not trustworthy"
        );
        assert_eq!(
            b.waiting.unparseable_results,
            vec![
                "t-blocked.md — `status: blocked` is not a status the brief can read (`done` or `failed`)"
                    .to_string(),
                "t-nofence.md — no `---` frontmatter block".to_string(),
                "t-nostatus.md — frontmatter has no `status:` line".to_string(),
            ],
            "each unreadable file must be named with its own reason"
        );
        assert_eq!(b.waiting.unreadable_aged, 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Build a throwaway repo whose `main` holds the committed results and
    /// whose working tree holds the rest — the same fixture shape as
    /// `settled_result_is_never_reported_as_stalled`, extended with a commit.
    fn gated_repo(
        tag: &str,
        committed: &[(&str, &str)],
        uncommitted: &[(&str, &str)],
    ) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "zolly-gate-merge-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let results = root.join(".agents/results");
        std::fs::create_dir_all(&results).unwrap();
        for (name, body) in committed {
            std::fs::write(results.join(name), body).unwrap();
        }
        std::fs::write(root.join("README.md"), "test\n").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["add", "-A"],
            vec![
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        ] {
            let _ = std::process::Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output();
        }
        for (name, body) in uncommitted {
            std::fs::write(results.join(name), body).unwrap();
        }
        root
    }

    #[test]
    fn bare_gate_on_merged_work_is_historical_not_waiting() {
        // The t-api-dev-lint shape: `gate: human`, task merged, no approval.
        // Before this rule the brief counted four of these as decisions
        // waiting on the founder.
        let root = gated_repo(
            "bare",
            &[(
                "t-api-dev-lint.md",
                "---\nid: t-api-dev-lint\nstatus: done\ngate: human\n---\nBody",
            )],
            &[],
        );
        let b = generate_brief(&root).unwrap();
        assert!(
            b.gates.is_empty(),
            "a merged bare gate is not waiting: {:?}",
            b.gates
        );
        assert_eq!(
            b.historical_gates,
            vec!["t-api-dev-lint.md".to_string()],
            "the marker must stay visible as history, not be deleted"
        );
        assert_eq!(
            b.next_action, "nothing, I have it",
            "nothing is waiting, so the brief must not ask for anything"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gates_split_by_merge_state_and_value() {
        // Both non-regression arms at once: an unmerged bare gate stays
        // waiting, and a merged *qualified* gate stays waiting (the #108
        // live-cutover shape) — merged does not mean settled for it.
        let root = gated_repo(
            "split",
            &[
                (
                    "t-merged-bare.md",
                    "---\nid: t-merged-bare\nstatus: done\ngate: human\n---\nBody",
                ),
                (
                    "t-merged-qualified.md",
                    "---\nid: t-merged-qualified\nstatus: done\ngate: human — live cutover only\n---\nBody",
                ),
            ],
            &[(
                "t-open-bare.md",
                "---\nid: t-open-bare\nstatus: done\ngate: human\n---\nBody",
            )],
        );
        let b = generate_brief(&root).unwrap();
        assert_eq!(
            b.gates,
            vec![
                "t-merged-qualified.md".to_string(),
                "t-open-bare.md".to_string()
            ],
            "unmerged bare gates and merged qualified gates both stay waiting"
        );
        assert_eq!(b.historical_gates, vec!["t-merged-bare.md".to_string()]);
        assert_eq!(b.next_action, "resolve the waiting gates");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approval_marker_reads_branch_copy_only() {
        // The branch copy is canonical. A stale `approved: yes` in the main
        // checkout must not clear a bare branch gate; a branch approval is
        // historical even when the main copy is bare.
        let root = std::env::temp_dir().join(format!(
            "zolly-approval-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let main = root.join(".agents/results");
        std::fs::create_dir_all(&main).unwrap();

        let branch_bare = root.join(".agents/worktrees/t-branch-bare/.agents/results");
        std::fs::create_dir_all(&branch_bare).unwrap();
        std::fs::write(
            branch_bare.join("t-branch-bare.md"),
            "---\nid: t-branch-bare\ngate: human\n---\nBody",
        )
        .unwrap();
        std::fs::write(
            main.join("t-branch-bare.md"),
            "---\nid: t-branch-bare\ngate: human\napproved: yes\n---\nStale main copy",
        )
        .unwrap();

        let branch_approved = root.join(".agents/worktrees/t-branch-approved/.agents/results");
        std::fs::create_dir_all(&branch_approved).unwrap();
        std::fs::write(
            branch_approved.join("t-branch-approved.md"),
            "---\nid: t-branch-approved\ngate: human\napproved: yes\n---\nBody",
        )
        .unwrap();
        std::fs::write(
            main.join("t-branch-approved.md"),
            "---\nid: t-branch-approved\ngate: human\n---\nStale main copy",
        )
        .unwrap();

        std::fs::write(
            main.join("t-main-only.md"),
            "---\nid: t-main-only\ngate: human\n---\nBody",
        )
        .unwrap();

        let (mut waiting, mut historical) = result_gates(&root).unwrap();
        waiting.sort();
        historical.sort();
        assert_eq!(
            waiting,
            vec!["t-branch-bare.md".to_string(), "t-main-only.md".to_string()],
            "a main-copy approval must not clear a bare branch gate"
        );
        assert_eq!(
            historical,
            vec!["t-branch-approved.md".to_string()],
            "a branch approval is historical, not waiting"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn generate_brief_on_empty_root_does_not_panic() {
        let dir = std::env::temp_dir().join(format!(
            "zolly-brief-test-{}-empty",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let result = generate_brief(&dir);
        match result {
            Ok(b) => {
                assert_eq!(b.waiting.workers, 0);
                assert!(b.waiting.stalls.is_empty());
                assert!(b.gates.is_empty());
            }
            Err(_) => {}
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── worker liveness (defect measured 2026-10-16) ────────────────────

    /// The mid-turn bar as omp draws it, verbatim from pane `w82:p1`
    /// (`worker-pulse-liveness`, 2026-10-16): braille spinner + elapsed token
    /// where the idle `π` prompt glyph sits.
    const WORKING_SCREEN: &str = "⎋ Reading the pane\n\
        ⠸ 2m > ◒ DeepSeek V4.1 Flash > 🌳 zylosync/t-pulse-worker-liveness > ⑂ foreman/t-pulse-worker-liveness > $0.01 ▶────5%──\n\
        ╰─";

    /// The at-rest bar for the same agent kind, measured on the same machine.
    const IDLE_SCREEN: &str = "⎋ Reading the pane\n\
        π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──\n\
        ╰─";

    #[test]
    fn a_working_screen_counts_as_a_live_worker() {
        assert!(worker_is_live("idle", WORKING_SCREEN));
    }

    #[test]
    fn an_idle_screen_is_not_a_live_worker() {
        assert!(!worker_is_live("idle", IDLE_SCREEN));
        // An unreadable screen (empty) is not evidence of work either — the
        // brief must never invent a worker.
        assert!(!worker_is_live("idle", ""));
    }

    #[test]
    fn idle_status_alone_cannot_exclude_a_worker_whose_screen_is_working() {
        // Fails-before: restore the shipped predicate — `agent_status ==
        // "working"`, no screen term — inside `worker_is_live` and this goes
        // red. That is the shipped defect: `herdr agent list` reports `idle`
        // for the *whole* turn of an omp agent, so the filter dropped every
        // live worker and `brief` printed "No workers running." over one.
        let status_herdr_actually_reports = "idle";
        assert!(
            worker_is_live(status_herdr_actually_reports, WORKING_SCREEN),
            "an omp worker mid-turn is reported idle; the screen must decide"
        );
        // The status term still affirms liveness where herdr reports it
        // accurately — it may add a worker, never withhold one.
        assert!(worker_is_live("working", IDLE_SCREEN));
    }
}
