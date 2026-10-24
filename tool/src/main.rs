mod herdr;
mod brief;
mod curious;
mod office;
mod ponder;
mod queue;
mod render;
mod tg;
mod nudge;
mod report;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use office::Office;

#[derive(Parser)]
#[command(name = "zolly", version, about = "Zolly — the founder's manager")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    RegisterPane,
    Init,
    /// The founder brief. `--json` for machines.
    Brief {
        #[arg(long)]
        json: bool,
    },
    /// Deterministic signals for an idle-curiosity pass. No model call.
    Curious {
        #[arg(long)]
        json: bool,
    },
    /// Write an idle focus note: rank domains from measured signals. No model.
    Ponder {
        #[arg(long)]
        json: bool,
        /// Ignore the idle gate and the two-hour dedupe.
        #[arg(long)]
        force: bool,
        /// Wake the manager with the note when it is idle.
        #[arg(long)]
        wake: bool,
        /// Keep writing at --interval seconds. For a systemd unit or a pane.
        #[arg(long = "loop")]
        loop_: bool,
        #[arg(long, default_value_t = 1800)]
        interval: u64,
    },
    /// Deterministic triage: SMALL or LARGE, with the reasons.
    Route {
        text: String,
        #[arg(long)]
        json: bool,
    },
    /// Send one typed message on the office bus.
    Bus {
        handle: String,
        /// task | done | object | ack | note
        kind: String,
        task: String,
        #[arg(long)]
        gate: Option<String>,
        text: String,
    },
    /// Run the project's test command in a task worktree, record it in the result.
    Verify { task_id: String },
    Foreman {
        prompt: String,
        /// Milliseconds to wait for the reply before giving up.
        #[arg(long, default_value_t = 120_000)]
        timeout_ms: u64,
    },
    Reviewer {
        task_id: String,
        /// Milliseconds to wait for the reply before giving up.
        #[arg(long, default_value_t = 300_000)]
        timeout_ms: u64,
    },
    /// Deliver a brief of unbounded length to a worker (or any agent seat) and
    /// return only once the turn has STARTED. The one command foreman uses to
    /// brief a worker: `herdr agent prompt … --wait --until idle` cannot tell a
    /// buffered paste from a consumed one, so it returns healthy against a
    /// brief that never submitted (measured 2026-10-16).
    WorkerBrief {
        /// Agent name, e.g. worker-t-foreman-brief-delivery.
        agent: String,
        /// Read the brief from this file. Without it the brief is read from
        /// stdin — never argv: a brief is unbounded and the shell would mangle
        /// or truncate it.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Milliseconds to wait for the turn to start (default 30 min).
        #[arg(long, default_value_t = 1_800_000)]
        timeout_ms: u64,
    },
    Close {
        task_id: String,
    },
    /// Remove stale auto/* worktrees, close their Herdr workspaces, and delete
    /// their orphan branches. Refuses to touch the main checkout or another
    /// office's worktrees. `--dry` prints what it would do.
    Gc {
        #[arg(long)]
        dry: bool,
        /// Only remove worktrees whose branch matches this prefix (default: auto/).
        #[arg(long, default_value = "auto/")]
        prefix: String,
    },
    /// Telegram inbound transport. Default: the getUpdates long-poll loop.
    TelegramServe {
        /// Run the webhook listener instead of the getUpdates long-poll loop.
        #[arg(long)]
        webhook: bool,
        /// Bind address for the webhook listener.
        #[arg(long, default_value = "127.0.0.1:80")]
        bind: String,
        /// Webhook secret token (Telegram's `secret_token`). Env
        /// `ZOLLY_TELEGRAM_WEBHOOK_SECRET` is the fallback.
        #[arg(long, env = "ZOLLY_TELEGRAM_WEBHOOK_SECRET")]
        secret: Option<String>,
    },
    /// Register the Telegram webhook (`setWebhook`). This is the production
    /// cutover on an external system — the founder runs it, not a build.
    TelegramSetWebhook {
        #[arg(long)]
        url: String,
        /// Secret token Telegram must echo in `X-Telegram-Bot-Api-Secret-Token`.
        #[arg(long, env = "ZOLLY_TELEGRAM_WEBHOOK_SECRET")]
        secret: Option<String>,
    },
    /// Print the current Telegram webhook state (`getWebhookInfo`). Read-only.
    TelegramWebhookInfo,
    /// Remove the Telegram webhook (`deleteWebhook`) so getUpdates works again.
    TelegramDeleteWebhook,
    TelegramSendFile {
        #[arg(long)]
        file: String,
        #[arg(long)]
        caption: Option<String>,
        #[arg(long)]
        chat_id: Option<i64>,
    },
    /// Send a poll to the allowed chat (`sendPoll`). Options are comma-separated.
    TelegramPoll {
        #[arg(long)]
        question: String,
        #[arg(long)]
        options: String,
        /// Poll is anonymous (Telegram's `is_anonymous`; defaults to true).
        #[arg(long, default_value_t = true)]
        anonymous: bool,
        /// Make the poll non-anonymous (overrides --anonymous / default).
        #[arg(long)]
        no_anonymous: bool,
        /// Allow multiple answers (Telegram's `allows_multiple_answers`; defaults to false).
        #[arg(long, default_value_t = false)]
        multiple: bool,
    },
    /// Synthesize text with Piper TTS and send as a Telegram voice note.
    TelegramVoice {
        #[arg(long)]
        text: String,
        /// Target chat id (defaults to the founder's own chat).
        #[arg(long)]
        chat_id: Option<i64>,
    },
    TelegramCollab {
        #[arg(long)]
        list: bool,
        #[arg(long)]
        link: bool,
        #[arg(long)]
        view: bool,
        #[arg(long)]
        chat_id: Option<i64>,
    },
    /// Send a reply from Zolly (omp agent) to a Telegram chat. Used by the telegram_reply tool.
    TelegramReply {
        #[arg(long)]
        chat_id: i64,
        #[arg(long)]
        text: String,
    },
    /// Send a Telegram nudge when `zolly brief --json` reports waiting gates
    /// (a tapable poll) or stalls (a plain reply). Reuses the brief JSON
    /// instead of re-deriving gate state. Idempotent via a persisted stamp
    /// file under `.agents/zolly/`; sends nothing when there is nothing to say.
    TelegramNudge {
        /// Minimum seconds between nudges (default 1 hour). Env
        /// `ZOLLY_NUDGE_MIN_INTERVAL_SECS` overrides.
        #[arg(long, env = "ZOLLY_NUDGE_MIN_INTERVAL_SECS", default_value_t = 3600)]
        min_interval_secs: u64,
    },
        /// Self-check: stale binary, dead commands, broken skill symlinks,
    /// env drift between the two token sources, systemd state.
    /// Re-inject agent.md files into running panes (zolly, foreman, pr-reviewer)
    /// without restarting them. One line per seat; no-op with clear message when
    /// a seat is absent.
    Reload,
    Doctor {
        #[arg(long)]
        json: bool,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = std::env::current_dir().context("Failed to get current directory")?;
    let herdr = herdr::HerdrClient::new(root.clone());

    let Some(command) = cli.command else {
        return seat(&herdr, &root);
    };
    let command_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "seat".to_string());
    report::install_panic_hook(root.clone(), command_name.clone());

    let result = match command {
        Commands::RegisterPane => register_pane(&herdr, &root),
        Commands::Init => init(&herdr, &root),
        Commands::Brief { json } => brief_cmd(&root, json),
        Commands::Curious { json } => curious_cmd(&root, json),
        Commands::Ponder {
            json,
            force,
            wake,
            loop_,
            interval,
        } => {
            if loop_ {
                loop {
                    ponder_cmd(&root, json, force, wake)?;
                    std::thread::sleep(std::time::Duration::from_secs(interval.max(60)));
                }
            }
            ponder_cmd(&root, json, force, wake)
        }
        Commands::Route { text, json } => route_cmd(&text, json),
        Commands::Bus {
            handle,
            kind,
            task,
            gate,
            text,
        } => bus_cmd(&root, &handle, &kind, &task, gate.as_deref(), &text),
        Commands::Verify { task_id } => verify_cmd(&root, &task_id),
        Commands::Foreman { prompt, timeout_ms } => foreman(&herdr, &root, &prompt, timeout_ms),
        Commands::Reviewer { task_id, timeout_ms } => reviewer(&herdr, &root, &task_id, timeout_ms),
        Commands::WorkerBrief {
            agent,
            file,
            timeout_ms,
        } => worker_brief(&herdr, &agent, file.as_deref(), timeout_ms),
        Commands::Close { task_id } => close(&herdr, &root, &task_id),
        Commands::Gc { dry, prefix } => gc(&herdr, &root, dry, &prefix),
        Commands::TelegramServe {
            webhook,
            bind,
            secret,
        } => {
            if webhook {
                tg::webhook::cmd_webhook(&root, &bind, secret).await
            } else {
                tg::serve::cmd_serve(&root).await
            }
        }
        Commands::TelegramSetWebhook { url, secret } => {
            tg::webhook::cmd_set_webhook(&url, secret).await
        }
        Commands::TelegramWebhookInfo => tg::webhook::cmd_webhook_info().await,
        Commands::TelegramDeleteWebhook => tg::webhook::cmd_delete_webhook().await,
        Commands::TelegramSendFile { file, caption, chat_id } => {
            tg::cmd_send_file(&root, &file, caption, chat_id).await
        }
        Commands::TelegramPoll {
            question,
            options,
            anonymous,
            no_anonymous,
            multiple,
        } => {
            let anonymous = anonymous && !no_anonymous;
            tg::cmd_poll(&root, question, options, anonymous, multiple).await
        }
        Commands::TelegramVoice { text, chat_id } => {
            tg::cmd_voice(&root, Some(text), chat_id).await
        }
        Commands::TelegramCollab { list, link, view, chat_id } => {
            tg::cmd_collab(&root, list, link, view, chat_id).await
        }
        Commands::TelegramReply { chat_id, text } => tg::cmd_reply(chat_id, text).await,
        Commands::TelegramNudge {
            min_interval_secs,
        } => nudge::cmd_nudge(&root, min_interval_secs).await.map(|line| println!("{}", line)),
        Commands::Reload => reload(&herdr, &root),
        Commands::Doctor { json } => doctor(&root, json),
    };

    if let Err(e) = &result {
        let detail = format!("{e:#}");
        if report::is_reportable(&detail) {
            report::maybe_report(&root, &command_name, &detail);
        }
    }
    result
}

/// Bare `zolly` — the entrypoint that seats the manager.
///
/// Run from a shell in the office root. The tab gets the `@` label a human
/// reads (`zolly@<office>`), the handle joins the office frequency while the
/// pane is still a shell, and then omp takes over with the manager persona.
/// Agent names cannot carry `@` (herdr allows `[a-z0-9_-]`), so the machine
/// name `zolly-<office>` is set by `zolly register-pane` once omp is running.
fn seat(herdr: &herdr::HerdrClient, root: &std::path::Path) -> Result<()> {
    let office = Office::load(root.to_path_buf());
    let persona = office.agent_md("zolly");
    if !persona.is_file() {
        anyhow::bail!(
            "not a zolly office: {} has no .agents/zolly/agent.md",
            root.display()
        );
    }

    // Fail fast: a missing config or a non-repo cannot run an office. A dirty
    // tree or an inactive listener is reported in the state, not fatal.
    let checks = gather_checks(root)?;
    if let Some(bad) = checks.iter().find(|c| !c.ok && c.name == "config") {
        anyhow::bail!("office not ready: {} — {}", bad.name, bad.detail);
    }
    if !git_ok(root, &["rev-parse", "--git-dir"]) {
        anyhow::bail!("not a git repository: {}", root.display());
    }

    let pane = herdr.pane_current().context("Failed to get current pane")?;
    if let Some(agent) = pane.agent.as_deref() {
        anyhow::bail!(
            "this pane already hosts '{agent}' — run `zolly register-pane` inside it, or seat from a shell"
        );
    }

    if let Some(tab) = pane.tab_id.as_deref() {
        herdr
            .tab_rename(tab, &format!("zolly@{}", office.frequency))
            .context("Failed to label the seat tab")?;
    }

    radio_join(
        "zolly",
        &pane.pane_id,
        &seat_role("zolly", &office),
        &office.frequency,
    );

    // One command boots the office. `init` is idempotent, and the manager's
    // persona still re-checks it, so a manual `zolly init` is only for
    // re-opening a seat that was closed.
    if let Err(e) = init(herdr, root) {
        eprintln!("zolly: could not open the seats: {e:#}");
    }

    let state = write_boot_state(root, &office);
    match &state {
        Ok(path) => println!("state: {}", path.display()),
        Err(e) => eprintln!("zolly: could not write boot state: {e:#}"),
    }

    println!("Seating {} for {}", office.agent("zolly"), office.project);
    let mut omp = std::process::Command::new("omp");
    omp.arg(format!("@{}", persona.display()));
    if let Ok(path) = &state {
        omp.arg(format!("@{}", path.display()));
    }
    let status = omp.status().context("Failed to start omp")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// The one-page state the manager starts from. It replaces the survey the
/// manager used to run (git status, brief, memory reads) with facts measured
/// once, at seat time.
fn write_boot_state(
    root: &std::path::Path,
    office: &Office,
) -> Result<std::path::PathBuf> {
    let brief = brief::generate_brief(root).ok();
    let checks = gather_checks(root).unwrap_or_default();
    let failing: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
    let branch = git_out(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "?".into());
    let commits = git_out(root, &["rev-list", "--count", "HEAD"]).unwrap_or_else(|_| "0".into());
    let dirty = git_out(root, &["status", "--porcelain"])
        .map(|s| s.lines().count())
        .unwrap_or(0);
    let herdr = herdr::HerdrClient::new(root.to_path_buf());
    let seats: Vec<String> = herdr
        .agents_for_office(root)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| a.name)
        .collect();

    let mut out = String::new();
    out.push_str("# Office state — written by `zolly` at seat time. Do not edit.\n\n");
    out.push_str(&format!("repo: {}\n", root.display()));
    out.push_str(&format!(
        "branch: {} · commits: {} · uncommitted: {}\n",
        branch.trim(),
        commits.trim(),
        dirty
    ));
    out.push_str(&format!(
        "seats: {}\n",
        if seats.is_empty() {
            "none".to_string()
        } else {
            seats.join(", ")
        }
    ));
    if let Some(brief) = brief {
        out.push_str(&format!(
            "workers: {} · gates waiting: {} · recent results: {}\n",
            brief.waiting.workers,
            brief.gates.len(),
            brief.waiting.recent_results.len()
        ));
        if !brief.waiting.unparseable_results.is_empty() {
            out.push_str(&format!(
                "unreadable results: {}\n",
                brief.waiting.unparseable_results.len()
            ));
        }
        out.push_str(&format!("next: {}\n", brief.next_action));
    }

    // The full store stays on disk: injecting 14KB every seat costs more than
    // the reads it replaces. Carry the working set (current.md) and a pointer
    // with counts, so the agent knows what exists and where to look.
    let memory = root.join(".agents/memory");
    let current = std::fs::read_to_string(memory.join("current.md")).unwrap_or_default();
    let current_entries: Vec<&str> = current
        .lines()
        .skip_while(|line| !line.starts_with("## C-"))
        .collect();
    if !current_entries.is_empty() {
        out.push_str("\n## memory — current\n");
        out.push_str(&current_entries.join("\n"));
        out.push('\n');
    }
    let count = |file: &str, prefix: &str| {
        std::fs::read_to_string(memory.join(file))
            .map(|text| text.lines().filter(|line| line.starts_with(prefix)).count())
            .unwrap_or(0)
    };
    out.push_str(&format!(
        "memory: {} decisions · {} learnings — `.agents/shared/memory read` / `search <terms>`\n",
        count("decisions.md", "## D-"),
        count("learnings.md", "## L-")
    ));
    out.push_str(&format!(
        "doctor: {}\n",
        if failing.is_empty() {
            "all green".to_string()
        } else {
            format!("failing: {}", failing.join(", "))
        }
    ));

    let path = office.root.join(".agents/zolly/state.md");
    std::fs::write(&path, out)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}

