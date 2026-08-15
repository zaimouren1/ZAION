pub mod agui;
pub mod channel;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod homeassistant;
pub mod matrix;
pub mod mattermost;
pub mod platform_gateway;
pub mod provider;
pub mod retry;
pub mod signal;
pub mod slack;
pub mod smart_router;
pub mod sms;
pub mod telegram_adapter;
pub mod tool_parsers;
pub mod webhook_runtime;
pub mod wechat;
pub mod whatsapp;

#[cfg(test)]
mod tests;

pub use agui::*;
pub use channel::*;
pub use dingtalk::DingTalkAdapter;
pub use discord::DiscordAdapter;
pub use email::{
    EmailAdapter, EmailFetchedMessage, EmailInboundPollReport, EmailInboundPollService,
    EmailInboundProvenance, EmailPollSource,
};
pub use feishu::FeishuAdapter;
pub use homeassistant::{
    HomeAssistantAdapter, HomeAssistantInboundProvenance, HomeAssistantWebSocketInboundService,
};
pub use matrix::MatrixAdapter;
pub use mattermost::MattermostAdapter;
pub use platform_gateway::{
    chunk_message_for_platform, merge_album_photos, BasePlatformAdapter, ChatInfo, InterruptMode,
    MediaCacheManager, MessageType, UnifiedMessageEvent,
};
pub use provider::*;
pub use retry::{RetryConfig, RetryProvider};
pub use signal::{SignalAdapter, SignalInboundProvenance, SignalSseInboundService};
pub use slack::SlackAdapter;
pub use smart_router::{CheapModel, RouteDecision, RouterConfig, RouterContext, SmartRouter};
pub use sms::{
    SmsAdapter, SmsTwilioWebhookAck, SmsTwilioWebhookRequest, SmsTwilioWebhookResponse,
    SmsTwilioWebhookService,
};
pub use telegram_adapter::{TelegramAdapter, TelegramDeliveryReport};
pub use tool_parsers::{all_parser_names, get_parser, try_all_parsers, ToolCallParser};
pub use webhook_runtime::{
    DeliveryReceipt, WebhookAgentDispatch, WebhookAgentDispatchResult, WebhookAgentHandler,
    WebhookProvenance, WebhookRoute, WebhookRuntime, WebhookRuntimeConfig,
};
pub use wechat::WeChatAdapter;
pub use whatsapp::WhatsAppAdapter;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("channel error: {0}")]
    Channel(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("runtime error: {0}")]
    Runtime(String),
}
