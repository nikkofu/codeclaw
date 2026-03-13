# CodeClaw Release `v0.12.0`

Repository: `https://github.com/nikkofu/codeclaw`  
Release date: `2026-03-13`

## Suggested Git Tag

`v0.12.0`

## Suggested GitHub Release Title

`CodeClaw v0.12.0`

## GitHub Release Body

```md
CodeClaw `v0.12.0` focuses on stable long-running supervision.

This release adds a default `onboard` supervisor session, bounded 7x24 continuation controls, daily archived logging, and a non-fatal recovery path for app-server notification lag. The goal is simple: operators should be able to understand what the system is doing, why it stopped, and how to keep it running safely without silent failures.

## Highlights

- added a default virtual `onboard` supervision session with a kanban-like board for pending, running, blocked, completed, and failed jobs
- added bounded master-loop delegation controls with `delegate-master-loop`, `continue-for-secs`, `continue-max-iterations`, and visible `auto-approve` markers
- changed `app-server notification channel error: channel lagged by N` from a fatal turn error into a logged warning with continued processing
- increased app-server notification buffer capacity and made it configurable through `[logging].notification_channel_capacity`
- added daily archived JSONL logs under `.codeclaw/logs/archive/YYYY-MM-DD/` with configurable retention, defaulting to 30 days
- added runtime log coverage for controller-side lag warnings, app-server stderr, parse failures, and stdout-closed conditions
- refreshed README, user guide, operations guide, acceptance cases, and release metadata for `0.12.0`

## Included In This Release

- terminal-first `codeclaw` CLI and TUI control plane
- master/worker orchestration over `codex app-server`
- onboard supervision board for 7x24 oversight
- bounded delegated continuation through `codeclaw serve`
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
cargo run -- gateway schema
```

## Upgrade Notes

- package version is now `0.12.0` in `Cargo.toml` and `Cargo.lock`
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

1. Confirm `Cargo.toml` and `Cargo.lock` both show `0.12.0`.
2. Confirm `CHANGELOG.md`, `README.md`, `docs/user-guide.md`, and `docs/operations-guide.md` match the release scope.
3. Run:

   ```bash
   cargo check
   cargo test
   ```

4. Create the tag:

   ```bash
   git tag v0.12.0
   ```

5. Push the tag:

   ```bash
   git push origin v0.12.0
   ```

6. Create a GitHub Release with title `CodeClaw v0.12.0`.
7. Paste the `GitHub Release Body` block above into the release description.

## Related Files

- [CHANGELOG.md](CHANGELOG.md)
- [README.md](README.md)
- [docs/project-delivery.md](docs/project-delivery.md)
- [docs/user-guide.md](docs/user-guide.md)
- [docs/operations-guide.md](docs/operations-guide.md)
- [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
- [docs/gateway-protocol.md](docs/gateway-protocol.md)
