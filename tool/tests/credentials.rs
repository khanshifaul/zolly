//! A hand-run command must find the office credentials on its own.
//!
//! `. ~/.config/zolly/<frequency>/credentials && zolly …` only sets shell
//! variables; it does not export them, so the binary never saw the token.
//! These tests run the real binary in an office with no Telegram environment
//! and a stub Bot API, and assert the request carries the stored token/secret.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "zolly-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..head_end]);
            let len = head.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            });
            if let Some(len) = len {
                if buf.len() >= head_end + 4 + len {
                    break;
                }
            } else {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// One-request Bot API stub: replies `{"ok":true,"result":true}`.
fn start_stub() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
    listener.set_nonblocking(true).expect("nonblocking stub");
    let addr = listener.local_addr().expect("stub addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_request(&mut stream);
                    let body = r#"{"ok":true,"result":true}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = tx.send(request);
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });
    (format!("http://{addr}"), rx)
}

/// Run `telegram-set-webhook` in a throwaway office with credentials on disk.
fn run(office: &Path, home: &Path, api: &str, secret_flag: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zolly"));
    command
        .current_dir(office)
        .env_clear()
        .env("HOME", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("ZOLLY_TELEGRAM_API", api)
        .args([
            "telegram-set-webhook",
            "--url",
            "https://example.test/hook",
        ]);
    if let Some(secret) = secret_flag {
        command.args(["--secret", secret]);
    }
    command.output().expect("run zolly")
}

fn office_with_credentials(home: &Path) -> PathBuf {
    let office = temp_dir("office");
    let frequency = "test-office";
    std::fs::create_dir_all(office.join(".agents/zolly")).expect("config dir");
    std::fs::write(
        office.join(".agents/zolly/config.toml"),
        format!("frequency = \"{frequency}\"\n"),
    )
    .expect("write config");
    // A committed project-tree .env must never override the machine
    // credentials; this token is deliberately wrong.
    std::fs::write(
        office.join(".agents/zolly/.env"),
        "ZOLLY_TELEGRAM_TOKEN=evil-token\nZOLLY_ALLOWED_IDS=666\n",
    )
    .expect("write ignored project env");
    let dir = home.join(".config/zolly").join(frequency);
    std::fs::create_dir_all(&dir).expect("credentials dir");
    std::fs::write(
        dir.join("project"),
        office
            .canonicalize()
            .expect("canonical office")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("write ownership marker");
    std::fs::write(
        dir.join("credentials"),
        "ZOLLY_TELEGRAM_TOKEN=test-token\n\
         ZOLLY_ALLOWED_IDS=1\n\
         ZOLLY_TELEGRAM_WEBHOOK_SECRET=s3cret\n",
    )
    .expect("write credentials");
    office
}

#[test]
fn set_webhook_reads_the_office_credentials() {
    let home = temp_dir("home");
    let office = office_with_credentials(&home);
    let (api, requests) = start_stub();

    let output = run(&office, &home, &api, None);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("stub received no request");
    assert!(
        request.starts_with("POST /bottest-token/setWebhook"),
        "request: {request}"
    );
    assert!(request.contains("\"secret_token\":\"s3cret\""), "request: {request}");
    assert!(
        request.contains("https://example.test/hook"),
        "request: {request}"
    );

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&office);
}

#[test]
fn an_explicit_secret_still_wins() {
    let home = temp_dir("home");
    let office = office_with_credentials(&home);
    let (api, requests) = start_stub();

    let output = run(&office, &home, &api, Some("flagsecret"));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("stub received no request");
    assert!(
        request.contains("\"secret_token\":\"flagsecret\""),
        "request: {request}"
    );
    assert!(!request.contains("s3cret"), "request: {request}");

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&office);
}
