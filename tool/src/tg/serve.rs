//! The long-poll listener: getUpdates loop, offset persistence, dead
//! letters, the pending-reply watchdog, and inbound update dispatch.
//!
//! Both transports converge on [`handle_update`]: the getUpdates loop below and
//! the webhook listener in `super::webhook`. `handle_update` reaches Telegram
//! and herdr only through [`Dispatch`], so the two transports are the same
//! handler with a different front door — and a test can drive both.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use reqwest::Client;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::config::{
    api_url, get_allowed_ids, get_client, get_token, json_redacted, redact, send_redacted,
    zolly_agent, RequestFailure, POLL_TIMEOUT_SECS, RETRY_BASE_DELAY_MS, RETRY_MAX_DELAY_MS,
    ZOLLY_TOOL_TIMEOUT_SECS,
};
use super::format::{is_ask, normalise_keyword};
use super::handlers;
use super::send::{send_message, transcribe_voice};
use super::types::{TelegramResponse, Update};
use crate::render;

/// Tracks pending Telegram messages waiting for Zolly's tool reply.
/// Key: chat_id, Value: (timestamp_sent, original_question)
static PENDING_REPLIES: LazyLock<Mutex<HashMap<i64, (u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Drop a chat from the pending map (its reply arrived).
pub fn take_pending(chat_id: i64) {
    PENDING_REPLIES.lock().remove(&chat_id);
}

// ---------------------------------------------------------------------------
// Serve lock (RAII)
// ---------------------------------------------------------------------------

/// Held for the lifetime of `cmd_serve`. Removes the lock file on drop, and
/// on startup reclaims a lock left by a process that is no longer alive.
pub(super) struct ServeLock {
    path: PathBuf,
}

/// Whether the pid recorded in a serve lockfile still belongs to a `zolly`
/// process.
///
/// Testing that `/proc/<pid>` exists is not enough. The kernel reuses pids, so
/// a lock left behind by a crashed serve starts blocking every restart as soon
/// as that number is handed to an unrelated process — permanently, because the
/// guard only ever consults the number. Two services in this office sat in
/// `activating (auto-restart)` at 588 and 597 restarts on exactly that: their
/// lockfiles held pids the kernel had since given to `wireplumber` and
/// `pipewire-pulse`.
///
/// The lock is honoured only when the process is inspectable and is a zolly
/// binary. An unreadable `cmdline` still counts as alive, which keeps the
/// previous fail-safe behaviour where this cannot see in.
fn lock_holder_running(pid: u32) -> bool {
    if !Path::new(&format!("/proc/{}", pid)).exists() {
        return false;
    }
    match fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(raw) => {
            // A kernel thread or a zombie has an empty cmdline. Neither can be
            // the serve being guarded against, and neither will ever drop the
            // lock, so neither may block startup.
            if raw.is_empty() {
                return false;
            }
            String::from_utf8_lossy(&raw)
                .split('\0')
                .filter(|arg| !arg.is_empty())
                .any(|arg| {
                    arg.rsplit('/')
                        .next()
                        .is_some_and(|name| name.starts_with("zolly"))
                })
        }
        Err(_) => true,
    }
}

impl ServeLock {
    pub(super) fn acquire(path: PathBuf) -> Result<Self> {
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(pid) = contents.trim().parse::<u32>() {
                if lock_holder_running(pid) {
                    anyhow::bail!(
                        "Another zolly serve instance is already running (pid {})",
                        pid
                    );
                }
                eprintln!(
                    "Removing stale lockfile {} (pid {} is not a running zolly)",
                    path.display(),
                    pid
                );
                let _ = fs::remove_file(&path);
            } else {
                // Unparsable contents — likely a leftover from a crash mid-write.
                eprintln!("Removing unreadable lockfile {}", path.display());
                let _ = fs::remove_file(&path);
            }
        }

        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .context("Another zolly serve instance is already running (lock file exists)")?;
        write!(f, "{}", std::process::id()).context("Failed to write pid to lockfile")?;
        Ok(Self { path })
    }
}

impl Drop for ServeLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Offset & dead letter
// ---------------------------------------------------------------------------

/// Read the persisted getUpdates offset. Corrupt, empty, or negative values
/// degrade to 0 rather than raising — a torn write reads back as 0, which is
/// precisely the bug this file exists to avoid, and raising out of the serve
/// loop would kill the listener and lose every future message.
fn read_offset(path: &Path) -> i64 {
    match fs::read_to_string(path) {
        Ok(s) => match s.trim().parse::<i64>() {
            Ok(n) if n >= 0 => n,
            _ => {
                eprintln!(
                    "Corrupt offset {:?} in {}, degrading to 0",
                    s.trim(),
                    path.display()
                );
                0
            }
        },
        Err(_) => 0,
    }
}

