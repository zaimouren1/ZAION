use zaion_runtime::TurnProof;

#[derive(Debug, Default, Clone)]
pub(crate) struct ToolReceiptProofJoinSummary {
    pub event_id: Option<String>,
    pub summary: Option<serde_json::Value>,
    pub found: bool,
    pub proof_hash_verified: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ToolResultStorageReceiptSummary {
    pub receipts: Vec<serde_json::Value>,
}

pub(crate) fn tool_receipt_proof_join_for_turn_proof(
    ledger: &zaion_ledger::EventLedger,
    proof_event: &zaion_types::event::LedgerEvent,
    proof: &TurnProof,
) -> Result<ToolReceiptProofJoinSummary, zaion_ledger::LedgerError> {
    if proof.tool_receipt_ids.is_empty() {
        return Ok(ToolReceiptProofJoinSummary::default());
    }
    let session_key = zaion_types::session::SessionKey(proof.namespace_key.clone());
    let mut latest_join = None;
    for receipt_id in &proof.tool_receipt_ids {
        latest_join = ledger
            .list_events_by_payload_string_array_contains(
                &session_key,
                "tool.receipt.proof_join",
                "tool_receipt_ids",
                receipt_id,
                1,
            )?
            .into_iter()
            .next();
        if latest_join.is_none() {
            return Ok(ToolReceiptProofJoinSummary::default());
        }
    }
    let Some(join) = latest_join else {
        return Ok(ToolReceiptProofJoinSummary::default());
    };
    let proof_hash_matches = join
        .payload
        .get("turn_proof_hash")
        .and_then(|value| value.as_str())
        == Some(proof.proof_hash.as_str());
    let proof_event_matches = join
        .payload
        .get("turn_proof_event_id")
        .and_then(|value| value.as_str())
        == Some(proof_event.event_id.0.as_str());
    let summary = serde_json::json!({
        "schema": join.payload.get("schema").cloned().unwrap_or_else(|| serde_json::json!("zaion.tool_receipt_proof_join.v1")),
        "event_id": join.event_id.0,
        "signed": join.signature.is_some(),
        "parent_turn_proof_event_id": join.parent_event_id.as_ref().map(|id| id.0.clone()),
        "turn_proof_event_id": join.payload.get("turn_proof_event_id").cloned().unwrap_or(serde_json::Value::Null),
        "tool_receipt_ids": join.payload.get("tool_receipt_ids").cloned().unwrap_or_else(|| serde_json::json!([])),
        "tool_receipt_count": join.payload.get("tool_receipt_count").cloned().unwrap_or(serde_json::Value::Null),
        "turn_proof_hash": join.payload.get("turn_proof_hash").cloned().unwrap_or(serde_json::Value::Null),
        "proof_hash_matches_turn_proof": proof_hash_matches,
        "turn_proof_event_matches": proof_event_matches,
    });
    Ok(ToolReceiptProofJoinSummary {
        event_id: Some(join.event_id.0),
        summary: Some(summary),
        found: true,
        proof_hash_verified: proof_hash_matches && proof_event_matches,
    })
}

pub(crate) fn tool_result_storage_receipts(
    ledger: &zaion_ledger::EventLedger,
    receipt_ids: &[String],
) -> Result<ToolResultStorageReceiptSummary, zaion_ledger::LedgerError> {
    let mut receipts = Vec::new();
    for receipt_id in receipt_ids {
        let Some(event) = ledger.get_event(receipt_id)? else {
            continue;
        };
        let Some(storage) = event.payload.get("tool_result_storage") else {
            continue;
        };
        if storage.is_null() {
            continue;
        }
        receipts.push(serde_json::json!({
            "receipt_event_id": event.event_id.0,
            "signed": event.signature.is_some(),
            "tool_name": event.payload.get("tool_name").cloned().unwrap_or(serde_json::Value::Null),
            "tool_call_id": event.payload.get("tool_call_id").cloned().unwrap_or(serde_json::Value::Null),
            "receipt_status": event.payload.get("receipt_status").cloned().unwrap_or(serde_json::Value::Null),
            "tool_result_storage": storage.clone(),
            "tool_result_storage_binding": event.payload.get("tool_result_storage_binding").cloned().unwrap_or(serde_json::Value::Null),
        }));
    }
    Ok(ToolResultStorageReceiptSummary { receipts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_crypto::ZaionKeypair;
    use zaion_types::event::EventType;
    use zaion_types::session::NamespaceKey;

    #[test]
    fn tool_result_storage_receipts_summarizes_persisted_storage_and_environment_binding() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-receipt-storage-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let ledger = zaion_ledger::EventLedger::new(root.join("receipt-storage.db"));
        let keypair = ZaionKeypair::generate();
        let namespace = NamespaceKey(keypair.principal_id().as_str().to_string());
        let output_event_id = ledger
            .append_signed_typed_event(
                &keypair,
                &namespace,
                EventType::ChannelSent,
                serde_json::json!({"text": "tool output parent"}),
                None,
            )
            .unwrap();
        let receipt_id = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::ToolReceipt,
                serde_json::json!({
                    "schema": "zaion.tool_receipt.v1",
                    "tool_name": "fs_read",
                    "tool_call_id": "call_large",
                    "receipt_status": "executed",
                    "tool_result_storage": {
                        "schema": "zaion.tool_result_storage.v1",
                        "tool_name": "fs_read",
                        "tool_call_id": "call_large",
                        "stored": true,
                        "truncated": true,
                        "bytes": 65536,
                        "preview_bytes": 4000,
                        "path": "D:/zaion-rust/.zaion/tool-results/fs_read/call_large.txt",
                        "storage_root": "D:/zaion-rust/.zaion/tool-results/fs_read",
                        "environment_id": "docker:workspace:zaion-main:container-42",
                        "environment_kind": "docker"
                    },
                    "tool_result_storage_binding": {
                        "schema": "zaion.tool_result_storage_binding.v1",
                        "environment": {
                            "environment_id": "docker:workspace:zaion-main:container-42",
                            "environment_kind": "docker",
                            "path": "D:/zaion-rust/.zaion/tool-results/fs_read/call_large.txt"
                        },
                        "binding_hash": "binding-hash"
                    }
                }),
                None,
                Some(&output_event_id),
            )
            .unwrap();

        let summary =
            tool_result_storage_receipts(&ledger, std::slice::from_ref(&receipt_id.0)).unwrap();

        assert_eq!(summary.receipts.len(), 1);
        assert_eq!(
            summary.receipts[0]["receipt_event_id"],
            serde_json::json!(receipt_id.0)
        );
        assert_eq!(summary.receipts[0]["signed"], serde_json::json!(true));
        assert_eq!(
            summary.receipts[0]["tool_name"],
            serde_json::json!("fs_read")
        );
        assert_eq!(
            summary.receipts[0]["tool_result_storage"]["environment_id"],
            serde_json::json!("docker:workspace:zaion-main:container-42")
        );
        assert_eq!(
            summary.receipts[0]["tool_result_storage_binding"]["environment"]["environment_kind"],
            serde_json::json!("docker")
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
