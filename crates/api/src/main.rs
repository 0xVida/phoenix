//! Swarm CI API server binary.
//!
//! Dev-mode defaults until Phase 3: workers are SIMULATED implementers and the
//! gate accepts simulated reports (`require_real_cargo_test = false`). The
//! supervisor/lease/fencing machinery is fully real.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use swarm_agents::ImplementerAgent;

use swarm_api::spawner::{AgentExecutor, DemoSpawner, ExecutorFactory, SimulatedImplementer};
use swarm_api::state::AppState;
use swarm_core::gate::MergePolicy;
use swarm_core::lease::LeaseConfig;
use swarm_core::mail::Assignment;
use swarm_supervisor::{Supervisor, SupervisorConfig};
use tracing_subscriber::EnvFilter;

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let lease = LeaseConfig {
        heartbeat_interval: Duration::from_millis(env_u64("SWARM_HEARTBEAT_MS", 500)),
        lease_timeout: Duration::from_millis(env_u64("SWARM_LEASE_TIMEOUT_MS", 1500)),
    };
    lease.validate()?;

    // Executor selection (Phase 3): the REAL implementer agent runs when
    // LLM_PROVIDER=anthropic; otherwise stay in clearly-labelled dev mode.
    let provider_name = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "mock".into());
    let agent_mode = match provider_name.as_str() {
        "groq" | "google" | "anthropic" => true,
        "mock" | "" => false,
        other => {
            tracing::error!(provider = %other, "unknown LLM_PROVIDER; falling back to simulated workers");
            false
        }
    };
    if !agent_mode {
        tracing::warn!(
            "DEV MODE: merge gate accepts SIMULATED test reports (set LLM_PROVIDER=anthropic for the real gate)"
        );
    }

    let config = SupervisorConfig {
        lease,
        reap_interval: Duration::from_millis(env_u64("SWARM_REAP_INTERVAL_MS", 250)),
        max_attempts: env_u64("SWARM_MAX_ATTEMPTS", 3).min(u32::MAX as u64) as u32,
        // In agent mode, the ONLY accepted provenance is a real cargo test run.
        merge_policy: MergePolicy {
            require_real_cargo_test: agent_mode,
        },
    };

    let factory: ExecutorFactory = if agent_mode {
        let provider = swarm_agents::llm::provider_from_env()?;
        let fixture = std::env::var("SWARM_FIXTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("fixtures/demo-pr"));
        let sandbox_root = std::env::var("SWARM_SANDBOX_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("swarm-ci-sandbox"));
        let agent = Arc::new(ImplementerAgent::new(
            provider,
            fixture,
            sandbox_root,
        ));
        tracing::info!("agent mode: implementer runs REAL cargo test per attempt");
        Arc::new(move |assignment: &Assignment| {
            Box::new(AgentExecutor {
                agent: agent.clone(),
                spec: assignment.spec.clone(),
            }) as Box<dyn swarm_worker::TaskExecutor>
        })

    } else {
        let work_for = Duration::from_millis(env_u64("SWARM_SIM_WORK_MS", 3000));
        Arc::new(move |_assignment: &Assignment| {
            Box::new(SimulatedImplementer { work_for }) as Box<dyn swarm_worker::TaskExecutor>
        })
    };

    let spawner = Arc::new(DemoSpawner::new(factory));
    // Clone the inner spawner (shares its state Arc) so we hand the supervisor
    // a plain `DemoSpawner`, which is what implements `WorkerSpawner`.
    let supervisor = Supervisor::new(config, Box::new((*spawner).clone()))?;
    spawner.set_handle(supervisor.handle());

    // Single supervisor loop for the whole process.
    let runner = supervisor.clone();
    tokio::spawn(async move {
        if let Err(e) = runner.run().await {
            tracing::error!(error = ?e, "supervisor loop terminated");
        }
    });

    let state = AppState {
        supervisor,
        tasks: Arc::default(),
        spawner,
    };
    let app = swarm_api::router(state);

    let addr = std::env::var("SWARM_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "swarm-ci api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
