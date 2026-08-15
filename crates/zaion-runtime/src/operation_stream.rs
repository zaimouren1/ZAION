use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationContext {
    pub stream_id: String,
    pub turn_id: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationStage {
    Ingress,
    Identity,
    Policy,
    Context,
    Reasoning,
    Tool,
    Ledger,
    Proof,
    Outcome,
    Safety,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationEventKind {
    TurnStarted,
    IngressAccepted,
    IdentityVerified,
    PolicyChecked,
    ContextCompiling,
    ContextCompiled,
    ProviderCalling,
    TokenDelta,
    ActionIntentDetected,
    ToolCallVisible,
    ToolProgress,
    ToolReceiptProduced,
    LedgerEventAppended,
    ProofClosing,
    TurnCompleted,
    TurnDegraded,
    TurnAborted,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationLevel {
    Trace,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RedactionClass {
    Public,
    PanelSafe,
    Sensitive,
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationEvent {
    pub stream_id: String,
    pub turn_id: String,
    pub sequence: u64,
    pub timestamp: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub stage: OperationStage,
    pub kind: OperationEventKind,
    pub level: OperationLevel,
    pub display_text: String,
    pub payload: Value,
    pub redaction_class: RedactionClass,
    pub ledger_event_id: Option<String>,
    pub proof_hash: Option<String>,
    pub parent_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStreamCursor {
    pub stream_id: String,
    pub sequence: u64,
}

impl OperationStreamCursor {
    pub fn new(stream_id: impl Into<String>, sequence: u64) -> Self {
        Self {
            stream_id: stream_id.into(),
            sequence,
        }
    }

    pub fn parse(cursor: &str) -> Option<Self> {
        let rest = cursor.strip_prefix("operation:")?;
        let (stream_id, sequence) = rest.rsplit_once(':')?;
        if stream_id.is_empty() {
            return None;
        }
        Some(Self::new(stream_id, sequence.parse().ok()?))
    }

    pub fn to_sse_id(&self) -> String {
        format!("operation:{}:{}", self.stream_id, self.sequence)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisibleToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: String,
    pub purpose: String,
    pub input_preview: Value,
    pub safety_class: String,
    pub permission_state: String,
    pub policy_decision_id: Option<String>,
}

impl VisibleToolCall {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        tool_kind: impl Into<String>,
        purpose: impl Into<String>,
        input_preview: Value,
        safety_class: impl Into<String>,
        permission_state: impl Into<String>,
        policy_decision_id: Option<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            tool_name: tool_name.into(),
            tool_kind: tool_kind.into(),
            purpose: purpose.into(),
            input_preview,
            safety_class: safety_class.into(),
            permission_state: permission_state.into(),
            policy_decision_id,
        }
    }

    pub fn redacted_for_panel(mut self) -> Self {
        self.input_preview = redact_json_value(self.input_preview);
        self
    }
}

#[derive(Debug, Clone)]
pub struct OperationStreamBacklog {
    capacity: usize,
    events: VecDeque<OperationEvent>,
}

impl OperationStreamBacklog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            events: VecDeque::new(),
        }
    }

    pub fn append(&mut self, event: OperationEvent) {
        self.events.push_back(event);
        while self.events.len() > self.capacity {
            self.events.pop_front();
        }
    }

    pub fn cursor_for(&self, event: &OperationEvent) -> String {
        OperationStreamCursor::new(event.stream_id.clone(), event.sequence).to_sse_id()
    }

    pub fn replay_after(&self, cursor: Option<&str>) -> Vec<OperationEvent> {
        let Some(cursor) = cursor.and_then(OperationStreamCursor::parse) else {
            return self.events.iter().cloned().collect();
        };
        self.events
            .iter()
            .filter(|event| event.stream_id == cursor.stream_id && event.sequence > cursor.sequence)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct OperationStreamBus {
    context: OperationContext,
    next_sequence: u64,
    capacity: usize,
    replay: VecDeque<OperationEvent>,
    transcript: Vec<OperationEvent>,
}

impl OperationStreamBus {
    pub fn new(context: OperationContext, replay_capacity: usize) -> Self {
        Self {
            context,
            next_sequence: 1,
            capacity: replay_capacity.max(1),
            replay: VecDeque::new(),
            transcript: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        stage: OperationStage,
        kind: OperationEventKind,
        level: OperationLevel,
        display_text: impl Into<String>,
        payload: Value,
        redaction_class: RedactionClass,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        let event = OperationEvent {
            stream_id: self.context.stream_id.clone(),
            turn_id: self.context.turn_id.clone(),
            sequence: self.next_sequence,
            timestamp: Utc::now().to_rfc3339(),
            principal_id: self.context.principal_id.clone(),
            channel_id: self.context.channel_id.clone(),
            thread_id: self.context.thread_id.clone(),
            stage,
            kind,
            level,
            display_text: display_text.into(),
            payload: redact_json_value(payload),
            redaction_class,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence,
        };
        self.next_sequence += 1;
        self.replay.push_back(event.clone());
        while self.replay.len() > self.capacity {
            self.replay.pop_front();
        }
        self.transcript.push(event.clone());
        event
    }

    pub fn replay_from(&self, sequence: u64) -> Vec<OperationEvent> {
        self.replay
            .iter()
            .filter(|event| event.sequence >= sequence)
            .cloned()
            .collect()
    }

    pub fn transcript_hash(&self) -> String {
        let bytes = serde_json::to_vec(&self.transcript).unwrap_or_default();
        let digest = Sha256::digest(bytes);
        format!("sha256:{}", hex::encode(digest))
    }
}

fn redact_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if lower.contains("key")
                        || lower.contains("token")
                        || lower.contains("secret")
                        || lower.contains("password")
                        || lower.contains("cookie")
                        || lower.contains("bearer")
                    {
                        (key, Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_json_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_context() -> OperationContext {
        OperationContext {
            stream_id: "stream-test".to_string(),
            turn_id: "turn-test".to_string(),
            principal_id: "did:key:test".to_string(),
            channel_id: "telegram".to_string(),
            thread_id: "thread-1".to_string(),
        }
    }

    #[test]
    fn bus_assigns_monotonic_sequences_and_hashes_transcript() {
        let mut bus = OperationStreamBus::new(base_context(), 8);
        let first = bus.emit(
            OperationStage::Ingress,
            OperationEventKind::IngressAccepted,
            OperationLevel::Info,
            "ingress accepted",
            serde_json::json!({"source": "telegram"}),
            RedactionClass::Public,
            None,
        );
        let second = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({"provider": "ollama"}),
            RedactionClass::Public,
            Some(first.sequence),
        );

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.parent_sequence, Some(1));
        assert_eq!(bus.replay_from(1).len(), 2);
        assert!(bus.transcript_hash().starts_with("sha256:"));
    }

    #[test]
    fn visible_tool_call_redacts_secret_preview() {
        let visible = VisibleToolCall::new(
            "call-1",
            "database_query",
            "database",
            "read quarterly revenue",
            serde_json::json!({
                "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'",
                "api_key": "sk-secret"
            }),
            "read_only",
            "approved",
            Some("policy-1".to_string()),
        )
        .redacted_for_panel();

        assert_eq!(visible.tool_name, "database_query");
        assert_eq!(
            visible.input_preview["api_key"],
            serde_json::json!("[REDACTED]")
        );
        assert_eq!(
            visible.input_preview["sql"],
            serde_json::json!("SELECT region, revenue FROM sales WHERE quarter = 'Q2'")
        );
    }

    #[test]
    fn operation_backlog_replays_events_after_cursor() {
        let mut bus = OperationStreamBus::new(base_context(), 8);
        let first = bus.emit(
            OperationStage::Ingress,
            OperationEventKind::IngressAccepted,
            OperationLevel::Info,
            "ingress accepted",
            serde_json::json!({"source": "telegram"}),
            RedactionClass::Public,
            None,
        );
        let second = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({"tool_name": "database_query"}),
            RedactionClass::PanelSafe,
            Some(first.sequence),
        );

        let mut backlog = OperationStreamBacklog::new(8);
        backlog.append(first.clone());
        backlog.append(second.clone());

        assert_eq!(backlog.cursor_for(&second), "operation:stream-test:2");
        let replayed = backlog.replay_after(Some(&backlog.cursor_for(&first)));

        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].sequence, second.sequence);
        assert_eq!(replayed[0].stream_id, "stream-test");
    }

    #[test]
    fn operation_backlog_is_bounded_without_reordering_events() {
        let mut bus = OperationStreamBus::new(base_context(), 8);
        let first = bus.emit(
            OperationStage::Ingress,
            OperationEventKind::IngressAccepted,
            OperationLevel::Info,
            "ingress accepted",
            serde_json::json!({}),
            RedactionClass::Public,
            None,
        );
        let second = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({}),
            RedactionClass::Public,
            Some(first.sequence),
        );
        let third = bus.emit(
            OperationStage::Outcome,
            OperationEventKind::TurnCompleted,
            OperationLevel::Info,
            "turn completed",
            serde_json::json!({}),
            RedactionClass::Public,
            Some(second.sequence),
        );

        let mut backlog = OperationStreamBacklog::new(2);
        backlog.append(first);
        backlog.append(second);
        backlog.append(third);

        let replayed = backlog.replay_after(None);
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].sequence, 2);
        assert_eq!(replayed[1].sequence, 3);
    }
}
