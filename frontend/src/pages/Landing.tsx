import { HEARTBEAT_MS, LEASE_TTL_MS } from "@/lib/phoenix";

const STATS: [string, string][] = [
  ["Lease TTL", `${LEASE_TTL_MS}ms`],
  ["Heartbeat", `${HEARTBEAT_MS}ms`],
  ["Merge proof", "real_cargo_test"],
  ["Agent trust", "zero"],
];

const PIPELINE = [
  {
    n: "01",
    k: "Planner",
    t: "Reads the diff",
    d: "An LLM emits a typed plan — nodes, target, strategy. It may be wrong; nothing downstream trusts it.",
  },
  {
    n: "02",
    k: "Dispatcher",
    t: "Deterministic hand-off",
    d: "Plain code turns the plan into a task, opens a lease, and spawns an implementer in a fresh sandbox.",
  },
  {
    n: "03",
    k: "Implementer",
    t: "Edits and tests",
    d: "A Tokio worker patches the working copy, emits heartbeats every 500ms, and runs the real test suite.",
  },
  {
    n: "04",
    k: "Merge gate",
    t: "Opens on proof",
    d: "Green only from an observed passing test run. Self-reported success is discarded.",
  },
];

const BOUNDARY: [string, string, boolean][] = [
  ["Agentic", "Plan generation, patch authoring", false],
  ["Deterministic", "State machine transitions", true],
  ["Deterministic", "Lease + heartbeat supervision", true],
  ["Deterministic", "Worker reassignment with fencing", true],
  ["Deterministic", "Test execution + merge gate", true],
];

const STREAM: [string, string, "muted" | "accent" | "info"][] = [
  ["heartbeat.pulse", "origin=w-7f31 seq=8", "muted"],
  ["worker.killed", "signal=SIGKILL (fault injection)", "accent"],
  ["heartbeat.lost", "supervisor watching lease…", "accent"],
  ["lease.expired", `worker=w-7f31 ttl=${LEASE_TTL_MS}ms`, "accent"],
  ["task.reassigned", "attempt=2 reason=lease_expired fence=ok", "info"],
  ["worker.spawned", "id=w-c904 sandbox=/tmp/swarm-sandbox/…", "info"],
  ["test.passed", "12 passed; 0 failed", "info"],
  ["gate.opened", "merge gate OPEN — attempt=2", "accent"],
];

