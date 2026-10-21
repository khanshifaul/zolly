//! Frontmatter predicates over `.agents/queue/*.md`, shared by the brief
//! generator and the Telegram keyword handlers. One parser, one meaning:
//! `gate: human`, `approved: yes`, `status: escalated` and friends are only
//! ever read from the YAML frontmatter block, never from the body.
//!
//! # Gate values: bare vs qualified
//!
//! `result-contract.md` names the gate provider (`human`) and forbids an
//! agent approving one. Nothing stops a result from *qualifying* the value
//! with what is actually owed:
//!
//! - `gate: human` — **bare**. The provider and nothing else. Once the task
//!   is merged, the merge itself is the review the marker asked for; the
//!   marker is settled history, not a decision anyone still owes.
//! - `gate: human — live cutover only` — **qualified**. The qualifier names a
//!   decision a merge cannot make (cutting live traffic over), so the gate
//!   stays waiting however the task's merge state changes.
//!
//! Only a bare provider is retired by a merge ([`is_waiting_gate`] with
//! `merged = true`); a qualified value never is. Inference runs one way
//! only: the view may retire a marker the merge already satisfied, and may
//! never manufacture an approval — nothing here writes `approved:`.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Flags parsed out of a task file's frontmatter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub gate_human: bool,
    /// The raw text after `gate:` on the frontmatter gate line, trimmed —
    /// `human` or `human — live cutover only`. `None` when the file has no
    /// `gate:` line. Retained rather than collapsed to `gate_human` so the
    /// merge-aware view can tell a bare provider from a qualified one.
    pub gate_value: Option<String>,
    pub approved: bool,
    pub escalated: bool,
    pub unverified: bool,
    pub settled: bool,
    /// The raw text after `status:` on the frontmatter status line, trimmed;
    /// `None` when there is no `status:` line. Retained rather than collapsed
    /// to `has_status` so a reader that must refuse an unreadable file can
    /// *name* the value it could not read (`status: blocked`).
    pub status_value: Option<String>,
    /// The file opened a `---` frontmatter block (even if unclosed).
    pub has_frontmatter: bool,
    /// A `status:` line was present in the frontmatter.
    pub has_status: bool,
}

/// Parse the leading `---` frontmatter block of `content`. A file with no
/// frontmatter yields default (all-false) flags — it is not a gate, not
/// approved, not settled.
pub fn parse_frontmatter(content: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut in_frontmatter = false;
    let mut saw_opening = false;

    for line in content.lines() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                fm.has_frontmatter = true;
                saw_opening = true;
                continue;
            }
            break; // closing delimiter
        }
        if !in_frontmatter {
            // Body content before any `---` means there is no frontmatter.
            if !line.trim().is_empty() {
                return fm;
            }
            continue;
        }

        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("gate:") {
            // Keep the value verbatim — the qualifier after the provider is
            // the whole difference between a gate a merge settles and one it
            // cannot (`gate: human` vs `gate: human — live cutover only`).
            let value = trimmed["gate:".len()..].trim().to_string();
            if value.to_lowercase().contains("human") {
                fm.gate_human = true;
            }
            fm.gate_value = Some(value);
        }
        if lower.starts_with("approved:") && lower.contains("yes") {
            fm.approved = true;
        }
        if lower.starts_with("status:") {
            fm.has_status = true;
            // Keep the value verbatim: `blocked` and a typo settle nothing,
            // and a reader that refuses an unreadable file has to say which
            // value it could not read.
            fm.status_value = Some(trimmed["status:".len()..].trim().to_string());
            if lower.contains("escalated") {
                fm.escalated = true;
            }
            if lower.contains("unverified") {
                fm.unverified = true;
            }
            if lower.contains("done")
                || lower.contains("settled")
                || lower.contains("failed")
                || lower.contains("escalated")
                || lower.contains("complete")
            {
                fm.settled = true;
            }
        }
    }

    // A file that opened `---` but never closed it still counts: the flags
    // seen so far are the best information available.
    let _ = saw_opening;
    fm
}

/// True when the task is gated on the human and has not been approved,
/// escalated or settled. This is the condition every approve/decline/status
/// path filters on — one predicate, so they can never drift apart.
pub fn is_pending_gate(fm: &Frontmatter) -> bool {
    fm.gate_human && !fm.approved && !fm.escalated && !fm.settled
}

/// True when a `gate:` value names the provider and nothing more: exactly
/// `human`, ignoring surrounding whitespace and case. Any qualifier —
/// `human — live cutover only`, `human after deploy` — makes it false.
pub fn gate_is_bare(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("human")
}

