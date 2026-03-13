# CodeClaw System Architecture vNext

## 1. Architecture Intent

The next architecture phase evolves CodeClaw from a local supervision prototype into an always-on agentic operations system.

This phase does not replace the current runtime contract.

The architecture keeps:

- `codex app-server` as the execution plane
- provider routing below CodeClaw
- current master/worker orchestration as the foundation

The architecture adds:

- a persistent `Job` model
- long-running service mode
- gateway adapters for remote control
- a normalized gateway compatibility contract
- proactive reporting
- watchdog and escalation loops

## 2. Layered Architecture

```text
Channels
  -> CLI / TUI / IM Gateway / Webhook Gateway
     -> Job API and Control Plane
        -> Orchestrator and Scheduler
           -> Runtime Adapter (Codex App Server)
              -> Codex runtime configuration
                 -> Provider backend (OpenAI / rightcode / other Responses-compatible endpoint)
```

## 3. Core Architectural Decision

CodeClaw should not directly talk to model providers in the next phase.

Instead:

- CodeClaw talks to `codex app-server`
- Codex runtime talks to the configured provider backend
- CodeClaw remains provider-aware only for visibility, policy, and reporting

This preserves the strongest existing runtime abstraction while allowing provider flexibility underneath.

## 4. Top-Level Components

### 4.1 Channel Adapters

Interfaces through which humans or systems interact with CodeClaw.

Planned adapters:

- TUI adapter
- CLI adapter
- IM gateway adapter
- optional webhook/task ingestion adapter

Responsibilities:

- accept commands and job requests
- normalize channel input into a shared job schema
- route responses and reports back to the origin channel

Compatibility rule:

- adapters must preserve platform-specific `type`, `event`, and `hook` semantics while mapping them into one normalized contract

### 4.2 Job Control Plane

The central business layer of the next phase.

Responsibilities:

- create, update, suspend, resume, and complete jobs
- persist job state
- track ownership, priorities, and policies
- map jobs to one or more orchestration batches

### 4.3 Orchestrator

The orchestrator sits above current batch logic.

Responsibilities:

- derive execution plans from jobs
- delegate tasks to master/worker sessions
- decide when to create follow-up batches
- aggregate outcomes into job-level status

The orchestrator should encode reusable agent-design patterns as policy-driven execution graphs rather than one-off prompt choreography.

Initial patterns to support:

- supervisor -> specialist workers
- planner -> executor
- planner -> executor -> reviewer
- watchdog -> retry -> escalate
- approval gate before risky execution

Pattern selection should be attached to the job or policy layer so TUI, CLI, and IM all drive the same orchestration behavior.

### 4.4 Scheduler and Watchdog

The scheduler is what enables 24x7 operation.

Responsibilities:

- poll active jobs
- trigger follow-up work
- detect stalled or blocked jobs
- enforce retry/backoff policies
- trigger proactive reports

### 4.5 Runtime Adapter

In the next phase this remains Codex-specific.

Responsibilities:

- session creation
- thread resume
- turn execution
- event streaming
- execution error translation

### 4.6 Reporting Engine

This is a first-class subsystem, not an afterthought.

Responsibilities:

- milestone report generation
- completion summaries
- blocker notices
- tone and formatting policy by channel
- report throttling and scheduling

## 5. Persistent Data Model

## 5.1 Job

The future top-level object.

Suggested shape:

```text
Job
- id
- source_channel
- requester
- title
- objective
- context
- status
- priority
- policy
- created_at
- updated_at
- active_batch_ids
- assigned_worker_ids
- latest_summary
- latest_report_at
- next_report_due_at
- escalation_state
- final_outcome
```

## 5.2 Batch

Retain batch as the unit of one orchestration chain.

Role in vNext:

- one job may create multiple batches
- batches remain useful for timeline replay and debugging
- batches stay subordinate to jobs

## 5.3 Session

Sessions remain runtime-facing execution contexts.

Role in vNext:

