use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AguiEvent {
    RunStarted {
        run_id: String,
        agent_id: String,
    },
    TextMessageStart {
        message_id: String,
    },
    TextMessageContent {
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        message_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_name: String,
    },
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    StateSnapshot {
        state: serde_json::Value,
    },
    StateDelta {
        delta: serde_json::Value,
    },
    RunFinished {
        run_id: String,
    },
    RunError {
        run_id: String,
        message: String,
    },
}

impl AguiEvent {
    pub fn to_sse(&self) -> String {
        let data = serde_json::to_string(self).unwrap_or_default();
        format!("data: {}\n\n", data)
    }
}
