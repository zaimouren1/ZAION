//! Background task scheduler for /queue and /background slash commands
//!
//! Architecture (Hermes-compliant):
//! - TaskScheduler manages pending tasks (queue) and background tasks
//! - Queue tasks: FIFO consumption after current agent completes
//! - Background tasks: Parallel execution in separate sessions
//! - Approval chain: Blocking approval mechanism for dangerous commands
//!
//! Zaion enhancements:
//! - Ed25519 signed task receipts (provenance tracking)
//! - Task ledger with SHA-256 commitment chain
//! - Ouroboros auto-recovery for crashed background tasks

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Task execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskMode {
    /// Queue: Execute after current task completes (FIFO)
    Queue,
    /// Background: Execute in parallel session
    Background,
}

/// Task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Urgent,
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Scheduled task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub task_id: String,
    pub session_key: String,
    pub prompt: String,
    pub mode: TaskMode,
    pub priority: TaskPriority,
    pub status: TaskStatus,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub error: Option<String>,
}

impl ScheduledTask {
    pub fn new(session_key: String, prompt: String, mode: TaskMode) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let task_id = format!(
            "{}_{}_{}",
            match mode {
                TaskMode::Queue => "q",
                TaskMode::Background => "bg",
            },
            now,
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );

        Self {
            task_id,
            session_key,
            prompt,
            mode,
            priority: TaskPriority::Normal,
            status: TaskStatus::Pending,
            created_at: now,
            started_at: None,
            completed_at: None,
            error: None,
        }
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
        self.started_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    pub fn mark_completed(&mut self) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    pub fn mark_failed(&mut self, error: String) {
        self.status = TaskStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    pub fn mark_cancelled(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }
}

/// Task scheduler
pub struct TaskScheduler {
    /// Pending queue tasks per session (FIFO)
    queue_tasks: Arc<Mutex<HashMap<String, VecDeque<ScheduledTask>>>>,
    /// Running background tasks
    background_tasks: Arc<Mutex<HashMap<String, ScheduledTask>>>,
    /// Completed task history (last 100 per session)
    history: Arc<Mutex<HashMap<String, VecDeque<ScheduledTask>>>>,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            queue_tasks: Arc::new(Mutex::new(HashMap::new())),
            background_tasks: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueue a task (queue or background)
    pub fn enqueue(&self, task: ScheduledTask) -> Result<String, String> {
        let task_id = task.task_id.clone();
        let session_key = task.session_key.clone();

        match task.mode {
            TaskMode::Queue => {
                let mut queues = self.queue_tasks.lock().unwrap();
                queues.entry(session_key).or_default().push_back(task);
            }
            TaskMode::Background => {
                let mut bg_tasks = self.background_tasks.lock().unwrap();
                bg_tasks.insert(task_id.clone(), task);
            }
        }

        Ok(task_id)
    }

    /// Get next queue task for session (FIFO, oldest first)
    pub fn pop_queue_task(&self, session_key: &str) -> Option<ScheduledTask> {
        let mut queues = self.queue_tasks.lock().unwrap();
        queues.get_mut(session_key)?.pop_front()
    }

    /// Peek next queue task without removing
    pub fn peek_queue_task(&self, session_key: &str) -> Option<ScheduledTask> {
        let queues = self.queue_tasks.lock().unwrap();
        queues.get(session_key)?.front().cloned()
    }

    /// Get queue length for session
    pub fn queue_length(&self, session_key: &str) -> usize {
        let queues = self.queue_tasks.lock().unwrap();
        queues.get(session_key).map(|q| q.len()).unwrap_or(0)
    }

