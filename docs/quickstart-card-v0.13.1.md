# CodeClaw v0.13.1 Quickstart Card

## Purpose

This is a one-page operator quickstart for CodeClaw `v0.13.1`.

Use it when:

- handing the project to a new operator
- attaching a short start guide to a release or IM message
- onboarding a teammate who only needs the minimum working path

## 1. Prerequisites

- Rust toolchain installed
- `codex` CLI installed and authenticated
- network access for real model turns
- a writable repository where `.codeclaw/` can be created

## 2. First-Time Setup

```bash
cargo run -- init
cargo run -- doctor
```

What success looks like:

- config loads
- `codex app-server` probe is `ok`
- thread/start probe is `ok`

## 3. Start the Operator Console

```bash
cargo run -- up
```

What to look for:

- `onboard` is visible in the session list
- live runtime info appears in the onboard header
- `Codex Sessions` and `Automations` panels are visible

## 4. Most Useful Commands

```bash
cargo run -- inspect --service
cargo run -- jobs
cargo run -- job create --title "Design an agentic CRM system blueprint"
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
```

## 5. Most Useful TUI Slash Commands

```text
/monitor sessions
/monitor runtime
/monitor jobs
/job create "Design an agentic CRM system blueprint"
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
```

## 6. Core Operator Rules

- ask monitoring questions through `/monitor ...` or `inspect --service`, not through a freeform prompt to Codex
- keep automations bounded with `--max-runs`, `--for-secs`, or both
- keep either `cargo run -- up` or `cargo run -- serve` active when deferred jobs, delegated loops, or session automations should continue running
- inspect archived logs before assuming a failure has no root cause

## 7. Keyboard Shortcuts

```text
Up / Down   switch sessions
Enter       open slash command entry
/           open slash mode
o           focus onboard
i           prompt master
e           prompt selected worker
n           spawn worker
f           cycle focus filter
b           toggle batch view
g           focus master
q           quit
```

## 8. Quick Troubleshooting

If the UI is up but work is not moving:

- run `cargo run -- inspect --service`
- check whether scheduler ticks are active
- check whether the job is blocked on approval
- check whether time/iteration budget is exhausted
- check `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl`

If automation is not firing:

- run `cargo run -- automation list`
- confirm the target session still exists
- confirm `up` or `serve` is running
- check the automation for `paused`, `failed`, or exhausted budget

## 9. Key File Paths

```text
.codeclaw/state.json
.codeclaw/runtime.json
.codeclaw/service.json
.codeclaw/status/
.codeclaw/tasks/
.codeclaw/logs/archive/YYYY-MM-DD/sessions/
.codeclaw/logs/archive/YYYY-MM-DD/runtime/
```

## 10. Canonical References

- Release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`
- Repository: `https://github.com/nikkofu/codeclaw`
- User guide: [user-guide.md](user-guide.md)
- Operations guide: [operations-guide.md](operations-guide.md)
- Upgrade notes: [upgrade-notes-v0.13.1.md](upgrade-notes-v0.13.1.md)
