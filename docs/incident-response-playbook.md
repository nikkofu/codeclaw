# CodeClaw Incident Response Playbook

## Purpose

This playbook defines the standard response flow for operational incidents involving CodeClaw.

Use it when:

- live work stops unexpectedly
- automation behaves incorrectly
- runtime visibility is unclear
- logs indicate system instability
- an IM or gateway delivery flow is failing

This is an operational playbook, not a product roadmap document.

## 1. Incident Priorities

### P1

Use P1 when:

- operators cannot supervise or control live work
- active customer or stakeholder use is blocked
- core runtime cannot initialize
- state or evidence appears at risk

### P2

Use P2 when:

- a major workflow is degraded but partial control remains
- automation is not firing correctly
- inspection or reporting is incomplete but the system is still partially usable

### P3

Use P3 when:

- the issue is localized
- there is a workaround
- the system is usable but confusing or degraded

## 2. First Five Minutes

Do these first:

1. Determine whether this is a runtime problem, a scheduler problem, a session problem, or a gateway problem.
2. Preserve evidence before making risky changes.
3. Confirm whether `up` or `serve` is actively driving the scheduler.
4. Check whether the problem is affecting one session, one job, or the whole control plane.
5. Record incident start time, observed symptoms, and current operator.

## 3. Immediate Triage Commands

Run:

```bash
cargo run -- doctor
cargo run -- inspect --service
cargo run -- jobs
```

If one session is suspicious:

```bash
cargo run -- inspect --session <session-id> --events 12 --output 12
```

If one job is suspicious:

```bash
cargo run -- job inspect <job-id>
```

If automation is involved:

```bash
cargo run -- automation list
```

## 4. Evidence Preservation

Before restarting, clearing, or changing anything significant, preserve:

```text
.codeclaw/state.json
.codeclaw/runtime.json
.codeclaw/service.json
.codeclaw/status/
.codeclaw/tasks/
.codeclaw/logs/archive/YYYY-MM-DD/sessions/
.codeclaw/logs/archive/YYYY-MM-DD/runtime/
```

If the incident affects delivery or gateway behavior, also preserve:

```text
.codeclaw/gateway/mock-outbox.jsonl
```

## 5. Incident Classification

Classify the incident into one primary type:

- runtime startup failure
- scheduler not progressing
- session blocked or stuck
- automation misfire
- monitor data mismatch
- gateway delivery failure
- logging or evidence gap

Only classify once you have real evidence, not a guess.

## 6. Runtime Startup Failure

Symptoms:

- `doctor` fails
- app-server cannot initialize
- master session cannot resume

Check:

- `codex` installed and authenticated
- network availability
- `codeclaw.toml` validity
- `.codeclaw/` write access
- runtime logs under `.codeclaw/logs/archive/YYYY-MM-DD/runtime/`

Response:

- avoid broad resets
- preserve runtime logs first
- confirm whether the issue is auth, environment, or workspace permission related

## 7. Scheduler Not Progressing

Symptoms:

- jobs exist but do not move
- deferred work does not start
- automation remains armed but does not dispatch

Check:

- whether `up` or `serve` is actually running
- `inspect --service`
- whether target sessions are busy
- whether automation or loop budget is exhausted
- whether approval is still required

Response:

- restore one active scheduler driver
- do not create more jobs until scheduler state is understood
- pause noisy automations if they are confusing triage

## 8. Session Blocked or Stuck

Symptoms:

- one worker never leaves blocked
- session summary and live output disagree
- a batch appears stalled on one child

Check:

- session lifecycle note
- latest user prompt
- latest assistant response
- recent timeline
- recent live output
- task file in `.codeclaw/tasks/`

Response:

- send a clarifying prompt only after reading lifecycle note and recent output
- avoid blindly repeating the same prompt if the blocker reason is already clear

## 9. Automation Misfire

Symptoms:

- automation is armed but never runs
- automation runs against the wrong context
- automation repeatedly fails

Check:

- `cargo run -- automation list`
- target session still exists
- target session not currently busy
- remaining runs and remaining time
- last error

Response:

- pause the automation if it is producing noise
- confirm prompt content is still valid
- resume only if the target session and context still make sense
- cancel if the automation is no longer safe or useful

## 10. Monitor Data Mismatch

Symptoms:

- operator perception differs from monitor output
- model-generated answers conflict with local state

Check:

- onboard `Codex Sessions`
- `/monitor sessions`
- `/monitor runtime`
- `inspect --service`

Response:

- trust local CodeClaw monitor paths over freeform model explanations
- document the mismatch with captured outputs

## 11. Gateway Delivery Failure

Symptoms:

- reports are generated but not delivered
- subscription exists but no outbound event appears
- channel capability downgrade is unclear

Check:

- `cargo run -- gateway capabilities --channel mock-file`
- `cargo run -- job inspect <job-id>`
- `.codeclaw/gateway/mock-outbox.jsonl`
- gateway capability assumptions in [gateway-protocol.md](gateway-protocol.md)

Response:

- verify the issue first with `mock_file`
- separate protocol/design failure from channel-specific implementation failure

## 12. Communication Rules

During an incident:

- do not overstate certainty
- distinguish observed fact from inference
- use exact command outputs or file references where possible
- explicitly state whether work is stopped, degraded, or only partially affected

## 13. Stabilization Actions

Only after evidence is preserved and the failure mode is understood:

- pause problematic automation
- stop creating new jobs
- restore one known-good scheduler driver
- reduce operator actions to the minimum required for stability

Avoid:

- deleting `.codeclaw/`
- rewriting history
- re-running many conflicting experiments without recording outcomes

## 14. Recovery Validation

Before closing the incident, confirm:

- runtime can be inspected
- scheduler state is understandable
- affected jobs or sessions are either recovered or explicitly paused
- logs and evidence are preserved
- operators know whether normal work may resume

## 15. Post-Incident Record

Capture:

- incident start and end time
- priority
- scope
- root cause if known
- evidence paths
- remediation taken
- remaining risk
- follow-up items

Use:

- [../templates/incident-report-template.md](../templates/incident-report-template.md)

## 16. Companion Documents

- [operator-runbook.md](operator-runbook.md)
- [operations-guide.md](operations-guide.md)
- [faq.md](faq.md)
- [im-gateway-rollout-checklist.md](im-gateway-rollout-checklist.md)
