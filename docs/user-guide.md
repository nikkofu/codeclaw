# CodeClaw User Guide

## Overview

CodeClaw is a terminal-first control plane for coordinating one master Codex session and multiple worker sessions. This guide covers day-to-day operator usage for release `0.10.0`.

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

## TUI Layout

```text
left: session list
right: selected session details + timeline + live output
bottom: status bar + input prompt area
```

The selected session pane shows:

- session title and lifecycle state
- queue depth and latest batch id
- summary, lifecycle note, and last message
- thread id and workspace/task-file location
- structured timeline and recent live output

## TUI Keybindings

| Key | Action |
| --- | --- |
| `Up` / `Down` | Switch selected session |
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

### 3. Spawn a worker manually

1. Run:

   ```bash
   cargo run -- spawn --group backend --task "Add request validation"
   ```

2. Verify the worker appears in the TUI or `cargo run -- list`.
3. Inspect its task file in `.codeclaw/tasks/`.
4. Watch the terminal progress indicator while bootstrap is running; CodeClaw now prints spinner/status feedback and newly produced worker log lines instead of waiting silently.

### 4. Investigate a blocked worker

1. Select the worker in the TUI or run `inspect --session`.
2. Review the `lifecycle note`, `last message`, timeline, and live output.
3. Send clarifying instructions with `e` in the TUI or `send --to <worker-id>`.
4. Confirm the worker moves out of `blocked` after the follow-up turn.

### 5. Recover after a restart

1. Restart CodeClaw with `cargo run -- up`.
2. Verify session timelines, output tails, and lifecycle notes are restored from `.codeclaw/state.json`.
3. Use `inspect` if a quick command-line audit is preferred.

## Data and File Layout

CodeClaw writes operational data under `.codeclaw/`:

- `state.json`: persisted master/worker state, output tail, live buffers, and batch history
- `status/*.json`: lightweight per-session status snapshots
- `tasks/*.md`: worker task files
- `logs/*.jsonl`: raw app-server event logs
- `locks/paths.json`: coordination lock file placeholder

## Configuration Notes

The default configuration file is [codeclaw.example.toml](../codeclaw.example.toml). Common fields:

- `[master]`: model, reasoning effort, sandbox, and approval policy
- `[workers]`: attach mode and concurrency settings
- `[git]`: branch/worktree conventions for future isolation work
- `[coordination]`: `.codeclaw/` path layout
- `[[groups]]`: worker groups and leased path patterns

## Operational Guidance

- keep `codeclaw.toml` under version control only if the team agrees on shared defaults
- preserve `.codeclaw/` if audit history matters
- use `inspect --session` and `inspect --batch` for lightweight review during CI, demos, or support calls
- treat lifecycle notes as the primary short explanation for `blocked`, `bootstrapped`, `handed_back`, and `failed` sessions

## Known Limits

- the right pane is not a full PTY terminal
- worktree isolation and path enforcement are not yet active
- merge automation is not yet included in this release
