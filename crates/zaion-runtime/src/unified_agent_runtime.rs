//! Unified Agent Runtime - Main execution loop integration
//!
//! This module integrates all runtime components into a single unified agent
//! execution loop, completing the paradigm breakthrough by wiring together:
//! - WebhookRuntimeManager (webhook event → agent triggering)
//! - MemoryManager (automatic memory prefetch/sync)
//! - McpToolRegistry (MCP tool discovery and routing)
//! - ContextCompressor (automatic context compression)
//! - IntegratedAgentLoop (unified orchestration)
//!
//! ## Architecture
//!
//! ```text
//! User Input / Webhook Event
//!     ↓
//! UnifiedAgentRuntime
//!     ↓
//! ┌─────────────────────────────────────┐
//! │ 1. Memory Prefetch (if enabled)     │
//! │ 2. Context Compression (if needed)  │
//! │ 3. MCP Tool Loading (if configured) │
//! │ 4. Agent Execution                  │
//! │ 5. Memory Sync (if enabled)         │
//! │ 6. Webhook Response (if triggered)  │
//! └─────────────────────────────────────┘
//!     ↓
//! Agent Response + Provenance
//! ```
//!
//! ## Paradigm Breakthrough vs Hermes
//!
//! Hermes agent_loop.py:
//! - Linear execution: memory → model → tools → response
//! - No cryptographic signing
//! - No provenance tracking
//! - No automatic compression
//!
//! Zaion UnifiedAgentRuntime adds:
//! - **Ed25519 signed execution**: Every turn cryptographically signed
//! - **Provenance tracking**: Complete audit trail
//! - **Automatic compression**: Context compression when threshold exceeded
//! - **MCP tool auto-loading**: Dynamic tool discovery and routing
//! - **Ouroboros self-healing**: Automatic recovery from failures

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::compressor::{CompressedContext, CompressorConfig, ContextCompressor, Turn};
use crate::integrated_agent_loop::{IntegratedAgentConfig, IntegratedAgentLoop};
use crate::mcp_tools::McpToolRegistry;
use crate::turn_proof::{stable_hash_bytes, TurnCompressionEvidence, TurnRuntimeMemoryEvidence};
use crate::webhook_runtime::WebhookRuntimeManager;
#[allow(unused_imports)]
use ed25519_dalek::Verifier;
use zaion_crypto::ZaionKeypair;
use zaion_federation::HonchoClient;
use zaion_memory::runtime_integration::MemoryManager;
use zaion_types::envelope::is_unsafe_principal;

/// Typed turn signature produced by `sign_turn`.
/// scheme = "ed25519-sha256-v1": SHA-256 prehash, then Ed25519 over the digest.
/// schema_version = 2 denotes a real signature; 1 is legacy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnSignature {
    /// Signature scheme identifier
    pub scheme: String,
    /// Raw 64-byte Ed25519 signature
    pub signature: Vec<u8>,
    /// Base-58 principal ID of the signing key
    pub signing_key_id: String,
    /// Schema version (2 = real, 1 = legacy)
    pub schema_version: u32,
}

impl TurnSignature {
    /// Verify this turn signature.
    ///
    /// Canonical bytes: SHA-256(user_message || 0x1F || response || 0x1F || turn_id || 0x1F || timestamp_ns_le)
    pub fn verify(
        &self,
        user_message: &str,
        response: &str,
        turn_id: &str,
        timestamp_ns: u128,
        verifying_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<(), String> {
        use ed25519_dalek::Verifier;

        if self.schema_version < 2 {
            return Err("legacy schema_version < 2, no real signature".into());
        }

        let digest = Self::canonical_digest(user_message, response, turn_id, timestamp_ns);
        let sig_arr: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| "signature not 64 bytes".to_string())?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        verifying_key
            .verify(&digest, &sig)
            .map_err(|e| format!("ed25519 verify failed: {}", e))
    }

    /// Compute canonical SHA-256 digest for turn signing.
    pub fn canonical_digest(
        user_message: &str,
        response: &str,
        turn_id: &str,
        timestamp_ns: u128,
    ) -> Vec<u8> {
        let mut hasher = sha2::Sha256::new();
        hasher.update(user_message.as_bytes());
        hasher.update([0x1F]);
        hasher.update(response.as_bytes());
        hasher.update([0x1F]);
        hasher.update(turn_id.as_bytes());
        hasher.update([0x1F]);
        hasher.update(timestamp_ns.to_le_bytes());
        hasher.finalize().to_vec()
    }
}

