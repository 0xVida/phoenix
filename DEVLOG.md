# Swarm CI — Dev Log

## Current status
Phase 1 COMPLETE — workspace `cargo test` exit 0 (10 unit + 2 integration tests), kill-and-reassign verified; next action: commit the runtime, then start Phase 2 (Axum API + SSE over EventBus).

## Phase progress
- Phase 0: done
- Phase 1: done — typed state machine, supervisor lease/reaper/reassignment with attempt-based fencing, killable single-task worker runtime; THE test (`killed_worker_is_reassigned_and_task_completes`) plus the attempts-exhaustion test both pass.
- Phase 2: not started
- Phase 3: not started
- Phase 4: not started
- Phase 5: not started

## Decisions made
- 2026-08-24 — Crate layout: Cargo workspace with five crates: `crates/core` (types/state/events/gate), `crates/supervisor` (leases, reaper, reassignment), `crates/worker` (killable Tokio worker runtime), `crates/api` (Axum, Phase 2), `crates/agents` (LlmProvider/planner/implementer, Phase 3). Dependency direction: supervisor/worker/api/agents may depend on core; nothing depends on agents except api/bin; supervisor and worker do NOT depend on each other in prod (only supervisor's tests use swarm-worker via dev-dependencies). Keeps LLM code out of reliability logic (BUILD_PROMPT §0.5).
- 2026-08-24 — State machine: `Pending → Assigned → InProgress → TestsRunning → Passed | Failed`, plus `Passed → Merged`. Reassignment is modeled NOT as a persistent status but as a transition back to `Assigned` with an incremented `attempt` counter (fencing token) plus a `task.reassigned` event. Why: keeps statuses truthful/minimal while making every reassignment observable and stale-worker messages rejectable by comparing attempt numbers.
- 2026-08-24 — Lease/heartbeat defaults: heartbeat every 500 ms; lease expires after 1500 ms (3 missed beats); reap scan every 250 ms; max 3 attempts before terminal `Failed`+`MergeGated`. All configurable (`LeaseConfig`, `SupervisorConfig`). Tests use 100/300/50 ms so kill→reassign→recover is near-instant and deterministic under paused time.
- 2026-08-24 — Timing lives in the supervisor crate using `tokio::time::Instant`, so paused/auto-advancing virtual time works in tests; `core` stays timing-free domain logic.
- 2026-08-24 — Single-writer principle: ONLY the supervisor loop mutates task state and emits `SwarmEvent`s. Workers send messages (`Started/Heartbeat/Finished/Died`) over an mpsc channel. Guarantees the future SSE stream is real backend state, never UI-invented.
- 2026-08-24 — Merge gate is a PURE function in core over a provenance-carrying `TestReport` (`TestOrigin::RealCargoTest | Simulated`). `MergePolicy.require_real_cargo_test` defaults TRUE; Phase 1 tests opt out so the lifecycle is provable before Phase 3 wires real `cargo test`. Agent self-report can never open the gate — only report provenance can. A passed-but-untrusted report blocks the gate (`MergeGated`, task `Failed`).
- 2026-08-24 — `Failed` (attempts exhausted / untrusted-or-failing reports) is terminal for MVP. Revisit post-hackathon.
- 2026-08-24 — Extra event beyond the canonical list: `StaleResultRejected { task_id, worker_id, attempt }` when a superseded worker generation tries to talk to us. Fencing must be visible; becomes the Phase 4 line "Old agent result rejected — this task had already been reassigned".
- 2026-08-24 — Worker failure model: death needs NO special protocol. Silent death (abort/process kill) = missed heartbeats → lease expiry; loud death = `Died` message fast path. Worker runs executor + heartbeat ticker in ONE tokio task (pinned-future `select!`) so `abort()` kills both at once.

## Open questions / blockers
- None blocking. rustc/cargo 1.85.0 on box; edition 2021 chosen for dep compatibility.

## How to resume
1. `cat DEVLOG.md` (this file), then skim `BUILD_PROMPT.md` for the contract.
2. `cargo test --workspace` must stay green — especially `crates/supervisor/tests/kill_and_reassign.rs` (THE test lives there).
3. Next work item (Phase 2): `crates/api` — Axum app exposing POST /tasks (→ `Supervisor::submit_pr`), GET /tasks/:id (→ snapshot), GET /events (SSE mux over `EventBus::subscribe`). Wire one real `WorkerSpawner` bridging assignments to `swarm_worker::run_worker`.
4. Phase 3 after that: `LlmProvider` trait in `crates/agents` (mock + Anthropic), typed plan structs, implementer agent producing `TestReport { origin: RealCargoTest }` ONLY by actually running `cargo test` in a sandbox copy; then flip `require_real_cargo_test` to true in production wiring.

## Log
### 2026-08-24 — Project bootstrap (Phase 0)
- Repo started completely empty. Created this devlog BEFORE any code per contract; saved the operating contract verbatim to `BUILD_PROMPT.md`; added `.gitignore` and `README.md` (env vars documented; keys never committed).
- Fixed architecture decisions above before coding.

### 2026-08-24 — Core domain model (`swarm-core`)
- Implemented: `TaskId`/`WorkerId` newtypes + `Attempt` fencing token; `TaskStatus` machine with CHECKED transitions (illegal moves return `SwarmError::IllegalTransition`; reassignment edges `Assigned/InProgress/TestsRunning → Assigned`, exhaustion edges `→ Failed`); canonical `SwarmEvent` set incl. extra `StaleResultRejected`; `LeaseConfig` with validate(); merge gate as PURE fn over provenance-carrying `TestReport`; worker→supervisor mail protocol in core so supervisor/worker crates stay decoupled.
- 10 unit tests green (state machine happy/skip/reassign/terminal, lease validation, gate policy matrix).

### 2026-08-24 — Supervisor + worker runtime; THE test is green
- Supervisor: one `select!` loop (mailbox + reap tick) owns ALL state and emits ALL events; `submit_pr → assign(attempt 1)`; expired leases emit `worker.failed` then reassign or fail-closed at `max_attempts`.
- Worker runtime: single task polling a PINNED executor future alongside the heartbeat ticker, so an external `abort()` kills both simultaneously and no partial work survives.
- Fencing verified in-test: zombie's late heartbeat AND late "real cargo pass" report are both rejected (`StaleResultRejected`) with zero state impact — the machine stays Merged, exactly one `merge.opened`.
- THE test passes end-to-end: submit → started#1 → abort mid-task → failed#1 via LEASE EXPIRY ONLY → reassigned#2 → started#2 → tests.passed → merge.opened → snapshot Merged/attempt=2, full event ORDER asserted.
- Second test covers the loud-death path: two consecutive crashes → attempts exhausted → terminal `Failed` + exactly one `MergeGated`; gate never opens on a failed task.
- LESSON / dead end: first draft of the fail-closed test used hanging executors for every attempt — but a hanging worker keeps heartbeating, so it NEVER dies and there is nothing to recover from. Tests must model death as silence (abort) or as `Crashed`. Fixed by switching that mode to `CrashFast`.
- Build fixes: `swarm-worker` was missing its `tracing` dep; removed a placeholder `cfg` warning in `swarm-api`.
- Workspace result: `cargo test --workspace` exit 0 — 10 unit + 2 integration tests. Reliability loop (3 sequential runs of the integration suite) launched to satisfy "passes reliably, repeatedly".


