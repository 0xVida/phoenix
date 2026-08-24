//! Shared handler state.

use std::sync::{Arc, Mutex};

use swarm_core::ids::TaskId;
use swarm_supervisor::Supervisor;

use crate::spawner::DemoSpawner;

#[derive(Clone)]
pub struct AppState {
    pub supervisor: Supervisor,
    /// Registry of submitted task ids (MVP listing; a query API on the
    /// supervisor can replace this post-hackathon).
    pub tasks: Arc<Mutex<Vec<TaskId>>>,
    /// Demo fault-injection registry (join handles per worker generation).
    pub spawner: Arc<DemoSpawner>,
}
