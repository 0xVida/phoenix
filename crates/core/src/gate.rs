//! The deterministic merge gate — the ONLY logic allowed to decide
//! mergeability, and a PURE function over a provenance-carrying report.
//!
//! Security invariant (BUILD_PROMPT.md §0.5, §3 Phase 1): an agent saying
//! "I fixed it" is never input here. Input is a `TestReport` recording HOW the
//! result was produced. Only `TestOrigin::RealCargoTest` can open the gate
//! when `require_real_cargo_test` is on (default; Phase 1 tests opt out so the
//! full lifecycle is provable before Phase 3 wires the real `cargo test`).

use crate::events::TestOrigin;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub passed: bool,
    pub origin: TestOrigin,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MergePolicy {
    pub require_real_cargo_test: bool,
}

impl Default for MergePolicy {
    fn default() -> Self {
        Self {
            require_real_cargo_test: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MergeDecision {
    Open { origin: TestOrigin },
    Gated { reason: String },
}

pub fn evaluate_merge_gate(report: &TestReport, policy: MergePolicy) -> MergeDecision {
    match (report.passed, report.origin, policy.require_real_cargo_test) {
        (true, TestOrigin::RealCargoTest, _) => MergeDecision::Open {
            origin: report.origin,
        },
        (true, TestOrigin::Simulated, false) => MergeDecision::Open {
            origin: report.origin,
        },
        (true, TestOrigin::Simulated, true) => MergeDecision::Gated {
            reason: "tests reported pass, but this was not a real cargo test run".into(),
        },
        (false, _, _) => MergeDecision::Gated {
            reason: format!("tests failed: {}", report.summary),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(passed: bool, origin: TestOrigin) -> TestReport {
        TestReport {
            passed,
            origin,
            summary: "s".into(),
        }
    }

    #[test]
    fn simulated_pass_never_opens_when_real_tests_required() {
        let d = evaluate_merge_gate(&report(true, TestOrigin::Simulated), MergePolicy::default());
        assert!(matches!(d, MergeDecision::Gated { .. }));
    }

    #[test]
    fn real_pass_opens_always() {
        let d = evaluate_merge_gate(&report(true, TestOrigin::RealCargoTest), MergePolicy::default());
        assert_eq!(d, MergeDecision::Open { origin: TestOrigin::RealCargoTest });
    }

    #[test]
    fn failure_gates_with_reason() {
        let d = evaluate_merge_gate(&report(false, TestOrigin::RealCargoTest), MergePolicy::default());
        assert!(matches!(d, MergeDecision::Gated { .. }));
    }

    #[test]
    fn simulated_pass_allowed_in_dev_mode() {
        let policy = MergePolicy { require_real_cargo_test: false };
        let d = evaluate_merge_gate(&report(true, TestOrigin::Simulated), policy);
        assert_eq!(d, MergeDecision::Open { origin: TestOrigin::Simulated });
    }
}
