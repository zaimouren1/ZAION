// ledger.rs — ACI 操作事件写入 Ledger
//
// H33 fix: implement basic AciLedger that wraps EventLedger and provides
// typed methods for logging ACI operations (WriteFile, ReadFile, SyntaxCheck,
// ReplaceAstNode, RealityCheck).

use serde_json::Value;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

/// ACI operation logger — wraps EventLedger with ACI-specific event types.
pub struct AciLedger {
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace: NamespaceKey,
}

impl AciLedger {
    /// Create a new ACI ledger backed by the given EventLedger.
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair, namespace: NamespaceKey) -> Self {
        Self {
            ledger,
            keypair,
            namespace,
        }
    }

    /// Log an ACI WriteFile operation.
    pub fn log_write_file(
        &self,
        op_id: &str,
        path: &str,
        content_hash: &str,
        success: bool,
    ) -> Result<(), zaion_ledger::LedgerError> {
        let payload = serde_json::json!({
            "op_id": op_id,
            "path": path,
            "content_hash": content_hash,
            "success": success,
        });
        self.append("aci.write_file", payload)
    }

    /// Log an ACI ReadFile operation.
    pub fn log_read_file(
        &self,
        op_id: &str,
        path: &str,
        success: bool,
    ) -> Result<(), zaion_ledger::LedgerError> {
        let payload = serde_json::json!({
            "op_id": op_id,
            "path": path,
            "success": success,
        });
        self.append("aci.read_file", payload)
    }

    /// Log an ACI SyntaxCheck operation.
    pub fn log_syntax_check(
        &self,
        op_id: &str,
        path: &str,
        language: &str,
        valid: bool,
    ) -> Result<(), zaion_ledger::LedgerError> {
        let payload = serde_json::json!({
            "op_id": op_id,
            "path": path,
            "language": language,
            "valid": valid,
        });
        self.append("aci.syntax_check", payload)
    }

    /// Log an ACI ReplaceAstNode operation.
    pub fn log_replace_ast_node(
        &self,
        op_id: &str,
        path: &str,
        old_node: &str,
        new_node: &str,
        success: bool,
    ) -> Result<(), zaion_ledger::LedgerError> {
        let payload = serde_json::json!({
            "op_id": op_id,
            "path": path,
            "old_node": old_node,
            "new_node": new_node,
            "success": success,
        });
        self.append("aci.replace_ast_node", payload)
    }

    /// Log an ACI RealityCheck operation.
    pub fn log_reality_check(
        &self,
        op_id: &str,
        path: &str,
        matches_reality: bool,
    ) -> Result<(), zaion_ledger::LedgerError> {
        let payload = serde_json::json!({
            "op_id": op_id,
            "path": path,
            "matches_reality": matches_reality,
        });
        self.append("aci.reality_check", payload)
    }

    /// Internal helper: append event with signature.
    fn append(&self, event_type: &str, payload: Value) -> Result<(), zaion_ledger::LedgerError> {
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace,
            event_type,
            payload,
            None,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_aci_ledger_write_file() {
        let dir = tempdir().unwrap();
        let ledger_path = dir.path().join("aci_ledger.db");
        let ledger = EventLedger::new(&ledger_path);
        let keypair = ZaionKeypair::generate();
        let ns = NamespaceKey("test-ns".to_string());
        let aci_ledger = AciLedger::new(ledger, keypair, ns);

        let result = aci_ledger.log_write_file("op-001", "/tmp/test.rs", "abc123", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_aci_ledger_syntax_check() {
        let dir = tempdir().unwrap();
        let ledger_path = dir.path().join("aci_ledger.db");
        let ledger = EventLedger::new(&ledger_path);
        let keypair = ZaionKeypair::generate();
        let ns = NamespaceKey("test-ns".to_string());
        let aci_ledger = AciLedger::new(ledger, keypair, ns);

        let result = aci_ledger.log_syntax_check("op-002", "/tmp/test.rs", "rust", true);
        assert!(result.is_ok());
    }
}
