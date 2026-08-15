use crate::builtin_tools::register_builtin_tools;
use crate::{McpError, McpToolRegistry};
/// MCP Dispatcher — validate, dispatch, and audit tool calls.
///
/// The dispatcher is the hot path for every tool invocation:
///   1. Parse McpCall (tool_name, input JSON)
///   2. Look up tool in registry
///   3. Validate input against schema (fill defaults)
///   4. Execute tool handler
///   5. Record McpCall + McpResult to event ledger (Ed25519 signed)
///   6. Return McpResult
use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::{NamespaceKey, RunId};

// ── McpCall ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCall {
    /// Unique call ID (UUID v4)
    pub call_id: String,
    pub tool_name: String,
    /// Optional: pin to specific version
    pub tool_version: Option<String>,
    /// Raw input parameters (JSON object)
    pub input: serde_json::Value,
    /// Caller context (e.g. task_id, session_id)
    pub context: Option<serde_json::Value>,
}

impl McpCall {
    pub fn new(tool_name: &str, input: serde_json::Value) -> Self {
        McpCall {
            call_id: format!("mcp-{}", uuid::Uuid::new_v4()),
            tool_name: tool_name.to_string(),
            tool_version: None,
            input,
            context: None,
        }
    }

    pub fn with_version(mut self, version: &str) -> Self {
        self.tool_version = Some(version.to_string());
        self
    }

    pub fn with_context(mut self, ctx: serde_json::Value) -> Self {
        self.context = Some(ctx);
        self
    }
}

// ── McpResult ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResult {
    pub call_id: String,
    pub tool_name: String,
    pub output: serde_json::Value,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl McpResult {
    fn ok(call: &McpCall, output: serde_json::Value, duration_ms: u64) -> Self {
        McpResult {
            call_id: call.call_id.clone(),
            tool_name: call.tool_name.clone(),
            output,
            success: true,
            error: None,
            duration_ms,
        }
    }

    fn err(call: &McpCall, error: String, duration_ms: u64) -> Self {
        McpResult {
            call_id: call.call_id.clone(),
            tool_name: call.tool_name.clone(),
            output: serde_json::Value::Null,
            success: false,
            error: Some(error),
            duration_ms,
        }
    }
}

// ── McpDispatcher ─────────────────────────────────────────────────────────────

pub struct McpDispatcher {
    registry: McpToolRegistry,
    ledger: EventLedger,
    keypair: ZaionKeypair,
    ns_key: NamespaceKey,
}

impl McpDispatcher {
    pub fn new(
        registry: McpToolRegistry,
        ledger: EventLedger,
        keypair: ZaionKeypair,
        ns_key: NamespaceKey,
    ) -> Self {
        let mut dispatcher = McpDispatcher {
            registry,
            ledger,
            keypair,
            ns_key,
        };
        register_builtin_tools(&mut dispatcher.registry);
        dispatcher
    }

    /// Dispatch a tool call. Validates, executes, audits.
    pub fn dispatch(&mut self, call: McpCall) -> McpResult {
        let start = std::time::Instant::now();
        let capability_class = self
            .resolve_capability_class(&call)
            .unwrap_or_else(|| "unknown".to_string());
        let input_hash = stable_hash_json(&call.input);

        let result = self.execute_call(&call);
        let duration_ms = start.elapsed().as_millis() as u64;

        let mcp_result = match result {
            Ok(output) => McpResult::ok(&call, output, duration_ms),
            Err(e) => McpResult::err(&call, e.to_string(), duration_ms),
        };
        let output_hash = stable_hash_json(&mcp_result.output);

        // Audit to ledger (best-effort — don't fail the call on ledger error)
        let payload = serde_json::json!({
            "call_id": mcp_result.call_id,
            "tool_name": mcp_result.tool_name,
            "success": mcp_result.success,
            "duration_ms": mcp_result.duration_ms,
            "error": mcp_result.error,
        });
        let run_id = RunId(mcp_result.call_id.clone());
        let _ = self.ledger.append_signed_event(
            &self.keypair,
            &self.ns_key,
            "mcp.tool_called",
            payload,
            Some(&run_id),
        );
        let receipt_payload = serde_json::json!({
            "schema": "zaion.tool_receipt.v1",
            "principal_id": self.keypair.principal_id().as_str(),
            "call_id": mcp_result.call_id,
            "tool_name": mcp_result.tool_name,
            "capability_class": capability_class,
            "source": "zaion-mcp",
            "input_hash": input_hash,
            "output_hash": output_hash,
            "success": mcp_result.success,
            "duration_ms": mcp_result.duration_ms,
            "error": mcp_result.error,
            "permission_decision": "allowed",
            "receipt_status": if mcp_result.success { "executed" } else { "failed" },
            "permission_proof": {
                "schema": "zaion.permission_proof.v1",
                "decision": "allowed",
                "enforced_at": "zaion_mcp::McpDispatcher::dispatch",
                "policy": "registered_tool_capability_class",
                "capability_class": capability_class,
                "tool_name": mcp_result.tool_name,
                "call_id": mcp_result.call_id,
            },
        });
        let _ = self.ledger.append_signed_event(
            &self.keypair,
            &self.ns_key,
            "tool.receipt",
            receipt_payload,
            Some(&run_id),
        );

        mcp_result
    }

