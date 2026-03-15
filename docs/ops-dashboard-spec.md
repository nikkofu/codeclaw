# CodeClaw Ops Dashboard Spec

## Purpose

This document specifies the intended product shape for CodeClaw as a large-scale Codex monitor and control surface.

The goal is not only to show sessions, but to let one operator understand, supervise, direct, and bound a multi-session Codex system from one place.

This spec builds on the current onboard/control-plane baseline in `v0.13.1`.

## 1. Problem Statement

The current TUI already supports:

- onboard visibility
- local monitor answers
- jobs
- bounded automation
- session inspection

But the target product is larger than a session list plus output panes. The intended system should behave more like a true operations dashboard for parallel Codex work:

- real session inventory
- job-to-session mapping
- direct command dispatch
- repeated and scheduled execution
- automation safety signals
- IM-ready event semantics
- operator trust based on local truth, not model narrative

## 2. Product Position

CodeClaw should be understood as:

- a Codex execution control plane
- a multi-session supervision console
- an automation boundary layer
- a future IM-connected operator surface

It should not degrade into:

- a thin wrapper around one Codex terminal
- a fake monitor that asks the model to describe its own runtime
- an unbounded autonomous loop runner without operator guardrails

## 3. Core Principles

### 3.1 Local Truth First

Runtime, scheduler, session, job, and automation answers should come from CodeClaw state whenever possible.

### 3.2 One-Screen Supervision

The default operator experience should make it possible to answer these questions quickly:

- how many Codex sessions exist right now
- which ones are active
- what each one is doing
- which jobs are blocked
- which automations are armed
- whether a scheduler driver is actually running

### 3.3 Bounded Autonomy

Automation must remain observable and bounded:

- time limit
- iteration limit
- explicit pause/resume/cancel
- visible approval state

### 3.4 Gateway-Ready Interaction Model

The dashboard must map cleanly into future IM surfaces that support:

- text
- markdown
- links
- image/audio/video/file
- typing indicators
- raw `type`, `event`, and `hook`

## 4. Primary User Roles

### Operator

Supervises live work, triages failures, controls automation, and routes commands.

### Project Lead

Uses the board to understand status, blockers, delivery progress, and current session distribution.

### IM Gateway User

Interacts through remote command/report surfaces that must still map back to the same control-plane truth.

## 5. Core Objects

The dashboard should make these objects first-class:

- runtime
- scheduler
- session
- job
- batch
- automation
- gateway channel
- incident

Each object needs:

- stable id
- current state
- last meaningful update
- primary operator action

## 6. Default Dashboard Layout

The intended default surface is a multi-panel operations view.

### Top Status Bar

Must show:

- runtime connection state
- app-server pid
- scheduler state
- active turns
- queued turns
- armed automations
- blocked jobs
- failed jobs

### Session Inventory Panel

Must show:

- all sessions
- role
- work state
- queue depth
- latest user prompt
- latest assistant preview
- linked job if present

This should behave like a truthful Codex fleet table, not a decorative list.

### Job Board

Must show:

- pending
- running
- blocked
- completed
- failed

Each card should expose:

- operator-facing state reason
- linked sessions
- automation markers
- approval markers

### Automation Panel

Must show:

- target session
- interval
- remaining run budget
- remaining time budget
- last dispatch
- last error
- current status

### Incident / Warning Panel

Must surface:

- lag warnings
- repeated automation failure
- disconnected runtime
- exhausted budget
- gateway delivery failures

## 7. Required Interactions

The dashboard should support, from one surface:

- inspect one session
- inspect one job
- inspect runtime state
- inspect scheduler state
- create a job
- dispatch a prompt to `master`
- dispatch a prompt to a worker
- create a new worker
- create automation
- pause/resume/cancel automation
- focus one session
- jump back to onboard

Slash commands remain a strong interaction model for this.

## 8. Required Queries

An operator should be able to answer these directly from local state:

- how many Codex sessions are running
- which sessions belong to which jobs
- what each active session last received from the user
- what each active session last produced
- which jobs are blocked and why
- which automations are currently due
- whether `up` or `serve` is driving the scheduler
- whether the runtime is actually healthy

## 9. IM Parity Requirements

When the dashboard is extended through IM, the following must remain true:

- remote actions map to the same job/session/automation objects
- markdown/link degradation is explicit
- typing state is modeled, not guessed
- raw `type`, `event`, and `hook` are preserved
- remote monitor answers still come from CodeClaw state

## 10. Operator Trust Model

The dashboard must avoid these failure modes:

- model-generated fake monitor answers
- stale scheduler state presented as active execution
- hidden auto-approve state
- invisible automation budget exhaustion
- session counts that depend on inference instead of state

Trust is built through:

- local snapshots
- inspectability
- explicit ids
- archived logs
- consistent state transitions

## 11. Suggested Data Additions

Future expansion should consider:

- explicit incident objects
- job-to-session graph snapshots
- gateway channel activity snapshots
- operator notes
- scheduled task history
- automation provenance

## 12. Phase Plan

### Phase 1

Strengthen the current onboard surface:

- denser session table
- better job-to-session linking
- better automation visibility
- warning and incident strip

### Phase 2

Add deeper control-plane operations:

- direct session command routing from onboard
- batch-centric command and rerun controls
- richer incident state and acknowledgement flow

### Phase 3

Add gateway-aware remote supervision:

- IM-safe command subset
- remote monitor cards
- channel-specific downgrade handling
- delivery audit views

## 13. Acceptance Criteria

This dashboard direction should be considered successful when an operator can:

1. identify all real sessions without asking Codex
2. understand current work distribution from one screen
3. see which jobs and automations are risky
4. dispatch bounded follow-up actions without leaving the control surface
5. explain incidents from local evidence instead of conversational guesswork

## 14. Companion Documents

- [architecture.md](architecture.md)
- [system-architecture-v2.md](system-architecture-v2.md)
- [operator-runbook.md](operator-runbook.md)
- [im-gateway-rollout-checklist.md](im-gateway-rollout-checklist.md)
- [gateway-protocol.md](gateway-protocol.md)