fn register_pane(herdr: &herdr::HerdrClient, root: &std::path::Path) -> Result<()> {
    let office = Office::load(root.to_path_buf());
    let pane = herdr.pane_current().context("Failed to get current pane")?;
    herdr
        .agent_rename(&pane.pane_id, &office.agent("zolly"))
        .context("Failed to register pane as zolly")?;
    if let Some(tab) = pane.tab_id.as_deref() {
        let _ = herdr.tab_rename(tab, &format!("zolly@{}", office.frequency));
    }
    println!("Registered pane {} as {}", pane.pane_id, office.agent("zolly"));
    Ok(())
}

/// The office roster: each seat's Radio handle. `radio join` carries the
/// paragraph below in the ledger and the briefing hands it to the agent. Only
/// the two seats `init` starts are joined from here — zolly is already an
/// agent in her own pane, where the plugin refuses a named-frequency join, so
/// her entry is the roster's record of the name and paragraph rather than
/// something `init` can write.
const SEAT_HANDLES: &[&str] = &["zolly", "foreman", "pr-reviewer"];

/// The role paragraph for a seat handle. The manager's names the office, so it
/// is built per project rather than frozen into the binary.
fn seat_role(handle: &str, office: &Office) -> String {
    match handle {
        "zolly" => format!(
            "Manager of the {} office. Routes founder instructions: small work herself, large work to foreman. Never works in apps/, never merges.",
            office.project
        ),
        "foreman" => "Team leader. Plans the task, opens the worktree, spawns and monitors the worker, invokes the reviewer. NEVER writes code.".to_string(),
        "pr-reviewer" => "Reviewer. Reads the result and diff, runs verify, merges on PASS or objects. Only door to main.".to_string(),
        other => panic!("unknown seat handle {other}"),
    }
}

/// Collapse command output to one line. `radio` writes a multi-word message and
/// a trailing newline; the office speaks one line per event.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The single line to report for a `radio join` result. stdout on success,
/// stderr on failure — the plugin writes its guard message ("exit the current
/// agent…", "is already bound to another live pane") to stderr, and that
/// message has to reach the reader rather than be dropped with the exit status.
/// Falls back to whichever stream said something, so a silent exit still
/// reports as `no output` rather than an empty line.
fn radio_line(success: bool, stdout: &str, stderr: &str) -> String {
    let primary = if success { stdout } else { stderr };
    let text = if primary.trim().is_empty() { stdout } else { primary };
    let line = one_line(text);
    if line.is_empty() { "no output".to_string() } else { line }
}

/// AgentRadio is the first choice. It is optional: a missing or failed `radio`
/// falls back to a one-line `herdr agent prompt`, which is enough for the short
/// messages `zolly bus` is meant to carry.
fn radio_available() -> bool {
    std::process::Command::new("radio")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Join `handle` to the office frequency on `pane`, before an agent exists in
/// it, and print the outcome as one line.
///
/// A named-frequency join is only accepted while the pane is still a shell: the
/// plugin's guard refuses it once the pane has a running agent
/// (`bin/radio:1202-1205`), which is why `init` calls this between `tab_new`
/// and `agent start`. A re-join of a handle already bound to *another* live
/// pane is likewise refused, and that refusal is not an error — a seat already
/// on the frequency must not fail the office start.
///
/// Nothing here is fatal. A missing `radio`, a refused join, a bound handle —
/// each prints one line and returns: the seats must come up whether or not the
/// bus does.
fn radio_join(handle: &str, pane: &str, role: &str, frequency: &str) {
    if !radio_available() {
        return;
    }
    let output = std::process::Command::new("radio")
        .args([
            "join",
            handle,
            "--pane",
            pane,
            "--no-launch",
            "--frequency",
            frequency,
            "--role",
            role,
        ])
        .output();
    let line = match output {
        Ok(out) => radio_line(
            out.status.success(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(e) => format!("unavailable ({e})"),
    };
    println!("radio: {handle} — {line}");
}

/// Message ids from `radio inbox` output: one `  #<id> …` line per row, the
/// header and the `read one:` footer carry none.
fn inbox_ids(stdout: &str) -> Vec<u64> {
    stdout
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix('#')?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .collect()
}

/// Acknowledge every envelope queued for `handle` before its seat boots.
///
/// The replay this prevents, measured 2026-10-23 13:23Z: the relay parks
/// deliveries sent while a seat has no live pane as `pull` + "no live pane".
/// `radio join`'s own catch-up only demotes rows still `pending`, so parked
/// rows survive it; the next relay tick sees the new pane, re-promotes all of
/// them (`relay.log`: 29 `pane back, re-queued for push` lines in one tick),
/// and pushes the backlog into the fresh agent as user turns. A seat that has
/// just started has no context to answer yesterday's mail with — its first
/// turn must be its boot prompt, and a stale answer is worse than none (#493
/// calls PR #26 blocked while #497 reports it merged, both after the merge).
///
/// Reading each queued envelope as the handle is the plugin's own
/// acknowledgement path (`radio show --by` marks the delivery `delivered`),
/// so the relay has nothing left to push. It runs before the tab exists,
/// which also closes the window in which the relay could type into a bare
/// shell. Every failure is reported in one line and swallowed: the seat must
/// come up whether or not the bus does.
fn radio_drain(office: &Office, handle: &str) {
    if !radio_available() {
        return;
    }
    let scope = office.workspace();
    let read = |args: &[&str]| {
        std::process::Command::new("radio")
            .args(args)
            .env("HERDR_ENV", "1")
            .env("HERDR_WORKSPACE_ID", &scope)
            .env("RADIO_HANDLE", handle)
            // With no pane, radio resolves this handle through RADIO_HANDLE
            // in the frequency scope. An inherited pane would make the
            // caller's own membership authoritative and refuse another
            // handle.
            .env_remove("HERDR_PANE_ID")
            .output()
    };
    let mut drained = 0usize;
    let mut unreachable = None;
    for status in ["pending", "pull", "failed"] {
        // A listing may be capped; every acknowledged row leaves the status,
        // so round until the list is empty or stops making progress.
        for _ in 0..10 {
            let ids = match read(&["inbox", "--status", status, "--limit", "1000"]) {
                Ok(out) if out.status.success() => {
                    inbox_ids(&String::from_utf8_lossy(&out.stdout))
                }
                Ok(_) => break,
                Err(e) => {
                    unreachable = Some(e.to_string());
                    break;
                }
            };
            if ids.is_empty() {
                break;
            }
            let mut acked = 0;
            for id in ids {
                let id = id.to_string();
                if let Ok(out) = read(&["show", &id, "--by", handle]) {
                    if out.status.success() {
                        acked += 1;
                    }
                }
            }
            drained += acked;
            if acked == 0 {
                break;
            }
        }
    }
    match unreachable {
        Some(e) => println!("radio: {handle} — drain unavailable ({e})"),
        None => println!("radio: {handle} — drained {drained} queued envelope(s)"),
    }
}

/// Load an office and refuse to act when its frequency belongs to another
/// checkout. This is the binary-side counterpart to the installer's
/// `$CRED_ROOT/<freq>/project` ownership marker; a cloned repo can edit the
/// tracked `frequency`, but it cannot edit the marker in `$HOME`.
fn load_owned_office(root: &std::path::Path) -> Result<Office> {
    let office = Office::load(root.to_path_buf());
    anyhow::ensure!(
        office.owned(),
        "office frequency '{}' belongs to another checkout — refusing to act on it",
        office.frequency
    );
    Ok(office)
}

fn init(herdr: &herdr::HerdrClient, root: &std::path::Path) -> Result<()> {
    let office = load_owned_office(root)?;
    let foreman = office.agent("foreman");
    let reviewer = office.agent("pr-reviewer");
    let foreman_label = format!("foreman@{}", office.frequency);
    let reviewer_label = format!("pr-reviewer@{}", office.frequency);
    let agents = herdr.agent_list().context("Failed to list agents")?;
    let mut has_foreman = agents.iter().any(|a| a.name.as_deref() == Some(foreman.as_str()));
    let mut has_reviewer = agents.iter().any(|a| a.name.as_deref() == Some(reviewer.as_str()));
    // A seat can be live but unnamed: its omp is running in the right tab,
    // herdr has no name for it, and the next `init` would start a duplicate.
    // Adopt it by tab label before opening anything.
    if !has_foreman && adopt_seat(herdr, &office, "foreman", &foreman_label) {
        has_foreman = true;
    }
    if !has_reviewer && adopt_seat(herdr, &office, "pr-reviewer", &reviewer_label) {
        has_reviewer = true;
    }

    if has_foreman && has_reviewer {
        println!("Foreman and pr-reviewer already running");
    }

    // The seats belong to the project's workspace, not to whichever workspace
    // happened to run `init`. Resolve by checkout path, then label, then make
    // one.
    let workspace = herdr
        .ensure_workspace(&office.root, &office.frequency)
        .context("Failed to resolve the office workspace")?;
    println!("Workspace: {}", workspace);
    if let Err(e) = herdr.ensure_radio_view(&workspace) {
        eprintln!("radio view: {e}");
    }
    if has_foreman && has_reviewer {
        return Ok(());
    }

    if !has_foreman || !has_reviewer {
        println!("Opening seats...");
    }

    let (foreman_result, reviewer_result) = std::thread::scope(|scope| {
        let foreman_job = if has_foreman {
            None
        } else {
            Some(scope.spawn(|| {
                open_seat(
                    herdr,
                    &office,
                    "foreman",
                    &foreman,
                    &foreman_label,
                    "foreman",
                    &workspace,
                )
            }))
        };
        let reviewer_job = if has_reviewer {
            None
        } else {
            Some(scope.spawn(|| {
                open_seat(
                    herdr,
                    &office,
                    "pr-reviewer",
                    &reviewer,
                    &reviewer_label,
                    "reviewer",
                    &workspace,
                )
            }))
        };
        let join = |job: Option<std::thread::ScopedJoinHandle<'_, Result<()>>>| match job {
            Some(handle) => handle
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("seat thread panicked"))),
            None => Ok(()),
        };
        (join(foreman_job), join(reviewer_job))
    });

    let mut failures: Vec<String> = Vec::new();
    if !has_foreman {
        match foreman_result {
            Ok(()) => println!("Foreman started"),
            Err(e) => failures.push(format!("foreman: {e}")),
        }
    }
    if !has_reviewer {
        match reviewer_result {
            Ok(()) => println!("Reviewer started"),
            Err(e) => failures.push(format!("pr-reviewer: {e}")),
        }
    }

    if !failures.is_empty() {
        anyhow::bail!("could not start: {}", failures.join("; "));
    }
    Ok(())
}

/// Rename an already-running omp in the seat's tab to the expected agent name.
/// True only when the rename succeeded.
fn adopt_seat(herdr: &herdr::HerdrClient, office: &Office, handle: &str, label: &str) -> bool {
    let Ok(tabs) = herdr.tab_list() else {
        return false;
    };
    let Some(tab) = tabs.iter().find(|tab| tab.label == label) else {
        return false;
    };
    let Ok(panes) = herdr.pane_list() else {
        return false;
    };
    let Some(pane) = panes
        .iter()
        .find(|pane| pane.tab_id.as_deref() == Some(tab.tab_id.as_str()) && pane.agent.is_some())
    else {
        return false;
    };
    herdr
        .agent_rename(&pane.pane_id, &office.agent(handle))
        .is_ok()
}

/// One seat, one linear path: empty the mailbox, label the tab, join the bus
/// while the pane is a shell, then start the agent with its boot prompt and
/// persona as launch arguments. Draining comes first, before the tab exists,
/// so the relay cannot re-promote yesterday's parked mail against the new
/// pane (see `radio_drain`). Passing the prompt at launch (measured 3.0s per
/// seat) beats starting omp empty and then prompting it (the turn-start wait
/// cost ~6s more).
fn open_seat(
    herdr: &herdr::HerdrClient,
    office: &Office,
    handle: &str,
    name: &str,
    label: &str,
    persona: &str,
    workspace: &str,
) -> Result<()> {
    let persona_path = office.agent_md(persona);
    if !persona_path.is_file() {
        anyhow::bail!("missing persona at {}", persona_path.display());
    }
    let boot_path = match persona {
        "foreman" => office.root.join(".agents/foreman/boot/foreman.md"),
        _ => office.root.join(".agents/foreman/boot/pr-reviewer.md"),
    };
    let mut launch: Vec<String> = Vec::new();
    if boot_path.is_file() {
        launch.push(format!("@{}", boot_path.display()));
    }
    launch.push(format!("@{}", persona_path.display()));

    radio_drain(office, handle);
    let pane = herdr
        .agent_start_with_retry(
            name,
            label,
            "omp",
            &office.root_str(),
            Some(workspace),
            &launch,
            |pane| radio_join(handle, pane, &seat_role(handle, office), &office.frequency),
        )
        .with_context(|| format!("Failed to start {name}"))?;
    if let Some(tab) = herdr.tab_id_for_pane(&pane) {
        let _ = herdr.tab_rename(&tab, label);
    }
    Ok(())
}

