//! A resumed manager seat must never stay unnamed.
//!
//! herdr keeps a restored pane and the omp inside it but loses the agent name
//! the Telegram listener prompts; the update then dies as `agent_not_found`
//! and lands in `dead_letter.jsonl` behind a `200 OK`. These tests drive the
//! real binary against a stateful fake `herdr` on PATH:
//!
//!   - bare `zolly` in a pane that already hosts an unnamed agent registers it
//!     instead of refusing (and never starts a second omp);
//!   - a pane hosting another seat's named agent is refused, not stolen;
//!   - a webhook-delivered message to an unnamed manager renames the live pane
//!     and delivers the prompt instead of dead-lettering it.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Fake `herdr`. Behaviour:
///   - `pane current` always reports a live omp in `w1:p1` / tab `w1:t1`;
///   - `agent list` reports that agent with the current name (`null` when
///     unnamed), seeded by `HERDR_INITIAL_NAME`;
///   - `agent rename` writes the new name to `$HERDR_STATE/name`;
///   - `agent prompt` fails with herdr's `agent_not_found` JSON until the
///     prompted name matches the state, then logs `prompt-ok <name>`.
const FAKE_HERDR: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HERDR_LOG"
name=null
if [ -f "$HERDR_STATE/name" ]; then
  read -r raw < "$HERDR_STATE/name"
  name="\"$raw\""
elif [ -n "$HERDR_INITIAL_NAME" ]; then
  name="\"$HERDR_INITIAL_NAME\""
fi
case "$1 $2" in
  "pane current")
    printf '{"result":{"pane":{"pane_id":"w1:p1","tab_id":"w1:t1","agent":"omp"}}}\n'
    ;;
  "tab list")
    printf '{"result":{"tabs":[{"tab_id":"w1:t1","label":"zolly@test"}]}}\n'
    ;;
  "pane list")
    printf '{"result":{"panes":[{"pane_id":"w1:p1","workspace_id":"w1","cwd":"%s","tab_id":"w1:t1","agent":"omp"}]}}\n' "$PWD"
    ;;
  "agent list")
    printf '{"result":{"agents":[{"agent":"omp","name":%s,"pane_id":"w1:p1","tab_id":"w1:t1","agent_status":"idle","cwd":"%s"}]}}\n' "$name" "$PWD"
    ;;
  "agent rename")
    printf '%s\n' "$4" > "$HERDR_STATE/name"
    ;;
  "agent prompt")
    want=
    [ -f "$HERDR_STATE/name" ] && read -r want < "$HERDR_STATE/name"
    if [ "$3" = "$want" ]; then
      printf 'prompt-ok %s\n' "$3" >> "$HERDR_LOG"
      exit 0
    fi
    printf '{"error":{"code":"agent_not_found","message":"agent target %s not found"},"id":"cli:agent:prompt"}\n' "$3" >&2
    exit 1
    ;;
  *)
    ;;
esac
exit 0
"#;

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zolly-resumed-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

fn write_script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).expect("write fake executable");
    let mut perms = fs::metadata(&path).expect("stat fake executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod fake executable");
}

/// A minimal office: the config `Office::load` reads, the persona bare `zolly`
/// insists on, and nothing else.
fn office(root: &Path) {
    let agents = root.join(".agents/zolly");
    fs::create_dir_all(&agents).expect("create office");
    fs::write(agents.join("config.toml"), "frequency = \"test\"\n").expect("write config");
    fs::write(agents.join("agent.md"), "# Zolly\n").expect("write persona");
}

