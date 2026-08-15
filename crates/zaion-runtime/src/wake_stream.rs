//! Runtime-owned streaming and cancellation protocol for one wake turn.
//!
//! This is the typed boundary between the turn kernel producer and terminal,
//! channel, HTTP, MCP, ACP, logging, and test consumers. Surfaces never need to
//! parse status strings to recover operation or completion semantics.

use crate::operation_stream::{
    OperationContext, OperationEvent, OperationEventKind, OperationLevel, OperationStage,
    OperationStreamBus, RedactionClass,
};
use crate::{PartialLedgerTail, ProofClosure, RuntimeOutput, TurnError, TurnExecution};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// One tool invocation the LLM is about to perform.
#[derive(Debug, Clone)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCallEvent {
    pub fn from_visible_tool_call(call: &crate::operation_stream::VisibleToolCall) -> Self {
        let call = call.clone().redacted_for_panel();
        Self {
            id: call.call_id.clone(),
            name: call.tool_name.clone(),
            arguments: serde_json::to_string_pretty(&call.input_preview)
                .unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

/// Message types sent from wake pipeline to consumer.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Token delta from LLM streaming.
    Token(String),
    /// Status update (e.g., "Thinking...", "Compressing history...").
    Status(String),
    /// A structured tool call detected in the response.
    ToolCall(ToolCallEvent),
    /// Runtime-owned operation stream event. This is the architecture-level
    /// stream contract; legacy variants remain for existing consumers.
    Operation(OperationEvent),
    /// A queued slash-command result or other system notice that should be
    /// shown inline in the chat area, not in the status bar.
    SystemNotice(String),
    /// Non-fatal warning (e.g. injection scan finding).
    Warning(String),
    /// Final response metadata.
    Complete {
        input_tokens: usize,
        output_tokens: usize,
    },
    /// Turn was cancelled by consumer.
    Cancelled,
    /// Fatal error occurred.
    Error(String),
}

#[cfg(test)]
impl StreamEvent {
    pub fn from_operation_event(event: OperationEvent) -> Self {
        StreamEvent::Operation(event)
    }
}

/// Callback wrapper that sends stream events to a channel. Holds an
/// optional cancel flag so the consumer can request abortion.
#[derive(Clone)]
pub struct StreamCallback {
    tx: Sender<StreamEvent>,
    cancel: Arc<AtomicBool>,
}

impl StreamCallback {
    pub fn new(tx: Sender<StreamEvent>) -> Self {
        Self {
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn send_token(&self, token: String) {
        let _ = self.tx.send(StreamEvent::Token(token));
    }

    pub fn send_status(&self, status: String) {
        let _ = self.tx.send(StreamEvent::Status(status));
    }

    pub fn send_warning(&self, warning: String) {
        let _ = self.tx.send(StreamEvent::Warning(warning));
    }

    pub fn send_notice(&self, notice: String) {
        let _ = self.tx.send(StreamEvent::SystemNotice(notice));
    }

    pub fn send_tool_call(&self, call: ToolCallEvent) {
        let _ = self.tx.send(StreamEvent::ToolCall(call));
    }

    pub fn send_operation(&self, event: OperationEvent) {
        let _ = self.tx.send(StreamEvent::Operation(event));
    }

    fn send_complete(&self, input_tokens: usize, output_tokens: usize) {
        let _ = self.tx.send(StreamEvent::Complete {
            input_tokens,
            output_tokens,
        });
    }

    fn send_cancelled(&self) {
        let _ = self.tx.send(StreamEvent::Cancelled);
    }

    pub fn send_error(&self, error: String) {
        let _ = self.tx.send(StreamEvent::Error(error));
    }
}

#[derive(Clone)]
pub struct WakeOperationRecorder {
    bus: Arc<Mutex<OperationStreamBus>>,
    callback: Option<StreamCallback>,
}

impl WakeOperationRecorder {
    pub fn new(
        context: OperationContext,
        callback: Option<StreamCallback>,
        replay_capacity: usize,
    ) -> Self {
        Self {
            bus: Arc::new(Mutex::new(OperationStreamBus::new(
                context,
                replay_capacity,
            ))),
            callback,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &self,
        stage: OperationStage,
        kind: OperationEventKind,
        level: OperationLevel,
        display_text: impl Into<String>,
        payload: serde_json::Value,
        redaction_class: RedactionClass,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        let event = {
            let mut bus = self
                .bus
                .lock()
                .expect("wake operation recorder mutex poisoned");
            bus.emit(
                stage,
                kind,
                level,
                display_text,
                payload,
                redaction_class,
                parent_sequence,
            )
        };
        if let Some(callback) = &self.callback {
            callback.send_operation(event.clone());
        }
        event
    }

    pub fn emit_turn_started(&self) -> OperationEvent {
        self.emit(
            OperationStage::Ingress,
            OperationEventKind::TurnStarted,
            OperationLevel::Info,
            "turn started",
            serde_json::json!({}),
            RedactionClass::Public,
            None,
        )
    }

    #[cfg(test)]
    pub fn emit_status(&self, status: impl Into<String>) -> OperationEvent {
        let status = status.into();
        self.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            status.clone(),
            serde_json::json!({"status": status}),
            RedactionClass::Public,
            None,
        )
    }

    pub fn emit_token_delta(&self, token: &str, parent_sequence: Option<u64>) -> OperationEvent {
        self.emit(
            OperationStage::Reasoning,
            OperationEventKind::TokenDelta,
            OperationLevel::Trace,
            "token delta",
            serde_json::json!({"char_count": token.chars().count()}),
            RedactionClass::Public,
            parent_sequence,
        )
    }

    pub fn emit_tool_visible(
        &self,
        call: &ToolCallEvent,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        let input_preview = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": call.arguments}));
        self.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            format!("tool {} visible", call.name),
            serde_json::json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "input_preview": input_preview,
            }),
            RedactionClass::PanelSafe,
            parent_sequence,
        )
    }

    pub fn emit_ledger_appended(
        &self,
        event_type: &str,
        ledger_event_id: &str,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        self.emit(
            OperationStage::Ledger,
            OperationEventKind::LedgerEventAppended,
            OperationLevel::Info,
            format!("ledger event appended: {}", event_type),
            serde_json::json!({
                "event_type": event_type,
                "ledger_event_id": ledger_event_id,
            }),
            RedactionClass::Public,
            parent_sequence,
        )
    }

    fn emit_turn_completed(
        &self,
        input_tokens: usize,
        output_tokens: usize,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        self.emit(
            OperationStage::Outcome,
            OperationEventKind::TurnCompleted,
            OperationLevel::Info,
            "turn completed",
            serde_json::json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
            }),
            RedactionClass::Public,
            parent_sequence,
        )
    }

    /// Finalize a proof-verified provider turn under one runtime-owned order:
    /// operation completion, final transcript hash, typed execution, then the
    /// legacy surface completion event.
    pub fn finish_completed_turn(
        &self,
        mut output: RuntimeOutput,
        closure: ProofClosure,
        input_tokens: usize,
        output_tokens: usize,
        parent_sequence: Option<u64>,
    ) -> TurnExecution {
        self.emit_turn_completed(input_tokens, output_tokens, parent_sequence);
        output.set_stream_hash(self.transcript_hash());
        let execution = TurnExecution::completed(output, closure);
        if let Some(callback) = &self.callback {
            callback.send_complete(input_tokens, output_tokens);
        }
        execution
    }

    pub fn finish_handled_turn(
        &self,
        kind: impl Into<String>,
        input_tokens: usize,
        output_tokens: usize,
        parent_sequence: Option<u64>,
    ) -> TurnExecution {
        self.emit_turn_completed(input_tokens, output_tokens, parent_sequence);
        let execution = TurnExecution::handled(kind);
        if let Some(callback) = &self.callback {
            callback.send_complete(input_tokens, output_tokens);
        }
        execution
    }

    pub fn finish_aborted_turn(
        &self,
        error: TurnError,
        ledger_tail: PartialLedgerTail,
        parent_sequence: Option<u64>,
    ) -> TurnExecution {
        self.emit(
            OperationStage::Outcome,
            OperationEventKind::TurnAborted,
            OperationLevel::Warning,
            "turn aborted",
            serde_json::json!({
                "reason_code": error.reason_code,
                "last_safe_parent_event_id": ledger_tail.last_safe_parent_event_id,
            }),
            RedactionClass::Public,
            parent_sequence,
        );
        let execution = TurnExecution::aborted(error, ledger_tail);
        if let Some(callback) = &self.callback {
            callback.send_cancelled();
        }
        execution
    }

    pub fn transcript_hash(&self) -> String {
        let bus = self
            .bus
            .lock()
            .expect("wake operation recorder mutex poisoned");
        bus.transcript_hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    #[test]
    fn stream_event_preserves_operation_event_for_legacy_consumers() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread".to_string(),
            },
            4,
        );
        let event = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({"tool_name": "database_query"}),
            RedactionClass::PanelSafe,
            None,
        );

        let stream_event = StreamEvent::from_operation_event(event.clone());
        match stream_event {
            StreamEvent::Operation(op) => {
                assert_eq!(op.sequence, event.sequence);
                assert_eq!(op.kind, OperationEventKind::ToolCallVisible);
            }
            other => panic!("expected operation event, got {other:?}"),
        }
    }

    #[test]
    fn visible_tool_call_renders_safe_preview_before_execution() {
        let visible = crate::operation_stream::VisibleToolCall::new(
            "call-db-1",
            "database_query",
            "database",
            "inspect revenue rows",
            serde_json::json!({
                "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'",
                "bearer_token": "secret"
            }),
            "read_only",
            "approved",
            Some("policy-42".to_string()),
        );

        let event = ToolCallEvent::from_visible_tool_call(&visible);
        assert_eq!(event.id, "call-db-1");
        assert_eq!(event.name, "database_query");
        assert!(event
            .arguments
            .contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
        assert!(event.arguments.contains("[REDACTED]"));
        assert!(!event.arguments.contains("secret"));
    }

    #[test]
    fn stream_callback_owns_cancellation_state_and_typed_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let callback = StreamCallback::new(tx);

        assert!(!callback.is_cancelled());
        callback
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(callback.is_cancelled());

        callback.send_cancelled();
        assert!(matches!(rx.recv(), Ok(StreamEvent::Cancelled)));
    }

    #[test]
    fn wake_operation_recorder_emits_ordered_operation_events_to_callback() {
        let (tx, rx) = std::sync::mpsc::channel();
        let callback = StreamCallback::new(tx);
        let recorder = WakeOperationRecorder::new(
            crate::operation_stream::OperationContext {
                stream_id: "wake-stream-1".to_string(),
                turn_id: "turn-1".to_string(),
                principal_id: "did:key:wake".to_string(),
                channel_id: "api".to_string(),
                thread_id: "run-1".to_string(),
            },
            Some(callback),
            8,
        );

        let first = recorder.emit_status("provider calling");
        let visible = ToolCallEvent {
            id: "call-db-1".to_string(),
            name: "database_query".to_string(),
            arguments: "{\"sql\":\"SELECT region, revenue FROM sales WHERE quarter = 'Q2'\"}"
                .to_string(),
        };
        let second = recorder.emit_tool_visible(&visible, Some(first.sequence));

        let operations = rx
            .try_iter()
            .filter_map(|event| match event {
                StreamEvent::Operation(operation) => Some(operation),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].kind, OperationEventKind::ProviderCalling);
        assert_eq!(operations[1].kind, OperationEventKind::ToolCallVisible);
        assert_eq!(operations[1].parent_sequence, Some(1));
        assert!(operations[1]
            .payload
            .to_string()
            .contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
    }

    #[test]
    fn completed_turn_is_typed_before_legacy_complete_is_published() {
        let (tx, rx) = std::sync::mpsc::channel();
        let recorder = WakeOperationRecorder::new(
            OperationContext {
                stream_id: "wake-complete-1".to_string(),
                turn_id: "turn-1".to_string(),
                principal_id: "did:key:wake".to_string(),
                channel_id: "api".to_string(),
                thread_id: "run-1".to_string(),
            },
            Some(StreamCallback::new(tx)),
            8,
        );
        let started = recorder.emit_turn_started();
        let execution = recorder.finish_completed_turn(
            RuntimeOutput {
                runtime_owner: "TurnKernelEntry:wake".to_string(),
                runtime_topology: vec!["RuntimeOutput".to_string(), "ProofClosure".to_string()],
                provider_response_hash: "response-hash".to_string(),
                context_pack_id: "context-pack".to_string(),
                memory_atom_ids: Vec::new(),
                tool_receipt_ids: Vec::new(),
                stream_hash: String::new(),
            },
            ProofClosure::for_test(),
            10,
            20,
            Some(started.sequence),
        );

        let events = rx.try_iter().collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [
                StreamEvent::Operation(started_event),
                StreamEvent::Operation(completed_event),
                StreamEvent::Complete {
                    input_tokens: 10,
                    output_tokens: 20
                }
            ] if started_event.kind == OperationEventKind::TurnStarted
                && completed_event.kind == OperationEventKind::TurnCompleted
        ));
        assert!(matches!(
            execution.outcome(),
            Some(crate::TurnOutcome::Completed(_))
        ));
        assert!(execution
            .output()
            .is_some_and(|output| !output.stream_hash.is_empty()));
    }
}