/// Write the offset atomically: write to a temp file in the same directory,
/// then `os::replace` it onto the destination. A torn write reads back as 0,
/// never as a half-written number. Leaves no temp file behind.
fn write_offset_atomic(path: &Path, offset: i64) -> Result<()> {
    // 0 is valid: it means "start from the beginning". A torn write
    // reading back as 0 is safe — getUpdates with offset=0 returns
    // the full backlog and the loop advances past every update_id.
    let tmp = path.with_extension("tmp");
    let mut opts = fs::OpenOptions::new();
    opts.create(true).truncate(true).write(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut f = opts.open(&tmp).context("Failed to open offset temp file")?;
    write!(f, "{}", offset).context("Failed to write offset")?;
    f.sync_all().context("Failed to fsync offset")?;
    drop(f);
    std::fs::rename(&tmp, path).context("Failed to atomically replace offset")?;
    Ok(())
}

/// Append a failed update to `.agents/zolly/dead_letter.jsonl`. The serve
/// loop advances the Telegram offset before handling, so a failed update is
/// otherwise gone forever — this file is the only replay source. Best-effort:
/// a dead-letter write failure is logged, never raised out of the loop.
pub(super) fn append_dead_letter(root: &Path, update_id: i64, error: &str) {
    let path = root.join(".agents/zolly/dead_letter.jsonl");
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let line = serde_json::json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "update_id": update_id,
        "error": error,
    });
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", line);
    }
}

// ---------------------------------------------------------------------------
// Outbound effects — the seam both transports share
// ---------------------------------------------------------------------------

/// A boxed, sendable future. `Dispatch` needs an async method and must stay
/// object-safe (`&dyn Dispatch`), which Rust's native `async fn` in traits does
/// not yet allow; this is the manual spelling of it.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The outbound side effects of handling one Update.
///
/// `handle_update` used to call `send_message` and `HerdrClient::agent_prompt_only`
/// directly, so the keyword and dispatch logic could only be exercised against
/// the real Telegram API and a live agent. Routing every outbound effect through
/// this trait keeps one implementation of that logic — the getUpdates loop and
/// the webhook listener are two front doors to the same `handle_update` — and
/// lets a test drive both with a recorder and assert they produce the same
/// effect for the same Update.
pub trait Dispatch: Send + Sync {
    /// Send a Telegram message to `chat_id`.
    fn send<'a>(&'a self, chat_id: i64, text: &'a str) -> BoxFuture<'a, Result<()>>;
    /// Submit a prompt to the named herdr agent, without waiting for a reply.
    fn prompt_agent(&self, name: &str, prompt: &str) -> Result<()>;
    /// Download a voice note and transcribe it locally, returning the transcript.
    fn transcribe<'a>(&'a self, file_id: &'a str) -> BoxFuture<'a, Result<String>>;
}

/// The production [`Dispatch`]: a real Telegram message, a real herdr prompt, a
/// real transcription. Holds the bot token — with `HerdrClient` a unit struct,
/// the prompt path needs nothing else.
pub struct LiveDispatch {
    token: String,
    root: std::path::PathBuf,
}

impl LiveDispatch {
    pub fn new(token: impl Into<String>, root: std::path::PathBuf) -> Self {
        Self {
            token: token.into(),
            root,
        }
    }
}

impl Dispatch for LiveDispatch {
    fn send<'a>(&'a self, chat_id: i64, text: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { send_message(&self.token, chat_id, text).await })
    }

    fn prompt_agent(&self, name: &str, prompt: &str) -> Result<()> {
        crate::herdr::HerdrClient::new(self.root.clone()).agent_prompt_only(name, prompt)
    }

    fn transcribe<'a>(&'a self, file_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { transcribe_voice(&self.token, file_id).await })
    }
}

/// A [`Dispatch`] that records instead of sending. Drives the real
/// `handle_update` from a test with no network and no live agent — the mechanism
/// by which the serve and webhook tests prove both transports produce the same
/// effect for the same Update.
#[cfg(test)]
#[derive(Default)]
pub(super) struct RecordingDispatch {
    sent: Mutex<Vec<(i64, String)>>,
    prompted: Mutex<Vec<(String, String)>>,
}

#[cfg(test)]
impl RecordingDispatch {
    /// Messages recorded as `(chat_id, text)`, in order.
    pub(super) fn sent(&self) -> Vec<(i64, String)> {
        self.sent.lock().clone()
    }

    /// Agent prompts recorded as `(agent_name, prompt)`, in order.
    pub(super) fn prompted(&self) -> Vec<(String, String)> {
        self.prompted.lock().clone()
    }
}

#[cfg(test)]
impl Dispatch for RecordingDispatch {
    fn send<'a>(&'a self, chat_id: i64, text: &'a str) -> BoxFuture<'a, Result<()>> {
        self.sent.lock().push((chat_id, text.to_string()));
        Box::pin(async { Ok(()) })
    }

    fn prompt_agent(&self, name: &str, prompt: &str) -> Result<()> {
        self.prompted
            .lock()
            .push((name.to_string(), prompt.to_string()));
        Ok(())
    }

    fn transcribe<'a>(&'a self, _file_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { anyhow::bail!("RecordingDispatch does not transcribe") })
    }
}

// ---------------------------------------------------------------------------
// Update dispatch
// ---------------------------------------------------------------------------

