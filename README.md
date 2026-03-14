# CodeClaw

CodeClaw is a terminal-first control plane for Codex, designed to coordinate one master session and multiple workers across a large codebase.

It combines task routing, session management, shared coordination files, and Git/worktree-oriented isolation so parallel AI-assisted delivery is easier to supervise.

## Release Information

- Version: `0.13.1`
- Repository: `https://github.com/nikkofu/codeclaw`
- Delivery and planning package:
  - [Release Notes](RELEASE.md)
  - [Release Announcement v0.13.1](docs/release-announcement-v0.13.1.md)
  - [Upgrade Notes v0.13.1](docs/upgrade-notes-v0.13.1.md)
  - [Community Post Kit v0.13.1](docs/community-post-kit-v0.13.1.md)
  - [Quickstart Card v0.13.1](docs/quickstart-card-v0.13.1.md)
  - [FAQ](docs/faq.md)
  - [IM Gateway Rollout Checklist](docs/im-gateway-rollout-checklist.md)
  - [Project Delivery](docs/project-delivery.md)
  - [User Guide](docs/user-guide.md)
  - [Operations Guide](docs/operations-guide.md)
  - [Acceptance Use Cases](docs/acceptance-use-cases.md)
  - [Gateway Protocol](docs/gateway-protocol.md)
  - [Product Strategy](docs/product-strategy.md)
  - [System Architecture vNext](docs/system-architecture-v2.md)
  - [Technical Roadmap](docs/technical-roadmap.md)
  - [Project Plan](docs/project-plan.md)

## Current Status

This repository now includes a working Rust control-plane prototype with:

- a local `codeclaw` CLI
- persistent `.codeclaw/` coordination state
- a `codex app-server` client over JSON-RPC stdio
- master thread bootstrapping
- worker task registration and tracking
- a left/right TUI for session navigation and live output
- terminal window title updates based on the selected Codex session
- a structured orchestration protocol so the master can spawn workers and send follow-up prompts
- queued turns for busy sessions
- automatic worker completion and failure updates routed back to the master session
- a structured orchestration timeline in the right pane
- source-tagged session events for user, bootstrap, orchestrator, runtime, command, status, and error activity
- batch-scoped CLI waiting so `send --to master` only waits for the orchestration it triggered
- a durable `Job` model above batches and workers, with CLI create/list/inspect flows
- persisted job report history for accepted, progress, blocker, completion, failure, and digest events
- a channel-neutral report delivery outbox with per-job subscriptions and delivery records
- a normalized gateway compatibility layer for text, markdown, links, image, audio, video, file, typing, and raw `type/event/hook` semantics
- a first delivery-safe `mock_file` gateway channel for IM adapter development and integration testing
- persisted session timeline and orchestration batch history across process restarts
- persisted rolling live-output tail, including in-flight assistant text, across process restarts
- a dedicated batch inspection view in the TUI for replaying one orchestration chain across multiple sessions
- a default virtual `onboard` supervision session with a kanban-like job board for pending, running, blocked, completed, and failed work
- a local codex-monitor snapshot that powers onboard session visibility without routing monitor questions through the master model
- color-coded and animated status cues across the sidebar, panels, status bar, and terminal title
- right-pane focus filters and colorized log rendering for summary, command, and error inspection
- explicit worker lifecycle supervision for spawn request, bootstrap, blocker, and handoff states
- bounded master-loop delegation for 7x24 service mode, with time-based and iteration-based continue guards
- bounded session automations that can target `master` or a specific worker on a fixed interval, with max-run and duration guards
- explicit auto-approve and delegated-loop markers across jobs, sessions, and service inspection
- foreground scheduler ticks while `up` is open, so supervision and continued execution can share one operator console
- daily archived JSONL logs under `.codeclaw/logs/archive/YYYY-MM-DD/` with configurable retention, defaulting to 30 days
- runtime log coverage for app-server stderr, parse failures, stdout-closed conditions, and lag warnings
- persisted lifecycle notes for blocker and handoff context across restarts
- persisted master summary and last-message status across restarts
- a `serve` mode skeleton with background scheduler ticks and persisted service heartbeat in `.codeclaw/service.json`

## Commands

