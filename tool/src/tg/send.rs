//! Outbound delivery: messages, documents, voice notes, polls, bot check.
//! Every send goes through `send_message`, which chunks at 4096 bytes and
//! retries rate limits, so callers never think about either.

use anyhow::{Context, Result};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::config::{
    api_root, api_url, get_allowed_ids, get_client, get_token, json_redacted, send_redacted,
    RequestFailure,
};
use super::format::{is_stdin_marker, read_stdin, split_message};
use super::types::{Message, TelegramResponse, User};

/// POST one chunk of a message, retrying on Telegram 429 up to 3 attempts.
async fn send_chunk(client: &Client, token: &str, chat_id: i64, part: &str) -> Result<()> {
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let resp = send_redacted(
            client
                .post(api_url(token, "sendMessage"))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": part,
                })),
            "Failed to send message",
        )
        .await?;
        let resp: TelegramResponse<Message> =
            json_redacted(resp, "Failed to parse sendMessage response").await?;

        if resp.ok {
            return Ok(());
        }

        let retry_after = resp.parameters.as_ref().and_then(|p| p.retry_after);
        if let Some(secs) = retry_after {
            if attempts < 3 {
                eprintln!("sendMessage rate-limited, retrying in {}s", secs);
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
        }

        anyhow::bail!(
            "sendMessage failed: {}",
            resp.description.unwrap_or_default()
        );
    }
}

pub async fn send_message(token: &str, chat_id: i64, text: &str) -> Result<()> {
    let client = get_client();
    let parts = split_message(text, 4096);

    let total = parts.len();
    for (i, part) in parts.iter().enumerate() {
        if let Err(e) = send_chunk(&client, token, chat_id, part).await {
            // Report which chunk failed; earlier chunks are already delivered.
            return Err(e).with_context(|| {
                format!("sendMessage failed on chunk {}/{}", i + 1, total)
            });
        }
    }

    Ok(())
}

/// Resolve the target chat: explicit `--chat-id`, else the first allowed id.
pub fn resolve_chat(chat_id: Option<i64>) -> Result<i64> {
    match chat_id {
        Some(id) => Ok(id),
        None => {
            let ids = get_allowed_ids()?;
            ids.first()
                .copied()
                .context("ZOLLY_ALLOWED_IDS is empty — need at least one chat id")
        }
    }
}

pub async fn cmd_check(_root: &Path) -> Result<()> {
    let token = get_token()?;
    let client = get_client();

    let resp = send_redacted(
        client.get(api_url(&token, "getMe")),
        "Failed to call getMe",
    )
    .await?;
    let resp: TelegramResponse<User> =
        json_redacted(resp, "Failed to parse getMe response").await?;

    if !resp.ok {
        anyhow::bail!("getMe failed: {}", resp.description.unwrap_or_default());
    }

    let user = resp.result.context("No result in getMe response")?;
    println!(
        "✓ Bot verified: @{} (id: {})",
        user.username.unwrap_or_default(),
        user.id
    );
    Ok(())
}

pub async fn cmd_send(_root: &Path, text: Option<String>, chat_id: Option<i64>) -> Result<()> {
    let token = get_token()?;
    let target_chat = resolve_chat(chat_id)?;

    let message = if is_stdin_marker(&text) {
        read_stdin()?
    } else {
        text.unwrap_or_default()
    };
    if message.trim().is_empty() {
        anyhow::bail!("Nothing to send: pass --text or pipe a report on stdin");
    }

    send_message(&token, target_chat, &message).await?;
    println!("Message sent to chat {}", target_chat);
    // Remove from pending replies since we got a response
    super::serve::take_pending(target_chat);
    Ok(())
}

/// Send a document file via Telegram Bot API.
pub async fn cmd_send_file(
    _root: &Path,
    file: &str,
    caption: Option<String>,
    chat_id: Option<i64>,
) -> Result<()> {
    let token = get_token()?;
    let target_chat = resolve_chat(chat_id)?;

    let file_path = Path::new(file);
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");

    let file_bytes = tokio::fs::read(file_path)
        .await
        .context("Failed to read file")?;

    let client = get_client();
    let form = reqwest::multipart::Form::new()
        .part(
            "chat_id",
            reqwest::multipart::Part::text(target_chat.to_string()),
        )
        .part(
            "document",
            reqwest::multipart::Part::bytes(file_bytes)
                .file_name(file_name.to_string())
                .mime_str("application/octet-stream")?,
        );

    let form = if let Some(cap) = caption {
        form.part("caption", reqwest::multipart::Part::text(cap))
    } else {
        form
    };

    let resp = send_redacted(
        client.post(api_url(&token, "sendDocument")).multipart(form),
        "Failed to send document",
    )
    .await?;
    let resp: TelegramResponse<Message> =
        json_redacted(resp, "Failed to parse sendDocument response").await?;

    if !resp.ok {
        anyhow::bail!(
            "sendDocument failed: {}",
            resp.description.unwrap_or_default()
        );
    }

    println!("Document sent to chat {}", target_chat);
    // Remove from pending replies since we got a response
    super::serve::take_pending(target_chat);
    Ok(())
}

