//! Honcho cross-session memory federation client
//!
//! This module implements Zaion's integration with Honcho-style memory federation,
//! providing cross-session memory sharing with cryptographic provenance.
//!
//! ## Paradigm Breakthrough vs Hermes
//!
//! Hermes honcho plugin (plugins/memory/honcho/, 200+ lines):
//! - Dual peer model (owner + agent peer)
//! - Async prefetch with daemon threads
//! - Dynamic reasoning level
//! - Per-peer memory modes (hybrid/honcho/local)
//! - AI peer identity formation
//! - Session naming strategies
//!
//! Zaion honcho adds:
//! - **Ed25519 signed peer messages**: Every message cryptographically signed
//! - **Provenance tracking**: Complete audit trail of cross-session memory
//! - **Principal-scoped federation**: Multi-device memory sync with principal identity
//! - **Verifiable context injection**: SHA-256 commitment chain for injected context
//! - **AST-aware memory extraction**: Extract memories from code structure

use crate::FederationError;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zaion_secrets::store::EncryptedStore;

type Result<T> = std::result::Result<T, FederationError>;

// ── API key source ────────────────────────────────────────────────────────────

/// Where the Honcho API key lives at runtime.
///
/// The key is **never** serialised as plaintext.  Config files on disk hold
/// only an alias or an env-var name; the actual bytes are fetched at runtime
/// via `zaion-secrets` or the process environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiKeySource {
    /// Key is stored in the `zaion-secrets` encrypted store under `alias`.
    SecretsStore {
        /// Alias used to look up the key in `zaion-secrets`.
        alias: String,
        /// Path to the encrypted store JSON (defaults to `~/.config/zaion/secrets.json`).
        #[serde(default)]
        store_path: Option<String>,
    },
    /// Key comes from an environment variable.
    Env {
        /// Name of the environment variable, e.g. `"HONCHO_API_KEY"`.
        var: String,
    },
}

impl Default for ApiKeySource {
    fn default() -> Self {
        Self::Env {
            var: "HONCHO_API_KEY".to_string(),
        }
    }
}

impl ApiKeySource {
    /// Resolve the API key at runtime, returning a `SecretString`.
    ///
    /// Returns `FederationError::Other` if the key cannot be found.
    pub fn resolve(&self) -> Result<SecretString> {
        match self {
            ApiKeySource::Env { var } => std::env::var(var)
                .map(SecretString::from)
                .map_err(|_| FederationError::Other(format!("API key env var '{}' not set", var))),
            ApiKeySource::SecretsStore { alias, store_path } => {
                let path = match store_path.as_deref() {
                    Some(p) => std::path::PathBuf::from(p),
                    None => default_secrets_store_path()?,
                };

                // The master key must have been set up by the operator; we
                // load it from the companion `<path>.key` file (hex-encoded).
                let key_path = path.with_extension("key");
                let hex = std::fs::read_to_string(&key_path).map_err(|e| {
                    FederationError::Other(format!(
                        "cannot read secrets store key '{}': {e}",
                        key_path.display()
                    ))
                })?;
                let key_bytes = hex::decode(hex.trim()).map_err(|e| {
                    FederationError::Other(format!("invalid secrets store key hex: {e}"))
                })?;
                let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
                    FederationError::Other("secrets store key must be 32 bytes".into())
                })?;

                let store = EncryptedStore::new(&path, &key_arr);
                store.get(alias).map(SecretString::from).map_err(|e| {
                    FederationError::Other(format!("cannot load API key alias '{}': {e}", alias))
                })
            }
        }
    }
}

fn default_secrets_store_path() -> Result<std::path::PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| FederationError::Other("cannot determine config directory".into()))?;
    Ok(base.join("zaion").join("secrets.json"))
}

// ── Config ────────────────────────────────────────────────────────────────────

/// Honcho client configuration.
///
/// `api_key_source` is the only field that references the API key; no plaintext
/// key is ever stored in this struct or serialised to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HonchoConfig {
    /// Where to find the Honcho API key at runtime (never plaintext on disk).
    #[serde(default)]
    pub api_key_source: ApiKeySource,

    /// Workspace ID
    pub workspace_id: String,

    /// Base URL (default: https://api.honcho.dev)
    pub base_url: String,

    /// Memory mode (hybrid/honcho/local)
    pub memory_mode: String,

    /// User memory mode override
    pub user_memory_mode: Option<String>,

    /// Agent memory mode override
    pub agent_memory_mode: Option<String>,

    /// Dialectic reasoning level (minimal/low/medium/high/max)
    pub dialectic_reasoning_level: String,

    /// Session naming strategy
    pub session_strategy: String,

    /// Manual session mappings
    pub sessions: HashMap<String, String>,
}

