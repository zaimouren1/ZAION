//! OmniSessionManager - Principal-centric unified session management
//!
//! **Paradigm Breakthrough vs Hermes:**
//! - Hermes: Per-channel isolated sessions (Telegram session ≠ CLI session)
//! - Zaion: Per-principal unified sessions (channels as "attachment points")
//!
//! **Key Innovations:**
//! 1. Principal-centric: Sessions indexed by Ed25519 PrincipalId, not channel_id
//! 2. Channel Attachment: Channels attach to principal's session, not create new sessions
//! 3. Unified Message: Cross-channel messages on same Ed25519 signature chain
//! 4. 5-Layer Context Pyramid: L0-L4 importance-based context organization
//! 5. Display Adaptation: Same content, different formatting per channel capabilities
//! 6. Session Splitting: Automatic context overflow handling with inheritance
//! 7. Signed Continuity: All messages cryptographically signed and chained

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use zaion_types::envelope::CanonicalEnvelope;
use zaion_types::event::LedgerEvent;
use zaion_types::PrincipalId;

use crate::unified_agent_runtime::TurnSignature;
use crate::RuntimeError;

/// Maximum number of L1Recent messages inherited during session split.
const MAX_INHERITED_L1_MESSAGES: usize = 20;

/// Channel type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    /// Command-line interface
    Cli,
    /// Telegram bot
    Telegram,
    /// Discord bot
    Discord,
    /// Feishu (Lark) bot
    Feishu,
    /// DingTalk bot
    DingTalk,
    /// Slack bot
    Slack,
    /// Matrix protocol
    Matrix,
    /// HTTP API server
    ApiServer,
    /// MCP (Model Context Protocol) client
    Mcp,
    /// ACP (Agent Communication Protocol) peer
    Acp,
    /// Webhook endpoint
    Webhook,
    /// Email interface
    Email,
}

impl ChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Feishu => "feishu",
            Self::DingTalk => "dingtalk",
            Self::Slack => "slack",
            Self::Matrix => "matrix",
            Self::ApiServer => "api-server",
            Self::Mcp => "mcp",
            Self::Acp => "acp",
            Self::Webhook => "webhook",
            Self::Email => "email",
        }
    }

    pub fn from_authority_value(value: &str, channel_id: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "cli" => Self::Cli,
            "telegram" => Self::Telegram,
            "discord" => Self::Discord,
            "feishu" | "lark" => Self::Feishu,
            "dingtalk" => Self::DingTalk,
            "slack" => Self::Slack,
            "matrix" => Self::Matrix,
            "api-server" | "api" | "http" => Self::ApiServer,
            "mcp" => Self::Mcp,
            "acp" => Self::Acp,
            "webhook" => Self::Webhook,
            "email" => Self::Email,
            _ => Self::from_ingress(value, channel_id),
        }
    }

    /// Resolve a runtime channel type from canonical ingress metadata.
    pub fn from_ingress(source: &str, channel_id: &str) -> Self {
        let source = source.trim().to_ascii_lowercase();
        let channel_id = channel_id.trim().to_ascii_lowercase();
        match source.as_str() {
            "telegram" => Self::Telegram,
            "discord" => Self::Discord,
            "feishu" | "lark" => Self::Feishu,
            "dingtalk" => Self::DingTalk,
            "slack" => Self::Slack,
            "matrix" => Self::Matrix,
            "mcp" => Self::Mcp,
            "acp" => Self::Acp,
            "webhook" => Self::Webhook,
            "email" => Self::Email,
            "http" | "api" | "api-server" => {
                if channel_id.contains("webhook") {
                    Self::Webhook
                } else {
                    Self::ApiServer
                }
            }
            "cli" | "terminal" | "tui" | "internal-queue" => Self::Cli,
            _ => match channel_id.as_str() {
                "telegram" => Self::Telegram,
                "mcp" => Self::Mcp,
                "acp" => Self::Acp,
                "webhook" | "http-webhook" => Self::Webhook,
                "api" | "http" => Self::ApiServer,
                "tui" | "terminal" | "cli" => Self::Cli,
                _ => Self::Cli,
            },
        }
    }

    /// Get default display capabilities for this channel type
    pub fn default_display_caps(&self) -> DisplayCapabilities {
        match self {
            Self::Cli => DisplayCapabilities {
                supports_markdown: true,
                supports_html: false,
                supports_ansi_colors: true,
                supports_images: false,
                supports_interactive: true,
                max_message_length: None,
            },
            Self::Telegram => DisplayCapabilities {
                supports_markdown: true,
                supports_html: true,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: true,
                max_message_length: Some(4096),
            },
            Self::Discord => DisplayCapabilities {
                supports_markdown: true,
                supports_html: false,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: true,
                max_message_length: Some(2000),
            },
            Self::Feishu | Self::DingTalk => DisplayCapabilities {
                supports_markdown: true,
                supports_html: false,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: true,
                max_message_length: Some(5000),
            },
            Self::Slack => DisplayCapabilities {
                supports_markdown: true,
                supports_html: false,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: true,
                max_message_length: Some(4000),
            },
            Self::Matrix => DisplayCapabilities {
                supports_markdown: true,
                supports_html: true,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: false,
                max_message_length: Some(65536),
            },
            Self::ApiServer | Self::Mcp | Self::Acp | Self::Webhook => DisplayCapabilities {
                supports_markdown: false,
                supports_html: false,
                supports_ansi_colors: false,
                supports_images: false,
                supports_interactive: false,
                max_message_length: None,
            },
            Self::Email => DisplayCapabilities {
                supports_markdown: false,
                supports_html: true,
                supports_ansi_colors: false,
                supports_images: true,
                supports_interactive: false,
                max_message_length: None,
            },
        }
    }

    /// Get default media capabilities for this channel type
    pub fn default_media_caps(&self) -> MediaCapabilities {
        match self {
            Self::Cli => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: false,
                supports_video: false,
                max_file_size_mb: None,
            },
            Self::Telegram => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: true,
                supports_video: true,
                max_file_size_mb: Some(2000),
            },
            Self::Discord => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: true,
                supports_video: true,
                max_file_size_mb: Some(25),
            },
            Self::Feishu | Self::DingTalk => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: true,
                supports_video: true,
                max_file_size_mb: Some(200),
            },
            Self::Slack => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: false,
                supports_video: false,
                max_file_size_mb: Some(1000),
            },
            Self::Matrix => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: true,
                supports_video: true,
                max_file_size_mb: Some(100),
            },
            Self::ApiServer | Self::Mcp | Self::Acp | Self::Webhook => MediaCapabilities {
                supports_file_upload: false,
                supports_file_download: false,
                supports_voice: false,
                supports_video: false,
                max_file_size_mb: None,
            },
            Self::Email => MediaCapabilities {
                supports_file_upload: true,
                supports_file_download: true,
                supports_voice: false,
                supports_video: false,
                max_file_size_mb: Some(25),
            },
        }
    }
}

