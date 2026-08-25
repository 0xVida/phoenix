//! Provider-agnostic LLM access. The supervisor/worker/gate never see a vendor.

use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("ANTHROPIC_API_KEY is required when LLM_PROVIDER=anthropic (keys come from env, never code)")]
    MissingApiKey,

    #[error("unsupported LLM_PROVIDER: {0} (expected `mock` or `anthropic`)")]
    UnsupportedProvider(String),

    #[error("llm http error: {0}")]
    Http(String),

    #[error("unexpected llm response shape: {0}")]
    BadResponse(String),
}

/// One method on purpose (spec §3): swap vendors/local mocks freely.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError>;
}

/// Deterministic offline provider for dev/tests. Recognizes the planner
/// contract ("TASK: PLAN" prefix, see `plan::planner_prompt`) and answers with
/// the known-good fix for the demo fixture. Built via serde_json so the
/// embedded multi-line file content is guaranteed valid JSON.
pub struct MockLlmProvider;

const FIXED_LIB_RS: &str = "pub fn sum(values: &[i64]) -> i64 {\n    values.iter().sum()\n}\n";

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        if prompt.starts_with("TASK: PLAN") {
            let plan = serde_json::json!({
                "summary": "Make sum include every element",
                "root_cause": "the slice expression dropped the final element",
                "edits": [
                    { "path": "src/lib.rs", "content": FIXED_LIB_RS }
                ]
            });
            return Ok(serde_json::to_string_pretty(&plan).expect("json! value serializes"));
        }
        Err(LlmError::BadResponse(
            "mock provider only handles TASK: PLAN prompts".into(),
        ))
    }
}

/// Real Anthropic Messages API implementation. Key/base URL/model from env —
/// NEVER hardcoded or committed.
pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl AnthropicProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".into());
        let model =
            std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into());
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url,
            model,
        })
    }
}

#[derive(serde::Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: [Message<'a>; 1],
}

#[derive(serde::Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(serde::Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&MessagesRequest {
                model: &self.model,
                max_tokens: 2048,
                messages: [Message {
                    role: "user",
                    content: prompt,
                }],
            })
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = truncate(response.text().await.unwrap_or_default(), 300);
            return Err(LlmError::Http(format!("{status}: {body}")));
        }

        let parsed: MessagesResponse = response
            .json()
            .await
            .map_err(|e| LlmError::BadResponse(e.to_string()))?;
        parsed
            .content
            .into_iter()
            .find_map(|block| block.text)
            .ok_or_else(|| LlmError::BadResponse("no text block in response".into()))
    }
}

fn truncate(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// Groq Cloud (OpenAI-compatible Chat Completions) — fast primary for
/// planner/implementer calls.
pub struct GroqProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl GroqProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("GROQ_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: std::env::var("GROQ_BASE_URL")
                .unwrap_or_else(|_| "https://api.groq.com/openai/v1".into()),
            model: std::env::var("GROQ_MODEL")
                .unwrap_or_else(|_| "openai/gpt-oss-120b".into()),
            api_key,
        })
    }
}

#[async_trait]
impl LlmProvider for GroqProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "temperature": 0.0,
                "messages": [{ "role": "user", "content": prompt }],
            }))
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = truncate(response.text().await.unwrap_or_default(), 300);
            return Err(LlmError::Http(format!("{status}: {body}")));
        }
        parse_openai_chat(&response.text().await.unwrap_or_default())
    }
}

/// Google AI Studio (Gemini generateContent) — heavy-reasoning backup.
pub struct GoogleProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl GoogleProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("GOOGLE_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: std::env::var("GOOGLE_BASE_URL")
                .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".into()),
            model: std::env::var("GOOGLE_MODEL")
                .unwrap_or_else(|_| "gemini-3.6-flash".into()),
            api_key,
        })
    }
}

#[async_trait]
impl LlmProvider for GoogleProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        let url = format!(
            "{}/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let response = self
            .http
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&serde_json::json!({
                "contents": [{ "parts": [{ "text": prompt }] }],
                "generationConfig": { "temperature": 0.0 },
            }))
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = truncate(response.text().await.unwrap_or_default(), 300);
            return Err(LlmError::Http(format!("{status}: {body}")));
        }
        parse_gemini(&response.text().await.unwrap_or_default())
    }
}

