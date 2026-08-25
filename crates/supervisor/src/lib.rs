//! The supervisor: deterministic brain of Swarm CI.
//!
//! Single-writer principle (DEVLOG 2026-08-24): this loop is the ONLY code
//! that mutates task state or emits [`SwarmEvent`]s. Workers talk to it
//! exclusively via [`WorkerMsg`]s, every message fenced by its `attempt`
//! token, so a zombie worker from before a reassignment can never corrupt the
//! machine — its messages are visibly rejected (`StaleResultRejected`).
//!
//! Recovery model: a worker death (abort, panic, process kill) is not signaled
//! by any special protocol; it manifests as MISSED HEARTBEATS. A reaper scan
//! expires the lease, emits `worker.failed`, and reassigns to a fresh worker
//! generation until `max_attempts` is exhausted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;

use swarm_core::error::{Result, SwarmError};
use swarm_core::events::{EventBus, SwarmEvent};
use swarm_core::gate::{evaluate_merge_gate, MergeDecision, MergePolicy, TestReport};
use swarm_core::ids::{Attempt, TaskId, WorkerId};
use swarm_core::lease::LeaseConfig;
use swarm_core::mail::{Assignment, SupervisorHandle, WorkerMsg};
use swarm_core::task::{TaskRecord, TaskSpec, TaskStatus};

/// Broadcast buffer for event subscribers (SSE later). Generous enough to
/// absorb heartbeat bursts without lagging slow UI consumers.
pub const DEFAULT_EVENT_BUFFER: usize = 1024;

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub lease: LeaseConfig,
    pub reap_interval: Duration,
    pub max_attempts: u32,
    pub merge_policy: MergePolicy,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            lease: LeaseConfig::default(),
            reap_interval: Duration::from_millis(250),
            max_attempts: 3,
            merge_policy: MergePolicy::default(),
        }
    }
}

/// Bridge to fresh worker generations. MUST NOT block: implementations should
/// `tokio::spawn` internally and return immediately.
pub trait WorkerSpawner: Send + Sync + 'static {
    fn spawn_worker(&self, assignment: Assignment);
}

struct LiveTask {
    record: TaskRecord,
    /// Tokio clock on purpose: respects paused time in tests.
    lease_until: Instant,
}

struct Inner {
    config: SupervisorConfig,
    events: EventBus,
    spawner: Box<dyn WorkerSpawner>,
    state: Mutex<HashMap<TaskId, LiveTask>>,
}

/// Cheap clone-able front door. Clones share the same supervisor.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
    handle: SupervisorHandle,
    rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<WorkerMsg>>>>,
}

impl Supervisor {
    pub fn new(config: SupervisorConfig, spawner: Box<dyn WorkerSpawner>) -> Result<Self> {
        config.lease.validate()?;
        let events = EventBus::new(DEFAULT_EVENT_BUFFER);
        let (tx, rx) = mpsc::unbounded_channel();
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                events,
                spawner,
                state: Mutex::new(HashMap::new()),
            }),
            handle: SupervisorHandle::new(tx),
            rx: Arc::new(Mutex::new(Some(rx))),
        })
    }

    /// Sender side for workers/tests. Survives `run()` consuming a clone.
    pub fn handle(&self) -> SupervisorHandle {
        self.handle.clone()
    }

    pub fn events(&self) -> &EventBus {
        &self.inner.events
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SwarmEvent> {
        self.inner.events.subscribe()
    }

    /// Deterministic snapshot for APIs/tests/UI state queries.
    pub fn snapshot(&self, task_id: TaskId) -> Option<TaskRecord> {
        self.inner
            .state
            .lock()
            .unwrap()
            .get(&task_id)
            .map(|live| live.record.clone())
    }

    /// Entry point: register a PR-review task and hand it to attempt #1.
    pub fn submit_pr(&self, spec: TaskSpec) -> TaskId {
        let id = TaskId::generate();
        let record = TaskRecord::new(id, spec.clone());
        self.inner.events.emit(SwarmEvent::TaskCreated {
            task_id: id,
            pr_id: spec.pr_id.clone(),
            title: spec.title.clone(),
        });
        // Pending tasks hold no meaningful lease until assigned below.
        self.inner.state.lock().unwrap().insert(
            id,
            LiveTask {
                record,
                lease_until: Instant::now(),
            },
        );
        if self.assign(id).is_none() {
            tracing::error!(task_id=%id, "initial assignment failed");
        }
        id
    }

    /// Give `task_id` its next attempt (fresh worker generation + lease).
    fn assign(&self, task_id: TaskId) -> Option<()> {
        let worker_id = WorkerId::generate();
        let assignment = {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.get_mut(&task_id)?;
            let next_attempt = live.record.attempt.checked_add(1)?;
            if next_attempt > self.inner.config.max_attempts {
                return None;
            }
            live.record.transition(TaskStatus::Assigned).ok()?;
            live.record.attempt = next_attempt;
            live.record.assigned_worker = Some(worker_id);
            live.lease_until = Instant::now() + self.inner.config.lease.lease_timeout;
            Assignment {
                task_id,
                worker_id,
                attempt: next_attempt,
                spec: live.record.spec.clone(),
                lease: self.inner.config.lease,
            }
        };
        tracing::info!(
            task_id=%task_id, worker_id=%assignment.worker_id, attempt=assignment.attempt,
            "worker assigned"
        );
        self.inner.spawner.spawn_worker(assignment);
        Some(())
    }
}

