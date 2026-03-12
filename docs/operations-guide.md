# CodeClaw Operations Guide

## Purpose

This guide describes installation, deployment, backup, upgrade, and troubleshooting practices for CodeClaw `0.10.0`.

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

Recommended practice:

- review group definitions before production use
- align `lease_paths` with actual repository ownership boundaries
- keep approval/sandbox settings consistent with the intended operating model

## Health Checks

Primary operational checks:

- `cargo run -- doctor`
- `cargo run -- list`
- `cargo run -- inspect --session master`

Healthy baseline indicators:

- configuration loads successfully
- `codex app-server` responds
- master session can be initialized or resumed
- `.codeclaw/status/master.json` is updated during runtime

## Persistence and Backup

Operational state lives in `.codeclaw/`.

Backup priorities:

1. `.codeclaw/state.json`
2. `.codeclaw/status/`
3. `.codeclaw/tasks/`
4. `.codeclaw/logs/`

Backup guidance:

- preserve `.codeclaw/` before upgrades if historical supervision data matters
- include `.codeclaw/tasks/` in incident evidence capture
- retain `.codeclaw/logs/*.jsonl` for runtime event reconstruction

## Upgrade Procedure

Recommended upgrade path for same-workspace deployments:

1. stop active operator sessions
2. preserve `.codeclaw/` as a backup
3. update the repository to the target release
4. verify `Cargo.toml` version and `CHANGELOG.md`
5. run `cargo test`
6. run `cargo run -- doctor`
7. restart with `cargo run -- up`
8. confirm session restoration with `inspect --session master`

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

### Worker appears blocked

Check:

- session `lifecycle note`
- session `last message`
- task file in `.codeclaw/tasks/`
- recent timeline/output in `inspect --session <worker-id>`

### Restart recovery looks incomplete

Check:

- `.codeclaw/state.json` exists and is writable
- the process had permission to persist updates during runtime
- the needed data falls within the rolling retention window

## Support Boundaries

This release does not yet provide:

- per-worker `git worktree` execution
- automated merge/integration orchestration
- full PTY replay for worker terminals
- hard path-lease enforcement

These items should be treated as planned follow-up work, not as operational defects in `0.10.0`.