/// Split comma-separated poll options, trimming whitespace and dropping empty
/// entries. Returns an error naming the surviving count when fewer than two
/// remain — Telegram's `sendPoll` requires at least two non-empty options.
fn parse_poll_options(options: &str) -> Result<Vec<String>> {
    let opts: Vec<String> = options
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if opts.len() < 2 {
        anyhow::bail!(
            "sendPoll needs at least two non-empty options, got {}",
            opts.len()
        );
    }
    Ok(opts)
}

/// Build the sendPoll query parameters for a given option set and flags.
/// Pure — no network, no token — so both states can be asserted without sending.
pub fn poll_query_params(question: &str, options: &[String], anonymous: bool, multiple: bool) -> Vec<(&'static str, String)> {
    let options_json = serde_json::to_string(options).expect("options serialize");
    vec![
        ("question", question.to_string()),
        ("options", options_json),
        ("is_anonymous", anonymous.to_string()),
        ("allows_multiple_answers", multiple.to_string()),
    ]
}

/// Send a poll to the allowed chat. `options` is comma-separated; individual
/// options may not contain commas.
pub async fn cmd_poll(
    _root: &Path,
    question: String,
    options: String,
    anonymous: bool,
    multiple: bool,
) -> Result<()> {
    let token = get_token()?;
    let chat_id = resolve_chat(None)?;

    let opts = parse_poll_options(&options)?;
    let mut query = vec![("chat_id", chat_id.to_string())];
    query.extend(poll_query_params(&question, &opts, anonymous, multiple));

    let client = get_client();
    let resp = send_redacted(
        client
            .post(api_url(&token, "sendPoll"))
            .query(&query),
        "Failed to send poll",
    )
    .await?;
    let resp: TelegramResponse<serde_json::Value> =
        json_redacted(resp, "Failed to parse poll response").await?;

    if !resp.ok {
        anyhow::bail!("sendPoll failed: {}", resp.description.unwrap_or_default());
    }

    let poll_id = resp
        .result
        .as_ref()
        .and_then(|r| r.get("poll"))
        .and_then(|p| p.get("id"))
        .and_then(|id| id.as_str())
        .unwrap_or("unknown");

    println!("Poll sent (id: {}): {}", poll_id, question);
    // Remove from pending replies since we got a response
    super::serve::take_pending(chat_id);
    Ok(())
}

pub async fn cmd_reply(chat_id: i64, text: String) -> Result<()> {
    let token = get_token()?;
    send_message(&token, chat_id, &text).await?;
    // Remove from pending replies since we got a response
    super::serve::take_pending(chat_id);
    println!("Reply sent to chat {}", chat_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Voice synthesis (Piper TTS → ffmpeg → sendVoice)
// ---------------------------------------------------------------------------

/// Path to the Piper voice model. Overridable via `ZOLLY_PIPER_MODEL`.
fn piper_model_path() -> String {
    std::env::var("ZOLLY_PIPER_MODEL").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|home| {
                home.join(".local/share/piper/voices/en/en_US/lessac/medium/en_US-lessac-medium.onnx")
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "en_US-lessac-medium.onnx".to_string())
    })
}
// Path to the Piper binary. `ZOLLY_PIPER_BIN` wins; otherwise the known
// `~/.local/bin/piper` location (the uv-tools install), falling back to a bare
// PATH lookup for `piper` so a worker's shell without `~/.local/bin` on PATH
// still finds it.
fn piper_bin_path() -> String {
    if let Ok(bin) = std::env::var("ZOLLY_PIPER_BIN") {
        return bin;
    }
    if let Some(home) = dirs::home_dir() {
        let known = home.join(".local/bin/piper");
        if known.exists() {
            return known.to_string_lossy().into_owned();
        }
    }
    "piper".to_string()
}