/// Display capabilities of a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayCapabilities {
    pub supports_markdown: bool,
    pub supports_html: bool,
    pub supports_ansi_colors: bool,
    pub supports_images: bool,
    pub supports_interactive: bool,
    pub max_message_length: Option<usize>,
}

/// Media capabilities of a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCapabilities {
    pub supports_file_upload: bool,
    pub supports_file_download: bool,
    pub supports_voice: bool,
    pub supports_video: bool,
    pub max_file_size_mb: Option<usize>,
}

/// Channel attachment - represents a channel attached to a principal's session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAttachment {
    /// Channel type
    pub channel_type: ChannelType,

    /// Channel-specific identifier (e.g., Telegram chat_id, Discord channel_id)
    pub channel_id: String,

    /// Display capabilities
    pub display_caps: DisplayCapabilities,

    /// Media capabilities
    pub media_caps: MediaCapabilities,

    /// When this channel was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,

    /// Last activity timestamp on this channel
    pub last_active_at: chrono::DateTime<chrono::Utc>,
}

impl ChannelAttachment {
    /// Create a new channel attachment
    pub fn new(channel_type: ChannelType, channel_id: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            channel_type,
            channel_id,
            display_caps: channel_type.default_display_caps(),
            media_caps: channel_type.default_media_caps(),
            attached_at: now,
            last_active_at: now,
        }
    }

    /// Update last active timestamp
    pub fn touch(&mut self) {
        self.last_active_at = chrono::Utc::now();
    }
}

/// Unified message - cross-channel message with source tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessage {
    /// Message ID (unique across all channels)
    pub message_id: String,

    /// Source channel type
    pub source_channel: ChannelType,

    /// Source channel ID
    pub source_channel_id: String,

    /// Message role (user, assistant, system, tool)
    pub role: String,

    /// Message content
    pub content: String,

    /// Turn signature (Ed25519 signature chain)
    pub signature: Option<TurnSignature>,

    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Importance score (0.0-1.0) for context pyramid
    pub importance: f32,

    /// Context layer (L0-L4)
    pub layer: ContextLayer,
}

/// Context layer in 5-layer pyramid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextLayer {
    /// L0: Critical system context (always included)
    L0Critical,
    /// L1: Recent conversation (last N turns)
    L1Recent,
    /// L2: Important context (high importance score)
    L2Important,
    /// L3: Background context (medium importance)
    L3Background,
    /// L4: Archive (low importance, compressed)
    L4Archive,
}

impl ContextLayer {
    /// Get layer priority (lower = higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Self::L0Critical => 0,
            Self::L1Recent => 1,
            Self::L2Important => 2,
            Self::L3Background => 3,
            Self::L4Archive => 4,
        }
    }

    /// Determine layer from importance score
    pub fn from_importance(importance: f32, is_recent: bool) -> Self {
        if importance >= 0.9 {
            Self::L0Critical
        } else if is_recent {
            Self::L1Recent
        } else if importance >= 0.7 {
            Self::L2Important
        } else if importance >= 0.4 {
            Self::L3Background
        } else {
            Self::L4Archive
        }
    }
}

/// Omni session - unified session for a principal across all channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniSession {
    /// Session ID
    pub session_id: String,

    /// Principal ID (Ed25519 public key)
    pub principal_id: PrincipalId,

    /// All messages in this session (unified across channels)
    pub messages: Vec<UnifiedMessage>,

    /// Attached channels
    pub attachments: Vec<ChannelAttachment>,

    /// Session creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last activity timestamp
    pub last_active_at: chrono::DateTime<chrono::Utc>,

    /// Parent session ID (if this is a split session)
    pub parent_session_id: Option<String>,

    /// Child session IDs (if this session has been split)
    pub child_session_ids: Vec<String>,

    /// Total token count (for split detection)
    pub total_tokens: usize,

    /// Maximum tokens before split
    pub max_tokens: usize,
}

impl OmniSession {
    /// Create a new omni session
    pub fn new(principal_id: PrincipalId, max_tokens: usize) -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            principal_id,
            messages: Vec::new(),
            attachments: Vec::new(),
            created_at: now,
            last_active_at: now,
            parent_session_id: None,
            child_session_ids: Vec::new(),
            total_tokens: 0,
            max_tokens,
        }
    }

    /// Estimate token count for a string (~4 chars per token)
    fn estimate_tokens(s: &str) -> usize {
        s.len() / 4
    }

    /// Add a message to this session
    pub fn add_message(&mut self, message: UnifiedMessage) {
        self.total_tokens += Self::estimate_tokens(&message.content);
        self.messages.push(message);
        self.last_active_at = chrono::Utc::now();
    }

    /// Attach a channel to this session
    pub fn attach_channel(&mut self, attachment: ChannelAttachment) {
        // Check if channel already attached
        if !self.attachments.iter().any(|a| {
            a.channel_type == attachment.channel_type && a.channel_id == attachment.channel_id
        }) {
            self.attachments.push(attachment);
        }
    }

    /// Get messages by context layer
    pub fn messages_by_layer(&self, layer: ContextLayer) -> Vec<&UnifiedMessage> {
        self.messages.iter().filter(|m| m.layer == layer).collect()
    }

    /// Check if session needs splitting
    pub fn needs_split(&self) -> bool {
        self.total_tokens >= self.max_tokens
    }

    /// Get active channels (active in last 24 hours)
    pub fn active_channels(&self) -> Vec<&ChannelAttachment> {
        let threshold = chrono::Utc::now() - chrono::Duration::hours(24);
        self.attachments
            .iter()
            .filter(|a| a.last_active_at > threshold)
            .collect()
    }
}

/// Channel key for routing
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelKey {
    pub channel_type: ChannelType,
    pub channel_id: String,
}

impl ChannelKey {
    pub fn new(channel_type: ChannelType, channel_id: String) -> Self {
        Self {
            channel_type,
            channel_id,
        }
    }
}

