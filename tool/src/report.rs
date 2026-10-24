//! Opt-in automatic reports for defects in zolly itself.
//!
//! This is not a project-error channel. It is enabled only by the installer's
//! contribution-policy consent (`contribute = true`), and it reports to the
//! zolly repository, never to the project using zolly. Errors include the
//! command name and a sanitized detail string; command arguments, prompts,
//! project files, environment variables and credentials are never sent.

use crate::office::Office;
use regex::Regex;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ZOLLY_REPO: &str = "khanshifaul/zolly";
const MAX_REPORTS_PER_DAY: usize = 5;

static TOKEN_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"gh[pousr]_[A-Za-z0-9]{20,}",
        r"github_pat_[A-Za-z0-9_]{20,}",
        r"sk-[A-Za-z0-9]{16,}",
        r"AKIA[0-9A-Z]{16}",
        r"bot\d+:[A-Za-z0-9_-]+",
        r"eyJ[A-Za-z0-9_=-]{10,}\.eyJ",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Only errors that name an internal zolly failure are auto-filed. Expected
/// user/configuration errors are deliberately excluded: a missing test
/// command, a dirty checkout, or a busy gate is not a kit defect.
pub fn is_reportable(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    let internal = [
        "panicked",
        "failed to parse",
        "failed to serialize",
        "internal error",
        "unexpected",
        "not implemented",
        "assertion failed",
    ];
    internal.iter().any(|needle| d.contains(needle))
}

/// File one report when consent is enabled, the failure is new, and the local
/// rate cap allows it. Never blocks or fails the caller.
pub fn maybe_report(root: &Path, command: &str, detail: &str) {
    if !Office::load(root.to_path_buf()).contribute {
        return;
    }
    let detail = sanitize(root, detail);
    let fingerprint = fingerprint(command, &detail);
    maybe_report_sanitized(root, command, &detail, &fingerprint);
}

/// Install a panic hook that files a report before the default hook prints.
/// A failing reporter must never mask or replace the panic output.
pub fn install_panic_hook(root: PathBuf, command: String) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let detail = format!("panic: {info}");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            maybe_report(&root, &command, &detail);
        }));
        default(info);
    }));
}

fn maybe_report_sanitized(root: &Path, command: &str, detail: &str, fingerprint: &str) {
    let state = state_path(root);
    if already_reported(&state, fingerprint) || rate_limited(&state) {
        return;
    }
    if !gh_ready() {
        return;
    }
    if issue_exists(fingerprint) {
        mark_reported(&state, fingerprint);
        return;
    }

    let title = title(command, detail);
    let body = body(fingerprint, command, detail);
    match create_issue(&title, &body) {
        Ok(url) => {
            mark_reported(&state, fingerprint);
            eprintln!("zolly: automatic report created: {url}");
        }
        Err(e) => {
            // A failed report must not turn a working command into a failure.
            eprintln!("zolly: automatic report skipped: {e}");
        }
    }
}

fn sanitize(root: &Path, detail: &str) -> String {
    let mut out = detail.to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            out = out.replace(&home, "~");
        }
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_text = root.to_string_lossy();
    if !root_text.is_empty() {
        out = out.replace(root_text.as_ref(), "<office>");
    }
    for re in TOKEN_RES.iter() {
        out = re.replace_all(&out, "[redacted]").into_owned();
    }
    out.chars().take(6000).collect()
}

fn fingerprint(command: &str, detail: &str) -> String {
    // FNV-1a: stable across builds, unlike DefaultHasher.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in command.bytes().chain(b"\0".iter().copied()).chain(detail.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn title(command: &str, detail: &str) -> String {
    let first = detail.lines().next().unwrap_or("zolly failure");
    let first: String = first
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut title = format!("[auto] zolly {command}: {}", first.trim());
    title = title.chars().take(110).collect();
    title
}

fn body(fingerprint: &str, command: &str, detail: &str) -> String {
    format!(
        "<!-- zolly-auto-report: {fingerprint} -->\n\
         ## Automated zolly report\n\n\
         - fingerprint: `{fingerprint}`\n\
         - zolly: {}\n\
         - command: `{command}`\n\
         - platform: {} / {}\n\
         - at: {}\n\n\
         ### Detail\n\n\
         ```text\n{detail}\n```\n\n\
         This report was enabled by the installer's contribution-policy consent.\n\
         It contains no command arguments, project files, environment variables, or credentials.\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Utc::now().to_rfc3339(),
    )
}

fn gh_ready() -> bool {
    std::process::Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn issue_exists(fingerprint: &str) -> bool {
    let query = format!("zolly auto report {fingerprint}");
    let Ok(out) = std::process::Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            ZOLLY_REPO,
            "--state",
            "all",
            "--search",
            &query,
            "--limit",
            "1",
            "--json",
            "number",
        ])
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim() != "[]"
}

fn create_issue(title: &str, body: &str) -> Result<String, String> {
    let path = temp_body_path();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("cannot create body file: {e}"))?;
    file.write_all(body.as_bytes())
        .map_err(|e| format!("cannot write body file: {e}"))?;
    drop(file);

    let output = std::process::Command::new("gh")
        .args([
            "issue",
            "create",
            "--repo",
            ZOLLY_REPO,
            "--title",
            title,
            "--body-file",
            &path.to_string_lossy(),
        ])
        .output();
    let _ = std::fs::remove_file(&path);
    match output {
        Ok(out) if out.status.success() => {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Ok(url)
        }
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn temp_body_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "zolly-auto-report-{}-{nanos}.md",
        std::process::id()
    ))
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".agents/zolly/auto-reports")
}

fn state_lines(state: &Path) -> Vec<(u64, String)> {
    let Ok(text) = std::fs::read_to_string(state) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (stamp, fp) = line.split_once('\t')?;
            Some((stamp.parse().ok()?, fp.to_string()))
        })
        .collect()
}

fn already_reported(state: &Path, fingerprint: &str) -> bool {
    state_lines(state).iter().any(|(_, fp)| fp == fingerprint)
}

fn rate_limited(state: &Path) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state_lines(state)
        .iter()
        .filter(|(stamp, _)| now.saturating_sub(*stamp) < 86_400)
        .count()
        >= MAX_REPORTS_PER_DAY
}

fn mark_reported(state: &Path, fingerprint: &str) {
    if let Some(parent) = state.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state)
    {
        let _ = writeln!(file, "{now}\t{fingerprint}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_user_errors_are_not_reported() {
        assert!(!is_reportable("no test-command — add `test-command:`"));
        assert!(!is_reportable("not a git repository"));
        assert!(!is_reportable("refusing to touch main: dirty"));
        assert!(!is_reportable("zolly: prompt submitted; do not resend"));
    }

    #[test]
    fn internal_failures_are_reportable() {
        assert!(is_reportable("Failed to parse herdr worktree list"));
        assert!(is_reportable("thread 'main' panicked at src/main.rs"));
        assert!(!is_reportable("Failed to run herdr agent prompt"));
    }

    #[test]
    fn sanitize_redacts_office_and_tokens() {
        let dir = std::env::temp_dir().join("zolly-report-sanitize");
        std::fs::create_dir_all(&dir).unwrap();
        let out = sanitize(
            &dir,
            "failed in /tmp/zolly-report-sanitize with ghp_abcdefghijklmnopqrstuvwxyz",
        );
        assert!(out.contains("<office>"), "{out}");
        assert!(!out.contains("ghp_"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(fingerprint("close", "detail"), fingerprint("close", "detail"));
        assert_ne!(fingerprint("close", "detail"), fingerprint("close", "other"));
    }
}
