//! The webhook transport: a local HTTP listener that receives Telegram
//! `Update` POSTs, verifies the secret-token header, and hands the update to
//! the same [`super::serve::handle_update`] the getUpdates loop uses. Plus the
//! three management verbs — `setWebhook`, `getWebhookInfo`, `deleteWebhook`.
//!
//! `getUpdates` and a webhook are mutually exclusive on one bot; the long-poll
//! path refuses to start while a webhook is registered
//! ([`super::serve`]'s guard), and this listener takes the same serve lock, so
//! two transports cannot run at once on one box.
//!
//! Binding `127.0.0.1:80` needs `CAP_NET_BIND_SERVICE` — under systemd that is
//! `AmbientCapabilities=CAP_NET_BIND_SERVICE` plus `CapabilityBoundingSet=` on
//! the unit (see [`BIND_HINT`]). `--bind` overrides the address for a high port.

use anyhow::{Context, Result};
use reqwest::Client;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::config::{api_url, get_allowed_ids, get_client, get_token, json_redacted, redact, send_redacted};
use super::serve::{append_dead_letter, handle_update, Dispatch, LiveDispatch, ServeLock};
use super::types::{TelegramResponse, Update, WebhookInfo};

/// The header Telegram echoes the configured `secret_token` in. Request header
/// names are lower-cased before lookup, so this is the lower-case form.
const SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

/// Largest request body the listener will buffer before refusing. Telegram
/// updates are small; the cap stops a hostile or misconfigured sender from
/// making the listener buffer unbounded bytes.
const MAX_BODY_BYTES: usize = 1 << 20; // 1 MiB

/// Largest header block, same reasoning.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// The one failure this command is expected to hit: `127.0.0.1:80` is a
/// privileged port. Named deterministically so the error says how to fix it.
const BIND_HINT: &str = "port 80 needs CAP_NET_BIND_SERVICE (under systemd: \
    `AmbientCapabilities=CAP_NET_BIND_SERVICE` plus `CapabilityBoundingSet=` on the unit), \
    or bind a high port with `--bind 127.0.0.1:8080`";

// ---------------------------------------------------------------------------
// Secret handling
// ---------------------------------------------------------------------------

/// Telegram's `secret_token` rules: 1–256 characters from `A-Za-z0-9_-`.
/// Checked locally so a bad secret fails before `setWebhook` reaches the API.
pub fn validate_secret(secret: &str) -> Result<()> {
    if secret.is_empty() || secret.len() > 256 {
        anyhow::bail!(
            "webhook secret must be 1–256 characters, got {}",
            secret.len()
        );
    }
    if !secret
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        anyhow::bail!("webhook secret may only contain A–Z, a–z, 0–9, '_' and '-'");
    }
    Ok(())
}

/// Compare the request's secret header to the configured secret. A missing
/// header never matches. Plain `==`: this is not a secret guessable over the
/// network byte-by-byte faster than rate limits allow.
fn secret_matches(expected: &str, got: Option<&str>) -> bool {
    matches!(got, Some(s) if s == expected)
}

// ---------------------------------------------------------------------------
// HTTP/1.1 request parsing — the minimum a Telegram delivery needs
// ---------------------------------------------------------------------------

/// One parsed HTTP request: method, lower-cased headers, body.
#[derive(Debug)]
struct HttpRequest {
    method: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    /// A header by lower-case name.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// Index of the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Read one HTTP/1.1 request off `stream`: headers up to `\r\n\r\n`, then
/// `Content-Length` bytes of body. Bounded in both directions.
async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];

    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > MAX_HEADER_BYTES {
            anyhow::bail!("header block exceeds {MAX_HEADER_BYTES} bytes");
        }
        let n = stream.read(&mut chunk).await.context("read request headers")?;
        if n == 0 {
            anyhow::bail!("connection closed before the headers completed");
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split_whitespace();
    let method = request_line.next().unwrap_or_default().to_string();
    let _target = request_line.next(); // the webhook URL's path; any path is accepted

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        anyhow::bail!("body exceeds {MAX_BODY_BYTES} bytes");
    }

    let mut body = buf[header_end.min(buf.len())..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await.context("read request body")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > MAX_BODY_BYTES {
            anyhow::bail!("body exceeds {MAX_BODY_BYTES} bytes");
        }
    }
    body.truncate(content_length.min(body.len()));

    Ok(HttpRequest {
        method,
        headers,
        body,
    })
}

