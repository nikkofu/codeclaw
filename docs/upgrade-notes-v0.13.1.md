# CodeClaw v0.13.1 Upgrade Notes

## Scope

These notes describe the packaging and operator-documentation changes between `v0.13.0` and `v0.13.1`.

Repository: `https://github.com/nikkofu/codeclaw`
Release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`

## Summary of Changes

- Finalized the tagged release package for the `0.13.x` control-plane line
- Included release announcement and upgrade notes directly in the tagged snapshot
- Synchronized version metadata, README delivery links, and release-maintainer materials to `0.13.1`
- Preserved the runtime behavior introduced in `v0.13.0`

## Behavior Changes to Know

### 1. Runtime behavior is unchanged from `v0.13.0`

This patch release does not change the runtime behavior introduced in `v0.13.0`.

Operational impact:

- local monitor visibility behaves the same
- session automations behave the same
- `up` still acts as a scheduler driver
- slash-command and input-bar behavior stay the same

### 2. What changed operationally

What changed is the release packaging:

- the tagged release now contains the release announcement
- the tagged release now contains the operator upgrade notes
- README delivery links now point at the files included in this tagged snapshot
- package and release metadata are synchronized at `0.13.1`

### 3. Recommended operator posture

Operators should continue to prefer local monitor and control paths:

- `/monitor sessions`
- `/monitor runtime`
- `/monitor jobs`
- `/monitor session <id>`
- `/automation create ...`
- `/automation list`
- `cargo run -- inspect --service`

## Upgrade Checklist

1. Pull the latest code and confirm `Cargo.toml` shows `0.13.1`.
2. Run:

   ```bash
   cargo check
   cargo test --quiet
   cargo run -- doctor
   ```

3. Review `README.md`, `docs/user-guide.md`, and `docs/operations-guide.md`.
4. If you use deferred jobs, delegated loops, or session automations, verify your runbooks still mention `up` or `serve` as scheduler drivers.
5. Review `codeclaw.toml` for logging retention and notification-channel capacity.
6. Preserve `.codeclaw/` before upgrading if operational history matters.

## Recommended Operator Validation

After upgrade, verify at least the following:

1. `cargo run -- up` shows `onboard` with live runtime information.
2. `/monitor sessions` reports real session data without going through Codex.
3. `cargo run -- inspect --service` shows runtime heartbeat plus scheduler information.
4. `cargo run -- automation create ...` creates a visible `AUTO-###` record.
5. The tagged release snapshot now includes these upgrade notes and the release announcement.

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
