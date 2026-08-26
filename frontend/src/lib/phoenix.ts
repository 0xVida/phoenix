// Phoenix CI client: talks to the Rust API (Axum + SSE) when reachable,
// otherwise drives a faithful local simulation so the dashboard is always live.

export type Stage = "planner" | "implementer" | "gate";
export type TaskStatus = "created" | "planning" | "implementing" | "testing" | "merged" | "failed";

export type PhoenixEvent = {
  id: number;
  ts: string;
  name: string;
  detail: string;
  level: "info" | "accent" | "muted";
};

export type TaskSnapshot = {
  task_id: string;
  pr_id: string;
  title: string;
  bug_description: string;
  status: TaskStatus;
  attempt: number;
  max_attempts: number;
  worker_id: string | null;
  sandbox: string | null;
  lease_remaining_ms: number;
  lease_ttl_ms: number;
  heartbeat_seq: number;
  provenance: "real_cargo_test" | "simulated" | null;
  merged: boolean;
};

// Priority: an explicit localStorage override (manual dev/testing escape
// hatch) → VITE_API_BASE (derived by vite.config.ts from the repo-root
// .env's SWARM_BIND — the exact same value scripts/swarm.sh binds the
// backend to) → the hardcoded fallback, matching swarm.sh's own default.
export const API_BASE =
  (typeof window !== "undefined" && window.localStorage.getItem("phoenix_api_base")) ||
  import.meta.env.VITE_API_BASE ||
  "http://localhost:3000";

export const LEASE_TTL_MS = 1500;
export const HEARTBEAT_MS = 500;

export function nowStamp() {
  const d = new Date();
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(
    Math.floor(d.getMilliseconds() / 10),
  )}`;
}

export function newTaskId() {
  return `TX-${Math.floor(100 + Math.random() * 900)}`;
}

export function workerId() {
  return `w-${Math.random().toString(16).slice(2, 7)}`;
}

export async function probeApi(base: string, ms = 900): Promise<boolean> {
  try {
    const ctl = new AbortController();
    const t = setTimeout(() => ctl.abort(), ms);
    const res = await fetch(`${base}/tasks`, { signal: ctl.signal });
    clearTimeout(t);
    return res.ok;
  } catch {
    return false;
  }
}

// ---- Real backend client (crates/api: Axum + SSE) ----------------------

export type TaskSubmission = {
  pr_id: string;
  title: string;
  bug_description: string;
  pr_url?: string;
  repo_url?: string;
  git_ref?: string;
};

/** Backend `TaskStatus` (snake_case, matches swarm-core::task::TaskStatus). */
export type BackendStatus =
  "pending" | "assigned" | "in_progress" | "tests_running" | "passed" | "failed" | "merged";

export type BackendTaskRecord = {
  id: string;
  spec: TaskSubmission;
  status: BackendStatus;
  attempt: number;
  assigned_worker: string | null;
};

/** Dotted SSE event names, matching `event_name()` in crates/api/src/routes.rs. */
export const BACKEND_EVENT_NAMES = [
  "task.created",
  "worker.started",
  "worker.heartbeat",
  "worker.failed",
  "task.reassigned",
  "stale_result.rejected",
  "tests.passed",
  "tests.failed",
  "merge.gated",
  "merge.opened",
  "github.push_ok",
  "github.pr_merged",
  "github.action_failed",
] as const;

export type BackendEventName = (typeof BACKEND_EVENT_NAMES)[number];

/** Loosely-typed payload — shape varies per event, callers narrow by name. */
export type BackendEventData = {
  task_id: string;
  worker_id?: string;
  attempt?: number;
  reason?: string;
  origin?: "real_cargo_test" | "simulated";
  branch?: string;
  url?: string;
  pr_id?: string;
  title?: string;
};

async function apiFetch(path: string, init?: RequestInit) {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers ?? {}) },
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body?.error ?? `${path} → HTTP ${res.status}`);
  }
  return res.json();
}

export async function submitTaskApi(spec: TaskSubmission): Promise<{ task_id: string }> {
  return apiFetch("/tasks", { method: "POST", body: JSON.stringify(spec) });
}

export async function getTaskApi(taskId: string): Promise<BackendTaskRecord> {
  return apiFetch(`/tasks/${taskId}`);
}

export async function killTaskApi(
  taskId: string,
): Promise<{ killed: boolean; attempt?: number; note?: string }> {
  return apiFetch(`/tasks/${taskId}/kill`, { method: "POST" });
}

/**
 * Subscribes to the real SSE stream and dispatches parsed payloads by dotted
 * event name. The stream carries EVERY task's events, not just one — callers
 * filter by `task_id`. EventSource reconnects on its own.
 */
export function openEventStream(
  onEvent: (name: BackendEventName, data: BackendEventData) => void,
): EventSource {
  const es = new EventSource(`${API_BASE}/events`);
  for (const name of BACKEND_EVENT_NAMES) {
    es.addEventListener(name, (ev) => {
      try {
        onEvent(name, JSON.parse((ev as MessageEvent).data));
      } catch {
        // malformed frame — ignore, next event carries on
      }
    });
  }
  return es;
}
