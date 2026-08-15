//! Shared HTTP route dispatcher for the Zaion gateway and daemon.
//!
//! `gateway_route` is the single entry point used by both the standalone
//! `zaion gateway serve` loop and the daemon's in-process HTTP handler.
//! Route logic is intentionally pure — input is `(method, path, body)` plus
//! an ACP run store, and the output is a `(status, body)` pair. Keeping the
//! dispatcher free of I/O allows it to be unit-tested in isolation.

use crate::commands::data_dir;
use crate::commands::operation_backlog::{
    append_shared_operation_backlog, shared_operation_backlog,
    wait_for_shared_operation_backlog_after,
};
use crate::commands::process::{
    cmd_wake_with_request, structured_wake_request, StreamCallback, StreamEvent, WakeRequest,
};
use crate::commands::webhook::{dispatch_runtime_webhooks, RuntimeWebhookDelivery};
use crate::config::{WebhookStore, ZaionConfig};
use sha2::{Digest, Sha256};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use zaion_runtime::{
    operation_stream::{OperationEvent, OperationStreamBacklog},
    TurnProof,
};
use zaion_types::envelope::{ingest as ingest_envelope, is_unsafe_principal, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

use super::console::web_console_html;
use super::gateway_contract::gateway_health_payload;
use super::WEBHOOK_EVENT_RECENT_LIMIT;

/// Public entry point — resolves the current `WebhookStore` from disk then
/// delegates to the pure router.
pub fn gateway_route(
    method: &str,
    path: &str,
    body: &str,
    acp: &zaion_a2a::AcpRunStore,
) -> (&'static str, String) {
    let webhook_store = WebhookStore::load();
    gateway_route_with_webhooks(method, path, body, acp, &webhook_store)
}

/// Strangler adapter (store-capturing variant): expose gateway_route as an axum
/// handler without axum State, so the unified server can use it as a fallback
/// service on a Router<()> (axum 0.7 only implements Service for Router<()>).
#[allow(dead_code)]
pub async fn gateway_route_axum_with_store(
    acp: zaion_a2a::AcpRunStore,
    req: axum::extract::Request,
) -> axum::response::Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let bytes = match axum::body::to_bytes(req.into_body(), 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let mut res = axum::response::Response::new(axum::body::Body::from(
                "{\"error\":\"body too large\"}",
            ));
            *res.status_mut() = axum::http::StatusCode::PAYLOAD_TOO_LARGE;
            return res;
        }
    };
    let body = String::from_utf8_lossy(&bytes).to_string();
    let (status, response_body) = gateway_route(&method, &path, &body, &acp);
    let mut res = axum::response::Response::new(axum::body::Body::from(response_body));
    *res.status_mut() = parse_gateway_status(status);
    res
}

/// Strangler adapter: expose the existing gateway_route dispatcher as an axum
/// handler so the unified GatewayServer can serve the CLI routes (M2).
/// Wired by the serve-unified command (upcoming Strangler step).
#[allow(dead_code)]
pub async fn gateway_route_axum(
    axum::extract::State(acp): axum::extract::State<zaion_a2a::AcpRunStore>,
    req: axum::extract::Request,
) -> axum::response::Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let bytes = match axum::body::to_bytes(req.into_body(), 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let mut res = axum::response::Response::new(axum::body::Body::from(
                "{\"error\":\"body too large\"}",
            ));
            *res.status_mut() = axum::http::StatusCode::PAYLOAD_TOO_LARGE;
            return res;
        }
    };
    let body = String::from_utf8_lossy(&bytes).to_string();
    let (status, response_body) = gateway_route(&method, &path, &body, &acp);
    let mut res = axum::response::Response::new(axum::body::Body::from(response_body));
    *res.status_mut() = parse_gateway_status(status);
    res
}

/// Parse a "200 OK" style status string into an axum StatusCode.
#[allow(dead_code)]
fn parse_gateway_status(status: &str) -> axum::http::StatusCode {
    status
        .split_whitespace()
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| axum::http::StatusCode::from_u16(code).ok())
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

