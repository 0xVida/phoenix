//! HTTP API + SSE event stream (Phase 2).
//!
//! Events are the REAL backend state: the supervisor is the single writer and
//! sole emitter, and `/events` simply muxes `EventBus::subscribe()` — there is
//! no UI-invented story anywhere in this crate.
//!
//! Endpoints:
//! - `POST /tasks`          → submit a PR (`Supervisor::submit_pr`)
//! - `GET  /tasks`          → list submitted tasks with live status
//! - `GET  /tasks/:id`      → deterministic snapshot of the `TaskRecord`
//! - `POST /tasks/:id/kill` → DEMO fault injection: aborts the current worker;
//!   recovery is then entirely the supervisor's job (lease expiry → reassign)
//! - `GET  /events`         → SSE; `event:` lines use the spec's dotted names

pub mod routes;
pub mod spawner;
pub mod state;

pub use routes::router;
