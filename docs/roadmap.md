# CodeClaw Roadmap

## Current Phase

CodeClaw now has a functioning master-worker control loop:

- one visible TUI shell
- a persistent master session
- worker creation from master actions
- queued follow-up turns for busy sessions
- automatic worker completion updates routed back to master
- a structured right-pane timeline for orchestration and execution state
- persisted timeline and batch history across CLI/TUI process boundaries
- a dedicated batch inspection view inside the TUI
- color-coded and animated task-state supervision in the TUI
- right-pane focus filters for summary, command, and error supervision
- explicit worker lifecycle milestones in supervision state and UI
- persisted lifecycle notes for blocker and handoff context in state, status files, and inspection views
- CLI session and batch inspection without launching the TUI
- status files and session monitoring under `.codeclaw/`
- persisted rolling live-output tail, including in-flight assistant text, so restarts recover more than the timeline

## Next Phase

The next engineering target is to turn CodeClaw into a long-running agentic operations system on top of `codex app-server` rather than a TUI-only local prototype.

Primary next-phase goals:

- introduce a persistent `Job` model above batches and sessions
- add `codeclaw serve` for 24x7 background orchestration
- add proactive progress, blocker, and completion reporting
- add a gateway-ready control plane so IM integration does not distort core orchestration logic
- support explicit planner/executor/reviewer and approval-gated orchestration patterns as job policy, not ad hoc prompt forks
- keep provider routing below CodeClaw so Codex runtime configuration can continue to use official or third-party Responses-compatible backends

Detailed next-phase documents:

- [Product Strategy](product-strategy.md)
- [System Architecture vNext](system-architecture-v2.md)
- [Technical Roadmap](technical-roadmap.md)
- [Project Plan](project-plan.md)

## After That

Once job orchestration, reporting, and gateway support are stable, CodeClaw should move deeper into workspace isolation:

- per-worker `git worktree` creation
- branch lifecycle management
- leased path enforcement before dispatch
- integration queue and conflict surfacing
- optional worker attach mode for manual intervention

## Product Goal

The intended end state remains:

- the user or external gateway speaks to one CodeClaw control plane
- CodeClaw coordinates master and worker Codex sessions over long-running jobs
- progress, blockers, approvals, and outcomes stay transparent across TUI, CLI, and gateway channels
- code changes stay isolated and mergeable
