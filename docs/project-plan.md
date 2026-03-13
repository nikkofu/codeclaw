# CodeClaw Project Plan

## 1. Planning Assumption

This plan assumes the next development phase starts from release `0.12.0` and focuses on making CodeClaw an always-on orchestration product rather than only a local terminal prototype.

## 2. Project Objective

Deliver a stable next-phase baseline with:

- persistent `Job` management
- `serve` mode for long-running orchestration
- proactive reporting
- IM gateway readiness

## 3. Workstreams

### Workstream A: Domain Model and Persistence

Scope:

- define `Job`
- define job policy and orchestration-pattern metadata
- define job-to-batch linkage
- add job persistence and migration logic
- update inspection surfaces

Primary outputs:

- state schema update
- CLI/TUI job visibility
- tests for recovery and linkage

### Workstream B: Service Runtime

Scope:

- implement `codeclaw serve`
- scheduler loop
- watchdog loop
- background orchestration ownership

Primary outputs:

- persistent service mode
- restart-safe background execution
- operator attach model for CLI/TUI

### Workstream C: Reporting

Scope:

- progress event classification
- report policy and templates
- summary generation
- notification throttling

Primary outputs:

- proactive acceptance/progress/blocker/completion updates
- channel-ready report payloads

### Workstream D: Gateway Interface

Scope:

- define gateway abstraction
- define job control API
- implement the first IM adapter after the control API is stable

Primary outputs:

- remote job creation
- remote job inspection
- remote approval and resume
- one normalized gateway protocol that can be reused across IM platforms

### Workstream E: UX and Operations

Scope:

- TUI changes for job-aware supervision
- CLI changes for job commands
- service health and troubleshooting support
- documentation refresh

Primary outputs:

- operator-friendly service workflows
- improved observability
- updated usage and operations guidance

## 4. Suggested Phase Plan

### Phase 1: Design and State Foundation

Goals:

- finalize `Job` schema
- finalize service mode contract
- finalize report model
- finalize the initial orchestration pattern set and policy hooks

Definition of done:

- approved documentation baseline
- persistence model drafted
- implementation interfaces agreed

### Phase 2: Job Implementation

Goals:

- land `Job` persistence
- expose basic CLI job commands
- connect batches to jobs

Definition of done:

- jobs are durable and inspectable
- session activity can be traced to a job

### Phase 3: Service Mode

Goals:

- land `codeclaw serve`
- move orchestration loops out of TUI-only lifecycle

Definition of done:

- active work survives without the TUI
- service can restart and recover state

### Phase 4: Reporting

Goals:

- milestone reports
- blocker alerts
- completion summaries

Definition of done:

- users receive useful updates without polling

### Phase 5: IM Gateway

Goals:

- first gateway adapter
- remote command and report flow
- compatibility definition for media, typing, markdown, links, and raw event semantics

Definition of done:

- job creation and status retrieval work remotely
- completion and blocker reports reach the gateway
- adapter behavior is bounded by an explicit compatibility contract instead of per-platform special cases

## 5. Exit Criteria for the Next Development Cycle

The next cycle should be considered successful when:

- CodeClaw can run as a long-lived service
- a job can be created, tracked, resumed, and completed across restarts
- the system can proactively emit progress and completion updates
- a gateway integration can be added on top without changing core orchestration logic

## 6. Key Risks and Responses

### Risk: Trying to Build Everything at Once

Response:

- sequence delivery around `Job -> serve -> report -> gateway`

### Risk: Making IM the Primary State System

Response:

- IM must remain a channel adapter, not the system of record

### Risk: Overfitting to Provider Details

Response:

- keep provider configuration below CodeClaw runtime boundaries

### Risk: Poor Operator Trust

Response:

- prioritize transparency, lifecycle notes, progress reporting, and clean summaries early

## 7. Recommended Immediate Next Development Tasks

When implementation starts, begin with:

1. Add `Job` structs and state persistence
2. Add `job_id` linkage to batches and sessions where needed
3. Add job policy fields for the default supervisor-worker pattern with room for reviewer/approval extensions
4. Add `codeclaw jobs` / `codeclaw job create` / `codeclaw job inspect`
5. Add `serve` command skeleton with background scheduler loop
6. Add report policy primitives

This preserves the lowest-risk, highest-leverage path into the next phase.
