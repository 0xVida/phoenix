//! The task state machine — explicit, enum-driven, checked at runtime.
//!
//! States (DEVLOG decision 2026-08-24):
//!   Pending → Assigned → InProgress → TestsRunning → Passed | Failed
//!   Passed  → Merged
//! Reassignment is NOT a persistent status: it is a transition back to
//! `Assigned` with an incremented `attempt` (fencing token) plus a
//! `task.reassigned` event. `Failed` and `Merged` are terminal.

use crate::error::SwarmError;
use crate::ids::{Attempt, TaskId, WorkerId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    /// External identifier of the PR under review.
    pub pr_id: String,
    pub title: String,
    /// Diff summary / bug description the planner will read.
    pub bug_description: String,
    /// Optional GitHub PR link (`…/pull/<n>`) — resolves to cloning
    /// `refs/pull/<n>/head` from that repository.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Optional direct git URL (https/ssh/file). Overrides `pr_url`.
    #[serde(default)]
    pub repo_url: Option<String>,
    /// Optional explicit ref to check out (branch, tag, or full ref like
    /// `refs/pull/N/head`). Defaults to the remote HEAD.
    #[serde(default)]
    pub git_ref: Option<String>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Assigned,
    InProgress,
    TestsRunning,
    Passed,
    Failed,
    Merged,
}

impl TaskStatus {
    /// The ONLY legal moves. Everything else is refused by `TaskRecord::transition`.
    pub fn can_transition_to(self, next: TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, next),
            (Pending, Assigned)
                | (Assigned, InProgress)
                // reassignment before/while working/before tests resolve:
                | (Assigned, Assigned)
                | (Assigned, TestsRunning) // defensive: result raced ahead of Started
                | (InProgress, TestsRunning)
                | (InProgress, Assigned)
                | (TestsRunning, Assigned)
                // resolution:
                | (TestsRunning, Passed)
                | (Assigned, Failed)      // attempts exhausted mid-flight
                | (InProgress, Failed)
                | (TestsRunning, Failed)
                | (Passed, Merged)
        )
    }

    /// States in which some worker generation holds (or must hold) a live lease.
    pub fn holds_lease(self) -> bool {
        matches!(
            self,
            TaskStatus::Assigned | TaskStatus::InProgress | TaskStatus::TestsRunning
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, TaskStatus::Failed | TaskStatus::Merged)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub status: TaskStatus,
    /// Fencing token — incremented on every reassignment.
    pub attempt: Attempt,
    /// Current lease holder, if any. Cleared on terminal transitions.
    pub assigned_worker: Option<WorkerId>,
}

impl TaskRecord {
    pub fn new(id: TaskId, spec: TaskSpec) -> Self {
        Self {
            id,
            spec,
            status: TaskStatus::Pending,
            attempt: 0,
            assigned_worker: None,
        }
    }

    /// Checked transition: refuses illegal moves so callers cannot corrupt the
    /// machine. Terminal states also release the worker slot.
    pub fn transition(&mut self, next: TaskStatus) -> Result<(), SwarmError> {
        if self.status.can_transition_to(next) {
            self.status = next;
            if !next.holds_lease() {
                self.assigned_worker = None;
            }
            Ok(())
        } else {
            Err(SwarmError::IllegalTransition {
                from: self.status,
                to: next,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TaskSpec {
        TaskSpec {
            pr_id: "PR-1".into(),
            title: "t".into(),
            bug_description: "d".into(),
            pr_url: None,
            repo_url: None,
            git_ref: None,
        }
    }

    #[test]
    fn happy_path_is_legal() {
        let mut r = TaskRecord::new(TaskId::generate(), spec());
        for s in [
            TaskStatus::Assigned,
            TaskStatus::InProgress,
            TaskStatus::TestsRunning,
            TaskStatus::Passed,
            TaskStatus::Merged,
        ] {
            r.transition(s).expect("legal transition");
        }
        assert_eq!(r.status, TaskStatus::Merged);
        assert!(r.assigned_worker.is_none());
    }

    #[test]
    fn pending_cannot_skip_straight_to_merged() {
        let mut r = TaskRecord::new(TaskId::generate(), spec());
        assert!(matches!(
            r.transition(TaskStatus::Merged),
            Err(SwarmError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn reassignment_edges_exist_from_every_lease_holding_state() {
        for from in [TaskStatus::Assigned, TaskStatus::InProgress, TaskStatus::TestsRunning] {
            assert!(from.can_transition_to(TaskStatus::Assigned), "{from:?} -> Assigned");
            assert!(from.can_transition_to(TaskStatus::Failed), "{from:?} -> Failed");
        }
    }

    #[test]
    fn terminal_states_have_no_exits() {
        for from in [TaskStatus::Failed, TaskStatus::Merged] {
            for to in [
                TaskStatus::Pending,
                TaskStatus::Assigned,
                TaskStatus::InProgress,
                TaskStatus::TestsRunning,
                TaskStatus::Passed,
                TaskStatus::Merged,
                TaskStatus::Failed,
            ] {
                assert!(!from.can_transition_to(to), "{from:?} -> {to:?} must be illegal");
            }
        }
    }
}
