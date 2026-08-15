//! Proposal generator — calls LLM to produce a concrete patch for each finding.

use crate::scanner::Finding;
use crate::EvolveError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    Pending,
    Accepted,
    Rejected,
    Applied,
}

/// A concrete improvement proposal generated from a Finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub finding: Finding,
    /// Short description of what the patch does.
    pub description: String,
    /// The actual code patch (unified diff or replacement snippet).
    pub patch: String,
    /// Rationale from the LLM.
    pub rationale: String,
    pub status: ProposalStatus,
    pub created_at: String,
}

/// LLM configuration for proposal generation.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl LlmConfig {
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        Some(Self {
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string()),
            api_key: key,
            model: std::env::var("ZAION_EVOLVE_MODEL")
                .unwrap_or_else(|_| "glm-4-flash".to_string()),
        })
    }
}

pub struct Proposer {
    llm: Option<LlmConfig>,
}

impl Proposer {
    pub fn new(llm: Option<LlmConfig>) -> Self {
        Self { llm }
    }

    /// Generate a proposal for a single finding.
    ///
    /// Proposal generation is fail-closed: static-analysis findings may create
    /// review work, but they must not become placeholder patches when the LLM
    /// path is unavailable.
    pub fn propose(&self, finding: &Finding) -> Result<Proposal, EvolveError> {
        let id = format!("prop-{}", uuid_short());
        let now = chrono::Utc::now().to_rfc3339();

        let cfg = self.llm.as_ref().ok_or_else(|| {
            EvolveError::Llm(
                "LLM config is required for proposal generation; static stub proposals are disabled"
                    .to_string(),
            )
        })?;

        let (desc, patch, rationale) = self.call_llm(cfg, finding)?;
        if patch.trim().is_empty() {
            return Err(EvolveError::Llm(
                "LLM returned an empty proposal patch".to_string(),
            ));
        }

        Ok(Proposal {
            id,
            finding: finding.clone(),
            description: desc,
            patch,
            rationale,
            status: ProposalStatus::Pending,
            created_at: now,
        })
    }

    fn call_llm(
        &self,
        cfg: &LlmConfig,
        finding: &Finding,
    ) -> Result<(String, String, String), EvolveError> {
        let prompt = format!(
            "You are a Rust code improvement assistant for the Zaion OS project.\n\
             \n\
             Finding: {kind} in {file}:{line}\n\
             Snippet:\n```rust\n{snippet}\n```\n\
             \n\
             Provide:\n\
             1. DESCRIPTION: one-line description of the fix (max 80 chars)\n\
             2. PATCH: the corrected code snippet (just the changed lines, not a full diff)\n\
             3. RATIONALE: one sentence explaining why this improves the code\n\
             \n\
             Format your response EXACTLY as:\n\
             DESCRIPTION: <text>\n\
             PATCH:\n<code>\nEND_PATCH\n\
             RATIONALE: <text>",
            kind = finding.kind,
            file = finding.file,
            line = finding.line,
            snippet = finding.snippet,
        );

        let body = serde_json::json!({
            "model": cfg.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 300,
            "temperature": 0.3,
        });

        // H24 fix: async reqwest + lazy-runtime sync wrapper.  Avoids the
        // per-call hidden runtime of `reqwest::blocking` and removes the
        // deadlock risk of nesting blocking HTTP inside a tokio worker.
        let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
        let auth = format!("Bearer {}", cfg.api_key);
        let text = run_async(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            let resp = client
                .post(&url)
                .header("Authorization", auth)
                .json(&body)
                .send()
                .await
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(EvolveError::Llm(format!("HTTP {}", resp.status())));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            Ok(json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string())
        })?;

        let desc = extract_section(&text, "DESCRIPTION:", "\n")
            .unwrap_or_else(|| format!("Fix: {}", finding.kind));
        let patch = extract_section(&text, "PATCH:\n", "\nEND_PATCH")
            .unwrap_or_else(|| finding.snippet.clone());
        let rationale = extract_section(&text, "RATIONALE:", "\n")
            .unwrap_or_else(|| "LLM-generated fix.".to_string());

        Ok((desc, patch, rationale))
    }
}

/// Drive an async future to completion from a sync context without requiring
/// the caller to run inside a Tokio runtime.
///
/// H24/H25 helper: if already inside a runtime, spawns a short-lived worker
/// thread with its own current-thread runtime (to avoid nested-runtime panic).
pub(crate) fn run_async<F, T>(fut: F) -> Result<T, EvolveError>
where
    F: std::future::Future<Output = Result<T, EvolveError>> + Send + 'static,
    T: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(EvolveError::Llm(e.to_string())));
                    return;
                }
            };
            let _ = tx.send(rt.block_on(fut));
        });
        rx.recv().map_err(|e| EvolveError::Llm(e.to_string()))?
    } else {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| EvolveError::Llm(e.to_string()))?;
        rt.block_on(fut)
    }
}

fn extract_section(text: &str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start = text.find(start_marker)? + start_marker.len();
    let rest = &text[start..];
    let end = rest.find(end_marker).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}", t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{Finding, FindingKind};

    fn make_finding(kind: FindingKind) -> Finding {
        Finding {
            kind,
            file: "src/lib.rs".to_string(),
            line: 42,
            snippet: "    let x = foo().unwrap();".to_string(),
            priority: 2,
        }
    }

    #[test]
    fn propose_without_llm_fails_closed_instead_of_returning_stub() {
        let p = Proposer::new(None);
        let err = p
            .propose(&make_finding(FindingKind::UnwrapInProd))
            .unwrap_err()
            .to_string();
        assert!(err.contains("LLM config is required"));
    }

    #[test]
    fn llm_failure_fails_closed_instead_of_returning_stub() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 2048];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });

        let p = Proposer::new(Some(LlmConfig {
            base_url: format!("http://{}", addr),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
        }));
        let err = p
            .propose(&make_finding(FindingKind::UnwrapInProd))
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(!err.contains("Using stub"));
        assert!(err.contains("llm error"));
    }

    #[test]
    fn extract_section_works() {
        let text = "DESCRIPTION: fix the bug\nPATCH:\nsome code\nEND_PATCH\nRATIONALE: better";
        assert_eq!(
            extract_section(text, "DESCRIPTION:", "\n"),
            Some("fix the bug".to_string())
        );
        assert_eq!(
            extract_section(text, "PATCH:\n", "\nEND_PATCH"),
            Some("some code".to_string())
        );
    }
}