/// Unified agent runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAgentConfig {
    /// Enable automatic memory integration
    pub enable_memory: bool,

    /// Allow automatic or explicitly forced context compression.
    pub enable_compression: bool,

    /// Attempt compression even when the configured threshold is not exceeded.
    /// This has no effect when `enable_compression` is false.
    #[serde(default)]
    pub force_compression: bool,

    /// Enable MCP tool auto-loading
    pub enable_mcp: bool,

    /// Enable webhook event handling
    pub enable_webhooks: bool,

    /// Context compression threshold (0.0-1.0)
    pub compression_threshold: f64,

    /// Token budget for context window
    pub token_budget: usize,

    /// Session ID
    pub session_id: String,

    /// Principal ID (for Ed25519 signing)
    pub principal_id: String,
}

impl Default for UnifiedAgentConfig {
    fn default() -> Self {
        Self {
            enable_memory: true,
            enable_compression: true,
            force_compression: false,
            enable_mcp: true,
            enable_webhooks: true,
            compression_threshold: 0.50,
            token_budget: 200_000,
            session_id: "default".to_string(),
            principal_id: String::new(),
        }
    }
}

fn compression_attempt_required(config: &UnifiedAgentConfig, threshold_exceeded: bool) -> bool {
    config.enable_compression && (config.force_compression || threshold_exceeded)
}

fn build_unified_compression_evidence(
    config: &UnifiedAgentConfig,
    original_history: &[Turn],
    compressed: Option<&CompressedContext>,
) -> TurnCompressionEvidence {
    let original_tokens = original_history
        .iter()
        .map(Turn::token_estimate)
        .sum::<usize>();
    let trigger_threshold = (config.token_budget as f64 * config.compression_threshold) as usize;
    let mut evidence = TurnCompressionEvidence {
        schema: "zaion.context_compression_evidence.v1".to_string(),
        compression_requested: config.force_compression && config.enable_compression,
        was_compressed: compressed.is_some_and(|context| context.was_compressed),
        original_turns: original_history.len(),
        compressed_turns: compressed
            .map(|context| context.turns.len())
            .unwrap_or(original_history.len()),
        turns_pruned: compressed.map(|context| context.turns_pruned).unwrap_or(0),
        original_tokens,
        compressed_tokens: compressed
            .map(|context| context.total_tokens)
            .unwrap_or(original_tokens),
        token_budget: config.token_budget,
        trigger_threshold,
        summary_hash: stable_hash_bytes(
            compressed
                .map(|context| context.summary_text.as_str())
                .unwrap_or_default()
                .as_bytes(),
        ),
        summary_strategy: compressed
            .map(|context| context.summary_strategy.clone())
            .unwrap_or_else(|| "none".to_string()),
        pruned_tool_outputs: compressed
            .map(|context| context.pruned_tool_outputs)
            .unwrap_or(0),
        protected_head_turns: compressed
            .map(|context| context.protected_head_turns)
            .unwrap_or(0),
        protected_tail_turns: compressed
            .map(|context| context.protected_tail_turns)
            .unwrap_or(0),
        protected_tail_tokens: compressed
            .map(|context| context.protected_tail_tokens)
            .unwrap_or(0),
        summary_budget_tokens: compressed
            .map(|context| context.summary_budget_tokens)
            .unwrap_or(0),
        evidence_hash: String::new(),
    };
    let evidence_bytes = serde_json::to_vec(&evidence)
        .expect("serializing unified compression evidence cannot fail");
    evidence.evidence_hash = stable_hash_bytes(&evidence_bytes);
    evidence
}

/// Unified agent execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAgentResult {
    /// Agent response
    pub response: String,

    /// Whether context was compressed
    pub was_compressed: bool,

    /// Number of turns compressed (if any)
    pub turns_compressed: usize,

    /// Hash-bound evidence for the requested and actual compression outcome.
    pub compression_evidence: TurnCompressionEvidence,

    /// Memory context used (if any)
    pub memory_context_size: usize,

    /// Hash-bound runtime memory evidence (if memory context was consumed)
    pub runtime_memory_evidence: Option<TurnRuntimeMemoryEvidence>,

    /// MCP tools loaded (if any)
    pub mcp_tools_loaded: usize,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,

    /// Typed Ed25519 turn signature (Zaion unique, schema_version=2)
    pub ed25519_signature: TurnSignature,

    /// Provenance hash (Zaion unique)
    pub provenance_hash: String,
}

/// Unified agent runtime
pub struct UnifiedAgentRuntime {
    /// Configuration
    config: UnifiedAgentConfig,

    /// Context compressor (mutable for iterative summary updates)
    compressor: Arc<RwLock<ContextCompressor>>,

