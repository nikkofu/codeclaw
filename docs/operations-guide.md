# CodeClaw Operations Guide

## Purpose

This guide describes installation, deployment, backup, upgrade, and troubleshooting practices for CodeClaw `0.13.1`.

Repository: `https://github.com/nikkofu/codeclaw`

## Deployment Baseline

CodeClaw is currently delivered as a Rust application built and run from source.

Required components:

- Rust toolchain
- `codex` CLI installed and authenticated
- terminal environment with raw-mode support
- network access for Codex API traffic

## Installation Procedure

1. Clone the repository:

   ```bash
   git clone https://github.com/nikkofu/codeclaw.git
   cd codeclaw
   ```

2. Initialize the workspace:

   ```bash
   cargo run -- init
   ```

3. Review and adjust `codeclaw.toml`.

4. Validate the environment:

   ```bash
   cargo run -- doctor
   ```

5. Launch the application:

   ```bash
   cargo run -- up
   ```

## Configuration Management

- baseline configuration source: [codeclaw.example.toml](../codeclaw.example.toml)
- active runtime configuration: `codeclaw.toml`
- coordination root: `.codeclaw/`
- archived logs: `.codeclaw/logs/archive/YYYY-MM-DD/`

Recommended practice:

- review group definitions before production use
- align `lease_paths` with actual repository ownership boundaries
- keep approval/sandbox settings consistent with the intended operating model
- keep `[logging].retention_days` at or above 30 unless there is a formal storage policy requiring shorter retention
- increase `[logging].notification_channel_capacity` if the runtime still sees bursty event lag in large sessions

## Health Checks

Primary operational checks:

- `cargo run -- doctor`
- `cargo run -- list`
- `cargo run -- inspect --session master`
- `cargo run -- automation list`
- `cargo run -- gateway capabilities --channel mock-file`

Healthy baseline indicators:

- configuration loads successfully
- `codex app-server` responds
- master session can be initialized or resumed
- `.codeclaw/status/master.json` is updated during runtime
- gateway capability output matches the intended downstream integration assumptions
- `cargo run -- inspect --service` shows sane delegated, auto-approve, and exhausted-budget counts when automation is enabled
- `cargo run -- inspect --service` now also shows the latest persisted runtime heartbeat, including app-server pid, mode, active turns, and queued turns

## Persistence and Backup

Operational state lives in `.codeclaw/`.

Backup priorities:

1. `.codeclaw/state.json`
2. `.codeclaw/status/`
3. `.codeclaw/tasks/`
4. `.codeclaw/logs/`
5. `.codeclaw/gateway/`
6. `.codeclaw/logs/archive/`

Backup guidance:

- preserve `.codeclaw/` before upgrades if historical supervision data matters
- include `.codeclaw/tasks/` in incident evidence capture
- retain `.codeclaw/logs/archive/YYYY-MM-DD/sessions/*.jsonl` and `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl` for runtime event reconstruction
- retain `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl` for controller and app-server incident analysis
- retain `.codeclaw/gateway/mock-outbox.jsonl` when validating delivery flows or auditing IM relay behavior

## Upgrade Procedure

Recommended upgrade path for same-workspace deployments:

1. stop active operator sessions
2. preserve `.codeclaw/` as a backup
3. update the repository to the target release
4. verify `Cargo.toml` version and `CHANGELOG.md`
5. run `cargo test`
6. run `cargo run -- doctor`
7. run `cargo run -- gateway schema`
8. restart with `cargo run -- up`
9. confirm session restoration with `inspect --session master`

## 7x24 Supervision Controls

CodeClaw now supports bounded master-loop delegation at the job level.

Recommended policy fields during job creation:

- `--delegate-master-loop` enables service-side continuation through the master session
- `--continue-for-secs <n>` limits how long the job may keep auto-continuing
- `--continue-max-iterations <n>` limits how many delegated loop passes are allowed
- `--auto-approve` allows blocked jobs to keep moving through CodeClaw-side approval checkpoints without waiting for a manual operator

Operational guidance:

- `job create` now performs the first master intake turn immediately unless `--defer` is used
- default `job create` output is concise; use `--follow` only when live current-batch logs are needed
- `--start-session <worker-id>` routes a new job into an existing worker session
- `--start-group <group>` opens a fresh worker session and starts the job there
- the same job-start patterns are available inside `cargo run -- up` from onboard via slash commands such as `/job create ...`
- inside `up`, `Enter` opens slash entry from the default command bar, `Tab` completes slash commands/groups/session ids, a selectable suggestion list appears in the input bar, and `Ctrl+P` / `Ctrl+N` recalls recent operator input
- use `/monitor sessions`, `/monitor runtime`, and `/monitor session <id>` inside `up` when operators need authoritative local session visibility instead of model-generated answers
- use `--defer` when a job should be registered now but only started later by the next scheduler driver in `codeclaw up` or `codeclaw serve`
- always set either a time budget, an iteration budget, or both for 7x24 jobs
- prefer small first budgets such as `3600` seconds or `10` iterations while tuning
- monitor `budget exhausted jobs` in `inspect --service`
- use `inspect --service` to distinguish `scheduler=stopped` from an actually disconnected Codex runtime
- prefer the onboard `Codex Sessions` panel for day-to-day monitoring of latest user prompt and response previews across master and worker sessions
- treat `auto_approve` as an explicit operational risk decision and keep it visible in runbooks; it does not enable autonomous looping by itself

