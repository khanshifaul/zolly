//! `zolly bus` prefers AgentRadio and falls back to a one-line herdr prompt.
//!
//! These tests use fake `radio`/`herdr` executables on PATH. They never touch
//! the real bus, the real herdr server, or an office outside their temp tree.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zolly-bus-{name}-{}-{}",
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

fn office(root: &Path) {
    let cfg = root.join(".agents/zolly");
    fs::create_dir_all(&cfg).expect("create office");
    fs::write(cfg.join("config.toml"), "frequency = \"test\"\n").expect("write config");
}

fn run_bus(root: &Path, bin: &Path, herdr_log: &Path, radio_log: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zolly"))
        .args(["bus", "foreman", "task", "ack", "hi"])
        .current_dir(root)
        .env("PATH", bin)
        .env("HOME", root.join("home"))
        .env("HERDR_LOG", herdr_log)
        .env("RADIO_LOG", radio_log)
        .output()
        .expect("run zolly bus")
}

#[test]
fn missing_radio_falls_back_to_herdr() {
    let base = temp_dir("missing");
    let root = base.join("repo");
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    office(&root);
    write_script(
        &bin,
        "herdr",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HERDR_LOG\"\nexit 0\n",
    );

    let out = run_bus(&root, &bin, &base.join("herdr.log"), &base.join("radio.log"));
    assert!(out.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let herdr = fs::read_to_string(base.join("herdr.log")).expect("herdr called");
    assert!(herdr.contains("agent prompt foreman-test"), "{herdr}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("radio unavailable"));
    assert!(!base.join("radio.log").exists());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn radio_is_the_first_choice() {
    let base = temp_dir("radio-wins");
    let root = base.join("repo");
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    office(&root);
    write_script(
        &bin,
        "radio",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$RADIO_LOG\"\nexit 0\n",
    );
    write_script(
        &bin,
        "herdr",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HERDR_LOG\"\nexit 0\n",
    );

    let out = run_bus(&root, &bin, &base.join("herdr.log"), &base.join("radio.log"));
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let radio = fs::read_to_string(base.join("radio.log")).expect("radio called");
    assert!(radio.contains("pm foreman"), "{radio}");
    assert!(!base.join("herdr.log").exists(), "herdr must not be used when radio succeeds");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn failing_radio_falls_back_to_herdr() {
    let base = temp_dir("radio-fails");
    let root = base.join("repo");
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    office(&root);
    write_script(&bin, "radio", "#!/bin/sh\nexit 1\n");
    write_script(
        &bin,
        "herdr",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HERDR_LOG\"\nexit 0\n",
    );

    let out = run_bus(&root, &bin, &base.join("herdr.log"), &base.join("radio.log"));
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let herdr = fs::read_to_string(base.join("herdr.log")).expect("herdr fallback called");
    assert!(herdr.contains("agent prompt foreman-test"), "{herdr}");
    let _ = fs::remove_dir_all(base);
}
