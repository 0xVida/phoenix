//! Production bridge from supervisor assignments to real Tokio workers.
//!
//! Until Phase 3 lands, the "implementer" is a SIMULATED one: it pretends to
//! work for `work_for`, then returns a clearly-labelled SIMULATED pass. The
//! merge gate still treats provenance honestly (dev mode only accepts this
//! because `require_real_cargo_test` is off).
//!
//! This type also owns the DEMO fault-injection registry: join handles per
//! (task, attempt), so the live demo can abort a worker mid-flight via
//! `POST /tasks/:id/kill` and let the supervisor recover on camera.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use swarm_core::events::TestOrigin;
use swarm_core::gate::TestReport;
use swarm_core::ids::{Attempt, TaskId};
use swarm_core::mail::Assignment;
use swarm_supervisor::WorkerSpawner;
use swarm_worker::{run_worker, ExecutorOutcome, TaskExecutor};

/// Stand-in implementer: works in slices so heartbeats interleave realistically.
pub struct SimulatedImplementer {
    pub work_for: Duration,
}

#[async_trait]
impl TaskExecutor for SimulatedImplementer {
    async fn execute(&mut self) -> ExecutorOutcome {
        let mut left = self.work_for;
        while left > Duration::ZERO {
            let step = left.min(Duration::from_millis(250));
            tokio::time::sleep(step).await;
            left -= step;
        }
        ExecutorOutcome::Completed(TestReport {
            passed: true,
            origin: TestOrigin::Simulated,
            summary: format!("simulated implementer finished in {:?}", self.work_for),
        })
    }
}

/// Bridges swarm_worker's `TaskExecutor` port to the Phase 3 implementer
/// agent. Agent errors become loud crashes (supervisor treats them like any
/// worker death — lease/Died handling, reassignment).
pub struct AgentExecutor {
    pub agent: Arc<swarm_agents::ImplementerAgent>,
    pub bug_description: String,
}

#[async_trait]
impl TaskExecutor for AgentExecutor {
    async fn execute(&mut self) -> ExecutorOutcome {
        match self.agent.fix(&self.bug_description).await {
            Ok(report) => ExecutorOutcome::Completed(report),
            Err(e) => ExecutorOutcome::Crashed(format!("implementer agent failed: {e}")),
        }
    }
}

#[derive(Default)]
struct SpawnerState {
    handle: Option<swarm_core::mail::SupervisorHandle>,
    live: HashMap<(TaskId, Attempt), tokio::task::JoinHandle<()>>,
}

/// Builds the `TaskExecutor` for one assignment. Receives the assignment so
/// agent-backed executors can read e.g. `spec.bug_description`.
pub type ExecutorFactory = Arc<dyn Fn(&Assignment) -> Box<dyn TaskExecutor> + Send + Sync>;

#[derive(Clone)]
pub struct DemoSpawner {
    factory: ExecutorFactory,
    state: Arc<Mutex<SpawnerState>>,
}

impl DemoSpawner {
    pub fn new(factory: ExecutorFactory) -> Self {
        Self {
            factory,
            state: Arc::default(),
        }
    }

    /// Must be called once after `Supervisor::new`, before traffic flows.
    pub fn set_handle(&self, handle: swarm_core::mail::SupervisorHandle) {
        self.state.lock().unwrap().handle = Some(handle);
    }

    /// THE DEMO MOMENT, exposed over HTTP: abort the CURRENT worker of a task
    /// mid-flight. Nothing else signals death — the supervisor must notice via
    /// lease expiry and reassign, live.
    pub fn kill_current(&self, task_id: TaskId) -> Option<Attempt> {
        let mut state = self.state.lock().unwrap();
        let current = state
            .live
            .keys()
            .filter(|(t, _)| *t == task_id)
            .map(|(_, a)| *a)
            .max()?;
        let handle = state.live.remove(&(task_id, current))?;
        tracing::warn!(task_id=%task_id, attempt=current, "DEMO KILL: aborting worker");
        handle.abort();
        Some(current)
    }
}

impl WorkerSpawner for DemoSpawner {
    fn spawn_worker(&self, assignment: swarm_core::mail::Assignment) {
        let mut state = self.state.lock().unwrap();
        let supervisor = state
            .handle
            .clone()
            .expect("supervisor handle must be set before serving traffic");
        // Drop bookkeeping for superseded generations of this task.
        state.live.retain(|(t, a), _| {
            !(*t == assignment.task_id && *a < assignment.attempt)
        });
        let executor = (self.factory)(&assignment);
        let join = tokio::spawn(run_worker(assignment.clone(), supervisor, executor));
        state
            .live
            .insert((assignment.task_id, assignment.attempt), join);
    }
}