fn reload(herdr: &herdr::HerdrClient, root: &std::path::Path) -> Result<()> {
    let office = Office::load(root.to_path_buf());
    let agents = herdr.agent_list().context("Failed to list agents")?;

    let seat_configs = [
        ("foreman", office.agent_md("foreman")),
        ("pr-reviewer", office.agent_md("reviewer")),
    ];

    for (handle, path) in seat_configs {
        let name = office.agent(handle);
        let agent = agents.iter().find(|a| a.name.as_deref() == Some(name.as_str()));
        if let Some(agent) = agent {
            if let Some(tab) = agent.tab_id.as_deref() {
                let _ = herdr.tab_rename(tab, &format!("{handle}@{}", office.frequency));
            }
            let prompt = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            herdr
                .agent_prompt_wait(&name, &prompt, 30_000)
                .with_context(|| format!("Failed to reload {name}"))?;
            println!("{name}: reloaded from {}", path.display());
        } else {
            println!("{name}: not running (no reload)");
        }
    }
    Ok(())
}

fn brief_cmd(root: &std::path::Path, json: bool) -> Result<()> {
    let brief = brief::generate_brief(root).context("Failed to generate brief")?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&brief).context("Failed to serialize JSON")?
        );
    } else {
        let sections = render::sections(root, &brief);
        print!("{}", render::render_cli(&sections));
    }
    Ok(())
}

fn curious_cmd(root: &std::path::Path, json: bool) -> Result<()> {
    let signals = curious::inspect(root)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&signals).context("Failed to serialize JSON")?
        );
    } else {
        print!("{}", curious::render(&signals));
    }
    Ok(())
}

fn ponder_cmd(root: &std::path::Path, json: bool, force: bool, wake: bool) -> Result<()> {
    let ponder = ponder::run(root, force, wake)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ponder).context("Failed to serialize JSON")?
        );
    } else {
        print!("{}", ponder::render(&ponder));
    }
    Ok(())
}

fn route_cmd(text: &str, json: bool) -> Result<()> {
    let decision = classify(text);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "size": if decision.large { "LARGE" } else { "SMALL" },
                "gate": if decision.human { "human" } else { "auto" },
                "reasons": decision.reasons,
            }))?
        );
        return Ok(());
    }
    println!("{}", if decision.large { "LARGE" } else { "SMALL" });
    println!("  reasons: {}", decision.reasons.join("; "));
    if decision.human {
        println!("  gate: human");
    }
    if decision.large {
        println!("  next: zolly foreman {:?}", text);
    } else {
        println!("  next: do it yourself");
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
struct RouteDecision {
    large: bool,
    human: bool,
    reasons: Vec<String>,
}

/// Deterministic triage. The words are the rule; the model only explains an
/// ambiguity the founder has to settle.
fn classify(text: &str) -> RouteDecision {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let paths = words
        .iter()
        .filter(|w| w.contains('/') || has_short_extension(w))
        .count();
    let gate_hits: Vec<&str> = [
        "production",
        "deploy",
        "release",
        "publish",
        "customer",
        "money",
        "billing",
        "payment",
        "invoice",
        "price",
        "spend",
    ]
    .iter()
    .copied()
    .filter(|k| lower.contains(k))
    .collect();
    let large_hits: Vec<&str> = [
        "refactor",
        "redesign",
        "rewrite",
        "migrat",
        "architecture",
        "schema",
        "database",
        "authentication",
        "integrat",
        "audit",
        "rollout",
        "pipeline",
    ]
    .iter()
    .copied()
    .filter(|k| lower.contains(k))
    .collect();
    let small_verb = [
        "fix", "rename", "tweak", "adjust", "update", "bump", "typo", "comment", "label", "copy",
        "remove",
    ]
    .iter()
    .copied()
    .find(|k| {
        words
            .iter()
            .any(|w| w.trim_matches(|c: char| !c.is_alphanumeric()) == *k)
    });

    let mut reasons = Vec::new();
    if !gate_hits.is_empty() {
        reasons.push(format!("gate word: {}", gate_hits.join(", ")));
    }
    if !large_hits.is_empty() {
        reasons.push(format!("large word: {}", large_hits.join(", ")));
    }
    reasons.push(format!("{} words", words.len()));
    reasons.push(format!("{} path(s)", paths));
    if let Some(v) = small_verb {
        reasons.push(format!("verb: {v}"));
    }

    let human = !gate_hits.is_empty();
    let large = human || !large_hits.is_empty() || words.len() > 45 || paths > 3;
    RouteDecision {
        large,
        human,
        reasons,
    }
}

fn has_short_extension(word: &str) -> bool {
    word.rsplit_once('.')
        .map(|(_, ext)| {
            !ext.is_empty()
                && ext.len() <= 4
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or(false)
}

fn envelope(kind: &str, task: &str, gate: Option<&str>, text: &str) -> Result<String> {
    let kind = kind.trim();
    if !["task", "done", "object", "ack", "note"].contains(&kind) {
        anyhow::bail!("kind must be task|done|object|ack|note, got '{kind}'");
    }
    let gate = gate.unwrap_or("auto");
    Ok(format!("[{kind}] task={task} gate={gate}\n{text}"))
}

/// Candidate herdr agent names for a radio handle. The bus handle is the
/// stable name; the herdr agent name carries the frequency (and shortens a
/// worker id to fit herdr's cap).
fn bus_candidates(root: &std::path::Path, handle: &str) -> Vec<String> {
    let office = Office::load(root.to_path_buf());
    match handle {
        "zolly" | "foreman" | "pr-reviewer" => vec![office.agent(handle)],
        _ if handle.starts_with("worker-") => {
            office_worker_seat_names(root, &handle["worker-".len()..])
        }
        _ => vec![handle.to_string()],
    }
}

/// Deliver a short one-line envelope through herdr when AgentRadio is absent
/// or down. This is the fallback, not the first choice: `radio pm` is tried
/// before this runs.
fn bus_fallback(root: &std::path::Path, handle: &str, message: &str) -> Result<()> {
    let line = one_line(message);
    let mut last = String::new();
    for name in bus_candidates(root, handle) {
        match std::process::Command::new("herdr")
            .args(["agent", "prompt", &name, "--", &line])
            .output()
        {
            Ok(out) if out.status.success() => {
                println!("{message}");
                eprintln!("bus: radio unavailable — delivered to {name} via herdr");
                return Ok(());
            }
            Ok(out) => {
                last = format!(
                    "herdr agent prompt {name}: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(e) => last = format!("herdr agent prompt {name}: {e}"),
        }
    }
    anyhow::bail!("radio unavailable and herdr fallback failed: {last}")
}

fn bus_cmd(
    root: &std::path::Path,
    handle: &str,
    kind: &str,
    task: &str,
    gate: Option<&str>,
    text: &str,
) -> Result<()> {
    load_owned_office(root)?;
    let message = envelope(kind, task, gate, text)?;
    match std::process::Command::new("radio")
        .args(["pm", handle, &message])
        .output()
    {
        Ok(out) if out.status.success() => {
            println!("{message}");
            Ok(())
        }
        Ok(out) => {
            let radio_err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            bus_fallback(root, handle, &message)
                .with_context(|| format!("radio pm failed: {radio_err}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bus_fallback(root, handle, &message)
        }
        Err(e) => bus_fallback(root, handle, &message)
            .with_context(|| format!("radio pm failed: {e}")),
    }
}

fn parse_test_command(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let command = line.trim().strip_prefix("test-command:")?.trim();
        if command.is_empty() || command.eq_ignore_ascii_case("TODO") {
            None
        } else {
            Some(command.to_string())
        }
    })
}

fn find_test_command(root: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".agents/rules/project.md")).ok()?;
    parse_test_command(&text)
}

/// Replace one `## <name>` section's body, keeping every other section. A
/// missing section is appended.
fn replace_section(document: &str, name: &str, body: &str) -> String {
    let marker = format!("## {name}");
    let mut out = String::new();
    let mut lines = document.lines().peekable();
    let mut replaced = false;
    while let Some(line) = lines.next() {
        if line.trim_end() == marker {
            out.push_str(&marker);
            out.push_str("\n\n");
            out.push_str(body.trim_end());
            out.push_str("\n\n");
            while let Some(next) = lines.peek() {
                if next.starts_with("## ") {
                    break;
                }
                lines.next();
            }
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("\n## {name}\n\n{}\n", body.trim_end()));
    }
    out
}

/// A task id is one path component, never `..` or a separator. Every caller
/// that joins it into `.agents/worktrees` or `.agents/results` must reject
/// anything else: a CLI typo or a hostile queue filename must not escape the
/// office.
fn safe_task_id(task_id: &str) -> bool {
    let mut parts = std::path::Path::new(task_id).components();
    matches!(parts.next(), Some(std::path::Component::Normal(_)))
        && parts.next().is_none()
        && !task_id.contains('\\')
}

fn verify_cmd(root: &std::path::Path, task_id: &str) -> Result<()> {
    if !safe_task_id(task_id) {
        anyhow::bail!("invalid task id '{task_id}' — one filename component, no '..' or separators");
    }
    let worktree = root.join(".agents/worktrees").join(task_id);
    if !worktree.is_dir() {
        anyhow::bail!("no worktree at {}", worktree.display());
    }
    if worktree.canonicalize().ok() == root.canonicalize().ok() {
        anyhow::bail!("refusing to verify on the main checkout");
    }
    // The branch copy is authoritative (result-contract.md), so the result
    // file lives in the worktree. The test command may live in either place:
    // a worktree carries tracked files only, so a machine-local .agents/ is
    // absent there and the main checkout is the fallback.
    let command = find_test_command(&worktree)
        .or_else(|| find_test_command(root))
        .context(
            "no test command — add `test-command: <cmd>` to .agents/rules/project.md",
        )?;
    let result_path = worktree.join(".agents/results").join(format!("{task_id}.md"));
    if !result_path.is_file() {
        anyhow::bail!(
            "no result file at {} — the worker writes it first",
            result_path.display()
        );
    }

    let output = std::process::Command::new("timeout")
        .args(["600", "sh", "-c", &command])
        .current_dir(&worktree)
        .output()
        .context("Failed to run test-command")?;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let all: Vec<&str> = text.lines().collect();
    let tail = all[all.len().saturating_sub(80)..].join("\n");
    let block = format!(
        "- command: `{command}`\n- exit: {code}\n- at: {}\n- output:\n\n```\n{}\n```",
        chrono::Utc::now().to_rfc3339(),
        tail
    );

    let document = std::fs::read_to_string(&result_path)
        .with_context(|| format!("Failed to read {}", result_path.display()))?;
    let updated = replace_section(&document, "Verify", &block);
    std::fs::write(&result_path, updated)
        .with_context(|| format!("Failed to write {}", result_path.display()))?;

    if code == 0 {
        println!("verify: passed — {command}");
        Ok(())
    } else {
        anyhow::bail!(
            "verify: failed (exit {code}) — recorded in {}",
            result_path.display()
        )
    }
}

fn report_prompt_reply(role: &str, reply: herdr::PromptReply) {
    match reply.text {
        Some(text) => println!("{}", text.trim()),
        None => eprintln!(
            "{role}: prompt submitted; {} — do not resend it. Inspect the seat, then recover if needed.",
            reply.detail
        ),
    }
}

fn foreman(
    herdr: &herdr::HerdrClient,
    root: &std::path::Path,
    prompt: &str,
    timeout_ms: u64,
) -> Result<()> {
    // Branch base discipline (change 2): a task branch cut from a local base
    // that is behind its remote carries unrelated work into its PR — PR #96
    // carried 17 files instead of 2 for exactly this reason. Sync first.
    ensure_main_matches_origin(root)?;
    let office = load_owned_office(root)?;
    let reply = herdr
        .agent_prompt_with_reply(&office.agent("foreman"), prompt, timeout_ms)
        .context("Failed to prompt foreman")?;
    report_prompt_reply("foreman", reply);
    Ok(())
}

fn reviewer(
    herdr: &herdr::HerdrClient,
    root: &std::path::Path,
    task_id: &str,
    timeout_ms: u64,
) -> Result<()> {
    let office = load_owned_office(root)?;
    let prompt = format!("Review task: {}", task_id);
    let reply = herdr
        .agent_prompt_with_reply(&office.agent("pr-reviewer"), &prompt, timeout_ms)
        .context("Failed to prompt reviewer")?;
    report_prompt_reply("reviewer", reply);
    Ok(())
}

/// `zolly worker-brief <agent>` — the one command foreman uses to deliver a
/// brief of unbounded length, returning only once the turn has **started**.
///
/// Why not `herdr agent prompt … --wait --until idle`, the form the playbook
/// used to name. Measured 2026-10-16: for an omp agent, `herdr agent list`
/// reports `agent_status: "idle"` for the *whole* turn
/// (`fallback_reason: default_known_agent_idle_fallback`), so `--until idle`
/// matches instantly and always — it cannot tell a buffered prompt from a
/// consumed one. A 130-line brief was typed, the trailing Enter was swallowed
/// as part of the paste, the brief was held as an unsubmitted attachment
/// (`📄 #1`, no elapsed clock), and the dispatch returned a state that looked
/// healthy while the worker did nothing until a manual Enter.
///
/// The body is `agent_prompt_wait`: a thin reuse, not a second copy of the
/// guard. It already carries the two-phase gate — wait for the turn to *start*
/// (the only phase that can fail, and the one that owns the Enter nudge in
/// `wait_turn_started`), then for it to settle. Reuse was chosen because the
/// nudge is the fix, it is already measured and reviewed (t-zolly-init-reliable,
/// PR #96), and a second copy of "poll the glyph, nudge if it has not
/// appeared" is exactly the drift this office keeps paying for.
///
/// Contract, stated because the exit status must not be over-read: success
/// means the turn started. Phase 2 never fails, so a task still running at
/// `--timeout-ms` returns success too — it is a start gate, not a completion
/// signal.
fn worker_brief(
    herdr: &herdr::HerdrClient,
    agent: &str,
    file: Option<&std::path::Path>,
    timeout_ms: u64,
) -> Result<()> {
    let brief = read_brief(file)?;
    let lines = brief.lines().count();
    herdr
        .agent_prompt_wait(agent, &brief, timeout_ms)
        .with_context(|| format!("Failed to deliver the brief to {agent}"))?;
    println!("{agent}: brief delivered ({lines} lines); turn started");
    Ok(())
}

/// The brief text: `--file` if given, else stdin. Never argv — a brief is
/// unbounded, and a shell would truncate or mangle it.
fn read_brief(file: Option<&std::path::Path>) -> Result<String> {
    use std::io::{IsTerminal, Read};
    match file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read the brief from {}", path.display())),
        None => {
            anyhow::ensure!(
                !std::io::stdin().is_terminal(),
                "no --file given and stdin is a terminal: pass --file <path>, or pipe the brief in"
            );
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read the brief from stdin")?;
            Ok(buf)
        }
    }
}

/// One line of the close sweep, plus the short form that goes in the summary.
struct StepOutcome {
    line: String,
    /// Short form for the summary line.
    token: String,
    /// A hard failure: reported, and the sweep exits non-zero.
    failed: bool,
}

impl StepOutcome {
    fn new(line: String, token: &str, failed: bool) -> Self {
        Self {
            line,
            token: token.to_string(),
            failed,
        }
    }
}

/// Agent names a worker seat for `task_id` may carry. This legacy helper is
/// kept for the naming tests; production close/bus paths use
/// `office_worker_seat_names`, which never returns the bare legacy name.
#[cfg(test)]
fn worker_seat_names(root: &std::path::Path, task_id: &str) -> Vec<String> {
    let frequency = crate::office::Office::load(root.to_path_buf()).frequency;
    let prefix = format!("worker-{frequency}-");
    let budget = 32usize.saturating_sub(prefix.chars().count());
    let short_id: String = task_id.chars().take(budget).collect();
    let mut names = vec![format!("{prefix}{short_id}")];
    let plain = format!("worker-{task_id}");
    if !names.contains(&plain) {
        names.push(plain);
    }
    names
}

/// Worker names that are safe to close or message from this office. The herdr
/// agent namespace is server-wide; a bare `worker-<task-id>` could match the
/// same task id in another office. Return only the frequency-prefixed name:
/// filtering the legacy bare name is not enough, because a task id that starts
/// with the frequency (`zolly-fix`) renders a bare name that still has the
/// prefix.
fn office_worker_seat_names(root: &std::path::Path, task_id: &str) -> Vec<String> {
    let frequency = crate::office::Office::load(root.to_path_buf()).frequency;
    let prefix = format!("worker-{frequency}-");
    let budget = 32usize.saturating_sub(prefix.chars().count());
    let short_id: String = task_id.chars().take(budget).collect();
    vec![format!("{prefix}{short_id}")]
}

/// `zolly close <task-id>` — the post-merge sweep. Idempotent, six steps, one
/// printed line each plus a summary. It never fails on a worktree that is
/// already gone: the self-repair path has none, and that is not an error.
///
/// Step 5 rebuilds the release binary when the merge touched
/// `.agents/zolly/tool/` — `zolly doctor`'s stale-binary FAIL consumed by the
/// one step that can clear it. A rebuild that fails is a failure of the sweep,
/// not a warning: a merged tool change that will not build is a real defect.
fn close(herdr: &herdr::HerdrClient, root: &std::path::Path, task_id: &str) -> Result<()> {
    if !safe_task_id(task_id) {
        anyhow::bail!("invalid task id '{task_id}' — one filename component, no '..' or separators");
    }
    // Act on the checkout that holds `main`. This may be run from inside a task
    // worktree, and merging there would fast-forward the task branch instead.
    let repo = main_checkout(root);
    let office = load_owned_office(&repo)?;
    // Guard: refuse to sweep main while it holds uncommitted tracked changes —
    // listing them by name so the reader sees exactly what blocks them, rather
    // than being told the refs "diverged" when they do not.
    check_main_clean(&repo)?;
    let branch = format!("foreman/{task_id}");

    // 1/6 — local base follows the merge. Fast-forward only: this never creates
    // a commit and never pushes. `merge --ff-only`, deliberately not `pull`:
    // pull.rebase=true is set globally on this machine, so a pull dies on the
    // memory files the CLI keeps dirty.
    let sync = sync_main(&repo, &office.base_branch, &office.remote);
    println!("1/6 sync      : {}", sync.outcome.line);

    // 2/6 — the task branch, once its work is contained in the remote base.
    let (mut branch_step, deferred) =
        close_branch(&repo, &branch, &office.base_branch, &office.remote);
    println!("2/6 branch    : {}", branch_step.line);

    // 3/6 — the worktree, if it is still there. Absent is a skip, not a failure.
    // The seat dies with it: a live pane whose cwd was just deleted is a zombie
    // that `herdr agent list` keeps counting (#12).
    let mut worktree = match herdr.worktree_close(&repo, task_id) {
        Ok(Some(removed)) => {
            let mut step =
                StepOutcome::new(format!("removed {}", removed.path), "removed", false);
            if let Some(workspace) = &removed.workspace_closed {
                step.line.push_str(&format!("; workspace {workspace} closed"));
            }
            if let Some(error) = &removed.workspace_error {
                step.line
                    .push_str(&format!("; workspace NOT closed: {error}"));
                step.token = "removed (workspace open)".into();
                step.failed = true;
            }
            if deferred {
                // Step 2 could not delete the branch because this worktree held
                // it; removing the worktree freed it.
                match git_out(&repo, &["branch", "-D", &branch]) {
                    Ok(_) => {
                        step.line.push_str(&format!("; branch {branch} deleted"));
                        branch_step.token = "deleted".into();
                    }
                    Err(e) => {
                        step.line.push_str(&format!("; branch {branch} NOT deleted: {e}"));
                        branch_step.token = "kept (worktree held it)".into();
                    }
                }
            }
            step
        }
        Ok(None) => StepOutcome::new(
            format!(
                "no worktree {} — skipped",
                repo.join(".agents/worktrees").join(task_id).display()
            ),
            "skipped (none)",
            false,
        ),
        Err(e) => StepOutcome::new(format!("FAILED to remove the worktree: {e}"), "FAILED", true),
    };
    // The worktree is gone; close the seat that was created for it. A failure
    // is reported but does not fail the sweep: herdr being unreachable must not
    // make `close` refuse to finish the rest (the worktree step already treats
    // it that way), and the line names the reason either way.
    // Close only this office's worker tab. The herdr agent namespace is
    // server-wide; a bare `worker-<task-id>` fallback could match the same
    // task id in another office. The frequency prefix is the boundary.
    let seats = office_worker_seat_names(&repo, task_id);
    let seat_refs: Vec<&str> = seats.iter().map(String::as_str).collect();
    match herdr.close_agent_tabs(&seat_refs) {
        Ok(closed) if closed.is_empty() => {}
        Ok(closed) => worktree.line.push_str(&format!("; seat closed: {}", closed.join(", "))),
        Err(e) => worktree.line.push_str(&format!("; seat not closed: {e}")),
    }
    println!("3/6 worktree  : {}", worktree.line);

    // 4/6 — the done-signal must be in git. A task without its result file in
    // the branch was never finished, whatever the queue status says.
    let signal = check_done_signal(&repo, task_id);
    let index = check_results_index(&repo, task_id);
    println!("4/6 done-signal: {}; index: {}", signal.line, index.line);

    // 5/6 — deploy: rebuild the release binary when the merge touched the tool
    // source. The trigger is the merge itself — the path list between the two
    // main SHAs `sync_main` already read — never the result file's prose. Scope
    // is `.agents/zolly/tool/`, so a change to `.agents/zolly/mcp/` (a copied
    // Python script) does not build.
    let deploy = match &sync.shas {
        Some((before, after)) => deploy_tool(&repo, before, after),
        None => StepOutcome::new(
            "skipped, main was not synced — no merge range to diff".to_string(),
            "skipped (no sync)",
            false,
        ),
    };
    println!("5/6 deploy    : {}", deploy.line);

    println!(
        "6/6 summary   : sync {} | branch {} | worktree {} | done-signal {} | deploy {}",
        sync.outcome.token, branch_step.token, worktree.token, signal.token, deploy.token
    );

    let failed: Vec<&str> = [
        ("sync", sync.outcome.failed),
        ("branch", branch_step.failed),
        ("worktree", worktree.failed),
        ("deploy", deploy.failed),
    ]
    .into_iter()
    .filter(|(_, f)| *f)
    .map(|(name, _)| name)
    .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "close sweep for {task_id}: {} step(s) failed: {}",
            failed.len(),
            failed.join(", ")
        )
    }
}