- master session handles planning and coordination
- worker sessions execute scoped tasks
- session state continues to feed TUI and CLI inspection

## 5.4 Report Subscription

Needed for IM and multi-channel support.

Suggested shape:

```text
ReportSubscription
- channel_type
- channel_target
- job_id
- notify_on_accept
- notify_on_progress
- notify_on_blocker
- notify_on_completion
- notify_on_failure
```

## 6. Runtime and Provider Model

## 6.1 Runtime Contract

The execution runtime remains:

- `codex app-server`

The provider layer remains below that runtime, configured through Codex.

This allows setups such as:

- official OpenAI-backed Codex runtime
- Responses-compatible backend such as `rightcode`, when configured in Codex

## 6.2 Why Not Direct Provider Mode Yet

Direct provider mode would require CodeClaw to own:

- session semantics
- streaming transport
- tool/runtime event models
- error normalization
- provider-specific lifecycle handling

That adds large complexity for limited immediate product gain.

## 7. Service Mode

## 7.1 `codeclaw serve`

The next major runtime addition should be a daemon/service mode.

Responsibilities:

- keep orchestration active without a TUI
- own job queue processing
- expose status to CLI and gateway adapters
- keep watch loops and report loops alive

## 7.2 TUI in vNext

The TUI becomes one client of the service layer, not the only operating mode.

Desired evolution:

- TUI can attach to an active local service
- TUI remains the best local supervision interface
- service mode remains active after the TUI exits

## 8. IM Gateway Model

The IM gateway should not become a free-form chat passthrough.

It should be a structured remote control and reporting channel.

Supported intents should include:

- create job
- query job status
- request progress report
- approve / reject pending action
- pause / resume job
- list active jobs

Suggested command style:

```text
/new <objective>
/status <job-id>
/jobs
/approve <job-id>
/pause <job-id>
/resume <job-id>
/report <job-id>
```

## 8.1 Compatibility Contract

The gateway layer should standardize a compatibility-first message contract across IM systems.

Required compatibility primitives:

- text
- markdown
- links
- image
- audio
- video
- file
- typing indicators
- raw `type`
- raw `event`
- raw `hook`

Design rules:

- one canonical inbound event shape
- one canonical outbound envelope shape
- explicit capability declaration per adapter
- graceful downgrade when a platform lacks media or markdown support
- mandatory fallback text for every outbound message

Reference document:

- [Gateway Protocol](gateway-protocol.md)

## 9. Proactive Reporting

Reporting should be event-driven and schedule-driven.

Trigger classes:

- job accepted
- batch started
- worker blocked
- approval required
- major milestone reached
- job completed
- job failed
- periodic digest while long tasks remain active

Each report should include:

- current state
- what changed
- what is blocked or risky
- what happens next
- whether operator action is required

## 10. Human-in-the-Loop

Agentic does not mean unchecked autonomy.

Human approval should remain explicit for:

- high-risk commands
- ambiguous destructive actions
- lease conflicts
- long-running blocked states that need policy decisions

The architecture should preserve clear approval points without stalling low-risk autonomous work.

## 11. Operational Concerns

## 11.1 Recovery

CodeClaw must recover after:

- process restart
- TUI exit
- machine reboot
- temporary provider/runtime failures

## 11.2 Observability

Required visibility:

- per-job status
- per-batch timeline
- per-session output and lifecycle notes
- gateway event logs
- report history

## 11.3 Security

Security boundaries should include:

- clear separation between runtime configuration and orchestration policy
- explicit environment handling for provider credentials
- auditable approval and escalation events

## 12. Architecture Sequence

Recommended implementation order:

1. Introduce `Job` and persistent job store
2. Introduce `serve` mode and background scheduler
3. Add proactive reporting engine
4. Add IM gateway adapter
5. Add review/escalation enhancements
6. Add worktree and integration controls afterward

This sequence keeps the next phase focused on operational leverage before deeper Git automation.
