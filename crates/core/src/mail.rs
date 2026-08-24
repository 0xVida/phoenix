//! Mail protocol between workers and the supervisor, living in core so the
//! supervisor and worker crates share it without depending on each other.
//!
//! EVERY message carries `attempt` — the supervisor fences (rejects) any
//! message from a superseded worker generation. This is what makes
//! kill-and-reassign safe against zombie workers.

use crate::error::SwarmError;
use crate::gate::TestReport;
use crate::ids::{Attempt, TaskId, WorkerId};
use crate::lease::LeaseConfig;
use crate::task::TaskSpec;
use tokio::sync::mpsc;

/// One (re)assignment of a task to a worker generation.
#[derive(Debug, Clone)]
pub struct Assignment {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub attempt: Attempt,
    pub spec: TaskSpec,
    pub lease: LeaseConfig,
}

/// Worker → supervisor, single direction. Workers never touch shared state.
#[derive(Debug)]
pub enum WorkerMsg {
    /// First contact: this worker generation picked up its assignment.
    Started {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
    },
    /// Lease renewal.
    Heartbeat {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
    },
    /// Executor finished. The report's PROVENANCE (not the worker's claim)
    /// decides what the merge gate does with it.
    Finished {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
        report: TestReport,
    },
    /// Worker noticed its own failure and exited cleanly.
    Died {
        task_id: TaskId,
        worker_id: WorkerId,
        attempt: Attempt,
        reason: String,
    },
}

/// Cloneable sender side. Workers and spawners hold copies.
#[derive(Clone, Debug)]
pub struct SupervisorHandle {
    tx: mpsc::UnboundedSender<WorkerMsg>,
}

impl SupervisorHandle {
    /// Intended caller: `swarm_supervisor::Supervisor::new`.
    pub fn new(tx: mpsc::UnboundedSender<WorkerMsg>) -> Self {
        Self { tx }
    }

    /// Fire-and-forget: a dead supervisor is discovered via lease expiry, not
    /// by workers blocking on send.
    pub fn send(&self, msg: WorkerMsg) -> Result<(), SwarmError> {
        self.tx.send(msg).map_err(|_| SwarmError::ChannelClosed)
    }
}