impl Supervisor {
    /// Consume the supervisor forever: message handling + lease reaping in one
    /// select loop. Run exactly once, e.g. `tokio::spawn(sup.clone().run())`.
    pub async fn run(self) -> Result<()> {
        let mut rx = self
            .rx
            .lock()
            .unwrap()
            .take()
            .ok_or(SwarmError::ChannelClosed)?;
        let mut reap = tokio::time::interval(self.inner.config.reap_interval);
        reap.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tracing::info!("supervisor loop running");
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(m) => {
                            // Spec §4: one span per task — every message the
                            // supervisor processes is traced with ids attached.
                            let (kind, task_id, worker_id, attempt) = msg_meta(&m);
                            let _guard = tracing::info_span!(
                                "supervisor_msg",
                                kind,
                                task_id = %task_id,
                                worker_id = %worker_id,
                                attempt = attempt,
                            )
                            .entered();
                            self.on_msg(m);
                        }
                        None => return Err(SwarmError::ChannelClosed),
                    }
                }
                _ = reap.tick() => self.reap_expired(),
            }
        }
    }

    fn on_msg(&self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Started { task_id, worker_id, attempt } => {
                if !self.is_current_holder(task_id, worker_id, attempt) {
                    return;
                }
                if let Some(live) = self.inner.state.lock().unwrap().get_mut(&task_id) {
                    let _ = live.record.transition(TaskStatus::InProgress);
                    live.lease_until = Instant::now() + self.inner.config.lease.lease_timeout;
                }
                self.inner.events.emit(SwarmEvent::WorkerStarted { task_id, worker_id, attempt });
            }
            WorkerMsg::Heartbeat { task_id, worker_id, attempt } => {
                if !self.is_current_holder(task_id, worker_id, attempt) {
                    return;
                }
                if let Some(live) = self.inner.state.lock().unwrap().get_mut(&task_id) {
                    live.lease_until = Instant::now() + self.inner.config.lease.lease_timeout;
                }
                self.inner.events.emit(SwarmEvent::WorkerHeartbeat { task_id, worker_id, attempt });
            }
            WorkerMsg::Finished { task_id, worker_id, attempt, report } => {
                if !self.is_current_holder(task_id, worker_id, attempt) {
                    return;
                }
                self.resolve_report(task_id, report);
            }
            WorkerMsg::Died { task_id, worker_id, attempt, reason } => {
                if !self.is_current_holder(task_id, worker_id, attempt) {
                    return;
                }
                self.inner.events.emit(SwarmEvent::WorkerFailed {
                    task_id,
                    worker_id,
                    attempt,
                    reason: reason.clone(),
                });
                self.retry_or_fail(task_id, &reason);
            }
        }
    }
}

impl Supervisor {
    /// Fencing check: is `(worker_id, attempt)` still the CURRENT holder of
    /// this task's lease? If not — zombie/late message — emit
    /// `StaleResultRejected` and refuse. This is what makes kill-and-reassign
    /// safe against old workers waking back up.
    fn is_current_holder(&self, task_id: TaskId, worker_id: WorkerId, attempt: Attempt) -> bool {
        let current = {
            let state = self.inner.state.lock().unwrap();
            match state.get(&task_id) {
                Some(live) => {
                    live.record.status.holds_lease()
                        && live.record.attempt == attempt
                        && live.record.assigned_worker == Some(worker_id)
                }
                None => false,
            }
        };
        if current {
            true
        } else {
            tracing::warn!(
                task_id=%task_id, worker_id=%worker_id, attempt,
                "stale worker message rejected"
            );
            self.inner.events.emit(SwarmEvent::StaleResultRejected {
                task_id,
                worker_id,
                attempt,
            });
            false
        }
    }