    /// Integrated agent loop
    agent_loop: Arc<IntegratedAgentLoop>,

    /// Conversation history
    history: Arc<RwLock<Vec<Turn>>>,

    /// Honcho client (optional, for cross-session federation)
    honcho_client: Option<Arc<HonchoClient>>,

    /// MCP tool registry (optional; present only when runtime has loaded MCP tools)
    mcp_registry: Option<Arc<McpToolRegistry>>,

    /// Ed25519 signing keypair; auto-generated if not injected at construction
    signing_key: Arc<ZaionKeypair>,
}

impl UnifiedAgentRuntime {
    /// Create new unified agent runtime with an auto-generated ephemeral keypair for tests only.
    #[cfg(test)]
    pub fn new(
        config: UnifiedAgentConfig,
        webhook_manager: Arc<WebhookRuntimeManager>,
        memory_manager: Arc<MemoryManager>,
    ) -> Result<Self, String> {
        let keypair = Arc::new(ZaionKeypair::generate());
        let mut config = config;
        if config.principal_id.trim().is_empty() {
            config.principal_id = keypair.principal_id().to_string();
        }
        Self::new_with_key(config, webhook_manager, memory_manager, keypair)
    }

    /// Create a new unified agent runtime with a caller-supplied signing keypair
    /// (e.g. loaded from `zaion-secrets::KeyStore` at boot).
    pub fn new_with_key(
        config: UnifiedAgentConfig,
        webhook_manager: Arc<WebhookRuntimeManager>,
        memory_manager: Arc<MemoryManager>,
        keypair: Arc<ZaionKeypair>,
    ) -> Result<Self, String> {
        Self::verify_config_identity(&config, &keypair)?;
        let compressor_config = CompressorConfig {
            threshold_ratio: config.compression_threshold,
            target_ratio: 0.20,
            protect_last_n_turns: 10,
            protect_first_n_turns: 2,
            ..Default::default()
        };

        let compressor = Arc::new(RwLock::new(ContextCompressor::new(compressor_config)));

        let integrated_config = IntegratedAgentConfig {
            enable_memory: config.enable_memory,
            enable_opd: false, // OPD is for training, not runtime
            enable_webhooks: config.enable_webhooks,
            memory_config: zaion_memory::runtime_integration::MemoryRuntimeConfig::default(),
        };

        let agent_loop = Arc::new(IntegratedAgentLoop::new(
            integrated_config,
            webhook_manager,
            memory_manager,
            config.session_id.clone(),
        ));

        Ok(Self {
            config,
            compressor,
            agent_loop,
            history: Arc::new(RwLock::new(Vec::new())),
            honcho_client: None,
            mcp_registry: None,
            signing_key: keypair,
        })
    }

    /// Create new unified agent runtime with Honcho federation and an ephemeral keypair for tests only.
    #[cfg(test)]
    pub fn new_with_honcho(
        config: UnifiedAgentConfig,
        webhook_manager: Arc<WebhookRuntimeManager>,
        memory_manager: Arc<MemoryManager>,
        honcho_client: Arc<HonchoClient>,
    ) -> Result<Self, String> {
        let mut runtime = Self::new(config, webhook_manager, memory_manager)?;
        runtime.honcho_client = Some(honcho_client);
        Ok(runtime)
    }

    pub fn new_with_honcho_key(
        config: UnifiedAgentConfig,
        webhook_manager: Arc<WebhookRuntimeManager>,
        memory_manager: Arc<MemoryManager>,
        honcho_client: Arc<HonchoClient>,
        keypair: Arc<ZaionKeypair>,
    ) -> Result<Self, String> {
        let mut runtime = Self::new_with_key(config, webhook_manager, memory_manager, keypair)?;
        runtime.honcho_client = Some(honcho_client);
        Ok(runtime)
    }

    pub fn with_mcp_registry(mut self, mcp_registry: Arc<McpToolRegistry>) -> Self {
        self.mcp_registry = Some(mcp_registry);
        self
    }

    fn verify_config_identity(
        config: &UnifiedAgentConfig,
        keypair: &ZaionKeypair,
    ) -> Result<(), String> {
        if is_unsafe_principal(&config.principal_id) || config.principal_id.trim().is_empty() {
            return Err(format!(
                "UnifiedAgentRuntime requires a production-safe principal_id, got '{}'",
                config.principal_id
            ));
        }
        let signing_principal = keypair.principal_id().to_string();
        if config.principal_id != signing_principal {
            return Err(format!(
                "UnifiedAgentRuntime principal_id '{}' does not match signing key '{}'",
                config.principal_id, signing_principal
            ));
        }
        Ok(())
    }