/// Write a minimal HTTP/1.1 response and close. `Connection: close` — the
/// socket is one-shot; Telegram opens a new connection per delivery.
async fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .context("write response")?;
    stream.flush().await.context("flush response")
}

// ---------------------------------------------------------------------------
// The listener
// ---------------------------------------------------------------------------

/// Serve one accepted connection. Every failure mode becomes a status code,
/// never a panic. Returns the status written so a test can assert it.
///
/// Order is deliberate: method, then authentication, then parsing, then the
/// handler. A request that is not an authenticated POST never parses as an
/// `Update` and never reaches [`handle_update`].
pub(super) async fn handle_connection(
    mut stream: TcpStream,
    dispatch: &dyn Dispatch,
    root: &Path,
    allowed_ids: &[i64],
    secret: &str,
) -> Result<u16> {
    let request = match read_request(&mut stream).await {
        Ok(request) => request,
        Err(e) => {
            let _ = respond(&mut stream, 400, "Bad Request", r#"{"ok":false}"#).await;
            eprintln!("webhook: malformed request: {e}");
            return Ok(400);
        }
    };

    if request.method != "POST" {
        respond(&mut stream, 405, "Method Not Allowed", r#"{"ok":false}"#).await?;
        return Ok(405);
    }

    if !secret_matches(secret, request.header(SECRET_HEADER)) {
        // The header value is never echoed back.
        respond(&mut stream, 401, "Unauthorized", r#"{"ok":false}"#).await?;
        return Ok(401);
    }

    let update: Update = match serde_json::from_slice(&request.body) {
        Ok(update) => update,
        Err(e) => {
            eprintln!("webhook: body is not a Telegram Update: {e}");
            respond(&mut stream, 400, "Bad Request", r#"{"ok":false}"#).await?;
            return Ok(400);
        }
    };

    let update_id = update.update_id;
    // The same handler the long-poll loop calls — the whole point of this module.
    if let Err(e) = handle_update(dispatch, root, allowed_ids, update).await {
        // Acknowledge anyway: Telegram redelivers a non-2xx delivery, so a
        // handler that always fails would be retried forever. Dead-letter
        // instead, exactly as the poll loop does, so the update is replayable.
        let e = redact(&format!("{:#}", e));
        eprintln!("webhook: error handling update {update_id}: {e}");
        append_dead_letter(root, update_id, &e);
    }

    respond(&mut stream, 200, "OK", r#"{"ok":true}"#).await?;
    Ok(200)
}

/// The accept loop. Each connection is handled on its own task so a slow
/// handler never blocks the next delivery. Never returns except on panic.
pub async fn run_listener(
    listener: TcpListener,
    dispatch: Arc<dyn Dispatch>,
    root: PathBuf,
    allowed_ids: Arc<Vec<i64>>,
    secret: Arc<String>,
) -> Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                // A persistent accept error must not spin the CPU; back off.
                eprintln!("webhook: accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };

        let dispatch = Arc::clone(&dispatch);
        let root = root.clone();
        let allowed_ids = Arc::clone(&allowed_ids);
        let secret = Arc::clone(&secret);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &*dispatch, &root, &allowed_ids, &secret).await
            {
                eprintln!("webhook: connection from {peer} failed: {e}");
            }
        });
    }
}

/// `zolly telegram-serve --webhook` — run the webhook listener.
pub async fn cmd_webhook(root: &Path, bind: &str, secret: Option<String>) -> Result<()> {
    let token = get_token()?;
    let allowed_ids = get_allowed_ids()?;
    let client = get_client();

    let secret = secret.context(
        "no webhook secret: pass --secret <s>, or set ZOLLY_TELEGRAM_WEBHOOK_SECRET",
    )?;
    validate_secret(&secret)?;

    // Informational and best-effort: report what Telegram currently believes,
    // so a half-finished cutover is visible at startup rather than as silence.
    match fetch_webhook_info(&client, &token).await {
        Ok(info) => {
            let url = info.url.clone().unwrap_or_default();
            if url.is_empty() {
                eprintln!(
                    "webhook: no webhook is registered with Telegram — the cutover \
                     (`zolly telegram-set-webhook --url <https-url> --secret <s>`) has not run"
                );
            } else {
                eprintln!(
                    "webhook: Telegram has {url} registered ({} pending update(s))",
                    info.pending_update_count
                );
            }
            if let Some(err) = &info.last_error_message {
                eprintln!("webhook: Telegram's last delivery error: {err}");
            }
        }
        Err(e) => eprintln!("webhook: could not read getWebhookInfo (continuing): {e}"),
    }

    // The same lock the long-poll listener takes: two listeners on one bot
    // steal each other's updates.
    let zolly_dir = root.join(".agents/zolly");
    std::fs::create_dir_all(&zolly_dir).context("Failed to create .agents/zolly")?;
    let _lock = ServeLock::acquire(zolly_dir.join("telegram_serve.lock"))?;

    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("Failed to bind {bind}. {BIND_HINT}"))?;
    let addr = listener
        .local_addr()
        .context("Failed to read the listener address")?;
    println!("zolly webhook listening on http://{addr} (POST, secret header {SECRET_HEADER})");

    let dispatch: Arc<dyn Dispatch> = Arc::new(LiveDispatch::new(token, root.to_path_buf()));
    run_listener(
        listener,
        dispatch,
        root.to_path_buf(),
        Arc::new(allowed_ids),
        Arc::new(secret),
    )
    .await
}

