//! HTTP API + SSE event stream — Phase 2.
//!
//! Plan (do not start before Phase 1 test passes; see DEVLOG):
//! - POST /tasks     → submit a PR (wraps `Supervisor::submit_pr`)
//! - GET  /tasks/:id → snapshot of `TaskRecord`
//! - GET  /events    → SSE over `EventBus::subscribe`; events are real backend
//!   state because the supervisor is the single writer/emitter.
