//! OpenAI-compatible LLM chat client (epic #243 Batch 4, ref #247).
//!
//! Minimal, synchronous-in-shape async client that POSTs a chat
//! completion request to `{base_url}/chat/completions` and parses the
//! assistant's JSON content. No retries, backoff, streaming, or caching
//! by design (plan scope).

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

/// Configuration for an OpenAI-compatible chat endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    /// Full API root, e.g. `https://api.openai.com/v1`; the request is
    /// sent to `{base_url}/chat/completions`.
    pub base_url: String,
    /// Bearer token for the `Authorization` header.
    pub api_key: String,
    /// Model identifier sent in the request body.
    pub model: String,
}

/// Errors produced by [`LlmClient`].
#[derive(Debug, Error)]
pub enum LlmError {
    /// A required configuration field is empty.
    #[error("llm not configured: {0}")]
    EmptyConfig(String),
    /// Transport-level failure (unreachable host, malformed URL, timeout).
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    /// Non-2xx HTTP status from the endpoint.
    #[error("http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body text (may be empty).
        body: String,
    },
    /// The response carried no usable assistant content.
    #[error("empty response content")]
    NoContent,
    /// The assistant content was not valid JSON.
    #[error("invalid JSON in response: {0}")]
    InvalidJson(String),
}

/// OpenAI-compatible chat client.
#[derive(Debug)]
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    /// Construct a [`LlmClient`].
    ///
    /// Validates that `base_url` and `model` are non-empty (→ [`LlmError::EmptyConfig`]).
    /// The `api_key` is deliberately not validated here; emptiness is the caller's
    /// concern (backend checks `is_configured`).
    pub fn new(config: LlmConfig) -> Result<LlmClient, LlmError> {
        if config.base_url.trim().is_empty() {
            return Err(LlmError::EmptyConfig("base_url".to_string()));
        }
        if config.model.trim().is_empty() {
            return Err(LlmError::EmptyConfig("model".to_string()));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(LlmClient { config, http })
    }

    /// Send a chat completion request and parse the assistant's JSON content.
    ///
    /// POSTs to `{base_url}/chat/completions` with Bearer auth, request body
    /// `{"model", "messages":[{role:system},{role:user}], "temperature":0.0,
    /// "response_format":{"type":"json_object"}}`. A 2xx response yields
    /// `choices[0].message.content`, which must parse as JSON.
    pub async fn chat_json(&self, system: &str, user: &str) -> Result<Value, LlmError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.0,
            "response_format": {"type": "json_object"},
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status: status.as_u16(),
                body: body_text,
            });
        }

        let payload: Value = response.json().await?;
        let content = payload
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or(LlmError::NoContent)?;

        serde_json::from_str(content).map_err(|e| LlmError::InvalidJson(e.to_string()))
    }
}