```bash
cargo run -- init
cargo run -- doctor
cargo run -- up
cargo run -- serve --once
cargo run -- spawn --group backend --task "Payment API refactor"
cargo run -- spawn --job JOB-001 --group backend --task "Payment API refactor"
cargo run -- send --to master "Plan the next backend refactor step."
cargo run -- send --job JOB-001 --to master "Plan and dispatch this job."
cargo run -- inspect --session master --events 8 --output 6
cargo run -- inspect --batch 3 --events 12
cargo run -- inspect --service
cargo run -- jobs
cargo run -- job create --title "Payment API refactor"
cargo run -- job create --title "Design CRM blueprint" --start-session backend-001-design-crm
cargo run -- job create --title "Design CRM blueprint" --start-group backend
cargo run -- job create --title "Nightly refactor" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10
cargo run -- job create --title "Auto recovery" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10 --auto-approve
cargo run -- job create --title "Backlog intake" --defer
cargo run -- job create --title "Visible intake" --follow
cargo run -- job inspect JOB-001
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
cargo run -- automation pause AUTO-001
cargo run -- automation resume AUTO-001
cargo run -- automation cancel AUTO-001
cargo run -- gateway schema
cargo run -- gateway capabilities --channel mock-file
cargo run -- gateway subscribe --job JOB-001 --channel mock-file
cargo run -- list
```

`up` opens the current TUI shell:

```text
left: session list
right: selected session overview, onboard kanban, or timeline/live output
bottom: status + input area
```

Current TUI keybindings:

```text
↑ / ↓   switch sessions
/       open slash command mode
Enter   open slash command mode from the bottom command bar
o       focus onboard
i       send a prompt to master
e       send a prompt to the selected worker
n       spawn a worker using "group: task"
f       cycle right-pane focus between all / summary / commands / errors
b       toggle batch view for the selected session
[ / ]   cycle older/newer batches for the selected session
g       focus master
q       quit
```

The TUI now also supports onboard-friendly slash commands from the bottom input bar. Press `/` and use commands such as:

```text
/help
/job create "Design an agentic CRM system blueprint"
/job create "Nightly backlog sweep" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10
/job create "CRM blueprint" --start-group backend
/job create "CRM blueprint" --start-session backend-001-inspect-api-health-check
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
/automation pause AUTO-001
/automation resume AUTO-001
/automation cancel AUTO-001
/monitor sessions
/monitor runtime
/monitor session master
/send master "Plan the next safe step"
/focus onboard
```

Slash commands are non-blocking inside `codeclaw up`: the command is queued immediately, the TUI stays responsive, and `onboard` continues to reflect job/session changes in the same screen.

`/monitor ...` is handled locally by CodeClaw itself, so session counts, runtime connectivity, latest user prompts, and recent assistant responses are shown from real control-plane state instead of being inferred by Codex.

`/automation ...` is also handled locally by CodeClaw. Automation definitions are created, paused, resumed, and cancelled directly against persisted control-plane state instead of being delegated to the model.

While `codeclaw up` is open, it also drives scheduler ticks in the foreground. That means deferred job intake, delegated job continuation, and session automations can keep progressing without a second `serve` process. Use `codeclaw serve` when you want the same scheduler behavior without the interactive TUI.

The `onboard` header now separates scheduler state from the live Codex runtime, so `scheduler=stopped` no longer implies the Codex app-server is down while `up` is actively driving turns.

Inside slash mode and spawn mode:

- `Tab` completes commands, flags, groups, and session ids from the current context
- matching suggestions render as a selectable list in the input bar, with the current item highlighted
- `Shift+Tab` or `Alt+Up` / `Alt+Down` cycles the active suggestion before accepting it
- `Ctrl+P` / `Ctrl+N` recalls earlier or newer command history
- slash mode now behaves more like a compact command palette instead of a plain footer hint

The input bar now behaves like a real editor instead of append-only input:

- `Left` / `Right` / `Up` / `Down` move the cursor while composing
- `Home` / `End` jump within the current visual line
- `Backspace` and `Delete` edit in place
- `Alt+Enter` or `Ctrl+J` inserts a newline
- `Ctrl+P` / `Ctrl+N` recalls prompt history for the current input mode
- the input box stays single-line until content actually wraps or you insert a newline, then auto-grows while keeping the cursor visible
- from the default bottom `Command` bar, `Enter` or any non-shortcut printable key now opens slash command entry instead of doing nothing

The sidebar now also reflects per-session queue depth with `qN` prefixes when a session has pending turns.

The first session is now a virtual `onboard` supervisor board. It aggregates job state, service status, running workers, queued deliveries, delegated loop counts, auto-approve counts, budget exhaustion signals, a dedicated `Codex Sessions` panel, and an `Automations` panel so one operator can supervise long-running work without drilling into every worker first.

`onboard` lane cards now also surface a more operator-friendly state reason such as `awaiting clarification` or `awaiting approval`, instead of collapsing everything into a generic done/blocked label.

The `Automations` panel shows the latest session-targeted repeated prompts with status, target session, interval, remaining budget, last dispatch, and last error, plus top-line armed/paused/due-now counts in the onboard header.

The right pane now separates supervision metadata from execution noise:

- `Selected Session` shows title, queue depth, summary, lifecycle note, latest user prompt, last message, thread id, and task file/workspace
- `Timeline` shows recent structured events such as user prompts, orchestrator dispatches, runtime acknowledgements, status changes, and command completions
- timeline entries carry `bNNN` batch markers so the same orchestration chain can be traced after `codeclaw send` or `codeclaw up` restarts
- `Live Output` remains the rolling text stream for assistant output and command/output lines
- `f` cycles both `Timeline` and `Live Output` through focused supervision modes so you can isolate summary signals, command noise, or failures without leaving the selected session

Press `b` to switch the right pane into a batch-centric supervision view. In that mode, CodeClaw shows:

- batch id, status, root prompt, related sessions, and last event
- an aggregated batch timeline merged across all involved sessions
- `[` and `]` navigation across historical batches for the selected session

The TUI now uses stronger visual supervision cues:

- session rows are color-coded by role/group and status
- running and queued sessions animate with lightweight ASCII spinners/pulses
- selected panel borders and the status bar shift color with the current task state
- timeline rows and live-output lines are color-tagged by source so assistant text, commands, output, and errors read differently at a glance
- the terminal window title also reflects live running state for the selected session

Workers now surface explicit lifecycle states instead of collapsing everything into `completed`:

- `spawn requested` while CodeClaw is creating and registering the worker
- `bootstrapping` during the worker's initial task boot
- `bootstrapped` once the first handoff is ready for the master
- `blocked` when the worker response indicates it cannot proceed without help
- `handed back` when a later worker turn finishes and returns control to the master

The master session is now instructed to append a machine-readable orchestration block at the end of its replies. CodeClaw parses that block and can automatically:

- spawn a worker
- update a worker summary for the sidebar
- send follow-up prompts to an existing worker

When a worker finishes or fails, CodeClaw also pushes a runtime update back into the master session. If the master is busy, the update is queued and processed afterward.

Recent session timeline history and orchestration batch metadata are now persisted into `.codeclaw/state.json`, so the TUI can reconstruct supervision history from earlier CLI-driven runs.

Worker lifecycle notes now persist alongside worker records and status files, so blocked sessions and completed handoffs keep a concise state-specific annotation after restart.

The CLI can now inspect the same supervision data without opening the TUI:

- `codeclaw inspect --session master` prints one session's status, summary, lifecycle note, recent timeline slice, and recent output slice
- `codeclaw inspect --batch 3` prints one batch's root prompt, participating sessions, and recent aggregated events
- `codeclaw inspect --service` prints both the persisted scheduler heartbeat and the latest persisted runtime snapshot, including app-server pid, command mode, active turns, and queued turns
- `--events` and `--output` tune how much recent history is printed

The `spawn` command now also shows terminal-side progress feedback while it waits for worker bootstrap, including a spinner, state updates, and fresh worker log lines.

When the current terminal does not expose an interactive TTY, CodeClaw now falls back to newline-based spawn progress updates so status changes are still visible in wrapped terminals, task runners, or IM-triggered command logs.

Jobs now provide a top-level operating object above batches and workers:

- `codeclaw job create` creates a durable job and immediately starts the first intake turn in concise mode
- `codeclaw job create --follow` streams current-batch progress when an operator wants live details
- `codeclaw job create --start-session <worker-id>` routes the first turn to an existing worker session instead of the master
- `codeclaw job create --start-group <group>` opens a new worker session in that group and starts the job there
- `codeclaw job create --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10` starts the job immediately and arms it for bounded 7x24 master-loop continuation
- `codeclaw job create --auto-approve` marks that automation-visible approvals can proceed without waiting for a manual operator checkpoint; it does not by itself enable looping
- `codeclaw job create --defer` preserves the old queue-only behavior when an operator wants to stage work without starting Codex yet
- `codeclaw jobs` lists known jobs with status, batch count, worker count, and pattern
- `codeclaw job inspect JOB-001` shows the current job summary, automation state, remaining loop budget, linked batches/workers, recent reports, subscriptions, and delivery history
- `codeclaw send --job ...` and `codeclaw spawn --job ...` attach new work to an existing job

Session automations now provide a second low-cost control-plane primitive for repeated supervision or nudging work forward:

- `codeclaw automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"` schedules repeated prompts into the master session
- `codeclaw automation create --to backend-001-inspect-api-health-check --every-secs 600 "Continue from the last blocker"` targets an existing worker directly
- `codeclaw automation list` prints armed, paused, completed, and failed automation state from persisted local data
- `codeclaw automation pause|resume|cancel AUTO-001` lets operators stop or change long-running repeated prompts without editing state files manually
- the same lifecycle is available from `up` through `/automation ...`, with live visibility in onboard

The new `serve` command is the first service-mode skeleton for long-running orchestration:

- `codeclaw serve` runs scheduler ticks without opening the TUI
- `codeclaw up` now runs the same scheduler ticks in the foreground while the operator console is open
- pending jobs created with `--defer` and no batches are automatically submitted to the master session for planning on the next scheduler tick
- delegated jobs and session automations continue whenever either `codeclaw up` or `codeclaw serve` is actively driving the scheduler
- delegated jobs can now be continued through the master session automatically when cooldown and budget guards allow it
- session automations can repeatedly prompt `master` or a specific worker when their interval and budget guards allow it
- blocked jobs that still require manual approval are intentionally not auto-continued unless the job is marked `auto_approve`
- due running/blocked jobs now emit persisted digest reports on the service loop cadence
- queued report deliveries now flow through a channel-neutral outbox and are emitted through a first `console/stdout` delivery path
- the latest service heartbeat is persisted to `.codeclaw/service.json` so CLI inspection and future gateways can observe background state
- the latest live command/runtime heartbeat is persisted separately to `.codeclaw/runtime.json` so `up`, `job create`, `send`, and `spawn` activity can be inspected across processes

Gateway compatibility is now explicitly defined for future IM integrations:

- `codeclaw gateway schema` prints the canonical inbound/outbound JSON contract
- `codeclaw gateway capabilities --channel ...` prints per-channel support for markdown, media, typing, and raw `type/event/hook`
- `codeclaw gateway subscribe --job ... --channel mock-file` adds a durable report subscription for integration testing or external delivery relays
- [docs/gateway-protocol.md](docs/gateway-protocol.md) defines the compatibility contract that future Slack, Telegram, WeCom, Feishu, Discord, or webhook adapters should follow

Logging and error visibility are now treated as first-class runtime concerns:

- session event logs are archived daily under `.codeclaw/logs/archive/YYYY-MM-DD/sessions/`
- controller and app-server runtime logs are archived daily under `.codeclaw/logs/archive/YYYY-MM-DD/runtime/`
- retention defaults to 30 days and is configurable through `[logging].retention_days`
- app-server notification lag is now logged as a warning and surfaced in session output instead of immediately failing the turn

## Requirements

- Rust toolchain
- `codex` CLI installed and authenticated
- network access for actual model turns

## Configuration

- Example configuration: [codeclaw.example.toml](codeclaw.example.toml)
- Local runtime config generated by `codeclaw init`: `codeclaw.toml`
- Coordination state root: `.codeclaw/`
- Logging config: `[logging]` in `codeclaw.toml`
- `master.reasoning_effort` defaults to `high` so CodeClaw can override incompatible global Codex defaults when launching `codex app-server`
- persisted supervision data currently lives in `.codeclaw/state.json` under `jobs`, `workers`, `session_history`, `session_output`, `session_live_buffers`, and `batches`
- persisted job reports currently live in `.codeclaw/state.json` under `reports`
- persisted report delivery subscriptions and outbox records currently live in `.codeclaw/state.json` under `report_subscriptions` and `report_deliveries`
- persisted service heartbeat currently lives in `.codeclaw/service.json`
- default mock gateway outbox lives in `.codeclaw/gateway/mock-outbox.jsonl`
- daily archived logs live under `.codeclaw/logs/archive/YYYY-MM-DD/`

## Known Gaps

- the right pane is still a structured log view, not a full PTY terminal emulator
- live output persistence is limited to a rolling tail; full scrollback and PTY capture are not implemented
- worker sessions do not yet run in dedicated `git worktree` directories
- path leases are documented but not yet hard-enforced at dispatch time
- merge gating and integration-branch automation are still ahead
- `serve` mode is an early skeleton; it persists heartbeat and auto-intakes pending jobs, but does not yet resume in-flight turns after process restart
- auto-approve currently governs CodeClaw-side continuation policy and visibility; it does not replace Codex runtime approval semantics for destructive commands

## References

- Changelog: [CHANGELOG.md](CHANGELOG.md)
- Release notes: [RELEASE.md](RELEASE.md)
- Architecture notes: [docs/architecture.md](docs/architecture.md)
- Product strategy: [docs/product-strategy.md](docs/product-strategy.md)
- System architecture vNext: [docs/system-architecture-v2.md](docs/system-architecture-v2.md)
- Technical roadmap: [docs/technical-roadmap.md](docs/technical-roadmap.md)
- Project plan: [docs/project-plan.md](docs/project-plan.md)
- Project delivery: [docs/project-delivery.md](docs/project-delivery.md)
- User guide: [docs/user-guide.md](docs/user-guide.md)
- Operations guide: [docs/operations-guide.md](docs/operations-guide.md)
- Acceptance use cases: [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
- Gateway protocol: [docs/gateway-protocol.md](docs/gateway-protocol.md)
- Roadmap: [docs/roadmap.md](docs/roadmap.md)
