//! Trajectory data structures for OPD training
//!
//! A trajectory represents a complete agent interaction session,
//! including all messages, tool calls, and results.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A complete agent trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    /// Unique trajectory ID
    pub id: String,

    /// Task prompt that initiated this trajectory
    pub task: String,

    /// All messages in the conversation
    pub messages: Vec<TrajectoryMessage>,

    /// Tool usage statistics
    pub tool_stats: HashMap<String, ToolUsage>,

    /// Total tokens used
    pub total_tokens: u64,

    /// Whether the task was completed successfully
    pub success: bool,

    /// Timestamp when trajectory was created
    pub timestamp: i64,
}

/// A single message in a trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryMessage {
    /// Role: "user", "assistant", or "tool"
    pub role: String,

    /// Message content
    pub content: String,

    /// Tool calls (if role is "assistant")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// Tool call ID (if role is "tool")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call made by the assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call
    pub id: String,

    /// Tool name
    pub name: String,

    /// Tool arguments (JSON string)
    pub arguments: String,
}

/// Tool usage statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolUsage {
    /// Number of times this tool was called
    pub count: u32,

    /// Number of successful calls
    pub success: u32,

    /// Number of failed calls
    pub failure: u32,
}

/// Tool result from execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool call ID this result corresponds to
    pub tool_call_id: String,

    /// Result content
    pub content: String,

    /// Whether the tool call succeeded
    pub success: bool,
}

impl Trajectory {
    /// Create a new trajectory
    pub fn new(id: String, task: String) -> Self {
        Self {
            id,
            task,
            messages: Vec::new(),
            tool_stats: HashMap::new(),
            total_tokens: 0,
            success: false,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Add a message to the trajectory
    pub fn add_message(&mut self, message: TrajectoryMessage) {
        self.messages.push(message);
    }

    /// Update tool statistics
    pub fn update_tool_stats(&mut self, tool_name: String, success: bool) {
        let stats = self.tool_stats.entry(tool_name).or_default();
        stats.count += 1;
        if success {
            stats.success += 1;
        } else {
            stats.failure += 1;
        }
    }

    /// Export to ShareGPT format (compatible with HuggingFace datasets)
    pub fn to_sharegpt(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "conversations": self.messages.iter().map(|m| {
                serde_json::json!({
                    "from": match m.role.as_str() {
                        "user" => "human",
                        "assistant" => "gpt",
                        "tool" => "system",
                        _ => "unknown",
                    },
                    "value": m.content,
                })
            }).collect::<Vec<_>>(),
            "tool_stats": self.tool_stats,
            "success": self.success,
        })
    }
}

// Add chrono dependency for timestamp
use chrono;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trajectory_creation() {
        let traj = Trajectory::new("test-1".to_string(), "Write a function".to_string());
        assert_eq!(traj.id, "test-1");
        assert_eq!(traj.task, "Write a function");
        assert_eq!(traj.messages.len(), 0);
    }

    #[test]
    fn test_add_message() {
        let mut traj = Trajectory::new("test-1".to_string(), "Task".to_string());
        traj.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        assert_eq!(traj.messages.len(), 1);
    }

    #[test]
    fn test_tool_stats() {
        let mut traj = Trajectory::new("test-1".to_string(), "Task".to_string());
        traj.update_tool_stats("read_file".to_string(), true);
        traj.update_tool_stats("read_file".to_string(), false);

        let stats = traj.tool_stats.get("read_file").unwrap();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.success, 1);
        assert_eq!(stats.failure, 1);
    }

    #[test]
    fn test_sharegpt_export() {
        let mut traj = Trajectory::new("test-1".to_string(), "Task".to_string());
        traj.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let sharegpt = traj.to_sharegpt();
        assert_eq!(sharegpt["id"], "test-1");
        assert!(sharegpt["conversations"].is_array());
    }
}
