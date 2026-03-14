# IM Gateway Rollout Template

## Rollout Identity

- Target platform:
- Environment:
- Owner:
- Date:
- Related release:

## Scope

Supported in this rollout:

- 
- 
- 

Not supported in this rollout:

- 
- 
- 

## Capability Matrix

- text:
- markdown:
- links:
- image:
- audio:
- video:
- file:
- typing:
- raw type:
- raw event:
- raw hook:

## Authorization Model

- user identity source:
- role or allowlist model:
- permitted remote commands:
- prohibited remote commands:
- automation policy:
- auto-approve policy:

## Reporting Flow

Validated:

- `cargo run -- gateway schema`
- `cargo run -- gateway capabilities --channel mock-file`
- `cargo run -- gateway subscribe --job <job-id> --channel mock-file`
- mock delivery end-to-end verified

Notes:

- 

## Observability

- logs preserved:
- delivery failures visible:
- scheduler state inspectable:
- session state inspectable:
- audit fields preserved:

## Rate Limits and Backpressure

- inbound rate limit:
- outbound rate limit:
- retry policy:
- duplicate suppression:
- large payload handling:

## Rollout Stages

- dry run completed:
- internal pilot completed:
- limited production pilot completed:
- full rollout approved:

## Rollback Plan

- disable inbound commands:
- disable outbound notifications:
- preserve gateway evidence:
- notify operators:

## Final Approval

- rollout approved by:
- date:
- notes:
