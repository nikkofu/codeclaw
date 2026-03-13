# CodeClaw User Guide

## Overview

CodeClaw is a terminal-first control plane for coordinating one master Codex session and multiple worker sessions. This guide covers day-to-day operator usage for release `0.12.0`.

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
| `cargo run -- job create --title "Nightly refactor" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10` | Create a bounded 7x24 continuation job |
| `cargo run -- job create --title "Auto recovery" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10 --auto-approve` | Create a delegated job that can pass CodeClaw-side approval checkpoints automatically |
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
- summary, lifecycle note, and last message
- thread id and workspace/task-file location
- structured timeline and recent live output

When `onboard` is selected, the right pane switches to a supervisory board:

- top summary for service status, running workers, queued deliveries, delegated loops, auto-approve counts, and exhausted budgets
- kanban-style lanes for pending/completed, running, and blocked/failed jobs
- control hints for `AUTO`, `LOOP`, and remaining budget markers

## TUI Keybindings

| Key | Action |
| --- | --- |
| `Up` / `Down` | Switch selected session |
| `o` | Jump focus to the onboard supervisor session |
| `i` | Prompt the master session |
| `e` | Prompt the selected worker |
| `n` | Spawn a worker using `group: task` |
| `f` | Cycle focus filter: all, summary, commands, errors |
| `b` | Toggle batch view for the selected session |
| `[` / `]` | Move to older/newer batch in batch view |
| `g` | Jump focus to the master session |
| `q` | Quit the TUI |

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
4. Use `o` to return to the board after inspecting detailed worker output.

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
3. Use `inspect` if a quick command-line audit is preferred.

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

### 8. Inspect archived logs for runtime errors

1. Look under:

   ```bash
   .codeclaw/logs/archive/YYYY-MM-DD/
   ```

2. Session notifications are stored in `sessions/<session-id>.jsonl`.
3. Controller and app-server runtime logs are stored in `runtime/`.
4. Use these files when TUI output shows warnings such as notification lag or app-server stderr.

### 7. Configure bounded 7x24 continuation

1. Create a job with delegated loop protection:

   ```bash
   cargo run -- job create \
     --title "Nightly backlog sweep" \
     --delegate-master-loop \
     --continue-for-secs 3600 \
     --continue-max-iterations 10
   ```

2. Add `--auto-approve` only when the job should continue through CodeClaw-side approval checkpoints automatically.
3. Run `cargo run -- serve`.
4. Watch `onboard` or `cargo run -- inspect --service` for:
   - delegated jobs
   - auto-approve jobs
   - budget exhausted jobs
   - jobs continued on the latest tick

Protection rules:

- delegated jobs only continue after a cooldown window
- time budget and iteration budget both stop infinite looping
- blocked jobs that still need manual approval remain visible instead of being auto-looped, unless `--auto-approve` is set

## Data and File Layout

CodeClaw writes operational data under `.codeclaw/`:

- `state.json`: persisted master/worker state, output tail, live buffers, and batch history
- `status/*.json`: lightweight per-session status snapshots
- `tasks/*.md`: worker task files
- `logs/*.jsonl`: raw app-server event logs
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
- inspect `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl` before assuming a session-level failure has no root cause

## Known Limits

- the right pane is not a full PTY terminal
- worktree isolation and path enforcement are not yet active
- merge automation is not yet included in this release
- real Slack, Telegram, WeCom, Feishu, or Discord adapters are not yet included in this release
