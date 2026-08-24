# Swarm CI — Build Prompt (spec + operating contract)

> This file is the verbatim build prompt pasted into whatever coding agent
> (Claude Code, another LLM, or a human dev picking up mid-build) starts or
> resumes work. It is both the spec and the operating contract for the build.

---

## 0. Role and operating contract

You are the engineering agent building **Swarm CI** for the Swarm Village hackathon. Before writing any code:

1. Check whether `DEVLOG.md` already exists in the repo root.
   - If it exists, **read it first**. It is the single source of truth for what's done, what's in progress, and what decisions have already been made. Do not re-derive architecture decisions that are already recorded there — follow them, or explicitly propose a change and log why.
   - If it doesn't exist, create it using the template in Section 6 before writing any other code.
2. Never treat this build prompt as read-once. Re-read `DEVLOG.md` at the start of every session/turn where you resume work — another agent or the user may have changed things since you last looked.
3. **Documentation is not a wrap-up task.** Update `DEVLOG.md` after every meaningful unit of work — a finished function, a design decision, a failed approach, a test passing — not just at the end of a session. If you would be embarrassed for the log to stop mid-phase with no note, update it now.
4. **Commit as you go**, not in one giant commit at the end. See Section 7 for cadence and message format.
5. Agents propose, code controls: keep the LLM calls (planner/implementer reasoning) cleanly separated from the deterministic Rust logic that actually validates, tests, and gates the merge. Never let an agent's self-report of success be the thing that opens the merge gate — only a real `cargo test` run does that.

---

## 1. Project pitch (fixed — do not redesign without logging why)

**Swarm CI — Self-Healing PR Review**: A planner agent and an implementer agent review and fix a pull request behind a deterministic merge gate. If the implementer process is killed mid-task, a supervisor detects the failure via a heartbeat/lease mechanism, reassigns the work to a fresh worker, and the system finishes correctly — demonstrated live.

**Problem**: Automated PR agents are typically all-or-nothing. If the process dies, the PR sits stuck with no recovery story.

**Why agents**: Reading a diff, planning a fix, and writing code are reasoning tasks. Deciding "is this safe to merge" is not — that's a deterministic gate.

**Why Rust**: The supervisor/worker/actor pattern with real process failure and reassignment is a natural fit for Tokio tasks plus a lease/heartbeat mechanism, and it's hard to fake convincingly in a weaker runtime.

---

## 2. Architecture

```text
User submits PR
      │
      ▼
   Router
      │
      ▼
 Planner agent  ──reads code, produces a plan──▶  plan (structured, typed)
      │
      ▼
Deterministic dispatcher
      │
      ▼
 Implementer worker (Tokio task)
   - edits files in a sandboxed working copy
   - runs tests
   - sends heartbeats to Supervisor via a lease
      │
      ▼
 Supervisor
   - tracks lease/heartbeat per in-flight task
   - on missed heartbeat / dead worker: reassigns task to a fresh worker
      │
      ▼
Deterministic merge gate
   - requires a REAL `cargo test` run to pass (never agent self-report)
      │
      ▼
Merge allowed / PR updated
```

**Deterministic vs agentic split**
- Agentic (may be wrong, is not trusted blindly): reading the diff, proposing a plan, writing the fix.
- Deterministic (owns truth): task state transitions, lease/heartbeat tracking, reassignment logic, running the actual test suite, opening/blocking the merge gate.

**Key failure/security constraint**: the implementer agent has no merge or deploy privilege. It can only edit a sandboxed working copy and run tests inside it. If it crashes mid-edit, the Supervisor must detect the stale lease and reassign — the task must never be silently lost.

---

## 3. Tech stack

- **Tokio** — async runtime, worker tasks, supervisor loop
- **Axum** — HTTP API / event stream for the frontend
- **Serde** — structured data for plans, tool results, events
- **`tracing` / `tracing-subscriber`** — observability; use structured fields (`task_id`, `agent_id`, `status`), not free-form strings; avoid stuffing full prompts/outputs into trace fields
- **`thiserror`** — typed errors
- **Rig** — optional, for the agent/LLM abstraction layer only; keep it out of the reliability/business logic

**LLM provider**: build this **provider-agnostic** behind a trait (e.g. `trait LlmProvider { async fn complete(&self, prompt: &str) -> Result<String, LlmError>; }`). Read the API key and base URL from environment variables, never hardcode them. This lets the same code run against Anthropic, Groq, Gemini, or a local mock without changes to the supervisor/worker logic. **Never commit API keys** — use a `.env` file that is gitignored, and document the required env vars in `README.md`.

## 4. Observability

