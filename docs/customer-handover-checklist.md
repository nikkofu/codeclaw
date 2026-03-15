# CodeClaw Customer Handover Checklist

## Purpose

This checklist is for handing CodeClaw to a customer, partner team, or internal consuming team in a way that is formal, traceable, and low-confusion.

Use it before declaring delivery complete.

Release baseline:

- `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`

## 1. Delivery Identity

Confirm all of the following are explicit:

- product name
- delivered version
- release date
- repository URL
- release URL
- primary support contact

Minimum references:

- [../RELEASE.md](../RELEASE.md)
- [project-delivery.md](project-delivery.md)
- [user-guide.md](user-guide.md)
- [operations-guide.md](operations-guide.md)

## 2. Delivery Scope Confirmation

Confirm the receiving party understands what is delivered now.

Delivered now:

- terminal-first CLI and TUI supervision
- onboard monitoring and local monitor paths
- jobs and bounded automation
- persisted state and archived logs
- gateway protocol and `mock_file` delivery path

Not delivered now:

- production Slack/Telegram/WeCom/Feishu/Discord adapter
- per-worker `git worktree` execution
- merge queue or integration automation
- full PTY replay in the right-side pane

## 3. Environment Preconditions

Confirm the target environment has:

- Rust toolchain
- authenticated `codex` CLI
- network access for real model turns
- a writable workspace for `.codeclaw/`
- a terminal environment that supports raw mode

## 4. Installation and Verification

Walk through:

```bash
cargo run -- init
cargo run -- doctor
cargo run -- inspect --service
```

Acceptance expectation:

- configuration loads
- app-server probe succeeds
- runtime inspection is usable

## 5. Operator Walkthrough

Show the receiving party:

- `cargo run -- up`
- onboard
- `Codex Sessions`
- `Automations`
- `/monitor sessions`
- `/monitor runtime`
- `cargo run -- inspect --service`

They should see that CodeClaw can answer runtime and session questions locally.

## 6. Job Walkthrough

Run at least one simple workflow:

```bash
cargo run -- job create --title "Design an agentic CRM system blueprint"
```

Show:

- new job creation
- onboard job visibility
- session activity
- CLI inspection

## 7. Automation Walkthrough

Show one bounded automation:

```bash
cargo run -- automation create \
  --to master \
  --every-secs 300 \
  --max-runs 2 \
  --for-secs 900 \
  "Review blocked jobs and continue"
```

Then show:

```bash
cargo run -- automation list
```

And optionally:

```bash
cargo run -- automation pause AUTO-001
cargo run -- automation resume AUTO-001
cargo run -- automation cancel AUTO-001
```

## 8. Logs and Evidence Walkthrough

Show the receiving party where operational evidence lives:

```text
.codeclaw/state.json
.codeclaw/runtime.json
.codeclaw/service.json
.codeclaw/status/
.codeclaw/tasks/
.codeclaw/logs/archive/YYYY-MM-DD/sessions/
.codeclaw/logs/archive/YYYY-MM-DD/runtime/
```

## 9. Documentation Package Confirmation

Confirm the receiving party has been given:

- [project-delivery.md](project-delivery.md)
- [user-guide.md](user-guide.md)
- [operations-guide.md](operations-guide.md)
- [quickstart-card-v0.13.1.md](quickstart-card-v0.13.1.md)
- [faq.md](faq.md)
- [upgrade-notes-v0.13.1.md](upgrade-notes-v0.13.1.md)

Optional but recommended:

- [operator-runbook.md](operator-runbook.md)
- [incident-response-playbook.md](incident-response-playbook.md)
- [im-gateway-rollout-checklist.md](im-gateway-rollout-checklist.md)

## 10. Explicit Limitation Review

Do not leave limitations implicit.

Review:

- no production IM adapter included yet
- no direct promise of 24x7 autonomous remote IM operation in this release
- automation should remain bounded
- `up` and `serve` are scheduler drivers, but they are not magic recovery layers for every provider/runtime issue

## 11. Acceptance Questions

Ask the receiving party to confirm:

- they can install and run the project
- they understand the operator console and monitor paths
- they know where logs and state live
- they understand current release boundaries
- they know which future items are roadmap, not delivered capability

## 12. Sign-Off Record

Capture:

- date and environment of handover
- delivered version
- release URL
- primary operator contact
- known accepted limitations
- outstanding follow-up items

## 13. Recommended Final Message

Use wording close to this:

```text
CodeClaw v0.13.1 has been handed over with its delivery package, user guide, operations guide, FAQ, quickstart card, and upgrade notes. The receiving side has reviewed the current runtime scope, operational boundaries, and evidence/logging paths. Items outside the delivered scope have been explicitly identified as follow-up work rather than implied capabilities.
```
