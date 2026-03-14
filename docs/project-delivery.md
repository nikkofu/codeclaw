# CodeClaw Project Delivery

## Release Metadata

| Item | Value |
| --- | --- |
| Product | CodeClaw |
| Version | `0.13.1` |
| Delivery date | `2026-03-14` |
| Repository | `https://github.com/nikkofu/codeclaw` |
| Primary package | `codeclaw` |
| Language/runtime | Rust 2021 CLI/TUI application |
| External dependency | Authenticated `codex` CLI with `app-server` support |

## Delivery Scope

This delivery provides a terminal-first control plane for supervising one master Codex session and multiple worker sessions in a shared repository. The current release is suitable for controlled pilot delivery and operator-guided use in a real repository.

Release note: `0.13.1` is a patch-level delivery freeze on top of the `0.13.0` monitor-and-automation control-plane scope. It synchronizes version metadata and ensures the tagged snapshot also contains the release announcement and upgrade notes used during handoff.

Delivered scope in `0.13.1`:

- master and worker orchestration over `codex app-server`
- CLI commands for initialization, health checks, worker creation, prompt dispatch, and inspection
- a `ratatui`-based supervision interface with session navigation, timeline review, and live-output review
- persisted coordination state under `.codeclaw/`
- persisted supervision history including timeline, output tail, live buffer recovery, and lifecycle notes
- batch-level orchestration inspection for one full master-to-worker execution chain
- durable job reports, subscriptions, and delivery outbox state
- a gateway compatibility contract for future IM and webhook adapters
- a delivery-safe `mock_file` gateway path for integration validation
- a virtual `onboard` supervisor board for kanban-style 7x24 oversight
- a local codex-monitor view for authoritative session/runtime visibility without asking Codex to describe itself
- bounded delegated master-loop automation with time and iteration guards
- bounded session-targeted automations with interval, run-count, and duration guards
- foreground scheduler ticks while `cargo run -- up` is open, plus headless scheduler ticks through `cargo run -- serve`
- daily archived runtime and session logs with configurable retention

## Included Artifacts

- source code in the repository root
- release summary in [CHANGELOG.md](../CHANGELOG.md)
- architecture reference in [docs/architecture.md](architecture.md)
- operator instructions in [docs/user-guide.md](user-guide.md)
- deployment and support instructions in [docs/operations-guide.md](operations-guide.md)
- acceptance scenarios in [docs/acceptance-use-cases.md](acceptance-use-cases.md)
- gateway protocol contract in [docs/gateway-protocol.md](gateway-protocol.md)
- sample configuration in [codeclaw.example.toml](../codeclaw.example.toml)

## Strategic Planning Addendum

The repository also includes a next-phase planning package derived from the `0.13.1` baseline. These documents describe the intended evolution path and do not imply that the capabilities are already delivered in the current release.

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
5. Review state with onboard, `inspect`, `.codeclaw/status/*.json`, and `.codeclaw/state.json`.
6. Keep either `cargo run -- up` or `cargo run -- serve` active when deferred jobs, delegated loops, or session automations should continue running.

## Primary Deliverables

### User-facing capabilities

- single-window supervision of master and worker sessions
- onboard supervision with kanban lanes, `Codex Sessions`, and `Automations` panels
- lifecycle-aware worker states: `spawn_requested`, `bootstrapping`, `bootstrapped`, `blocked`, `handed_back`, `failed`
- queue-aware prompt routing for busy sessions
- restart recovery for recent timeline and output context
- CLI inspection without launching the TUI
- local monitor answers for session counts, runtime health, latest prompts, and recent assistant previews
- bounded repeated prompts into `master` or a specific worker through `automation create|list|pause|resume|cancel`
- explicit gateway schema visibility and per-channel capability inspection
- durable report subscription management for remote delivery paths

### Operational outputs

- `.codeclaw/state.json` for persisted supervision state
- `.codeclaw/status/*.json` for per-session status snapshots
- `.codeclaw/tasks/` for worker task files
- `.codeclaw/logs/archive/YYYY-MM-DD/sessions/*.jsonl` for archived session notifications
- `.codeclaw/logs/archive/YYYY-MM-DD/runtime/*.jsonl` for archived controller and app-server runtime logs
- `.codeclaw/runtime.json` for the latest persisted live runtime heartbeat
- `.codeclaw/service.json` for the latest persisted scheduler heartbeat
- `.codeclaw/gateway/mock-outbox.jsonl` for mock gateway delivery replay

## Acceptance Summary

The recommended delivery acceptance set is captured in [docs/acceptance-use-cases.md](acceptance-use-cases.md). A formal acceptance run should at minimum verify:

- workspace initialization
- health checks
- master prompt dispatch
- worker spawn and lifecycle transitions
- local monitor visibility from onboard or `inspect --service`
- blocker propagation
- restart recovery of supervision data
- CLI inspection output
- bounded delegated-loop continuation
- bounded session automation lifecycle
- gateway schema and capability inspection
- mock gateway delivery emission

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
- keep at least one scheduler driver (`cargo run -- up` or `cargo run -- serve`) active when deferred jobs, delegated loops, or session automations must keep progressing
- use [docs/operations-guide.md](operations-guide.md) for upgrade, backup, and troubleshooting procedures
- use [docs/user-guide.md](user-guide.md) for daily operation and [docs/acceptance-use-cases.md](acceptance-use-cases.md) for formal sign-off
