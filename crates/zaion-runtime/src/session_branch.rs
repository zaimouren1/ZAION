//! Session branching/forking implementation for /branch command
//!
//! Architecture (Hermes-compliant):
//! - Creates new session with copied conversation history
//! - Original session marked as ended with "branched" reason
//! - Auto-generated titles use lineage numbering (#2, #3, etc.)
//! - parent_session_id links preserved
//! - Custom branch names supported

use serde::{Deserialize, Serialize};
use zaion_types::envelope::is_unsafe_principal;

/// Session branch request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRequest {
    /// Original session ID to branch from
    pub parent_session_id: String,
    /// Optional custom name for the branch
    pub branch_name: Option<String>,
    /// Conversation history to copy
    pub history: Vec<BranchTurn>,
}

/// Turn in conversation history for branching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchTurn {
    pub role: String,
    pub content: String,
}

/// Session branch result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchResult {
    /// New session ID
    pub new_session_id: String,
    /// New session title
    pub new_title: String,
    /// Parent session ID
    pub parent_session_id: String,
    /// Number of turns copied
    pub turns_copied: usize,
}

/// Session brancher
pub struct SessionBrancher {
    session_store: Box<dyn SessionStore>,
}

/// Trait for session storage operations
pub trait SessionStore: Send + Sync {
    /// Get session by ID
    fn get_session(&self, session_id: &str) -> Result<Option<SessionMetadata>, String>;

    /// Create new session
    fn create_session(&self, metadata: SessionMetadata) -> Result<(), String>;

    /// Update session metadata
    fn update_session(&self, session_id: &str, metadata: SessionMetadata) -> Result<(), String>;

    /// Get session title
    fn get_title(&self, session_id: &str) -> Result<Option<String>, String>;

    /// Set session title
    fn set_title(&self, session_id: &str, title: &str) -> Result<(), String>;

    /// Copy conversation history to new session
    fn copy_history(&self, from_session: &str, to_session: &str) -> Result<usize, String>;
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub principal_id: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub title: Option<String>,
    pub model: String,
    pub source: String,
    pub chat_id: Option<String>,
    pub user_id: Option<String>,
    pub thread_id: Option<String>,
    pub end_reason: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
}

impl SessionBrancher {
    pub fn new(session_store: Box<dyn SessionStore>) -> Self {
        Self { session_store }
    }

    /// Branch a session (create a fork with copied history)
    pub fn branch(&self, request: BranchRequest) -> Result<BranchResult, String> {
        self.branch_with_parent_end_reason(request, "branched")
    }

    /// Branch a session and archive the parent with an explicit end reason.
    ///
    /// Plain `/branch` uses `branched`; compression-triggered lineage uses
    /// `compression`, matching Hermes' parent_session_id split semantics.
    pub fn branch_with_parent_end_reason(
        &self,
        request: BranchRequest,
        parent_end_reason: impl Into<String>,
    ) -> Result<BranchResult, String> {
        let parent_end_reason = parent_end_reason.into();
        if parent_end_reason.trim().is_empty() {
            return Err("parent end reason cannot be empty".into());
        }
        // Validate parent session exists
        let parent = self
            .session_store
            .get_session(&request.parent_session_id)?
            .ok_or_else(|| format!("Parent session not found: {}", request.parent_session_id))?;
        if is_unsafe_principal(&parent.principal_id) {
            return Err(format!(
                "Parent session has non-production principal: {}",
                parent.principal_id
            ));
        }

        // Validate history is not empty
        if request.history.is_empty() {
            return Err("Cannot branch empty conversation".into());
        }

        // Generate new session ID
        let new_session_id = self.generate_session_id();

        // Determine new title
        let parent_title = self
            .session_store
            .get_title(&request.parent_session_id)?
            .unwrap_or_else(|| "Session".to_string());

        let new_title = if let Some(custom_name) = request.branch_name {
            custom_name
        } else {
            self.generate_lineage_title(&parent_title)
        };

        // Create new session metadata
        let new_metadata = SessionMetadata {
            principal_id: parent.principal_id.clone(),
            session_id: new_session_id.clone(),
            parent_session_id: Some(request.parent_session_id.clone()),
            title: Some(new_title.clone()),
            model: parent.model.clone(),
            source: parent.source.clone(),
            chat_id: parent.chat_id.clone(),
            user_id: parent.user_id.clone(),
            thread_id: parent.thread_id.clone(),
            end_reason: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            ended_at: None,
        };

        // Create new session
        self.session_store.create_session(new_metadata)?;

        // Set title
        self.session_store.set_title(&new_session_id, &new_title)?;

        // Copy conversation history
        let turns_copied = self
            .session_store
            .copy_history(&request.parent_session_id, &new_session_id)?;

        // Mark parent session as ended with the caller-specified lineage reason.
        let mut parent_updated = parent.clone();
        parent_updated.end_reason = Some(parent_end_reason);
        parent_updated.ended_at = Some(chrono::Utc::now().to_rfc3339());
        self.session_store
            .update_session(&request.parent_session_id, parent_updated)?;

        Ok(BranchResult {
            new_session_id,
            new_title,
            parent_session_id: request.parent_session_id,
            turns_copied,
        })
    }

