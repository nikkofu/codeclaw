# Changelog

## Unreleased

- no entries yet

## 0.13.1 - 2026-03-14

- finalized the `0.13.x` delivery package so the tagged release now includes the release announcement and operator upgrade notes
- synchronized repository version metadata, README delivery links, and release-maintainer materials to `0.13.1`
- no runtime behavior change from `0.13.0`; this patch release freezes the complete release package into a clean tagged snapshot

## 0.13.0 - 2026-03-14

- added a local codex-monitor snapshot plus onboard `Codex Sessions` visibility so runtime/session answers come from CodeClaw state instead of a model guess
- added bounded session automations with `automation create|list|pause|resume|cancel` targeting `master` or an existing worker session
- made `codeclaw up` an active foreground scheduler driver and extended `codeclaw serve` so delegated jobs and session automations continue under cooldown, time-budget, and iteration-budget protections
- expanded onboard with dedicated `Codex Sessions` and `Automations` panels plus clearer runtime-versus-scheduler visibility
- added local slash-command control for `/monitor ...` and `/automation ...`, with richer command-bar completion, cursor movement, multiline editing, and history recall
- persisted live runtime heartbeat into `.codeclaw/runtime.json` and extended `inspect --service` with app-server pid, mode, active turns, and queued turns
- refreshed release notes, usage documentation, operations guidance, project delivery notes, acceptance criteria, and architecture references for the `0.13.0` release

## 0.12.0 - 2026-03-13

- added a default virtual `onboard` supervision session with a kanban-like board for pending, running, blocked, completed, and failed jobs
- added bounded master-loop delegation controls for jobs, including `delegate_to_master_loop`, `continue_for_secs`, `continue_max_iterations`, and explicit `auto_approve` visibility
- extended `codeclaw serve` so delegated jobs can continue through the master session under cooldown, time-budget, and iteration-budget protections
- changed app-server notification lag from a fatal turn error into a logged warning so transient broadcast backlog no longer fails the session by default
- increased app-server notification buffer capacity and made it configurable through `[logging].notification_channel_capacity`
- added daily archived JSONL logging under `.codeclaw/logs/archive/YYYY-MM-DD/` with configurable retention, defaulting to 30 days
- added runtime log coverage for app-server stderr, parse failures, stdout-closed conditions, and controller-side lag warnings
- refreshed release notes, usage documentation, operations guidance, and acceptance criteria for the `0.12.0` release

## 0.11.0 - 2026-03-13

- added a normalized gateway compatibility layer for text, markdown, links, image, audio, video, file, typing, and raw `type/event/hook` semantics
- integrated `src/gateway.rs` into the report delivery path so queued notifications now flow through a single gateway-backed dispatch function
- extended report channels with `mock_file` for safe IM adapter prototyping and delivery replay via `.codeclaw/gateway/mock-outbox.jsonl`
- added `codeclaw gateway schema`, `codeclaw gateway capabilities`, and `codeclaw gateway subscribe` for protocol visibility and operator control
- added a dedicated `docs/gateway-protocol.md` delivery document covering compatibility rules and adapter mapping guidance
- improved CLI spawn progress rendering so non-TTY environments still receive visible status-line updates instead of a silent wait
- synchronized release metadata, README references, and delivery documents to repository version `0.11.0`

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
