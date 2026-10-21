# Defect report: every voice note fails to transcribe because the download URL is missing `/file/`

Repo: `/home/khan/Projects/zolly` (the office-kit source; the `zolly` binary at
`/home/khan/.local/bin/zolly` is built from `tool/` here).
Symptom observed in: `/home/khan/Projects/zylosync`, 2026-10-22.

## What the founder sees

He sends a voice note on Telegram. The bot replies:

```
Voice transcription failed: voice transcription unavailable: ffmpeg could not
decode the note: Error opening input files: End of file
```

`ffmpeg` is installed and working. `voxtype` is installed, and the parakeet
model is present. The audio is fine. The download is what fails — and it fails
in a way that looks like a codec problem.

## Root cause — the download URL is wrong

`tool/src/tg/send.rs:470`:

```rust
let url = format!("{}{token}/{file_path}", api_root());
```

With `api_root()` = `https://api.telegram.org/bot`, that builds:

```
https://api.telegram.org/bot<token>/voice/file_123.oga
```

Telegram serves files from a **different** path. The `file_path` returned by
`getFile` is relative to the `/file/bot<token>/` prefix:

```
https://api.telegram.org/file/bot<token>/<file_path>
```

The missing `/file/` segment is the whole bug.

## Measured, against the real API

Using the bot's own profile photo as the file (a real `file_id` from
`getUserProfilePhotos`, then its `file_path` from `getFile`):

```console
$ curl -sS -o /tmp/tgfile.bin -w 'http=%{http_code} size=%{size_download}\n' \
    "https://api.telegram.org/bot${TOKEN}/${FP}"
http=404 size=55

$ file /tmp/tgfile.bin
/tmp/tgfile.bin: JSON text data

$ cat /tmp/tgfile.bin
{"ok":false,"error_code":404,"description":"Not Found"}
```

The URL the code builds returns **404 with a 55-byte JSON error body**. The
URL Telegram documents returns the file:

```console
$ curl -sS -o /tmp/tgfile2.bin -w 'http=%{http_code} size=%{size_download}\n' \
    "https://api.telegram.org/file/bot${TOKEN}/${FP}"
http=200 size=5669

$ file /tmp/tgfile2.bin
/tmp/tgfile2.bin: JPEG image data, JFIF standard 1.01, ... 160x160
```

## Why the error message points at ffmpeg instead of the download

`transcribe_voice` never checks the HTTP status. It reads the body and writes
it to the temp file:

```rust
let audio = send_redacted(client.get(&url), "voice file download failed")
    .await?
    .bytes()
    .await
    ...
std::fs::write(&tmp, &audio).context("failed to write temp voice file")?;
```

So the 55-byte JSON error becomes the `.ogg` file. ffmpeg is then handed a
file that is not audio, and reports:

```
[in#0 @ ...] Error opening input: End of file
Error opening input file /tmp/xxx.ogg.
Error opening input files: End of file
```

That message is what reaches the founder.

### The empty-bytes guard does not catch it

Commit `35db6f9` ("telegram: name an empty voice download instead of ffmpeg's
EOF", 2026-10-22 03:31) added:

```rust
if audio.is_empty() {
    anyhow::bail!("voice download returned no bytes (Telegram delivered an empty file or the connection dropped); send the note again");
}
```

The 404 body is **55 bytes, not zero**, so the guard does not fire. The
defect survives it — which is why the founder still saw the ffmpeg error after
that fix landed. Reproduced:

```console
$ printf 'OggS' > /tmp/trunc.ogg
$ ffmpeg -y -loglevel error -i /tmp/trunc.ogg -ar 16000 -ac 1 /tmp/trunc.wav
[in#0 @ ...] Error opening input: End of file
Error opening input files: End of file
```

A 4-byte file produces exactly the message the founder received.

## The fix

Two changes, both needed.

**1. Build the URL correctly.** The `/file/` segment belongs between the API
root and the token:

```rust
let url = format!("{}file/bot{token}/{file_path}", api_root());
```

Note `api_root()` already ends in `/bot`, so the correct form is
`{api_root()}/file/bot{token}/{file_path}` — **or** restructure so the root
does not include `/bot`. Whichever is chosen, the resulting URL must be
`https://api.telegram.org/file/bot<token>/<file_path>`. Test it against the
real API, not against a mock.

**2. Check the HTTP status before writing the bytes.** A non-2xx response must
never reach the decoder. The error body is JSON, and its `description` says
exactly what went wrong (`Not Found`). Surfacing that is the difference
between "ffmpeg could not decode the note" and the actual cause.

```rust
let response = send_redacted(client.get(&url), "voice file download failed").await?;
if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("voice download failed: HTTP {status} — {body}");
}
```

The `is_empty()` guard from `35db6f9` stays — it is correct for the case it
names. The new check is what catches this one.

**3. A sanity check on the bytes.** The Telegram Bot API documents the
`file_path` extension; an ogg voice note is `.oga`/`.ogg`. A body that starts
with `{` is a JSON error, not audio. Checking the magic bytes would turn any
future URL mistake into a clear message instead of a codec error. This is
optional if the status check is in place, but it is cheap.

## Why this is the worst bug in the office right now

Voice notes are how the founder talks to the office. Every single one fails.
And it fails with a message that says "ffmpeg could not decode the note" —
which sends the reader looking at the codec, the model, and the decoder, none
of which are wrong. The founder's own `haa` path had the same shape of
defect: the office reported a symptom from the wrong layer.

## Evidence to reproduce

```sh
# in the kit repo
sed -n '465,490p' tool/src/tg/send.rs    # the URL and the missing status check

# with a real token
TOKEN=...
FID=$(curl -sS "https://api.telegram.org/bot${TOKEN}/getUserProfilePhotos?user_id=<bot_id>" \
      | python3 -c "import json,sys; print(json.load(sys.stdin)['result']['photos'][0][0]['file_id'])")
FP=$(curl -sS "https://api.telegram.org/bot${TOKEN}/getFile?file_id=$FID" \
     | python3 -c "import json,sys; print(json.load(sys.stdin)['result']['file_path'])")

# the code's URL -> 404 JSON
curl -sS -o /tmp/a -w '%{http_code} %{size_download}\n' "https://api.telegram.org/bot${TOKEN}/${FP}"

# the correct URL -> the file
curl -sS -o /tmp/b -w '%{http_code} %{size_download}\n' "https://api.telegram.org/file/bot${TOKEN}/${FP}"
```

## Scope

Documentation only; this report ships no code change. The fix belongs in
`tool/src/tg/send.rs` in `transcribe_voice`.

There is no test for this path today. A test that builds the download URL and
asserts it contains `/file/bot<token>/` would have caught this without a
network call. The status check deserves one too — assert that a 404 body is
never written to the temp file.

The repo also has an unrelated uncommitted change in `tool/src/herdr.rs` (a
`launch_args` parameter). It is deliberately not swept into this branch.
