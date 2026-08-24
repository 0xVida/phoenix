# Swarm CI — Self-Healing PR Review

A planner agent and an implementer agent review and fix a pull request behind a **deterministic merge gate**. If the implementer process is killed mid-task, a supervisor detects the failure via a heartbeat/lease mechanism, reassigns the work to a fresh worker, and the system finishes correctly — demonstrated live.

Built for the Swarm Village hackathon. See `BUILD_PROMPT.md` for the full spec and operating contract, and `DEVLOG.md` for the living build log (source of truth for current status).

## Architecture (one paragraph)

The user submits a PR → the **planner agent** (LLM) reads the diff and produces a typed plan → a deterministic dispatcher hands the plan to an **implementer worker** (Tokio task) that edits a sandboxed working copy, sends heartbeats, and runs tests → the **supervisor** tracks a lease per in-flight task and reassigns to a fresh worker whenever heartbeats stop → a **deterministic merge gate** opens only on a real passing `cargo test` run (never on agent self-report).

Agentic layers may be wrong and are never trusted blindly. Deterministic layers own the truth: state transitions, leases, reassignment, test execution, the merge gate.

## Layout


```
crates/
  core/        domain types: task state machine, events, lease config, merge gate (pure)
  supervisor/  lease tracking, reap loop, reassignment with fencing (attempt numbers)
  worker/      killable Tokio worker runtime with pluggable TaskExecutor
  api/         Axum HTTP API + SSE event stream (Phase 2)
  agents/      LlmProvider trait, planner + implementer agents (Phase 3)
```

## Quickstart

```bash
cargo build            # workspace build
cargo test             # unit + integration tests (the Phase 1 kill-and-reassign test lives in swarm-supervisor)
RUST_LOG=info cargo run -p swarm-api   # once Phase 2 lands
```

## Environment variables

Never commit API keys. Put them in `.env` (gitignored) — a `.env.example` will ship with Phase 3.

| Variable           | Required | Default                        | Purpose                                   |
|--------------------|----------|--------------------------------|-------------------------------------------|
| `LLM_PROVIDER`     | no       | `mock`                         | `mock` \| `anthropic` (extensible)         |
| `ANTHROPIC_API_KEY`| yes if provider=anthropic | —              | API key for the Anthropic implementation   |
| `ANTHROPIC_BASE_URL`| no      | `https://api.anthropic.com`    | Override for proxies/local gateways        |
| `SWARM_HEARTBEAT_MS`| no      | `500`                          | Worker heartbeat interval                  |
| `SWARM_LEASE_TIMEOUT_MS`| no  | `1500`                         | Lease expiry (≈3 missed heartbeats)        |
| `SWARM_MAX_ATTEMPTS`| no      | `3`                            | Reassignments before the task fails        |

The LLM layer is provider-agnostic behind a trait (`LlmProvider::complete`); the supervisor/worker/gate logic never talks to a vendor SDK.