    /// Generate new session ID (timestamp-based)
    fn generate_session_id(&self) -> String {
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let random = uuid::Uuid::new_v4().simple().to_string();
        format!("{}_{}", timestamp, &random[..8])
    }

    /// Generate lineage title (e.g., "My Session" -> "My Session #2")
    fn generate_lineage_title(&self, parent_title: &str) -> String {
        // Check if parent title already has a lineage number
        if let Some(pos) = parent_title.rfind(" #") {
            let base = &parent_title[..pos];
            let number_part = &parent_title[pos + 2..];
            if let Ok(num) = number_part.parse::<u32>() {
                return format!("{} #{}", base, num + 1);
            }
        }

        // No lineage number, add #2
        format!("{} #2", parent_title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    struct MockSessionStore {
        sessions: Arc<Mutex<HashMap<String, SessionMetadata>>>,
        titles: Arc<Mutex<HashMap<String, String>>>,
        history_counts: Arc<Mutex<HashMap<String, usize>>>,
    }

    impl MockSessionStore {
        fn new() -> Self {
            Self {
                sessions: Arc::new(Mutex::new(HashMap::new())),
                titles: Arc::new(Mutex::new(HashMap::new())),
                history_counts: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn add_session(&self, metadata: SessionMetadata) {
            self.sessions
                .lock()
                .unwrap()
                .insert(metadata.session_id.clone(), metadata);
        }

        fn add_title(&self, session_id: &str, title: &str) {
            self.titles
                .lock()
                .unwrap()
                .insert(session_id.to_string(), title.to_string());
        }

        fn set_history_count(&self, session_id: &str, count: usize) {
            self.history_counts
                .lock()
                .unwrap()
                .insert(session_id.to_string(), count);
        }
    }

    impl SessionStore for MockSessionStore {
        fn get_session(&self, session_id: &str) -> Result<Option<SessionMetadata>, String> {
            Ok(self.sessions.lock().unwrap().get(session_id).cloned())
        }

        fn create_session(&self, metadata: SessionMetadata) -> Result<(), String> {
            self.sessions
                .lock()
                .unwrap()
                .insert(metadata.session_id.clone(), metadata);
            Ok(())
        }

        fn update_session(
            &self,
            session_id: &str,
            metadata: SessionMetadata,
        ) -> Result<(), String> {
            self.sessions
                .lock()
                .unwrap()
                .insert(session_id.to_string(), metadata);
            Ok(())
        }

        fn get_title(&self, session_id: &str) -> Result<Option<String>, String> {
            Ok(self.titles.lock().unwrap().get(session_id).cloned())
        }

        fn set_title(&self, session_id: &str, title: &str) -> Result<(), String> {
            self.titles
                .lock()
                .unwrap()
                .insert(session_id.to_string(), title.to_string());
            Ok(())
        }

        fn copy_history(&self, from_session: &str, to_session: &str) -> Result<usize, String> {
            let count = self
                .history_counts
                .lock()
                .unwrap()
                .get(from_session)
                .copied()
                .unwrap_or(0);
            self.history_counts
                .lock()
                .unwrap()
                .insert(to_session.to_string(), count);
            Ok(count)
        }
    }

    #[test]
    fn test_branch_creates_new_session() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-branch-test".to_string(),
            session_id: "parent-123".to_string(),
            parent_session_id: None,
            title: Some("Original Session".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });
        store.add_title("parent-123", "Original Session");
        store.set_history_count("parent-123", 4);

        let brancher = SessionBrancher::new(Box::new(store));

        let request = BranchRequest {
            parent_session_id: "parent-123".to_string(),
            branch_name: None,
            history: vec![
                BranchTurn {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                },
                BranchTurn {
                    role: "assistant".to_string(),
                    content: "Hi".to_string(),
                },
            ],
        };

        let result = brancher.branch(request).unwrap();

        assert_ne!(result.new_session_id, "parent-123");
        assert_eq!(result.parent_session_id, "parent-123");
        assert_eq!(result.new_title, "Original Session #2");
        assert_eq!(result.turns_copied, 4);
    }

    #[test]
    fn test_branch_with_custom_name() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-branch-test".to_string(),
            session_id: "parent-456".to_string(),
            parent_session_id: None,
            title: Some("Main Session".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });
        store.add_title("parent-456", "Main Session");
        store.set_history_count("parent-456", 2);

        let brancher = SessionBrancher::new(Box::new(store));

        let request = BranchRequest {
            parent_session_id: "parent-456".to_string(),
            branch_name: Some("Refactor Approach".to_string()),
            history: vec![BranchTurn {
                role: "user".to_string(),
                content: "Test".to_string(),
            }],
        };

        let result = brancher.branch(request).unwrap();

        assert_eq!(result.new_title, "Refactor Approach");
    }

