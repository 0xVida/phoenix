# Swarm CI — Dev Log

## Current status
Phase 5 COMPLETE — SUBMISSION READY, plus post-Done hardening: hierarchical tracing spans (spec §4 fully honored: per-task/per-message/per-worker with planner·apply_edits·cargo_test children) and 3 in-process HTTP contract tests guarding the API. Total 26 workspace tests green. Remaining optional polish: none blocking.

## Phase progress
- Phase 0: done
- Phase 1: done — typed state machine, supervisor lease/reaper/reassignment with attempt-based fencing, killable single-task worker runtime; THE test (`killed_worker_is_reassigned_and_task_completes`) plus the attempts-exhaustion test both pass.
- Phase 2: done — `POST /tasks`, `GET /tasks(/:id)`, demo-kill endpoint, SSE `/events` muxing real backend events; live HTTP smoke test green.
- Phase 3: done — provider-agnostic `LlmProvider` with LIVE providers (Groq `openai/gpt-oss-120b` primary, Google Gemini auto-fallback), typed `FixPlan`, implementer runs REAL cargo test in per-attempt sandboxes; gate requires RealCargoTest provenance in agent mode. Live PR-100 merged.
- Phase 4: done — embedded dashboard (`crates/api/assets/index.html` served at `/`): EventSource stream → plain-language feed (incl. the spec's exact "Old agent result rejected…" line), submit form, kill button, heartbeats as live indicator. Verified: HTML served + PR-104 merged through the real agent path.
- Phase 5: done — `scripts/rehearse.sh 2 2` drove submit→kill@2s→reassign→real-test-pass→merge TWICE consecutively (SSE showed worker.failed/task.reassigned both times); `scripts/failure_drill.sh` invalidated both provider keys → system failed closed (task Failed, exactly 1 merge.gated, 0 merge.opened).
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
- 2026-08-25 (Phase 4) — Dashboard is served BY the binary (`include_str!` of one static file at `/`): zero build step, zero CDN, no CORS. Events consumed via native `EventSource` (auto-reconnect). Heartbeats render as a pulsing indicator + counter instead of feed rows so the recovery narrative stays readable; the spec's exact Phase 4 line ("Old agent result rejected — this task had already been reassigned") is implemented for `stale_result.rejected`.
- 2026-08-25 (Phase 5) — Demo repeatability: `scripts/rehearse.sh [runs] [kill_after_s]` and `scripts/failure_drill.sh` committed so rehearsal/me drills are one command each, not tribal memory.
- 2026-08-25 (Phase 5) — `SWARM_PLAN_DELAY_MS` demo knob (default 0): with fast Groq responses the whole fix finished <2 s, racing the presenter's kill; the knob pauses pre-planning to widen the mid-flight window. Async sleep only — heartbeats keep flowing during the pause. Unset in tests/production.
- 2026-08-25 (Phase 5) — Failure-drill semantics locked: provider outage ⇒ attempts exhaust ⇒ task `Failed`, exactly ONE `merge.gated`, ZERO `merge.opened`. Asserted by script, proven by drill.
- 2026-08-25 (hardening) — Spec §4 spans completed: rootless `worker` span created in the api spawner bridge (`Instrument` on the run_worker future) so planner/apply_edits/cargo_test nest beneath it with task/worker/attempt fields; supervisor wraps every inbound message in a `supervisor_msg{kind,task_id,worker_id,attempt}` span (sync enter() only — no guard across await). Console fmt shows the hierarchy as event prefixes; OTel export deliberately deferred (spec: only if spare time at the very end).
- 2026-08-25 (hardening) — Added `crates/api/tests/http_smoke.rs`: router exercised directly via `tower::ServiceExt::oneshot` (hermetic, no sockets). Covers dashboard-at-`/`, submit→snapshot→list shapes, plain-language 400/404 translation, kill-on-unknown → NOT_FOUND. SSE streaming intentionally excluded (covered by live smokes). Gotcha recorded: axum Router is consumed by oneshot — clone per request; `&format!` URIs need `&str` params, not `&'static str`.
- 2026-08-25 (ergonomics) — Blessed launcher is `scripts/swarm.sh`, NOT a `dotenvy` dep inside main.rs: keeps the binary's contract as plain env vars (spec §3), avoids an extra crate, and gives us free real estate for pre-flight UX (provider key check before compiling, port-busy hint with exact recovery command, provider/model banner). `.env` fills gaps but never overrides already-exported vars; `--mock` forces offline regardless of `.env`.

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

### 2026-08-25 — Phase 4: self-hosted dashboard over the real SSE stream
- New `crates/api/assets/index.html` (embedded via `include_str!`, served at `/`): submit form (prefilled with the ledger bug), live task panel (status badge, attempt badge, heartbeat pulse+counter), ⚡KILL button wired to `POST /tasks/:id/kill`, and an event feed translating every `SwarmEvent` into plain language — e.g. lease expiry renders as "worker died mid-task — stopped sending heartbeats", reassignment as "supervisor reassigned the task to a FRESH worker", and the spec-mandated line for stale-result fencing.
- Verified: binary builds (`EXIT:0`), `GET /` returns the page, and a fresh PR-104 submission through the REAL Groq agent merged again over HTTP.
- Remaining for Phase 5: rehearse the exact 2-minute flow twice, drill failure modes (bad key / network blip / attempts exhaustion), confirm the kill moment reads instantly on screen.

### 2026-08-25 — Phase 5: rehearsed ×2, drilled, submission-ready
- `scripts/rehearse.sh 2 2`: two consecutive full demos — submit PR → planner (real Groq) starts → ⚡KILL at t=2 s → lease expiry detected → visible reassignment → fresh implementer plans + runs REAL cargo test → merged. SSE transcript per run contained worker.failed AND task.reassigned before merge.opened. **2/2 PASS.**
- Discovery during rehearsal: Groq+cargo now finish in <2 s, so the default kill window raced completion (first attempt showed no reassigned events). Added demo knob `SWARM_PLAN_DELAY_MS` (async pause pre-planning; heartbeats unaffected) and re-ran with 4000 ms — recovery path fully visible.
- `scripts/failure_drill.sh` (both provider keys invalidated): attempts exhausted → task `failed`, exactly one `merge.gated`, ZERO `merge.opened`. **PASS — fail-closed proven under total provider outage.**
- Fixed along the way: swarm-agents needed tokio as a real dep for async sleep; shell typo in drill summary; scripts no longer swallow build errors.
- DEFINITION OF DONE: all five boxes met (reliable Phase-1 test; live real-kill demo with visible recovery; gate provably tied to RealCargoTest; stranger-ready DEVLOG; ≥2 end-to-end rehearsals).

### 2026-08-25 — Post-Done hardening: spans + API contract tests
- §4 observability finished: `worker` rootless span (api spawner bridge) parents every worker-generation log and its `planner` / `apply_edits` / `cargo_test` child spans (cargo now timed with duration_ms + passed fields); supervisor loop wraps each inbound message in `supervisor_msg{kind,task_id,worker_id,attempt}`. Verified live: console lines render as `supervisor_msg{kind="heartbeat" task_id=… attempt=1}: swarm_event …`.
- New hermetic API tests (`crates/api/tests/http_smoke.rs`, tower oneshot): dashboard served, submit→snapshot→list contract, plain-language 400/404s, kill-unknown → 404. Workspace total now **26 tests, all green**.

### 2026-08-25 — One-command launcher: `scripts/swarm.sh`
- Auto-loads repo-root `.env` (without overriding exported vars), pre-flights provider keys (fails fast with plain-language message BEFORE compiling) and the port (busy → exact `pkill` hint), prints provider/model banner, then `exec`s `cargo run -p swarm-api` in the foreground. Flags: `--mock` (force offline), `--release`.
- Verified all paths live: help text; groq-without-any-key exits 1 with "GROQ_API_KEY is not set"; `--mock` serves `{"tasks":[]}` on :3000; second concurrent launch exits 1 with port-busy hint.

### 2026-08-25 — Side-project demo target + PR-link input layer
- New sibling repo **`swarm-demo-target`** (pushed to `github.com/Ay-obami/swarm-demo-target`, public): multi-module `warehouse` crate (currency bps math, pricing, discount stacking, batch reservation, report helpers). Green `main`; THREE seeded bug branches (`pr/101-discount-stack`, `pr/102-fx-rounding`, `pr/103-oversell-boundary`) each carrying its red acceptance tests; published both as branches AND as GitHub PRs (#1 fx, #2 oversell, #3 discount-cap — repo-global numbering!). `scripts/setup_prs.sh [remote]` re-seeds deterministically from patch files; `scripts/open_prs.sh` opens the real PRs via gh.
- Swarm CI input layer: `TaskSpec` gained optional `pr_url` / `repo_url` / `git_ref`. New `agents::workspace` resolves priority repo_url > pr_url > fixture; GitHub PR links map to cloning `refs/pull/N/head` (quote-aware `parse_github_pr` unit-tested); git checkout is shallow fetch + FORCE checkout of FETCH_HEAD. `ImplementerAgent::fix` now takes the whole `TaskSpec`, prepares the sandbox FIRST, then reads context from the clone itself.
- TWO real failure modes found & fixed during live runs:
  1. Blocking `git clone`/`cargo test` starved the worker's own heartbeat timer → supervisor correctly reaped healthy workers mid-clone (3× loop). Fix: heavy FS/subprocess work moved to `tokio::task::spawn_blocking` so heartbeats keep flowing. Great accidental proof the lease mechanism catches slow (not just dead) workers.
  2. Groq 429 TPM + model prose around JSON broke naive plan extraction. Fix: quote-aware balanced-brace extractor tries direct parse → fenced blocks → every complete `{…}` object until one validates.
- FINAL LIVE RESULT: pasted `…/pull/1` → **status merged, attempt 1, WALL 12.3 s**, `TestsPassed { origin: RealCargoTest }`. Workspace suite green throughout (`/tmp/swarm_git_t3.log`). GitHub-side gotcha logged: force-pushing a PR branch auto-updates its head (used to sync reseeded branches); refs/pull/* themselves are read-only on GitHub.