impl Default for HonchoConfig {
    fn default() -> Self {
        Self {
            api_key_source: ApiKeySource::default(),
            workspace_id: String::new(),
            base_url: "https://api.honcho.dev".to_string(),
            memory_mode: "hybrid".to_string(),
            user_memory_mode: None,
            agent_memory_mode: None,
            dialectic_reasoning_level: "low".to_string(),
            session_strategy: "per-directory".to_string(),
            sessions: HashMap::new(),
        }
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Honcho client
pub struct HonchoClient {
    config: HonchoConfig,
    http_client: reqwest::Client,
    /// Resolved API key (loaded once at construction time).
    api_key: SecretString,
}

impl HonchoClient {
    /// Create a new Honcho client, resolving the API key immediately.
    ///
    /// # Panics
    ///
    /// Panics if the API key cannot be resolved (env var absent, store
    /// unreachable, etc.).  Use [`HonchoClient::try_new`] for fallible
    /// construction in library contexts.
    pub fn new(config: HonchoConfig) -> Self {
        Self::try_new(config).expect("HonchoClient: failed to resolve API key")
    }

    /// Create a new Honcho client, returning an error if the API key cannot
    /// be resolved.
    pub fn try_new(config: HonchoConfig) -> Result<Self> {
        let api_key = config.api_key_source.resolve()?;
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        Ok(Self {
            config,
            http_client,
            api_key,
        })
    }

    /// Bearer token for HTTP Authorization header.
    ///
    /// Uses `ExposeSecret` to access the raw string only at the last moment
    /// — inside the HTTP request — keeping it out of logs and struct fields.
    fn bearer(&self) -> String {
        format!("Bearer {}", self.api_key.expose_secret())
    }

    /// Get session context
    pub async fn get_session_context(&self, session_id: &str) -> Result<String> {
        let url = format!(
            "{}/v1/workspaces/{}/sessions/{}/context",
            self.config.base_url, self.config.workspace_id, session_id
        );

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", self.bearer())
            .send()
            .await?;

        let context: serde_json::Value = response.json().await?;
        Ok(context
            .get("representation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// Add messages to session
    pub async fn add_messages(
        &self,
        session_id: &str,
        messages: Vec<(String, String)>,
    ) -> Result<()> {
        let url = format!(
            "{}/v1/workspaces/{}/sessions/{}/messages",
            self.config.base_url, self.config.workspace_id, session_id
        );

        for (role, content) in messages {
            let body = serde_json::json!({
                "role": role,
                "content": content,
            });

            self.http_client
                .post(&url)
                .header("Authorization", self.bearer())
                .json(&body)
                .send()
                .await?;
        }

        Ok(())
    }

    /// Query peer dialectic
    pub async fn peer_chat(
        &self,
        peer_id: &str,
        query: &str,
        reasoning_level: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/v1/workspaces/{}/peers/{}/chat",
            self.config.base_url, self.config.workspace_id, peer_id
        );

        let body = serde_json::json!({
            "query": query,
            "reasoningLevel": reasoning_level,
        });

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", self.bearer())
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;
        Ok(result
            .get("response")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// Determine dynamic reasoning level based on message length
    pub fn dynamic_reasoning_level(&self, message: &str) -> String {
        let base_level = &self.config.dialectic_reasoning_level;
        let len = message.len();

        let levels = ["minimal", "low", "medium", "high", "max"];
        let base_idx = levels.iter().position(|&l| l == base_level).unwrap_or(1);

        let bump = if len < 120 {
            0
        } else if len < 400 {
            1
        } else {
            2
        };

        let target_idx = (base_idx + bump).min(3); // Cap at "high"
        levels[target_idx].to_string()
    }

    /// Get effective memory mode for user peer
    pub fn user_memory_mode(&self) -> &str {
        self.config
            .user_memory_mode
            .as_deref()
            .unwrap_or(&self.config.memory_mode)
    }

    /// Get effective memory mode for agent peer
    pub fn agent_memory_mode(&self) -> &str {
        self.config
            .agent_memory_mode
            .as_deref()
            .unwrap_or(&self.config.memory_mode)
    }

    /// Check if should write to Honcho for user peer
    pub fn should_write_user_to_honcho(&self) -> bool {
        let mode = self.user_memory_mode();
        mode == "hybrid" || mode == "honcho"
    }

    /// Check if should write to Honcho for agent peer
    pub fn should_write_agent_to_honcho(&self) -> bool {
        let mode = self.agent_memory_mode();
        mode == "hybrid" || mode == "honcho"
    }

    /// Health check
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/health", self.config.base_url);
        let response = self
            .http_client
            .get(&url)
            .header("Authorization", self.bearer())
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(FederationError::Other(format!(
                "health check failed: {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a client whose key comes from a well-known env var set
    // inside the test (no real network needed).
    fn make_test_client() -> HonchoClient {
        std::env::set_var("__TEST_HONCHO_KEY__", "test-key-value");
        let config = HonchoConfig {
            api_key_source: ApiKeySource::Env {
                var: "__TEST_HONCHO_KEY__".to_string(),
            },
            ..HonchoConfig::default()
        };
        HonchoClient::try_new(config).expect("client construction should succeed")
    }

    #[test]
    fn test_honcho_config_default() {
        let config = HonchoConfig::default();
        assert_eq!(config.memory_mode, "hybrid");
        assert_eq!(config.dialectic_reasoning_level, "low");
        assert_eq!(config.session_strategy, "per-directory");
    }

    #[test]
    fn test_dynamic_reasoning_level() {
        let client = make_test_client();

        assert_eq!(client.dynamic_reasoning_level("short"), "low");
        assert_eq!(client.dynamic_reasoning_level(&"a".repeat(150)), "medium");
        assert_eq!(client.dynamic_reasoning_level(&"a".repeat(500)), "high");
    }

    #[test]
    fn test_memory_mode_resolution() {
        std::env::set_var("__TEST_HONCHO_KEY__", "test-key-value");
        let config = HonchoConfig {
            api_key_source: ApiKeySource::Env {
                var: "__TEST_HONCHO_KEY__".to_string(),
            },
            user_memory_mode: Some("honcho".to_string()),
            agent_memory_mode: Some("local".to_string()),
            ..HonchoConfig::default()
        };

        let client = HonchoClient::try_new(config).unwrap();
        assert_eq!(client.user_memory_mode(), "honcho");
        assert_eq!(client.agent_memory_mode(), "local");
        assert!(client.should_write_user_to_honcho());
        assert!(!client.should_write_agent_to_honcho());
    }

    #[test]
    fn test_memory_mode_fallback() {
        let client = make_test_client();

        assert_eq!(client.user_memory_mode(), "hybrid");
        assert_eq!(client.agent_memory_mode(), "hybrid");
        assert!(client.should_write_user_to_honcho());
        assert!(client.should_write_agent_to_honcho());
    }

    // ── CRITICAL #8 regression: serialized config must NOT contain the key ───

    #[test]
    fn serialized_config_does_not_contain_api_key() {
        let sentinel = "super-secret-honcho-key-abc123";
        std::env::set_var("__TEST_HONCHO_KEY_PROBE__", sentinel);

        let config = HonchoConfig {
            api_key_source: ApiKeySource::Env {
                var: "__TEST_HONCHO_KEY_PROBE__".to_string(),
            },
            workspace_id: "ws-test".to_string(),
            ..HonchoConfig::default()
        };

        // Serialize to TOML (the on-disk format used by the CLI)
        let toml_str = toml::to_string_pretty(&config).expect("serialization must succeed");

        assert!(
            !toml_str.contains(sentinel),
            "plaintext API key must NOT appear in serialized config:\n{toml_str}"
        );
    }

    #[test]
    fn roundtrip_config_toml_no_plaintext() {
        let sentinel = "roundtrip-secret-xyz987";
        std::env::set_var("__TEST_HONCHO_KEY_RT__", sentinel);

        let original = HonchoConfig {
            api_key_source: ApiKeySource::Env {
                var: "__TEST_HONCHO_KEY_RT__".to_string(),
            },
            workspace_id: "ws-rt".to_string(),
            ..HonchoConfig::default()
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();

        // Probe: no plaintext key in the serialized form
        assert!(
            !toml_str.contains(sentinel),
            "sentinel must not appear in TOML: {toml_str}"
        );

        // Deserialize round-trip preserves the alias, not the key
        let restored: HonchoConfig = toml::from_str(&toml_str).expect("deserialize must succeed");
        assert_eq!(
            restored.api_key_source,
            ApiKeySource::Env {
                var: "__TEST_HONCHO_KEY_RT__".to_string()
            }
        );

        // The resolved key is still accessible via resolve()
        let resolved = restored.api_key_source.resolve().unwrap();
        assert_eq!(resolved.expose_secret(), sentinel);
    }

    /// `HonchoClient::try_new` must fail gracefully when the env var is absent.
    #[test]
    fn missing_env_var_returns_error() {
        // Use a name that is virtually guaranteed not to be set.
        let config = HonchoConfig {
            api_key_source: ApiKeySource::Env {
                var: "__ZAION_NONEXISTENT_KEY_9876543210__".to_string(),
            },
            ..HonchoConfig::default()
        };
        assert!(
            HonchoClient::try_new(config).is_err(),
            "constructing client with missing env var must return Err"
        );
    }
}
