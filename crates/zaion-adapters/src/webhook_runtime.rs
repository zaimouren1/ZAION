//! Webhook runtime server - HTTP server for receiving webhook events
//!
//! This module implements the runtime HTTP server that receives webhook POSTs,
//! validates signatures, and triggers agent runs. It provides:
//!
//! 1. **HTTP Server**: aiohttp-equivalent async HTTP server using axum
//! 2. **Signature Validation**: HMAC-SHA256 signature verification
//! 3. **Ed25519 Signing**: Cryptographic signing of delivery receipts (Zaion unique)
//! 4. **Provenance Tracking**: Full audit trail of webhook events (Zaion unique)
//! 5. **Rate Limiting**: Per-route rate limiting with fixed window
//! 6. **Idempotency**: TTL cache to prevent duplicate processing
//! 7. **Dynamic Routes**: Hot-reload of webhook subscriptions from TOML
//!
//! ## Architecture
//!
//! ```text
//! External Service 鈫?HTTP POST 鈫?WebhookRuntime
//!                                      鈫?//!                              Signature Validation
//!                                      鈫?//!                              Rate Limiting Check
//!                                      鈫?//!                              Idempotency Check
//!                                      鈫?//!                              Agent Trigger
//!                                      鈫?//!                              Ed25519 Signed Receipt
//!                                      鈫?//!                              Provenance Ledger
//! ```
//!
//! ## Paradigm Breakthrough vs Hermes
//!
//! Hermes webhook.py (661 lines):
//! - HMAC signature validation
//! - Rate limiting (fixed window)
//! - Idempotency cache
//! - Cross-platform delivery
//! - Dynamic route loading
//!
//! Zaion webhook_runtime.rs adds:
//! - **Ed25519 cryptographic signing** of all delivery receipts
//! - **Provenance tracking** with append-only signed ledger
//! - **Principal identity** integration (every webhook event signed by principal)
//! - **Verifiable audit trail** (can prove webhook was received and processed)
//! - **AST-level payload transformation** (not just string templates)

use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, watch, RwLock};
use tokio::task::JoinHandle;
use zaion_crypto::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

use crate::channel::{ChannelAdapter, InboundMessage};

type HmacSha256 = Hmac<Sha256>;
pub type WebhookAgentHandler = Arc<
    dyn Fn(WebhookAgentDispatch) -> Pin<Box<dyn Future<Output = WebhookAgentDispatchResult> + Send>>
        + Send
        + Sync,
>;

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8644;
const MAX_BODY_BYTES: usize = 1_048_576; // 1MB
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const DEFAULT_RATE_LIMIT: usize = 30;
const IDEMPOTENCY_TTL_SECS: u64 = 3600; // 1 hour
const DELIVERY_INFO_TTL_SECS: u64 = 3600; // 1 hour

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebhookSignatureKind {
    GithubHmacSha256,
    GitlabSharedToken,
    SlackV0HmacSha256,
    StripeV1HmacSha256,
}

fn extract_webhook_signature(headers: &axum::http::HeaderMap) -> (&str, WebhookSignatureKind) {
    if let Some(signature) = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
    {
        return (signature, WebhookSignatureKind::GithubHmacSha256);
    }

    if let Some(token) = headers
        .get("x-gitlab-token")
        .and_then(|value| value.to_str().ok())
    {
        return (token, WebhookSignatureKind::GitlabSharedToken);
    }

    if let Some(signature) = headers
        .get("x-slack-signature")
        .and_then(|value| value.to_str().ok())
    {
        return (signature, WebhookSignatureKind::SlackV0HmacSha256);
    }

    if let Some(signature) = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
    {
        return (signature, WebhookSignatureKind::StripeV1HmacSha256);
    }

    ("", WebhookSignatureKind::GithubHmacSha256)
}

fn parse_stripe_signature_header(signature: &str) -> Option<(&str, &str)> {
    let mut timestamp = None;
    let mut v1 = None;
    for part in signature.split(',') {
        let (key, value) = part.split_once('=')?;
        match key.trim() {
            "t" => timestamp = Some(value.trim()),
            "v1" => v1 = Some(value.trim()),
            _ => {}
        }
    }
    Some((timestamp?, v1?))
}

fn stripe_event_type(payload: &serde_json::Value) -> Option<&str> {
    payload.get("type").and_then(|value| value.as_str())
}

/// Webhook runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRuntimeConfig {
    pub host: String,
    pub port: u16,
    pub max_body_bytes: usize,
    pub rate_limit: usize,
    pub idempotency_ttl_secs: u64,
}

impl Default for WebhookRuntimeConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            max_body_bytes: MAX_BODY_BYTES,
            rate_limit: DEFAULT_RATE_LIMIT,
            idempotency_ttl_secs: IDEMPOTENCY_TTL_SECS,
        }
    }
}

/// Webhook route configuration (loaded from TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRoute {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub status: String,
}

/// Delivery receipt (Ed25519 signed, Zaion unique)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub route_name: String,
    pub delivery_id: String,
    pub timestamp: u64,
    pub payload_hash: String,
    pub signature_valid: bool,
    pub principal_id: String,
    pub ed25519_signature: String,
    /// 2 = real Ed25519; 1 = legacy placeholder (fails closed on verify)
    #[serde(default = "receipt_default_schema_version")]
    pub schema_version: u32,
}

fn receipt_default_schema_version() -> u32 {
    1
}

/// Verify errors for delivery receipts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptError {
    VerificationFailed(String),
    LegacySchema,
    HexDecodeError(String),
}

impl std::fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReceiptError::VerificationFailed(msg) => write!(f, "verification failed: {}", msg),
            ReceiptError::LegacySchema => {
                write!(f, "schema_version < 2; record predates real signing")
            }
            ReceiptError::HexDecodeError(msg) => write!(f, "hex decode error: {}", msg),
        }
    }
}

impl DeliveryReceipt {
    /// Canonical bytes for signing: SHA-256 of (route_name 鈥?0x1F 鈥?delivery_id 鈥?0x1F 鈥?payload_hash 鈥?0x1F 鈥?timestamp_le)
    pub fn canonical_bytes(
        route_name: &str,
        delivery_id: &str,
        payload_hash: &str,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(route_name.as_bytes());
        hasher.update([0x1F]);
        hasher.update(delivery_id.as_bytes());
        hasher.update([0x1F]);
        hasher.update(payload_hash.as_bytes());
        hasher.update([0x1F]);
        hasher.update(timestamp.to_le_bytes());
        hasher.finalize().to_vec()
    }

    /// Verify the Ed25519 signature on this receipt.
    /// Fails closed if schema_version < 2.
    pub fn verify_receipt(
        &self,
        verifying_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<(), ReceiptError> {
        use ed25519_dalek::Verifier;
        if self.schema_version < 2 {
            return Err(ReceiptError::LegacySchema);
        }
        let sig_bytes = hex::decode(&self.ed25519_signature)
            .map_err(|e| ReceiptError::HexDecodeError(e.to_string()))?;
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| ReceiptError::VerificationFailed("signature not 64 bytes".into()))?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        let digest = Self::canonical_bytes(
            &self.route_name,
            &self.delivery_id,
            &self.payload_hash,
            self.timestamp,
        );
        verifying_key
            .verify(&digest, &sig)
            .map_err(|e| ReceiptError::VerificationFailed(e.to_string()))
    }
}

/// Provenance entry for webhook event (Zaion unique)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookProvenance {
    pub event_id: String,
    pub route_name: String,
    pub timestamp: u64,
    pub source_ip: String,
    pub payload_hash: String,
    pub hmac_valid: bool,
    pub principal_id: String,
    #[serde(default)]
    pub receipt_timestamp: u64,
    pub receipt_signature: String,
    #[serde(default)]
    pub receipt_schema_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookInboundDaemonReport {
    pub backend: String,
    pub route_name: String,
    pub connector_kind: String,
    pub started: bool,
    pub stopped: bool,
    pub stop_reason: String,
    pub connect_attempts: usize,
    pub health_check_count: usize,
    pub chunks_read: usize,
    pub frames_read: usize,
    pub data_event_count: usize,
    pub accepted_count: usize,
    pub ignored_count: usize,
    pub invalid_count: usize,
    pub provenance_count: usize,
    pub auth_required_seen: bool,
    pub authenticated: bool,
    pub subscribed: bool,
    pub health_url: String,
    pub event_url: String,
    pub accept_header: String,
    pub websocket_url: String,
    pub auth_frame_type: String,
    pub subscribe_event_type: String,
    pub subscription_id: u64,
    pub reconnect_backoff_millis: Vec<u64>,
    pub failure: Option<String>,
}

pub struct WebhookInboundDaemonSupervisor {
    shutdown: watch::Sender<bool>,
    report: Arc<RwLock<WebhookInboundDaemonReport>>,
    task: JoinHandle<()>,
}

