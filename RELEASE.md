# CodeClaw Release `v0.11.0`

Repository: `https://github.com/nikkofu/codeclaw`  
Release date: `2026-03-13`

## Suggested Git Tag

`v0.11.0`

## Suggested GitHub Release Title

`CodeClaw v0.11.0`

## GitHub Release Body

```md
CodeClaw `v0.11.0` turns the current reporting and remote-control direction into a concrete, compatible gateway baseline.

This release adds a formal IM/webhook compatibility contract, integrates a gateway-backed delivery path, and exposes the protocol through CLI inspection and subscription commands. It keeps `codex app-server` as the execution runtime while making CodeClaw more transparent, more integration-ready, and easier to operate in long-running service mode.

## Highlights

- added a normalized gateway protocol for text, markdown, links, image, audio, video, file, typing, and raw `type/event/hook` semantics
- integrated queued report delivery through one gateway abstraction instead of a controller-local console-only path
- added a delivery-safe `mock_file` channel for schema validation, IM adapter development, and outbox replay
- added `gateway schema`, `gateway capabilities`, and `gateway subscribe` CLI commands for operator visibility and control
- improved `spawn` progress rendering so non-TTY environments now print visible status updates instead of appearing silent
- synchronized release metadata and delivery documentation to `0.11.0`

## Included In This Release

- terminal-first `codeclaw` CLI and TUI control plane
- master/worker orchestration over `codex app-server`
- durable jobs, reports, subscriptions, and delivery outbox state
- service-mode heartbeat and queued report dispatch
- channel-neutral gateway protocol and compatibility documentation

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
cargo run -- serve --once
cargo run -- gateway schema
cargo run -- gateway capabilities --channel mock-file
cargo run -- gateway subscribe --job JOB-001 --channel mock-file
cargo run -- spawn --group backend --task "Payment API refactor"
cargo run -- inspect --service
```

## Upgrade Notes

- package version is now `0.11.0` in `Cargo.toml` and `Cargo.lock`
- preserve `.codeclaw/` before upgrading if supervision history or queued delivery state must be retained
- validate the environment with `cargo run -- doctor` after pulling the release
- use `cargo run -- gateway schema` and `cargo run -- gateway capabilities --channel ...` to validate downstream IM compatibility assumptions

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

1. Confirm `Cargo.toml` and `Cargo.lock` both show `0.11.0`.
2. Confirm `CHANGELOG.md`, `README.md`, and `docs/gateway-protocol.md` match the release scope.
3. Run:

   ```bash
   cargo check
   cargo test
   ```

4. Create the tag:

   ```bash
   git tag v0.11.0
   ```

5. Push the tag:

   ```bash
   git push origin v0.11.0
   ```

6. Create a GitHub Release with title `CodeClaw v0.11.0`.
7. Paste the `GitHub Release Body` block above into the release description.

## Related Files

- [CHANGELOG.md](CHANGELOG.md)
- [README.md](README.md)
- [docs/project-delivery.md](docs/project-delivery.md)
- [docs/user-guide.md](docs/user-guide.md)
- [docs/operations-guide.md](docs/operations-guide.md)
- [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
- [docs/gateway-protocol.md](docs/gateway-protocol.md)
