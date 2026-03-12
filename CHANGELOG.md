# Changelog

## Unreleased

- added next-phase planning documents for product strategy, system architecture, technical roadmap, and project plan
- aligned `README.md`, `docs/architecture.md`, and `docs/roadmap.md` so the current release baseline and next-phase direction are linked clearly
- added a durable `Job` model with persisted job state, job-aware batch/worker linkage, and CLI commands for `jobs`, `job create`, and `job inspect`
- added optional `--job` association for `send` and `spawn` so orchestration batches can be tracked under a named job
- added `codeclaw serve` as a service-mode skeleton with scheduler ticks, persisted `service.json` heartbeat, and `inspect --service`
- added service-side intake for pending jobs so background mode can submit new jobs into the master orchestration loop
- added persisted job report history with accepted/progress/blocker/completion/failure/digest report kinds
- extended `job inspect` and `inspect --service` so operators can see recent reports and report cadence state before any gateway integration

## 0.10.0 - 2026-03-12

- persisted a rolling tail of live output per session, including in-flight assistant buffers, so restarts restore more than the timeline
- restored persisted output into session views and `codeclaw inspect` output
- persisted worker lifecycle notes so blocker reasons and handoff annotations survive restarts and show up in TUI/CLI inspection
- synchronized package/version metadata to the GitHub repository release line
- added a formal delivery documentation set covering handoff, user workflows, operations, and acceptance use cases
- added visible CLI progress feedback for `spawn`, including spinner updates and streamed worker log lines during bootstrap waits

## 0.9.0 - 2026-03-10

- added `codeclaw inspect --session ...` and `codeclaw inspect --batch ...` for terminal-side supervision without launching the TUI
- reused persisted session and batch snapshots so CLI inspection reflects the same orchestration state shown in the UI
- formatted recent timeline and output slices into readable command-line summaries for faster operator checks

## 0.8.0 - 2026-03-10

- added explicit worker lifecycle states including `spawn_requested`, `bootstrapping`, `bootstrapped`, `blocked`, and `handed_back`
- surfaced lifecycle-aware worker supervision across persisted state, status files, runtime updates, and the TUI
- detected common blocker phrases from worker replies so blocked workers stand out instead of looking merely completed
- extended the master runtime prompt so orchestration decisions can react differently to bootstrap handoff, blocker, and failed-worker events

## 0.7.0 - 2026-03-09

- added right-pane focus modes so `f` can cycle between all, summary, command, and error supervision views
- colorized timeline bodies and live-output lines by source so assistant text, command activity, and failures are easier to scan
- surfaced visible/total counts for filtered timeline, batch, and output panels

## 0.6.0 - 2026-03-09

- added color-coded session and batch supervision cues across the TUI
- animated running and queued states with lightweight ASCII spinners in the sidebar, panel headers, and terminal title
- tinted panel borders and the status bar by task state so failures, completions, queueing, and active execution are easier to distinguish at a glance
- strengthened sidebar metadata with role/group badges and active-session counts

## 0.5.0 - 2026-03-09

- added a dedicated TUI batch inspection view for replaying one orchestration chain across sessions
- added `b`, `[` and `]` keyboard controls for toggling batch view and navigating historical batches
- aggregated batch timelines from persisted per-session history so master and worker events can be reviewed as one stream
- surfaced batch metadata such as root prompt, related sessions, status, and last event directly in the right pane

## 0.4.0 - 2026-03-09

- persisted per-session timeline history into `.codeclaw/state.json` so supervision context survives `codeclaw up` restarts
- persisted orchestration batch metadata, including root prompt, status, involved sessions, and last event
- restored saved timeline history when rebuilding session views from disk
- surfaced batch ids directly in the TUI detail pane and timeline rows

## 0.3.1 - 2026-03-09

- scoped CLI quiescence waiting to the active orchestration batch instead of every session globally
- propagated orchestration batch ids across master prompts, worker bootstrap turns, worker follow-ups, and runtime feedback loops
- kept the new timeline supervision model intact while removing false waits from unrelated sessions

## 0.3.0 - 2026-03-09

- added a structured timeline panel to the right side of the TUI
- tagged timeline events by source so user, bootstrap, orchestrator, runtime, command, status, and error activity are visually distinct
- surfaced worker runtime acknowledgements and master orchestration actions in the session timeline
- separated session metadata/timeline supervision from rolling live output
- added unit coverage for timeline retention and trimming

## 0.2.0 - 2026-03-09

- added a master orchestration protocol based on `<codeclaw-actions>` JSON blocks
- added automatic worker spawn, worker follow-up prompts, and worker summary updates
- improved CLI runtime behavior so `send` and `spawn` wait for real turn completion instead of exiting early
- added explicit `thread/resume` handling so sessions can be recovered across process restarts
- added compatibility overrides for `codex app-server` reasoning effort
- filtered machine-readable action blocks out of the human-readable master stream
- persisted master summary and last message into `.codeclaw/state.json` and status files
- added pending-turn queueing for busy sessions
- added automatic worker completion/failure updates back into the master session
- improved TUI metadata so the sidebar and detail view show live summary, queue depth, and last message

## 0.1.0 - 2026-03-09

- initial Rust CLI and TUI shell
- local coordination state under `.codeclaw/`
- `codex app-server` client over JSON-RPC stdio
- master thread bootstrap and worker registration