Scheduler driver guidance:

- `cargo run -- up` and `cargo run -- serve` both drive scheduler ticks
- prefer `up` when an operator wants live supervision plus control in one screen
- prefer `serve` for headless or IM-triggered runs where a foreground TUI is unnecessary
- deferred jobs and delegated loops only progress while at least one scheduler driver is running

Session automation guidance:

- use `cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs"` for supervisory repeated prompts
- use `cargo run -- automation create --to <worker-id> --every-secs 600 "Continue from the last blocker"` when a repeated prompt should target an existing worker directly
- use `/automation create ...` from onboard when operators want to create or adjust repeated prompts without leaving the TUI
- review `automation list` and the onboard `Automations` panel together so automation state is verified from local persisted control-plane data
- pause with `cargo run -- automation pause AUTO-001` during incident response or when a target session is being manually handled
- resume with `cargo run -- automation resume AUTO-001` only after confirming the target session still exists and the work still needs continued prompting
- cancel with `cargo run -- automation cancel AUTO-001` when the automation has achieved its purpose or the prompt is no longer safe to replay
- always set `--max-runs`, `--for-secs`, or both unless the automation is intentionally open-ended and explicitly supervised

## Logging and Retention

CodeClaw now archives logs by day automatically.

Layout:

- `.codeclaw/logs/archive/YYYY-MM-DD/sessions/*.jsonl`
- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl`

Defaults:

- retention: 30 days
- app-server notification buffer: 2048

Operators should review:

- runtime/controller logs for lag warnings and service-side automation events
- runtime/app-server logs for stderr output, parse failures, and stdout closure
- session logs for raw notification timelines when replaying an incident

## Rollback Procedure

If a release must be rolled back:

1. stop the current process
2. restore the previous repository revision
3. restore the saved `.codeclaw/` backup if state compatibility is a concern
4. rerun `cargo run -- doctor`
5. confirm `inspect --session master` still shows expected state

## Troubleshooting

### `doctor` fails

Check:

- `codex` is installed and authenticated
- network access is available
- `codeclaw.toml` is syntactically valid

### TUI starts but sessions do not progress

Check:

- `doctor` output
- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl`
- `.codeclaw/logs/archive/YYYY-MM-DD/sessions/*.jsonl`
- `.codeclaw/status/*.json`
- approval/sandbox settings in `codeclaw.toml`

If the issue is specifically `spawn` looking silent, also check:

- whether the terminal wrapper is non-interactive
- whether stderr is being captured or suppressed
- whether newline-based progress lines are now appearing instead of a single animated spinner

If the issue is specifically `channel lagged by N`, also check:

- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/controller.jsonl`
- whether `[logging].notification_channel_capacity` is large enough for the workload
- whether the terminal or service was under heavy event burst load rather than a true session failure

If the issue is specifically `up` showing runtime but work not auto-progressing, also check:

- whether the onboard header still shows scheduler ticks advancing
- whether the job or automation is waiting on approval or exhausted budget instead of being actually stalled
- whether another process has already moved the relevant session into a busy state

### Worker appears blocked

Check:

- session `lifecycle note`
- session `last message`
- task file in `.codeclaw/tasks/`
- recent timeline/output in `inspect --session <worker-id>`

If the job is delegated but not moving, also check:

- whether the job has exhausted its time or iteration budget
- whether the job is waiting on manual approval because `auto_approve` is not enabled
- whether the service cooldown window has elapsed since the last delegated continue

### Restart recovery looks incomplete

Check:

- `.codeclaw/state.json` exists and is writable
- the process had permission to persist updates during runtime
- the needed data falls within the rolling retention window

### Gateway delivery does not appear

Check:

- `cargo run -- gateway capabilities --channel <channel>`
- `cargo run -- job inspect <job-id>`
- `.codeclaw/state.json` for `report_subscriptions` and `report_deliveries`
- `.codeclaw/gateway/mock-outbox.jsonl` when using `mock_file`
- [docs/gateway-protocol.md](gateway-protocol.md) for expected capability downgrade behavior

### 7x24 delegated jobs stop unexpectedly

Check:

- `cargo run -- job inspect <job-id>` for `automation state`
- `cargo run -- inspect --service` for `last continued jobs` and `budget exhausted jobs`
- whether `--continue-for-secs` or `--continue-max-iterations` was too small
- whether the job is blocked and still waiting for manual approval

### Session automation does not fire

Check:

- `cargo run -- automation list` for `status`, `remaining runs`, `remaining secs`, `next run`, and `last error`
- the onboard `Automations` panel for a failed or paused state
- whether the target session still exists and is not currently busy
- whether `up` or `serve` is actively running the scheduler
- whether the automation already exhausted `--max-runs` or `--for-secs`

## Support Boundaries

This release does not yet provide:

- per-worker `git worktree` execution
- automated merge/integration orchestration
- full PTY replay for worker terminals
- hard path-lease enforcement

These items should be treated as planned follow-up work, not as operational defects in `0.13.1`.