async fn send_voice_note(token: &str, chat_id: i64, ogg_data: Vec<u8>) -> Result<()> {
    let client = get_client();
    let form = reqwest::multipart::Form::new()
        .part(
            "chat_id",
            reqwest::multipart::Part::text(chat_id.to_string()),
        )
        .part(
            "voice",
            reqwest::multipart::Part::bytes(ogg_data)
                .file_name("voice.ogg")
                .mime_str("audio/ogg")?,
        );

    let resp = send_redacted(
        client.post(api_url(token, "sendVoice")).multipart(form),
        "Failed to send voice",
    )
    .await?;
    let resp: TelegramResponse<Message> =
        json_redacted(resp, "Failed to parse sendVoice response").await?;

    if !resp.ok {
        anyhow::bail!("sendVoice failed: {}", resp.description.unwrap_or_default());
    }
    Ok(())
}

async fn text_fallback_voice(token: &str, chat_id: i64, message: &str) -> Result<()> {
    send_message(
        token,
        chat_id,
        &format!("🎤 Voice (text fallback):\n{}", message),
    )
    .await
}

/// Synthesize `message` with Piper, convert to OGG/Opus, send as a voice
/// note. Any synthesis failure degrades to a plain text message so the
/// founder still gets the words.
pub async fn cmd_voice(_root: &Path, text: Option<String>, chat_id: Option<i64>) -> Result<()> {
    let token = get_token()?;
    let target_chat = resolve_chat(chat_id)?;

    let message = if is_stdin_marker(&text) {
        read_stdin()?
    } else {
        text.unwrap_or_default()
    };

    let model_path = piper_model_path();
    let wav_path = "/tmp/zolly_voice.wav";
    let ogg_path = "/tmp/zolly_voice.ogg";

    // Generate WAV with Piper. We take stdin and write the text, then drop
    // stdin so the child sees EOF. wait_with_output() drains stdout and
    // stderr concurrently — reading them only after wait() can deadlock if
    // the child fills the 64 KiB pipe buffer.
    let piper_bin = piper_bin_path();
    let mut child = std::process::Command::new(&piper_bin)
        .args(["--model", &model_path, "--output-file", wav_path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn piper (is it on PATH?)")?;

    {
        let mut stdin = child
            .stdin
            .take()
            .context("Failed to take piper stdin")?;
        std::io::Write::write_all(&mut stdin, message.as_bytes())
            .context("Failed to write to piper stdin")?;
        // stdin drops here, closing the pipe.
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for piper")?;
    if !output.status.success() {
        eprintln!(
            "Piper failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        text_fallback_voice(&token, target_chat, &message).await?;
        // Remove from pending replies since we got a response
        super::serve::take_pending(target_chat);
        return Ok(());
    }

    // Convert WAV to OGG/Opus 48 kHz mono.
    let ffmpeg_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            wav_path,
            "-c:a",
            "libopus",
            "-b:a",
            "48k",
            "-ac",
            "1",
            "-ar",
            "48000",
            ogg_path,
        ])
        .status()
        .context("Failed to run ffmpeg")?;

    if !ffmpeg_status.success() {
        eprintln!("ffmpeg exited {}", ffmpeg_status);
        text_fallback_voice(&token, target_chat, &message).await?;
        // Remove from pending replies since we got a response
        super::serve::take_pending(target_chat);
        return Ok(());
    }

    let ogg_data = std::fs::read(ogg_path).context("Failed to read OGG file")?;
    send_voice_note(&token, target_chat, ogg_data).await?;
    println!("Voice note sent to chat {}", target_chat);
    // Remove from pending replies since we got a response
    super::serve::take_pending(target_chat);
    Ok(())
}

/// Download a voice note from Telegram and transcribe it locally.
///
/// Telegram delivers OGG/Opus; voxtype takes 16 kHz mono WAV, so ffmpeg
/// converts first. The engine is Parakeet, and `parakeet-redux` is selected by
/// path because voxtype's catalog does not list downloaded directory models.
/// `ZOLLY_STT_MODEL` overrides the model with a name or a directory.
/// The URL Telegram serves a bot file from.
///
/// Telegram serves files from `/file/bot<token>/<file_path>`, NOT from
/// `/bot<token>/<file_path>`. `api_root()` already ends in `/bot`, so the
/// download URL must spell out the `file/bot` segment. Building
/// `/bot<token>/<path>` returns HTTP 404 with a 55-byte JSON error body;
/// written to the temp file as if it were audio, that made ffmpeg report
/// "Error opening input files: End of file" and reach the founder as "ffmpeg
/// could not decode the note". Measured against the live API on 2026-10-22:
/// the old URL returns 404 and 55 bytes of JSON; this one returns 200 and
/// the real file.
fn voice_download_url(api_root: &str, token: &str, file_path: &str) -> String {
    // `api_root` is `<base>/bot`; strip the trailing `/bot` so the segment
    // is spelled exactly once.
    let base = api_root.strip_suffix("/bot").unwrap_or(api_root);
    format!("{base}/file/bot{token}/{file_path}")
}

pub async fn transcribe_voice(token: &str, file_id: &str) -> Result<String> {
    let client = get_client();

    // 1. Resolve file_id → file_path
    let resp = send_redacted(
        client
            .get(&api_url(token, "getFile"))
            .query(&[("file_id", file_id)]),
        "getFile request failed",
    )
    .await?;
    let body: super::types::FileResponse =
        json_redacted(resp, "getFile response parse failed").await?;
    if !body.ok {
        anyhow::bail!(
            "getFile failed: {}",
            body.description.unwrap_or_else(|| "ok=false".into())
        );
    }
    let file_path = body
        .result
        .and_then(|r| r.file_path)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "getFile failed: {}",
                body.description.unwrap_or_else(|| "no file_path".into())
            )
        })?;

    // 2. Download the ogg/opus file.
    let url = voice_download_url(&api_root(), token, &file_path);
    let response = send_redacted(client.get(&url), "voice file download failed").await?;
    // Never hand a non-2xx body to the decoder: an error body is not audio,
    // and its `description` names the real cause.
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("voice download failed: HTTP {status} — {body}");
    }
    let audio = response
        .bytes()
        .await
        .map_err(|e| RequestFailure::from_reqwest("voice file read failed", &e))?;
    if audio.is_empty() {
        anyhow::bail!(
            "voice download returned no bytes (Telegram delivered an empty file or the connection dropped); send the note again"
        );
    }

    let tmp = unique_temp("ogg");
    std::fs::write(&tmp, &audio).context("failed to write temp voice file")?;
    let wav = tmp.with_extension("wav");

    // 3. Decode to the only format voxtype accepts.
    let convert = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(&tmp)
        .args(["-ar", "16000", "-ac", "1"])
        .arg(&wav)
        .output()
        .await;
    let _ = std::fs::remove_file(&tmp);
    let convert = match convert {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("voice transcription unavailable: ffmpeg is not installed")
        }
        Err(e) => return Err(e).context("failed to run ffmpeg"),
    };
    if !convert.status.success() {
        let err = stt_error_line(&convert.stderr);
        let _ = std::fs::remove_file(&wav);
        anyhow::bail!(
            "voice transcription unavailable: ffmpeg could not decode the note: {}",
            err
        );
    }

    // 4. Transcribe. `--model` is global, so it must precede the subcommand.
    let mut command = tokio::process::Command::new("voxtype");
    // -q drops the INFO lines; the progress lines remain and are filtered
    // out below. There is no flag for "transcript only".
    command.arg("-q");
    if let Some(model) = stt_model() {
        command.arg("--model").arg(model);
    }
    let output = command
        .args(["transcribe", "--engine", "parakeet"])
        .arg(&wav)
        .output()
        .await;
    let _ = std::fs::remove_file(&wav);
    let output = match output {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("voice transcription unavailable: voxtype is not installed")
        }
        Err(e) => return Err(e).context("failed to run voxtype"),
    };

    if !output.status.success() {
        anyhow::bail!(
            "voice transcription unavailable: {}",
            stt_error_line(&output.stderr)
        );
    }

    let transcript = voxtype_transcript(&output.stdout);
    if transcript.is_empty() {
        anyhow::bail!("transcription produced no text (audio may be silent)");
    }
    Ok(transcript)
}

