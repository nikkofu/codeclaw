# CodeClaw Acceptance Use Cases

## Scope

This document defines recommended acceptance scenarios for release `0.12.0`.

Repository: `https://github.com/nikkofu/codeclaw`

## Acceptance Preconditions

- repository checkout is available locally
- Rust toolchain is installed
- `codex` CLI is installed and authenticated
- target machine has network access for Codex model turns
- operator can read and write `.codeclaw/`

## UC-01 Workspace Initialization

**Objective**  
Initialize CodeClaw in a clean repository workspace.

**Steps**

1. Run `cargo run -- init`.
2. Confirm `codeclaw.toml` exists when no local config was present.
3. Confirm `.codeclaw/` layout is created after first runtime use.

**Expected results**

- initialization completes without error
- configuration file is available for review
- coordination directories can be created successfully

## UC-02 Environment Health Check

**Objective**  
Verify the local runtime can talk to `codex app-server`.

**Steps**

1. Run `cargo run -- doctor`.
2. Review the reported config source, coordination root, app-server check, and thread/start probe.

**Expected results**

- configuration path is correct
- app-server health reports `ok`
- thread/start probe reports `ok`

## UC-03 Master Session Startup

**Objective**  
Start the TUI and confirm the master session is available.

**Steps**

1. Run `cargo run -- up`.
2. Confirm the session list contains `master`.
3. Confirm the right pane shows session metadata, timeline, and live output.

**Expected results**

- TUI opens successfully
- master session is selectable
- UI reacts to navigation keys
- the default `onboard` session is visible and can be selected

## UC-04 Master Dispatch Creates Worker

**Objective**  
Confirm the master can create a worker and supervise its lifecycle.

**Steps**

1. In the TUI, press `i`.
2. Ask the master to split work into at least one worker.
3. Observe the new worker in the session list.

**Expected results**

- worker is created with a stable worker id
- worker transitions through `spawn_requested` and `bootstrapping`
- task file is created under `.codeclaw/tasks/`

## UC-05 Worker Blocker Propagation

**Objective**  
Confirm blocker context is preserved and visible.

**Steps**

1. Trigger a worker scenario that results in a blocker.
2. Inspect the worker in the TUI or via `cargo run -- inspect --session <worker-id>`.

**Expected results**

- worker state becomes `blocked`
- `lifecycle note` contains a concise blocker explanation
- master timeline records the worker runtime update

## UC-06 CLI Inspection

**Objective**  
Validate non-TUI supervision.

**Steps**

1. Run `cargo run -- inspect --session master --events 8 --output 6`.
2. Run `cargo run -- inspect --batch <batch-id> --events 12` using a real batch id.

**Expected results**

- session inspection prints status, summary, lifecycle note, timeline, and output
- batch inspection prints status, root prompt, sessions, and aggregated events

## UC-07 Restart Recovery

**Objective**  
Confirm supervision state survives process restart.

**Steps**

1. Use CodeClaw to produce session activity.
2. Stop the process.
3. Restart with `cargo run -- up` or inspect with `cargo run -- inspect --session master`.

**Expected results**

- recent timeline events are restored
- recent output tail is restored
- in-flight assistant text is restored if the previous process persisted it
- lifecycle notes remain visible after restart

## UC-08 Manual Worker Spawn

**Objective**  
Confirm CLI worker creation works without the TUI.

**Steps**

1. Run `cargo run -- spawn --group backend --task "Add validation"`.
2. Run `cargo run -- list`.
3. Inspect the new worker session.

**Expected results**

- worker creation completes successfully
- worker appears in `list`
- worker task file and status file are created

## UC-09 Gateway Compatibility and Delivery

**Objective**  
Confirm the gateway contract and delivery path are usable for formal integration work.

**Steps**

1. Run `cargo run -- gateway schema`.
2. Run `cargo run -- gateway capabilities --channel mock-file`.
3. Create or reuse a job.
4. Run `cargo run -- gateway subscribe --job <job-id> --channel mock-file`.
5. Trigger at least one report for that job.

**Expected results**

- the CLI prints normalized inbound and outbound JSON examples
- capability output explicitly covers markdown, links, media, typing, and raw `type/event/hook`
- the subscription is persisted under `report_subscriptions`
- the report is delivered to `.codeclaw/gateway/mock-outbox.jsonl`
- the delivery is visible in `cargo run -- job inspect <job-id>`

## UC-10 Bounded Master-Loop Delegation

**Objective**  
Confirm long-running automation can continue safely without unbounded looping.

**Steps**

1. Create a job with:

   ```bash
   cargo run -- job create \
     --title "Nightly backlog sweep" \
     --delegate-master-loop \
     --continue-for-secs 3600 \
     --continue-max-iterations 3
   ```

2. Start `cargo run -- serve`.
3. Inspect `onboard` in the TUI or run `cargo run -- inspect --service`.
4. Verify the job is marked as delegated and the remaining budget decreases over time or iterations.
5. Repeat with `--auto-approve` for a scenario that would otherwise wait for manual approval.

**Expected results**

- the job is clearly marked as delegated to the master loop
- `onboard` shows delegated and auto-approve markers distinctly
- service inspection shows continued jobs and budget-exhausted jobs
- the job stops auto-continuing after the configured time or iteration budget is exhausted
- blocked jobs remain visible instead of looping forever when approval is still required

## UC-11 Runtime Logging and Retention

**Objective**  
Confirm runtime errors and session notifications are archived daily and retained by policy.

**Steps**

1. Run `cargo run -- doctor` or another command that starts the app-server path.
2. Inspect `.codeclaw/logs/archive/<today>/runtime/`.
3. Inspect `.codeclaw/logs/archive/<today>/sessions/`.
4. Confirm `[logging].retention_days` exists in `codeclaw.toml`.

**Expected results**

- runtime logs exist for controller and/or app-server activity
- session notification logs are written under the current day archive
- log paths are day-partitioned rather than one unbounded flat file
- retention and notification buffer are configurable from `codeclaw.toml`

## Sign-off Recommendation

Formal delivery sign-off should record:

- environment used for acceptance
- command outputs for `init`, `doctor`, and at least one `inspect`
- evidence of one worker lifecycle transition
- evidence of restart recovery
- evidence of one gateway delivery
- evidence of one bounded delegated-loop run
- evidence of archived runtime logs for the acceptance day
- any approved known-gap exceptions for the current release
