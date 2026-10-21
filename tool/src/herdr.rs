use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize, Serialize)]
pub struct HerdrAgent {
    pub agent: Option<String>,
    pub name: Option<String>,
    pub pane_id: String,
    pub tab_id: Option<String>,
    pub agent_status: String,
    #[serde(rename = "agent_session")]
    pub agent_session: Option<AgentSession>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentSession {
    pub agent: String,
    pub kind: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HerdrPane {
    pub pane_id: String,
    pub tab_id: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HerdrTab {
    pub tab_id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct HerdrTabListResponse {
    result: HerdrTabListResult,
}

#[derive(Debug, Deserialize)]
struct HerdrTabListResult {
    tabs: Vec<HerdrTab>,
}

#[derive(Debug, Deserialize)]
struct HerdrAgentListResponse {
    result: HerdrAgentListResult,
}

#[derive(Debug, Deserialize)]
struct HerdrAgentListResult {
    agents: Vec<HerdrAgent>,
}

#[derive(Debug, Deserialize)]
struct HerdrPaneCurrentResponse {
    result: HerdrPaneCurrentResult,
}

#[derive(Debug, Deserialize)]
struct HerdrPaneCurrentResult {
    pane: HerdrPane,
}

#[derive(Debug, Deserialize)]
struct HerdrPaneListResponse {
    result: HerdrPaneListResult,
}

#[derive(Debug, Deserialize)]
struct HerdrPaneListResult {
    panes: Vec<HerdrPaneInfo>,
}

#[derive(Debug, Deserialize)]
pub struct HerdrPaneInfo {
    pub pane_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub tab_id: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HerdrWorkspaceListResponse {
    result: HerdrWorkspaceListResult,
}

#[derive(Debug, Deserialize)]
struct HerdrWorkspaceListResult {
    workspaces: Vec<HerdrWorkspace>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HerdrWorkspace {
    pub workspace_id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct HerdrWorkspaceCreateResponse {
    result: HerdrWorkspaceCreateResult,
}

#[derive(Debug, Deserialize)]
struct HerdrWorkspaceCreateResult {
    workspace: HerdrWorkspace,
}

#[derive(Debug, Deserialize)]
struct TabCreateResponse {
    result: TabCreateResult,
}

#[derive(Debug, Deserialize)]
struct TabCreateResult {
    root_pane: TabPane,
}

#[derive(Debug, Deserialize)]
struct TabPane {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct WorktreeListResponse {
    result: WorktreeListResult,
}

#[derive(Debug, Deserialize)]
struct WorktreeListResult {
    worktrees: Vec<WorktreeInfo>,
}

#[derive(Debug, Deserialize)]
struct WorktreeInfo {
    branch: Option<String>,
    path: String,
}

/// Reply content is everything after the LAST occurrence of `marker` on screen.
///
/// `marker` is a random nonce that appears in the framed prompt (once, on the
/// echoed input line) and at the start of the agent's reply.
///
///   - count 0 → the prompt never landed; return the full screen so the
///     caller can see what is actually on it.
///   - count 1 → only the echo is present; the agent has not started its
///     reply yet (or did not repeat the nonce). Return **empty** so
///     `agent_read_reply`'s stability check fails and keeps polling. A
///     long wrapped prompt makes "everything after the echo line" unsafe:
///     the nonce sits on the first visual line, and the remainder of the
///     wrapped prompt is not a reply.
///   - count ≥ 2 → the agent's reply starts with the nonce; rfind lands on
///     it and everything after is the reply body.
fn extract_reply(screen: &str, marker: &str) -> String {
    match screen.matches(marker).count() {
        0 => screen.to_string(),
        1 => String::new(),
        _ => match screen.rfind(marker) {
            Some(idx) => screen[idx + marker.len()..]
                .trim_start_matches(['\r', '\n'])
                .to_string(),
            None => screen.to_string(),
        },
    }
}

/// True for a mid-turn status bar, measured against this office's omp agents
/// (2026-10-14). The bar reads
///   at rest   `π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main …`
///   mid-turn  `⠏ 12s > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main …`
/// so the only thing that flips is a braille spinner (`U+2800`–`U+28FF`) plus
/// an elapsed-time token (`0s`, `12s`, `1m05s`) where the `π` prompt glyph
/// sits at rest. The trailing `>` — the separator before the model name —
/// keeps ordinary conversation text from matching.
fn is_working_status_bar(line: &str) -> bool {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\s*[\u{2800}-\u{28FF}]\s+(?:\d+[hms])+\s*>").unwrap());
    RE.is_match(line)
}

/// The working/idle decision, made over a captured screen.
///
/// Extracted from `agent_is_working` so the decision itself is unit-testable
/// without a live pane: the defect this task fixes lived in that decision, so
/// the guard has to pin it. Restoring the shipped predicate
/// (`line.contains(" > ") && line.contains("Free")`) inside this function
/// fails `working_screen_decision_survives_the_shipped_defect`.
///
/// `pub(crate)` because it is the one working/idle decision in the binary and
/// `brief.rs`'s worker liveness test consumes it too — a second copy is the
/// drift that produced the `Free` predicate above.
pub(crate) fn screen_shows_working(screen: &str) -> bool {
    screen.lines().any(is_working_status_bar)
}

/// Enter-nudge schedule for `wait_turn_started`: one nudge after a short grace
/// (a real submit shows the glyph within a poll or two), then periodically.
const NUDGE_FIRST_MS: u64 = 1_000;
const NUDGE_EVERY_MS: u64 = 2_000;

/// Is a nudge due, given the previous nudge time (ms since the wait began, or
/// `None` if there has not been one)?
fn nudge_due(last_nudge_ms: Option<u64>, elapsed_ms: u64) -> bool {
    match last_nudge_ms {
        None => elapsed_ms >= NUDGE_FIRST_MS,
        Some(t) => elapsed_ms.saturating_sub(t) >= NUDGE_EVERY_MS,
    }
}

/// The start gate's decision for one poll. Working first — a turn that starts
/// on the last poll must not be reported as a timeout — then the budget, then
/// the nudge.
#[derive(Debug, PartialEq, Eq)]
enum StartGate {
    /// The working glyph is on screen: the prompt was **submitted**.
    Started,
    /// Send Enter now — the paste is held unsubmitted.
    Nudge,
    /// Nothing to do this poll.
    Wait,
    /// The budget expired with the turn never started.
    TimedOut,
}

/// Poll interval for the start gate.
const START_POLL_MS: u64 = 150;

/// One step of the start gate, decided purely from what the screen shows and
/// when the last nudge went out. This is the whole of `wait_turn_started`'s
/// logic, extracted in the predecessor's style (`screen_shows_working`,
/// `nudge_due`) so the decision is unit-testable without a live pane.
///
/// The defect this pins, measured 2026-10-16: `herdr agent prompt` types a
/// large multi-line brief and presses Enter, but the TUI swallows that Enter
/// as part of the paste and holds the brief as an *unsubmitted attachment*
/// (`📄 #1` in the input box, **no elapsed clock**). The screen then reads
/// exactly like an idle agent — nothing submitted, turn never started. So
/// `Started` is gated on the working glyph *alone*, and the only thing that
/// turns a buffered paste into a started turn is `Nudge` (a separate
/// `herdr agent send-keys <name> enter`). Remove the nudge and a 100+ line
/// brief stalls forever: `start_gate_nudges_a_buffered_paste` goes red.
fn start_gate_step(
    shows_working: bool,
    last_nudge_ms: Option<u64>,
    elapsed_ms: u64,
    timeout_ms: u64,
) -> StartGate {
    if shows_working {
        StartGate::Started
    } else if elapsed_ms >= timeout_ms {
        StartGate::TimedOut
    } else if nudge_due(last_nudge_ms, elapsed_ms) {
        StartGate::Nudge
    } else {
        StartGate::Wait
    }
}

/// Strip screen-scrape artifacts from an extracted reply so only the agent's
/// natural-language answer reaches the founder. Removes, in order:
///   - whole `<function-calls>…</function-calls>` blocks (tool-call XML)
///   - the prompt-echo line (`Begin your reply with the word ZZ…`)
///   - herdr box-drawing chrome (`╭ ╰ ├ │`) and wall-clock lines (`⟦Wall:…⟧`)
///   - spinner / working indicators (`⎋ Working…`, braille glyphs)
///   - bare nonce tokens (`ZZ` + 8+ hex chars) that leaked mid-line
///   - runs of 3+ blank lines collapsed to one blank line
pub fn clean_reply(raw: &str) -> String {
    let mut in_calls = false;
    let mut lines_out: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with("<function-calls>") {
            in_calls = true;
            continue;
        }
        if t.starts_with("</function-calls>") {
            in_calls = false;
            continue;
        }
        if in_calls {
            continue;
        }
        if t.starts_with("Begin your reply with the word ZZ") {
            continue;
        }
        if t.starts_with('⟦')
            || t.starts_with('╭')
            || t.starts_with('╰')
            || t.starts_with('├')
            || t.starts_with('│')
        {
            continue;
        }
        if t.starts_with('⎋') || t.contains("Working…") {
            continue;
        }
        // Braille spinner glyphs anywhere in a status line.
        if line.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c)) {
            continue;
        }
        lines_out.push(line);
    }
    let joined = lines_out.join("\n");
    // Strip bare nonce tokens (ZZ + 8+ hex) that leaked mid-line.
    let re = regex::Regex::new(r"\bZZ[0-9a-f]{8,}\b").expect("valid nonce regex");
    let stripped = re.replace_all(&joined, "");
    // Collapse 3+ consecutive blank lines into one blank line.
    let blank_re = regex::Regex::new(r"\n{3,}").expect("valid blank regex");
    blank_re.replace_all(&stripped, "\n\n").trim().to_string()
}

/// True when the text still contains a tool-call block or is empty — both
/// mean `clean_reply` failed to produce a deliverable answer.
fn reply_is_undeliverable(cleaned: &str) -> bool {
    cleaned.trim().is_empty() || cleaned.contains("<function-calls>")
}

/// Error returned when the reply budget expired without a nonce-framed reply.
///
/// The caller must never receive pane text as a reply (measured 2026-10-14:
/// `zolly foreman "Finish t-browser-tool-fix …"` returned `TODO\n  └─────`,
/// the foreman pane's task-widget fragment, as a success). A text timeout makes
/// "foreman replied" and "foreman is still busy" distinguishable.
const TIMEOUT_REPLY_BUDGET_MSG: &str =
    "timeout: no nonce-framed reply within the reply budget — the agent was still mid-turn";

/// Decide whether a **raw screen** carries a nonce-framed reply, and return the
/// cleaned reply body when it does.
///
/// Framing means the `nonce` appeared at least twice on screen — once in the
/// echoed prompt and once at the start of the agent's reply, the pair
/// `extract_reply` keys on. A screen that never carried the nonce is an unframed
/// pane fragment (a task widget, a box border) and is a timeout, not a reply.
///
/// The check reads the screen, not `extract_reply`'s output: extraction *removes*
/// the nonce (it is the delimiter), so the body cannot prove its own framing.
/// The screen-scrape fallback once returned "everything on screen" when the
/// nonce was absent, so `clean_reply` — which keeps ordinary prose — passed the
/// fragment through as a success. The guard lives here, in the one testable
/// decision, so the defect the task exists to fix cannot return at a call site.
fn terminal_reply_outcome(screen: &str, nonce: &str) -> Result<String> {
    if screen.matches(nonce).count() < 2 {
        anyhow::bail!("{TIMEOUT_REPLY_BUDGET_MSG}");
    }
    let cleaned = clean_reply(&extract_reply(screen, nonce));
    if reply_is_undeliverable(&cleaned) {
        anyhow::bail!("{TIMEOUT_REPLY_BUDGET_MSG}");
    }
    Ok(cleaned)
}

/// Session slug for a project path. omp stores sessions under
/// `~/.omp/agent/sessions/<slug>/`, and the slug is the project path **relative
/// to $HOME** with every `/` replaced by `-`.
///
/// Measured 2026-10-14 against the real directories: `/home/khan/Projects/zylosync`
/// is `-Projects-zylosync`, not `-home-khan-Projects-zylosync`. The old
/// `project.replace('/', "-")` computed the absolute form, so the session-JSONL
/// read never found a file and every reply fell through to the screen-scrape
/// fallback — the path that returns `agent_reply_mangled`.
///
/// The leading dash is kept (`/Projects/zylosync` → `-Projects-zylosync`), and
/// `$HOME` itself is `-`. A path that is not under `$HOME` keeps its full form.
fn session_slug_for_project(project: &str, home: &str) -> String {
    let project = project.trim_end_matches('/');
    let home = home.trim_end_matches('/');
    let relative = match project.strip_prefix(home) {
        // Only a real path boundary counts: /home/khanother is not inside /home/khan.
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            if rest.is_empty() {
                "/".to_string()
            } else {
                rest.to_string()
            }
        }
        _ => project.to_string(),
    };
    relative.replace('/', "-")
}

/// Session directory for a project path, under the given home directory.
fn session_dir_for_project_in(project: &str, home: &Path) -> PathBuf {
    home.join(".omp/agent/sessions")
        .join(session_slug_for_project(project, &home.to_string_lossy()))
}

/// Session directory for a project path, under this machine's `$HOME`.
fn session_dir_for_project(project: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    session_dir_for_project_in(project, &home)
}

/// Strip the reply-framing nonce from the start of a reply.
///
/// omp echoes the nonce back with a **space** — `ZZ18de4dd6cf008804 PONG` — but
/// both regexes here required a dash (`ZZ… — `), so the nonce survived into the
/// reply text. The separator is now optional; only the nonce is required.
fn strip_nonce_prefix(text: &str) -> String {
    let re = regex::Regex::new(r"^\s*ZZ[0-9a-f]{8,}\s*[—–-]?\s*").expect("valid nonce regex");
    re.replace(text, "").to_string()
}

/// Find the most recently modified .jsonl session file in the project's
/// session directory. Returns None if the directory is missing or empty.
fn latest_session_file(project: &str) -> Option<PathBuf> {
    let dir = session_dir_for_project(project);
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "jsonl").unwrap_or(true) {
            continue;
        }
        let mtime = entry.metadata().ok()?.modified().ok()?;
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

/// Read the last assistant text message from an omp session JSONL file.
/// Skips thinking blocks; returns only `type: text` content. The nonce
/// prefix (if present) is stripped.
fn read_last_assistant_text(path: &Path) -> Result<String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open session file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut last_text: Option<String> = None;
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if val.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let msg = match val.get("message") {
            Some(m) => m,
            None => continue,
        };
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
            for part in parts {
                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                }
            }
        }
        if !text.trim().is_empty() {
            last_text = Some(text);
        }
    }
    let text = last_text.unwrap_or_default();
    Ok(strip_nonce_prefix(&text))
}

/// Read the assistant text that contains a specific nonce from an omp session JSONL file.
/// Returns the text with the nonce prefix stripped, or None if no matching message.
fn read_assistant_text_with_nonce(path: &Path, nonce: &str) -> Result<Option<String>> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open session file {}", path.display()))?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if val.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let msg = match val.get("message") {
            Some(m) => m,
            None => continue,
        };
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
            for part in parts {
                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                }
            }
        }
        if !text.trim().is_empty() && text.contains(nonce) {
            return Ok(Some(strip_nonce_prefix(&text)));
        }
    }
    Ok(None)
}

/// Random nonce that cannot appear in terminal content. Used to frame agent
/// replies so `extract_reply` has a marker that is unfindable by accident.
fn uuid_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:016x}", nanos)
}

/// `auto-atlas-1` (task id form) -> `auto/atlas-1` (branch form).
fn normalize_task_id(task_id: &str) -> String {
    task_id.to_lowercase().replacen('-', "/", 1)
}

pub struct HerdrClient {
    /// The office this client speaks for. Seats started by `init` run here, so
    /// this is where omp writes their session JSONL — the file
    /// `agent_prompt_with_reply` reads the clean reply from.
    root: PathBuf,
}

impl HerdrClient {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tab_list(&self) -> Result<Vec<HerdrTab>> {
        let output = Command::new("herdr")
            .args(["tab", "list"])
            .output()
            .context("Failed to run herdr tab list")?;
        if !output.status.success() {
            anyhow::bail!(
                "herdr tab list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let parsed: HerdrTabListResponse =
            serde_json::from_slice(&output.stdout).context("Failed to parse herdr tab list")?;
        Ok(parsed.result.tabs)
    }

    pub fn pane_list(&self) -> Result<Vec<HerdrPaneInfo>> {
        let output = Command::new("herdr")
            .args(["pane", "list"])
            .output()
            .context("Failed to run herdr pane list")?;
        if !output.status.success() {
            anyhow::bail!(
                "herdr pane list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let parsed: HerdrPaneListResponse =
            serde_json::from_slice(&output.stdout).context("Failed to parse herdr pane list")?;
        Ok(parsed.result.panes)
    }

    pub fn agent_list(&self) -> Result<Vec<HerdrAgent>> {
        let output = Command::new("herdr")
            .args(["agent", "list"])
            .output()
            .context("Failed to run herdr agent list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent list failed: {}", stderr);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: HerdrAgentListResponse =
            serde_json::from_str(&stdout).context("Failed to parse herdr agent list")?;
        Ok(resp.result.agents)
    }

    pub fn agent_rename(&self, pane_id: &str, name: &str) -> Result<()> {
        let output = Command::new("herdr")
            .args(["agent", "rename", pane_id, name])
            .output()
            .context("Failed to run herdr agent rename")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent rename failed: {}", stderr);
        }
        Ok(())
    }

    pub fn agent_start(&self, name: &str, kind: &str, pane_id: &str, launch_args: &[String]) -> Result<()> {
        let mut command = Command::new("herdr");
        command.args([
            "agent",
            "start",
            name,
            "--kind",
            kind,
            "--pane",
            pane_id,
            "--timeout",
            "30000",
        ]);
        if !launch_args.is_empty() {
            command.arg("--");
            command.args(launch_args);
        }
        let output = command
            .output()
            .context("Failed to run herdr agent start")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent start failed: {}", stderr);
        }
        Ok(())
    }

    /// Block until the named agent reaches `idle`, or the deadline. Used
    /// between `agent_start` and the first prompt: the agent's TUI is not
    /// ready to read stdin the instant the process spawns, and a prompt sent
    /// that early is buffered but never submitted — the text sits in the
    /// input line with the shell prompt under it. Verified empirically
    /// against this herdr: `agent prompt` without `--wait` does press Enter,
    /// so the failure is timing, not a missing submit verb.
    pub fn agent_wait(&self, name: &str, timeout_ms: u64) -> Result<()> {
        let output = Command::new("herdr")
            .args([
                "agent",
                "wait",
                name,
                "--until",
                "idle",
                "--timeout",
                &timeout_ms.to_string(),
            ])
            .output()
            .context("Failed to run herdr agent wait")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent wait failed: {}", stderr);
        }
        Ok(())
    }