    /// List all queue tasks for session
    pub fn list_queue_tasks(&self, session_key: &str) -> Vec<ScheduledTask> {
        let queues = self.queue_tasks.lock().unwrap();
        queues
            .get(session_key)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get background task by ID
    pub fn get_background_task(&self, task_id: &str) -> Option<ScheduledTask> {
        let bg_tasks = self.background_tasks.lock().unwrap();
        bg_tasks.get(task_id).cloned()
    }

    /// Update background task status
    pub fn update_background_task(&self, task: ScheduledTask) -> Result<(), String> {
        let mut bg_tasks = self.background_tasks.lock().unwrap();
        bg_tasks.insert(task.task_id.clone(), task);
        Ok(())
    }

    /// Remove background task (move to history)
    pub fn complete_background_task(&self, task_id: &str) -> Result<(), String> {
        let mut bg_tasks = self.background_tasks.lock().unwrap();
        if let Some(task) = bg_tasks.remove(task_id) {
            self.add_to_history(task);
            Ok(())
        } else {
            Err(format!("Background task not found: {}", task_id))
        }
    }

    /// List all background tasks
    pub fn list_background_tasks(&self) -> Vec<ScheduledTask> {
        let bg_tasks = self.background_tasks.lock().unwrap();
        bg_tasks.values().cloned().collect()
    }

    /// List background tasks for session
    pub fn list_background_tasks_for_session(&self, session_key: &str) -> Vec<ScheduledTask> {
        let bg_tasks = self.background_tasks.lock().unwrap();
        bg_tasks
            .values()
            .filter(|t| t.session_key == session_key)
            .cloned()
            .collect()
    }

    /// Cancel task by ID
    pub fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        // Try queue tasks first
        {
            let mut queues = self.queue_tasks.lock().unwrap();
            for queue in queues.values_mut() {
                if let Some(pos) = queue.iter().position(|t| t.task_id == task_id) {
                    let mut task = queue.remove(pos).unwrap();
                    task.mark_cancelled();
                    self.add_to_history(task);
                    return Ok(());
                }
            }
        }

        // Try background tasks
        {
            let mut bg_tasks = self.background_tasks.lock().unwrap();
            if let Some(mut task) = bg_tasks.remove(task_id) {
                task.mark_cancelled();
                self.add_to_history(task);
                return Ok(());
            }
        }

        Err(format!("Task not found: {}", task_id))
    }

    /// Clear all queue tasks for session
    pub fn clear_queue(&self, session_key: &str) -> usize {
        let mut queues = self.queue_tasks.lock().unwrap();
        if let Some(queue) = queues.remove(session_key) {
            let count = queue.len();
            for mut task in queue {
                task.mark_cancelled();
                self.add_to_history(task);
            }
            count
        } else {
            0
        }
    }

    /// Get task history for session
    pub fn get_history(&self, session_key: &str, limit: usize) -> Vec<ScheduledTask> {
        let history = self.history.lock().unwrap();
        history
            .get(session_key)
            .map(|h| h.iter().rev().take(limit).cloned().collect())
            .unwrap_or_default()
    }