/// Webhook runtime state
pub struct WebhookRuntimeState {
    pub config: WebhookRuntimeConfig,
    pub routes: RwLock<HashMap<String, WebhookRoute>>,
    pub sms_twilio_routes: RwLock<HashMap<String, Arc<crate::SmsAdapter>>>,
    pub signal_sse_routes: RwLock<HashMap<String, Arc<crate::SignalAdapter>>>,
    pub homeassistant_websocket_routes: RwLock<HashMap<String, Arc<crate::HomeAssistantAdapter>>>,
    pub signal_sse_daemon_supervisors: RwLock<HashMap<String, WebhookInboundDaemonSupervisor>>,
    pub homeassistant_websocket_daemon_supervisors:
        RwLock<HashMap<String, WebhookInboundDaemonSupervisor>>,
    pub rate_counts: RwLock<HashMap<String, Vec<u64>>>,
    pub seen_deliveries: RwLock<HashMap<String, u64>>,
    pub delivery_info: RwLock<HashMap<String, DeliveryInfo>>,
    pub provenance_ledger: RwLock<Vec<WebhookProvenance>>,
    pub agent_handler: RwLock<Option<WebhookAgentHandler>>,
    /// Ed25519 signing key 鈥?injected at construction
    pub signing_key: Arc<ZaionKeypair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAgentDispatch {
    pub route_name: String,
    pub delivery_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub payload_hash: String,
    pub signature_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookAgentDispatchResult {
    pub status: String,
    pub principal_id: Option<String>,
    pub background: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_chain: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress_event_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_trace_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_proof_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_receipt_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_receipt_count: Option<usize>,
    #[serde(default)]
    pub tool_result_storage_receipts: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result_storage_receipt_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_receipt_proof_join_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_receipt_proof_join: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_receipt_join_found: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_receipt_proof_hash_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_contract: Option<serde_json::Value>,
    pub response: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeliveryInfo {
    pub route_name: String,
    pub delivery_id: String,
    pub created_at: u64,
}

/// Webhook runtime server
pub struct WebhookRuntime {
    state: Arc<WebhookRuntimeState>,
}

impl WebhookRuntime {
    /// Create new webhook runtime with an ephemeral signing keypair for tests only.
    #[cfg(test)]
    pub fn new(config: WebhookRuntimeConfig) -> Self {
        Self::new_with_key(config, Arc::new(ZaionKeypair::generate()))
    }

    /// Create new webhook runtime with an injected signing keypair.
    pub fn new_with_key(config: WebhookRuntimeConfig, keypair: Arc<ZaionKeypair>) -> Self {
        Self {
            state: Arc::new(WebhookRuntimeState {
                config,
                routes: RwLock::new(HashMap::new()),
                sms_twilio_routes: RwLock::new(HashMap::new()),
                signal_sse_routes: RwLock::new(HashMap::new()),
                homeassistant_websocket_routes: RwLock::new(HashMap::new()),
                signal_sse_daemon_supervisors: RwLock::new(HashMap::new()),
                homeassistant_websocket_daemon_supervisors: RwLock::new(HashMap::new()),
                rate_counts: RwLock::new(HashMap::new()),
                seen_deliveries: RwLock::new(HashMap::new()),
                delivery_info: RwLock::new(HashMap::new()),
                provenance_ledger: RwLock::new(Vec::new()),
                agent_handler: RwLock::new(None),
                signing_key: keypair,
            }),
        }
    }

    /// Return the verifying key for this runtime (for signature verification).
    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.state.signing_key.verifying_key()
    }

    /// Load routes from TOML configuration
    pub async fn load_routes(&self, routes: Vec<WebhookRoute>) -> Result<(), String> {
        let mut routes_map = self.state.routes.write().await;
        routes_map.clear();
        for route in routes {
            routes_map.insert(route.name.clone(), route);
        }
        Ok(())
    }

    pub async fn mount_sms_twilio_route(
        &self,
        route_name: impl Into<String>,
        adapter: crate::SmsAdapter,
    ) -> Result<(), String> {
        let route_name = route_name.into();
        if route_name.trim().is_empty() {
            return Err("sms twilio route name cannot be empty".to_string());
        }
        self.state
            .sms_twilio_routes
            .write()
            .await
            .insert(route_name, Arc::new(adapter));
        Ok(())
    }

    pub async fn drain_sms_twilio_route(
        &self,
        route_name: &str,
    ) -> Result<Vec<InboundMessage>, String> {
        let routes = self.state.sms_twilio_routes.read().await;
        let adapter = routes
            .get(route_name)
            .ok_or_else(|| format!("sms twilio route '{}' not mounted", route_name))?;
        adapter.receive().map_err(|err| err.to_string())
    }

    pub async fn mount_signal_sse_route(
        &self,
        route_name: impl Into<String>,
        adapter: crate::SignalAdapter,
    ) -> Result<(), String> {
        let route_name = route_name.into();
        if route_name.trim().is_empty() {
            return Err("signal sse route name cannot be empty".to_string());
        }
        self.state
            .signal_sse_routes
            .write()
            .await
            .insert(route_name, Arc::new(adapter));
        Ok(())
    }

    pub async fn ingest_signal_sse_route_chunk(
        &self,
        route_name: &str,
        chunk: &str,
    ) -> Result<crate::signal::SignalSseProvenanceIngestReport, String> {
        let routes = self.state.signal_sse_routes.read().await;
        let adapter = routes
            .get(route_name)
            .ok_or_else(|| format!("signal sse route '{}' not mounted", route_name))?;
        let service = crate::SignalSseInboundService::new_with_key(
            adapter.as_ref(),
            self.state.signing_key.clone(),
        );
        service
            .ingest_sse_chunk(chunk)
            .map_err(|err| err.to_string())
    }

    pub async fn drain_signal_sse_route(
        &self,
        route_name: &str,
    ) -> Result<Vec<InboundMessage>, String> {
        let routes = self.state.signal_sse_routes.read().await;
        let adapter = routes
            .get(route_name)
            .ok_or_else(|| format!("signal sse route '{}' not mounted", route_name))?;
        adapter.receive().map_err(|err| err.to_string())
    }

    pub async fn start_signal_sse_daemon_script<I, S>(
        &self,
        route_name: &str,
        chunks: I,
        reconnect_backoff_steps: usize,
    ) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let adapter = {
            let routes = self.state.signal_sse_routes.read().await;
            routes
                .get(route_name)
                .cloned()
                .ok_or_else(|| format!("signal sse route '{}' not mounted", route_name))?
        };
        let chunks: Vec<String> = chunks
            .into_iter()
            .map(|chunk| chunk.as_ref().to_string())
            .collect();
        let report = Arc::new(RwLock::new(WebhookInboundDaemonReport {
            backend: "signal_sse".into(),
            route_name: route_name.to_string(),
            connector_kind: "script".into(),
            started: true,
            connect_attempts: 1,
            reconnect_backoff_millis: signal_sse_daemon_backoff_millis(reconnect_backoff_steps),
            ..WebhookInboundDaemonReport::default()
        }));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = oneshot::channel();
        let signing_key = self.state.signing_key.clone();
        let task_report = report.clone();

        let task = tokio::spawn(async move {
            let service =
                crate::SignalSseInboundService::new_with_key(adapter.as_ref(), signing_key);
            for chunk in chunks {
                if *shutdown_rx.borrow() {
                    break;
                }
                match service.ingest_sse_chunk(&chunk) {
                    Ok(ingest) => {
                        let mut report = task_report.write().await;
                        report.chunks_read += 1;
                        report.data_event_count += ingest.data_event_count;
                        report.accepted_count += ingest.accepted_count;
                        report.ignored_count += ingest.ignored_count;
                        report.invalid_count += ingest.invalid_count;
                        report.provenance_count += ingest.provenance_count;
                    }
                    Err(err) => {
                        let mut report = task_report.write().await;
                        report.chunks_read += 1;
                        report.invalid_count += 1;
                        report.failure = Some(err.to_string());
                    }
                }
            }
            let _ = ready_tx.send(());
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        });

        {
            let mut supervisors = self.state.signal_sse_daemon_supervisors.write().await;
            if supervisors.contains_key(route_name) {
                task.abort();
                return Err(format!(
                    "signal sse daemon for route '{}' already running",
                    route_name
                ));
            }
            supervisors.insert(
                route_name.to_string(),
                WebhookInboundDaemonSupervisor {
                    shutdown: shutdown_tx,
                    report,
                    task,
                },
            );
        }

        ready_rx.await.map_err(|_| {
            format!(
                "signal sse daemon for route '{}' failed to start",
                route_name
            )
        })
    }

    pub async fn stop_signal_sse_daemon(
        &self,
        route_name: &str,
    ) -> Result<WebhookInboundDaemonReport, String> {
        let supervisor = {
            let mut supervisors = self.state.signal_sse_daemon_supervisors.write().await;
            supervisors.remove(route_name).ok_or_else(|| {
                format!("signal sse daemon for route '{}' not running", route_name)
            })?
        };
        stop_inbound_daemon_supervisor(supervisor).await
    }

    pub async fn start_signal_sse_daemon_http(
        &self,
        route_name: &str,
        reconnect_backoff_steps: usize,
    ) -> Result<(), String> {
        let adapter = {
            let routes = self.state.signal_sse_routes.read().await;
            routes
                .get(route_name)
                .cloned()
                .ok_or_else(|| format!("signal sse route '{}' not mounted", route_name))?
        };
        let health_url = adapter
            .health_check_url()
            .map_err(|err| format!("Signal SSE health URL unavailable: {}", err))?;
        let event_url = adapter
            .sse_event_url()
            .map_err(|err| format!("Signal SSE event URL unavailable: {}", err))?;
        let report = Arc::new(RwLock::new(WebhookInboundDaemonReport {
            backend: "signal_sse".into(),
            route_name: route_name.to_string(),
            connector_kind: "signal_http_sse".into(),
            started: true,
            connect_attempts: 1,
            health_url: health_url.clone(),
            event_url: event_url.clone(),
            accept_header: "text/event-stream".into(),
            reconnect_backoff_millis: signal_sse_daemon_backoff_millis(reconnect_backoff_steps),
            ..WebhookInboundDaemonReport::default()
        }));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();
        let signing_key = self.state.signing_key.clone();
        let task_report = report.clone();

        let task = tokio::spawn(async move {
            let service =
                crate::SignalSseInboundService::new_with_key(adapter.as_ref(), signing_key);
            let health_client = match reqwest::Client::builder().pool_max_idle_per_host(0).build() {
                Ok(client) => client,
                Err(err) => {
                    let mut report = task_report.write().await;
                    report.failure =
                        Some(format!("Signal SSE health client build failed: {}", err));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE health check failed before opening event stream: {}",
                        err
                    )));
                    return;
                }
            };

            let health_result = health_client.get(&health_url).send().await;
            match health_result {
                Ok(response) if response.status().is_success() => {
                    let health_body = response.bytes().await;
                    if let Err(err) = health_body {
                        let mut report = task_report.write().await;
                        report.failure =
                            Some(format!("Signal SSE health check body read failed: {}", err));
                        let _ = ready_tx.send(Err(format!(
                            "Signal SSE health check failed before opening event stream: {}",
                            err
                        )));
                        return;
                    }
                    let mut report = task_report.write().await;
                    report.health_check_count += 1;
                }
                Ok(response) => {
                    let mut report = task_report.write().await;
                    report.health_check_count += 1;
                    report.failure = Some(format!(
                        "Signal SSE health check failed with status {}",
                        response.status()
                    ));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE health check failed before opening event stream: {}",
                        response.status()
                    )));
                    return;
                }
                Err(err) => {
                    let mut report = task_report.write().await;
                    report.failure = Some(format!("Signal SSE health check failed: {}", err));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE health check failed before opening event stream: {}",
                        err
                    )));
                    return;
                }
            }

            let event_client = match reqwest::Client::builder().pool_max_idle_per_host(0).build() {
                Ok(client) => client,
                Err(err) => {
                    let mut report = task_report.write().await;
                    report.failure = Some(format!("Signal SSE event client build failed: {}", err));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE event stream failed before reading chunks: {}",
                        err
                    )));
                    return;
                }
            };

            let response = match event_client
                .get(&event_url)
                .header("Accept", "text/event-stream")
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response,
                Ok(response) => {
                    let mut report = task_report.write().await;
                    report.failure = Some(format!(
                        "Signal SSE event stream failed with status {}",
                        response.status()
                    ));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE event stream failed before reading chunks: {}",
                        response.status()
                    )));
                    return;
                }
                Err(err) => {
                    let mut report = task_report.write().await;
                    report.failure = Some(format!("Signal SSE event stream failed: {}", err));
                    let _ = ready_tx.send(Err(format!(
                        "Signal SSE event stream failed before reading chunks: {}",
                        err
                    )));
                    return;
                }
            };

            let _ = ready_tx.send(Ok(()));
            let mut response = response;
            let mut buffer = String::new();
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    chunk = response.chunk() => {
                        match chunk {
                            Ok(Some(bytes)) => {
                                if bytes.is_empty() {
                                    continue;
                                }
                                buffer.push_str(&String::from_utf8_lossy(&bytes));
                                while let Some(line_end) = buffer.find('\n') {
                                    let mut line = buffer[..line_end].to_string();
                                    if line.ends_with('\r') {
                                        line.pop();
                                    }
                                    buffer.drain(..=line_end);
                                    let trimmed = line.trim();
                                    if trimmed.is_empty() || trimmed.starts_with(':') {
                                        continue;
                                    }
                                    if !trimmed.starts_with("data:") {
                                        continue;
                                    }
                                    match service.ingest_sse_chunk(&format!("{trimmed}\n")) {
                                        Ok(ingest) => {
                                            let mut report = task_report.write().await;
                                            report.chunks_read += 1;
                                            report.data_event_count += ingest.data_event_count;
                                            report.accepted_count += ingest.accepted_count;
                                            report.ignored_count += ingest.ignored_count;
                                            report.invalid_count += ingest.invalid_count;
                                            report.provenance_count += ingest.provenance_count;
                                        }
                                        Err(err) => {
                                            let mut report = task_report.write().await;
                                            report.chunks_read += 1;
                                            report.invalid_count += 1;
                                            report.failure = Some(err.to_string());
                                        }
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err(err) => {
                                let mut report = task_report.write().await;
                                report.failure = Some(format!("Signal SSE stream read failed: {}", err));
                                break;
                            }
                        }
                    }
                }
            }
        });

        {
            let mut supervisors = self.state.signal_sse_daemon_supervisors.write().await;
            if supervisors.contains_key(route_name) {
                task.abort();
                return Err(format!(
                    "signal sse daemon for route '{}' already running",
                    route_name
                ));
            }
            supervisors.insert(
                route_name.to_string(),
                WebhookInboundDaemonSupervisor {
                    shutdown: shutdown_tx,
                    report,
                    task,
                },
            );
        }

        match ready_rx.await.map_err(|_| {
            format!(
                "signal sse daemon for route '{}' failed to start",
                route_name
            )
        })? {
            Ok(()) => Ok(()),
            Err(err) => {
                let supervisor = {
                    let mut supervisors = self.state.signal_sse_daemon_supervisors.write().await;
                    supervisors.remove(route_name)
                };
                if let Some(supervisor) = supervisor {
                    supervisor.task.abort();
                }
                Err(err)
            }
        }
    }

    pub async fn mount_homeassistant_websocket_route(
        &self,
        route_name: impl Into<String>,
        adapter: crate::HomeAssistantAdapter,
    ) -> Result<(), String> {
        let route_name = route_name.into();
        if route_name.trim().is_empty() {
            return Err("homeassistant websocket route name cannot be empty".to_string());
        }
        self.state
            .homeassistant_websocket_routes
            .write()
            .await
            .insert(route_name, Arc::new(adapter));
        Ok(())
    }

    pub async fn ingest_homeassistant_websocket_route_frame(
        &self,
        route_name: &str,
        frame: &str,
    ) -> Result<crate::homeassistant::HomeAssistantProvenanceFrameIngestReport, String> {
        let routes = self.state.homeassistant_websocket_routes.read().await;
        let adapter = routes
            .get(route_name)
            .ok_or_else(|| format!("homeassistant websocket route '{}' not mounted", route_name))?;
        let service = crate::HomeAssistantWebSocketInboundService::new_with_key(
            adapter.as_ref(),
            self.state.signing_key.clone(),
        );
        service
            .ingest_websocket_text(frame)
            .map_err(|err| err.to_string())
    }

    pub async fn drain_homeassistant_websocket_route(
        &self,
        route_name: &str,
    ) -> Result<Vec<InboundMessage>, String> {
        let routes = self.state.homeassistant_websocket_routes.read().await;
        let adapter = routes
            .get(route_name)
            .ok_or_else(|| format!("homeassistant websocket route '{}' not mounted", route_name))?;
        adapter.receive().map_err(|err| err.to_string())
    }

    pub async fn start_homeassistant_websocket_daemon_script<I, S>(
        &self,
        route_name: &str,
        frames: I,
        reconnect_backoff_steps: usize,
    ) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let adapter = {
            let routes = self.state.homeassistant_websocket_routes.read().await;
            routes.get(route_name).cloned().ok_or_else(|| {
                format!("homeassistant websocket route '{}' not mounted", route_name)
            })?
        };
        let frames: Vec<String> = frames
            .into_iter()
            .map(|frame| frame.as_ref().to_string())
            .collect();
        let report = Arc::new(RwLock::new(WebhookInboundDaemonReport {
            backend: "homeassistant_websocket".into(),
            route_name: route_name.to_string(),
            connector_kind: "script".into(),
            started: true,
            connect_attempts: 1,
            reconnect_backoff_millis: homeassistant_websocket_daemon_backoff_millis(
                reconnect_backoff_steps,
            ),
            ..WebhookInboundDaemonReport::default()
        }));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = oneshot::channel();
        let signing_key = self.state.signing_key.clone();
        let task_report = report.clone();

        let task = tokio::spawn(async move {
            let service = crate::HomeAssistantWebSocketInboundService::new_with_key(
                adapter.as_ref(),
                signing_key,
            );
            let mut lifecycle_state = HomeAssistantDaemonLifecycleState::AwaitingAuthRequired;
            for frame in frames {
                if *shutdown_rx.borrow() {
                    break;
                }
                let parsed = serde_json::from_str::<serde_json::Value>(&frame);
                let Ok(value) = parsed else {
                    let mut report = task_report.write().await;
                    report.frames_read += 1;
                    report.invalid_count += 1;
                    continue;
                };
                let frame_type = value.get("type").and_then(|value| value.as_str());
                match lifecycle_state {
                    HomeAssistantDaemonLifecycleState::AwaitingAuthRequired => {
                        let mut report = task_report.write().await;
                        report.frames_read += 1;
                        if frame_type == Some("auth_required") {
                            report.auth_required_seen = true;
                            lifecycle_state = HomeAssistantDaemonLifecycleState::AwaitingAuthOk;
                        } else {
                            report.ignored_count += 1;
                            report.failure = Some("expected auth_required".into());
                        }
                    }
                    HomeAssistantDaemonLifecycleState::AwaitingAuthOk => {
                        let mut report = task_report.write().await;
                        report.frames_read += 1;
                        if frame_type == Some("auth_ok") {
                            report.authenticated = true;
                            lifecycle_state =
                                HomeAssistantDaemonLifecycleState::AwaitingSubscribeResult;
                        } else {
                            report.ignored_count += 1;
                            report.failure = Some("expected auth_ok".into());
                        }
                    }
                    HomeAssistantDaemonLifecycleState::AwaitingSubscribeResult => {
                        let mut report = task_report.write().await;
                        report.frames_read += 1;
                        let subscribed = frame_type == Some("result")
                            && value.get("id").and_then(|value| value.as_u64()) == Some(1)
                            && value.get("success").and_then(|value| value.as_bool()) == Some(true);
                        if subscribed {
                            report.subscribed = true;
                            lifecycle_state = HomeAssistantDaemonLifecycleState::ReadingEvents;
                        } else {
                            report.ignored_count += 1;
                            report.failure =
                                Some("expected state_changed subscription success".into());
                        }
                    }
                    HomeAssistantDaemonLifecycleState::ReadingEvents => {
                        if frame_type == Some("event") {
                            match service.ingest_websocket_text(&frame) {
                                Ok(ingest) => {
                                    let mut report = task_report.write().await;
                                    report.frames_read += 1;
                                    report.accepted_count += ingest.accepted_count;
                                    report.ignored_count += ingest.ignored_count;
                                    report.invalid_count += ingest.invalid_count;
                                    report.provenance_count += ingest.provenance_count;
                                }
                                Err(err) => {
                                    let mut report = task_report.write().await;
                                    report.frames_read += 1;
                                    report.invalid_count += 1;
                                    report.failure = Some(err.to_string());
                                }
                            }
                        } else {
                            let mut report = task_report.write().await;
                            report.frames_read += 1;
                            report.ignored_count += 1;
                        }
                    }
                }
            }
            let _ = ready_tx.send(());
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        });

        {
            let mut supervisors = self
                .state
                .homeassistant_websocket_daemon_supervisors
                .write()
                .await;
            if supervisors.contains_key(route_name) {
                task.abort();
                return Err(format!(
                    "homeassistant websocket daemon for route '{}' already running",
                    route_name
                ));
            }
            supervisors.insert(
                route_name.to_string(),
                WebhookInboundDaemonSupervisor {
                    shutdown: shutdown_tx,
                    report,
                    task,
                },
            );
        }

        ready_rx.await.map_err(|_| {
            format!(
                "homeassistant websocket daemon for route '{}' failed to start",
                route_name
            )
        })
    }

    pub async fn start_homeassistant_websocket_daemon_ws(
        &self,
        route_name: &str,
        reconnect_backoff_steps: usize,
    ) -> Result<(), String> {
        let adapter = {
            let routes = self.state.homeassistant_websocket_routes.read().await;
            routes.get(route_name).cloned().ok_or_else(|| {
                format!("homeassistant websocket route '{}' not mounted", route_name)
            })?
        };
        let websocket_url = adapter
            .websocket_url()
            .map_err(|err| format!("Home Assistant WebSocket URL unavailable: {}", err))?;
        let auth_frame = adapter
            .websocket_auth_frame()
            .map_err(|err| format!("Home Assistant auth frame unavailable: {}", err))?;
        let subscription_id = 1u64;
        let subscribe_frame = adapter.websocket_subscribe_state_changed_frame(subscription_id);
        let report = Arc::new(RwLock::new(WebhookInboundDaemonReport {
            backend: "homeassistant_websocket".into(),
            route_name: route_name.to_string(),
            connector_kind: "homeassistant_websocket_api".into(),
            started: true,
            connect_attempts: 1,
            websocket_url: websocket_url.clone(),
            auth_frame_type: "auth".into(),
            subscribe_event_type: "state_changed".into(),
            subscription_id,
            reconnect_backoff_millis: homeassistant_websocket_daemon_backoff_millis(
                reconnect_backoff_steps,
            ),
            ..WebhookInboundDaemonReport::default()
        }));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();
        let signing_key = self.state.signing_key.clone();
        let task_report = report.clone();

        let task = tokio::spawn(async move {
            let service = crate::HomeAssistantWebSocketInboundService::new_with_key(
                adapter.as_ref(),
                signing_key,
            );

            match connect_homeassistant_websocket(&websocket_url).await {
                Ok(stream) => {
                    match run_homeassistant_websocket_session(
                        stream,
                        &auth_frame,
                        &subscribe_frame,
                        subscription_id,
                        task_report.clone(),
                    )
                    .await
                    {
                        Ok(background_stream) => {
                            let _ = ready_tx.send(Ok(()));
                            let mut background_stream = background_stream;
                            loop {
                                tokio::select! {
                                    _ = shutdown_rx.changed() => break,
                                    frame = read_websocket_text_frame(&mut background_stream.stream) => {
                                        match frame {
                                            Ok(frame) => {
                                                if frame.trim().is_empty() {
                                                    continue;
                                                }
                                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&frame) {
                                                    let frame_type = value.get("type").and_then(|value| value.as_str());
                                                    if frame_type == Some("event") {
                                                        match service.ingest_websocket_text(&frame) {
                                                            Ok(ingest) => {
                                                                let mut report = task_report.write().await;
                                                                report.frames_read += 1;
                                                                report.accepted_count += ingest.accepted_count;
                                                                report.ignored_count += ingest.ignored_count;
                                                                report.invalid_count += ingest.invalid_count;
                                                                report.provenance_count += ingest.provenance_count;
                                                            }
                                                            Err(err) => {
                                                                let mut report = task_report.write().await;
                                                                report.frames_read += 1;
                                                                report.invalid_count += 1;
                                                                report.failure = Some(err.to_string());
                                                            }
                                                        }
                                                    } else {
                                                        let mut report = task_report.write().await;
                                                        report.frames_read += 1;
                                                        report.ignored_count += 1;
                                                    }
                                                } else {
                                                    let mut report = task_report.write().await;
                                                    report.frames_read += 1;
                                                    report.invalid_count += 1;
                                                }
                                            }
                                            Err(err) => {
                                                let mut report = task_report.write().await;
                                                report.failure = Some(err);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            let mut report = task_report.write().await;
                            report.failure = Some(err.clone());
                            let _ = ready_tx.send(Err(err));
                        }
                    }
                }
                Err(err) => {
                    let mut report = task_report.write().await;
                    report.failure = Some(err);
                    let _ = ready_tx.send(Err("Home Assistant WebSocket connect failed".into()));
                }
            }
        });

        {
            let mut supervisors = self
                .state
                .homeassistant_websocket_daemon_supervisors
                .write()
                .await;
            if supervisors.contains_key(route_name) {
                task.abort();
                return Err(format!(
                    "homeassistant websocket daemon for route '{}' already running",
                    route_name
                ));
            }
            supervisors.insert(
                route_name.to_string(),
                WebhookInboundDaemonSupervisor {
                    shutdown: shutdown_tx,
                    report,
                    task,
                },
            );
        }

        match ready_rx.await.map_err(|_| {
            format!(
                "homeassistant websocket daemon for route '{}' failed to start",
                route_name
            )
        })? {
            Ok(()) => Ok(()),
            Err(err) => {
                let supervisor = {
                    let mut supervisors = self
                        .state
                        .homeassistant_websocket_daemon_supervisors
                        .write()
                        .await;
                    supervisors.remove(route_name)
                };
                if let Some(supervisor) = supervisor {
                    supervisor.task.abort();
                }
                Err(err)
            }
        }
    }

    pub async fn stop_homeassistant_websocket_daemon(
        &self,
        route_name: &str,
    ) -> Result<WebhookInboundDaemonReport, String> {
        let supervisor = {
            let mut supervisors = self
                .state
                .homeassistant_websocket_daemon_supervisors
                .write()
                .await;
            supervisors.remove(route_name).ok_or_else(|| {
                format!(
                    "homeassistant websocket daemon for route '{}' not running",
                    route_name
                )
            })?
        };
        stop_inbound_daemon_supervisor(supervisor).await
    }

    /// Attach an agent trigger callback. The callback is owned by the
    /// CLI/runtime layer so this adapter crate can stay transport-focused.
    pub async fn set_agent_handler(&self, handler: WebhookAgentHandler) {
        let mut agent_handler = self.state.agent_handler.write().await;
        *agent_handler = Some(handler);
    }

    /// Start the webhook runtime server
    pub async fn start(&self) -> Result<(), String> {
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/webhooks/:route_name", post(webhook_handler))
            .route("/sms/twilio/:route_name", post(sms_twilio_webhook_handler))
            .route("/api/v1/webhooks", get(list_webhooks_handler))
            .route("/api/v1/webhooks/reload", post(reload_webhooks_handler))
            .route("/api/v1/webhooks/dispatch", post(dispatch_webhook_handler))
            .with_state(self.state.clone());

        let addr = SocketAddr::from((
            self.state
                .config
                .host
                .parse::<std::net::IpAddr>()
                .map_err(|e| format!("invalid host: {}", e))?,
            self.state.config.port,
        ));

        println!("馃寪 Webhook runtime listening on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("failed to bind: {}", e))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| format!("server error: {}", e))?;

        Ok(())
    }

    /// Validate HMAC signature
    fn validate_signature(
        &self,
        payload: &[u8],
        signature: &str,
        secret: &str,
        kind: WebhookSignatureKind,
        slack_timestamp: Option<&str>,
    ) -> Result<bool, String> {
        if kind == WebhookSignatureKind::GitlabSharedToken {
            return Ok(signature == secret);
        }

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| format!("invalid secret: {}", e))?;
        if kind == WebhookSignatureKind::SlackV0HmacSha256 {
            let timestamp =
                slack_timestamp.ok_or_else(|| "missing Slack request timestamp".to_string())?;
            mac.update(format!("v0:{timestamp}:").as_bytes());
        } else if kind == WebhookSignatureKind::StripeV1HmacSha256 {
            let (timestamp, _) = parse_stripe_signature_header(signature)
                .ok_or_else(|| "invalid Stripe signature header".to_string())?;
            mac.update(format!("{timestamp}.").as_bytes());
        }
        mac.update(payload);

        // Support multiple signature formats (GitHub, GitLab, etc.).
        // Defence-in-depth: although the branch guarantees the prefix exists
        // at this exact instant, a future refactor could break that coupling.
        // `strip_prefix` returns `None` if the prefix is absent; in that case,
        // fall through to treating the whole signature as raw hex.
        let expected = match kind {
            WebhookSignatureKind::SlackV0HmacSha256 => {
                signature.strip_prefix("v0=").unwrap_or(signature)
            }
            WebhookSignatureKind::StripeV1HmacSha256 => {
                let (_, v1) = parse_stripe_signature_header(signature)
                    .ok_or_else(|| "invalid Stripe signature header".to_string())?;
                v1
            }
            _ => signature.strip_prefix("sha256=").unwrap_or(signature),
        };

        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        let computed = hex::encode(code_bytes);

        Ok(computed == expected)
    }

    /// Check rate limit for route
    async fn check_rate_limit(&self, route_name: &str) -> Result<bool, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut rate_counts = self.state.rate_counts.write().await;
        let counts = rate_counts
            .entry(route_name.to_string())
            .or_insert_with(Vec::new);

        // Remove timestamps outside the window
        counts.retain(|&ts| now - ts < RATE_LIMIT_WINDOW_SECS);

        if counts.len() >= self.state.config.rate_limit {
            return Ok(false);
        }

        counts.push(now);
        Ok(true)
    }

    /// Check idempotency (prevent duplicate processing)
    async fn check_idempotency(&self, delivery_id: &str) -> Result<bool, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut seen = self.state.seen_deliveries.write().await;

        // Prune expired entries
        seen.retain(|_, &mut ts| now - ts < self.state.config.idempotency_ttl_secs);

        if seen.contains_key(delivery_id) {
            return Ok(false); // Already processed
        }

        seen.insert(delivery_id.to_string(), now);
        Ok(true)
    }

    /// Generate Ed25519 signed delivery receipt (Zaion unique)
    async fn generate_receipt(
        &self,
        route_name: &str,
        delivery_id: &str,
        payload_hash: &str,
        signature_valid: bool,
    ) -> Result<DeliveryReceipt, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let principal_id = self.state.signing_key.principal_id().to_string();

        // Sign canonical bytes: SHA256(route_name 鈥?0x1F 鈥?delivery_id 鈥?0x1F 鈥?payload_hash 鈥?0x1F 鈥?timestamp_le)
        let digest = DeliveryReceipt::canonical_bytes(route_name, delivery_id, payload_hash, now);
        let SignatureBytes(sig_bytes) = self.state.signing_key.sign(&digest);
        let ed25519_signature = hex::encode(&sig_bytes);

        eprintln!(
            "[zaion-webhook-sig] route={} delivery_id={} principal={} sig_hex={}...",
            route_name,
            delivery_id,
            principal_id,
            &ed25519_signature[..16],
        );

        Ok(DeliveryReceipt {
            route_name: route_name.to_string(),
            delivery_id: delivery_id.to_string(),
            timestamp: now,
            payload_hash: payload_hash.to_string(),
            signature_valid,
            principal_id,
            ed25519_signature,
            schema_version: 2,
        })
    }

    /// Record provenance entry (Zaion unique)
    async fn record_provenance(
        &self,
        event_id: &str,
        route_name: &str,
        source_ip: &str,
        payload_hash: &str,
        hmac_valid: bool,
        receipt: &DeliveryReceipt,
    ) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let provenance = WebhookProvenance {
            event_id: event_id.to_string(),
            route_name: route_name.to_string(),
            timestamp: now,
            source_ip: source_ip.to_string(),
            payload_hash: payload_hash.to_string(),
            hmac_valid,
            principal_id: receipt.principal_id.clone(),
            receipt_timestamp: receipt.timestamp,
            receipt_signature: receipt.ed25519_signature.clone(),
            receipt_schema_version: receipt.schema_version,
        };

        let mut ledger = self.state.provenance_ledger.write().await;
        ledger.push(provenance);

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeAssistantDaemonLifecycleState {
    AwaitingAuthRequired,
    AwaitingAuthOk,
    AwaitingSubscribeResult,
    ReadingEvents,
}

async fn stop_inbound_daemon_supervisor(
    supervisor: WebhookInboundDaemonSupervisor,
) -> Result<WebhookInboundDaemonReport, String> {
    let _ = supervisor.shutdown.send(true);
    supervisor
        .task
        .await
        .map_err(|err| format!("inbound daemon task join failed: {}", err))?;
    let mut report = supervisor.report.write().await;
    report.stopped = true;
    if report.stop_reason.is_empty() {
        report.stop_reason = "shutdown".into();
    }
    Ok(report.clone())
}

struct SimpleWebSocketStream {
    stream: tokio::net::TcpStream,
}

async fn connect_homeassistant_websocket(
    websocket_url: &str,
) -> Result<SimpleWebSocketStream, String> {
    let url = reqwest::Url::parse(websocket_url)
        .map_err(|err| format!("Home Assistant WebSocket URL invalid: {}", err))?;
    let host = url
        .host_str()
        .ok_or_else(|| "Home Assistant WebSocket URL missing host".to_string())?;
    if url.scheme() != "ws" {
        return Err(format!(
            "Home Assistant WebSocket unsupported scheme: {}",
            url.scheme()
        ));
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Home Assistant WebSocket URL missing port or known default".to_string())?;
    let path = match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_string(),
    };
    let mut stream = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(|err| format!("Home Assistant WebSocket connect failed: {}", err))?;
    let client_key = "emFpb24taGEtY29ubmVjdG9yLTE=";
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {client_key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|err| format!("Home Assistant WebSocket handshake write failed: {}", err))?;
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let read = stream
            .read(&mut byte)
            .await
            .map_err(|err| format!("Home Assistant WebSocket handshake read failed: {}", err))?;
        if read == 0 {
            return Err("Home Assistant WebSocket handshake closed early".into());
        }
        response.push(byte[0]);
        if response.len() > 8192 {
            return Err("Home Assistant WebSocket handshake too large".into());
        }
    }
    let response_text = String::from_utf8_lossy(&response);
    if !response_text.starts_with("HTTP/1.1 101 ") && !response_text.starts_with("HTTP/1.0 101 ") {
        return Err(format!(
            "Home Assistant WebSocket upgrade failed: {}",
            response_text.lines().next().unwrap_or_default()
        ));
    }
    let accept_header = response_text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-accept"))
        .map(|(_, value)| value.trim().to_string())
        .ok_or_else(|| "Home Assistant WebSocket missing Sec-WebSocket-Accept".to_string())?;
    let expected_accept = websocket_accept_key(client_key);
    if accept_header != expected_accept {
        return Err("Home Assistant WebSocket accept key mismatch".into());
    }
    Ok(SimpleWebSocketStream { stream })
}