/// Pure router — accepts an explicit `WebhookStore` so tests can inject
/// fixtures without touching the filesystem.
pub(super) fn gateway_route_with_webhooks(
    method: &str,
    path: &str,
    body: &str,
    acp: &zaion_a2a::AcpRunStore,
    webhook_store: &WebhookStore,
) -> (&'static str, String) {
    fn json(v: serde_json::Value) -> String {
        serde_json::to_string(&v).unwrap_or_else(|_| "{}".into())
    }
    let (route_path, query) = path.split_once('?').unwrap_or((path, ""));

    match (method, route_path) {
        ("OPTIONS", _) => ("204 No Content", String::new()),
        // Health
        ("GET", "/health") => ("200 OK", json(gateway_health_payload())),
        // Process list
        ("GET", "/api/v1/processes") => {
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let procs = store.list_all().unwrap_or_default();
            let arr: Vec<_> = procs
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "principal_id": p.principal_id,
                        "state": format!("{:?}", p.state),
                        "workspace": p.workspace_id,
                    })
                })
                .collect();
            ("200 OK", json(serde_json::json!({"processes": arr})))
        }
        // ACP: create run
        ("POST", "/v1/runs") => {
            let req: Result<zaion_a2a::CreateRunRequest, _> = serde_json::from_str(body);
            match req {
                Err(e) => (
                    "400 Bad Request",
                    json(serde_json::json!({"error": e.to_string()})),
                ),
                Ok(r) => {
                    let submitter = match r.submitter_principal.as_deref().map(str::trim) {
                        Some(value) if !value.is_empty() && !is_unsafe_principal(value) => value,
                        _ => {
                            return (
                                "401 Unauthorized",
                                json(serde_json::json!({
                                    "error": "POST /v1/runs requires a non-anonymous submitter_principal",
                                    "required_ingress": "CanonicalEnvelope",
                                })),
                            );
                        }
                    };
                    let process_store = zaion_core::process::ProcessStore::new(data_dir());
                    let (_process, _keypair) = match process_store.load(submitter) {
                        Ok(loaded) => loaded,
                        Err(error) => {
                            return (
                                "401 Unauthorized",
                                json(serde_json::json!({
                                    "error": format!("submitter identity unavailable: {}", error),
                                    "required_identity": "onboarded long-lived principal",
                                    "required_ingress": "CanonicalEnvelope",
                                })),
                            );
                        }
                    };
                    let idempotency_key = r
                        .idempotency_key
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let idempotency_fingerprint =
                        idempotency_key.map(|_| run_idempotency_fingerprint(&r.task, submitter));
                    if let (Some(key), Some(fingerprint)) =
                        (idempotency_key, idempotency_fingerprint.as_deref())
                    {
                        if let Ok(existing) = acp.get_by_idempotency_key(key) {
                            if existing.idempotency_fingerprint.as_deref() != Some(fingerprint) {
                                return (
                                    "409 Conflict",
                                    json(serde_json::json!({
                                        "error": "Idempotency-Key reuse conflicts with a different signed ACP run request",
                                        "idempotency_key": key,
                                        "idempotency_reused": false,
                                        "existing_run_id": existing.run_id,
                                    })),
                                );
                            }
                            let mut value = serde_json::to_value(&existing).unwrap_or_default();
                            if let serde_json::Value::Object(ref mut object) = value {
                                object.insert(
                                    "runtime_route".to_string(),
                                    serde_json::Value::String("wake".to_string()),
                                );
                                object.insert(
                                    "idempotency_reused".to_string(),
                                    serde_json::Value::Bool(true),
                                );
                            }
                            return ("200 OK", json(value));
                        }
                    }
                    let create_result = match (idempotency_key, idempotency_fingerprint.as_deref())
                    {
                        (Some(key), Some(fingerprint)) => {
                            acp.create_idempotent(&r.task, submitter, key, fingerprint)
                        }
                        _ => acp.create(&r.task, submitter),
                    };
                    match create_result {
                        Ok(run) => {
                            // The API route now dispatches into wake; ACP status mirrors that runtime turn.
                            let _ = acp.update_status(
                                &run.run_id,
                                zaion_a2a::RunStatus::Running,
                                None,
                                None,
                            );
                            let message_id = format!("acp-{}", uuid::Uuid::new_v4());
                            let envelope = match CanonicalEnvelope::new(
                                "api",
                                PrincipalId(submitter.to_string()),
                                ChannelId("api".to_string()),
                                ThreadId(run.run_id.clone()),
                                message_id,
                                r.task.clone(),
                                None,
                            ) {
                                Ok(envelope) => envelope,
                                Err(error) => {
                                    let message = format!("canonical envelope rejected: {}", error);
                                    let _ = acp.update_status(
                                        &run.run_id,
                                        zaion_a2a::RunStatus::Failed,
                                        None,
                                        Some(&message),
                                    );
                                    return (
                                        "400 Bad Request",
                                        json(serde_json::json!({
                                            "error": message,
                                            "required_ingress": "CanonicalEnvelope",
                                            "run_id": run.run_id,
                                        })),
                                    );
                                }
                            };
                            let envelope = match ingest_envelope(&envelope) {
                                Ok(envelope) => envelope,
                                Err(error) => {
                                    let message = format!("canonical envelope rejected: {}", error);
                                    let _ = acp.update_status(
                                        &run.run_id,
                                        zaion_a2a::RunStatus::Failed,
                                        None,
                                        Some(&message),
                                    );
                                    return (
                                        "400 Bad Request",
                                        json(serde_json::json!({
                                            "error": message,
                                            "required_ingress": "CanonicalEnvelope",
                                            "run_id": run.run_id,
                                        })),
                                    );
                                }
                            };
                            let cfg = ZaionConfig::load();
                            let request = api_run_wake_request(
                                submitter.to_string(),
                                envelope.clone(),
                                cfg.provider.clone(),
                                cfg.model.clone(),
                            );

                            let (tx, rx) = std::sync::mpsc::channel();
                            let callback = StreamCallback::new(tx);
                            let runtime_result = cmd_wake_with_request(request, Some(callback));
                            let transcript = collect_runtime_stream(rx);
                            let operation_events =
                                append_shared_operation_backlog(&transcript.operation_events);
                            let ledger = zaion_ledger::EventLedger::new(
                                process_store.ledger_path(submitter),
                            );
                            let runtime_error = runtime_result
                                .as_ref()
                                .err()
                                .map(|error| error.to_string())
                                .or_else(|| transcript.errors.first().cloned());
                            if let Some(error) = runtime_error {
                                let _ = acp.update_status(
                                    &run.run_id,
                                    zaion_a2a::RunStatus::Failed,
                                    None,
                                    Some(&error),
                                );
                                let mut value =
                                    serde_json::to_value(acp.get(&run.run_id).unwrap_or(run))
                                        .unwrap_or_default();
                                if let serde_json::Value::Object(ref mut object) = value {
                                    object.insert(
                                        "runtime_route".to_string(),
                                        serde_json::Value::String("wake".to_string()),
                                    );
                                    object.insert(
                                        "runtime_error".to_string(),
                                        serde_json::Value::String(error),
                                    );
                                    object.insert(
                                        "runtime_warnings".to_string(),
                                        serde_json::Value::Array(
                                            transcript
                                                .warnings
                                                .into_iter()
                                                .map(serde_json::Value::String)
                                                .collect(),
                                        ),
                                    );
                                    object.insert(
                                        "stream_contract".to_string(),
                                        transcript_stream_contract_value(&operation_events),
                                    );
                                    object.insert(
                                        "ingress".to_string(),
                                        envelope.to_channel_received_payload(),
                                    );
                                    object.insert(
                                        "idempotency_reused".to_string(),
                                        serde_json::Value::Bool(false),
                                    );
                                }
                                return ("500 Internal Server Error", json(value));
                            }
                            let Some(proof) = runtime_proof_for_api_run(&ledger, &run.run_id)
                            else {
                                let error = "wake runtime completed without API turn proof";
                                let _ = acp.update_status(
                                    &run.run_id,
                                    zaion_a2a::RunStatus::Failed,
                                    None,
                                    Some(error),
                                );
                                return (
                                    "500 Internal Server Error",
                                    json(serde_json::json!({
                                        "error": error,
                                        "run_id": run.run_id,
                                        "required_ledger_chain": "channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof",
                                        "stream_contract": transcript_stream_contract_value(&operation_events),
                                    })),
                                );
                            };
                            let _ = acp.update_status(
                                &run.run_id,
                                zaion_a2a::RunStatus::Completed,
                                Some(&transcript.response_text),
                                None,
                            );
                            let run = acp.get(&run.run_id).unwrap_or(run);
                            let ingress_event_id =
                                zaion_types::event::EventId(proof.ingress_event_id.clone());
                            let mut value = serde_json::to_value(&run).unwrap_or_default();
                            if let serde_json::Value::Object(ref mut object) = value {
                                object.insert(
                                    "ingress".to_string(),
                                    envelope.to_channel_received_payload(),
                                );
                                object.insert(
                                    "ingress_event_id".to_string(),
                                    serde_json::Value::String(ingress_event_id.0),
                                );
                                object.insert(
                                    "ingress_event_type".to_string(),
                                    serde_json::Value::String("channel.received".to_string()),
                                );
                                object.insert(
                                    "output_event_id".to_string(),
                                    serde_json::Value::String(proof.output_event_id),
                                );
                                object.insert(
                                    "answer_trace_event_id".to_string(),
                                    serde_json::Value::String(proof.answer_trace_event_id),
                                );
                                object.insert(
                                    "turn_proof_event_id".to_string(),
                                    serde_json::Value::String(proof.turn_proof_event_id.clone()),
                                );
                                object.insert(
                                    "tool_receipt_ids".to_string(),
                                    serde_json::json!(proof.tool_receipt_ids),
                                );
                                object.insert(
                                    "tool_receipt_count".to_string(),
                                    serde_json::json!(proof.tool_receipt_count),
                                );
                                object.insert(
                                    "tool_result_storage_receipts".to_string(),
                                    serde_json::json!(proof.tool_result_storage_receipts),
                                );
                                object.insert(
                                    "tool_result_storage_receipt_count".to_string(),
                                    serde_json::json!(proof.tool_result_storage_receipt_count),
                                );
                                object.insert(
                                    "tool_receipt_proof_join_event_id".to_string(),
                                    proof
                                        .tool_receipt_proof_join_event_id
                                        .map(serde_json::Value::String)
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                object.insert(
                                    "tool_receipt_proof_join".to_string(),
                                    proof
                                        .tool_receipt_proof_join
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                object.insert(
                                    "tool_receipt_join_found".to_string(),
                                    serde_json::Value::Bool(proof.tool_receipt_join_found),
                                );
                                object.insert(
                                    "tool_receipt_proof_hash_verified".to_string(),
                                    serde_json::Value::Bool(proof.tool_receipt_proof_hash_verified),
                                );
                                object.insert(
                                    "runtime_route".to_string(),
                                    serde_json::Value::String("wake".to_string()),
                                );
                                object.insert(
                                    "response_text".to_string(),
                                    serde_json::Value::String(transcript.response_text),
                                );
                                object.insert(
                                    "runtime_warnings".to_string(),
                                    serde_json::Value::Array(
                                        transcript
                                            .warnings
                                            .into_iter()
                                            .map(serde_json::Value::String)
                                            .collect(),
                                    ),
                                );
                                object.insert(
                                    "stream_contract".to_string(),
                                    transcript_stream_contract_value(&operation_events),
                                );
                                object.insert(
                                    "idempotency_reused".to_string(),
                                    serde_json::Value::Bool(false),
                                );
                            }
                            ("201 Created", json(value))
                        }
                        Err(e) => (
                            "500 Internal Server Error",
                            json(serde_json::json!({"error": e.to_string()})),
                        ),
                    }
                }
            }
        }
        // ACP: get run
        ("GET", p) if p.starts_with("/v1/runs/") && !p.ends_with("/stream") => {
            let run_id = p.trim_start_matches("/v1/runs/");
            match acp.get(run_id) {
                Ok(run) => (
                    "200 OK",
                    json(serde_json::to_value(&run).unwrap_or_default()),
                ),
                Err(_) => (
                    "404 Not Found",
                    json(serde_json::json!({"error": "run not found"})),
                ),
            }
        }
        // ACP: SSE stream (simplified — returns current state as one event).
        ("GET", p) if p.starts_with("/v1/runs/") && p.ends_with("/stream") => {
            let run_id = p
                .trim_start_matches("/v1/runs/")
                .trim_end_matches("/stream");
            match acp.get(run_id) {
                Ok(run) => (
                    "200 OK",
                    api_run_stream_live_sse_with_backlog(
                        &run,
                        query_param(query, "after").as_deref(),
                        &shared_operation_backlog(),
                    ),
                ),
                Err(_) => (
                    "404 Not Found",
                    sse_event("run.error", &serde_json::json!({"error": "not found"})),
                ),
            }
        }
        // ACP: list runs
        ("GET", "/v1/runs") => match acp.list(50) {
            Ok(runs) => ("200 OK", json(serde_json::json!({"runs": runs}))),
            Err(e) => (
                "500 Internal Server Error",
                json(serde_json::json!({"error": e.to_string()})),
            ),
        },
        // ACP: cancel run
        ("DELETE", p) if p.starts_with("/v1/runs/") => {
            let run_id = p.trim_start_matches("/v1/runs/");
            match acp.cancel(run_id) {
                Ok(_) => ("200 OK", json(serde_json::json!({"cancelled": run_id}))),
                Err(e) => (
                    "404 Not Found",
                    json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        // Web Console UI
        ("GET", "/ui") | ("GET", "/ui/") => ("200 OK", web_console_html()),
        // Operation event live transport: backlog-backed long-poll SSE.
        ("GET", "/api/v1/operations/stream") => {
            let resume_after = query_param(query, "after");
            (
                "200 OK",
                operation_live_stream_sse_after_wait(
                    resume_after.as_deref(),
                    operation_live_stream_wait_timeout(),
                ),
            )
        }
        // Operation event live transport: WebSocket upgrade contract fallback.
        ("GET", "/api/v1/operations/ws") => {
            let resume_after = query_param(query, "after");
            (
                "426 Upgrade Required",
                operation_live_websocket_upgrade_required_body(resume_after.as_deref()),
            )
        }
        // Global ledger event stream (last N events, SSE format).
        ("GET", "/api/v1/events/stream") => {
            let resume_after = query_param(query, "after");
            (
                "200 OK",
                global_ledger_stream_live_sse(resume_after.as_deref(), &shared_operation_backlog()),
            )
        }
        ("GET", "/api/v1/webhooks") => {
            let subscriptions = webhook_store
                .subscriptions
                .iter()
                .map(subscription_to_value)
                .collect::<Vec<_>>();
            (
                "200 OK",
                json(serde_json::json!({"subscriptions": subscriptions})),
            )
        }
        ("POST", "/api/v1/webhooks/reload") => {
            let reloaded_store = WebhookStore::load();
            let subscriptions = reloaded_store
                .subscriptions
                .iter()
                .map(subscription_to_value)
                .collect::<Vec<_>>();
            (
                "200 OK",
                json(serde_json::json!({
                    "reloaded": subscriptions.len(),
                    "subscriptions": subscriptions,
                })),
            )
        }
        ("POST", "/api/v1/webhooks/dispatch") => {
            let payload: Result<serde_json::Value, _> = serde_json::from_str(body);
            match payload {
                Err(error) => (
                    "400 Bad Request",
                    json(serde_json::json!({
                        "error": format!("invalid webhook dispatch payload: {}", error)
                    })),
                ),
                Ok(payload) => {
                    let event = payload
                        .get("event")
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .unwrap_or("");
                    if event.is_empty() {
                        return (
                            "400 Bad Request",
                            json(serde_json::json!({
                                "error": "webhook dispatch payload requires non-empty 'event'"
                            })),
                        );
                    }
                    let body_payload = payload
                        .get("payload")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let deliveries = dispatch_runtime_webhooks(webhook_store, event, &body_payload);
                    let delivered = deliveries.iter().filter(|result| result.is_ok()).count();
                    let failed = deliveries.len().saturating_sub(delivered);
                    (
                        "200 OK",
                        json(serde_json::json!({
                            "event": event,
                            "delivered": delivered,
                            "failed": failed,
                            "results": deliveries.into_iter()
                                .map(runtime_delivery_result_to_value)
                                .collect::<Vec<_>>(),
                        })),
                    )
                }
            }
        }
        _ => (
            "404 Not Found",
            json(serde_json::json!({"error": "not found"})),
        ),
    }
}

/// Aggregate the most recent ledger events across every process, sorted by
/// `created_at` descending, truncated to `limit`.
fn collect_recent_global_events(limit: usize) -> Vec<serde_json::Value> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let processes = store.list_all().unwrap_or_default();
    let mut all_events = Vec::new();
    for process in &processes {
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        if let Ok(events) = ledger.list_global_events(limit) {
            for event in events {
                all_events.push(ledger_event_to_value(event));
            }
        }
    }
    all_events.sort_by(|left, right| {
        let left_created = left["created_at"].as_str().unwrap_or("");
        let right_created = right["created_at"].as_str().unwrap_or("");
        right_created.cmp(left_created)
    });
    all_events.truncate(limit);
    all_events
}

fn ledger_event_to_value(event: zaion_types::event::LedgerEvent) -> serde_json::Value {
    let sig_ok = event
        .signature
        .as_ref()
        .map(|signature| !signature.0.is_empty())
        .unwrap_or(false);
    serde_json::json!({
        "event_id": event.event_id.0,
        "principal_id": event.principal_id.0,
        "event_type": event.event_type,
        "created_at": event.created_at,
        "sig_valid": sig_ok,
        "payload": event.payload,
    })
}

#[derive(Debug, Default)]
struct RuntimeTranscript {
    response_text: String,
    warnings: Vec<String>,
    errors: Vec<String>,
    operation_events: Vec<OperationEvent>,
}

fn collect_runtime_stream(rx: Receiver<StreamEvent>) -> RuntimeTranscript {
    let mut transcript = RuntimeTranscript::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            StreamEvent::Token(token) | StreamEvent::SystemNotice(token) => {
                transcript.response_text.push_str(&token);
            }
            StreamEvent::Warning(warning) | StreamEvent::Status(warning) => {
                transcript.warnings.push(warning);
            }
            StreamEvent::Error(error) => transcript.errors.push(error),
            StreamEvent::Operation(event) => transcript.operation_events.push(event),
            StreamEvent::ToolCall(_) | StreamEvent::Complete { .. } | StreamEvent::Cancelled => {}
        }
    }
    transcript
}

fn transcript_stream_contract_value(operation_events: &[OperationEvent]) -> serde_json::Value {
    let operation_event_cursor = operation_events
        .last()
        .map(operation_event_sse_id)
        .unwrap_or_default();
    let operation_event_values = operation_events
        .iter()
        .map(operation_event_payload)
        .collect::<Vec<_>>();
    serde_json::json!({
        "sink": "TranscriptSink",
        "live": false,
        "schema": "zaion.operation_stream.transcript.v1",
        "operation_backlog": "shared_process_local",
        "operation_event_count": operation_events.len(),
        "operation_event_cursor": operation_event_cursor,
        "operation_events": operation_event_values,
    })
}

fn api_run_stream_contract_value(run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.operation_stream.sse.v1",
        "sink": "ApiRunSseSnapshot",
        "live": false,
        "replayable": true,
        "run_id": run_id,
        "event_id_policy": "run_id:event_name",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "resume": {
            "mode": "snapshot_backlog",
            "cursor": "run_id:event_name",
            "operation_event_cursor": "operation:<stream_id>:<sequence>",
            "supports_after_query": true,
            "supports_last_event_id": true,
            "no_new_events_event": "stream.resume",
        },
        "events": [
            "run.snapshot",
            "operation.event",
            "stream.contract",
            "stream.resume",
        ],
    })
}

fn api_run_resume_value(after: &str) -> serde_json::Value {
    serde_json::json!({
        "mode": "snapshot_backlog",
        "requested_after": after,
        "cursor": "run_id:event_name",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "live": false,
        "no_new_events": true,
    })
}

