//! Home Assistant platform adapter implementation.
//!
//! This adapter targets Home Assistant's REST service call endpoint for
//! `persistent_notification.create`, giving webhook delivery probes a
//! mockable outbound Home Assistant notification path.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::webhook_runtime::DeliveryReceipt;
use crate::AdapterError;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use zaion_crypto::ZaionKeypair;
use zaion_types::identity::SignatureBytes;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const HASS_URL_DEFAULT: &str = "http://homeassistant.local:8123";
const HASS_MESSAGE_LIMIT: usize = 4096;

pub struct HomeAssistantAdapter {
    access_token: Zeroizing<String>,
    api_base_url: String,
    client: Option<reqwest::blocking::Client>,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
    watch_domains: HashSet<String>,
    watch_entities: HashSet<String>,
    ignored_entities: HashSet<String>,
    watch_all: bool,
    cooldown_seconds: i64,
    last_event_time: Mutex<HashMap<String, i64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeAssistantFrameIngestReport {
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeAssistantProvenanceFrameIngestReport {
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
    pub provenance_count: usize,
    pub provenance_delivery_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeAssistantWebSocketLifecycleReport {
    pub websocket_url: String,
    pub auth_required_seen: bool,
    pub authenticated: bool,
    pub subscribed: bool,
    pub subscription_id: u64,
    pub frames_read: usize,
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeAssistantDeliveryReport {
    pub notification_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HomeAssistantInboundProvenance {
    pub route_name: String,
    pub delivery_id: String,
    pub message_id: String,
    pub entity_id: String,
    pub timestamp: u64,
    pub payload_hash: String,
    pub principal_id: String,
    pub receipt_timestamp: u64,
    pub receipt_signature: String,
    pub receipt_schema_version: u32,
}

impl HomeAssistantInboundProvenance {
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

pub struct HomeAssistantWebSocketInboundService<'a> {
    adapter: &'a HomeAssistantAdapter,
    provenance_ledger: Mutex<Vec<HomeAssistantInboundProvenance>>,
    signing_key: Arc<ZaionKeypair>,
}

impl<'a> HomeAssistantWebSocketInboundService<'a> {
    #[cfg(test)]
    pub fn new(adapter: &'a HomeAssistantAdapter) -> Self {
        Self::new_with_key(adapter, Arc::new(ZaionKeypair::generate()))
    }

    pub fn new_with_key(adapter: &'a HomeAssistantAdapter, signing_key: Arc<ZaionKeypair>) -> Self {
        Self {
            adapter,
            provenance_ledger: Mutex::new(Vec::new()),
            signing_key,
        }
    }

    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn provenance_ledger(&self) -> Result<Vec<HomeAssistantInboundProvenance>, AdapterError> {
        self.provenance_ledger
            .lock()
            .map(|ledger| ledger.clone())
            .map_err(|_| {
                AdapterError::Channel("homeassistant provenance ledger lock poisoned".into())
            })
    }

    pub fn ingest_websocket_text(
        &self,
        frame: &str,
    ) -> Result<HomeAssistantProvenanceFrameIngestReport, AdapterError> {
        self.adapter.validate_token()?;
        let mut report = HomeAssistantProvenanceFrameIngestReport::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(frame) else {
            report.invalid_count += 1;
            return Ok(report);
        };
        if value.get("type").and_then(|value| value.as_str()) != Some("event") {
            report.ignored_count += 1;
            return Ok(report);
        }
        let Some(mut message) = self.adapter.ingest_state_changed_event(&value)? else {
            report.ignored_count += 1;
            return Ok(report);
        };

        let payload_hash = inbound_payload_hash(&message)?;
        let delivery_id = homeassistant_delivery_id(&message);
        let receipt = self.generate_provenance_receipt(&delivery_id, &payload_hash)?;
        let entity_id = message
            .metadata
            .get("entity_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let provenance = HomeAssistantInboundProvenance {
            route_name: "homeassistant.inbound.websocket".into(),
            delivery_id: delivery_id.clone(),
            message_id: message.message_id.clone(),
            entity_id,
            timestamp: now_unix_secs(),
            payload_hash: payload_hash.clone(),
            principal_id: receipt.principal_id.clone(),
            receipt_timestamp: receipt.timestamp,
            receipt_signature: receipt.ed25519_signature.clone(),
            receipt_schema_version: receipt.schema_version,
        };
        insert_provenance_metadata(&mut message, "homeassistant_provenance", &provenance);
        self.record_provenance(provenance)?;
        self.adapter.push_inbound(message);
        report.accepted_count += 1;
        report.provenance_count += 1;
        report.provenance_delivery_ids.push(delivery_id);
        Ok(report)
    }

    fn generate_provenance_receipt(
        &self,
        delivery_id: &str,
        payload_hash: &str,
    ) -> Result<DeliveryReceipt, AdapterError> {
        let route_name = "homeassistant.inbound.websocket";
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

    fn record_provenance(
        &self,
        provenance: HomeAssistantInboundProvenance,
    ) -> Result<(), AdapterError> {
        self.provenance_ledger
            .lock()
            .map_err(|_| {
                AdapterError::Channel("homeassistant provenance ledger lock poisoned".into())
            })?
            .push(provenance);
        Ok(())
    }
}

impl HomeAssistantAdapter {
    pub fn new(access_token: impl Into<String>) -> Self {
        let client = build_homeassistant_blocking_client(false)
            .expect("reqwest::blocking::Client builder must succeed");
        let api_base_url = std::env::var("HASS_URL")
            .unwrap_or_else(|_| HASS_URL_DEFAULT.to_string())
            .trim_end_matches('/')
            .to_string();

        Self {
            access_token: Zeroizing::new(access_token.into()),
            api_base_url,
            client: Some(client),
            inbound_buffer: Mutex::new(Vec::new()),
            watch_domains: HashSet::new(),
            watch_entities: HashSet::new(),
            ignored_entities: HashSet::new(),
            watch_all: false,
            cooldown_seconds: 30,
            last_event_time: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = build_homeassistant_blocking_client(true) {
                let old_client = self.client.replace(client);
                drop_homeassistant_blocking_client_safely(old_client);
            }
        }
        self
    }

    pub fn with_watch_domains<I, S>(mut self, domains: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.watch_domains = domains
            .into_iter()
            .map(Into::into)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn with_watch_entities<I, S>(mut self, entities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.watch_entities = entities
            .into_iter()
            .map(Into::into)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn with_ignored_entities<I, S>(mut self, entities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignored_entities = entities
            .into_iter()
            .map(Into::into)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn with_watch_all(mut self, enabled: bool) -> Self {
        self.watch_all = enabled;
        self
    }

    pub fn with_cooldown_seconds(mut self, seconds: i64) -> Self {
        self.cooldown_seconds = seconds.max(0);
        self
    }

    fn validate_token(&self) -> Result<(), AdapterError> {
        if self.access_token.trim().is_empty() {
            return Err(AdapterError::Channel(
                "Home Assistant access token not configured".into(),
            ));
        }
        Ok(())
    }

    fn notification_url(&self) -> Result<String, AdapterError> {
        let mut url = reqwest::Url::parse(&self.api_base_url).map_err(|e| {
            AdapterError::Channel(format!("Home Assistant API base URL invalid: {}", e))
        })?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Home Assistant API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "services", "persistent_notification", "create"]);
        }
        Ok(url.to_string())
    }

    pub fn websocket_url(&self) -> Result<String, AdapterError> {
        let mut url = reqwest::Url::parse(&self.api_base_url).map_err(|e| {
            AdapterError::Channel(format!("Home Assistant API base URL invalid: {}", e))
        })?;
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            other => {
                return Err(AdapterError::Channel(format!(
                    "Home Assistant WebSocket URL unsupported scheme: {other}"
                )))
            }
        };
        url.set_scheme(scheme).map_err(|_| {
            AdapterError::Channel("Home Assistant WebSocket URL scheme invalid".into())
        })?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Home Assistant API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "websocket"]);
        }
        Ok(url.to_string())
    }

    pub fn websocket_auth_frame(&self) -> Result<serde_json::Value, AdapterError> {
        self.validate_token()?;
        Ok(serde_json::json!({
            "type": "auth",
            "access_token": self.access_token.as_str(),
        }))
    }

    pub fn websocket_subscribe_state_changed_frame(
        &self,
        subscription_id: u64,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": subscription_id,
            "type": "subscribe_events",
            "event_type": "state_changed",
        })
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        redact_sensitive_values(text.into(), &[self.access_token.as_str()])
    }

    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn ingest_state_changed_event(
        &self,
        raw: &serde_json::Value,
    ) -> Result<Option<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let event = raw.get("event").unwrap_or(raw);
        if let Some(event_type) = event.get("event_type").and_then(|value| value.as_str()) {
            if event_type != "state_changed" {
                return Ok(None);
            }
        }

