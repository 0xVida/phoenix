//! THE critical test of the project (BUILD_PROMPT.md §5 Phase 1):
//!
//! 1. start a task,
//! 2. KILL the worker mid-task (`tokio::task::abort` — heartbeats just stop),
//! 3. assert the Supervisor notices via lease expiry and REASSIGNS,
//! 4. assert the fresh worker completes and the merge gate OPENS,
//! 5. assert the zombie's late messages are FENCED OUT without side effects.
//!
//! Time is virtual (`start_paused = true`) so lease expiry is deterministic;
//! the wait helpers use real time only as a hang watchdog.
//!
//! The whole demo hinges on this file passing reliably.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swarm_core::events::{SwarmEvent, TestOrigin};
use swarm_core::gate::{MergePolicy, TestReport};
use swarm_core::ids::{Attempt, TaskId, WorkerId};
use swarm_core::lease::LeaseConfig;
use swarm_core::mail::{Assignment, SupervisorHandle, WorkerMsg};
use swarm_core::task::{TaskSpec, TaskStatus};
use swarm_supervisor::{Supervisor, SupervisorConfig, WorkerSpawner};
use swarm_worker::{run_worker, ExecutorOutcome, TaskExecutor};

/// Real-time watchdog: if this fires, the machinery is wedged for real.
const WAIT_BUDGET: Duration = Duration::from_secs(30);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// ---------- scripted executors ----------

/// Simulates a worker wedged mid-task; ONLY an external kill ends it.
struct HangForever;

#[async_trait::async_trait]
impl TaskExecutor for HangForever {
    async fn execute(&mut self) -> ExecutorOutcome {
        std::future::pending::<()>().await;
        unreachable!("std::future::pending never resolves");
    }
}

/// Simulates a fast implementer that finishes with a test report.
struct PassFast(TestOrigin);

#[async_trait::async_trait]
impl TaskExecutor for PassFast {
    async fn execute(&mut self) -> ExecutorOutcome {
        tokio::time::sleep(ms(10)).await;
        ExecutorOutcome::Completed(TestReport {
            passed: true,
            origin: self.0,
            summary: "simulated pass".into(),
        })
    }
}

/// Simulates a worker that notices its own death (crashed tool/sandbox) and
/// reports it cleanly — exercises the supervisor's `Died` fast path.
struct CrashFast;

#[async_trait::async_trait]
impl TaskExecutor for CrashFast {
    async fn execute(&mut self) -> ExecutorOutcome {
        tokio::time::sleep(ms(20)).await;
        ExecutorOutcome::Crashed("sandbox toolchain exploded".into())
    }
}

// ---------- scripted spawner (stands in for the dispatcher) ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Attempt 1 hangs forever (the test kills it); later attempts pass.
    HangFirstOnly,
    /// Every attempt crashes loudly (Died fast path) — exercises
    /// attempts-exhausted gating without any external kill.
    HangAll,
}

#[derive(Default)]
struct SpawnerState {
    handle: Option<SupervisorHandle>,
    spawned: Vec<(TaskId, Attempt, WorkerId, tokio::task::JoinHandle<()>)>,
}

#[derive(Clone)]
struct ScriptSpawner {
    mode: Mode,
    state: Arc<Mutex<SpawnerState>>,
}

impl Default for ScriptSpawner {
    fn default() -> Self {
        Self {
            mode: Mode::HangFirstOnly,
            state: Arc::default(),
        }
    }
}

impl ScriptSpawner {
    fn hang_all() -> Self {
        Self {
            mode: Mode::HangAll,
            ..Default::default()
        }
    }

    fn set_handle(&self, handle: SupervisorHandle) {
        self.state.lock().unwrap().handle = Some(handle);
    }

    /// THE DEMO MOMENT: kill one attempt's worker mid-flight.
    fn kill_attempt(&self, task_id: TaskId, attempt: Attempt) -> Option<WorkerId> {
        let mut state = self.state.lock().unwrap();
        state.spawned.iter_mut().find_map(|(t, a, w, jh)| {
            if *t == task_id && *a == attempt && !jh.is_finished() {
                jh.abort();
                Some(*w)
            } else {
                None
            }
        })
    }
}

impl WorkerSpawner for ScriptSpawner {
    fn spawn_worker(&self, assignment: Assignment) {
        let supervisor = self
            .state
            .lock()
            .unwrap()
            .handle
            .clone()
            .expect("supervisor handle must be set before any spawn");
        let executor: Box<dyn TaskExecutor> = match self.mode {
            Mode::HangFirstOnly if assignment.attempt == 1 => Box::new(HangForever),
            Mode::HangAll => Box::new(CrashFast),
            _ => Box::new(PassFast(TestOrigin::Simulated)),
        };
        let join = tokio::spawn(run_worker(assignment.clone(), supervisor, executor));
        self.state.lock().unwrap().spawned.push((
            assignment.task_id,
            assignment.attempt,
            assignment.worker_id,
            join,
        ));
    }
}