/// 5-layer context pyramid for importance-based context assembly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPyramid {
    pub layers: [Vec<String>; 5],
    pub token_budget: usize,
    pub tokens_used: usize,
}

/// Runtime authority returned by OmniSessionManager after routing an envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniRouteAuthority {
    pub authority: String,
    pub authority_schema: String,
    pub session_id: String,
    pub omni_session_id: String,
    pub principal_id: String,
    pub channel_type: String,
    pub channel_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub envelope_id: String,
    pub source_hash: String,
    pub route: String,
    pub channel_attached: bool,
    pub message_count: usize,
    pub attachment_count: usize,
    pub session_graph_hash: String,
}

impl OmniRouteAuthority {
    pub fn authority_hash(&self) -> String {
        let bytes = serde_json::to_vec(&OmniRouteAuthorityHashBasis {
            authority: &self.authority,
            authority_schema: &self.authority_schema,
            session_id: &self.session_id,
            omni_session_id: &self.omni_session_id,
            principal_id: &self.principal_id,
            channel_type: &self.channel_type,
            channel_id: &self.channel_id,
            thread_id: &self.thread_id,
            message_id: &self.message_id,
            envelope_id: &self.envelope_id,
            source_hash: &self.source_hash,
            route: &self.route,
            channel_attached: self.channel_attached,
            message_count: self.message_count,
            attachment_count: self.attachment_count,
            session_graph_hash: &self.session_graph_hash,
        })
        .expect("serializing omni route authority cannot fail");
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    pub fn to_ledger_payload(
        &self,
        parent_received_event_id: impl Into<String>,
    ) -> serde_json::Value {
        serde_json::json!({
            "schema": "zaion.omni_route.v1",
            "authority": self.authority,
            "authority_schema": self.authority_schema,
            "authority_hash": self.authority_hash(),
            "principal_id": self.principal_id,
            "channel_type": self.channel_type,
            "channel_id": self.channel_id,
            "thread_id": self.thread_id,
            "session_id": self.session_id,
            "omni_session_id": self.omni_session_id,
            "message_id": self.message_id,
            "envelope_id": self.envelope_id,
            "route": self.route,
            "channel_attached": self.channel_attached,
            "message_count": self.message_count,
            "attachment_count": self.attachment_count,
            "session_graph_hash": self.session_graph_hash,
            "source_hash": self.source_hash,
            "parent_received_event_id": parent_received_event_id.into(),
        })
    }
}

#[derive(Serialize)]
struct OmniRouteAuthorityHashBasis<'a> {
    authority: &'a str,
    authority_schema: &'a str,
    session_id: &'a str,
    omni_session_id: &'a str,
    principal_id: &'a str,
    channel_type: &'a str,
    channel_id: &'a str,
    thread_id: &'a str,
    message_id: &'a str,
    envelope_id: &'a str,
    source_hash: &'a str,
    route: &'a str,
    channel_attached: bool,
    message_count: usize,
    attachment_count: usize,
    session_graph_hash: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniSessionGraphReplay {
    pub schema: String,
    pub principal_id: String,
    pub active_omni_session_id: String,
    pub route_event_count: usize,
    pub verified_route_event_count: usize,
    pub message_count: usize,
    pub attachment_count: usize,
    pub channel_count: usize,
    pub last_route_event_id: Option<String>,
    pub last_authority_hash: Option<String>,
    pub replay_hash: String,
}

#[derive(Serialize)]
struct OmniSessionGraphHashBasis {
    schema: &'static str,
    principal_id: String,
    active_omni_session_id: String,
    parent_session_id: Option<String>,
    child_session_ids: Vec<String>,
    messages: Vec<OmniSessionGraphMessage>,
    attachments: Vec<OmniSessionGraphAttachment>,
}

#[derive(Serialize)]
struct OmniSessionGraphMessage {
    message_id: String,
    source_channel: String,
    source_channel_id: String,
    role: String,
    layer: ContextLayer,
}

#[derive(Serialize)]
struct OmniSessionGraphAttachment {
    channel_type: String,
    channel_id: String,
}

impl ContextPyramid {
    pub fn new(token_budget: usize) -> Self {
        Self {
            layers: Default::default(),
            token_budget,
            tokens_used: 0,
        }
    }

    pub fn add_to_layer(&mut self, layer: ContextLayer, content: String) -> bool {
        let tokens = content.len() / 4;
        if self.tokens_used + tokens > self.token_budget {
            return false;
        }
        self.tokens_used += tokens;
        self.layers[layer.priority() as usize].push(content);
        true
    }

    pub fn assemble(&self) -> String {
        self.layers
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn build_from_session(session: &OmniSession, token_budget: usize) -> Self {
        let mut pyramid = Self::new(token_budget);
        for msg in &session.messages {
            let entry = format!("[{}] {}: {}", msg.source_channel_id, msg.role, msg.content);
            if !pyramid.add_to_layer(msg.layer, entry) {
                break;
            }
        }
        pyramid
    }

    pub fn layer_token_count(&self, layer: ContextLayer) -> usize {
        self.layers[layer.priority() as usize]
            .iter()
            .map(|s| s.len() / 4)
            .sum()
    }

    pub fn utilization(&self) -> f32 {
        if self.token_budget == 0 {
            return 0.0;
        }
        self.tokens_used as f32 / self.token_budget as f32
    }
}

/// OmniSessionManager - manages all principal sessions
pub struct OmniSessionManager {
    /// Active sessions indexed by principal ID (one per principal)
    sessions: HashMap<PrincipalId, OmniSession>,

    /// Archived (split) sessions indexed by session_id for audit/retrieval
    archived_sessions: HashMap<String, OmniSession>,

    /// Channel to principal mapping
    channel_map: HashMap<ChannelKey, PrincipalId>,

    /// Default max tokens per session
    default_max_tokens: usize,
}

impl OmniSessionManager {
    /// Create a new OmniSessionManager
    pub fn new(default_max_tokens: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            archived_sessions: HashMap::new(),
            channel_map: HashMap::new(),
            default_max_tokens,
        }
    }

    /// Get or create session for a principal
    pub fn get_or_create_session(&mut self, principal_id: PrincipalId) -> &mut OmniSession {
        let default_max_tokens = self.default_max_tokens;
        self.sessions
            .entry(principal_id.clone())
            .or_insert_with(|| OmniSession::new(principal_id, default_max_tokens))
    }

    /// Get session by principal ID
    pub fn get_session(&self, principal_id: &PrincipalId) -> Option<&OmniSession> {
        self.sessions.get(principal_id)
    }

