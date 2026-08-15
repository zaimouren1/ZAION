//! Signal platform adapter implementation.
//!
//! This adapter targets the signal-cli HTTP daemon JSON-RPC send path so
//! webhook delivery probes can verify Signal delivery through isolated mocks
//! or an explicit `SIGNAL_HTTP_URL` deployment target.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::webhook_runtime::DeliveryReceipt;
use crate::AdapterError;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zaion_crypto::ZaionKeypair;
use zaion_types::identity::SignatureBytes;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const SIGNAL_HTTP_URL_DEFAULT: &str = "http://127.0.0.1:8080";
const SIGNAL_MESSAGE_LIMIT: usize = 8000;
const SIGNAL_MAX_ATTACHMENT_SIZE: u64 = 100 * 1024 * 1024;
const SIGNAL_SSE_RETRY_DELAY_INITIAL: Duration = Duration::from_secs(2);
const SIGNAL_SSE_RETRY_DELAY_MAX: Duration = Duration::from_secs(60);

pub struct SignalAdapter {
    account: Zeroizing<String>,
    api_base_url: String,
    client: Option<reqwest::blocking::Client>,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
    attachment_cache_dir: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalSseIngestReport {
    pub data_event_count: usize,
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalSseProvenanceIngestReport {
    pub data_event_count: usize,
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
    pub provenance_count: usize,
    pub provenance_delivery_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalSseLifecycleReport {
    pub health_url: String,
    pub event_url: String,
    pub accept_header: String,
    pub connect_attempts: usize,
    pub chunk_count: usize,
    pub data_event_count: usize,
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
    pub reconnect_backoff_millis: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalDeliveryReport {
    pub recipient_id: String,
    pub message_id: Option<String>,
    pub chunk_count: usize,
    pub character_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalAttachmentCacheRecord {
    pub attachment_id: String,
    pub cache_path: String,
    pub extension: String,
    pub content_type: String,
    pub media_kind: String,
    pub byte_len: usize,
    pub payload_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignalInboundProvenance {
    pub route_name: String,
    pub delivery_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub thread_id: String,
    pub timestamp: u64,
    pub payload_hash: String,
    pub principal_id: String,
    pub receipt_timestamp: u64,
    pub receipt_signature: String,
    pub receipt_schema_version: u32,
}

impl SignalInboundProvenance {
    pub fn to_delivery_receipt(&self) -> DeliveryReceipt {
        DeliveryReceipt {
            route_name: self.route_name.clone(),
            delivery_id: self.delivery_id.clone(),
            timestamp: self.receipt_timestamp,
            payload_hash: self.payload_hash.clone(),
            signature_valid: true,
            principal_id: self.principal_id.clone(),
            ed25519_signature: self.receipt_signature.clone(),
            schema_version: self.receipt_schema_version,
        }
    }
}

pub struct SignalSseInboundService<'a> {
    adapter: &'a SignalAdapter,
    provenance_ledger: Mutex<Vec<SignalInboundProvenance>>,
    signing_key: Arc<ZaionKeypair>,
}

impl<'a> SignalSseInboundService<'a> {
    #[cfg(test)]
    pub fn new(adapter: &'a SignalAdapter) -> Self {
        Self::new_with_key(adapter, Arc::new(ZaionKeypair::generate()))
    }

    pub fn new_with_key(adapter: &'a SignalAdapter, signing_key: Arc<ZaionKeypair>) -> Self {
        Self {
            adapter,
            provenance_ledger: Mutex::new(Vec::new()),
            signing_key,
        }
    }

    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn provenance_ledger(&self) -> Result<Vec<SignalInboundProvenance>, AdapterError> {
        self.provenance_ledger
            .lock()
            .map(|ledger| ledger.clone())
            .map_err(|_| AdapterError::Channel("signal provenance ledger lock poisoned".into()))
    }

    pub fn ingest_sse_chunk(
        &self,
        chunk: &str,
    ) -> Result<SignalSseProvenanceIngestReport, AdapterError> {
        self.adapter.validate_account()?;
        let mut report = SignalSseProvenanceIngestReport::default();
        for line in chunk.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
                continue;
            }
            report.data_event_count += 1;
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() {
                report.ignored_count += 1;
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                report.invalid_count += 1;
                continue;
            };
            let Some(mut message) = self.adapter.ingest_envelope(&value)? else {
                report.ignored_count += 1;
                continue;
            };
            let payload_hash = inbound_payload_hash(&message)?;
            let delivery_id = signal_delivery_id(&message);
            let receipt = self.generate_provenance_receipt(&delivery_id, &payload_hash)?;
            let provenance = SignalInboundProvenance {
                route_name: "signal.inbound.sse".into(),
                delivery_id: delivery_id.clone(),
                message_id: message.message_id.clone(),
                sender_id: message.sender_id.clone(),
                thread_id: message.thread_id.clone(),
                timestamp: now_unix_secs(),
                payload_hash: payload_hash.clone(),
                principal_id: receipt.principal_id.clone(),
                receipt_timestamp: receipt.timestamp,
                receipt_signature: receipt.ed25519_signature.clone(),
                receipt_schema_version: receipt.schema_version,
            };
            insert_provenance_metadata(&mut message, "signal_provenance", &provenance);
            self.record_provenance(provenance)?;
            self.adapter.push_inbound(message);
            report.accepted_count += 1;
            report.provenance_count += 1;
            report.provenance_delivery_ids.push(delivery_id);
        }
        Ok(report)
    }

    fn generate_provenance_receipt(
        &self,
        delivery_id: &str,
        payload_hash: &str,
    ) -> Result<DeliveryReceipt, AdapterError> {
        let route_name = "signal.inbound.sse";
        let timestamp = now_unix_secs();
        let digest =
            DeliveryReceipt::canonical_bytes(route_name, delivery_id, payload_hash, timestamp);
        let SignatureBytes(sig_bytes) = self.signing_key.sign(&digest);

        Ok(DeliveryReceipt {
            route_name: route_name.to_string(),
            delivery_id: delivery_id.to_string(),
            timestamp,
            payload_hash: payload_hash.to_string(),
            signature_valid: true,
            principal_id: self.signing_key.principal_id().to_string(),
            ed25519_signature: hex::encode(sig_bytes),
            schema_version: 2,
        })
    }

    fn record_provenance(&self, provenance: SignalInboundProvenance) -> Result<(), AdapterError> {
        self.provenance_ledger
            .lock()
            .map_err(|_| AdapterError::Channel("signal provenance ledger lock poisoned".into()))?
            .push(provenance);
        Ok(())
    }
}

impl SignalAdapter {
    pub fn new(account: impl Into<String>) -> Self {
        let client = build_signal_blocking_client(false)
            .expect("reqwest::blocking::Client builder must succeed");
        let api_base_url = std::env::var("SIGNAL_HTTP_URL")
            .unwrap_or_else(|_| SIGNAL_HTTP_URL_DEFAULT.to_string())
            .trim_end_matches('/')
            .to_string();

        Self {
            account: Zeroizing::new(account.into()),
            api_base_url,
            client: Some(client),
            inbound_buffer: Mutex::new(Vec::new()),
            attachment_cache_dir: std::env::temp_dir().join("zaion-signal-attachments"),
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = build_signal_blocking_client(true) {
                let old_client = self.client.replace(client);
                drop_signal_blocking_client_safely(old_client);
            }
        }
        self
    }

    pub fn with_attachment_cache_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.attachment_cache_dir = dir.as_ref().to_path_buf();
        self
    }

    fn validate_account(&self) -> Result<(), AdapterError> {
        if self.account.trim().is_empty() {
            return Err(AdapterError::Channel(
                "Signal account not configured".into(),
            ));
        }
        Ok(())
    }

    fn rpc_url(&self) -> Result<String, AdapterError> {
        let mut url = reqwest::Url::parse(&self.api_base_url)
            .map_err(|e| AdapterError::Channel(format!("Signal API base URL invalid: {}", e)))?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Signal API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "v1", "rpc"]);
        }
        Ok(url.to_string())
    }

    pub fn health_check_url(&self) -> Result<String, AdapterError> {
        let mut url = reqwest::Url::parse(&self.api_base_url)
            .map_err(|e| AdapterError::Channel(format!("Signal API base URL invalid: {}", e)))?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Signal API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "v1", "check"]);
        }
        Ok(url.to_string())
    }

    pub fn sse_event_url(&self) -> Result<String, AdapterError> {
        self.validate_account()?;
        let mut url = reqwest::Url::parse(&self.api_base_url)
            .map_err(|e| AdapterError::Channel(format!("Signal API base URL invalid: {}", e)))?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Signal API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "v1", "events"]);
        }
        url.query_pairs_mut()
            .clear()
            .append_pair("account", self.account.as_str());
        Ok(url.to_string())
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        redact_sensitive_values(text.into(), &[self.account.as_str()])
    }

    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn ingest_envelope(
        &self,
        raw: &serde_json::Value,
    ) -> Result<Option<InboundMessage>, AdapterError> {
        self.validate_account()?;
        let mut envelope_data = raw.get("envelope").unwrap_or(raw).clone();
        let mut is_note_to_self = false;

        if let Some(sync_message) = envelope_data.get("syncMessage").and_then(|v| v.as_object()) {
            let sent_message = sync_message.get("sentMessage");
            let destination = sent_message
                .and_then(|sent| {
                    sent.get("destinationNumber")
                        .or_else(|| sent.get("destination"))
                })
                .and_then(|value| value.as_str());
            if destination == Some(self.account.as_str()) {
                if let Some(sent_message) = sent_message {
                    envelope_data["dataMessage"] = sent_message.clone();
                    is_note_to_self = true;
                }
            } else {
                return Ok(None);
            }
        }

        if envelope_data.get("storyMessage").is_some() {
            return Ok(None);
        }

        let sender = value_at_paths(&envelope_data, &["/sourceNumber", "/sourceUuid", "/source"])
            .unwrap_or_default();
        if sender.trim().is_empty() {
            return Ok(None);
        }
        if sender == self.account.as_str() && !is_note_to_self {
            return Ok(None);
        }

        let data_message = envelope_data
            .get("dataMessage")
            .or_else(|| envelope_data.pointer("/editMessage/dataMessage"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if data_message.is_null() {
            return Ok(None);
        }

        let group_info = data_message.get("groupInfo");
        let group_id = group_info
            .and_then(|group| group.get("groupId"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let group_name = group_info
            .and_then(|group| group.get("groupName"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let chat_type = if group_id.is_some() { "group" } else { "dm" };
        let thread_id = group_id
            .as_ref()
            .map(|id| format!("group:{id}"))
            .unwrap_or_else(|| sender.clone());

        let mut text = data_message
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        text = render_signal_mentions(text, data_message.get("mentions"));
        let attachments = signal_attachment_metadata(&data_message);
        if text.trim().is_empty() && attachments.is_empty() {
            return Ok(None);
        }
        if text.trim().is_empty() {
            text = "[Signal attachment]".into();
        }

        let message_id = data_message
            .get("timestamp")
            .or_else(|| envelope_data.get("timestamp"))
            .or_else(|| data_message.get("serverGuid"))
            .and_then(json_scalar_to_string)
            .unwrap_or_else(|| format!("signal-{}", &stable_signal_hash(&envelope_data)[..16]));
        let timestamp = envelope_data
            .get("timestamp")
            .or_else(|| data_message.get("timestamp"))
            .and_then(json_scalar_to_string)
            .unwrap_or_else(|| now_millis().to_string());
        let source_uuid = envelope_data
            .get("sourceUuid")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let source_name = envelope_data
            .get("sourceName")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let attachment_count = attachments.len();

        Ok(Some(InboundMessage {
            channel_id: "signal".into(),
            thread_id,
            sender_id: sender.clone(),
            text,
            message_id: message_id.clone(),
            timestamp,
            metadata: serde_json::json!({
                "provider": "signal-cli",
                "transport": "signal_cli_sse",
                "chat_type": chat_type,
                "sender": sender,
                "source_uuid": source_uuid,
                "source_name": source_name,
                "group_id": group_id,
                "group_name": group_name,
                "message_id": message_id,
                "is_note_to_self": is_note_to_self,
                "attachment_count": attachment_count,
                "attachments": attachments,
                "raw_envelope_hash": stable_signal_hash(&envelope_data),
            }),
        }))
    }

    pub fn ingest_envelope_to_buffer(&self, raw: &serde_json::Value) -> Result<bool, AdapterError> {
        let Some(message) = self.ingest_envelope(raw)? else {
            return Ok(false);
        };
        self.push_inbound(message);
        Ok(true)
    }

    pub fn ingest_sse_chunk_to_buffer(
        &self,
        chunk: &str,
    ) -> Result<SignalSseIngestReport, AdapterError> {
        self.validate_account()?;
        let mut report = SignalSseIngestReport::default();
        for line in chunk.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
                continue;
            }
            report.data_event_count += 1;
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() {
                report.ignored_count += 1;
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                report.invalid_count += 1;
                continue;
            };
            if self.ingest_envelope_to_buffer(&value)? {
                report.accepted_count += 1;
            } else {
                report.ignored_count += 1;
            }
        }
        Ok(report)
    }

    pub fn run_sse_lifecycle_script_to_buffer<I, S>(
        &self,
        chunks: I,
        reconnect_backoff_steps: usize,
    ) -> Result<SignalSseLifecycleReport, AdapterError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.validate_account()?;
        let mut report = SignalSseLifecycleReport {
            health_url: self.health_check_url()?,
            event_url: self.sse_event_url()?,
            accept_header: "text/event-stream".into(),
            connect_attempts: 1,
            reconnect_backoff_millis: signal_sse_reconnect_backoff_millis(reconnect_backoff_steps),
            ..SignalSseLifecycleReport::default()
        };

        for chunk in chunks {
            report.chunk_count += 1;
            let ingest = self.ingest_sse_chunk_to_buffer(chunk.as_ref())?;
            report.data_event_count += ingest.data_event_count;
            report.accepted_count += ingest.accepted_count;
            report.ignored_count += ingest.ignored_count;
            report.invalid_count += ingest.invalid_count;
        }

        Ok(report)
    }

    pub fn fetch_attachment_to_cache(
        &self,
        attachment_id: &str,
    ) -> Result<SignalAttachmentCacheRecord, AdapterError> {
        self.validate_account()?;
        let attachment_id = attachment_id.trim();
        if attachment_id.is_empty() {
            return Err(AdapterError::Channel(
                "Signal attachment id not configured".into(),
            ));
        }

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getAttachment",
            "params": {
                "account": self.account.as_str(),
                "id": attachment_id,
            },
            "id": format!("zaion-signal-attachment-{}", now_millis()),
        });

        let resp = self
            .client
            .as_ref()
            .ok_or_else(|| AdapterError::Channel("Signal HTTP client unavailable".into()))?
            .post(&self.rpc_url()?)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| {
                AdapterError::Channel(
                    self.redact_error(format!("Signal attachment request failed: {}", e)),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "Signal attachment HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        let json: serde_json::Value = resp.json().map_err(|e| {
            AdapterError::Channel(format!("Signal attachment response parse error: {}", e))
        })?;
        if let Some(error) = json.get("error") {
            return Err(AdapterError::Channel(format!(
                "Signal attachment RPC error: {}",
                self.redact_error(error.to_string())
            )));
        }
        let encoded = json
            .pointer("/result/data")
            .or_else(|| json.get("result"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AdapterError::Channel("Signal attachment response missing data".into())
            })?;
        let payload = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| AdapterError::Channel(format!("Signal attachment base64 error: {e}")))?;
        if payload.len() as u64 > SIGNAL_MAX_ATTACHMENT_SIZE {
            return Err(AdapterError::Channel(
                "Signal attachment exceeds maximum size".into(),
            ));
        }

        std::fs::create_dir_all(&self.attachment_cache_dir).map_err(|e| {
            AdapterError::Channel(format!("Signal attachment cache create failed: {e}"))
        })?;
        let extension = signal_payload_extension(&payload);
        let content_type = signal_extension_content_type(extension);
        let media_kind = signal_content_media_kind(content_type);
        let safe_id = sanitize_signal_attachment_id(attachment_id);
        let cache_path = self
            .attachment_cache_dir
            .join(format!("{safe_id}.{extension}"));
        std::fs::write(&cache_path, &payload).map_err(|e| {
            AdapterError::Channel(format!("Signal attachment cache write failed: {e}"))
        })?;

        Ok(SignalAttachmentCacheRecord {
            attachment_id: attachment_id.to_string(),
            cache_path: cache_path.to_string_lossy().to_string(),
            extension: extension.to_string(),
            content_type: content_type.to_string(),
            media_kind: media_kind.to_string(),
            byte_len: payload.len(),
            payload_hash: sha256_hex(&payload),
        })
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<SignalDeliveryReport, AdapterError> {
        self.validate_account()?;
        let rpc_url = self.rpc_url()?;
        let chunks = chunk_signal_message(&msg.text, SIGNAL_MESSAGE_LIMIT);
        let mut message_ids = Vec::new();

        for (index, chunk) in chunks.iter().enumerate() {
            let mut params = serde_json::json!({
                "account": self.account.as_str(),
                "message": chunk,
            });
            if let Some(group_id) = msg.thread_id.strip_prefix("group:") {
                params["groupId"] = serde_json::Value::String(group_id.to_string());
            } else {
                params["recipient"] = serde_json::json!([msg.thread_id]);
            }

            let rpc_id = format!("zaion-signal-{}-{}", now_millis(), index);
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "send",
                "params": params,
                "id": rpc_id,
            });

            let resp = self
                .client
                .as_ref()
                .ok_or_else(|| AdapterError::Channel("Signal HTTP client unavailable".into()))?
                .post(&rpc_url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| {
                    AdapterError::Channel(
                        self.redact_error(format!("Signal API request failed: {}", e)),
                    )
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                return Err(AdapterError::Channel(format!(
                    "Signal API HTTP {}: {}",
                    status,
                    self.redact_error(text)
                )));
            }

            let json: serde_json::Value = resp.json().map_err(|e| {
                AdapterError::Channel(format!("Signal response parse error: {}", e))
            })?;
            if let Some(error) = json.get("error") {
                return Err(AdapterError::Channel(format!(
                    "Signal API error: {}",
                    self.redact_error(error.to_string())
                )));
            }
            if let Some(message_id) = json
                .pointer("/result/timestamp")
                .or_else(|| json.pointer("/result/message_id"))
                .or_else(|| json.pointer("/result/messageId"))
                .or_else(|| json.get("message_id"))
                .and_then(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .or_else(|| value.as_i64().map(|number| number.to_string()))
                        .or_else(|| value.as_u64().map(|number| number.to_string()))
                })
            {
                message_ids.push(message_id);
            }
        }

        Ok(SignalDeliveryReport {
            recipient_id: msg.thread_id.clone(),
            message_id: message_ids.into_iter().next(),
            chunk_count: chunks.len(),
            character_count: msg.text.chars().count(),
        })
    }
}

