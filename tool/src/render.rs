//! One section model, two renderers. The terminal brief (`zolly brief`)
//! and the Telegram report previously diverged: Telegram showed `unverified`
//! but no gates; the CLI showed gates but no unverified. Both gaps were
//! real — the founder could be told "resolve the waiting gates" without
//! ever seeing which. Sections are computed once; renderers only format.

use crate::brief::Brief;
use crate::queue;
use std::path::Path;

/// The founder's display name. Source of truth is the office's
/// `.agents/zolly/config.toml`; the fallback is the word "founder", never a
/// name — one binary serves several offices, and a baked-in name greets the
/// wrong person in the other one.
pub fn founder_name(root: &Path) -> String {
    let name = crate::office::Office::load(root.to_path_buf()).founder;
    if name.is_empty() { "founder".to_string() } else { name }
}

/// Marker for the persona's decision section, hoisted to the top of
/// Telegram reports so it is not buried under status noise.
pub const DECISION_MARKER: &str = "What to do now:";

/// Zolly's voice file, relative to the repo root. This file is the only
/// copy — edit `.agents/memory/zolly-personality.md` directly.
const PERSONALITY_PATH: &str = ".agents/memory/zolly-personality.md";

/// Everything a brief has to say, as facts. Renderers must not invent,
/// drop or reorder facts — only present them for their medium.
#[derive(Debug)]
pub struct Sections {
    pub shipped: Vec<String>,
    pub workers: Vec<(String, String)>, // (name, pane_id)
    pub results: Vec<String>,
    pub unverified: Vec<String>,
    pub gates: Vec<String>,
    /// Gates the merge already settled: bare `gate: human` on merged work.
    /// Rendered as historical so the record stays visible.
    pub historical_gates: Vec<String>,
    pub stalls: Vec<String>,
    pub aged: usize,
    /// Result files pulse cannot read, as `name — reason`. Rendered loudly
    /// by both renderers — these are never counted as waiting.
    pub unparseable: Vec<String>,
    /// How many aged-out files share the same unreadable frontmatter.
    pub unreadable_aged: usize,
    pub commits: usize,
    pub next_action: String,
}

/// Compute sections from a generated brief plus the queue on disk.
/// `unverified` comes from the queue (a reviewer crash is not a pass) and
/// is listed ahead of ordinary stalls in both renderers.
pub fn sections(root: &Path, brief: &Brief) -> Sections {
    let mut unverified: Vec<String> = queue::read_queue(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, content)| queue::parse_frontmatter(content).unverified)
        .map(|(path, _)| queue::display_name(&path))
        .collect();
    unverified.sort();

    Sections {
        shipped: brief.finished.clone(),
        workers: brief
            .waiting
            .worker_details
            .iter()
            .map(|w| (w.name.clone(), w.pane_id.clone()))
            .collect(),
        results: brief.waiting.recent_results.clone(),
        unverified,
        gates: brief.gates.clone(),
        historical_gates: brief.historical_gates.clone(),
        stalls: brief.waiting.stalls.clone(),
        aged: brief.waiting.aged_results,
        unparseable: brief.waiting.unparseable_results.clone(),
        unreadable_aged: brief.waiting.unreadable_aged,
        commits: brief.waiting.recent_commits,
        next_action: brief.next_action.clone(),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {}", word)
    } else {
        format!("{} {}s", n, word)
    }
}

/// Word for word the same block in both renderers: settled gates stay
/// visible so the record is not silently deleted, and are labelled so they
/// are never read as a decision that is waiting.
fn render_settled_gates(items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "{} settled by merge (historical, not waiting):\n",
        plural(items.len(), "gate")
    );
    for g in items {
        out.push_str(&format!("  - {}\n", g));
    }
    out
}