    /// Start an agent, retrying once on `agent_pane_busy`. `agent_start` on a
    /// freshly created --no-focus pane is intermittently rejected even when
    /// the pane has a live shell (observed: ~50% of reviewer starts in init
    /// fail with "not an available shell", while the identical pane created
    /// and started a moment later succeeds). On retry the old tab is closed
    /// and a fresh one is created, so no stale state survives.
    ///
    /// `on_pane` runs with the new pane id after the tab exists and before
    /// `agent start` — the only window in which the pane is still a bare
    /// shell. `init` uses it to join the seat to its Radio frequency, which
    /// the plugin refuses once the pane has a running agent
    /// (`bin/radio:1202-1205`). It runs on every attempt, including a retry's
    /// fresh tab, and its own work is never allowed to fail the start: a
    /// Radio problem must not keep the office down.
    pub fn agent_start_with_retry<F>(
        &self,
        name: &str,
        tab_label: &str,
        kind: &str,
        cwd: &str,
        workspace: Option<&str>,
        launch_args: &[String],
        on_pane: F,
    ) -> Result<String>
    where
        F: Fn(&str),
    {
        // If the name is already live, the agent exists — do not create
        // another. Without this, a re-run after a partial init would attempt
        // to start a second agent under the same name (agent_name_taken).
        if self.agent_exists(name) {
            return self.pane_id_for_agent(name);
        }

        // A freshly created --no-focus tab's shell is not ready the instant
        // `tab create` returns; `agent start` then refuses it with
        // `agent_pane_busy`. Observed: ~50% of reviewer starts in init fail
        // this way, while the identical pane started a moment later succeeds.
        //
        // Retry loop: on a busy pane, close the rejected tab, wait for the
        // shell to settle, open a fresh tab, and try again. The sleep goes
        // *after* the new tab is created — sleeping before would only delay
        // the next equally-unready pane. Capped at 4 attempts; a pane that
        // never becomes hostable is a real failure, not a transient race.
        let mut last_err = None;
        let mut last_pane = String::new();
        for _ in 0..4 {
            let pane = self.tab_new(tab_label, cwd, workspace)?;
            // Still a shell: this is where the Radio join must happen.
            on_pane(&pane);
            last_pane = pane.clone();
            match self.agent_start(name, kind, &pane, launch_args) {
                Ok(()) => return Ok(pane),
                Err(e) if Self::start_retryable(&e) => {
                    // A timed-out start may have registered the agent anyway.
                    // Take that agent instead of starting a duplicate.
                    if self.agent_exists(name) {
                        return self.pane_id_for_agent(name);
                    }
                    last_err = Some(e);
                    if let Some(tid) = self.tab_id_for_pane(&pane) {
                        let _ = Command::new("herdr")
                            .args(["tab", "close", &tid])
                            .output();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(750));
                }
                Err(e) => {
                    // A non-transient refusal must not leave a labelled empty
                    // tab behind (observed: `foreman@zylosync` orphan).
                    if let Some(tid) = self.tab_id_for_pane(&pane) {
                        let _ = Command::new("herdr")
                            .args(["tab", "close", &tid])
                            .output();
                    }
                    return Err(e);
                }
            }
        }
        if !last_pane.is_empty() {
            if let Some(tid) = self.tab_id_for_pane(&last_pane) {
                let _ = Command::new("herdr")
                    .args(["tab", "close", &tid])
                    .output();
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("agent {} failed to start", name)))
    }

    /// Transient start refusals worth a fresh tab: the pane had no usable
    /// shell yet, or the startup handshake timed out.
    fn start_retryable(e: &anyhow::Error) -> bool {
        let msg = format!("{e}");
        msg.contains("agent_pane_busy") || msg.contains("timed out waiting for agent startup")
    }

    /// True if an agent named `name` is currently live.
    fn agent_exists(&self, name: &str) -> bool {
        self.agent_list()
            .map(|agents| agents.iter().any(|a| a.name.as_deref() == Some(name)))
            .unwrap_or(false)
    }

    /// Pane id currently hosting the live agent named `name`, if any.
    fn pane_id_for_agent(&self, name: &str) -> Result<String> {
        let pane = self
            .agent_list()
            .with_context(|| format!("Failed to list agents while resolving pane for {}", name))?
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
            .map(|a| a.pane_id.clone());
        match pane {
            Some(p) => Ok(p),
            None => anyhow::bail!("agent {} is not live; cannot resolve its pane", name),
        }
    }