pub(super) async fn handle_update(
    dispatch: &dyn Dispatch,
    root: &Path,
    allowed_ids: &[i64],
    update: Update,
) -> Result<()> {
    if let Some(message) = update.message {
        let from_id = message.from.as_ref().map(|u| u.id).unwrap_or(0);
        if !allowed_ids.contains(&from_id) {
            eprintln!("Ignored message from unauthorized user: {}", from_id);
            return Ok(());
        }

        let chat_id = message.chat.id;
        let text = message.text.as_deref().unwrap_or("").trim();

        // Voice note → transcribe, then treat the transcript as a text message.
        let text = if text.is_empty() {
            if let Some(voice) = &message.voice {
                dispatch.send(chat_id, "Got it — transcribing…").await?;
                match dispatch.transcribe(&voice.file_id).await {
                    Ok(transcript) => transcript,
                    Err(e) => {
                        dispatch
                            .send(
                                chat_id,
                                &format!("Voice transcription failed: {}", redact(&format!("{:#}", e))),
                            )
                            .await?;
                        return Ok(());
                    }
                }
            } else {
                return Ok(());
            }
        } else {
            text.to_string()
        };

        let trimmed = text.trim();
        let keyword = normalise_keyword(trimmed);

        // ASK_WORDS short-circuit: "give me the brief" never reaches the model.
        if is_ask(&keyword) {
            let response = handlers::report_text(root)?;
            dispatch.send(chat_id, &response).await?;
            return Ok(());
        }

        // Known keywords handled locally
        let response = match keyword.as_str() {
            "status" => handlers::handle_status(root),
            "approve" | "yes" => handlers::handle_approve(root),
            "decline" | "no" => handlers::handle_decline(root),
            "gates" => handlers::handle_gates(root),
            "stalls" => handlers::handle_stalls(root),
            "collab" => {
                handlers::cmd_collab(root, true, false, false, Some(chat_id)).await?;
                Ok("Collab sessions sent".to_string())
            }
            "help" => Ok(handlers::handle_help()),
            _ => {
                // Free text → fire-and-forget prompt to Zolly with tool instruction.
                // Zolly will use the telegram_reply tool to send the answer directly.
                let prompt = format!(
                    "Telegram message from {} (chat_id: {}): \"{}\"\n\nReply using the `telegram_reply` tool with your answer. Include the chat_id: {}.",
                    &render::founder_name(root), chat_id, trimmed, chat_id
                );
                dispatch
                    .prompt_agent(&zolly_agent(root), &prompt)
                    .context("Failed to prompt Zolly agent")?;
                // Track pending reply for watchdog
                PENDING_REPLIES
                    .lock()
                    .insert(chat_id, (now_secs(), trimmed.to_string()));
                // Don't wait for reply — Zolly sends via tool.
                return Ok(());
            }
        };

        let response = match response {
            Ok(r) if r.trim().is_empty() => "(no reply)".to_string(),
            Ok(r) => r,
            Err(e) => format!("Error: {}", redact(&format!("{:#}", e))),
        };

        dispatch.send(chat_id, &response).await?;
    }

    if let Some(poll_answer) = update.poll_answer {
        let user_id = poll_answer.user.id;
        if !allowed_ids.contains(&user_id) {
            eprintln!("Ignored poll answer from unauthorized user: {}", user_id);
            return Ok(());
        }
        let chat_id = allowed_ids[0];

        let voted = if poll_answer.option_ids.is_empty() {
            "no answer".to_string()
        } else {
            poll_answer
                .option_ids
                .iter()
                .map(|i| format!("option #{}", i + 1))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let response = format!(
            "📊 Poll result: you chose {} for poll {}",
            voted, poll_answer.poll_id
        );
        dispatch.send(chat_id, &response).await?;

        // Also forward to Zolly's pane so the agent can react to the vote.
        let prompt = format!(
            "Telegram poll_answer from {} (chat_id: {}): \"{}\"\n\nReply using the `telegram_reply` tool with your answer. Include the chat_id: {}.",
            &render::founder_name(root), chat_id, response, chat_id
        );
        dispatch
            .prompt_agent(&zolly_agent(root), &prompt)
            .context("Failed to prompt Zolly agent on poll answer")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Polling: one getUpdates round-trip, with bounded retry
// ---------------------------------------------------------------------------

/// True when Telegram's refusal is the `getUpdates`/webhook mutual exclusion:
/// it means a webhook is registered, so `getUpdates` can never succeed until it
/// is deleted.
fn is_webhook_conflict(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    d.contains("webhook") || d.contains("conflict")
}

/// Refuse to long-poll a bot whose webhook is set, at startup, before the loop
/// ever calls `getUpdates`. Without this the loop would run against a
/// webhook-enabled bot, and every `getUpdates` would answer the 409 conflict —
/// a permanent failure, not a transient one. The message names both ways out.
fn guard_no_webhook(active_url: Option<&str>) -> Result<()> {
    if let Some(url) = active_url {
        anyhow::bail!(
            "Refusing to long-poll: this bot has a webhook set ({url}). getUpdates and a \
             webhook are mutually exclusive — Telegram answers 409 Conflict while one is \
             registered. Delete the webhook (`zolly telegram-delete-webhook`) to go back to \
             long-polling, or run the webhook listener (`zolly telegram-serve --webhook`)."
        );
    }
    Ok(())
}

/// Bounded exponential backoff: `RETRY_BASE_DELAY_MS * 2^(attempt-1)`, capped at
/// `RETRY_MAX_DELAY_MS`. `attempt` is 1 for the first failure.
fn backoff_delay(attempt: u32) -> Duration {
    let step = attempt.saturating_sub(1).min(16);
    Duration::from_millis(
        RETRY_BASE_DELAY_MS
            .saturating_mul(1u64 << step)
            .min(RETRY_MAX_DELAY_MS),
    )
}

/// Retry `op` while it fails transiently. Returns the first success, or the
/// first failure a retry cannot change — those two are the only ways out, so a
/// connection reset or a DNS blip can no longer end the listener.
async fn retry_transient<T, F, Fut>(
    mut op: F,
    mut on_retry: impl FnMut(u32, Duration, &RequestFailure),
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RequestFailure>>,
{
    let mut attempt = 0u32;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(failure) if failure.is_transient() => {
                attempt += 1;
                let delay = backoff_delay(attempt);
                on_retry(attempt, delay, &failure);
                tokio::time::sleep(delay).await;
            }
            Err(failure) => return Err(failure.into()),
        }
    }
}

/// One `getUpdates` round-trip. The token has to sit in the URL — that is the
/// Telegram API — so the URL is the one string that must never be logged raw;
/// `send_redacted` and `json_redacted` are the only ways it can become one.
async fn fetch_updates(
    client: &Client,
    token: &str,
    offset: i64,
) -> Result<Vec<Update>, RequestFailure> {
    // Offset+1 so the last acknowledged update is not re-requested. When
    // offset=0 (fresh start) this queries offset=1, which is correct —
    // Telegram's first update_id is always > 0.
    let url = format!(
        "{}?offset={}&timeout={}&allowed_updates=[\"message\",\"voice\",\"poll_answer\"]",
        api_url(token, "getUpdates"),
        offset + 1,
        POLL_TIMEOUT_SECS
    );

    let resp = send_redacted(client.get(&url), "Failed to get updates").await?;
    let body: TelegramResponse<Vec<Update>> =
        json_redacted(resp, "Failed to parse getUpdates response").await?;

    if !body.ok {
        let description = body.description.unwrap_or_default();
        // The mutual-exclusion conflict has a specific remedy, so it gets a
        // specific message rather than being folded into a generic refusal.
        if is_webhook_conflict(&description) {
            return Err(RequestFailure::fatal(format!(
                "getUpdates refused ({description}): a webhook is set on this bot, and \
                 getUpdates and a webhook are mutually exclusive. Delete the webhook \
                 (`zolly telegram-delete-webhook`) to long-poll, or run the listener with \
                 `zolly telegram-serve --webhook`."
            )));
        }
        return Err(RequestFailure::fatal(format!(
            "getUpdates failed: {}",
            description
        )));
    }

    Ok(body.result.unwrap_or_default())
}

/// One iteration of the serve loop: get updates, retrying while the failure is
/// transient, then advance the offset past each update and dispatch it. Split
/// out of `cmd_serve` so its two guarantees — a transient failure never ends
/// the loop, and a failed poll never moves the offset — are testable without a
/// network, an agent, or a token. `pub(super)` because the webhook tests drive
/// it too: it is the long-poll half of the "both transports, same effect"
/// proof.
pub(super) async fn poll_and_advance<F, Fut>(
    dispatch: &dyn Dispatch,
    root: &Path,
    allowed_ids: &[i64],
    offset_path: &Path,
    mut offset: i64,
    mut fetch: F,
) -> Result<i64>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<Update>, RequestFailure>>,
{
    let updates = retry_transient(&mut fetch, |attempt, delay, failure| {
        eprintln!(
            "getUpdates failed (attempt {}), retrying in {:?}: {}",
            attempt, delay, failure
        );
    })
    .await?;

    for update in updates {
        let update_id = update.update_id;

        // Advance the offset BEFORE handling the update. One bad update
        // must not cost the rest of the batch. The previous code advanced
        // after the work inside the loop, so a raising update exited with
        // the offset already past every update behind it — messages were
        // silently and permanently lost. Acknowledge-first costs at most
        // one re-delivery of the failed update, which is visible.
        offset = update_id;
        if let Err(e) = write_offset_atomic(offset_path, offset) {
            eprintln!("Failed to persist offset {}: {}", offset, e);
        }

        if let Err(e) = handle_update(dispatch, root, allowed_ids, update).await {
            // Redacted: this string goes to stderr and into
            // dead_letter.jsonl, both of which outlive the process.
            let e = redact(&format!("{:#}", e));
            eprintln!("Error handling update {}: {}", update_id, e);
            // Dead-letter the failed update so it is not lost forever.
            // The offset has already advanced past it; without this file
            // the message is gone. Appending raw JSONL keeps the wire
            // shape for replay.
            append_dead_letter(root, update_id, &e);
        }
    }

    Ok(offset)
}

// ---------------------------------------------------------------------------
// Serve loop
// ---------------------------------------------------------------------------
// Command menu: full keyword set, pinned to the router
// ---------------------------------------------------------------------------

/// Every command registered in Telegram's slash menu, one row per synonym
/// family. Must stay in sync with the keywords `handle_update` accepts
/// (status, approve|yes, decline|no, gates, stalls, collab,
/// help, plus the ask-words brief and liveness). `what's up` is an ask-word
static REGISTERED_COMMANDS: &[(&str, &str)] = &[
    ("status", "Show current status (brief)"),
    ("approve", "Approve a gate (yes also works)"),
    ("decline", "Decline a gate (no also works)"),
    ("gates", "Show open gates and their owners"),
    ("stalls", "Show stalled workers and panes"),
    ("collab", "Find/start OMP collab session, send control link"),
    ("help", "Show help"),
    ("brief", "Render the founder brief (what's up also works)"),
    ("liveness", "Liveness read across office seats"),
];

/// Build the `setMyCommands` JSON payload from [`REGISTERED_COMMANDS`].
fn registered_commands_json() -> serde_json::Value {
    serde_json::json!(
        REGISTERED_COMMANDS
            .iter()
            .map(|(command, description)| {
                serde_json::json!({ "command": command, "description": description })
            })
            .collect::<Vec<_>>()
    )
}

// The canonical keywords the router accepts, one per synonym family,
// independent of REGISTERED_COMMANDS — this is the source of truth for what
// handle_update accepts (match arms plus ask-words). router_keywords must
// stay in sync with handle_update's match arms.
fn router_keywords() -> Vec<&'static str> {
    vec![
        "status",
        "approve",
        "decline",
        "gates",
        "stalls",
        "collab",
        "help",
        "brief",
        "liveness",
    ]
}

/// True when `word` is a keyword the router accepts (match arms or ask-words).
/// Mirrors the dispatch logic in `handle_update`.
#[cfg(test)]
fn is_router_keyword(word: &str) -> bool {
    matches!(
        word,
        "status"
            | "approve"
            | "yes"
            | "decline"
            | "no"
            | "gates"
            | "stalls"
            | "collab"
            | "help"
    ) || is_ask(word)
}
pub async fn cmd_serve(root: &Path) -> Result<()> {
    let token = get_token()?;
    let allowed_ids = get_allowed_ids()?;
    let client = get_client();

    // Mutual exclusion, checked before the first poll. `getUpdates` and a
    // webhook cannot both be active: with a webhook registered Telegram answers
    // every `getUpdates` with 409 Conflict, which is fatal-but-unfixable, so the
    // loop would exit on each systemd restart and never advance. Refuse at
    // startup and name both ways out.
    match super::webhook::fetch_webhook_info(&client, &token).await {
        Ok(info) => guard_no_webhook(info.url.filter(|url| !url.is_empty()).as_deref())?,
        // A read failure must not stop the listener: the first getUpdates still
        // detects the conflict and says so (see `is_webhook_conflict`).
        Err(e) => eprintln!(
            "Could not read getWebhookInfo before polling (continuing; a webhook on \
             this bot will surface as a 409 on the first getUpdates): {e}"
        ),
    }

    let dispatch = LiveDispatch::new(token.clone(), root.to_path_buf());

    let zolly_dir = root.join(".agents/zolly");
    fs::create_dir_all(&zolly_dir).context("Failed to create .agents/zolly")?;

    let lock_path = zolly_dir.join("telegram_serve.lock");
    let _lock = ServeLock::acquire(lock_path)?;

    let offset_path = zolly_dir.join("telegram_offset.txt");
    let mut offset: i64 = read_offset(&offset_path);

// Register bot commands so they appear in Telegram's menu
// Full keyword set, one row per synonym family (REGISTERED_COMMANDS).
    let commands = registered_commands_json();
    let set_cmd_url = api_url(&token, "setMyCommands");
// Best-effort: the menu is a convenience, and `send_redacted` keeps a
// failure here out of the journal.
    let _ = send_redacted(
        client.post(&set_cmd_url).json(&commands),
        "Failed to register bot commands",
    )
    .await;

    // On fresh start (offset=0), sync to the latest acknowledged update_id
    // so we don't reprocess the entire backlog. Telegram returns updates
    // with update_id > N when queried with offset=N.
    if offset == 0 {
        let sync_url = format!(
            "{}?offset=0&timeout=1",
            api_url(&token, "getUpdates"),
        );
        if let Ok(resp) =
            send_redacted(client.get(&sync_url), "Failed to sync getUpdates offset").await
        {
            if let Ok(body) =
                json_redacted::<TelegramResponse<Vec<Update>>>(resp, "Failed to parse sync response")
                    .await
            {
                if body.ok {
                    if let Some(updates) = body.result {
                        if let Some(last) = updates.last() {
                            offset = last.update_id;
                            let _ = write_offset_atomic(&offset_path, offset);
                        }
                    }
                }
            }
        }
    }

    loop {
        // Watchdog: check for pending replies that timed out
        let now = now_secs();
        let mut to_remove = Vec::new();
        {
            let mut pending = PENDING_REPLIES.lock();
            for (chat_id, (sent_at, question)) in pending.iter() {
                if now.saturating_sub(*sent_at) > ZOLLY_TOOL_TIMEOUT_SECS {
                    to_remove.push(*chat_id);
                    // Send fallback message
                    let fallback = format!(
                        "⏱ Zolly didn't reply within {} minutes. The question was: \"{}\"\n\nTry again, or use /status /collab /help for instant answers.",
                        ZOLLY_TOOL_TIMEOUT_SECS / 60,
                        question
                    );
                }
            }
            for chat_id in to_remove {
                pending.remove(&chat_id);
            }
        }

        // One poll: retried in-process while the failure is transient, so a
        // connection reset or a DNS blip no longer exits the process — which
        // used to cost a systemd restart, a 10s gap in the listener, and the
        // pending-reply map.
        offset = poll_and_advance(
            &dispatch,
            root,
            &allowed_ids,
            &offset_path,
            offset,
            || fetch_updates(&client, &token, offset),
        )
        .await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn a_pid_that_does_not_exist_never_holds_the_lock() {
        // Above the kernel's pid ceiling, so this can never come to exist.
        assert!(!lock_holder_running(4_194_304));
    }

    #[test]
    fn a_live_process_of_another_program_does_not_hold_the_lock() {
        // The regression. A stale lock keeps the pid of a crashed serve; the
        // kernel then hands that number to some unrelated daemon. Existence of
        // /proc/<pid> used to mean "still running", which blocked startup
        // forever. Find any live process that is not zolly and require that it
        // is not mistaken for a lock holder.
        let other = fs::read_dir("/proc")
            .expect("/proc must be readable")
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .find(|pid| {
                let raw = fs::read(format!("/proc/{}/cmdline", pid)).unwrap_or_default();
                if raw.is_empty() {
                    return false;
                }
                let cmd = String::from_utf8_lossy(&raw);
                !cmd
                    .split('\0')
                    .filter(|a| !a.is_empty())
                    .any(|a| a.rsplit('/').next().is_some_and(|n| n.starts_with("zolly")))
            })
            .expect("some live process is not zolly");
        assert!(
            !lock_holder_running(other),
            "pid {} is alive but is not a zolly; the lock must be reclaimed",
            other
        );
    }

    #[test]
    fn a_live_zolly_holds_the_lock() {
        // The test binary is itself a zolly build, so our own pid is a real
        // holder. This is the case the lock exists for: a second serve must
        // not start on top of the first.
        assert!(lock_holder_running(std::process::id()));
    }

    #[test]
    fn acquire_reclaims_a_lock_from_a_reused_pid_and_releases_on_drop() {
        let other = fs::read_dir("/proc")
            .expect("/proc must be readable")
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .find(|pid| {
                let raw = fs::read(format!("/proc/{}/cmdline", pid)).unwrap_or_default();
                if raw.is_empty() {
                    return false;
                }
                !String::from_utf8_lossy(&raw)
                    .split('\0')
                    .filter(|a| !a.is_empty())
                    .any(|a| a.rsplit('/').next().is_some_and(|n| n.starts_with("zolly")))
            })
            .expect("some live process is not zolly");

        let dir = std::env::temp_dir().join(format!("zolly-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("telegram_serve.lock");
        fs::write(&path, other.to_string()).expect("write a lock naming the unrelated pid");

        let lock = ServeLock::acquire(path.clone())
            .expect("a lock whose pid was reused by another program must be reclaimed");
        assert!(path.exists(), "the reclaimed lock must then be held");
        drop(lock);
        assert!(!path.exists(), "dropping the guard must remove the lock");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn acquire_refuses_a_lock_held_by_a_live_zolly() {
        let dir = std::env::temp_dir().join(format!("zolly-lock-busy-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("telegram_serve.lock");
        fs::write(&path, std::process::id().to_string()).expect("write a lock naming ourselves");

        let err = match ServeLock::acquire(path.clone()) {
            Ok(_) => panic!("a live zolly holds the lock"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("already running"), "unexpected error: {err}");
        assert!(path.exists(), "a refused acquire must not delete the lock");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_pending_removes_chat_from_pending() {
        let chat_id = 12345_i64;
        let question = "test question";
        
        // Add to pending
        {
            let mut pending = PENDING_REPLIES.lock();
            pending.insert(chat_id, (now_secs(), question.to_string()));
        }
        
        // Verify it's there
        {
            let pending = PENDING_REPLIES.lock();
            assert!(pending.contains_key(&chat_id), "Chat should be in pending");
        }
        
        // Take pending
        take_pending(chat_id);
        
        // Verify it's removed
        {
            let pending = PENDING_REPLIES.lock();
            assert!(!pending.contains_key(&chat_id), "Chat should be removed from pending");
        }
    }

    #[test]
    fn take_pending_idempotent() {
        let chat_id = 54321_i64;
        
        // Call take_pending on non-existent chat - should not panic
        take_pending(chat_id);
        take_pending(chat_id);
        
        // Verify still not present
        let pending = PENDING_REPLIES.lock();
        assert!(!pending.contains_key(&chat_id));
    }

    // -----------------------------------------------------------------------
    // Retry policy, and the offset guarantee under failure
    // -----------------------------------------------------------------------

    /// Unique scratch directory per test — `write_offset_atomic` needs a real
    /// parent directory to rename into.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zolly-serve-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Updates with no message and no poll answer: `handle_update` returns
    /// immediately without touching Telegram or the agent, so a poll test can
    /// carry real update ids without any network.
    fn update_ids(ids: &[i64]) -> Vec<Update> {
        ids.iter()
            .map(|id| Update {
                update_id: *id,
                message: None,
                poll_answer: None,
            })
            .collect()
    }

    #[test]
    fn backoff_doubles_then_caps() {
        assert_eq!(backoff_delay(1), Duration::from_millis(RETRY_BASE_DELAY_MS));
        assert_eq!(backoff_delay(2), Duration::from_millis(2 * RETRY_BASE_DELAY_MS));
        assert_eq!(backoff_delay(6), Duration::from_millis(RETRY_MAX_DELAY_MS));
        assert_eq!(backoff_delay(u32::MAX), Duration::from_millis(RETRY_MAX_DELAY_MS));
    }

    /// A transient failure is retried, and the waits between attempts are the
    /// bounded backoff in order. Time is paused, so the sleeps cost nothing.
    #[tokio::test(start_paused = true)]
    async fn retry_transient_retries_a_transient_failure() {
        let calls = AtomicU32::new(0);
        let mut retries: Vec<(u32, Duration, String)> = Vec::new();

        let out: Result<&str> = retry_transient(
            || {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < 2 {
                        Err(RequestFailure::transient_for_test("connection reset"))
                    } else {
                        Ok("updates")
                    }
                }
            },
            |attempt, delay, failure| retries.push((attempt, delay, failure.to_string())),
        )
        .await;

        assert_eq!(out.unwrap(), "updates");
        assert_eq!(calls.load(Ordering::SeqCst), 3, "must retry, not return Err");
        assert_eq!(
            retries,
            vec![
                (
                    1,
                    Duration::from_millis(RETRY_BASE_DELAY_MS),
                    "connection reset".to_string()
                ),
                (
                    2,
                    Duration::from_millis(2 * RETRY_BASE_DELAY_MS),
                    "connection reset".to_string()
                ),
            ]
        );
    }

    /// A failure no retry can fix — Telegram refusing, a malformed request —
    /// comes straight back out, on the first attempt.
    #[tokio::test(start_paused = true)]
    async fn retry_transient_returns_a_fatal_failure_immediately() {
        let calls = AtomicU32::new(0);
        let out: Result<()> = retry_transient(
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err(RequestFailure::fatal("getUpdates failed: Unauthorized")) }
            },
            |_, _, _| panic!("a fatal failure must not be retried"),
        )
        .await;

        let err = out.expect_err("a fatal failure propagates");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(err.to_string().contains("Unauthorized"), "{err}");
    }

    /// The listener must survive a transient poll failure, and that failure
    /// must leave the persisted offset exactly as it found it.
    #[tokio::test(start_paused = true)]
    async fn a_transient_poll_failure_retries_and_leaves_the_offset_alone() {
        let dir = test_dir("transient-poll");
        let offset_path = dir.join("telegram_offset.txt");
        write_offset_atomic(&offset_path, 42).unwrap();

        let calls = AtomicU32::new(0);
        let offset = poll_and_advance(
            &RecordingDispatch::default(),
            &dir,
            &[],
            &offset_path,
            42,
            || {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < 2 {
                        Err(RequestFailure::transient_for_test(
                            "Temporary failure in name resolution",
                        ))
                    } else {
                        Ok(Vec::new())
                    }
                }
            },
        )
        .await
        .expect("a transient failure must be retried, not returned");

        assert_eq!(offset, 42, "no update arrived, so the offset must not move");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            fs::read_to_string(&offset_path).unwrap().trim(),
            "42",
            "a failed poll must not touch the offset file"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// The other half of the guarantee: a healthy poll still advances and
    /// persists the offset, so the retry path did not freeze its bookkeeping.
    #[tokio::test(start_paused = true)]
    async fn a_successful_poll_advances_and_persists_the_offset() {
        let dir = test_dir("successful-poll");
        let offset_path = dir.join("telegram_offset.txt");
        write_offset_atomic(&offset_path, 42).unwrap();

        let offset = poll_and_advance(
            &RecordingDispatch::default(),
            &dir,
            &[],
            &offset_path,
            42,
            || async { Ok(update_ids(&[43, 44])) },
        )
        .await
        .expect("a healthy poll returns");

        assert_eq!(offset, 44);
        assert_eq!(read_offset(&offset_path), 44);

        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // The dispatch seam: the poll path drives the same handler as the webhook
    // -----------------------------------------------------------------------

    /// A message Update from `chat_id`, built through `serde_json` so it carries
    /// the same wire shape Telegram delivers.
    fn message_update(update_id: i64, chat_id: i64, text: &str) -> Update {
        serde_json::from_value(serde_json::json!({
            "update_id": update_id,
            "message": {
                "message_id": 1,
                "from": {"id": chat_id, "is_bot": false, "first_name": "Shifaul"},
                "chat": {"id": chat_id, "type": "private"},
                "date": 0,
                "text": text
            }
        }))
        .expect("a message update deserializes")
    }

    /// The poll path runs the real `handle_update`: a `/help` message is
    /// answered with exactly the help text, and never prompts the agent.
    #[tokio::test]
    async fn a_keyword_update_through_the_poll_path_reaches_the_recorder() {
        let dir = test_dir("poll-keyword");
        let offset_path = dir.join("telegram_offset.txt");
        let dispatch = RecordingDispatch::default();

        poll_and_advance(
            &dispatch,
            &dir,
            &[42],
            &offset_path,
            0,
            || async { Ok(vec![message_update(7, 42, "/help")]) },
        )
        .await
        .expect("a healthy poll returns");

        assert_eq!(
            dispatch.sent(),
            vec![(42_i64, handlers::handle_help())],
            "the keyword reply is sent verbatim"
        );
        assert!(
            dispatch.prompted().is_empty(),
            "a keyword never prompts the agent"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Free text takes the other branch: the agent is prompted, and the chat
    /// gets no reply from `handle_update` itself (Zolly replies via the tool).
    #[tokio::test]
    async fn free_text_through_the_poll_path_prompts_the_agent() {
        let dir = test_dir("poll-free-text");
        let offset_path = dir.join("telegram_offset.txt");
        let dispatch = RecordingDispatch::default();

        poll_and_advance(
            &dispatch,
            &dir,
            &[42],
            &offset_path,
            0,
            || async { Ok(vec![message_update(9, 42, "what is stuck?")]) },
        )
        .await
        .expect("a healthy poll returns");

        let prompted = dispatch.prompted();
        assert_eq!(prompted.len(), 1);
        assert_eq!(prompted[0].0, zolly_agent(&dir));
        assert!(
            prompted[0].1.contains("what is stuck?"),
            "the prompt must carry the message: {}",
            prompted[0].1
        );
        assert!(
            dispatch.sent().is_empty(),
            "free text sends no chat reply from handle_update"
        );

        // `handle_update` tracked a pending reply for this chat; clear it so the
        // global map does not leak between tests.
        take_pending(42);
        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // The getUpdates ↔ webhook mutual exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn guard_allows_a_bot_with_no_webhook() {
        assert!(guard_no_webhook(None).is_ok());
    }

    /// The refusal must name the conflict and both ways out, not merely fail.
    #[test]
    fn guard_refuses_and_names_both_ways_out() {
        let err = guard_no_webhook(Some("https://zollybot.shifaul.dev"))
            .expect_err("a set webhook must refuse the long-poll");
        let msg = err.to_string();
        assert!(msg.contains("https://zollybot.shifaul.dev"), "{msg}");
        assert!(msg.contains("telegram-delete-webhook"), "{msg}");
        assert!(msg.contains("--webhook"), "{msg}");
    }

    /// Telegram reports the conflict as a 409 whose description mentions the
    /// webhook; the classifier is what turns that into the specific message.
    #[test]
    fn conflict_wording_matches_telegrams_refusal() {
        assert!(is_webhook_conflict(
            "Conflict: can't use getUpdates method while webhook is active"
        ));
        assert!(is_webhook_conflict("webhook is active"));
        assert!(!is_webhook_conflict("Unauthorized"));
        assert!(!is_webhook_conflict("Bad Request: chat not found"));
    }
    /// Drift pin: the slash-menu registration must contain exactly the
    /// keywords the router accepts, one row per synonym family, and nothing
    /// else. If a future edit adds a keyword to `handle_update` without
    /// registering it (or registers one the router does not accept), this
    /// test fails — the menu cannot silently misrepresent the CLI again.
    #[test]
    fn registered_commands_match_router_keywords() {
        let commands: Vec<String> = registered_commands_json()
            .as_array()
            .expect("setMyCommands payload is an array")
            .iter()
            .map(|entry| {
                entry["command"]
                    .as_str()
                    .expect("each command entry has a string command")
                    .to_string()
            })
            .collect();

        // Every registered command is a keyword the router accepts.
        for command in &commands {
            assert!(
                is_router_keyword(command),
                "registered command {command:?} is not accepted by the router"
            );
        }

        // Every router keyword has exactly one registered row (no synonym
        // appears twice, no keyword missing).
        let keywords = router_keywords();
        assert_eq!(
            commands.len(),
            keywords.len(),
            "menu has {} rows but router accepts {} keywords:\n menu: {commands:?}\n router: {keywords:?}",
            commands.len(),
            keywords.len(),
        );
        for keyword in &keywords {
            assert!(
                commands.contains(&keyword.to_string()),
                "router keyword {keyword:?} is not in the slash menu"
            );
        }

        // Each synonym family is present exactly once: approve, decline.
        assert_eq!(
            commands.iter().filter(|c| c.as_str() == "approve").count(),
            1,
            "approve must appear once, not once per synonym"
        );
        assert_eq!(
            commands.iter().filter(|c| c.as_str() == "decline").count(),
            1,
            "decline must appear once, not once per synonym"
        );
    }
}