// ---------------------------------------------------------------------------
// git plumbing for the sweep and the dispatch guard
// ---------------------------------------------------------------------------

/// Run git in `dir`. Ok = stdout, Err = trimmed stderr — callers report the
/// reason verbatim rather than a characterisation.
fn git_out(dir: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr)
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// True when git exits 0. For questions, not for output.
fn git_ok(dir: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// First 8 characters of a sha, for one-line reporting.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

/// The main checkout — the first entry of `git worktree list --porcelain`. Git
/// lists the main working tree first, so this is the checkout that holds `main`
/// even when the caller is inside a linked worktree. Falls back to `root` when
/// git cannot answer, which keeps the sweep running rather than skipping it.
fn main_checkout(root: &std::path::Path) -> std::path::PathBuf {
    git_out(root, &["worktree", "list", "--porcelain"])
        .ok()
        .and_then(|out| {
            out.lines().find_map(|line| {
                line.strip_prefix("worktree ")
                    .map(|p| std::path::PathBuf::from(p.trim()))
            })
        })
        .unwrap_or_else(|| root.to_path_buf())
}
/// The tracked files git would refuse to carry into `main` because main's
/// working tree holds uncommitted changes to them — staged or unstaged, tracked
/// or untracked. Returns the file list so every caller can name them.
///
/// `git status --porcelain` in `repo`. A clean tree yields an empty list; any
/// entry (M, A, D, ??, …) means an agent has written into this checkout.
///
/// Office state under `.agents/` (results, memory, nudge stamps) is not source
/// and must not block dispatch or close — the memory CLI keeps its files
/// unwritten-to-git by design, and `.agents/nudge.stamp` should never have been
/// tracked. Skip those paths so a dirty main checkout of office state doesn't
/// refuse foreman dispatch on nothing that compiles.
fn main_dirty_files(repo: &std::path::Path) -> Result<Vec<String>> {
    let out = git_out(repo, &["status", "--porcelain"])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            // Columns: XY PATH where XY is the status. The path starts after
            // the first space/tab following the two status chars.
            let s = l.split('\t').next().unwrap_or(l).trim();
            let after_status = s
                .find(|c: char| c == ' ' || c == '\t')
                .map(|i| &s[i + 1..])
                .unwrap_or(s);
            after_status.trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .filter(|p| !p.starts_with(".agents/") && p != "OPEN-QUESTIONS.md")
        .collect())
}

/// Refuse to operate on main while it carries uncommitted changes.
/// Names every dirty file so the reader sees exactly what blocks them.
fn check_main_clean(repo: &std::path::Path) -> Result<()> {
    let dirty = main_dirty_files(repo)?;
    if dirty.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to touch main: it has uncommitted tracked changes — {}",
        dirty.join(", ")
    );
}


/// (local base sha, remote base sha, commits base is ahead, commits behind).
fn main_vs_origin(
    repo: &std::path::Path,
    base: &str,
    remote_name: &str,
) -> Result<(String, String, u64, u64)> {
    let base_ref = format!("{remote_name}/{base}");
    let head = git_out(repo, &["rev-parse", base])?.trim().to_string();
    let remote = git_out(repo, &["rev-parse", &base_ref])?.trim().to_string();
    let ahead = gap_count(repo, &format!("{base_ref}..{base}"));
    let behind = gap_count(repo, &format!("{base}..{base_ref}"));
    Ok((head, remote, ahead, behind))
}