/// The model voxtype should load.
///
/// `ZOLLY_STT_MODEL` wins — a name or a directory. Otherwise the
/// `parakeet-redux` directory under voxtype's XDG data dir, where downloaded
/// models live; `None` lets voxtype use its configured Parakeet model.
fn stt_model() -> Option<PathBuf> {
    stt_model_with(
        std::env::var("ZOLLY_STT_MODEL").ok().as_deref(),
        dirs::data_dir().as_deref(),
    )
}

fn stt_model_with(explicit: Option<&str>, data_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(value) = explicit {
        let value = value.trim();
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    data_dir
        .map(|dir| dir.join("voxtype/models/parakeet-redux"))
        .filter(|path| path.is_dir())
}

/// voxtype writes its progress to stdout, before the transcript and even in
/// quiet mode. Drop the known progress lines; a no-speech notice disappears
/// the same way, leaving an empty transcript the caller reports as silence.
fn voxtype_transcript(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty()
                && !line.starts_with("Loading audio file:")
                && !line.starts_with("Audio format:")
                && !line.starts_with("Processing ")
                && !line.starts_with("VAD:")
                && !line.starts_with("No speech detected")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// One line for the founder. voxtype logs INFO lines to stderr even on
/// success; on failure the last non-empty line is the `Error:` line.
fn stt_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "no error output".to_string())
}

