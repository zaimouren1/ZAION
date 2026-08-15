//! Real-time log streaming via WebSocket
//!
//! Bridges gateway events with runtime logs for live monitoring.

use crate::{EventType, GatewayState, ServerEvent};
use std::path::Path;
use std::sync::Arc;

/// Log level for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// Log entry for streaming
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub process_id: Option<String>,
    pub timestamp: i64,
    pub module: Option<String>,
}

impl LogEntry {
    pub fn new(level: LogLevel, message: String) -> Self {
        Self {
            level,
            message,
            process_id: None,
            timestamp: now_ms(),
            module: None,
        }
    }

    pub fn with_process(mut self, process_id: String) -> Self {
        self.process_id = Some(process_id);
        self
    }

    pub fn with_module(mut self, module: String) -> Self {
        self.module = Some(module);
        self
    }

    /// Convert to ServerEvent for WebSocket broadcast
    pub fn to_event(&self) -> ServerEvent {
        ServerEvent {
            event_type: EventType::Message,
            process_id: self.process_id.clone(),
            payload: serde_json::json!({
                "log_level": self.level.as_str(),
                "message": self.message,
                "module": self.module,
            }),
            ts: self.timestamp,
        }
    }
}

/// Log streamer that broadcasts to gateway
pub struct LogStreamer {
    state: Arc<GatewayState>,
    min_level: LogLevel,
}

impl LogStreamer {
    pub fn new(state: Arc<GatewayState>, min_level: LogLevel) -> Self {
        Self { state, min_level }
    }

    /// Stream a log entry if it meets the level threshold
    pub fn log(&self, entry: LogEntry) {
        if self.should_stream(&entry) {
            self.state.broadcast(entry.to_event());
        }
    }

    fn should_stream(&self, entry: &LogEntry) -> bool {
        let entry_priority = match entry.level {
            LogLevel::Debug => 0,
            LogLevel::Info => 1,
            LogLevel::Warn => 2,
            LogLevel::Error => 3,
        };

        let min_priority = match self.min_level {
            LogLevel::Debug => 0,
            LogLevel::Info => 1,
            LogLevel::Warn => 2,
            LogLevel::Error => 3,
        };

        entry_priority >= min_priority
    }

    /// Stream debug log
    pub fn debug(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Debug, message.into()));
    }

    /// Stream info log
    pub fn info(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Info, message.into()));
    }

    /// Stream warn log
    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Warn, message.into()));
    }

    /// Stream error log
    pub fn error(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Error, message.into()));
    }
}

/// Status update for real-time monitoring
#[derive(Debug, Clone)]
pub struct StatusUpdate {
    pub process_id: String,
    pub status: ProcessStatus,
    pub timestamp: i64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Starting,
    Running,
    Idle,
    Thinking,
    Sleeping,
    Crashed,
    Stopped,
}

impl ProcessStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessStatus::Starting => "starting",
            ProcessStatus::Running => "running",
            ProcessStatus::Idle => "idle",
            ProcessStatus::Thinking => "thinking",
            ProcessStatus::Sleeping => "sleeping",
            ProcessStatus::Crashed => "crashed",
            ProcessStatus::Stopped => "stopped",
        }
    }
}