/// Terminal rendering: one fact per line, plain ASCII markers.
pub fn render_cli(s: &Sections) -> String {
    let mut out = String::new();

    if s.shipped.is_empty() {
        out.push_str("Nothing shipped in the last couple hours.\n");
    } else if s.shipped.len() == 1 {
        out.push_str(&format!("Shipped: {}\n", s.shipped[0]));
    } else {
        out.push_str(&format!("Shipped {} things:\n", s.shipped.len()));
        for f in &s.shipped {
            out.push_str(&format!("  - {}\n", f));
        }
    }

    if s.workers.is_empty() {
        out.push_str("No workers running.\n");
    } else if s.workers.len() == 1 {
        out.push_str(&format!(
            "One worker running ({}).\n",
            s.workers[0].0
        ));
    } else {
        out.push_str(&format!("{} workers running:\n", s.workers.len()));
        for (name, pane) in &s.workers {
            out.push_str(&format!("  - {} ({})\n", name, pane));
        }
    }

    if !s.results.is_empty() {
        out.push_str(&format!(
            "{} waiting for review:\n",
            plural(s.results.len(), "result")
        ));
        for r in &s.results {
            out.push_str(&format!("  - {}\n", r));
        }
    }

    if !s.unverified.is_empty() {
        out.push_str(&format!(
            "UNVERIFIED (a crashed review is not a pass): {}\n",
            s.unverified.join(", ")
        ));
    }

    // Loud, and separate from "waiting for review": a file pulse cannot read
    // must never be reported as a review it is waiting on.
    if !s.unparseable.is_empty() {
        out.push_str(&format!(
            "⚠ {} pulse cannot read (NOT counted as waiting) — fix the frontmatter:\n",
            plural(s.unparseable.len(), "result file")
        ));
        for u in &s.unparseable {
            out.push_str(&format!("  - {}\n", u));
        }
    }

    if !s.gates.is_empty() {
        out.push_str(&format!(
            "⏳ {} waiting on you:\n",
            plural(s.gates.len(), "gate")
        ));
        for g in &s.gates {
            out.push_str(&format!("  - {}\n", g));
        }
    }

    out.push_str(&render_settled_gates(&s.historical_gates));

    if !s.stalls.is_empty() {
        out.push_str(&format!("⚠ {} stalled:\n", s.stalls.len()));
        for st in &s.stalls {
            out.push_str(&format!("  - {}\n", st));
        }
    }

    if s.aged > 0 {
        out.push_str(&format!(
            "{} older result{} on disk (aged out, not waiting{}).\n",
            s.aged,
            if s.aged == 1 { "" } else { "s" },
            if s.unreadable_aged > 0 {
                format!("; {} with unreadable frontmatter", s.unreadable_aged)
            } else {
                String::new()
            }
        ));
    }
    out.push_str(&format!("Commits (2h): {}\n", s.commits));
    out.push_str(&format!("\n{}\n", human_next_action(&s.next_action)));
    out
}

/// Telegram rendering: same facts, addressed to the founder, signed with
/// Zolly's name. The caller is
/// expected to run `strip_markdown` + `hoist_decision` on the result.
pub fn render_telegram(root: &Path, s: &Sections) -> String {
    let mut out = String::new();

    out.push_str(&format!("{}, here is where we are.\n\n", founder_name(root)));

    if !s.shipped.is_empty() {
        out.push_str("Shipped:\n");
        for f in &s.shipped {
            out.push_str(&format!("  - {}\n", f));
        }
        out.push('\n');
    }

    if s.workers.is_empty() {
        out.push_str("No workers running.\n");
    } else {
        out.push_str(&format!("{} running.\n", plural(s.workers.len(), "worker")));
        for (name, pane) in &s.workers {
            out.push_str(&format!("  - {} ({})\n", name, pane));
        }
    }

    if !s.results.is_empty() {
        out.push_str(&format!(
            "{} waiting for review.\n",
            plural(s.results.len(), "result")
        ));
        for r in &s.results {
            out.push_str(&format!("  - {}\n", r));
        }
    }

    // Unverified tasks get their own line, ahead of ordinary stalls,
    // because a crashed review is not a pass.
    if !s.unverified.is_empty() {
        out.push_str(&format!(
            "UNVERIFIED (a crashed review is not a pass): {}\n",
            s.unverified.join(", ")
        ));
    }

    // A file pulse cannot read is named with its reason, never reported as a
    // review the office is waiting on.
    if !s.unparseable.is_empty() {
        out.push_str(&format!(
            "⚠ {} pulse cannot read (NOT counted as waiting) — fix the frontmatter:\n",
            plural(s.unparseable.len(), "result file")
        ));
        for u in &s.unparseable {
            out.push_str(&format!("  - {}\n", u));
        }
    }

    // Gates were previously missing here entirely — the founder was told
    // "resolve the waiting gates" without ever seeing which.
    if !s.gates.is_empty() {
        out.push_str(&format!(
            "⏳ {} waiting on you:\n",
            plural(s.gates.len(), "gate")
        ));
        for g in &s.gates {
            out.push_str(&format!("  - {}\n", g));
        }
    }

    out.push_str(&render_settled_gates(&s.historical_gates));

    if s.stalls.is_empty() {
        out.push_str("Stalls: none\n");
    } else {
        out.push_str(&format!("Stalls: {}\n", s.stalls.join("; ")));
    }

    out.push_str(&format!("Commits (2h): {}\n", s.commits));
    out.push_str(&format!("\n{} {}\n", DECISION_MARKER, human_next_action(&s.next_action)));

    out.push_str("\n— Zolly\n");

    out
}