/// True when a gate is still *waiting* on the founder.
///
/// `merged` is the caller's finding that this task's result file already
/// sits on the integration ref (see `brief::merged_into_integration`). The
/// rule, in full:
///
/// - present and `approved:` → not waiting (unchanged)
/// - **bare** provider + merged → not waiting: the merge is the review the
///   marker asked for, so the marker is settled history
/// - **qualified** provider (`gate: human — live cutover only`) → waiting,
///   merged or not: the qualifier names a decision the merge cannot make
/// - not merged → waiting, bare or qualified. An agent reports a gate, never
///   passes one, so this arm must never be retired by anything here.
pub fn is_waiting_gate(content: &str, merged: bool) -> bool {
    let fm = parse_frontmatter(content);
    if !fm.gate_human || fm.approved {
        return false;
    }
    let bare = fm
        .gate_value
        .as_deref()
        .map(gate_is_bare)
        .unwrap_or(false);
    !(merged && bare)
}

/// A result file is an unresolved gate when its frontmatter says
/// `gate: human` and does not say `approved: yes`. Settled status does NOT
/// clear a gate: `status: done` plus an unapproved gate is exactly the
/// "finished but founder never signed off" state this check exists for.
///
/// Content-only by contract: the queue scan uses this, and a queued task is
/// by definition not merged. The merge-aware results path uses
/// [`is_waiting_gate`] with the task's merge state instead.
pub fn is_unresolved_gate(content: &str) -> bool {
    is_waiting_gate(content, false)
}
/// Why a result file fails the result-contract settledness test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleReason {
    /// No `---` frontmatter block at all (e.g. body-only `# heading` +
    /// `**Status:** DONE`).
    NoFrontmatter,
    /// Frontmatter exists but has no `status:` line.
    NoStatusLine,
    /// `status:` is present but not one of the settled values
    /// (done/settled/failed/escalated/complete/completed). `unverified`
    /// deliberately lands here.
    NotSettled,
}

/// True exactly when `parse_frontmatter` reports the file settled: a
/// frontmatter block with a `status:` line whose value is a settled
/// terminal status. One predicate for both the close sweep and the brief —
/// they must judge the same file the same way.
pub fn result_is_settled(content: &str) -> bool {
    let fm = parse_frontmatter(content);
    fm.has_frontmatter && fm.has_status && fm.settled
}

/// Classifies why a result file is not settled, or `None` when it is.
pub fn settle_reason(content: &str) -> Option<SettleReason> {
    let fm = parse_frontmatter(content);
    if !fm.has_frontmatter {
        return Some(SettleReason::NoFrontmatter);
    }
    if !fm.has_status {
        return Some(SettleReason::NoStatusLine);
    }
    if !fm.settled {
        return Some(SettleReason::NotSettled);
    }
    None
}

/// Is this `status:` value one `parse_frontmatter` can act on? True for the
/// terminal values and for `unverified`/`escalated`, which the reviewer and
/// gate paths own. Deliberately the same `contains` test the parser uses, so
/// "readable" and "settled" can never disagree about a file.
fn status_is_readable(value: &str) -> bool {
    let v = value.to_lowercase();
    ["done", "settled", "failed", "escalated", "complete", "unverified"]
        .iter()
        .any(|word| v.contains(word))
}

/// Why a result file cannot be read as the result contract requires, or
/// `None` when it can. The reason names the defect, never the symptom.
///
/// This is the loud path that replaces a silent one: a file whose frontmatter
/// carries no status pulse can read used to fall through to the age buckets
/// in `brief::generate_brief` and be reported as "waiting for review" or
/// stalled. That is how twelve merged, closed tasks — whose result files were
/// all present on `origin/main` — were listed as outstanding on 2026-10-16,
/// two of them as "stalled over 2h". A file listed here must never be
/// reported as waiting; its defect is printed instead.
pub fn frontmatter_defect(content: &str) -> Option<String> {
    let fm = parse_frontmatter(content);
    if !fm.has_frontmatter {
        return Some("no `---` frontmatter block".to_string());
    }
    if !fm.has_status {
        return Some("frontmatter has no `status:` line".to_string());
    }
    let value = fm.status_value.as_deref().unwrap_or("");
    if !status_is_readable(value) {
        return Some(format!(
            "`status: {value}` is not a status pulse can read (`done` or `failed`)"
        ));
    }
    None
}

/// Read every `*.md` in `.agents/queue` along with its contents. Missing
/// directory is an empty queue, not an error.
pub fn read_queue(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let queue_dir = root.join(".agents/queue");
    let mut out = Vec::new();
    if !queue_dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&queue_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        out.push((path, content));
    }
    Ok(out)
}

