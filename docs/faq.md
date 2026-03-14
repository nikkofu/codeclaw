# CodeClaw FAQ

## Scope

This FAQ covers the most common operator and delivery questions for CodeClaw.

Repository:

- `https://github.com/nikkofu/codeclaw`

Release:

- `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`

## 1. What is CodeClaw?

CodeClaw is a terminal-first control plane for supervising one master Codex session and multiple worker sessions inside the same repository. It combines local state, orchestration, session visibility, jobs, bounded automation, and a TUI operator console.

## 2. Is CodeClaw replacing Codex?

No. CodeClaw sits above `codex app-server` and uses it as the execution plane. The point is not to replace Codex, but to make multi-session supervision, job control, monitoring, and future IM integration easier and more transparent.

## 3. Why not just run multiple Codex terminals directly?

Because raw terminal multiplexing does not give you enough structure for:

- durable session state
- explicit jobs and reports
- local monitor answers
- batch history
- bounded automation
- consistent delivery into future IM gateways

CodeClaw adds the control-plane layer those workflows need.

## 4. Does CodeClaw talk directly to providers?

The current product line is built to work through `codex app-server`. That keeps the execution runtime stable and lets CodeClaw focus on orchestration, visibility, persistence, and operator experience.

If your Codex setup already talks to a provider such as a Codex-compatible OpenAI-style endpoint, CodeClaw can usually benefit from that through Codex itself without re-implementing provider logic.

## 5. How many sessions are actually running right now?

Use local monitor paths, not a freeform model question:

```bash
cargo run -- inspect --service
```

Inside `up`:

```text
/monitor sessions
/monitor runtime
/monitor session <id>
```

These answers come from CodeClaw state instead of asking Codex to guess.

## 6. Why did a job appear in onboard but no Codex work seemed to happen?

That usually means one of these:

- the job was only created and not yet dispatched
- the job is deferred and waiting for a scheduler tick
- the target session is busy
- the job is blocked on approval
- the automation budget is exhausted

Check:

```bash
cargo run -- inspect --service
cargo run -- job inspect <job-id>
```

And inside `up`, check onboard runtime state plus the `Codex Sessions` panel.

## 7. Does `cargo run -- up` actually drive work, or is it only a viewer?

`up` is an active scheduler driver in the current release line. While the TUI is open, deferred jobs, delegated loops, and session automations can continue to progress.

Use `serve` when you want the same scheduler behavior without the TUI.

## 8. When should I use `serve` instead of `up`?

Use `up` when:

- an operator is actively supervising work
- you want one-screen visibility plus control
- you want to use slash commands from onboard

Use `serve` when:

- you want a headless scheduler process
- work should continue without an open TUI
- you are wiring future automation or IM-triggered execution

## 9. What is session automation?

Session automation is a bounded repeated prompt that targets `master` or an existing worker session on an interval.

Examples:

```bash
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
```

Inside `up`:

```text
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
```

## 10. How is session automation different from delegated master loops?

Delegated master loops operate at the job policy level and ask the master session to continue a job under bounded conditions.

Session automation is lower-level and simpler:

- it targets one session directly
- it replays a specific prompt on an interval
- it is easy to pause, resume, or cancel

Use delegated loops for job-centric orchestration. Use session automation for repeated nudges or bounded supervision prompts.

## 11. What should I do before enabling automation?

Always decide:

- what session should be targeted
- what the repeated prompt should do
- what stop condition should apply

Prefer setting `--max-runs`, `--for-secs`, or both. Open-ended automation should be a deliberate operational decision, not a default.

## 12. Why does monitor visibility matter so much?

Because the alternative is letting the model describe its own runtime state, which is weaker than reading the actual control-plane state directly. CodeClaw's monitor surfaces are supposed to behave like a truthful operator console, not a conversational guess.

## 13. Where are logs stored?

Key paths:

```text
.codeclaw/logs/archive/YYYY-MM-DD/sessions/
.codeclaw/logs/archive/YYYY-MM-DD/runtime/
.codeclaw/runtime.json
.codeclaw/service.json
.codeclaw/state.json
```

## 14. How long are logs kept?

By default, archived logs are retained for 30 days. Retention is configurable in `codeclaw.toml`.

## 15. What does `channel lagged by N` mean?

It usually means the app-server notification stream briefly outpaced the current consumer. In the current release line, that condition is logged and surfaced, but it is no longer treated as an automatic fatal turn failure.

If it happens often, review:

- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl`
- `[logging].notification_channel_capacity`
- whether the terminal or runtime was under burst load

## 16. Are Slack, Telegram, WeCom, Feishu, or Discord adapters already included?

No. The current releases define the gateway protocol and capability model, but real production IM adapters are still planned follow-up work.

What exists today:

- gateway schema output
- gateway capability inspection
- durable report subscriptions
- `mock_file` delivery for safe integration development

## 17. Is CodeClaw already a 24x7 autonomous IM agent?

Not yet. The current foundation supports:

- local supervision
- bounded automation
- persisted reports
- gateway compatibility modeling

The always-on IM-connected agentic deployment path is part of the next-phase roadmap, not a completed production capability in this release.

## 18. What is the recommended first-time learning path?

Read in this order:

1. [quickstart-card-v0.13.1.md](quickstart-card-v0.13.1.md)
2. [user-guide.md](user-guide.md)
3. [operations-guide.md](operations-guide.md)
4. [gateway-protocol.md](gateway-protocol.md)
5. [upgrade-notes-v0.13.1.md](upgrade-notes-v0.13.1.md)

## 19. What should I hand to a new operator or stakeholder?

Use this package:

- [quickstart-card-v0.13.1.md](quickstart-card-v0.13.1.md)
- [project-delivery.md](project-delivery.md)
- [user-guide.md](user-guide.md)
- [operations-guide.md](operations-guide.md)
- [faq.md](faq.md)

## 20. What is the cleanest way to describe v0.13.1?

Describe it as:

- a finalized tagged delivery package for the `0.13.x` monitor-and-automation control-plane line

Do not describe it as:

- a brand-new runtime feature release beyond `v0.13.0`