    fn resolve_capability_class(&self, call: &McpCall) -> Option<String> {
        let tool = match &call.tool_version {
            Some(version) => self.registry.get_versioned(&call.tool_name, version),
            None => self.registry.get(&call.tool_name),
        }?;
        Some(tool.meta.capability_class.clone())
    }

    fn execute_call(&self, call: &McpCall) -> Result<serde_json::Value, McpError> {
        // Look up tool
        let tool = match &call.tool_version {
            Some(v) => self
                .registry
                .get_versioned(&call.tool_name, v)
                .ok_or_else(|| McpError::ToolNotFound(format!("{}@{}", call.tool_name, v)))?,
            None => self
                .registry
                .get(&call.tool_name)
                .ok_or_else(|| McpError::ToolNotFound(call.tool_name.clone()))?,
        };

        // Validate and fill input
        let validated = tool.meta.schema.validate_and_fill(&call.input)?;

        // Execute
        tool.call(validated).map_err(McpError::ExecutionFailed)
    }

    pub fn registry(&self) -> &McpToolRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut McpToolRegistry {
        &mut self.registry
    }

    /// Read-only access to the signed event ledger.
    ///
    /// Every `dispatch` appends Ed25519-signed `mcp.tool_called` and
    /// `tool.receipt` events. This accessor lets consumers (and auditors)
    /// read that trail back to verify signatures and chain integrity.
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// The public key bytes of the dispatcher's signing keypair.
    ///
    /// Use with [`zaion_ledger::verify_event_signature`] to verify the
    /// authenticity of audit events emitted by this dispatcher.
    pub fn public_key_bytes(&self) -> zaion_types::identity::PublicKeyBytes {
        self.keypair.public_key_bytes()
    }

