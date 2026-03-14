# CodeClaw User Guide

## Overview

CodeClaw is a terminal-first control plane for coordinating one master Codex session and multiple worker sessions. This guide covers day-to-day operator usage for release `0.13.0`.

Repository: `https://github.com/nikkofu/codeclaw`

## Prerequisites

- Rust toolchain installed
- authenticated `codex` CLI available on `PATH`
- network access for Codex requests
- a repository where `.codeclaw/` can be created and updated

## Quick Start

1. Initialize configuration and coordination directories:

   ```bash
   cargo run -- init
   ```

2. Verify the runtime environment:

   ```bash
   cargo run -- doctor
   ```

3. Launch the TUI:

   ```bash
   cargo run -- up
   ```

## Core Commands

| Command | Purpose |
| --- | --- |
| `cargo run -- init` | Create `codeclaw.toml` if missing and initialize `.codeclaw/` layout |
| `cargo run -- doctor` | Verify config loading and `codex app-server` reachability |
| `cargo run -- up` | Launch the supervision TUI |
| `cargo run -- list` | Print registered workers |
| `cargo run -- spawn --group backend --task "Refactor API"` | Create and bootstrap a worker |
| `cargo run -- send --to master "Plan next step"` | Send a prompt to the master session |
| `cargo run -- send --to backend-001-task "Continue with validation"` | Send a prompt directly to a worker |
| `cargo run -- inspect --session master --events 8 --output 6` | Inspect one session from the CLI |
| `cargo run -- inspect --batch 3 --events 12` | Inspect one orchestration batch from the CLI |
| `cargo run -- inspect --service` | Inspect scheduler heartbeat plus the latest persisted runtime snapshot |
| `cargo run -- job create --title "Nightly refactor"` | Create a job and immediately start the first intake turn in concise mode |
| `cargo run -- job create --title "Nightly refactor" --follow` | Create a job and stream current-batch progress in the terminal |
| `cargo run -- job create --title "Nightly refactor" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10` | Create a bounded 7x24 continuation job and start it immediately |
| `cargo run -- job create --title "Auto recovery" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10 --auto-approve` | Create a delegated job that can pass CodeClaw-side approval checkpoints automatically |
| `cargo run -- job create --title "CRM blueprint" --start-session backend-001-crm` | Start a new job on an existing worker session |
| `cargo run -- job create --title "CRM blueprint" --start-group backend` | Open a new worker session in the selected group and start there |
| `cargo run -- job create --title "Backlog intake" --defer` | Create a job without starting Codex yet |
| `cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs"` | Create a bounded repeated prompt against the master session |
| `cargo run -- automation create --to backend-001-task --every-secs 600 "Continue from the last blocker"` | Create a repeated prompt that targets an existing worker |
| `cargo run -- automation list` | List session automations and their local control-plane status |
| `cargo run -- automation pause AUTO-001` | Pause a session automation without deleting it |
| `cargo run -- automation resume AUTO-001` | Resume a paused session automation immediately |
| `cargo run -- automation cancel AUTO-001` | Cancel a session automation permanently |
| `cargo run -- gateway schema` | Print the normalized gateway protocol examples |
| `cargo run -- gateway capabilities --channel mock-file` | Inspect gateway support for media, typing, and raw event fields |
| `cargo run -- gateway subscribe --job JOB-001 --channel mock-file` | Add a durable report subscription for a job |

## TUI Layout

```text
left: session list, beginning with onboard
right: selected session details, onboard kanban, or batch supervision
bottom: status bar + input prompt area
```

The selected session pane shows:

- session title and lifecycle state
- queue depth and latest batch id
- summary, lifecycle note, latest user prompt, and last message
- thread id and workspace/task-file location
- structured timeline and recent live output

When `onboard` is selected, the right pane switches to a supervisory board:

- top summary for service status, running workers, queued deliveries, delegated loops, auto-approve counts, and exhausted budgets
- live runtime visibility for Codex connectivity, app-server pid, active turns, queued turns, and the current command mode
- kanban-style lanes for pending/completed, running, and blocked/failed jobs
- a `Codex Sessions` panel that lists the real monitored sessions, their runtime state, latest user prompt, and latest response preview
- an `Automations` panel that lists repeated prompts, target sessions, intervals, remaining run/time budget, last dispatch, and last error
- control hints for `AUTO`, `LOOP`, and remaining budget markers
- operator-friendly state reasons such as `awaiting clarification` or `awaiting approval` on each job card
- slash commands can be entered from the bottom input bar without leaving the board

## TUI Keybindings

| Key | Action |
| --- | --- |
| `Up` / `Down` | Switch selected session |
| `Enter` | Open slash command entry from the bottom command bar |
| `/` | Open slash command mode |
| `o` | Jump focus to the onboard supervisor session |
| `i` | Prompt the master session |
| `e` | Prompt the selected worker |
| `n` | Spawn a worker using `group: task` |
| `f` | Cycle focus filter: all, summary, commands, errors |
| `b` | Toggle batch view for the selected session |
| `[` / `]` | Move to older/newer batch in batch view |
| `g` | Jump focus to the master session |
| `q` | Quit the TUI |

## Slash Commands Inside `up`

Press `/` from the TUI to open slash command mode. The input bar shows contextual help while you type.

You can also press `Enter` from the default bottom `Command` bar to jump straight into slash command entry.

Supported commands:

- `/help`
- `/job create "Design CRM blueprint"`
- `/job create "Nightly backlog sweep" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10`
- `/job create "CRM blueprint" --start-group backend`
- `/job create "CRM blueprint" --start-session backend-001-inspect-api-health-check`
- `/job create "Queued intake only" --defer`
- `/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"`
- `/automation create --to backend-001-inspect-api-health-check --every-secs 600 "Continue from the last blocker"`
- `/automation list`
- `/automation pause AUTO-001`
- `/automation resume AUTO-001`
- `/automation cancel AUTO-001`
- `/monitor sessions`
- `/monitor runtime`
- `/monitor jobs`
- `/monitor session master`
- `/send master "Plan the next safe step"`
- `/send backend-001-inspect-api-health-check "Continue from the last blocker"`
- `/spawn backend: Payment API refactor`
- `/focus onboard`

Behavior notes:

- slash commands are non-blocking; `up` stays live after the command is queued
- use slash commands from `onboard` when you want one-screen supervision plus action entry
- `/monitor ...` answers from local CodeClaw state instead of sending a monitor question through Codex
- `/automation ...` creates and controls repeated prompts from local CodeClaw state instead of asking Codex to manage its own loop
- command errors stay in the input bar so they can be corrected without leaving the TUI
- `Tab` completes slash commands, job flags, group names, and session ids from the current context
- matching suggestions render as a selectable list directly in the input bar
- `Shift+Tab` or `Alt+Up` / `Alt+Down` cycles the active suggestion before acceptance
- `Ctrl+P` / `Ctrl+N` recalls earlier or newer input history for the current compose mode
- the input bar now supports cursor movement and in-place editing with `Left` / `Right` / `Up` / `Down`, `Home` / `End`, `Backspace`, and `Delete`
- use `Alt+Enter` or `Ctrl+J` when you need an actual newline inside a prompt
- the input box stays single-line until content actually wraps or you insert a newline, then auto-expands while keeping the cursor visible
- `onboard` now shows scheduler state separately from the live Codex runtime pid/connection state and active turn counts
- `up` now drives scheduler ticks in the foreground, so deferred jobs, delegated loops, and session automations can continue while the TUI stays open

## Typical Operator Workflows

### 1. Start a fresh workspace

1. Run `cargo run -- init`.
2. Review `codeclaw.toml`.
3. Update group definitions and lease paths if needed.
4. Run `cargo run -- doctor` before first use.