    /// Get mutable session by principal ID
    pub fn get_session_mut(&mut self, principal_id: &PrincipalId) -> Option<&mut OmniSession> {
        self.sessions.get_mut(principal_id)
    }

    /// Route message from channel to principal's session
    pub fn route_message(
        &mut self,
        channel_type: ChannelType,
        channel_id: String,
        principal_id: PrincipalId,
        message: UnifiedMessage,
    ) -> Result<(), RuntimeError> {
        // Update channel mapping
        let channel_key = ChannelKey::new(channel_type, channel_id.clone());
        self.channel_map.insert(channel_key, principal_id.clone());

        // Get or create session
        let session = self.get_or_create_session(principal_id);

        // Attach channel if not already attached
        let attachment = ChannelAttachment::new(channel_type, channel_id);
        session.attach_channel(attachment);

        // Add message
        session.add_message(message);

        Ok(())
    }

    /// Route a canonical ingress envelope through the OmniSession authority.
    pub fn route_envelope(
        &mut self,
        envelope: &CanonicalEnvelope,
    ) -> Result<OmniRouteAuthority, RuntimeError> {
        let channel_type = ChannelType::from_ingress(&envelope.source, &envelope.channel.0);
        let principal_id = envelope.principal.clone();
        let timestamp = chrono::DateTime::parse_from_rfc3339(&envelope.received_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let message = UnifiedMessage {
            message_id: envelope.message_id.clone(),
            source_channel: channel_type,
            source_channel_id: envelope.channel.0.clone(),
            role: "user".to_string(),
            content: envelope.body.clone(),
            signature: None,
            timestamp,
            importance: 0.6,
            layer: ContextLayer::L1Recent,
        };

        self.route_message(
            channel_type,
            envelope.channel.0.clone(),
            principal_id.clone(),
            message,
        )?;

        let session = self.get_session(&principal_id).ok_or_else(|| {
            RuntimeError::Internal(
                "OmniSessionManager authority missing routed session".to_string(),
            )
        })?;
        let channel_attached = session.attachments.iter().any(|attachment| {
            attachment.channel_type == channel_type && attachment.channel_id == envelope.channel.0
        });
        let session_graph_hash = Self::session_graph_hash(session);

        Ok(OmniRouteAuthority {
            authority: "OmniSessionManager".to_string(),
            authority_schema: "zaion.omni_session_authority.v1".to_string(),
            session_id: envelope.session_id(),
            omni_session_id: session.session_id.clone(),
            principal_id: principal_id.as_str().to_string(),
            channel_type: channel_type.as_str().to_string(),
            channel_id: envelope.channel.0.clone(),
            thread_id: envelope.thread.0.clone(),
            message_id: envelope.message_id.clone(),
            envelope_id: envelope.envelope_id(),
            source_hash: envelope.source_hash.clone(),
            route: "CanonicalEnvelope -> OmniSessionManager -> principal session graph".to_string(),
            channel_attached,
            message_count: session.messages.len(),
            attachment_count: session.attachments.len(),
            session_graph_hash,
        })
    }

    /// Rebuild the principal-centric session graph from signed `omni.route`
    /// ledger events.
    pub fn replay_signed_route_events(
        &mut self,
        events: &[LedgerEvent],
        principal_filter: Option<&str>,
    ) -> Result<OmniSessionGraphReplay, RuntimeError> {
        let mut routes = events
            .iter()
            .filter(|event| {
                event.event_type == "omni.route"
                    && event.signature.is_some()
                    && event.payload.get("authority").and_then(|v| v.as_str())
                        == Some("OmniSessionManager")
                    && event
                        .payload
                        .get("authority_schema")
                        .and_then(|v| v.as_str())
                        == Some("zaion.omni_session_authority.v1")
                    && principal_filter
                        .map(|principal| {
                            event.payload.get("principal_id").and_then(|v| v.as_str())
                                == Some(principal)
                        })
                        .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();

        routes.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        if routes.is_empty() {
            return Err(RuntimeError::Internal(
                "no signed OmniSessionManager route events available for replay".to_string(),
            ));
        }

        self.sessions.clear();
        self.archived_sessions.clear();
        self.channel_map.clear();

        let mut last_route_event_id = None;
        let mut last_authority_hash = None;
        let mut active_principal = String::new();
        let mut active_omni_session_id = String::new();

        for event in &routes {
            let payload = &event.payload;
            let principal = required_payload_str(payload, "principal_id")?.to_string();
            let omni_session_id = required_payload_str(payload, "omni_session_id")?.to_string();
            let channel_id = required_payload_str(payload, "channel_id")?.to_string();
            let _ = required_payload_str(payload, "thread_id")?;
            let message_id = required_payload_str(payload, "message_id")?.to_string();
            let source_hash = required_payload_str(payload, "source_hash")?.to_string();
            let authority_hash = required_payload_str(payload, "authority_hash")?.to_string();
            let channel_type = ChannelType::from_authority_value(
                payload
                    .get("channel_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                &channel_id,
            );

            let principal_id = PrincipalId(principal.clone());
            let session = self
                .sessions
                .entry(principal_id.clone())
                .or_insert_with(|| OmniSession::new(principal_id.clone(), self.default_max_tokens));
            session.session_id = omni_session_id.clone();
            session.last_active_at = parse_event_time(&event.created_at);

            let attachment = ChannelAttachment::new(channel_type, channel_id.clone());
            session.attach_channel(attachment);
            let already_has_message = session
                .messages
                .iter()
                .any(|message| message.message_id == message_id);
            if !already_has_message {
                session.add_message(UnifiedMessage {
                    message_id,
                    source_channel: channel_type,
                    source_channel_id: channel_id.clone(),
                    role: "user".to_string(),
                    content: format!("source_hash:{}", source_hash),
                    signature: None,
                    timestamp: parse_event_time(&event.created_at),
                    importance: 0.6,
                    layer: ContextLayer::L1Recent,
                });
            }

            self.channel_map.insert(
                ChannelKey::new(channel_type, channel_id),
                principal_id.clone(),
            );
            active_principal = principal;
            active_omni_session_id = omni_session_id;
            last_route_event_id = Some(event.event_id.0.clone());
            last_authority_hash = Some(authority_hash);
        }

        let principal_id = PrincipalId(active_principal.clone());
        let session = self.get_session(&principal_id).ok_or_else(|| {
            RuntimeError::Internal(
                "OmniSession graph replay produced no active session".to_string(),
            )
        })?;
        let replay_hash = Self::session_graph_hash(session);

        Ok(OmniSessionGraphReplay {
            schema: "zaion.omni_session_graph_replay.v1".to_string(),
            principal_id: active_principal,
            active_omni_session_id,
            route_event_count: routes.len(),
            verified_route_event_count: routes.len(),
            message_count: session.messages.len(),
            attachment_count: session.attachments.len(),
            channel_count: self.channel_map.len(),
            last_route_event_id,
            last_authority_hash,
            replay_hash,
        })
    }

    pub fn replay_from_ledger(
        &mut self,
        ledger: &zaion_ledger::EventLedger,
        principal_filter: Option<&str>,
        limit: usize,
    ) -> Result<OmniSessionGraphReplay, RuntimeError> {
        let events = ledger.list_global_events(limit)?;
        self.replay_signed_route_events(&events, principal_filter)
    }

    pub fn session_graph_hash(session: &OmniSession) -> String {
        let mut messages = session
            .messages
            .iter()
            .map(|message| OmniSessionGraphMessage {
                message_id: message.message_id.clone(),
                source_channel: message.source_channel.as_str().to_string(),
                source_channel_id: message.source_channel_id.clone(),
                role: message.role.clone(),
                layer: message.layer,
            })
            .collect::<Vec<_>>();
        messages.sort_by(|a, b| a.message_id.cmp(&b.message_id));

        let mut attachments = session
            .attachments
            .iter()
            .map(|attachment| OmniSessionGraphAttachment {
                channel_type: attachment.channel_type.as_str().to_string(),
                channel_id: attachment.channel_id.clone(),
            })
            .collect::<Vec<_>>();
        attachments.sort_by(|a, b| {
            a.channel_type
                .cmp(&b.channel_type)
                .then(a.channel_id.cmp(&b.channel_id))
        });

        let basis = OmniSessionGraphHashBasis {
            schema: "zaion.omni_session_graph_hash.v1",
            principal_id: session.principal_id.as_str().to_string(),
            active_omni_session_id: session.session_id.clone(),
            parent_session_id: session.parent_session_id.clone(),
            child_session_ids: session.child_session_ids.clone(),
            messages,
            attachments,
        };
        let bytes = serde_json::to_vec(&basis)
            .expect("serializing omni session graph hash basis cannot fail");
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    /// Get principal ID from channel
    pub fn get_principal_by_channel(
        &self,
        channel_type: ChannelType,
        channel_id: &str,
    ) -> Option<PrincipalId> {
        let channel_key = ChannelKey::new(channel_type, channel_id.to_string());
        self.channel_map.get(&channel_key).cloned()
    }

    /// Split session when context overflows.
    ///
    /// The parent session is archived (preserving full history for audit),
    /// and a new child session becomes the active session inheriting
    /// L0 (critical) + last N L1 (recent) messages and all channel attachments.
    pub fn split_session(&mut self, principal_id: &PrincipalId) -> Result<String, RuntimeError> {
        let default_max_tokens = self.default_max_tokens;

        let session = self
            .sessions
            .remove(principal_id)
            .ok_or_else(|| RuntimeError::Internal("Session not found".to_string()))?;

        // Create child session
        let mut child_session = OmniSession::new(principal_id.clone(), default_max_tokens);
        child_session.parent_session_id = Some(session.session_id.clone());

        // Inherit all L0 (critical) messages and the last N L1 (recent) messages
        let l0_messages: Vec<UnifiedMessage> = session
            .messages
            .iter()
            .filter(|m| m.layer == ContextLayer::L0Critical)
            .cloned()
            .collect();

        let l1_messages: Vec<UnifiedMessage> = session
            .messages
            .iter()
            .filter(|m| m.layer == ContextLayer::L1Recent)
            .rev()
            .take(MAX_INHERITED_L1_MESSAGES)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let mut inherited_messages = l0_messages;
        inherited_messages.extend(l1_messages);

        // Recalculate token count for inherited messages
        child_session.total_tokens = inherited_messages
            .iter()
            .map(|m| OmniSession::estimate_tokens(&m.content))
            .sum();
        child_session.messages = inherited_messages;

        // Inherit all channel attachments
        child_session.attachments = session.attachments.clone();

        let child_session_id = child_session.session_id.clone();

        // Archive parent session (preserve full history for audit/retrieval)
        let mut archived_parent = session;
        archived_parent
            .child_session_ids
            .push(child_session_id.clone());
        self.archived_sessions
            .insert(archived_parent.session_id.clone(), archived_parent);

        // Install child as the active session
        self.sessions.insert(principal_id.clone(), child_session);

        Ok(child_session_id)
    }

    /// Get an archived session by its session ID
    pub fn get_archived_session(&self, session_id: &str) -> Option<&OmniSession> {
        self.archived_sessions.get(session_id)
    }

    /// Get the count of archived sessions
    pub fn archived_session_count(&self) -> usize {
        self.archived_sessions.len()
    }

    /// Get total session count
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Get total channel count
    pub fn channel_count(&self) -> usize {
        self.channel_map.len()
    }
}

fn required_payload_str<'a>(
    payload: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, RuntimeError> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            RuntimeError::Internal(format!(
                "omni.route replay missing required payload field `{}`",
                key
            ))
        })
}

fn parse_event_time(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_type_display_caps() {
        let cli_caps = ChannelType::Cli.default_display_caps();
        assert!(cli_caps.supports_markdown);
        assert!(cli_caps.supports_ansi_colors);
        assert!(!cli_caps.supports_html);

        let telegram_caps = ChannelType::Telegram.default_display_caps();
        assert!(telegram_caps.supports_markdown);
        assert!(telegram_caps.supports_html);
        assert_eq!(telegram_caps.max_message_length, Some(4096));
    }

    #[test]
    fn test_channel_attachment_creation() {
        let attachment = ChannelAttachment::new(ChannelType::Telegram, "123456".to_string());
        assert_eq!(attachment.channel_type, ChannelType::Telegram);
        assert_eq!(attachment.channel_id, "123456");
        assert!(attachment.display_caps.supports_markdown);
    }

    #[test]
    fn test_context_layer_priority() {
        assert_eq!(ContextLayer::L0Critical.priority(), 0);
        assert_eq!(ContextLayer::L1Recent.priority(), 1);
        assert_eq!(ContextLayer::L4Archive.priority(), 4);
    }

    #[test]
    fn test_context_layer_from_importance() {
        assert_eq!(
            ContextLayer::from_importance(0.95, false),
            ContextLayer::L0Critical
        );
        assert_eq!(
            ContextLayer::from_importance(0.5, true),
            ContextLayer::L1Recent
        );
        assert_eq!(
            ContextLayer::from_importance(0.75, false),
            ContextLayer::L2Important
        );
        assert_eq!(
            ContextLayer::from_importance(0.2, false),
            ContextLayer::L4Archive
        );
    }

    #[test]
    fn test_omni_session_creation() {
        let principal_id = PrincipalId("principal1".to_string());
        let session = OmniSession::new(principal_id.clone(), 100000);

        assert_eq!(session.principal_id, principal_id);
        assert_eq!(session.messages.len(), 0);
        assert_eq!(session.attachments.len(), 0);
        assert_eq!(session.max_tokens, 100000);
    }

    #[test]
    fn test_omni_session_attach_channel() {
        let principal_id = PrincipalId("principal1".to_string());
        let mut session = OmniSession::new(principal_id, 100000);

        let attachment = ChannelAttachment::new(ChannelType::Telegram, "123".to_string());
        session.attach_channel(attachment);

        assert_eq!(session.attachments.len(), 1);
        assert_eq!(session.attachments[0].channel_type, ChannelType::Telegram);

        // Attach same channel again - should not duplicate
        let attachment2 = ChannelAttachment::new(ChannelType::Telegram, "123".to_string());
        session.attach_channel(attachment2);
        assert_eq!(session.attachments.len(), 1);
    }

    #[test]
    fn test_omni_session_manager_creation() {
        let manager = OmniSessionManager::new(100000);
        assert_eq!(manager.session_count(), 0);
        assert_eq!(manager.channel_count(), 0);
    }

    #[test]
    fn test_omni_session_manager_get_or_create() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let session = manager.get_or_create_session(principal_id.clone());
        assert_eq!(session.principal_id, principal_id);
        assert_eq!(manager.session_count(), 1);

        // Get same session again
        let session2 = manager.get_or_create_session(principal_id.clone());
        assert_eq!(session2.principal_id, principal_id);
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_omni_session_manager_route_message() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let message = UnifiedMessage {
            message_id: "msg1".to_string(),
            source_channel: ChannelType::Telegram,
            source_channel_id: "123".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.8,
            layer: ContextLayer::L1Recent,
        };

        manager
            .route_message(
                ChannelType::Telegram,
                "123".to_string(),
                principal_id.clone(),
                message,
            )
            .unwrap();

        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.channel_count(), 1);

        let session = manager.get_session(&principal_id).unwrap();
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.attachments.len(), 1);
    }

