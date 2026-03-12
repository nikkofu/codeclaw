# CodeClaw Product Strategy

## 1. Product Definition

CodeClaw is not intended to become another model CLI.

Its product role is:

- an orchestration layer above `codex app-server`
- an operations layer for long-running agentic work
- a transparency layer for task state, progress, blockers, and outcomes
- a channel layer that can serve TUI, CLI, and future IM gateways

In practical terms:

- `codex` is the execution runtime
- provider routing such as `rightcode` remains under the Codex runtime configuration
- `CodeClaw` is responsible for planning, supervision, persistence, reporting, escalation, and operator experience

## 2. Product Goal

The target product is a 24x7 agentic operations system that can:

- accept work from human operators or external gateways
- decompose goals into structured tasks
- coordinate master and worker sessions over time
- recover state across restarts and interruptions
- proactively report progress and outcomes
- escalate blockers to humans when needed
- keep work transparent, auditable, and easy to steer

The end state is not "a better terminal wrapper for Codex."  
The end state is "an always-on work orchestration system powered by Codex."

## 3. Problem Statement

Directly running `codex` in a terminal is strong for single-session execution, but weak for:

- multiple concurrent tasks
- persistent tracking across restarts
- structured visibility into progress and blockers
- proactive reporting instead of passive waiting
- remote interaction over IM or service gateways
- 24x7 unattended operation

CodeClaw exists to fill those gaps without reimplementing the model runtime.

## 4. Product Principles

### 4.1 Do Not Rebuild the Runtime

CodeClaw should continue to use `codex app-server` as the execution plane.  
It should not directly replace Codex with a custom provider client unless the runtime contract becomes a hard blocker.

### 4.2 Keep Provider Routing Below CodeClaw

Provider selection such as official OpenAI endpoints or a `rightcode` Responses-compatible backend should stay below CodeClaw, inside Codex runtime configuration whenever possible.

This keeps CodeClaw focused on orchestration instead of transport churn.

### 4.3 Treat User Requests as Jobs, Not Prompts

The main product object must evolve from "prompt" to "job."

A job persists beyond one turn and can contain:

- origin channel
- objective
- context
- policy
- current stage
- related batches and workers
- reporting state
- final outcome

### 4.4 Be Proactive, Not Passive

CodeClaw should not wait for humans to ask "what happened?"

It should proactively emit:

- task accepted
- major milestone reached
- blocker found
- approval requested
- task completed
- summary delivered

### 4.5 Be Clear, Not Magical

The product should feel agentic without becoming opaque.

Operators must always be able to answer:

- what is running
- why it is running
- what changed
- what is blocked
- what happens next
- whether human approval is needed

### 4.6 Be Helpful Without Pretending

The reporting tone can be warm, concise, and supportive, but it should not rely on fake emotional claims.

The desired "emotional value" comes from:

- proactive progress updates
- low-anxiety status phrasing
- clear next steps
- polished completion summaries
- visible ownership of follow-up

## 5. Product Positioning

### 5.1 What CodeClaw Is

- a terminal-first orchestration and supervision product
- a multi-agent work coordinator
- a job-state and reporting system for Codex-powered execution
- a bridge between local development workflows and always-on agent operations

### 5.2 What CodeClaw Is Not

- not a direct replacement for `codex`
- not a generic chat UI
- not a model router for arbitrary LLM providers
- not a pure task manager disconnected from execution

## 6. Target Users

### 6.1 Solo Technical Operator

Needs:

- one place to supervise multiple AI workstreams
- restart-safe progress tracking
- less manual babysitting

### 6.2 Engineering Lead / AI Operator

Needs:

- transparent work decomposition
- auditable progress and blocker trails
- remote check-ins over IM or scripts

### 6.3 Always-On Automation Owner

Needs:

- unattended execution
- queueing and retry
- proactive reports
- escalation on failure or blocked states

## 7. Core User Experience

The future user experience should feel like:

- direct enough for terminal-native operators
- structured enough for operational clarity
- asynchronous enough for long-running work
- polite and proactive enough for daily collaboration

Primary interaction modes:

- TUI for rich local supervision
- CLI for scripts, inspection, and control
- IM gateway for remote command and reporting

## 8. Product Capability Map

### 8.1 Foundation Capabilities

- session supervision
- worker lifecycle tracking
- batch inspection
- persisted output and timeline state
- lifecycle notes

### 8.2 Next-Stage Capabilities

- `Job` abstraction above batches
- `codeclaw serve` long-running daemon mode
- proactive reporting policy
- IM gateway integration
- approval and escalation flows
- watchdogs and auto-recovery

### 8.3 Later-Stage Capabilities

- worktree isolation
- lease enforcement
- review/critic stage
- merge and integration gates
- richer automation policies

## 9. Agent Design Pattern Strategy

CodeClaw should support common multi-agent execution patterns as explicit orchestration policy rather than burying them inside one giant system prompt.

The initial pattern set should include:

- supervisor -> specialist workers
- planner -> executor
- planner -> executor -> reviewer
- watchdog -> retry -> escalate
- approval-gated execution for risky actions

Design rule:

- the selected pattern belongs to the job policy layer
- channels such as TUI, CLI, and IM should trigger the same pattern semantics
- runtime/provider wiring should stay below these orchestration choices

## 10. Differentiation vs Running Codex Directly

CodeClaw should be measurably better than launching `codex` directly in a terminal because it adds:

- multi-session orchestration
- persisted job and supervision context
- state transparency
- channel-agnostic interaction
- structured progress reporting
- operational recovery
- escalation and approval mechanics

The value is in coordination and operations, not in replacing model execution.

## 11. Success Metrics

Product success should be measured by:

- reduced operator idle waiting during long tasks
- lower need to manually restate context after restart
- faster detection of blockers
- shorter time to understand current task status
- percentage of tasks completed without manual babysitting
- quality and timeliness of proactive reports

## 12. Product Boundaries

The following remain explicitly out of scope for the next phase unless they become critical blockers:

- direct provider execution bypassing `codex app-server`
- rebuilding a custom model transport for each provider
- building a generic enterprise chat platform
- over-abstracting into a massive workflow engine before job-state basics are stable

## 13. Strategic Recommendation

The strategic direction for the next phase is:

1. keep `codex app-server` as the execution runtime
2. make provider choice visible, but not central, inside CodeClaw
3. introduce `Job` as the top-level operating object
4. add `serve` mode for 24x7 execution
5. add proactive reporting and escalation
6. add IM gateway support after the internal job/reporting model is stable

That path delivers the highest product leverage with the lowest architectural churn.
