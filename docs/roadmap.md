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

The next engineering target is to harden supervision into long-running operator workflows:

- deepen right-pane folding and saved filter presets so operators can keep stable inspection views across long sessions
- add machine-readable CLI output modes for scripting, dashboards, and external status polling

## After That

Once supervision history is stable, CodeClaw should move deeper into workspace isolation:

- per-worker `git worktree` creation
- branch lifecycle management
- leased path enforcement before dispatch
- integration queue and conflict surfacing
- optional worker attach mode for manual intervention

## Product Goal

The intended end state remains:

- the user speaks only to the master Codex
- the master schedules and supervises child Codex workers
- the UI shows each worker title, status, summary, and live execution state
- code changes stay isolated and mergeable
