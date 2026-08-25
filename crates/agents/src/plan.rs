//! Typed plans. Model output is parsed INTO these structs — never trusted as
//! prose — and each edit REPLACES a whole file inside the review sandbox.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::AgentError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPlan {
    pub summary: String,
    pub root_cause: String,
    pub edits: Vec<PlannedEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedEdit {
    pub path: String,
    pub content: String,
}

impl FixPlan {
    /// Extract a JSON object from model output that may wrap it in prose or
    /// code fences — and may leave trailing chatter after it. Strategy:
    ///   1. whole-text direct parse (fast path),
    ///   2. fenced blocks (```json … ```),
    ///   3. quote-aware balanced-brace scan: try every complete `{…}` object,
    ///      first one that parses wins.
    pub fn from_llm_text(text: &str) -> Result<Self, AgentError> {
        if let Ok(plan) = serde_json::from_str::<FixPlan>(text.trim()) {
            return Self::finalize(plan);
        }
        for fence in ["```json", "```JSON", "```"] {
            if let Some(start) = text.find(fence) {
                let rest = &text[start + fence.len()..];
                if let Some(end_rel) = rest.find("```") {
                    if let Ok(plan) =
                        serde_json::from_str::<FixPlan>(rest[..end_rel].trim())
                    {
                        return Self::finalize(plan);
                    }
                }
            }
        }

        let chars: Vec<char> = text.chars().collect();
        let mut stack: Vec<usize> = Vec::new();
        let mut in_string = false;
        let mut escaped = false;
        for (i, &c) in chars.iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                '{' => stack.push(i),
                '}' => {
                    if let Some(start) = stack.pop() {
                        if stack.is_empty() && start < i {
                            let candidate: String = chars[start..=i].iter().collect();
                            if let Ok(plan) =
                                serde_json::from_str::<FixPlan>(&candidate)
                            {
                                return Self::finalize(plan);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Err(AgentError::PlanParse(
            "no valid JSON object found in model output".into(),
        ))
    }

    fn finalize(plan: FixPlan) -> Result<Self, AgentError> {
        if plan.edits.is_empty() {
            return Err(AgentError::PlanParse("plan contained no edits".into()));
        }
        for edit in &plan.edits {
            validate_path(&edit.path)?;
        }
        Ok(plan)
    }
}

/// Sandbox safety: relative paths only, no `..` components, no absolute paths.
fn validate_path(path: &str) -> Result<(), AgentError> {
    let p = Path::new(path);
    if path.is_empty()
        || p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(AgentError::UnsafePath(path.to_string()));
    }
    Ok(())
}

/// Prompt contract for planners. The literal "TASK: PLAN" prefix is also how
/// `MockLlmProvider` detects planner calls. `context_files` carries the
/// sandbox's CURRENT source so the model fixes real code, not guesses.
pub fn planner_prompt(bug_description: &str, context_files: &[(String, String)]) -> String {
    let mut context = String::from("CURRENT FILES IN THE REVIEW SANDBOX:\n");
    for (path, content) in context_files {
        context.push_str(&format!("--- FILE: {path} ---\n{content}\n"));
    }
    format!(
        "TASK: PLAN\n\
         You are the planner agent of Swarm CI, reviewing a pull request.\n\
         BUG DESCRIPTION:\n{}\n\n\
         {context}\n\
         Produce the minimal correct fix. Respond with ONLY one JSON object — \
         no prose, no markdown fences — shaped exactly like:\n\
         {{\"summary\": string, \"root_cause\": string, \
         \"edits\": [{{\"path\": string, \"content\": string}}]}}\n\
         Each edit fully REPLACES the target file inside the review sandbox.\n",
        bug_description.trim()
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plan_wrapped_in_prose_and_fences() {
        let text = "Sure! Here's my plan:\n```json\n{\"summary\":\"s\",\"root_cause\":\"r\",\
                    \"edits\":[{\"path\":\"src/lib.rs\",\"content\":\"fn x() {}\"}]}\n```\nDone.";
        let plan = FixPlan::from_llm_text(text).expect("parses");
        assert_eq!(plan.edits[0].path, "src/lib.rs");
    }

    #[test]
    fn rejects_path_traversal() {
        let text = r#"{"summary":"s","root_cause":"r","edits":[{"path":"../../etc/passwd","content":"x"}]}"#;
        assert!(matches!(
            FixPlan::from_llm_text(text),
            Err(AgentError::UnsafePath(_))
        ));
    }

    #[test]
    fn rejects_empty_edits() {
        let text = r#"{"summary":"s","root_cause":"r","edits":[]}"#;
        assert!(matches!(
            FixPlan::from_llm_text(text),
            Err(AgentError::PlanParse(_))
        ));
    }
}
