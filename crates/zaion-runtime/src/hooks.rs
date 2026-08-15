use crate::{RuntimeError, SkillSandbox};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
/// Hooks subsystem (C3.4) — event-driven skill execution.
///
/// A Hook is a (trigger_pattern, handler_script) pair stored in SQLite.
/// When an event fires matching the trigger, HookRunner executes the handler
/// in a SkillSandbox and records the result to the ledger.
use std::path::{Path, PathBuf};
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    pub hook_id: String,
    pub name: String,
    /// Glob-style trigger pattern: "message.received", "cron.*", "process.*"
    pub trigger: String,
    /// Path to handler script (.ts/.js/.py/.sh or executable).
    pub handler_path: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub hook_id: String,
    pub hook_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Decision returned by the PreToolUse lifecycle gate.
///
/// Ported from Claude Code's hook contract: a `PreToolUse` hook can veto a
/// tool call *before* it executes. `Allow` lets the call proceed; `Block`
/// stops it and surfaces a reason back to the model as the tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookGate {
    Allow,
    Block { hook_name: String, reason: String },
}

impl HookGate {
    pub fn is_blocked(&self) -> bool {
        matches!(self, HookGate::Block { .. })
    }
}

impl HookResult {
    /// Interpret this hook's output as a PreToolUse gate decision.
    ///
    /// Fail-closed contract (matching Claude Code's exit-code-2 = block):
    /// - A hook that *failed* to run blocks the tool call (we cannot trust a
    ///   silent failure to have approved a side-effecting tool).
    /// - A successful hook blocks when its JSON output declares
    ///   `{"decision":"block"}` or `{"permission":"deny"}` /
    ///   `{"permissionDecision":"deny"}`. The optional `reason` string is
    ///   surfaced back to the caller.
    /// - Any other successful output is treated as approval (`None`).
    pub fn pre_tool_use_gate(&self) -> Option<HookGate> {
        if !self.success {
            return Some(HookGate::Block {
                hook_name: self.hook_name.clone(),
                reason: self
                    .error
                    .clone()
                    .unwrap_or_else(|| "hook execution failed".to_string()),
            });
        }
        let decision = self.output.get("decision").and_then(|v| v.as_str());
        let permission = self
            .output
            .get("permission")
            .and_then(|v| v.as_str())
            .or_else(|| {
                self.output
                    .get("permissionDecision")
                    .and_then(|v| v.as_str())
            });
        let blocked = matches!(decision, Some("block") | Some("deny"))
            || matches!(permission, Some("deny") | Some("block"));
        if blocked {
            let reason = self
                .output
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("blocked by pre_tool_use hook")
                .to_string();
            Some(HookGate::Block {
                hook_name: self.hook_name.clone(),
                reason,
            })
        } else {
            None
        }
    }
}

pub struct HookStore {
    db_path: PathBuf,
}

