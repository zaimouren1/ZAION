//! OpenAI-compatible embeddings API.

use crate::AdapterError;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: String,
    pub base_url: String,
    pub api_key: String,
}

impl EmbeddingRequest {
    /// OpenAI-compatible defaults (text-embedding-3-small, 1536 dims).
    pub fn openai(api_key: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            model: "text-embedding-3-small".into(),
            input: input.into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: api_key.into(),
        }
    }
}

/// Call an OpenAI-compatible embedding endpoint and return the embedding vector.
pub fn embed_text(req: &EmbeddingRequest) -> Result<Vec<f32>, AdapterError> {
    #[derive(Deserialize)]
    struct EmbedResp {
        data: Vec<EmbedData>,
    }
    #[derive(Deserialize)]
    struct EmbedData {
        embedding: Vec<f32>,
    }

    let url = format!("{}/embeddings", req.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": req.model,
        "input": req.input,
        "encoding_format": "float",
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| AdapterError::Provider(e.to_string()))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(AdapterError::Provider(format!(
            "embed HTTP {}: {}",
            status, text
        )));
    }
    let parsed: EmbedResp = resp
        .json()
        .map_err(|e| AdapterError::Provider(e.to_string()))?;
    parsed
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or_else(|| AdapterError::Provider("empty embedding response".into()))
}