fn api_run_wake_request(
    submitter: String,
    envelope: CanonicalEnvelope,
    provider: Option<String>,
    model: Option<String>,
) -> WakeRequest {
    let mut request = structured_wake_request(submitter, envelope.body.clone(), envelope);
    request.provider = provider;
    request.model = model;
    request.stream = false;
    request
}

#[cfg(test)]
fn api_run_stream_snapshot_sse_with_backlog(
    run: &zaion_a2a::AcpRun,
    resume_after: Option<&str>,
    backlog: &OperationStreamBacklog,
) -> String {
    let snapshot = serde_json::to_value(run).unwrap_or_else(|_| serde_json::json!({}));
    let backlog_events = match resume_after {
        Some(after) if after.starts_with("operation:") => backlog.replay_after(Some(after)),
        Some(_) => Vec::new(),
        None => backlog.replay_after(None),
    }
    .into_iter()
    .filter(|event| event.thread_id == run.run_id || event.turn_id == run.run_id)
    .collect::<Vec<_>>();
    let resume_event = resume_after.map(|after| {
        sse_event_with_id(
            &format!("{}:stream.resume", run.run_id),
            "stream.resume",
            &api_run_resume_value(after),
        )
    });
    format!(
        "{}{}{}{}",
        sse_event_with_id(
            &format!("{}:run.snapshot", run.run_id),
            "run.snapshot",
            &snapshot
        ),
        api_run_operation_backlog_sse(&backlog_events),
        resume_event.unwrap_or_default(),
        sse_event_with_id(
            &format!("{}:stream.contract", run.run_id),
            "stream.contract",
            &api_run_stream_contract_value(&run.run_id)
        )
    )
}

fn api_run_stream_live_sse_with_backlog(
    run: &zaion_a2a::AcpRun,
    resume_after: Option<&str>,
    backlog: &OperationStreamBacklog,
) -> String {
    let backlog_events =
        replay_operation_backlog_for_run(run.run_id.as_str(), resume_after, backlog);
    let resume_event = if backlog_events.is_empty() {
        Some(sse_event_with_id(
            &format!("{}:stream.resume", run.run_id),
            "stream.resume",
            &api_run_resume_value(resume_after.unwrap_or("")),
        ))
    } else {
        None
    };
    format!(
        "{}{}{}{}",
        sse_event_with_id(
            &format!("{}:run.snapshot", run.run_id),
            "run.snapshot",
            &serde_json::to_value(run).unwrap_or_else(|_| serde_json::json!({}))
        ),
        api_run_operation_backlog_sse(&backlog_events),
        resume_event.unwrap_or_default(),
        sse_event_with_id(
            &format!("{}:stream.contract", run.run_id),
            "stream.contract",
            &api_run_stream_contract_value(&run.run_id)
        )
    )
}

fn replay_operation_backlog_for_run(
    run_id: &str,
    resume_after: Option<&str>,
    backlog: &OperationStreamBacklog,
) -> Vec<OperationEvent> {
    if resume_after.is_some_and(|after| !after.starts_with("operation:")) {
        return Vec::new();
    }

    let current_events = match resume_after {
        Some(after) => backlog.replay_after(Some(after)),
        None => backlog.replay_after(None),
    };
    let current_events = current_events
        .into_iter()
        .filter(|event| event.thread_id == run_id || event.turn_id == run_id)
        .collect::<Vec<_>>();
    if !current_events.is_empty() {
        return current_events;
    }

    wait_for_shared_operation_backlog_after(resume_after, operation_live_stream_wait_timeout())
        .into_iter()
        .filter(|event| event.thread_id == run_id || event.turn_id == run_id)
        .collect()
}

fn operation_event_sse_id(event: &OperationEvent) -> String {
    zaion_runtime::operation_stream::OperationStreamCursor::new(
        event.stream_id.clone(),
        event.sequence,
    )
    .to_sse_id()
}

fn operation_event_payload(event: &OperationEvent) -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.operation_event.v1",
        "stream_id": event.stream_id,
        "turn_id": event.turn_id,
        "sequence": event.sequence,
        "timestamp": event.timestamp,
        "principal_id": event.principal_id,
        "channel_id": event.channel_id,
        "thread_id": event.thread_id,
        "stage": event.stage,
        "kind": event.kind,
        "level": event.level,
        "display_text": event.display_text,
        "payload": event.payload,
        "redaction_class": event.redaction_class,
        "ledger_event_id": event.ledger_event_id,
        "proof_hash": event.proof_hash,
        "parent_sequence": event.parent_sequence,
        "cursor": operation_event_sse_id(event),
    })
}

fn api_run_operation_backlog_sse(events: &[OperationEvent]) -> String {
    events
        .iter()
        .map(|event| {
            sse_event_with_id(
                &operation_event_sse_id(event),
                "operation.event",
                &operation_event_payload(event),
            )
        })
        .collect()
}

fn operation_live_stream_contract_value(
    resume_after: Option<&str>,
    events: &[OperationEvent],
) -> serde_json::Value {
    let cursor = events
        .last()
        .map(operation_event_sse_id)
        .or_else(|| resume_after.map(str::to_string))
        .unwrap_or_default();
    serde_json::json!({
        "schema": "zaion.operation_stream.live_sse.v1",
        "sink": "OperationLiveSseLongPoll",
        "live": true,
        "replayable": true,
        "transport": "long_poll_sse",
        "mode": "operation_backlog",
        "operation_backlog": "shared_process_local",
        "event_id_policy": "operation:<stream_id>:<sequence>",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "requested_after": resume_after.unwrap_or_default(),
        "cursor": cursor,
        "operation_event_count": events.len(),
        "resume": {
            "mode": "operation_backlog",
            "cursor": "operation:<stream_id>:<sequence>",
            "supports_after_query": true,
            "supports_last_event_id": true,
            "empty_poll_event": "stream.resume",
        },
        "events": [
            "operation.event",
            "stream.contract",
            "stream.resume",
        ],
    })
}

fn operation_live_websocket_contract_value(
    resume_after: Option<&str>,
    events: &[OperationEvent],
) -> serde_json::Value {
    let cursor = events
        .last()
        .map(operation_event_sse_id)
        .or_else(|| resume_after.map(str::to_string))
        .unwrap_or_default();
    serde_json::json!({
        "schema": "zaion.operation_stream.live_ws.v1",
        "sink": "OperationLiveWebSocket",
        "live": true,
        "replayable": true,
        "transport": "websocket",
        "mode": "operation_backlog",
        "operation_backlog": "shared_process_local",
        "endpoint": "/api/v1/operations/ws",
        "event_id_policy": "operation:<stream_id>:<sequence>",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "requested_after": resume_after.unwrap_or_default(),
        "cursor": cursor,
        "operation_event_count": events.len(),
        "resume": {
            "mode": "operation_backlog",
            "cursor": "operation:<stream_id>:<sequence>",
            "supports_after_query": true,
            "supports_last_event_id": true,
            "empty_poll_message": "stream.resume",
        },
        "events": [
            "operation.event",
            "stream.contract",
            "stream.resume",
        ],
    })
}

fn operation_live_resume_value(after: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "mode": "operation_backlog",
        "requested_after": after.unwrap_or_default(),
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "live": true,
        "no_new_events": true,
    })
}

pub(super) fn operation_live_stream_wait_timeout() -> Duration {
    Duration::from_secs(15)
}

fn operation_live_stream_sse_after_wait(resume_after: Option<&str>, timeout: Duration) -> String {
    let backlog_events = match resume_after {
        Some(after) if after.starts_with("operation:") => {
            wait_for_shared_operation_backlog_after(Some(after), timeout)
        }
        Some(_) => Vec::new(),
        None => wait_for_shared_operation_backlog_after(None, timeout),
    };
    let empty_poll = backlog_events.is_empty().then(|| {
        sse_event_with_id(
            "operation-live:stream.resume",
            "stream.resume",
            &operation_live_resume_value(resume_after),
        )
    });
    format!(
        "{}{}{}",
        sse_event_with_id(
            "operation-live:stream.contract",
            "stream.contract",
            &operation_live_stream_contract_value(resume_after, &backlog_events)
        ),
        api_run_operation_backlog_sse(&backlog_events),
        empty_poll.unwrap_or_default()
    )
}

fn operation_live_websocket_message(
    id: &str,
    message_type: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": message_type,
        "payload": payload,
    })
}

pub(super) fn operation_live_stream_ws_messages_after_wait(
    resume_after: Option<&str>,
    timeout: Duration,
) -> Vec<serde_json::Value> {
    let backlog_events = match resume_after {
        Some(after) if after.starts_with("operation:") => {
            wait_for_shared_operation_backlog_after(Some(after), timeout)
        }
        Some(_) => Vec::new(),
        None => wait_for_shared_operation_backlog_after(None, timeout),
    };
    let mut messages = vec![operation_live_websocket_message(
        "operation-live:stream.contract",
        "stream.contract",
        operation_live_websocket_contract_value(resume_after, &backlog_events),
    )];
    messages.extend(backlog_events.iter().map(|event| {
        operation_live_websocket_message(
            &operation_event_sse_id(event),
            "operation.event",
            operation_event_payload(event),
        )
    }));
    if messages.len() == 1 {
        messages.push(operation_live_websocket_message(
            "operation-live:stream.resume",
            "stream.resume",
            operation_live_resume_value(resume_after),
        ));
    }
    messages
}

fn operation_live_websocket_upgrade_required_body(resume_after: Option<&str>) -> String {
    serde_json::to_string(&serde_json::json!({
        "error": "websocket upgrade required",
        "endpoint": "/api/v1/operations/ws",
        "stream_contract": operation_live_websocket_contract_value(resume_after, &[]),
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn global_ledger_stream_contract_value(limit: usize) -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.operation_stream.events_sse.v1",
        "sink": "GlobalLedgerSseSnapshot",
        "live": false,
        "replayable": true,
        "limit": limit,
        "event_id_policy": "global-ledger:event_name",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "resume": {
            "mode": "snapshot",
            "cursor": "global-ledger:event_name",
            "operation_event_cursor": "operation:<stream_id>:<sequence>",
            "supports_after_query": true,
            "supports_last_event_id": true,
            "no_new_events_event": "stream.resume",
        },
        "events": [
            "ledger.snapshot",
            "operation.event",
            "stream.contract",
            "stream.resume",
        ],
    })
}

fn global_ledger_resume_value(after: &str) -> serde_json::Value {
    serde_json::json!({
        "mode": "snapshot",
        "requested_after": after,
        "cursor": "global-ledger:event_name",
        "operation_event_cursor": "operation:<stream_id>:<sequence>",
        "live": false,
        "no_new_events": true,
    })
}

fn global_ledger_stream_live_sse(
    resume_after: Option<&str>,
    backlog: &OperationStreamBacklog,
) -> String {
    let backlog_events = replay_operation_backlog_for_global(resume_after, backlog);
    let events = collect_recent_global_events(WEBHOOK_EVENT_RECENT_LIMIT);
    let resume_event = if backlog_events.is_empty() {
        Some(sse_event_with_id(
            "global-ledger:stream.resume",
            "stream.resume",
            &global_ledger_resume_value(resume_after.unwrap_or("")),
        ))
    } else {
        None
    };
    format!(
        "{}{}{}{}",
        sse_event_with_id(
            "global-ledger:stream.contract",
            "stream.contract",
            &global_ledger_stream_contract_value(events.len())
        ),
        api_run_operation_backlog_sse(&backlog_events),
        resume_event.unwrap_or_default(),
        sse_event_with_id(
            "global-ledger:ledger.snapshot",
            "ledger.snapshot",
            &serde_json::Value::Array(events)
        )
    )
}

fn replay_operation_backlog_for_global(
    resume_after: Option<&str>,
    backlog: &OperationStreamBacklog,
) -> Vec<OperationEvent> {
    if resume_after.is_some_and(|after| !after.starts_with("operation:")) {
        return Vec::new();
    }

    let current_events = match resume_after {
        Some(after) => backlog.replay_after(Some(after)),
        None => backlog.replay_after(None),
    };
    if !current_events.is_empty() {
        return current_events;
    }

    wait_for_shared_operation_backlog_after(resume_after, operation_live_stream_wait_timeout())
}

fn sse_event(name: &str, payload: &serde_json::Value) -> String {
    sse_event_with_id(name, name, payload)
}

fn sse_event_with_id(id: &str, name: &str, payload: &serde_json::Value) -> String {
    format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        id,
        name,
        serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string())
    )
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == name && !value.is_empty()).then(|| value.to_string())
    })
}

