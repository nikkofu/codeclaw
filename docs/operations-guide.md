# CodeClaw Operations Guide

## Purpose

This guide describes installation, deployment, backup, upgrade, and troubleshooting practices for CodeClaw `0.12.0`.

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
- `cargo run -- gateway capabilities --channel mock-file`

Healthy baseline indicators:

- configuration loads successfully
- `codex app-server` responds
- master session can be initialized or resumed
- `.codeclaw/status/master.json` is updated during runtime
- gateway capability output matches the intended downstream integration assumptions
- `cargo run -- inspect --service` shows sane delegated, auto-approve, and exhausted-budget counts when automation is enabled

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
- retain `.codeclaw/logs/*.jsonl` for runtime event reconstruction
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

- always set either a time budget, an iteration budget, or both for 7x24 jobs
- prefer small first budgets such as `3600` seconds or `10` iterations while tuning
- monitor `budget exhausted jobs` in `inspect --service`
- treat `auto_approve` as an explicit operational risk decision and keep it visible in runbooks

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
- `.codeclaw/logs/*.jsonl`
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

## Support Boundaries

This release does not yet provide:

- per-worker `git worktree` execution
- automated merge/integration orchestration
- full PTY replay for worker terminals
- hard path-lease enforcement

These items should be treated as planned follow-up work, not as operational defects in `0.12.0`.
