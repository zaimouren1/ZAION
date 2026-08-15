use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    Read,
    Write,
    Execute,
    Memory,
    Network,
    External,
}

impl CapabilityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityClass::Read => "read",
            CapabilityClass::Write => "write",
            CapabilityClass::Execute => "execute",
            CapabilityClass::Memory => "memory",
            CapabilityClass::Network => "network",
            CapabilityClass::External => "external",
        }
    }

    /// Parse a tool-metadata capability string, **failing closed**.
    ///
    /// An unrecognized capability class must NOT silently downgrade to the most
    /// permissive `Read` (which the concurrency scheduler treats as pure and
    /// parallel-safe). Instead, unknown metadata maps to `Execute` — the most
    /// restrictive class — so a tool with malformed or future metadata is
    /// scheduled serially and sandboxed under the tightest scope until its
    /// capability is explicitly declared.
    ///
    /// Use [`CapabilityClass::try_from_tool_meta`] when the caller needs to
    /// distinguish "unknown" from a real class.
    pub fn from_tool_meta(value: &str) -> Self {
        Self::try_from_tool_meta(value).unwrap_or(CapabilityClass::Execute)
    }

    /// Strict parse: returns `None` for any capability string that is not an
    /// explicitly recognized class. Callers that schedule or sandbox based on
    /// capability should treat `None` as fail-closed (most restrictive).
    pub fn try_from_tool_meta(value: &str) -> Option<Self> {
        match value {
            "read" => Some(CapabilityClass::Read),
            "write" => Some(CapabilityClass::Write),
            "execute" => Some(CapabilityClass::Execute),
            "memory" => Some(CapabilityClass::Memory),
            "network" => Some(CapabilityClass::Network),
            "diagnostic" | "external" => Some(CapabilityClass::External),
            _ => None,
        }
    }

    /// True if a tool of this capability class is pure / idempotent enough to be
    /// executed concurrently with sibling safe calls. Only observation-style
    /// classes qualify; anything with write/execute/network side effects must
    /// run serially to preserve causal ordering.
    pub fn is_concurrency_safe(self) -> bool {
        matches!(
            self,
            CapabilityClass::Read | CapabilityClass::Memory | CapabilityClass::External
        )
    }

    pub fn default_sandbox_scope(self) -> &'static str {
        match self {
            CapabilityClass::Execute => "workspace_allowlisted_shell",
            CapabilityClass::Write => "workspace_write_policy",
            CapabilityClass::Memory => "principal_memory_atoms",
            CapabilityClass::Network => "configured_network_endpoint",
            CapabilityClass::External => "mcp_stdio_subprocess",
            CapabilityClass::Read => "workspace_readonly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

impl PolicyEffect {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyEffect::Allow => "allow",
            PolicyEffect::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub schema: String,
    pub permission_id: String,
    pub capability_class: String,
    pub effect: String,
    pub sandbox_scope: String,
    pub reason_code: String,
    pub enforced_at: String,
}

impl PolicyDecision {
    pub const SCHEMA: &'static str = "zaion.policy_decision.v1";

    pub fn allow_builtin(tool_name: &str, capability_class: CapabilityClass) -> Self {
        Self::new(
            format!("builtin.{}.{}", tool_name, capability_class.as_str()),
            capability_class,
            PolicyEffect::Allow,
            capability_class.default_sandbox_scope(),
            "native_builtin_dispatch_allowed",
            "zaion_mcp::builtin_tools",
        )
    }

    pub fn failed_builtin(tool_name: &str, capability_class: CapabilityClass) -> Self {
        Self::new(
            format!("builtin.{}.{}", tool_name, capability_class.as_str()),
            capability_class,
            PolicyEffect::Allow,
            capability_class.default_sandbox_scope(),
            "native_builtin_dispatch_failed",
            "zaion_mcp::builtin_tools",
        )
    }

    pub fn allow_mcp(tool_name: &str) -> Self {
        Self::new(
            format!("mcp.{}.external", tool_name),
            CapabilityClass::External,
            PolicyEffect::Allow,
            CapabilityClass::External.default_sandbox_scope(),
            "mcp_registry_dispatch_allowed",
            "zaion_runtime::mcp_tools",
        )
    }

    pub fn failed_mcp(tool_name: &str) -> Self {
        Self::new(
            format!("mcp.{}.external", tool_name),
            CapabilityClass::External,
            PolicyEffect::Allow,
            CapabilityClass::External.default_sandbox_scope(),
            "mcp_registry_dispatch_failed",
            "zaion_runtime::mcp_tools",
        )
    }

    pub fn deny_unknown_tool(tool_name: &str) -> Self {
        Self::new(
            format!("unknown.{}.none", tool_name),
            CapabilityClass::External,
            PolicyEffect::Deny,
            "none",
            "denied_unknown_tool_no_mcp_registry",
            "zaion_cli::commands::process::wake",
        )
    }

    /// A PreToolUse lifecycle hook vetoed the call before execution.
    ///
    /// Ported from Claude Code's hook contract (exit-code-2 = block): the tool
    /// never runs, and the deny receipt records which hook blocked it so the
    /// decision is auditable in the signed ledger.
    pub fn denied_by_hook(tool_name: &str, hook_name: &str) -> Self {
        Self::new(
            format!("hook.{}.{}.blocked", hook_name, tool_name),
            CapabilityClass::External,
            PolicyEffect::Deny,
            "none",
            "denied_by_pre_tool_use_hook",
            "zaion_runtime::hooks",
        )
    }

    pub fn recorded_not_executed(tool_name: &str) -> Self {
        Self::new(
            format!("recorded.{}.none", tool_name),
            CapabilityClass::External,
            PolicyEffect::Deny,
            "none",
            "not_executed_requires_explicit_dispatch",
            "zaion_cli::commands::process::wake",
        )
    }

    fn new(
        permission_id: String,
        capability_class: CapabilityClass,
        effect: PolicyEffect,
        sandbox_scope: &str,
        reason_code: &str,
        enforced_at: &str,
    ) -> Self {
        Self {
            schema: Self::SCHEMA.to_string(),
            permission_id,
            capability_class: capability_class.as_str().to_string(),
            effect: effect.as_str().to_string(),
            sandbox_scope: sandbox_scope.to_string(),
            reason_code: reason_code.to_string(),
            enforced_at: enforced_at.to_string(),
        }
    }

    pub fn permission_proof(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": self.schema,
            "permission_id": self.permission_id,
            "capability_class": self.capability_class,
            "effect": self.effect,
            "sandbox_scope": self.sandbox_scope,
            "reason_code": self.reason_code,
            "enforced_at": self.enforced_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_decision_serializes_shared_receipt_contract() {
        let decision = PolicyDecision::allow_builtin("fs_read", CapabilityClass::Read);
        let value = serde_json::to_value(&decision).unwrap();

        assert_eq!(value["schema"], "zaion.policy_decision.v1");
        assert_eq!(value["permission_id"], "builtin.fs_read.read");
        assert_eq!(value["capability_class"], "read");
        assert_eq!(value["effect"], "allow");
        assert_eq!(value["sandbox_scope"], "workspace_readonly");
        assert_eq!(value["reason_code"], "native_builtin_dispatch_allowed");
        assert_eq!(value["enforced_at"], "zaion_mcp::builtin_tools");
    }

    #[test]
    fn from_tool_meta_recognizes_known_classes() {
        assert_eq!(
            CapabilityClass::from_tool_meta("read"),
            CapabilityClass::Read
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("write"),
            CapabilityClass::Write
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("execute"),
            CapabilityClass::Execute
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("memory"),
            CapabilityClass::Memory
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("network"),
            CapabilityClass::Network
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("diagnostic"),
            CapabilityClass::External
        );
        assert_eq!(
            CapabilityClass::from_tool_meta("external"),
            CapabilityClass::External
        );
    }

    #[test]
    fn from_tool_meta_fails_closed_to_execute() {
        // Unknown / malformed / future capability strings must NOT downgrade to
        // the permissive Read; they fail closed to the most restrictive class.
        for unknown in ["", "utility", "system", "READ", "garbage", "rea d"] {
            assert_eq!(
                CapabilityClass::from_tool_meta(unknown),
                CapabilityClass::Execute,
                "'{unknown}' should fail closed to Execute"
            );
        }
    }

    #[test]
    fn try_from_tool_meta_distinguishes_unknown() {
        assert_eq!(
            CapabilityClass::try_from_tool_meta("read"),
            Some(CapabilityClass::Read)
        );
        assert_eq!(CapabilityClass::try_from_tool_meta("utility"), None);
        assert_eq!(CapabilityClass::try_from_tool_meta(""), None);
    }

    #[test]
    fn concurrency_safe_only_for_observation_classes() {
        assert!(CapabilityClass::Read.is_concurrency_safe());
        assert!(CapabilityClass::Memory.is_concurrency_safe());
        assert!(CapabilityClass::External.is_concurrency_safe());
        assert!(!CapabilityClass::Write.is_concurrency_safe());
        assert!(!CapabilityClass::Execute.is_concurrency_safe());
        assert!(!CapabilityClass::Network.is_concurrency_safe());
    }
}
