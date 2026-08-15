use crate::operation_stream::{OperationEvent, OperationEventKind};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFlushPolicy {
    Immediate,
    Throttled { min_interval_ms: u64 },
    FinalOnly,
}

#[derive(Debug, Error)]
pub enum PanelSinkError {
    #[error("panel sink delivery failed: {0}")]
    Delivery(String),
}

pub trait PanelSink {
    fn flush_policy(&self) -> StreamFlushPolicy;
    fn handle_event(&mut self, event: &OperationEvent) -> Result<(), PanelSinkError>;
}

#[derive(Debug, Default, Clone)]
pub struct TranscriptSink {
    events: Vec<OperationEvent>,
    visible: String,
}

impl TranscriptSink {
    pub fn events(&self) -> &[OperationEvent] {
        &self.events
    }

    pub fn visible_text(&self) -> &str {
        &self.visible
    }

    pub fn delivery_summary(&self) -> String {
        format!("events={}", self.events.len())
    }
}

impl PanelSink for TranscriptSink {
    fn flush_policy(&self) -> StreamFlushPolicy {
        StreamFlushPolicy::FinalOnly
    }

    fn handle_event(&mut self, event: &OperationEvent) -> Result<(), PanelSinkError> {
        if matches!(
            event.kind,
            OperationEventKind::ToolCallVisible
                | OperationEventKind::ToolProgress
                | OperationEventKind::TurnDegraded
                | OperationEventKind::TurnAborted
                | OperationEventKind::Quarantined
        ) {
            if !self.visible.is_empty() {
                self.visible.push('\n');
            }
            self.visible.push_str(&event.display_text);
        }
        self.events.push(event.clone());
        Ok(())
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
    fn transcript_sink_keeps_tool_visibility_and_final_hash() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "tui".to_string(),
                thread_id: "thread".to_string(),
            },
            16,
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

        let mut sink = TranscriptSink::default();
        sink.handle_event(&event).expect("sink event");

        assert!(sink.visible_text().contains("tool visible"));
        assert_eq!(sink.events().len(), 1);
        assert!(sink.delivery_summary().contains("events=1"));
    }

    #[test]
    fn transcript_sink_does_not_make_lifecycle_events_visible() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread".to_string(),
            },
            16,
        );
        let provider = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({"provider": "openai"}),
            RedactionClass::Public,
            None,
        );
        let done = bus.emit(
            OperationStage::Outcome,
            OperationEventKind::TurnCompleted,
            OperationLevel::Info,
            "turn completed",
            serde_json::json!({"tokens_in": 1, "tokens_out": 2}),
            RedactionClass::Public,
            Some(provider.sequence),
        );

        let mut sink = TranscriptSink::default();
        sink.handle_event(&provider).expect("provider event");
        sink.handle_event(&done).expect("done event");

        assert!(
            sink.visible_text().trim().is_empty(),
            "panel lifecycle events are observability-only and must not become chat reply text: {}",
            sink.visible_text()
        );
        assert_eq!(sink.events().len(), 2);
    }
}