        let data = event.get("data").unwrap_or(event);
        let entity_id = data
            .get("entity_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if entity_id.is_empty() {
            return Ok(None);
        }
        if self.ignored_entities.contains(&entity_id) {
            return Ok(None);
        }

        let domain = entity_id
            .split_once('.')
            .map(|(domain, _)| domain.to_string())
            .unwrap_or_default();
        if !self.watch_domains.is_empty() || !self.watch_entities.is_empty() {
            let domain_match = self.watch_domains.contains(&domain);
            let entity_match = self.watch_entities.contains(&entity_id);
            if !domain_match && !entity_match {
                return Ok(None);
            }
        } else if !self.watch_all {
            return Ok(None);
        }

        let old_state = data.get("old_state").unwrap_or(&serde_json::Value::Null);
        let new_state = data.get("new_state").unwrap_or(&serde_json::Value::Null);
        let text = format_homeassistant_state_change(&entity_id, old_state, new_state);
        let Some(text) = text else {
            return Ok(None);
        };

        let timestamp = event
            .get("time_fired")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("")
            .to_string();
        let event_epoch = parse_homeassistant_event_epoch(&timestamp);
        if self.cooldown_seconds > 0 {
            let mut last_event_time = self.last_event_time.lock().map_err(|_| {
                AdapterError::Channel("homeassistant cooldown lock poisoned".into())
            })?;
            if let Some(last) = last_event_time.get(&entity_id) {
                if event_epoch.saturating_sub(*last) < self.cooldown_seconds {
                    return Ok(None);
                }
            }
            last_event_time.insert(entity_id.clone(), event_epoch);
        }

