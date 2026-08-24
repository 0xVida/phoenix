//! Typed event stream. These events ARE the backend's real state changes:
//! only the supervisor emits them (single-writer principle), so anything the
//! UI shows downstream of this stream is truth, never a UI-invented story.

use crate::ids::{Attempt, TaskId, WorkerId};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestOrigin {
    /// Produced by actually executing `cargo test` in the sandbox (Phase 3).
    RealCargoTest,
    /// Produced by a scripted/simulated runner (Phase 1 dev mode only).
    Simulated,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SwarmEvent {
    TaskCreated {
        task_id: TaskId,
        pr_id: String,
        title: String,
    },
    WorkerStarted {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
    },
    WorkerHeartbeat {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
    },
    WorkerFailed {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
        reason: String,
    },
    TaskReassigned {
        task_id: TaskId,
        attempt: Attempt,
    },
    /// Fencing in action: a superseded (zombie) worker tried to talk to us.
    StaleResultRejected {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
    },
    TestsPassed {
        task_id: TaskId,
        origin: TestOrigin,
    },
    TestsFailed {
        task_id: TaskId,
        reason: String,
    },
    MergeGated {
        task_id: TaskId,
        reason: String,
    },
    MergeOpened {
        task_id: TaskId,
    },
}

/// Fan-out bus over `tokio::sync::broadcast`. Emitting with zero subscribers
/// is fine; lagging subscribers skip missed events rather than blocking truth.
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<SwarmEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Single-writer note: production emitters are the supervisor loop and its
    /// pure helpers only.
    pub fn emit(&self, event: SwarmEvent) {
        tracing::info!(event = ?event, "swarm_event");
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SwarmEvent> {
        self.tx.subscribe()
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus").finish_non_exhaustive()
    }
}