impl Drop for SignalAdapter {
    fn drop(&mut self) {
        drop_signal_blocking_client_safely(self.client.take());
    }
}

fn drop_signal_blocking_client_safely(client: Option<reqwest::blocking::Client>) {
    let Some(client) = client else {
        return;
    };
    let _ = std::thread::spawn(move || drop(client)).join();
}

fn build_signal_blocking_client(
    no_proxy: bool,
) -> Result<reqwest::blocking::Client, reqwest::Error> {
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

fn signal_sse_reconnect_backoff_millis(steps: usize) -> Vec<u64> {
    let mut delay = SIGNAL_SSE_RETRY_DELAY_INITIAL;
    let mut schedule = Vec::with_capacity(steps);
    for _ in 0..steps {
        schedule.push(delay.as_millis() as u64);
        delay = (delay * 2).min(SIGNAL_SSE_RETRY_DELAY_MAX);
    }
    schedule
}

fn chunk_signal_message(text: &str, max_len: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= max_len {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn value_at_paths(value: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(json_scalar_to_string))
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
        .or_else(|| value.as_f64().map(|number| number.to_string()))
}

fn render_signal_mentions(mut text: String, mentions: Option<&serde_json::Value>) -> String {
    if !text.contains('\u{fffc}') {
        return text;
    }
    let mut mentions = mentions
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    mentions.sort_by_key(|mention| {
        std::cmp::Reverse(
            mention
                .get("start")
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
        )
    });

    for mention in mentions {
        let start = mention
            .get("start")
            .and_then(|value| value.as_u64())
            .unwrap_or_default() as usize;
        let length = mention
            .get("length")
            .and_then(|value| value.as_u64())
            .unwrap_or(1) as usize;
        let replacement = mention
            .get("number")
            .or_else(|| mention.get("uuid"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("@{value}"))
            .unwrap_or_else(|| "@user".into());
        let start_byte = byte_index_for_char(&text, start);
        let end_byte = byte_index_for_char(&text, start.saturating_add(length));
        if start_byte <= end_byte && end_byte <= text.len() {
            text.replace_range(start_byte..end_byte, &replacement);
        }
    }
    text
}

fn byte_index_for_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn signal_attachment_metadata(data_message: &serde_json::Value) -> Vec<serde_json::Value> {
    data_message
        .get("attachments")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let id = attachment
                .get("id")
                .and_then(json_scalar_to_string)
                .filter(|value| !value.trim().is_empty())?;
            let byte_len = attachment
                .get("size")
                .or_else(|| attachment.get("byte_len"))
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            if byte_len > SIGNAL_MAX_ATTACHMENT_SIZE {
                return None;
            }
            Some(serde_json::json!({
                "id": id,
                "content_type": attachment
                    .get("contentType")
                    .or_else(|| attachment.get("content_type"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("application/octet-stream"),
                "filename": attachment
                    .get("filename")
                    .or_else(|| attachment.get("fileName"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("attachment.bin"),
                "byte_len": byte_len,
            }))
        })
        .collect()
}

fn stable_signal_hash(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    sha256_hex(&bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn inbound_payload_hash(message: &InboundMessage) -> Result<String, AdapterError> {
    let bytes = serde_json::to_vec(message)?;
    Ok(sha256_hex(&bytes))
}

fn signal_delivery_id(message: &InboundMessage) -> String {
    format!(
        "signal:{}:{}:{}",
        message.thread_id.trim(),
        message.sender_id.trim(),
        message.message_id.trim()
    )
}

fn insert_provenance_metadata(
    message: &mut InboundMessage,
    key: &str,
    provenance: &SignalInboundProvenance,
) {
    if let Some(object) = message.metadata.as_object_mut() {
        object.insert(
            key.to_string(),
            serde_json::json!({
                "route_name": provenance.route_name,
                "delivery_id": provenance.delivery_id,
                "payload_hash": provenance.payload_hash,
                "principal_id": provenance.principal_id,
                "receipt_timestamp": provenance.receipt_timestamp,
                "receipt_signature": provenance.receipt_signature,
                "receipt_schema_version": provenance.receipt_schema_version,
            }),
        );
    }
}

fn signal_payload_extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8") {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"%PDF") {
        "pdf"
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        "mp4"
    } else if bytes.starts_with(b"OggS") {
        "ogg"
    } else if bytes.len() >= 2 && bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0 {
        "mp3"
    } else if bytes.starts_with(b"PK") {
        "zip"
    } else {
        "bin"
    }
}

fn signal_extension_content_type(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn signal_content_media_kind(content_type: &str) -> &'static str {
    if content_type.starts_with("image/") {
        "image"
    } else if content_type.starts_with("audio/") {
        "audio"
    } else if content_type.starts_with("video/") {
        "video"
    } else {
        "document"
    }
}

fn sanitize_signal_attachment_id(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "attachment".into()
    } else {
        sanitized
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

impl ChannelAdapter for SignalAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Signal
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_account()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("signal inbound buffer lock poisoned".into()))?;
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
    use base64::Engine;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn spawn_signal_mock<F>(
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
                let mut buf = [0u8; 4096];
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
    fn signal_channel_type() {
        let adapter = SignalAdapter::new("+15551234567");
        assert_eq!(adapter.channel_type(), ChannelType::Signal);
    }

    #[test]
    fn signal_rpc_url_formats_signal_cli_path() {
        let adapter =
            SignalAdapter::new("+15551234567").with_api_base_url("http://127.0.0.1:9917/");
        assert_eq!(
            adapter.rpc_url().unwrap(),
            "http://127.0.0.1:9917/api/v1/rpc"
        );
    }

    #[test]
    fn signal_send_with_report_posts_json_rpc_send() {
        let (base_url, server) = spawn_signal_mock(1, |request| {
            assert!(request.starts_with("POST /api/v1/rpc "));
            assert!(request.contains("\"jsonrpc\":\"2.0\""));
            assert!(request.contains("\"method\":\"send\""));
            assert!(request.contains("\"account\":\"+15551234567\""));
            assert!(request.contains("\"recipient\":[\"+15557654321\"]"));
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": {
                    "timestamp": "signal-ts-1",
                },
                "id": "zaion-signal-test",
            })
            .to_string()
        });
        let adapter = SignalAdapter::new("+15551234567").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "signal".into(),
            thread_id: "+15557654321".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({"source": "webhook"}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&msg).unwrap();
        assert_eq!(report.recipient_id, "+15557654321");
        assert_eq!(report.message_id.as_deref(), Some("signal-ts-1"));
        assert_eq!(report.chunk_count, 1);
        server.join().unwrap();
    }

    #[test]
    fn signal_api_errors_redact_account_identifier() {
        let account = "+15551234567";
        let err = SignalAdapter::new(account).redact_error(format!("account={account}"));
        assert!(!err.contains(account));
        assert!(err.contains("[REDACTED]"));
    }

    #[test]
    fn signal_ingest_envelope_builds_canonical_inbound_dm() {
        let adapter = SignalAdapter::new("+15551234567");
        let envelope = serde_json::json!({
            "envelope": {
                "sourceNumber": "+15557654321",
                "sourceUuid": "uuid-sender-1",
                "sourceName": "Ada",
                "timestamp": 1771417200123_i64,
                "dataMessage": {
                    "timestamp": 1771417200123_i64,
                    "message": "hello from signal"
                }
            }
        });

        let message = adapter
            .ingest_envelope(&envelope)
            .unwrap()
            .expect("signal envelope should produce an inbound message");

        assert_eq!(message.channel_id, "signal");
        assert_eq!(message.thread_id, "+15557654321");
        assert_eq!(message.sender_id, "+15557654321");
        assert_eq!(message.text, "hello from signal");
        assert_eq!(message.message_id, "1771417200123");
        assert_eq!(message.timestamp, "1771417200123");
        assert_eq!(message.metadata["provider"], "signal-cli");
        assert_eq!(message.metadata["transport"], "signal_cli_sse");
        assert_eq!(message.metadata["chat_type"], "dm");
        assert_eq!(message.metadata["source_uuid"], "uuid-sender-1");

        let envelope = message
            .to_canonical_envelope(
                "signal",
                zaion_types::identity::PrincipalId("did:key:signal-inbound".into()),
            )
            .expect("signal inbound should be canonical-envelope ready");
        assert_eq!(envelope.channel.0, "signal");
        assert_eq!(envelope.thread.0, "+15557654321");
        assert_eq!(
            envelope.metadata["adapter_metadata"]["transport"],
            "signal_cli_sse"
        );
        assert_eq!(envelope.source_hash.len(), 64);
    }

    #[test]
    fn signal_ingest_envelope_builds_group_thread_renders_mentions_and_attachments() {
        let adapter = SignalAdapter::new("+15551234567");
        let envelope = serde_json::json!({
            "sourceNumber": "+15557654321",
            "sourceUuid": "uuid-sender-1",
            "timestamp": 1771417200456_i64,
            "dataMessage": {
                "message": "Hi \u{fffc}",
                "mentions": [{
                    "start": 3,
                    "length": 1,
                    "number": "+15550001111"
                }],
                "groupInfo": {
                    "groupId": "group-abc",
                    "groupName": "Research Group"
                },
                "attachments": [{
                    "id": "att-1",
                    "contentType": "image/png",
                    "filename": "plot.png",
                    "size": 42
                }]
            }
        });

        let message = adapter
            .ingest_envelope(&envelope)
            .unwrap()
            .expect("signal group envelope should produce an inbound message");

        assert_eq!(message.thread_id, "group:group-abc");
        assert_eq!(message.text, "Hi @+15550001111");
        assert_eq!(message.metadata["chat_type"], "group");
        assert_eq!(message.metadata["group_id"], "group-abc");
        assert_eq!(message.metadata["group_name"], "Research Group");
        assert_eq!(message.metadata["attachments"][0]["id"], "att-1");
        assert_eq!(
            message.metadata["attachments"][0]["content_type"],
            "image/png"
        );
        assert_eq!(message.metadata["attachments"][0]["filename"], "plot.png");
        assert_eq!(message.metadata["attachments"][0]["byte_len"], 42);
    }

    #[test]
    fn signal_ingest_envelope_to_buffer_feeds_channel_receive() {
        let adapter = SignalAdapter::new("+15551234567");
        let envelope = serde_json::json!({
            "sourceNumber": "+15557654321",
            "timestamp": 1771417200999_i64,
            "dataMessage": {
                "message": "buffer me"
            }
        });

        assert!(adapter.ingest_envelope_to_buffer(&envelope).unwrap());
        let received = adapter.receive().unwrap();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].thread_id, "+15557654321");
        assert_eq!(received[0].metadata["transport"], "signal_cli_sse");
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn signal_ingest_sse_chunk_buffers_valid_data_events_and_reports_invalid_lines() {
        let adapter = SignalAdapter::new("+15551234567");
        let chunk = concat!(
            ": keepalive\n\n",
            "event: message\n",
            "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417201000,\"dataMessage\":{\"message\":\"one\"}}\n\n",
            "data: not-json\n\n",
            "data: {\"sourceNumber\":\"+15557654322\",\"timestamp\":1771417201001,\"dataMessage\":{\"message\":\"two\"}}\n\n"
        );

        let report = adapter.ingest_sse_chunk_to_buffer(chunk).unwrap();
        let received = adapter.receive().unwrap();

        assert_eq!(report.data_event_count, 3);
        assert_eq!(report.accepted_count, 2);
        assert_eq!(report.ignored_count, 0);
        assert_eq!(report.invalid_count, 1);
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].text, "one");
        assert_eq!(received[1].text, "two");
    }

    #[test]
    fn signal_sse_lifecycle_builds_daemon_urls_backoff_and_buffers_chunks() {
        let adapter = SignalAdapter::new("+15551234567").with_api_base_url("http://signal:8080/");

        assert_eq!(
            adapter.health_check_url().unwrap(),
            "http://signal:8080/api/v1/check"
        );
        assert_eq!(
            adapter.sse_event_url().unwrap(),
            "http://signal:8080/api/v1/events?account=%2B15551234567"
        );

        let report = adapter
            .run_sse_lifecycle_script_to_buffer(
                [
                    ": keepalive\n",
                    "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417202000,\"dataMessage\":{\"message\":\"life\"}}\n",
                    "data: not-json\n",
                ],
                4,
            )
            .unwrap();
        let received = adapter.receive().unwrap();

        assert_eq!(report.health_url, "http://signal:8080/api/v1/check");
        assert_eq!(
            report.event_url,
            "http://signal:8080/api/v1/events?account=%2B15551234567"
        );
        assert_eq!(report.accept_header, "text/event-stream");
        assert_eq!(report.connect_attempts, 1);
        assert_eq!(report.chunk_count, 3);
        assert_eq!(report.data_event_count, 2);
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.invalid_count, 1);
        assert_eq!(
            report.reconnect_backoff_millis,
            vec![2000, 4000, 8000, 16000]
        );
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "life");
    }

    #[test]
    fn signal_fetch_attachment_caches_rpc_payload_with_hash_and_media_kind() {
        let (base_url, server) = spawn_signal_mock(1, |request| {
            assert!(request.starts_with("POST /api/v1/rpc "));
            assert!(request.contains("\"method\":\"getAttachment\""));
            assert!(request.contains("\"account\":\"+15551234567\""));
            assert!(request.contains("\"id\":\"att-png-1\""));
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": {
                    "data": base64::engine::general_purpose::STANDARD.encode(
                        b"\x89PNG\r\n\x1a\nzaion-signal-attachment"
                    )
                },
                "id": "zaion-signal-attachment-test",
            })
            .to_string()
        });
        let cache_dir = tempfile::tempdir().unwrap();
        let adapter = SignalAdapter::new("+15551234567")
            .with_api_base_url(base_url)
            .with_attachment_cache_dir(cache_dir.path());

        let record = adapter.fetch_attachment_to_cache("att-png-1").unwrap();

        assert_eq!(record.attachment_id, "att-png-1");
        assert_eq!(record.extension, "png");
        assert_eq!(record.content_type, "image/png");
        assert_eq!(record.media_kind, "image");
        assert_eq!(record.byte_len, 31);
        assert_eq!(record.payload_hash.len(), 64);
        assert!(record.cache_path.ends_with("att-png-1.png"));
        assert!(std::path::Path::new(&record.cache_path).exists());
        server.join().unwrap();
    }

    #[test]
    fn signal_sse_inbound_service_records_signed_provenance_receipt_before_buffering() {
        let adapter = SignalAdapter::new("+15551234567");
        let keypair = std::sync::Arc::new(zaion_crypto::ZaionKeypair::generate());
        let service = SignalSseInboundService::new_with_key(&adapter, keypair.clone());
        let chunk = concat!(
            "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417203000,",
            "\"dataMessage\":{\"message\":\"signed signal\"}}\n"
        );

        let report = service.ingest_sse_chunk(chunk).unwrap();
        let received = adapter.receive().unwrap();
        let ledger = service.provenance_ledger().unwrap();

        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);
        assert_eq!(report.provenance_delivery_ids.len(), 1);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].route_name, "signal.inbound.sse");
        assert_eq!(ledger[0].message_id, "1771417203000");
        assert_eq!(ledger[0].payload_hash.len(), 64);
        ledger[0]
            .to_delivery_receipt()
            .verify_receipt(&service.verifying_key())
            .expect("Signal provenance receipt should verify");

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "signed signal");
        assert_eq!(
            received[0].metadata["signal_provenance"]["delivery_id"],
            ledger[0].delivery_id
        );
        assert_eq!(
            received[0].metadata["signal_provenance"]["payload_hash"],
            ledger[0].payload_hash
        );
        assert_eq!(
            received[0].metadata["signal_provenance"]["receipt_schema_version"],
            2
        );
    }
}