struct ApiRunProof {
    ingress_event_id: String,
    output_event_id: String,
    answer_trace_event_id: String,
    turn_proof_event_id: String,
    tool_receipt_ids: Vec<String>,
    tool_receipt_count: usize,
    tool_result_storage_receipts: Vec<serde_json::Value>,
    tool_result_storage_receipt_count: usize,
    tool_receipt_proof_join_event_id: Option<String>,
    tool_receipt_proof_join: Option<serde_json::Value>,
    tool_receipt_join_found: bool,
    tool_receipt_proof_hash_verified: bool,
}

fn runtime_proof_for_api_run(
    ledger: &zaion_ledger::EventLedger,
    run_id: &str,
) -> Option<ApiRunProof> {
    let events = ledger.list_global_events(100).ok()?;
    let proof = events.iter().find(|event| {
        event.event_type == "turn.proof"
            && event.payload["channel_id"].as_str() == Some("api")
            && event.payload["thread_id"].as_str() == Some(run_id)
    })?;
    let ingress_event_id = proof.payload["user_event_id"].as_str()?.to_string();
    let output_event_id = proof.payload["output_event_id"].as_str()?.to_string();
    let answer_trace_event_id = proof.payload["answer_trace_event_id"]
        .as_str()
        .or_else(|| proof.parent_event_id.as_ref().map(|id| id.0.as_str()))?
        .to_string();
    let omni_route_event_id = proof.payload["omni_route_event_id"].as_str()?.to_string();
    let received = events.iter().find(|event| {
        event.event_type == "channel.received"
            && event.event_id.0 == ingress_event_id
            && event.payload["channel_id"].as_str() == Some("api")
            && event.payload["thread_id"].as_str() == Some(run_id)
    })?;
    let route = events.iter().find(|event| {
        event.event_type == "omni.route"
            && event.event_id.0 == omni_route_event_id
            && event.payload["channel_id"].as_str() == Some("api")
            && event.payload["thread_id"].as_str() == Some(run_id)
    })?;
    let sent = events.iter().find(|event| {
        event.event_type == "channel.sent"
            && event.event_id.0 == output_event_id
            && event.payload["channel_id"].as_str() == Some("api")
            && event.payload["thread_id"].as_str() == Some(run_id)
    })?;
    let answer_trace = events.iter().find(|event| {
        event.event_type == "answer.trace"
            && event.event_id.0 == answer_trace_event_id
            && event.payload["channel_id"].as_str() == Some("api")
            && event.payload["thread_id"].as_str() == Some(run_id)
    })?;

    if [received, route, sent, answer_trace, proof]
        .iter()
        .any(|event| event.signature.is_none())
    {
        return None;
    }
    if route.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(received.event_id.0.as_str())
    {
        return None;
    }
    if route.payload["parent_received_event_id"].as_str() != Some(received.event_id.0.as_str()) {
        return None;
    }
    if sent.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace
        .parent_event_id
        .as_ref()
        .map(|id| id.0.as_str())
        != Some(sent.event_id.0.as_str())
    {
        return None;
    }
    if proof.parent_event_id.as_ref().map(|id| id.0.as_str())
        != Some(answer_trace.event_id.0.as_str())
    {
        return None;
    }
    let route_authority_hash = route.payload["authority_hash"].as_str()?;
    if proof.payload["answer_trace_event_id"].as_str() != Some(answer_trace.event_id.0.as_str()) {
        return None;
    }
    if proof.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }
    if answer_trace.payload["omni_route_event_id"].as_str() != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }
    let decoded_proof = serde_json::from_value::<TurnProof>(proof.payload.clone()).ok()?;
    let receipt_join = crate::commands::receipt_join::tool_receipt_proof_join_for_turn_proof(
        ledger,
        proof,
        &decoded_proof,
    )
    .unwrap_or_default();
    let storage_receipts = crate::commands::receipt_join::tool_result_storage_receipts(
        ledger,
        &decoded_proof.tool_receipt_ids,
    )
    .unwrap_or_default();

    Some(ApiRunProof {
        ingress_event_id,
        output_event_id,
        answer_trace_event_id,
        turn_proof_event_id: proof.event_id.0.clone(),
        tool_receipt_ids: decoded_proof.tool_receipt_ids.clone(),
        tool_receipt_count: decoded_proof.tool_receipt_count,
        tool_result_storage_receipt_count: storage_receipts.receipts.len(),
        tool_result_storage_receipts: storage_receipts.receipts,
        tool_receipt_proof_join_event_id: receipt_join.event_id,
        tool_receipt_proof_join: receipt_join.summary,
        tool_receipt_join_found: receipt_join.found,
        tool_receipt_proof_hash_verified: receipt_join.proof_hash_verified,
    })
}

fn subscription_to_value(subscription: &crate::config::WebhookSubscription) -> serde_json::Value {
    serde_json::json!({
        "name": subscription.name,
        "url": subscription.url,
        "events": subscription.events,
        "status": subscription.status,
        "has_secret": subscription.secret.is_some(),
    })
}

fn runtime_delivery_result_to_value(
    result: Result<RuntimeWebhookDelivery, String>,
) -> serde_json::Value {
    match result {
        Ok(delivery) => serde_json::json!({
            "subscription": delivery.subscription,
            "event": delivery.event,
            "status": "delivered",
            "delivery_backend": delivery.delivery_backend,
            "delivery_target": delivery.delivery_target,
            "backend_delivery": delivery.backend_delivery,
            "resolved_addrs": delivery.resolved_addrs,
            "status_code": delivery.status_code,
            "content_type": delivery.content_type,
            "body_preview": delivery.body_preview,
        }),
        Err(error) => serde_json::json!({
            "status": "failed",
            "error": error,
        }),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

fn run_idempotency_fingerprint(task: &str, submitter: &str) -> String {
    let canonical = serde_json::json!({
        "schema": "zaion.acp_run.idempotency_fingerprint.v1",
        "submitter_principal": submitter,
        "task": task,
    });
    let canonical = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

pub(super) fn gateway_http_response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}\r\n{}",
        status,
        content_type,
        body.len(),
        gateway_http_close_headers(),
        body
    )
}

pub(super) fn gateway_http_with_cors_origin(
    response: String,
    allowed_origin: Option<&str>,
) -> String {
    let Some(origin) = allowed_origin else {
        return response;
    };
    response.replacen(
        "X-Content-Type-Options: nosniff\r\n",
        &format!(
            "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nX-Content-Type-Options: nosniff\r\n"
        ),
        1,
    )
}

pub(super) fn gateway_http_contract_headers() -> &'static str {
    concat!(
        "Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n",
        "Access-Control-Allow-Headers: Authorization, Content-Type, Idempotency-Key, Last-Event-ID\r\n",
        "Access-Control-Max-Age: 600\r\n",
        "X-Content-Type-Options: nosniff\r\n",
        "Referrer-Policy: no-referrer\r\n",
        "Cache-Control: no-store\r\n"
    )
}

pub(super) fn gateway_http_close_headers() -> &'static str {
    concat!(
        "Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n",
        "Access-Control-Allow-Headers: Authorization, Content-Type, Idempotency-Key, Last-Event-ID\r\n",
        "Access-Control-Max-Age: 600\r\n",
        "X-Content-Type-Options: nosniff\r\n",
        "Referrer-Policy: no-referrer\r\n",
        "Cache-Control: no-store\r\n",
        "Connection: close\r\n"
    )
}

