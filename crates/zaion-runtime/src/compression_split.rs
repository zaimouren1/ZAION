//! Compression-triggered session splitting
//!
//! Hermes architecture: When context compression is triggered, create a new session
//! with the compressed history and mark the old session as ended with "compression" reason.
//! This creates a parent_session_id chain for session lineage tracking.
//!
//! Integration points:
//! - ContextCompressor: detects when compression is needed
//! - SessionBrancher: creates new session with parent_session_id link
//! - SessionStore: persists session metadata and history

use crate::compressor::{CompressedContext, ContextCompressor, Turn};
use crate::session_branch::{BranchRequest, BranchTurn, SessionBrancher, SessionStore};
use crate::todo_tool::TodoStore;
use serde::{Deserialize, Serialize};

/// Compression split request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionSplitRequest {
    /// Current session ID
    pub current_session_id: String,
    /// Conversation history before compression
    pub history: Vec<Turn>,
    /// Token budget
    pub token_budget: usize,
    /// Force a compression attempt below the configured trigger threshold.
    #[serde(default)]
    pub force_compression: bool,
    /// Optional LLM summary for compressed middle section
    pub llm_summary: Option<String>,
}

/// Compression split result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionSplitResult {
    /// New session ID (if split occurred)
    pub new_session_id: Option<String>,
    /// Compressed context
    pub compressed: CompressedContext,
    /// Whether a split was performed
    pub split_performed: bool,
    /// Parent session ID (if split occurred)
    pub parent_session_id: Option<String>,
}

/// Compression splitter - integrates ContextCompressor with SessionBrancher
pub struct CompressionSplitter {
    compressor: ContextCompressor,
    brancher: SessionBrancher,
}

impl CompressionSplitter {
    /// Create new compression splitter
    pub fn new(compressor: ContextCompressor, session_store: Box<dyn SessionStore>) -> Self {
        let brancher = SessionBrancher::new(session_store);
        Self {
            compressor,
            brancher,
        }
    }

    /// Compress history and optionally split session
    ///
    /// If compression is triggered, creates a new session with compressed history
    /// and marks the old session as ended with "compression" reason.
    pub fn compress_and_split(
        &mut self,
        request: CompressionSplitRequest,
    ) -> Result<CompressionSplitResult, String> {
        let compressed = if request.force_compression {
            self.compressor.compress_forced(
                &request.history,
                request.token_budget,
                request.llm_summary.as_deref(),
            )
        } else {
            self.compressor.compress(
                &request.history,
                request.token_budget,
                request.llm_summary.as_deref(),
            )
        };
        self.split_compressed(request, compressed)
    }

    /// Compress history with active todo reinjection and optionally split session.
    pub fn compress_and_split_with_todo_reinjection(
        &mut self,
        request: CompressionSplitRequest,
        todo_store: &TodoStore,
    ) -> Result<CompressionSplitResult, String> {
        let compressed = if request.force_compression {
            self.compressor.compress_with_todo_reinjection_forced(
                &request.history,
                request.token_budget,
                request.llm_summary.as_deref(),
                todo_store,
            )
        } else {
            self.compressor.compress_with_todo_reinjection(
                &request.history,
                request.token_budget,
                request.llm_summary.as_deref(),
                todo_store,
            )
        };
        self.split_compressed(request, compressed)
    }

    fn split_compressed(
        &mut self,
        request: CompressionSplitRequest,
        compressed: CompressedContext,
    ) -> Result<CompressionSplitResult, String> {
        if compressed.was_compressed {
            let branch_history: Vec<BranchTurn> = compressed
                .turns
                .iter()
                .map(|turn| BranchTurn {
                    role: turn.role.clone(),
                    content: turn.content.clone(),
                })
                .collect();

            let branch_request = BranchRequest {
                parent_session_id: request.current_session_id.clone(),
                branch_name: Some(format!(
                    "compression:{}:{}",
                    request.current_session_id,
                    &uuid::Uuid::new_v4().simple().to_string()[..8]
                )),
                history: branch_history,
            };

            let branch_result = self
                .brancher
                .branch_with_parent_end_reason(branch_request, "compression")?;

            Ok(CompressionSplitResult {
                new_session_id: Some(branch_result.new_session_id),
                compressed,
                split_performed: true,
                parent_session_id: Some(request.current_session_id),
            })
        } else {
            Ok(CompressionSplitResult {
                new_session_id: None,
                compressed,
                split_performed: false,
                parent_session_id: None,
            })
        }
    }

