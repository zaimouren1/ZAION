//! ACP Stdio Service - JSON-RPC agent protocol over stdin/stdout
//!
//! This module implements the Agent Client Protocol (ACP) stdio service,
//! enabling external clients to interact with Zaion agents via JSON-RPC.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::acp::{AcpProtocolEvent, AcpRunStore, RunStatus};
use zaion_types::envelope::{ingest as ingest_envelope, is_unsafe_principal, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

/// JSON-RPC request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC notification frame emitted for server-side ACP protocol events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Wake-backed runtime dispatch request injected by the host crate.
#[derive(Debug, Clone)]
pub struct AcpRuntimeDispatchRequest {
    pub run_id: String,
    pub task: String,
    pub submitter_principal: String,
    pub envelope: CanonicalEnvelope,
}

/// Proof summary returned by a host runtime dispatcher.
#[derive(Debug, Clone)]
pub struct AcpRuntimeResult {
    pub response_text: String,
    pub runtime_warnings: Vec<String>,
    pub ingress_event_id: String,
    pub output_event_id: String,
    pub answer_trace_event_id: String,
    pub turn_proof_event_id: String,
    pub tool_receipt_ids: Vec<String>,
    pub tool_receipt_count: usize,
    pub tool_result_storage_receipts: Vec<Value>,
    pub tool_result_storage_receipt_count: usize,
    pub tool_receipt_proof_join_event_id: Option<String>,
    pub tool_receipt_proof_join: Option<Value>,
    pub tool_receipt_join_found: bool,
    pub tool_receipt_proof_hash_verified: bool,
    pub stream_contract: Option<Value>,
}

pub type AcpRuntimeDispatcher =
    Arc<dyn Fn(AcpRuntimeDispatchRequest) -> Result<AcpRuntimeResult> + Send + Sync>;

/// Sink used by runtimes to egress live ACP protocol events.
pub trait AcpProtocolEventSink {
    fn emit(&mut self, event: AcpProtocolEvent) -> Result<()>;
}

/// Writes ACP protocol events as newline-delimited stdio JSON-RPC notifications.
pub struct AcpStdioProtocolEventSink<W: Write> {
    writer: W,
}

impl<W: Write> AcpStdioProtocolEventSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> AcpProtocolEventSink for AcpStdioProtocolEventSink<W> {
    fn emit(&mut self, event: AcpProtocolEvent) -> Result<()> {
        let notification = AcpStdioService::protocol_event_notification(event)?;
        serde_json::to_writer(&mut self.writer, &notification)?;
        writeln!(&mut self.writer)?;
        self.writer.flush()?;
        Ok(())
    }
}

/// In-memory recorder for tests and host runtimes that need to inspect frames.
#[derive(Debug, Default, Clone)]
pub struct AcpProtocolEventCollector {
    frames: Vec<String>,
}

impl AcpProtocolEventCollector {
    pub fn frames(&self) -> &[String] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<String> {
        self.frames
    }
}

impl AcpProtocolEventSink for AcpProtocolEventCollector {
    fn emit(&mut self, event: AcpProtocolEvent) -> Result<()> {
        let notification = AcpStdioService::protocol_event_notification(event)?;
        let mut frame = serde_json::to_string(&notification)?;
        frame.push('\n');
        self.frames.push(frame);
        Ok(())
    }
}

/// ACP stdio service
pub struct AcpStdioService {
    /// Run store for persistence
    store: AcpRunStore,
    /// Principal ID for this service
    principal_id: String,
    /// Optional host-provided wake runtime dispatcher.
    runtime_dispatcher: Option<AcpRuntimeDispatcher>,
}

impl AcpStdioService {
    /// Create a new ACP stdio service
    pub fn new(store: AcpRunStore, principal_id: String) -> Self {
        Self {
            store,
            principal_id,
            runtime_dispatcher: None,
        }
    }

    /// Attach a host-provided wake runtime dispatcher.
    pub fn with_runtime_dispatcher<F>(mut self, dispatcher: F) -> Self
    where
        F: Fn(AcpRuntimeDispatchRequest) -> Result<AcpRuntimeResult> + Send + Sync + 'static,
    {
        self.runtime_dispatcher = Some(Arc::new(dispatcher));
        self
    }

    /// Build a stable JSON-RPC notification for a protocol event.
    pub fn protocol_event_notification(event: AcpProtocolEvent) -> Result<JsonRpcNotification> {
        Ok(JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "protocol/event".to_string(),
            params: serde_json::to_value(event)?,
        })
    }

    /// Serialize and write one newline-delimited protocol event notification.
    pub fn write_protocol_event<W: Write>(
        &self,
        writer: &mut W,
        event: AcpProtocolEvent,
    ) -> Result<()> {
        AcpStdioProtocolEventSink::new(writer).emit(event)
    }

    /// Run the stdio service (blocking)
    pub fn run(&self) -> Result<()> {
        info!(
            "ACP stdio service started (principal: {})",
            self.principal_id
        );

        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut stdout = std::io::stdout();

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF reached
                    info!("ACP stdio service: EOF reached, exiting");
                    break;
                }
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    debug!("Received request: {}", line);

                    // Parse JSON-RPC request
                    let response = match serde_json::from_str::<JsonRpcRequest>(line) {
                        Ok(request) => self.handle_request(request),
                        Err(e) => JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: None,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32700,
                                message: format!("Parse error: {}", e),
                                data: None,
                            }),
                        },
                    };

                    // Send response
                    let response_json = serde_json::to_string(&response)?;
                    writeln!(stdout, "{}", response_json)?;
                    stdout.flush()?;
                }
                Err(e) => {
                    error!("Failed to read from stdin: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle a JSON-RPC request
    fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request),
            "runs/create" => self.handle_create_run(&request),
            "runs/get" => self.handle_get_run(&request),
            "runs/list" => self.handle_list_runs(&request),
            "runs/cancel" => self.handle_cancel_run(&request),
            "new_session" => self.handle_new_session(&request),
            "load_session" => self.handle_load_session(&request),
            "resume_session" => self.handle_resume_session(&request),
            "fork_session" => self.handle_fork_session(&request),
            _ => Err(anyhow::anyhow!("Method not found: {}", request.method)),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            },
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, _request: &JsonRpcRequest) -> Result<Value> {
        Ok(serde_json::json!({
            "protocol_version": "1.0",
            "server_name": "zaion-acp",
            "server_version": env!("CARGO_PKG_VERSION"),
            "capabilities": {
                "runs": true,
                "sessions": true,
                "session_lifecycle": true,
                "protocol_events": true,
                "streaming": false,
            },
            "session_methods": [
                "new_session",
                "load_session",
                "resume_session",
                "fork_session"
            ],
            "event_kinds": [
                "tool.progress",
                "permission.request",
                "permission.result",
                "thinking.delta",
                "text.delta"
            ]
        }))
    }

    fn handle_new_session(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;
        let submitter = self.session_submitter(params, "new_session")?;
        let title = params.get("title").and_then(|v| v.as_str());
        let session = self.store.create_session(submitter, title, None, None)?;
        Ok(serde_json::to_value(session)?)
    }

    fn handle_load_session(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .context("Missing session_id")?;
        let session = self.store.get_session(session_id)?;
        self.ensure_session_owner(params, &session, "load_session")?;
        Ok(serde_json::to_value(session)?)
    }

    fn handle_resume_session(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .context("Missing session_id")?;
        let session = self.store.get_session(session_id)?;
        self.ensure_session_owner(params, &session, "resume_session")?;
        Ok(serde_json::to_value(
            self.store.resume_session(session_id)?,
        )?)
    }

    fn handle_fork_session(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .context("Missing session_id")?;
        let parent = self.store.get_session(session_id)?;
        let submitter = self.ensure_session_owner(params, &parent, "fork_session")?;
        let title = params.get("title").and_then(|v| v.as_str());
        let forked =
            self.store
                .create_session(submitter, title, Some(session_id), Some(session_id))?;
        Ok(serde_json::to_value(forked)?)
    }

    fn session_submitter<'a>(&'a self, params: &'a Value, method: &str) -> Result<&'a str> {
        let submitter = params
            .get("submitter_principal")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.principal_id);
        if is_unsafe_principal(submitter) {
            return Err(anyhow::anyhow!(
                "{method} requires a non-anonymous submitter_principal"
            ));
        }
        if submitter != self.principal_id {
            return Err(anyhow::anyhow!(
                "{method} submitter_principal must match the ACP service principal"
            ));
        }
        Ok(submitter)
    }

    fn ensure_session_owner<'a>(
        &'a self,
        params: &'a Value,
        session: &crate::acp::AcpSession,
        method: &str,
    ) -> Result<&'a str> {
        let submitter = self.session_submitter(params, method)?;
        if submitter != session.submitter_principal {
            return Err(anyhow::anyhow!(
                "{method} submitter_principal must match the session owner"
            ));
        }
        Ok(submitter)
    }

    /// Handle create run request
    fn handle_create_run(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;

        let task = params
            .get("task")
            .and_then(|v| v.as_str())
            .context("Missing task")?;

        let submitter = params
            .get("submitter_principal")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.principal_id);
        if is_unsafe_principal(submitter) {
            return Err(anyhow::anyhow!(
                "runs/create requires a non-anonymous submitter_principal"
            ));
        }

        let requested_runtime_route = params
            .get("runtime_route")
            .or_else(|| params.get("route"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("ingress");
        let wake_runtime_requested = requested_runtime_route.eq_ignore_ascii_case("wake");
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let thread_id = if wake_runtime_requested {
            run_id.clone()
        } else {
            "acp-runs".to_string()
        };

        let message_id = format!("acp-{}", uuid::Uuid::new_v4());
        let envelope = CanonicalEnvelope::new(
            "acp-stdio",
            PrincipalId(submitter.to_string()),
            ChannelId("acp-stdio".to_string()),
            ThreadId(thread_id),
            message_id,
            task.to_string(),
            None,
        )
        .map_err(|error| anyhow::anyhow!("canonical envelope rejected: {}", error))?;
        let envelope = ingest_envelope(&envelope)
            .map_err(|error| anyhow::anyhow!("canonical envelope rejected: {}", error))?;
        let mut ingress_payload = envelope.to_channel_received_payload();

        let process_store = zaion_core::process::ProcessStore::new(self.store.base_dir());
        let (_process, keypair) = process_store
            .load(submitter)
            .map_err(|error| anyhow::anyhow!("submitter identity unavailable: {}", error))?;

        if wake_runtime_requested {
            annotate_acp_turn_runtime_scope(&mut ingress_payload);
            let dispatcher = self.runtime_dispatcher.as_ref().ok_or_else(|| {
                anyhow::anyhow!("ACP wake runtime requested but no runtime dispatcher is installed")
            })?;
            let run = self
                .store
                .create_with_run_id(&run_id, &envelope.body, submitter)?;
            self.store
                .update_status(&run.run_id, RunStatus::Running, None, None)
                .map_err(|error| anyhow::anyhow!("failed to mark ACP run running: {}", error))?;
            let runtime = match dispatcher(AcpRuntimeDispatchRequest {
                run_id: run.run_id.clone(),
                task: envelope.body.clone(),
                submitter_principal: submitter.to_string(),
                envelope: envelope.clone(),
            }) {
                Ok(runtime) => runtime,
                Err(error) => {
                    let message = error.to_string();
                    let _ = self.store.update_status(
                        &run.run_id,
                        RunStatus::Failed,
                        None,
                        Some(&message),
                    );
                    return Err(anyhow::anyhow!("ACP wake runtime failed: {}", message));
                }
            };
            self.store
                .update_status(
                    &run.run_id,
                    RunStatus::Completed,
                    Some(&runtime.response_text),
                    None,
                )
                .map_err(|error| anyhow::anyhow!("failed to mark ACP run completed: {}", error))?;
            let run = self.store.get(&run.run_id).unwrap_or(run);
            let mut value = serde_json::to_value(run)?;
            if let serde_json::Value::Object(ref mut object) = value {
                object.insert("runtime_scope".to_string(), "turn_runtime".into());
                object.insert("runtime_route".to_string(), "wake".into());
                object.insert("proof_chain".to_string(), acp_turn_proof_chain_value());
                object.insert("ingress".to_string(), ingress_payload);
                object.insert(
                    "ingress_event_id".to_string(),
                    serde_json::Value::String(runtime.ingress_event_id),
                );
                object.insert(
                    "ingress_event_type".to_string(),
                    serde_json::Value::String("channel.received".to_string()),
                );
                object.insert(
                    "output_event_id".to_string(),
                    serde_json::Value::String(runtime.output_event_id),
                );
                object.insert(
                    "answer_trace_event_id".to_string(),
                    serde_json::Value::String(runtime.answer_trace_event_id),
                );
                object.insert(
                    "turn_proof_event_id".to_string(),
                    serde_json::Value::String(runtime.turn_proof_event_id),
                );
                object.insert(
                    "tool_receipt_ids".to_string(),
                    serde_json::json!(runtime.tool_receipt_ids),
                );
                object.insert(
                    "tool_receipt_count".to_string(),
                    serde_json::json!(runtime.tool_receipt_count),
                );
                object.insert(
                    "tool_result_storage_receipts".to_string(),
                    serde_json::json!(runtime.tool_result_storage_receipts),
                );
                object.insert(
                    "tool_result_storage_receipt_count".to_string(),
                    serde_json::json!(runtime.tool_result_storage_receipt_count),
                );
                object.insert(
                    "tool_receipt_proof_join_event_id".to_string(),
                    runtime
                        .tool_receipt_proof_join_event_id
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
                object.insert(
                    "tool_receipt_proof_join".to_string(),
                    runtime
                        .tool_receipt_proof_join
                        .unwrap_or(serde_json::Value::Null),
                );
                object.insert(
                    "tool_receipt_join_found".to_string(),
                    serde_json::Value::Bool(runtime.tool_receipt_join_found),
                );
                object.insert(
                    "tool_receipt_proof_hash_verified".to_string(),
                    serde_json::Value::Bool(runtime.tool_receipt_proof_hash_verified),
                );
                object.insert(
                    "response_text".to_string(),
                    serde_json::Value::String(runtime.response_text),
                );
                object.insert(
                    "runtime_warnings".to_string(),
                    serde_json::Value::Array(
                        runtime
                            .runtime_warnings
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
                if let Some(stream_contract) = runtime.stream_contract {
                    object.insert("stream_contract".to_string(), stream_contract);
                }
            }
            return Ok(value);
        }

        annotate_acp_ingress_scope(&mut ingress_payload);
        let ledger = zaion_ledger::EventLedger::new(process_store.ledger_path(submitter));
        let namespace = zaion_types::session::NamespaceKey(submitter.to_string());
        let ingress_event_id = ledger
            .append_signed_event(
                &keypair,
                &namespace,
                "channel.received",
                ingress_payload.clone(),
                None,
            )
            .map_err(|error| {
                anyhow::anyhow!("failed to append ACP ingress ledger event: {}", error)
            })?;

        let run = self
            .store
            .create_with_run_id(&run_id, &envelope.body, submitter)?;
        let mut value = serde_json::to_value(run)?;
        if let serde_json::Value::Object(ref mut object) = value {
            object.insert("runtime_scope".to_string(), "ingress_only".into());
            object.insert("proof_chain".to_string(), serde_json::Value::Null);
            object.insert("ingress".to_string(), ingress_payload);
            object.insert(
                "ingress_event_id".to_string(),
                serde_json::Value::String(ingress_event_id.0),
            );
            object.insert(
                "ingress_event_type".to_string(),
                serde_json::Value::String("channel.received".to_string()),
            );
        }

        Ok(value)
    }

    /// Handle get run request
    fn handle_get_run(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;

        let run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .context("Missing run_id")?;

        let run = self.store.get(run_id)?;

        Ok(serde_json::to_value(run)?)
    }

    /// Handle list runs request
    fn handle_list_runs(&self, request: &JsonRpcRequest) -> Result<Value> {
        let limit = request
            .params
            .as_ref()
            .and_then(|p| p.get("limit"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let runs = self.store.list(limit)?;

        Ok(serde_json::to_value(runs)?)
    }

    /// Handle cancel run request
    fn handle_cancel_run(&self, request: &JsonRpcRequest) -> Result<Value> {
        let params = request.params.as_ref().context("Missing params")?;

        let run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .context("Missing run_id")?;

        self.store.cancel(run_id)?;

        Ok(serde_json::json!({
            "success": true
        }))
    }
}

fn annotate_acp_ingress_scope(payload: &mut serde_json::Value) {
    if let serde_json::Value::Object(object) = payload {
        object.insert("runtime_scope".to_string(), "ingress_only".into());
        object.insert(
            "runtime_scope_reason".to_string(),
            "ACP stdio queues a run; route through API /v1/runs for wake turn proofs".into(),
        );
    }
}

fn annotate_acp_turn_runtime_scope(payload: &mut serde_json::Value) {
    if let serde_json::Value::Object(object) = payload {
        object.insert("runtime_scope".to_string(), "turn_runtime".into());
        object.insert(
            "runtime_scope_reason".to_string(),
            "ACP stdio requested wake dispatch and must return a turn proof chain".into(),
        );
    }
}

fn acp_turn_proof_chain_value() -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.turn_proof_chain.v1",
        "events": [
            "channel.received",
            "omni.route",
            "channel.sent",
            "answer.trace",
            "turn.proof"
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AcpProtocolEvent, AcpTextDeltaEvent, AcpThinkingDeltaEvent, AcpToolProgressEvent};

    fn temp_process_and_store() -> (AcpRunStore, String) {
        let data_dir =
            std::env::temp_dir().join(format!("zaion_acp_test_{}", uuid::Uuid::new_v4()));
        let process_store = zaion_core::process::ProcessStore::new(&data_dir);
        let (process, _keypair) = process_store
            .create("acp-test-workspace", "acp-test-project")
            .expect("test process created");
        (
            AcpRunStore::new(data_dir.join("acp-runs.db")),
            process.principal_id,
        )
    }

    #[test]
    fn test_stdio_service_creation() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal.clone());
        assert_eq!(service.principal_id, principal);
    }

    #[test]
    fn test_handle_initialize() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: None,
        };

        let response = service.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["protocol_version"], "1.0");
        assert_eq!(result["server_name"], "zaion-acp");
    }

    #[test]
    fn acp_stdio_initialize_advertises_session_lifecycle_methods() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);

        let response = service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: None,
        });

        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.expect("initialize result");
        assert_eq!(result["capabilities"]["sessions"], true);
        assert_eq!(result["capabilities"]["session_lifecycle"], true);
        assert!(result["session_methods"]
            .as_array()
            .expect("session methods")
            .iter()
            .any(|method| method == "new_session"));
        assert!(result["session_methods"]
            .as_array()
            .expect("session methods")
            .iter()
            .any(|method| method == "fork_session"));
    }

    #[test]
    fn acp_protocol_event_records_are_protocol_shaped() {
        let tool_progress = AcpProtocolEvent::ToolProgress(AcpToolProgressEvent {
            session_id: Some("session-1".to_string()),
            run_id: Some("run-1".to_string()),
            tool_call_id: "tool-1".to_string(),
            tool_name: "fs_read".to_string(),
            status: "running".to_string(),
            message: Some("reading file".to_string()),
            progress: Some(0.5),
        });
        let thinking_delta = AcpProtocolEvent::ThinkingDelta(AcpThinkingDeltaEvent {
            session_id: Some("session-1".to_string()),
            run_id: Some("run-1".to_string()),
            delta: "checking context".to_string(),
        });
        let text_delta = AcpProtocolEvent::TextDelta(AcpTextDeltaEvent {
            session_id: Some("session-1".to_string()),
            run_id: Some("run-1".to_string()),
            delta: "Hello".to_string(),
        });

        let value = serde_json::to_value(vec![tool_progress, thinking_delta, text_delta])
            .expect("protocol events serialize");
        assert_eq!(value[0]["schema"], "zaion.acp.event.v1");
        assert_eq!(value[0]["type"], "tool.progress");
        assert_eq!(value[0]["tool_call_id"], "tool-1");
        assert_eq!(value[0]["progress"], 0.5);
        assert_eq!(value[1]["type"], "thinking.delta");
        assert_eq!(value[1]["delta"], "checking context");
        assert_eq!(value[2]["type"], "text.delta");
        assert_eq!(value[2]["delta"], "Hello");
    }

    #[test]
    fn acp_stdio_protocol_event_notification_shape_is_stable() {
        let notification = AcpStdioService::protocol_event_notification(
            AcpProtocolEvent::TextDelta(AcpTextDeltaEvent {
                session_id: Some("session-1".to_string()),
                run_id: Some("run-1".to_string()),
                delta: "Hello from ACP".to_string(),
            }),
        )
        .expect("notification builds");

        let value = serde_json::to_value(notification).expect("notification serializes");
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "protocol/event");
        assert!(
            value.get("id").is_none(),
            "notifications must not carry ids"
        );
        assert_eq!(value["params"]["schema"], "zaion.acp.event.v1");
        assert_eq!(value["params"]["type"], "text.delta");
        assert_eq!(value["params"]["session_id"], "session-1");
        assert_eq!(value["params"]["run_id"], "run-1");
        assert_eq!(value["params"]["delta"], "Hello from ACP");
    }

    #[test]
    fn acp_stdio_writes_newline_delimited_protocol_event_frame() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);
        let mut output = Vec::new();

        service
            .write_protocol_event(
                &mut output,
                AcpProtocolEvent::ToolProgress(AcpToolProgressEvent {
                    session_id: Some("session-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    tool_call_id: "tool-1".to_string(),
                    tool_name: "fs_read".to_string(),
                    status: "running".to_string(),
                    message: Some("reading file".to_string()),
                    progress: Some(0.25),
                }),
            )
            .expect("event frame writes");

        let frame = String::from_utf8(output).expect("utf8 frame");
        assert!(
            frame.ends_with('\n'),
            "stdio protocol frames must be newline-delimited"
        );
        let value: serde_json::Value =
            serde_json::from_str(frame.trim_end()).expect("frame parses");
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "protocol/event");
        assert!(value.get("id").is_none(), "event frame is a notification");
        assert_eq!(value["params"]["schema"], "zaion.acp.event.v1");
        assert_eq!(value["params"]["type"], "tool.progress");
        assert_eq!(value["params"]["tool_call_id"], "tool-1");
        assert_eq!(value["params"]["tool_name"], "fs_read");
        assert_eq!(value["params"]["progress"], 0.25);
    }

    #[test]
    fn acp_protocol_event_sink_writes_text_delta_notification_frame() {
        let mut output = Vec::new();
        let mut sink = AcpStdioProtocolEventSink::new(&mut output);

        sink.emit(AcpProtocolEvent::TextDelta(AcpTextDeltaEvent {
            session_id: Some("session-1".to_string()),
            run_id: Some("run-1".to_string()),
            delta: "Hello".to_string(),
        }))
        .expect("text delta writes through sink");

        let frame = String::from_utf8(output).expect("utf8 frame");
        assert!(frame.ends_with('\n'));
        assert_eq!(frame.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(frame.trim()).expect("frame parses");
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "protocol/event");
        assert!(value.get("id").is_none(), "sink emits notifications");
        assert_eq!(value["params"]["type"], "text.delta");
        assert_eq!(value["params"]["delta"], "Hello");
    }

    #[test]
    fn acp_protocol_event_collector_records_tool_progress_notification_frame() {
        let mut collector = AcpProtocolEventCollector::default();

        collector
            .emit(AcpProtocolEvent::ToolProgress(AcpToolProgressEvent {
                session_id: Some("session-1".to_string()),
                run_id: Some("run-1".to_string()),
                tool_call_id: "tool-1".to_string(),
                tool_name: "fs_read".to_string(),
                status: "running".to_string(),
                message: Some("reading".to_string()),
                progress: Some(0.5),
            }))
            .expect("tool progress records through collector");

        assert_eq!(collector.frames().len(), 1);
        let value: serde_json::Value =
            serde_json::from_str(&collector.frames()[0]).expect("frame parses");
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "protocol/event");
        assert!(value.get("id").is_none(), "collector records notifications");
        assert_eq!(value["params"]["type"], "tool.progress");
        assert_eq!(value["params"]["tool_call_id"], "tool-1");
        assert_eq!(value["params"]["tool_name"], "fs_read");
        assert_eq!(value["params"]["progress"], 0.5);
    }

    #[test]
    fn acp_stdio_initialize_advertises_protocol_event_kinds() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);

        let response = service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: None,
        });

        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.expect("initialize result");
        assert_eq!(result["capabilities"]["protocol_events"], true);
        assert_eq!(
            result["event_kinds"],
            serde_json::json!([
                "tool.progress",
                "permission.request",
                "permission.result",
                "thinking.delta",
                "text.delta"
            ])
        );
    }

    #[test]
    fn acp_stdio_session_lifecycle_persists_resume_and_fork() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());

        let create = service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "new_session".to_string(),
            params: Some(serde_json::json!({
                "submitter_principal": principal,
                "title": "ACP lifecycle"
            })),
        });
        assert!(create.error.is_none(), "{:?}", create.error);
        let created = create.result.expect("created session");
        let session_id = created["session_id"].as_str().expect("session id");
        assert!(session_id.starts_with("session-"));
        assert_eq!(created["status"], "active");
        assert_eq!(created["parent_session_id"], serde_json::Value::Null);

        let persisted_service = AcpStdioService::new(store.clone(), principal.clone());
        let load = persisted_service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(2)),
            method: "load_session".to_string(),
            params: Some(serde_json::json!({
                "session_id": session_id
            })),
        });
        assert!(load.error.is_none(), "{:?}", load.error);
        let loaded = load.result.expect("loaded session");
        assert_eq!(loaded["session_id"], session_id);
        assert_eq!(loaded["resume_count"], 0);

        let resume = persisted_service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(3)),
            method: "resume_session".to_string(),
            params: Some(serde_json::json!({
                "session_id": session_id
            })),
        });
        assert!(resume.error.is_none(), "{:?}", resume.error);
        let resumed = resume.result.expect("resumed session");
        assert_eq!(resumed["session_id"], session_id);
        assert_eq!(resumed["resume_count"], 1);

        let fork = persisted_service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(4)),
            method: "fork_session".to_string(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "title": "ACP lifecycle branch"
            })),
        });
        assert!(fork.error.is_none(), "{:?}", fork.error);
        let forked = fork.result.expect("forked session");
        assert_ne!(forked["session_id"], session_id);
        assert_eq!(forked["parent_session_id"], session_id);
        assert_eq!(forked["forked_from_session_id"], session_id);
    }

    #[test]
    fn acp_stdio_session_lifecycle_rejects_cross_principal_access() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());
        let create = service.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "new_session".to_string(),
            params: Some(serde_json::json!({
                "submitter_principal": principal,
                "title": "owner-only"
            })),
        });
        let created = create.result.expect("created session");
        let session_id = created["session_id"].as_str().expect("session id");

        let other_service = AcpStdioService::new(store.clone(), "did:key:other".to_string());
        for method in ["load_session", "resume_session", "fork_session"] {
            let response = other_service.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(Value::from(2)),
                method: method.to_string(),
                params: Some(serde_json::json!({
                    "session_id": session_id,
                    "submitter_principal": "did:key:other"
                })),
            });
            assert!(response.result.is_none(), "{method} should fail");
            let error = response.error.expect("cross-principal access denied");
            assert!(
                error.message.contains("session owner"),
                "{method} returned unexpected error: {}",
                error.message
            );
        }
    }

    #[test]
    fn test_handle_create_run() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal.clone());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/create".to_string(),
            params: Some(serde_json::json!({
                "task": "Test task",
                "submitter_principal": principal
            })),
        };

        let response = service.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert!(result["run_id"].as_str().unwrap().starts_with("run-"));
        assert_eq!(result["task"], "Test task");
        assert_eq!(result["submitter_principal"], principal);
        assert_eq!(result["ingress"]["schema"], "zaion.canonical_envelope.v1");
        assert_eq!(result["ingress"]["source"], "acp-stdio");
        assert_eq!(result["ingress"]["channel_id"], "acp-stdio");
        assert_eq!(result["ingress"]["source_hash"].as_str().unwrap().len(), 64);
        assert!(result["ingress_event_id"]
            .as_str()
            .unwrap()
            .starts_with("evt-"));
        assert_eq!(result["ingress_event_type"], "channel.received");
    }

    #[test]
    fn acp_stdio_create_run_records_signed_ingress_only_scope() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/create".to_string(),
            params: Some(serde_json::json!({
                "task": "Queue this ACP task",
                "submitter_principal": principal
            })),
        };

        let response = service.handle_request(request);
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.expect("create run result");
        assert_eq!(result["runtime_scope"], "ingress_only");
        assert_eq!(result["proof_chain"], serde_json::Value::Null);
        assert_eq!(result["ingress"]["runtime_scope"], "ingress_only");
        assert_eq!(
            result["ingress"]["runtime_scope_reason"],
            "ACP stdio queues a run; route through API /v1/runs for wake turn proofs"
        );

        let process_store = zaion_core::process::ProcessStore::new(store.base_dir());
        let ledger = zaion_ledger::EventLedger::new(process_store.ledger_path(&principal));
        let events = ledger.list_global_events(20).expect("ledger events");
        let ingress_event_id = result["ingress_event_id"].as_str().unwrap();
        let ingress_event = events
            .iter()
            .find(|event| event.event_id.0 == ingress_event_id)
            .expect("ingress ledger event");
        assert_eq!(ingress_event.event_type, "channel.received");
        assert!(ingress_event.signature.is_some());
        assert_eq!(ingress_event.payload["source"], "acp-stdio");
        assert_eq!(ingress_event.payload["runtime_scope"], "ingress_only");
        assert!(events.iter().all(|event| event.event_type != "turn.proof"));
    }

    #[test]
    fn acp_stdio_create_run_can_route_through_injected_wake_runtime() {
        let (store, principal) = temp_process_and_store();
        let expected_principal = principal.clone();
        let service = AcpStdioService::new(store.clone(), principal.clone())
            .with_runtime_dispatcher(move |request| {
                assert_eq!(request.submitter_principal, expected_principal);
                assert_eq!(request.task, "Dispatch this ACP task through wake");
                assert_eq!(request.envelope.channel.0, "acp-stdio");
                assert_eq!(request.envelope.thread.0, request.run_id);
                Ok(AcpRuntimeResult {
                    response_text: "wake-backed ACP complete".to_string(),
                    runtime_warnings: vec!["mock warning".to_string()],
                    ingress_event_id: "evt-received".to_string(),
                    output_event_id: "evt-sent".to_string(),
                    answer_trace_event_id: "evt-answer".to_string(),
                    turn_proof_event_id: "evt-proof".to_string(),
                    tool_receipt_ids: vec!["evt-receipt".to_string()],
                    tool_receipt_count: 1,
                    tool_result_storage_receipts: vec![serde_json::json!({
                        "receipt_event_id": "evt-receipt",
                        "tool_name": "fs_read",
                        "tool_result_storage": {
                            "environment_id": "docker:acp:container-1",
                            "environment_kind": "docker"
                        },
                        "tool_result_storage_binding": {
                            "environment": {
                                "environment_id": "docker:acp:container-1",
                                "environment_kind": "docker"
                            }
                        }
                    })],
                    tool_result_storage_receipt_count: 1,
                    tool_receipt_proof_join_event_id: Some("evt-join".to_string()),
                    tool_receipt_proof_join: Some(serde_json::json!({
                        "event_id": "evt-join",
                        "turn_proof_event_id": "evt-proof",
                        "tool_receipt_ids": ["evt-receipt"],
                        "proof_hash_matches_turn_proof": true,
                        "turn_proof_event_matches": true,
                    })),
                    tool_receipt_join_found: true,
                    tool_receipt_proof_hash_verified: true,
                    stream_contract: Some(serde_json::json!({
                        "sink": "TranscriptSink",
                        "live": false,
                        "schema": "zaion.operation_stream.transcript.v1",
                        "operation_backlog": "shared_process_local",
                        "operation_event_count": 1,
                        "operation_event_cursor": "operation:acp-test-stream:1",
                        "operation_events": [{
                            "schema": "zaion.operation_event.v1",
                            "cursor": "operation:acp-test-stream:1"
                        }],
                    })),
                })
            });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/create".to_string(),
            params: Some(serde_json::json!({
                "task": "Dispatch this ACP task through wake",
                "submitter_principal": principal,
                "runtime_route": "wake"
            })),
        };

        let response = service.handle_request(request);
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.expect("create run result");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["runtime_scope"], "turn_runtime");
        assert_eq!(result["runtime_route"], "wake");
        assert_eq!(result["response_text"], "wake-backed ACP complete");
        assert_eq!(result["runtime_warnings"][0], "mock warning");
        assert_eq!(
            result["stream_contract"]["operation_backlog"],
            "shared_process_local"
        );
        assert_eq!(result["stream_contract"]["operation_event_count"], 1);
        assert_eq!(
            result["stream_contract"]["operation_events"][0]["schema"],
            "zaion.operation_event.v1"
        );
        assert_eq!(result["ingress"]["runtime_scope"], "turn_runtime");
        assert_eq!(
            result["proof_chain"]["events"],
            serde_json::json!([
                "channel.received",
                "omni.route",
                "channel.sent",
                "answer.trace",
                "turn.proof"
            ])
        );
        assert_eq!(result["ingress_event_id"], "evt-received");
        assert_eq!(result["output_event_id"], "evt-sent");
        assert_eq!(result["answer_trace_event_id"], "evt-answer");
        assert_eq!(result["turn_proof_event_id"], "evt-proof");
        assert_eq!(result["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            result["tool_receipt_ids"],
            serde_json::json!(["evt-receipt"])
        );
        assert_eq!(
            result["tool_result_storage_receipt_count"],
            serde_json::json!(1)
        );
        assert_eq!(
            result["tool_result_storage_receipts"][0]["receipt_event_id"],
            serde_json::json!("evt-receipt")
        );
        assert_eq!(
            result["tool_result_storage_receipts"][0]["tool_result_storage"]["environment_id"],
            serde_json::json!("docker:acp:container-1")
        );
        assert_eq!(result["tool_receipt_join_found"], serde_json::json!(true));
        assert_eq!(
            result["tool_receipt_proof_hash_verified"],
            serde_json::json!(true)
        );
        assert_eq!(
            result["tool_receipt_proof_join_event_id"],
            serde_json::json!("evt-join")
        );
        assert_eq!(
            result["tool_receipt_proof_join"]["turn_proof_event_id"],
            serde_json::json!("evt-proof")
        );

        let run_id = result["run_id"].as_str().expect("run id");
        let stored = store.get(run_id).expect("stored run");
        assert_eq!(stored.status, RunStatus::Completed);
        assert_eq!(stored.result.as_deref(), Some("wake-backed ACP complete"));
    }

    #[test]
    fn test_handle_create_run_rejects_anonymous_submitter() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/create".to_string(),
            params: Some(serde_json::json!({
                "task": "Test task",
                "submitter_principal": "anonymous"
            })),
        };

        let response = service.handle_request(request);
        assert!(response.result.is_none());
        let error = response.error.expect("anonymous submitter is rejected");
        assert!(error.message.contains("non-anonymous submitter_principal"));
    }

    #[test]
    fn test_handle_get_run() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());

        // Create a run first
        let run = store.create("Test task", &principal).unwrap();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/get".to_string(),
            params: Some(serde_json::json!({
                "run_id": run.run_id
            })),
        };

        let response = service.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["run_id"], run.run_id);
        assert_eq!(result["task"], "Test task");
    }

    #[test]
    fn test_handle_list_runs() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());

        // Create some runs
        store.create("Task 1", &principal).unwrap();
        store.create("Task 2", &principal).unwrap();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/list".to_string(),
            params: Some(serde_json::json!({
                "limit": 10
            })),
        };

        let response = service.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        let runs = result.as_array().unwrap();
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn test_handle_cancel_run() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store.clone(), principal.clone());

        // Create a run first
        let run = store.create("Test task", &principal).unwrap();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "runs/cancel".to_string(),
            params: Some(serde_json::json!({
                "run_id": run.run_id
            })),
        };

        let response = service.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        // Verify run was cancelled
        let cancelled_run = store.get(&run.run_id).unwrap();
        assert_eq!(cancelled_run.status, RunStatus::Cancelled);
    }

    #[test]
    fn test_handle_unknown_method() {
        let (store, principal) = temp_process_and_store();
        let service = AcpStdioService::new(store, principal);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "unknown_method".to_string(),
            params: None,
        };

        let response = service.handle_request(request);
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert!(error.message.contains("Method not found"));
    }
}