    #[test]
    fn test_omni_session_manager_get_principal_by_channel() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let message = UnifiedMessage {
            message_id: "msg1".to_string(),
            source_channel: ChannelType::Discord,
            source_channel_id: "456".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.8,
            layer: ContextLayer::L1Recent,
        };

        manager
            .route_message(
                ChannelType::Discord,
                "456".to_string(),
                principal_id.clone(),
                message,
            )
            .unwrap();

        let found_principal = manager
            .get_principal_by_channel(ChannelType::Discord, "456")
            .unwrap();
        assert_eq!(found_principal, principal_id);

        // Non-existent channel
        let not_found = manager.get_principal_by_channel(ChannelType::Slack, "999");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_omni_session_messages_by_layer() {
        let principal_id = PrincipalId("principal1".to_string());
        let mut session = OmniSession::new(principal_id, 100000);

        let msg1 = UnifiedMessage {
            message_id: "msg1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli1".to_string(),
            role: "user".to_string(),
            content: "Critical".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.95,
            layer: ContextLayer::L0Critical,
        };

        let msg2 = UnifiedMessage {
            message_id: "msg2".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli1".to_string(),
            role: "user".to_string(),
            content: "Recent".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.6,
            layer: ContextLayer::L1Recent,
        };

        session.add_message(msg1);
        session.add_message(msg2);

        let critical_msgs = session.messages_by_layer(ContextLayer::L0Critical);
        assert_eq!(critical_msgs.len(), 1);
        assert_eq!(critical_msgs[0].content, "Critical");

        let recent_msgs = session.messages_by_layer(ContextLayer::L1Recent);
        assert_eq!(recent_msgs.len(), 1);
        assert_eq!(recent_msgs[0].content, "Recent");
    }