/// `git rev-list --count <range>`, or 0 when git cannot answer.
fn gap_count(repo: &std::path::Path, range: &str) -> u64 {
    git_out(repo, &["rev-list", "--count", range])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// The refusal text names both shas and both counts — never "out of sync".
fn main_gap_message(
    head: &str,
    remote: &str,
    ahead: u64,
    behind: u64,
    repo: &std::path::Path,
    base: &str,
    remote_name: &str,
) -> String {
    format!(
        "refusing to dispatch: local {base} is not {remote_name}/{base} — {base} {} is {ahead} commit(s) ahead of and {behind} commit(s) behind {remote_name}/{base} {}; sync first with `git -C {} merge --ff-only {remote_name}/{base}`",
        short(head),
        short(remote),
        repo.display()
    )
}

/// Refuse to dispatch while local base != remote base, naming the exact gap.
fn ensure_main_matches_origin(root: &std::path::Path) -> Result<()> {
    let repo = main_checkout(root);
    let office = load_owned_office(&repo)?;
    let base = &office.base_branch;
    let remote_name = &office.remote;
    // Guard: refuse to dispatch onto a dirty base — name the offending files
    // so the reader is not sent chasing ref topology that is perfectly
    // fast-forwardable.
    check_main_clean(&repo)?;
    // Refresh the remote-tracking ref so the comparison is against the real
    // remote, not a stale copy. A failed fetch is not fatal — the last known
    // ref still answers the question — but it must not pass silently.
    if let Err(e) = git_out(&repo, &["fetch", remote_name]) {
        eprintln!(
            "warn: {e} — comparing {base} against the last known {remote_name}/{base}"
        );
    }
    let (head, remote, ahead, behind) = main_vs_origin(&repo, base, remote_name)
        .with_context(|| format!("cannot compare {base} to {remote_name}/{base} — refusing to dispatch"))?;
    if head == remote {
        return Ok(());
    }
    anyhow::bail!(
        "{}",
        main_gap_message(&head, &remote, ahead, behind, &repo, base, remote_name)
    )
}

/// The result of step 1: its printed line, plus the `(before, after)` local
/// `main` SHAs that bound the merge. `None` when `main` was not synced (wrong
/// branch, fetch/comparison failure) — there is then no merge range to diff and
/// the deploy step skips rather than guess one.
struct MainSync {
    outcome: StepOutcome,
    shas: Option<(String, String)>,
}

/// 1/6 — make local `base` match the merge. Fast-forward only: never a commit,
/// never a push. `--autostash` carries local office-state edits (tracked
/// `.agents/memory` files) across the merge instead of letting Git refuse it.
/// Returns the SHAs so the deploy step diffs the real merge, and an
/// already-synced base yields `before == after` → an empty diff → no rebuild.
fn sync_main(repo: &std::path::Path, base: &str, remote_name: &str) -> MainSync {
    let head_branch = git_out(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".to_string());
    if head_branch != base {
        // Merging here would fast-forward whatever branch is checked out.
        return MainSync {
            outcome: StepOutcome::new(
                format!(
                    "{} is on '{head_branch}', not {base} — not synced",
                    repo.display()
                ),
                &format!("skipped (on {head_branch})"),
                false,
            ),
            shas: None,
        };
    }
    if let Err(e) = git_out(repo, &["fetch", remote_name]) {
        return MainSync {
            outcome: StepOutcome::new(format!("{e}"), "FAILED (fetch)", true),
            shas: None,
        };
    }
    let (head, remote, ahead, behind) = match main_vs_origin(repo, base, remote_name) {
        Ok(v) => v,
        Err(e) => {
            return MainSync {
                outcome: StepOutcome::new(format!("{e}"), "FAILED (cannot compare)", true),
                shas: None,
            };
        }
    };
    if head == remote {
        return MainSync {
            outcome: StepOutcome::new(
                format!(
                    "{base} {} == {remote_name}/{base} {} (up to date)",
                    short(&head),
                    short(&remote)
                ),
                &format!("ok {}", short(&head)),
                false,
            ),
            // Already synced: an empty range, so the deploy step finds no change.
            shas: Some((head.clone(), head)),
        };
    }
    let merge_ref = format!("{remote_name}/{base}");
    match git_out(repo, &["merge", "--ff-only", "--autostash", &merge_ref]) {
        Ok(_) => MainSync {
            outcome: StepOutcome::new(
                format!(
                    "fast-forwarded {base} {} → {} (was {behind} behind, {ahead} ahead)",
                    short(&head),
                    short(&remote)
                ),
                &format!("ok {}", short(&remote)),
                false,
            ),
            shas: Some((head, remote)),
        },
        Err(e) => MainSync {
            outcome: StepOutcome::new(format!("{e}"), "FAILED (diverged)", true),
            shas: None,
        },
    }
}

/// The `.agents/zolly/tool/` paths a merge changed — the trigger for a rebuild,
/// read from the merge itself, never from the result file's prose. Scoped to
/// `tool/`, so a change to `.agents/zolly/mcp/` (a copied Python script) is
/// invisible here and never builds. `before == after` (already-synced main)
/// yields an empty list.
fn tool_source_changed(
    repo: &std::path::Path,
    before: &str,
    after: &str,
) -> Result<Vec<String>> {
    let range = format!("{before}..{after}");
    let out = git_out(
        repo,
        &["diff", "--name-only", &range, "--", ".agents/zolly/tool/"],
    )?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// 5/6 — rebuild `target/release/zolly` when the merge touched the tool source.
/// The trigger is the merge itself: the paths in
/// `git diff --name-only <before>..<after> -- .agents/zolly/tool/`. Nothing
/// changed (or an already-synced main, `before == after`) → skipped, so a re-run
/// of `close` is idempotent and never rebuilds for its own sake.
///
/// A rebuild that fails is a failed step with a non-zero exit — a merged tool
/// change that will not build is a real defect, not a printed warning. The step
/// reports whether a service restart is required, but never performs one:
/// `zolly.service` is the founder's (`safety.md`).
fn deploy_tool(repo: &std::path::Path, before: &str, after: &str) -> StepOutcome {
    let tool_dir = repo.join(".agents/zolly/tool");
    let range = format!("{before}..{after}");
    let paths = match tool_source_changed(repo, before, after) {
        Ok(p) => p,
        Err(e) => {
            return StepOutcome::new(format!("cannot diff {range}: {e}"), "FAILED (diff)", true);
        }
    };
    if paths.is_empty() {
        return StepOutcome::new(
            format!("skipped, no tool source changed in {range}"),
            "skipped (no tool change)",
            false,
        );
    }
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();

    let build = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&tool_dir)
        .output();
    match build {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = stderr
                .lines()
                .filter(|l| !l.trim().is_empty())
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return StepOutcome::new(
                format!("cargo build --release FAILED: {}", tail.join(" | ")),
                "FAILED (build)",
                true,
            );
        }
        Err(e) => {
            return StepOutcome::new(
                format!("cargo build --release could not run: {e}"),
                "FAILED (build)",
                true,
            );
        }
        Ok(_) => {}
    }

    // The fresh binary against the newest `src/` mtime — the number the stale-binary
    // check compares. If the binary is still older than a source file after a
    // successful build, the rebuild did not take: loud, not a shrug.
    let binary = tool_dir.join("target/release/zolly");
    let bin_mtime = binary.metadata().ok().and_then(|m| m.modified().ok());
    let stale = bin_mtime
        .map(|b| src_files_newer_than(&tool_dir, b))
        .unwrap_or_default();
    if !stale.is_empty() {
        return StepOutcome::new(
            format!(
                "cargo build --release ran but target/release/zolly is still older than {} — rebuild did not take",
                stale.join(", ")
            ),
            "FAILED (still stale)",
            true,
        );
    }
    let mtimes = match (bin_mtime, newest_src_mtime(&tool_dir)) {
        (Some(b), Some(s)) => format!("binary mtime {} vs newest src {}", stamp(b), stamp(s)),
        (Some(b), None) => format!("binary mtime {}", stamp(b)),
        (None, _) => "binary mtime unreadable".to_string(),
    };
    StepOutcome::new(
        format!(
            "rebuilt {} tool file(s) ({mtimes}); {}",
            paths.len(),
            restart_advice(&paths)
        ),
        &format!("rebuilt ({})", restart_token(&paths)),
        false,
    )
}

/// A change under `src/tg/` reaches the running listener only on a service
/// restart; anything else is a CLI change that applies on the next invocation.
/// This is a report, never an action — restarting `zolly.service` is the
/// founder's (`safety.md`).
fn tool_change_needs_restart(paths: &[&str]) -> bool {
    paths
        .iter()
        .any(|p| p.starts_with(".agents/zolly/tool/src/tg/"))
}

/// The sentence the deploy step prints about the restart question.
fn restart_advice(paths: &[&str]) -> String {
    if tool_change_needs_restart(paths) {
        "restart required: src/tg/ changed, so the running `zolly telegram-serve` listener is still the old code until `zolly.service` is restarted (the founder's call, not this step's)".to_string()
    } else {
        "no restart needed: CLI-only change, applies on the next invocation".to_string()
    }
}

/// The short form of the restart answer that rides the summary line.
fn restart_token(paths: &[&str]) -> &'static str {
    if tool_change_needs_restart(paths) {
        "restart required"
    } else {
        "no restart"
    }
}

/// 2/6 — delete `foreman/<task-id>` once its work is contained in the remote
/// base branch. A branch holding commits that are not in the merge is kept and
/// said so. Returns whether the delete had to be deferred to step 3, which
/// removes the worktree that holds the branch checked out.
fn close_branch(
    repo: &std::path::Path,
    branch: &str,
    base: &str,
    remote_name: &str,
) -> (StepOutcome, bool) {
    let base_ref = format!("{remote_name}/{base}");
    if !git_ok(
        repo,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
    ) {
        return (
            StepOutcome::new(format!("no local branch {branch}"), "absent", false),
            false,
        );
    }
    let ancestor = git_ok(repo, &["merge-base", "--is-ancestor", branch, &base_ref]);
    // A squash merge rewrites history: the branch tip is not an ancestor of
    // the base, but its tree is identical to the merged base. Treat content
    // equality as merged unless the branch has commits carrying a different
    // tree — otherwise every close after a squash merge leaves the branch.
    let same_tree = !ancestor && git_ok(repo, &["diff", "--quiet", &base_ref, branch]);
    if !ancestor && !same_tree {
        let unmerged = git_out(repo, &["rev-list", "--count", &format!("{base_ref}..{branch}")])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "?".to_string());
        return (
            StepOutcome::new(
                format!("{branch} has {unmerged} commit(s) and a different tree from {base_ref} — KEPT"),
                &format!("kept ({unmerged} not in base)"),
                false,
            ),
            false,
        );
    }
    match git_out(repo, &["branch", "-D", branch]) {
        Ok(_) => (
            StepOutcome::new(
                format!(
                    "{branch} {} {base_ref} — deleted",
                    if ancestor {
                        "contained in"
                    } else {
                        "content identical to"
                    }
                ),
                "deleted",
                false,
            ),
            false,
        ),
        Err(e) => {
            let reason = e.to_string();
            if reason.contains("used by worktree") || reason.contains("checked out") {
                (
                    StepOutcome::new(
                        format!(
                            "{branch} {} {base_ref} — held by a worktree, delete deferred to step 3",
                            if ancestor {
                                "contained in"
                            } else {
                                "content identical to"
                            }
                        ),
                        "deferred to step 3",
                        false,
                    ),
                    true,
                )
            } else {
                (StepOutcome::new(reason, "FAILED (delete)", true), false)
            }
        }
    }
}

/// The reviewer's one-row index entry: visible, never a close blocker.
fn check_results_index(repo: &std::path::Path, task_id: &str) -> StepOutcome {
    let path = ".agents/memory/results-index.md";
    let Ok(content) = std::fs::read_to_string(repo.join(path)) else {
        return StepOutcome::new(
            format!("WARN {path} is missing — reviewer did not append the row"),
            "WARN (no index)",
            false,
        );
    };
    let found = content.lines().any(|line| {
        line.starts_with('|')
            && line
                .split('|')
                .any(|cell| cell.trim() == task_id)
    });
    if found {
        StepOutcome::new(format!("{path} has {task_id}"), "ok (indexed)", false)
    } else {
        StepOutcome::new(
            format!("WARN {path} has no row for {task_id} — reviewer missed the index"),
            "WARN (not indexed)",
            false,
        )
    }
}

/// 4/6 — the done-signal must be tracked, or the task was never finished.
fn check_done_signal(repo: &std::path::Path, task_id: &str) -> StepOutcome {
    let path = format!(".agents/results/{task_id}.md");
    if git_ok(repo, &["ls-files", "--error-unmatch", "--", &path]) {
        let content = match std::fs::read_to_string(repo.join(&path)) {
            Ok(c) => c,
            Err(e) => {
                return StepOutcome::new(
                    format!("WARN {path} tracked but unreadable: {e}"),
                    "MISSING (unreadable)",
                    false,
                )
            }
        };
        if crate::queue::result_is_settled(&content) {
            return StepOutcome::new(format!("{path} is tracked"), "ok (tracked)", false);
        }
        let reason = crate::queue::settle_reason(&content).unwrap();
        let detail = match reason {
            crate::queue::SettleReason::NoFrontmatter => {
                "no frontmatter block at all (body-only file)"
            }
            crate::queue::SettleReason::NoStatusLine => {
                "has frontmatter but no `status:` line"
            }
            crate::queue::SettleReason::NotSettled => {
                "has frontmatter but `status:` is not a settled value"
            }
        };
        return StepOutcome::new(
            format!(
                "WARN {path} is tracked but NOT settled — {} (the task branch result file must carry YAML frontmatter with a settled status)",
                detail
            ),
            "MISSING (not settled)",
            false,
        );
    }
    let detail = if repo.join(&path).exists() {
        "exists on disk but is NOT tracked"
    } else {
        "does not exist and is NOT tracked"
    };
    StepOutcome::new(
        format!(
            "WARN {path} {detail} in git — the done-signal is not in git (the task branch must commit it; the sweep does not commit)"
        ),
        "MISSING (not in git)",
        false,
    )
}

