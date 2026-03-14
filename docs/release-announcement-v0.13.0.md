# CodeClaw v0.13.0 Release Announcement

CodeClaw `v0.13.0` is now available.

This release improves the operator experience for multi-session Codex supervision. The main theme is transparency: CodeClaw should answer runtime and session-status questions from its own control-plane state, expose more of that state directly in `onboard`, and reduce the amount of operator friction required to keep long-running work moving.

## What Is New

- Local codex-monitor visibility in `onboard`, including a dedicated `Codex Sessions` panel
- Bounded session automations with `automation create|list|pause|resume|cancel`
- `up` now acts as an active foreground scheduler driver, so delegated loops and automations can progress while the TUI is open
- Local slash commands for `/monitor ...` and `/automation ...`
- Improved command-bar UX with completion, cursor movement, multiline input, and history recall
- Persisted live runtime heartbeat in `.codeclaw/runtime.json`
- Expanded `inspect --service` output with runtime pid, mode, active turns, and queued turns

## Why This Matters

Before this release, operators still had too many cases where they had to infer whether Codex was actually running, whether a job was only planned versus truly executing, or whether continued work required a separate scheduler process. `v0.13.0` tightens that loop:

- runtime visibility is local and authoritative
- automation is explicit and bounded
- `onboard` is a real control surface, not only a passive summary board
- day-to-day supervision requires fewer shell hops and fewer model-mediated monitor prompts

## Representative Commands

```bash
cargo run -- up
cargo run -- inspect --service
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
```

Inside `up`:

```text
/monitor sessions
/monitor runtime
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
```

## Release References

- GitHub Release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.0`
- Release notes: [../RELEASE.md](../RELEASE.md)
- User guide: [user-guide.md](user-guide.md)
- Operations guide: [operations-guide.md](operations-guide.md)
- Project delivery: [project-delivery.md](project-delivery.md)

## Suggested External Post

```md
CodeClaw v0.13.0 is live.

This release improves multi-session Codex supervision with local runtime monitoring, bounded session automations, stronger onboard visibility, and a much more capable TUI command bar.

Highlights:
- authoritative local monitor answers for runtime/session visibility
- onboard `Codex Sessions` and `Automations` panels
- bounded `automation create|list|pause|resume|cancel`
- `up` now drives scheduler ticks while the TUI is open
- richer slash commands, completion, cursor movement, and multiline input

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.0
Repo: https://github.com/nikkofu/codeclaw
```
