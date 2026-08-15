//! Session todo tool state for preserving active work across long runs.
//!
//! Hermes keeps a per-session in-memory todo list, returns the full list after
//! tool calls, and re-injects active items after context compression. This
//! module provides the same runtime primitive for Zaion without coupling it to
//! CLI/TUI surfaces.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Valid lifecycle states for a session todo item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl TodoStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Coarse priority used for stable sorting and compact context injection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

impl TodoPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }
}

/// A single structured task in the current runtime session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    #[serde(alias = "content")]
    pub title: String,
    #[serde(default)]
    pub status: TodoStatus,
    #[serde(default)]
    pub priority: TodoPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl TodoItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Normal,
            notes: None,
        }
    }

    fn normalize(mut self) -> Self {
        self.id = clean_or(self.id, "?");
        self.title = clean_or(self.title, "(no title)");
        self.notes = self.notes.and_then(non_empty_trimmed_owned);
        self
    }
}

/// Partial update payload for an existing item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TodoStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<TodoPriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Summary counts returned with every tool-style response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub cancelled: usize,
}

/// Stable response shape for tool calls and API consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoToolResponse {
    pub todos: Vec<TodoItem>,
    pub summary: TodoSummary,
}

/// Supported todo tool operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TodoToolRequest {
    Add {
        id: String,
        title: String,
        #[serde(default)]
        priority: TodoPriority,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
    Update {
        id: String,
        #[serde(flatten)]
        update: TodoUpdate,
    },
    Complete {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
    Remove {
        id: String,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus>,
        #[serde(default)]
        active_only: bool,
    },
    Replace {
        todos: Vec<TodoItem>,
    },
}

/// In-memory todo list scoped to one runtime/session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoStore {
    items: Vec<TodoItem>,
}