pub(super) fn route_body_with_idempotency_header(
    method: &str,
    path: &str,
    body: &str,
    idempotency_key_header: Option<&str>,
) -> String {
    let route_path = path.split_once('?').map(|(route, _)| route).unwrap_or(path);
    let Some(idempotency_key) = idempotency_key_header
        .map(str::trim)
        .filter(|key| !key.is_empty())
    else {
        return body.to_string();
    };
    if method != "POST" || route_path != "/v1/runs" {
        return body.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };
    let Some(object) = value.as_object_mut() else {
        return body.to_string();
    };
    object
        .entry("idempotency_key")
        .or_insert_with(|| serde_json::Value::String(idempotency_key.to_string()));
    serde_json::to_string(&value).unwrap_or_else(|_| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WebhookStore, WebhookSubscription, ZaionConfig};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;

    fn test_acp_store() -> zaion_a2a::AcpRunStore {
        let path =
            std::env::temp_dir().join(format!("zaion-gateway-test-{}.db", uuid::Uuid::new_v4()));
        zaion_a2a::AcpRunStore::new(path)
    }

    #[test]
    fn health_route_preserves_compatibility_fields_and_adds_gateway_identity() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) = gateway_route_with_webhooks("GET", "/health", "", &acp, &store);
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, "200 OK");
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["service"], "zaion-gateway");
        assert_eq!(body["schema"], "zaion.gateway.health.v1");
    }

    #[test]
    fn api_runtime_delivery_result_preserves_resolved_addrs() {
        let value = runtime_delivery_result_to_value(Ok(RuntimeWebhookDelivery {
            subscription: "audit-webhook".to_string(),
            event: "channel.received".to_string(),
            delivery_backend: None,
            delivery_target: None,
            backend_delivery: None,
            resolved_addrs: vec![
                "203.0.113.10:443".to_string(),
                "2001:db8::10:443".to_string(),
            ],
            status_code: 202,
            content_type: Some("application/json".to_string()),
            body_preview: Some("{\"ok\":true}".to_string()),
        }));

        assert_eq!(value["resolved_addrs"][0], "203.0.113.10:443");
        assert_eq!(value["resolved_addrs"][1], "2001:db8::10:443");
    }

    fn spawn_openai_compatible_mock(
        expected_requests: usize,
        content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(expected_requests) {
                handle_mock_request(stream.unwrap(), content);
            }
        });
        (addr, handle)
    }

    fn handle_mock_request(mut stream: TcpStream, content: &str) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        let mut content_length = 0usize;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse::<usize>().unwrap_or(0);
                }
            }
            line.clear();
        }
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).unwrap();
        }

        let body = serde_json::json!({
            "model": "llama3.2",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": content,
                },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 4,
            },
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn spawn_openai_tool_call_mock(
        final_content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        spawn_openai_named_tool_call_mock(
            final_content,
            "call_api_fs_list",
            "fs_list",
            "{\"path\":\".\"}",
        )
    }

    fn spawn_openai_named_tool_call_mock(
        final_content: &'static str,
        call_id: &'static str,
        tool_name: &'static str,
        arguments: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
        let addr = listener.local_addr().expect("mock addr");
        let handle = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            let mut handled = 0;
            while handled < 2 && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _body = read_mock_request_body(&mut stream);
                        if handled == 0 {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": null,
                                            "tool_calls": [{
                                                "id": call_id,
                                                "type": "function",
                                                "function": {
                                                    "name": tool_name,
                                                    "arguments": arguments
                                                }
                                            }]
                                        },
                                        "finish_reason": "tool_calls"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 13,
                                        "completion_tokens": 1
                                    }
                                }),
                            );
                        } else {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": final_content
                                        },
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 19,
                                        "completion_tokens": 5
                                    }
                                }),
                            );
                        }
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            handled
        });
        (addr, handle)
    }

    fn read_mock_request_body(stream: &mut TcpStream) -> String {
        stream
            .set_nonblocking(false)
            .expect("blocking request stream");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        let mut content_length = 0usize;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if line == "\r\n" || line == "\n" {
                break;
            }
            let trimmed = line.trim_end();
            if let Some((name, value)) = trimmed.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }

        let mut request_body = vec![0u8; content_length];
        if content_length > 0 {
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
        }
        String::from_utf8_lossy(&request_body).into_owned()
    }

    fn write_mock_json_response(stream: &mut TcpStream, body: serde_json::Value) {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    }

    #[test]
    fn gateway_route_options_preflight_is_explicit_and_bodyless() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) = gateway_route_with_webhooks("OPTIONS", "/v1/runs", "", &acp, &store);

        assert_eq!(status, "204 No Content");
        assert!(
            body.is_empty(),
            "CORS preflight must not trigger command-control route bodies: {body}"
        );
    }

    #[test]
    fn api_run_structured_wake_request_uses_workspace_tool_result_root() {
        let envelope = CanonicalEnvelope::new(
            "api",
            PrincipalId("did:key:api".to_string()),
            ChannelId("api".to_string()),
            ThreadId("run-a".to_string()),
            "message-a".to_string(),
            "run task".to_string(),
            None,
        )
        .unwrap();
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = api_run_wake_request("did:key:api".to_string(), envelope, None, None);

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(
                std::env::current_dir()
                    .unwrap()
                    .join(".zaion")
                    .join("tool-results")
                    .as_path()
            )
        );
    }

    #[test]
    fn api_run_wake_request_inherits_automatic_compression_without_forcing_it() {
        let envelope = CanonicalEnvelope::new(
            "api",
            PrincipalId("did:key:api".to_string()),
            ChannelId("api".to_string()),
            ThreadId("run-a".to_string()),
            "message-a".to_string(),
            "run task".to_string(),
            None,
        )
        .unwrap();
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = api_run_wake_request(
            "did:key:api".to_string(),
            envelope,
            Some("openai".to_string()),
            Some("gpt-5.5".to_string()),
        );
        let disabled = req.effective_features(zaion_runtime::WakeFeatureDefaults::default());
        let enabled = req.effective_features(zaion_runtime::WakeFeatureDefaults {
            compression_enabled: true,
            ..zaion_runtime::WakeFeatureDefaults::default()
        });

        assert_eq!(req.provider.as_deref(), Some("openai"));
        assert_eq!(req.model.as_deref(), Some("gpt-5.5"));
        assert!(!req.stream);
        assert!(!req.compress);
        assert!(!disabled.compression_enabled);
        assert!(!disabled.compression_requested);
        assert!(enabled.compression_enabled);
        assert!(!enabled.compression_requested);
    }

    #[test]
    fn api_run_wake_request_preserves_environment_identity_from_envelope_metadata() {
        let envelope = CanonicalEnvelope::new(
            "api",
            PrincipalId("did:key:api".to_string()),
            ChannelId("api".to_string()),
            ThreadId("run-a".to_string()),
            "message-a".to_string(),
            "run task".to_string(),
            None,
        )
        .unwrap()
        .with_metadata(
            "tool_result_environment",
            serde_json::json!({
                "environment_id": "daytona:workspace:api:sandbox-3",
                "environment_kind": "daytona",
            }),
        );
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = api_run_wake_request("did:key:api".to_string(), envelope, None, None);

        assert_eq!(
            req.tool_result_environment_id.as_deref(),
            Some("daytona:workspace:api:sandbox-3")
        );
        assert_eq!(req.tool_result_environment_kind.as_deref(), Some("daytona"));
    }

    #[test]
    fn gateway_http_response_adds_cors_preflight_and_security_headers() {
        let response = gateway_http_response("204 No Content", "application/json", "");

        for needle in [
            "Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n",
            "Access-Control-Allow-Headers: Authorization, Content-Type, Idempotency-Key, Last-Event-ID\r\n",
            "Access-Control-Max-Age: 600\r\n",
            "X-Content-Type-Options: nosniff\r\n",
            "Referrer-Policy: no-referrer\r\n",
            "Cache-Control: no-store\r\n",
        ] {
            assert!(
                response.contains(needle),
                "HTTP response missing CORS/security header {needle:?}: {response}"
            );
        }
        assert!(!response.contains("Access-Control-Allow-Origin: *"));

        let same_origin = gateway_http_with_cors_origin(response, Some("http://127.0.0.1:7821"));
        assert!(same_origin
            .contains("Access-Control-Allow-Origin: http://127.0.0.1:7821\r\nVary: Origin\r\n"));
    }

    #[test]
    fn web_console_route_serves_product_webui_with_existing_controls() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) = gateway_route_with_webhooks("GET", "/ui", "", &acp, &store);

        assert_eq!(status, "200 OK");
        for needle in [
            "Zaion 母舰控制台",
            "data-lang-button=\"zh\"",
            "data-lang-button=\"en\"",
            "三步启动 Zaion",
            "Zaion Carrier Console",
            "workspace-shell",
            "onboarding-deck",
            "carrier-map",
            "command-control",
            "runtime-grid",
            "run-submit-form",
            "run-idempotency-key-input",
            "operation-ws-button",
            "operation-ws-disconnect-button",
            "webhook-dispatch-form",
        ] {
            assert!(
                body.contains(needle),
                "WebUI missing product control-plane marker {needle:?}"
            );
        }
        assert!(
            !body.contains("ZAION GATEWAY"),
            "WebUI must replace the legacy terminal-console header"
        );
    }

    #[test]
    fn gateway_route_dispatch_requires_event_name() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) = gateway_route_with_webhooks(
            "POST",
            "/api/v1/webhooks/dispatch",
            r#"{"payload":{"hello":"world"}}"#,
            &acp,
            &store,
        );
        assert_eq!(status, "400 Bad Request");
        assert!(body.contains("requires non-empty 'event'"));
    }

    #[test]
    fn acp_create_run_reuses_idempotency_key_without_duplicate_signed_runtime() {
        let _guard = crate::config::env_test_lock();
        let temp_root =
            std::env::temp_dir().join(format!("zaion-api-idempotent-{}", uuid::Uuid::new_v4()));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        std::fs::create_dir_all(&temp_home).unwrap();
        std::fs::create_dir_all(&temp_zaion_home).unwrap();
        std::fs::create_dir_all(&temp_data).unwrap();

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);

        let (addr, server) = spawn_openai_compatible_mock(1, "api idempotent runtime");
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("api-idempotent-workspace", "api-idempotent-project")
            .expect("test process created");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");

        let acp = test_acp_store();
        let store = WebhookStore::default();
        let body = format!(
            r#"{{"task":"do idempotent API work","submitter_principal":"{}","idempotency_key":"console-key-1"}}"#,
            process.principal_id
        );
        let (status_one, body_one) =
            gateway_route_with_webhooks("POST", "/v1/runs", &body, &acp, &store);
        let (status_two, body_two) =
            gateway_route_with_webhooks("POST", "/v1/runs", &body, &acp, &store);

        assert_eq!(status_one, "201 Created", "body:\n{body_one}");
        assert_eq!(status_two, "200 OK", "body:\n{body_two}");
        let first: serde_json::Value = serde_json::from_str(&body_one).expect("first response");
        let second: serde_json::Value = serde_json::from_str(&body_two).expect("second response");
        assert_eq!(first["run_id"], second["run_id"]);
        assert_eq!(first["idempotency_key"], "console-key-1");
        assert_eq!(second["idempotency_key"], "console-key-1");
        assert_eq!(second["idempotency_reused"], true);
        assert_eq!(acp.list(10).expect("runs").len(), 1);

        server.join().unwrap();
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn acp_create_run_rejects_idempotency_key_reuse_for_different_request() {
        let _guard = crate::config::env_test_lock();
        let temp_root = std::env::temp_dir().join(format!(
            "zaion-api-idempotent-conflict-{}",
            uuid::Uuid::new_v4()
        ));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        std::fs::create_dir_all(&temp_home).unwrap();
        std::fs::create_dir_all(&temp_zaion_home).unwrap();
        std::fs::create_dir_all(&temp_data).unwrap();

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);

        let (addr, server) = spawn_openai_compatible_mock(1, "api conflict runtime");
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create(
                "api-idempotent-conflict-workspace",
                "api-idempotent-conflict-project",
            )
            .expect("test process created");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");

        let acp = test_acp_store();
        let store = WebhookStore::default();
        let first = format!(
            r#"{{"task":"do first API work","submitter_principal":"{}","idempotency_key":"console-key-conflict"}}"#,
            process.principal_id
        );
        let second = format!(
            r#"{{"task":"do different API work","submitter_principal":"{}","idempotency_key":"console-key-conflict"}}"#,
            process.principal_id
        );
        let (status_one, body_one) =
            gateway_route_with_webhooks("POST", "/v1/runs", &first, &acp, &store);
        let (status_two, body_two) =
            gateway_route_with_webhooks("POST", "/v1/runs", &second, &acp, &store);

        assert_eq!(status_one, "201 Created", "body:\n{body_one}");
        assert_eq!(status_two, "409 Conflict", "body:\n{body_two}");
        assert!(body_two.contains("Idempotency-Key reuse conflicts"));
        assert_eq!(acp.list(10).expect("runs").len(), 1);

        server.join().unwrap();
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn acp_create_run_requires_non_anonymous_envelope_principal() {
        let _guard = crate::config::env_test_lock();
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) =
            gateway_route_with_webhooks("POST", "/v1/runs", r#"{"task":"do work"}"#, &acp, &store);
        assert_eq!(status, "401 Unauthorized");
        assert!(body.contains("non-anonymous submitter_principal"));

        let temp_data =
            std::env::temp_dir().join(format!("zaion-api-run-{}", uuid::Uuid::new_v4()));
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("api-test-workspace", "api-test-project")
            .expect("test process created");
        let (addr, server) = spawn_openai_compatible_mock(1, "api envelope accepted");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");
        let (status, body) = gateway_route_with_webhooks(
            "POST",
            "/v1/runs",
            &format!(
                r#"{{"task":"do work","submitter_principal":"{}"}}"#,
                process.principal_id
            ),
            &acp,
            &store,
        );
        assert_eq!(status, "201 Created", "body:\n{}", body);
        assert!(body.contains("zaion.canonical_envelope.v1"));
        assert!(body.contains("\"source_hash\""));
        assert!(body.contains(&format!("\"principal_id\":\"{}\"", process.principal_id)));
        assert!(body.contains("\"run_id\""));
        assert!(body.contains("\"ingress_event_id\":\"evt-"));
        assert!(body.contains("\"ingress_event_type\":\"channel.received\""));
        assert!(!body.contains("\"run\":"));
        server.join().unwrap();

        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_data);
    }

    #[test]
    fn acp_create_run_executes_wake_runtime_and_returns_turn_proofs() {
        let _guard = crate::config::env_test_lock();
        let temp_root =
            std::env::temp_dir().join(format!("zaion-api-runtime-{}", uuid::Uuid::new_v4()));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        std::fs::create_dir_all(&temp_home).unwrap();
        std::fs::create_dir_all(&temp_zaion_home).unwrap();
        std::fs::create_dir_all(&temp_data).unwrap();

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);

        let (addr, server) = spawn_openai_compatible_mock(1, "api runtime proof ok");
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("api-runtime-workspace", "api-runtime-project")
            .expect("test process created");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");

        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) = gateway_route_with_webhooks(
            "POST",
            "/v1/runs",
            &format!(
                r#"{{"task":"do API runtime work","submitter_principal":"{}"}}"#,
                process.principal_id
            ),
            &acp,
            &store,
        );

        assert_eq!(status, "201 Created", "body:\n{}", body);
        let response: serde_json::Value = serde_json::from_str(&body).expect("json response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(response["response_text"], "api runtime proof ok");
        assert_eq!(response["ingress_event_type"], "channel.received");
        assert!(response["ingress_event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("evt-")));
        assert!(response["output_event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("evt-")));
        assert!(response["answer_trace_event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("evt-")));
        assert!(response["turn_proof_event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("evt-")));

        let run_id = response["run_id"].as_str().expect("run id");
        let ledger =
            zaion_ledger::EventLedger::new(process_store.ledger_path(&process.principal_id));
        let events = ledger.list_global_events(20).expect("ledger events");
        for event_type in [
            "channel.received",
            "channel.sent",
            "answer.trace",
            "turn.proof",
        ] {
            assert!(
                events.iter().any(|event| {
                    event.event_type == event_type
                        && event.payload["channel_id"] == "api"
                        && event.payload["thread_id"] == run_id
                }),
                "missing {event_type} event for API run thread {run_id}: {events:#?}"
            );
        }

        server.join().unwrap();
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn api_create_run_wake_tool_call_exposes_receipt_proof_trace() {
        let _guard = crate::config::env_test_lock();
        let temp_root =
            std::env::temp_dir().join(format!("zaion-api-runtime-tool-{}", uuid::Uuid::new_v4()));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        std::fs::create_dir_all(&temp_home).unwrap();
        std::fs::create_dir_all(&temp_zaion_home).unwrap();
        std::fs::create_dir_all(&temp_data).unwrap();

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);

        let (addr, server) = spawn_openai_tool_call_mock("api runtime tool proof ok");
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("api-runtime-tool-workspace", "api-runtime-tool-project")
            .expect("test process created");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");

        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) = gateway_route_with_webhooks(
            "POST",
            "/v1/runs",
            &format!(
                r#"{{"task":"do API runtime tool work","submitter_principal":"{}"}}"#,
                process.principal_id
            ),
            &acp,
            &store,
        );

        assert_eq!(status, "201 Created", "body:\n{}", body);
        let response: serde_json::Value = serde_json::from_str(&body).expect("json response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(response["response_text"], "api runtime tool proof ok");
        assert_eq!(
            response["tool_receipt_count"],
            serde_json::json!(1),
            "API run response should expose wake tool receipt count: {response:#?}"
        );
        let receipt_ids = response["tool_receipt_ids"]
            .as_array()
            .expect("tool receipt ids");
        assert_eq!(receipt_ids.len(), 1, "response: {response:#?}");
        let receipt_id = receipt_ids[0].as_str().expect("receipt id");
        assert!(receipt_id.starts_with("evt-"));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(0),
            "API run response should expose default storage receipt count: {response:#?}"
        );
        assert_eq!(
            response["tool_result_storage_receipts"],
            serde_json::json!([])
        );
        assert_eq!(response["tool_receipt_join_found"], serde_json::json!(true));
        assert_eq!(
            response["tool_receipt_proof_hash_verified"],
            serde_json::json!(true)
        );
        assert!(response["tool_receipt_proof_join_event_id"]
            .as_str()
            .is_some_and(|event_id| event_id.starts_with("evt-")));
        assert_eq!(
            response["tool_receipt_proof_join"]["turn_proof_event_id"],
            response["turn_proof_event_id"]
        );
        assert_eq!(
            response["tool_receipt_proof_join"]["tool_receipt_ids"],
            response["tool_receipt_ids"]
        );
        assert_eq!(
            response["tool_receipt_proof_join"]["proof_hash_matches_turn_proof"],
            serde_json::json!(true)
        );

        let ledger =
            zaion_ledger::EventLedger::new(process_store.ledger_path(&process.principal_id));
        let receipt = ledger
            .get_event(receipt_id)
            .expect("read receipt")
            .expect("receipt event");
        assert_eq!(receipt.event_type, "tool.receipt");
        assert_eq!(receipt.payload["source"], "native-provider");
        assert_eq!(receipt.payload["tool_name"], "fs_list");
        let join = ledger
            .list_events_by_payload_string_array_contains(
                &zaion_types::session::SessionKey(process.principal_id.clone()),
                "tool.receipt.proof_join",
                "tool_receipt_ids",
                receipt_id,
                1,
            )
            .expect("receipt join")
            .into_iter()
            .next()
            .expect("join event");
        assert_eq!(
            join.payload["turn_proof_event_id"],
            response["turn_proof_event_id"]
        );

        assert_eq!(server.join().unwrap(), 2);
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn api_create_run_wake_tool_call_exposes_persisted_storage_receipt_summary() {
        let _guard = crate::config::env_test_lock();
        let temp_root = std::env::temp_dir().join(format!(
            "zaion-api-runtime-storage-tool-{}",
            uuid::Uuid::new_v4()
        ));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&temp_home).unwrap();
        std::fs::create_dir_all(&temp_zaion_home).unwrap();
        std::fs::create_dir_all(&temp_data).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let large_file = workspace.join("large-search-source.txt");
        let mut large_content = String::new();
        let long_preview = "x".repeat(1_600);
        for idx in 0..120 {
            large_content.push_str(&format!(
                "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
            ));
        }
        std::fs::write(&large_file, large_content).expect("large search source");

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        let old_cwd = std::env::current_dir().expect("cwd");
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_current_dir(&workspace).expect("switch workspace");

        let (addr, server) = spawn_openai_named_tool_call_mock(
            "api runtime storage tool proof ok",
            "call_api_fs_search_large",
            "fs_search",
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}",
        );
        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create(
                "api-runtime-storage-tool-workspace",
                "api-runtime-storage-tool-project",
            )
            .expect("test process created");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("config saved");

        let acp = test_acp_store();
        let store = WebhookStore::default();
        let (status, body) = gateway_route_with_webhooks(
            "POST",
            "/v1/runs",
            &format!(
                r#"{{"task":"do API runtime storage tool work","submitter_principal":"{}"}}"#,
                process.principal_id
            ),
            &acp,
            &store,
        );

        assert_eq!(status, "201 Created", "body:\n{}", body);
        let response: serde_json::Value = serde_json::from_str(&body).expect("json response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(
            response["response_text"],
            "api runtime storage tool proof ok"
        );
        assert_eq!(response["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(1),
            "API run response should expose persisted storage receipt summary: {response:#?}"
        );
        let storage_receipts = response["tool_result_storage_receipts"]
            .as_array()
            .expect("storage receipt summaries");
        assert_eq!(storage_receipts.len(), 1, "response: {response:#?}");
        let storage_summary = &storage_receipts[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_api_fs_search_large")
        );
        assert_eq!(
            storage_summary["tool_result_storage"]["stored"],
            serde_json::json!(true)
        );
        assert_eq!(
            storage_summary["tool_result_storage_binding"]["environment"]["environment_kind"],
            serde_json::json!("storage_target")
        );
        let stored_path = storage_summary["tool_result_storage"]["path"]
            .as_str()
            .expect("stored path");
        assert!(
            stored_path.contains(".zaion") && stored_path.contains("tool-results"),
            "stored path should be workspace-visible: {stored_path}"
        );
        assert!(
            std::path::Path::new(stored_path).exists(),
            "stored output file should exist: {stored_path}"
        );

        assert_eq!(server.join().unwrap(), 2);
        std::env::set_current_dir(old_cwd).expect("restore cwd");
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn api_run_stream_returns_operation_snapshot_contract() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-contract run", "did:key:stream-contract")
            .expect("run created");

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!("/v1/runs/{}/stream", run.run_id),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: run.snapshot"));
        assert!(body.contains("event: stream.contract"));
        assert!(body.contains("\"schema\":\"zaion.operation_stream.sse.v1\""));
        assert!(body.contains("\"sink\":\"ApiRunSseSnapshot\""));
        assert!(body.contains("\"replayable\":true"));
        assert!(body.contains("\"run_id\""));
    }

    #[test]
    fn api_run_stream_includes_replay_event_ids() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-replay run", "did:key:stream-replay")
            .expect("run created");

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!("/v1/runs/{}/stream", run.run_id),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains(&format!("id: {}:run.snapshot", run.run_id)));
        assert!(body.contains(&format!("id: {}:stream.contract", run.run_id)));
        assert!(body.contains("\"event_id_policy\":\"run_id:event_name\""));
    }

    #[test]
    fn api_run_stream_contract_declares_resume_boundary() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-run-resume-contract", "did:key:stream-run-resume")
            .expect("run created");

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!("/v1/runs/{}/stream", run.run_id),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("\"resume\""));
        assert!(body.contains("\"mode\":\"snapshot_backlog\""));
        assert!(body.contains("\"supports_after_query\":true"));
        assert!(body.contains("\"supports_last_event_id\":true"));
        assert!(body.contains("\"no_new_events_event\":\"stream.resume\""));
    }

    #[test]
    fn api_run_stream_after_cursor_returns_resume_event() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-run-resume", "did:key:stream-run-resume")
            .expect("run created");

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!(
                "/v1/runs/{}/stream?after={}:run.snapshot",
                run.run_id, run.run_id
            ),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: stream.resume"), "body:\n{}", body);
        assert!(body.contains(&format!("id: {}:stream.resume", run.run_id)));
        assert!(body.contains(&format!(
            "\"requested_after\":\"{}:run.snapshot\"",
            run.run_id
        )));
        assert!(body.contains("\"mode\":\"snapshot_backlog\""));
        assert!(body.contains("event: run.snapshot"));
    }

    #[test]
    fn api_run_stream_waits_for_shared_operation_backlog_after_resume_cursor() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-run-live", "did:key:stream-run-live")
            .expect("run created");
        let run_id = run.run_id.clone();

        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "api-run-live-stream".to_string(),
            turn_id: run_id.clone(),
            sequence: 1,
            timestamp: "2026-05-07T00:00:00Z".to_string(),
            principal_id: "did:key:api-run-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "live API run provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        }]);

        let waiter_run_id = run_id.clone();
        let waiter = std::thread::spawn(move || {
            let (status, body) = gateway_route_with_webhooks(
                "GET",
                &format!(
                    "/v1/runs/{}/stream?after=operation:api-run-live-stream:1",
                    waiter_run_id
                ),
                "",
                &acp,
                &store,
            );
            assert_eq!(status, "200 OK");
            body
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "api-run-live-stream".to_string(),
            turn_id: run_id.clone(),
            sequence: 2,
            timestamp: "2026-05-07T00:00:01Z".to_string(),
            principal_id: "did:key:api-run-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: run_id,
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "live API run tool visible".to_string(),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        }]);

        let body = waiter
            .join()
            .expect("api run stream waiter should not panic");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(body.contains("id: operation:api-run-live-stream:2"));
        assert!(body.contains("\"display_text\":\"live API run tool visible\""));
        assert!(
            !body.contains("event: stream.resume"),
            "live API run stream should not emit resume when new backlog arrives: {body}"
        );
    }

    #[test]
    fn api_run_stream_can_render_operation_backlog_events() {
        let event = zaion_runtime::operation_stream::OperationEvent {
            stream_id: "stream-api-run".to_string(),
            turn_id: "run-api-backlog".to_string(),
            sequence: 1,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: "did:key:api-backlog".to_string(),
            channel_id: "api".to_string(),
            thread_id: "run-api-backlog".to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "tool database_query visible".to_string(),
            payload: serde_json::json!({
                "tool_name": "database_query",
                "input_preview": {
                    "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"
                }
            }),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        };

        let body = api_run_operation_backlog_sse(&[event]);

        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(body.contains("id: operation:stream-api-run:1"));
        assert!(body.contains("\"schema\":\"zaion.operation_event.v1\""));
        assert!(body.contains("\"display_text\":\"tool database_query visible\""));
        assert!(body.contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
    }

    #[test]
    fn api_run_stream_replays_operation_backlog_after_operation_cursor() {
        let acp = test_acp_store();
        let run = acp
            .create("stream-run-backlog-replay", "did:key:stream-run-backlog")
            .expect("run created");
        let mut backlog = zaion_runtime::operation_stream::OperationStreamBacklog::new(8);

        let first = zaion_runtime::operation_stream::OperationEvent {
            stream_id: "stream-api-run".to_string(),
            turn_id: run.run_id.clone(),
            sequence: 1,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: "did:key:api-backlog".to_string(),
            channel_id: "api".to_string(),
            thread_id: run.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "provider calling".to_string(),
            payload: serde_json::json!({"provider": "ollama"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        };
        let second = zaion_runtime::operation_stream::OperationEvent {
            stream_id: "stream-api-run".to_string(),
            turn_id: run.run_id.clone(),
            sequence: 2,
            timestamp: "2026-05-06T00:00:01Z".to_string(),
            principal_id: "did:key:api-backlog".to_string(),
            channel_id: "api".to_string(),
            thread_id: run.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "tool database_query visible".to_string(),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        };
        backlog.append(first);
        backlog.append(second);

        let body = api_run_stream_snapshot_sse_with_backlog(
            &run,
            Some("operation:stream-api-run:1"),
            &backlog,
        );

        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:stream-api-run:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:stream-api-run:2"),
            "body:\n{}",
            body
        );
        assert!(body.contains("\"sequence\":2"), "body:\n{}", body);
        assert!(
            body.contains("\"display_text\":\"tool database_query visible\""),
            "body:\n{}",
            body
        );
    }

    #[test]
    fn api_run_stream_replays_shared_wake_operation_backlog() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-run-shared-backlog", "did:key:stream-run-shared")
            .expect("run created");

        let mut transcript = RuntimeTranscript::default();
        transcript.operation_events.push(OperationEvent {
            stream_id: "wake-stream-shared".to_string(),
            turn_id: run.run_id.clone(),
            sequence: 1,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: "did:key:stream-run-shared".to_string(),
            channel_id: "api".to_string(),
            thread_id: run.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        });
        transcript.operation_events.push(OperationEvent {
            stream_id: "wake-stream-shared".to_string(),
            turn_id: run.run_id.clone(),
            sequence: 2,
            timestamp: "2026-05-06T00:00:01Z".to_string(),
            principal_id: "did:key:stream-run-shared".to_string(),
            channel_id: "api".to_string(),
            thread_id: run.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "tool database_query visible".to_string(),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        });

        append_shared_operation_backlog(&transcript.operation_events);

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!(
                "/v1/runs/{}/stream?after=operation:wake-stream-shared:1",
                run.run_id
            ),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:wake-stream-shared:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:wake-stream-shared:2"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("\"display_text\":\"tool database_query visible\""),
            "body:\n{}",
            body
        );
    }

    #[test]
    fn api_run_stream_replays_persisted_operation_backlog_after_process_restart() {
        let _guard = crate::config::env_test_lock();
        let temp_data =
            std::env::temp_dir().join(format!("zaion-api-run-stream-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_data).expect("temp data dir");
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        let old_persistence = std::env::var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST").ok();
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST", "1");

        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create(
                "stream-run-persisted-backlog",
                "did:key:stream-run-persisted",
            )
            .expect("run created");

        append_shared_operation_backlog(&[
            OperationEvent {
                stream_id: "wake-stream-persisted".to_string(),
                turn_id: run.run_id.clone(),
                sequence: 1,
                timestamp: "2026-05-06T00:00:00Z".to_string(),
                principal_id: "did:key:stream-run-persisted".to_string(),
                channel_id: "api".to_string(),
                thread_id: run.run_id.clone(),
                stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
                kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "persisted provider calling".to_string(),
                payload: serde_json::json!({"provider": "test"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: None,
            },
            OperationEvent {
                stream_id: "wake-stream-persisted".to_string(),
                turn_id: run.run_id.clone(),
                sequence: 2,
                timestamp: "2026-05-06T00:00:01Z".to_string(),
                principal_id: "did:key:stream-run-persisted".to_string(),
                channel_id: "api".to_string(),
                thread_id: run.run_id.clone(),
                stage: zaion_runtime::operation_stream::OperationStage::Tool,
                kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "persisted tool database_query visible".to_string(),
                payload: serde_json::json!({"tool_name": "database_query"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: Some(1),
            },
        ]);
        crate::commands::operation_backlog::reset_shared_operation_backlog_memory_only_for_test();

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!(
                "/v1/runs/{}/stream?after=operation:wake-stream-persisted:1",
                run.run_id
            ),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:wake-stream-persisted:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:wake-stream-persisted:2"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("\"display_text\":\"persisted tool database_query visible\""),
            "body:\n{}",
            body
        );

        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        match old_persistence {
            Some(value) => std::env::set_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST", value),
            None => std::env::remove_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST"),
        }
        let _ = std::fs::remove_dir_all(temp_data);
    }

    #[test]
    fn api_run_stream_filters_shared_operation_backlog_by_run_thread() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let run = acp
            .create("stream-run-isolated-backlog", "did:key:stream-run-isolated")
            .expect("run created");
        let other = acp
            .create("stream-run-other-backlog", "did:key:stream-run-other")
            .expect("other run created");
        let mut backlog = OperationStreamBacklog::new(8);

        backlog.append(OperationEvent {
            stream_id: "wake-stream-isolated".to_string(),
            turn_id: other.run_id.clone(),
            sequence: 1,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: "did:key:stream-run-other".to_string(),
            channel_id: "api".to_string(),
            thread_id: other.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "other run tool visible".to_string(),
            payload: serde_json::json!({"tool_name": "other_tool"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        });
        backlog.append(OperationEvent {
            stream_id: "wake-stream-isolated".to_string(),
            turn_id: run.run_id.clone(),
            sequence: 2,
            timestamp: "2026-05-06T00:00:01Z".to_string(),
            principal_id: "did:key:stream-run-isolated".to_string(),
            channel_id: "api".to_string(),
            thread_id: run.run_id.clone(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "target run provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        });

        let body = api_run_stream_snapshot_sse_with_backlog(&run, None, &backlog);

        assert!(
            body.contains("\"display_text\":\"target run provider calling\""),
            "body:\n{}",
            body
        );
        assert!(!body.contains("other run tool visible"), "body:\n{}", body);
    }

    #[test]
    fn api_run_stream_contract_declares_operation_backlog_cursor() {
        let acp = test_acp_store();
        let store = WebhookStore::default();
        let run = acp
            .create("stream-run-backlog-contract", "did:key:stream-run-backlog")
            .expect("run created");

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            &format!("/v1/runs/{}/stream", run.run_id),
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("\"mode\":\"snapshot_backlog\""));
        assert!(body.contains("\"operation_event_cursor\":\"operation:<stream_id>:<sequence>\""));
        assert!(body.contains("\"operation.event\""));
    }

    #[test]
    fn global_event_stream_is_not_captured_by_api_run_stream_route() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) =
            gateway_route_with_webhooks("GET", "/api/v1/events/stream", "", &acp, &store);

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: ledger.snapshot"), "body:\n{}", body);
        assert!(!body.contains("event: run.error"), "body:\n{}", body);
    }

    #[test]
    fn global_event_stream_returns_named_snapshot_contract() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) =
            gateway_route_with_webhooks("GET", "/api/v1/events/stream", "", &acp, &store);

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: ledger.snapshot"));
        assert!(body.contains("event: stream.contract"));
        assert!(body.contains("\"schema\":\"zaion.operation_stream.events_sse.v1\""));
        assert!(body.contains("\"sink\":\"GlobalLedgerSseSnapshot\""));
        assert!(body.contains("\"replayable\":true"));
        assert!(body.contains("\"events\":["));
    }

    #[test]
    fn global_event_stream_includes_replay_event_ids() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) =
            gateway_route_with_webhooks("GET", "/api/v1/events/stream", "", &acp, &store);

        assert_eq!(status, "200 OK");
        assert!(body.contains("id: global-ledger:stream.contract"));
        assert!(body.contains("id: global-ledger:ledger.snapshot"));
        assert!(body.contains("\"event_id_policy\":\"global-ledger:event_name\""));
    }

    #[test]
    fn global_event_stream_contract_declares_resume_boundary() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) =
            gateway_route_with_webhooks("GET", "/api/v1/events/stream", "", &acp, &store);

        assert_eq!(status, "200 OK");
        assert!(body.contains("\"resume\""));
        assert!(body.contains("\"mode\":\"snapshot\""));
        assert!(body.contains("\"supports_after_query\":true"));
        assert!(body.contains("\"supports_last_event_id\":true"));
        assert!(body.contains("\"no_new_events_event\":\"stream.resume\""));
    }

    #[test]
    fn global_event_stream_after_cursor_returns_resume_event() {
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            "/api/v1/events/stream?after=global-ledger:ledger.snapshot",
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: stream.resume"), "body:\n{}", body);
        assert!(body.contains("id: global-ledger:stream.resume"));
        assert!(body.contains("\"requested_after\":\"global-ledger:ledger.snapshot\""));
        assert!(body.contains("\"mode\":\"snapshot\""));
        assert!(body.contains("event: ledger.snapshot"));
    }

    #[test]
    fn global_event_stream_waits_for_shared_operation_backlog_after_resume_cursor() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();

        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "global-live-stream".to_string(),
            turn_id: "global-live-run".to_string(),
            sequence: 1,
            timestamp: "2026-05-07T00:00:00Z".to_string(),
            principal_id: "did:key:global-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: "global-live-run".to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "global live provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        }]);

        let waiter = std::thread::spawn(move || {
            let (status, body) = gateway_route_with_webhooks(
                "GET",
                "/api/v1/events/stream?after=operation:global-live-stream:1",
                "",
                &acp,
                &store,
            );
            assert_eq!(status, "200 OK");
            body
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "global-live-stream".to_string(),
            turn_id: "global-live-run".to_string(),
            sequence: 2,
            timestamp: "2026-05-07T00:00:01Z".to_string(),
            principal_id: "did:key:global-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: "global-live-run".to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "global live tool visible".to_string(),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        }]);

        let body = waiter
            .join()
            .expect("global event stream waiter should not panic");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(body.contains("id: operation:global-live-stream:2"));
        assert!(body.contains("\"display_text\":\"global live tool visible\""));
        assert!(
            !body.contains("event: stream.resume"),
            "global event stream should not emit resume when new backlog arrives: {body}"
        );
    }

    #[test]
    fn global_event_stream_replays_shared_operation_backlog_after_operation_cursor() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();

        append_shared_operation_backlog(&[
            OperationEvent {
                stream_id: "global-wake-stream".to_string(),
                turn_id: "global-run-1".to_string(),
                sequence: 1,
                timestamp: "2026-05-06T00:00:00Z".to_string(),
                principal_id: "did:key:global-stream".to_string(),
                channel_id: "api".to_string(),
                thread_id: "global-run-1".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
                kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "global provider calling".to_string(),
                payload: serde_json::json!({"provider": "test"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: None,
            },
            OperationEvent {
                stream_id: "global-wake-stream".to_string(),
                turn_id: "global-run-1".to_string(),
                sequence: 2,
                timestamp: "2026-05-06T00:00:01Z".to_string(),
                principal_id: "did:key:global-stream".to_string(),
                channel_id: "api".to_string(),
                thread_id: "global-run-1".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Tool,
                kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "global tool database_query visible".to_string(),
                payload: serde_json::json!({"tool_name": "database_query"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: Some(1),
            },
        ]);

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            "/api/v1/events/stream?after=operation:global-wake-stream:1",
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:global-wake-stream:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:global-wake-stream:2"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("\"display_text\":\"global tool database_query visible\""),
            "body:\n{}",
            body
        );
        assert!(body.contains("event: ledger.snapshot"), "body:\n{}", body);
    }

    #[test]
    fn global_event_stream_replays_persisted_operation_backlog_after_process_restart() {
        let _guard = crate::config::env_test_lock();
        let temp_data =
            std::env::temp_dir().join(format!("zaion-global-events-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_data).expect("temp data dir");
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        let old_persistence = std::env::var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST").ok();
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST", "1");

        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();

        append_shared_operation_backlog(&[
            OperationEvent {
                stream_id: "global-persisted-stream".to_string(),
                turn_id: "global-run-persisted".to_string(),
                sequence: 1,
                timestamp: "2026-05-06T00:00:00Z".to_string(),
                principal_id: "did:key:global-persisted".to_string(),
                channel_id: "api".to_string(),
                thread_id: "global-run-persisted".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
                kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "global persisted provider calling".to_string(),
                payload: serde_json::json!({"provider": "test"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: None,
            },
            OperationEvent {
                stream_id: "global-persisted-stream".to_string(),
                turn_id: "global-run-persisted".to_string(),
                sequence: 2,
                timestamp: "2026-05-06T00:00:01Z".to_string(),
                principal_id: "did:key:global-persisted".to_string(),
                channel_id: "api".to_string(),
                thread_id: "global-run-persisted".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Tool,
                kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "global persisted tool visible".to_string(),
                payload: serde_json::json!({"tool_name": "database_query"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: Some(1),
            },
        ]);
        crate::commands::operation_backlog::reset_shared_operation_backlog_memory_only_for_test();

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            "/api/v1/events/stream?after=operation:global-persisted-stream:1",
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:global-persisted-stream:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:global-persisted-stream:2"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("\"display_text\":\"global persisted tool visible\""),
            "body:\n{}",
            body
        );

        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        match old_persistence {
            Some(value) => std::env::set_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST", value),
            None => std::env::remove_var("ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST"),
        }
        let _ = std::fs::remove_dir_all(temp_data);
    }

    #[test]
    fn operation_live_stream_replays_operation_events_without_ledger_snapshot() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();

        append_shared_operation_backlog(&[
            OperationEvent {
                stream_id: "live-operation-stream".to_string(),
                turn_id: "live-operation-run".to_string(),
                sequence: 1,
                timestamp: "2026-05-06T00:00:00Z".to_string(),
                principal_id: "did:key:live-operation".to_string(),
                channel_id: "api".to_string(),
                thread_id: "live-operation-run".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
                kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "live provider calling".to_string(),
                payload: serde_json::json!({"provider": "test"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: None,
            },
            OperationEvent {
                stream_id: "live-operation-stream".to_string(),
                turn_id: "live-operation-run".to_string(),
                sequence: 2,
                timestamp: "2026-05-06T00:00:01Z".to_string(),
                principal_id: "did:key:live-operation".to_string(),
                channel_id: "api".to_string(),
                thread_id: "live-operation-run".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Tool,
                kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "live tool database_query visible".to_string(),
                payload: serde_json::json!({"tool_name": "database_query"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: Some(1),
            },
        ]);

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            "/api/v1/operations/stream?after=operation:live-operation-stream:1",
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "200 OK");
        assert!(body.contains("event: stream.contract"), "body:\n{}", body);
        assert!(body.contains("\"schema\":\"zaion.operation_stream.live_sse.v1\""));
        assert!(body.contains("\"transport\":\"long_poll_sse\""));
        assert!(body.contains("\"sink\":\"OperationLiveSseLongPoll\""));
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            !body.contains("id: operation:live-operation-stream:1"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("id: operation:live-operation-stream:2"),
            "body:\n{}",
            body
        );
        assert!(body.contains("\"display_text\":\"live tool database_query visible\""));
        assert!(
            !body.contains("event: ledger.snapshot"),
            "operation live transport must not fall back to ledger snapshot: {body}"
        );
    }

    #[test]
    fn operation_live_stream_waits_for_new_operation_events_before_resume() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "blocking-live-stream".to_string(),
            turn_id: "blocking-live-run".to_string(),
            sequence: 1,
            timestamp: "2026-05-07T00:00:00Z".to_string(),
            principal_id: "did:key:blocking-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: "blocking-live-run".to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
            kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "blocking live provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        }]);

        let waiter = std::thread::spawn(|| {
            let acp = test_acp_store();
            let store = WebhookStore::default();
            let (status, body) = gateway_route_with_webhooks(
                "GET",
                "/api/v1/operations/stream?after=operation:blocking-live-stream:1",
                "",
                &acp,
                &store,
            );
            assert_eq!(status, "200 OK");
            body
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        append_shared_operation_backlog(&[OperationEvent {
            stream_id: "blocking-live-stream".to_string(),
            turn_id: "blocking-live-run".to_string(),
            sequence: 2,
            timestamp: "2026-05-07T00:00:01Z".to_string(),
            principal_id: "did:key:blocking-live".to_string(),
            channel_id: "api".to_string(),
            thread_id: "blocking-live-run".to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: "blocking live tool visible".to_string(),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: Some(1),
        }]);

        let body = waiter.join().expect("live stream waiter should not panic");

        assert!(body.contains("event: stream.contract"), "body:\n{}", body);
        assert!(body.contains("event: operation.event"), "body:\n{}", body);
        assert!(
            body.contains("id: operation:blocking-live-stream:2"),
            "body:\n{}",
            body
        );
        assert!(
            body.contains("\"display_text\":\"blocking live tool visible\""),
            "body:\n{}",
            body
        );
        assert!(
            !body.contains("event: stream.resume"),
            "event arrival should prevent empty resume poll: {body}"
        );
    }

    #[test]
    fn operation_live_websocket_messages_replay_operation_events_without_ledger_snapshot() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();

        append_shared_operation_backlog(&[
            OperationEvent {
                stream_id: "live-ws-operation-stream".to_string(),
                turn_id: "live-ws-operation-run".to_string(),
                sequence: 1,
                timestamp: "2026-05-07T00:00:00Z".to_string(),
                principal_id: "did:key:live-ws-operation".to_string(),
                channel_id: "api".to_string(),
                thread_id: "live-ws-operation-run".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Reasoning,
                kind: zaion_runtime::operation_stream::OperationEventKind::ProviderCalling,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "live ws provider calling".to_string(),
                payload: serde_json::json!({"provider": "test"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::Public,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: None,
            },
            OperationEvent {
                stream_id: "live-ws-operation-stream".to_string(),
                turn_id: "live-ws-operation-run".to_string(),
                sequence: 2,
                timestamp: "2026-05-07T00:00:01Z".to_string(),
                principal_id: "did:key:live-ws-operation".to_string(),
                channel_id: "api".to_string(),
                thread_id: "live-ws-operation-run".to_string(),
                stage: zaion_runtime::operation_stream::OperationStage::Tool,
                kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
                level: zaion_runtime::operation_stream::OperationLevel::Info,
                display_text: "live ws tool visible".to_string(),
                payload: serde_json::json!({"tool_name": "database_query"}),
                redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
                ledger_event_id: None,
                proof_hash: None,
                parent_sequence: Some(1),
            },
        ]);

        let messages = operation_live_stream_ws_messages_after_wait(
            Some("operation:live-ws-operation-stream:1"),
            Duration::ZERO,
        );

        assert_eq!(messages.len(), 2, "messages: {messages:#?}");
        assert_eq!(messages[0]["type"], "stream.contract");
        assert_eq!(
            messages[0]["payload"]["schema"],
            "zaion.operation_stream.live_ws.v1"
        );
        assert_eq!(messages[0]["payload"]["transport"], "websocket");
        assert_eq!(messages[0]["payload"]["sink"], "OperationLiveWebSocket");
        assert_eq!(
            messages[0]["payload"]["cursor"],
            "operation:live-ws-operation-stream:2"
        );
        assert_eq!(messages[1]["type"], "operation.event");
        assert_eq!(messages[1]["id"], "operation:live-ws-operation-stream:2");
        assert_eq!(
            messages[1]["payload"]["display_text"],
            "live ws tool visible"
        );
        assert_eq!(
            messages[1]["payload"]["cursor"],
            "operation:live-ws-operation-stream:2"
        );
        assert!(
            messages
                .iter()
                .all(|message| message["type"] != "ledger.snapshot"),
            "operation WebSocket transport must not fall back to ledger snapshots: {messages:#?}"
        );
    }

    #[test]
    fn operation_live_websocket_route_declares_upgrade_contract() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        let acp = test_acp_store();
        let store = WebhookStore::default();

        let (status, body) = gateway_route_with_webhooks(
            "GET",
            "/api/v1/operations/ws?after=operation:live-ws-route:8",
            "",
            &acp,
            &store,
        );

        assert_eq!(status, "426 Upgrade Required");
        let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(payload["error"], "websocket upgrade required");
        assert_eq!(payload["endpoint"], "/api/v1/operations/ws");
        assert_eq!(
            payload["stream_contract"]["schema"],
            "zaion.operation_stream.live_ws.v1"
        );
        assert_eq!(payload["stream_contract"]["transport"], "websocket");
        assert_eq!(
            payload["stream_contract"]["requested_after"],
            "operation:live-ws-route:8"
        );
    }

    #[test]
    fn api_runtime_proof_rejects_unsigned_or_broken_ledger_chain() {
        let ledger_path = std::env::temp_dir().join(format!(
            "zaion-api-proof-broken-{}.db",
            uuid::Uuid::new_v4()
        ));
        let ledger = zaion_ledger::EventLedger::new(&ledger_path);
        let principal = zaion_types::identity::PrincipalId("api-proof-principal".to_string());
        let ns_key = zaion_types::session::NamespaceKey("api-proof-principal".to_string());
        let run_id = "run-broken-proof";

        let received = ledger
            .append_event(
                &principal,
                &ns_key,
                "channel.received",
                serde_json::json!({
                    "channel_id": "api",
                    "thread_id": run_id,
                }),
                None,
                None,
            )
            .expect("append unsigned received");
        let route = ledger
            .append_event_with_parent(
                &principal,
                &ns_key,
                "omni.route",
                serde_json::json!({
                    "authority": "OmniSessionManager",
                    "authority_hash": "unsigned-authority",
                    "channel_id": "api",
                    "thread_id": run_id,
                    "parent_received_event_id": received.0,
                }),
                None,
                None,
                Some(&received),
            )
            .expect("append unsigned route");
        let sent = ledger
            .append_event_with_parent(
                &principal,
                &ns_key,
                "channel.sent",
                serde_json::json!({
                    "channel_id": "api",
                    "thread_id": run_id,
                }),
                None,
                None,
                Some(&route),
            )
            .expect("append unsigned sent");
        let answer_trace = ledger
            .append_event_with_parent(
                &principal,
                &ns_key,
                "answer.trace",
                serde_json::json!({
                    "channel_id": "api",
                    "thread_id": run_id,
                    "omni_route_event_id": route.0,
                    "omni_route_authority_hash": "unsigned-authority",
                }),
                None,
                None,
                Some(&sent),
            )
            .expect("append unsigned answer trace");
        ledger
            .append_event_with_parent(
                &principal,
                &ns_key,
                "turn.proof",
                serde_json::json!({
                    "channel_id": "api",
                    "thread_id": run_id,
                    "user_event_id": received.0,
                    "output_event_id": sent.0,
                    "answer_trace_event_id": answer_trace.0,
                    "omni_route_event_id": route.0,
                    "omni_route_authority_hash": "unsigned-authority",
                }),
                None,
                None,
                Some(&answer_trace),
            )
            .expect("append unsigned proof");

        assert!(
            runtime_proof_for_api_run(&ledger, run_id).is_none(),
            "API proof extraction must reject unsigned proof chains"
        );

        let _ = std::fs::remove_file(&ledger_path);
    }

    #[test]
    fn gateway_route_reload_reads_latest_webhooks_from_disk() {
        let _guard = crate::config::env_test_lock();
        let acp = test_acp_store();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let temp_home = std::env::temp_dir().join(format!("zaion-webhook-home-{}", millis));
        std::fs::create_dir_all(&temp_home).unwrap();
        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_home);

        let persisted = WebhookStore {
            subscriptions: vec![WebhookSubscription {
                name: "reloaded".into(),
                url: "https://example.com/reloaded".into(),
                secret: None,
                events: vec!["channel.received".into()],
                description: None,
                skills: Vec::new(),
                deliver: None,
                deliver_chat_id: None,
                status: "active".into(),
                principal_id: None,
                prompt_template: None,
                background: None,
                timeout_secs: None,
            }],
        };
        persisted.save().unwrap();

        let in_memory = WebhookStore::default();
        let (status, body) =
            gateway_route_with_webhooks("POST", "/api/v1/webhooks/reload", "", &acp, &in_memory);

        assert_eq!(status, "200 OK");
        assert!(body.contains("reloaded"));
        assert!(body.contains("channel.received"));

        // Restore env to avoid polluting other tests.
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(v) => std::env::set_var("ZAION_HOME", v),
            None => std::env::remove_var("ZAION_HOME"),
        }
        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[tokio::test]
    async fn gateway_route_axum_adapter_serves_health() {
        use tower::ServiceExt;
        let acp = test_acp_store();
        let router = axum::Router::new()
            .route("/health", axum::routing::get(gateway_route_axum).post(gateway_route_axum))
            .with_state(acp);
        let req = axum::http::Request::builder()
            .uri("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200, "adapter should dispatch /health via gateway_route");
    }

}
