//! Agent/LLM abstraction layer — Phase 3. Kept OUT of reliability logic
//! (BUILD_PROMPT.md §0.5): nothing in core/supervisor/worker may import this.
//!
//! Plan:
//! - `LlmProvider` trait (`complete(&str) -> Result<String, LlmError>`),
//!   provider-agnostic; key/base URL from env (`LLM_PROVIDER`,
//!   `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`) — never hardcoded, never committed.
//! - `MockLlmProvider` for offline dev; `AnthropicProvider` as first real impl.
//! - Planner agent: diff/bug description → TYPED plan (Serde structs, not free text).
//! - Implementer agent: executes plan inside a sandbox working copy; produces a
//!   `TestReport` only by actually running tests (origin = RealCargoTest).

/// Placeholder to keep the crate compiling until Phase 3 lands.
#[derive(Debug, thiserror::Error)]
#[error("agents not implemented yet (Phase 3)")]
pub struct NotImplemented;
