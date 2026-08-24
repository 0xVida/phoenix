//! Typed errors. Every failure mode a caller can react to is its own variant —
//! no string soup (BUILD_PROMPT.md §5 Phase 1).

use crate::ids::{Attempt, TaskId, WorkerId};
use crate::task::TaskStatus;

#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("task {0} not found")]
    TaskNotFound(TaskId),

    #[error("illegal state transition from {from:?} to {to:?}")]
    IllegalTransition { from: TaskStatus, to: TaskStatus },

    #[error(
        "old agent result rejected — task was already reassigned \
         (message from attempt {from_attempt}, current attempt {current_attempt})"
    )]
    StaleWorkerMessage {
        task_id: TaskId,
        worker_id: WorkerId,
        from_attempt: Attempt,
        current_attempt: Attempt,
    },

    #[error("attempts exhausted for task {0}")]
    AttemptsExhausted(TaskId),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("internal channel closed")]
    ChannelClosed,
}

pub type Result<T> = std::result::Result<T, SwarmError>;