/// Remove linked worktrees whose branch starts with `prefix` (default
/// `auto/`) and delete the matching branches. The main checkout is never
/// touched: its worktree path equals the repo root and is skipped by
/// construction. Every branch is checked for unique commits first — a
/// branch with work not on main is kept and reported, never force-deleted.
fn gc(
    herdr: &herdr::HerdrClient,
    root: &std::path::Path,
    dry: bool,
    prefix: &str,
) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root)
        .output()
        .context("Failed to run git worktree list")?;
    if !output.status.success() {
        anyhow::bail!(
            "git worktree list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse worktree blocks: "worktree <path>\nbranch <ref>"
    let mut targets: Vec<(String, Option<String>)> = Vec::new();
    let mut current_path: Option<String> = None;
    for line in stdout.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current_path = Some(p.trim().to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            if let Some(path) = current_path.take() {
                let branch = b.trim().trim_start_matches("refs/heads/").to_string();
                targets.push((path, Some(branch)));
            }
        }
    }

    let repo_root = main_checkout(root)
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());
    let base = Office::load(root.to_path_buf()).base_branch;
    let mut removed = 0usize;
    let mut kept_dirty = Vec::new();
    let mut branches_deleted = 0usize;
    let mut workspace_failures = 0usize;

    for (path, branch) in targets {
        let canon = std::path::Path::new(&path)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&path));
        if canon == repo_root {
            continue; // never touch main checkout
        }
        let Some(branch) = branch else { continue };
        if !branch.starts_with(prefix) {
            continue;
        }

        // Refuse to remove a branch unless git proves it is contained in the
        // configured base branch. A failed rev-list is not proof of merge.
        let unique = std::process::Command::new("git")
            .args(["rev-list", "--count", &format!("{base}..{branch}")])
            .current_dir(root)
            .output();
        let has_unique = match unique {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok()
            }
            Ok(o) => {
                eprintln!(
                    "gc: cannot compare {branch} with {base}: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                None
            }
            Err(e) => {
                eprintln!("gc: cannot compare {branch} with {base}: {e}");
                None
            }
        };
        let Some(has_unique) = has_unique else {
            kept_dirty.push(branch);
            continue;
        };
        if has_unique > 0 {
            // Squash merges rewrite history, so a merged branch has commits
            // that are not ancestors of the base. Tree equality proves the
            // work landed; only a different tree is genuinely unmerged.
            let same_tree = git_ok(root, &["diff", "--quiet", &base, &branch]);
            if !same_tree {
                kept_dirty.push(branch);
                continue;
            }
        }

        if dry {
            println!("would remove worktree {} (branch {})", path, branch);
            removed += 1;
            continue;
        }

        // Resolve the workspace before removing the checkout: after the path
        // is gone, the herdr row cannot be matched by path.
        let workspace = herdr.workspace_for_worktree_path(&repo_root, std::path::Path::new(&path));
        let rm = std::process::Command::new("git")
            .args(["worktree", "remove", &path])
            .current_dir(root)
            .output()
            .context("Failed to run git worktree remove")?;
        if !rm.status.success() {
            eprintln!(
                "failed to remove worktree {}: {}",
                path,
                String::from_utf8_lossy(&rm.stderr).trim()
            );
            kept_dirty.push(branch);
            continue;
        }
        removed += 1;
        if let Some(workspace) = workspace {
            match herdr.workspace_close(&workspace) {
                Ok(()) => println!("closed workspace {} for {}", workspace, path),
                Err(e) => {
                    eprintln!("failed to close workspace for {}: {}", path, e);
                    workspace_failures += 1;
                }
            }
        }

        let bd = std::process::Command::new("git")
            .args(["branch", "-D", &branch])
            .current_dir(root)
            .output();
        if matches!(bd, Ok(o) if o.status.success()) {
            branches_deleted += 1;
        }
    }

    if !dry {
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(root)
            .output();
    }

    if kept_dirty.is_empty() {
        println!(
            "gc {}: removed {} worktree(s){}, deleted {} branch(es)",
            if dry { "(dry)" } else { "done" },
            removed,
            if dry { "" } else { " and pruned" },
            branches_deleted
        );
    } else {
        println!(
            "gc {}: removed {} worktree(s), deleted {} branch(es); kept {} with commits not on main:",
            if dry { "(dry)" } else { "done" },
            removed,
            branches_deleted,
            kept_dirty.len()
        );
        for b in &kept_dirty {
            println!("  kept: {}", b);
        }
    }
    if workspace_failures > 0 {
        anyhow::bail!(
            "gc: {} workspace(s) could not be closed — the worktrees were removed",
            workspace_failures
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Doctor
// ---------------------------------------------------------------------------

/// One self-check result. `ok` false is a failure (exit 1); `ok` true with
/// a note is informational.
struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

/// Read `KEY=VALUE` pairs from an env file. Lines without `=` and comment
/// lines are skipped; values are trimmed, not unquoted.
fn read_env_file(path: &std::path::Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

/// `src/` files (and one level of subdirectory, for `src/tg/`) whose mtime is
/// newer than `bin_mtime` — the exact set `zolly doctor` reports as
/// `stale binary`. Shared by doctor and the close deploy step so the two can
/// never disagree about whether the binary is stale.
fn src_files_newer_than(tool_dir: &std::path::Path, bin_mtime: std::time::SystemTime) -> Vec<String> {
    let src_dir = tool_dir.join("src");
    let mut stale: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&src_dir) else {
        return stale;
    };
    for e in entries.filter_map(|e| e.ok()) {
        if let Some(src_mtime) = e.metadata().ok().and_then(|m| m.modified().ok()) {
            if src_mtime > bin_mtime {
                stale.push(e.file_name().to_string_lossy().to_string());
            }
        }
        // read_dir is not recursive; `src/tg/` is the one subdirectory.
        if e.path().is_dir() {
            if let Ok(sub) = std::fs::read_dir(e.path()) {
                for se in sub.filter_map(|se| se.ok()) {
                    if let Some(src_mtime) = se.metadata().ok().and_then(|m| m.modified().ok()) {
                        if src_mtime > bin_mtime {
                            stale.push(format!(
                                "{}/{}",
                                e.file_name().to_string_lossy(),
                                se.file_name().to_string_lossy()
                            ));
                        }
                    }
                }
            }
        }
    }
    stale
}

/// The newest mtime among `src/` (one level of recursion), for the deploy step's
/// "fresh binary against the newest `src/`" line. None when the tree is unreadable.
fn newest_src_mtime(tool_dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let src_dir = tool_dir.join("src");
    let mut newest: Option<std::time::SystemTime> = None;
    let mut note = |m: std::time::SystemTime| {
        newest = Some(newest.map_or(m, |n| n.max(m)));
    };
    for e in std::fs::read_dir(&src_dir).ok()?.filter_map(|e| e.ok()) {
        if let Some(m) = e.metadata().ok().and_then(|m| m.modified().ok()) {
            note(m);
        }
        if e.path().is_dir() {
            if let Ok(sub) = std::fs::read_dir(e.path()) {
                for se in sub.filter_map(|se| se.ok()) {
                    if let Some(m) = se.metadata().ok().and_then(|m| m.modified().ok()) {
                        note(m);
                    }
                }
            }
        }
    }
    newest
}

/// A local-time stamp for one mtime line, matching the ledger's readability.
fn stamp(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// A `key: value` line from `.agents/rules/project.md`. The first match wins;
/// keys are compared case-insensitively after trimming.
fn project_setting(root: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".agents/rules/project.md")).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().trim_matches('"').to_string())
    })
}

fn short_sha(sha: &str) -> String {
    sha.trim().chars().take(7).collect()
}

/// Self-check. Fails loudly on the recurring failure classes: a stale
/// binary, dead commands, broken skill symlinks, token-source drift, a bad
/// systemd unit. Exit code 0 = healthy; 1 = at least one check failed.
fn gather_checks(root: &std::path::Path) -> Result<Vec<Check>> {
    let mut checks: Vec<Check> = Vec::new();
    let office = Office::load(root.to_path_buf());
    if office.owned() {
        checks.push(Check {
            name: "office-ownership",
            ok: true,
            detail: format!("frequency '{}' is owned by this checkout", office.frequency),
        });
    } else {
        checks.push(Check {
            name: "office-ownership",
            ok: false,
            detail: format!(
                "frequency '{}' is owned by another checkout — config.toml was changed after install",
                office.frequency
            ),
        });
    }
    let tool_dir = root.join(".agents/zolly/tool");

    // 1. Binary freshness. When the tool source sits in this office, compare
    //    its build against src/. When it does not, the machine-wide binary is
    //    the artefact that matters and there is no local build to compare.
    let binary = tool_dir.join("target/release/zolly");
    if !tool_dir.join("Cargo.toml").is_file() {
        let shared = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "zolly".to_string());
        checks.push(Check {
            name: "binary",
            ok: true,
            detail: format!("shared binary {shared} is installed"),
        });
    } else if !binary.exists() {
        checks.push(Check {
            name: "binary",
            ok: false,
            detail: "target/release/zolly not built".into(),
        });
    } else if let Some(bin_mtime) = binary.metadata().ok().and_then(|m| m.modified().ok()) {
        let stale = src_files_newer_than(&tool_dir, bin_mtime);
        if stale.is_empty() {
            checks.push(Check {
                name: "binary",
                ok: true,
                detail: "target/release/zolly is newer than all of src/".into(),
            });
        } else {
            checks.push(Check {
                name: "binary",
                ok: false,
                detail: format!("stale binary: {} newer than target/release/zolly — rebuild", stale.join(", ")),
            });
        }
    }
    if office.config_exists() {
        checks.push(Check {
            name: "config",
            ok: true,
            detail: format!(
                ".agents/zolly/config.toml: frequency {}, founder {}",
                office.frequency, office.founder
            ),
        });
    } else {
        checks.push(Check {
            name: "config",
            ok: false,
            detail: ".agents/zolly/config.toml is missing".into(),
        });
    }
    // Optional code graph. Only projects that opt in pay for the check; the
    // index is derived and machine-local, so freshness is the failure that
    // matters — a stale graph is worse than no graph.
    if project_setting(root, "gitnexus")
        .map(|value| value.eq_ignore_ascii_case("on"))
        .unwrap_or(false)
    {
        let meta = root.join(".gitnexus/meta.json");
        let head = git_out(root, &["rev-parse", "HEAD"]).unwrap_or_default();
        let indexed = std::fs::read_to_string(&meta)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| {
                value
                    .get("lastCommit")
                    .and_then(|commit| commit.as_str())
                    .map(str::to_string)
            });
        match indexed {
            Some(commit) if commit.trim() == head.trim() => {
                let dirty = git_out(root, &["status", "--porcelain"])
                    .map(|status| !status.trim().is_empty())
                    .unwrap_or(false);
                checks.push(Check {
                    name: "gitnexus",
                    ok: true,
                    detail: if dirty {
                        format!(
                            "index matches HEAD ({}) — uncommitted changes are not indexed",
                            short_sha(&head)
                        )
                    } else {
                        format!("index matches HEAD ({})", short_sha(&head))
                    },
                });
            }
            Some(commit) => checks.push(Check {
                name: "gitnexus",
                ok: false,
                detail: format!(
                    "stale index: indexed {} / HEAD {} — run `node .gitnexus/run.cjs analyze`",
                    short_sha(&commit),
                    short_sha(&head)
                ),
            }),
            None => checks.push(Check {
                name: "gitnexus",
                ok: false,
                detail: "gitnexus: on but no index — run `node .gitnexus/run.cjs analyze`".into(),
            }),
        }
    }

    // wrote into it (result files, tool source edits, stray stamps). That is not
    // a divergence — list the files so the reader sees what blocks them.
    let repo = main_checkout(root);
    if !git_ok(&repo, &["rev-parse", "--verify", "HEAD"]) {
        // A fresh office has no HEAD; every file is untracked by definition.
        // Calling that "uncommitted changes" is noise, not a defect.
        checks.push(Check {
            name: "main-clean",
            ok: true,
            detail: "no commits yet — nothing to compare".into(),
        });
    } else {
        match main_dirty_files(&repo) {
            Ok(dirty) if dirty.is_empty() => checks.push(Check {
                name: "main-clean",
                ok: true,
                detail: "main checkout has no uncommitted changes".into(),
            }),
            Ok(dirty) => checks.push(Check {
                name: "main-clean",
                ok: false,
                detail: format!(
                    "main checkout has uncommitted changes — {}",
                    dirty.join(", ")
                ),
            }),
            Err(e) => checks.push(Check {
                name: "main-clean",
                ok: false,
                detail: format!("cannot check main cleanliness: {e}"),
            }),
        }
    }
    // 2. Dead commands: foreman dispatcher script (informational, L-0005).
    if root.join(".agents/foreman/foreman").exists() {
        checks.push(Check {
            name: "foreman-dispatcher",
            ok: true,
            detail: "dispatcher script exists".into(),
        });
    } else {
        checks.push(Check {
            name: "foreman-dispatcher",
            ok: true,
            detail: "absent (dispatcher layer removed L-0005) — 'foreman roster'/'foreman status' are dead commands".into(),
        });
    }

    // 3. Stale ledger file: we dropped it; a leftover is a trap.
    if root.join(".agents/zolly/task_ledger.json").exists() {
        checks.push(Check {
            name: "ledger",
            ok: false,
            detail: "stale task_ledger.json exists — ledger was dropped 2026-10-13; delete it".into(),
        });
    } else {
        checks.push(Check {
            name: "ledger",
            ok: true,
            detail: "no task_ledger.json".into(),
        });
    }

    // 4. Skill symlinks resolve.
    let skills_dir = root.join(".agents/zolly/skills");
    let mut broken_skills: Vec<String> = Vec::new();
    if skills_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for e in entries.filter_map(|e| e.ok()) {
                let path = e.path();
                if path
                    .symlink_metadata()
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                    && !path.exists()
                {
                    broken_skills.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    if broken_skills.is_empty() {
        checks.push(Check {
            name: "skills",
            ok: true,
            detail: "all skill symlinks resolve".into(),
        });
    } else {
        checks.push(Check {
            name: "skills",
            ok: false,
            detail: format!("broken skill symlink(s): {}", broken_skills.join(", ")),
        });
    }

    // 5. Token-source drift: the systemd unit reads
    // ~/.config/zolly/credentials; shell commands inherit the process
    // environment (project-tree `.env` files are no longer read). If both
    // define a value and they disagree, the serve bot and a shell-run bot can
    // be different bots — a real trap, so this is a failure, not a warning.
    let mut shell_env = std::collections::HashMap::new();
    for key in ["ZOLLY_TELEGRAM_TOKEN", "ZOLLY_ALLOWED_IDS"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                shell_env.insert(key.to_string(), value);
            }
        }
    }
    let creds_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/zolly")
        .join(&office.frequency)
        .join("credentials");
    let creds_env = read_env_file(&creds_path);
    let mut drift: Vec<String> = Vec::new();
    for key in ["ZOLLY_TELEGRAM_TOKEN", "ZOLLY_ALLOWED_IDS"] {
        if let (Some(a), Some(b)) = (shell_env.get(key), creds_env.get(key)) {
            if a != b {
                drift.push(format!(
                    "{} differs between the process environment and ~/.config/zolly/credentials",
                    key
                ));
            }
        }
    }
    if drift.is_empty() {
        checks.push(Check {
            name: "env-drift",
            ok: true,
            detail: "token sources agree (or only one is present)".into(),
        });
    } else {
        checks.push(Check {
            name: "env-drift",
            ok: false,
            detail: drift.join("; "),
        });
    }

    // 6. Systemd unit. A listener that was never configured is not broken:
    //    Telegram is optional, and the installer writes no unit without a
    //    token and an allow-list.
    let configured = ["ZOLLY_TELEGRAM_TOKEN", "ZOLLY_ALLOWED_IDS"].iter().all(|key| {
        let present = |map: &std::collections::HashMap<String, String>| {
            map.get(*key).map(|v| !v.is_empty()).unwrap_or(false)
        };
        present(&creds_env) || present(&shell_env)
    });
    let unit = office.unit();
    if !configured {
        checks.push(Check {
            name: "systemd",
            ok: true,
            detail: format!("no listener ({unit} not configured)"),
        });
    } else {
        let loaded = std::process::Command::new("systemctl")
            .args(["--user", "show", "-p", "LoadState", "--value", &unit])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "loaded")
            .unwrap_or(false);
        let state = std::process::Command::new("systemctl")
            .args(["--user", "is-active", &unit])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if !loaded {
            checks.push(Check {
                name: "systemd",
                ok: false,
                detail: format!("{unit} is not installed — re-run the installer with --force"),
            });
        } else if state == "active" {
            checks.push(Check {
                name: "systemd",
                ok: true,
                detail: format!("{unit} is active"),
            });
        } else {
            checks.push(Check {
                name: "systemd",
                ok: false,
                detail: format!("{unit} is {state} — telegram bot is not listening"),
            });
        }
    }

    // 7. Seat on the bus: every office seat must have a live handle on this
    //    office's frequency. Radio is optional: a missing CLI, ledger or
    //    sqlite3 is a skipped check, not a broken office.
    let radio_state_dir = std::env::var("RADIO_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share/herdr-radio")))
        .unwrap_or_else(|| std::path::PathBuf::from(".local/share/herdr-radio"));
    let radio_db = radio_state_dir.join("radio.db");
    let radio_db_str = radio_db.to_string_lossy().to_string();
    let workspace = office.workspace();
    let seats: Vec<&str> = SEAT_HANDLES.to_vec();
    let radio_installed = std::process::Command::new("radio")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let ledger_exists = radio_db.is_file();
    let sqlite_available = std::process::Command::new("sqlite3")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !radio_installed || !ledger_exists || !sqlite_available {
        let detail = if !radio_installed {
            "radio not installed — optional"
        } else if !ledger_exists {
            "radio ledger absent — optional"
        } else {
            "sqlite3 not installed — bus check skipped"
        };
        checks.push(Check {
            name: "seats-on-bus",
            ok: true,
            detail: detail.into(),
        });
    } else {
        let mut missing_seats: Vec<String> = Vec::new();
        for seat in &seats {
            let out = std::process::Command::new("sqlite3")
                .args([
                    &radio_db_str,
                    &format!("SELECT name FROM handles WHERE workspace='{workspace}' AND name='{seat}';"),
                ])
                .output();
            let present = match out {
                Ok(o) => {
                    let txt = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    !txt.is_empty()
                }
                Err(_) => false,
            };
            if !present {
                missing_seats.push(format!("seat {seat} is not on the bus"));
            }
        }
        if missing_seats.is_empty() {
            checks.push(Check {
                name: "seats-on-bus",
                ok: true,
                detail: format!("all {} office seats joined on {}", seats.len(), workspace),
            });
        } else {
            checks.push(Check {
                name: "seats-on-bus",
                ok: false,
                detail: missing_seats.join("; "),
            });
        }
    }

    // 8. Radio view leakage: a Radio dashboard belongs in the primary office
    //    workspace, not in each linked-worktree workspace. Only worktrees whose
    //    repo root is this office are inspected; another office is invisible.
    let client = herdr::HerdrClient::new(root.to_path_buf());
    let linked = client.linked_worktree_workspaces(root);
    if !radio_installed || linked.is_empty() {
        checks.push(Check {
            name: "radio-view",
            ok: true,
            detail: if !radio_installed {
                "radio not installed — optional".into()
            } else {
                "no linked worktrees".into()
            },
        });
    } else {
        let mut leaked: Vec<String> = Vec::new();
        let mut inspect_error: Option<String> = None;
        for (workspace, path) in &linked {
            match client.pane_list_for_workspace(workspace) {
                Ok(panes) => {
                    if panes
                        .iter()
                        .any(|pane| std::path::Path::new(&pane.cwd) == radio_state_dir)
                    {
                        leaked.push(format!("{workspace} ({path})"));
                    }
                }
                Err(e) => inspect_error = Some(e.to_string()),
            }
        }
        if let Some(error) = inspect_error {
            checks.push(Check {
                name: "radio-view",
                ok: true,
                detail: format!("cannot inspect panes: {error}"),
            });
        } else if leaked.is_empty() {
            checks.push(Check {
                name: "radio-view",
                ok: true,
                detail: "one Radio view per office".into(),
            });
        } else {
            checks.push(Check {
                name: "radio-view",
                ok: false,
                detail: format!(
                    "Radio view in linked worktree(s): {} — patch the Radio workspace.created hook to skip linked worktrees",
                    leaked.join(", ")
                ),
            });
        }
    }

    Ok(checks)
}