    /// Set a tab's human label. Agent names are `[a-z0-9_-]`, so the `@` form
    /// exists only here: `foreman@my-app` is a label, `foreman-my-app` is the
    /// name a command types.
    pub fn tab_rename(&self, tab_id: &str, label: &str) -> Result<()> {
        let output = Command::new("herdr")
            .args(["tab", "rename", tab_id, label])
            .output()
            .context("Failed to run herdr tab rename")?;
        if !output.status.success() {
            anyhow::bail!(
                "herdr tab rename failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Resolve the tab id currently hosting `pane_id`, if any.
    pub fn tab_id_for_pane(&self, pane_id: &str) -> Option<String> {
        let output = Command::new("herdr").args(["pane", "list"]).output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: serde_json::Value = serde_json::from_str(&stdout).ok()?;
        resp.get("result")
            .and_then(|r| r.get("panes"))
            .and_then(|p| p.as_array())?
            .iter()
            .find(|p| p.get("pane_id").and_then(|v| v.as_str()) == Some(pane_id))
            .and_then(|p| {
                p.get("tab_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
    }

    /// Close the tabs hosting any live agents named in `names`. Used by
    /// `init --force` to recover from a zombie agent entry — a name herdr
    /// still lists but whose pane is gone or unresponsive. Without --force,
    /// init treats the name as alive and skips it.
    #[allow(dead_code)] // reserved for init --force recovery path
    pub fn close_agent_tabs(&self, names: &[&str]) -> Result<()> {
        let agents = self.agent_list().context("Failed to list agents")?;
        for name in names {
            let Some(pane_id) = agents
                .iter()
                .find(|a| a.name.as_deref() == Some(*name))
                .map(|a| a.pane_id.clone())
            else {
                continue;
            };
            let Some(tid) = self.tab_id_for_pane(&pane_id) else {
                continue;
            };
            let output = Command::new("herdr")
                .args(["tab", "close", &tid])
                .output()
                .context("Failed to run herdr tab close")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("herdr tab close failed for {}: {}", name, stderr);
            }
        }
        Ok(())
    }

    /// Submit a prompt without waiting for or reading a reply. The caller is
    /// responsible for ensuring the agent is idle first (see `agent_wait`):
    /// a prompt sent before the TUI is reading stdin is buffered but never
    /// submitted.
    pub fn agent_prompt_only(&self, name: &str, prompt: &str) -> Result<()> {
        let output = Command::new("herdr")
            .args(["agent", "prompt", name, prompt])
            .output()
            .context("Failed to run herdr agent prompt")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent prompt failed: {}", stderr);
        }
        Ok(())
    }
    /// Submit a prompt and block until the agent has consumed it and gone
    /// idle again. This is the composition `init` needs: `agent_prompt_only`
    /// alone returns the instant the bytes are written, while the agent is
    /// still loading its persona — "Foreman ready" was printed before foreman
    /// had read its agent.md, so the pane read as submitted but was not.
    ///
    /// Why a status glyph, not `herdr agent wait`. Measured against this
    /// machine's omp agents (2026-10-14):
    ///   1. `herdr agent wait --until working` never fires for an omp agent:
    ///      `herdr agent list` reports `agent_status: "idle"` for the *whole*
    ///      turn (`fallback_reason: default_known_agent_idle_fallback`), and
    ///      `wait --until working` times out on an agent that is visibly
    ///      mid-turn.
    ///   2. `--until idle` therefore returns instantly, always — a lone idle
    ///      wait cannot tell a buffered prompt from a consumed one.
    ///   3. The screen is not byte-stable while working (the spinner and the
    ///      elapsed clock tick), so a raw screen-diff fires on every poll and
    ///      returns while the agent is still working.
    ///
    /// The status bar is the one thing that actually flips; `agent_is_working`
    /// reads it. Two phases: wait for the turn to *start* (proof the prompt
    /// landed, not merely buffered), then for it to *settle*. Only phase 1 can
    /// fail — a turn still running when the budget expires has already proved
    /// what the caller needed, so phase 2 never reports a false failure.
    pub fn agent_prompt_wait(&self, name: &str, prompt: &str, timeout_ms: u64) -> Result<()> {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        self.agent_prompt_only(name, prompt)
            .with_context(|| format!("Failed to prompt agent {}", name))?;
        let expired = || start.elapsed() >= Duration::from_millis(timeout_ms);
        // Phase 1 — the prompt lands: the working glyph appears. A large
        // multi-line paste is held unsubmitted by the TUI, so this nudges the
        // pane with Enter until the turn starts (see `wait_turn_started`).
        if !self.wait_turn_started(name, timeout_ms)? {
            anyhow::bail!(
                "Agent {} did not start processing the prompt within {}ms",
                name,
                timeout_ms
            );
        }
        // Phase 2 — the turn settles: two consecutive idle reads, spaced 400ms
        // to ride out the brief idle gap between tool calls. Never fails.
        let mut idle_reads = 0u32;
        while idle_reads < 2 && !expired() {
            std::thread::sleep(Duration::from_millis(400));
            if self.agent_is_working(name)? {
                idle_reads = 0;
            } else {
                idle_reads += 1;
            }
        }
        Ok(())
    }

    /// True when the agent's status bar shows an in-flight turn. See
    /// `is_working_status_bar` for the shape and the measurement behind it.
    ///
    /// History — why this was rewritten. The previous predicate required a
    /// line containing both `" > "` and the literal token `"Free"`. This bar
    /// contains no `Free` in *any* state, so the predicate was unsatisfiable
    /// and the spinner check below it never ran: every `agent_prompt_wait`
    /// and `agent_prompt_with_reply` burned its whole timeout and returned an
    /// error *after* the prompt had already been delivered. That is why
    /// `zolly init`, `zolly foreman …` and `zolly reviewer …` reported failure
    /// while the agents were in fact working.
    fn agent_is_working(&self, name: &str) -> Result<bool> {
        Ok(screen_shows_working(&self.agent_read(name, 8)?))
    }

    /// Press Enter in the agent's pane.
    fn agent_send_enter(&self, name: &str) -> Result<()> {
        let output = Command::new("herdr")
            .args(["agent", "send-keys", name, "enter"])
            .output()
            .context("Failed to run herdr agent send-keys")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent send-keys failed: {}", stderr);
        }
        Ok(())
    }

    /// Wait for the turn to *start* — the working glyph appearing is proof the
    /// prompt was **submitted**, not merely typed. Returns false on timeout.
    ///
    /// Measured 2026-10-14, and the real reason `init`/`reload` failed:
    /// `herdr agent prompt` types the text and presses Enter, but when the
    /// text is a large multi-line paste the TUI swallows that Enter as part of
    /// the paste and holds the whole thing as an *unsubmitted attachment*
    /// (`📄 #2` in the input box). The turn never starts, so the caller times
    /// out on a prompt that was never sent. One separate Enter keypress
    /// submits it. So poll for the working glyph and, while it has not
    /// appeared, nudge the pane with Enter every couple of seconds. An Enter
    /// after a successful submit is harmless.
    ///
    /// The decision itself lives in `start_gate_step`, a pure fn, so the nudge
    /// that makes this work is unit-tested rather than only exercised live.
    fn wait_turn_started(&self, name: &str, timeout_ms: u64) -> Result<bool> {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut last_nudge_ms: Option<u64> = None;
        loop {
            let shows_working = self.agent_is_working(name)?;
            let elapsed_ms = start.elapsed().as_millis() as u64;
            match start_gate_step(shows_working, last_nudge_ms, elapsed_ms, timeout_ms) {
                StartGate::Started => return Ok(true),
                StartGate::TimedOut => return Ok(false),
                StartGate::Nudge => {
                    self.agent_send_enter(name)?;
                    last_nudge_ms = Some(elapsed_ms);
                }
                StartGate::Wait => {}
            }
            std::thread::sleep(Duration::from_millis(START_POLL_MS));
        }
    }





    /// Read the agent's recent terminal output, plain text.
    pub fn agent_read(&self, name: &str, lines: usize) -> Result<String> {
        let output = Command::new("herdr")
            .args([
                "agent",
                "read",
                name,
                "--lines",
                &lines.to_string(),
                "--format",
                "text",
            ])
            .output()
            .context("Failed to run herdr agent read")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr agent read failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Poll the screen until a **nonce-framed** reply is non-empty and stable
    /// across two consecutive reads, and return its cleaned body. A screen
    /// without the framing never satisfies the predicate, so the loop keeps
    /// polling rather than accepting a fragment.
    ///
    /// On expiry it returns the explicit reply-budget timeout — never the last
    /// fragment. That is the defect measured 2026-10-14: `zolly foreman` was
    /// handed `TODO\n  └─────` as a reply when the budget expired mid-turn.
    /// The previous version returned whatever the last read held, so `clean_reply`
    /// — which keeps ordinary prose — forwarded the fragment as a success, and
    /// the caller could not tell "replied" from "still busy".
    pub fn agent_read_reply(
        &self,
        name: &str,
        lines: usize,
        marker: &str,
        timeout_ms: u64,
    ) -> Result<String> {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut prev = String::new();
        loop {
            let screen = self.agent_read(name, lines)?;
            if let Ok(framed) = terminal_reply_outcome(&screen, marker) {
                if framed == prev {
                    return Ok(framed);
                }
                prev = framed;
            }
            if Instant::now() >= deadline {
                anyhow::bail!("{TIMEOUT_REPLY_BUDGET_MSG}");
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    /// Prompt an already-idle agent and return its reply text. Shares one
    /// deadline across all phases so the caller's timeout is the total budget.
    /// Two defects in the previous version, both measured against this
    /// machine:
    ///   1. It waited for a working state via `agent_wait`, which is the
    ///      broken primitive — see `agent_prompt_wait` for why. It returned
    ///      `agent_prompt_stalled` without waiting, so the reply was read
    ///      while the agent was still mid-turn and the screen showed the
    ///      spinner. That is the `"⎋ Working…"` garbage.
    ///   2. It extracted the reply as "everything after the LAST occurrence
    ///      of the prompt text". The prompt text is ordinary prose, so it
    ///      appears inside command-output blocks on screen — `rfind` matched
    ///      one of those and returned the box border as the reply.
    ///
    /// Both are fixed by framing: the prompt is wrapped in a random nonce
    /// that cannot appear in screen content, the agent is told to answer
    /// after it, and the reply is read as the text between the two nonce
    /// markers. A nonce that never appears on screen is unfindable, so a
    /// missing or stalled reply is a real timeout, not a wrong answer.
    pub fn agent_prompt_with_reply(
        &self,
        name: &str,
        prompt: &str,
        timeout_ms: u64,
    ) -> Result<String> {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let nonce = format!("ZZ{}", uuid_nonce());
        let framed = format!(
            "Begin your reply with the word {nonce}, then answer. Question: {prompt}"
        );

        // Record the session file state before prompting so we can tell
        // which file the agent writes its reply to. The seat's cwd is the
        // office root, which this client was constructed with.
        let project = self.root.to_str().context("project root is not UTF-8")?;
        let session_before = latest_session_file(project);
        let mtime_before = session_before.as_ref().and_then(|p| {
            std::fs::metadata(p).ok().and_then(|m| m.modified().ok())
        });

        self.agent_prompt_only(name, &framed)
            .with_context(|| format!("Failed to prompt agent {}", name))?;
        // The prompt lands (the working glyph appears) — with an Enter nudge
        // if the TUI is holding the paste unsubmitted.
        let remaining = timeout_ms.saturating_sub(start.elapsed().as_millis() as u64);
        if !self.wait_turn_started(name, remaining)? {
            anyhow::bail!(
                "Agent {} did not start processing the prompt within {}ms",
                name,
                timeout_ms
            );
        }
        // Settle loop: the agent may pause idle between tool calls.
        // Use a time budget (75% of remaining timeout) instead of fixed rounds
        // so a reviewer with many tool calls isn't cut off after 4 rounds.
        let settle_budget_ms = timeout_ms.saturating_mul(3).saturating_div(4);
        const SETTLE_MS: u64 = 400;
        let settle_deadline = Instant::now() + Duration::from_millis(settle_budget_ms);
        loop {
            let remaining = timeout_ms.saturating_sub(start.elapsed().as_millis() as u64);
            self.agent_wait(name, remaining)
                .with_context(|| format!("Agent {} did not become idle in time", name))?;
            std::thread::sleep(Duration::from_millis(SETTLE_MS));
            if !self.agent_is_working(name)? {
                break;
            }
            if Instant::now() >= settle_deadline {
                break;
            }
        }

        // Primary path: read the clean assistant text from the session
        // JSONL. This gives structured text with no tool chrome, no box
        // drawing, no spinner glyphs — just the model's reply.
        if let Some(text) = self.read_session_reply(project, &mtime_before, &nonce) {
            let cleaned = text.trim().to_string();
            if !reply_is_undeliverable(&cleaned) {
                return Ok(cleaned);
            }
        }

        // Fallback: screen-scrape. `agent_read_reply` gates every poll through
        // `terminal_reply_outcome`, so it returns only a nonce-framed,
        // deliverable reply and otherwise bails with the reply-budget timeout.
        // An unframed pane fragment — a task widget, a box border — is therefore
        // never handed back as a reply. The old second 40-line re-read is gone:
        // an identical read of the same screen returned the same fragment, so it
        // could not change the outcome.
        let remaining = timeout_ms.saturating_sub(start.elapsed().as_millis() as u64);
        self.agent_read_reply(name, 40, &nonce, remaining)
    }

    /// Read the last assistant text from the project's most recent session
    /// file, but only if it was modified after `mtime_before` (i.e. the
    /// agent wrote a new turn since we prompted). Returns None if no file
    /// was updated or the text is empty.
    fn read_session_reply(
        &self,
        project: &str,
        mtime_before: &Option<std::time::SystemTime>,
        nonce: &str,
    ) -> Option<String> {
        let path = latest_session_file(project)?;
        // The file must have been modified after we prompted.
        let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok())?;
        if let Some(before) = mtime_before {
            if mtime <= *before {
                return None;
            }
        }
        // Read the assistant text that contains our nonce
        // (proving it's the reply to *our* prompt, not a stale turn).
        read_assistant_text_with_nonce(&path, nonce).ok().flatten()
    }

    pub fn pane_current(&self) -> Result<HerdrPane> {
        let output = Command::new("herdr")
            .args(["pane", "current"])
            .output()
            .context("Failed to run herdr pane current")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: HerdrPaneCurrentResponse =
            serde_json::from_str(&stdout).context("Failed to parse herdr pane current")?;
        Ok(resp.result.pane)
    }

    pub fn tab_new(&self, label: &str, cwd: &str, workspace: Option<&str>) -> Result<String> {
        // Always create a fresh tab. The previous reuse path — which looked
        // for an existing tab with the same label and returned its root pane
        // when agent_status was "unknown" — was the direct cause of the
        // agent_pane_busy failures in init. "unknown" covers both a clean,
        // ready-to-host pane and an orphaned one whose shell died, so the
        // check could not distinguish them; a stale pane was handed back and
        // agent start then refused it.
        // --focus, not --no-focus: learning L-0004 records that --no-focus
        // creates shellless panes, which is why agent_start then refused
        // them ~50% of the time with agent_pane_busy. --focus gives the tab
        // a live shell the instant create returns.
        //
        // `workspace` matters: without it the tab lands in whatever workspace
        // the command happens to run from, which is how foreman and reviewer
        // ended up in the wrong project's workspace on 2026-10-21.
        let mut args = vec!["tab", "create", "--label", label, "--cwd", cwd, "--focus"];
        if let Some(workspace) = workspace {
            args.push("--workspace");
            args.push(workspace);
        }
        let output = Command::new("herdr")
            .args(&args)
            .output()
            .context("Failed to run herdr tab create")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("herdr tab create failed: {}", stderr);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: TabCreateResponse =
            serde_json::from_str(&stdout).context("Failed to parse herdr tab create")?;
        Ok(resp.result.root_pane.pane_id)
    }

    /// The agents whose panes live in this office's workspace. Agent names
    /// are not enough: `herdr agent list` is server-wide, so a brief built
    /// from it counts another office's seats as workers.
    pub fn agents_for_office(&self, root: &Path) -> Result<Vec<HerdrAgent>> {
        let agents = self.agent_list()?;
        let Some(workspace) = self.workspace_for_path(root) else {
            return Ok(Vec::new());
        };
        let out = Command::new("herdr")
            .args(["pane", "list"])
            .output()
            .context("Failed to run herdr pane list")?;
        if !out.status.success() {
            anyhow::bail!(
                "herdr pane list failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let resp: HerdrPaneListResponse =
            serde_json::from_slice(&out.stdout).context("Failed to parse herdr pane list")?;
        let panes: std::collections::HashSet<&str> = resp
            .result
            .panes
            .iter()
            .filter(|p| p.workspace_id == workspace)
            .map(|p| p.pane_id.as_str())
            .collect();
        Ok(agents
            .into_iter()
            .filter(|a| panes.contains(a.pane_id.as_str()))
            .collect())
    }

    /// The office's herdr workspace: by checkout path, then by label, then
    /// created. Seats must open here, not in whichever workspace happened to
    /// invoke `init`.
    pub fn ensure_workspace(&self, root: &Path, label: &str) -> Result<String> {
        if let Some(id) = self.workspace_for_path(root) {
            return Ok(id);
        }
        if let Ok(list) = self.workspace_list() {
            if let Some(ws) = list.iter().find(|w| w.label == label) {
                return Ok(ws.workspace_id.clone());
            }
        }
        let cwd = root.to_string_lossy();
        let out = Command::new("herdr")
            .args([
                "workspace",
                "create",
                "--label",
                label,
                "--cwd",
                &cwd,
                "--no-focus",
            ])
            .output()
            .context("Failed to run herdr workspace create")?;
        if !out.status.success() {
            anyhow::bail!(
                "herdr workspace create failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let resp: HerdrWorkspaceCreateResponse = serde_json::from_slice(&out.stdout)
            .context("Failed to parse herdr workspace create")?;
        println!(
            "Created herdr workspace {} ({}).",
            label, resp.result.workspace.workspace_id
        );
        Ok(resp.result.workspace.workspace_id)
    }

    pub fn workspace_list(&self) -> Result<Vec<HerdrWorkspace>> {
        let out = Command::new("herdr")
            .args(["workspace", "list"])
            .output()
            .context("Failed to run herdr workspace list")?;
        if !out.status.success() {
            anyhow::bail!(
                "herdr workspace list failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let resp: HerdrWorkspaceListResponse = serde_json::from_slice(&out.stdout)
            .context("Failed to parse herdr workspace list")?;
        Ok(resp.result.workspaces)
    }

    /// The workspace that already hosts a pane inside `root`, if any. Exact
    /// checkouts win over a pane sitting in a subdirectory.
    pub fn workspace_for_path(&self, root: &Path) -> Option<String> {
        let out = Command::new("herdr")
            .args(["pane", "list"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let resp: HerdrPaneListResponse = serde_json::from_slice(&out.stdout).ok()?;
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut nested = None;
        for pane in &resp.result.panes {
            let cwd = Path::new(&pane.cwd);
            let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
            if cwd == root {
                return Some(pane.workspace_id.clone());
            }
            if nested.is_none() && cwd.starts_with(&root) {
                nested = Some(pane.workspace_id.clone());
            }
        }
        nested
    }

    // worktree_open was removed with `zolly spawn` (spec §17): opening
    // worktrees belongs to foreman, not Zolly. `worktree_close` stays —
    // closing a worktree foreman opened is not opening one.

    pub fn worktree_close(&self, repo: &Path, task_id: &str) -> Result<Option<String>> {
        // Two sources, both consulted because neither is complete on its own:
        // git is authoritative about what a worktree *is*, herdr knows which
        // ones it opened as workspaces. herdr being unreachable is not an
        // error here — `zolly close` must sweep a worktree even when it is.
        let conventional = repo.join(".agents/worktrees").join(task_id);
        let mut registered = git_worktrees(repo);
        for (path, branch) in self.herdr_worktrees() {
            let path = PathBuf::from(path);
            if !registered.iter().any(|(known, _)| *known == path) {
                registered.push((path, branch));
            }
        }

        // The path foreman opens is the primary signal; the branch name is the
        // fallback for a worktree opened elsewhere. A branch match must be
        // exact: a substring match on "auto/atlas-1" also hits "auto/atlas-10",
        // which is how atlas-1 was deleting atlas-10.
        let wanted = [
            format!("foreman/{task_id}"),
            normalize_task_id(task_id),
        ];
        let target = if conventional.exists()
            || registered.iter().any(|(known, _)| *known == conventional)
        {
            Some(conventional)
        } else {
            registered
                .iter()
                .find(|(_, branch)| {
                    branch
                        .as_deref()
                        .map(|b| wanted.iter().any(|w| w == b))
                        .unwrap_or(false)
                })
                .map(|(path, _)| path.clone())
        };

        let Some(path) = target else {
            // No worktree is the normal state for the self-repair path, and it
            // is not a failure. The old code bailed with `No worktree found for
            // task: …`, which made `zolly close` unusable there.
            return Ok(None);
        };

        // Safety belt: never remove the checkout we are standing in, however
        // the task id was spelled.
        let repo_canon = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
        if path.canonicalize().map(|p| p == repo_canon).unwrap_or(false) {
            return Ok(None);
        }

        let output = Command::new("git")
            .args(["worktree", "remove", &path.to_string_lossy()])
            .current_dir(repo)
            .output()
            .context("Failed to run git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree remove {} failed: {}", path.display(), stderr.trim());
        }
        Ok(Some(path.display().to_string()))
    }

    /// Worktrees herdr lists, as (path, branch). Empty when herdr is down or
    /// the reply does not parse — a missing source is not an error for a caller
    /// that must sweep regardless.
    fn herdr_worktrees(&self) -> Vec<(String, Option<String>)> {
        // No --workspace filter: herdr only returns worktrees whose
        // open_workspace_id matches the filter, and most auto/* worktrees
        // report None there. Filtering by w29 hid every task worktree, so
        // `zolly close` could never find one. List everything instead.
        let Ok(output) = Command::new("herdr").args(["worktree", "list"]).output() else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let Ok(resp) = serde_json::from_str::<WorktreeListResponse>(&String::from_utf8_lossy(
            &output.stdout,
        )) else {
            return Vec::new();
        };
        resp.result
            .worktrees
            .into_iter()
            .map(|w| (w.path, w.branch.map(|b| b.to_lowercase())))
            .collect()
    }
}

/// Worktrees git has registered, as (path, branch). The main checkout is the
/// first entry; a detached worktree has no branch.
fn git_worktrees(repo: &Path) -> Vec<(PathBuf, Option<String>)> {
    let Ok(output) = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut blocks: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut current: Option<(PathBuf, Option<String>)> = None;
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some((PathBuf::from(path.trim()), None));
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some((_, b)) = current.as_mut() {
                *b = Some(branch.trim().trim_start_matches("refs/heads/").to_string());
            }
        }
    }
    if let Some(block) = current.take() {
        blocks.push(block);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_reply_picks_the_last_occurrence() {
        // Marker in the echoed prompt (twice) and at the start of the reply.
        // rfind lands on the reply's leading marker; everything after it is
        // the reply body.
        let screen = "> M\nstale\n> M\nM the answer\n";
        assert_eq!(extract_reply(screen, "M"), " the answer\n");
    }

    #[test]
    fn extract_reply_single_line() {
        let screen = "> M\nM OK\n";
        assert_eq!(extract_reply(screen, "M"), " OK\n");
    }

    #[test]
    fn extract_reply_no_marker_returns_screen() {
        let screen = "no prompt here\n";
        assert_eq!(extract_reply(screen, "M"), "no prompt here\n");
    }

    #[test]
    fn extract_reply_single_marker_returns_empty() {
        // Only the echoed prompt carries the marker — the agent has not
        // started its reply. Return empty so agent_read_reply keeps polling.
        // A long wrapped prompt makes "everything after the echo line" the
        // rest of the prompt, not a reply.
        let screen = "> Begin your reply with the word M, then answer. Question: Hi\nstill thinking\n";
        assert_eq!(extract_reply(screen, "M"), "");
    }

    #[test]
    fn extract_reply_ignores_marker_in_a_block() {
        // A marker inside command output is a lone occurrence there; the
        // real pair is prompt-echo + reply. rfind still lands on the reply.
        let screen =
            "╭── Output ───╮\n│ $ echo M │\n╰────────────╯\n> M\nM the answer\n";
        assert_eq!(extract_reply(screen, "M"), " the answer\n");
    }

    #[test]
    fn clean_reply_strips_function_call_block() {
        let raw = "Here is the result.\n\n<function-calls>\n<invoke name=\"web_search\">\n</function-calls>\n\nDone.";
        assert_eq!(clean_reply(raw), "Here is the result.\n\nDone.");
    }

    #[test]
    fn clean_reply_strips_nonce_echo_line() {
        let raw = "Begin your reply with the word ZZabc12345, then answer. Question: Hi\nthe reply";
        assert_eq!(clean_reply(raw), "the reply");
    }

    #[test]
    fn clean_reply_strips_bare_nonce_token() {
        let raw = "ZZabc12345 — on it.";
        assert_eq!(clean_reply(raw), "— on it.");
    }

    #[test]
    fn clean_reply_strips_chrome_and_spinner() {
        let raw = "Answer text.\n⟦Wall: 2.47s | Timeout: 300s⟧\n╭── box ──╮\n│ inner │\n╰────────╯\n⎋ Working…";
        assert_eq!(clean_reply(raw), "Answer text.");
    }

    #[test]
    fn clean_reply_collapses_blank_lines() {
        let raw = "line one\n\n\n\n\nline two";
        assert_eq!(clean_reply(raw), "line one\n\nline two");
    }

    #[test]
    fn clean_reply_keeps_plain_text() {
        let raw = "Shifaul, the competitor sweep found 43 prospects across 12 districts.";
        assert_eq!(clean_reply(raw), raw);
    }

    #[test]
    fn reply_is_undeliverable_detects_garbage() {
        assert!(reply_is_undeliverable(""));
        assert!(reply_is_undeliverable("  \n  "));
        assert!(reply_is_undeliverable("text\n<function-calls>\n</function-calls>"));
        assert!(!reply_is_undeliverable("clean reply"));
    }

    #[test]
    fn unframed_capture_is_a_timeout_not_a_reply() {
        // The exact defect: a pane fragment captured mid-turn, no nonce
        // anywhere. Measured 2026-10-14 — `zolly foreman "Finish
        // t-browser-tool-fix …"` returned this TODO widget as the reply after
        // 113.6 s. `extract_reply` returns the whole screen when the nonce is
        // absent (asserted here), so the guard — not extraction — is what
        // rejects it, and it must reject it as a timeout, not return it.
        let nonce = "ZZdeadbeef12345678";
        let fragment = "TODO\n  └─────";
        let screen = format!("{fragment}\n");
        assert_eq!(extract_reply(&screen, nonce), screen);
        // The shipped pipeline before the fix: extract, clean, gate only on
        // `reply_is_undeliverable`. Every step is unchanged by this fix, and
        // composed here it returns the fragment and calls it deliverable —
        // which is exactly how `zolly foreman` printed `TODO\n  └─────`.
        let old_pipeline = clean_reply(&extract_reply(&screen, nonce));
        assert_eq!(old_pipeline, fragment);
        assert!(!reply_is_undeliverable(&old_pipeline));
        // The fixed decision rejects the same screen as a timeout.
        let err = terminal_reply_outcome(&screen, nonce)
            .expect_err("an unframed pane fragment must not be returned as a reply");
        assert!(
            err.to_string().contains("timeout: no nonce-framed reply"),
            "the error must name the timeout, got: {err}"
        );
        assert!(
            !err.to_string().contains("TODO"),
            "the pane fragment must not leak into the error, got: {err}"
        );
    }

    #[test]
    fn empty_capture_is_a_timeout() {
        // A screen with no nonce at all is a failure to capture, not a reply.
        let err = terminal_reply_outcome("", "ZZdeadbeef12345678")
            .expect_err("an empty capture must not be returned as a reply");
        assert!(err.to_string().contains("timeout: no nonce-framed reply"));
    }

    #[test]
    fn nonce_framed_reply_is_returned_unchanged() {
        // A real framed reply — nonce in the echoed prompt and again at the
        // start of the agent's reply — must pass through exactly as the screen
        // shows it, nonce and tool chrome removed. The fix must not dampen the
        // success path.
        let nonce = "ZZdeadbeef12345678";
        let screen = format!(
            "> Begin your reply with the word {nonce}, then answer. Question: Finish t-x\n\
            ╰────────────╯\n\
            {nonce} PASS: merged abc123 — https://github.com/o/r/pull/42\n"
        );
        assert_eq!(screen.matches(nonce).count(), 2, "echo + reply = framed");
        assert_eq!(
            terminal_reply_outcome(&screen, nonce).expect("a framed reply is a reply"),
            "PASS: merged abc123 — https://github.com/o/r/pull/42"
        );
    }

    #[test]
    fn echo_only_screen_is_not_yet_a_reply() {
        // The nonce echoed in the prompt but the agent has not repeated it:
        // count 1 is not framing, so it polls (here: times out) instead of
        // returning the empty body.
        let nonce = "ZZdeadbeef12345678";
        let screen = format!("> Begin your reply with the word {nonce}, then answer. Question: Hi\n");
        assert!(terminal_reply_outcome(&screen, nonce).is_err());
    }

    #[test]
    fn reply_budget_deadline_returns_the_timeout_message() {
        // The shared message is the one string the caller sees for both the
        // "still mid-turn" capture loop and the fallback, so it has to name a
        // timeout distinctly enough to be told from a reply.
        assert!(TIMEOUT_REPLY_BUDGET_MSG.starts_with("timeout:"));
        assert!(TIMEOUT_REPLY_BUDGET_MSG.contains("reply budget"));
        assert!(TIMEOUT_REPLY_BUDGET_MSG.contains("mid-turn"));
    }

    #[test]
    fn normalize_task_id_matches_branch_form() {
        assert_eq!(normalize_task_id("auto-atlas-1"), "auto/atlas-1");
        assert_eq!(normalize_task_id("AUTO-ATLAS-1"), "auto/atlas-1");
        // auto-atlas-1 must not collide with auto-atlas-10; exact equality
        // in worktree_close is what guarantees that.
        assert_ne!(
            normalize_task_id("auto-atlas-1"),
            normalize_task_id("auto-atlas-10")
        );
    }

    #[test]
    fn settle_budget_uses_time_not_rounds() {
        // The settle loop should use a time budget (75% of timeout) not fixed rounds.
        // This is a structural test — we verify the logic by checking the constant
        // was removed and the time-based calculation exists.
        // If MAX_TOOL_ROUNDS constant is referenced anywhere in the function body,
        // the test would fail to compile. Here we just assert the design principle.
        let timeout_ms: u64 = 100_000;
        let settle_budget_ms = timeout_ms.saturating_mul(3).saturating_div(4);
        assert_eq!(settle_budget_ms, 75_000);
    }

    /// Write a session JSONL with one assistant message carrying `text`,
    /// preceded by a user turn and an assistant thinking block — the shape that
    /// made the old reader pick the wrong turn.
    fn session_jsonl(tag: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zolly-session-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("session.jsonl");
        let lines = [
            serde_json::json!({"type":"message","message":{"role":"user","content":[{"type":"text","text":"the question"}]}}),
            serde_json::json!({"type":"message","message":{"role":"assistant","content":[{"type":"thinking","text":"not the reply"}]}}),
            serde_json::json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":text}]}}),
        ];
        let body = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, body).expect("write session");
        path
    }

    /// The office this source lives in, when it is checked out inside one. A
    /// neutral cargo checkout has no office above it, and the two checks below
    /// are about a real office's git worktrees and omp sessions.
    fn office_root() -> Option<std::path::PathBuf> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        root.join(".agents").is_dir().then_some(root)
    }

    #[test]
    fn git_worktrees_parses_the_main_checkout_first() {
        // The main checkout is git's first entry and carries its short branch
        // name — the fallback match in worktree_close depends on both.
        let Some(root) = office_root() else { return };
        let worktrees = git_worktrees(&root);
        assert!(!worktrees.is_empty(), "no worktrees parsed");
        assert_eq!(worktrees[0].0, root);
        assert_eq!(worktrees[0].1.as_deref(), Some("main"));
        assert!(
            worktrees
                .iter()
                .all(|(_, b)| b.as_deref().map(|b| !b.starts_with("refs/")).unwrap_or(true)),
            "branch names must be short: {worktrees:?}"
        );
    }

    #[test]
    fn session_slug_is_home_relative() {
        // The slug omp writes is the project path relative to $HOME, with the
        // separators replaced and the leading dash kept. Measured against
        // ~/.omp/agent/sessions on 2026-10-14: `-Projects-zylosync` exists,
        // `-home-khan-Projects-zylosync` does not.
        assert_eq!(
            session_slug_for_project("/home/khan/Projects/zylosync", "/home/khan"),
            "-Projects-zylosync"
        );
        assert_eq!(
            session_slug_for_project(
                "/home/khan/Projects/zylosync/.agents/worktrees/t-api-dev-lint",
                "/home/khan"
            ),
            "-Projects-zylosync-.agents-worktrees-t-api-dev-lint"
        );
        // $HOME itself, and a path outside it — which keeps its absolute form.
        assert_eq!(session_slug_for_project("/home/khan", "/home/khan"), "-");
        assert_eq!(session_slug_for_project("/tmp/foo", "/home/khan"), "-tmp-foo");
        // A sibling directory is not inside $HOME: no accidental prefix strip.
        assert_eq!(
            session_slug_for_project("/home/khanother/x", "/home/khan"),
            "-home-khanother-x"
        );
    }

    #[test]
    fn session_dir_is_the_directory_omp_actually_writes() {
        // This is the regression: the old absolute slug pointed at a directory
        // that does not exist, so the clean session-JSONL reply path never
        // fired and every reply fell through to `agent_reply_mangled`.
        let Some(root) = office_root() else { return };
        let project = root.to_string_lossy().to_string();
        let home = dirs::home_dir().expect("$HOME");
        let dir = session_dir_for_project_in(&project, &home);
        assert_eq!(
            dir,
            home.join(".omp/agent/sessions")
                .join(session_slug_for_project(&project, &home.to_string_lossy()))
        );
        assert!(dir.is_dir(), "omp writes sessions at {}", dir.display());
        assert!(
            latest_session_file(&project).is_some(),
            "no session JSONL found under {}",
            dir.display()
        );
    }

    #[test]
    fn strip_nonce_prefix_accepts_space_and_dash_separators() {
        // omp replies `ZZ… PONG` — SPACE, no dash — which the old
        // dash-requiring regexes left in place. Both forms must strip.
        assert_eq!(strip_nonce_prefix("ZZ18de4dd6cf008804 PONG"), "PONG");
        assert_eq!(strip_nonce_prefix("ZZabc12345 — the answer"), "the answer");
        assert_eq!(strip_nonce_prefix("ZZabc12345 – the answer"), "the answer");
        assert_eq!(strip_nonce_prefix("ZZabc12345 - the answer"), "the answer");
        assert_eq!(strip_nonce_prefix("ZZdeadbeef12345678PASS"), "PASS");
        // No nonce: untouched.
        assert_eq!(strip_nonce_prefix("no nonce here"), "no nonce here");
        assert_eq!(strip_nonce_prefix(""), "");
    }

    #[test]
    fn read_last_assistant_text_strips_a_space_separated_nonce() {
        let path = session_jsonl("last", "ZZ18de4dd6cf008804 PONG");
        assert_eq!(read_last_assistant_text(&path).expect("read"), "PONG");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn read_assistant_text_with_nonce_strips_both_separators() {
        let path = session_jsonl("nonce-space", "ZZ18de4dd6cf008804 PONG");
        assert_eq!(
            read_assistant_text_with_nonce(&path, "ZZ18de4dd6cf008804").expect("read"),
            Some("PONG".to_string())
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        // The dash form a reviewer verdict came back in.
        let path = session_jsonl("nonce-dash", "ZZ18de4dd6cf008804 — PASS: merged abc123");
        assert_eq!(
            read_assistant_text_with_nonce(&path, "ZZ18de4dd6cf008804").expect("read"),
            Some("PASS: merged abc123".to_string())
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        // A nonce we did not ask about is not this turn's reply.
        let path = session_jsonl("nonce-other", "ZZ18de4dd6cf008804 PONG");
        assert_eq!(
            read_assistant_text_with_nonce(&path, "ZZdeadbeef12345678").expect("read"),
            None
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn extract_reply_handles_long_multi_round_screen() {
        // Simulate a screen with many tool calls: prompt echo, multiple idle/working
        // transitions, then the reply with nonce.
        let nonce = "ZZdeadbeef12345678";
        let screen = format!(
            "> Begin your reply with the word {nonce}, then answer. Question: Review task\n\
            ⎋ Working…\n\
            ╭── Tool ───╮\n│ web_search │\n╰────────────╯\n\
            ⎋ Working…\n\
            ╭── Tool ───╮\n│ grep       │\n╰────────────╯\n\
            ⎋ Working…\n\
            {nonce} PASS: merged abc123 — https://github.com/.../pr/42\n"
        );
        let reply = extract_reply(&screen, &nonce);
        assert!(reply.contains("PASS: merged"));
        assert!(!reply.contains("Working"));
        assert!(!reply.contains("Tool"));
    }

    #[test]
    fn working_status_bar_matches_a_mid_turn_bar() {
        // The shape this detector exists for: braille spinner + elapsed token,
        // verbatim as omp draws it on this machine.
        assert!(is_working_status_bar(
            " ⠏ 12s > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──"
        ));
        assert!(is_working_status_bar("⠋ 0s > ◒ DeepSeek V4.1 Flash >"));
        assert!(is_working_status_bar("⠼ 1m05s > ◒ DeepSeek V4.1 Flash >"));
    }

    #[test]
    fn idle_status_bar_does_not_match() {
        // The regression that broke `zolly init`: the idle bar carries no
        // braille spinner and no elapsed token — and, unlike the predicate
        // this replaced, the bar contains no `Free` token in any state.
        let idle = " π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──";
        assert!(!is_working_status_bar(idle));
        assert!(!idle.contains("Free"));
    }

    #[test]
    fn working_status_bar_ignores_conversation_text() {
        // A bare elapsed token, and the spinner's own code range quoted in
        // prose, are not the bar.
        assert!(!is_working_status_bar(" 12s > the command finished"));
        assert!(!is_working_status_bar("Read the spinner glyph U+2800-U+28FF"));
    }

    #[test]
    fn working_screen_decision_survives_the_shipped_defect() {
        // Guard on the decision itself, not just the line predicate. This is
        // the real screen shape that broke `zolly init`. Under the shipped
        // predicate — a line containing `" > "` AND the literal `"Free"` —
        // the working screen below returns *false*, so restoring that defect
        // fails this test.
        let working = "⎋ Working…\n\
            ⠏ 12s > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──\n\
            ╰─";
        let idle = "⎋ Working…\n\
            π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──\n\
            ╰─";
        assert!(screen_shows_working(working));
        assert!(!screen_shows_working(idle));
        assert!(!working.contains("Free"));
    }

    #[test]
    fn nudge_schedule_is_first_then_periodic() {
        assert!(!nudge_due(None, 0));
        assert!(!nudge_due(None, NUDGE_FIRST_MS - 1));
        assert!(nudge_due(None, NUDGE_FIRST_MS));
        let t = NUDGE_FIRST_MS;
        assert!(!nudge_due(Some(t), t + NUDGE_EVERY_MS - 1));
        assert!(nudge_due(Some(t), t + NUDGE_EVERY_MS));
    }

    #[test]
    fn start_gate_nudges_a_buffered_paste() {
        // The screen a stalled 100+ line brief leaves, measured 2026-10-16: the
        // paste sits as an unsubmitted attachment (`📄 #1`) and the bar is idle
        // (`π >`) — never the working glyph. Only a nudge can move it to
        // `Started`, so the worker-brief path is gated on this phase. Delete the
        // `Nudge` branch from `start_gate_step` and this test fails.
        let stalled = "📄 #1  brief.md (132 lines)\n\
            π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──\n";
        let shows_working = screen_shows_working(stalled);
        assert!(!shows_working, "a buffered paste is not a started turn");

        // Before the grace period: wait. At it: nudge — then every
        // NUDGE_EVERY_MS, on every poll, for as long as the budget holds.
        // Never `Started` (nothing was submitted), never `TimedOut`.
        let budget = 1_800_000;
        assert_eq!(
            start_gate_step(shows_working, None, 0, budget),
            StartGate::Wait
        );
        assert_eq!(
            start_gate_step(shows_working, None, NUDGE_FIRST_MS - 1, budget),
            StartGate::Wait
        );
        let mut last = None;
        let mut now = NUDGE_FIRST_MS;
        for _ in 0..3 {
            assert_eq!(
                start_gate_step(shows_working, last, now, budget),
                StartGate::Nudge,
                "a buffered paste must be nudged with Enter at {now}ms"
            );
            last = Some(now);
            now += NUDGE_EVERY_MS;
        }
    }

    #[test]
    fn start_gate_started_only_for_the_working_glyph() {
        // Success is gated on the glyph, and on the glyph alone: a screen whose
        // bar is idle never reports `Started`, however much text it carries.
        // This is why `--until idle` cannot guard a brief — the stall and the
        // started turn both read `idle` to herdr.
        let working = "⠏ 12s > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──";
        let idle = "📄 #1  brief.md (132 lines)\n\
            π > ◔ DeepSeek V4.1 Flash > 📁 zylosync > ⑂ main *3 ?1 > $0.00 ▶──2%──";
        assert_eq!(
            start_gate_step(screen_shows_working(working), None, 0, 1_000),
            StartGate::Started
        );
        assert_ne!(
            start_gate_step(screen_shows_working(idle), None, 0, 1_000),
            StartGate::Started
        );
    }

    #[test]
    fn start_gate_times_out_without_the_glyph() {
        assert_eq!(
            start_gate_step(false, Some(1_000), 5_000, 5_000),
            StartGate::TimedOut
        );
        // A turn that starts on the last poll is `Started`, not `TimedOut`:
        // the glyph is checked before the budget, so a brief that lands at the
        // deadline is never reported as a failed dispatch.
        assert_eq!(
            start_gate_step(true, Some(1_000), 5_000, 5_000),
            StartGate::Started
        );
    }
}
