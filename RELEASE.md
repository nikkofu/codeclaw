# CodeClaw Release `v0.13.1`

Repository: `https://github.com/nikkofu/codeclaw`  
Release date: `2026-03-14`

## Suggested Git Tag

`v0.13.1`

## Suggested GitHub Release Title

`CodeClaw v0.13.1`

## GitHub Release Body

```md
CodeClaw `v0.13.1` finalizes the `0.13.x` release package.

This patch release does not introduce a new runtime feature set beyond `v0.13.0`. Its purpose is to freeze the complete delivery package into a clean tagged snapshot, including the release announcement, upgrade notes, and synchronized repository metadata for the monitor-and-automation control-plane release line.

## Highlights

- finalized the `0.13.x` delivery package so the tagged release includes the release announcement and operator upgrade notes
- synchronized repository version metadata, README delivery links, and release-maintainer materials to `0.13.1`
- preserves the `v0.13.0` runtime scope: local codex-monitor visibility, session automations, foreground scheduler ticks in `up`, improved slash-command UX, and expanded `inspect --service`

## Included In This Release

- the full `v0.13.0` monitor-and-automation control-plane scope
- release announcement and upgrade notes frozen into the tagged repository snapshot
- synchronized delivery documentation and release-maintainer assets

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
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs"
cargo run -- automation list
cargo run -- gateway schema
```

## Upgrade Notes

- package version is now `0.13.1` in `Cargo.toml` and `Cargo.lock`
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

1. Confirm `Cargo.toml` and `Cargo.lock` both show `0.13.1`.
2. Confirm `CHANGELOG.md`, `README.md`, `docs/user-guide.md`, and `docs/operations-guide.md` match the release scope.
3. Run:

   ```bash
   cargo check
   cargo test
   ```

4. Create the tag:

   ```bash
   git tag v0.13.1
   ```

5. Push the tag:

   ```bash
   git push origin v0.13.1
   ```

6. Create a GitHub Release with title `CodeClaw v0.13.1`.
7. Paste the `GitHub Release Body` block above into the release description.

## Related Files

- [CHANGELOG.md](CHANGELOG.md)
- [README.md](README.md)
- [docs/project-delivery.md](docs/project-delivery.md)
- [docs/user-guide.md](docs/user-guide.md)
- [docs/operations-guide.md](docs/operations-guide.md)
- [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
- [docs/gateway-protocol.md](docs/gateway-protocol.md)
