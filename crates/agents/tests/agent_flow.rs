//! End-to-end agent flow against the REAL fixture, using the mock provider
//! (offline). The implementer genuinely runs `cargo test` in a sandbox copy —
//! these tests exercise that for real.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use swarm_agents::{ImplementerAgent, LlmError, LlmProvider, MockLlmProvider};
use swarm_core::events::TestOrigin;
use swarm_core::ids::TaskId;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/demo-pr");

fn unique_sandbox(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("swarm-agents-{tag}-{}", TaskId::generate()))
}

#[tokio::test]
async fn mock_plan_then_real_cargo_test_makes_fixture_pass() {
    let sandbox = unique_sandbox("pass");
    let agent = ImplementerAgent::new(
        Arc::new(MockLlmProvider),
        PathBuf::from(FIXTURE),
        sandbox.clone(),
    );
    let report = agent
        .fix("sum(&[1,2,3]) returns 3 — it skips the final element")
        .await
        .expect("agent flow completes");

    assert_eq!(report.origin, TestOrigin::RealCargoTest);
    assert!(
        report.passed,
        "fixture must PASS after the planned fix; summary: {}",
        report.summary
    );
    let _ = std::fs::remove_dir_all(&sandbox);
}

/// A plan whose fix is WRONG must produce an honest FAILED report with real
/// provenance — this is the anti-self-report property end to end.
struct BrokenPlanner;

#[async_trait]
impl LlmProvider for BrokenPlanner {
    async fn complete(&self, _prompt: &str) -> Result<String, LlmError> {
        Ok(
            r#"Here you go: {"summary":"wrong","root_cause":"wrong","edits":[{"path":"src/lib.rs","content":"pub fn sum(values: &[i64]) -> i64 { values.len() as i64 }"}]}"#
                .to_string(),
        )
    }
}

#[tokio::test]
async fn failing_real_tests_report_failure_not_success() {
    let sandbox = unique_sandbox("fail");
    let agent = ImplementerAgent::new(
        Arc::new(BrokenPlanner),
        PathBuf::from(FIXTURE),
        sandbox.clone(),
    );
    let report = agent
        .fix("deliberately broken plan — tests must fail")
        .await
        .expect("flow still completes when tests fail");

    assert_eq!(report.origin, TestOrigin::RealCargoTest);
    assert!(!report.passed, "a broken fix MUST be reported as failure");
    let _ = std::fs::remove_dir_all(&sandbox);
}
