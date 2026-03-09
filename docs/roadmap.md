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
- status files and session monitoring under `.codeclaw/`

## Next Phase

The next engineering target is to harden supervision into long-running operator workflows:

- add right-pane filtering/folding so command and output noise can be focused on demand
- expose more explicit worker lifecycle milestones such as spawn requested, bootstrapped, blocked, and handed back
- persist selected portions of live output so restart recovery includes more than the timeline
- add explicit batch inspection commands and/or a dedicated TUI batch view

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
