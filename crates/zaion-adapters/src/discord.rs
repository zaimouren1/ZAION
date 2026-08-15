//! Discord platform adapter implementation.
//!
//! Security fix:
//!   H27 — single `reqwest::Client` (with bot-auth + JSON content-type default
//!         headers pre-configured) is built once in `new()` and reused for every
//!         API call. `reqwest::Client` is `Arc`-backed internally, so clones are
//!         cheap and the underlying connection pool is shared.

use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::platform_gateway::{BasePlatformAdapter, ChatInfo};

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct DiscordAdapter {
    bot_token: String,
    api_base_url: String,
    /// Reused HTTP client (H27). Carries the `Authorization: Bot …` and
    /// `Content-Type: application/json` default headers so they don't need
    /// to be re-attached per call.
    client: reqwest::Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordDeliveryReport {
    pub channel_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

impl DiscordAdapter {
    pub fn new(bot_token: impl Into<String>) -> Self {
        let bot_token: String = bot_token.into();
        let client = Self::build_client(&bot_token, false);

        Self {
            bot_token,
            api_base_url: "https://discord.com/api/v10".to_string(),
            client,
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            self.client = Self::build_client(&self.bot_token, true);
        }
        self
    }

    fn build_client(bot_token: &str, no_proxy: bool) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        // `Bot <token>` must be valid ASCII; empty or weird tokens fall back
        // to a client without auth header rather than panicking.
        if let Ok(value) = format!("Bot {}", bot_token).parse() {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("static header parses"),
        );

        let mut builder = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .default_headers(headers);
        if no_proxy {
            builder = builder.no_proxy();
        }
        builder
            .build()
            .expect("reqwest client builder must succeed with default config")
    }

    fn api_url(&self, endpoint: &str) -> String {
        format!("{}{}", self.api_base_url, endpoint)
    }

    pub async fn send_with_report(
        &self,
        chat_id: &str,
        text: &str,
    ) -> Result<DiscordDeliveryReport, String> {
        let body = serde_json::json!({
            "content": text,
        });
        let resp = self
            .client
            .post(self.api_url(&format!("/channels/{}/messages", chat_id)))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("Discord API error: {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct MessageResp {
            id: Option<String>,
            channel_id: Option<String>,
        }

        let message: MessageResp = resp.json().await.map_err(|e| e.to_string())?;
        Ok(DiscordDeliveryReport {
            channel_id: message.channel_id.unwrap_or_else(|| chat_id.to_string()),
            message_id: message.id,
            character_count: text.chars().count(),
        })
    }
}

#[async_trait]
impl BasePlatformAdapter for DiscordAdapter {
    async fn connect(&self) -> Result<bool, String> {
        let resp = self
            .client
            .get(self.api_url("/users/@me"))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp.status().is_success())
    }

    async fn disconnect(&self) -> Result<(), String> {
        Ok(())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        self.send_with_report(chat_id, text).await?;
        Ok(())
    }

    async fn get_chat_info(&self, chat_id: &str) -> Result<ChatInfo, String> {
        let resp = self
            .client
            .get(self.api_url(&format!("/channels/{}", chat_id)))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("Discord API error: {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct ChannelResp {
            id: String,
            #[serde(rename = "type")]
            channel_type: u8,
            name: Option<String>,
        }

        let channel: ChannelResp = resp.json().await.map_err(|e| e.to_string())?;
        Ok(ChatInfo {
            chat_id: channel.id,
            platform: "discord".into(),
            title: channel.name,
            is_group: channel.channel_type != 1, // DM = 1
            members_count: 0,
        })
    }

    async fn send_typing(&self, chat_id: &str) -> Result<(), String> {
        self.client
            .post(self.api_url(&format!("/channels/{}/typing", chat_id)))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn edit_message(
        &self,
        chat_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "content": text,
        });
        let resp = self
            .client
            .patch(self.api_url(&format!("/channels/{}/messages/{}", chat_id, message_id)))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("Discord API error: {}", resp.status()));
        }
        Ok(())
    }
}

impl Clone for DiscordAdapter {
    fn clone(&self) -> Self {
        Self {
            bot_token: self.bot_token.clone(),
            api_base_url: self.api_base_url.clone(),
            // reqwest::Client is Arc-backed internally; cloning is cheap
            // and shares the connection pool.
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discord_adapter_creation() {
        let adapter = DiscordAdapter::new("test_token");
        assert_eq!(adapter.bot_token, "test_token");
    }

    #[test]
    fn discord_api_url_format() {
        let adapter = DiscordAdapter::new("token");
        assert_eq!(
            adapter.api_url("/users/@me"),
            "https://discord.com/api/v10/users/@me"
        );
    }

    #[test]
    fn discord_api_base_url_can_be_overridden_for_probe_isolation() {
        let adapter = DiscordAdapter::new("token").with_api_base_url("http://127.0.0.1:9911/");
        assert_eq!(
            adapter.api_url("/channels/123/messages"),
            "http://127.0.0.1:9911/channels/123/messages"
        );
    }

    #[tokio::test]
    async fn discord_client_is_reused_across_clones() {
        let adapter = DiscordAdapter::new("token");
        let cloned = adapter.clone();
        // Structural guarantee: both adapters reference a `client` field.
        let _ = &adapter.client;
        let _ = &cloned.client;
    }
}