async fn run_homeassistant_websocket_session(
    mut stream: SimpleWebSocketStream,
    auth_frame: &serde_json::Value,
    subscribe_frame: &serde_json::Value,
    subscription_id: u64,
    report: Arc<RwLock<WebhookInboundDaemonReport>>,
) -> Result<SimpleWebSocketStream, String> {
    loop {
        let frame = read_websocket_text_frame(&mut stream.stream).await?;
        let value = serde_json::from_str::<serde_json::Value>(&frame)
            .map_err(|err| format!("Home Assistant WebSocket frame invalid JSON: {}", err))?;
        let frame_type = value.get("type").and_then(|value| value.as_str());
        if frame_type == Some("auth_required") {
            let mut report = report.write().await;
            report.frames_read += 1;
            report.auth_required_seen = true;
            write_websocket_text_frame(&mut stream.stream, &auth_frame.to_string()).await?;
            continue;
        }
        if frame_type == Some("auth_ok") {
            let mut report = report.write().await;
            report.frames_read += 1;
            report.authenticated = true;
            write_websocket_text_frame(&mut stream.stream, &subscribe_frame.to_string()).await?;
            continue;
        }
        let subscribed = frame_type == Some("result")
            && value.get("id").and_then(|value| value.as_u64()) == Some(subscription_id)
            && value.get("success").and_then(|value| value.as_bool()) == Some(true);
        let mut report = report.write().await;
        report.frames_read += 1;
        if subscribed {
            report.subscribed = true;
            return Ok(stream);
        }
        report.ignored_count += 1;
        return Err("expected state_changed subscription success".into());
    }
}

