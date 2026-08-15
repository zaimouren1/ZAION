use zaion_runtime::operation_stream::OperationEvent;

pub fn render_telegram_operation_event(event: &OperationEvent) -> String {
    crate::commands::panel_render::render_operation_panel_event(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    #[test]
    fn telegram_panel_renders_visible_tool_call_with_safe_preview() {
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
        let event = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({
                "tool_name": "database_query",
                "input_preview": {"sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"}
            }),
            RedactionClass::PanelSafe,
            None,
        );

        let rendered = render_telegram_operation_event(&event);
        assert!(rendered.contains("database_query"));
        assert!(rendered.contains("执行中"));
        assert!(rendered.contains("│ → SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
        assert!(!rendered.contains("(running)"));
    }
}
