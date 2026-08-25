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
cargo build                              # workspace build
cargo test                               # 10 unit + 2 integration tests (kill-and-reassign lives in swarm-supervisor)

# Run the API (dev mode: workers are SIMULATED implementers until Phase 3):
RUST_LOG=info cargo run -p swarm-api
```

Then exercise the self-healing flow live:

```bash
TID=$(curl -s -XPOST localhost:3000/tasks -H 'content-type: application/json' \
  -d '{"pr_id":"PR-7","title":"Fix off-by-one","bug_description":"sum skips last element"}' \
  | sed -E 's/.*"task_id":"([^"]+)".*/\1/')

curl -sN localhost:3000/events &                    # watch real backend events (SSE)
sleep 1 && curl -s -XPOST localhost:3000/tasks/$TID/kill   # THE DEMO MOMENT: kill mid-task
sleep 6    && curl -s localhost:3000/tasks/$TID            # reassigned + merged (attempt: 2)
```

## HTTP API

| Endpoint                 | Method | Purpose                                                        |
|--------------------------|--------|----------------------------------------------------------------|
| `/tasks`                 | POST   | Submit a PR (`{pr_id,title,bug_description}`) → `{task_id}`     |
| `/tasks`                 | GET    | List submitted tasks with live status/attempt                   |
| `/tasks/:id`             | GET    | Deterministic snapshot of the task record                       |
| `/tasks/:id/kill`        | POST   | Demo fault injection: abort current worker; supervisor recovers |
| `/events`                | GET    | SSE; `event:` lines use spec names (`task.created`, …)          |

| Variable           | Required | Default                        | Purpose                                   |
|--------------------|----------|--------------------------------|-------------------------------------------|
| `LLM_PROVIDER`     | no       | `mock`                         | `mock` \| `anthropic` (extensible, Phase 3)|
| `ANTHROPIC_API_KEY`| yes if provider=anthropic | —              | API key for the Anthropic implementation   |
| `ANTHROPIC_BASE_URL`| no      | `https://api.anthropic.com`    | Override for proxies/local gateways        |
| `SWARM_BIND`       | no       | `0.0.0.0:3000`                 | API listen address                         |
| `SWARM_HEARTBEAT_MS`| no      | `500`                          | Worker heartbeat interval                  |
| `SWARM_LEASE_TIMEOUT_MS`| no  | `1500`                         | Lease expiry (≈3 missed heartbeats)        |
| `SWARM_REAP_INTERVAL_MS`| no  | `250`                          | Supervisor lease-reaper scan cadence       |
| `SWARM_MAX_ATTEMPTS`| no      | `3`                            | Reassignments before the task fails        |
| `SWARM_SIM_WORK_MS`| no       | `3000`                         | Simulated implementer duration (Phase 2)   |
| `SWARM_FIXTURE_DIR`| no       | `fixtures/demo-pr`             | PR fixture copied into sandboxes (agent mode) |
| `SWARM_SANDBOX_ROOT`| no      | `$TMPDIR/swarm-ci-sandbox`     | Where per-attempt sandboxes are created    |
| `GROQ_API_KEY`     | yes if provider=groq      | —                              | Primary LLM (Groq Cloud, OpenAI-compatible)|
| `GROQ_MODEL`       | no       | `openai/gpt-oss-120b`          | Groq chat model id                         |
| `GOOGLE_API_KEY`   | no       | —                              | Gemini heavy-reasoning **auto-fallback**   |
| `GOOGLE_MODEL`     | no       | `gemini-3.6-flash`             | Google model id                            |
| `JINA_API_KEY`     | no       | —                              | Reserved: diff/page fetching (later phase) |

Never commit API keys. Copy `.env.example` → `.env`, fill it in, and load it
before running (`set -a; source .env; set +a`) — the binary reads plain
environment variables.

### Agent mode (real merge gate, Phase 3)

With `LLM_PROVIDER=anthropic` + a valid key:

1. the **planner** turns the bug description into a typed JSON plan (`FixPlan`),
2. the **implementer** copies `fixtures/demo-pr` into a fresh sandbox per attempt,
   applies the planned whole-file edits, and runs **real `cargo test`**,
3. the merge gate opens ONLY for reports with provenance `real_cargo_test` —
   a model claiming success proves nothing.

Without a key, `LLM_PROVIDER=mock` keeps everything offline in dev mode
(simulated reports allowed; loudly warned at startup).



The LLM layer is provider-agnostic behind a trait (`LlmProvider::complete`); the supervisor/worker/gate logic never talks to a vendor SDK.
