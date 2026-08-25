//! The implementer agent: plan → sandbox edits → REAL `cargo test`.
//!
//! Provenance honesty lives HERE: the returned [`TestReport`] always carries
//! `TestOrigin::RealCargoTest` because we genuinely execute cargo inside the
//! sandbox; pass/fail comes from the process exit status — never from any
//! model claim. This is what the deterministic merge gate keys off.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use swarm_core::events::TestOrigin;
use swarm_core::gate::TestReport;
use swarm_core::ids::TaskId;

use crate::llm::LlmProvider;
use crate::plan::{planner_prompt, FixPlan};
use crate::AgentError;

pub struct ImplementerAgent {
    provider: Arc<dyn LlmProvider>,
    /// The pristine "PR under review" working copy (e.g. fixtures/demo-pr).
    fixture_src: PathBuf,
    /// Parent directory for per-attempt sandbox copies.
    sandbox_root: PathBuf,
}

impl ImplementerAgent {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        fixture_src: PathBuf,
        sandbox_root: PathBuf,
    ) -> Self {
        Self {
            provider,
            fixture_src,
            sandbox_root,
        }
    }

    /// One attempt's full flow. MVP note: fs + subprocess calls are
    /// synchronous inside this async fn; they are short (tiny fixture crate)
    /// and each worker runs on its own task, so blocking here does not stall
    /// other tasks on multi-thread runtimes.
    pub async fn fix(&self, bug_description: &str) -> Result<TestReport, AgentError> {
        // Demo affordance (Phase 5): optionally widen the mid-flight window so
        // a presenter can hit ⚡KILL while the planner is thinking.
        // Read once per process; unset => zero delay.
        static PLAN_DELAY: std::sync::OnceLock<std::time::Duration> = std::sync::OnceLock::new();
        let delay = *PLAN_DELAY.get_or_init(|| {
            std::time::Duration::from_millis(
                std::env::var("SWARM_PLAN_DELAY_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            )
        });
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }

        // Gather the sandbox's current source so the planner fixes real code.
        // Nested fn keeps this single-purpose and side-effect free.
        fn collect_context(root: &Path) -> std::io::Result<Vec<(String, String)>> {
            fn walk(
                dir: &Path,
                base: &Path,
                out: &mut Vec<(String, String)>,
            ) -> std::io::Result<()> {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_dir() {
                        if entry.file_name().to_string_lossy() == "target" {
                            continue;
                        }
                        walk(&path, base, out)?;
                    } else if let Ok(content) = std::fs::read_to_string(&path) {
                        let rel = match path.strip_prefix(base) {
                            Ok(rel) => rel.to_string_lossy().into_owned(),
                            Err(_) => continue,
                        };
                        out.push((rel, content));
                    }
                }
                Ok(())
            }
            let mut files = Vec::new();
            walk(root, root, &mut files)?;
            files.sort();
            Ok(files)
        }
        let context = collect_context(&self.fixture_src)?;

        let raw = self
            .provider
            .complete(&planner_prompt(bug_description, &context))
            .await?;
        let plan = FixPlan::from_llm_text(&raw)?;

        // Fresh sandbox per attempt: a killed worker can never leave half a
        // fix behind for the next generation to trip over.
        let sandbox = self.sandbox_root.join(TaskId::generate().to_string());
        copy_dir(&self.fixture_src, &sandbox)?;
        for edit in &plan.edits {
            // Paths were validated at parse time (no absolute / no `..`).
            let dest = sandbox.join(&edit.path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, &edit.content)?;
        }

        // THE ONLY source of truth for pass/fail: a real cargo test run.
        let output = Command::new("cargo")
            .args(["test", "--quiet"])
            .current_dir(&sandbox)
            .output()?;
        let passed = output.status.success();
        let summary = summarize(&output);

        // Keep passing sandboxes around for demo inspection; clean up failures.
        if !passed {
            let _ = std::fs::remove_dir_all(&sandbox);
        }

        Ok(TestReport {
            passed,
            origin: TestOrigin::RealCargoTest,
            summary,
        })
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn summarize(output: &std::process::Output) -> String {
    let combined = format!(
        "exit={:?}\nstderr:\n{}\nstdout(tail):\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        tail(String::from_utf8_lossy(&output.stdout).as_ref(), 600)
    );
    tail(&combined, 1000)
}

/// Keep the TAIL — cargo prints its failure summary last.
fn tail(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().skip(count - max_chars).collect()
}
