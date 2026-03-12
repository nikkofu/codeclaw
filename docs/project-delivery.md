# CodeClaw Project Delivery

## Release Metadata

| Item | Value |
| --- | --- |
| Product | CodeClaw |
| Version | `0.10.0` |
| Delivery date | `2026-03-12` |
| Repository | `https://github.com/nikkofu/codeclaw` |
| Primary package | `codeclaw` |
| Language/runtime | Rust 2021 CLI/TUI application |
| External dependency | Authenticated `codex` CLI with `app-server` support |

## Delivery Scope

This delivery provides a terminal-first control plane for supervising one master Codex session and multiple worker sessions in a shared repository. The current release is suitable for controlled pilot delivery and operator-guided use in a real repository.

Delivered scope in `0.10.0`:

- master and worker orchestration over `codex app-server`
- CLI commands for initialization, health checks, worker creation, prompt dispatch, and inspection
- a `ratatui`-based supervision interface with session navigation, timeline review, and live-output review
- persisted coordination state under `.codeclaw/`
- persisted supervision history including timeline, output tail, live buffer recovery, and lifecycle notes
- batch-level orchestration inspection for one full master-to-worker execution chain

## Included Artifacts

- source code in the repository root
- release summary in [CHANGELOG.md](../CHANGELOG.md)
- architecture reference in [docs/architecture.md](architecture.md)
- operator instructions in [docs/user-guide.md](user-guide.md)
- deployment and support instructions in [docs/operations-guide.md](operations-guide.md)
- acceptance scenarios in [docs/acceptance-use-cases.md](acceptance-use-cases.md)
- sample configuration in [codeclaw.example.toml](../codeclaw.example.toml)

## Strategic Planning Addendum

The repository also includes a next-phase planning package derived from the `0.10.0` baseline. These documents describe the intended evolution path and do not imply that the capabilities are already delivered in the current release.

- [docs/product-strategy.md](product-strategy.md)
- [docs/system-architecture-v2.md](system-architecture-v2.md)
- [docs/technical-roadmap.md](technical-roadmap.md)
- [docs/project-plan.md](project-plan.md)

## Runtime Requirements

- Rust toolchain capable of building the workspace
- authenticated `codex` CLI installed on the target machine
- terminal environment that supports raw-mode TUI rendering
- network access for Codex model turns
- writable working directory for `.codeclaw/` state and logs

## Delivery Baseline

The expected baseline workflow is:

1. Initialize the workspace with `cargo run -- init`.
2. Verify environment readiness with `cargo run -- doctor`.
3. Launch the supervision UI with `cargo run -- up`.
4. Dispatch work through the master session or by spawning workers directly.
5. Review state with `inspect`, `.codeclaw/status/*.json`, and `.codeclaw/state.json`.

## Primary Deliverables

### User-facing capabilities

- single-window supervision of master and worker sessions
- lifecycle-aware worker states: `spawn_requested`, `bootstrapping`, `bootstrapped`, `blocked`, `handed_back`, `failed`
- queue-aware prompt routing for busy sessions
- restart recovery for recent timeline and output context
- CLI inspection without launching the TUI

### Operational outputs

- `.codeclaw/state.json` for persisted supervision state
- `.codeclaw/status/*.json` for per-session status snapshots
- `.codeclaw/tasks/` for worker task files
- `.codeclaw/logs/*.jsonl` for event logging

## Acceptance Summary

The recommended delivery acceptance set is captured in [docs/acceptance-use-cases.md](acceptance-use-cases.md). A formal acceptance run should at minimum verify:

- workspace initialization
- health checks
- master prompt dispatch
- worker spawn and lifecycle transitions
- blocker propagation
- restart recovery of supervision data
- CLI inspection output

## Known Delivery Boundaries

This release is intentionally not positioned as a full multi-worktree automation platform yet. The following items remain outside the current delivery scope:

- dedicated per-worker `git worktree` execution
- hard lease enforcement on changed paths
- merge queue and integration-branch automation
- full PTY terminal emulation in the right-side pane

## Handover Checklist

- confirm the target environment has `codex` authenticated before final acceptance
- review `codeclaw.toml` against repository-specific group and lease-path needs
- retain `.codeclaw/` when operational history must survive restarts
- use [docs/operations-guide.md](operations-guide.md) for upgrade, backup, and troubleshooting procedures
- use [docs/user-guide.md](user-guide.md) for daily operation and [docs/acceptance-use-cases.md](acceptance-use-cases.md) for formal sign-off