    /// Check if compression would be triggered (without actually compressing)
    pub fn needs_compression(&self, history: &[Turn], token_budget: usize) -> bool {
        self.compressor.needs_compression(history, token_budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compressor::CompressorConfig;
    use crate::session_branch::SessionMetadata;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
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
                .insert(metadata.session_id.clone(), metadata.clone());
            if let Some(title) = &metadata.title {
                self.titles
                    .lock()
                    .unwrap()
                    .insert(metadata.session_id.clone(), title.clone());
            }
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

    fn make_large_history(n: usize) -> Vec<Turn> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Turn::new(
                        "user",
                        format!(
                            "message {} with lots of content to trigger compression threshold",
                            i
                        ),
                    )
                } else {
                    Turn::new(
                        "assistant",
                        format!("response {} with detailed answer and more content", i),
                    )
                }
            })
            .collect()
    }

    #[test]
    fn legacy_request_deserialization_defaults_force_compression_to_false() {
        let serialized = serde_json::json!({
            "current_session_id": "legacy-session",
            "history": [{"role": "user", "content": "legacy request"}],
            "token_budget": 10_000,
            "llm_summary": null
        });

        let request: CompressionSplitRequest = serde_json::from_value(serialized).unwrap();

        assert!(!request.force_compression);
    }

    #[test]
    fn test_no_compression_no_split() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-compression-test".to_string(),
            session_id: "session-123".to_string(),
            parent_session_id: None,
            title: Some("Test Session".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });

        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);
        let mut splitter = CompressionSplitter::new(compressor, Box::new(store));

        let history = vec![
            Turn::new("user", "hello"),
            Turn::new("assistant", "hi there"),
        ];

        let request = CompressionSplitRequest {
            current_session_id: "session-123".to_string(),
            history,
            token_budget: 10_000,
            force_compression: false,
            llm_summary: None,
        };

        let result = splitter.compress_and_split(request).unwrap();

        assert!(!result.split_performed);
        assert!(result.new_session_id.is_none());
        assert!(!result.compressed.was_compressed);
    }

    #[test]
    fn test_compression_triggers_split() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-compression-test".to_string(),
            session_id: "session-456".to_string(),
            parent_session_id: None,
            title: Some("Long Session".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: Some("compression-thread".to_string()),
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });

        let config = CompressorConfig {
            threshold_ratio: 0.01, // Very low threshold to trigger compression
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let store_observer = store.clone();
        let mut splitter = CompressionSplitter::new(compressor, Box::new(store));

        let history = make_large_history(30);

        let request = CompressionSplitRequest {
            current_session_id: "session-456".to_string(),
            history,
            token_budget: 10_000,
            force_compression: false,
            llm_summary: None,
        };

        let result = splitter.compress_and_split(request).unwrap();

        assert!(result.split_performed);
        assert!(result.new_session_id.is_some());
        assert!(result.compressed.was_compressed);
        assert_eq!(result.parent_session_id, Some("session-456".to_string()));
        let parent = store_observer
            .get_session("session-456")
            .unwrap()
            .expect("parent session should remain readable after split");
        assert_eq!(parent.end_reason, Some("compression".to_string()));
        let child = store_observer
            .get_session(result.new_session_id.as_deref().unwrap())
            .unwrap()
            .expect("compressed child session should be persisted");
        assert_eq!(child.thread_id.as_deref(), Some("compression-thread"));
    }

    #[test]
    fn forced_compression_below_threshold_triggers_split() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-compression-test".to_string(),
            session_id: "session-forced".to_string(),
            parent_session_id: None,
            title: Some("Forced Compression".to_string()),
            model: "gpt-5.5".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: Some("forced-thread".to_string()),
            end_reason: None,
            created_at: "2026-07-15T00:00:00Z".to_string(),
            ended_at: None,
        });

        let config = CompressorConfig {
            threshold_ratio: 0.95,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let mut splitter = CompressionSplitter::new(compressor, Box::new(store));
        let history = make_large_history(30);
        let token_budget = 200_000;
        assert!(!splitter.needs_compression(&history, token_budget));

        let result = splitter
            .compress_and_split(CompressionSplitRequest {
                current_session_id: "session-forced".to_string(),
                history,
                token_budget,
                force_compression: true,
                llm_summary: None,
            })
            .unwrap();

        assert!(result.split_performed);
        assert!(result.compressed.was_compressed);
        assert!(result.compressed.turns_pruned > 0);
        assert_eq!(result.parent_session_id.as_deref(), Some("session-forced"));
    }

    #[test]
    fn test_needs_compression_check() {
        let store = MockSessionStore::new();
        let config = CompressorConfig {
            threshold_ratio: 0.01,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let splitter = CompressionSplitter::new(compressor, Box::new(store));

        let small_history = vec![Turn::new("user", "hello"), Turn::new("assistant", "hi")];

        let large_history = make_large_history(50);

        assert!(!splitter.needs_compression(&small_history, 10_000));
        assert!(splitter.needs_compression(&large_history, 10_000));
    }

    #[test]
    fn test_split_with_llm_summary() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-compression-test".to_string(),
            session_id: "session-789".to_string(),
            parent_session_id: None,
            title: Some("Summary Test".to_string()),
            model: "claude-sonnet-4-6".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        });

        let config = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let mut splitter = CompressionSplitter::new(compressor, Box::new(store));

        let history = make_large_history(25);
        let llm_summary = "Goal: test compression\nProgress: done\nDecisions: N/A";

        let request = CompressionSplitRequest {
            current_session_id: "session-789".to_string(),
            history,
            token_budget: 10_000,
            force_compression: false,
            llm_summary: Some(llm_summary.to_string()),
        };

        let result = splitter.compress_and_split(request).unwrap();

        assert!(result.split_performed);
        assert!(result
            .compressed
            .summary_text
            .contains("Goal: test compression"));
    }

    #[test]
    fn compression_split_reinjects_active_todos_before_child_branch() {
        let store = MockSessionStore::new();
        store.add_session(SessionMetadata {
            principal_id: "principal-compression-test".to_string(),
            session_id: "session-todo".to_string(),
            parent_session_id: None,
            title: Some("Todo Compression".to_string()),
            model: "gpt-5.5".to_string(),
            source: "cli".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: Some("todo-thread".to_string()),
            end_reason: None,
            created_at: "2026-05-23T00:00:00Z".to_string(),
            ended_at: None,
        });

        let config = CompressorConfig {
            threshold_ratio: 0.95,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let mut splitter = CompressionSplitter::new(compressor, Box::new(store));
        let mut todos = crate::todo_tool::TodoStore::new();
        todos.add(crate::todo_tool::TodoItem {
            id: "hermes-parity".to_string(),
            title: "Preserve active todos through compression split".to_string(),
            status: crate::todo_tool::TodoStatus::InProgress,
            priority: crate::todo_tool::TodoPriority::High,
            notes: Some("runtime todo reinjection".to_string()),
        });

        let history = make_large_history(30);
        assert!(!splitter.needs_compression(&history, 200_000));
        let request = CompressionSplitRequest {
            current_session_id: "session-todo".to_string(),
            history,
            token_budget: 200_000,
            force_compression: true,
            llm_summary: None,
        };

        let result = splitter
            .compress_and_split_with_todo_reinjection(request, &todos)
            .unwrap();

        assert!(result.split_performed);
        let compressed_text = result
            .compressed
            .turns
            .iter()
            .map(|turn| turn.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(compressed_text.contains("hermes-parity"));
        assert!(compressed_text.contains("runtime todo reinjection"));
        assert!(result.compressed.protected_tail_turns >= 3);
    }
}