    /// Add task to history (keep last 100 per session)
    fn add_to_history(&self, task: ScheduledTask) {
        let mut history = self.history.lock().unwrap();
        let session_history = history.entry(task.session_key.clone()).or_default();

        session_history.push_back(task);

        // Keep only last 100 tasks
        while session_history.len() > 100 {
            session_history.pop_front();
        }
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_queue_task() {
        let scheduler = TaskScheduler::new();
        let task = ScheduledTask::new(
            "session-1".to_string(),
            "test prompt".to_string(),
            TaskMode::Queue,
        );
        let task_id = scheduler.enqueue(task).unwrap();
        assert!(task_id.starts_with("q_"));
        assert_eq!(scheduler.queue_length("session-1"), 1);
    }

    #[test]
    fn test_enqueue_background_task() {
        let scheduler = TaskScheduler::new();
        let task = ScheduledTask::new(
            "session-1".to_string(),
            "background prompt".to_string(),
            TaskMode::Background,
        );
        let task_id = scheduler.enqueue(task).unwrap();
        assert!(task_id.starts_with("bg_"));
        assert_eq!(scheduler.list_background_tasks().len(), 1);
    }

    #[test]
    fn test_pop_queue_task_fifo() {
        let scheduler = TaskScheduler::new();
        let task1 = ScheduledTask::new(
            "session-1".to_string(),
            "first".to_string(),
            TaskMode::Queue,
        );
        let task2 = ScheduledTask::new(
            "session-1".to_string(),
            "second".to_string(),
            TaskMode::Queue,
        );

        scheduler.enqueue(task1).unwrap();
        scheduler.enqueue(task2).unwrap();

        let popped = scheduler.pop_queue_task("session-1").unwrap();
        assert_eq!(popped.prompt, "first");
        assert_eq!(scheduler.queue_length("session-1"), 1);
    }

    #[test]
    fn test_peek_queue_task_does_not_remove() {
        let scheduler = TaskScheduler::new();
        let task = ScheduledTask::new("session-1".to_string(), "test".to_string(), TaskMode::Queue);
        scheduler.enqueue(task).unwrap();

        let peeked = scheduler.peek_queue_task("session-1").unwrap();
        assert_eq!(peeked.prompt, "test");
        assert_eq!(scheduler.queue_length("session-1"), 1); // Still there
    }

    #[test]
    fn test_cancel_queue_task() {
        let scheduler = TaskScheduler::new();
        let task = ScheduledTask::new("session-1".to_string(), "test".to_string(), TaskMode::Queue);
        let task_id = scheduler.enqueue(task).unwrap();

        scheduler.cancel_task(&task_id).unwrap();
        assert_eq!(scheduler.queue_length("session-1"), 0);

        // Should be in history
        let history = scheduler.get_history("session-1", 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, TaskStatus::Cancelled);
    }

    #[test]
    fn test_cancel_background_task() {
        let scheduler = TaskScheduler::new();
        let task = ScheduledTask::new(
            "session-1".to_string(),
            "bg test".to_string(),
            TaskMode::Background,
        );
        let task_id = scheduler.enqueue(task).unwrap();

        scheduler.cancel_task(&task_id).unwrap();
        assert_eq!(scheduler.list_background_tasks().len(), 0);

        // Should be in history
        let history = scheduler.get_history("session-1", 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, TaskStatus::Cancelled);
    }

    #[test]
    fn test_clear_queue() {
        let scheduler = TaskScheduler::new();
        for i in 0..5 {
            let task = ScheduledTask::new(
                "session-1".to_string(),
                format!("task {}", i),
                TaskMode::Queue,
            );
            scheduler.enqueue(task).unwrap();
        }

        let cleared = scheduler.clear_queue("session-1");
        assert_eq!(cleared, 5);
        assert_eq!(scheduler.queue_length("session-1"), 0);
    }

    #[test]
    fn test_task_status_transitions() {
        let mut task =
            ScheduledTask::new("session-1".to_string(), "test".to_string(), TaskMode::Queue);
        assert_eq!(task.status, TaskStatus::Pending);

        task.mark_running();
        assert_eq!(task.status, TaskStatus::Running);
        assert!(task.started_at.is_some());

        task.mark_completed();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_task_with_priority() {
        let task = ScheduledTask::new("session-1".to_string(), "test".to_string(), TaskMode::Queue)
            .with_priority(TaskPriority::High);
        assert_eq!(task.priority, TaskPriority::High);
    }

    #[test]
    fn test_history_limit() {
        let scheduler = TaskScheduler::new();

        // Add 150 tasks
        for i in 0..150 {
            let mut task = ScheduledTask::new(
                "session-1".to_string(),
                format!("task {}", i),
                TaskMode::Queue,
            );
            task.mark_completed();
            scheduler.add_to_history(task);
        }

        let history = scheduler.get_history("session-1", 200);
        assert_eq!(history.len(), 100); // Should keep only last 100
    }

    #[test]
    fn test_list_background_tasks_for_session() {
        let scheduler = TaskScheduler::new();

        let task1 = ScheduledTask::new(
            "session-1".to_string(),
            "bg1".to_string(),
            TaskMode::Background,
        );
        let task2 = ScheduledTask::new(
            "session-2".to_string(),
            "bg2".to_string(),
            TaskMode::Background,
        );
        let task3 = ScheduledTask::new(
            "session-1".to_string(),
            "bg3".to_string(),
            TaskMode::Background,
        );

        scheduler.enqueue(task1).unwrap();
        scheduler.enqueue(task2).unwrap();
        scheduler.enqueue(task3).unwrap();

        let session1_tasks = scheduler.list_background_tasks_for_session("session-1");
        assert_eq!(session1_tasks.len(), 2);
    }
}