    /// The principal id derived from the dispatcher's signing keypair.
    ///
    /// Use with [`EventLedger::verify_chain`] to verify hash-chain
    /// integrity for this dispatcher's audit events.
    pub fn principal_id(&self) -> zaion_types::identity::PrincipalId {
        self.keypair.principal_id()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

fn stable_hash_json(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    stable_hash_text(&encoded)
}

fn stable_hash_text(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta};
    use serde_json::json;
    use zaion_crypto::keypair::ZaionKeypair;
    use zaion_ledger::EventLedger;
    use zaion_types::session::NamespaceKey;

    fn test_dispatcher() -> McpDispatcher {
        let mut registry = McpToolRegistry::new();

        // Register an "add" tool
        let meta = McpToolMeta::new(
            "add",
            "1.0",
            "Adds two numbers",
            McpSchema::new(vec![
                McpParam::required("a", McpParamType::Number, "first number"),
                McpParam::required("b", McpParamType::Number, "second number"),
            ]),
            "read",
        );
        registry.register(McpTool::new(meta, |input| {
            let a = input["a"].as_f64().unwrap_or(0.0);
            let b = input["b"].as_f64().unwrap_or(0.0);
            Ok(json!({ "sum": a + b }))
        }));

        // Register a "greet" tool
        let greet_meta = McpToolMeta::new(
            "greet",
            "1.0",
            "Returns a greeting",
            McpSchema::new(vec![McpParam::required(
                "name",
                McpParamType::String,
                "person to greet",
            )]),
            "read",
        );
        registry.register(McpTool::new(greet_meta, |input| {
            let name = input["name"].as_str().unwrap_or("World");
            Ok(json!({ "greeting": format!("Hello, {name}!") }))
        }));

        let ledger = EventLedger::new(":memory:");
        let keypair = ZaionKeypair::generate();
        let ns_key = NamespaceKey("test-ns".to_string());

        McpDispatcher::new(registry, ledger, keypair, ns_key)
    }

    #[test]
    fn dispatch_add_tool_succeeds() {
        let mut d = test_dispatcher();
        let call = McpCall::new("add", json!({"a": 3.0, "b": 4.0}));
        let result = d.dispatch(call);
        assert!(result.success);
        assert_eq!(result.output["sum"], json!(7.0));
    }

    #[test]
    fn dispatch_greet_tool_succeeds() {
        let mut d = test_dispatcher();
        let call = McpCall::new("greet", json!({"name": "Zaion"}));
        let result = d.dispatch(call);
        assert!(result.success);
        assert_eq!(result.output["greeting"], json!("Hello, Zaion!"));
    }

    #[test]
    fn dispatch_unknown_tool_fails_gracefully() {
        let mut d = test_dispatcher();
        let call = McpCall::new("nonexistent", json!({}));
        let result = d.dispatch(call);
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("not found"));
    }

    #[test]
    fn dispatch_schema_validation_error() {
        let mut d = test_dispatcher();
        let call = McpCall::new("add", json!({"a": "not-a-number", "b": 4}));
        let result = d.dispatch(call);
        assert!(!result.success);
    }

    #[test]
    fn dispatch_records_call_id() {
        let mut d = test_dispatcher();
        let call = McpCall::new("greet", json!({"name": "test"}));
        let call_id = call.call_id.clone();
        let result = d.dispatch(call);
        assert_eq!(result.call_id, call_id);
    }

    #[test]
    fn dispatch_writes_standard_tool_receipt_with_permission_proof() {
        let mut registry = McpToolRegistry::new();
        let meta = McpToolMeta::new(
            "add",
            "1.0",
            "Adds two numbers",
            McpSchema::new(vec![
                McpParam::required("a", McpParamType::Number, "first number"),
                McpParam::required("b", McpParamType::Number, "second number"),
            ]),
            "read",
        );
        registry.register(McpTool::new(meta, |input| {
            Ok(json!({ "sum": input["a"].as_f64().unwrap() + input["b"].as_f64().unwrap() }))
        }));

        let ledger = EventLedger::new(":memory:");
        let keypair = ZaionKeypair::generate();
        let ns_key = NamespaceKey("tool-receipt-test".to_string());
        let mut dispatcher = McpDispatcher::new(registry, ledger, keypair, ns_key.clone());
        let result = dispatcher.dispatch(McpCall::new("add", json!({"a": 2.0, "b": 5.0})));
        assert!(result.success);

        let events = dispatcher
            .ledger
            .list_events(
                &zaion_types::session::SessionKey(ns_key.0),
                Some("tool.receipt"),
                10,
            )
            .expect("receipt events");
        assert_eq!(events.len(), 1);
        let receipt = &events[0].payload;
        assert_eq!(receipt["schema"], "zaion.tool_receipt.v1");
        assert_eq!(receipt["tool_name"], "add");
        assert_eq!(receipt["capability_class"], "read");
        assert_eq!(receipt["permission_decision"], "allowed");
        assert_eq!(receipt["receipt_status"], "executed");
        assert_eq!(
            receipt["permission_proof"]["schema"],
            "zaion.permission_proof.v1"
        );
        assert_eq!(receipt["permission_proof"]["decision"], "allowed");
        assert_eq!(
            receipt["permission_proof"]["enforced_at"],
            "zaion_mcp::McpDispatcher::dispatch"
        );
        assert!(receipt["input_hash"].as_str().unwrap().len() == 64);
        assert!(receipt["output_hash"].as_str().unwrap().len() == 64);
    }
}
