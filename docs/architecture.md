# CodeClaw Architecture

## 0. Implementation Status

As of March 9, 2026, the repository implements a working control-plane prototype, not just a design stub.

Implemented now:

- one master session and multiple worker sessions inside a single Rust TUI
- persistent state and status files under `.codeclaw/`
- master orchestration actions for spawning workers, sending worker prompts, and updating summaries
- queued turns for busy sessions
- automatic worker completion/failure updates routed back into the master session
- a structured right-pane timeline for session supervision
- batch-scoped CLI waiting for the orchestration chain initiated by the active prompt
- persisted `session_history` and `batches` metadata inside `.codeclaw/state.json`
- a dedicated batch inspection mode in the TUI, built from persisted batch history
- color/animation-based task-state cues in the TUI for faster operator scanning
- right-pane focus filters and colorized live-output rendering for faster supervision triage
- session recovery using `thread/resume`

Not implemented yet:

- dedicated per-worker `git worktree` isolation
- hard lease enforcement on file paths
- merge queue / integration branch automation
- true PTY attachment inside the right-side pane

## 1. Problem Statement

Running one interactive `codex` process in one terminal does not scale well for:

- project-wide planning
- multi-team task decomposition
- parallel code generation
- coordinated integration
- concurrent file change control

The goal is a macOS-compatible terminal application that starts one master Codex and multiple worker Codex sessions, shows them in one window, and keeps Git state safe even when multiple agents operate in parallel.

## 2. Key Decision

Do **not** build this as a pile of shell scripts around multiple full-screen `codex` TUIs.

That approach breaks down in three places:

- nested interactive terminal rendering becomes fragile
- worker state is hard to inspect programmatically
- Git and task coordination become ad hoc and unsafe

The recommended design is a **control plane + execution plane**:

- the user-facing terminal application is `codeclaw`
- `codeclaw` owns the layout, session list, routing, locks, and worktree lifecycle
- Codex is started from the command line underneath `codeclaw`, but not exposed as raw unmanaged panes

## 3. Recommended Runtime Model

### 3.1 Master Session

Use `codex app-server` as the master control interface.

Why:

- it is an official machine-facing protocol
- it exposes structured thread and turn events
- local schema generation confirms support for collaboration and agent lifecycle events such as `spawnAgent`, `wait`, `resumeAgent`, and `closeAgent`
- it avoids trying to embed one full-screen Codex TUI inside another terminal UI

The user still launches everything from the terminal, but the visible UI is `codeclaw`, not the raw Codex TUI.

### 3.2 Worker Sessions

The current repository uses `codex app-server` threads for both master and workers.

Why the current implementation does this:

- it keeps the transport consistent for every session
- it makes queueing, recovery, and event handling much simpler
- it avoids mixing multiple runtime protocols too early

Planned follow-up:

- keep the current thread-based supervision path as the default
- add an attach mode later, backed by `codex resume` or a PTY-based worker view when a human needs direct intervention

## 4. Why This Fits the Requested UX

The requested UX is:

- left sidebar with fast switching
- right side with the selected runtime
- clear group name and task summary for each child
- one large terminal window that feels similar to Termius

A custom TUI is the right layer for this.

Recommended stack:

- Rust
- `ratatui` for layout and widgets
- `crossterm` for terminal event handling
- `tokio` for async orchestration
- `portable-pty` only for the views that must attach to an interactive Codex session
- `serde` and JSON-RPC client code for `codex app-server`

Rust is the best fit because it gives:

- a single native binary on macOS
- stable PTY handling when attach mode is needed
- good performance for many concurrent streams
- strong typing for the control plane

## 5. System Architecture

## 5.1 Components

1. `codeclaw tui`
   - renders the left session list and right active view
   - can switch the right pane between session supervision and orchestration-batch supervision
   - handles keyboard shortcuts, filtering, session focus, and attach/detach