/// The canonical result files for this office: one per task, preferring the
/// branch copy inside `.agents/worktrees/<id>/` over the main-checkout copy.
/// A task with a worktree has its authoritative file there; the main copy may
/// be stale, so it is not also returned.
pub fn result_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut covered = std::collections::HashSet::new();
    let worktrees = root.join(".agents/worktrees");
    if let Ok(entries) = fs::read_dir(&worktrees) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            covered.insert(format!("{id}.md"));
            let path = entry.path().join(".agents/results").join(format!("{id}.md"));
            if path.is_file() {
                out.push(path);
            }
        }
    }
    let results = root.join(".agents/results");
    if let Ok(entries) = fs::read_dir(&results) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if covered.contains(&name) {
                continue;
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Display name of a queue path: the file name including `.md`, matching
/// how results and gates are shown everywhere else.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_flags_are_parsed_only_from_the_block() {
        let content = "---\nid: t-1\ngate: human\nstatus: open\n---\n\nBody mentions gate: human but must not count";
        let fm = parse_frontmatter(content);
        assert!(fm.gate_human);
        assert!(!fm.approved);
        assert!(!fm.settled);
        assert!(is_pending_gate(&fm));
    }

    #[test]
    fn approved_gate_is_not_pending() {
        let content = "---\ngate: human\napproved: yes\n---\nBody";
        let fm = parse_frontmatter(content);
        assert!(is_unresolved_gate(content) == false);
        assert!(!is_pending_gate(&fm));
    }

    #[test]
    fn body_only_gate_does_not_count() {
        let content = "no frontmatter here\njust says gate: human in prose";
        assert!(!is_unresolved_gate(content));
        assert!(!is_pending_gate(&parse_frontmatter(content)));
    }

    #[test]
    fn unverified_is_not_settled() {
        let content = "---\nstatus: unverified\ngate: human\n---\nBody";
        let fm = parse_frontmatter(content);
        assert!(fm.unverified);
        assert!(!fm.settled, "unverified must stay a live obligation");
        assert!(is_pending_gate(&fm));
    }

    #[test]
    fn terminal_statuses_settle() {
        for status in ["done", "settled", "failed", "escalated", "complete", "completed"] {
            let content = format!("---\nstatus: {}\n---\nBody", status);
            assert!(parse_frontmatter(&content).settled, "status: {} should settle", status);
        }
    }

    #[test]
    fn done_with_unapproved_gate_is_still_an_unresolved_gate() {
        let content = "---\nstatus: done\ngate: human\n---\nBody";
        assert!(is_unresolved_gate(content));
    }

    #[test]
    fn gate_value_is_retained_verbatim() {
        let bare = parse_frontmatter("---\ngate: human\n---\nBody");
        assert_eq!(bare.gate_value.as_deref(), Some("human"));
        assert!(gate_is_bare(bare.gate_value.as_deref().unwrap()));

        let qualified = parse_frontmatter("---\ngate: human — live cutover only\n---\nBody");
        assert_eq!(
            qualified.gate_value.as_deref(),
            Some("human — live cutover only"),
            "the qualifier must survive parsing — it is the whole discriminator"
        );
        assert!(qualified.gate_human, "a qualified gate is still a gate");
        assert!(!gate_is_bare(qualified.gate_value.as_deref().unwrap()));

        let none = parse_frontmatter("---\nid: t-1\n---\nBody");
        assert_eq!(none.gate_value, None);
    }

    #[test]
    fn bare_gate_on_merged_work_is_not_waiting() {
        // t-api-dev-lint shape: `gate: human`, task merged, no approval.
        let content = "---\nid: t-api-dev-lint\nstatus: done\ngate: human\n---\nBody";
        assert!(!is_waiting_gate(content, true));
        // ...and it is still a gate while the work is unmerged.
        assert!(is_waiting_gate(content, false));
        assert!(is_unresolved_gate(content));
    }

    #[test]
    fn qualified_gate_on_merged_work_still_waits() {
        // t-telegram-webhook-transport shape: the live cutover is owed no
        // matter what the merge did. This is the arm a "merged ⇒ settled"
        // rule would silently mask.
        let content =
            "---\nid: t-telegram-webhook-transport\nstatus: done\ngate: human — live cutover only\n---\nBody";
        assert!(is_waiting_gate(content, true));
        assert!(is_waiting_gate(content, false));
    }

    #[test]
    fn approved_gate_waits_for_nothing_merged_or_not() {
        for merged in [true, false] {
            let content = "---\ngate: human\napproved: yes\n---\nBody";
            assert!(!is_waiting_gate(content, merged));
        }
    }

    #[test]
    fn gate_less_and_auto_files_never_wait() {
        for merged in [true, false] {
            assert!(!is_waiting_gate("---\ngate: auto\n---\nBody", merged));
            assert!(!is_waiting_gate("---\nid: t-1\n---\nBody", merged));
        }
    }
    #[test]
    fn no_frontmatter_is_not_settled() {
        // The real offender shape: body-only `# heading` + `**Status:** DONE`,
        // no `---` block at all.
        let content = "# t-catalog-feed-proxy-ctype — storefront feed proxy mislabelled every response as XML\n\n**Status:** DONE\n**Branch:** `foreman/t-catalog-feed-proxy-ctype`\n";
        let fm = parse_frontmatter(content);
        assert!(!fm.has_frontmatter, "{}", format!("{fm:?}"));
        assert!(!fm.has_status);
        assert!(!fm.settled);
        assert!(!result_is_settled(content));
        assert_eq!(settle_reason(content), Some(SettleReason::NoFrontmatter));
    }

    #[test]
    fn status_done_is_settled() {
        let content = "---\nid: t-1\ntitle: x\nstatus: done\ngate: human\n---\nBody";
        let fm = parse_frontmatter(content);
        assert!(fm.has_frontmatter);
        assert!(fm.has_status);
        assert!(fm.settled);
        assert!(result_is_settled(content));
        assert_eq!(settle_reason(content), None);
    }

    #[test]
    fn status_not_a_terminal_value_is_not_settled() {
        // `unverified` deliberately does not settle; any free text neither.
        for status in ["unverified", "open", "in progress", ""] {
            let content = format!("---\nstatus: {status}\n---\nBody");
            assert!(!result_is_settled(&content), "status: {status} must not settle");
            assert_ne!(settle_reason(&content), Some(SettleReason::NoFrontmatter));
            assert_ne!(settle_reason(&content), Some(SettleReason::NoStatusLine));
        }
    }

    #[test]
    fn frontmatter_without_status_is_not_settled() {
        let content = "---\nid: t-1\ntitle: x\ngate: human\n---\nBody";
        assert!(!result_is_settled(content));
        assert_eq!(settle_reason(content), Some(SettleReason::NoStatusLine));
    }

    #[test]
    fn defect_names_the_offender_and_lets_readable_files_through() {
        // The three shapes that were reported as "waiting for review" on
        // 2026-10-16 while merged and closed — each must be refused, by name.
        let no_fence = "id: t-docs-verify-b6\ntitle: x\nstatus: complete\nbranch: foreman/t-docs-verify\n";
        assert_eq!(
            frontmatter_defect(no_fence).as_deref(),
            Some("no `---` frontmatter block")
        );

        let no_status = "---\nid: t-1\ngate: human\n---\nBody";
        assert_eq!(
            frontmatter_defect(no_status).as_deref(),
            Some("frontmatter has no `status:` line")
        );

        // t-k3s-deploy-prisma-rerun's committed shape: fenced, with a status
        // value that is neither terminal nor owned by another path.
        let unreadable = "---\nid: t-k3s-deploy-prisma-rerun\nstatus: blocked\ngate: human — no kubectl here\n---\nBody";
        assert_eq!(
            frontmatter_defect(unreadable).as_deref(),
            Some("`status: blocked` is not a status pulse can read (`done` or `failed`)")
        );

        // Every value the parser can act on is readable — including the
        // non-terminal ones other paths own, so `unverified` keeps its
        // current meaning instead of becoming an alarm.
        for status in [
            "done",
            "failed",
            "settled",
            "complete",
            "completed",
            "escalated",
            "unverified",
        ] {
            let content = format!("---\nid: t-1\nstatus: {status}\n---\nBody");
            assert_eq!(frontmatter_defect(&content), None, "status: {status}");
        }
    }

    #[test]
    fn unreadable_status_is_exactly_where_the_parser_recognises_nothing() {
        // The two predicates must not disagree: any value the parser settles
        // or flags is readable, and only an unrecognised value is a defect.
        for status in ["done", "failed", "escalated", "unverified", "blocked", "open", "don"] {
            let content = format!("---\nstatus: {status}\n---\nBody");
            let fm = parse_frontmatter(&content);
            let recognised = fm.settled || fm.unverified || fm.escalated;
            assert_eq!(
                frontmatter_defect(&content).is_none(),
                recognised,
                "status: {status} — parser recognised={recognised}"
            );
        }
    }

    #[test]
    fn close_and_brief_judge_the_same_file_identically() {
        // One file, one verdict: the close sweep (via result_is_settled) and
        // the brief (via the same shared predicate) must agree, not each
        // assert in isolation.
        let offender = "# t-catalog-feed-proxy-ctype — storefront feed proxy mislabelled every response as XML\n\n**Status:** DONE\n";
        let settled = "---\nid: t-catalog-feed-proxy-ctype\ntitle: storefront feed proxy ctype\nstatus: done\ngate: human\n---\nBody";
        for content in [offender, settled] {
            let brief_settled = result_is_settled(content);
            assert_eq!(
                brief_settled,
                settle_reason(content).is_none(),
                "brief settle and settle_reason must agree on the same content: {content}"
            );
        }
        assert!(!result_is_settled(offender), "offender must not be settled by either path");
        assert!(result_is_settled(settled), "status: done must be settled by either path");
    }
}
