# CodeClaw Roadmap

## Current Phase

CodeClaw now has a functioning master-worker control loop:

- one visible TUI shell
- a persistent master session
- worker creation from master actions
- queued follow-up turns for busy sessions
- automatic worker completion updates routed back to master
- status files and session monitoring under `.codeclaw/`

## Next Phase

The next engineering target is to make orchestration easier to supervise:

- richer right-pane execution view with clearer command/output grouping
- explicit orchestration timeline for master -> worker dispatch, wait, complete, fail
- stronger queue visibility in the sidebar and detail panel
- better distinction between direct user prompts and internal runtime prompts

## After That

Once orchestration visibility is solid, CodeClaw should move into workspace isolation:

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
