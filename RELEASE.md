# CodeClaw Release `v0.10.0`

Repository: `https://github.com/nikkofu/codeclaw`  
Release date: `2026-03-12`

## Suggested Git Tag

`v0.10.0`

## Suggested GitHub Release Title

`CodeClaw v0.10.0`

## GitHub Release Body

```md
CodeClaw `v0.10.0` is a delivery-focused release that brings the repository metadata, supervision model, and delivery documentation set into a publishable state.

This release strengthens restart recovery, lifecycle-aware supervision, and formal project handoff materials for pilot or controlled production adoption.

## Highlights

- persisted rolling live-output tail per session, including in-flight assistant text, so restart recovery restores more than the structured timeline
- persisted worker lifecycle notes so blocker reasons and handoff annotations remain visible in both TUI and CLI inspection
- improved supervision continuity across restarts with restored timeline, output, and lifecycle context
- synchronized package metadata with the current GitHub release line at `0.10.0`
- added a formal delivery documentation set for handoff, usage, operations, and acceptance
- added visible CLI progress feedback for `spawn` so worker bootstrap waits are no longer silent

## Included In This Release

- terminal-first `codeclaw` CLI and TUI control plane
- master/worker orchestration over `codex app-server`
- queued prompt handling for busy sessions
- session timeline supervision and batch inspection
- CLI inspection for sessions and batches
- persisted supervision state under `.codeclaw/`

## Delivery Documentation

- Project delivery: `docs/project-delivery.md`
- User guide: `docs/user-guide.md`
- Operations guide: `docs/operations-guide.md`
- Acceptance use cases: `docs/acceptance-use-cases.md`
- Architecture: `docs/architecture.md`

## Key Commands

```bash
cargo run -- init
cargo run -- doctor
cargo run -- up
cargo run -- spawn --group backend --task "Payment API refactor"
cargo run -- send --to master "Plan the next backend refactor step."
cargo run -- inspect --session master --events 8 --output 6
cargo run -- inspect --batch 3 --events 12
```

## Upgrade Notes

- package version is now `0.10.0` in `Cargo.toml` and `Cargo.lock`
- preserve `.codeclaw/` before upgrading if supervision history must be retained
- validate the environment with `cargo run -- doctor` after pulling the release

## Verification

- `cargo test`
- 13 tests passed

## Known Boundaries

This release does not yet include:

- per-worker `git worktree` execution
- hard path-lease enforcement
- merge queue / integration-branch automation
- full PTY terminal emulation in the right-side pane

## Repository

`https://github.com/nikkofu/codeclaw`
```

## Release Maintainer Checklist

1. Confirm `Cargo.toml` and `Cargo.lock` both show `0.10.0`.
2. Confirm `CHANGELOG.md` and `README.md` match the release scope.
3. Run:

   ```bash
   cargo test
   ```

4. Create the tag:

   ```bash
   git tag v0.10.0
   ```

5. Push the tag:

   ```bash
   git push origin v0.10.0
   ```

6. Create a GitHub Release with title `CodeClaw v0.10.0`.
7. Paste the `GitHub Release Body` block above into the release description.

## Related Files

- [CHANGELOG.md](CHANGELOG.md)
- [README.md](README.md)
- [docs/project-delivery.md](docs/project-delivery.md)
- [docs/user-guide.md](docs/user-guide.md)
- [docs/operations-guide.md](docs/operations-guide.md)
- [docs/acceptance-use-cases.md](docs/acceptance-use-cases.md)
