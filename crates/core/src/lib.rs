//! Swarm CI domain core: types, state machine, events, deterministic gate.
//!
//! Contract (BUILD_PROMPT.md §0.5): NO LLM/vendor code here. This crate owns
//! the truth — checked transitions, typed events, provenance-based merge
//! gating — so the agentic layers can be wrong without corrupting anything.

pub mod error;
pub mod events;
pub mod gate;
pub mod ids;
pub mod lease;
pub mod mail;
pub mod task;
