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

export const API_BASE =
  (typeof window !== "undefined" && window.localStorage.getItem("phoenix_api_base")) ||
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
