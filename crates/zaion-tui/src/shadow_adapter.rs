//! Shadow Event Adapter - Converts shadow events to rendering instructions
//!
//! This adapter bridges the shadow runtime and the streaming renderer,
//! translating low-level shadow events into high-level UI updates.

use crate::streaming_renderer::{StreamingRenderer, ToolCallStatus};
use crate::theme::ThemeName;
use std::collections::HashMap;
use std::time::Instant;
use zaion_shadow::{ShadowEvent, TaskId};

/// Tracks active tool calls and their states
#[derive(Debug)]
struct ToolCallTracker {
    name: String,
    started_at: Instant,
    status: ToolCallState,
}

#[derive(Debug, Clone, PartialEq)]
enum ToolCallState {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Shadow event adapter for real-time rendering
pub struct ShadowEventAdapter {
    renderer: StreamingRenderer,
    active_tools: HashMap<TaskId, ToolCallTracker>,
    thinking_step_count: usize,
    executor_active: bool,
}

impl ShadowEventAdapter {
    pub fn new() -> Self {
        Self {
            renderer: StreamingRenderer::new(),
            active_tools: HashMap::new(),
            thinking_step_count: 0,
            executor_active: false,
        }
    }

    /// Create adapter with a specific theme
    pub fn with_theme(theme_name: ThemeName) -> Self {
        Self {
            renderer: StreamingRenderer::with_theme(theme_name),
            active_tools: HashMap::new(),
            thinking_step_count: 0,
            executor_active: false,
        }
    }

    /// Get mutable reference to renderer
    pub fn renderer_mut(&mut self) -> &mut StreamingRenderer {
        &mut self.renderer
    }

    /// Process a shadow event and update the UI
    pub fn handle_event(&mut self, event: ShadowEvent) -> std::io::Result<()> {
        match event {
            ShadowEvent::ExecutorStarted => {
                self.executor_active = true;
                self.renderer.section_header("Agent Active")?;
            }

            ShadowEvent::ExecutorStopped => {
                self.executor_active = false;
                // Don't render anything - let the conversation continue
            }

            ShadowEvent::TaskSpawned { task_id, name } => {
                // Track the task
                self.active_tools.insert(
                    task_id,
                    ToolCallTracker {
                        name: name.clone(),
                        started_at: Instant::now(),
                        status: ToolCallState::Queued,
                    },
                );

                // Render queued status (optional - can be removed for cleaner output)
                // self.renderer.tool_call_status(&name, ToolCallStatus::Running)?;
            }

            ShadowEvent::TaskStarted { task_id, name } => {
                // Update tracker
                if let Some(tracker) = self.active_tools.get_mut(&task_id) {
                    tracker.status = ToolCallState::Running;
                    tracker.started_at = Instant::now();
                }

                // Render running status
                self.renderer
                    .tool_call_status(&name, ToolCallStatus::Running)?;
            }

            ShadowEvent::TaskCompleted {
                task_id,
                name,
                success,
                duration_ms,
            } => {
                // Update tracker
                if let Some(tracker) = self.active_tools.get_mut(&task_id) {
                    tracker.status = if success {
                        ToolCallState::Completed
                    } else {
                        ToolCallState::Failed
                    };
                }

                // Render completion status
                if success {
                    self.renderer
                        .tool_call_status(&name, ToolCallStatus::Success(duration_ms))?;
                } else {
                    self.renderer.tool_call_status(
                        &name,
                        ToolCallStatus::Failed("Task failed".to_string()),
                    )?;
                }

                // Remove from active tracking after a delay
                // (keep in memory for potential queries)
                // self.active_tools.remove(&task_id);
            }

            ShadowEvent::TaskCancelled { task_id } => {
                // Get task name before removing
                if let Some(tracker) = self.active_tools.remove(&task_id) {
                    self.renderer.tool_call_status(
                        &tracker.name,
                        ToolCallStatus::Failed("Cancelled".to_string()),
                    )?;
                }
            }

            ShadowEvent::AciOperation { op, ok, .. } => {
                // ACI operations can be rendered as tool calls
                let status = if ok {
                    ToolCallStatus::Success(0) // Duration unknown
                } else {
                    ToolCallStatus::Failed("ACI operation failed".to_string())
                };
                self.renderer
                    .tool_call_status(&format!("ACI: {}", op), status)?;
            }
        }

        Ok(())
    }

    /// Add a thinking step (from LLM reasoning)
    pub fn add_thinking_step(&mut self, content: &str, total: usize) -> std::io::Result<()> {
        self.thinking_step_count += 1;
        self.renderer
            .thinking_step(content, self.thinking_step_count, total)?;
        Ok(())
    }

    /// Reset thinking step counter
    pub fn reset_thinking(&mut self) {
        self.thinking_step_count = 0;
    }

    /// Get active tool call count
    pub fn active_tool_count(&self) -> usize {
        self.active_tools
            .values()
            .filter(|t| t.status == ToolCallState::Running)
            .count()
    }

    /// Get completed tool call count
    pub fn completed_tool_count(&self) -> usize {
        self.active_tools
            .values()
            .filter(|t| t.status == ToolCallState::Completed)
            .count()
    }