- Events: `task.created`, `worker.started`, `worker.heartbeat`, `worker.failed`, `task.reassigned`, `tests.passed`, `tests.failed`, `merge.gated`, `merge.opened`.
- Spans: one span per task, with the planner call, implementer call, and test run as nested spans underneath it.
- Console `tracing` output is sufficient for the hackathon MVP — do not build OpenTelemetry export infrastructure unless there's spare time at the very end.

---

## 5. Phase plan

Work through phases in order. Do not skip ahead into UI polish before Phase 1 is solid — a convincing kill-and-recover demo depends entirely on Phase 1 being real.

### Phase 0 — Architecture (no code yet, or skeleton only)
- [ ] Confirm crate layout (suggest: `swarm-ci/` workspace with `core` (types, state), `supervisor`, `worker`, `api`, `agents` crates or modules)
- [ ] Define the task state machine explicitly (e.g. `Pending → Assigned → InProgress → TestsRunning → Passed/Failed → Reassigned → Merged`)
- [ ] Decide the lease/heartbeat parameters (heartbeat interval, lease timeout) and write them down
- [ ] Initialize git repo, first commit: skeleton + this build prompt + empty `DEVLOG.md`

### Phase 1 — Core behavior (prove it without UI)
- [ ] Implement the task state machine with types (no `String` soup — use enums)
- [ ] Implement Supervisor with lease tracking and reassignment logic
- [ ] Implement Worker as a Tokio task that can be killed (simulate via `tokio::task::abort` or an actual killed process, whichever is more demoable)
- [ ] Write a test that: starts a task, kills the worker mid-task, asserts the Supervisor reassigns it, asserts the task completes
- [ ] This is the single most important test in the project — the whole demo hinges on it being real

### Phase 2 — Expose it
- [ ] Axum API: submit a PR/task, stream events (SSE or WebSocket) for task state changes
- [ ] Make sure events are the real backend state, not a separate UI-invented story

### Phase 3 — Agent integration
- [ ] Implement `LlmProvider` trait + at least one real implementation (Anthropic, since it's the most likely to be reachable in most build environments) + a mock implementation for offline dev
- [ ] Planner agent: reads a diff/bug description, produces a structured plan (typed, via Serde, not free text)
- [ ] Implementer agent: takes the plan, edits files in a sandboxed working copy, triggers `cargo test`
- [ ] Merge gate only opens on a real passing `cargo test` run — never on the agent's own claim of success

### Phase 4 — UX
- [ ] Minimal frontend (even a simple HTML/JS page is fine) that shows real backend events as human-readable state changes
- [ ] Translate internal errors into plain language for the UI (e.g. "Old agent result rejected — this task had already been reassigned" instead of a raw error struct)

### Phase 5 — Demo hardening
- [ ] Rehearse the exact 2-minute flow: submit PR → planner explains fix → implementer starts → kill implementer live → supervisor visibly reassigns → fresh worker resumes → tests pass → merge gate opens
- [ ] Test failure modes: network blip, model API failure, slow response, demo reset button
- [ ] Confirm the "kill" moment is fast and visually obvious, not an awkward silent pause

---

## 6. `DEVLOG.md` template

Create this file at the repo root before writing any other code. Update it continuously — every phase checkbox ticked, every non-trivial decision, every dead end.

```markdown
# Swarm CI — Dev Log

## Current status
<one line: what phase, what's the very next action>

## Phase progress
- Phase 0: <not started / in progress / done>
- Phase 1: <...>
- Phase 2: <...>
- Phase 3: <...>
- Phase 4: <...>
- Phase 5: <...>

## Decisions made
- <date/commit> — <decision> — <why>

## Open questions / blockers
- <anything unresolved that the next agent needs to make a call on>

## How to resume
<the exact next command/file to look at if a new agent picks this up cold>

## Log
### <date> — <short heading>
<what was done, what was learned, what's next>
```

---

## 7. Commit discipline

- Commit after every meaningful unit of work (a passing test, a working module, a fixed bug) — not once at the end of a session.
- Use conventional commit style: `feat:`, `fix:`, `test:`, `docs:`, `chore:`.
- Every commit that changes behavior should be paired with a `DEVLOG.md` update in the same commit or the very next one.
- Never leave the working tree in an uncommitted state at the end of a session — a new agent picking this up should be able to `git log` and `cat DEVLOG.md` and understand exactly where things stand.

---

## 8. Definition of done (hackathon submission)

- [ ] The Phase 1 kill-and-reassign test passes reliably, repeatedly
- [ ] The live demo can trigger a real worker kill and show a real, visible reassignment and recovery
- [ ] Merge gate is provably tied to a real `cargo test` result, not agent self-report
- [ ] `DEVLOG.md` is current and a stranger could resume the project from it alone
- [ ] 2-minute demo rehearsed at least twice end-to-end

