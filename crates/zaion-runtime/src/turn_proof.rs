use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnContextLayer {
    pub layer: u8,
    pub label: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnCompressionEvidence {
    pub schema: String,
    pub compression_requested: bool,
    pub was_compressed: bool,
    pub original_turns: usize,
    pub compressed_turns: usize,
    pub turns_pruned: usize,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub token_budget: usize,
    pub trigger_threshold: usize,
    pub summary_hash: String,
    #[serde(default)]
    pub summary_strategy: String,
    #[serde(default)]
    pub pruned_tool_outputs: usize,
    #[serde(default)]
    pub protected_head_turns: usize,
    #[serde(default)]
    pub protected_tail_turns: usize,
    #[serde(default)]
    pub protected_tail_tokens: usize,
    #[serde(default)]
    pub summary_budget_tokens: usize,
    pub evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnCanonicalUsageEvidence {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnCostEvidence {
    pub schema: String,
    pub provider: String,
    pub model: String,
    pub billing_provider: String,
    pub billing_mode: String,
    pub usage: TurnCanonicalUsageEvidence,
    pub cost_status: String,
    pub cost_source: String,
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub actual_cost_usd: Option<f64>,
    pub session_estimated_cost_usd: f64,
    #[serde(default)]
    pub session_actual_cost_usd: Option<f64>,
    #[serde(default)]
    pub pricing_version: Option<String>,
    #[serde(default)]
    pub rollup_event_id: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnRuntimeMemoryEvidence {
    pub schema: String,
    pub memory_enabled: bool,
    pub memory_context_bytes: usize,
    pub memory_context_hash: String,
    pub fenced_context: bool,
    pub evidence_hash: String,
}

impl TurnRuntimeMemoryEvidence {
    pub fn from_context(memory_enabled: bool, memory_context: &str) -> Option<Self> {
        if !memory_enabled || memory_context.is_empty() {
            return None;
        }
        let mut evidence = Self {
            schema: "zaion.runtime_memory_evidence.v1".to_string(),
            memory_enabled,
            memory_context_bytes: memory_context.len(),
            memory_context_hash: stable_hash_bytes(memory_context.as_bytes()),
            fenced_context: memory_context.contains("<memory-context>")
                && memory_context.contains("</memory-context>"),
            evidence_hash: String::new(),
        };
        evidence.evidence_hash = runtime_memory_evidence_hash(&evidence);
        Some(evidence)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCapabilityManifest {
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub memory_enabled: bool,
    pub mcp_enabled: bool,
    pub cache_enabled: bool,
    pub smart_route_enabled: bool,
    pub compression_requested: bool,
    pub tools_requested: Vec<String>,
    pub boundaries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnProofInput {
    pub principal_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub namespace_key: String,
    pub user_event_id: String,
    pub output_event_id: String,
    pub omni_route_event_id: Option<String>,
    pub omni_route_authority_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace_transition_event_id: Option<String>,
    pub identity_contract: String,
    pub capability_manifest: TurnCapabilityManifest,
    pub context_pack_id: Option<String>,
    pub context_layers: Vec<TurnContextLayer>,
    pub memory_atom_ids: Vec<String>,
    pub compression_evidence: Option<TurnCompressionEvidence>,
    pub cost_evidence: Option<TurnCostEvidence>,
    pub runtime_memory_evidence: Option<TurnRuntimeMemoryEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_graph_hash: Option<String>,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub tool_call_count: usize,
    #[serde(default)]
    pub tool_receipt_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnProof {
    pub schema_version: u8,
    pub proof_id: String,
    pub principal_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub namespace_key: String,
    pub user_event_id: String,
    pub output_event_id: String,
    pub omni_route_event_id: Option<String>,
    pub omni_route_authority_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace_transition_event_id: Option<String>,
    pub event_lineage: Vec<String>,
    pub identity_contract_hash: String,
    pub capability_manifest_hash: String,
    pub context_pack_id: Option<String>,
    pub context_digest: String,
    pub context_layers: Vec<TurnContextLayer>,
    pub memory_atom_ids: Vec<String>,
    #[serde(default)]
    pub compression_evidence: Option<TurnCompressionEvidence>,
    #[serde(default)]
    pub compression_evidence_hash: Option<String>,
    #[serde(default)]
    pub cost_evidence: Option<TurnCostEvidence>,
    #[serde(default)]
    pub cost_evidence_hash: Option<String>,
    #[serde(default)]
    pub runtime_memory_evidence: Option<TurnRuntimeMemoryEvidence>,
    #[serde(default)]
    pub runtime_memory_evidence_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_graph_hash: Option<String>,
    pub capability_manifest: TurnCapabilityManifest,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub tool_call_count: usize,
    #[serde(default)]
    pub tool_receipt_ids: Vec<String>,
    #[serde(default)]
    pub tool_receipt_count: usize,
    pub proof_hash: String,
}

pub fn build_turn_proof(input: TurnProofInput) -> TurnProof {
    let identity_contract_hash = stable_hash_bytes(input.identity_contract.as_bytes());
    let capability_manifest_hash = stable_hash_json(&input.capability_manifest);
    let context_digest = stable_hash_json(&serde_json::json!({
        "context_layers": &input.context_layers,
        "memory_atom_ids": &input.memory_atom_ids,
        "compression_evidence": &input.compression_evidence,
        "cost_evidence": &input.cost_evidence,
        "runtime_memory_evidence": &input.runtime_memory_evidence,
    }));
    let compression_evidence_hash = input
        .compression_evidence
        .as_ref()
        .map(|evidence| evidence.evidence_hash.clone());
    let cost_evidence_hash = input
        .cost_evidence
        .as_ref()
        .map(|evidence| evidence.evidence_hash.clone());
    let runtime_memory_evidence_hash = input
        .runtime_memory_evidence
        .as_ref()
        .map(|evidence| evidence.evidence_hash.clone());
    let proof_seed = format!(
        "{}:{}:{}:{}",
        input.principal_id, input.namespace_key, input.user_event_id, input.output_event_id
    );
    let proof_id = format!(
        "turn-proof-{}",
        &stable_hash_bytes(proof_seed.as_bytes())[..16]
    );
    let mut event_lineage = vec![input.user_event_id.clone()];
    if let Some(omni_route_event_id) = &input.omni_route_event_id {
        event_lineage.push(omni_route_event_id.clone());
    }
    if let Some(namespace_transition_event_id) = &input.namespace_transition_event_id {
        event_lineage.push(namespace_transition_event_id.clone());
    }
    event_lineage.push(input.output_event_id.clone());
    event_lineage.extend(input.tool_receipt_ids.iter().cloned());
    let tool_receipt_count = input.tool_receipt_ids.len();

    let mut proof = TurnProof {
        schema_version: 1,
        proof_id,
        principal_id: input.principal_id,
        workspace_id: input.workspace_id,
        project_id: input.project_id,
        channel_id: input.channel_id,
        thread_id: input.thread_id,
        namespace_key: input.namespace_key,
        user_event_id: input.user_event_id,
        output_event_id: input.output_event_id,
        omni_route_event_id: input.omni_route_event_id,
        omni_route_authority_hash: input.omni_route_authority_hash,
        namespace_transition_event_id: input.namespace_transition_event_id,
        event_lineage,
        identity_contract_hash,
        capability_manifest_hash,
        context_digest,
        context_pack_id: input.context_pack_id,
        context_layers: input.context_layers,
        memory_atom_ids: input.memory_atom_ids,
        compression_evidence: input.compression_evidence,
        compression_evidence_hash,
        cost_evidence: input.cost_evidence,
        cost_evidence_hash,
        runtime_memory_evidence: input.runtime_memory_evidence,
        runtime_memory_evidence_hash,
        evidence_graph_hash: input.evidence_graph_hash,
        capability_manifest: input.capability_manifest,
        tokens_in: input.tokens_in,
        tokens_out: input.tokens_out,
        tool_call_count: input.tool_call_count,
        tool_receipt_ids: input.tool_receipt_ids,
        tool_receipt_count,
        proof_hash: String::new(),
    };
    proof.proof_hash = stable_hash_json(&proof);
    proof
}

pub fn verify_turn_proof_hash(proof: &TurnProof) -> bool {
    if proof.proof_hash.is_empty() {
        return false;
    }
    let mut normalized = proof.clone();
    let expected = std::mem::take(&mut normalized.proof_hash);
    stable_hash_json(&normalized) == expected
}

pub fn stable_hash_json<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("serializing turn proof data cannot fail");
    stable_hash_bytes(&bytes)
}

pub fn stable_hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn runtime_memory_evidence_hash(evidence: &TurnRuntimeMemoryEvidence) -> String {
    stable_hash_json(&serde_json::json!({
        "schema": evidence.schema,
        "memory_enabled": evidence.memory_enabled,
        "memory_context_bytes": evidence.memory_context_bytes,
        "memory_context_hash": evidence.memory_context_hash,
        "fenced_context": evidence.fenced_context,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> TurnCapabilityManifest {
        TurnCapabilityManifest {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.1),
            memory_enabled: false,
            mcp_enabled: false,
            cache_enabled: false,
            smart_route_enabled: false,
            compression_requested: false,
            tools_requested: vec!["fs_read".to_string()],
            boundaries: vec!["ledger_event_lineage_required".to_string()],
        }
    }

    #[test]
    fn turn_proof_records_tool_receipt_ids_in_lineage() {
        let proof = build_turn_proof(TurnProofInput {
            principal_id: "pid-test".to_string(),
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            channel_id: "terminal".to_string(),
            thread_id: "main".to_string(),
            namespace_key: "session-test".to_string(),
            user_event_id: "evt-user".to_string(),
            output_event_id: "evt-output".to_string(),
            omni_route_event_id: Some("evt-route".to_string()),
            omni_route_authority_hash: Some("route-hash".to_string()),
            namespace_transition_event_id: Some("evt-transition".to_string()),
            identity_contract: "identity-contract".to_string(),
            capability_manifest: test_manifest(),
            context_pack_id: None,
            context_layers: Vec::new(),
            memory_atom_ids: Vec::new(),
            compression_evidence: None,
            cost_evidence: None,
            runtime_memory_evidence: None,
            evidence_graph_hash: None,
            tokens_in: 10,
            tokens_out: 20,
            tool_call_count: 1,
            tool_receipt_ids: vec!["evt-receipt-1".to_string(), "evt-receipt-2".to_string()],
        });

        assert_eq!(
            proof.tool_receipt_ids,
            vec!["evt-receipt-1".to_string(), "evt-receipt-2".to_string()]
        );
        assert_eq!(
            proof.event_lineage,
            vec![
                "evt-user".to_string(),
                "evt-route".to_string(),
                "evt-transition".to_string(),
                "evt-output".to_string(),
                "evt-receipt-1".to_string(),
                "evt-receipt-2".to_string(),
            ]
        );
        assert_eq!(proof.tool_receipt_count, 2);
        assert!(verify_turn_proof_hash(&proof));

        let mut tampered = proof.clone();
        tampered.namespace_transition_event_id = Some("evt-transition-tampered".to_string());
        assert!(!verify_turn_proof_hash(&tampered));
    }

    #[test]
    fn optional_evidence_graph_hash_is_bound_without_breaking_v1_none_shape() {
        let proof = build_turn_proof(TurnProofInput {
            principal_id: "pid-test".to_string(),
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            channel_id: "terminal".to_string(),
            thread_id: "main".to_string(),
            namespace_key: "session-test".to_string(),
            user_event_id: "evt-user".to_string(),
            output_event_id: "evt-output".to_string(),
            omni_route_event_id: None,
            omni_route_authority_hash: None,
            namespace_transition_event_id: None,
            identity_contract: "identity-contract".to_string(),
            capability_manifest: test_manifest(),
            context_pack_id: None,
            context_layers: Vec::new(),
            memory_atom_ids: Vec::new(),
            compression_evidence: None,
            cost_evidence: None,
            runtime_memory_evidence: None,
            evidence_graph_hash: Some("evidence-graph-hash".to_string()),
            tokens_in: 0,
            tokens_out: 0,
            tool_call_count: 0,
            tool_receipt_ids: Vec::new(),
        });

        assert_eq!(
            proof.evidence_graph_hash.as_deref(),
            Some("evidence-graph-hash")
        );
        assert!(verify_turn_proof_hash(&proof));
        assert!(
            serde_json::to_value(&proof)
                .expect("proof json")
                .get("namespace_transition_event_id")
                .is_none(),
            "legacy same-namespace proof shape must omit the optional transition field"
        );

        let mut tampered = proof.clone();
        tampered.evidence_graph_hash = Some("different-graph".to_string());
        assert!(!verify_turn_proof_hash(&tampered));
    }
}