/// Tries the primary; on ANY error falls back to the backup (heavy reasoner).
pub struct FallbackProvider {
    pub primary: std::sync::Arc<dyn LlmProvider>,
    pub backup: Box<dyn LlmProvider>,
}

#[async_trait]
impl LlmProvider for FallbackProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        match self.primary.complete(prompt).await {
            Ok(text) => Ok(text),
            Err(primary_err) => {
                tracing::warn!(error = %primary_err, "primary llm failed; falling back to backup");
                self.backup.complete(prompt).await
            }
        }
    }
}

pub(crate) fn parse_openai_chat(body: &str) -> Result<String, LlmError> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| LlmError::BadResponse("choices[0].message.content missing".into()))
}

pub(crate) fn parse_gemini(body: &str) -> Result<String, LlmError> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    let parts = v["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("candidates[0].content.parts missing".into()))?;
    for part in parts {
        if let Some(text) = part["text"].as_str() {
            if !text.is_empty() {
                return Ok(text.to_owned());
            }
        }
    }
    Err(LlmError::BadResponse(
        "no text part in gemini response".into(),
    ))
}

/// Env-driven selection. `LLM_PROVIDER=groq|google|anthropic` picks a real
/// provider; when a `GOOGLE_API_KEY` is also present it becomes the automatic
/// heavy-reasoning fallback. `mock` stays fully offline.
pub fn provider_from_env() -> Result<std::sync::Arc<dyn LlmProvider>, LlmError> {
    use std::sync::Arc;
    let name = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "mock".into());
    let primary: Arc<dyn LlmProvider> = match name.as_str() {
        "groq" => Arc::new(GroqProvider::from_env()?),
        "google" => Arc::new(GoogleProvider::from_env()?),
        "anthropic" => Arc::new(AnthropicProvider::from_env()?),
        "mock" | "" => return Ok(Arc::new(MockLlmProvider)),
        other => return Err(LlmError::UnsupportedProvider(other.into())),
    };
    if name != "google" {
        if let Ok(backup) = GoogleProvider::from_env() {
            tracing::info!(primary = %name, "llm fallback enabled: google backup");
            return Ok(Arc::new(FallbackProvider {
                primary,
                backup: Box::new(backup),
            }));
        }
    }
    Ok(primary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_openai_chat_shape() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"hello plan"}}]}"#;
        assert_eq!(parse_openai_chat(body).unwrap(), "hello plan");
    }

    #[test]
    fn openai_missing_content_is_error() {
        assert!(parse_openai_chat(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn parses_gemini_multi_part() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":""},{"text":"real answer"}]}}]}"#;
        assert_eq!(parse_gemini(body).unwrap(), "real answer");
    }

    #[test]
    fn gemini_missing_candidates_is_error() {
        assert!(parse_gemini(r#"{}"#).is_err());
    }

    struct Fixed(&'static str);
    #[async_trait]
    impl LlmProvider for Fixed {
        async fn complete(&self, _p: &str) -> Result<String, LlmError> {
            Ok(self.0.to_string())
        }
    }
    struct AlwaysFails;
    #[async_trait]
    impl LlmProvider for AlwaysFails {
        async fn complete(&self, _p: &str) -> Result<String, LlmError> {
            Err(LlmError::Http("boom".into()))
        }
    }

    #[tokio::test]
    async fn fallback_used_when_primary_fails() {
        let chain = FallbackProvider {
            primary: Arc::new(AlwaysFails),
            backup: Box::new(Fixed("backup says hi")),
        };
        assert_eq!(chain.complete("x").await.unwrap(), "backup says hi");
    }

    #[tokio::test]
    async fn primary_wins_when_healthy() {
        let chain = FallbackProvider {
            primary: Arc::new(Fixed("primary")),
            backup: Box::new(AlwaysFails),
        };
        assert_eq!(chain.complete("x").await.unwrap(), "primary");
    }
}