    #[test]
    fn test_omni_session_split() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let session = manager.get_or_create_session(principal_id.clone());
        let original_session_id = session.session_id.clone();

        let msg1 = UnifiedMessage {
            message_id: "msg1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli1".to_string(),
            role: "user".to_string(),
            content: "Critical".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.95,
            layer: ContextLayer::L0Critical,
        };

        let msg2 = UnifiedMessage {
            message_id: "msg2".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli1".to_string(),
            role: "user".to_string(),
            content: "Archive".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.2,
            layer: ContextLayer::L4Archive,
        };

        session.add_message(msg1);
        session.add_message(msg2);

        let child_session_id = manager.split_session(&principal_id).unwrap();

        let child_session = manager.get_session(&principal_id).unwrap();
        assert_eq!(child_session.session_id, child_session_id);
        assert_eq!(child_session.parent_session_id, Some(original_session_id));
        assert_eq!(child_session.messages.len(), 1);
        assert_eq!(child_session.messages[0].content, "Critical");
    }

    #[test]
    fn test_context_pyramid_creation() {
        let pyramid = ContextPyramid::new(10000);
        assert_eq!(pyramid.token_budget, 10000);
        assert_eq!(pyramid.tokens_used, 0);
        assert!((pyramid.utilization() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_context_pyramid_add_to_layer() {
        let mut pyramid = ContextPyramid::new(10000);
        assert!(pyramid.add_to_layer(ContextLayer::L0Critical, "system prompt".to_string()));
        assert!(pyramid.tokens_used > 0);
        assert_eq!(pyramid.layers[0].len(), 1);
    }

    #[test]
    fn test_context_pyramid_budget_overflow() {
        let mut pyramid = ContextPyramid::new(4);
        assert!(pyramid.add_to_layer(ContextLayer::L0Critical, "abcdefghijklmnop".to_string()));
        // Budget full, next add should fail
        assert!(!pyramid.add_to_layer(ContextLayer::L1Recent, "more content here".to_string()));
    }

    #[test]
    fn test_context_pyramid_assemble() {
        let mut pyramid = ContextPyramid::new(10000);
        pyramid.add_to_layer(ContextLayer::L0Critical, "L0 content".to_string());
        pyramid.add_to_layer(ContextLayer::L1Recent, "L1 content".to_string());
        let assembled = pyramid.assemble();
        assert!(assembled.contains("L0 content"));
        assert!(assembled.contains("L1 content"));
    }

    #[test]
    fn test_context_pyramid_build_from_session() {
        let principal_id = PrincipalId("p1".to_string());
        let mut session = OmniSession::new(principal_id, 100000);

        session.add_message(UnifiedMessage {
            message_id: "m1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "user".to_string(),
            content: "hello world".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.95,
            layer: ContextLayer::L0Critical,
        });

        session.add_message(UnifiedMessage {
            message_id: "m2".to_string(),
            source_channel: ChannelType::Telegram,
            source_channel_id: "tg1".to_string(),
            role: "assistant".to_string(),
            content: "response".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.5,
            layer: ContextLayer::L3Background,
        });

        let pyramid = ContextPyramid::build_from_session(&session, 10000);
        assert!(pyramid.tokens_used > 0);
        assert_eq!(pyramid.layers[0].len(), 1); // L0
        assert_eq!(pyramid.layers[3].len(), 1); // L3
    }

    #[test]
    fn test_context_pyramid_layer_token_count() {
        let mut pyramid = ContextPyramid::new(10000);
        pyramid.add_to_layer(
            ContextLayer::L2Important,
            "important context data".to_string(),
        );
        let count = pyramid.layer_token_count(ContextLayer::L2Important);
        assert!(count > 0);
        assert_eq!(pyramid.layer_token_count(ContextLayer::L4Archive), 0);
    }

    #[test]
    fn test_omni_session_token_tracking() {
        let principal_id = PrincipalId("p1".to_string());
        let mut session = OmniSession::new(principal_id, 100);

        assert_eq!(session.total_tokens, 0);
        assert!(!session.needs_split());

        session.add_message(UnifiedMessage {
            message_id: "m1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "user".to_string(),
            content: "a]".repeat(200),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.5,
            layer: ContextLayer::L1Recent,
        });

        assert!(session.total_tokens > 0);
        assert!(session.needs_split());
    }

    #[test]
    fn test_multi_channel_single_principal() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("p1".to_string());

        let mk_msg = |id: &str, ch: ChannelType, ch_id: &str| UnifiedMessage {
            message_id: id.to_string(),
            source_channel: ch,
            source_channel_id: ch_id.to_string(),
            role: "user".to_string(),
            content: "msg".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.5,
            layer: ContextLayer::L1Recent,
        };

        manager
            .route_message(
                ChannelType::Telegram,
                "tg1".into(),
                principal_id.clone(),
                mk_msg("m1", ChannelType::Telegram, "tg1"),
            )
            .unwrap();
        manager
            .route_message(
                ChannelType::Discord,
                "dc1".into(),
                principal_id.clone(),
                mk_msg("m2", ChannelType::Discord, "dc1"),
            )
            .unwrap();
        manager
            .route_message(
                ChannelType::Cli,
                "cli1".into(),
                principal_id.clone(),
                mk_msg("m3", ChannelType::Cli, "cli1"),
            )
            .unwrap();

        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.channel_count(), 3);

        let session = manager.get_session(&principal_id).unwrap();
        assert_eq!(session.messages.len(), 3);
        assert_eq!(session.attachments.len(), 3);
    }

    #[test]
    fn test_split_nonexistent_session() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("ghost".to_string());
        assert!(manager.split_session(&principal_id).is_err());
    }

    #[test]
    fn test_media_caps_per_channel() {
        let tg = ChannelType::Telegram.default_media_caps();
        assert!(tg.supports_voice);
        assert!(tg.supports_video);
        assert_eq!(tg.max_file_size_mb, Some(2000));

        let api = ChannelType::ApiServer.default_media_caps();
        assert!(!api.supports_file_upload);
        assert!(!api.supports_voice);

        let email = ChannelType::Email.default_media_caps();
        assert!(email.supports_file_upload);
        assert!(!email.supports_voice);
        assert_eq!(email.max_file_size_mb, Some(25));
    }

    #[test]
    fn test_context_layer_l3_background() {
        assert_eq!(
            ContextLayer::from_importance(0.5, false),
            ContextLayer::L3Background
        );
        assert_eq!(ContextLayer::L3Background.priority(), 3);
    }

    #[test]
    fn test_split_archives_parent_session() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let session = manager.get_or_create_session(principal_id.clone());
        let parent_id = session.session_id.clone();

        // Add messages across multiple layers
        session.add_message(UnifiedMessage {
            message_id: "m1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "system".to_string(),
            content: "System prompt".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.95,
            layer: ContextLayer::L0Critical,
        });
        session.add_message(UnifiedMessage {
            message_id: "m2".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "user".to_string(),
            content: "Old message".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.3,
            layer: ContextLayer::L3Background,
        });
        session.add_message(UnifiedMessage {
            message_id: "m3".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "user".to_string(),
            content: "Recent message".to_string(),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.6,
            layer: ContextLayer::L1Recent,
        });

        let child_id = manager.split_session(&principal_id).unwrap();

        // Child should be active
        let child = manager.get_session(&principal_id).unwrap();
        assert_eq!(child.session_id, child_id);
        assert_eq!(child.parent_session_id, Some(parent_id.clone()));
        // Child inherits L0 + L1, not L3
        assert_eq!(child.messages.len(), 2);

        // Parent should be archived with full history
        assert_eq!(manager.archived_session_count(), 1);
        let archived = manager.get_archived_session(&parent_id).unwrap();
        assert_eq!(archived.messages.len(), 3); // all original messages preserved
        assert!(archived.child_session_ids.contains(&child_id));
    }

    #[test]
    fn test_split_l1_inheritance_capped() {
        let mut manager = OmniSessionManager::new(1_000_000);
        let principal_id = PrincipalId("principal1".to_string());

        let session = manager.get_or_create_session(principal_id.clone());

        // Add 30 L1Recent messages (exceeds MAX_INHERITED_L1_MESSAGES = 20)
        for i in 0..30 {
            session.add_message(UnifiedMessage {
                message_id: format!("m{}", i),
                source_channel: ChannelType::Cli,
                source_channel_id: "cli".to_string(),
                role: "user".to_string(),
                content: format!("Message {}", i),
                signature: None,
                timestamp: chrono::Utc::now(),
                importance: 0.6,
                layer: ContextLayer::L1Recent,
            });
        }

        manager.split_session(&principal_id).unwrap();

        let child = manager.get_session(&principal_id).unwrap();
        // Only last 20 L1 messages inherited
        assert_eq!(child.messages.len(), MAX_INHERITED_L1_MESSAGES);
        // Should be messages 10..29 (the last 20)
        assert_eq!(child.messages[0].content, "Message 10");
        assert_eq!(child.messages[19].content, "Message 29");
    }

    #[test]
    fn test_child_token_count_recalculated() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());

        let session = manager.get_or_create_session(principal_id.clone());
        session.add_message(UnifiedMessage {
            message_id: "m1".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "system".to_string(),
            content: "x".repeat(400), // ~100 tokens at len/4
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.95,
            layer: ContextLayer::L0Critical,
        });
        // L3 message won't be inherited
        session.add_message(UnifiedMessage {
            message_id: "m2".to_string(),
            source_channel: ChannelType::Cli,
            source_channel_id: "cli".to_string(),
            role: "user".to_string(),
            content: "y".repeat(800),
            signature: None,
            timestamp: chrono::Utc::now(),
            importance: 0.3,
            layer: ContextLayer::L3Background,
        });

        manager.split_session(&principal_id).unwrap();

        let child = manager.get_session(&principal_id).unwrap();
        assert_eq!(child.messages.len(), 1); // only L0 inherited
                                             // Token count should reflect only the inherited message, not the background one
        assert_eq!(child.total_tokens, 100); // 400 chars / 4
    }

    #[test]
    fn test_route_envelope_returns_ledger_authority_payload() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());
        let envelope = zaion_types::envelope::CanonicalEnvelope::new(
            "telegram",
            principal_id.clone(),
            zaion_types::session::ChannelId("telegram".to_string()),
            zaion_types::session::ThreadId("phase8".to_string()),
            "m1",
            "hello from telegram",
            None,
        )
        .unwrap();

        let authority = manager.route_envelope(&envelope).unwrap();
        assert_eq!(authority.authority, "OmniSessionManager");
        assert_eq!(
            authority.authority_schema,
            "zaion.omni_session_authority.v1"
        );
        assert_eq!(authority.session_id, envelope.session_id());
        assert_eq!(authority.principal_id, principal_id.as_str());
        assert_eq!(authority.channel_id, "telegram");
        assert_eq!(authority.thread_id, "phase8");
        assert_eq!(authority.message_count, 1);
        assert_eq!(authority.attachment_count, 1);
        assert!(!authority.omni_session_id.is_empty());
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.channel_count(), 1);

        let payload = authority.to_ledger_payload("received-1");
        assert_eq!(payload["schema"], "zaion.omni_route.v1");
        assert_eq!(payload["authority"], "OmniSessionManager");
        assert_eq!(
            payload["authority_schema"],
            "zaion.omni_session_authority.v1"
        );
        assert_eq!(payload["authority_hash"].as_str().unwrap().len(), 64);
        assert_eq!(payload["parent_received_event_id"], "received-1");
    }

    #[test]
    fn test_replay_signed_omni_route_events_rebuilds_session_graph() {
        let mut manager = OmniSessionManager::new(100000);
        let principal_id = PrincipalId("principal1".to_string());
        let envelope1 = zaion_types::envelope::CanonicalEnvelope::new(
            "telegram",
            principal_id.clone(),
            zaion_types::session::ChannelId("telegram".to_string()),
            zaion_types::session::ThreadId("phase8".to_string()),
            "m1",
            "hello from telegram",
            None,
        )
        .unwrap();
        let authority1 = manager.route_envelope(&envelope1).unwrap();
        let envelope2 = zaion_types::envelope::CanonicalEnvelope::new(
            "slack",
            principal_id.clone(),
            zaion_types::session::ChannelId("slack-team".to_string()),
            zaion_types::session::ThreadId("phase8".to_string()),
            "m2",
            "hello from slack",
            None,
        )
        .unwrap();
        let authority2 = manager.route_envelope(&envelope2).unwrap();

        assert_eq!(authority2.message_count, 2);
        assert_eq!(authority2.attachment_count, 2);
        assert_eq!(authority2.session_graph_hash.len(), 64);

        let route_event1 = signed_route_event(
            "evt-route-1",
            &principal_id,
            authority1.to_ledger_payload("evt-received-1"),
            "2026-05-04T00:00:01Z",
            Some("evt-received-1"),
        );
        let route_event2 = signed_route_event(
            "evt-route-2",
            &principal_id,
            authority2.to_ledger_payload("evt-received-2"),
            "2026-05-04T00:00:02Z",
            Some("evt-received-2"),
        );

        let mut replayed = OmniSessionManager::new(100000);
        let replay = replayed
            .replay_signed_route_events(&[route_event2, route_event1], Some(principal_id.as_str()))
            .unwrap();

        assert_eq!(replay.schema, "zaion.omni_session_graph_replay.v1");
        assert_eq!(replay.principal_id, principal_id.as_str());
        assert_eq!(replay.route_event_count, 2);
        assert_eq!(replay.message_count, 2);
        assert_eq!(replay.attachment_count, 2);
        assert_eq!(replay.active_omni_session_id, authority2.omni_session_id);
        assert_eq!(replay.last_route_event_id.as_deref(), Some("evt-route-2"));
        assert_eq!(
            replay.last_authority_hash.as_deref(),
            Some(authority2.authority_hash().as_str())
        );
        assert_eq!(replay.replay_hash.len(), 64);
        assert_eq!(replayed.session_count(), 1);
        assert_eq!(replayed.channel_count(), 2);

        let replayed_session = replayed.get_session(&principal_id).unwrap();
        assert_eq!(replayed_session.messages.len(), 2);
        assert_eq!(replayed_session.attachments.len(), 2);
    }

    fn signed_route_event(
        event_id: &str,
        principal_id: &PrincipalId,
        payload: serde_json::Value,
        created_at: &str,
        parent_event_id: Option<&str>,
    ) -> zaion_types::event::LedgerEvent {
        zaion_types::event::LedgerEvent {
            event_id: zaion_types::event::EventId(event_id.to_string()),
            principal_id: principal_id.clone(),
            namespace_key: zaion_types::session::NamespaceKey(principal_id.as_str().to_string()),
            run_id: None,
            event_type: "omni.route".to_string(),
            payload,
            signature: Some(zaion_types::identity::SignatureBytes(vec![7; 64])),
            created_at: created_at.to_string(),
            parent_event_id: parent_event_id.map(|id| zaion_types::event::EventId(id.to_string())),
        }
    }
}
