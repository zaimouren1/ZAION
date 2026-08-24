//! Federated session management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

/// Session naming strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionNamingStrategy {
    PerDirectory,
    Global,
    Manual,
    TitleBased,
}

/// Session strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStrategy {
    pub strategy: SessionNamingStrategy,
    pub manual_mappings: HashMap<String, String>,
}

impl Default for SessionStrategy {
    fn default() -> Self {
        Self {
            strategy: SessionNamingStrategy::PerDirectory,
            manual_mappings: HashMap::new(),
        }
    }
}

impl SessionStrategy {
    /// Resolve session key for a given directory
    pub fn resolve_session_key(&self, cwd: &Path, title: Option<&str>) -> String {
        // Check manual mapping first
        if let Some(key) = self.manual_mappings.get(&cwd.to_string_lossy().to_string()) {
            return key.clone();
        }

        // Check title-based
        if let Some(title) = title {
            if !title.is_empty() {
                return sanitize_session_key(title);
            }
        }

        // Apply strategy
        match self.strategy {
            SessionNamingStrategy::PerDirectory => cwd
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("default")
                .to_string(),
            SessionNamingStrategy::Global => "global".to_string(),
            SessionNamingStrategy::Manual => {
                // Fallback to per-directory if no manual mapping
                cwd.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("default")
                    .to_string()
            }
            SessionNamingStrategy::TitleBased => {
                // Fallback to per-directory if no title
                cwd.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("default")
                    .to_string()
            }
        }
    }
}

/// Sanitize session key
fn sanitize_session_key(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

/// Federated session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedSession {
    pub session_id: String,
    pub owner_peer_id: String,
    pub agent_peer_id: String,
    pub last_saved_index: usize,
}

impl FederatedSession {
    /// Create new federated session
    pub fn new(session_id: String, owner_peer_id: String, agent_peer_id: String) -> Self {
        Self {
            session_id,
            owner_peer_id,
            agent_peer_id,
            last_saved_index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_strategy_per_directory() {
        let strategy = SessionStrategy::default();
        let cwd = PathBuf::from("/home/user/projects/zaion-rust");
        let key = strategy.resolve_session_key(&cwd, None);
        assert_eq!(key, "zaion-rust");
    }

    #[test]
    fn test_session_strategy_global() {
        let strategy = SessionStrategy {
            strategy: SessionNamingStrategy::Global,
            ..SessionStrategy::default()
        };
        let cwd = PathBuf::from("/home/user/projects/zaion-rust");
        let key = strategy.resolve_session_key(&cwd, None);
        assert_eq!(key, "global");
    }

    #[test]
    fn test_session_strategy_manual() {
        let mut strategy = SessionStrategy::default();
        strategy.manual_mappings.insert(
            "/home/user/projects/zaion-rust".to_string(),
            "zaion-main".to_string(),
        );
        let cwd = PathBuf::from("/home/user/projects/zaion-rust");
        let key = strategy.resolve_session_key(&cwd, None);
        assert_eq!(key, "zaion-main");
    }

    #[test]
    fn test_session_strategy_title_based() {
        let strategy = SessionStrategy::default();
        let cwd = PathBuf::from("/home/user/projects/zaion-rust");
        let key = strategy.resolve_session_key(&cwd, Some("My Project Session"));
        assert_eq!(key, "my-project-session");
    }

    #[test]
    fn test_sanitize_session_key() {
        assert_eq!(sanitize_session_key("Hello World!"), "hello-world-");
        assert_eq!(sanitize_session_key("test@123"), "test-123");
        assert_eq!(sanitize_session_key("valid-key_123"), "valid-key_123");
    }

    #[test]
    fn sanitize_session_key_neutralizes_path_traversal() {
        // Session keys feed file paths; traversal must be neutralized.
        let s = sanitize_session_key("../etc/passwd");
        assert!(!s.contains(".."));
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
    }

    #[test]
    fn sanitize_session_key_empty_input_is_empty() {
        assert_eq!(sanitize_session_key(""), "");
    }

    #[test]
    fn test_federated_session_creation() {
        let session = FederatedSession::new(
            "session_1".to_string(),
            "owner_1".to_string(),
            "agent_1".to_string(),
        );
        assert_eq!(session.session_id, "session_1");
        assert_eq!(session.last_saved_index, 0);
    }
}