        let friendly_name = homeassistant_friendly_name(&entity_id, new_state);
        let old_state_value = homeassistant_state_value(old_state);
        let new_state_value = homeassistant_state_value(new_state);
        let message_id = format!("ha_{}_{}", entity_id, event_epoch);
        let timestamp = if timestamp.is_empty() {
            chrono::Utc::now().to_rfc3339()
        } else {
            timestamp
        };

        Ok(Some(InboundMessage {
            channel_id: "homeassistant".into(),
            thread_id: "ha_events".into(),
            sender_id: "homeassistant".into(),
            text,
            message_id: message_id.clone(),
            timestamp,
            metadata: serde_json::json!({
                "provider": "homeassistant",
                "transport": "homeassistant_websocket",
                "event_type": "state_changed",
                "entity_id": entity_id,
                "domain": domain,
                "friendly_name": friendly_name,
                "old_state": old_state_value,
                "new_state": new_state_value,
                "message_id": message_id,
                "old_attributes": old_state.get("attributes").cloned().unwrap_or(serde_json::Value::Null),
                "new_attributes": new_state.get("attributes").cloned().unwrap_or(serde_json::Value::Null),
            }),
        }))
    }

    pub fn ingest_state_changed_event_to_buffer(
        &self,
        raw: &serde_json::Value,
    ) -> Result<bool, AdapterError> {
        let Some(message) = self.ingest_state_changed_event(raw)? else {
            return Ok(false);
        };
        self.push_inbound(message);
        Ok(true)
    }

    pub fn ingest_websocket_text_to_buffer(
        &self,
        frame: &str,
    ) -> Result<HomeAssistantFrameIngestReport, AdapterError> {
        self.validate_token()?;
        let mut report = HomeAssistantFrameIngestReport::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(frame) else {
            report.invalid_count += 1;
            return Ok(report);
        };
        if value.get("type").and_then(|value| value.as_str()) != Some("event") {
            report.ignored_count += 1;
            return Ok(report);
        }
        if self.ingest_state_changed_event_to_buffer(&value)? {
            report.accepted_count += 1;
        } else {
            report.ignored_count += 1;
        }
        Ok(report)
    }

    pub fn ingest_websocket_lifecycle_to_buffer<I, S>(
        &self,
        frames: I,
    ) -> Result<HomeAssistantWebSocketLifecycleReport, AdapterError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.validate_token()?;
        let mut report = HomeAssistantWebSocketLifecycleReport {
            websocket_url: self.websocket_url()?,
            subscription_id: 1,
            ..HomeAssistantWebSocketLifecycleReport::default()
        };
        let mut lifecycle_state = HomeAssistantLifecycleState::AwaitingAuthRequired;

        for frame in frames {
            report.frames_read += 1;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(frame.as_ref()) else {
                report.invalid_count += 1;
                continue;
            };
            let frame_type = value.get("type").and_then(|value| value.as_str());
            match lifecycle_state {
                HomeAssistantLifecycleState::AwaitingAuthRequired => {
                    if frame_type == Some("auth_required") {
                        report.auth_required_seen = true;
                        let _auth_frame = self.websocket_auth_frame()?;
                        lifecycle_state = HomeAssistantLifecycleState::AwaitingAuthOk;
                    } else {
                        report.failure = Some("expected auth_required".into());
                        report.ignored_count += 1;
                    }
                }
                HomeAssistantLifecycleState::AwaitingAuthOk => {
                    if frame_type == Some("auth_ok") {
                        report.authenticated = true;
                        let _subscribe_frame =
                            self.websocket_subscribe_state_changed_frame(report.subscription_id);
                        lifecycle_state = HomeAssistantLifecycleState::AwaitingSubscribeResult;
                    } else {
                        report.failure = Some("expected auth_ok".into());
                        report.ignored_count += 1;
                    }
                }
                HomeAssistantLifecycleState::AwaitingSubscribeResult => {
                    let is_subscription_ack = frame_type == Some("result")
                        && value.get("id").and_then(|value| value.as_u64())
                            == Some(report.subscription_id)
                        && value.get("success").and_then(|value| value.as_bool()) == Some(true);
                    if is_subscription_ack {
                        report.subscribed = true;
                        lifecycle_state = HomeAssistantLifecycleState::ReadingEvents;
                    } else {
                        report.failure = Some("expected state_changed subscription success".into());
                        report.ignored_count += 1;
                    }
                }
                HomeAssistantLifecycleState::ReadingEvents => {
                    if frame_type == Some("event") {
                        let ingest = self.ingest_websocket_text_to_buffer(frame.as_ref())?;
                        report.accepted_count += ingest.accepted_count;
                        report.ignored_count += ingest.ignored_count;
                        report.invalid_count += ingest.invalid_count;
                    } else {
                        report.ignored_count += 1;
                    }
                }
            }
        }

        Ok(report)
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<HomeAssistantDeliveryReport, AdapterError> {
        self.validate_token()?;
        let notification_url = self.notification_url()?;
        let notification_id = msg.thread_id.trim();
        if notification_id.is_empty() {
            return Err(AdapterError::Channel(
                "Home Assistant notification_id not configured".into(),
            ));
        }
        let body = serde_json::json!({
            "title": "Zaion Webhook",
            "message": truncate_chars(&msg.text, HASS_MESSAGE_LIMIT),
            "notification_id": notification_id,
        });

        let resp = self
            .client
            .as_ref()
            .ok_or_else(|| AdapterError::Channel("Home Assistant HTTP client unavailable".into()))?
            .post(&notification_url)
            .bearer_auth(self.access_token.as_str())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| {
                AdapterError::Channel(
                    self.redact_error(format!("Home Assistant API request failed: {}", e)),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "Home Assistant API HTTP {}: {}",
                status,
                self.redact_error(text)
            )));
        }

        let json: serde_json::Value = resp.json().unwrap_or(serde_json::Value::Null);
        if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
            return Err(AdapterError::Channel(format!(
                "Home Assistant API error: {}",
                self.redact_error(error)
            )));
        }
        let message_id = json
            .get("id")
            .or_else(|| json.get("notification_id"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| Some(format!("ha-{}", Uuid::new_v4().simple())));

        Ok(HomeAssistantDeliveryReport {
            notification_id: notification_id.to_string(),
            message_id,
            character_count: msg.text.chars().count(),
        })
    }
}