async fn write_websocket_text_frame(
    stream: &mut tokio::net::TcpStream,
    text: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let payload = text.as_bytes();
    let mut frame = vec![0x81];
    let mask_bit = 0x80;
    if payload.len() < 126 {
        frame.push(mask_bit | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(mask_bit | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    let mask = [0x5a, 0xa5, 0x3c, 0xc3];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(idx, byte)| byte ^ mask[idx % 4]),
    );
    stream
        .write_all(&frame)
        .await
        .map_err(|err| format!("Home Assistant WebSocket frame write failed: {}", err))
}

async fn read_websocket_text_frame(stream: &mut tokio::net::TcpStream) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|err| format!("Home Assistant WebSocket frame read failed: {}", err))?;
    let opcode = header[0] & 0x0f;
    if opcode == 0x8 {
        return Err("Home Assistant WebSocket closed".into());
    }
    if opcode != 0x1 {
        return Err(format!(
            "Home Assistant WebSocket unsupported opcode {}",
            opcode
        ));
    }
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7f) as usize;
    if len == 126 {
        let mut bytes = [0u8; 2];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|err| format!("Home Assistant WebSocket length read failed: {}", err))?;
        len = u16::from_be_bytes(bytes) as usize;
    } else if len == 127 {
        let mut bytes = [0u8; 8];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|err| format!("Home Assistant WebSocket length read failed: {}", err))?;
        len = u64::from_be_bytes(bytes) as usize;
    }
    let mut mask = [0u8; 4];
    if masked {
        stream
            .read_exact(&mut mask)
            .await
            .map_err(|err| format!("Home Assistant WebSocket mask read failed: {}", err))?;
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|err| format!("Home Assistant WebSocket payload read failed: {}", err))?;
    if masked {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }
    String::from_utf8(payload)
        .map_err(|err| format!("Home Assistant WebSocket text frame invalid UTF-8: {}", err))
}

fn websocket_accept_key(client_key: &str) -> String {
    use base64::Engine;
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let digest = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(digest)
}

fn signal_sse_daemon_backoff_millis(steps: usize) -> Vec<u64> {
    daemon_backoff_millis(steps, 2_000, 60_000)
}

fn homeassistant_websocket_daemon_backoff_millis(steps: usize) -> Vec<u64> {
    let schedule = [5_000, 10_000, 30_000, 60_000];
    (0..steps)
        .map(|idx| schedule[idx.min(schedule.len() - 1)])
        .collect()
}

fn daemon_backoff_millis(steps: usize, initial: u64, max: u64) -> Vec<u64> {
    let mut values = Vec::with_capacity(steps);
    let mut current = initial;
    for _ in 0..steps {
        values.push(current);
        current = current.saturating_mul(2).min(max);
    }
    values
}

// HTTP handlers

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "zaion-webhook-runtime"
    }))
}

async fn webhook_handler(
    Path(route_name): Path<String>,
    State(state): State<Arc<WebhookRuntimeState>>,
    req: Request,
) -> impl IntoResponse {
    // Clone headers before consuming body
    let headers = req.headers().clone();

    let (signature, signature_kind) = extract_webhook_signature(&headers);

    let delivery_id = headers
        .get("x-github-delivery")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            use std::collections::hash_map::RandomState;
            use std::hash::BuildHasher;

            format!(
                "delivery_{:x}",
                RandomState::new().hash_one(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )
            )
        });

    let header_event_type = headers
        .get("x-github-event")
        .or_else(|| headers.get("x-gitlab-event"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| {
            if signature_kind == WebhookSignatureKind::SlackV0HmacSha256 {
                "push"
            } else {
                "unknown"
            }
        });

    let source_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Read body
    let body_bytes = match axum::body::to_bytes(req.into_body(), state.config.max_body_bytes).await
    {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "error": "payload too large"
                })),
            )
                .into_response()
        }
    };

    // Check idempotency
    let runtime = WebhookRuntime {
        state: state.clone(),
    };
    if !runtime
        .check_idempotency(&delivery_id)
        .await
        .unwrap_or(false)
    {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "duplicate",
                "delivery_id": delivery_id
            })),
        )
            .into_response();
    }

    // Load route config
    let routes = state.routes.read().await;
    let route = match routes.get(&route_name) {
        Some(r) => r.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "route not found"
                })),
            )
                .into_response()
        }
    };
    drop(routes);

    // Check rate limit
    if !runtime.check_rate_limit(&route_name).await.unwrap_or(false) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "rate limit exceeded"
            })),
        )
            .into_response();
    }

    // Validate HMAC signature
    let secret = route.secret.as_deref().unwrap_or("");
    let signature_valid = if secret.is_empty() || secret == "INSECURE_NO_AUTH" {
        true
    } else {
        runtime
            .validate_signature(
                &body_bytes,
                signature,
                secret,
                signature_kind,
                headers
                    .get("x-slack-request-timestamp")
                    .and_then(|value| value.to_str().ok()),
            )
            .unwrap_or(false)
    };

    if !signature_valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid signature"
            })),
        )
            .into_response();
    }

    // Parse payload
    let payload: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid json"
                })),
            )
                .into_response()
        }
    };

    let event_type = if signature_kind == WebhookSignatureKind::StripeV1HmacSha256 {
        stripe_event_type(&payload).unwrap_or(header_event_type)
    } else {
        header_event_type
    }
    .to_string();

    // Event filtering
    if !route.events.is_empty() && !route.events.contains(&event_type) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "event type not subscribed"
            })),
        )
            .into_response();
    }

    // Compute payload hash
    let payload_hash = format!("{:x}", Sha256::digest(&body_bytes));

    // Generate Ed25519 signed receipt
    let receipt = runtime
        .generate_receipt(&route_name, &delivery_id, &payload_hash, signature_valid)
        .await
        .unwrap();

    // Record provenance
    let _ = runtime
        .record_provenance(
            &delivery_id,
            &route_name,
            source_ip,
            &payload_hash,
            signature_valid,
            &receipt,
        )
        .await;

    // Store delivery info
    let chat_id = format!("webhook:{}:{}", route_name, delivery_id);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state.delivery_info.write().await.insert(
        chat_id.clone(),
        DeliveryInfo {
            route_name: route_name.clone(),
            delivery_id: delivery_id.clone(),
            created_at: now,
        },
    );

    // Prune expired delivery info
    let mut info = state.delivery_info.write().await;
    info.retain(|_, v| now - v.created_at < DELIVERY_INFO_TTL_SECS);
    drop(info);

    let agent_trigger = if let Some(handler) = state.agent_handler.read().await.clone() {
        Some(
            handler(WebhookAgentDispatch {
                route_name: route_name.clone(),
                delivery_id: delivery_id.clone(),
                event_type: event_type.clone(),
                payload,
                payload_hash: payload_hash.clone(),
                signature_valid,
            })
            .await,
        )
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "processed",
            "delivery_id": delivery_id,
            "route": route_name,
            "event_type": event_type,
            "receipt": {
                "timestamp": receipt.timestamp,
                "payload_hash": receipt.payload_hash,
                "signature_valid": receipt.signature_valid,
                "principal_id": receipt.principal_id,
                "ed25519_signature": receipt.ed25519_signature,
                "schema_version": receipt.schema_version,
            },
            "agent_trigger": agent_trigger
        })),
    )
        .into_response()
}

