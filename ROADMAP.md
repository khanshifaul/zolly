# zolly roadmap

**Updated:** 2026-11-05
**Status vocabulary:** planned · in progress · done
**Plan of record:** workstream W below. The comparison that motivates it is [COMPARISON.md](COMPARISON.md); OpenBot (openbot.run) ships signed macOS, Windows, Linux, iOS and Android clients, and platform reach is the gap it exploits most directly.

## Windows support — planned

### Why now

The office is Linux-first by construction, not by design. Nothing in the control plane needs Linux: rules, personas, memory, briefs, results and the merge gate are files and git. The coupling is at the edges — process liveness, file modes, the service manager, one `timeout`/`sh` wrapper, and the POSIX shell scripts. OpenBot proves the demand: a local-first agent workspace is expected on Windows, where a founder's work machine usually is.

**Works today:** the whole office under WSL2 (Ubuntu 24.04 with systemd). That is the interim answer until W explicitly lands; the README says so.

### Measured blockers (2026-11-05)

| # | Blocker | Where | Why Windows fails | Direction |
|---|---|---|---|---|
| 1 | Unix-only file APIs | `tool/src/tg/serve.rs:16,174` (`OpenOptionsExt::mode(0600)`), `tool/src/nudge.rs:93` (`PermissionsExt`), four integration tests | The traits do not exist on Windows | `#[cfg(unix)]` the mode; Windows relies on the profile ACL and documents it |
| 2 | `/proc` liveness | `tool/src/tg/serve.rs:75-95` (`lock_holder_running` reads `/proc/<pid>/cmdline`) | No `/proc`; a lock left by a crashed listener can never be reclaimed | A `process` module: unix keeps `/proc`, Windows opens the pid and reads its image name; keep the "unreadable = alive" fail-safe |
| 3 | `systemctl` | `tool/src/main.rs:2730-2754` (doctor), `install.sh:1304-1390` (unit writing) | No systemd | Service backend behind one interface: systemd on Linux, Scheduled Task on Windows |
| 4 | `timeout 600 sh -c` | `tool/src/main.rs:1277` (`verify` runs the project test command) | Neither `timeout` nor `sh` exists | Platform runner: `sh -c` on unix, `cmd.exe /C` on Windows; enforce the deadline with a job object (`taskkill /T`) |
| 5 | POSIX shell scripts | `install.sh`, `test.sh`, `.agents/shared/memory`, `.agents/zolly/mcp/install.sh` | No `sh`/`bash` | Move the logic into the binary (`zolly memory`, `zolly mcp install`), add `install.ps1`, port `test.sh` to Python |
| 6 | Seat runtime | `herdr` pane/tab/worktree server, `omp` harness | Windows builds unconfirmed; both are terminal-oriented | Confirm builds in W1; otherwise Windows gets the control plane and seats stay in WSL2 |
| 7 | Voice | `tool/src/tg/send.rs` (`ffmpeg`, `voxtype`) | Linux binaries | `#[cfg(unix)]` the voice path; document voice as Linux-only |

### Phases

**W0 — compile gate (0.5 day)**
- Add `cargo check --target x86_64-pc-windows-msvc` to CI; keep Linux tests green.
- `#[cfg]`-gate blockers 1, 2 and 7 with clear `unsupported on this platform` errors; no behaviour change on Linux.
- Done when: the cross-check job is green and `cargo test` on Linux is unchanged.

**W1 — portable primitives (2–3 days)**
- New `tool/src/process.rs`: liveness probe with unix and Windows implementations; `ServeLock` and `doctor` use it.
- File modes: unix keeps `0600`; Windows is a documented no-op (profile ACL), with an ACL-hardening follow-up only if the office ever holds a secret file.
- Port the `/proc`-dependent tests under `cfg(unix)`; add the Windows equivalents where the behaviour is shared.
- Probe `herdr --help` and `omp --version` on a Windows runner and record the answer in this file. If both work, seats are in scope for W2; if not, W2 stops at the control plane.
- Done when: `cargo test` is green on `windows-latest`, and the probe result is written down.

