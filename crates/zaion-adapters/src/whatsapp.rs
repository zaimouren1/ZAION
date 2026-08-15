//! WhatsApp Business platform adapter implementation.
//!
//! Integrates with WhatsApp Cloud API:
//!   - Webhook for incoming messages
//!   - Send messages via Cloud API (text + media)
//!   - Bearer token authentication
//!
//! Security:
//!   H27 — single `reqwest::blocking::Client` reused for all API calls.
//!   Access token wrapped in `Zeroizing` for memory safety.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use std::sync::Mutex;
use std::time::Duration;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const GRAPH_API_BASE: &str = "https://graph.facebook.com/v18.0";

pub struct WhatsAppAdapter {
    access_token: Zeroizing<String>,
    phone_number_id: String,
    client: reqwest::blocking::Client,
    api_base_url: String,
    /// Buffer for messages received via webhook.
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppDeliveryReport {
    pub recipient_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

impl WhatsAppAdapter {
    pub fn new(access_token: impl Into<String>, phone_number_id: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed");

        Self {
            access_token: Zeroizing::new(access_token.into()),
            phone_number_id: phone_number_id.into(),
            client,
            api_base_url: GRAPH_API_BASE.to_string(),
            inbound_buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .no_proxy()
                .build()
            {
                self.client = client;
            }
        }
        self
    }

    fn messages_url(&self) -> String {
        format!("{}/{}/messages", self.api_base_url, self.phone_number_id)
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        redact_sensitive_values(text.into(), &[self.access_token.as_str()])
    }

    pub fn media_url(&self, media_id: &str) -> String {
        format!("{}/{}", self.api_base_url, media_id)
    }

    fn validate_token(&self) -> Result<(), AdapterError> {
        if self.access_token.is_empty() {
            return Err(AdapterError::Channel(
                "WhatsApp access_token not configured".into(),
            ));
        }
        Ok(())
    }

    /// Push a message into the inbound buffer (called by webhook handler).
    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    /// Send a media message (image, document, audio, video) via Cloud API.
    pub fn send_media(
        &self,
        to: &str,
        media_type: &str,
        media_url: &str,
        caption: Option<&str>,
    ) -> Result<(), AdapterError> {
        self.validate_token()?;

        let media_obj = match caption {
            Some(cap) => serde_json::json!({ "link": media_url, "caption": cap }),
            None => serde_json::json!({ "link": media_url }),
        };

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": media_type,
            media_type: media_obj,
        });

        let resp = self
            .client
            .post(self.messages_url())
            .header(
                "Authorization",
                format!("Bearer {}", self.access_token.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| {
                AdapterError::Channel(
                    self.redact_error(format!("WhatsApp API request failed: {}", e)),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "WhatsApp API HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        Ok(())
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<WhatsAppDeliveryReport, AdapterError> {
        self.validate_token()?;

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": msg.thread_id,
            "type": "text",
            "text": {
                "preview_url": false,
                "body": msg.text,
            }
        });

        let resp = self
            .client
            .post(self.messages_url())
            .header(
                "Authorization",
                format!("Bearer {}", self.access_token.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| {
                AdapterError::Channel(
                    self.redact_error(format!("WhatsApp API request failed: {}", e)),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "WhatsApp API HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| AdapterError::Channel(format!("WhatsApp response parse error: {}", e)))?;

        if let Some(err) = json.get("error") {
            let msg = err["message"].as_str().unwrap_or("unknown");
            return Err(AdapterError::Channel(format!(
                "WhatsApp API error: {}",
                self.redact_error(msg)
            )));
        }

        let message_id = json
            .pointer("/messages/0/id")
            .or_else(|| json.get("message_id"))
            .or_else(|| json.get("messageId"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        Ok(WhatsAppDeliveryReport {
            recipient_id: msg.thread_id.clone(),
            message_id,
            character_count: msg.text.chars().count(),
        })
    }
}

fn redact_sensitive_values(text: String, values: &[&str]) -> String {
    let mut redacted = text;
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        redacted = redacted.replace(value, "[REDACTED]");
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        if encoded != value {
            redacted = redacted.replace(&encoded, "[REDACTED]");
        }
    }
    redacted
}

impl ChannelAdapter for WhatsAppAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::WhatsApp
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("whatsapp inbound buffer lock poisoned".into()))?;
        let messages = buf.drain(..).collect();
        Ok(messages)
    }

    fn send(&self, msg: &OutboundMessage) -> Result<(), AdapterError> {
        self.send_with_report(msg)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn spawn_whatsapp_mock<F>(
        expected_requests: usize,
        mut handler: F,
    ) -> (String, thread::JoinHandle<()>)
    where
        F: FnMut(String) -> String + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(expected_requests) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = handler(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{}", addr), handle)
    }

    #[test]
    fn whatsapp_adapter_creation() {
        let adapter = WhatsAppAdapter::new("EAAx...", "1234567890");
        assert_eq!(adapter.access_token.as_str(), "EAAx...");
        assert_eq!(adapter.phone_number_id, "1234567890");
    }

    #[test]
    fn whatsapp_validate_token_rejects_empty() {
        let adapter = WhatsAppAdapter::new("", "12345");
        let err = adapter.validate_token().unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn whatsapp_validate_token_accepts_valid() {
        let adapter = WhatsAppAdapter::new("EAAx_valid", "12345");
        assert!(adapter.validate_token().is_ok());
    }

    #[test]
    fn whatsapp_channel_type() {
        let adapter = WhatsAppAdapter::new("token", "phone");
        assert_eq!(adapter.channel_type(), ChannelType::WhatsApp);
    }

    #[test]
    fn whatsapp_receive_empty_buffer() {
        let adapter = WhatsAppAdapter::new("token", "phone");
        let msgs = adapter.receive().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn whatsapp_push_and_receive() {
        let adapter = WhatsAppAdapter::new("token", "phone");
        adapter.push_inbound(InboundMessage {
            channel_id: "whatsapp".into(),
            thread_id: "+1234567890".into(),
            message_id: "wamid.abc123".into(),
            sender_id: "+1234567890".into(),
            text: "Hello from WhatsApp".into(),
            timestamp: "1650000000".into(),
            metadata: serde_json::json!({}),
        });
        let msgs = adapter.receive().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "Hello from WhatsApp");
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn whatsapp_send_rejects_empty_token() {
        let adapter = WhatsAppAdapter::new("", "phone");
        let msg = OutboundMessage {
            channel_id: "whatsapp".into(),
            thread_id: "+1234567890".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };
        let err = adapter.send(&msg).unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn whatsapp_messages_url_format() {
        let adapter = WhatsAppAdapter::new("token", "9876543210");
        assert_eq!(
            adapter.messages_url(),
            "https://graph.facebook.com/v18.0/9876543210/messages"
        );
    }

    #[test]
    fn whatsapp_api_base_override_formats_urls() {
        let adapter =
            WhatsAppAdapter::new("token", "phone").with_api_base_url("http://127.0.0.1:9915/");
        assert_eq!(
            adapter.messages_url(),
            "http://127.0.0.1:9915/phone/messages"
        );
        assert_eq!(
            adapter.media_url("media123"),
            "http://127.0.0.1:9915/media123"
        );
    }

    #[test]
    fn whatsapp_media_url_format() {
        let adapter = WhatsAppAdapter::new("token", "phone");
        assert_eq!(
            adapter.media_url("media123"),
            "https://graph.facebook.com/v18.0/media123"
        );
    }

    #[test]
    fn whatsapp_access_token_wrapped_in_zeroizing() {
        let adapter = WhatsAppAdapter::new("secret", "phone");
        let _: &Zeroizing<String> = &adapter.access_token;
    }

    #[test]
    fn whatsapp_api_errors_redact_access_token() {
        let token = "whatsapp-access-token-live";
        let (base_url, server) = spawn_whatsapp_mock(1, move |_request| {
            serde_json::json!({
                "error": {
                    "message": format!("invalid OAuth access token {token}")
                }
            })
            .to_string()
        });
        let adapter = WhatsAppAdapter::new(token, "phone").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "whatsapp".into(),
            thread_id: "15551234567".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let err = adapter.send_with_report(&msg).unwrap_err().to_string();
        assert!(!err.contains(token), "error leaked access token: {err}");
        assert!(err.contains("[REDACTED]"), "error was not redacted: {err}");
        server.join().unwrap();
    }
}
