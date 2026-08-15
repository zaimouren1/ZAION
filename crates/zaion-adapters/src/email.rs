//! Email relay platform adapter implementation.
//!
//! This adapter gives Zaion a stable, mockable email delivery surface for
//! webhook/runtime evidence. It targets HTTP email relays so tests and live
//! matrices can verify delivery without depending on a real SMTP account.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::webhook_runtime::DeliveryReceipt;
use crate::AdapterError;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zaion_crypto::ZaionKeypair;
use zaion_types::identity::SignatureBytes;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const EMAIL_RELAY_API_BASE: &str = "https://api.zaion.email/v1";
const AUTOMATED_SENDER_PATTERNS: &[&str] = &[
    "noreply",
    "no-reply",
    "no_reply",
    "donotreply",
    "do-not-reply",
    "mailer-daemon",
    "postmaster",
    "bounce",
    "notifications@",
    "automated@",
    "auto-confirm",
    "auto-reply",
    "automailer",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailDeliveryReport {
    pub recipient: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailFetchedMessage {
    pub uid: String,
    pub raw_rfc822: Vec<u8>,
}

impl EmailFetchedMessage {
    pub fn new(uid: impl Into<String>, raw_rfc822: &[u8]) -> Self {
        Self {
            uid: uid.into(),
            raw_rfc822: raw_rfc822.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmailInboundPollReport {
    pub fetched_count: usize,
    pub enqueued_count: usize,
    pub duplicate_count: usize,
    pub skipped_count: usize,
    pub parse_error_count: usize,
    pub seen_uid_count: usize,
    pub message_ids: Vec<String>,
    pub provenance_count: usize,
    pub provenance_delivery_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EmailInboundProvenance {
    pub route_name: String,
    pub delivery_id: String,
    pub uid: String,
    pub message_id: String,
    pub timestamp: u64,
    pub payload_hash: String,
    pub principal_id: String,
    pub receipt_timestamp: u64,
    pub receipt_signature: String,
    pub receipt_schema_version: u32,
}

impl EmailInboundProvenance {
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

pub struct EmailAdapter {
    from_address: String,
    relay_secret: Zeroizing<String>,
    api_base_url: String,
    client: reqwest::blocking::Client,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

impl EmailAdapter {
    pub fn new(from_address: impl Into<String>, relay_secret: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed");

        Self {
            from_address: from_address.into(),
            relay_secret: Zeroizing::new(relay_secret.into()),
            api_base_url: EMAIL_RELAY_API_BASE.to_string(),
            client,
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

    fn send_url(&self) -> String {
        format!("{}/email/send", self.api_base_url)
    }

    fn validate_credentials(&self) -> Result<(), AdapterError> {
        if self.from_address.trim().is_empty() || self.relay_secret.trim().is_empty() {
            return Err(AdapterError::Channel(
                "Email relay credentials not configured".into(),
            ));
        }
        Ok(())
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        redact_sensitive_values(text.into(), &[self.relay_secret.as_str()])
    }

    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn ingest_rfc822(&self, raw: &[u8]) -> Result<Option<InboundMessage>, AdapterError> {
        self.validate_credentials()?;
        let raw = std::str::from_utf8(raw)
            .map_err(|e| AdapterError::Channel(format!("Email RFC822 parse error: {}", e)))?;
        let (headers, body) = split_rfc822_message(raw)?;
        let from_raw = header_value(&headers, "from").unwrap_or_default();
        let from = extract_email_address(&from_raw);
        if from.is_empty() {
            return Err(AdapterError::Channel(
                "Email inbound missing From header".into(),
            ));
        }
        if from == self.from_address.trim().to_ascii_lowercase()
            || is_automated_email_sender(&from, &headers)
        {
            return Ok(None);
        }

        let subject = header_value(&headers, "subject").unwrap_or_else(|| "(no subject)".into());
        let to = header_value(&headers, "to").unwrap_or_default();
        let date =
            header_value(&headers, "date").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let message_id = normalize_email_message_id(
            header_value(&headers, "message-id")
                .as_deref()
                .unwrap_or_default(),
        )
        .unwrap_or_else(|| stable_email_message_id(&from, &subject, body));
        let in_reply_to = normalize_email_message_id(
            header_value(&headers, "in-reply-to")
                .as_deref()
                .unwrap_or_default(),
        );

        let parsed = parse_email_content(&headers, body)?;
        let mut text = parsed.body.trim().to_string();
        if subject.trim().is_empty() || subject.trim().eq_ignore_ascii_case("(no subject)") {
            // Keep body as-is.
        } else if !subject.trim_start().to_ascii_lowercase().starts_with("re:") {
            text = format!("[Subject: {}]\n\n{}", subject.trim(), text);
        }
        if text.trim().is_empty() {
            text = "(empty email)".to_string();
        }

        let attachment_count = parsed.attachments.len();
        Ok(Some(InboundMessage {
            channel_id: "email".into(),
            thread_id: from.clone(),
            sender_id: from.clone(),
            text,
            message_id: message_id.clone(),
            timestamp: date.clone(),
            metadata: serde_json::json!({
                "provider": "email",
                "transport": "rfc822",
                "from": from,
                "from_raw": from_raw,
                "to": to,
                "subject": subject,
                "date": date,
                "message_id": message_id,
                "in_reply_to": in_reply_to,
                "attachments": parsed.attachments,
                "attachment_count": attachment_count,
            }),
        }))
    }

    pub fn ingest_rfc822_to_buffer(&self, raw: &[u8]) -> Result<bool, AdapterError> {
        let Some(message) = self.ingest_rfc822(raw)? else {
            return Ok(false);
        };
        self.push_inbound(message);
        Ok(true)
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<EmailDeliveryReport, AdapterError> {
        self.validate_credentials()?;
        let recipient = msg.thread_id.trim();
        if recipient.is_empty() {
            return Err(AdapterError::Channel(
                "Email recipient address not configured".into(),
            ));
        }

        let subject = msg
            .metadata
            .get("event")
            .and_then(|value| value.as_str())
            .map(|event| format!("Zaion webhook {}", event))
            .unwrap_or_else(|| "Zaion webhook delivery".to_string());
        let body = serde_json::json!({
            "from": self.from_address,
            "to": recipient,
            "subject": subject,
            "text": msg.text,
            "metadata": msg.metadata,
        });

        let resp = self
            .client
            .post(self.send_url())
            .header(
                "Authorization",
                format!("Bearer {}", self.relay_secret.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| {
                AdapterError::Channel(
                    self.redact_error(format!("Email relay request failed: {}", e)),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "Email relay HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        let json: serde_json::Value = resp.json().map_err(|e| {
            AdapterError::Channel(format!("Email relay response parse error: {}", e))
        })?;

        if json.get("ok").and_then(|value| value.as_bool()) == Some(false) {
            let err = json
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            return Err(AdapterError::Channel(format!(
                "Email relay error: {}",
                self.redact_error(err)
            )));
        }

        let message_id = json
            .get("id")
            .or_else(|| json.get("message_id"))
            .or_else(|| json.get("messageId"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        Ok(EmailDeliveryReport {
            recipient: recipient.to_string(),
            message_id,
            character_count: msg.text.chars().count(),
        })
    }
}

pub trait EmailPollSource {
    fn fetch_messages(&mut self) -> Result<Vec<EmailFetchedMessage>, AdapterError>;
}

pub struct EmailInboundPollService<'a> {
    adapter: &'a EmailAdapter,
    seen_uids: Mutex<VecDeque<String>>,
    seen_uid_limit: usize,
    provenance_ledger: Mutex<Vec<EmailInboundProvenance>>,
    signing_key: Arc<ZaionKeypair>,
}

impl<'a> EmailInboundPollService<'a> {
    #[cfg(test)]
    pub fn new(adapter: &'a EmailAdapter) -> Self {
        Self::new_with_key(adapter, Arc::new(ZaionKeypair::generate()))
    }

    pub fn new_with_key(adapter: &'a EmailAdapter, signing_key: Arc<ZaionKeypair>) -> Self {
        Self {
            adapter,
            seen_uids: Mutex::new(VecDeque::new()),
            seen_uid_limit: 2_000,
            provenance_ledger: Mutex::new(Vec::new()),
            signing_key,
        }
    }

    pub fn with_seen_uid_limit(mut self, seen_uid_limit: usize) -> Self {
        self.seen_uid_limit = seen_uid_limit.max(1);
        self
    }

    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn provenance_ledger(&self) -> Result<Vec<EmailInboundProvenance>, AdapterError> {
        self.provenance_ledger
            .lock()
            .map(|ledger| ledger.clone())
            .map_err(|_| AdapterError::Channel("email provenance ledger lock poisoned".into()))
    }

    pub fn ingest_fetched_messages(
        &self,
        messages: Vec<EmailFetchedMessage>,
    ) -> Result<EmailInboundPollReport, AdapterError> {
        let mut report = EmailInboundPollReport {
            fetched_count: messages.len(),
            ..EmailInboundPollReport::default()
        };

        for fetched in messages {
            if self.uid_seen(&fetched.uid)? {
                report.duplicate_count += 1;
                continue;
            }

            let maybe_message = match self.adapter.ingest_rfc822(&fetched.raw_rfc822) {
                Ok(value) => value,
                Err(err) => {
                    report.parse_error_count += 1;
                    return Err(err);
                }
            };

            let Some(mut message) = maybe_message else {
                self.mark_uid_seen(&fetched.uid)?;
                report.skipped_count += 1;
                continue;
            };

            if !self.mark_uid_seen(&fetched.uid)? {
                report.duplicate_count += 1;
                continue;
            }

            if let Some(object) = message.metadata.as_object_mut() {
                object.insert(
                    "uid".to_string(),
                    serde_json::Value::String(fetched.uid.clone()),
                );
                object.insert(
                    "poll_lifecycle".to_string(),
                    serde_json::json!("email_inbound_poll"),
                );
            }
            report.message_ids.push(message.message_id.clone());

            let payload_hash = email_inbound_payload_hash(&message)?;
            let delivery_id = email_delivery_id(&fetched.uid, &message.message_id);
            let receipt = self.generate_provenance_receipt(&delivery_id, &payload_hash)?;
            let provenance = EmailInboundProvenance {
                route_name: "email.inbound.poll".to_string(),
                delivery_id: delivery_id.clone(),
                uid: fetched.uid.clone(),
                message_id: message.message_id.clone(),
                timestamp: now_unix_secs(),
                payload_hash: payload_hash.clone(),
                principal_id: receipt.principal_id.clone(),
                receipt_timestamp: receipt.timestamp,
                receipt_signature: receipt.ed25519_signature.clone(),
                receipt_schema_version: receipt.schema_version,
            };
            if let Some(object) = message.metadata.as_object_mut() {
                object.insert(
                    "email_provenance".to_string(),
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
            self.record_provenance(provenance)?;
            report.provenance_count += 1;
            report.provenance_delivery_ids.push(delivery_id);
            self.adapter.push_inbound(message);
            report.enqueued_count += 1;
        }

        report.seen_uid_count = self.seen_uid_count()?;
        Ok(report)
    }

    pub fn poll_source(
        &self,
        source: &mut impl EmailPollSource,
    ) -> Result<EmailInboundPollReport, AdapterError> {
        let messages = source.fetch_messages()?;
        self.ingest_fetched_messages(messages)
    }

    fn uid_seen(&self, uid: &str) -> Result<bool, AdapterError> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(false);
        }

        self.seen_uids
            .lock()
            .map(|seen| seen.iter().any(|existing| existing == uid))
            .map_err(|_| AdapterError::Channel("email poll seen UID lock poisoned".into()))
    }

    fn mark_uid_seen(&self, uid: &str) -> Result<bool, AdapterError> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(true);
        }

        let mut seen = self
            .seen_uids
            .lock()
            .map_err(|_| AdapterError::Channel("email poll seen UID lock poisoned".into()))?;
        if seen.iter().any(|existing| existing == uid) {
            return Ok(false);
        }
        seen.push_back(uid.to_string());
        while seen.len() > self.seen_uid_limit {
            seen.pop_front();
        }
        Ok(true)
    }

    fn seen_uid_count(&self) -> Result<usize, AdapterError> {
        self.seen_uids
            .lock()
            .map(|seen| seen.len())
            .map_err(|_| AdapterError::Channel("email poll seen UID lock poisoned".into()))
    }

    fn generate_provenance_receipt(
        &self,
        delivery_id: &str,
        payload_hash: &str,
    ) -> Result<DeliveryReceipt, AdapterError> {
        let route_name = "email.inbound.poll";
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

    fn record_provenance(&self, provenance: EmailInboundProvenance) -> Result<(), AdapterError> {
        self.provenance_ledger
            .lock()
            .map_err(|_| AdapterError::Channel("email provenance ledger lock poisoned".into()))?
            .push(provenance);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct ParsedEmailContent {
    body: String,
    attachments: Vec<serde_json::Value>,
}

fn split_rfc822_message(raw: &str) -> Result<(BTreeMap<String, String>, &str), AdapterError> {
    let (headers, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or_else(|| {
            AdapterError::Channel("Email RFC822 message missing header/body split".into())
        })?;
    Ok((parse_headers(headers), body))
}

fn parse_headers(raw: &str) -> BTreeMap<String, String> {
    let mut unfolded = Vec::<String>::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = unfolded.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else if !line.trim().is_empty() {
            unfolded.push(line.to_string());
        }
    }

    let mut headers = BTreeMap::new();
    for line in unfolded {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                decode_header_value(value.trim()),
            );
        }
    }
    headers
}

fn header_value(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers.get(&name.to_ascii_lowercase()).cloned()
}

fn decode_header_value(value: &str) -> String {
    value.trim().to_string()
}

fn extract_email_address(raw: &str) -> String {
    if let Some(start) = raw.find('<') {
        if let Some(end) = raw[start + 1..].find('>') {
            return raw[start + 1..start + 1 + end]
                .trim()
                .trim_matches('"')
                .to_ascii_lowercase();
        }
    }
    raw.trim().trim_matches('"').to_ascii_lowercase()
}

fn normalize_email_message_id(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('<').trim_matches('>').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn stable_email_message_id(from: &str, subject: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(from.as_bytes());
    hasher.update([0x1f]);
    hasher.update(subject.as_bytes());
    hasher.update([0x1f]);
    hasher.update(body.as_bytes());
    format!("email:{}", hex::encode(hasher.finalize()))
}

fn email_delivery_id(uid: &str, message_id: &str) -> String {
    format!("email:uid:{}:{}", uid.trim(), message_id.trim())
}

fn email_inbound_payload_hash(message: &InboundMessage) -> Result<String, AdapterError> {
    let bytes = serde_json::to_vec(message)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_automated_email_sender(sender: &str, headers: &BTreeMap<String, String>) -> bool {
    let sender = sender.to_ascii_lowercase();
    if AUTOMATED_SENDER_PATTERNS
        .iter()
        .any(|pattern| sender.contains(pattern))
    {
        return true;
    }
    if headers
        .get("auto-submitted")
        .is_some_and(|value| !value.eq_ignore_ascii_case("no"))
    {
        return true;
    }
    if headers.get("precedence").is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "bulk" | "list" | "junk"
        )
    }) {
        return true;
    }
    headers.contains_key("list-unsubscribe") || headers.contains_key("x-auto-response-suppress")
}

fn parse_email_content(
    headers: &BTreeMap<String, String>,
    body: &str,
) -> Result<ParsedEmailContent, AdapterError> {
    let content_type = header_value(headers, "content-type").unwrap_or_else(|| "text/plain".into());
    if content_type.to_ascii_lowercase().starts_with("multipart/") {
        if let Some(boundary) = content_type_parameter(&content_type, "boundary") {
            return parse_multipart_email(body, &boundary);
        }
    }

    let bytes = decode_part_bytes(headers, body)?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if content_type.to_ascii_lowercase().contains("text/html") {
        text = strip_html(&text);
    }
    Ok(ParsedEmailContent {
        body: text,
        attachments: Vec::new(),
    })
}

fn parse_multipart_email(body: &str, boundary: &str) -> Result<ParsedEmailContent, AdapterError> {
    let marker = format!("--{}", boundary);
    let mut parsed = ParsedEmailContent::default();
    let mut html_fallback = None::<String>;

    for segment in body.split(&marker).skip(1) {
        let segment = segment.trim_start_matches(['\r', '\n']);
        if segment.starts_with("--") {
            break;
        }
        let segment = segment.trim_end_matches(['\r', '\n']);
        let Some((raw_headers, part_body)) = segment
            .split_once("\r\n\r\n")
            .or_else(|| segment.split_once("\n\n"))
        else {
            continue;
        };
        let part_headers = parse_headers(raw_headers);
        let content_type =
            header_value(&part_headers, "content-type").unwrap_or_else(|| "text/plain".into());
        let disposition = header_value(&part_headers, "content-disposition").unwrap_or_default();
        let content_type_lower = content_type.to_ascii_lowercase();
        let disposition_lower = disposition.to_ascii_lowercase();
        let filename = content_type_parameter(&disposition, "filename")
            .or_else(|| content_type_parameter(&content_type, "name"));
        let is_attachment = filename.is_some()
            || disposition_lower.contains("attachment")
            || (disposition_lower.contains("inline")
                && !content_type_lower.starts_with("text/plain")
                && !content_type_lower.starts_with("text/html"));

        if is_attachment {
            let bytes = decode_part_bytes(&part_headers, part_body)?;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            parsed.attachments.push(serde_json::json!({
                "filename": filename.unwrap_or_else(|| "attachment.bin".into()),
                "content_type": content_type,
                "byte_len": bytes.len(),
                "sha256": hex::encode(hasher.finalize()),
            }));
            continue;
        }

        if content_type_lower.starts_with("text/plain") && parsed.body.trim().is_empty() {
            let bytes = decode_part_bytes(&part_headers, part_body)?;
            parsed.body = String::from_utf8_lossy(&bytes).into_owned();
        } else if content_type_lower.starts_with("text/html") && html_fallback.is_none() {
            let bytes = decode_part_bytes(&part_headers, part_body)?;
            html_fallback = Some(strip_html(&String::from_utf8_lossy(&bytes)));
        }
    }

    if parsed.body.trim().is_empty() {
        parsed.body = html_fallback.unwrap_or_default();
    }
    Ok(parsed)
}

fn content_type_parameter(value: &str, param: &str) -> Option<String> {
    let param = param.to_ascii_lowercase();
    value.split(';').skip(1).find_map(|part| {
        let (name, value) = part.split_once('=')?;
        if name.trim().eq_ignore_ascii_case(&param) {
            Some(value.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

fn decode_part_bytes(
    headers: &BTreeMap<String, String>,
    body: &str,
) -> Result<Vec<u8>, AdapterError> {
    let encoding = header_value(headers, "content-transfer-encoding")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let body = body.trim_matches(['\r', '\n']);
    match encoding.as_str() {
        "base64" => {
            let compacted = body
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            base64::engine::general_purpose::STANDARD
                .decode(compacted.as_bytes())
                .map_err(|e| {
                    AdapterError::Channel(format!("Email base64 attachment parse error: {}", e))
                })
        }
        "quoted-printable" => Ok(decode_quoted_printable(body.as_bytes())),
        _ => Ok(body.as_bytes().to_vec()),
    }
}

fn decode_quoted_printable(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'=' {
            if i + 2 < input.len() && input[i + 1] == b'\r' && input[i + 2] == b'\n' {
                i += 3;
                continue;
            }
            if i + 1 < input.len() && input[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            if i + 2 < input.len() {
                if let (Some(hi), Some(lo)) = (hex_value(input[i + 1]), hex_value(input[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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

impl ChannelAdapter for EmailAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Email
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_credentials()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("email inbound buffer lock poisoned".into()))?;
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

    #[test]
    fn email_adapter_rejects_empty_credentials() {
        let adapter = EmailAdapter::new("", "");
        let err = adapter.validate_credentials().unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn email_api_base_url_can_be_overridden_for_probe_isolation() {
        let adapter = EmailAdapter::new("agent@example.com", "secret")
            .with_api_base_url("http://127.0.0.1:9917/");
        assert_eq!(adapter.send_url(), "http://127.0.0.1:9917/email/send");
    }

    #[test]
    fn email_channel_type_is_stable() {
        let adapter = EmailAdapter::new("agent@example.com", "secret");
        assert_eq!(adapter.channel_type(), ChannelType::Email);
    }

    #[test]
    fn email_ingest_rfc822_builds_canonical_inbound_with_attachment_metadata() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let raw = concat!(
            "From: Researcher <researcher@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Paper Found\r\n",
            "Message-ID: <msg-123@example.com>\r\n",
            "Date: Tue, 19 May 2026 10:00:00 +0000\r\n",
            "Content-Type: multipart/mixed; boundary=\"zaion-boundary\"\r\n",
            "\r\n",
            "--zaion-boundary\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Review this candidate.\r\n",
            "--zaion-boundary\r\n",
            "Content-Type: application/pdf\r\n",
            "Content-Disposition: attachment; filename=\"paper.pdf\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "cGRmLWJ5dGVz\r\n",
            "--zaion-boundary--\r\n",
        );

        let message = adapter
            .ingest_rfc822(raw.as_bytes())
            .unwrap()
            .expect("email should produce an inbound message");

        assert_eq!(message.channel_id, "email");
        assert_eq!(message.thread_id, "researcher@example.com");
        assert_eq!(message.sender_id, "researcher@example.com");
        assert_eq!(message.message_id, "msg-123@example.com");
        assert!(message.text.contains("[Subject: Paper Found]"));
        assert!(message.text.contains("Review this candidate."));
        assert_eq!(message.metadata["subject"], "Paper Found");
        assert_eq!(message.metadata["from"], "researcher@example.com");
        assert_eq!(message.metadata["attachments"][0]["filename"], "paper.pdf");
        assert_eq!(
            message.metadata["attachments"][0]["content_type"],
            "application/pdf"
        );
        assert_eq!(message.metadata["attachments"][0]["byte_len"], 9);

        let envelope = message
            .to_canonical_envelope(
                "email",
                zaion_types::identity::PrincipalId("did:key:email-inbound".into()),
            )
            .expect("email inbound should be canonical-envelope ready");
        assert_eq!(envelope.channel.0, "email");
        assert_eq!(envelope.thread.0, "researcher@example.com");
        assert_eq!(
            envelope.metadata["adapter_metadata"]["subject"],
            "Paper Found"
        );
        assert_eq!(envelope.source_hash.len(), 64);
    }

    #[test]
    fn email_ingest_rfc822_to_buffer_feeds_channel_receive() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let raw = concat!(
            "From: Researcher <researcher@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Buffered\r\n",
            "Message-ID: <buffered@example.com>\r\n",
            "\r\n",
            "buffer me\r\n",
        );

        assert!(adapter.ingest_rfc822_to_buffer(raw.as_bytes()).unwrap());
        let received = adapter.receive().unwrap();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].thread_id, "researcher@example.com");
        assert_eq!(received[0].metadata["subject"], "Buffered");
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn email_inbound_poll_service_deduplicates_uids_and_buffers_messages() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let service = EmailInboundPollService::new(&adapter).with_seen_uid_limit(2);
        let first = concat!(
            "From: Researcher <researcher@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: First\r\n",
            "Message-ID: <first@example.com>\r\n",
            "\r\n",
            "first body\r\n",
        );
        let duplicate = first.replace("first body", "duplicate body");

        let report = service
            .ingest_fetched_messages(vec![
                EmailFetchedMessage::new("101", first.as_bytes()),
                EmailFetchedMessage::new("101", duplicate.as_bytes()),
            ])
            .unwrap();

        assert_eq!(report.fetched_count, 2);
        assert_eq!(report.enqueued_count, 1);
        assert_eq!(report.duplicate_count, 1);
        assert_eq!(report.skipped_count, 0);
        assert_eq!(report.message_ids, vec!["first@example.com".to_string()]);

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message_id, "first@example.com");
        assert_eq!(received[0].metadata["uid"], "101");
    }

    #[test]
    fn email_inbound_poll_service_caps_seen_uid_memory_and_skips_automated_mail() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let service = EmailInboundPollService::new(&adapter).with_seen_uid_limit(2);
        let human = |uid: &str| {
            format!(
                "From: Researcher <researcher{}@example.com>\r\n\
                 To: agent@example.com\r\n\
                 Subject: Human {}\r\n\
                 Message-ID: <human{}@example.com>\r\n\
                 \r\n\
                 body {}\r\n",
                uid, uid, uid, uid
            )
        };
        let automated = concat!(
            "From: No Reply <noreply@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Automated\r\n",
            "Message-ID: <auto@example.com>\r\n",
            "\r\n",
            "ignore me\r\n",
        );

        let report = service
            .ingest_fetched_messages(vec![
                EmailFetchedMessage::new("201", human("201").as_bytes()),
                EmailFetchedMessage::new("202", automated.as_bytes()),
                EmailFetchedMessage::new("203", human("203").as_bytes()),
            ])
            .unwrap();

        assert_eq!(report.fetched_count, 3);
        assert_eq!(report.enqueued_count, 2);
        assert_eq!(report.skipped_count, 1);
        assert_eq!(report.seen_uid_count, 2);

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].metadata["uid"], "201");
        assert_eq!(received[1].metadata["uid"], "203");
    }

    #[test]
    fn email_inbound_poll_service_does_not_poison_uid_after_parse_error() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let service = EmailInboundPollService::new(&adapter).with_seen_uid_limit(2);
        let malformed = b"From: Researcher <researcher@example.com>\r\nSubject: Broken";

        let err = service
            .ingest_fetched_messages(vec![EmailFetchedMessage::new("301", malformed)])
            .unwrap_err();

        assert!(err.to_string().contains("header/body split"));

        let repaired = concat!(
            "From: Researcher <researcher@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Repaired\r\n",
            "Message-ID: <repaired@example.com>\r\n",
            "\r\n",
            "repaired body\r\n",
        );
        let report = service
            .ingest_fetched_messages(vec![EmailFetchedMessage::new("301", repaired.as_bytes())])
            .unwrap();

        assert_eq!(report.enqueued_count, 1);
        assert_eq!(report.duplicate_count, 0);
        assert_eq!(report.seen_uid_count, 1);

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].metadata["uid"], "301");
        assert_eq!(received[0].message_id, "repaired@example.com");
    }

    struct StaticEmailPollSource {
        messages: Vec<EmailFetchedMessage>,
    }

    impl EmailPollSource for StaticEmailPollSource {
        fn fetch_messages(&mut self) -> Result<Vec<EmailFetchedMessage>, AdapterError> {
            Ok(std::mem::take(&mut self.messages))
        }
    }

    #[test]
    fn email_inbound_poll_service_polls_source_and_reports_lifecycle() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let service = EmailInboundPollService::new(&adapter);
        let raw = concat!(
            "From: Operator <operator@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Poll Source\r\n",
            "Message-ID: <poll-source@example.com>\r\n",
            "\r\n",
            "source body\r\n",
        );
        let mut source = StaticEmailPollSource {
            messages: vec![EmailFetchedMessage::new("401", raw.as_bytes())],
        };

        let report = service.poll_source(&mut source).unwrap();

        assert_eq!(report.fetched_count, 1);
        assert_eq!(report.enqueued_count, 1);
        assert_eq!(report.message_ids, vec!["poll-source@example.com"]);

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].metadata["poll_lifecycle"], "email_inbound_poll");
        assert_eq!(received[0].metadata["uid"], "401");
    }

    #[test]
    fn email_inbound_poll_service_records_signed_provenance_receipt_for_accepted_uid() {
        let adapter = EmailAdapter::new("agent@example.com", "relay-secret");
        let keypair = std::sync::Arc::new(zaion_crypto::ZaionKeypair::generate());
        let service = EmailInboundPollService::new_with_key(&adapter, keypair.clone());
        let raw = concat!(
            "From: Operator <operator@example.com>\r\n",
            "To: agent@example.com\r\n",
            "Subject: Receipt Source\r\n",
            "Message-ID: <receipt-source@example.com>\r\n",
            "\r\n",
            "source body\r\n",
        );

        let first = service
            .ingest_fetched_messages(vec![EmailFetchedMessage::new("501", raw.as_bytes())])
            .unwrap();
        let duplicate = service
            .ingest_fetched_messages(vec![EmailFetchedMessage::new("501", raw.as_bytes())])
            .unwrap();

        assert_eq!(first.enqueued_count, 1);
        assert_eq!(first.provenance_count, 1);
        assert_eq!(duplicate.duplicate_count, 1);
        assert_eq!(duplicate.provenance_count, 0);

        let ledger = service.provenance_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        let provenance = &ledger[0];
        assert_eq!(provenance.uid, "501");
        assert_eq!(provenance.message_id, "receipt-source@example.com");
        assert_eq!(provenance.route_name, "email.inbound.poll");
        assert_eq!(
            provenance.delivery_id,
            "email:uid:501:receipt-source@example.com"
        );
        assert_eq!(provenance.principal_id, keypair.principal_id().to_string());
        assert_eq!(provenance.receipt_signature.len(), 128);
        assert_eq!(provenance.receipt_schema_version, 2);

        let received = adapter.receive().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].metadata["email_provenance"]["payload_hash"],
            provenance.payload_hash
        );
        assert_eq!(
            received[0].metadata["email_provenance"]["receipt_schema_version"],
            2
        );

        provenance
            .to_delivery_receipt()
            .verify_receipt(&service.verifying_key())
            .expect("email inbound provenance receipt should verify");
    }
}