impl TodoStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&mut self, todos: Vec<TodoItem>) -> Vec<TodoItem> {
        self.items = dedupe_keep_last_position(todos.into_iter().map(TodoItem::normalize));
        self.list()
    }

    pub fn add(&mut self, item: TodoItem) -> Vec<TodoItem> {
        let item = item.normalize();
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|existing| existing.id == item.id)
        {
            *existing = item;
        } else {
            self.items.push(item);
        }
        self.list()
    }

    pub fn update(&mut self, id: &str, update: TodoUpdate) -> Result<Vec<TodoItem>, String> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("todo '{}' not found", id))?;

        if let Some(title) = update.title.and_then(non_empty_trimmed_owned) {
            item.title = title;
        }
        if let Some(status) = update.status {
            item.status = status;
        }
        if let Some(priority) = update.priority {
            item.priority = priority;
        }
        if let Some(notes) = update.notes {
            item.notes = non_empty_trimmed_owned(notes);
        }
        Ok(self.list())
    }

    pub fn complete(&mut self, id: &str, notes: Option<String>) -> Result<Vec<TodoItem>, String> {
        self.update(
            id,
            TodoUpdate {
                status: Some(TodoStatus::Completed),
                notes,
                ..TodoUpdate::default()
            },
        )
    }

    pub fn remove(&mut self, id: &str) -> Result<Vec<TodoItem>, String> {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        if self.items.len() == before {
            return Err(format!("todo '{}' not found", id));
        }
        Ok(self.list())
    }

    pub fn list(&self) -> Vec<TodoItem> {
        self.items.clone()
    }

    pub fn list_filtered(&self, status: Option<TodoStatus>, active_only: bool) -> Vec<TodoItem> {
        self.items
            .iter()
            .filter(|item| {
                status.is_none_or(|status| item.status == status)
                    && (!active_only || item.status.is_active())
            })
            .cloned()
            .collect()
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn summary(&self) -> TodoSummary {
        TodoSummary {
            total: self.items.len(),
            pending: self
                .items
                .iter()
                .filter(|item| item.status == TodoStatus::Pending)
                .count(),
            in_progress: self
                .items
                .iter()
                .filter(|item| item.status == TodoStatus::InProgress)
                .count(),
            completed: self
                .items
                .iter()
                .filter(|item| item.status == TodoStatus::Completed)
                .count(),
            cancelled: self
                .items
                .iter()
                .filter(|item| item.status == TodoStatus::Cancelled)
                .count(),
        }
    }

    pub fn response(&self) -> TodoToolResponse {
        TodoToolResponse {
            todos: self.list(),
            summary: self.summary(),
        }
    }

    pub fn handle_request(&mut self, request: TodoToolRequest) -> Result<TodoToolResponse, String> {
        match request {
            TodoToolRequest::Add {
                id,
                title,
                priority,
                notes,
            } => {
                let mut item = TodoItem::new(id, title);
                item.priority = priority;
                item.notes = notes;
                self.add(item);
                Ok(self.response())
            }
            TodoToolRequest::Update { id, update } => {
                self.update(&id, update)?;
                Ok(self.response())
            }
            TodoToolRequest::Complete { id, notes } => {
                self.complete(&id, notes)?;
                Ok(self.response())
            }
            TodoToolRequest::Remove { id } => {
                self.remove(&id)?;
                Ok(self.response())
            }
            TodoToolRequest::List {
                status,
                active_only,
            } => Ok(TodoToolResponse {
                todos: self.list_filtered(status, active_only),
                summary: self.summary(),
            }),
            TodoToolRequest::Replace { todos } => {
                self.replace(todos);
                Ok(self.response())
            }
        }
    }

    pub fn handle_json(&mut self, args: &Value) -> Result<Value, String> {
        let request: TodoToolRequest =
            serde_json::from_value(args.clone()).map_err(|err| err.to_string())?;
        let response = self.handle_request(request)?;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    /// Render active tasks as compact handoff context after compression.
    ///
    /// Completed/cancelled items are intentionally omitted so a compressed
    /// session does not encourage the model to redo already-settled work.
    pub fn compression_reinjection_text(&self) -> Option<String> {
        let active = self.list_filtered(None, true);
        if active.is_empty() {
            return None;
        }

        let mut lines =
            vec!["[Active session todo list preserved across context compression]".to_string()];
        for item in active {
            let marker = match item.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[>]",
                TodoStatus::Completed => "[x]",
                TodoStatus::Cancelled => "[~]",
            };
            let mut line = format!(
                "- {} {}. {} (status={}, priority={})",
                marker,
                item.id,
                item.title,
                item.status.as_str(),
                item.priority.as_str()
            );
            if let Some(notes) = item.notes {
                line.push_str(&format!(" notes={}", notes));
            }
            lines.push(line);
        }
        Some(lines.join("\n"))
    }

    /// Rehydrate from the latest previous tool response JSON.
    pub fn hydrate_from_tool_response_json(&mut self, content: &str) -> Result<bool, String> {
        let value: Value = serde_json::from_str(content).map_err(|err| err.to_string())?;
        let Some(todos) = value.get("todos").and_then(Value::as_array) else {
            return Ok(false);
        };
        let items: Vec<TodoItem> =
            serde_json::from_value(Value::Array(todos.clone())).map_err(|err| err.to_string())?;
        self.replace(items);
        Ok(true)
    }
}

fn clean_or(value: String, fallback: &str) -> String {
    non_empty_trimmed_owned(value).unwrap_or_else(|| fallback.to_string())
}

