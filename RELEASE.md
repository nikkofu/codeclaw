# CodeClaw Release `v0.13.0`

Repository: `https://github.com/nikkofu/codeclaw`  
Release date: `2026-03-14`

## Suggested Git Tag

`v0.13.0`

## Suggested GitHub Release Title

`CodeClaw v0.13.0`

## GitHub Release Body

```md
CodeClaw `v0.13.0` focuses on transparent multi-session supervision.

This release builds on the `0.12.0` supervision baseline with local runtime monitoring, bounded session automations, a more operator-capable command bar, and a stronger onboard control surface. The goal is still the same: operators should be able to understand what the system is doing, why it stopped, and how to keep it running safely without silent failures, but now with less model indirection and lower operator friction.

## Highlights

- added a local codex-monitor snapshot plus onboard `Codex Sessions` visibility so runtime/session answers come from CodeClaw state instead of a model guess
- added session-targeted automations with `automation create|list|pause|resume|cancel`, plus an onboard `Automations` panel
- changed `codeclaw up` into an active foreground scheduler driver, so delegated loops and automations can keep progressing while the TUI is open
- added local slash-command control for `/monitor ...` and `/automation ...`, with completion, editing, and history improvements in the command bar
- persisted live runtime heartbeat into `.codeclaw/runtime.json` and expanded `inspect --service` with runtime pid/mode/turn visibility
- refreshed README, user guide, operations guide, project delivery notes, acceptance cases, and architecture references for `0.13.0`

## Included In This Release

- terminal-first `codeclaw` CLI and TUI control plane
- master/worker orchestration over `codex app-server`
- onboard supervision board for 7x24 oversight
- authoritative local runtime/session monitoring without routing monitor questions through Codex
- bounded delegated continuation through the scheduler driver in `codeclaw up` or `codeclaw serve`
- bounded repeated session automations targeting `master` or a specific worker
- channel-neutral gateway protocol and report delivery
- daily archived runtime and session logs

## Delivery Documentation

- Project delivery: `docs/project-delivery.md`
- User guide: `docs/user-guide.md`
- Operations guide: `docs/operations-guide.md`
- Acceptance use cases: `docs/acceptance-use-cases.md`
- Gateway protocol: `docs/gateway-protocol.md`
- Architecture: `docs/architecture.md`

## Key Commands

```bash
cargo run -- init
cargo run -- doctor
cargo run -- up
cargo run -- serve
cargo run -- inspect --service
cargo run -- job create --title "Nightly backlog sweep" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10
cargo run -- job create --title "Auto recovery" --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10 --auto-approve
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs"
cargo run -- automation list
cargo run -- gateway schema
```

## Upgrade Notes

- package version is now `0.13.0` in `Cargo.toml` and `Cargo.lock`
- preserve `.codeclaw/` before upgrading if supervision history, queued deliveries, or archived logs must be retained
- validate the environment with `cargo run -- doctor` after pulling the release
- review `[logging]` in `codeclaw.toml` to confirm retention and notification buffer settings

## Verification

- `cargo check`
- `cargo test`

## Known Boundaries

This release does not yet include:

- a real Slack / Telegram / WeCom / Feishu adapter
- per-worker `git worktree` execution
- hard path-lease enforcement
- merge queue / integration-branch automation
- full PTY terminal emulation in the right-side pane

## Repository

`https://github.com/nikkofu/codeclaw`
```

## Release Maintainer Checklist

1. Confirm `Cargo.toml` and `Cargo.lock` both show `0.13.0`.
2. Confirm `CHANGELOG.md`, `README.md`, `docs/user-guide.md`, and `docs/operations-guide.md` match the release scope.
3. Run:

   ```bash
   cargo check
   cargo test
   ```

4. Create the tag:

   ```bash
   git tag v0.13.0
   ```

5. Push the tag:

   ```bash
   git push origin v0.13.0
   ```

6. Create a GitHub Release with title `CodeClaw v0.13.0`.
7. Paste the `GitHub Release Body` block above into the release description.

## Related Files

- [CHANGELOG.md](CHANGELOG.md)
- [README.md](README.md)
- [docs/project-delivery.md](docs/project-delivery.md)
- [docs/user-guide.md](docs/user-guide.md)
- [docs/operations-guide.md](docs/operations-guide.md)
- [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
- [docs/gateway-protocol.md](docs/gateway-protocol.md)
