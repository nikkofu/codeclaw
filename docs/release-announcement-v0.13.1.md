# CodeClaw v0.13.1 Release Announcement

CodeClaw `v0.13.1` is now available.

This patch release freezes the full `0.13.x` delivery package into a clean tagged snapshot. The runtime scope is the same monitor-and-automation control plane introduced in `v0.13.0`, but now the release announcement, operator upgrade notes, and synchronized delivery references are included directly in the tagged repository state.

## What Is New

- Finalized the `0.13.x` delivery package in a new tagged release
- Included release announcement and upgrade notes inside the tagged repository snapshot
- Preserved the `v0.13.0` runtime scope: local codex-monitor visibility, session automations, foreground scheduler ticks in `up`, improved slash-command UX, and expanded `inspect --service`

## Why This Matters

The practical goal is straightforward: when a team downloads the tagged release snapshot, the release notes and operator handoff package should already be inside it. `v0.13.1` closes that gap without changing the operator-facing runtime behavior introduced in `v0.13.0`.

- the tagged snapshot now includes the full release package
- operator upgrade guidance ships with the release itself
- repository metadata, version references, and delivery links are synchronized
- the monitor-and-automation control-plane scope remains clear and stable

## Representative Commands

```bash
cargo run -- up
cargo run -- inspect --service
cargo run -- automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
cargo run -- automation list
```

Inside `up`:

```text
/monitor sessions
/monitor runtime
/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 "Review blocked jobs and continue"
/automation list
```

## Release References

- GitHub Release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`
- Release notes: [../RELEASE.md](../RELEASE.md)
- User guide: [user-guide.md](user-guide.md)
- Operations guide: [operations-guide.md](operations-guide.md)
- Project delivery: [project-delivery.md](project-delivery.md)

## Suggested External Post

```md
CodeClaw v0.13.1 is live.

This patch release finalizes the 0.13.x delivery package and freezes the complete monitor-and-automation control-plane documentation into the tagged repository snapshot.

Highlights:
- complete tagged delivery package
- bundled release announcement and operator upgrade notes
- synchronized repository metadata and delivery references
- preserves the v0.13.0 runtime scope for monitoring, automation, and onboard supervision

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repo: https://github.com/nikkofu/codeclaw
```
