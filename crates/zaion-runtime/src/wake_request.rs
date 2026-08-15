//! Runtime-owned input contract for one canonical wake turn.
//!
//! Surface adapters may add source-specific validation, but they all hand the
//! same structured request to the turn kernel. Keeping this type in
//! `zaion-runtime` prevents CLI, TUI, HTTP, Telegram, MCP, and ACP adapters from
//! defining parallel request shapes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zaion_types::envelope::CanonicalEnvelope;

/// Product/config defaults applied before request-level feature overrides.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WakeFeatureDefaults {
    pub cache_enabled: bool,
    pub memory_enabled: bool,
    pub mcp_enabled: bool,
    pub smart_route_enabled: bool,
    pub compression_enabled: bool,
    pub webhooks_enabled: bool,
}

/// Effective, contradiction-free feature policy for one wake turn.
///
/// Where available, explicit disable flags win over defaults and enable flags.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WakeFeaturePolicy {
    pub cache_enabled: bool,
    pub memory_enabled: bool,
    pub mcp_enabled: bool,
    pub smart_route_enabled: bool,
    pub compression_enabled: bool,
    pub compression_requested: bool,
    pub webhooks_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WakeRequest {
    pub pid: String,
    pub message: String,
    pub extra_model_context: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub stream: bool,
    pub enable_cache: bool,
    pub enable_memory: bool,
    pub enable_mcp: bool,
    pub smart_route: bool,
    pub compress: bool,
    pub unified: bool,
    pub disable_memory: bool,
    pub disable_mcp: bool,
    pub disable_compression: bool,
    pub disable_webhooks: bool,
    /// Opt into the staged authenticated-ingress/state/tool-broker contract.
    ///
    /// This remains false by default while production surfaces migrate one at
    /// a time. The CLI may also enable it through its deployment feature flag.
    pub turn_contract_v2: bool,
    pub parser: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub source: Option<String>,
    pub source_message_id: Option<String>,
    pub source_hash: Option<String>,
    pub envelope: Option<CanonicalEnvelope>,
    pub tool_result_storage_root: Option<PathBuf>,
    pub tool_result_environment_id: Option<String>,
    pub tool_result_environment_kind: Option<String>,
}

