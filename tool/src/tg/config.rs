//! Token, allowlist and HTTP client. Env var wins; `.agents/zolly/.env` is
//! the shell fallback. The systemd unit reads `~/.config/zolly/credentials`
//! via EnvironmentFile instead — `zolly doctor` checks the two sources for
//! drift, because two tokens means the serve bot and a shell-run bot can be
//! different bots.
//!
//! Every request in this submodule goes through [`send_redacted`] /
//! [`json_redacted`] / [`RequestFailure::from_reqwest`], and every failure
//! becomes a [`RequestFailure`] whose message has the bot token stripped.
//! `reqwest::Error`'s `Display` embeds the request URL, and the URL carries the
//! token — that is the leak this file closes. Measured 2026-10-14: the
//! unredacted form had been written to the persistent journal three times by
//! the serve loop's error path.

use anyhow::{Context, Result};
use regex::Regex;
use reqwest::Client;
use std::sync::LazyLock;
use std::time::Duration;

pub const API_BASE: &str = "https://api.telegram.org/bot";

/// herdr agent name of this office's manager. Agent names are unique
/// server-wide, so the frequency is part of the name.
pub fn zolly_agent(root: &std::path::Path) -> String {
    crate::office::Office::load(root.to_path_buf()).agent("zolly")
}

/// Shell fallback for commands run by hand. Precedence: the environment, then
/// `<office>/.agents/zolly/.env`, then the file the installer writes for
/// systemd (`~/.config/zolly/<frequency>/credentials`). The last one matters
/// because a sourced credentials file sets shell variables, not exported ones,
/// so `. credentials && zolly …` did not reach this process.
pub(crate) fn load_credentials() {
    let Ok(root) = std::env::current_dir() else {
        return;
    };
    let local = root.join(".agents/zolly/.env");
    if local.is_file() {
        let _ = dotenv::from_path(&local);
    } else {
        let _ = dotenv::dotenv();
    }
    let office = crate::office::Office::load(root);
    let credentials = dirs::home_dir()
        .map(|home| {
            home.join(".config/zolly")
                .join(&office.frequency)
                .join("credentials")
        })
        .filter(|path| path.is_file());
    if let Some(path) = credentials {
        // dotenv never overrides a variable that is already set.
        let _ = dotenv::from_path(path);
    }
}

/// Telegram long-poll timeout. Kept short so the serve loop's watchdog
/// (pending-reply fallback) still runs at a useful cadence.
pub const POLL_TIMEOUT_SECS: u64 = 5;

/// How long we're willing to wait for the zolly agent's tool reply before
/// sending the founder a fallback. Long enough for the manager to think,
/// short enough that a stuck agent doesn't wedge the conversation.
pub const ZOLLY_TOOL_TIMEOUT_SECS: u64 = 180;

/// First delay after a transient `getUpdates` failure; doubled per consecutive
/// failure up to [`RETRY_MAX_DELAY_MS`]. The listener used to exit and let
/// systemd's `RestartSec=10` do the retrying, which also dropped every pending
/// reply; retrying in-process recovers in a second and keeps that map.
pub const RETRY_BASE_DELAY_MS: u64 = 1_000;
pub const RETRY_MAX_DELAY_MS: u64 = 30_000;

/// `<bot><id>:<secret>` anywhere in a string. Telegram's token charset is
/// `[A-Za-z0-9_-]`; the id before the colon is public and stays.
static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"bot(\d+):[A-Za-z0-9_-]+").expect("token regex is valid"));

/// Strip every bot token from `s`, leaving `https://api.telegram.org/bot<id>:***`.
/// Idempotent — after one pass nothing follows the colon but `***`, which the
/// character class rejects, so a second pass is a no-op.
pub fn redact(s: &str) -> String {
    TOKEN_RE.replace_all(s, "bot${1}:***").into_owned()
}

/// The full `Display` chain of an error, every link redacted. A raw `{:#}` on a
/// reqwest error prints the request URL, token and all, so error strings are
/// built here instead.
pub fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = redact(&e.to_string());
    let mut source = e.source();
    while let Some(inner) = source {
        out.push_str(": ");
        out.push_str(&redact(&inner.to_string()));
        source = inner.source();
    }
    out
}

/// A failed Telegram request: its message — redacted at construction, so the
/// token cannot enter a string — and whether a retry can change the outcome.
#[derive(Debug)]
pub struct RequestFailure {
    message: String,
    transient: bool,
}

impl RequestFailure {
    /// Wrap a reqwest error. A builder error (malformed request), a decode
    /// error, a redirect failure, and an HTTP status error are all things a
    /// retry cannot improve; a connect, DNS, timeout, or reset failure is
    /// exactly what a retry is for.
    pub fn from_reqwest(what: &str, e: &reqwest::Error) -> Self {
        Self {
            message: redact(&format!("{}: {}", what, error_chain(e))),
            transient: !(e.is_builder() || e.is_decode() || e.is_status() || e.is_redirect()),
        }
    }

    /// A refusal we built ourselves — Telegram answering `ok: false`, or a
    /// missing field. Retrying cannot change it.
    pub fn fatal(message: impl AsRef<str>) -> Self {
        Self {
            message: redact(message.as_ref()),
            transient: false,
        }
    }

    /// A transient failure with a caller-chosen message. Only the retry
    /// policy's own tests need to manufacture one; production failures come
    /// from `from_reqwest`.
    #[cfg(test)]
    pub fn transient_for_test(message: &str) -> Self {
        Self {
            message: redact(message),
            transient: true,
        }
    }

    pub fn is_transient(&self) -> bool {
        self.transient
    }
}

impl std::fmt::Display for RequestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RequestFailure {}