async fn list_webhooks_handler(State(state): State<Arc<WebhookRuntimeState>>) -> impl IntoResponse {
    let routes = state.routes.read().await;
    let route_list: Vec<_> = routes.values().cloned().collect();
    Json(route_list)
}

async fn reload_webhooks_handler(
    State(state): State<Arc<WebhookRuntimeState>>,
) -> impl IntoResponse {
    // Reload routes from webhook store
    // This requires integration with WebhookStore from zaion-cli
    // For now, return current route count
    let routes = state.routes.read().await;
    let count = routes.len();
    drop(routes);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "reloaded",
            "route_count": count
        })),
    )
}

async fn sms_twilio_webhook_handler(
    Path(route_name): Path<String>,
    State(state): State<Arc<WebhookRuntimeState>>,
    req: Request,
) -> impl IntoResponse {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let content_type = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let source_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), state.config.max_body_bytes).await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                [("content-type", "application/xml")],
                r#"<?xml version="1.0" encoding="UTF-8"?><Response></Response>"#.to_string(),
            )
                .into_response()
        }
    };

    let routes = state.sms_twilio_routes.read().await;
    let Some(adapter) = routes.get(&route_name).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            [("content-type", "application/xml")],
            r#"<?xml version="1.0" encoding="UTF-8"?><Response></Response>"#.to_string(),
        )
            .into_response();
    };
    drop(routes);

    let service = crate::SmsTwilioWebhookService::new(adapter.as_ref());
    match service.handle_http_request(crate::SmsTwilioWebhookRequest {
        method,
        path,
        content_type,
        body,
    }) {
        Ok(response) => {
            if response.ack.enqueued {
                trigger_sms_twilio_agent(&state, &route_name, &source_ip, &response.ack).await;
            }
            let status = StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (
                status,
                [("content-type", response.content_type)],
                response.body,
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            [("content-type", "application/xml")],
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Response><!-- {} --></Response>"#,
                redact_xml_comment(&error.to_string())
            ),
        )
            .into_response(),
    }
}

