# Swarm CI — Dev Log

## Current status
Phase 3 COMPLETE + LIVE PROVIDERS WIRED — Groq (`openai/gpt-oss-120b`) plans fixes, implementer runs REAL cargo test in per-attempt sandboxes, Google Gemini auto-fallback armed; live HTTP run merged PR-100 on attempt 1 with `origin: RealCargoTest`. Next action: Phase 4 minimal frontend over SSE.

## Phase progress
- Phase 0: done
- Phase 1: done — typed state machine, supervisor lease/reaper/reassignment with attempt-based fencing, killable single-task worker runtime; THE test (`killed_worker_is_reassigned_and_task_completes`) plus the attempts-exhaustion test both pass.
- Phase 2: done — `POST /tasks`, `GET /tasks(/:id)`, demo-kill endpoint, SSE `/events` muxing real backend events; live HTTP smoke test green.
- Phase 3: done — `LlmProvider` trait (`MockLlmProvider`, `AnthropicProvider` via env), planner→`FixPlan`(typed), implementer applies plan in sandbox and emits `TestReport{origin: RealCargoTest}` from actual cargo runs; api selects agent mode when `LLM_PROVIDER=anthropic` and flips gate to require real provenance.
- Phase 4: not started
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
- 2026-08-24 (Phase 2) — Added `POST /tasks/:id/kill` as DEMO FAULT INJECTION (beyond spec's endpoint list): the 2-minute demo needs a fast, visually obvious kill moment, and an HTTP endpoint makes it reproducible for Phase 4/5 without external tooling. It only aborts a worker — recovery remains entirely the supervisor's deterministic job. Clearly labelled demo-only.
- 2026-08-24 (Phase 2) — Pinned axum to 0.7 (not 0.8): 0.8 changed path-parameter syntax (`:id` → `{id}`); 0.7 matches all docs/examples we rely on and is stable on this toolchain.
- 2026-08-24 (Phase 2) — SSE frames carry the spec's dotted names on the `event:` line (e.g. `task.created`) with the full serialized `SwarmEvent` as data; lagging subscribers skip missed events rather than blocking the bus.
- 2026-08-24 (Phase 2) — Until Phase 3, the implementer is a SIMULATED one (`SimulatedImplementer`, reports origin=simulated) and the binary runs the gate in dev mode (`require_real_cargo_test=false`) with a loud warning log. Provenance honesty is preserved everywhere else.
- 2026-08-24 (Phase 2) — MVP task listing uses an in-process registry (`Arc<Mutex<Vec<TaskId>>>` in AppState); post-hackathon replace with a supervisor query API if persistence matters.
- 2026-08-24 (Phase 3) — LLM access stays behind ONE trait method (`complete`); vendor specifics live only in `AnthropicProvider` (Messages API via reqwest+rustls; key/model/base-URL from env). No SDK dependency anywhere else.
- 2026-08-24 (Phase 3) — Planner output is parsed into typed `FixPlan { summary, root_cause, edits:[{path,content}] }`; edits REPLACE whole files. Parser tolerates prose/fences (first `{`..last `}`) and REJECTS absolute paths / `..` components (AgentError::UnsafePath) — the sandbox cannot be escaped by a prompt injection.
- 2026-08-24 (Phase 3) — Implementer copies the pristine fixture into a FRESH per-attempt sandbox (`/tmp/swarm-ci-sandbox/<task-uuid>`) before applying edits, so a killed worker can never leave a half-applied fix for the next generation; failing sandboxes are cleaned up.
- 2026-08-24 (Phase 3) — Provenance honesty is structural: `ImplementerAgent::fix` derives pass/fail from the real cargo exit status and ALWAYS stamps `TestOrigin::RealCargoTest`. The api flips `require_real_cargo_test=true` ONLY in agent mode; simulated mode keeps dev gate + loud warning. A "broken plan" integration test proves wrong fixes report failure, never success.
- 2026-08-24 (Phase 3) — MSRV pin: reqwest's url→idna chain pulled icu 2.x needing rustc 1.88 (we have 1.85). Fixed durably via committed Cargo.lock pin `idna_adapter 1.2.0` (icu 1.x line). Fresh clones inherit it.
- 2026-08-25 — Real providers wired: `GroqProvider` (OpenAI-compatible chat completions, primary) + `GoogleProvider` (Gemini generateContent, heavy-reasoning backup) + `FallbackProvider` chain; `LLM_PROVIDER=groq|google|anthropic|mock`, and any configured `GOOGLE_API_KEY` auto-arms the fallback behind a non-google primary. Keys live ONLY in gitignored `.env`.
- 2026-08-25 — Model defaults corrected against live catalogs: Groq's `llama-3.3-70b-versatile` no longer exists → `openai/gpt-oss-120b`; Google retired `gemini-2.0-flash` → `gemini-3.6-flash`. Lesson: model ids rot — treat them as env config, never hardcode.
- 2026-08-25 — Planner prompt now embeds the sandbox's CURRENT files (`collect_context`) so real models fix actual source instead of guessing; mock flow unaffected.
- 2026-08-25 — Jina AI key stored in `.env` for later diff/page fetching; no code path uses it yet (deliberately — no speculative features).

## Open questions / blockers
- None blocking. rustc/cargo 1.85.0 on box; edition 2021 chosen for dep compatibility.

## How to resume
1. `cat DEVLOG.md` (this file), then skim `BUILD_PROMPT.md` for the contract.
2. `cargo test --workspace` must stay green — especially `crates/supervisor/tests/kill_and_reassign.rs` (THE test lives there).
3. Next work item (Phase 4): minimal frontend — a static HTML/JS page that opens `GET /events` SSE and renders human-readable state changes; translate internal events to plain language (e.g. `stale_result.rejected` → "Old agent result rejected — this task had already been reassigned").
4. Phase 5 after that: rehearse the 2-minute demo (submit → planner → implementer → kill via `POST /tasks/:id/kill` → visible reassign → real cargo test passes → merge gate opens) and drill failure modes (bad API key, network blip, attempts exhaustion).

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
- Workspace result: `cargo test --workspace` exit 0 — 10 unit + 2 integration tests. Reliability CONFIRMED: the integration suite ran 3 consecutive times (`--test-threads=1`) — 3/3 green.

### 2026-08-24 — Phase 2: Axum API + SSE live
- `crates/api` now real: `router(AppState)` with `POST /tasks` (→ `submit_pr`), `GET /tasks` (MVP in-process registry), `GET /tasks/:id` (deterministic snapshot; added `TaskId::parse`/`FromStr` in core for path params), `POST /tasks/:id/kill` (demo fault injection aborting the current worker generation via a join-handle registry inside `DemoSpawner`), and `GET /events` (SSE over `EventBus::subscribe()` via `BroadcastStream`; dotted `event:` names per spec).
- Binary `swarm-api`: env-tuned config (heartbeat/lease/reap/max-attempts/sim-work/bind), single supervisor loop, dev-mode gate warning.
- LIVE SMOKE TEST (real HTTP, real timing): submit PR-7 → status `in_progress`(attempt 1) → POST kill → within ~2 s supervisor observed lease expiry and reassigned → fresh worker finished → final snapshot `"status":"merged","attempt":2`. SSE capture shows exactly: `worker.failed` → `task.reassigned` → `worker.started`(#2) → `tests.passed(simulated)` → `merge.opened`. Events are provably backend truth — they come from the same bus the supervisor alone writes to.
- Compile fixes along the way: tokio_stream's `filter_map` is sync (no async block); `tracing-subscriber` missing from api deps; hand the supervisor a plain `DemoSpawner` clone instead of `Arc<...>` (trait not implemented for Arc wrapper).
- Full workspace `cargo test` re-run after ids.rs changes — still green (see /tmp/swarm_p2_test.log).

### 2026-08-24 — Phase 3: agents + REAL merge gate provenance
- `crates/agents` implemented: `LlmProvider` trait; `MockLlmProvider` (deterministic, offline — answers the planner contract with the known-good fixture fix); `AnthropicProvider` (Messages API via reqwest+rustls, env-driven key/base-url/model).
- Typed planning: `planner_prompt` contract ("TASK: PLAN" prefix) → model JSON → `FixPlan` via tolerant extractor (first `{`..last `}`) with strict validation (no absolute paths, no `..`, ≥1 edit) — unit-tested against prose-wrapped and malicious outputs.
- `ImplementerAgent::fix`: fresh sandbox copy of `fixtures/demo-pr` per attempt → apply whole-file edits → **real** `cargo test --quiet` → honest `TestReport { origin: RealCargoTest }`; failing sandboxes removed.
- Fixture PR: `fixtures/demo-pr` (ledger crate whose `sum` skips the last element; tests fail pre-fix).
- API wiring: `DemoSpawner` now takes an `ExecutorFactory(&Assignment) -> Box<dyn TaskExecutor>`; new `AgentExecutor` bridges agent→worker port. main selects agent mode when `LLM_PROVIDER=anthropic` and flips `require_real_cargo_test=true`; mock/simulated mode keeps dev gate with loud warning.
- Tests: plan parsing trio; agent-flow integration pair runs REAL cargo in sandboxes — good fix passes (`RealCargoTest`, passed), deliberately-broken fix reports FAILURE (anti-self-report property proven end to end). Full workspace suite green (`/tmp/swarm_p3_all2.log`, EXIT:0), kill-and-reassign untouched.
- Dead end hit + fixed: reqwest→url→idna_adapter 1.2.2 needs rustc 1.88 > our 1.85 → pinned `idna_adapter 1.2.0` (icu 1.x) via committed Cargo.lock.

### 2026-08-25 — Live providers (Groq primary, Google fallback) — REAL merge gate demoed
- Added `GroqProvider` / `GoogleProvider` / `FallbackProvider` + `provider_from_env()`; `.env` (gitignored) holds the user's Groq/Google/Jina keys. Jina intentionally unused for now.
- First live run FAILED CLOSED beautifully: retired model ids on BOTH providers (`llama-3.3-70b-versatile` gone from Groq; `gemini-2.0-flash` retired on Google) → all 3 attempts crashed → supervisor reassigned twice → terminal `Failed` + single `merge.gated`. Zero silent losses; the failure-mode drill ran itself.
- Diagnosed via structured logs; queried Groq `/v1/models`; switched to `openai/gpt-oss-120b` + `gemini-3.6-flash`.
- SECOND live run: **PR-100 merged on attempt 1** — planner produced a valid typed plan, implementer applied it in a fresh sandbox, REAL cargo test passed, `TestsPassed { origin: RealCargoTest }`, `merge.opened`. Gate is now provably tied to genuine test runs in production config.
- Workspace tests green after all changes (`/tmp/swarm_p3d_all.log`, EXIT:0): 9 agents units (incl. parser + fallback chain), 2 real-cargo agent flows, 10 core, 2 kill-and-reassign.