// ---------------------------------------------------------------------------
// Management verbs
// ---------------------------------------------------------------------------

/// `getWebhookInfo` — read-only. Also used by the long-poll startup guard.
pub async fn fetch_webhook_info(client: &Client, token: &str) -> Result<WebhookInfo> {
    let resp = send_redacted(
        client.get(api_url(token, "getWebhookInfo")),
        "Failed to get webhook info",
    )
    .await?;
    let parsed: TelegramResponse<WebhookInfo> =
        json_redacted(resp, "Failed to parse getWebhookInfo response").await?;
    if !parsed.ok {
        anyhow::bail!(
            "getWebhookInfo failed: {}",
            parsed.description.unwrap_or_default()
        );
    }
    parsed.result.context("getWebhookInfo returned no result")
}

/// `zolly telegram-set-webhook --url <https-url> [--secret <s>]` — register the
/// webhook. This is the production cutover on an external system: the command is
/// built here and run by the founder, never in a build or a verify step.
pub async fn cmd_set_webhook(url: &str, secret: Option<String>) -> Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!("webhook url must be https (Telegram requires it): {url}");
    }
    let token = get_token()?;
    let client = get_client();

    // A shell run has no systemd EnvironmentFile. Load the office credentials
    // when the secret did not arrive by flag or environment, so the command is
    // just `zolly telegram-set-webhook --url <url>`.
    if secret.is_none() && std::env::var("ZOLLY_TELEGRAM_WEBHOOK_SECRET").is_err() {
        super::config::load_credentials();
    }
    let secret = secret.or_else(|| {
        std::env::var("ZOLLY_TELEGRAM_WEBHOOK_SECRET")
            .ok()
            .filter(|value| !value.is_empty())
    });
    if let Some(s) = &secret {
        validate_secret(s)?;
    }

    // Mirror the long-poll loop's `allowed_updates` so the two transports
    // observe exactly the same update kinds.
    let mut body = serde_json::json!({
        "url": url,
        "allowed_updates": ["message", "voice", "poll_answer"],
    });
    if let Some(s) = secret {
        body["secret_token"] = serde_json::Value::String(s);
    }

    let resp = send_redacted(
        client.post(api_url(&token, "setWebhook")).json(&body),
        "Failed to set webhook",
    )
    .await?;
    let parsed: TelegramResponse<bool> =
        json_redacted(resp, "Failed to parse setWebhook response").await?;
    if !parsed.ok {
        anyhow::bail!(
            "setWebhook failed: {}",
            parsed.description.unwrap_or_default()
        );
    }
    println!("webhook set to {url}");
    Ok(())
}

/// `zolly telegram-webhook-info` — print the current webhook state. Read-only:
/// changes nothing, safe to run against the live bot.
pub async fn cmd_webhook_info() -> Result<()> {
    let token = get_token()?;
    let client = get_client();
    let info = fetch_webhook_info(&client, &token).await?;

    let url = info.url.clone().unwrap_or_default();
    if url.is_empty() {
        println!("no webhook set — getUpdates (long-polling) is available");
    } else {
        println!("url: {url}");
    }
    println!("pending_update_count: {}", info.pending_update_count);
    if let Some(allowed) = info.allowed_updates {
        if allowed.is_empty() {
            println!("allowed_updates: (all)");
        } else {
            println!("allowed_updates: {}", allowed.join(", "));
        }
    }
    if let Some(msg) = info.last_error_message {
        println!("last_error_message: {msg}");
    }
    if let Some(date) = info.last_error_date {
        println!("last_error_date: {date}");
    }
    Ok(())
}

/// `zolly telegram-delete-webhook` — remove the webhook so `getUpdates` works
/// again. Also an external-system change; run by the founder.
pub async fn cmd_delete_webhook() -> Result<()> {
    let token = get_token()?;
    let client = get_client();
    let resp = send_redacted(
        client.post(api_url(&token, "deleteWebhook")),
        "Failed to delete webhook",
    )
    .await?;
    let parsed: TelegramResponse<bool> =
        json_redacted(resp, "Failed to parse deleteWebhook response").await?;
    if !parsed.ok {
        anyhow::bail!(
            "deleteWebhook failed: {}",
            parsed.description.unwrap_or_default()
        );
    }
    println!("webhook deleted — getUpdates (long-polling) is available again");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tg::serve::{poll_and_advance, take_pending, RecordingDispatch};

    /// A message Update body in the exact wire shape Telegram delivers. `text`
    /// is normalised to a keyword by `handle_update`, so `/help` routes to the
    /// keyword branch and free words route to the agent prompt.
    fn message_body(update_id: i64, text: &str) -> String {
        serde_json::json!({
            "update_id": update_id,
            "message": {
                "message_id": 1,
                "from": {"id": 42, "is_bot": false, "first_name": "Shifaul"},
                "chat": {"id": 42, "type": "private"},
                "date": 0,
                "text": text
            }
        })
        .to_string()
    }

    /// Unique scratch directory per test.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zolly-webhook-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Bind a listener on an ephemeral loopback port, spawn `run_listener` on
    /// it, POST `body` with `header` (the secret header value; `None` omits it),
    /// and return the HTTP status. `dispatch` receives whatever the handler does.
    async fn post(
        body: String,
        header: Option<&str>,
        dispatch: Arc<RecordingDispatch>,
        secret: &str,
    ) -> u16 {
        let root = scratch_dir("post");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind an ephemeral loopback port");
        let addr = listener.local_addr().expect("read the bound address");

        let dyn_dispatch: Arc<dyn Dispatch> = dispatch;
        let listener_task = tokio::spawn(run_listener(
            listener,
            dyn_dispatch,
            root.clone(),
            Arc::new(vec![42_i64]),
            Arc::new(secret.to_string()),
        ));

        let mut req = reqwest::Client::new()
            .post(format!("http://{addr}/"))
            .body(body);
        if let Some(value) = header {
            req = req.header(SECRET_HEADER, value);
        }
        let status = req
            .send()
            .await
            .expect("the request completes")
            .status()
            .as_u16();

        listener_task.abort();
        let _ = std::fs::remove_dir_all(&root);
        status
    }

    /// Drive the long-poll body with the same Update, through a recorder.
    async fn drive_poll_path(body: &str, dispatch: &RecordingDispatch) {
        let root = scratch_dir("poll-path");
        let offset_path = root.join("telegram_offset.txt");
        poll_and_advance(
            dispatch,
            &root,
            &[42],
            &offset_path,
            0,
            || {
                let body = body.to_string();
                async move {
                    Ok(vec![serde_json::from_str::<Update>(&body)
                        .expect("test body is an Update")])
                }
            },
        )
        .await
        .expect("a healthy poll returns");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Acceptance: the same Update — a keyword — produces the same observable
    /// effect through the long-poll body and through a real HTTP POST.
    #[tokio::test]
    async fn both_transports_produce_the_same_effect_for_a_keyword() {
        let body = message_body(1001, "/help");

        let poll_dispatch = RecordingDispatch::default();
        drive_poll_path(&body, &poll_dispatch).await;

        let hook_dispatch = Arc::new(RecordingDispatch::default());
        let status = post(body, Some("s3cret"), hook_dispatch.clone(), "s3cret").await;

        assert_eq!(status, 200, "a correctly-authenticated Update is accepted");
        assert_eq!(
            poll_dispatch.sent(),
            hook_dispatch.sent(),
            "both transports produce the same chat reply"
        );
        assert_eq!(
            hook_dispatch.sent(),
            vec![(42_i64, crate::tg::handlers::handle_help())],
            "the keyword reply is the help text, sent to the sender's chat"
        );
        assert!(
            hook_dispatch.prompted().is_empty(),
            "a keyword never prompts the agent"
        );
    }

    /// Acceptance: the same Update — free text — reaches the agent prompt
    /// identically through both transports.
    #[tokio::test]
    async fn both_transports_produce_the_same_effect_for_free_text() {
        let body = message_body(1002, "what is stuck?");

        let poll_dispatch = RecordingDispatch::default();
        drive_poll_path(&body, &poll_dispatch).await;

        let hook_dispatch = Arc::new(RecordingDispatch::default());
        let status = post(body, Some("s3cret"), hook_dispatch.clone(), "s3cret").await;

        assert_eq!(status, 200);
        let poll = poll_dispatch.prompted();
        let hook = hook_dispatch.prompted();
        assert_eq!(poll.len(), 1);
        assert_eq!(hook.len(), 1);
        assert_eq!(
            poll[0].1, hook[0].1,
            "both transports prompt the agent with the same text"
        );
        assert!(
            poll[0].0.starts_with("zolly-") && hook[0].0.starts_with("zolly-"),
            "the prompt targets this office's zolly agent: {:?} / {:?}",
            poll[0].0,
            hook[0].0
        );
        assert!(
            hook[0].1.contains("what is stuck?"),
            "the prompt carries the message: {}",
            hook[0].1
        );

        // Both paths tracked a pending reply for chat 42; clear the global map.
        take_pending(42);
    }

    /// Acceptance: a missing or wrong secret is 401, and the handler never runs.
    #[tokio::test]
    async fn a_missing_or_wrong_secret_is_401_and_never_reaches_the_handler() {
        let dispatch = Arc::new(RecordingDispatch::default());
        let missing = post(message_body(1, "/help"), None, dispatch.clone(), "s3cret").await;
        let wrong = post(
            message_body(2, "/help"),
            Some("wrong"),
            dispatch.clone(),
            "s3cret",
        )
        .await;

        assert_eq!(missing, 401, "a request with no secret header is refused");
        assert_eq!(wrong, 401, "a request with the wrong secret is refused");
        assert!(
            dispatch.sent().is_empty() && dispatch.prompted().is_empty(),
            "a refused request must never reach handle_update"
        );
    }

    #[test]
    fn secret_matches_requires_an_exact_present_header() {
        assert!(secret_matches("s3cret", Some("s3cret")));
        assert!(!secret_matches("s3cret", Some("S3CRET")));
        assert!(!secret_matches("s3cret", Some("")));
        assert!(!secret_matches("s3cret", None));
    }

    #[test]
    fn validate_secret_enforces_telegrams_rules() {
        assert!(validate_secret("abc_123-XYZ").is_ok());
        assert!(validate_secret("").is_err());
        assert!(validate_secret(&"a".repeat(257)).is_err());
        assert!(validate_secret("has space").is_err());
        assert!(validate_secret("emoji🙂").is_err());
    }

    #[test]
    fn find_subslice_locates_the_header_terminator() {
        assert_eq!(find_subslice(b"a\r\n\r\nb", b"\r\n\r\n"), Some(1));
        assert_eq!(find_subslice(b"nothing", b"\r\n\r\n"), None);
    }
}
