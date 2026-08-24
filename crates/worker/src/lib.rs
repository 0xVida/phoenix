//! Killable implementer-worker runtime.
//!
//! Failure model (the whole point, BUILD_PROMPT.md §1/§2): the worker owns NO
//! shared state and NO authority. It runs a pluggable [`TaskExecutor`] between
//! heartbeats and reports outcomes to the supervisor over a channel. If the
//! task is aborted/killed mid-flight (demo kill, panic, OOM, `kill -9` of a
//! future real process), its heartbeats simply stop — and the supervisor's
//! lease expiry does the recovery. Death needs no special protocol.
//!
//! Implementation note: heartbeat ticker and executor are polled in ONE task
//! via `select!` over a pinned executor future, so an `abort()` kills both at
//! once and the executor's partial progress is never silently resumed.

use async_trait::async_trait;
use swarm_core::gate::TestReport;
use swarm_core::mail::{Assignment, SupervisorHandle, WorkerMsg};

/// What the pluggable work layer reports back.
#[derive(Debug)]
pub enum ExecutorOutcome {
    /// Work finished; the report's provenance (not this claim) drives the gate.
    Completed(TestReport),
    /// The worker noticed its own failure and exited cleanly.
    Crashed(String),
}

/// The actual work between heartbeats: Phase 3 plugs the implementer agent
/// here (edit sandbox copy + run tests); tests plug scripted behavior.
///
/// Must be cancellation-safe: dropping the future mid-`await` must leave
/// nothing half-applied that a later attempt would trip over.
#[async_trait]
pub trait TaskExecutor: Send {
    async fn execute(&mut self) -> ExecutorOutcome;
}

/// Run one worker generation for one assignment until it finishes, crashes,
/// or is killed from outside (`tokio::task::abort` / process death).
pub async fn run_worker(
    assignment: Assignment,
    supervisor: SupervisorHandle,
    mut executor: Box<dyn TaskExecutor>,
) {
    let Assignment {
        task_id,
        worker_id,
        attempt,
        lease,
        ..
    } = assignment;

    tracing::debug!(task_id=%task_id, worker_id=%worker_id, attempt, "worker starting");
    let _ = supervisor.send(WorkerMsg::Started { task_id, worker_id, attempt });

    let mut beat = tokio::time::interval(lease.heartbeat_interval);
    beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Pin once; poll across loop iterations so progress is never dropped by
    // the heartbeat branch.
    let mut work = Box::pin(executor.execute());

    let outcome = loop {
        tokio::select! {
            _ = beat.tick() => {
                let _ = supervisor.send(WorkerMsg::Heartbeat { task_id, worker_id, attempt });
            }
            done = work.as_mut() => break done,
        }
    };

    match outcome {
        ExecutorOutcome::Completed(report) => {
            let _ = supervisor.send(WorkerMsg::Finished { task_id, worker_id, attempt, report });
        }
        ExecutorOutcome::Crashed(reason) => {
            tracing::warn!(task_id=%task_id, worker_id=%worker_id, attempt, "worker crashed itself");
            let _ = supervisor.send(WorkerMsg::Died { task_id, worker_id, attempt, reason });
        }
    }
}