/// A temp path unique per process and call: webhook deliveries can transcribe
/// concurrently with the poll loop.
fn unique_temp(ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "zolly_voice_{}_{}_{}.{}",
        std::process::id(),
        nanos,
        SEQ.fetch_add(1, Ordering::Relaxed),
        ext
    ))
}

// ---------------------------------------------------------------------------
// Poll options parsing tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod poll_tests {
    use super::*;

    #[test]
    fn options_are_trimmed_and_empty_entries_dropped() {
        let got = parse_poll_options(" yes , no , , maybe ").unwrap();
        assert_eq!(got, vec!["yes", "no", "maybe"]);
    }

    #[test]
    fn whitespace_only_entries_count_as_empty() {
        let got = parse_poll_options("a,  ,b").unwrap();
        assert_eq!(got, vec!["a", "b"]);
    }

    #[test]
    fn under_two_options_names_the_count() {
        let err = parse_poll_options("only one").unwrap_err().to_string();
        assert!(err.contains("got 1"), "{err}");

        let err = parse_poll_options("").unwrap_err().to_string();
        assert!(err.contains("got 0"), "{err}");

        let err = parse_poll_options("a, , ").unwrap_err().to_string();
        assert!(err.contains("got 1"), "{err}");
    }

    #[test]
    fn two_options_pass() {
        let got = parse_poll_options("yes,no").unwrap();
        assert_eq!(got, vec!["yes", "no"]);
    }
}

    #[test]
    fn anonymous_true_sends_is_anonymous_true() {
        let q = poll_query_params("q?", &["a".to_string(), "b".to_string()], true, false);
        let anon: Vec<_> = q.iter().filter(|(k, _)| *k == "is_anonymous").collect();
        assert_eq!(anon.len(), 1);
        assert_eq!(anon[0].1, "true");
    }

    #[test]
    fn anonymous_false_sends_is_anonymous_false() {
        let q = poll_query_params("q?", &["a".to_string(), "b".to_string()], false, false);
        let anon: Vec<_> = q.iter().filter(|(k, _)| *k == "is_anonymous").collect();
        assert_eq!(anon.len(), 1);
        assert_eq!(anon[0].1, "false");
    }

    #[test]
    fn multiple_true_sends_allows_multiple_answers_true() {
        let q = poll_query_params("q?", &["a".to_string(), "b".to_string()], true, true);
        let m: Vec<_> = q.iter().filter(|(k, _)| *k == "allows_multiple_answers").collect();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, "true");
    }

    #[test]
    fn multiple_false_default_sends_allows_multiple_answers_false() {
        let q = poll_query_params("q?", &["a".to_string(), "b".to_string()], true, false);
        let m: Vec<_> = q.iter().filter(|(k, _)| *k == "allows_multiple_answers").collect();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, "false");
    }

    #[test]
    fn chat_id_is_always_first_query_param() {
        // N/A here — chat_id is added by cmd_poll; this asserts poll_query_params
        // itself has no chat_id, keeping it pure.
        let q = poll_query_params("q?", &["a".to_string(), "b".to_string()], true, false);
        assert!(!q.iter().any(|(k, _)| *k == "chat_id"));
    }
