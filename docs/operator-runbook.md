# CodeClaw Operator Runbook

## Purpose

This runbook is for the day-to-day operator who is responsible for keeping CodeClaw usable, observable, and safe during active work.

Use this document when:

- supervising live Codex work
- handling job or automation issues
- preparing a demo or stakeholder session
- responding to runtime or delivery problems

Release baseline:

- `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`

## 1. Minimum Operator Responsibilities

The operator is expected to:

- verify the runtime is healthy before active work starts
- understand whether `up` or `serve` is currently driving the scheduler
- know which sessions are active and what each one is doing
- keep automation bounded and visible
- inspect logs before declaring a failure unexplained
- preserve `.codeclaw/` when evidence or history matters

## 2. Start-of-Shift Checklist

Run:

```bash
cargo run -- doctor
cargo run -- inspect --service
```

Confirm:

- config loads
- `codex app-server` is reachable
- runtime heartbeat is visible
- no obviously stale or failed scheduler state is being mistaken for active execution

If you are actively supervising work in the TUI:

```bash
cargo run -- up
```

Check onboard for:

- runtime connectivity
- `Codex Sessions`
- `Automations`
- blocked jobs
- failed jobs
- exhausted automation or loop budget

## 3. Core Operating Modes

### `up`

Use `up` when:

- an operator is actively supervising work
- you need local monitor answers quickly
- you want to drive scheduler ticks from the foreground
- you want onboard plus slash commands in one screen

### `serve`

Use `serve` when:

- you need a headless scheduler driver
- work should continue without an open TUI
- you are testing or preparing future IM-triggered flows

### `inspect`

Use `inspect` when:

- you need a fast CLI audit
- you want to confirm state without opening the TUI
- you need a support-friendly text output for logs, demos, or triage

## 4. Most Important Commands

```bash
cargo run -- doctor
cargo run -- up
cargo run -- serve
cargo run -- inspect --service
cargo run -- inspect --session master --events 8 --output 6
cargo run -- jobs
cargo run -- job inspect <job-id>
cargo run -- automation list
```

Inside `up`:

```text
/monitor sessions
/monitor runtime
/monitor jobs
/monitor session <id>
/automation list
```

## 5. Normal Supervision Flow

1. Check `doctor`.
2. Check `inspect --service`.
3. Open `up` if interactive supervision is needed.
4. Review onboard before touching individual sessions.
5. Use `Codex Sessions` to understand current real work, not guessed work.
6. Use `Automations` to verify what repeated prompts are armed, paused, or failed.
7. Only then drill into a specific session or job.

## 6. Job Intake Guidance

For normal job intake:

```bash
cargo run -- job create --title "Design an agentic CRM system blueprint"
```

For bounded continued work:

```bash
cargo run -- job create \
  --title "Nightly backlog sweep" \
  --delegate-master-loop \
  --continue-for-secs 3600 \
  --continue-max-iterations 10
```

For explicit operator-risk acceptance:

```bash
cargo run -- job create \
  --title "Auto recovery" \
  --delegate-master-loop \
  --continue-for-secs 3600 \
  --continue-max-iterations 10 \
  --auto-approve
```

Operator rule:

- do not use `--auto-approve` casually

## 7. Automation Guidance

Create bounded automation:

```bash
cargo run -- automation create \
  --to master \
  --every-secs 300 \
  --max-runs 10 \
  --for-secs 3600 \
  "Review blocked jobs and continue"
```

Review automation state:

```bash
cargo run -- automation list
```

Pause, resume, or cancel:

```bash
cargo run -- automation pause AUTO-001
cargo run -- automation resume AUTO-001
cargo run -- automation cancel AUTO-001
```

Operator rules:

- always prefer bounded automation
- verify the target session still exists
- verify a scheduler driver is active
- verify the repeated prompt is still safe and useful

## 8. Session Triage

When a session looks wrong, inspect:

- latest user prompt
- latest assistant response
- lifecycle note
- pending turns
- latest batch
- recent timeline
- recent live output

Useful commands:

```bash
cargo run -- inspect --session <session-id> --events 12 --output 12
cargo run -- inspect --service
```

In `up`:

```text
/monitor session <id>
```

## 9. Common Incident Types

### A. Job exists but work is not actually progressing

Check:

- whether the job was deferred
- whether the target session is busy
- whether approval is required
- whether loop budget is exhausted
- whether no scheduler driver is active

### B. Automation is armed but not firing

Check:

- `automation list`
- target session still exists
- target session not busy
- scheduler driver active
- no paused or failed state
- remaining runs or remaining time not exhausted

### C. `channel lagged by N`

Check:

- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl`
- notification channel capacity
- whether event volume briefly spiked

### D. TUI is open but reality is unclear

Check:

- onboard runtime header
- `Codex Sessions`
- `/monitor runtime`
- `/monitor sessions`
- `cargo run -- inspect --service`

## 10. Logs and Evidence

Key evidence paths:

```text
.codeclaw/state.json
.codeclaw/runtime.json
.codeclaw/service.json
.codeclaw/status/
.codeclaw/tasks/
.codeclaw/logs/archive/YYYY-MM-DD/sessions/
.codeclaw/logs/archive/YYYY-MM-DD/runtime/
```

Preserve these before destructive troubleshooting or major upgrades if history matters.

## 11. Pre-Demo Checklist

Before showing CodeClaw to users, stakeholders, or customers:

- run `cargo run -- doctor`
- verify `inspect --service`
- ensure `onboard` is populated with sane data
- avoid unbounded automation
- clear obviously stale failed items if they will confuse the audience
- know the specific sessions and jobs you intend to show
- know the fallback explanation if the model blocks on clarification

## 12. Escalation Rules

Escalate before continuing when:

- logs suggest authentication or provider-side failure
- automation is repeatedly failing with the same cause
- release/documentation mismatch is discovered
- a session appears to have destructive or unsafe intent
- gateway rollout work is proceeding without a capability matrix or authorization plan

## 13. End-of-Shift Checklist

- review blocked and failed jobs
- review armed automations
- confirm whether `up` or `serve` should remain active
- preserve logs if an incident occurred
- record outstanding risks, approvals, or manual follow-ups

## 14. Reference Set

- [quickstart-card-v0.13.1.md](quickstart-card-v0.13.1.md)
- [user-guide.md](user-guide.md)
- [operations-guide.md](operations-guide.md)
- [faq.md](faq.md)
- [im-gateway-rollout-checklist.md](im-gateway-rollout-checklist.md)
