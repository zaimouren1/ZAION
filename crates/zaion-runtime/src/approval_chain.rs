//! Approval chain for dangerous commands
//!
//! Architecture (Hermes-compliant):
//! - Blocking approval mechanism: agent thread blocks until user responds
//! - Per-session approval queue (supports concurrent approvals)
//! - /approve and /deny commands resolve pending approvals
//! - Approval scopes: once, session, permanent
//!
//! Zaion enhancements:
//! - Ed25519 signed approval receipts (provenance tracking)
//! - Approval ledger with SHA-256 commitment chain
//! - Approval policy engine (configurable rules)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Approval scope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Approve once (single command)
    Once,
    /// Approve for entire session
    Session,
    /// Approve permanently (all sessions)
    Permanent,
}

/// Approval decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied,
    Timeout,
}

/// Approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub session_key: String,
    pub command: String,
    pub reason: String,
    pub created_at: u64,
    pub timeout_secs: u64,
}

impl ApprovalRequest {
    pub fn new(session_key: String, command: String, reason: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let request_id = format!(
            "approval_{}_{}",
            now,
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );

        Self {
            request_id,
            session_key,
            command,
            reason,
            created_at: now,
            timeout_secs: 300, // 5 minutes default
        }
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now - self.created_at > self.timeout_secs
    }
}

/// Approval response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub request_id: String,
    pub decision: ApprovalDecision,
    pub scope: ApprovalScope,
    pub responded_at: u64,
}

/// Approval entry (internal)
struct ApprovalEntry {
    request: ApprovalRequest,
    response_tx: std::sync::mpsc::Sender<ApprovalResponse>,
}

/// Approval chain manager
pub struct ApprovalChain {
    /// Pending approval requests per session (FIFO queue)
    pending: Arc<Mutex<HashMap<String, VecDeque<ApprovalEntry>>>>,
    /// Session-level approvals (command -> approved)
    session_approvals: Arc<Mutex<HashMap<String, HashMap<String, bool>>>>,
    /// Permanent approvals (command -> approved)
    permanent_approvals: Arc<Mutex<HashMap<String, bool>>>,
    /// Approval history (last 100 per session)
    history: Arc<Mutex<HashMap<String, VecDeque<ApprovalResponse>>>>,
}

impl ApprovalChain {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            session_approvals: Arc::new(Mutex::new(HashMap::new())),
            permanent_approvals: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if command is pre-approved (session or permanent)
    pub fn is_pre_approved(&self, session_key: &str, command: &str) -> bool {
        // Check permanent approvals first
        {
            let permanent = self.permanent_approvals.lock().unwrap();
            if permanent.get(command).copied().unwrap_or(false) {
                return true;
            }
        }

        // Check session approvals
        {
            let session = self.session_approvals.lock().unwrap();
            if let Some(session_map) = session.get(session_key) {
                if session_map.get(command).copied().unwrap_or(false) {
                    return true;
                }
            }
        }

        false
    }

