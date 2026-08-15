use tokio::sync::mpsc;
use zaion_adapters::AguiEvent;

pub type StreamTx = mpsc::UnboundedSender<AguiEvent>;

#[derive(Debug, Clone)]
pub struct StreamingResponse {
    pub id: String,
    pub tx: StreamTx,
}

impl StreamingResponse {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<AguiEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = format!("stream-{}", uuid::Uuid::new_v4());
        (StreamingResponse { id, tx }, rx)
    }

    pub fn send_text(&self, delta: &str) -> Result<(), mpsc::error::SendError<AguiEvent>> {
        self.tx.send(AguiEvent::TextMessageContent {
            message_id: self.id.clone(),
            delta: delta.to_string(),
        })
    }

    pub fn send_tool_call_start(
        &self,
        tool_name: &str,
    ) -> Result<(), mpsc::error::SendError<AguiEvent>> {
        let tool_call_id = format!("tool-{}", uuid::Uuid::new_v4());
        self.tx.send(AguiEvent::ToolCallStart {
            tool_call_id,
            tool_name: tool_name.to_string(),
        })
    }

    pub fn send_state_snapshot(
        &self,
        state: serde_json::Value,
    ) -> Result<(), mpsc::error::SendError<AguiEvent>> {
        self.tx.send(AguiEvent::StateSnapshot { state })
    }
}

#[derive(Debug, Clone)]
pub struct StreamCollector {
    pub buffer: String,
    pub message_id: String,
}

impl StreamCollector {
    pub fn new(message_id: String) -> Self {
        Self {
            buffer: String::new(),
            message_id,
        }
    }

    pub fn add_token(&mut self, token: &str) {
        self.buffer.push_str(token);
    }

    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }
}
