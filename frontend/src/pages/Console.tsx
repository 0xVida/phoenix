import { useCallback, useEffect, useRef, useState } from "react";
import {
  HEARTBEAT_MS,
  LEASE_TTL_MS,
  nowStamp,
  newTaskId,
  workerId,
  type PhoenixEvent,
  type TaskSnapshot,
} from "@/lib/phoenix";

const MAX_ATTEMPTS = 3;
const TICK = 100;

type Phase = "idle" | "planning" | "implementing" | "testing" | "merged" | "failed";

export default function Console() {
  const [task, setTask] = useState<TaskSnapshot | null>(null);
  const [events, setEvents] = useState<PhoenixEvent[]>([]);
  const [pulses, setPulses] = useState<number[]>(() => Array(28).fill(0));
  const [verdicts, setVerdicts] = useState<{ pr_id: string; merged: boolean; attempt: number }[]>(
    [],
  );
  const [prId, setPrId] = useState("");
  const [prLink, setPrLink] = useState("");
  const [bug, setBug] = useState("");
  const [uptime, setUptime] = useState(0);

  const taskRef = useRef<TaskSnapshot | null>(null);
  taskRef.current = task;

  const evId = useRef(0);
  const phase = useRef<Phase>("idle");
  const alive = useRef(false); // worker heartbeating?
  const lease = useRef(LEASE_TTL_MS);
  const phaseLeft = useRef(0);
  const beatLeft = useRef(HEARTBEAT_MS);
  const seq = useRef(0);

  const log = useCallback((name: string, detail: string, level: PhoenixEvent["level"] = "info") => {
    evId.current += 1;
    setEvents((prev) =>
      [...prev, { id: evId.current, ts: nowStamp(), name, detail, level }].slice(-120),
    );
  }, []);

  const spawnWorker = useCallback(
    (t: TaskSnapshot) => {
      const w = workerId();
      const sandbox = `/tmp/swarm-sandbox/${t.task_id.toLowerCase()}-${t.attempt}`;
      alive.current = true;
      lease.current = LEASE_TTL_MS;
      beatLeft.current = HEARTBEAT_MS;
      seq.current = 0;
      phase.current = "implementing";
      phaseLeft.current = 6000;
      log("worker.spawned", `id=${w} sandbox=${sandbox}`, "accent");
      log("plan.executing", `stage="apply_diff" attempt=${t.attempt}`);
      return { ...t, worker_id: w, sandbox, status: "implementing" as const };
    },
    [log],
  );

  const submit = useCallback(() => {
    const id = newTaskId();
    const t: TaskSnapshot = {
      task_id: id,
      pr_id: prId.trim() || "PR-102",
      title: bug.trim().slice(0, 60) || "FX conversion loses cents",
      bug_description: bug.trim() || "convert() must apply rate_bps once and preserve cents.",
      status: "planning",
      attempt: 1,
      max_attempts: MAX_ATTEMPTS,
      worker_id: null,
      sandbox: null,
      lease_remaining_ms: LEASE_TTL_MS,
      lease_ttl_ms: LEASE_TTL_MS,
      heartbeat_seq: 0,
      provenance: null,
      merged: false,
    };
    setEvents([]);
    evId.current = 0;
    setPulses(Array(28).fill(0));
    alive.current = false;
    phase.current = "planning";
    phaseLeft.current = 2200;
    setTask(t);
    log("task.created", `pr_id=${t.pr_id} task_id=${t.task_id}`);
    if (prLink.trim()) log("pr.linked", `url=${prLink.trim()}`, "muted");
    log("planner.dispatch", "model=gpt-oss-120b reading diff…", "muted");
  }, [bug, prId, prLink, log]);

  const kill = useCallback(() => {
    if (!task || !alive.current) return;
    alive.current = false;
    log("worker.killed", `id=${task.worker_id} signal=SIGKILL (fault injection)`, "accent");
    log("heartbeat.lost", "supervisor watching lease…", "accent");
  }, [task, log]);

  // Deterministic clock
  useEffect(() => {
    const iv = setInterval(() => {
      setUptime((u) => u + TICK);
      const prev = taskRef.current;
      if (!prev) return;
      let t: TaskSnapshot = { ...prev };
      phaseLeft.current -= TICK;

      if (phase.current === "planning") {
        if (phaseLeft.current <= 0) {
          log("plan.ready", `nodes=4 target="${t.pr_id}" strategy=sandboxed_execution`);
          t = spawnWorker(t);
        }
        setTask(t);
        return;
      }

      if (phase.current === "implementing" || phase.current === "testing") {
        if (alive.current) {
          lease.current = LEASE_TTL_MS;
          beatLeft.current -= TICK;
          if (beatLeft.current <= 0) {
            beatLeft.current = HEARTBEAT_MS;
            seq.current += 1;
            t.heartbeat_seq = seq.current;
            log("heartbeat.pulse", `origin=${t.worker_id} seq=${seq.current}`, "muted");
            setPulses((p) => [...p.slice(1), 0.55 + Math.random() * 0.45]);
          } else {
            setPulses((p) => [...p.slice(1), Math.max(0.06, (p[p.length - 1] ?? 0) * 0.6)]);
          }
        } else {
          lease.current = Math.max(0, lease.current - TICK);
          setPulses((p) => [...p.slice(1), 0.02]);
          if (lease.current === 0) {
            if (t.attempt >= t.max_attempts) {
              phase.current = "failed";
              t.status = "failed";
              log("task.failed", `attempts_exhausted=${t.max_attempts}`, "accent");
              setVerdicts((v) =>
                [{ pr_id: t.pr_id, merged: false, attempt: t.attempt }, ...v].slice(0, 4),
              );
              setTask(t);
              return;
            }
            log("lease.expired", `worker=${t.worker_id} ttl=${LEASE_TTL_MS}ms`, "accent");
            t.attempt += 1;
            log("task.reassigned", `attempt=${t.attempt} reason=lease_expired fence=ok`);
            t = spawnWorker(t);
            setTask(t);
            return;
          }
        }

        if (
          phase.current === "implementing" &&
          phaseLeft.current <= 2600 &&
          t.status !== "testing"
        ) {
          t.status = "testing";
          log("cargo.test", 'running="swarm-core" --lib');
        }
        if (phaseLeft.current <= 0 && alive.current) {
          phase.current = "merged";
          t.status = "merged";
          t.merged = true;
          t.provenance = "real_cargo_test";
          alive.current = false;
          log("test.passed", "12 passed; 0 failed; provenance=real_cargo_test");
          log("gate.opened", `merge gate OPEN — attempt=${t.attempt}`, "accent");
          setVerdicts((v) =>
            [{ pr_id: t.pr_id, merged: true, attempt: t.attempt }, ...v].slice(0, 4),
          );
        }
      }
      t.lease_remaining_ms = lease.current;
      setTask(t);
    }, TICK);
    return () => clearInterval(iv);
  }, [log, spawnWorker]);

  const stage: 1 | 2 | 3 =
    phase.current === "planning"
      ? 1
      : phase.current === "merged" || phase.current === "failed"
        ? 3
        : 2;
  const running = phase.current === "implementing" || phase.current === "testing";
  const leasePct = task ? (task.lease_remaining_ms / LEASE_TTL_MS) * 100 : 100;

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background font-sans text-foreground selection:bg-accent selection:text-accent-foreground">
      <header className="z-20 flex h-10 items-center justify-between border-b border-border bg-panel/50 px-4 backdrop-blur-sm">
        <div className="flex items-center gap-6">
          <div className="flex items-center gap-2">
            <div className="size-2 animate-[hb_1s_infinite] bg-accent" />
            <span className="font-mono text-[10px] font-bold uppercase tracking-widest">
              Phoenix CI // System.Active
            </span>
          </div>
          <div className="h-4 w-px bg-border" />
          <div className="font-mono text-[10px] uppercase tracking-tighter text-muted">
            Host: <span className="text-foreground">swarm-prod-04</span>
          </div>
        </div>
        <div className="flex gap-4 font-mono text-[10px] text-muted">
          <span>UPTIME: {fmtUptime(uptime)}</span>
          <span className="text-accent">LEASE_FAILSAFE: ARMED</span>
        </div>
      </header>

      <main className="relative flex flex-1 overflow-hidden">
        {/* Left rail */}
        <nav className="z-20 flex w-64 shrink-0 flex-col border-r border-border bg-panel">
          <div className="border-b border-border p-4">
            <h2 className="mb-4 font-mono text-[10px] uppercase tracking-widest text-muted">
              Active Pipeline
            </h2>
            <div className="space-y-1">
              <StageRow n="01" label="Planner" active={stage === 1} done={stage > 1} idle={!task} />
              <StageRow
                n="02"
                label="Implementer"
                active={stage === 2}
                done={stage > 2}
                idle={!task}
              />
              <StageRow
                n="03"
                label="Merge Gate"
                active={stage === 3}
                done={task?.merged ?? false}
                idle={!task}
              />
            </div>
          </div>

          <div className="flex flex-1 flex-col p-4">
            <h2 className="mb-4 font-mono text-[10px] uppercase tracking-widest text-muted">
              Provenance
            </h2>
            <div className="space-y-3 rounded-sm border border-border bg-background p-3">
              <div>
                <div className="mb-1 font-mono text-[9px] uppercase text-muted">Origin PR</div>
                <div className="font-mono text-xs">{task ? `#${task.pr_id}` : "—"}</div>
              </div>
              <div>
                <div className="mb-1 font-mono text-[9px] uppercase text-muted">Sandbox</div>
                <div className="break-all font-mono text-xs">{task?.sandbox ?? "—"}</div>
              </div>
              <div className="border-t border-border pt-2">
                <span
                  className={`inline-block border px-1.5 py-0.5 font-mono text-[9px] uppercase ${
                    task?.provenance
                      ? "border-accent/40 bg-accent/20 text-accent"
                      : "border-border bg-white/5 text-muted"
                  }`}
                >
                  {task?.provenance ?? "awaiting provenance"}
                </span>
              </div>
            </div>
          </div>
        </nav>

        {/* Center */}
        <section className="relative z-20 flex flex-1 flex-col overflow-hidden bg-background">
          <div className="flex flex-1 flex-col gap-8 overflow-hidden p-8">
            <div className="flex items-start justify-between">
              <div>
                <h1 className="mb-2 text-4xl font-extrabold uppercase tracking-tighter">
                  Implementer Task <span className="text-accent">#{task?.task_id ?? "IDLE"}</span>
                </h1>
                <p className="max-w-md text-sm text-muted">
                  {task
                    ? statusLine(task, phase.current)
                    : "No task in flight. Submit a pull request to arm the pipeline."}
                </p>
              </div>
              <div className="text-right font-mono">
                <div className="mb-1 text-[10px] uppercase text-muted">Current Attempt</div>
                <div className="text-4xl font-bold tracking-tighter">
                  {String(task?.attempt ?? 0).padStart(2, "0")}{" "}
                  <span className="text-muted">/ {String(MAX_ATTEMPTS).padStart(2, "0")}</span>
                </div>
              </div>
            </div>

            <div className="grid shrink-0 grid-cols-3 gap-4">
              <div className="border border-border bg-panel p-4">
                <div className="mb-3 font-mono text-[9px] uppercase text-muted">
                  Heartbeat / Pulse
                </div>
                <div className="flex h-8 items-end gap-1">
                  {pulses.map((v, i) => (
                    <div
                      key={i}
                      className="w-1 bg-accent transition-all duration-100"
                      style={{ height: `${Math.max(4, v * 100)}%`, opacity: 0.25 + v * 0.75 }}
                    />
                  ))}
                </div>
                <div className="mt-4 flex justify-between font-mono text-[10px]">
                  <span className={alive.current ? "text-accent" : "text-muted"}>
                    {alive.current ? "SYNCED" : task ? "FLATLINE" : "STANDBY"}
                  </span>
                  <span className="text-muted">{HEARTBEAT_MS}ms INTERVAL</span>
                </div>
              </div>

              <div className="relative overflow-hidden border border-border bg-panel p-4">
                <div className="mb-3 font-mono text-[9px] uppercase text-muted">
                  Task Lease Expiry
                </div>
                <div className="relative h-8 w-full border border-white/5 bg-muted/10">
                  <div
                    className="absolute inset-y-0 left-0 bg-accent/80 transition-[width] duration-100"
                    style={{ width: `${leasePct}%` }}
                  />
                </div>
                <div className="mt-4 flex justify-between font-mono text-[10px]">
                  <span>{Math.round(task?.lease_remaining_ms ?? LEASE_TTL_MS)}ms REMAINING</span>
                  <span className="text-muted">TTL: {LEASE_TTL_MS}ms</span>
                </div>
              </div>

              <button
                onClick={kill}
                disabled={!running || !alive.current}
                className="group border border-accent bg-accent/5 p-4 text-left transition-colors active:bg-accent/20 disabled:cursor-not-allowed disabled:border-border disabled:bg-transparent disabled:opacity-40"
              >
                <div className="mb-3 font-mono text-[9px] font-bold uppercase text-accent">
                  Fault Injection
                </div>
                <div className="flex h-8 items-center justify-center border border-accent/40 bg-accent/10">
                  <span className="text-xs font-bold uppercase tracking-[0.2em] text-accent">
                    Kill Worker
                  </span>
                </div>
                <div className="mt-4 text-center font-mono text-[9px] uppercase text-accent/60 group-hover:text-accent">
                  Manual Lease Termination
                </div>
              </button>
            </div>

            <div className="flex min-h-0 flex-1 flex-col border border-border bg-panel font-mono text-xs">
              <div className="flex h-8 items-center justify-between border-b border-border bg-white/5 px-4">
                <span className="text-[10px] uppercase tracking-widest text-muted">
                  Raw Event Stream
                </span>
                <span className="text-[10px] text-accent">{running ? "LIVE" : "IDLE"}</span>
              </div>
              <EventLog events={events} />
            </div>
          </div>

          <div className="flex h-8 items-center justify-between border-t border-border px-6 font-mono text-[10px] text-muted">
            <span>WORKERS: {alive.current ? 1 : 0} ACTIVE</span>
            <span className="text-foreground">SWARM_CI v2.4.0-NIGHTLY</span>
          </div>
        </section>

        {/* Right */}
        <aside className="z-20 w-80 shrink-0 overflow-y-auto border-l border-border bg-panel p-6">
          <h2 className="mb-6 font-mono text-[10px] uppercase tracking-widest text-muted">
            New Task Submission
          </h2>
          <div className="space-y-4">
            <div>
              <label className="mb-1.5 block font-mono text-[9px] uppercase text-muted">
                PR Identifier
              </label>
              <input
                value={prId}
                onChange={(e) => setPrId(e.target.value)}
                type="text"
                placeholder="PR-103"
                className="w-full border border-border bg-background px-3 py-2 font-mono text-xs outline-none placeholder:text-muted/50 focus:border-accent"
              />
            </div>
            <div>
              <label className="mb-1.5 block font-mono text-[9px] uppercase text-muted">
                PR Link
              </label>
              <input
                value={prLink}
                onChange={(e) => setPrLink(e.target.value)}
                type="url"
                inputMode="url"
                placeholder="https://github.com/org/repo/pull/103"
                className="w-full border border-border bg-background px-3 py-2 font-mono text-xs outline-none placeholder:text-muted/50 focus:border-accent"
              />
            </div>
            <div>
              <label className="mb-1.5 block font-mono text-[9px] uppercase text-muted">
                Target Bug Description
              </label>
              <textarea
                value={bug}
                onChange={(e) => setBug(e.target.value)}
                rows={4}
                placeholder="Memory leak in tokio runtime bridge during heavy reassignment load..."
                className="w-full resize-none border border-border bg-background px-3 py-2 font-mono text-xs outline-none placeholder:text-muted/50 focus:border-accent"
              />
            </div>

            <button
              onClick={submit}
              className="w-full border border-white/10 bg-foreground py-3 text-xs font-bold uppercase tracking-[0.2em] text-background transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              Submit to Planner
            </button>
          </div>

          <div className="mt-12 border-t border-border pt-12">
            <h3 className="mb-4 font-mono text-[10px] uppercase text-muted">Last Verdict</h3>
            {verdicts.length === 0 ? (
              <div className="border border-white/5 bg-white/[0.02] p-4 font-mono text-[10px] uppercase text-muted">
                No completed runs
              </div>
            ) : (
              <div className="space-y-2">
                {verdicts.map((v, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between border border-white/5 bg-white/[0.02] p-4"
                  >
                    <div>
                      <div className="mb-1 text-xs font-bold">{v.pr_id}</div>
                      <div className="text-[10px] text-muted">
                        {v.merged ? `Merged via Gate · attempt ${v.attempt}` : "Gate held closed"}
                      </div>
                    </div>
                    <div
                      className={`flex size-6 items-center justify-center border ${
                        v.merged ? "border-white/20" : "border-accent/60"
                      }`}
                    >
                      <div className={`size-2 ${v.merged ? "bg-white/40" : "bg-accent"}`} />
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </aside>
      </main>
    </div>
  );
}

function EventLog({ events }: { events: PhoenixEvent[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [events]);
  return (
    <div ref={ref} className="flex-1 space-y-1 overflow-y-auto p-4 opacity-90">
      {events.length === 0 && <div className="text-muted">// awaiting task submission…</div>}
      {events.map((e) => (
        <div key={e.id} className="flex gap-4 animate-[rowin_.18s_ease-out]">
          <span className="shrink-0 text-muted">[{e.ts}]</span>
          <span
            className={
              e.level === "accent"
                ? "font-bold text-accent"
                : e.level === "muted"
                  ? "text-muted"
                  : "text-foreground"
            }
          >
            {e.name} {e.detail}
          </span>
        </div>
      ))}
    </div>
  );
}

function StageRow({
  n,
  label,
  active,
  done,
  idle,
}: {
  n: string;
  label: string;
  active: boolean;
  done: boolean;
  idle: boolean;
}) {
  if (idle)
    return (
      <div className="flex items-center gap-3 border border-transparent p-2 opacity-40">
        <div className="flex size-4 items-center justify-center bg-muted/20 font-mono text-[10px]">
          {n}
        </div>
        <span className="text-xs font-medium uppercase">{label}</span>
        <span className="ml-auto font-mono text-[9px]">STANDBY</span>
      </div>
    );
  if (active)
    return (
      <div className="flex items-center gap-3 border border-accent/30 bg-accent/10 p-2">
        <div className="flex size-4 items-center justify-center bg-accent font-mono text-[10px] text-accent-foreground">
          {n}
        </div>
        <span className="text-xs font-medium uppercase text-accent">{label}</span>
        <span className="ml-auto animate-pulse font-mono text-[9px]">RUNNING</span>
      </div>
    );
  if (done)
    return (
      <div className="flex items-center gap-3 border border-white/10 bg-white/5 p-2">
        <div className="flex size-4 items-center justify-center bg-muted/20 font-mono text-[10px] text-muted">
          {n}
        </div>
        <span className="text-xs font-medium uppercase">{label}</span>
        <span className="ml-auto font-mono text-[9px] text-muted">DONE</span>
      </div>
    );
  return (
    <div className="flex items-center gap-3 border border-transparent p-2 opacity-40">
      <div className="flex size-4 items-center justify-center bg-muted/20 font-mono text-[10px]">
        {n}
      </div>
      <span className="text-xs font-medium uppercase">{label}</span>
      <span className="ml-auto font-mono text-[9px]">WAIT</span>
    </div>
  );
}

function statusLine(t: TaskSnapshot, phase: Phase) {
  if (phase === "planning") return "Planner reading the diff. Typed plan pending before dispatch.";
  if (t.merged)
    return "Merge gate opened on a real passing cargo test run. Provenance verified, task closed.";
  if (phase === "failed")
    return "Attempts exhausted. Gate held closed — no merge on agent self-report.";
  if (t.attempt > 1)
    return "Reassignment triggered. Attempting second pass on sandboxed working copy. Deterministic merge gate pending test pass.";
  return "Implementer editing the sandboxed working copy. Heartbeats holding the lease open.";
}

function fmtUptime(ms: number) {
  const s = Math.floor(ms / 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}