    /// Deterministic gate application: transition through TestsRunning, then
    /// resolve per the PURE `evaluate_merge_gate` decision.
    fn resolve_report(&self, task_id: TaskId, report: TestReport) {
        let decision = evaluate_merge_gate(&report, self.inner.config.merge_policy);
        let mut state = self.inner.state.lock().unwrap();
        let Some(live) = state.get_mut(&task_id) else {
            return;
        };
        if live.record.transition(TaskStatus::TestsRunning).is_err() {
            return;
        }
        match decision {
            MergeDecision::Open { origin } => {
                let _ = live.record.transition(TaskStatus::Passed);
                self.inner.events.emit(SwarmEvent::TestsPassed { task_id, origin });
                if live.record.transition(TaskStatus::Merged).is_ok() {
                    drop(state);
                    self.inner.events.emit(SwarmEvent::MergeOpened { task_id });
                }
            }
            MergeDecision::Gated { reason } => {
                if !report.passed {
                    self.inner.events.emit(SwarmEvent::TestsFailed {
                        task_id,
                        reason: report.summary.clone(),
                    });
                }
                let _ = live.record.transition(TaskStatus::Failed);
                drop(state);
                self.inner.events.emit(SwarmEvent::MergeGated { task_id, reason });
            }
        }
    }
}

impl Supervisor {
    /// After a confirmed worker failure: reassign, or fail terminally when out
    /// of attempts. The task is NEVER silently lost.
    fn retry_or_fail(&self, task_id: TaskId, why: &str) {
        let next_attempt = {
            let mut state = self.inner.state.lock().unwrap();
            let Some(live) = state.get_mut(&task_id) else {
                return;
            };
            let next = live.record.attempt + 1;
            if next > self.inner.config.max_attempts {
                let _ = live.record.transition(TaskStatus::Failed);
                None
            } else {
                Some(next)
            }
        };
        match next_attempt {
            None => {
                let reason = format!(
                    "max attempts ({}) exhausted: {}",
                    self.inner.config.max_attempts, why
                );
                tracing::warn!(task_id=%task_id, "{reason}");
                self.inner.events.emit(SwarmEvent::MergeGated { task_id, reason });
            }
            Some(next) => {
                self.inner
                    .events
                    .emit(SwarmEvent::TaskReassigned { task_id, attempt: next });
                if self.assign(task_id).is_none() {
                    tracing::error!(task_id=%task_id, "reassignment failed unexpectedly");
                }
            }
        }
    }

    /// Lease reaper: any lease-holding task whose deadline has passed had its
    /// worker die (abort/panic/process kill) — emit and recover.
    fn reap_expired(&self) {
        let now = Instant::now();
        let expired: Vec<(TaskId, WorkerId, Attempt)> = {
            let state = self.inner.state.lock().unwrap();
            state
                .iter()
                .filter(|(_, live)| live.record.status.holds_lease() && now >= live.lease_until)
                .map(|(id, live)| {
                    (
                        *id,
                        live.record.assigned_worker.unwrap_or_else(WorkerId::nil),
                        live.record.attempt,
                    )
                })
                .collect()
        };
        for (task_id, worker_id, attempt) in expired {
            let reason = "lease expired: no heartbeat within timeout".to_string();
            tracing::warn!(task_id=%task_id, worker_id=%worker_id, attempt, "worker presumed dead");
            self.inner.events.emit(SwarmEvent::WorkerFailed {
                task_id,
                worker_id,
                attempt,
                reason: reason.clone(),
            });
            self.retry_or_fail(task_id, &reason);
        }
    }
}

/// Spec §4 span helper: pull the ids off any worker message so the supervisor
/// loop can attach them to a per-message tracing span.
fn msg_meta(msg: &WorkerMsg) -> (&'static str, TaskId, WorkerId, Attempt) {
    match msg {
        WorkerMsg::Started { task_id, worker_id, attempt } => {
            ("started", *task_id, *worker_id, *attempt)
        }
        WorkerMsg::Heartbeat { task_id, worker_id, attempt } => {
            ("heartbeat", *task_id, *worker_id, *attempt)
        }
        WorkerMsg::Finished { task_id, worker_id, attempt, .. } => {
            ("finished", *task_id, *worker_id, *attempt)
        }
        WorkerMsg::Died { task_id, worker_id, attempt, .. } => {
            ("died", *task_id, *worker_id, *attempt)
        }
    }
}