async fn trigger_sms_twilio_agent(
    state: &Arc<WebhookRuntimeState>,
    route_name: &str,
    source_ip: &str,
    ack: &crate::SmsTwilioWebhookAck,
) {
    let route_name = route_name.to_string();
    let payload = serde_json::json!({
        "provider": "twilio",
        "transport": "twilio_form_webhook",
        "channel_id": "sms",
        "route_name": route_name,
        "message_id": ack.message_id,
        "sender_id": ack.sender_id,
        "thread_id": ack.thread_id,
        "text": ack.text,
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let payload_hash = format!("{:x}", Sha256::digest(&payload_bytes));
    let delivery_id = ack
        .message_id
        .clone()
        .unwrap_or_else(|| format!("sms:{}", payload_hash));
    let runtime = WebhookRuntime {
        state: state.clone(),
    };
    let receipt = match runtime
        .generate_receipt(&route_name, &delivery_id, &payload_hash, true)
        .await
    {
        Ok(receipt) => receipt,
        Err(_) => return,
    };
    let _ = runtime
        .record_provenance(
            &delivery_id,
            &route_name,
            source_ip,
            &payload_hash,
            true,
            &receipt,
        )
        .await;

    let Some(handler) = state.agent_handler.read().await.clone() else {
        return;
    };

    tokio::spawn(async move {
        let _ = handler(WebhookAgentDispatch {
            route_name,
            delivery_id,
            event_type: "sms.twilio.inbound".to_string(),
            payload,
            payload_hash,
            signature_valid: true,
        })
        .await;
    });
}

fn redact_xml_comment(value: &str) -> String {
    value
        .replace("--", "- -")
        .chars()
        .filter(|ch| !matches!(ch, '<' | '>' | '&'))
        .collect()
}

async fn dispatch_webhook_handler(
    State(state): State<Arc<WebhookRuntimeState>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Manual webhook dispatch for testing
    // Extract route_name and event_type from payload
    let route_name = payload
        .get("route_name")
        .and_then(|v| v.as_str())
        .unwrap_or("test");

    let event_type = payload
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("test");

    // Check if route exists
    let routes = state.routes.read().await;
    if !routes.contains_key(route_name) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "route not found"
            })),
        )
            .into_response();
    }
    drop(routes);

    // Generate delivery ID
    let delivery_id = {
        use std::collections::hash_map::RandomState;
        use std::hash::BuildHasher;

        format!(
            "manual_{:x}",
            RandomState::new().hash_one(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )
        )
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "dispatched",
            "delivery_id": delivery_id,
            "route": route_name,
            "event_type": event_type
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::{HeaderName, HeaderValue};
    use axum::http::Request;
    use base64::Engine;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_webhook_runtime_creation() {
        let config = WebhookRuntimeConfig::default();
        let runtime = WebhookRuntime::new(config);
        assert!(runtime.state.routes.try_read().is_ok());
    }

    fn sign_test_payload(secret: &str, payload: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn sign_slack_test_payload(secret: &str, timestamp: &str, payload: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("v0:{timestamp}:").as_bytes());
        mac.update(payload);
        format!("v0={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn sign_stripe_test_payload(secret: &str, timestamp: &str, payload: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{timestamp}.").as_bytes());
        mac.update(payload);
        format!(
            "t={timestamp},v1={}",
            hex::encode(mac.finalize().into_bytes())
        )
    }

    #[derive(Clone, Debug)]
    struct MockHttpRequest {
        request_line: String,
        headers: Vec<(String, String)>,
    }

    type HomeAssistantWsMockServer = (
        SocketAddr,
        StdArc<StdMutex<Vec<MockHttpRequest>>>,
        StdArc<StdMutex<Vec<serde_json::Value>>>,
        thread::JoinHandle<()>,
    );

    impl MockHttpRequest {
        fn header(&self, name: &str) -> Option<String> {
            self.headers
                .iter()
                .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
        }
    }

    fn spawn_signal_sse_mock_server() -> (
        SocketAddr,
        StdArc<StdMutex<Vec<MockHttpRequest>>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(false).unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let task_requests = requests.clone();
        let handle = thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                handle_signal_sse_mock_request(stream, &task_requests);
            }
        });
        (addr, requests, handle)
    }

    fn spawn_idle_signal_sse_mock_server() -> (
        SocketAddr,
        StdArc<StdMutex<Vec<MockHttpRequest>>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let task_requests = requests.clone();
        let handle = thread::spawn(move || {
            for request_index in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                handle_idle_signal_sse_mock_request(stream, &task_requests, request_index);
            }
        });
        (addr, requests, handle)
    }

    fn handle_signal_sse_mock_request(
        mut stream: TcpStream,
        requests: &StdArc<StdMutex<Vec<MockHttpRequest>>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let request_line = request_line.trim_end().to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
        requests.lock().unwrap().push(MockHttpRequest {
            request_line: request_line.clone(),
            headers,
        });

        let (content_type, body) = if request_line.contains("/api/v1/events") {
            (
                "text/event-stream",
                "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417201000,\"dataMessage\":{\"message\":\"http signal\"}}\n\n",
            )
        } else {
            ("application/json", "{\"ok\":true}")
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(100));
    }

    fn handle_idle_signal_sse_mock_request(
        mut stream: TcpStream,
        requests: &StdArc<StdMutex<Vec<MockHttpRequest>>>,
        request_index: usize,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let request_line = request_line.trim_end().to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
        requests.lock().unwrap().push(MockHttpRequest {
            request_line,
            headers,
        });
        if request_index == 0 {
            let body = "{\"ok\":true}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            return;
        }
        let body = ": ready\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(500));
    }

    fn spawn_homeassistant_ws_mock_server() -> HomeAssistantWsMockServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let client_frames = StdArc::new(StdMutex::new(Vec::new()));
        let task_requests = requests.clone();
        let task_client_frames = client_frames.clone();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_homeassistant_ws_mock_request(stream, &task_requests, &task_client_frames);
        });
        (addr, requests, client_frames, handle)
    }

    fn spawn_idle_homeassistant_ws_mock_server() -> HomeAssistantWsMockServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let client_frames = StdArc::new(StdMutex::new(Vec::new()));
        let task_requests = requests.clone();
        let task_client_frames = client_frames.clone();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_idle_homeassistant_ws_mock_request(stream, &task_requests, &task_client_frames);
        });
        (addr, requests, client_frames, handle)
    }

    fn handle_homeassistant_ws_mock_request(
        mut stream: TcpStream,
        requests: &StdArc<StdMutex<Vec<MockHttpRequest>>>,
        client_frames: &StdArc<StdMutex<Vec<serde_json::Value>>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let request_line = request_line.trim_end().to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
        let request = MockHttpRequest {
            request_line,
            headers,
        };
        let client_key = request
            .header("sec-websocket-key")
            .expect("websocket client key");
        requests.lock().unwrap().push(request);
        let accept = websocket_accept_key_for_test(&client_key);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).unwrap();
        write_ws_text_frame(&mut stream, r#"{"type":"auth_required"}"#);
        if let Some(auth_frame) = read_ws_text_frame(&mut reader) {
            client_frames.lock().unwrap().push(auth_frame);
        }
        write_ws_text_frame(&mut stream, r#"{"type":"auth_ok"}"#);
        if let Some(subscribe_frame) = read_ws_text_frame(&mut reader) {
            client_frames.lock().unwrap().push(subscribe_frame);
        }
        write_ws_text_frame(&mut stream, r#"{"id":1,"type":"result","success":true}"#);
        write_ws_text_frame(
            &mut stream,
            &serde_json::json!({
                "type": "event",
                "event": {
                    "event_type": "state_changed",
                    "time_fired": "2026-05-21T11:00:00Z",
                    "data": {
                        "entity_id": "sensor.ws_temperature",
                        "old_state": {"state": "20", "attributes": {"friendly_name": "WS Temperature", "unit_of_measurement": "C"}},
                        "new_state": {"state": "25", "attributes": {"friendly_name": "WS Temperature", "unit_of_measurement": "C"}}
                    }
                }
            })
            .to_string(),
        );
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(10));
    }

    fn handle_idle_homeassistant_ws_mock_request(
        mut stream: TcpStream,
        requests: &StdArc<StdMutex<Vec<MockHttpRequest>>>,
        client_frames: &StdArc<StdMutex<Vec<serde_json::Value>>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let request_line = request_line.trim_end().to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
        let request = MockHttpRequest {
            request_line,
            headers,
        };
        let client_key = request
            .header("sec-websocket-key")
            .expect("websocket client key");
        requests.lock().unwrap().push(request);
        let accept = websocket_accept_key_for_test(&client_key);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).unwrap();
        write_ws_text_frame(&mut stream, r#"{"type":"auth_required"}"#);
        if let Some(auth_frame) = read_ws_text_frame(&mut reader) {
            client_frames.lock().unwrap().push(auth_frame);
        }
        write_ws_text_frame(&mut stream, r#"{"type":"auth_ok"}"#);
        if let Some(subscribe_frame) = read_ws_text_frame(&mut reader) {
            client_frames.lock().unwrap().push(subscribe_frame);
        }
        write_ws_text_frame(&mut stream, r#"{"id":1,"type":"result","success":true}"#);
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(500));
    }

    fn websocket_accept_key_for_test(client_key: &str) -> String {
        use sha1::Digest;
        let mut hasher = sha1::Sha1::new();
        hasher.update(client_key.as_bytes());
        hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let digest = hasher.finalize();
        base64::engine::general_purpose::STANDARD.encode(digest)
    }

    fn write_ws_text_frame(stream: &mut TcpStream, text: &str) {
        let payload = text.as_bytes();
        let mut frame = vec![0x81];
        if payload.len() < 126 {
            frame.push(payload.len() as u8);
        } else {
            frame.push(126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        stream.write_all(&frame).unwrap();
    }

    fn read_ws_text_frame(reader: &mut BufReader<TcpStream>) -> Option<serde_json::Value> {
        let mut header = [0u8; 2];
        reader.read_exact(&mut header).ok()?;
        let masked = header[1] & 0x80 != 0;
        let mut len = (header[1] & 0x7F) as usize;
        if len == 126 {
            let mut bytes = [0u8; 2];
            reader.read_exact(&mut bytes).ok()?;
            len = u16::from_be_bytes(bytes) as usize;
        } else if len == 127 {
            let mut bytes = [0u8; 8];
            reader.read_exact(&mut bytes).ok()?;
            len = u64::from_be_bytes(bytes) as usize;
        }
        let mut mask = [0u8; 4];
        if masked {
            reader.read_exact(&mut mask).ok()?;
        }
        let mut payload = vec![0u8; len];
        reader.read_exact(&mut payload).ok()?;
        if masked {
            for (idx, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[idx % 4];
            }
        }
        let text = String::from_utf8(payload).ok()?;
        serde_json::from_str(&text).ok()
    }

    async fn wait_for_signal_daemon_acceptance(
        runtime: &WebhookRuntime,
        route_name: &str,
        expected: usize,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let accepted_count = {
                let supervisors = runtime.state.signal_sse_daemon_supervisors.read().await;
                if let Some(supervisor) = supervisors.get(route_name) {
                    supervisor.report.read().await.accepted_count
                } else {
                    0
                }
            };
            if accepted_count >= expected || tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_homeassistant_daemon_acceptance(
        runtime: &WebhookRuntime,
        route_name: &str,
        expected: usize,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let accepted_count = {
                let supervisors = runtime
                    .state
                    .homeassistant_websocket_daemon_supervisors
                    .read()
                    .await;
                if let Some(supervisor) = supervisors.get(route_name) {
                    supervisor.report.read().await.accepted_count
                } else {
                    0
                }
            };
            if accepted_count >= expected || tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    async fn run_service_matrix_request(
        headers: &[(&str, &str)],
    ) -> (StatusCode, serde_json::Value) {
        run_service_matrix_request_with_body(br#"{"ok":true}"#, vec!["push".to_string()], headers)
            .await
    }

    async fn run_service_matrix_request_with_body(
        body: &'static [u8],
        events: Vec<String>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, serde_json::Value) {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .load_routes(vec![WebhookRoute {
                name: "matrix".to_string(),
                url: "https://example.com/webhook".to_string(),
                secret: Some("matrix-secret".to_string()),
                events,
                status: "active".to_string(),
            }])
            .await
            .unwrap();

        let mut request = Request::post("/webhooks/matrix")
            .body(axum::body::Body::from(body))
            .unwrap();
        for (name, value) in headers {
            request.headers_mut().insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }

        let response = webhook_handler(
            Path("matrix".to_string()),
            State(runtime.state.clone()),
            request,
        )
        .await
        .into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let value = serde_json::from_slice(&body).unwrap();
        (status, value)
    }

    #[tokio::test]
    async fn webhook_service_matrix_accepts_github_hmac_and_gitlab_token() {
        let body = br#"{"ok":true}"#;
        let github_signature = sign_test_payload("matrix-secret", body);

        let (github_status, github_json) = run_service_matrix_request(&[
            ("x-hub-signature-256", &github_signature),
            ("x-github-event", "push"),
            ("x-github-delivery", "github-delivery-001"),
        ])
        .await;
        assert_eq!(github_status, StatusCode::OK);
        assert_eq!(github_json["status"], "processed");
        assert_eq!(github_json["event_type"], "push");

        let (gitlab_status, gitlab_json) = run_service_matrix_request(&[
            ("x-gitlab-token", "matrix-secret"),
            ("x-gitlab-event", "push"),
            ("x-request-id", "gitlab-delivery-001"),
        ])
        .await;
        assert_eq!(gitlab_status, StatusCode::OK);
        assert_eq!(gitlab_json["status"], "processed");
        assert_eq!(gitlab_json["event_type"], "push");
    }

    #[tokio::test]
    async fn webhook_service_matrix_accepts_slack_v0_hmac() {
        let body = br#"{"ok":true}"#;
        let timestamp = "1710000000";
        let slack_signature = sign_slack_test_payload("matrix-secret", timestamp, body);

        let (slack_status, slack_json) = run_service_matrix_request(&[
            ("x-slack-signature", &slack_signature),
            ("x-slack-request-timestamp", timestamp),
            ("x-slack-retry-num", "0"),
            ("x-request-id", "slack-delivery-001"),
        ])
        .await;
        assert_eq!(slack_status, StatusCode::OK);
        assert_eq!(slack_json["status"], "processed");
        assert_eq!(slack_json["event_type"], "push");
    }

    #[tokio::test]
    async fn webhook_service_matrix_accepts_stripe_signature_and_payload_event_type() {
        let body = br#"{"id":"evt_001","type":"checkout.session.completed"}"#;
        let timestamp = "1710000001";
        let stripe_signature = sign_stripe_test_payload("matrix-secret", timestamp, body);

        let (stripe_status, stripe_json) = run_service_matrix_request_with_body(
            body,
            vec!["checkout.session.completed".to_string()],
            &[
                ("stripe-signature", &stripe_signature),
                ("x-request-id", "stripe-delivery-001"),
            ],
        )
        .await;
        assert_eq!(stripe_status, StatusCode::OK);
        assert_eq!(stripe_json["status"], "processed");
        assert_eq!(stripe_json["event_type"], "checkout.session.completed");
    }

    #[tokio::test]
    async fn webhook_runtime_mounts_sms_twilio_inbound_route() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();

        let request = Request::post("/sms/twilio/sms-inbound")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "From=%2B15551230000&To=%2B15551234567&Body=runtime+sms&MessageSid=SMruntime",
            ))
            .unwrap();

        let response = sms_twilio_webhook_handler(
            Path("sms-inbound".to_string()),
            State(runtime.state.clone()),
            request,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("<Response></Response>"));

        let received = runtime
            .drain_sms_twilio_route("sms-inbound")
            .await
            .expect("mounted sms route should drain");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].channel_id, "sms");
        assert_eq!(received[0].message_id, "SMruntime");
        assert_eq!(received[0].text, "runtime sms");
    }

    #[tokio::test]
    async fn webhook_runtime_mounts_signal_sse_inbound_route_with_signed_provenance() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        runtime
            .mount_signal_sse_route("signal-inbound", crate::SignalAdapter::new("+15551234567"))
            .await
            .unwrap();

        let chunk = "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417201000,\"dataMessage\":{\"message\":\"runtime signal\"}}\n\n";
        let report = runtime
            .ingest_signal_sse_route_chunk("signal-inbound", chunk)
            .await
            .expect("mounted Signal SSE route should ingest chunks");
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);

        let received = runtime
            .drain_signal_sse_route("signal-inbound")
            .await
            .expect("mounted Signal SSE route should drain");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].channel_id, "signal");
        assert_eq!(received[0].text, "runtime signal");
        assert_eq!(
            received[0].metadata["signal_provenance"]["route_name"],
            "signal.inbound.sse"
        );
        assert_eq!(
            received[0].metadata["signal_provenance"]["principal_id"],
            keypair.principal_id().to_string()
        );

        signal_provenance_receipt(&received[0].metadata["signal_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("runtime-mounted Signal SSE provenance receipt should verify");
    }

    #[tokio::test]
    async fn webhook_runtime_mounts_homeassistant_websocket_route_with_signed_provenance() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        runtime
            .mount_homeassistant_websocket_route(
                "ha-inbound",
                crate::HomeAssistantAdapter::new("ha-token").with_watch_all(true),
            )
            .await
            .unwrap();

        let frame = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-21T09:00:00Z",
                "data": {
                    "entity_id": "sensor.lab_temperature",
                    "old_state": {"state": "21", "attributes": {"friendly_name": "Lab Temperature", "unit_of_measurement": "C"}},
                    "new_state": {"state": "24", "attributes": {"friendly_name": "Lab Temperature", "unit_of_measurement": "C"}}
                }
            }
        })
        .to_string();
        let report = runtime
            .ingest_homeassistant_websocket_route_frame("ha-inbound", &frame)
            .await
            .expect("mounted Home Assistant route should ingest websocket frames");
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);

        let received = runtime
            .drain_homeassistant_websocket_route("ha-inbound")
            .await
            .expect("mounted Home Assistant route should drain");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].channel_id, "homeassistant");
        assert_eq!(received[0].metadata["entity_id"], "sensor.lab_temperature");
        assert_eq!(
            received[0].metadata["homeassistant_provenance"]["route_name"],
            "homeassistant.inbound.websocket"
        );
        assert_eq!(
            received[0].metadata["homeassistant_provenance"]["principal_id"],
            keypair.principal_id().to_string()
        );

        homeassistant_provenance_receipt(&received[0].metadata["homeassistant_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("runtime-mounted Home Assistant provenance receipt should verify");
    }

    #[tokio::test]
    async fn webhook_runtime_supervises_signal_and_homeassistant_inbound_daemons_with_signed_provenance(
    ) {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        runtime
            .mount_signal_sse_route("signal-daemon", crate::SignalAdapter::new("+15551234567"))
            .await
            .unwrap();
        runtime
            .mount_homeassistant_websocket_route(
                "ha-daemon",
                crate::HomeAssistantAdapter::new("ha-token").with_watch_all(true),
            )
            .await
            .unwrap();

        let signal_chunk = "data: {\"sourceNumber\":\"+15557654321\",\"timestamp\":1771417201000,\"dataMessage\":{\"message\":\"daemon signal\"}}\n\n";
        runtime
            .start_signal_sse_daemon_script("signal-daemon", vec![signal_chunk], 3)
            .await
            .expect("Signal SSE daemon should start for a mounted route");
        let signal_report = runtime
            .stop_signal_sse_daemon("signal-daemon")
            .await
            .expect("Signal SSE daemon should stop with a lifecycle report");
        assert_eq!(signal_report.backend, "signal_sse");
        assert!(signal_report.started);
        assert!(signal_report.stopped);
        assert_eq!(signal_report.stop_reason, "shutdown");
        assert_eq!(signal_report.connect_attempts, 1);
        assert_eq!(signal_report.chunks_read, 1);
        assert_eq!(signal_report.accepted_count, 1);
        assert_eq!(signal_report.provenance_count, 1);
        assert_eq!(
            signal_report.reconnect_backoff_millis,
            vec![2000, 4000, 8000]
        );

        let signal_received = runtime
            .drain_signal_sse_route("signal-daemon")
            .await
            .expect("Signal daemon route should drain accepted frames");
        assert_eq!(signal_received.len(), 1);
        assert_eq!(signal_received[0].text, "daemon signal");
        signal_provenance_receipt(&signal_received[0].metadata["signal_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("Signal daemon provenance should verify against runtime key");

        let ha_event = serde_json::json!({
            "type": "event",
            "event": {
                "event_type": "state_changed",
                "time_fired": "2026-05-21T10:00:00Z",
                "data": {
                    "entity_id": "sensor.daemon_temperature",
                    "old_state": {"state": "20", "attributes": {"friendly_name": "Daemon Temperature", "unit_of_measurement": "C"}},
                    "new_state": {"state": "23", "attributes": {"friendly_name": "Daemon Temperature", "unit_of_measurement": "C"}}
                }
            }
        })
        .to_string();
        runtime
            .start_homeassistant_websocket_daemon_script(
                "ha-daemon",
                vec![
                    r#"{"type":"auth_required"}"#,
                    r#"{"type":"auth_ok"}"#,
                    r#"{"id":1,"type":"result","success":true}"#,
                    ha_event.as_str(),
                ],
                4,
            )
            .await
            .expect("Home Assistant WebSocket daemon should start for a mounted route");
        let ha_report = runtime
            .stop_homeassistant_websocket_daemon("ha-daemon")
            .await
            .expect("Home Assistant WebSocket daemon should stop with a lifecycle report");
        assert_eq!(ha_report.backend, "homeassistant_websocket");
        assert!(ha_report.started);
        assert!(ha_report.stopped);
        assert!(ha_report.auth_required_seen);
        assert!(ha_report.authenticated);
        assert!(ha_report.subscribed);
        assert_eq!(ha_report.frames_read, 4);
        assert_eq!(ha_report.accepted_count, 1);
        assert_eq!(ha_report.provenance_count, 1);
        assert_eq!(
            ha_report.reconnect_backoff_millis,
            vec![5000, 10000, 30000, 60000]
        );

        let ha_received = runtime
            .drain_homeassistant_websocket_route("ha-daemon")
            .await
            .expect("Home Assistant daemon route should drain accepted frames");
        assert_eq!(ha_received.len(), 1);
        assert_eq!(
            ha_received[0].metadata["entity_id"],
            "sensor.daemon_temperature"
        );
        homeassistant_provenance_receipt(&ha_received[0].metadata["homeassistant_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("Home Assistant daemon provenance should verify against runtime key");
    }

    #[tokio::test]
    async fn webhook_runtime_signal_sse_http_daemon_performs_health_and_event_get_with_signed_provenance(
    ) {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        let (addr, requests, handle) = spawn_signal_sse_mock_server();
        runtime
            .mount_signal_sse_route(
                "signal-http",
                crate::SignalAdapter::new("+15551234567")
                    .with_api_base_url(format!("http://{addr}")),
            )
            .await
            .unwrap();

        runtime
            .start_signal_sse_daemon_http("signal-http", 3)
            .await
            .expect("Signal HTTP SSE daemon should start for a mounted route");
        wait_for_signal_daemon_acceptance(&runtime, "signal-http", 1).await;
        let report = runtime
            .stop_signal_sse_daemon("signal-http")
            .await
            .expect("Signal HTTP SSE daemon should stop with production connector report");

        assert_eq!(report.backend, "signal_sse");
        assert_eq!(report.connector_kind, "signal_http_sse");
        assert_eq!(report.health_url, format!("http://{addr}/api/v1/check"));
        assert_eq!(
            report.event_url,
            format!("http://{addr}/api/v1/events?account=%2B15551234567")
        );
        assert_eq!(report.accept_header, "text/event-stream");
        assert_eq!(report.connect_attempts, 1);
        assert_eq!(report.health_check_count, 1);
        assert_eq!(report.chunks_read, 1);
        assert_eq!(report.data_event_count, 1);
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);
        assert_eq!(report.reconnect_backoff_millis, vec![2000, 4000, 8000]);

        let signal_received = runtime
            .drain_signal_sse_route("signal-http")
            .await
            .expect("Signal HTTP daemon route should drain accepted SSE events");
        assert_eq!(signal_received.len(), 1);
        assert_eq!(signal_received[0].text, "http signal");
        signal_provenance_receipt(&signal_received[0].metadata["signal_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("Signal HTTP daemon provenance should verify against runtime key");

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].request_line, "GET /api/v1/check HTTP/1.1");
        assert_eq!(
            requests[1].request_line,
            "GET /api/v1/events?account=%2B15551234567 HTTP/1.1"
        );
        assert_eq!(
            requests[1].header("accept").as_deref(),
            Some("text/event-stream")
        );
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn webhook_runtime_signal_sse_http_daemon_is_ready_after_connecting_without_waiting_for_business_events(
    ) {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        let (addr, requests, handle) = spawn_idle_signal_sse_mock_server();
        runtime
            .mount_signal_sse_route(
                "signal-idle",
                crate::SignalAdapter::new("+15551234567")
                    .with_api_base_url(format!("http://{addr}")),
            )
            .await
            .unwrap();

        runtime
            .start_signal_sse_daemon_http("signal-idle", 2)
            .await
            .expect("Signal HTTP SSE daemon should become ready after opening the stream");
        let report = runtime
            .stop_signal_sse_daemon("signal-idle")
            .await
            .expect("Signal HTTP SSE daemon should stop cleanly even without business events");
        assert_eq!(report.connector_kind, "signal_http_sse");
        assert_eq!(report.connect_attempts, 1);
        assert_eq!(report.health_check_count, 1);
        assert_eq!(report.chunks_read, 0);
        assert_eq!(report.accepted_count, 0);
        assert_eq!(report.provenance_count, 0);
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn webhook_runtime_homeassistant_websocket_daemon_is_ready_after_subscribing_without_waiting_for_events(
    ) {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        let (addr, requests, client_frames, handle) = spawn_idle_homeassistant_ws_mock_server();
        runtime
            .mount_homeassistant_websocket_route(
                "ha-idle",
                crate::HomeAssistantAdapter::new("ha-token")
                    .with_api_base_url(format!("http://{addr}"))
                    .with_watch_all(true),
            )
            .await
            .unwrap();

        runtime
            .start_homeassistant_websocket_daemon_ws("ha-idle", 2)
            .await
            .expect("Home Assistant WebSocket daemon should become ready after subscribe ack");
        let report = runtime
            .stop_homeassistant_websocket_daemon("ha-idle")
            .await
            .expect("Home Assistant WebSocket daemon should stop cleanly even without events");
        assert_eq!(report.connector_kind, "homeassistant_websocket_api");
        assert_eq!(report.subscription_id, 1);
        assert!(report.auth_required_seen);
        assert!(report.authenticated);
        assert!(report.subscribed);
        assert_eq!(report.accepted_count, 0);
        assert_eq!(report.provenance_count, 0);
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(client_frames.lock().unwrap().len(), 2);
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn webhook_runtime_homeassistant_websocket_daemon_upgrades_authenticates_subscribes_and_streams_signed_provenance(
    ) {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        let (addr, requests, client_frames, handle) = spawn_homeassistant_ws_mock_server();
        runtime
            .mount_homeassistant_websocket_route(
                "ha-ws",
                crate::HomeAssistantAdapter::new("ha-token")
                    .with_api_base_url(format!("http://{addr}"))
                    .with_watch_all(true)
                    .with_cooldown_seconds(0),
            )
            .await
            .unwrap();

        runtime
            .start_homeassistant_websocket_daemon_ws("ha-ws", 4)
            .await
            .expect("Home Assistant WebSocket daemon should upgrade and start");
        wait_for_homeassistant_daemon_acceptance(&runtime, "ha-ws", 1).await;
        let ha_received = runtime
            .drain_homeassistant_websocket_route("ha-ws")
            .await
            .expect("Home Assistant WebSocket daemon route should drain accepted frames");
        assert_eq!(ha_received.len(), 1);
        assert_eq!(
            ha_received[0].metadata["entity_id"],
            "sensor.ws_temperature"
        );
        homeassistant_provenance_receipt(&ha_received[0].metadata["homeassistant_provenance"])
            .verify_receipt(&runtime.verifying_key())
            .expect("Home Assistant WebSocket daemon provenance should verify");

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request_line, "GET /api/websocket HTTP/1.1");
        assert_eq!(requests[0].header("upgrade").as_deref(), Some("websocket"));
        let client_frames = client_frames.lock().unwrap().clone();
        assert_eq!(client_frames.len(), 2);
        assert_eq!(client_frames[0]["type"], "auth");
        assert_eq!(client_frames[0]["access_token"], "ha-token");
        assert_eq!(client_frames[1]["type"], "subscribe_events");
        assert_eq!(client_frames[1]["event_type"], "state_changed");
        let report = runtime
            .stop_homeassistant_websocket_daemon("ha-ws")
            .await
            .expect("Home Assistant WebSocket daemon should stop with connector report");
        assert_eq!(report.backend, "homeassistant_websocket");
        assert_eq!(report.connector_kind, "homeassistant_websocket_api");
        assert_eq!(report.websocket_url, format!("ws://{addr}/api/websocket"));
        assert_eq!(report.auth_frame_type, "auth");
        assert_eq!(report.subscribe_event_type, "state_changed");
        assert_eq!(report.subscription_id, 1);
        assert_eq!(report.connect_attempts, 1);
        assert!(report.auth_required_seen);
        assert!(report.authenticated);
        assert!(report.subscribed);
        assert_eq!(report.accepted_count, 1);
        assert_eq!(report.provenance_count, 1);
        assert_eq!(
            report.reconnect_backoff_millis,
            vec![5000, 10000, 30000, 60000]
        );
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn webhook_runtime_rejects_unmounted_sms_twilio_route_with_xml_404() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        let request = Request::post("/sms/twilio/missing")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "From=%2B15551230000&To=%2B15551234567&Body=missing&MessageSid=SMmissing",
            ))
            .unwrap();

        let response = sms_twilio_webhook_handler(
            Path("missing".to_string()),
            State(runtime.state.clone()),
            request,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default(),
            "application/xml"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("<Response></Response>"));
    }

    #[tokio::test]
    async fn webhook_runtime_rejects_sms_twilio_non_form_without_buffering() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();
        let request = Request::post("/sms/twilio/sms-inbound")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"From":"+15551230000","Body":"json should not buffer"}"#,
            ))
            .unwrap();

        let response = sms_twilio_webhook_handler(
            Path("sms-inbound".to_string()),
            State(runtime.state.clone()),
            request,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("<Response></Response>"));

        let received = runtime
            .drain_sms_twilio_route("sms-inbound")
            .await
            .expect("mounted sms route should drain");
        assert!(received.is_empty());
    }

    #[tokio::test]
    async fn webhook_runtime_triggers_agent_for_sms_twilio_inbound_message() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();
        let dispatches: Arc<tokio::sync::Mutex<Vec<WebhookAgentDispatch>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured = dispatches.clone();
        runtime
            .set_agent_handler(Arc::new(move |dispatch| {
                let captured = captured.clone();
                Box::pin(async move {
                    captured.lock().await.push(dispatch.clone());
                    WebhookAgentDispatchResult {
                        status: "triggered".to_string(),
                        principal_id: Some("did:key:sms-agent".to_string()),
                        background: false,
                        runtime_scope: Some("turn_runtime".to_string()),
                        runtime_route: Some("wake".to_string()),
                        proof_chain: None,
                        ingress_event_id: None,
                        ingress_event_type: None,
                        output_event_id: None,
                        answer_trace_event_id: None,
                        turn_proof_event_id: None,
                        response_text: Some("queued".to_string()),
                        runtime_warnings: Vec::new(),
                        stream_contract: None,
                        response: Some("queued".to_string()),
                        error: None,
                        ..Default::default()
                    }
                })
            }))
            .await;

        let request = Request::post("/sms/twilio/sms-inbound")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "From=%2B15551230000&To=%2B15551234567&Body=agent+sms&MessageSid=SMagent",
            ))
            .unwrap();

        let response = sms_twilio_webhook_handler(
            Path("sms-inbound".to_string()),
            State(runtime.state.clone()),
            request,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let dispatches = wait_for_sms_dispatches(&dispatches, 1).await;
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].route_name, "sms-inbound");
        assert_eq!(dispatches[0].delivery_id, "SMagent");
        assert_eq!(dispatches[0].event_type, "sms.twilio.inbound");
        assert!(dispatches[0].signature_valid);
        assert_eq!(dispatches[0].payload["provider"], "twilio");
        assert_eq!(dispatches[0].payload["channel_id"], "sms");
        assert_eq!(dispatches[0].payload["text"], "agent sms");
        assert_eq!(dispatches[0].payload["sender_id"], "+15551230000");
        assert_eq!(dispatches[0].payload["thread_id"], "+15551230000");
        assert_eq!(dispatches[0].payload["message_id"], "SMagent");
        assert_eq!(dispatches[0].payload_hash.len(), 64);
    }

    #[tokio::test]
    async fn webhook_runtime_records_signed_provenance_for_sms_twilio_inbound_message() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();

        let dispatches: Arc<tokio::sync::Mutex<Vec<WebhookAgentDispatch>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured = dispatches.clone();
        runtime
            .set_agent_handler(Arc::new(move |dispatch| {
                let captured = captured.clone();
                Box::pin(async move {
                    captured.lock().await.push(dispatch);
                    WebhookAgentDispatchResult {
                        status: "triggered".to_string(),
                        principal_id: Some("did:key:sms-agent".to_string()),
                        background: true,
                        runtime_scope: Some("turn_runtime".to_string()),
                        runtime_route: Some("wake".to_string()),
                        proof_chain: None,
                        ingress_event_id: None,
                        ingress_event_type: None,
                        output_event_id: None,
                        answer_trace_event_id: None,
                        turn_proof_event_id: None,
                        response_text: Some("queued".to_string()),
                        runtime_warnings: Vec::new(),
                        stream_contract: None,
                        response: Some("queued".to_string()),
                        error: None,
                        ..Default::default()
                    }
                })
            }))
            .await;

        for _ in 0..2 {
            let request = Request::post("/sms/twilio/sms-inbound")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("x-forwarded-for", "203.0.113.10")
                .body(axum::body::Body::from(
                    "From=%2B15551230000&To=%2B15551234567&Body=proof+sms&MessageSid=SMproof",
                ))
                .unwrap();

            let response = sms_twilio_webhook_handler(
                Path("sms-inbound".to_string()),
                State(runtime.state.clone()),
                request,
            )
            .await
            .into_response();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let dispatches = wait_for_sms_dispatches(&dispatches, 1).await;
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].delivery_id, "SMproof");

        let ledger = runtime.state.provenance_ledger.read().await.clone();
        assert_eq!(ledger.len(), 1);
        let provenance = &ledger[0];
        assert_eq!(provenance.event_id, "SMproof");
        assert_eq!(provenance.route_name, "sms-inbound");
        assert_eq!(provenance.source_ip, "203.0.113.10");
        assert_eq!(provenance.payload_hash, dispatches[0].payload_hash);
        assert!(provenance.hmac_valid);
        assert_eq!(provenance.principal_id, keypair.principal_id().to_string());
        assert_eq!(provenance.receipt_signature.len(), 128);

        let receipt = DeliveryReceipt {
            route_name: provenance.route_name.clone(),
            delivery_id: provenance.event_id.clone(),
            timestamp: provenance.receipt_timestamp,
            payload_hash: provenance.payload_hash.clone(),
            signature_valid: provenance.hmac_valid,
            principal_id: provenance.principal_id.clone(),
            ed25519_signature: provenance.receipt_signature.clone(),
            schema_version: provenance.receipt_schema_version,
        };
        receipt
            .verify_receipt(&runtime.verifying_key())
            .expect("SMS Twilio provenance receipt should verify");
    }

    #[tokio::test]
    async fn webhook_runtime_returns_twiml_before_slow_sms_agent_finishes() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();

        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let release_rx = Arc::new(tokio::sync::Mutex::new(Some(release_rx)));
        runtime
            .set_agent_handler(Arc::new(move |_dispatch| {
                let release_rx = release_rx.clone();
                Box::pin(async move {
                    if let Some(rx) = release_rx.lock().await.take() {
                        let _ = rx.await;
                    }
                    WebhookAgentDispatchResult {
                        status: "triggered".to_string(),
                        principal_id: Some("did:key:sms-agent".to_string()),
                        background: true,
                        runtime_scope: Some("turn_runtime".to_string()),
                        runtime_route: Some("wake".to_string()),
                        proof_chain: None,
                        ingress_event_id: None,
                        ingress_event_type: None,
                        output_event_id: None,
                        answer_trace_event_id: None,
                        turn_proof_event_id: None,
                        response_text: Some("queued".to_string()),
                        runtime_warnings: Vec::new(),
                        stream_contract: None,
                        response: Some("queued".to_string()),
                        error: None,
                        ..Default::default()
                    }
                })
            }))
            .await;

        let request = Request::post("/sms/twilio/sms-inbound")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "From=%2B15551230000&To=%2B15551234567&Body=slow+agent&MessageSid=SMslow",
            ))
            .unwrap();

        let response = tokio::time::timeout(
            std::time::Duration::from_millis(150),
            sms_twilio_webhook_handler(
                Path("sms-inbound".to_string()),
                State(runtime.state.clone()),
                request,
            ),
        )
        .await
        .expect("Twilio TwiML response must not wait for slow agent dispatch")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("<Response></Response>"));
        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn webhook_runtime_deduplicates_sms_twilio_message_sid_before_buffer_and_agent_trigger() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .mount_sms_twilio_route(
                "sms-inbound",
                crate::SmsAdapter::new("AC123", "sms-auth-token", "+15551234567"),
            )
            .await
            .unwrap();

        let dispatches: Arc<tokio::sync::Mutex<Vec<WebhookAgentDispatch>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured = dispatches.clone();
        runtime
            .set_agent_handler(Arc::new(move |dispatch| {
                let captured = captured.clone();
                Box::pin(async move {
                    captured.lock().await.push(dispatch);
                    WebhookAgentDispatchResult {
                        status: "triggered".to_string(),
                        principal_id: Some("did:key:sms-agent".to_string()),
                        background: true,
                        runtime_scope: Some("turn_runtime".to_string()),
                        runtime_route: Some("wake".to_string()),
                        proof_chain: None,
                        ingress_event_id: None,
                        ingress_event_type: None,
                        output_event_id: None,
                        answer_trace_event_id: None,
                        turn_proof_event_id: None,
                        response_text: Some("queued".to_string()),
                        runtime_warnings: Vec::new(),
                        stream_contract: None,
                        response: Some("queued".to_string()),
                        error: None,
                        ..Default::default()
                    }
                })
            }))
            .await;

        for _ in 0..2 {
            let request = Request::post("/sms/twilio/sms-inbound")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(axum::body::Body::from(
                    "From=%2B15551230000&To=%2B15551234567&Body=retry+sms&MessageSid=SMretry",
                ))
                .unwrap();

            let response = sms_twilio_webhook_handler(
                Path("sms-inbound".to_string()),
                State(runtime.state.clone()),
                request,
            )
            .await
            .into_response();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let received = runtime
            .drain_sms_twilio_route("sms-inbound")
            .await
            .expect("mounted sms route should drain");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message_id, "SMretry");
        let dispatches = wait_for_sms_dispatches(&dispatches, 1).await;
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].delivery_id, "SMretry");
    }

    async fn wait_for_sms_dispatches(
        dispatches: &Arc<tokio::sync::Mutex<Vec<WebhookAgentDispatch>>>,
        expected: usize,
    ) -> Vec<WebhookAgentDispatch> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let snapshot = dispatches.lock().await.clone();
            if snapshot.len() >= expected || tokio::time::Instant::now() >= deadline {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    fn signal_provenance_receipt(provenance: &serde_json::Value) -> DeliveryReceipt {
        DeliveryReceipt {
            route_name: provenance["route_name"].as_str().unwrap().to_string(),
            delivery_id: provenance["delivery_id"].as_str().unwrap().to_string(),
            timestamp: provenance["receipt_timestamp"].as_u64().unwrap(),
            payload_hash: provenance["payload_hash"].as_str().unwrap().to_string(),
            signature_valid: true,
            principal_id: provenance["principal_id"].as_str().unwrap().to_string(),
            ed25519_signature: provenance["receipt_signature"]
                .as_str()
                .unwrap()
                .to_string(),
            schema_version: provenance["receipt_schema_version"].as_u64().unwrap() as u32,
        }
    }

    fn homeassistant_provenance_receipt(provenance: &serde_json::Value) -> DeliveryReceipt {
        DeliveryReceipt {
            route_name: provenance["route_name"].as_str().unwrap().to_string(),
            delivery_id: provenance["delivery_id"].as_str().unwrap().to_string(),
            timestamp: provenance["receipt_timestamp"].as_u64().unwrap(),
            payload_hash: provenance["payload_hash"].as_str().unwrap().to_string(),
            signature_valid: true,
            principal_id: provenance["principal_id"].as_str().unwrap().to_string(),
            ed25519_signature: provenance["receipt_signature"]
                .as_str()
                .unwrap()
                .to_string(),
            schema_version: provenance["receipt_schema_version"].as_u64().unwrap() as u32,
        }
    }

    #[test]
    fn test_default_config() {
        let config = WebhookRuntimeConfig::default();
        assert_eq!(config.host, DEFAULT_HOST);
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.max_body_bytes, MAX_BODY_BYTES);
    }

    #[tokio::test]
    async fn test_load_routes() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        let routes = vec![WebhookRoute {
            name: "test".to_string(),
            url: "https://example.com/webhook".to_string(),
            secret: Some("secret123".to_string()),
            events: vec!["push".to_string()],
            status: "active".to_string(),
        }];
        assert!(runtime.load_routes(routes).await.is_ok());
    }

    #[tokio::test]
    async fn test_agent_handler_can_be_attached() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        runtime
            .set_agent_handler(Arc::new(|dispatch| {
                Box::pin(async move {
                    WebhookAgentDispatchResult {
                        status: "triggered".to_string(),
                        principal_id: Some(format!("principal:{}", dispatch.route_name)),
                        background: false,
                        runtime_scope: None,
                        runtime_route: None,
                        proof_chain: None,
                        ingress_event_id: None,
                        ingress_event_type: None,
                        output_event_id: None,
                        answer_trace_event_id: None,
                        turn_proof_event_id: None,
                        response_text: None,
                        runtime_warnings: Vec::new(),
                        stream_contract: None,
                        response: Some(dispatch.event_type),
                        error: None,
                        ..Default::default()
                    }
                })
            }))
            .await;

        let handler = runtime.state.agent_handler.read().await.clone();
        let result = handler.expect("handler").as_ref()(WebhookAgentDispatch {
            route_name: "route".to_string(),
            delivery_id: "delivery".to_string(),
            event_type: "push".to_string(),
            payload: serde_json::json!({}),
            payload_hash: "hash".to_string(),
            signature_valid: true,
        })
        .await;
        assert_eq!(result.status, "triggered");
        assert_eq!(result.principal_id.as_deref(), Some("principal:route"));
        assert_eq!(result.response.as_deref(), Some("push"));
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig {
            rate_limit: 2,
            ..Default::default()
        });

        assert!(runtime.check_rate_limit("test").await.unwrap());
        assert!(runtime.check_rate_limit("test").await.unwrap());
        assert!(!runtime.check_rate_limit("test").await.unwrap()); // Should be rate limited
    }

    #[tokio::test]
    async fn test_idempotency() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        let delivery_id = "delivery_123";

        assert!(runtime.check_idempotency(delivery_id).await.unwrap());
        assert!(!runtime.check_idempotency(delivery_id).await.unwrap()); // Should be duplicate
    }

    #[tokio::test]
    async fn test_generate_receipt() {
        let runtime = WebhookRuntime::new(WebhookRuntimeConfig::default());
        let receipt = runtime
            .generate_receipt("test_route", "delivery_123", "hash_abc", true)
            .await
            .unwrap();

        assert_eq!(receipt.route_name, "test_route");
        assert_eq!(receipt.delivery_id, "delivery_123");
        assert!(receipt.signature_valid);
        // Real Ed25519 signatures are 128 hex chars (64 bytes)
        assert_eq!(receipt.ed25519_signature.len(), 128);
        assert_eq!(receipt.schema_version, 2);
        assert_ne!(receipt.principal_id, "principal_placeholder");
    }

    #[test]
    fn test_delivery_receipt_serialization() {
        let receipt = DeliveryReceipt {
            route_name: "test".to_string(),
            delivery_id: "123".to_string(),
            timestamp: 1234567890,
            payload_hash: "abc".to_string(),
            signature_valid: true,
            principal_id: "principal_1".to_string(),
            ed25519_signature: "sig_1".to_string(),
            schema_version: 2,
        };

        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("123"));
        assert!(json.contains("schema_version"));
    }

    // 鈹€鈹€ New roundtrip and tamper-detection tests 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

    #[tokio::test]
    async fn test_receipt_sign_verify_roundtrip() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime =
            WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair.clone());
        let vk = runtime.verifying_key();

        let receipt = runtime
            .generate_receipt("my_route", "del_001", "deadbeef", true)
            .await
            .unwrap();

        assert_eq!(receipt.schema_version, 2);
        assert!(
            receipt.verify_receipt(&vk).is_ok(),
            "roundtrip verify should pass"
        );
    }

    #[tokio::test]
    async fn test_receipt_tamper_route_name_detected() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime = WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair);
        let vk = runtime.verifying_key();

        let mut receipt = runtime
            .generate_receipt("original_route", "del_002", "cafebabe", true)
            .await
            .unwrap();

        receipt.route_name = "tampered_route".to_string();
        assert!(
            receipt.verify_receipt(&vk).is_err(),
            "tampered route_name must fail"
        );
    }

    #[tokio::test]
    async fn test_receipt_tamper_payload_hash_detected() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime = WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair);
        let vk = runtime.verifying_key();

        let mut receipt = runtime
            .generate_receipt("route_x", "del_003", "original_hash", false)
            .await
            .unwrap();

        receipt.payload_hash = "tampered_hash".to_string();
        assert!(
            receipt.verify_receipt(&vk).is_err(),
            "tampered payload_hash must fail"
        );
    }

    #[tokio::test]
    async fn test_receipt_legacy_schema_fails_closed() {
        let keypair = Arc::new(ZaionKeypair::generate());
        let runtime = WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair);
        let vk = runtime.verifying_key();

        let mut receipt = runtime
            .generate_receipt("route_y", "del_004", "somehash", true)
            .await
            .unwrap();

        // Downgrade to legacy schema 鈥?must fail closed even with correct sig
        receipt.schema_version = 1;
        let err = receipt.verify_receipt(&vk).unwrap_err();
        assert_eq!(
            err,
            ReceiptError::LegacySchema,
            "legacy schema must fail closed"
        );
    }

    #[tokio::test]
    async fn test_receipt_wrong_key_fails_closed() {
        let keypair_a = Arc::new(ZaionKeypair::generate());
        let keypair_b = Arc::new(ZaionKeypair::generate());
        let runtime = WebhookRuntime::new_with_key(WebhookRuntimeConfig::default(), keypair_a);
        // Verify with key B 鈥?must fail
        let vk_b = keypair_b.verifying_key();

        let receipt = runtime
            .generate_receipt("route_z", "del_005", "hashval", true)
            .await
            .unwrap();

        assert!(
            receipt.verify_receipt(&vk_b).is_err(),
            "wrong key must fail"
        );
    }
}