impl Drop for HomeAssistantAdapter {
    fn drop(&mut self) {
        drop_homeassistant_blocking_client_safely(self.client.take());
    }
}

fn drop_homeassistant_blocking_client_safely(client: Option<reqwest::blocking::Client>) {
    let Some(client) = client else {
        return;
    };
    let _ = std::thread::spawn(move || drop(client)).join();
}

fn build_homeassistant_blocking_client(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeAssistantLifecycleState {
    AwaitingAuthRequired,
    AwaitingAuthOk,
    AwaitingSubscribeResult,
    ReadingEvents,
}

fn truncate_chars(text: &str, max_len: usize) -> String {
    text.chars().take(max_len).collect()
}

fn parse_homeassistant_event_epoch(timestamp: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn homeassistant_state_value(state: &serde_json::Value) -> String {
    state
        .get("state")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn homeassistant_friendly_name(entity_id: &str, new_state: &serde_json::Value) -> String {
    new_state
        .pointer("/attributes/friendly_name")
        .and_then(|value| value.as_str())
        .unwrap_or(entity_id)
        .to_string()
}

fn homeassistant_unit(state: &serde_json::Value) -> &str {
    state
        .pointer("/attributes/unit_of_measurement")
        .and_then(|value| value.as_str())
        .unwrap_or("")
}

fn format_homeassistant_state_change(
    entity_id: &str,
    old_state: &serde_json::Value,
    new_state: &serde_json::Value,
) -> Option<String> {
    if new_state.is_null() {
        return None;
    }
    let old_value = homeassistant_state_value(old_state);
    let new_value = homeassistant_state_value(new_state);
    if old_value == new_value {
        return None;
    }
    let friendly_name = homeassistant_friendly_name(entity_id, new_state);
    let domain = entity_id
        .split_once('.')
        .map(|(domain, _)| domain)
        .unwrap_or("");

    match domain {
        "climate" => {
            let current = new_state
                .pointer("/attributes/current_temperature")
                .and_then(json_value_to_string)
                .unwrap_or_else(|| "?".into());
            let target = new_state
                .pointer("/attributes/temperature")
                .and_then(json_value_to_string)
                .unwrap_or_else(|| "?".into());
            Some(format!(
                "[Home Assistant] {friendly_name}: HVAC mode changed from '{old_value}' to '{new_value}' (current: {current}, target: {target})"
            ))
        }
        "sensor" => {
            let unit = homeassistant_unit(new_state);
            Some(format!(
                "[Home Assistant] {friendly_name}: changed from {old_value}{unit} to {new_value}{unit}"
            ))
        }
        "binary_sensor" => Some(format!(
            "[Home Assistant] {friendly_name}: {} (was {})",
            if new_value == "on" {
                "triggered"
            } else {
                "cleared"
            },
            if old_value == "on" {
                "triggered"
            } else {
                "cleared"
            }
        )),
        "light" | "switch" | "fan" => Some(format!(
            "[Home Assistant] {friendly_name}: turned {}",
            if new_value == "on" { "on" } else { "off" }
        )),
        "alarm_control_panel" => Some(format!(
            "[Home Assistant] {friendly_name}: alarm state changed from '{old_value}' to '{new_value}'"
        )),
        _ => Some(format!(
            "[Home Assistant] {friendly_name} ({entity_id}): changed from '{old_value}' to '{new_value}'"
        )),
    }
}

fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
        .or_else(|| value.as_f64().map(|number| number.to_string()))
}

fn inbound_payload_hash(message: &InboundMessage) -> Result<String, AdapterError> {
    let bytes = serde_json::to_vec(message)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn homeassistant_delivery_id(message: &InboundMessage) -> String {
    let entity_id = message
        .metadata
        .get("entity_id")
        .and_then(|value| value.as_str())
        .unwrap_or(&message.thread_id);
    format!(
        "homeassistant:{}:{}",
        entity_id.trim(),
        message.message_id.trim()
    )
}

fn insert_provenance_metadata(
    message: &mut InboundMessage,
    key: &str,
    provenance: &HomeAssistantInboundProvenance,
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

impl ChannelAdapter for HomeAssistantAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::HomeAssistant
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let mut buf = self.inbound_buffer.lock().map_err(|_| {
            AdapterError::Channel("homeassistant inbound buffer lock poisoned".into())
        })?;
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

    fn spawn_homeassistant_mock<F>(
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
    fn homeassistant_channel_type() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_watch_all(true);
        assert_eq!(adapter.channel_type(), ChannelType::HomeAssistant);
    }

    #[test]
    fn homeassistant_notification_url_formats_service_path() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_api_base_url("http://ha:8123/");
        assert_eq!(
            adapter.notification_url().unwrap(),
            "http://ha:8123/api/services/persistent_notification/create"
        );
    }

    #[test]
    fn homeassistant_send_with_report_posts_persistent_notification() {
        let (base_url, server) = spawn_homeassistant_mock(1, |request| {
            assert!(request.starts_with("POST /api/services/persistent_notification/create "));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer ha-token"));
            assert!(request.contains("\"notification_id\":\"zaion-research\""));
            assert!(request.contains("\"message\":\"hello\""));
            serde_json::json!({
                "id": "ha-notification-1",
            })
            .to_string()
        });
        let adapter = HomeAssistantAdapter::new("ha-token").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "homeassistant".into(),
            thread_id: "zaion-research".into(),
            text: "hello".into(),
            reply_to: None,
            metadata: serde_json::json!({"source": "webhook"}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&msg).unwrap();
        assert_eq!(report.notification_id, "zaion-research");
        assert_eq!(report.message_id.as_deref(), Some("ha-notification-1"));
        assert_eq!(report.character_count, 5);
        server.join().unwrap();
    }

    #[test]
    fn homeassistant_api_errors_redact_access_token() {
        let token = "ha-long-lived-token";
        let err = HomeAssistantAdapter::new(token).redact_error(format!("token={token}"));
        assert!(!err.contains(token));
        assert!(err.contains("[REDACTED]"));
    }

    #[test]
    fn homeassistant_ingest_state_changed_builds_canonical_inbound() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_watch_all(true);
        let event = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-20T08:30:00.000000+00:00",
                "data": {
                    "entity_id": "sensor.lab_temperature",
                    "old_state": {
                        "state": "21",
                        "attributes": {
                            "friendly_name": "Lab Temperature",
                            "unit_of_measurement": "C"
                        }
                    },
                    "new_state": {
                        "state": "24",
                        "attributes": {
                            "friendly_name": "Lab Temperature",
                            "unit_of_measurement": "C"
                        }
                    }
                }
            }
        });

        let message = adapter
            .ingest_state_changed_event(&event)
            .unwrap()
            .expect("state_changed event should produce an inbound message");

        assert_eq!(message.channel_id, "homeassistant");
        assert_eq!(message.thread_id, "ha_events");
        assert_eq!(message.sender_id, "homeassistant");
        assert!(message.message_id.starts_with("ha_sensor.lab_temperature_"));
        assert_eq!(message.timestamp, "2026-05-20T08:30:00.000000+00:00");
        assert_eq!(
            message.text,
            "[Home Assistant] Lab Temperature: changed from 21C to 24C"
        );
        assert_eq!(message.metadata["provider"], "homeassistant");
        assert_eq!(message.metadata["transport"], "homeassistant_websocket");
        assert_eq!(message.metadata["domain"], "sensor");
        assert_eq!(message.metadata["entity_id"], "sensor.lab_temperature");
        assert_eq!(message.metadata["old_state"], "21");
        assert_eq!(message.metadata["new_state"], "24");

        let envelope = message
            .to_canonical_envelope(
                "homeassistant",
                zaion_types::identity::PrincipalId("did:key:ha-inbound".into()),
            )
            .expect("Home Assistant inbound should be canonical-envelope ready");
        assert_eq!(envelope.channel.0, "homeassistant");
        assert_eq!(envelope.thread.0, "ha_events");
        assert_eq!(
            envelope.metadata["adapter_metadata"]["transport"],
            "homeassistant_websocket"
        );
        assert_eq!(envelope.source_hash.len(), 64);
    }

    #[test]
    fn homeassistant_ingest_state_changed_filters_watch_rules_and_cooldown() {
        let adapter = HomeAssistantAdapter::new("ha-token")
            .with_watch_domains(["binary_sensor"])
            .with_ignored_entities(["binary_sensor.back_door"])
            .with_cooldown_seconds(30);
        let ignored = serde_json::json!({
            "event_type": "state_changed",
            "time_fired": "2026-05-20T08:31:00Z",
            "data": {
                "entity_id": "binary_sensor.back_door",
                "old_state": {"state": "off", "attributes": {"friendly_name": "Back Door"}},
                "new_state": {"state": "on", "attributes": {"friendly_name": "Back Door"}}
            }
        });
        assert!(adapter
            .ingest_state_changed_event(&ignored)
            .unwrap()
            .is_none());

        let watched = serde_json::json!({
            "event_type": "state_changed",
            "time_fired": "2026-05-20T08:31:05Z",
            "data": {
                "entity_id": "binary_sensor.front_door",
                "old_state": {"state": "off", "attributes": {"friendly_name": "Front Door"}},
                "new_state": {"state": "on", "attributes": {"friendly_name": "Front Door"}}
            }
        });
        let first = adapter.ingest_state_changed_event(&watched).unwrap();
        let second = adapter.ingest_state_changed_event(&watched).unwrap();

        assert_eq!(
            first.expect("watched entity should be accepted").text,
            "[Home Assistant] Front Door: triggered (was cleared)"
        );
        assert!(second.is_none(), "cooldown should suppress duplicate event");
    }

    #[test]
    fn homeassistant_ingest_state_changed_to_buffer_feeds_channel_receive() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_watch_all(true);
        let event = serde_json::json!({
            "event_type": "state_changed",
            "time_fired": "2026-05-20T08:32:00Z",
            "data": {
                "entity_id": "light.office",
                "old_state": {"state": "off", "attributes": {"friendly_name": "Office"}},
                "new_state": {"state": "on", "attributes": {"friendly_name": "Office"}}
            }
        });

        assert!(adapter
            .ingest_state_changed_event_to_buffer(&event)
            .unwrap());
        let received = adapter.receive().unwrap();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].metadata["entity_id"], "light.office");
        assert_eq!(received[0].metadata["transport"], "homeassistant_websocket");
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn homeassistant_ingest_websocket_text_frame_buffers_state_changed_events() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_watch_all(true);
        let frame = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-20T08:33:00Z",
                "data": {
                    "entity_id": "switch.pump",
                    "old_state": {"state": "off", "attributes": {"friendly_name": "Pump"}},
                    "new_state": {"state": "on", "attributes": {"friendly_name": "Pump"}}
                }
            }
        })
        .to_string();

        let accepted = adapter.ingest_websocket_text_to_buffer(&frame).unwrap();
        let ignored = adapter
            .ingest_websocket_text_to_buffer("{\"type\":\"result\",\"success\":true}")
            .unwrap();
        let invalid = adapter.ingest_websocket_text_to_buffer("not json").unwrap();
        let received = adapter.receive().unwrap();

        assert_eq!(accepted.accepted_count, 1);
        assert_eq!(accepted.ignored_count, 0);
        assert_eq!(accepted.invalid_count, 0);
        assert_eq!(ignored.accepted_count, 0);
        assert_eq!(ignored.ignored_count, 1);
        assert_eq!(invalid.invalid_count, 1);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "[Home Assistant] Pump: turned on");
    }

    #[test]
    fn homeassistant_websocket_lifecycle_authenticates_subscribes_and_buffers_events() {
        let adapter = HomeAssistantAdapter::new("ha-token")
            .with_api_base_url("http://ha.local:8123/")
            .with_watch_all(true);

        assert_eq!(
            adapter.websocket_url().unwrap(),
            "ws://ha.local:8123/api/websocket"
        );
        assert_eq!(
            HomeAssistantAdapter::new("ha-token")
                .with_api_base_url("https://ha.example")
                .websocket_url()
                .unwrap(),
            "wss://ha.example/api/websocket"
        );
        assert_eq!(
            adapter.websocket_auth_frame().unwrap(),
            serde_json::json!({
                "type": "auth",
                "access_token": "ha-token",
            })
        );
        assert_eq!(
            adapter.websocket_subscribe_state_changed_frame(7),
            serde_json::json!({
                "id": 7,
                "type": "subscribe_events",
                "event_type": "state_changed",
            })
        );

        let event_frame = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-20T08:34:00Z",
                "data": {
                    "entity_id": "binary_sensor.motion",
                    "old_state": {"state": "off", "attributes": {"friendly_name": "Motion"}},
                    "new_state": {"state": "on", "attributes": {"friendly_name": "Motion"}}
                }
            }
        })
        .to_string();
        let frames = [
            "{\"type\":\"auth_required\"}".to_string(),
            "{\"type\":\"auth_ok\"}".to_string(),
            "{\"id\":1,\"type\":\"result\",\"success\":true}".to_string(),
            event_frame,
            "not-json".to_string(),
        ];

        let report = adapter
            .ingest_websocket_lifecycle_to_buffer(frames)
            .unwrap();
        let received = adapter.receive().unwrap();

        assert_eq!(report.websocket_url, "ws://ha.local:8123/api/websocket");
        assert!(report.auth_required_seen);
        assert!(report.authenticated);
        assert!(report.subscribed);
        assert_eq!(report.subscription_id, 1);
        assert_eq!(report.frames_read, 5);
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.invalid_count, 1);
        assert_eq!(report.failure, None);
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].text,
            "[Home Assistant] Motion: triggered (was cleared)"
        );
    }

    #[test]
    fn homeassistant_websocket_inbound_service_records_signed_provenance_before_buffering() {
        let adapter = HomeAssistantAdapter::new("ha-token").with_watch_all(true);
        let keypair = std::sync::Arc::new(zaion_crypto::ZaionKeypair::generate());
        let service = HomeAssistantWebSocketInboundService::new_with_key(&adapter, keypair);
        let frame = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-20T08:35:00Z",
                "data": {
                    "entity_id": "light.signed_lamp",
                    "old_state": {"state": "off", "attributes": {"friendly_name": "Signed Lamp"}},
                    "new_state": {"state": "on", "attributes": {"friendly_name": "Signed Lamp"}}
                }
            }
        })
        .to_string();

        let report = service.ingest_websocket_text(&frame).unwrap();
        let received = adapter.receive().unwrap();
        let ledger = service.provenance_ledger().unwrap();

        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);
        assert_eq!(report.provenance_delivery_ids.len(), 1);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].route_name, "homeassistant.inbound.websocket");
        assert_eq!(ledger[0].message_id, received[0].message_id);
        assert_eq!(ledger[0].payload_hash.len(), 64);
        ledger[0]
            .to_delivery_receipt()
            .verify_receipt(&service.verifying_key())
            .expect("Home Assistant provenance receipt should verify");

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "[Home Assistant] Signed Lamp: turned on");
        assert_eq!(
            received[0].metadata["homeassistant_provenance"]["delivery_id"],
            ledger[0].delivery_id
        );
        assert_eq!(
            received[0].metadata["homeassistant_provenance"]["payload_hash"],
            ledger[0].payload_hash
        );
        assert_eq!(
            received[0].metadata["homeassistant_provenance"]["receipt_schema_version"],
            2
        );
    }
}