impl HookStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn conn(&self) -> Result<Connection, RuntimeError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RuntimeError::Internal(e.to_string()))?;
        }
        let conn =
            Connection::open(&self.db_path).map_err(|e| RuntimeError::Internal(e.to_string()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-64000;
            PRAGMA temp_store=MEMORY;
            PRAGMA mmap_size=268435456;
            CREATE TABLE IF NOT EXISTS hooks (
                hook_id      TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                trigger      TEXT NOT NULL,
                handler_path TEXT NOT NULL,
                enabled      INTEGER NOT NULL DEFAULT 1,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_hooks_trigger ON hooks(trigger);
        ",
        )
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(conn)
    }

    pub fn install(
        &self,
        name: &str,
        trigger: &str,
        handler_path: &str,
    ) -> Result<HookDef, RuntimeError> {
        let now = chrono::Utc::now().to_rfc3339();
        let hook_id = format!("hook-{}", uuid::Uuid::new_v4());
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO hooks (hook_id, name, trigger, handler_path, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
            params![hook_id, name, trigger, handler_path, now],
        ).map_err(|e| RuntimeError::Internal(e.to_string()))?;
        Ok(HookDef {
            hook_id,
            name: name.to_string(),
            trigger: trigger.to_string(),
            handler_path: handler_path.to_string(),
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list(&self) -> Result<Vec<HookDef>, RuntimeError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT hook_id, name, trigger, handler_path, enabled, created_at, updated_at FROM hooks ORDER BY created_at"
        ).map_err(|e| RuntimeError::Internal(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(HookDef {
                    hook_id: row.get(0)?,
                    name: row.get(1)?,
                    trigger: row.get(2)?,
                    handler_path: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    }

    pub fn set_enabled(&self, hook_id: &str, enabled: bool) -> Result<(), RuntimeError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE hooks SET enabled = ?1, updated_at = ?2 WHERE hook_id = ?3",
                params![enabled as i64, now, hook_id],
            )
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        if rows == 0 {
            return Err(RuntimeError::Internal(format!(
                "hook not found: {}",
                hook_id
            )));
        }
        Ok(())
    }

    pub fn remove(&self, hook_id: &str) -> Result<(), RuntimeError> {
        let conn = self.conn()?;
        let rows = conn
            .execute("DELETE FROM hooks WHERE hook_id = ?1", params![hook_id])
            .map_err(|e| RuntimeError::Internal(e.to_string()))?;
        if rows == 0 {
            return Err(RuntimeError::Internal(format!(
                "hook not found: {}",
                hook_id
            )));
        }
        Ok(())
    }

    /// Return all enabled hooks whose trigger matches `event_type`.
    /// Supports exact match and wildcard suffix: "cron.*" matches "cron.triggered".
    pub fn matching(&self, event_type: &str) -> Result<Vec<HookDef>, RuntimeError> {
        let all = self.list()?;
        Ok(all
            .into_iter()
            .filter(|h| h.enabled && trigger_matches(&h.trigger, event_type))
            .collect())
    }
}

/// True if trigger pattern matches event_type.
/// Supports: exact "foo.bar", prefix wildcard "foo.*", global "*".
fn trigger_matches(pattern: &str, event_type: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern == event_type {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return event_type.starts_with(prefix);
    }
    false
}

pub struct HookRunner {
    store: HookStore,
    sandbox: SkillSandbox,
}

/// Canonical lifecycle event names fired around the native tool loop.
///
/// These mirror Claude Code's hook events so a single hook subsystem can
/// gate/observe tool execution as well as serve the existing manual triggers.
impl HookRunner {
    /// Fired *before* a tool executes. A matching hook can block the call.
    pub const EVENT_PRE_TOOL_USE: &'static str = "tool.pre_use";
    /// Fired *after* a tool executes (observation only).
    pub const EVENT_POST_TOOL_USE: &'static str = "tool.post_use";
    /// Fired once when the wake/agent turn begins.
    pub const EVENT_SESSION_START: &'static str = "session.start";
    /// Fired once when the wake/agent turn ends.
    pub const EVENT_STOP: &'static str = "session.stop";
}

impl HookRunner {
    pub fn new(
        db_path: impl AsRef<Path>,
        ledger: EventLedger,
        keypair: ZaionKeypair,
        namespace_key: NamespaceKey,
    ) -> Self {
        Self {
            store: HookStore::new(db_path),
            sandbox: SkillSandbox::new(ledger, keypair, namespace_key),
        }
    }

    /// Fire all hooks matching `event_type` with `payload` as input.
    /// Never panics — failures are captured in HookResult.
    pub fn fire(&self, event_type: &str, payload: serde_json::Value) -> Vec<HookResult> {
        let hooks = match self.store.matching(event_type) {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };
        hooks
            .into_iter()
            .map(|hook| {
                let path = Path::new(&hook.handler_path);
                let start = std::time::Instant::now();
                match self.sandbox.run(path, payload.clone()) {
                    Ok(result) => HookResult {
                        hook_id: hook.hook_id,
                        hook_name: hook.name,
                        success: true,
                        output: result.output,
                        error: None,
                        duration_ms: result.duration_ms,
                    },
                    Err(e) => HookResult {
                        hook_id: hook.hook_id,
                        hook_name: hook.name,
                        success: false,
                        output: serde_json::Value::Null,
                        error: Some(e.to_string()),
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                }
            })
            .collect()
    }

    /// True if any hook is registered for `event_type`. Lets the caller skip
    /// building a payload (and signing/sandbox setup) on the hot path when no
    /// lifecycle hooks are installed — the common case.
    pub fn has_hooks_for(&self, event_type: &str) -> bool {
        self.store
            .matching(event_type)
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// PreToolUse gate: fire all `tool.pre_use` hooks and collapse their
    /// results into a single allow/block decision. The first blocking hook
    /// wins (fail-closed). Returns `HookGate::Allow` when no hook is installed.
    pub fn fire_pre_tool_use(&self, tool_name: &str, arguments: &serde_json::Value) -> HookGate {
        if !self.has_hooks_for(Self::EVENT_PRE_TOOL_USE) {
            return HookGate::Allow;
        }
        let payload = serde_json::json!({
            "event": Self::EVENT_PRE_TOOL_USE,
            "tool_name": tool_name,
            "arguments": arguments,
        });
        for result in self.fire(Self::EVENT_PRE_TOOL_USE, payload) {
            if let Some(gate @ HookGate::Block { .. }) = result.pre_tool_use_gate() {
                return gate;
            }
        }
        HookGate::Allow
    }

    /// PostToolUse observation: fire all `tool.post_use` hooks with the tool's
    /// outcome. Observation only — results are returned for logging/ledgering
    /// but never alter control flow.
    pub fn fire_post_tool_use(
        &self,
        tool_name: &str,
        success: bool,
        output_preview: &str,
    ) -> Vec<HookResult> {
        if !self.has_hooks_for(Self::EVENT_POST_TOOL_USE) {
            return Vec::new();
        }
        let payload = serde_json::json!({
            "event": Self::EVENT_POST_TOOL_USE,
            "tool_name": tool_name,
            "success": success,
            "output_preview": output_preview,
        });
        self.fire(Self::EVENT_POST_TOOL_USE, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_result(output: serde_json::Value) -> HookResult {
        HookResult {
            hook_id: "hook-1".to_string(),
            hook_name: "guard".to_string(),
            success: true,
            output,
            error: None,
            duration_ms: 1,
        }
    }

    #[test]
    fn trigger_matches_exact_prefix_and_global() {
        assert!(trigger_matches("tool.pre_use", "tool.pre_use"));
        assert!(trigger_matches("tool.*", "tool.pre_use"));
        assert!(trigger_matches("*", "anything.at.all"));
        assert!(!trigger_matches("tool.pre_use", "tool.post_use"));
        assert!(!trigger_matches("cron.*", "tool.pre_use"));
    }

    #[test]
    fn failed_hook_blocks_fail_closed() {
        let result = HookResult {
            hook_id: "h".into(),
            hook_name: "broken".into(),
            success: false,
            output: serde_json::Value::Null,
            error: Some("boom".into()),
            duration_ms: 0,
        };
        match result.pre_tool_use_gate() {
            Some(HookGate::Block { hook_name, reason }) => {
                assert_eq!(hook_name, "broken");
                assert_eq!(reason, "boom");
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn decision_block_is_honored_with_reason() {
        let result = ok_result(serde_json::json!({
            "decision": "block",
            "reason": "writes outside workspace"
        }));
        assert_eq!(
            result.pre_tool_use_gate(),
            Some(HookGate::Block {
                hook_name: "guard".into(),
                reason: "writes outside workspace".into(),
            })
        );
    }

    #[test]
    fn permission_deny_variants_block() {
        for output in [
            serde_json::json!({"permission": "deny"}),
            serde_json::json!({"permissionDecision": "deny"}),
            serde_json::json!({"decision": "deny"}),
        ] {
            assert!(
                ok_result(output.clone()).pre_tool_use_gate().is_some(),
                "expected {output} to block"
            );
        }
    }

    #[test]
    fn approval_outputs_do_not_block() {
        for output in [
            serde_json::json!({}),
            serde_json::json!({"decision": "allow"}),
            serde_json::json!({"permission": "allow"}),
            serde_json::json!({"note": "looks fine"}),
        ] {
            assert_eq!(
                ok_result(output.clone()).pre_tool_use_gate(),
                None,
                "expected {output} to allow"
            );
        }
    }

    #[test]
    fn block_reason_falls_back_when_missing() {
        let result = ok_result(serde_json::json!({"decision": "block"}));
        match result.pre_tool_use_gate() {
            Some(HookGate::Block { reason, .. }) => {
                assert_eq!(reason, "blocked by pre_tool_use hook");
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_event_constants_are_stable() {
        assert_eq!(HookRunner::EVENT_PRE_TOOL_USE, "tool.pre_use");
        assert_eq!(HookRunner::EVENT_POST_TOOL_USE, "tool.post_use");
        assert_eq!(HookRunner::EVENT_SESSION_START, "session.start");
        assert_eq!(HookRunner::EVENT_STOP, "session.stop");
    }

    #[test]
    fn hook_gate_is_blocked_helper() {
        assert!(HookGate::Block {
            hook_name: "x".into(),
            reason: "y".into()
        }
        .is_blocked());
        assert!(!HookGate::Allow.is_blocked());
    }
}