/// `RequestBuilder::send` — the only place this submodule calls it, so no
/// reqwest error can escape carrying the request URL.
pub async fn send_redacted(
    req: reqwest::RequestBuilder,
    what: &str,
) -> Result<reqwest::Response, RequestFailure> {
    req.send()
        .await
        .map_err(|e| RequestFailure::from_reqwest(what, &e))
}

/// Decode a JSON body — the only place this submodule calls `Response::json`.
pub async fn json_redacted<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    what: &str,
) -> Result<T, RequestFailure> {
    resp.json::<T>()
        .await
        .map_err(|e| RequestFailure::from_reqwest(what, &e))
}

pub fn get_token() -> Result<String> {
    if std::env::var("ZOLLY_TELEGRAM_TOKEN").is_err() {
        load_credentials();
    }
    std::env::var("ZOLLY_TELEGRAM_TOKEN").context(
        "ZOLLY_TELEGRAM_TOKEN not set. Set it to your bot token from @BotFather.",
    )
}

pub fn get_allowed_ids() -> Result<Vec<i64>> {
    if std::env::var("ZOLLY_ALLOWED_IDS").is_err() {
        load_credentials();
    }
    let ids_str = std::env::var("ZOLLY_ALLOWED_IDS").context(
        "ZOLLY_ALLOWED_IDS not set. Set it to comma-separated Telegram user IDs.",
    )?;
    ids_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<i64>()
                .with_context(|| format!("Invalid user ID in ZOLLY_ALLOWED_IDS: {:?}", s))
        })
        .collect()
}

pub fn get_client() -> Client {
    // 60s timeout: Telegram's long-poll can hold a request for its full
    // timeout, so the client must outlive it or every poll times out.
    Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client")
}

/// Bot API root, including `/bot`. `ZOLLY_TELEGRAM_API` is the server base
/// (the same variable the installer uses for discovery), so a local Bot API
/// server or a test stub can stand in for Telegram.
pub fn api_root() -> String {
    std::env::var("ZOLLY_TELEGRAM_API")
        .ok()
        .map(|base| base.trim().trim_end_matches('/').to_string())
        .filter(|base| !base.is_empty())
        .map(|base| format!("{base}/bot"))
        .unwrap_or_else(|| API_BASE.to_string())
}

pub fn api_url(token: &str, method: &str) -> String {
    format!("{}{token}/{method}", api_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the real credential. Long enough to look like a token,
    /// unique enough that a leaked copy is unmistakable in an assertion.
    const SECRET: &str = "SECRET-do-not-log_this_0123456789abc";

    /// The exact shape the serve loop wrote to the persistent journal on
    /// 2026-10-14 — context, reqwest's URL-bearing `Display`, and the source
    /// chain under it.
    fn journal_line() -> String {
        format!(
            "Failed to get updates: error sending request for url ({}?offset=766343967&timeout=5&allowed_updates=[%22message%22,%22voice%22,%22poll_answer%22]): client error (SendRequest): connection error: Connection reset by peer (os error 104)",
            api_url(&format!("8954499776:{}", SECRET), "getUpdates")
        )
    }

    #[test]
    fn api_url_joins_base_token_method() {
        assert_eq!(api_url("TOK", "getMe"), "https://api.telegram.org/botTOK/getMe");
    }

    #[test]
    fn redact_keeps_the_bot_id_and_strips_the_secret() {
        let out = redact(&journal_line());
        assert!(!out.contains(SECRET), "token survived redaction: {out}");
        assert!(
            out.contains("https://api.telegram.org/bot8954499776:***/getUpdates"),
            "unexpected render: {out}"
        );
        // The diagnostic value of the line survives: the offset and the cause.
        assert!(out.contains("offset=766343967"), "lost the offset: {out}");
        assert!(out.contains("Connection reset by peer"), "lost the cause: {out}");
    }

    #[test]
    fn redact_handles_every_occurrence_and_is_idempotent() {
        let token = format!("42:{}", SECRET);
        let s = format!(
            "{} then {} again",
            api_url(&token, "getMe"),
            api_url(&token, "getUpdates")
        );
        let once = redact(&s);
        assert_eq!(
            once,
            "https://api.telegram.org/bot42:***/getMe then https://api.telegram.org/bot42:***/getUpdates again"
        );
        assert_eq!(redact(&once), once, "redaction must be idempotent");
    }

    #[test]
    fn redact_leaves_text_without_a_token_alone() {
        assert_eq!(redact("no url here"), "no url here");
        assert_eq!(redact(API_BASE), API_BASE);
        assert_eq!(redact("bot123:"), "bot123:");
    }

    /// A genuine transport failure, mapped exactly as the serve loop maps one,
    /// against a closed loopback port. The connect is refused by the kernel;
    /// nothing leaves the machine.
    #[tokio::test]
    async fn a_real_transport_failure_is_redacted_and_classified_transient() {
        let url = format!("http://127.0.0.1:1/bot{}/getUpdates", format!("12345:{}", SECRET));
        let failure = send_redacted(get_client().get(&url), "Failed to get updates")
            .await
            .expect_err("port 1 refuses connections");

        let rendered = failure.to_string();
        assert!(!rendered.contains(SECRET), "token survived: {rendered}");
        assert!(rendered.starts_with("Failed to get updates: "), "{rendered}");
        assert!(rendered.contains("bot12345:***/getUpdates"), "{rendered}");
        assert!(failure.is_transient(), "a refused connect must be retried: {rendered}");
    }

    #[test]
    fn a_self_built_refusal_is_fatal_and_redacted() {
        let failure = RequestFailure::fatal(format!("getUpdates failed: bad token ibot1:{SECRET}"));
        assert!(!failure.is_transient());
        assert!(!failure.to_string().contains(SECRET), "{failure}");
        assert!(failure.to_string().contains("bot1:***"), "{failure}");
    }
}