/// A fake recorder the seat path needs: gather_checks asks git questions.
fn fake_git(bin: &Path) {
    write_script(bin, "git", "#!/bin/sh\nexit 0\n");
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    state: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = temp_dir(name);
        let root = base.join("repo");
        let bin = base.join("bin");
        let state = base.join("herdr-state");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        office(&root);
        write_script(&bin, "herdr", FAKE_HERDR);
        let log = base.join("herdr.log");
        Self {
            base,
            root,
            bin,
            state,
            log,
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn seat_command(fixture: &Fixture, initial_name: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_zolly"))
        .current_dir(&fixture.root)
        .env("PATH", &fixture.bin)
        .env("HOME", fixture.root.join("home"))
        .env("HERDR_STATE", &fixture.state)
        .env("HERDR_LOG", &fixture.log)
        .env("HERDR_INITIAL_NAME", initial_name)
        .env_remove("ZOLLY_FREQUENCY")
        .output()
        .expect("run zolly")
}

#[test]
fn a_resumed_manager_pane_is_named_without_starting_omp() {
    let fixture = Fixture::new("seat-unnamed");
    fake_git(&fixture.bin);

    let out = seat_command(&fixture, "");

    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("registered it as zolly-test"),
        "the seat must say what it registered: {stdout}"
    );
    let log = fixture.log();
    assert!(log.contains("agent rename w1:p1 zolly-test"), "{log}");
    assert!(
        !log.contains("omp"),
        "the seat must not start a second agent in a live pane: {log}"
    );
}

#[test]
fn a_pane_hosting_another_seat_is_refused_not_stolen() {
    let fixture = Fixture::new("seat-worker");
    fake_git(&fixture.bin);

    let out = seat_command(&fixture, "worker-t-1");

    assert!(
        !out.status.success(),
        "a worker's pane must not be adopted as the manager"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already hosts 'worker-t-1'"), "{stderr}");
    assert!(
        !fixture.log().contains("agent rename"),
        "nothing may be renamed: {}",
        fixture.log()
    );
}

/// A bare HTTP stub for the one Telegram call the webhook listener makes at
/// startup (`getWebhookInfo`). It claims no webhook is registered.
fn telegram_stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind telegram stub");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let body = r#"{"ok":true,"result":{"url":"","pending_update_count":0}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

/// POST one Update the way Telegram's webhook would, then read the reply.
fn post_update(addr: &str, secret: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect webhook");
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nX-Telegram-Bot-Api-Secret-Token: {secret}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write update");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_listener_names_a_resumed_manager_and_delivers_the_message() {
    let fixture = Fixture::new("webhook-recovery");
    let api_port = telegram_stub();

    let child = Command::new(env!("CARGO_BIN_EXE_zolly"))
        .args([
            "telegram-serve",
            "--webhook",
            "--bind",
            "127.0.0.1:0",
            "--secret",
            "test-secret",
        ])
        .current_dir(&fixture.root)
        .env("PATH", &fixture.bin)
        .env("HOME", fixture.root.join("home"))
        .env("HERDR_STATE", &fixture.state)
        .env("HERDR_LOG", &fixture.log)
        .env("HERDR_INITIAL_NAME", "")
        .env("ZOLLY_TELEGRAM_TOKEN", "123456:test-token")
        .env("ZOLLY_ALLOWED_IDS", "42")
        .env("ZOLLY_TELEGRAM_API", format!("http://127.0.0.1:{api_port}"))
        .env_remove("ZOLLY_FREQUENCY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn webhook listener");
    let mut child = KillOnDrop(child);
    let stdout = child.0.stdout.take().expect("child stdout");

    // `println!` is line-buffered, so this returns as soon as the listener is
    // bound; the accept loop is started immediately after the line is written.
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read startup line");
    let addr = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("http://"))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("no bind address in startup line: {line:?}"));

    let update = r#"{"update_id":5,"message":{"message_id":1,"from":{"id":42,"is_bot":false,"first_name":"Founder"},"chat":{"id":42,"type":"private"},"date":1700000000,"text":"what is stuck?"}}"#;
    let response = post_update(&addr, "test-secret", update);

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "the webhook must acknowledge: {response}"
    );
    let log = fixture.log();
    assert!(log.contains("agent rename w1:p1 zolly-test"), "{log}");
    assert!(log.contains("prompt-ok zolly-test"), "{log}");
    assert_eq!(
        log.matches("agent prompt zolly-test").count(),
        2,
        "one failed prompt, then one delivered after the rename: {log}"
    );
    assert!(
        !fixture.root.join(".agents/zolly/dead_letter.jsonl").exists(),
        "a recovered message must not be dead-lettered"
    );
}
