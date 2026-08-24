//! Route handlers. Every response is derived from supervisor state — the API
//! never invents status on its own.

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use swarm_core::events::SwarmEvent;
use swarm_core::ids::TaskId;
use swarm_core::task::{TaskRecord, TaskSpec};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/tasks", post(submit_task).get(list_tasks))
        .route("/tasks/:id", get(get_task))
        .route("/tasks/:id/kill", post(kill_task))
        .route("/events", get(events))
        .with_state(state)
}

async fn submit_task(State(st): State<AppState>, Json(spec): Json<TaskSpec>) -> Json<serde_json::Value> {
    let id = st.supervisor.submit_pr(spec);
    st.tasks.lock().unwrap().push(id);
    tracing::info!(task_id=%id, "task submitted via API");
    Json(serde_json::json!({ "task_id": id.to_string() }))
}

async fn list_tasks(State(st): State<AppState>) -> Json<serde_json::Value> {
    let ids = st.tasks.lock().unwrap().clone();
    let tasks = ids
        .into_iter()
        .filter_map(|id| {
            let rec = st.supervisor.snapshot(id)?;
            Some(serde_json::json!({
                "task_id": id.to_string(),
                "pr_id": rec.spec.pr_id,
                "title": rec.spec.title,
                "status": rec.status,
                "attempt": rec.attempt,
            }))
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({ "tasks": tasks }))
}

fn parse_id(id: &str) -> Result<TaskId, (StatusCode, Json<serde_json::Value>)> {
    TaskId::parse(id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "That task id is not a valid identifier.",
            })),
        )
    })
}

async fn get_task(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskRecord>, (StatusCode, Json<serde_json::Value>)> {
    let task_id = parse_id(&id)?;
    match st.supervisor.snapshot(task_id) {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No task with that id has been submitted.",
            })),
        )),
    }
}

/// Demo fault injection: kill the current worker; the supervisor's lease
/// expiry does the visible recovery.
async fn kill_task(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let task_id = parse_id(&id)?;
    if st.supervisor.snapshot(task_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No task with that id has been submitted." })),
        ));
    }
    match st.spawner.kill_current(task_id) {
        Some(attempt) => Ok(Json(serde_json::json!({
            "killed": true,
            "attempt": attempt,
            "note": "worker aborted; expect worker.failed + task.reassigned within the lease timeout",
        }))),
        None => Ok(Json(serde_json::json!({
            "killed": false,
            "note": "no live worker generation found for this task",
        }))),
    }
}

/// Spec-style dotted event names for SSE `event:` lines.
fn event_name(ev: &SwarmEvent) -> &'static str {
    match ev {
        SwarmEvent::TaskCreated { .. } => "task.created",
        SwarmEvent::WorkerStarted { .. } => "worker.started",
        SwarmEvent::WorkerHeartbeat { .. } => "worker.heartbeat",
        SwarmEvent::WorkerFailed { .. } => "worker.failed",
        SwarmEvent::TaskReassigned { .. } => "task.reassigned",
        SwarmEvent::StaleResultRejected { .. } => "stale_result.rejected",
        SwarmEvent::TestsPassed { .. } => "tests.passed",
        SwarmEvent::TestsFailed { .. } => "tests.failed",
        SwarmEvent::MergeGated { .. } => "merge.gated",
        SwarmEvent::MergeOpened { .. } => "merge.opened",
    }
}

async fn events(State(st): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = st.supervisor.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| {
        match item {
            Ok(ev) => {
                let name = event_name(&ev);
                match serde_json::to_string(&ev) {
                    Ok(data) => Some(Ok::<_, Infallible>(Event::default().event(name).data(data))),
                    Err(e) => {
                        tracing::error!(error = ?e, "failed to serialize event for SSE");
                        None
                    }
                }
            }
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "sse subscriber lagged; skipping missed events");
                None
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// Keep IntoResponse import used even if handlers evolve (compile-time canary).
#[allow(unused)]
fn _assert_into_response<T: IntoResponse>(_: T) {}