// ---------------------------------------------------------------------------
// Voice resolution and fallback tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod voice_tests {
    use super::*;

    /// Resolution order: ZOLLY_PIPER_BIN wins, then the known install path,
    /// then the bare command name on PATH. One test: the env var is
    /// process-global, and two tests mutating it race under the parallel
    /// harness (observed 2026-10-21).
    #[test]
    fn piper_bin_resolution_order() {
        unsafe { std::env::set_var("ZOLLY_PIPER_BIN", "/custom/piper") };
        assert_eq!(piper_bin_path(), "/custom/piper");
        unsafe { std::env::remove_var("ZOLLY_PIPER_BIN") };
        let known = dirs::home_dir().map(|h| h.join(".local/bin/piper"));
        match known {
            Some(path) if path.exists() => {
                assert_eq!(piper_bin_path(), path.to_string_lossy());
            }
            _ => assert_eq!(piper_bin_path(), "piper"),
        }
    }

    /// text_fallback_voice prefixes the message exactly as send.rs does.
    #[test]
    fn voice_text_fallback_prefix() {
        let message = "hello founder";
        let expected = format!("🎤 Voice (text fallback):\n{}", message);
        assert_eq!(expected, "🎤 Voice (text fallback):\nhello founder");
        assert!(expected.starts_with("🎤 Voice (text fallback):\n"));
    }
}

#[cfg(test)]
mod download_url_tests {
    use super::*;

    /// Telegram serves bot files from `/file/bot<token>/<file_path>`.
    ///
    /// The API root already ends in `/bot`, so the download URL must spell
    /// out the `file/bot` segment. Building `/bot<token>/<path>` returns
    /// HTTP 404 with a 55-byte JSON error body; written to the temp file as
    /// if it were audio, that made ffmpeg report "Error opening input files:
    /// End of file" and reach the founder as "ffmpeg could not decode the
    /// note" (measured against the live API, 2026-10-22).
    ///
    /// The URL is built by `voice_download_url` in the parent module, so
    /// this test pins the function the download path actually calls — not a
    /// copy of it.

    #[test]
    fn voice_download_url_carries_the_file_segment() {
        let url = voice_download_url(
            "https://api.telegram.org/bot",
            "123:ABC",
            "voice/file_9.oga",
        );
        assert_eq!(
            url,
            "https://api.telegram.org/file/bot123:ABC/voice/file_9.oga"
        );
        // The bug this pins: the API root already ends in `/bot`, so
        // `"{api_root}{token}/{path}"` produced `/bot<token>/<path>` — the
        // path Telegram does NOT serve files from.
        assert!(!url.contains("/botfile/bot"));
        assert!(url.contains("/file/bot"));
    }

    #[test]
    fn voice_download_url_never_drops_the_file_segment() {
        // Whatever the API root looks like, the `/file/bot` segment and the
        // token must both be present, in that order.
        let url = voice_download_url("https://mirror.example/bot", "TOK", "a/b.oga");
        assert!(url.contains("/file/botTOK/"));
        assert!(url.ends_with("/a/b.oga"));
    }
}

#[cfg(test)]
mod stt_tests {
    use super::*;

    #[test]
    fn explicit_model_wins_and_blank_is_ignored() {
        assert_eq!(
            stt_model_with(Some("some-model"), Some(Path::new("/does/not/exist"))),
            Some(PathBuf::from("some-model"))
        );
        assert_eq!(stt_model_with(Some("   "), None), None);
    }

    #[test]
    fn redux_is_found_in_the_voxtype_data_dir() {
        let root = std::env::temp_dir().join(format!(
            "zolly-stt-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let model = root.join("voxtype/models/parakeet-redux");
        std::fs::create_dir_all(&model).expect("create test model dir");
        assert_eq!(stt_model_with(None, Some(&root)), Some(model));
        std::fs::remove_dir_all(&root).expect("remove test model dir");
        assert_eq!(stt_model_with(None, Some(&root)), None);
    }

    #[test]
    fn transcript_ignores_the_progress_block() {
        let stdout = b"Loading audio file: \"x.wav\"\nAudio format: 16000 Hz, 1 channel(s), Int\nProcessing 176000 samples (11.00s)...\nVAD: 8.20s speech (74.5% of audio)\n\nhello world\n";
        assert_eq!(voxtype_transcript(stdout), "hello world");
        let silent = b"Loading audio file: \"x.wav\"\nAudio format: 16000 Hz, 1 channel(s), Int\nProcessing 32000 samples (2.00s)...\nVAD: 0.00s speech (0.0% of audio)\nNo speech detected, skipping transcription.\n";
        assert_eq!(voxtype_transcript(silent), "");
        assert_eq!(voxtype_transcript(b"plain transcript\n"), "plain transcript");
    }

    #[test]
    fn the_founder_sees_the_last_error_line() {
        let stderr = b"2026-10-21T14:00:00Z INFO Loading model\nError: Unknown model 'x'\n\n";
        assert_eq!(stt_error_line(stderr), "Error: Unknown model 'x'");
        assert_eq!(stt_error_line(b""), "no error output");
    }
}
