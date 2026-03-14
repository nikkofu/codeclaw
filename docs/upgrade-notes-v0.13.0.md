# CodeClaw v0.13.0 Upgrade Notes

## Scope

These notes describe the operator-facing changes between `v0.12.0` and `v0.13.0`.

Repository: `https://github.com/nikkofu/codeclaw`
Release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.0`

## Summary of Changes

- Added local codex-monitor visibility and onboard `Codex Sessions`
- Added bounded session automations and onboard `Automations`
- `cargo run -- up` now drives scheduler ticks in the foreground
- Added `/monitor ...` and `/automation ...` slash-command families
- Improved TUI input editing, completion, cursor handling, and multiline behavior
- Expanded `inspect --service` with live runtime heartbeat details

## Behavior Changes to Know

### 1. `up` is no longer just a viewer

In `v0.13.0`, `cargo run -- up` actively drives scheduler ticks while the TUI is open.

Operational impact:

- deferred jobs can be picked up while `up` is open
- delegated loops can continue while `up` is open
- session automations can dispatch while `up` is open
- `serve` is still useful for headless or IM-triggered runs, but it is no longer the only way to keep the scheduler moving

### 2. Monitoring questions should stay local

Operators should prefer:

- `/monitor sessions`
- `/monitor runtime`
- `/monitor jobs`
- `/monitor session <id>`
- `cargo run -- inspect --service`

These paths answer from CodeClaw's local persisted state instead of asking Codex to describe the system.

### 3. Session automation is now a first-class control primitive

New CLI:

```bash
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
cargo run -- automation pause AUTO-001
cargo run -- automation resume AUTO-001
cargo run -- automation cancel AUTO-001
```

New TUI slash commands:

```text
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
/automation pause AUTO-001
/automation resume AUTO-001
/automation cancel AUTO-001
```

### 4. Command bar interaction changed

Operators can now rely on:

- `Enter` from the default command bar to enter slash mode
- `Tab` completion for commands, flags, groups, session ids, and automation ids
- `Left` / `Right` / `Up` / `Down`, `Home`, `End`, `Backspace`, and `Delete`
- `Alt+Enter` or `Ctrl+J` for newline insertion
- `Ctrl+P` / `Ctrl+N` for history recall

## Upgrade Checklist

1. Pull the latest code and confirm `Cargo.toml` shows `0.13.0`.
2. Run:

   ```bash
   cargo check
   cargo test --quiet
   cargo run -- doctor
   ```

3. Review `README.md`, `docs/user-guide.md`, and `docs/operations-guide.md`.
4. If you use deferred jobs, delegated loops, or session automations, verify your runbooks now mention `up` or `serve` as scheduler drivers.
5. Review `codeclaw.toml` for logging retention and notification-channel capacity.
6. Preserve `.codeclaw/` before upgrading if operational history matters.

## Recommended Operator Validation

After upgrade, verify at least the following:

1. `cargo run -- up` shows `onboard` with live runtime information.
2. `/monitor sessions` reports real session data without going through Codex.
3. `cargo run -- inspect --service` shows runtime heartbeat plus scheduler information.
4. `cargo run -- automation create ...` creates a visible `AUTO-###` record.
5. `onboard` shows both `Codex Sessions` and `Automations`.

## Runbook Adjustments

Update operator runbooks to reflect these points:

- `up` can keep automation moving; `serve` is not mandatory for every continued workflow
- monitor-style questions should use CodeClaw monitor paths, not freeform prompts to Codex
- automation should always be bounded with `--max-runs`, `--for-secs`, or both unless there is explicit approval for open-ended supervision

## Reference Documents

- [../README.md](../README.md)
- [../RELEASE.md](../RELEASE.md)
- [user-guide.md](user-guide.md)
- [operations-guide.md](operations-guide.md)
- [project-delivery.md](project-delivery.md)
