use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEvent {
    pub channel_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub delivery_id: Option<String>,
    pub principal_hint: Option<String>,
    pub text: String,
    pub attachments: Vec<serde_json::Value>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel_id: String,
    pub thread_id: String,
    pub text: String,
    pub in_reply_to: Option<String>,
    pub attachments: Vec<serde_json::Value>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}