    #[test]
    fn test_branch_empty_conversation_fails() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-branch-test".to_string(),
            session_id: "parent-789".to_string(),
            parent_session_id: None,
            title: Some("Empty Session".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });

        let brancher = SessionBrancher::new(Box::new(store));

        let request = BranchRequest {
            parent_session_id: "parent-789".to_string(),
            branch_name: None,
            history: vec![],
        };

        let result = brancher.branch(request);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty conversation"));
    }

    #[test]
    fn test_generate_lineage_title() {
        let store = MockSessionStore::new();
        let brancher = SessionBrancher::new(Box::new(store));

        assert_eq!(
            brancher.generate_lineage_title("My Session"),
            "My Session #2"
        );
        assert_eq!(
            brancher.generate_lineage_title("My Session #2"),
            "My Session #3"
        );
        assert_eq!(
            brancher.generate_lineage_title("My Session #10"),
            "My Session #11"
        );
    }

    #[test]
    fn test_branch_marks_parent_as_ended() {
        let store = MockSessionStore::new();
        let parent_id = "parent-abc";
        store.add_session(SessionMetadata {
            principal_id: "principal-branch-test".to_string(),
            session_id: parent_id.to_string(),
            parent_session_id: None,
            title: Some("Parent".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: Some("branch-thread".to_string()),
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });
        store.add_title(parent_id, "Parent");
        store.set_history_count(parent_id, 2);

        let brancher = SessionBrancher::new(Box::new(store));

        let request = BranchRequest {
            parent_session_id: parent_id.to_string(),
            branch_name: None,
            history: vec![BranchTurn {
                role: "user".to_string(),
                content: "Hi".to_string(),
            }],
        };

        let result = brancher.branch(request).unwrap();

        // Verify parent session was marked as ended
        let parent = brancher
            .session_store
            .get_session(parent_id)
            .unwrap()
            .unwrap();
        assert_eq!(parent.end_reason, Some("branched".to_string()));
        assert!(parent.ended_at.is_some());
        let child = brancher
            .session_store
            .get_session(&result.new_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(child.parent_session_id.as_deref(), Some(parent_id));
        assert_eq!(child.thread_id.as_deref(), Some("branch-thread"));
    }

    #[test]
    fn test_branch_rejects_unsafe_parent_principal() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "default".to_string(),
            session_id: "parent-unsafe".to_string(),
            parent_session_id: None,
            title: Some("Unsafe Parent".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });
        store.add_title("parent-unsafe", "Unsafe Parent");

        let brancher = SessionBrancher::new(Box::new(store));
        let err = brancher
            .branch(BranchRequest {
                parent_session_id: "parent-unsafe".to_string(),
                branch_name: None,
                history: vec![BranchTurn {
                    role: "user".to_string(),
                    content: "Hi".to_string(),
                }],
            })
            .unwrap_err();

        assert!(err.contains("non-production principal"));
    }
}