impl WakeRequest {
    pub fn new(pid: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pid: pid.into(),
            message: message.into(),
            extra_model_context: Vec::new(),
            ..Default::default()
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn with_mcp(mut self, enable: bool) -> Self {
        self.enable_mcp = enable;
        self.disable_mcp = !enable;
        self
    }

    pub fn with_memory(mut self, enable: bool) -> Self {
        self.enable_memory = enable;
        self.disable_memory = !enable;
        self
    }

    pub fn with_turn_contract_v2(mut self, enable: bool) -> Self {
        self.turn_contract_v2 = enable;
        self
    }

    pub fn effective_features(&self, defaults: WakeFeatureDefaults) -> WakeFeaturePolicy {
        WakeFeaturePolicy {
            cache_enabled: defaults.cache_enabled || self.enable_cache,
            memory_enabled: (defaults.memory_enabled || self.enable_memory) && !self.disable_memory,
            mcp_enabled: (defaults.mcp_enabled || self.enable_mcp) && !self.disable_mcp,
            smart_route_enabled: defaults.smart_route_enabled || self.smart_route,
            compression_enabled: (defaults.compression_enabled || self.compress)
                && !self.disable_compression,
            compression_requested: self.compress && !self.disable_compression,
            webhooks_enabled: defaults.webhooks_enabled && !self.disable_webhooks,
        }
    }

    pub fn with_envelope(mut self, envelope: CanonicalEnvelope) -> Self {
        self.pid = envelope.principal.as_str().to_string();
        self.message = envelope.body.clone();
        self.channel_id = Some(envelope.channel.0.clone());
        self.thread_id = Some(envelope.thread.0.clone());
        self.source = Some(envelope.source.clone());
        self.source_message_id = Some(envelope.message_id.clone());
        self.source_hash = Some(envelope.source_hash.clone());
        self.envelope = Some(envelope);
        self
    }

    pub fn with_tool_result_storage_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.tool_result_storage_root = Some(root.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_types::identity::PrincipalId;
    use zaion_types::session::{ChannelId, ThreadId};

    #[test]
    fn request_builders_preserve_runtime_turn_options() {
        let request = WakeRequest::new("did:key:runtime", "hello")
            .with_provider("openai")
            .with_model("gpt-5.5")
            .streaming()
            .with_memory(true)
            .with_mcp(true)
            .with_turn_contract_v2(true)
            .with_tool_result_storage_root(".zaion/tool-results");

        assert_eq!(request.pid, "did:key:runtime");
        assert_eq!(request.message, "hello");
        assert_eq!(request.provider.as_deref(), Some("openai"));
        assert_eq!(request.model.as_deref(), Some("gpt-5.5"));
        assert!(request.stream);
        assert!(request.enable_memory);
        assert!(request.enable_mcp);
        assert!(request.turn_contract_v2);
        assert_eq!(
            request.tool_result_storage_root,
            Some(PathBuf::from(".zaion/tool-results"))
        );
    }

    #[test]
    fn canonical_envelope_is_the_authority_for_ingress_identity() {
        let envelope = CanonicalEnvelope::new(
            "telegram",
            PrincipalId("did:key:canonical".to_string()),
            ChannelId("telegram:42".to_string()),
            ThreadId("topic:7".to_string()),
            "message-1",
            "canonical body",
            None,
        )
        .expect("canonical envelope");

        let request = WakeRequest::new("did:key:stale", "stale body").with_envelope(envelope);

        assert_eq!(request.pid, "did:key:canonical");
        assert_eq!(request.message, "canonical body");
        assert_eq!(request.channel_id.as_deref(), Some("telegram:42"));
        assert_eq!(request.thread_id.as_deref(), Some("topic:7"));
        assert_eq!(request.source.as_deref(), Some("telegram"));
        assert_eq!(request.source_message_id.as_deref(), Some("message-1"));
        assert!(request
            .source_hash
            .as_deref()
            .is_some_and(|hash| !hash.is_empty()));
        assert!(request.envelope.is_some());
    }

    #[test]
    fn effective_feature_policy_applies_defaults_and_negative_precedence() {
        let mut request = WakeRequest::new("did:key:runtime", "hello")
            .with_memory(true)
            .with_mcp(true);
        request.compress = true;
        request.disable_memory = true;
        request.disable_mcp = true;
        request.disable_compression = true;
        request.disable_webhooks = true;

        let policy = request.effective_features(WakeFeatureDefaults {
            cache_enabled: false,
            memory_enabled: true,
            mcp_enabled: true,
            smart_route_enabled: false,
            compression_enabled: true,
            webhooks_enabled: true,
        });

        assert_eq!(policy, WakeFeaturePolicy::default());
    }

    #[test]
    fn explicit_enable_overrides_disabled_defaults_without_creating_conflicts() {
        let mut request = WakeRequest::new("did:key:runtime", "hello")
            .with_memory(true)
            .with_mcp(true);
        request.compress = true;

        let policy = request.effective_features(WakeFeatureDefaults::default());

        assert!(policy.memory_enabled);
        assert!(policy.mcp_enabled);
        assert!(policy.compression_enabled);
        assert!(policy.compression_requested);
        assert!(!policy.webhooks_enabled);
        assert!(!request.disable_memory);
        assert!(!request.disable_mcp);
    }

    #[test]
    fn memory_and_mcp_false_setters_override_enabled_defaults() {
        let request = WakeRequest::new("did:key:runtime", "hello")
            .with_memory(true)
            .with_mcp(true)
            .with_memory(false)
            .with_mcp(false);

        let policy = request.effective_features(WakeFeatureDefaults {
            memory_enabled: true,
            mcp_enabled: true,
            ..WakeFeatureDefaults::default()
        });

        assert!(!request.enable_memory);
        assert!(!request.enable_mcp);
        assert!(request.disable_memory);
        assert!(request.disable_mcp);
        assert!(!policy.memory_enabled);
        assert!(!policy.mcp_enabled);
    }

    #[test]
    fn cache_and_smart_route_resolve_from_defaults_or_request() {
        let defaults_policy =
            WakeRequest::new("did:key:runtime", "hello").effective_features(WakeFeatureDefaults {
                cache_enabled: true,
                smart_route_enabled: true,
                ..WakeFeatureDefaults::default()
            });

        assert!(defaults_policy.cache_enabled);
        assert!(defaults_policy.smart_route_enabled);

        let mut request = WakeRequest::new("did:key:runtime", "hello");
        request.enable_cache = true;
        request.smart_route = true;
        let request_policy = request.effective_features(WakeFeatureDefaults::default());

        assert!(request_policy.cache_enabled);
        assert!(request_policy.smart_route_enabled);
    }

    #[test]
    fn compression_config_enables_automatic_compression_without_forcing_it() {
        let policy =
            WakeRequest::new("did:key:runtime", "hello").effective_features(WakeFeatureDefaults {
                compression_enabled: true,
                ..WakeFeatureDefaults::default()
            });

        assert!(policy.compression_enabled);
        assert!(!policy.compression_requested);
    }

    #[test]
    fn compression_request_forces_compression_when_config_is_disabled() {
        let mut request = WakeRequest::new("did:key:runtime", "hello");
        request.compress = true;

        let policy = request.effective_features(WakeFeatureDefaults::default());

        assert!(policy.compression_enabled);
        assert!(policy.compression_requested);
    }

    #[test]
    fn compression_disable_overrides_config_and_forced_request() {
        let mut request = WakeRequest::new("did:key:runtime", "hello");
        request.compress = true;
        request.disable_compression = true;

        let policy = request.effective_features(WakeFeatureDefaults {
            compression_enabled: true,
            ..WakeFeatureDefaults::default()
        });

        assert!(!policy.compression_enabled);
        assert!(!policy.compression_requested);
    }
}