impl StatusUpdate {
    pub fn new(process_id: String, status: ProcessStatus) -> Self {
        Self {
            process_id,
            status,
            timestamp: now_ms(),
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Convert to ServerEvent for WebSocket broadcast
    pub fn to_event(&self) -> ServerEvent {
        ServerEvent {
            event_type: EventType::StateChange,
            process_id: Some(self.process_id.clone()),
            payload: serde_json::json!({
                "status": self.status.as_str(),
                "metadata": self.metadata,
            }),
            ts: self.timestamp,
        }
    }
}

/// Status streamer for process monitoring
pub struct StatusStreamer {
    state: Arc<GatewayState>,
}

impl StatusStreamer {
    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }

    /// Broadcast status update
    pub fn update(&self, status: StatusUpdate) {
        self.state.broadcast(status.to_event());
    }

    /// Broadcast process list
    pub fn broadcast_process_list(&self, processes: Vec<ProcessInfo>) {
        let event = ServerEvent {
            event_type: EventType::ProcessList,
            process_id: None,
            payload: serde_json::json!({ "processes": processes }),
            ts: now_ms(),
        };
        self.state.broadcast(event);
    }
}

/// Process info for process list broadcasts
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessInfo {
    pub pid: String,
    pub status: String,
    pub name: String,
    pub started_at: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// File watcher for log tailing
pub struct LogTailer {
    streamer: LogStreamer,
}

impl LogTailer {
    pub fn new(state: Arc<GatewayState>, min_level: LogLevel) -> Self {
        Self {
            streamer: LogStreamer::new(state, min_level),
        }
    }

    /// Tail a log file and stream new lines
    pub fn tail_file(&self, path: &Path, process_id: Option<String>) -> std::io::Result<()> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        use std::io::BufRead;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            // Parse log level from line (simple heuristic)
            let level = if line.contains("ERROR") || line.contains("[error]") {
                LogLevel::Error
            } else if line.contains("WARN") || line.contains("[warn]") {
                LogLevel::Warn
            } else if line.contains("DEBUG") || line.contains("[debug]") {
                LogLevel::Debug
            } else {
                LogLevel::Info
            };

            let mut entry = LogEntry::new(level, line);
            if let Some(ref pid) = process_id {
                entry = entry.with_process(pid.clone());
            }

            self.streamer.log(entry);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_creation() {
        let entry = LogEntry::new(LogLevel::Info, "test message".to_string());
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "test message");
        assert!(entry.process_id.is_none());
    }

    #[test]
    fn test_log_entry_with_process() {
        let entry = LogEntry::new(LogLevel::Warn, "warning".to_string())
            .with_process("pid-123".to_string());
        assert_eq!(entry.process_id, Some("pid-123".to_string()));
    }

    #[test]
    fn test_log_entry_to_event() {
        let entry = LogEntry::new(LogLevel::Error, "error occurred".to_string())
            .with_process("pid-456".to_string())
            .with_module("runtime".to_string());

        let event = entry.to_event();
        assert_eq!(event.event_type, EventType::Message);
        assert_eq!(event.process_id, Some("pid-456".to_string()));
        assert_eq!(event.payload["log_level"], "error");
        assert_eq!(event.payload["message"], "error occurred");
        assert_eq!(event.payload["module"], "runtime");
    }

    #[test]
    fn test_log_streamer_filtering() {
        let state = Arc::new(GatewayState::new("".to_string()));
        let streamer = LogStreamer::new(state.clone(), LogLevel::Warn);

        // Debug and Info should be filtered out
        let debug_entry = LogEntry::new(LogLevel::Debug, "debug".to_string());
        assert!(!streamer.should_stream(&debug_entry));

        let info_entry = LogEntry::new(LogLevel::Info, "info".to_string());
        assert!(!streamer.should_stream(&info_entry));

        // Warn and Error should pass through
        let warn_entry = LogEntry::new(LogLevel::Warn, "warn".to_string());
        assert!(streamer.should_stream(&warn_entry));

        let error_entry = LogEntry::new(LogLevel::Error, "error".to_string());
        assert!(streamer.should_stream(&error_entry));
    }

    #[test]
    fn test_status_update_creation() {
        let update = StatusUpdate::new("pid-789".to_string(), ProcessStatus::Running);
        assert_eq!(update.process_id, "pid-789");
        assert_eq!(update.status, ProcessStatus::Running);
    }

    #[test]
    fn test_status_update_to_event() {
        let update = StatusUpdate::new("pid-abc".to_string(), ProcessStatus::Idle)
            .with_metadata(serde_json::json!({"reason": "waiting for input"}));

        let event = update.to_event();
        assert_eq!(event.event_type, EventType::StateChange);
        assert_eq!(event.process_id, Some("pid-abc".to_string()));
        assert_eq!(event.payload["status"], "idle");
        assert_eq!(event.payload["metadata"]["reason"], "waiting for input");
    }

    #[test]
    fn test_process_status_as_str() {
        assert_eq!(ProcessStatus::Starting.as_str(), "starting");
        assert_eq!(ProcessStatus::Running.as_str(), "running");
        assert_eq!(ProcessStatus::Idle.as_str(), "idle");
        assert_eq!(ProcessStatus::Thinking.as_str(), "thinking");
        assert_eq!(ProcessStatus::Sleeping.as_str(), "sleeping");
        assert_eq!(ProcessStatus::Crashed.as_str(), "crashed");
        assert_eq!(ProcessStatus::Stopped.as_str(), "stopped");
    }

    #[test]
    fn test_status_streamer_broadcast_process_list() {
        let state = Arc::new(GatewayState::new("".to_string()));
        let streamer = StatusStreamer::new(state.clone());

        let processes = vec![
            ProcessInfo {
                pid: "p1".to_string(),
                status: "running".to_string(),
                name: "agent-1".to_string(),
                started_at: 1234567890,
            },
            ProcessInfo {
                pid: "p2".to_string(),
                status: "idle".to_string(),
                name: "agent-2".to_string(),
                started_at: 1234567900,
            },
        ];

        streamer.broadcast_process_list(processes);
        // No panic = success
    }
}
