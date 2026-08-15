use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// TaskId uses uuid with "serde" feature enabled in Cargo.toml
pub type TaskId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskResult {
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub files_written: Vec<String>,
    pub files_read: Vec<String>,
    pub aci_operations: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowTask {
    pub id: TaskId,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub env_vars: std::collections::HashMap<String, String>,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<TaskResult>,
    pub priority: i32,
    pub timeout_seconds: Option<u64>,
    pub retry_count: u32,
    pub max_retries: u32,
}

impl ShadowTask {
    pub fn new(name: String, command: String, args: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            command,
            args,
            working_dir: None,
            env_vars: std::collections::HashMap::new(),
            status: TaskStatus::Queued,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            priority: 0,
            timeout_seconds: None,
            retry_count: 0,
            max_retries: 0,
        }
    }

    pub fn with_working_dir(mut self, dir: String) -> Self {
        self.working_dir = Some(dir);
        self
    }

    pub fn with_env(mut self, key: String, value: String) -> Self {
        self.env_vars.insert(key, value);
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }

    pub fn with_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    pub fn can_retry(&self) -> bool {
        self.status == TaskStatus::Failed && self.retry_count < self.max_retries
    }

    pub fn duration_ms(&self) -> Option<u64> {
        if let (Some(start), Some(end)) = (self.started_at, self.completed_at) {
            Some((end - start).num_milliseconds() as u64)
        } else {
            None
        }
    }
}