**W2 — service and runtime boundary (2–4 days)**
- `tool/src/service.rs`: `install`, `remove`, `status` for a listener unit, implemented twice (systemd, Scheduled Task). `doctor` reports the backend it found.
- `install.ps1`: same flags and defaults as `install.sh`, same placeholder substitution, credentials under `%APPDATA%\zolly\<freq>`, MCP registration in `.omp/mcp.json`, `--force` backup semantics, `--dry-run`.
- `verify`: platform runner replaces `timeout 600 sh -c`; the deadline is enforced per platform.
- Done when: a fresh Windows checkout installs, `zolly doctor` passes, and the listener answers a signed webhook POST and a `getUpdates` poll.

**W3 — kit scripts become binary commands (2–3 days)**
- `zolly memory add|read|confirm|retract|compact` in Rust, preserving the 20/30/5 entries and 24 KB caps, atomic write and secret-pattern refusal. `.agents/shared/memory` stays as a unix wrapper for one release.
- `zolly mcp install --frequency <f> --root <dir>` replaces `.agents/zolly/mcp/install.sh`; the shell script stays as a wrapper.
- `test.sh` becomes `tests/run.py` (the same checks, minus the dash-only install smoke), so one command runs on all three operating systems.
- Done when: memory and MCP handshake pass on Windows, and Linux still runs the wrapper path.

**W4 — packaging and proof (2–3 days)**
- Release artifact `zolly-<version>-x86_64-pc-windows-msvc.zip`; MSI (cargo-wix) afterwards, signing a later gate.
- CI matrix: `ubuntu-latest`, `windows-latest`, `macos-latest` — macOS is compiled but not yet supported; the matrix makes its rot visible.
- Windows smoke: temp git repo → `install.ps1 --yes` → `doctor` → `brief` → listener POST → `close`.
- Done when: the artifact is published by the release workflow and the smoke job is green.

**W5 — documentation (0.5 day)**
- README: supported-platforms table, Windows install steps, WSL2 note, known limits (voice Linux-only; herdr/omp requirement).
- `.agents/rules/*` and personas: "systemd" becomes "service manager (systemd on Linux, Scheduled Task on Windows)" wherever the sentence is a rule, not a Linux fact.
- COMPARISON.md: mark the platform gap closed and re-date the OpenBot section.

### Dependencies and risks

- **`herdr`/`omp` on Windows is the gating dependency.** If neither ships a build, Windows support stops at the control plane — `brief`, `route`, `verify`, `close`, `doctor`, `telegram-serve`, `memory` — and seats stay in WSL2. W1 records the verdict so W2 does not guess.
- **CRLF.** Add `.gitattributes`: `*.sh text eol=lf`, `shared/memory text eol=lf`, `*.py text eol=lf`. A CRLF checkout breaks the memory CLI and the wrappers before any Rust code runs.
- **MAX_PATH.** Worktrees nest under `.agents/worktrees/<task-id>`; enable long paths in the manifest and use `\\?\` prefixes where Rust builds paths.
- **Console.** Spawning a TUI agent needs ConPTY. In scope only if W1 confirms `omp` on Windows; out of scope while seats are WSL2.
- **Defender.** Cache `target/` in CI; a cold cargo build on Windows is minutes, not seconds.

### Acceptance criteria (Windows is "supported" when all hold)

1. `cargo check --target x86_64-pc-windows-msvc` is a required CI check.
2. `cargo test` is green on `windows-latest` and unchanged on Linux and macOS.
3. `zolly doctor` on a Windows checkout reports no platform failure — service state, liveness, memory and MCP all answer.
4. `install.ps1` materialises `.agents/`, writes `config.toml`, registers the MCP server and the listener task, and a second run is idempotent.
5. The listener receives a signed webhook POST and a `getUpdates` poll on Windows, and survives a restart with the lock reclaimed.
6. `zolly close` completes its six-step sweep on Windows without `sh`, `timeout` or a POSIX shell on `PATH`.
7. Memory add/read/confirm enforces the same caps and refuses credential-shaped text on Windows.
8. README states the supported versions (Windows 10 1809+ and 11, x64 first) and the WSL2 fallback.

## After Windows

The rest of the priority order lives in [COMPARISON.md §10](COMPARISON.md#10-what-would-make-zolly-competitive). In order: a harness seam (ACP or `--worker-cmd`) so zolly can orchestrate more than omp; an enforced gate (branch protection + required checks + a worker sandbox); read-only web/mobile status; reliability evals; task fan-out.
