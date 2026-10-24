# zolly vs Grok Bot vs Cindy — comparison report

**Date:** 2026-10-24  
**Scope:** architecture, control model, trust, memory, orchestration, portability, maturity  
**Audience:** technical founder choosing a multi-agent setup

## 0. Verdict first

| Question | Short answer |
|---|---|
| Is zolly competing with Grok Bot or Cindy? | Not directly. zolly is a repo-local **control plane** for agents. Grok Bot and Cindy are **products** that bundle runtime, harness and UI. |
| What is zolly best at? | Repo-resident rules and personas, an explicit authority chain, a single reviewer merge gate, deterministic triage/briefing, and git-native state. |
| What is Grok Bot best at? | Zero-ops, always-on, cross-device AI teammates with a persistent cloud computer and enterprise governance. |
| What is Cindy best at? | An open-source client, local-first execution, multi-harness desktop/mobile workspace, and a real Lead/Worker orchestration called Orca. |
| Closest competitors to zolly | Bernstein (governance/audit), Agent Orchestrator (worktree supervision UX), OpenHands Agent Canvas (self-hosted control center). |
| Biggest zolly gaps | Procedural rather than enforced merge gate; no sandbox; Linux/omp/herdr coupling; no cloud or mobile app; single-maintainer maturity. |

The rest of this report defends those claims with source evidence. Judgments about relative quality are architectural, not benchmark results.

## 1. What each thing actually is

### zolly

zolly is a **per-project agent office**. The repository root gets a `.agents/` tree holding rules, personas, prompts, memory, queue/result contracts, an HR tool, skills and an MCP server. A shared Rust binary, `zolly`, orchestrates the seats. The agents themselves run in **omp**; zolly is the control plane, not a model or a coding harness.

The documented chain is:

```text
founder → Zolly (manager) → foreman (team leader) → worker → reviewer → Zolly (close)
```

The reviewer is the only door to `main`. Money, production, publishing and customers are `gate: human`. State is git-native: a task branch, a worktree, `.agents/results/<id>.md`, and a PR. Runtime directories are gitignored; `.agents/memory/` is committed on purpose.

Interfaces are the terminal (omp/herdr panes), optional Telegram (long-poll or webhook, with voice in/out), an MCP server, and the CLI.

Evidence: [README](README.md), [.agents/AGENTS.md](.agents/AGENTS.md), [.agents/rules/system.md](.agents/rules/system.md), [.agents/zolly/agent.md](.agents/zolly/agent.md), [tool/src/main.rs](tool/src/main.rs).

### Grok Bot (xAI)

Grok Bot is a **hosted AI-teammate product**. Each named Bot runs on a persistent cloud computer with a browser, filesystem and terminal, so work continues when the laptop is closed. You message a Bot from desktop or mobile, grant access as it asks, and approve consequential actions. Bots can coordinate with each other, share a computer’s files and browser sessions, learn workflows by demonstration, and run routines on a schedule.

Access and billing are documented through Cursor and SuperGrok plans, with usage separate from those plans. The launch post describes it as beta, with an enterprise waitlist; the security docs already document enterprise-only controls.

Grok **Build** is the separate coding agent in the same family: a TUI/headless/ACP coding CLI with custom models, skills, plugins, marketplaces and MCP. Grok Bot is the teammate product, not the CLI.