### 2. Ask the master to plan and dispatch work

1. Launch `cargo run -- up`.
2. Press `i`.
3. Enter a planning prompt for the master.
4. Observe worker creation, status transitions, and timeline events.

### 2a. Start from the onboard supervisor board

1. Launch `cargo run -- up`.
2. Stay on the default `onboard` session.
3. Review the pending, running, blocked, completed, and failed lanes before drilling into a worker.
4. Use the `Codex Sessions` panel or run `/monitor sessions` when you need the exact session count, runtime state, and latest prompt/response previews.
5. Use `/monitor session <id>` to jump directly into one session with the right pane already focused there.
6. Use `o` to return to the board after inspecting detailed worker output.

### 3. Spawn a worker manually

1. Run:

   ```bash
   cargo run -- spawn --group backend --task "Add request validation"
   ```

2. Verify the worker appears in the TUI or `cargo run -- list`.
3. Inspect its task file in `.codeclaw/tasks/`.
4. Watch the terminal progress indicator while bootstrap is running; CodeClaw now prints spinner/status feedback and newly produced worker log lines instead of waiting silently.
5. If the current shell or wrapper is non-interactive, CodeClaw falls back to newline-based progress updates so status changes still appear in captured logs.

### 4. Investigate a blocked worker

1. Select the worker in the TUI or run `inspect --session`.
2. Review the `lifecycle note`, `last message`, timeline, and live output.
3. Send clarifying instructions with `e` in the TUI or `send --to <worker-id>`.
4. Confirm the worker moves out of `blocked` after the follow-up turn.

### 5. Recover after a restart

1. Restart CodeClaw with `cargo run -- up`.
2. Verify session timelines, output tails, and lifecycle notes are restored from `.codeclaw/state.json`.
3. Use `cargo run -- inspect --service` if a quick cross-process audit of scheduler and runtime state is preferred.

### 6. Inspect gateway compatibility before wiring an IM adapter

1. Run:

   ```bash
   cargo run -- gateway schema
   cargo run -- gateway capabilities --channel console
   cargo run -- gateway capabilities --channel mock-file
   ```

2. Review the normalized inbound and outbound JSON examples.
3. Confirm the target platform assumptions for markdown, links, media, and typing indicators.
4. Use [docs/gateway-protocol.md](gateway-protocol.md) as the compatibility reference for adapter work.

### 7. Inspect archived logs for runtime errors

1. Look under:

   ```bash
   .codeclaw/logs/archive/YYYY-MM-DD/
   ```

2. Session notifications are stored in `sessions/<session-id>.jsonl`.
3. Controller and app-server runtime logs are stored in `runtime/`.
4. Use these files when TUI output shows warnings such as notification lag or app-server stderr.

### 8. Configure bounded 7x24 continuation

1. Create a job with delegated loop protection:

   ```bash
   cargo run -- job create \
     --title "Nightly backlog sweep" \
     --delegate-master-loop \
     --continue-for-secs 3600 \
     --continue-max-iterations 10
   ```

2. Watch the initial master intake progress directly in the terminal. `job create` now starts the first planning turn immediately instead of only writing a pending record.
   By default the CLI stays concise and prints only receipt/status lines. Add `--follow` when you want live current-batch logs.
3. Add `--auto-approve` only when the job should continue through CodeClaw-side approval checkpoints automatically.
4. Keep either `cargo run -- up` or `cargo run -- serve` running if the delegated job should keep auto-continuing after the initial intake turn.
5. Watch `onboard` or `cargo run -- inspect --service` for:
   - delegated jobs
   - auto-approve jobs
   - budget exhausted jobs
   - jobs continued on the latest tick
   - runtime mode, app-server pid, active turns, and queued turns

Queue-only staging:

- use `cargo run -- job create --title "..." --defer` when you want to register a job without starting Codex immediately
- deferred jobs are picked up later by the next scheduler tick in `cargo run -- up` or `cargo run -- serve`

Alternate start targets:

- use `--start-session <worker-id>` when a known worker session should continue the job directly
- use `--start-group <group>` when CodeClaw should open a fresh worker session and start there
- omit both flags to use the master session as the default planner/dispatcher

Protection rules:

- delegated jobs only continue after a cooldown window
- time budget and iteration budget both stop infinite looping
- blocked jobs that still need manual approval remain visible instead of being auto-looped, unless `--auto-approve` is set

### 9. Create a bounded session automation

1. Create a repeated prompt from the CLI:

   ```bash
   cargo run -- automation create \
     --to master \
     --every-secs 300 \
     --max-runs 10 \
     --for-secs 3600 \
     "Review blocked jobs and continue"
   ```

2. Or create it directly from `onboard` with:

   ```text
   /automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
   ```

3. Verify the new automation appears in the onboard `Automations` panel with status, target, interval, and remaining budget.
4. Use `cargo run -- automation list` or `/automation list` when you need the full local snapshot without sending a monitor question through Codex.
5. Pause or stop the automation explicitly when the work no longer needs nudging:

   ```bash
   cargo run -- automation pause AUTO-001
   cargo run -- automation resume AUTO-001
   cargo run -- automation cancel AUTO-001
   ```

Practical guidance:

- target `master` when the repeated prompt is supervisory or dispatch-oriented
- target an existing worker session when the repeated prompt should continue execution in place
- always set `--max-runs`, `--for-secs`, or both for long-lived automations
- keep `up` or `serve` running, because session automations dispatch on the scheduler tick

## Data and File Layout

CodeClaw writes operational data under `.codeclaw/`:

- `state.json`: persisted master/worker state, output tail, live buffers, and batch history
- `status/*.json`: lightweight per-session status snapshots
- `tasks/*.md`: worker task files
- `logs/archive/YYYY-MM-DD/sessions/*.jsonl`: archived session notification logs
- `logs/archive/YYYY-MM-DD/runtime/*.jsonl`: archived controller and app-server runtime logs
- `runtime.json`: latest persisted live runtime heartbeat for `up`, `serve`, `job create`, `send`, and `spawn`
- `locks/paths.json`: coordination lock file placeholder
- `gateway/mock-outbox.jsonl`: mock gateway outbox for delivery replay and IM adapter testing

## Configuration Notes

The default configuration file is [codeclaw.example.toml](../codeclaw.example.toml). Common fields:

- `[master]`: model, reasoning effort, sandbox, and approval policy
- `[workers]`: attach mode and concurrency settings
- `[git]`: branch/worktree conventions for future isolation work
- `[coordination]`: `.codeclaw/` path layout
- `[logging]`: archived log retention and app-server notification buffer sizing
- `[[groups]]`: worker groups and leased path patterns

## Operational Guidance

- keep `codeclaw.toml` under version control only if the team agrees on shared defaults
- preserve `.codeclaw/` if audit history matters
- use `inspect --session` and `inspect --batch` for lightweight review during CI, demos, or support calls
- treat lifecycle notes as the primary short explanation for `blocked`, `bootstrapped`, `handed_back`, and `failed` sessions
- use `gateway schema` and `gateway capabilities` before implementing or approving a new IM bridge
- treat `AUTO`, `LOOP`, and remaining budget markers on `onboard` as the primary automation safety signals during 7x24 supervision
- use `automation list` and the onboard `Automations` panel together when validating repeated prompts against the real local runtime
- inspect `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl` before assuming a session-level failure has no root cause

## Known Limits

- the right pane is not a full PTY terminal
- worktree isolation and path enforcement are not yet active
- merge automation is not yet included in this release
- real Slack, Telegram, WeCom, Feishu, or Discord adapters are not yet included in this release