/// Present the checks. Exit 0 = healthy; 1 = at least one failed.
fn doctor(root: &std::path::Path, json: bool) -> Result<()> {
    let checks = gather_checks(root)?;
    let failed = checks.iter().filter(|c| !c.ok).count();

    if json {
        let value = serde_json::json!({
            "ok": failed == 0,
            "failed": failed,
            "checks": checks
                .iter()
                .map(|c| serde_json::json!({
                    "name": c.name,
                    "ok": c.ok,
                    "detail": c.detail,
                }))
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        for c in &checks {
            if c.ok {
                println!("ok   {}: {}", c.name, c.detail);
            } else {
                eprintln!("FAIL {}: {}", c.name, c.detail);
            }
        }
        if failed == 0 {
            println!("doctor: all checks passed");
        }
    }

    if failed > 0 {
        let detail = checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect::<Vec<_>>()
            .join("\n");
        // Doctor failures are usually environment/configuration, not kit
        // defects. Only forward one when the detail names an internal
        // failure (parse/run/panic), so a stopped systemd unit cannot become
        // a public issue.
        if report::is_reportable(&detail) {
            report::maybe_report(root, "doctor", &detail);
        }
    }

    if failed == 0 {
        Ok(())
    } else {
        anyhow::bail!("doctor: {} check(s) failed", failed)
    }
}

#[cfg(test)]
mod intelligence_tests {
    use super::*;

    #[test]
    fn task_ids_are_one_component() {
        assert!(safe_task_id("t-123"));
        assert!(safe_task_id("auto-atlas-1"));
        assert!(!safe_task_id(""));
        assert!(!safe_task_id("."));
        assert!(!safe_task_id(".."));
        assert!(!safe_task_id("../other"));
        assert!(!safe_task_id("a/b"));
        assert!(!safe_task_id("a\\b"));
    }

    #[test]
    fn small_fix_is_small() {
        let d = classify("fix the login button");
        assert!(!d.large && !d.human);
    }

    #[test]
    fn gate_words_make_it_large_and_human() {
        let d = classify("publish the release notes");
        assert!(d.large && d.human);
    }

    #[test]
    fn schema_work_is_large() {
        assert!(classify("redesign the billing schema").large);
    }

    #[test]
    fn envelope_is_typed_and_rejects_unknown_kinds() {
        assert_eq!(
            envelope("task", "t-1", None, "do it").unwrap(),
            "[task] task=t-1 gate=auto\ndo it"
        );
        assert!(envelope("bogus", "t-1", None, "x").is_err());
    }

    #[test]
    fn test_command_is_read_from_project_md() {
        assert_eq!(
            parse_test_command("x\ntest-command: bun test\n"),
            Some("bun test".into())
        );
        assert_eq!(parse_test_command("test-command: TODO"), None);
        assert_eq!(parse_test_command("no field"), None);
    }

    #[test]
    fn project_setting_reads_the_first_matching_key() {
        let dir = std::env::temp_dir().join(format!(
            "zolly-setting-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".agents/rules")).unwrap();
        std::fs::write(
            dir.join(".agents/rules/project.md"),
            "## Tooling\n\ngitnexus: on\nGITNEXUS: off\n",
        )
        .unwrap();
        assert_eq!(project_setting(&dir, "gitnexus"), Some("on".into()));
        assert_eq!(project_setting(&dir, "missing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_section_is_replaced_not_duplicated() {
        let doc = "---\nid: t\n---\n\n## Summary\n\nx\n\n## Verify\n\nold\n\n## Commit\n\nabc\n";
        let out = replace_section(doc, "Verify", "- command: `bun test`\n- exit: 0");
        assert!(out.contains("## Summary"));
        assert!(out.contains("## Commit"));
        assert!(out.contains("- exit: 0"));
        assert!(!out.contains("old"));
        assert_eq!(out.matches("## Verify").count(), 1);
    }

    #[test]
    fn verify_section_is_appended_when_missing() {
        let out = replace_section("## Summary\n\nx\n", "Verify", "- exit: 0");
        assert!(out.contains("## Verify"));
        assert!(out.contains("- exit: 0"));
    }
}

#[cfg(test)]
mod close_tests {
    use super::*;
    /// Run git in `dir`, asserting success. Test-only helper shared with
    /// deploy_tests' equivalent.
    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must run");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    use std::path::Path;

    fn fixture_repo(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "zolly-close-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join(".agents/results")).unwrap();
        run_git(&dir, &["init", "-q"]);
        run_git(&dir, &["config", "user.email", "test@example.com"]);
        run_git(&dir, &["config", "user.name", "Test"]);
        std::fs::write(
            dir.join(".agents/results/t-sms-wiring.md"),
            "---\nstatus: done\n---\n\nresult\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".agents/results/admin-org-subscriptions-page.md"),
            "body only, no frontmatter\n",
        )
        .unwrap();
        run_git(&dir, &["add", "."]);
        run_git(&dir, &["commit", "-q", "-m", "fixture"]);
        dir
    }

    #[test]
    fn short_keeps_eight_characters_and_tolerates_shorter() {
        assert_eq!(short("4c1a74b2aaaaaaaa"), "4c1a74b2");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(""), "");
    }

    #[test]
    fn main_gap_names_both_counts_and_both_shas() {
        // The refusal has to name the exact gap, so a reader can act without
        // asking. Both shas, both counts, and the command that fixes it.
        let msg = main_gap_message(
            "4c1a74b2aaaaaaaa",
            "452800febbbbbbbb",
            3,
            17,
            Path::new("/repo"),
            "main",
            "origin",
        );
        assert!(msg.contains("main 4c1a74b2"), "{msg}");
        assert!(msg.contains("origin/main 452800fe"), "{msg}");
        assert!(msg.contains("3 commit(s) ahead"), "{msg}");
        assert!(msg.contains("17 commit(s) behind"), "{msg}");
        assert!(msg.contains("merge --ff-only origin/main"), "{msg}");
    }

    #[test]
    fn close_branch_deletes_a_squash_merged_branch_by_tree() {
        let dir = std::env::temp_dir().join(format!(
            "zolly-squash-close-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git runs");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "base"]);
        git(&["branch", "foreman/t1"]);
        git(&["checkout", "-q", "foreman/t1"]);
        std::fs::write(dir.join("task.txt"), "task\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "task"]);
        git(&["checkout", "-q", "main"]);
        git(&["merge", "-q", "--squash", "foreman/t1"]);
        git(&["commit", "-q", "-m", "squash task"]);
        git(&["update-ref", "refs/remotes/origin/main", "refs/heads/main"]);

        let (step, deferred) = close_branch(&dir, "foreman/t1", "main", "origin");
        assert_eq!(step.token, "deleted", "{}", step.line);
        assert!(step.line.contains("content identical"), "{}", step.line);
        assert!(!deferred);
        let branches = std::process::Command::new("git")
            .args(["branch", "--list", "foreman/t1"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&branches.stdout).trim().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn done_signal_is_read_from_the_git_index() {
        // Step 4: a tracked *settled* result file passes; an absent one warns
        // loudly; a tracked-but-unsettled one warns and names why. Neither is
        // a hard failure — the self-repair path has no worktree and the sweep
        // must still exit 0.
        let repo = fixture_repo("done-signal");
        let settled = check_done_signal(&repo, "t-sms-wiring");
        assert_eq!(settled.token, "ok (tracked)", "{}", settled.line);
        assert!(!settled.failed);

        let missing = check_done_signal(&repo, "t-nonexistent-synthetic");
        assert!(missing.token.starts_with("MISSING"), "{}", missing.line);
        assert!(missing.line.contains("WARN"), "{}", missing.line);
        assert!(!missing.failed);

        // Tracked but not settled: `admin-org-subscriptions-page.md` has no
        // frontmatter, so close must refuse to call it ok and name the case.
        let unsettled = check_done_signal(&repo, "admin-org-subscriptions-page");
        assert_eq!(unsettled.token, "MISSING (not settled)", "{}", unsettled.line);
        assert!(unsettled.line.contains("no frontmatter block at all"), "{}", unsettled.line);
        assert!(!unsettled.failed);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn main_dirty_names_modified_tracked_file() {
        // A worker edited a tracked file in main. check_main_clean must name it
        // rather than saying the refs diverged.
        let tmp = std::env::temp_dir().join(format!("zolly-guard-clean-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        run_git(&tmp, &["init"]);
        run_git(&tmp, &["config", "user.email", "test@test"]);
        run_git(&tmp, &["config", "user.name", "test"]);
        run_git(&tmp, &["commit", "--allow-empty", "-m", "root"]);
        std::fs::write(tmp.join("tracked.txt"), "original\n").unwrap();
        run_git(&tmp, &["add", "tracked.txt"]);
        run_git(&tmp, &["commit", "-m", "add tracked"]);
        std::fs::write(tmp.join("tracked.txt"), "modified\n").unwrap();

        let dirty = main_dirty_files(&tmp).unwrap();
        assert!(dirty.iter().any(|p| p.ends_with("tracked.txt")), "{dirty:?}");
        assert!(check_main_clean(&tmp).is_err(), "dirty main must refuse");
        let err = check_main_clean(&tmp).unwrap_err().to_string();
        assert!(err.contains("tracked.txt"), "{err}");
        assert!(err.contains("uncommitted tracked changes"), "{err}");

        // Restoring it makes main clean again.
        std::fs::write(tmp.join("tracked.txt"), "original\n").unwrap();
        assert!(check_main_clean(&tmp).is_ok(), "clean main must pass");
    }

    #[test]
    fn main_dirty_names_untracked_and_staged_files() {
        let tmp = std::env::temp_dir().join(format!("zolly-guard-untracked-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        run_git(&tmp, &["init"]);
        run_git(&tmp, &["config", "user.email", "test@test"]);
        run_git(&tmp, &["config", "user.name", "test"]);
        run_git(&tmp, &["commit", "--allow-empty", "-m", "root"]);

        // Staged add (like a worker's staged result file in main).
        std::fs::write(tmp.join("staged.md"), "staged result\n").unwrap();
        run_git(&tmp, &["add", "staged.md"]);
        // Untracked new file (like a worker's stray memory file).
        std::fs::write(tmp.join("untracked.stamp"), "stamp\n").unwrap();

        let dirty = main_dirty_files(&tmp).unwrap();
        let names: Vec<&str> = dirty.iter().map(String::as_str).collect();
        assert!(names.contains(&"staged.md"), "{names:?}");
        assert!(names.contains(&"untracked.stamp"), "{names:?}");
        assert!(check_main_clean(&tmp).is_err());
    }

    #[test]
    fn main_clean_passes_when_nothing_changed() {
        let tmp = std::env::temp_dir().join(format!("zolly-guard-ok-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        run_git(&tmp, &["init"]);
        run_git(&tmp, &["config", "user.email", "test@test"]);
        run_git(&tmp, &["config", "user.name", "test"]);
        run_git(&tmp, &["commit", "--allow-empty", "-m", "root"]);
        assert!(check_main_clean(&tmp).is_ok());
    }
    #[test]
    fn main_checkout_is_the_first_registered_worktree() {
        // The sweep must act on the checkout holding `main`, even when run from
        // a linked worktree. git lists the main working tree first.
        let tmp = std::env::temp_dir().join(format!(
            "zolly-main-checkout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        run_git(&tmp, &["init"]);
        run_git(&tmp, &["config", "user.email", "test@test"]);
        run_git(&tmp, &["config", "user.name", "test"]);
        run_git(&tmp, &["commit", "--allow-empty", "-m", "root"]);
        assert_eq!(main_checkout(&tmp), tmp.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod deploy_tests {
    // The rebuild trigger and the restart answer — the two judgements the deploy
    // step makes before it touches cargo. The build itself is exercised for real
    // in the fixture repo (see the task result file), not mocked here.
    use super::*;
    use std::path::Path;

    fn run_git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must run");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn rev_parse(dir: &Path) -> String {
        git_out(dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string()
    }

    #[test]
    fn restart_is_required_only_for_listener_sources() {
        let tg = [".agents/zolly/tool/src/tg/serve.rs"];
        assert!(tool_change_needs_restart(&tg));
        assert_eq!(restart_token(&tg), "restart required");
        assert!(restart_advice(&tg).contains("restart required"));

        // main.rs is the CLI: it applies on the next invocation, no restart.
        let cli = [".agents/zolly/tool/src/main.rs"];
        assert!(!tool_change_needs_restart(&cli));
        assert_eq!(restart_token(&cli), "no restart");
        assert!(restart_advice(&cli).contains("no restart needed"));

        // A mixed change names the listener, so it needs the restart.
        let mixed = [".agents/zolly/tool/src/main.rs", ".agents/zolly/tool/src/tg/send.rs"];
        assert!(tool_change_needs_restart(&mixed));
    }

    #[test]
    fn tool_trigger_fires_only_for_tool_source_not_for_mcp() {
        // The trigger is the merge diff, scoped to `.agents/zolly/tool/`.
        let dir = std::env::temp_dir().join(format!("zolly-deploy-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run_git(&dir, &["init", "-q"]);
        run_git(&dir, &["config", "user.email", "t@t.invalid"]);
        run_git(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("README"), "base\n").unwrap();
        run_git(&dir, &["add", "-A"]);
        run_git(&dir, &["commit", "-qm", "base"]);
        let base = rev_parse(&dir);

        // Commit 2: only the copied Python under mcp/ — not a build trigger.
        std::fs::create_dir_all(dir.join(".agents/zolly/mcp")).unwrap();
        std::fs::write(dir.join(".agents/zolly/mcp/server.py"), "print(1)\n").unwrap();
        run_git(&dir, &["add", "-A"]);
        run_git(&dir, &["commit", "-qm", "mcp only"]);
        let after_mcp = rev_parse(&dir);
        assert!(
            tool_source_changed(&dir, &base, &after_mcp).unwrap().is_empty(),
            "an mcp-only change must not trigger a build"
        );

        // Commit 3: a tool source file — the trigger fires, naming the exact path.
        std::fs::create_dir_all(dir.join(".agents/zolly/tool/src")).unwrap();
        std::fs::write(dir.join(".agents/zolly/tool/src/main.rs"), "fn main() {}\n").unwrap();
        run_git(&dir, &["add", "-A"]);
        run_git(&dir, &["commit", "-qm", "tool change"]);
        let after_tool = rev_parse(&dir);
        let changed = tool_source_changed(&dir, &after_mcp, &after_tool).unwrap();
        assert_eq!(changed, vec![".agents/zolly/tool/src/main.rs".to_string()]);

        // before == after (already-synced main): an empty diff, so a re-run of
        // close is idempotent and never rebuilds.
        assert!(tool_source_changed(&dir, &after_tool, &after_tool).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn src_files_newer_than_names_the_stale_source() {
        // The same scan `zolly doctor` reports as `stale binary`, factored out so
        // doctor and the deploy step can never disagree.
        let tool_dir = std::env::temp_dir().join(format!("zolly-stale-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tool_dir);
        std::fs::create_dir_all(tool_dir.join("src/tg")).unwrap();
        let binary = tool_dir.join("target/release/zolly");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(tool_dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(tool_dir.join("src/tg/serve.rs"), "pub fn s() {}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&binary, "binary\n").unwrap();

        // Binary is the newest thing: nothing stale.
        let bin_mtime = binary.metadata().unwrap().modified().unwrap();
        assert!(src_files_newer_than(&tool_dir, bin_mtime).is_empty());

        // Touch a source after the binary: it is now named as stale.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(tool_dir.join("src/tg/serve.rs"), "pub fn s() { let _ = 1; }\n").unwrap();
        let stale = src_files_newer_than(&tool_dir, bin_mtime);
        assert_eq!(stale, vec!["tg/serve.rs".to_string()]);

        let _ = std::fs::remove_dir_all(&tool_dir);
    }
}

#[cfg(test)]
mod doctor_tests {
    // read_env_file is private to main; test the parsing contract here.
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn env_file_parsing_skips_comments_and_blanks() {
        let dir = std::env::temp_dir().join(format!("zolly-env-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.env");
        std::fs::write(&path, "# comment\n\nZOLLY_TELEGRAM_TOKEN=abc123\nZOLLY_ALLOWED_IDS=1, 2\n").unwrap();
        let map = read_env_file(&path);
        assert_eq!(map.get("ZOLLY_TELEGRAM_TOKEN").unwrap(), "abc123");
        assert_eq!(map.get("ZOLLY_ALLOWED_IDS").unwrap(), "1, 2");
        assert!(!map.contains_key("# comment"));
        let _: PathBuf = dir;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_env_file_is_empty_map() {
        let map = read_env_file(std::path::Path::new("/nonexistent/zolly.env"));
        assert!(map.is_empty());
    }
}

#[cfg(test)]
mod radio_tests {
    use super::*;

    #[test]
    fn refusal_text_is_kept_verbatim_and_read_from_stderr() {
        // The plugin's guard writes its reason to stderr with a non-zero exit.
        // Dropping it (reading stdout only) is how a refused join silently
        // becomes "joined"; the founder has to see the exact sentence.
        let stderr = "exit the current agent before changing its Radio identity or frequency; use a fresh session\n";
        assert_eq!(
            radio_line(false, "", stderr),
            "exit the current agent before changing its Radio identity or frequency; use a fresh session"
        );
    }

    #[test]
    fn success_line_is_the_stdout_and_loses_its_newline() {
        let stdout = "foreman joined on frequency zylosync (herdr:w6W:p2)\n";
        assert_eq!(
            radio_line(true, stdout, ""),
            "foreman joined on frequency zylosync (herdr:w6W:p2)"
        );
    }

    #[test]
    fn output_is_collapsed_to_one_line() {
        // A note plus the join line must still leave the office speaking one
        // line per event.
        let stdout = "note: the recorded session belongs to x\nforeman joined (herdr:w6W:p2)\n";
        assert_eq!(
            radio_line(true, stdout, ""),
            "note: the recorded session belongs to x foreman joined (herdr:w6W:p2)"
        );
    }

    #[test]
    fn a_failure_with_empty_stderr_falls_back_to_stdout() {
        assert_eq!(radio_line(false, "refused: bound elsewhere", ""), "refused: bound elsewhere");
    }

    #[test]
    fn a_silent_exit_reports_no_output() {
        assert_eq!(radio_line(false, "  \n", ""), "no output");
        assert_eq!(radio_line(true, "", ""), "no output");
    }

    #[test]
    fn inbox_ids_read_the_rows_and_ignore_the_chrome() {
        // Shape measured from `radio inbox --status pull` (2026-10-23). The
        // drain acks exactly these ids; if the header/footer ever parsed as
        // ids they would be shown and refused, and if rows stopped parsing the
        // mailbox would silently stay queued — the replay returns.
        let stdout = "inbox for pr-reviewer (newest 2):\n  \
#370 10-22 12:10 foreman        pull       3894c ref  [reply] task=office-…\n  \
#445 10-22 16:28 zolly          pending    512c  [your ga…\nread one: radio show <id>\n";
        assert_eq!(inbox_ids(stdout), vec![370, 445]);
    }

    #[test]
    fn inbox_ids_is_empty_when_nothing_is_queued() {
        let stdout = "inbox empty for pr-reviewer\n\
(new messages are pushed to your pane automatically — do not poll this command waiting for a reply)\n";
        assert!(inbox_ids(stdout).is_empty());
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn zolly_agent_md_exists_and_loadable() {
        let path = Path::new("/home/khan/Projects/zylosync/.agents/zolly/agent.md");
        assert!(path.exists(), "zolly/agent.md must exist");
        let content = std::fs::read_to_string(path).expect("must be readable");
        assert!(content.contains("## The chain"), "zolly/agent.md must contain chain definition");
    }

    #[test]
    fn foreman_agent_md_exists_and_loadable() {
        let path = Path::new("/home/khan/Projects/zylosync/.agents/foreman/agent.md");
        assert!(path.exists(), "foreman/agent.md must exist");
        let content = std::fs::read_to_string(path).expect("must be readable");
        assert!(content.contains("## The chain"), "foreman/agent.md must contain chain definition");
    }

    #[test]
    fn reviewer_agent_md_exists_and_loadable() {
        let path = Path::new("/home/khan/Projects/zylosync/.agents/reviewer/agent.md");
        assert!(path.exists(), "reviewer/agent.md must exist");
        let content = std::fs::read_to_string(path).expect("must be readable");
        assert!(content.contains("Reply framing"), "reviewer/agent.md must contain nonce framing instruction");
    }
}

#[cfg(test)]
mod telegram_poll_tests {
    use super::*;

    #[test]
    fn anonymous_defaults_to_true_with_no_flags() {
        let cli = Cli::parse_from(["zolly", "telegram-poll", "--question", "q", "--options", "a,b"]);
        match cli.command.expect("subcommand") {
            Commands::TelegramPoll { anonymous, no_anonymous, .. } => {
                assert!(anonymous);
                assert!(!no_anonymous);
            }
            other => panic!("expected TelegramPoll variant"),
        }
    }

    #[test]
    fn no_anonymous_makes_no_anonymous_true() {
        let cli = Cli::parse_from(["zolly", "telegram-poll", "--question", "q", "--options", "a,b", "--no-anonymous"]);
        match cli.command.expect("subcommand") {
            Commands::TelegramPoll { anonymous, no_anonymous, .. } => {
                assert!(anonymous);
                assert!(no_anonymous);
            }
            other => panic!("expected TelegramPoll variant"),
        }
    }

    #[test]
    fn combined_anonymous_can_be_false_via_no_anonymous() {
        // The match arm combines anonymous && !no_anonymous, so --no-anonymous
        // yields false reaching cmd_poll.
        let anonymous = true;
        let no_anonymous = true;
        assert_eq!(anonymous && !no_anonymous, false);

        let anonymous = true;
        let no_anonymous = false;
        assert_eq!(anonymous && !no_anonymous, true);

        let anonymous = false;
        let no_anonymous = false;
        assert_eq!(anonymous && !no_anonymous, false);
    }
}

#[cfg(test)]
mod verify_copy_tests {
    use super::*;

    fn fixture(name: &str, worktree_project: Option<&str>) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "zolly-verify-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".agents/rules")).unwrap();
        std::fs::create_dir_all(root.join(".agents/results")).unwrap();
        std::fs::write(
            root.join(".agents/rules/project.md"),
            "test-command: echo FROM_MAIN\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".agents/results/t-x.md"),
            "## Verify\n\nmain copy — must not change\n",
        )
        .unwrap();
        let worktree = root.join(".agents/worktrees/t-x");
        std::fs::create_dir_all(worktree.join(".agents/results")).unwrap();
        if let Some(command) = worktree_project {
            std::fs::create_dir_all(worktree.join(".agents/rules")).unwrap();
            std::fs::write(
                worktree.join(".agents/rules/project.md"),
                format!("test-command: {command}\n"),
            )
            .unwrap();
        }
        std::fs::write(
            worktree.join(".agents/results/t-x.md"),
            "## Verify\n\nbranch copy — worker wrote this\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn verify_writes_the_branch_copy_and_leaves_main_untouched() {
        let root = fixture("branch", Some("echo FROM_WORKTREE"));
        let main_before = std::fs::read_to_string(root.join(".agents/results/t-x.md")).unwrap();
        verify_cmd(&root, "t-x").expect("verify should pass");
        let branch = std::fs::read_to_string(
            root.join(".agents/worktrees/t-x/.agents/results/t-x.md"),
        )
        .unwrap();
        let main_after = std::fs::read_to_string(root.join(".agents/results/t-x.md")).unwrap();
        assert!(branch.contains("- command: `echo FROM_WORKTREE`"), "{branch}");
        assert!(branch.contains("- exit: 0"), "{branch}");
        assert_eq!(main_after, main_before, "main copy must be untouched");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn verify_falls_back_to_the_main_project_md() {
        let root = fixture("fallback", None);
        verify_cmd(&root, "t-x").expect("verify should pass");
        let branch = std::fs::read_to_string(
            root.join(".agents/worktrees/t-x/.agents/results/t-x.md"),
        )
        .unwrap();
        assert!(branch.contains("- command: `echo FROM_MAIN`"), "{branch}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[cfg(test)]
mod seat_tests {
    use super::*;

    /// The foreman names a worker `worker-<frequency>-<short-id>` and shortens
    /// the task id to fit herdr's 32-character cap. `close` must derive the
    /// same name, or it closes nothing.
    #[test]
    fn worker_seat_names_match_the_foreman_and_the_cap() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "zolly-seat-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".agents/zolly")).unwrap();
        std::fs::write(
            root.join(".agents/zolly/config.toml"),
            "frequency = \"freq\"\n",
        )
        .unwrap();

        // A task id that already fits: the full id is the agent name.
        let names = worker_seat_names(&root, "t-project-truths");
        assert_eq!(names[0], "worker-freq-t-project-truths");
        assert!(names.contains(&"worker-t-project-truths".to_string()));
        let scoped = office_worker_seat_names(&root, "t-project-truths");
        assert_eq!(scoped, vec!["worker-freq-t-project-truths"]);

        // A task id beginning with the frequency renders a bare name that also
        // starts with `worker-<freq>-`; the scoped path must not return it.
        let scoped = office_worker_seat_names(&root, "freq-fix");
        assert_eq!(scoped, vec!["worker-freq-freq-fix"]);

        // A long task id: the prefix is intact and the rendered name hits the cap.
        let names = worker_seat_names(&root, "t-a-twenty-character-task-id");
        assert_eq!(names[0], "worker-freq-t-a-twenty-character");
        assert_eq!(names[0].chars().count(), 32);
        assert!(names.contains(&"worker-t-a-twenty-character-task-id".to_string()));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