2. `master adapter`
   - starts `codex app-server`
   - opens the master thread
   - sends user input into the master thread
   - receives structured turn, plan, and collaboration events

3. `worker runtime`
   - starts workers as additional `codex app-server` threads in the current prototype
   - captures turn, command, output, and error events
   - can evolve into interactive attach mode later when PTY support is added

4. `workspace manager`
   - creates and deletes `git worktree` directories
   - creates per-worker branches
   - tracks base branch and merge targets

5. `coordination store`
   - stores plan, task, lock, and status files under `.codeclaw/`
   - acts as the shared memory between master and workers
   - now also persists session timeline history and orchestration batch metadata

6. `merge controller`
   - checks overlap and mergeability
   - gates integration into the shared branch
   - opens PRs or local merge queues later

Current implementation note:

- items 1 through 5 exist in prototype form
- item 6 is still planned

## 5.2 High-Level Flow

```text
user
  -> codeclaw tui
      -> master adapter
          -> codex app-server
              -> master thread decides task split
                  -> worker runtime starts worker sessions
                      -> status, logs, and timeline updates flow through .codeclaw/ and in-memory session views
                          -> planned worktree isolation and merge control land in later phases
```

## 6. Session and Workspace Model

Each worker gets today:

- a stable worker id
- a group name
- a short task title
- a dedicated task file
- a dedicated status file
- a dedicated Codex thread
- a persisted timeline history in `.codeclaw/state.json`
- an in-memory rolling log in the active TUI process

Planned later:

- a dedicated worktree
- a dedicated branch
- an optional attachable interactive session

Suggested branch naming:

- `orch/master/<timestamp>` for orchestration control work
- `agent/<group>/<slug>` for worker branches

Suggested worktree layout:

```text
.codeclaw/
  config.toml
  state.json
  tasks/
    TASK-001.md
    TASK-002.md
  status/
    master.json
    backend-api.json
    frontend-web.json
  locks/
    paths.json
  decisions/
    ADR-0001-task-routing.md
  logs/
    master.log
    backend-api.jsonl
    frontend-web.jsonl
  worktrees/
    backend-api/
    frontend-web/
```

Important rule:

- master owns shared planning files
- each worker owns only its own status/log files in `.codeclaw/`
- once worktree isolation lands, code changes should happen inside the worker worktree, not the control directory

This keeps coordination files from turning into a conflict hotspot.

## 7. File Conflict and Version Control Strategy

This is the most important part of the system.

Do not allow unconstrained concurrent edits in the same checkout.

### 7.1 Default Isolation

Every worker runs in a separate `git worktree`.

That guarantees:

- separate index and working tree state
- independent branches
- no accidental file overwrites in a shared checkout

### 7.2 Path Lease Model

Before a task is assigned, the master records a soft path lease.

Example:

- backend worker leases `src/api/**`
- frontend worker leases `src/web/**`
- infra worker leases `deploy/**`

Rules:

- lease conflicts block automatic assignment
- lease overrides require explicit master approval
- leases are advisory during execution and hard-gated during merge

### 7.3 Merge Gate

Before integration, run three checks:

1. changed-path overlap check
2. `git merge-tree` or equivalent dry-run merge check
3. optional task-specific validation such as tests or linters

If all checks pass:

- merge or cherry-pick into the integration branch

If overlap exists but merge is clean:

- queue the merge and warn the master

If merge conflicts exist:

- mark the worker as `blocked_on_integration`
- route the conflict back to the master

### 7.4 Merge Queue

Do not merge directly into the main branch from workers.

Use:

- `main` or `trunk` as the stable branch
- `integration/current` as the orchestrated merge target
- worker branches merged into `integration/current`
- PR or final merge from integration into `main`

This gives the master Codex a stable place to integrate and validate combined work.

## 8. Document and GitHub Collaboration

Use docs for structured coordination, but keep ownership strict.

Recommended files:

- `.codeclaw/tasks/TASK-xxx.md`
- `.codeclaw/status/<worker>.json`
- `.codeclaw/decisions/ADR-xxxx.md`
- `.codeclaw/logs/<worker>.jsonl`

Task files should include:

- goal
- acceptance criteria
- leased paths
- dependencies
- merge target
- handoff notes

GitHub integration should come after the local orchestrator is stable.

Later additions:

- open PR per worker branch
- post task summary into PR description
- label PR with group and task id
- attach lock or dependency metadata

## 9. UI Layout

Target layout:

```text
+---------------------------+----------------------------------------------+
| Sessions                  | Selected Session                             |
|                           |                                              |
| master                    | live master thread / worker log / attach PTY |
| backend-api               |                                              |
| frontend-web              |                                              |
| ios-client                |                                              |
| qa-e2e                    |                                              |
|                           |                                              |
| filter: pay               |                                              |
+---------------------------+----------------------------------------------+
| mode | branch | cwd | status | tokens | locks | next action           |
+-----------------------------------------------------------------------+
```

Left panel width:

- fixed 26 to 32 columns

Right panel:

- default to the selected session stream
- can display master chat, worker JSON event log, or attached interactive PTY

Status line:

- current branch
- worktree path
- worker state
- lock summary
- unread event count

## 10. Titles and Labels

Each session row should expose:

- group
- short task title
- status
- branch

Suggested title format:

```text
[backend] Payment API refactor
[frontend] Checkout page wiring
[infra] CI cache stabilization
```

If a worker is attached to a real PTY, also emit terminal title sequences so terminal tabs and process hosts reflect the same title.

## 11. CLI Surface

Suggested commands:

```text
codeclaw init
codeclaw up
codeclaw spawn --group backend --task "Payment API refactor"
codeclaw spawn --group frontend --task "Checkout page wiring"
codeclaw focus backend
codeclaw attach backend
codeclaw stop backend
codeclaw merge backend
codeclaw sync
codeclaw doctor
```

Recommended behavior:

- `init`: create `.codeclaw/` config and local folders
- `up`: start the TUI and the master adapter
- `spawn`: create the task file and worker runtime today, then add worktree/branch setup in a later phase
- `focus`: move selection in the sidebar
- `attach`: upgrade the selected worker into interactive PTY view
- `merge`: run the merge gate
- `sync`: refresh Git and status state
- `doctor`: validate Codex, Git, and terminal prerequisites

## 12. Suggested MVP Phases

### Phase 1: Core Orchestration

Ship only:

- Rust TUI shell
- one master session via `codex app-server`
- worker spawning via additional `codex app-server` threads
- queued orchestration and runtime feedback
- local status files under `.codeclaw/`
- sidebar + selected-session view

Do not ship yet:

- GitHub PR automation
- embedded full worker terminal emulation
- complex conflict resolution UX

### Phase 2: Safe Integration

Add:

- path lease registry
- merge gate
- integration branch workflow
- conflict surfacing in the TUI

### Phase 3: Interactive Drill-Down

Add:

- attachable PTY mode for selected workers
- better log replay and session resume
- keyboard-driven steering of paused workers

### Phase 4: Team Features

Add:

- GitHub PR lifecycle
- dependency graph between tasks
- richer planning and review dashboards

## 13. Non-Goals for the First Version

Do not build these first:

- full tmux replacement
- distributed multi-host execution
- automatic semantic merge resolution
- shared writable workspace for all workers

These add complexity before the control model is proven.

## 14. Concrete Recommendation

Build the first version as:

- a Rust TUI application
- `codex app-server` for the master control plane
- `codex app-server` threads for autonomous worker execution in the first release
- `git worktree` for hard workspace isolation in the next phase
- `.codeclaw/` files for shared plan, status, and locks

Only add raw embedded interactive Codex sessions after the control plane is stable.

That gives you:

- the terminal UX you asked for
- much better performance than one manual Codex per terminal tab
- safe parallelism with clear merge control
- a realistic path to GitHub and docs-based collaboration
