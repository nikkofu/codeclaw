# Changelog

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
