# CodeClaw IM Gateway Rollout Checklist

## Purpose

This checklist is for preparing a real IM gateway rollout on top of CodeClaw's current gateway and reporting foundations.

Target platforms may include:

- Slack
- Telegram
- WeCom
- Feishu
- Discord
- custom webhook or bot bridges

This document is intentionally operational. It focuses on what should be true before a production rollout, not on aspirational architecture.

## 1. Product Scope Check

Before implementation, confirm the rollout scope is explicit.

- identify the first target platform
- identify the first supported user actions
- identify whether the gateway is command-only, report-only, or both
- identify whether media, markdown, typing, and raw event passthrough are required
- identify whether the rollout is internal-only or customer-facing

Recommended first rollout scope:

- text and markdown
- links
- job/report notifications
- limited command entry
- typing-state support where available

## 2. Capability Mapping Check

Review [gateway-protocol.md](gateway-protocol.md) and confirm the target platform can be mapped into:

- text
- markdown
- links
- image
- audio
- video
- file
- typing
- raw `type`
- raw `event`
- raw `hook`

For each capability, mark one of:

- native
- downgraded
- unsupported

Do not start implementation before this matrix is written down.

## 3. Safety and Control Boundaries

Confirm the gateway will not become an uncontrolled freeform shell.

- decide which slash commands are allowed remotely
- decide which job actions are allowed remotely
- decide whether direct session prompts are allowed remotely
- decide what approval boundary applies to automation actions
- decide whether auto-approve can be enabled from the IM channel
- decide which roles can create or cancel automation

Minimum recommendation:

- allow monitor commands
- allow job creation with bounded defaults
- allow report subscription control
- require stronger approval for automation creation, cancellation, and any privileged session control

## 4. Identity and Authorization

Define how CodeClaw maps remote users to operator identity.

- platform user id mapping
- tenant or workspace mapping
- allowlist or role mapping
- audit identity recorded in events and reports
- revocation process

Do not rely on display names alone.

## 5. Event Model Validation

Confirm the gateway bridge preserves raw platform semantics.

- inbound raw type preserved
- inbound raw event preserved
- inbound raw hook preserved
- outbound downgrade behavior documented
- unsupported platform constructs mapped predictably

This is required for debugging, audits, and future adapter portability.

## 6. UX Contract

Define the remote interaction style before rollout.

- supported markdown subset
- link rendering rules
- image/file fallback behavior
- typing indicator behavior
- error message style
- partial progress update behavior
- final completion summary format

Recommended default:

- concise, structured progress updates
- explicit status markers
- direct links to job id, session id, and release documentation where relevant

## 7. Reporting and Subscription Flow

Validate the reporting pipeline before exposing the gateway to real users.

- `cargo run -- gateway schema`
- `cargo run -- gateway capabilities --channel mock-file`
- `cargo run -- gateway subscribe --job <job-id> --channel mock-file`
- create a real job and verify delivery into `.codeclaw/gateway/mock-outbox.jsonl`
- verify delivery history in `cargo run -- job inspect <job-id>`

Do not roll out a real IM adapter before `mock_file` delivery is understood end-to-end.

## 8. Operational Visibility

Make sure operators can see enough when the gateway misbehaves.

- runtime logs archived by day
- gateway delivery failures visible
- subscription state inspectable
- report delivery history inspectable
- scheduler state inspectable
- session/runtime state inspectable

Minimum required checks:

```bash
cargo run -- inspect --service
cargo run -- job inspect <job-id>
cargo run -- gateway capabilities --channel mock-file
```

## 9. Rate Limits and Backpressure

Confirm behavior under platform constraints.

- inbound request rate limiting
- outbound notification rate limiting
- retry policy
- duplicate suppression policy
- burst handling policy
- message-size fallback behavior

This matters especially for typing events, large markdown payloads, and repeated progress updates.

## 10. Automation Policy

If IM users can trigger automation, define strict rules first.

- max default run count
- max default duration
- which sessions may be targeted
- whether `master` is allowed remotely
- whether existing workers may be targeted remotely
- who can pause, resume, or cancel automation

Recommended first rollout:

- bounded automation only
- no open-ended defaults
- operator-visible audit trail required

## 11. Rollout Stages

Recommended sequence:

1. schema and capability validation
2. `mock_file` dry run
3. internal operator-only bridge
4. limited real-channel pilot
5. monitored production rollout

Do not skip directly from protocol design to broad user rollout.

## 12. Acceptance Checklist

Before rollout sign-off, confirm all of the following:

- capability matrix completed
- security and authorization rules documented
- remote command surface documented
- reporting flow tested with `mock_file`
- failure and retry paths tested
- audit fields preserved
- operator runbook written
- rollback plan written
- on-call owner assigned

## 13. Rollback Checklist

Prepare rollback before launch.

- disable inbound commands
- disable outbound notifications if required
- preserve gateway logs and subscription state
- retain failed payload samples where permitted
- notify operators of degraded or disabled channel state

## 14. Required Companion Docs

Use these together:

- [gateway-protocol.md](gateway-protocol.md)
- [operations-guide.md](operations-guide.md)
- [project-delivery.md](project-delivery.md)
- [faq.md](faq.md)
- [quickstart-card-v0.13.1.md](quickstart-card-v0.13.1.md)

## 15. Final Recommendation

Treat the first IM gateway rollout as an operator tooling deployment, not a mass end-user chat product launch.

That means:

- keep scope narrow
- preserve auditability
- keep automations bounded
- prioritize local monitor truth over conversational guesswork
- validate downgrade behavior across markdown, media, typing, and raw event fields before promising broad compatibility