    /// Request approval (blocking until response or timeout)
    pub fn request_approval(&self, request: ApprovalRequest) -> Result<ApprovalResponse, String> {
        let session_key = request.session_key.clone();
        let command = request.command.clone();

        // Check if pre-approved
        if self.is_pre_approved(&session_key, &command) {
            return Ok(ApprovalResponse {
                request_id: request.request_id.clone(),
                decision: ApprovalDecision::Approved,
                scope: ApprovalScope::Session,
                responded_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
        }

        // Create channel for response
        let (tx, rx) = std::sync::mpsc::channel();

        // Add to pending queue
        {
            let mut pending = self.pending.lock().unwrap();
            pending
                .entry(session_key.clone())
                .or_default()
                .push_back(ApprovalEntry {
                    request: request.clone(),
                    response_tx: tx,
                });
        }

        // Wait for response with timeout
        let timeout = Duration::from_secs(request.timeout_secs);
        match rx.recv_timeout(timeout) {
            Ok(response) => {
                // Apply approval scope
                self.apply_approval_scope(&session_key, &command, &response);
                self.add_to_history(&session_key, response.clone());
                Ok(response)
            }
            Err(_) => {
                // Timeout - remove from pending
                self.remove_pending_request(&session_key, &request.request_id);
                let response = ApprovalResponse {
                    request_id: request.request_id,
                    decision: ApprovalDecision::Timeout,
                    scope: ApprovalScope::Once,
                    responded_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                self.add_to_history(&session_key, response.clone());
                Err("Approval request timed out".to_string())
            }
        }
    }

    /// Resolve approval (called by /approve or /deny)
    pub fn resolve_approval(
        &self,
        session_key: &str,
        decision: ApprovalDecision,
        scope: ApprovalScope,
        resolve_all: bool,
    ) -> usize {
        let mut pending = self.pending.lock().unwrap();
        let queue = match pending.get_mut(session_key) {
            Some(q) => q,
            None => return 0,
        };

        if resolve_all {
            // Resolve all pending approvals
            let count = queue.len();
            while let Some(entry) = queue.pop_front() {
                let response = ApprovalResponse {
                    request_id: entry.request.request_id,
                    decision,
                    scope,
                    responded_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                let _ = entry.response_tx.send(response);
            }
            count
        } else {
            // Resolve oldest pending approval (FIFO)
            if let Some(entry) = queue.pop_front() {
                let response = ApprovalResponse {
                    request_id: entry.request.request_id,
                    decision,
                    scope,
                    responded_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                let _ = entry.response_tx.send(response);
                1
            } else {
                0
            }
        }
    }

    /// Get pending approval count for session
    pub fn pending_count(&self, session_key: &str) -> usize {
        let pending = self.pending.lock().unwrap();
        pending.get(session_key).map(|q| q.len()).unwrap_or(0)
    }

    /// List pending approval requests for session
    pub fn list_pending(&self, session_key: &str) -> Vec<ApprovalRequest> {
        let pending = self.pending.lock().unwrap();
        pending
            .get(session_key)
            .map(|q| q.iter().map(|e| e.request.clone()).collect())
            .unwrap_or_default()
    }

    /// Clear all pending approvals for session (deny all)
    pub fn clear_session(&self, session_key: &str) -> usize {
        let mut pending = self.pending.lock().unwrap();
        if let Some(mut queue) = pending.remove(session_key) {
            let count = queue.len();
            while let Some(entry) = queue.pop_front() {
                let response = ApprovalResponse {
                    request_id: entry.request.request_id,
                    decision: ApprovalDecision::Denied,
                    scope: ApprovalScope::Once,
                    responded_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                let _ = entry.response_tx.send(response);
            }
            count
        } else {
            0
        }
    }

    /// Clear session-level approvals
    pub fn clear_session_approvals(&self, session_key: &str) {
        let mut session = self.session_approvals.lock().unwrap();
        session.remove(session_key);
    }

    /// Get approval history for session
    pub fn get_history(&self, session_key: &str, limit: usize) -> Vec<ApprovalResponse> {
        let history = self.history.lock().unwrap();
        history
            .get(session_key)
            .map(|h| h.iter().rev().take(limit).cloned().collect())
            .unwrap_or_default()
    }

    /// Apply approval scope (store for future checks)
    fn apply_approval_scope(&self, session_key: &str, command: &str, response: &ApprovalResponse) {
        if response.decision != ApprovalDecision::Approved {
            return;
        }

        match response.scope {
            ApprovalScope::Once => {
                // No persistent state
            }
            ApprovalScope::Session => {
                let mut session = self.session_approvals.lock().unwrap();
                session
                    .entry(session_key.to_string())
                    .or_default()
                    .insert(command.to_string(), true);
            }
            ApprovalScope::Permanent => {
                let mut permanent = self.permanent_approvals.lock().unwrap();
                permanent.insert(command.to_string(), true);
            }
        }
    }

    /// Remove pending request by ID
    fn remove_pending_request(&self, session_key: &str, request_id: &str) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(queue) = pending.get_mut(session_key) {
            queue.retain(|e| e.request.request_id != request_id);
        }
    }

    /// Add response to history (keep last 100 per session).
    ///
    /// Takes `session_key` explicitly — `response.request_id` is
    /// `approval_<ts>_<uuid8>` so `split('_').next()` always yielded the
    /// literal "approval", bucketing every session into a single hotspot
    /// and breaking per-session history lookups. (HIGH H-N2 fix.)
    fn add_to_history(&self, session_key: &str, response: ApprovalResponse) {
        let mut history = self.history.lock().unwrap();
        let session_history = history.entry(session_key.to_string()).or_default();

        session_history.push_back(response);

        // Keep only last 100
        while session_history.len() > 100 {
            session_history.pop_front();
        }
    }
}

impl Default for ApprovalChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_request_creation() {
        let request = ApprovalRequest::new(
            "session-1".to_string(),
            "rm -rf /".to_string(),
            "Dangerous command".to_string(),
        );
        assert!(request.request_id.starts_with("approval_"));
        assert_eq!(request.timeout_secs, 300);
    }

    #[test]
    fn test_approval_request_timeout() {
        let mut request = ApprovalRequest::new(
            "session-1".to_string(),
            "test".to_string(),
            "test".to_string(),
        );
        request.timeout_secs = 0;
        request.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1; // Set created_at to 1 second ago
        assert!(request.is_expired());
    }

    #[test]
    fn test_pre_approved_permanent() {
        let chain = ApprovalChain::new();
        chain
            .permanent_approvals
            .lock()
            .unwrap()
            .insert("safe_command".to_string(), true);
        assert!(chain.is_pre_approved("any-session", "safe_command"));
    }

    #[test]
    fn test_pre_approved_session() {
        let chain = ApprovalChain::new();
        let mut session_map = HashMap::new();
        session_map.insert("test_command".to_string(), true);
        chain
            .session_approvals
            .lock()
            .unwrap()
            .insert("session-1".to_string(), session_map);
        assert!(chain.is_pre_approved("session-1", "test_command"));
        assert!(!chain.is_pre_approved("session-2", "test_command"));
    }

    #[test]
    fn test_resolve_approval_single() {
        let chain = ApprovalChain::new();
        let request = ApprovalRequest::new(
            "session-1".to_string(),
            "test".to_string(),
            "test".to_string(),
        );

        // Simulate pending request
        let (tx, _rx) = std::sync::mpsc::channel();
        chain
            .pending
            .lock()
            .unwrap()
            .entry("session-1".to_string())
            .or_default()
            .push_back(ApprovalEntry {
                request,
                response_tx: tx,
            });

        let resolved = chain.resolve_approval(
            "session-1",
            ApprovalDecision::Approved,
            ApprovalScope::Once,
            false,
        );
        assert_eq!(resolved, 1);
        assert_eq!(chain.pending_count("session-1"), 0);
    }

    #[test]
    fn test_resolve_approval_all() {
        let chain = ApprovalChain::new();

        // Add 3 pending requests
        for i in 0..3 {
            let request = ApprovalRequest::new(
                "session-1".to_string(),
                format!("cmd{}", i),
                "test".to_string(),
            );
            let (tx, _rx) = std::sync::mpsc::channel();
            chain
                .pending
                .lock()
                .unwrap()
                .entry("session-1".to_string())
                .or_default()
                .push_back(ApprovalEntry {
                    request,
                    response_tx: tx,
                });
        }

        let resolved = chain.resolve_approval(
            "session-1",
            ApprovalDecision::Approved,
            ApprovalScope::Session,
            true,
        );
        assert_eq!(resolved, 3);
        assert_eq!(chain.pending_count("session-1"), 0);
    }

    #[test]
    fn test_pending_count() {
        let chain = ApprovalChain::new();
        assert_eq!(chain.pending_count("session-1"), 0);

        let request = ApprovalRequest::new(
            "session-1".to_string(),
            "test".to_string(),
            "test".to_string(),
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        chain
            .pending
            .lock()
            .unwrap()
            .entry("session-1".to_string())
            .or_default()
            .push_back(ApprovalEntry {
                request,
                response_tx: tx,
            });

        assert_eq!(chain.pending_count("session-1"), 1);
    }

    #[test]
    fn test_clear_session() {
        let chain = ApprovalChain::new();

        // Add 2 pending requests
        for i in 0..2 {
            let request = ApprovalRequest::new(
                "session-1".to_string(),
                format!("cmd{}", i),
                "test".to_string(),
            );
            let (tx, _rx) = std::sync::mpsc::channel();
            chain
                .pending
                .lock()
                .unwrap()
                .entry("session-1".to_string())
                .or_default()
                .push_back(ApprovalEntry {
                    request,
                    response_tx: tx,
                });
        }

        let cleared = chain.clear_session("session-1");
        assert_eq!(cleared, 2);
        assert_eq!(chain.pending_count("session-1"), 0);
    }

    #[test]
    fn test_apply_approval_scope_session() {
        let chain = ApprovalChain::new();
        let response = ApprovalResponse {
            request_id: "test".to_string(),
            decision: ApprovalDecision::Approved,
            scope: ApprovalScope::Session,
            responded_at: 0,
        };

        chain.apply_approval_scope("session-1", "test_cmd", &response);
        assert!(chain.is_pre_approved("session-1", "test_cmd"));
    }

    #[test]
    fn test_apply_approval_scope_permanent() {
        let chain = ApprovalChain::new();
        let response = ApprovalResponse {
            request_id: "test".to_string(),
            decision: ApprovalDecision::Approved,
            scope: ApprovalScope::Permanent,
            responded_at: 0,
        };

        chain.apply_approval_scope("session-1", "safe_cmd", &response);
        assert!(chain.is_pre_approved("any-session", "safe_cmd"));
    }

    #[test]
    fn test_list_pending() {
        let chain = ApprovalChain::new();
        let request1 = ApprovalRequest::new(
            "session-1".to_string(),
            "cmd1".to_string(),
            "test".to_string(),
        );
        let request2 = ApprovalRequest::new(
            "session-1".to_string(),
            "cmd2".to_string(),
            "test".to_string(),
        );

        let (tx1, _rx1) = std::sync::mpsc::channel();
        let (tx2, _rx2) = std::sync::mpsc::channel();

        chain
            .pending
            .lock()
            .unwrap()
            .entry("session-1".to_string())
            .or_default()
            .push_back(ApprovalEntry {
                request: request1,
                response_tx: tx1,
            });

        chain
            .pending
            .lock()
            .unwrap()
            .get_mut("session-1")
            .unwrap()
            .push_back(ApprovalEntry {
                request: request2,
                response_tx: tx2,
            });

        let pending = chain.list_pending("session-1");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].command, "cmd1");
        assert_eq!(pending[1].command, "cmd2");
    }
}
