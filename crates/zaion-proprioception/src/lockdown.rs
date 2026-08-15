//! Lockdown State — enforcement layer for transplantation shock
//!
//! When shock severity reaches Moderate or Severe, the system engages
//! a lockdown that blocks outbound requests via a global flag. Unlock must be
//! performed through a verified challenge/token path; arbitrary non-empty
//! strings must never be accepted as proof of authorization.
use crate::ShockSeverity;
use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex, OnceLock};

/// Global singleton lockdown state, lazily initialised.
static GLOBAL_LOCKDOWN: OnceLock<Arc<Mutex<LockdownState>>> = OnceLock::new();

/// Returns a reference-counted handle to the process-wide lockdown state.
pub fn global_lockdown() -> Arc<Mutex<LockdownState>> {
    GLOBAL_LOCKDOWN
        .get_or_init(|| Arc::new(Mutex::new(LockdownState::new())))
        .clone()
}

/// Tracks whether the runtime is currently in lockdown.
#[derive(Debug, Clone)]
pub struct LockdownState {
    /// Whether the lockdown is currently active.
    pub locked: bool,
    /// Human-readable reason for the lockdown.
    pub reason: String,
    /// Severity level that triggered this lockdown.
    pub severity: ShockSeverity,
    /// UTC timestamp when lockdown was engaged, if active.
    pub locked_at: Option<DateTime<Utc>>,
    /// Pairing challenge token required to disengage (placeholder).
    pub unlock_token: Option<String>,
}

impl LockdownState {
    /// Create a new, inactive lockdown state.
    pub fn new() -> Self {
        Self {
            locked: false,
            reason: String::new(),
            severity: ShockSeverity::None,
            locked_at: None,
            unlock_token: None,
        }
    }

    /// Engage lockdown with the given severity and reason string.
    ///
    /// Idempotent: if already locked at the same or higher severity,
    /// the existing lockdown is preserved (only escalations overwrite).
    pub fn engage(&mut self, severity: ShockSeverity, reason: String) {
        // Escalate severity order: None < Mild < Moderate < Severe
        if self.locked && !is_more_severe(severity, self.severity) {
            return;
        }
        self.locked = true;
        self.reason = reason;
        self.severity = severity;
        self.locked_at = Some(Utc::now());
        self.unlock_token = None;
    }

    /// Disengage the lockdown unconditionally.
    ///
    /// In production the caller is responsible for verifying the unlock
    /// token via Ed25519 before calling this. The CLI `propri unlock`
    /// command performs that check.
    pub fn disengage(&mut self) {
        self.locked = false;
        self.reason = String::new();
        self.severity = ShockSeverity::None;
        self.locked_at = None;
        self.unlock_token = None;
    }

    /// Disengage the lockdown only when a previously issued unlock token
    /// matches the provided response.
    ///
    /// This is intentionally strict: if no challenge/token has been issued,
    /// the method rejects every response. That keeps unfinished pairing flows
    /// from degrading into "any non-empty code unlocks the system".
    pub fn disengage_with_token(&mut self, token: &str) -> Result<(), String> {
        if !self.locked {
            return Ok(());
        }

        let token = token.trim();
        if token.is_empty() {
            return Err("unlock token must not be empty".to_string());
        }

        let expected = self.unlock_token.as_deref().ok_or_else(|| {
            "no verified unlock challenge is active; refusing arbitrary unlock code".to_string()
        })?;

        if token != expected {
            return Err("unlock token did not match active challenge".to_string());
        }

        self.disengage();
        Ok(())
    }

    /// Returns `true` when the system is currently locked down.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Returns a display-ready summary line.
    pub fn summary(&self) -> String {
        if self.locked {
            let ts = self
                .locked_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "LOCKED [{:?}] since {} — {}",
                self.severity, ts, self.reason
            )
        } else {
            "UNLOCKED".to_string()
        }
    }
}

impl Default for LockdownState {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if `candidate` is strictly more severe than `current`.
fn is_more_severe(candidate: ShockSeverity, current: ShockSeverity) -> bool {
    severity_rank(candidate) > severity_rank(current)
}

fn severity_rank(s: ShockSeverity) -> u8 {
    match s {
        ShockSeverity::None => 0,
        ShockSeverity::Mild => 1,
        ShockSeverity::Moderate => 2,
        ShockSeverity::Severe => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_unlocked() {
        let s = LockdownState::new();
        assert!(!s.is_locked());
    }

    #[test]
    fn engage_sets_locked() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Moderate, "test reason".to_string());
        assert!(s.is_locked());
        assert_eq!(s.severity, ShockSeverity::Moderate);
        assert!(s.locked_at.is_some());
    }

    #[test]
    fn disengage_clears_state() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Severe, "severe".to_string());
        s.disengage();
        assert!(!s.is_locked());
        assert_eq!(s.severity, ShockSeverity::None);
    }

    #[test]
    fn token_unlock_rejects_when_no_challenge_is_active() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Severe, "severe".to_string());
        let result = s.disengage_with_token("anything");

        assert!(result.is_err());
        assert!(s.is_locked());
    }

    #[test]
    fn token_unlock_rejects_wrong_token() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Severe, "severe".to_string());
        s.unlock_token = Some("expected-token".to_string());
        let result = s.disengage_with_token("wrong-token");

        assert!(result.is_err());
        assert!(s.is_locked());
    }

    #[test]
    fn token_unlock_accepts_matching_token() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Severe, "severe".to_string());
        s.unlock_token = Some("expected-token".to_string());
        let result = s.disengage_with_token("expected-token");

        assert!(result.is_ok());
        assert!(!s.is_locked());
    }

    #[test]
    fn engage_escalates_severity() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Moderate, "first".to_string());
        s.engage(ShockSeverity::Severe, "escalated".to_string());
        assert_eq!(s.severity, ShockSeverity::Severe);
        assert_eq!(s.reason, "escalated");
    }

    #[test]
    fn engage_does_not_downgrade() {
        let mut s = LockdownState::new();
        s.engage(ShockSeverity::Severe, "severe".to_string());
        s.engage(ShockSeverity::Moderate, "attempt downgrade".to_string());
        // Should still be Severe
        assert_eq!(s.severity, ShockSeverity::Severe);
        assert_eq!(s.reason, "severe");
    }

    #[test]
    fn global_lockdown_returns_same_arc() {
        let a = global_lockdown();
        let b = global_lockdown();
        // Both point to the same allocation
        assert!(Arc::ptr_eq(&a, &b));
    }
}
