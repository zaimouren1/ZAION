use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionResetPolicy {
    None,
    Daily,
    Idle,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetPolicyConfig {
    pub default_policy: SessionResetPolicy,
    pub reset_by_platform: std::collections::HashMap<String, SessionResetPolicy>,
    pub reset_by_type: std::collections::HashMap<String, SessionResetPolicy>,
    pub idle_timeout_minutes: i64,
    pub reset_triggers: Vec<String>,
}

impl Default for ResetPolicyConfig {
    fn default() -> Self {
        Self {
            default_policy: SessionResetPolicy::None,
            reset_by_platform: std::collections::HashMap::new(),
            reset_by_type: std::collections::HashMap::new(),
            idle_timeout_minutes: 60,
            reset_triggers: vec!["/new".into(), "/reset".into()],
        }
    }
}

pub fn resolve_reset_policy(
    cfg: &ResetPolicyConfig,
    platform: &str,
    session_type: &str,
) -> SessionResetPolicy {
    cfg.reset_by_platform
        .get(platform)
        .cloned()
        .or_else(|| cfg.reset_by_type.get(session_type).cloned())
        .unwrap_or_else(|| cfg.default_policy.clone())
}

pub fn should_reset_for_trigger(cfg: &ResetPolicyConfig, message: &str) -> bool {
    cfg.reset_triggers
        .iter()
        .any(|trigger| message.trim_start().starts_with(trigger))
}

pub fn should_reset_for_idle(
    last_updated_rfc3339: &str,
    now_rfc3339: &str,
    idle_timeout_minutes: i64,
) -> bool {
    let last = chrono::DateTime::parse_from_rfc3339(last_updated_rfc3339).ok();
    let now = chrono::DateTime::parse_from_rfc3339(now_rfc3339).ok();
    match (last, now) {
        (Some(last), Some(now)) => {
            now.signed_duration_since(last).num_minutes() >= idle_timeout_minutes
        }
        _ => false,
    }
}

pub fn should_reset_for_new_day(last_updated_rfc3339: &str, now_rfc3339: &str) -> bool {
    let last = chrono::DateTime::parse_from_rfc3339(last_updated_rfc3339).ok();
    let now = chrono::DateTime::parse_from_rfc3339(now_rfc3339).ok();
    match (last, now) {
        (Some(last), Some(now)) => last.date_naive() != now.date_naive(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_rule_overrides_type_and_default() {
        let mut cfg = ResetPolicyConfig {
            default_policy: SessionResetPolicy::None,
            ..ResetPolicyConfig::default()
        };
        cfg.reset_by_type
            .insert("group".into(), SessionResetPolicy::Idle);
        cfg.reset_by_platform
            .insert("telegram".into(), SessionResetPolicy::Daily);
        assert_eq!(
            resolve_reset_policy(&cfg, "telegram", "group"),
            SessionResetPolicy::Daily
        );
    }

    #[test]
    fn type_rule_used_when_platform_missing() {
        let mut cfg = ResetPolicyConfig::default();
        cfg.reset_by_type
            .insert("dm".into(), SessionResetPolicy::Idle);
        assert_eq!(
            resolve_reset_policy(&cfg, "discord", "dm"),
            SessionResetPolicy::Idle
        );
    }

    #[test]
    fn trigger_matches_reset_commands() {
        let cfg = ResetPolicyConfig::default();
        assert!(should_reset_for_trigger(&cfg, "/new session"));
        assert!(should_reset_for_trigger(&cfg, "   /reset now"));
        assert!(!should_reset_for_trigger(&cfg, "hello"));
    }

    #[test]
    fn idle_timeout_detected() {
        assert!(should_reset_for_idle(
            "2026-04-12T00:00:00+00:00",
            "2026-04-12T02:00:00+00:00",
            60
        ));
        assert!(!should_reset_for_idle(
            "2026-04-12T00:00:00+00:00",
            "2026-04-12T00:30:00+00:00",
            60
        ));
    }

    #[test]
    fn new_day_detected() {
        assert!(should_reset_for_new_day(
            "2026-04-12T23:59:00+00:00",
            "2026-04-13T00:01:00+00:00"
        ));
        assert!(!should_reset_for_new_day(
            "2026-04-12T08:00:00+00:00",
            "2026-04-12T20:00:00+00:00"
        ));
    }
}