fn non_empty_trimmed_owned(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn dedupe_keep_last_position(items: impl Iterator<Item = TodoItem>) -> Vec<TodoItem> {
    let mut deduped = Vec::new();
    for item in items {
        if let Some(index) = deduped
            .iter()
            .position(|existing: &TodoItem| existing.id == item.id)
        {
            deduped.remove(index);
        }
        deduped.push(item);
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str, title: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            id: id.to_string(),
            title: title.to_string(),
            status,
            priority: TodoPriority::Normal,
            notes: None,
        }
    }

    #[test]
    fn todo_replace_dedupes_duplicate_ids_keep_last_position() {
        let mut store = TodoStore::new();

        let result = store.replace(vec![
            item("1", "old", TodoStatus::Pending),
            item("2", "second", TodoStatus::Pending),
            item("1", "new", TodoStatus::InProgress),
        ]);

        assert_eq!(
            result,
            vec![
                item("2", "second", TodoStatus::Pending),
                item("1", "new", TodoStatus::InProgress),
            ]
        );
    }

    #[test]
    fn todo_add_update_complete_remove_and_list() {
        let mut store = TodoStore::new();

        store
            .handle_request(TodoToolRequest::Add {
                id: "plan".into(),
                title: "Read Hermes evidence".into(),
                priority: TodoPriority::High,
                notes: Some("tools/todo_tool.py".into()),
            })
            .unwrap();
        store
            .handle_request(TodoToolRequest::Update {
                id: "plan".into(),
                update: TodoUpdate {
                    status: Some(TodoStatus::InProgress),
                    ..TodoUpdate::default()
                },
            })
            .unwrap();
        let response = store
            .handle_request(TodoToolRequest::Complete {
                id: "plan".into(),
                notes: Some("done".into()),
            })
            .unwrap();

        assert_eq!(response.summary.completed, 1);
        assert_eq!(response.todos[0].status, TodoStatus::Completed);
        assert_eq!(response.todos[0].notes.as_deref(), Some("done"));

        let response = store
            .handle_request(TodoToolRequest::Remove { id: "plan".into() })
            .unwrap();
        assert_eq!(response.summary.total, 0);
    }

    #[test]
    fn todo_list_filters_by_active_or_status() {
        let mut store = TodoStore::new();
        store.replace(vec![
            item("1", "pending", TodoStatus::Pending),
            item("2", "working", TodoStatus::InProgress),
            item("3", "done", TodoStatus::Completed),
            item("4", "cancel", TodoStatus::Cancelled),
        ]);

        let active = store
            .handle_request(TodoToolRequest::List {
                status: None,
                active_only: true,
            })
            .unwrap();
        assert_eq!(active.todos.len(), 2);
        assert_eq!(active.summary.total, 4);

        let completed = store
            .handle_request(TodoToolRequest::List {
                status: Some(TodoStatus::Completed),
                active_only: false,
            })
            .unwrap();
        assert_eq!(
            completed.todos,
            vec![item("3", "done", TodoStatus::Completed)]
        );
    }

    #[test]
    fn todo_json_request_returns_serializable_full_state() {
        let mut store = TodoStore::new();

        let value = store
            .handle_json(&json!({
                "action": "add",
                "id": "ship",
                "title": "Run focused tests",
                "priority": "urgent",
                "notes": "cargo test -p zaion-runtime todo -- --nocapture"
            }))
            .unwrap();

        assert_eq!(value["summary"]["total"], 1);
        assert_eq!(value["summary"]["pending"], 1);
        assert_eq!(value["todos"][0]["priority"], "urgent");
    }

    #[test]
    fn todo_compression_reinjection_preserves_only_active_tasks() {
        let mut store = TodoStore::new();
        store.replace(vec![
            item("1", "already done", TodoStatus::Completed),
            item("2", "next task", TodoStatus::Pending),
            item("3", "current task", TodoStatus::InProgress),
            item("4", "dropped task", TodoStatus::Cancelled),
        ]);

        let text = store.compression_reinjection_text().unwrap();

        assert!(text.contains("context compression"));
        assert!(text.contains("[ ] 2. next task"));
        assert!(text.contains("[>] 3. current task"));
        assert!(!text.contains("already done"));
        assert!(!text.contains("dropped task"));
    }

    #[test]
    fn todo_hydrates_from_previous_tool_response_json() {
        let mut store = TodoStore::new();
        let content = serde_json::to_string(&TodoToolResponse {
            todos: vec![item("1", "restore me", TodoStatus::Pending)],
            summary: TodoSummary {
                total: 99,
                ..TodoSummary::default()
            },
        })
        .unwrap();

        assert!(store.hydrate_from_tool_response_json(&content).unwrap());
        assert_eq!(store.summary().total, 1);
        assert_eq!(store.list()[0].title, "restore me");
    }

    #[test]
    fn todo_hydrates_hermes_content_field_alias() {
        let mut store = TodoStore::new();
        let content = json!({
            "todos": [
                {"id": "1", "content": "Hermes shaped item", "status": "pending"}
            ],
            "summary": {"total": 1}
        })
        .to_string();

        assert!(store.hydrate_from_tool_response_json(&content).unwrap());
        assert_eq!(store.list()[0].title, "Hermes shaped item");
    }
}