Evidence: [overview](https://docs.x.ai/grok-bot/overview), [security](https://docs.x.ai/grok-bot/security), [launch post](https://x.ai/news/introducing-grok-bot), [Grok Build](https://docs.x.ai/build/overview).

### Cindy

Cindy is an **open-source client** (Apache-2.0) for a local-first AI agent: an Electron desktop app, an Expo/React Native mobile app, and shared packages. The backend lives in a separate repository and is not included here.

It runs on your machine, using real files and logged-in apps. The supported harnesses are Claude Code and Codex today, with Pi in the headless runtime and a native harness in progress. Models and harnesses can switch mid-task; one task can be planned, executed and reviewed by different harness × model combinations.

**Orca** is Cindy’s multi-agent layer: a Lead session plus full Worker sessions, each with its own model, effort, tools, context and history. The MCP control plane exposes team and worker tools, queue control, focus switching, optional worktrees, SSH/device-link remotes, and mobile control. Memory, skills, MCP and plugins are first-class; the client can use the hosted service or bring its own keys.

Evidence: [README](https://github.com/makecindy/cindy), [headless README](https://github.com/makecindy/cindy/blob/main/apps/cindy-headless/README.md), [Orca architecture](https://github.com/makecindy/cindy/blob/main/docs/dev-rules/orca-team-architecture.md).

### The rest of the field

The comparison is confusing if the categories are mixed:

- **Harnesses** — the thing that actually edits code: Claude Code, Codex CLI, Grok Build, Aider, Goose, OpenCode.
- **Cloud SWE products** — Devin, Cursor Cloud Agents.
- **Self-hosted platforms** — OpenHands Agent Canvas.
- **Worktree/orchestration layers** — Bernstein, Agent Orchestrator, Claude Squad, Vibe Kanban, Emdash, Microsoft Conductor.

zolly is an orchestration layer; Grok Bot is a hosted product; Cindy is a client/platform. Comparing raw feature counts across those categories is misleading. The useful comparison is **control model**: who decides, who executes, who reviews, and what evidence survives.

## 2. Head-to-head

### 2.1 Core comparison

| Dimension | zolly | Grok Bot | Cindy |
|---|---|---|---|
| Category | Repo-local control plane + prompt kit | Hosted teammate product | Open-source client/platform |
| Unit of work | Task → worktree → worker → review | Persistent named Bot | Session/team (Lead + Workers) |
| Runtime | Your machine (Linux-first), omp + herdr | Vendor cloud computer | Your machine, plus SSH/device-link; hosted sync optional |
| Source/license | Source public, **Apache-2.0** | No published source | Apache-2.0 client; backend not in this repo |
| Coding agent | omp (coupled) | Bundled, not user-replaceable | Claude Code / Codex / Pi; native in progress |
| Orchestration | Fixed role chain | Peer Bots, group chats, handoffs | Lead delegates to full Worker sessions; Orca |
| Human gate | `gate: human` for money/prod/publish/customers; reviewer merges | Per-action approvals + Auto Review | Permission modes and approvals; Lead acceptance |
| Merge authority | Reviewer only (documented), via `gh pr merge` | No repo-level merge-authority contract documented | No dedicated reviewer seat; normal PR workflow |
| State | Git + plain files under `.agents/` | Vendor DB/cloud | App SQLite + MCP control plane |
| Memory | Bounded text store, committed, atomic, secret-refusing | Per-Bot vendor memory | Maker Memory, shared across harnesses |
| Parallelism | One worker per task; no DAG or merge queue | Multiple Bots/screens; one computer-use task per screen | Multiple Workers per Lead; queues; optional worktrees |
| Interfaces | CLI, terminal panes, Telegram, MCP | Desktop (macOS/Windows/Linux), iOS/Android, X `@bot` | Desktop, mobile, CLI/headless |
| Extensibility | Edit markdown/personas/skills; MCP; Rust binary | Skills/routines, connectors, MCP; no source | Skills, MCP, plugins; forkable client |
| Isolation | Frequency/names/credentials; same OS user | Per-user cloud isolation; enterprise egress/MCP controls | Local process/DB; SSH/remote; permission modes |
| Offline | Yes, except model and Telegram calls | No, cloud-dependent | Local agents can run; hosted features need an account |
| Cost | Model usage + your hardware | Plan + separate usage | Free client; model/subscription or hosted usage |
| Maturity | 73 commits, 0 stars/forks, solo, pre-release | Funded product, beta | 3.5k stars, 527 forks, active, ~1.9k issues |

Point-in-time metrics: zolly from the local git history and the public repo page; Cindy and the other open-source projects from their GitHub pages on 2026-10-24.

### 2.2 Wider field

| Tool | Category | Runtime | License | Closest to zolly in… | Differentiator |
|---|---|---|---|---|---|
| Claude Code | Harness | Local, cloud, web, mobile | Closed | Worker and reviewer capability | Subagents, agent view, hooks, skills, `CLAUDE.md`/auto memory, channels |
| Codex CLI | Harness | Local, cloud | Apache-2.0 | Worker capability | OS sandbox + approvals, `AGENTS.md`, subagents, MCP, cloud handoff |
| OpenHands Agent Canvas | Self-hosted platform | Local, Docker, VM, cloud | MIT | Control center + automations | Multi-backend, ACP agents, Slack/GitHub/Linear triggers |
| Goose | Local agent | Local | Apache-2.0 | Local extensibility | Rust, 15+ providers, 70+ MCP, desktop/CLI/API |
| Devin | Cloud SWE | Vendor cloud | Closed | Autonomous worker | Slack/Teams/Linear/Jira intake, IDE, API, CLI handoff |
| Cursor Cloud Agents | Cloud SWE | Vendor cloud | Closed | Parallel PR workers | Cloud VMs, artifacts, remote desktop, hooks, multi-repo |
| Bernstein | Governance/orchestration | Local | Apache-2.0 | Deterministic control + audit | YAML DAG, no LLM in scheduling, worktrees, Janitor, replay/HMAC |
| Agent Orchestrator | Worktree orchestrator | Local desktop + daemon | Apache-2.0 | Orchestrator/worker + review UX | 30+ harnesses, Kanban, PR/CI/review, mobile |
| Claude Squad | TUI orchestrator | Local | AGPL-3.0 | Parallel worktrees | tmux + worktrees, simple TUI, no coordination |
| Vibe Kanban | Kanban orchestrator | Local web | Apache-2.0 | Visual task board | MCP decomposition, browser preview; community-maintained |
| Emdash | Desktop orchestrator | Local | Apache-2.0 | Multi-harness UX | 34 providers, port injection, no agent-to-agent coordination |
| Microsoft Conductor | Workflow orchestrator | Local, CLI + web | MIT | Deterministic workflows | YAML parallel groups, Copilot/Claude, conditional routing |

Table 2 uses primary READMEs/docs where available. Vibe Kanban, Emdash and Microsoft Conductor details come from a vendor-authored landscape review, flagged in Sources. Metrics are point-in-time.

## 3. Dimension analysis

### 3.1 Autonomy vs control

**Grok Bot** has the highest autonomy. A Bot owns a cloud computer, runs routines, coordinates with peers, and comes back only for approvals. That is the right shape for long-running, cross-app work where the human does not want to supervise each step.

**Cindy** is high-autonomy inside a session. The Lead delegates to Workers and accepts their output; the human can take over, steer, or interrupt. Orca’s queue controls and focus switching make it a real cockpit rather than a chat wrapper.

**zolly** is deliberately low-autonomy per step. Every LARGE change goes through foreman → worker → reviewer. Money, production, publishing and customers are hard human gates. The founder’s role is to decide, not to babysit panes.

Verdict: zolly is the control-max option, Grok Bot the autonomy-max option, Cindy sits between.

### 3.2 Auditability and trust

**zolly** has the strongest *repo-level* audit trail in this comparison. Rules are files. Task briefs and results are files. The branch, PR and merge SHA are git facts. Memory is diffable. The reviewer’s PASS/OBJECTION is a written artifact.

The weakness is that the gate is **procedural**. The reviewer is an agent following a prompt. `gh pr merge` is available to any agent that can run a shell. zolly’s own `system.md` says the herdr socket has no authentication and that anything enforceable must be decided in agent code before the call. Without GitHub branch protection and required checks, a misbehaving agent can bypass the rule.

**Grok Bot** has the strongest *enterprise* auditability: SSO/SCIM, network allowlists, MCP allowlist, inference hooks, audit logs, action recording, OpenTelemetry export, enforced Auto Review, and per-user isolation. But the audit lives in the vendor’s cloud, and memory/coordination are opaque. The security docs are explicit that Auto Review does not cover every side effect (memory writes and most settings changes are examples).

**Cindy** has the most *inspectable client*: the client is Apache-2.0 and the Orca implementation is documented in detail. The backend is not in this repository. Memory and orchestration state are app/DB-managed, not repo files. If no worker permission preference is saved, the Orca doc says the product default is `bypassPermissions` (full access); the UI allows `auto`.

Verdict: zolly wins on repo-native auditability; Grok Bot wins on enterprise auditability; Cindy wins on client inspectability.

### 3.3 Isolation and blast radius

**zolly** runs all offices as the same OS user. The frequency separates names, paths, credentials and systemd units; the README is explicit that it is **not** an OS security boundary. A shell-capable agent can read `~/.config/zolly/*`. Worktrees isolate code, not processes or secrets. No container or sandbox is part of the design.

**Grok Bot** isolates users strictly in the cloud. Within one user, all Bots share one computer’s files and browser sessions, and one Bot runs one computer-use task on its screen at a time. Enterprise controls can restrict egress and MCP destinations. Bots act as the signed-in member and have no identity of their own, so a Bot cannot exceed that member’s access — but the member’s access is broad by design.

**Cindy** runs locally, so isolation depends on the worker’s workspace and permission mode. Worktrees are optional; workers can share a working directory. SSH/device-link extends reach to remote machines. The app has explicit permission prompts, but the documented default for a new worker, with no saved preference, is full access.

Verdict: Grok Bot for blast-radius containment; zolly for code isolation only; Cindy depends on how worktrees and permissions are configured.

### 3.4 Memory

**zolly** uses a bounded plain-text store: 20 decisions, 30 learnings, 5 current entries, 24 KB total. Writes are `tmp → sync → mv`, so a crash leaves the old or new file, never a torn one. Entries are `believed` until a human runs `memory confirm`; corrections supersede rather than delete; overflow archives. The store refuses credential-shaped text. It is committed so it survives a machine switch.

That is unusually legible for this class. It is also per-office: good for one repo, not a firm-wide knowledge base.

**Grok Bot** keeps per-Bot memory of preferences, role context and prior work; files/browser sessions move context between Bots. It is vendor-managed and not designed to be diffed or exported by the user.

**Cindy** has Maker Memory shared across harnesses: correct once, reuse everywhere. Skills are shared too. The state is app-managed, not a repo file.

Verdict: zolly is the most legible; Cindy the most cross-harness; Grok Bot the most zero-effort.

### 3.5 Multi-agent coordination

**zolly** has a fixed chain and one worker per task. The foreman is a persona, not a scheduler. There is no dependency graph, merge queue, or automatic fan-out. Separation of duties is strong; throughput on a multi-task epic is limited by the human router.

**Grok Bot** treats peer coordination as first-class: multiple Bots, group chats, direct messages, handoffs, and a “chief of staff” pattern where one Bot manages specialists. Parallelism is per Bot screen, but computer-use tasks serialize per screen.

**Cindy/Orca** is the richest in-IDE orchestration here: a Lead plus multiple full Worker sessions, per-worker model/effort, queue controls, focus switching, optional worktrees, SSH/device-link, and mobile control. It is DB-backed and UI-driven, with a 21-tool MCP control plane.

**Bernstein** is the strongest deterministic scheduler: YAML phases/roles/dependencies, no LLM in the coordination loop, worktrees, a Janitor that runs lint/type/test before merge, a replay journal, and optional HMAC-chained audit receipts.

**Agent Orchestrator** is the strongest productized supervision UX: a project orchestrator, workers in worktrees, a live Kanban, and PR/CI/review feedback routed back to the owning worker.

Verdict: Grok Bot for peer teams, Cindy for IDE-style teams, Bernstein for deterministic pipelines, Agent Orchestrator for worktree ops, zolly for role-separated repo work.

### 3.6 Model and harness choice

**zolly** is model-agnostic only through omp. It is harness-coupled to omp/herdr: the foreman starts `herdr agent start ... --kind omp`, and the orchestration code works around the omp TUI’s paste and idle-status behavior. There is no ACP or multi-harness seam.

**Grok Bot** is bundled. The user chooses neither harness nor model beyond what the product exposes.

**Cindy** is the strongest: Claude Code, Codex and Pi, switchable mid-task, with different harness × model combinations per plan/execute/review, plus a headless runtime with pinned profiles.

**Claude Code** and **Codex** are harnesses first; their model choice is part of their product. zolly could in principle orchestrate them, but that would require code changes.

Verdict: Cindy wins; zolly is the most locked-in to a single harness; Grok Bot is locked by design.

### 3.7 Ops burden and cost

**zolly** has the highest ops burden: git, POSIX shell, Rust/cargo or a prebuilt binary, omp, herdr, `gh`, optional radio, optional Telegram bot + systemd + public tunnel, a per-machine model name, and a filled-in `project.md`. Cost is model usage plus your hardware.

**Grok Bot** has the lowest: download, sign in, create a Bot. Cost is the plan plus separate usage, and the data plane is the vendor’s cloud.

**Cindy** is medium-high: Node 22, pnpm 10, Git LFS and a monorepo build, or download the app. Cost is the free client plus model/subscription or hosted usage.

Verdict: Grok Bot for zero ops; Cindy for a GUI; zolly for control at the cost of setup.

### 3.8 Lock-in and exit

**zolly** has the best data portability: git plus markdown, no vendor database. But it has hard runtime coupling to omp/herdr and Linux/systemd; radio is optional and falls back to a one-line `herdr agent prompt`. The office is portable; the runtime is not.

**Grok Bot** has the highest lock-in: closed runtime, vendor memory, cloud computer, vendor-managed routines. Exit means losing the environment and automation.

**Cindy** has a forkable client and external harnesses, but the backend and hosted features are not open. Local data and BYO keys reduce lock-in.

Verdict: zolly wins on data portability; Cindy wins on harness/model portability; Grok Bot is the most locked in.

## 4. zolly: strengths, weaknesses, and doc/code gaps

### 4.1 Strengths, with evidence

1. **The repo is the control plane.** `.agents/` holds rules, personas, prompts, memory, queue and result contracts, skills, HR and MCP. `AGENTS.md` maintains a load-line register and states the principle: “a committed file proves nothing about being read.” That is a stronger documentation discipline than most agent projects.

2. **Explicit authority chain.** `system.md` defines the divisions; `zolly/agent.md` §6 and §10 make the reviewer the only merge door and the founder the only `gate: human`; `safety.md` records the founder decision that the reviewer may push to `main`. The chain is written down, not implied.

3. **Deterministic control functions.** `route`, `brief`, `curious`, `ponder`, `verify`, `close`, `gc` and `doctor` are implemented in Rust without model calls. `route` is keyword/heuristic; `curious`/`ponder` measure the tree; `verify` runs the project’s test command and rewrites the result’s `## Verify`; `close` performs a six-step sweep. This is a real differentiator against products where every step is an agent loop.

4. **Git-native state.** Worktree + branch + result file + PR are the state. Runtime directories (`queue/`, `worktrees/`, `runs/`, `events/`) are gitignored; `.agents/memory/` is committed. Crash recovery means reading git, not querying a database.

5. **Per-project isolation.** One binary, N offices. The frequency scopes radio handles, herdr agent names, credentials and systemd units; the installer refuses collisions. The README is honest that Telegram tokens and MCP registration are the remaining shared-resource risks.

6. **Bounded memory with integrity.** Atomic writes, size caps, archive rollover, secret-pattern refusal, supersession, confirm/retract, and a `compact` that only touches retracted bodies. This is more rigorous than most “memory” features in agent products.

7. **Founder channel.** Telegram long-poll/webhook, local STT, optional Piper voice replies, and idle nudges. The manager’s §2a is explicitly written for a context-switching founder: decision first, five lines, two options, one question.

### 4.2 Weaknesses, with evidence

1. **License is in place; contributor policy is not.** Apache-2.0 was added on 2026-10-24, so the license itself no longer blocks adoption. The remaining legal work is a contribution policy (DCO or CLA), a third-party notice check, and a decision on the project name/trademark.

2. **The merge gate is procedural, not enforced.** `system.md` documents that herdr has no authentication, that any process running as the user can call every method, and that policy must be decided in agent code before the call. The reviewer uses `gh pr merge`; any shell-capable agent can run the same command. Unless the repo has branch protection and required checks, “only the reviewer merges” is a prompt, not a control. This is the biggest gap versus Grok Bot’s Auto Review/network policy or Codex’s OS sandbox.

3. **Triage is a fixed instruction heuristic.** The actual `classify()` function hardcodes 45 words, 3 paths, and fixed keyword lists (`tool/src/main.rs:987`). `rules/workflows.md` now states that `zolly route` is a first pass and that `project.md` → **Size thresholds** governs foreman/worker scoping; the heuristic does not read those thresholds, so changing a project's size policy does not change routing.

4. **Integration brittleness.** `zolly worker-brief` exists because `herdr agent prompt --wait --until idle` cannot distinguish a buffered paste from a consumed one; `foreman/agent.md` documents that an omp agent reports `idle` for the whole turn, so `--until idle` matches instantly and returns success against a brief nobody submitted. The office depends on TUI heuristics, not a stable RPC.

5. **Prerequisite gap.** The README’s Requirements list covers git, POSIX shell/curl, Rust, Linux/systemd and optional Telegram/voice/GitNexus. The office de facto also needs `omp`, `herdr` and `gh` (the reviewer uses `gh pr merge`; the foreman uses `gh pr create`). Radio is optional: `zolly bus` falls back to a one-line `herdr agent prompt`. The installer does not install the required trio. A fresh user following “Requirements” will not get a working office.

6. **Platform coupling.** Linux-first; the Telegram listener is systemd-specific; no Windows path; no cloud execution; the model name is per-machine; no desktop/web/mobile app (Telegram is the remote channel). `zolly doctor` checks binary, config, graph, units and bus membership when radio is installed; the README does not document a preflight for omp/herdr/gh.

7. **Scale and quality evidence.** 73 commits in the current history; the public repo shows 0 stars and 0 forks at the time of writing; no benchmarks or evals; single maintainer. About 15.9k lines across the Rust crate, installer, memory CLI, HR and MCP tooling. The documentation is strong; operational evidence at scale is thin.

8. **No fan-out or DAG.** The documented workflow is one task → one worktree → one worker. The foreman is a persona, not a scheduler. There is no dependency graph, merge queue, or automatic parallel dispatch for a multi-task epic.

9. **Memory is per-office, not cross-project.** Deliberate, and good for repo-specific truth, but it is not a firm-wide knowledge base like Cindy’s shared memory or Grok Bot’s cross-session Bot memory.

### 4.3 What is genuinely differentiated

The combination of **repo-native policy + role separation + a single merge door + a deterministic control CLI** is not matched by any of the three named tools:

- Grok Bot has no repo policy layer and no documented merge-authority contract.
- Cindy has a strong multi-agent runtime but no founder-authored rules tree or dedicated reviewer seat.
- Claude Code and Codex are harnesses; they can edit and review, but do not ship an office, queue, gate or result-file contract.
- The worktree orchestrators isolate and supervise, but mostly leave policy and merge decisions to the human.

Bernstein is the closest on determinism and audit; Agent Orchestrator is the closest on supervision UX. Neither is a founder-facing office with a Telegram channel and a committed memory store.

The thesis is coherent: **the founder’s office, not a chatbot.** Measure, brief, route, gate, review, close.

## 5. Grok Bot: what it wins and what it costs

### Wins

- **Zero ops, always-on.** A persistent cloud computer with browser, filesystem and terminal; work continues with the laptop closed.
- **Persistent named Bots with memory.** Preferences, role context and prior work carry over; files and browser sessions move context between Bots.
- **Peer coordination.** Bots message each other, share group chats, hand off ownership, and can be organized as a chief of staff plus specialists.
- **Learn-by-demonstration.** Walk a Bot through a workflow once; it can save it as a skill or routine and run it on a schedule.
- **Cross-device.** Desktop and mobile; message it like a teammate; voice memos and drafts.
- **Enterprise governance.** SSO/SCIM, network allowlists, MCP allowlist, inference hooks, audit logs, action recording, OpenTelemetry export, enforced Auto Review, per-user isolation.
- **Non-code work.** It can operate apps and websites without APIs, which is where a repo-bound tool cannot go.

### Costs

- **No published source, no self-host, vendor cloud.** The security docs describe requests going to Cursor’s servers and work running on the member’s hosted computer. Data handling is vendor-defined.
- **No repo-native rules or personas.** Memory and coordination are vendor-managed; there is no `CLAUDE.md`/`.agents` equivalent you can review in git.
- **No documented repo-level review/merge gate.** Approvals and Auto Review are action-level. Auto Review explicitly does not cover every side effect (memory writes, most settings changes). A merge in GitHub is whatever the Bot does under the member’s identity, subject to the GitHub integration’s permissions.
- **Shared computer.** All of a user’s Bots share files and browser sessions; one computer-use task per screen. Cross-user isolation is strict; intra-user isolation is not.
- **Plan coupling.** Access is documented through Cursor and SuperGrok plans, with separate usage; the launch was beta with an enterprise waitlist. Availability and pricing can change.
- **Limited control.** You cannot inspect, fork, self-host, or swap the harness/model/orchestration.

### Against zolly

Grok Bot wins on autonomy, polish, cross-device reach, non-code work, and enterprise controls. zolly wins on repo-native policy, git audit trail, deterministic gating, data portability, and cost control (your model account and hardware).

If the goal is “a teammate who finishes work,” Grok Bot is the benchmark. If the goal is “a governed engineering office,” Grok Bot is the wrong shape.

## 6. Cindy: what it wins and what it costs

### Wins

- **Apache-2.0 client.** Forkable, inspectable, and free to use with BYO keys or local models.
- **Local execution on real files and apps.** Not a cloud sandbox; the agent works where you work.
- **Multi-harness.** Claude Code, Codex and Pi today, switchable mid-task, with different harness × model combinations per role.
- **Orca.** Lead + full Workers, per-worker model/effort, queue control, focus switching, 21 MCP tools, optional worktrees, SSH/device-link, mobile control.
- **Memory and skills shared across harnesses.** Correct once, reuse everywhere; skills are reusable assets.
- **Desktop + mobile.** A real cockpit, not a terminal-only tool.
- **Headless runtime.** Container-friendly, pinned profiles, reproducible bundles, detailed usage/artifact accounting for benchmarks.
- **Community and CI.** 3.5k stars, 527 forks, active development, a documented architecture and contribution process.

### Costs

- **Open client only.** The backend is a separate repository not included here. Full features need a Cindy account; “Skip Sign-In” loses server-backed capabilities.
- **Heavy stack.** Electron + React Native + pnpm monorepo + Git LFS + Node 22. Contributor setup is substantial.
- **Telemetry.** Official builds include TapDB analytics and an online heartbeat when signed in; stripping it requires building from source.
- **Orca is app/DB-backed.** There is no repo-file policy layer as the primary control surface, and no dedicated reviewer seat with sole merge authority. The Lead accepts work; the normal PR workflow follows.
- **Worktrees are optional.** Workers can share a working directory; conflicts are possible if you do not isolate them.
- **Permissions can default to full access.** The Orca doc says that with no saved preference, the product default is `bypassPermissions` (full access); the UI allows `auto`.
- **Large bug surface.** ~1.9k open issues and a very large codebase; hosted features add a dependency on the vendor.

### Against zolly

Cindy wins on harness/model choice, UI, mobile, local app control, and orchestration richness. zolly wins on repo-native policy, git audit trail, an explicit review gate, deterministic control, and per-office isolation on one machine.

Cindy is what zolly might become if it were a funded product. zolly is what Cindy’s Orca would look like if its state were a repo file tree with a reviewer.

## 7. The wider field

- **Claude Code** — the strongest general harness: subagents, agent view for parallel sessions, hooks, skills, MCP, `CLAUDE.md`/`AGENTS.md`/auto memory, terminal/IDE/desktop/web/mobile, scheduled/cloud tasks, and channels (Telegram, Discord, iMessage, webhooks). zolly’s workers and reviewer could be built on it, but zolly currently uses omp.
- **Codex CLI** — a local terminal harness with OS-enforced sandboxing and approvals, `AGENTS.md`, subagents, MCP, skills/plugins, and cloud handoff. Best if you want strong sandboxing and a single vendor.
- **OpenHands Agent Canvas** — an MIT self-hosted control center for agents and automations; local/Docker/VM/cloud backends; Claude Code, Codex, Gemini, Pi and OpenCode via ACP; Slack/GitHub/Linear triggers. The closest open-source “platform” to zolly’s office, but web UI + Docker + heavier.
- **Goose** — an Apache-2.0 Rust local agent, 15+ providers, 70+ MCP extensions, desktop/CLI/API, part of the Linux Foundation’s AAIF. A harness/tool, not an orchestrator.
- **Devin** — a closed hosted autonomous SWE; Slack/Teams/Linear/Jira intake, embedded IDE, API, CLI with handoff, parallel tasks. The “AI engineer” product; no repo-native policy layer.
- **Cursor Cloud Agents** — closed cloud VMs, PRs, Slack/Linear/GitHub triggers, artifacts, remote desktop, hooks, MCP, multi-repo. Strong for teams already in Cursor.
- **Bernstein** — an Apache-2.0 governance layer: YAML DAG, deterministic scheduler with no LLM in the coordination loop, worktrees, a Janitor that verifies before merge, replay journal, optional HMAC audit, 50+ adapters, air-gap profile. The closest philosophical match to zolly; stronger audit primitives, weaker founder-office UX.
- **Agent Orchestrator** — an Apache-2.0 desktop+mobile orchestrator: a project orchestrator plus workers, worktrees, a live Kanban, PR/CI/review feedback, 30+ harnesses, a local daemon. The closest productized version of zolly’s “office” UX.
- **Claude Squad / Vibe Kanban / Emdash / Microsoft Conductor** — the worktree-orchestrator layer: parallel isolation and dashboards, mostly without policy or review authority. Claude Squad (AGPL) is the simplest TUI; Vibe Kanban is a community Kanban; Emdash is a multi-provider Electron app with port injection; Microsoft Conductor is YAML workflows with Copilot/Claude.

## 8. Decision guide

| If you are… | Pick | Why |
|---|---|---|
| A solo founder who wants rules/personas/memory in the repo, a reviewer gate, and Telegram | zolly | The only option here whose control plane is the repo itself. |
| A non-technical operator handing off general work across apps | Grok Bot | Zero ops, cloud computer, mobile, approvals. |
| A developer who wants local multi-harness agents with desktop/mobile and real orchestration | Cindy | Apache-2.0 client, Orca, Claude Code/Codex/Pi, BYO keys. |
| A team wanting a self-hosted agent control center with automations | OpenHands Agent Canvas | MIT, multi-backend, Slack/GitHub/Linear, ACP. |
| A team needing deterministic, auditable multi-agent workflows | Bernstein | YAML DAG, no LLM in scheduling, replay/HMAC, worktrees. |
| A team wanting polished parallel worktree supervision | Agent Orchestrator | Desktop+mobile, 30+ harnesses, Kanban, PR/CI/review. |
| Just the best coding agent | Claude Code or Codex CLI | Mature harnesses with sandboxing, hooks, skills, MCP. |

## 9. What would make zolly competitive

Priority order, based on the gaps above:

1. **Keep the license and add a contributor policy.** Apache-2.0 is in place; decide DCO vs CLA and add a third-party notice check.
2. **Enforce the gate, do not document it.** Use GitHub rulesets/branch protection and required checks; make a CI job the only path to `main`; sandbox the worker (container, Landlock, or a dedicated OS user) so `gh pr merge` is unavailable outside the reviewer.
3. **Fix the triage contract.** Make `route` read `project.md` thresholds, or remove the claim. Add a test that fails when the documented threshold and the code diverge.
4. **Reduce the stack.** One-command bootstrap that verifies or installs omp, herdr and `gh`; a container image; macOS support; a dry-run mode that works without omp. Radio stays optional.
5. **Add a harness seam.** ACP or a `--worker-cmd` adapter so zolly can orchestrate Claude Code/Codex, not just omp.
6. **Add read-only web/mobile status.** A small HTTP UI for brief/queue/gates; keep Telegram for actions.
7. **Prove reliability.** Evals for task success, gate-bypass attempts, crash recovery and stale-worker detection; publish numbers.
8. **Add fan-out.** A task DAG and a merge queue for multi-task epics, while keeping the reviewer as the only door.

## 10. Bottom line

zolly is not a worse Grok Bot or a smaller Cindy. It is a different bet: **the repo is the office, the rules are files, the gate is a role, and the control plane is deterministic.**

- **Grok Bot** wins on autonomy, zero-ops, cross-device reach and enterprise governance. It is the right choice when the work is general, long-running, and outside a repository.
- **Cindy** wins on open-source client inspectability, local execution, multi-harness choice, mobile, and orchestration richness. It is the right choice when you want a product-grade local agent cockpit.
- **zolly** wins on repo-native policy, git auditability, explicit review authority, deterministic control, and data portability. It is the right choice when the repository *is* the organization and unreviewed changes are the risk.

The closest competitors are **Bernstein** (governance/audit) and **Agent Orchestrator** (worktree supervision), not Grok Bot. The gaps that matter are legal (contributor policy), enforced (sandbox/branch protection), platform (harness/cross-platform), and evidence (evals/maturity).

## Sources

### Local zolly sources

- [README.md](README.md)
- [.agents/AGENTS.md](.agents/AGENTS.md)
- [.agents/rules/system.md](.agents/rules/system.md)
- [.agents/rules/safety.md](.agents/rules/safety.md)
- [.agents/rules/workflows.md](.agents/rules/workflows.md)
- [.agents/rules/project.md](.agents/rules/project.md)
- [.agents/zolly/agent.md](.agents/zolly/agent.md)
- [.agents/foreman/agent.md](.agents/foreman/agent.md)
- [.agents/memory/README.md](.agents/memory/README.md)
- [tool/src/main.rs](tool/src/main.rs)
- [tool/src/tg/](tool/src/tg/)
- [tests/](tests/)
- Public repo: <https://github.com/khanshifaul/zolly>

### Grok Bot / xAI

- Grok Bot overview: <https://docs.x.ai/grok-bot/overview>
- Grok Bot security: <https://docs.x.ai/grok-bot/security>
- Launch post: <https://x.ai/news/introducing-grok-bot>
- Grok Build: <https://docs.x.ai/build/overview>

### Cindy

- Repository: <https://github.com/makecindy/cindy>
- Headless runtime: <https://github.com/makecindy/cindy/blob/main/apps/cindy-headless/README.md>
- Orca architecture: <https://github.com/makecindy/cindy/blob/main/docs/dev-rules/orca-team-architecture.md>

### Other tools

- Claude Code: <https://code.claude.com/docs/en/overview>
- Codex CLI: <https://developers.openai.com/codex/cli>
- OpenHands: <https://github.com/OpenHands/OpenHands>
- Goose: <https://github.com/aaif-goose/goose>
- Aider: <https://github.com/Aider-AI/aider>
- Devin: <https://docs.devin.ai/get-started/devin-intro>
- Cursor Cloud Agents: <https://cursor.com/docs/cloud-agent>
- Bernstein: <https://github.com/sipyourdrink-ltd/bernstein>
- Agent Orchestrator: <https://github.com/OrchestratorInc/agent-orchestrator>
- Claude Squad: <https://github.com/smtg-ai/claude-squad>

### Secondary

- “9 Open-Source Agent Orchestrators for AI Coding (2026)”, Augment Code, updated 2026-08-12: <https://www.augmentcode.com/tools/open-source-agent-orchestrators>. Vendor-authored; used only for Vibe Kanban, Emdash and Microsoft Conductor details and for the worktree-orchestrator framing.

## Method and caveats

- zolly claims were checked against the local tree at the commit dated 2026-10-23, plus the public repo page. Metrics: 73 commits in the current history; approximately 15.9k lines across `tool/src`, `install.sh`, `.agents/shared/memory`, `.agents/hr/hr.py` and `.agents/zolly/mcp/zolly_mcp.py`; Apache-2.0 (added 2026-10-24).
- Grok Bot claims come from xAI’s docs and launch post as of 2026-10-24. The product is beta; plan availability, pricing and enterprise controls can change.
- Cindy claims come from its README, headless README and Orca architecture doc. The client is Apache-2.0; the backend is not in the repository.
- Star/fork/issue counts are point-in-time GitHub values and should not be read as quality scores.
- The comparison is architectural, not a benchmark. No task-success, cost or latency measurements were run. Claims about relative quality are therefore about design and evidence, not measured performance.
