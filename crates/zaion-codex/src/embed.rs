use crate::CodexError;
/// Embedding engine for semantic code search.
///
/// Design principle: zero native library dependencies.
/// Embeddings are generated via any OpenAI-compatible `/v1/embeddings` endpoint:
///   - OpenAI text-embedding-3-small (API key required)
///   - Ollama nomic-embed-text (local, no API key)
///   - Any other compatible endpoint
///
/// Cosine similarity search is pure Rust — no vector DB needed.
use serde::{Deserialize, Serialize};

// ─── EmbeddingConfig ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

impl EmbeddingConfig {
    /// OpenAI text-embedding-3-small (1536 dims, best quality).
    pub fn openai(api_key: impl Into<String>) -> Self {
        EmbeddingConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key: Some(api_key.into()),
            model: "text-embedding-3-small".into(),
        }
    }

    /// Ollama local server running nomic-embed-text (768 dims, no API key).
    /// Default Ollama endpoint: http://localhost:11434
    pub fn ollama_nomic(base_url: Option<&str>) -> Self {
        EmbeddingConfig {
            base_url: base_url.unwrap_or("http://localhost:11434/v1").into(),
            api_key: None,
            model: "nomic-embed-text".into(),
        }
    }

    /// Load from environment variables.
    /// CODEX_EMBED_URL  — base URL (default: http://localhost:11434/v1)
    /// CODEX_EMBED_KEY  — API key (optional)
    /// CODEX_EMBED_MODEL— model name (default: nomic-embed-text)
    pub fn from_env() -> Self {
        EmbeddingConfig {
            base_url: std::env::var("CODEX_EMBED_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into()),
            api_key: std::env::var("CODEX_EMBED_KEY").ok(),
            model: std::env::var("CODEX_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".into()),
        }
    }
}

// ─── EmbeddingEngine ──────────────────────────────────────────────────────

pub struct EmbeddingEngine {
    config: EmbeddingConfig,
    client: reqwest::blocking::Client,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

impl EmbeddingEngine {
    pub fn new(config: EmbeddingConfig) -> Self {
        EmbeddingEngine {
            config,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Create engine from environment variables.
    pub fn from_env() -> Self {
        Self::new(EmbeddingConfig::from_env())
    }

    /// Embed a single string. Returns the embedding vector.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, CodexError> {
        let mut results = self.embed_batch(&[text])?;
        results
            .pop()
            .ok_or_else(|| CodexError::Parse("empty embed response".into()))
    }

    /// Embed a batch of strings efficiently.
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, CodexError> {
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));
        let body = EmbedRequest {
            model: &self.config.model,
            input: texts.to_vec(),
        };
        let mut req = self.client.post(&url).json(&body);
        if let Some(ref key) = self.config.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let resp = req
            .send()
            .map_err(|e| CodexError::Parse(format!("embed HTTP: {}", e)))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(CodexError::Parse(format!("embed {} : {}", status, text)));
        }
        let parsed: EmbedResponse = resp
            .json()
            .map_err(|e| CodexError::Parse(format!("embed parse: {}", e)))?;
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    /// Embed a chunk's canonical "query text":
    ///   "<kind> <name>: <first line of doc comment or content>"
    pub fn embed_chunk_text(
        kind_str: &str,
        name: &str,
        doc: Option<&str>,
        content: &str,
    ) -> String {
        let desc = doc
            .and_then(|d| d.lines().next())
            .or_else(|| content.lines().next())
            .unwrap_or("");
        format!("{} {}: {}", kind_str, name, desc)
    }
}

// ─── Cosine similarity ─────────────────────────────────────────────────────

/// Cosine similarity in [0.0, 1.0]. Returns 0.0 for zero-norm vectors.
#[inline]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let (mut dot, mut norm_a, mut norm_b) = (0f32, 0f32, 0f32);
    for i in 0..len {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        0.0
    } else {
        (dot / denom).clamp(0.0, 1.0)
    }
}

// ─── Serialisation helpers ─────────────────────────────────────────────────

#[inline]
pub fn f32_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[inline]
pub fn blob_to_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0f32, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0f32, 0.0, 0.0];
        let b = vec![0.0f32, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_zero() {
        let z = vec![0.0f32; 4];
        let v = vec![1.0f32, 2.0, 3.0, 4.0];
        assert_eq!(cosine_similarity(&z, &v), 0.0);
    }

    #[test]
    fn test_blob_roundtrip() {
        let orig = vec![1.5f32, -2.3, 0.0, 100.0];
        assert_eq!(blob_to_f32(&f32_to_blob(&orig)), orig);
    }

    #[test]
    fn test_chunk_text_with_doc() {
        let text = EmbeddingEngine::embed_chunk_text(
            "fn",
            "process",
            Some("Process a request"),
            "fn process() {}",
        );
        assert_eq!(text, "fn process: Process a request");
    }

    #[test]
    fn test_chunk_text_from_content() {
        let text =
            EmbeddingEngine::embed_chunk_text("struct", "Foo", None, "pub struct Foo { x: i32 }");
        assert_eq!(text, "struct Foo: pub struct Foo { x: i32 }");
    }
}
