use serde_json::Value;
use zaion_runtime::operation_stream::{OperationEvent, OperationEventKind};

pub fn render_operation_panel_event(event: &OperationEvent) -> String {
    match event.kind {
        OperationEventKind::ToolCallVisible => {
            let tool_name = event
                .payload
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let preview = event
                .payload
                .get("input_preview")
                .map(render_preview)
                .unwrap_or_else(|| "{}".to_string());
            format!("🛠️ {tool_name} (执行中...)\n│ → {preview}")
        }
        OperationEventKind::ToolProgress => format!("🛠️ {} (进行中...)", event.display_text),
        OperationEventKind::ToolReceiptProduced => format!("✅ {} (已完成)", event.display_text),
        OperationEventKind::TurnDegraded => format!("⚠️ {} (降级)", event.display_text),
        OperationEventKind::TurnAborted => format!("⛔ {} (已中止)", event.display_text),
        OperationEventKind::Quarantined => format!("🔒 {} (隔离)", event.display_text),
        _ => String::new(),
    }
}

fn render_preview(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(map) if map.len() == 1 => map
            .values()
            .next()
            .map(render_preview)
            .unwrap_or_else(|| "{}".to_string()),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| "{}".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    fn tool_event(input_preview: serde_json::Value) -> OperationEvent {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "stream-panel".to_string(),
                turn_id: "turn-panel".to_string(),
                principal_id: "did:key:panel".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread-panel".to_string(),
            },
            8,
        );
        bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool database_query visible",
            serde_json::json!({
                "tool_name": "database_query",
                "input_preview": input_preview,
            }),
            RedactionClass::PanelSafe,
            None,
        )
    }

    #[test]
    fn panel_render_visible_tool_call_uses_chinese_running_status_and_preview() {
        let rendered = render_operation_panel_event(&tool_event(serde_json::json!({
            "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"
        })));

        assert!(rendered.contains("🛠️ database_query (执行中...)"));
        assert!(rendered.contains("│ → SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
        assert!(!rendered.contains("(running)"));
    }

    #[test]
    fn panel_render_visible_tool_call_keeps_multi_field_preview_structured() {
        let rendered = render_operation_panel_event(&tool_event(serde_json::json!({
            "sql": "SELECT region FROM sales",
            "limit": 5
        })));

        assert!(rendered.contains("🛠️ database_query (执行中...)"));
        assert!(rendered.contains("\"sql\""));
        assert!(rendered.contains("\"limit\""));
    }

    #[test]
    fn panel_render_suppresses_lifecycle_events_for_chat_surfaces() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "stream-panel".to_string(),
                turn_id: "turn-panel".to_string(),
                principal_id: "did:key:panel".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread-panel".to_string(),
            },
            8,
        );
        let event = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({"provider": "openai"}),
            RedactionClass::Public,
            None,
        );

        assert_eq!(
            render_operation_panel_event(&event),
            "",
            "telegram/TUI chat surfaces must never turn lifecycle operations into visible replies"
        );
    }
}
