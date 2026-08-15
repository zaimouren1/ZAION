//! SMS relay platform adapter implementation.
//!
//! The adapter targets a Twilio-compatible HTTP API so webhook delivery
//! matrices can probe SMS delivery through isolated local mocks.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const TWILIO_API_BASE: &str = "https://api.twilio.com/2010-04-01/Accounts";
const SEEN_TWILIO_MESSAGE_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsDeliveryReport {
    pub recipient: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsTwilioWebhookAck {
    pub status_code: u16,
    pub content_type: String,
    pub body: String,
    pub enqueued: bool,
    pub message_id: Option<String>,
    pub sender_id: Option<String>,
    pub thread_id: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsTwilioWebhookRequest {
    pub method: String,
    pub path: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsTwilioWebhookResponse {
    pub status_code: u16,
    pub content_type: String,
    pub body: String,
    pub ack: SmsTwilioWebhookAck,
}

pub struct SmsAdapter {
    account_sid: String,
    auth_token: Zeroizing<String>,
    from_number: String,
    api_base_url: String,
    client: Option<reqwest::blocking::Client>,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
    seen_twilio_message_ids: Mutex<VecDeque<String>>,
}

impl SmsAdapter {
    pub fn new(
        account_sid: impl Into<String>,
        auth_token: impl Into<String>,
        from_number: impl Into<String>,
    ) -> Self {
        let client =
            build_blocking_client(false).expect("reqwest::blocking::Client builder must succeed");

        Self {
            account_sid: account_sid.into(),
            auth_token: Zeroizing::new(auth_token.into()),
            from_number: from_number.into(),
            api_base_url: TWILIO_API_BASE.to_string(),
            client: Some(client),
            inbound_buffer: Mutex::new(Vec::new()),
            seen_twilio_message_ids: Mutex::new(VecDeque::new()),
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = build_blocking_client(true) {
                let old_client = self.client.replace(client);
                drop_blocking_client_safely(old_client);
            }
        }
        self
    }

    fn messages_url(&self) -> String {
        format!("{}/{}/Messages.json", self.api_base_url, self.account_sid)
    }

    fn validate_credentials(&self) -> Result<(), AdapterError> {
        if self.account_sid.trim().is_empty()
            || self.auth_token.trim().is_empty()
            || self.from_number.trim().is_empty()
        {
            return Err(AdapterError::Channel(
                "SMS relay credentials not configured".into(),
            ));
        }
        Ok(())
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        let basic = format!("{}:{}", self.account_sid, self.auth_token.as_str());
        let encoded_basic: String =
            base64::engine::general_purpose::STANDARD.encode(basic.as_bytes());
        redact_sensitive_values(
            text.into(),
            &[
                self.account_sid.as_str(),
                self.auth_token.as_str(),
                self.from_number.as_str(),
                &basic,
                &encoded_basic,
            ],
        )
    }

    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn ingest_twilio_form(&self, raw: &[u8]) -> Result<Option<InboundMessage>, AdapterError> {
        self.validate_credentials()?;
        let raw = std::str::from_utf8(raw)
            .map_err(|e| AdapterError::Channel(format!("SMS Twilio form parse error: {}", e)))?;
        let fields = parse_urlencoded_form(raw)?;
        let from_number = form_field(&fields, "From").unwrap_or_default();
        let to_number = form_field(&fields, "To").unwrap_or_default();
        let text = form_field(&fields, "Body").unwrap_or_default();
        let message_sid =
            form_field(&fields, "MessageSid").unwrap_or_else(|| stable_sms_message_id(raw));

        if from_number.trim().is_empty() || text.trim().is_empty() {
            return Ok(None);
        }
        if from_number == self.from_number {
            return Ok(None);
        }

        Ok(Some(InboundMessage {
            channel_id: "sms".into(),
            thread_id: from_number.clone(),
            sender_id: from_number.clone(),
            text,
            message_id: message_sid.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            metadata: serde_json::json!({
                "provider": "twilio",
                "transport": "twilio_form_webhook",
                "from": from_number,
                "to": to_number,
                "message_sid": message_sid,
                "account_sid_hash": hash_sms_metadata_value(&self.account_sid),
                "raw_field_count": fields.len(),
            }),
        }))
    }

    pub fn ingest_twilio_form_to_buffer(&self, raw: &[u8]) -> Result<bool, AdapterError> {
        let Some(message) = self.ingest_twilio_form(raw)? else {
            return Ok(false);
        };
        self.push_inbound(message);
        Ok(true)
    }

    pub fn ingest_twilio_form_to_buffer_once(
        &self,
        raw: &[u8],
    ) -> Result<Option<InboundMessage>, AdapterError> {
        let Some(message) = self.ingest_twilio_form(raw)? else {
            return Ok(None);
        };
        if !self.mark_twilio_message_seen(&message.message_id)? {
            return Ok(None);
        }
        self.push_inbound(message.clone());
        Ok(Some(message))
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<SmsDeliveryReport, AdapterError> {
        self.validate_credentials()?;
        let recipient = msg.thread_id.trim();
        if recipient.is_empty() {
            return Err(AdapterError::Channel(
                "SMS recipient number not configured".into(),
            ));
        }

        let form = [
            ("From", self.from_number.as_str()),
            ("To", recipient),
            ("Body", msg.text.as_str()),
        ];

        let client = self.client.as_ref().ok_or_else(|| {
            AdapterError::Channel("SMS relay HTTP client is not available".into())
        })?;

        let resp = client
            .post(self.messages_url())
            .basic_auth(&self.account_sid, Some(self.auth_token.as_str()))
            .form(&form)
            .send()
            .map_err(|e| {
                AdapterError::Channel(self.redact_error(format!("SMS relay request failed: {}", e)))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "SMS relay HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| AdapterError::Channel(format!("SMS relay response parse error: {}", e)))?;

        if let Some(err) = json.get("error").or_else(|| json.get("message")) {
            if json.get("sid").is_none() {
                let err = err.as_str().unwrap_or("unknown");
                return Err(AdapterError::Channel(format!(
                    "SMS relay error: {}",
                    self.redact_error(err)
                )));
            }
        }

        let message_id = json
            .get("sid")
            .or_else(|| json.get("id"))
            .or_else(|| json.get("message_id"))
            .or_else(|| json.get("messageId"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        Ok(SmsDeliveryReport {
            recipient: recipient.to_string(),
            message_id,
            character_count: msg.text.chars().count(),
        })
    }

    fn mark_twilio_message_seen(&self, message_id: &str) -> Result<bool, AdapterError> {
        let mut seen = self
            .seen_twilio_message_ids
            .lock()
            .map_err(|_| AdapterError::Channel("sms twilio idempotency lock poisoned".into()))?;
        if seen.iter().any(|seen_id| seen_id == message_id) {
            return Ok(false);
        }
        seen.push_back(message_id.to_string());
        while seen.len() > SEEN_TWILIO_MESSAGE_LIMIT {
            seen.pop_front();
        }
        Ok(true)
    }
}

impl Drop for SmsAdapter {
    fn drop(&mut self) {
        drop_blocking_client_safely(self.client.take());
    }
}

fn drop_blocking_client_safely(client: Option<reqwest::blocking::Client>) {
    let Some(client) = client else {
        return;
    };
    let _ = std::thread::spawn(move || drop(client)).join();
}

fn build_blocking_client(no_proxy: bool) -> Result<reqwest::blocking::Client, reqwest::Error> {
    std::thread::spawn(move || {
        let mut builder = reqwest::blocking::Client::builder().timeout(HTTP_TIMEOUT);
        if no_proxy {
            builder = builder.no_proxy();
        }
        builder.build()
    })
    .join()
    .expect("reqwest blocking client builder thread must not panic")
}

pub struct SmsTwilioWebhookService<'a> {
    adapter: &'a SmsAdapter,
}

impl<'a> SmsTwilioWebhookService<'a> {
    pub fn new(adapter: &'a SmsAdapter) -> Self {
        Self { adapter }
    }

    pub fn handle_http_request(
        &self,
        request: SmsTwilioWebhookRequest,
    ) -> Result<SmsTwilioWebhookResponse, AdapterError> {
        if !request.method.eq_ignore_ascii_case("POST") {
            return Ok(Self::http_response(
                Self::empty_ack(false, None, None, None, None),
                405,
            ));
        }
        if !request
            .content_type
            .to_ascii_lowercase()
            .starts_with("application/x-www-form-urlencoded")
        {
            return Ok(Self::http_response(
                Self::empty_ack(false, None, None, None, None),
                415,
            ));
        }
        if !request.path.trim_start_matches('/').contains("twilio") {
            return Ok(Self::http_response(
                Self::empty_ack(false, None, None, None, None),
                404,
            ));
        }

        let ack = self.handle_form(&request.body)?;
        Ok(Self::http_response(ack, 200))
    }

    pub fn handle_form(&self, raw: &[u8]) -> Result<SmsTwilioWebhookAck, AdapterError> {
        let Some(message) = self.adapter.ingest_twilio_form_to_buffer_once(raw)? else {
            return Ok(Self::empty_ack(false, None, None, None, None));
        };
        let message_id = message.message_id.clone();
        let sender_id = message.sender_id.clone();
        let thread_id = message.thread_id.clone();
        let text = message.text.clone();
        Ok(Self::empty_ack(
            true,
            Some(message_id),
            Some(sender_id),
            Some(thread_id),
            Some(text),
        ))
    }

    fn empty_ack(
        enqueued: bool,
        message_id: Option<String>,
        sender_id: Option<String>,
        thread_id: Option<String>,
        text: Option<String>,
    ) -> SmsTwilioWebhookAck {
        SmsTwilioWebhookAck {
            status_code: 200,
            content_type: "application/xml".to_string(),
            body: r#"<?xml version="1.0" encoding="UTF-8"?><Response></Response>"#.to_string(),
            enqueued,
            message_id,
            sender_id,
            thread_id,
            text,
        }
    }

    fn http_response(ack: SmsTwilioWebhookAck, status_code: u16) -> SmsTwilioWebhookResponse {
        let ack = SmsTwilioWebhookAck { status_code, ..ack };
        SmsTwilioWebhookResponse {
            status_code,
            content_type: ack.content_type.clone(),
            body: ack.body.clone(),
            ack,
        }
    }
}

fn parse_urlencoded_form(raw: &str) -> Result<BTreeMap<String, String>, AdapterError> {
    let mut fields = BTreeMap::new();
    for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        fields.insert(percent_decode_form(key)?, percent_decode_form(value)?);
    }
    Ok(fields)
}

fn form_field(fields: &BTreeMap<String, String>, name: &str) -> Option<String> {
    fields.get(name).cloned()
}

fn percent_decode_form(value: &str) -> Result<String, AdapterError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_value(bytes[i + 1]).ok_or_else(|| {
                    AdapterError::Channel("SMS Twilio form contains invalid percent escape".into())
                })?;
                let lo = hex_value(bytes[i + 2]).ok_or_else(|| {
                    AdapterError::Channel("SMS Twilio form contains invalid percent escape".into())
                })?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'%' => {
                return Err(AdapterError::Channel(
                    "SMS Twilio form contains truncated percent escape".into(),
                ))
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out)
        .map_err(|e| AdapterError::Channel(format!("SMS Twilio form UTF-8 error: {}", e)))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn stable_sms_message_id(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    format!("sms:{}", hex::encode(hasher.finalize()))
}

fn hash_sms_metadata_value(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
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

impl ChannelAdapter for SmsAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Sms
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_credentials()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("sms inbound buffer lock poisoned".into()))?;
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

    fn spawn_sms_mock<F>(
        expected_requests: usize,
        mut handler: F,
    ) -> (String, thread::JoinHandle<()>)
    where
        F: FnMut(String) -> (u16, String) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(expected_requests) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let (status, body) = handler(request);
                let reason = if status >= 400 { "Bad Request" } else { "OK" };
                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    reason,
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{}", addr), handle)
    }

    #[test]
    fn sms_adapter_rejects_empty_credentials() {
        let adapter = SmsAdapter::new("", "", "");
        let err = adapter.validate_credentials().unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn sms_api_base_url_can_be_overridden_for_probe_isolation() {
        let adapter = SmsAdapter::new("AC123", "secret", "+15551234567")
            .with_api_base_url("http://127.0.0.1:9918/");
        assert_eq!(
            adapter.messages_url(),
            "http://127.0.0.1:9918/AC123/Messages.json"
        );
    }

    #[test]
    fn sms_channel_type_is_stable() {
        let adapter = SmsAdapter::new("AC123", "secret", "+15551234567");
        assert_eq!(adapter.channel_type(), ChannelType::Sms);
    }

    #[test]
    fn sms_api_errors_redact_auth_material() {
        let account_sid = "AC123456789";
        let auth_token = "sms-auth-token-live";
        let from_number = "+15551234567";
        let (base_url, server) = spawn_sms_mock(1, move |_request| {
            (
                400,
                serde_json::json!({
                    "message": format!("invalid credentials {account_sid}:{auth_token} from {from_number}")
                })
                .to_string(),
            )
        });
        let adapter =
            SmsAdapter::new(account_sid, auth_token, from_number).with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "sms".into(),
            thread_id: "+15551230000".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let err = adapter.send_with_report(&msg).unwrap_err().to_string();
        assert!(
            !err.contains(account_sid),
            "error leaked account SID: {err}"
        );
        assert!(!err.contains(auth_token), "error leaked auth token: {err}");
        assert!(
            !err.contains(from_number),
            "error leaked from number: {err}"
        );
        assert!(err.contains("[REDACTED]"), "error was not redacted: {err}");
        server.join().unwrap();
    }

    #[test]
    fn sms_ingest_twilio_form_builds_canonical_inbound() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let form = "From=%2B15551230000&To=%2B15551234567&Body=hello+from+sms&MessageSid=SMabc123";

        let message = adapter
            .ingest_twilio_form(form.as_bytes())
            .unwrap()
            .expect("twilio form should produce an inbound message");

        assert_eq!(message.channel_id, "sms");
        assert_eq!(message.thread_id, "+15551230000");
        assert_eq!(message.sender_id, "+15551230000");
        assert_eq!(message.text, "hello from sms");
        assert_eq!(message.message_id, "SMabc123");
        assert_eq!(message.metadata["to"], "+15551234567");
        assert_eq!(message.metadata["provider"], "twilio");
        assert!(message.metadata["account_sid"].is_null());
        assert_eq!(
            message.metadata["account_sid_hash"]
                .as_str()
                .unwrap_or_default()
                .len(),
            64
        );

        let envelope = message
            .to_canonical_envelope(
                "sms",
                zaion_types::identity::PrincipalId("did:key:sms-inbound".into()),
            )
            .expect("sms inbound should be canonical-envelope ready");
        assert_eq!(envelope.channel.0, "sms");
        assert_eq!(envelope.thread.0, "+15551230000");
        assert_eq!(envelope.metadata["adapter_metadata"]["provider"], "twilio");
        assert_eq!(envelope.source_hash.len(), 64);
    }

    #[test]
    fn sms_ingest_twilio_form_drops_echo_from_own_number() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let form = "From=%2B15551234567&To=%2B15551230000&Body=echo&MessageSid=SMecho";

        let message = adapter.ingest_twilio_form(form.as_bytes()).unwrap();

        assert!(message.is_none());
    }

    #[test]
    fn sms_ingest_twilio_form_to_buffer_feeds_channel_receive() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let form = "From=%2B15551230000&To=%2B15551234567&Body=buffer+me&MessageSid=SMbuffer";

        assert!(adapter
            .ingest_twilio_form_to_buffer(form.as_bytes())
            .unwrap());
        let received = adapter.receive().unwrap();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].thread_id, "+15551230000");
        assert_eq!(received[0].metadata["provider"], "twilio");
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn sms_ingest_twilio_form_to_buffer_once_deduplicates_message_sid() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let form = "From=%2B15551230000&To=%2B15551234567&Body=retry&MessageSid=SMretry";

        let first = adapter
            .ingest_twilio_form_to_buffer_once(form.as_bytes())
            .unwrap();
        let second = adapter
            .ingest_twilio_form_to_buffer_once(form.as_bytes())
            .unwrap();

        assert!(first.is_some());
        assert!(second.is_none());
        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message_id, "SMretry");
    }

    #[test]
    fn sms_twilio_webhook_service_acknowledges_and_buffers_inbound_form() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let service = SmsTwilioWebhookService::new(&adapter);
        let form = "From=%2B15551230000&To=%2B15551234567&Body=service+entry&MessageSid=SMservice";

        let ack = service.handle_form(form.as_bytes()).unwrap();

        assert_eq!(ack.status_code, 200);
        assert_eq!(ack.content_type, "application/xml");
        assert!(ack.body.contains("<Response></Response>"));
        assert!(ack.enqueued);
        assert_eq!(ack.message_id.as_deref(), Some("SMservice"));

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "service entry");
        assert_eq!(received[0].metadata["transport"], "twilio_form_webhook");
    }

    #[test]
    fn sms_twilio_webhook_service_acknowledges_echo_without_buffering() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let service = SmsTwilioWebhookService::new(&adapter);
        let form = "From=%2B15551234567&To=%2B15551230000&Body=echo&MessageSid=SMecho";

        let ack = service.handle_form(form.as_bytes()).unwrap();

        assert_eq!(ack.status_code, 200);
        assert!(!ack.enqueued);
        assert!(ack.message_id.is_none());
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn sms_twilio_webhook_service_handles_http_form_request() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let service = SmsTwilioWebhookService::new(&adapter);
        let request = SmsTwilioWebhookRequest {
            method: "POST".to_string(),
            path: "/sms/twilio".to_string(),
            content_type: "application/x-www-form-urlencoded".to_string(),
            body: b"From=%2B15551230000&To=%2B15551234567&Body=http+entry&MessageSid=SMhttp"
                .to_vec(),
        };

        let response = service.handle_http_request(request).unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "application/xml");
        assert!(response.body.contains("<Response></Response>"));
        assert_eq!(response.ack.message_id.as_deref(), Some("SMhttp"));

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "http entry");
    }

    #[test]
    fn sms_twilio_webhook_service_rejects_non_form_http_request_without_buffering() {
        let adapter = SmsAdapter::new("AC123", "sms-auth-token", "+15551234567");
        let service = SmsTwilioWebhookService::new(&adapter);
        let request = SmsTwilioWebhookRequest {
            method: "GET".to_string(),
            path: "/sms/twilio".to_string(),
            content_type: "text/plain".to_string(),
            body: b"not a form".to_vec(),
        };

        let response = service.handle_http_request(request).unwrap();

        assert_eq!(response.status_code, 405);
        assert_eq!(response.content_type, "application/xml");
        assert_eq!(response.ack.status_code, 405);
        assert!(!response.ack.enqueued);
        assert!(adapter.receive().unwrap().is_empty());
    }
}