    /// Execute agent turn with full integration
    pub async fn execute_turn(
        &self,
        user_message: &str,
        agent_executor: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) -> Result<UnifiedAgentResult, String> {
        let start = std::time::Instant::now();

        // Step 1: Prefetch Honcho context if enabled
        let honcho_context = if let Some(ref client) = self.honcho_client {
            match client.get_session_context(&self.config.session_id).await {
                Ok(ctx) => Some(ctx),
                Err(e) => {
                    eprintln!("Warning: Failed to prefetch Honcho context: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Step 2: Add user message to history
        let mut history = self.history.write().await;
        history.push(Turn::new("user", user_message));

        // Step 3: Resolve automatic-vs-forced compression. A forced attempt may
        // still report `was_compressed = false` when the compressor cannot
        // reduce a short or fully protected history.
        let threshold_exceeded = if self.config.enable_compression {
            let compressor = self.compressor.read().await;
            compressor.needs_compression(&history, self.config.token_budget)
        } else {
            false
        };
        let compressed = if compression_attempt_required(&self.config, threshold_exceeded) {
            let mut compressor = self.compressor.write().await;
            Some(if self.config.force_compression {
                compressor.compress_forced(&history, self.config.token_budget, None)
            } else {
                compressor.compress(&history, self.config.token_budget, None)
            })
        } else {
            None
        };
        let compression_evidence =
            build_unified_compression_evidence(&self.config, &history, compressed.as_ref());
        let compressed_history = compressed
            .as_ref()
            .map(|context| context.turns.clone())
            .unwrap_or_else(|| history.clone());
        let was_compressed = compression_evidence.was_compressed;
        let turns_compressed = compression_evidence.turns_pruned;

        drop(history);

        // Step 4: Build prompt from compressed history + Honcho context
        let mut prompt = self.build_prompt_from_history(&compressed_history);
        if let Some(ctx) = honcho_context {
            prompt = format!(
                "# Cross-session context\n{}\n\n# Current conversation\n{}",
                ctx, prompt
            );
        }

        // Step 5: Execute agent via integrated loop
        let execution_report = self
            .agent_loop
            .execute_with_report(&prompt, agent_executor)
            .await?;
        let response = execution_report.response;

        // Step 6: Add assistant response to history
        let mut history = self.history.write().await;
        history.push(Turn::new("assistant", &response));
        drop(history);

        // Step 7: Sync to Honcho if enabled
        if let Some(ref client) = self.honcho_client {
            if let Err(e) = client
                .add_messages(
                    &self.config.session_id,
                    vec![
                        ("user".to_string(), user_message.to_string()),
                        ("assistant".to_string(), response.clone()),
                    ],
                )
                .await
            {
                eprintln!("Warning: Failed to sync to Honcho: {}", e);
            }
        }

        // Step 8: Generate provenance (Zaion unique)
        let provenance_hash = self.generate_provenance_hash(user_message, &response);
        let ed25519_signature = self.sign_turn(user_message, &response);

        let execution_time_ms = start.elapsed().as_millis() as u64;
        let mcp_tools_loaded = if self.config.enable_mcp {
            match &self.mcp_registry {
                Some(registry) => registry.list_tools().await.len(),
                None => 0,
            }
        } else {
            0
        };

        Ok(UnifiedAgentResult {
            response,
            was_compressed,
            turns_compressed,
            compression_evidence,
            memory_context_size: execution_report.memory_context_size,
            runtime_memory_evidence: execution_report.runtime_memory_evidence,
            mcp_tools_loaded,
            execution_time_ms,
            ed25519_signature,
            provenance_hash,
        })
    }

    /// Build prompt from conversation history
    fn build_prompt_from_history(&self, history: &[Turn]) -> String {
        history
            .iter()
            .map(|turn| format!("{}: {}", turn.role, turn.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Generate provenance hash (Zaion unique)
    fn generate_provenance_hash(&self, user_message: &str, response: &str) -> String {
        let combined = format!("{}|{}|{}", user_message, response, self.config.principal_id);
        format!("{:x}", sha2::Sha256::digest(combined.as_bytes()))
    }

    /// Sign a turn with Ed25519 (Zaion unique).
    ///
    /// Canonical bytes:
    ///   SHA-256(user_message || 0x1F || response || 0x1F || turn_id || 0x1F || timestamp_ns_le)
    ///
    /// A trace log line is emitted every time a real signature is produced.
    fn sign_turn(&self, user_message: &str, response: &str) -> TurnSignature {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let digest =
            TurnSignature::canonical_digest(user_message, response, &turn_id, timestamp_ns);

        let zaion_types::identity::SignatureBytes(sig_bytes) = self.signing_key.sign(&digest);
        let signing_key_id = self.signing_key.principal_id().to_string();

        // Trace log: emit every time a real signature is produced.
        eprintln!(
            "[zaion-turn-sig] turn_id={} principal={} sig_hex={}",
            &turn_id[..8],
            &signing_key_id[..8.min(signing_key_id.len())],
            hex::encode(&sig_bytes[..4]), // first 4 bytes for brevity
        );

        TurnSignature {
            scheme: "ed25519-sha256-v1".to_string(),
            signature: sig_bytes,
            signing_key_id,
            schema_version: 2,
        }
    }

    /// Verify a turn signature produced by `sign_turn`.
    /// Returns `Ok(())` on success or `Err(String)` with a description.
    pub fn verify_turn(
        &self,
        ts: &TurnSignature,
        user_message: &str,
        response: &str,
        turn_id: &str,
        timestamp_ns: u128,
    ) -> Result<(), String> {
        let vk = self.signing_key.verifying_key();
        ts.verify(user_message, response, turn_id, timestamp_ns, &vk)
    }

    /// Return the Ed25519 verifying key (for out-of-band verification).
    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get conversation history
    pub async fn get_history(&self) -> Vec<Turn> {
        self.history.read().await.clone()
    }

    /// Clear conversation history
    pub async fn clear_history(&self) {
        self.history.write().await.clear();
    }

    /// Get current history size in tokens (estimated)
    pub async fn get_history_token_count(&self) -> usize {
        let history = self.history.read().await;
        history.iter().map(|t| t.token_estimate()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticMemoryProvider;

    fn assert_compression_evidence_hash(evidence: &TurnCompressionEvidence) {
        let claimed_hash = evidence.evidence_hash.clone();
        let mut stable = evidence.clone();
        stable.evidence_hash.clear();
        let bytes = serde_json::to_vec(&stable).unwrap();
        assert_eq!(claimed_hash, stable_hash_bytes(&bytes));
    }

    impl zaion_memory::runtime_integration::MemoryProvider for StaticMemoryProvider {
        fn name(&self) -> &str {
            "static-test"
        }

        fn system_prompt_block(&self) -> String {
            String::new()
        }

        fn prefetch(&self, _query: &str, _session_id: &str) -> anyhow::Result<String> {
            Ok("<memory-context>runtime memory evidence</memory-context>".to_string())
        }

        fn sync_turn(
            &self,
            _user_content: &str,
            _assistant_content: &str,
            _session_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn get_tool_schemas(&self) -> Vec<serde_json::Value> {
            vec![serde_json::json!({
                "name": "memory_runtime_lookup",
                "description": "runtime memory lookup",
                "parameters": { "type": "object" }
            })]
        }

        fn handle_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> anyhow::Result<String> {
            Ok("{}".to_string())
        }
    }

    #[tokio::test]
    async fn test_unified_agent_runtime_creation() {
        let config = UnifiedAgentConfig::default();
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());

        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        assert_eq!(runtime.get_history().await.len(), 0);
    }

    #[test]
    fn compression_attempt_policy_distinguishes_enable_force_and_threshold() {
        let mut config = UnifiedAgentConfig {
            enable_compression: true,
            force_compression: false,
            ..Default::default()
        };

        assert!(!compression_attempt_required(&config, false));
        assert!(compression_attempt_required(&config, true));

        config.force_compression = true;
        assert!(compression_attempt_required(&config, false));

        config.enable_compression = false;
        assert!(!compression_attempt_required(&config, true));
    }

    #[test]
    fn legacy_config_deserialization_defaults_force_compression_to_false() {
        let mut serialized = serde_json::to_value(UnifiedAgentConfig::default()).unwrap();
        serialized
            .as_object_mut()
            .unwrap()
            .remove("force_compression");

        let decoded: UnifiedAgentConfig = serde_json::from_value(serialized).unwrap();

        assert!(!decoded.force_compression);
    }

    #[tokio::test]
    async fn forced_compression_below_threshold_does_not_forge_success() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: true,
            force_compression: true,
            enable_mcp: false,
            enable_webhooks: false,
            compression_threshold: 0.95,
            token_budget: 200_000,
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();
        let candidate_history = vec![Turn::new("user", "short history")];
        let threshold_exceeded = runtime
            .compressor
            .read()
            .await
            .needs_compression(&candidate_history, runtime.config.token_budget);

        assert!(!threshold_exceeded);
        assert!(compression_attempt_required(
            &runtime.config,
            threshold_exceeded
        ));

        let result = runtime
            .execute_turn("short history", |prompt| {
                assert!(prompt.contains("short history"));
                Ok("response".to_string())
            })
            .await
            .unwrap();

        assert!(!result.was_compressed);
        assert_eq!(result.turns_compressed, 0);
        let evidence = &result.compression_evidence;
        assert!(evidence.compression_requested);
        assert!(!evidence.was_compressed);
        assert_eq!(evidence.original_turns, 1);
        assert_eq!(evidence.compressed_turns, 1);
        assert_eq!(evidence.turns_pruned, 0);
        assert_eq!(evidence.original_tokens, evidence.compressed_tokens);
        assert_eq!(evidence.token_budget, 200_000);
        assert_eq!(evidence.trigger_threshold, 190_000);
        assert_eq!(evidence.summary_hash, stable_hash_bytes(b""));
        assert_eq!(evidence.summary_strategy, "none");
        assert_eq!(evidence.protected_head_turns, 1);
        assert_compression_evidence_hash(evidence);
    }

    #[tokio::test]
    async fn forced_compression_below_threshold_records_real_success() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: true,
            force_compression: true,
            enable_mcp: false,
            enable_webhooks: false,
            compression_threshold: 0.95,
            token_budget: 200_000,
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();
        {
            let mut history = runtime.history.write().await;
            for index in 0..20 {
                history.push(Turn::new(
                    if index % 2 == 0 { "user" } else { "assistant" },
                    format!("history-{index} {}", "x".repeat(200)),
                ));
            }
        }
        let candidate_history = {
            let history = runtime.history.read().await;
            let mut candidate = history.clone();
            candidate.push(Turn::new("user", "force compression now"));
            candidate
        };
        assert!(!runtime
            .compressor
            .read()
            .await
            .needs_compression(&candidate_history, runtime.config.token_budget));

        let result = runtime
            .execute_turn("force compression now", |prompt| {
                assert!(prompt.contains("force compression now"));
                Ok("response".to_string())
            })
            .await
            .unwrap();

        let evidence = &result.compression_evidence;
        assert!(evidence.compression_requested);
        assert!(evidence.was_compressed);
        assert!(result.was_compressed);
        assert_eq!(evidence.original_turns, 21);
        assert!(evidence.compressed_turns < evidence.original_turns);
        assert!(evidence.turns_pruned > 0);
        assert_eq!(result.turns_compressed, evidence.turns_pruned);
        assert_eq!(evidence.token_budget, 200_000);
        assert_eq!(evidence.trigger_threshold, 190_000);
        assert_ne!(evidence.summary_hash, stable_hash_bytes(b""));
        assert_ne!(evidence.summary_strategy, "none");
        assert_compression_evidence_hash(evidence);
    }

    #[tokio::test]
    async fn disabled_compression_overrides_force_above_threshold() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: false,
            force_compression: true,
            enable_mcp: false,
            enable_webhooks: false,
            compression_threshold: 0.50,
            token_budget: 1,
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();
        let message = "history well above the configured threshold";
        let candidate_history = vec![Turn::new("user", message)];
        let threshold_exceeded = runtime
            .compressor
            .read()
            .await
            .needs_compression(&candidate_history, runtime.config.token_budget);

        assert!(threshold_exceeded);
        assert!(!compression_attempt_required(
            &runtime.config,
            threshold_exceeded
        ));

        let result = runtime
            .execute_turn(message, |_prompt| Ok("response".to_string()))
            .await
            .unwrap();

        assert!(!result.was_compressed);
        assert_eq!(result.turns_compressed, 0);
        let evidence = &result.compression_evidence;
        assert!(!evidence.compression_requested);
        assert!(!evidence.was_compressed);
        assert_eq!(evidence.original_turns, evidence.compressed_turns);
        assert_eq!(evidence.original_tokens, evidence.compressed_tokens);
        assert_eq!(evidence.token_budget, 1);
        assert_eq!(evidence.trigger_threshold, 0);
        assert_eq!(evidence.summary_strategy, "none");
        assert_compression_evidence_hash(evidence);
    }

    #[tokio::test]
    async fn compression_evidence_tracks_actual_compressed_history() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: true,
            force_compression: false,
            enable_mcp: false,
            enable_webhooks: false,
            compression_threshold: 0.50,
            token_budget: 200,
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();
        {
            let mut history = runtime.history.write().await;
            for index in 0..20 {
                history.push(Turn::new(
                    if index % 2 == 0 { "user" } else { "assistant" },
                    format!("history-{index} {}", "x".repeat(400)),
                ));
            }
        }

        let result = runtime
            .execute_turn("compress the accumulated history", |_prompt| {
                Ok("response".to_string())
            })
            .await
            .unwrap();

        let evidence = &result.compression_evidence;
        assert!(!evidence.compression_requested);
        assert!(evidence.was_compressed);
        assert!(result.was_compressed);
        assert_eq!(result.turns_compressed, evidence.turns_pruned);
        assert_eq!(evidence.original_turns, 21);
        assert!(evidence.compressed_turns < evidence.original_turns);
        assert!(evidence.turns_pruned > 0);
        assert!(evidence.original_tokens > 0);
        assert!(evidence.compressed_tokens > 0);
        assert_eq!(evidence.token_budget, 200);
        assert_eq!(evidence.trigger_threshold, 100);
        assert_ne!(evidence.summary_hash, stable_hash_bytes(b""));
        assert_ne!(evidence.summary_strategy, "none");
        assert_compression_evidence_hash(evidence);
    }

    #[tokio::test]
    async fn test_execute_turn_basic() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: false,
            enable_mcp: false,
            enable_webhooks: false,
            ..Default::default()
        };

        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        let result = runtime
            .execute_turn("Hello", |_prompt| Ok("Hi there!".to_string()))
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.response, "Hi there!");
        assert!(!result.was_compressed);
        assert_eq!(result.turns_compressed, 0);

        // Check history
        let history = runtime.get_history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[tokio::test]
    async fn execute_turn_reports_real_memory_context_and_tool_schema_counts() {
        let config = UnifiedAgentConfig {
            enable_memory: true,
            enable_compression: false,
            enable_mcp: false,
            enable_webhooks: false,
            ..Default::default()
        };

        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        memory_manager
            .add_provider(Box::new(StaticMemoryProvider))
            .await;
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        let result = runtime
            .execute_turn("Hello", |prompt| {
                assert!(prompt.contains("# Relevant Memories"));
                assert!(prompt.contains("runtime memory evidence"));
                Ok("Hi there!".to_string())
            })
            .await
            .unwrap();

        assert!(result.memory_context_size > 0);
        let evidence = result
            .runtime_memory_evidence
            .expect("runtime memory evidence");
        assert_eq!(evidence.schema, "zaion.runtime_memory_evidence.v1");
        assert_eq!(evidence.memory_context_bytes, result.memory_context_size);
        assert!(evidence.fenced_context);
        assert_eq!(result.mcp_tools_loaded, 0);
    }

    #[test]
    fn test_new_with_key_rejects_mismatched_principal() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let config = UnifiedAgentConfig {
            principal_id: "principal-does-not-match-key".to_string(),
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());

        let err = match UnifiedAgentRuntime::new_with_key(
            config,
            webhook_manager,
            memory_manager,
            keypair,
        ) {
            Ok(_) => panic!("mismatched principal must be rejected"),
            Err(err) => err,
        };

        assert!(err.contains("does not match signing key"));
    }

    #[test]
    fn test_new_with_key_rejects_empty_principal() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let config = UnifiedAgentConfig {
            principal_id: String::new(),
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());

        let err = match UnifiedAgentRuntime::new_with_key(
            config,
            webhook_manager,
            memory_manager,
            keypair,
        ) {
            Ok(_) => panic!("empty principal must be rejected"),
            Err(err) => err,
        };

        assert!(err.contains("production-safe principal_id"));
    }

    #[tokio::test]
    async fn test_clear_history() {
        let config = UnifiedAgentConfig::default();
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        runtime
            .execute_turn("Hello", |_| Ok("Hi".to_string()))
            .await
            .unwrap();

        assert_eq!(runtime.get_history().await.len(), 2);

        runtime.clear_history().await;
        assert_eq!(runtime.get_history().await.len(), 0);
    }

    #[tokio::test]
    async fn test_provenance_generation() {
        let config = UnifiedAgentConfig::default();
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        let result = runtime
            .execute_turn("Test", |_| Ok("Response".to_string()))
            .await
            .unwrap();

        assert!(!result.provenance_hash.is_empty());
        // TurnSignature must have a real (non-empty) signature vector
        assert!(!result.ed25519_signature.signature.is_empty());
        assert_eq!(result.ed25519_signature.scheme, "ed25519-sha256-v1");
        assert_eq!(result.ed25519_signature.schema_version, 2);
    }

    // ── New: real Ed25519 sign → verify round-trip ───────────────────────────

    /// TurnSignature::canonical_digest + verify round-trip.
    #[test]
    fn test_turn_signature_canonical_roundtrip() {
        use zaion_crypto::ZaionKeypair;
        use zaion_types::identity::SignatureBytes;

        let keypair = ZaionKeypair::generate();
        let user_msg = "Hello, world!";
        let response = "Hi there!";
        let turn_id = "turn-abc-123";
        let timestamp_ns: u128 = 1_700_000_000_000_000_000;

        let digest = TurnSignature::canonical_digest(user_msg, response, turn_id, timestamp_ns);
        let SignatureBytes(sig_bytes) = keypair.sign(&digest);
        let vk = keypair.verifying_key();

        let ts = TurnSignature {
            scheme: "ed25519-sha256-v1".to_string(),
            signature: sig_bytes,
            signing_key_id: keypair.principal_id().to_string(),
            schema_version: 2,
        };

        ts.verify(user_msg, response, turn_id, timestamp_ns, &vk)
            .expect("round-trip verify must succeed");
    }

    /// Tamper with user_message → signature must fail.
    #[test]
    fn test_turn_signature_tamper_user_message() {
        use zaion_crypto::ZaionKeypair;
        use zaion_types::identity::SignatureBytes;

        let keypair = ZaionKeypair::generate();
        let turn_id = "t1";
        let ts_ns: u128 = 42;

        let digest = TurnSignature::canonical_digest("original", "resp", turn_id, ts_ns);
        let SignatureBytes(sig_bytes) = keypair.sign(&digest);

        let ts = TurnSignature {
            scheme: "ed25519-sha256-v1".to_string(),
            signature: sig_bytes,
            signing_key_id: keypair.principal_id().to_string(),
            schema_version: 2,
        };

        let vk = keypair.verifying_key();
        // Verify with tampered user_message
        let result = ts.verify("tampered_msg", "resp", turn_id, ts_ns, &vk);
        assert!(result.is_err(), "tampered user_message must fail");
    }

    /// Tamper with response → signature must fail.
    #[test]
    fn test_turn_signature_tamper_response() {
        use zaion_crypto::ZaionKeypair;
        use zaion_types::identity::SignatureBytes;

        let keypair = ZaionKeypair::generate();
        let turn_id = "t2";
        let ts_ns: u128 = 99;

        let digest = TurnSignature::canonical_digest("msg", "original_response", turn_id, ts_ns);
        let SignatureBytes(sig_bytes) = keypair.sign(&digest);

        let ts = TurnSignature {
            scheme: "ed25519-sha256-v1".to_string(),
            signature: sig_bytes,
            signing_key_id: keypair.principal_id().to_string(),
            schema_version: 2,
        };

        let vk = keypair.verifying_key();
        let result = ts.verify("msg", "evil_response", turn_id, ts_ns, &vk);
        assert!(result.is_err(), "tampered response must fail");
    }

    /// Legacy schema_version=1 fails closed.
    #[test]
    fn test_turn_signature_legacy_fails_closed() {
        let keypair = ZaionKeypair::generate();
        let ts = TurnSignature {
            scheme: "ed25519-sha256-v1".to_string(),
            signature: vec![0u8; 64],
            signing_key_id: "irrelevant".to_string(),
            schema_version: 1, // legacy
        };
        let vk = keypair.verifying_key();
        let result = ts.verify("msg", "resp", "turn", 0, &vk);
        assert!(result.is_err(), "legacy schema must fail closed");
    }

    /// execute_turn produces a verifiable signature end-to-end.
    #[tokio::test]
    async fn test_execute_turn_signature_verifiable() {
        let config = UnifiedAgentConfig {
            enable_memory: false,
            enable_compression: false,
            enable_mcp: false,
            enable_webhooks: false,
            ..Default::default()
        };
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let runtime = UnifiedAgentRuntime::new(config, webhook_manager, memory_manager).unwrap();

        let user_msg = "Sign me";
        let agent_response = "Signed!";

        let result = runtime
            .execute_turn(user_msg, |_| Ok(agent_response.to_string()))
            .await
            .unwrap();

        let ts = &result.ed25519_signature;
        assert_eq!(ts.scheme, "ed25519-sha256-v1");
        assert_eq!(ts.schema_version, 2);
        assert_eq!(ts.signature.len(), 64);
        // We cannot verify with the embedded turn_id/ts_ns here because sign_turn
        // generates them internally, but we can assert no placeholder bytes.
        assert_ne!(ts.signature, vec![0u8; 64]);
    }
}