// ---------- event capture + predicates ----------

type Sink = Arc<Mutex<Vec<SwarmEvent>>>;

async fn collect_events(mut rx: tokio::sync::broadcast::Receiver<SwarmEvent>, sink: Sink) {
    loop {
        match rx.recv().await {
            Ok(event) => sink.lock().unwrap().push(event),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Bounded wait on a predicate over the captured event stream.
async fn wait_for(sink: &Sink, pred: impl Fn(&SwarmEvent) -> bool) -> bool {
    let deadline = std::time::Instant::now() + WAIT_BUDGET;
    loop {
        if sink.lock().unwrap().iter().any(|e| pred(e)) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(ms(5)).await;
    }
}

fn count(v: &[SwarmEvent], pred: &dyn Fn(&SwarmEvent) -> bool) -> usize {
    v.iter().filter(|e| pred(e)).count()
}

fn started(a: Attempt) -> impl Fn(&SwarmEvent) -> bool {
    move |e| matches!(e, SwarmEvent::WorkerStarted { attempt, .. } if *attempt == a)
}
fn failed(a: Attempt) -> impl Fn(&SwarmEvent) -> bool {
    move |e| matches!(e, SwarmEvent::WorkerFailed { attempt, .. } if *attempt == a)
}
fn reassigned_to(a: Attempt) -> impl Fn(&SwarmEvent) -> bool {
    move |e| matches!(e, SwarmEvent::TaskReassigned { attempt, .. } if *attempt == a)
}
fn stale_from(a: Attempt) -> impl Fn(&SwarmEvent) -> bool {
    move |e| matches!(e, SwarmEvent::StaleResultRejected { attempt, .. } if *attempt == a)
}
fn merge_opened(e: &SwarmEvent) -> bool {
    matches!(e, SwarmEvent::MergeOpened { .. })
}
fn merge_gated(e: &SwarmEvent) -> bool {
    matches!(e, SwarmEvent::MergeGated { .. })
}

// ---------- harness ----------

struct Harness {
    supervisor: Supervisor,
    spawner: ScriptSpawner,
    sink: Sink,
}

fn harness(config: SupervisorConfig, spawner: ScriptSpawner) -> Harness {
    let supervisor = Supervisor::new(config, Box::new(spawner.clone())).expect("valid config");
    spawner.set_handle(supervisor.handle());
    let sink: Sink = Arc::default();
    tokio::spawn(collect_events(supervisor.subscribe(), sink.clone()));
    let runner = supervisor.clone();
    tokio::spawn(async move {
        runner.run().await.expect("supervisor loop must not fail");
    });
    Harness {
        supervisor,
        spawner,
        sink,
    }
}

fn spec() -> TaskSpec {
    TaskSpec {
        pr_id: "PR-42".into(),
        title: "Fix off-by-one in ledger sum".into(),
        bug_description: "ledger sum skips the final element".into(),
    }
}

/// Fast lease timings. Phase 1 dev mode: simulated reports may open the gate
/// (real-cargo-test enforcement is proven by core unit tests and lands fully
/// in Phase 3).
fn fast_config(max_attempts: u32) -> SupervisorConfig {
    SupervisorConfig {
        lease: LeaseConfig {
            heartbeat_interval: ms(100),
            lease_timeout: ms(300),
        },
        reap_interval: ms(50),
        max_attempts,
        merge_policy: MergePolicy {
            require_real_cargo_test: false,
        },
    }
}

/// THE test (BUILD_PROMPT.md §5 Phase 1): kill worker #1 mid-task → the
/// supervisor must notice via lease expiry, reassign, and the task must still
/// complete with the merge gate opening.
#[tokio::test(start_paused = true)]
async fn killed_worker_is_reassigned_and_task_completes() {
    let h = harness(fast_config(3), ScriptSpawner::default());

    let task_id = h.supervisor.submit_pr(spec());

    // Worker #1 picks the task up.
    assert!(
        wait_for(&h.sink, started(1)).await,
        "worker #1 never started"
    );

    // THE DEMO MOMENT: kill it mid-task. Heartbeats simply stop; nothing else
    // signals death — recovery must come from lease expiry alone.
    let zombie = h
        .spawner
        .kill_attempt(task_id, 1)
        .expect("worker #1 was running");

    assert!(
        wait_for(&h.sink, failed(1)).await,
        "worker #1 death never observed via lease expiry"
    );
    assert!(wait_for(&h.sink, reassigned_to(2)).await, "no reassignment to attempt 2");
    assert!(wait_for(&h.sink, started(2)).await, "fresh worker #2 never started");

    // Fresh worker finishes; deterministic gate opens on its report.
    assert!(wait_for(&h.sink, merge_opened).await, "merge gate never opened");

    // Deterministic end state.
    let record = h.supervisor.snapshot(task_id).expect("task present");
    assert_eq!(record.status, TaskStatus::Merged);
    assert_eq!(record.attempt, 2);
    assert!(record.assigned_worker.is_none());

    // Ordering proof: the full recovery story happened IN ORDER.
    let events = h.sink.lock().unwrap().clone();
    let find = |pred: &dyn Fn(&SwarmEvent) -> bool| {
        events
            .iter()
            .position(pred)
            .expect("milestone event present")
    };
    let i_start1 = find(&started(1));
    let i_fail1 = find(&failed(1));
    let i_reassign = find(&reassigned_to(2));
    let i_start2 = find(&started(2));
    let i_pass = find(&|e| matches!(e, SwarmEvent::TestsPassed { .. }));
    let i_open = find(&merge_opened);
    assert!(i_start1 < i_fail1, "started#1 must precede failed#1");
    assert!(i_fail1 < i_reassign, "failed#1 must precede reassignment");
    assert!(i_reassign < i_start2, "reassignment must precede started#2");
    assert!(i_start2 < i_pass, "started#2 must precede tests.passed");
    assert!(i_pass < i_open, "tests.passed must precede merge.opened");

    // FENCING: zombie #1's late heartbeat must be rejected loudly…
    h.supervisor
        .handle()
        .send(WorkerMsg::Heartbeat { task_id, worker_id: zombie, attempt: 1 })
        .expect("send heartbeat");
    assert!(
        wait_for(&h.sink, stale_from(1)).await,
        "stale heartbeat was not rejected"
    );

    // …and its late "result" (even claiming a REAL cargo pass) must be
    // rejected too — the exact scenario behind the Phase 4 UX line
    // "Old agent result rejected — this task had already been reassigned".
    let stale_before = count(&events, &stale_from(1));
    h.supervisor
        .handle()
        .send(WorkerMsg::Finished {
            task_id,
            worker_id: zombie,
            attempt: 1,
            report: TestReport {
                passed: true,
                origin: TestOrigin::RealCargoTest,
                summary: "zombie claims victory".into(),
            },
        })
        .expect("send finished");
    let deadline = std::time::Instant::now() + WAIT_BUDGET;
    loop {
        let n = count(&h.sink.lock().unwrap(), &stale_from(1));
        if n > stale_before {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "zombie's late result was not rejected"
        );
        tokio::time::sleep(ms(5)).await;
    }

    // …leaving the machine UNMOVED: still merged, exactly one of each.
    let record = h.supervisor.snapshot(task_id).unwrap();
    assert_eq!(record.status, TaskStatus::Merged);
    let events_now = h.sink.lock().unwrap().clone();
    assert_eq!(count(&events_now, &merge_opened), 1, "gate must open exactly once");
    assert_eq!(count(&events_now, &reassigned_to(2)), 1);
    assert_eq!(count(&events_now, &started(2)), 1);
}

/// Every worker crashes loudly → attempts exhaust → terminal, VISIBLE gate
/// block (covers the supervisor's `Died` fast path end to end).
#[tokio::test(start_paused = true)]
async fn task_fails_closed_after_max_attempts_when_all_workers_die() {
    let h = harness(fast_config(2), ScriptSpawner::hang_all());

    let task_id = h.supervisor.submit_pr(spec());

    assert!(
        wait_for(&h.sink, merge_gated).await,
        "task should terminate with a visible gate block"
    );

    let record = h.supervisor.snapshot(task_id).expect("task present");
    assert_eq!(record.status, TaskStatus::Failed);
    assert_eq!(record.attempt, 2); // both attempts really ran

    let events = h.sink.lock().unwrap().clone();
    assert!(
        events.iter().any(|e| failed(1)(e)),
        "attempt 1 failure unobserved"
    );
    assert!(
        events.iter().any(|e| failed(2)(e)),
        "attempt 2 failure unobserved"
    );
    assert!(events.iter().any(|e| reassigned_to(2)(e)));
    assert_eq!(count(&events, &merge_gated), 1);
    assert!(
        !events.iter().any(merge_opened),
        "a failed task must never open the gate"
    );
}