/// The brief stores machine-ish next_action strings; both renderers speak
/// these sentences verbatim (the CLI prints them bare, Telegram hoists
/// them under DECISION_MARKER).
fn human_next_action(next: &str) -> &'static str {
    match next {
        "nothing, I have it" => "Nothing for you right now — I've got it.",
        "review results" => "Next: review the results above.",
        "unparseable result frontmatter" | "repair the unparseable result frontmatter" => {
            "Next: the result files listed above have unreadable frontmatter — fix them so pulse can read them."
        }
        "unstall the stalled tasks" => "Next: the stalled tasks above need unsticking.",
        "resolve the waiting gates" => "Next: the gates above are waiting on you.",
        "nothing — older results are aged out" => {
            "Nothing for you — the older results on disk are aged out, not waiting."
        }
        _ => "",
    }
}

/// Read Zolly's voice from .agents/memory/zolly-personality.md. Loaded
/// once and rendered into every delivered report. A trait is adopted only
/// after repeated observations that conflict with an earlier belief, and
/// traits must be instructions, not descriptions of the founder.
pub fn load_personality(root: &Path) -> String {
    let path = root.join(PERSONALITY_PATH);
    let Ok(s) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Extract the traits section, skipping the header and the
    // "Why:" rationale so only the trait text is rendered.
    let mut traits = String::new();
    let mut in_traits = false;
    for line in trimmed.lines() {
        if line.trim() == "## Traits" {
            in_traits = true;
            continue;
        }
        if in_traits && line.starts_with("## ") {
            break;
        }
        if in_traits && line.trim_start().starts_with("- ") {
            traits.push_str(line.trim_start());
            traits.push('\n');
        }
    }
    traits.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief::{Brief, WaitingStatus};

    fn empty_brief() -> Brief {
        Brief {
            finished: vec![],
            waiting: WaitingStatus {
                workers: 0,
                worker_details: vec![],
                recent_results: vec![],
                stalled_results: vec![],
                aged_results: 0,
                unparseable_results: vec![],
                unreadable_aged: 0,
                recent_commits: 0,
                stalls: vec![],
                worktree_count: 0,
                worktree_leak: false,
            },
            gates: vec![],
            historical_gates: vec![],
            next_action: "nothing, I have it".to_string(),
        }
    }

    #[test]
    fn both_renderers_show_gates_and_unverified() {
        let dir = std::env::temp_dir().join(format!("zolly-render-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".agents/zolly")).unwrap();
        std::fs::write(
            dir.join(".agents/zolly/config.toml"),
            "founder = \"Shifaul\"\n",
        )
        .unwrap();

        let mut brief = empty_brief();
        brief.gates = vec!["t-gate.md".to_string()];
        brief.next_action = "resolve the waiting gates".to_string();

        let s = sections(&dir, &brief);
        assert_eq!(s.gates, vec!["t-gate.md".to_string()]);
        assert!(s.unverified.is_empty());

        let cli = render_cli(&s);
        assert!(cli.contains("t-gate.md"), "CLI must list gates:\n{}", cli);
        assert!(cli.contains("UNVERIFIED") == false || s.unverified.is_empty());

        let tg = render_telegram(&dir, &s);
        assert!(tg.contains("t-gate.md"), "Telegram must list gates:\n{}", tg);
        assert!(tg.contains(DECISION_MARKER));
        assert!(tg.contains("Shifaul"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unverified_from_queue_reaches_both_renderers() {
        let dir = std::env::temp_dir().join(format!("zolly-render-unver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".agents/queue")).unwrap();
        std::fs::write(
            dir.join(".agents/queue/t-crash.md"),
            "---\nstatus: unverified\n---\nBody",
        )
        .unwrap();

        let s = sections(&dir, &empty_brief());
        assert_eq!(s.unverified, vec!["t-crash.md".to_string()]);
        assert!(render_cli(&s).contains("t-crash.md"));
        assert!(render_telegram(&dir, &s).contains("t-crash.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settled_gates_stay_visible_and_apart_from_waiting_ones() {
        let dir = std::env::temp_dir().join(format!("zolly-render-settled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut brief = empty_brief();
        brief.gates = vec!["t-still-waiting.md".to_string()];
        brief.historical_gates = vec!["t-api-dev-lint.md".to_string()];
        brief.next_action = "resolve the waiting gates".to_string();

        let s = sections(&dir, &brief);
        assert_eq!(s.gates, vec!["t-still-waiting.md".to_string()]);
        assert_eq!(s.historical_gates, vec!["t-api-dev-lint.md".to_string()]);

        for (label, text) in [("cli", render_cli(&s)), ("telegram", render_telegram(&dir, &s))] {
            assert!(
                text.contains("t-api-dev-lint.md"),
                "{} must keep the settled gate visible:\n{}",
                label,
                text
            );
            assert!(
                text.contains("historical, not waiting"),
                "{} must label it as settled, not waiting:\n{}",
                label,
                text
            );
            // The settled gate must not appear in the waiting block: the
            // waiting header counts only the gate that is still owed.
            assert!(
                text.contains("1 gate waiting on you"),
                "{} must count only waiting gates:\n{}",
                label,
                text
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_settled_gates_means_no_settled_block() {
        let dir = std::env::temp_dir().join(format!("zolly-render-nosettled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let s = sections(&dir, &empty_brief());
        assert!(!render_cli(&s).contains("settled by merge"));
        assert!(!render_telegram(&dir, &s).contains("settled by merge"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn both_renderers_name_an_unreadable_result_instead_of_waiting_on_it() {
        let dir = std::env::temp_dir().join(format!(
            "zolly-render-unparseable-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut brief = empty_brief();
        brief.waiting.unparseable_results =
            vec!["t-x.md — no `---` frontmatter block".to_string()];
        // 7 aged-out results, all of them unreadable — the real shape: an
        // aged count whose unreadable ones are a subset of it.
        brief.waiting.aged_results = 7;
        brief.waiting.unreadable_aged = 7;
        brief.next_action = "repair the unparseable result frontmatter".to_string();

        let s = sections(&dir, &brief);
        assert_eq!(s.unparseable.len(), 1);
        assert_eq!(s.unreadable_aged, 7);
        assert_eq!(human_next_action(&s.next_action), "Next: the result files listed above have unreadable frontmatter — fix them so pulse can read them.");

        for (label, text) in [("cli", render_cli(&s)), ("telegram", render_telegram(&dir, &s))] {
            assert!(
                text.contains("t-x.md"),
                "{} must name the unreadable file:\n{}",
                label,
                text
            );
            assert!(
                text.contains("no `---` frontmatter block"),
                "{} must print the reason:\n{}",
                label,
                text
            );
            assert!(
                !text.contains("waiting for review"),
                "{} must not report an unreadable file as waiting:\n{}",
                label,
                text
            );
        }
        // The aged count keeps the population honest without naming 30+
        // historical files.
        assert!(render_cli(&s).contains("7 with unreadable frontmatter"), "{}", render_cli(&s));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn human_next_action_covers_all_known_strings() {
        for known in [
            "nothing, I have it",
            "review results",
            "unstall the stalled tasks",
            "resolve the waiting gates",
            "repair the unparseable result frontmatter",
            "nothing — older results are aged out",
        ] {
            assert!(!human_next_action(known).is_empty(), "{}", known);
        }
    }

    #[test]
    fn founder_name_matches_founder_md() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(md) = std::fs::read_to_string(root.join(".agents/memory/founder.md")) else {
            return;
        };
        let name = founder_name(&root);
        assert!(
            md.contains(&name),
            "founder_name drifted from .agents/memory/founder.md: {name}"
        );
    }

    #[test]
    fn render_telegram_omits_personality_traits() {
        let dir = std::env::temp_dir().join(format!("zolly-render-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let s = sections(&dir, &empty_brief());
        let tg = render_telegram(&dir, &s);

        // Should contain the sign-off
        assert!(tg.contains("— Zolly"), "Must keep Zolly sign-off:\n{}", tg);
        
        // Must NOT contain any personality traits
        let forbidden = [
            "brave",
            "name the exact defect",
            "short sentences",
            "no bullet walls",
            "lead with the answer",
            "never say",
        ];
        for trait_text in forbidden {
            assert!(
                !tg.to_lowercase().contains(trait_text),
                "Telegram output must not contain personality trait '{}':\n{}",
                trait_text, tg
            );
        }
    }

    #[test]
    fn render_telegram_signoff_only_once() {
        let dir = std::env::temp_dir().join(format!("zolly-render-signoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let s = sections(&dir, &empty_brief());
        let tg = render_telegram(&dir, &s);
        
        // Sign-off should appear exactly once at the end
        let signoff_count = tg.matches("— Zolly").count();
        assert_eq!(signoff_count, 1, "Sign-off should appear exactly once, got {}:\n{}", signoff_count, tg);
        
        // Should end with sign-off
        assert!(tg.trim_end().ends_with("— Zolly"), "Must end with Zolly sign-off:\n{}", tg);
    }
}
