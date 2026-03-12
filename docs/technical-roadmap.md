# CodeClaw Technical Roadmap

## 1. Roadmap Intent

This roadmap defines the technical sequence for evolving CodeClaw from a local supervision prototype into a long-running orchestration service.

The roadmap assumes:

- `codex app-server` remains the execution runtime
- provider routing stays under Codex
- CodeClaw focuses on orchestration, persistence, reporting, and gateway integration

## 2. Milestone Overview

### Milestone A: Job Foundation

Objective:

- introduce a persistent `Job` model above batches and sessions

Deliverables:

- `Job` state model and storage
- CLI commands for creating and listing jobs
- session-to-job and batch-to-job linkage
- job-aware status summaries in CLI/TUI inspection

Exit criteria:

- operators can create a job that survives restarts
- batches can be traced back to a job
- job-level status is queryable without reading raw session output

### Milestone B: Service Mode

Objective:

- make CodeClaw run independently of an interactive TUI

Deliverables:

- `codeclaw serve`
- service lifecycle management
- scheduler loop
- watchdog loop
- durable state reload on startup

Exit criteria:

- active jobs continue without TUI
- restart restores active work supervision state
- CLI can inspect the live service state

### Milestone C: Proactive Reporting

Objective:

- convert passive supervision into active communication

Deliverables:

- report policy model
- milestone report generation
- blocker report generation
- completion summary generation
- report throttling and cadence rules

Exit criteria:

- jobs emit operator-facing updates without manual polling
- blocked tasks trigger actionable notifications
- completed tasks produce structured summaries

### Milestone D: IM Gateway

Objective:

- enable remote command and reporting via messaging channels

Deliverables:

- gateway interface abstraction
- one concrete IM adapter
- job creation and status query over IM
- approval and resume controls over IM
- outbound report delivery

Exit criteria:

- operator can create and track jobs from IM
- CodeClaw can push blocker/completion messages back to IM

### Milestone E: Review and Escalation

Objective:

- improve autonomy safety and completion quality

Deliverables:

- review/critic pass
- escalation state tracking
- retry/backoff policies
- stalled-job detection
- high-risk action approval states

Exit criteria:

- tasks do not silently fail or stall
- risky states become visible and actionable
- job outcomes are more consistent and auditable

### Milestone F: Workspace Isolation and Integration

Objective:

- harden execution boundaries and mergeability

Deliverables:

- per-worker worktrees
- branch lifecycle control
- lease enforcement
- integration readiness checks

Exit criteria:

- concurrent workers operate on isolated workspaces
- merge risk is lower and easier to inspect

## 3. Cross-Cutting Technical Themes

### 3.1 State Evolution

Each milestone should preserve backward-compatible state loading where practical.

Priority additions:

- `jobs`
- `report_subscriptions`
- `gateway_events`
- richer scheduler metadata

### 3.2 Runtime Stability

The runtime integration layer must remain thin and explicit.

Target outcome:

- do not move business orchestration logic into the runtime adapter
- do not make provider-specific configuration infect job logic

### 3.3 UX Continuity

TUI, CLI, and IM should all surface the same core state model.

That means:

- one canonical status source
- one canonical job summary
- one canonical blocker note

### 3.4 Pattern Library

Agent design patterns should be modeled as channel-neutral orchestration policies.

Priority pattern support:

- supervisor -> worker specialization
- planner -> executor
- planner -> executor -> reviewer
- approval-gated execution for high-risk actions

## 4. Technical Risks

### 4.1 Overloading Session State

Risk:

- trying to represent job semantics only with sessions and batches

Mitigation:

- introduce `Job` explicitly instead of stretching session metadata further

### 4.2 Service/TUI Coupling

Risk:

- keeping orchestration bound to the TUI process

Mitigation:

- isolate service loops from terminal rendering early

### 4.3 Gateway Drift

Risk:

- IM-specific commands or report formats diverge from CLI/TUI semantics

Mitigation:

- define one channel-neutral job control API first

### 4.4 Premature Direct Provider Support

Risk:

- reimplementing execution semantics better handled by Codex runtime

Mitigation:

- keep runtime abstraction narrow and Codex-backed until a hard requirement emerges

## 5. Recommended Delivery Order

The most leverage-efficient order is:

1. Job foundation
2. Service mode
3. Proactive reporting
4. IM gateway
5. Review/escalation
6. Worktree isolation and merge controls

This order maximizes 24x7 operational value before deeper Git automation.