    /// Get failed tool call count
    pub fn failed_tool_count(&self) -> usize {
        self.active_tools
            .values()
            .filter(|t| t.status == ToolCallState::Failed)
            .count()
    }

    /// Clear all tracked tool calls
    pub fn clear_tool_tracking(&mut self) {
        self.active_tools.clear();
    }
}

/// Poll shadow events from a broadcast receiver with a callback handler
/// Returns true if the channel is still open, false if closed
pub fn poll_shadow_events<F>(
    shadow_rx: &mut Option<zaion_shadow::ShadowEventRx>,
    mut handler: F,
) -> bool
where
    F: FnMut(ShadowEvent),
{
    if let Some(ref mut rx) = shadow_rx {
        loop {
            match rx.try_recv() {
                Ok(event) => handler(event),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    return true; // Channel still open, no more events
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    *shadow_rx = None;
                    return false; // Channel closed
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                    // Events were dropped, continue polling
                    continue;
                }
            }
        }
    }
    false // No channel
}

impl Default for ShadowEventAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_creation() {
        let adapter = ShadowEventAdapter::new();
        assert_eq!(adapter.active_tool_count(), 0);
        assert_eq!(adapter.thinking_step_count, 0);
        assert!(!adapter.executor_active);
    }

    #[test]
    fn test_task_lifecycle() {
        let mut adapter = ShadowEventAdapter::new();
        let task_id = TaskId::new_v4();

        // Spawn task
        adapter
            .handle_event(ShadowEvent::TaskSpawned {
                task_id,
                name: "read_file".to_string(),
            })
            .unwrap();

        assert_eq!(adapter.active_tools.len(), 1);

        // Start task
        adapter
            .handle_event(ShadowEvent::TaskStarted {
                task_id,
                name: "read_file".to_string(),
            })
            .unwrap();

        assert_eq!(adapter.active_tool_count(), 1);

        // Complete task
        adapter
            .handle_event(ShadowEvent::TaskCompleted {
                task_id,
                name: "read_file".to_string(),
                success: true,
                duration_ms: 150,
            })
            .unwrap();

        assert_eq!(adapter.active_tool_count(), 0);
        assert_eq!(adapter.completed_tool_count(), 1);
    }

    #[test]
    fn test_thinking_steps() {
        let mut adapter = ShadowEventAdapter::new();

        adapter.add_thinking_step("Step 1", 3).unwrap();
        assert_eq!(adapter.thinking_step_count, 1);

        adapter.add_thinking_step("Step 2", 3).unwrap();
        assert_eq!(adapter.thinking_step_count, 2);

        adapter.reset_thinking();
        assert_eq!(adapter.thinking_step_count, 0);
    }

    #[test]
    fn test_executor_lifecycle() {
        let mut adapter = ShadowEventAdapter::new();

        adapter.handle_event(ShadowEvent::ExecutorStarted).unwrap();
        assert!(adapter.executor_active);

        adapter.handle_event(ShadowEvent::ExecutorStopped).unwrap();
        assert!(!adapter.executor_active);
    }

    #[test]
    fn test_task_cancellation() {
        let mut adapter = ShadowEventAdapter::new();
        let task_id = TaskId::new_v4();

        adapter
            .handle_event(ShadowEvent::TaskSpawned {
                task_id,
                name: "long_operation".to_string(),
            })
            .unwrap();

        adapter
            .handle_event(ShadowEvent::TaskStarted {
                task_id,
                name: "long_operation".to_string(),
            })
            .unwrap();

        assert_eq!(adapter.active_tool_count(), 1);

        adapter
            .handle_event(ShadowEvent::TaskCancelled { task_id })
            .unwrap();

        assert_eq!(adapter.active_tool_count(), 0);
        assert_eq!(adapter.active_tools.len(), 0);
    }

    #[test]
    fn test_multiple_concurrent_tasks() {
        let mut adapter = ShadowEventAdapter::new();

        // Spawn multiple tasks
        let task_ids: Vec<TaskId> = (1..=5).map(|_| TaskId::new_v4()).collect();

        for (i, &task_id) in task_ids.iter().enumerate() {
            adapter
                .handle_event(ShadowEvent::TaskSpawned {
                    task_id,
                    name: format!("operation_{}", i + 1),
                })
                .unwrap();

            adapter
                .handle_event(ShadowEvent::TaskStarted {
                    task_id,
                    name: format!("operation_{}", i + 1),
                })
                .unwrap();
        }

        assert_eq!(adapter.active_tool_count(), 5);

        // Complete some tasks
        adapter
            .handle_event(ShadowEvent::TaskCompleted {
                task_id: task_ids[0],
                name: "operation_1".to_string(),
                success: true,
                duration_ms: 100,
            })
            .unwrap();

        adapter
            .handle_event(ShadowEvent::TaskCompleted {
                task_id: task_ids[1],
                name: "operation_2".to_string(),
                success: false,
                duration_ms: 50,
            })
            .unwrap();

        assert_eq!(adapter.active_tool_count(), 3);
        assert_eq!(adapter.completed_tool_count(), 1);
        assert_eq!(adapter.failed_tool_count(), 1);
    }
}