export default function Landing() {
  return (
    <div className="min-h-screen bg-background font-sans text-foreground">
      {/* NAV */}
      <header className="sticky top-0 z-40 border-b border-border bg-background">
        <div className="mx-auto grid max-w-[1400px] grid-cols-12 items-center px-6">
          <div className="col-span-6 flex h-16 items-center gap-3 border-r border-border pr-6 md:col-span-3">
            <span className="size-2 bg-accent" />
            <span className="font-mono text-[12px] font-bold uppercase tracking-[0.2em]">
              Phoenix CI
            </span>
          </div>
          <nav className="col-span-6 hidden h-16 items-center gap-8 border-r border-border px-6 font-mono text-[11px] uppercase tracking-[0.14em] text-muted md:flex">
            <a href="#pipeline" className="transition-colors hover:text-foreground">
              Pipeline
            </a>
            <a href="#boundary" className="transition-colors hover:text-foreground">
              Boundary
            </a>
            <a href="#recovery" className="transition-colors hover:text-foreground">
              Recovery
            </a>
          </nav>
          <div className="col-span-6 flex h-16 items-center justify-end md:col-span-3">
            <a
              href="/console"
              className="bg-accent px-5 py-2 font-mono text-[11px] font-bold uppercase tracking-[0.16em] text-accent-foreground transition-opacity hover:opacity-85"
            >
              Console
            </a>
          </div>
        </div>
      </header>

      {/* HERO — asymmetric 8/4 */}
      <section className="border-b border-border">
        <div className="mx-auto grid max-w-[1400px] grid-cols-12 px-6">
          <div className="col-span-12 border-border py-16 md:col-span-8 md:border-r md:py-24 md:pr-12">
            <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-muted">
              Swarm Village Hackathon / Live fault injection
            </p>
            <h1 className="mt-8 font-display text-[13vw] font-bold leading-[0.88] tracking-[-0.04em] md:text-[6.5rem]">
              Agents review
              <br />
              the PR.
              <br />
              <span className="text-accent">Determinism</span>
              <br />
              decides the merge.
            </h1>
            <p className="mt-10 max-w-xl text-base leading-relaxed text-muted">
              A planner agent reads the diff, an implementer worker fixes it in a sandbox, and a
              supervisor holds a lease on every in-flight task. Kill the worker mid-run — heartbeats
              stop, the lease expires, work is reassigned, and the gate still opens only on a real
              passing <span className="font-mono text-foreground">cargo test</span>.
            </p>
            <div className="mt-10 flex flex-wrap gap-px bg-border">
              <a
                href="/console"
                className="bg-accent px-8 py-4 font-mono text-[11px] font-bold uppercase tracking-[0.18em] text-accent-foreground transition-opacity hover:opacity-85"
              >
                Run the demo
              </a>
              <a
                href="#recovery"
                className="bg-background px-8 py-4 font-mono text-[11px] font-bold uppercase tracking-[0.18em] text-foreground transition-colors hover:bg-panel"
              >
                Kill-and-recover
              </a>
            </div>
          </div>

          <aside className="col-span-12 flex flex-col justify-between border-t border-border py-10 md:col-span-4 md:border-t-0 md:py-24 md:pl-12">
            <div>
              <div className="flex items-center justify-between border-b border-border pb-3 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                <span>supervisor.status</span>
                <span className="flex items-center gap-2 text-accent">
                  <span className="size-1.5 animate-[hb_1s_infinite] bg-accent" />
                  armed
                </span>
              </div>
              <dl className="mt-6 space-y-5 font-mono text-[12px]">
                {[
                  ["state", "watching_lease"],
                  ["in_flight", "1 task"],
                  ["attempt", "2 / 3"],
                  ["provenance", "real_cargo_test"],
                ].map(([k, v]) => (
                  <div key={k} className="flex items-baseline justify-between gap-4">
                    <dt className="uppercase tracking-[0.14em] text-muted">{k}</dt>
                    <dd className="text-right text-foreground">{v}</dd>
                  </div>
                ))}
              </dl>
            </div>
            <p className="mt-10 border-t border-border pt-6 font-mono text-[11px] leading-relaxed text-muted">
              The supervisor never asks a worker whether it is healthy. It watches a lease and acts
              when the lease drains.
            </p>
          </aside>
        </div>
      </section>

      {/* STATS HAIRLINE STRIP */}
      <section className="border-b border-border">
        <div className="mx-auto grid max-w-[1400px] grid-cols-2 gap-px bg-border px-6 md:grid-cols-4">
          {STATS.map(([k, v]) => (
            <div key={k} className="bg-background px-6 py-8">
              <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                {k}
              </div>
              <div className="mt-3 font-mono text-2xl font-bold tracking-tight">{v}</div>
            </div>
          ))}
        </div>
      </section>

      {/* PIPELINE */}
      <section id="pipeline" className="border-b border-border">
        <div className="mx-auto max-w-[1400px] px-6 py-20">
          <SectionHead index="01" title="The pipeline" />
          <div className="mt-12 grid grid-cols-1 gap-px bg-border md:grid-cols-4">
            {PIPELINE.map((s) => (
              <article key={s.n} className="bg-background p-8">
                <div className="font-mono text-5xl font-bold leading-none tracking-tight text-border">
                  {s.n}
                </div>
                <div className="mt-8 font-mono text-[10px] uppercase tracking-[0.2em] text-accent">
                  {s.k}
                </div>
                <h3 className="mt-3 font-display text-2xl font-bold tracking-tight">{s.t}</h3>
                <p className="mt-3 text-sm leading-relaxed text-muted">{s.d}</p>
              </article>
            ))}
          </div>
        </div>
      </section>

      {/* TRUST BOUNDARY TABLE */}
      <section id="boundary" className="border-b border-border">
        <div className="mx-auto grid max-w-[1400px] grid-cols-12 px-6 py-20">
          <div className="col-span-12 md:col-span-4 md:pr-12">
            <SectionHead index="02" title="Trust boundary" />
            <p className="mt-10 font-display text-3xl font-bold leading-tight tracking-tight">
              Agentic layers may be wrong and are never trusted blindly.
            </p>
            <p className="mt-5 text-sm leading-relaxed text-muted">
              Deterministic layers own the truth: state transitions, leases, reassignment, test
              execution, and the merge gate. The agents only ever propose.
            </p>
          </div>
          <div className="col-span-12 mt-12 md:col-span-8 md:mt-0">
            <div className="grid grid-cols-[120px_1fr_auto] items-center border-b border-border pb-3 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
              <span>Layer</span>
              <span>Responsibility</span>
              <span>Authority</span>
            </div>
            {BOUNDARY.map(([kind, label, hard]) => (
              <div
                key={label}
                className="grid grid-cols-[120px_1fr_auto] items-center gap-4 border-b border-border py-5"
              >
                <span
                  className={`font-mono text-[10px] font-bold uppercase tracking-[0.14em] ${
                    hard ? "text-accent" : "text-muted"
                  }`}
                >
                  {kind}
                </span>
                <span className="text-sm">{label}</span>
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
                  {hard ? "decides" : "proposes"}
                </span>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* RECOVERY */}
      <section id="recovery" className="border-b border-border">
        <div className="mx-auto grid max-w-[1400px] grid-cols-12 px-6 py-20">
          <div className="col-span-12 md:col-span-5 md:pr-12">
            <SectionHead index="03" title="Kill-and-recover, live" />
            <h2 className="mt-10 font-display text-4xl font-bold leading-[0.95] tracking-tight">
              SIGKILL the worker.
              <br />
              <span className="text-accent">The PR still lands.</span>
            </h2>
            <p className="mt-6 text-sm leading-relaxed text-muted">
              When heartbeats stop, the lease drains to zero, the task is fenced and reassigned to a
              fresh worker, and the attempt counter increments — no duplicate merges, no lost work.
            </p>
            <a
              href="/console"
              className="mt-10 inline-block bg-accent px-8 py-4 font-mono text-[11px] font-bold uppercase tracking-[0.18em] text-accent-foreground transition-opacity hover:opacity-85"
            >
              Inject a fault
            </a>
          </div>

          <div className="col-span-12 mt-12 border border-border md:col-span-7 md:mt-0">
            <div className="flex items-center justify-between border-b border-border px-5 py-3">
              <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                event.stream
              </span>
              <span className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.18em] text-accent">
                <span className="size-1.5 animate-[hb_1s_infinite] bg-accent" />
                live
              </span>
            </div>
            <div className="divide-y divide-border font-mono text-[12px]">
              {STREAM.map(([name, detail, level], i) => (
                <div key={name} className="grid grid-cols-[2.5rem_11rem_1fr] gap-3 px-5 py-2.5">
                  <span className="text-border">{String(i).padStart(2, "0")}</span>
                  <span className={level === "accent" ? "text-accent" : "text-foreground"}>
                    {name}
                  </span>
                  <span className="truncate text-muted">{detail}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      {/* FOOTER */}
      <footer className="mx-auto max-w-[1400px] px-6 py-16">
        <a
          href="/console"
          className="group block font-display text-[14vw] font-bold leading-[0.85] tracking-[-0.05em] transition-colors hover:text-accent md:text-[9rem]"
        >
          Console →
        </a>
        <div className="mt-12 flex flex-col gap-3 border-t border-border pt-6 font-mono text-[10px] uppercase tracking-[0.18em] text-muted sm:flex-row sm:items-center sm:justify-between">
          <span>Phoenix CI — formerly Swarm CI · crates still say swarm-*</span>
          <span className="text-accent">Lease failsafe: armed</span>
        </div>
      </footer>
    </div>
  );
}

function SectionHead({ index, title }: { index: string; title: string }) {
  return (
    <div className="flex items-center gap-4">
      <span className="font-mono text-[10px] font-bold tracking-[0.2em] text-accent">{index}</span>
      <span className="font-mono text-[10px] uppercase tracking-[0.2em] text-muted">{title}</span>
      <div className="h-px flex-1 bg-border" />
    </div>
  );
}
