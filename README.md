# CodeClaw

CodeClaw is a terminal-first control plane for Codex, designed to coordinate one master session and multiple workers across a large codebase.

It combines task routing, session management, shared coordination files, and Git/worktree-oriented isolation so parallel AI-assisted delivery is easier to supervise.

## Release Information

- Version: `0.10.0`
- Repository: `https://github.com/nikkofu/codeclaw`
- Delivery and planning package:
  - [Release Notes](RELEASE.md)
  - [Project Delivery](docs/project-delivery.md)
  - [User Guide](docs/user-guide.md)
  - [Operations Guide](docs/operations-guide.md)
  - [Acceptance Use Cases](docs/acceptance-use-cases.md)
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
- persisted session timeline and orchestration batch history across process restarts
- persisted rolling live-output tail, including in-flight assistant text, across process restarts
- a dedicated batch inspection view in the TUI for replaying one orchestration chain across multiple sessions
- color-coded and animated status cues across the sidebar, panels, status bar, and terminal title
- right-pane focus filters and colorized log rendering for summary, command, and error inspection
- explicit worker lifecycle supervision for spawn request, bootstrap, blocker, and handoff states
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
cargo run -- job inspect JOB-001
cargo run -- list
```

`up` opens the current TUI shell:

```text
left: session list
right: selected session overview + timeline + live output
bottom: status + input area
```

Current TUI keybindings:

```text
↑ / ↓   switch sessions
i       send a prompt to master
e       send a prompt to the selected worker
n       spawn a worker using "group: task"
f       cycle right-pane focus between all / summary / commands / errors
b       toggle batch view for the selected session
[ / ]   cycle older/newer batches for the selected session
g       focus master
q       quit
```

The sidebar now also reflects per-session queue depth with `qN` prefixes when a session has pending turns.

The right pane now separates supervision metadata from execution noise:

- `Selected Session` shows title, queue depth, summary, lifecycle note, last message, thread id, and task file/workspace
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
- `codeclaw inspect --service` prints the latest persisted service heartbeat, including pending/running/blocked job buckets
- `--events` and `--output` tune how much recent history is printed

The `spawn` command now also shows terminal-side progress feedback while it waits for worker bootstrap, including a spinner, state updates, and fresh worker log lines.

Jobs now provide a top-level operating object above batches and workers:

- `codeclaw job create` creates a durable pending job with orchestration policy metadata
- `codeclaw jobs` lists known jobs with status, batch count, worker count, and pattern
- `codeclaw job inspect JOB-001` shows the current job summary, report cadence fields, linked batches/workers, recent reports, subscriptions, and delivery history
- `codeclaw send --job ...` and `codeclaw spawn --job ...` attach new work to an existing job

The new `serve` command is the first service-mode skeleton for long-running orchestration:

- `codeclaw serve` runs scheduler ticks without opening the TUI
- pending jobs with no batches are automatically submitted to the master session for planning
- due running/blocked jobs now emit persisted digest reports on the service loop cadence
- queued report deliveries now flow through a channel-neutral outbox and are emitted through a first `console/stdout` delivery path
- the latest service heartbeat is persisted to `.codeclaw/service.json` so CLI inspection and future gateways can observe background state

## Requirements

- Rust toolchain
- `codex` CLI installed and authenticated
- network access for actual model turns

## Configuration

- Example configuration: [codeclaw.example.toml](codeclaw.example.toml)
- Local runtime config generated by `codeclaw init`: `codeclaw.toml`
- Coordination state root: `.codeclaw/`
- `master.reasoning_effort` defaults to `high` so CodeClaw can override incompatible global Codex defaults when launching `codex app-server`
- persisted supervision data currently lives in `.codeclaw/state.json` under `jobs`, `workers`, `session_history`, `session_output`, `session_live_buffers`, and `batches`
- persisted job reports currently live in `.codeclaw/state.json` under `reports`
- persisted report delivery subscriptions and outbox records currently live in `.codeclaw/state.json` under `report_subscriptions` and `report_deliveries`
- persisted service heartbeat currently lives in `.codeclaw/service.json`

## Known Gaps

- the right pane is still a structured log view, not a full PTY terminal emulator
- live output persistence is limited to a rolling tail; full scrollback and PTY capture are not implemented
- worker sessions do not yet run in dedicated `git worktree` directories
- path leases are documented but not yet hard-enforced at dispatch time
- merge gating and integration-branch automation are still ahead
- `serve` mode is an early skeleton; it persists heartbeat and auto-intakes pending jobs, but does not yet resume in-flight turns after process restart

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
- Roadmap: [docs/roadmap.md](docs/roadmap.md)
