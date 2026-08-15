use serde::{Deserialize, Serialize};
use thiserror::Error;
use zaion_ledger::{verify_event_signature, EventLedger};
use zaion_types::identity::PublicKeyBytes;

use crate::evidence_graph::EvidenceSubgraph;
use crate::turn_proof::{verify_turn_proof_hash, TurnProof};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofClosure {
    schema_version: u8,
    answer_trace_event_id: String,
    turn_proof_event_id: String,
    proof_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt_proof_join_event_id: Option<String>,
    evidence_graph_hash: String,
}

impl ProofClosure {
    pub fn schema_version(&self) -> u8 {
        self.schema_version
    }

    pub fn answer_trace_event_id(&self) -> &str {
        &self.answer_trace_event_id
    }

    pub fn turn_proof_event_id(&self) -> &str {
        &self.turn_proof_event_id
    }

    pub fn proof_hash(&self) -> &str {
        &self.proof_hash
    }

    pub fn receipt_proof_join_event_id(&self) -> Option<&str> {
        self.receipt_proof_join_event_id.as_deref()
    }

    pub fn evidence_graph_hash(&self) -> &str {
        &self.evidence_graph_hash
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            schema_version: 1,
            answer_trace_event_id: "evt-answer".to_string(),
            turn_proof_event_id: "evt-proof".to_string(),
            proof_hash: "proof-hash".to_string(),
            receipt_proof_join_event_id: None,
            evidence_graph_hash: "evidence-graph-hash".to_string(),
        }
    }
}

/// Compatibility name for callers that previously used the incomplete copy.
pub type ProofClosureRef = ProofClosure;

#[derive(Debug, Error)]
pub enum ProofClosureError {
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("missing closure event: {0}")]
    MissingEvent(String),
    #[error("closure event {event_id} has type {actual}, expected {expected}")]
    WrongEventType {
        event_id: String,
        expected: &'static str,
        actual: String,
    },
    #[error("closure event principals or namespaces do not match")]
    ScopeMismatch,
    #[error("closure event {event_id} has invalid parent; expected {expected}")]
    ParentMismatch { event_id: String, expected: String },
    #[error("closure event {event_id} signature verification failed: {reason}")]
    InvalidSignature { event_id: String, reason: String },
    #[error("turn proof payload is invalid: {0}")]
    InvalidTurnProof(String),
    #[error("turn proof hash verification failed")]
    InvalidTurnProofHash,
    #[error("answer trace and turn proof evidence graph hashes do not match")]
    EvidenceGraphMismatch,
    #[error("namespace transition proof is invalid: {0}")]
    InvalidNamespaceTransition(String),
    #[error("answer trace evidence graph is missing or invalid")]
    InvalidEvidenceGraph,
    #[error("answer evidence graph is missing required node {0}")]
    MissingEvidenceNode(String),
    #[error("tool receipt proof join is required for a proof with tool receipts")]
    MissingReceiptProofJoin,
    #[error("tool receipt proof join does not match the turn proof")]
    InvalidReceiptProofJoin,
    #[error("principal ledger chain is broken at sequence {0}")]
    BrokenLedgerChain(i64),
}

pub struct ProofClosureVerifier<'a> {
    ledger: &'a EventLedger,
    public_key: &'a PublicKeyBytes,
}

impl<'a> ProofClosureVerifier<'a> {
    pub fn new(ledger: &'a EventLedger, public_key: &'a PublicKeyBytes) -> Self {
        Self { ledger, public_key }
    }

    pub fn verify(
        &self,
        answer_trace_event_id: &str,
        turn_proof_event_id: &str,
        receipt_proof_join_event_id: Option<&str>,
    ) -> Result<ProofClosure, ProofClosureError> {
        let answer_trace = self.event(answer_trace_event_id, "answer.trace")?;
        let turn_proof_event = self.event(turn_proof_event_id, "turn.proof")?;
        self.verify_signature(&answer_trace)?;
        self.verify_signature(&turn_proof_event)?;

        if answer_trace.principal_id != turn_proof_event.principal_id
            || answer_trace.namespace_key != turn_proof_event.namespace_key
        {
            return Err(ProofClosureError::ScopeMismatch);
        }
        if turn_proof_event
            .parent_event_id
            .as_ref()
            .map(|id| id.0.as_str())
            != Some(answer_trace_event_id)
        {
            return Err(ProofClosureError::ParentMismatch {
                event_id: turn_proof_event_id.to_string(),
                expected: answer_trace_event_id.to_string(),
            });
        }

        let evidence_graph: EvidenceSubgraph = serde_json::from_value(
            answer_trace
                .payload
                .get("evidence_graph")
                .cloned()
                .ok_or(ProofClosureError::InvalidEvidenceGraph)?,
        )
        .map_err(|_| ProofClosureError::InvalidEvidenceGraph)?;
        let answer_graph_hash = answer_trace
            .payload
            .get("evidence_graph_hash")
            .and_then(serde_json::Value::as_str)
            .ok_or(ProofClosureError::InvalidEvidenceGraph)?;
        if !evidence_graph.verify_hash() || evidence_graph.graph_hash != answer_graph_hash {
            return Err(ProofClosureError::InvalidEvidenceGraph);
        }

        let turn_proof: TurnProof = serde_json::from_value(turn_proof_event.payload.clone())
            .map_err(|error| ProofClosureError::InvalidTurnProof(error.to_string()))?;
        if !verify_turn_proof_hash(&turn_proof) {
            return Err(ProofClosureError::InvalidTurnProofHash);
        }
        if turn_proof.principal_id.as_str() != turn_proof_event.principal_id.0.as_str()
            || turn_proof.namespace_key.as_str() != turn_proof_event.namespace_key.0.as_str()
        {
            return Err(ProofClosureError::ScopeMismatch);
        }
        if turn_proof_event
            .payload
            .get("answer_trace_event_id")
            .and_then(serde_json::Value::as_str)
            != Some(answer_trace_event_id)
        {
            return Err(ProofClosureError::InvalidTurnProof(
                "answer_trace_event_id does not match parent".to_string(),
            ));
        }
        if turn_proof.evidence_graph_hash.as_deref() != Some(answer_graph_hash) {
            return Err(ProofClosureError::EvidenceGraphMismatch);
        }
        if answer_trace
            .payload
            .get("namespace_transition_event_id")
            .and_then(serde_json::Value::as_str)
            != turn_proof.namespace_transition_event_id.as_deref()
        {
            return Err(ProofClosureError::InvalidNamespaceTransition(
                "answer trace and turn proof transition ids do not match".to_string(),
            ));
        }

        let received = self.event(&turn_proof.user_event_id, "channel.received")?;
        let sent = self.event(&turn_proof.output_event_id, "channel.sent")?;
        self.verify_signature(&received)?;
        self.verify_signature(&sent)?;
        for event in [&received, &sent] {
            if event.principal_id != turn_proof_event.principal_id {
                return Err(ProofClosureError::ScopeMismatch);
            }
        }
        if sent.namespace_key != turn_proof_event.namespace_key {
            return Err(ProofClosureError::ScopeMismatch);
        }
        if answer_trace
            .parent_event_id
            .as_ref()
            .map(|id| id.0.as_str())
            != Some(turn_proof.output_event_id.as_str())
            || answer_trace
                .payload
                .get("user_event_id")
                .and_then(serde_json::Value::as_str)
                != Some(turn_proof.user_event_id.as_str())
            || answer_trace
                .payload
                .get("output_event_id")
                .and_then(serde_json::Value::as_str)
                != Some(turn_proof.output_event_id.as_str())
        {
            return Err(ProofClosureError::InvalidTurnProof(
                "answer trace lineage does not match turn proof".to_string(),
            ));
        }

        if let Some(route_event_id) = &turn_proof.omni_route_event_id {
            let route = self.event(route_event_id, "omni.route")?;
            self.verify_signature(&route)?;
            if route.principal_id != turn_proof_event.principal_id
                || route.namespace_key != received.namespace_key
            {
                return Err(ProofClosureError::ScopeMismatch);
            }
            if sent.parent_event_id.as_ref().map(|id| id.0.as_str())
                != Some(route_event_id.as_str())
                || route.parent_event_id.as_ref().map(|id| id.0.as_str())
                    != Some(turn_proof.user_event_id.as_str())
            {
                return Err(ProofClosureError::InvalidTurnProof(
                    "received -> route -> sent lineage is not closed".to_string(),
                ));
            }
        } else if sent.parent_event_id.as_ref().map(|id| id.0.as_str())
            != Some(turn_proof.user_event_id.as_str())
        {
            return Err(ProofClosureError::InvalidTurnProof(
                "received -> sent lineage is not closed".to_string(),
            ));
        }

        match (
            received.namespace_key == turn_proof_event.namespace_key,
            turn_proof.namespace_transition_event_id.as_deref(),
        ) {
            (true, None) => {}
            (true, Some(_)) => {
                return Err(ProofClosureError::InvalidNamespaceTransition(
                    "same-namespace turn must not declare a transition event".to_string(),
                ));
            }
            (false, None) => {
                return Err(ProofClosureError::InvalidNamespaceTransition(
                    "cross-namespace turn is missing a transition event".to_string(),
                ));
            }
            (false, Some(transition_event_id)) => {
                let transition = self.event(transition_event_id, "channel.received")?;
                self.verify_signature(&transition)?;
                if transition.principal_id != turn_proof_event.principal_id
                    || transition.namespace_key != turn_proof_event.namespace_key
                {
                    return Err(ProofClosureError::ScopeMismatch);
                }
                if transition.parent_event_id.as_ref().map(|id| id.0.as_str())
                    != Some(turn_proof.user_event_id.as_str())
                {
                    return Err(ProofClosureError::ParentMismatch {
                        event_id: transition_event_id.to_string(),
                        expected: turn_proof.user_event_id.clone(),
                    });
                }
                let payload = &transition.payload;
                if payload
                    .get("source_parent_namespace_key")
                    .and_then(serde_json::Value::as_str)
                    != Some(received.namespace_key.0.as_str())
                    || payload
                        .get("source_parent_received_event_id")
                        .and_then(serde_json::Value::as_str)
                        != Some(turn_proof.user_event_id.as_str())
                    || payload.get("source").and_then(serde_json::Value::as_str)
                        != Some("compression.active_child_continuation")
                    || payload
                        .get("copy_policy")
                        .and_then(serde_json::Value::as_str)
                        != Some("active_child_turn_materialization")
                {
                    return Err(ProofClosureError::InvalidNamespaceTransition(
                        "signed transition payload does not bind the source ingress and policy"
                            .to_string(),
                    ));
                }
            }
        }

        let mut expected_lineage = vec![turn_proof.user_event_id.clone()];
        if let Some(route_event_id) = &turn_proof.omni_route_event_id {
            expected_lineage.push(route_event_id.clone());
        }
        if let Some(transition_event_id) = &turn_proof.namespace_transition_event_id {
            expected_lineage.push(transition_event_id.clone());
        }
        expected_lineage.push(turn_proof.output_event_id.clone());
        expected_lineage.extend(turn_proof.tool_receipt_ids.iter().cloned());
        if turn_proof.event_lineage != expected_lineage {
            return Err(ProofClosureError::InvalidTurnProof(
                "event_lineage does not match the proof event ids".to_string(),
            ));
        }

        let require_graph_node = |prefix: &str, id: &str| {
            let node_id = format!("{prefix}:{id}");
            evidence_graph
                .nodes
                .iter()
                .any(|node| node.id == node_id)
                .then_some(())
                .ok_or(ProofClosureError::MissingEvidenceNode(node_id))
        };
        require_graph_node("ledger-event", &turn_proof.user_event_id)?;
        require_graph_node("ledger-event", &turn_proof.output_event_id)?;
        if let Some(route_event_id) = &turn_proof.omni_route_event_id {
            require_graph_node("ledger-event", route_event_id)?;
        }
        if let Some(transition_event_id) = &turn_proof.namespace_transition_event_id {
            require_graph_node("ledger-event", transition_event_id)?;
        }
        if let Some(context_pack_id) = &turn_proof.context_pack_id {
            require_graph_node("context-pack", context_pack_id)?;
        }
        for memory_atom_id in &turn_proof.memory_atom_ids {
            require_graph_node("memory-atom", memory_atom_id)?;
        }
        for receipt_id in &turn_proof.tool_receipt_ids {
            require_graph_node("tool-receipt", receipt_id)?;
            let receipt = self.event(receipt_id, "tool.receipt")?;
            self.verify_signature(&receipt)?;
            if receipt.principal_id != turn_proof_event.principal_id
                || receipt.namespace_key != turn_proof_event.namespace_key
            {
                return Err(ProofClosureError::ScopeMismatch);
            }
        }

        let receipt_join_id = match receipt_proof_join_event_id {
            Some(event_id) => {
                let join = self.event(event_id, "tool.receipt.proof_join")?;
                self.verify_signature(&join)?;
                if join.principal_id != turn_proof_event.principal_id
                    || join.namespace_key != turn_proof_event.namespace_key
                {
                    return Err(ProofClosureError::ScopeMismatch);
                }
                if join.parent_event_id.as_ref().map(|id| id.0.as_str())
                    != Some(turn_proof_event_id)
                {
                    return Err(ProofClosureError::ParentMismatch {
                        event_id: event_id.to_string(),
                        expected: turn_proof_event_id.to_string(),
                    });
                }
                let joined_receipts = join
                    .payload
                    .get("tool_receipt_ids")
                    .and_then(serde_json::Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if join
                    .payload
                    .get("turn_proof_event_id")
                    .and_then(serde_json::Value::as_str)
                    != Some(turn_proof_event_id)
                    || join
                        .payload
                        .get("turn_proof_hash")
                        .and_then(serde_json::Value::as_str)
                        != Some(turn_proof.proof_hash.as_str())
                    || join
                        .payload
                        .get("answer_trace_event_id")
                        .and_then(serde_json::Value::as_str)
                        != Some(answer_trace_event_id)
                    || joined_receipts != turn_proof.tool_receipt_ids
                {
                    return Err(ProofClosureError::InvalidReceiptProofJoin);
                }
                Some(event_id.to_string())
            }
            None if turn_proof.tool_receipt_ids.is_empty() => None,
            None => return Err(ProofClosureError::MissingReceiptProofJoin),
        };

        let chain = self.ledger.verify_chain(&answer_trace.principal_id)?;
        if let Some(sequence) = chain.broken_at {
            return Err(ProofClosureError::BrokenLedgerChain(sequence));
        }

        Ok(ProofClosure {
            schema_version: 1,
            answer_trace_event_id: answer_trace_event_id.to_string(),
            turn_proof_event_id: turn_proof_event_id.to_string(),
            proof_hash: turn_proof.proof_hash,
            receipt_proof_join_event_id: receipt_join_id,
            evidence_graph_hash: answer_graph_hash.to_string(),
        })
    }

    fn event(
        &self,
        event_id: &str,
        expected_type: &'static str,
    ) -> Result<zaion_types::event::LedgerEvent, ProofClosureError> {
        let event = self
            .ledger
            .get_event(event_id)?
            .ok_or_else(|| ProofClosureError::MissingEvent(event_id.to_string()))?;
        if event.event_type != expected_type {
            return Err(ProofClosureError::WrongEventType {
                event_id: event_id.to_string(),
                expected: expected_type,
                actual: event.event_type,
            });
        }
        Ok(event)
    }

    fn verify_signature(
        &self,
        event: &zaion_types::event::LedgerEvent,
    ) -> Result<(), ProofClosureError> {
        verify_event_signature(self.public_key, event)
            .map(|_| ())
            .map_err(|error| ProofClosureError::InvalidSignature {
                event_id: event.event_id.0.clone(),
                reason: error.to_string(),
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DegradationReport {
    pub reason_code: String,
    pub safe_response: bool,
    pub lost_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnError {
    pub reason_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartialLedgerTail {
    pub appended_event_ids: Vec<String>,
    pub last_safe_parent_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuarantineEvent {
    pub level: u8,
    pub reason_code: String,
    pub diagnostic_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed(ProofClosure),
    Degraded(ProofClosure, DegradationReport),
    Aborted(TurnError, PartialLedgerTail),
    Quarantined(QuarantineEvent),
}

impl TurnOutcome {
    pub fn ledger_event_type(&self) -> &'static str {
        match self {
            Self::Completed(_) => "turn.proof",
            Self::Degraded(_, _) => "turn.degraded",
            Self::Aborted(_, _) => "turn.aborted",
            Self::Quarantined(_) => "system.quarantine",
        }
    }

    pub fn is_safe_to_reply(&self) -> bool {
        match self {
            Self::Completed(_) => true,
            Self::Degraded(_, report) => report.safe_response,
            Self::Aborted(_, _) | Self::Quarantined(_) => false,
        }
    }

    pub fn allows_tool_execution(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Degraded(_, _))
    }

    pub fn allows_memory_write(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence_graph::{build_answer_evidence_subgraph, AnswerEvidenceInput};
    use crate::turn_proof::{build_turn_proof, TurnCapabilityManifest, TurnProofInput};
    use zaion_crypto::ZaionKeypair;
    use zaion_types::event::EventType;
    use zaion_types::session::NamespaceKey;

    #[test]
    fn degraded_outcome_requires_proof_closure_and_report() {
        let outcome = TurnOutcome::Degraded(
            ProofClosure::for_test(),
            DegradationReport {
                reason_code: "provider_retry_exhausted".to_string(),
                safe_response: true,
                lost_capabilities: vec!["web_search".to_string()],
            },
        );

        assert_eq!(outcome.ledger_event_type(), "turn.degraded");
        assert!(outcome.is_safe_to_reply());
    }

    #[test]
    fn quarantined_outcome_blocks_tool_and_memory_writes() {
        let outcome = TurnOutcome::Quarantined(QuarantineEvent {
            level: 3,
            reason_code: "proof_chain_broken".to_string(),
            diagnostic_scope: "safe_only".to_string(),
        });

        assert_eq!(outcome.ledger_event_type(), "system.quarantine");
        assert!(!outcome.allows_tool_execution());
        assert!(!outcome.allows_memory_write());
    }

    #[test]
    fn verifier_constructs_closure_only_from_signed_matching_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ledger = EventLedger::new(dir.path().join("closure.db"));
        let keypair = ZaionKeypair::generate();
        let namespace = NamespaceKey(keypair.principal_id().as_str().to_string());

        let received = ledger
            .append_signed_typed_event(
                &keypair,
                &namespace,
                EventType::ChannelReceived,
                serde_json::json!({"message": "hello"}),
                None,
            )
            .expect("received");
        let sent = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::ChannelSent,
                serde_json::json!({"message": "world"}),
                None,
                Some(&received),
            )
            .expect("sent");
        let receipt = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::ToolReceipt,
                serde_json::json!({"tool_name": "fs_read", "status": "ok"}),
                None,
                Some(&sent),
            )
            .expect("tool receipt");
        let graph = build_answer_evidence_subgraph(AnswerEvidenceInput {
            response_hash: "response-hash".to_string(),
            tool_receipt_ids: vec![receipt.0.clone()],
            source_ledger_event_ids: vec![received.0.clone()],
            output_ledger_event_id: sent.0.clone(),
            ..Default::default()
        });
        let answer = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::AnswerTrace,
                serde_json::json!({
                    "response_hash": "response-hash",
                    "user_event_id": received.0,
                    "output_event_id": sent.0,
                    "evidence_graph_hash": graph.graph_hash,
                    "evidence_graph": graph,
                }),
                None,
                Some(&sent),
            )
            .expect("answer trace");
        let proof = build_turn_proof(TurnProofInput {
            principal_id: keypair.principal_id().as_str().to_string(),
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            channel_id: "terminal".to_string(),
            thread_id: "main".to_string(),
            namespace_key: namespace.0.clone(),
            user_event_id: received.0.clone(),
            output_event_id: sent.0.clone(),
            omni_route_event_id: None,
            omni_route_authority_hash: None,
            namespace_transition_event_id: None,
            identity_contract: "identity".to_string(),
            capability_manifest: TurnCapabilityManifest {
                provider: "test".to_string(),
                model: "test".to_string(),
                max_tokens: None,
                temperature: None,
                memory_enabled: false,
                mcp_enabled: false,
                cache_enabled: false,
                smart_route_enabled: false,
                compression_requested: false,
                tools_requested: Vec::new(),
                boundaries: Vec::new(),
            },
            context_pack_id: None,
            context_layers: Vec::new(),
            memory_atom_ids: Vec::new(),
            compression_evidence: None,
            cost_evidence: None,
            runtime_memory_evidence: None,
            evidence_graph_hash: Some(graph.graph_hash.clone()),
            tokens_in: 1,
            tokens_out: 1,
            tool_call_count: 1,
            tool_receipt_ids: vec![receipt.0.clone()],
        });
        let proof_hash = proof.proof_hash.clone();
        let mut proof_payload = serde_json::to_value(proof).expect("proof payload");
        proof_payload["answer_trace_event_id"] = serde_json::json!(answer.0);
        let proof_event = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::TurnProof,
                proof_payload,
                None,
                Some(&answer),
            )
            .expect("turn proof");

        let public_key = keypair.public_key_bytes();
        let missing_join = ProofClosureVerifier::new(&ledger, &public_key)
            .verify(&answer.0, &proof_event.0, None)
            .expect_err("receipt-bearing proof must require join");
        assert!(matches!(
            missing_join,
            ProofClosureError::MissingReceiptProofJoin
        ));
        let join = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &namespace,
                EventType::ToolReceiptProofJoin,
                serde_json::json!({
                    "tool_receipt_ids": [receipt.0],
                    "turn_proof_event_id": proof_event.0,
                    "turn_proof_hash": proof_hash,
                    "answer_trace_event_id": answer.0,
                }),
                None,
                Some(&proof_event),
            )
            .expect("receipt proof join");
        let closure = ProofClosureVerifier::new(&ledger, &public_key)
            .verify(&answer.0, &proof_event.0, Some(&join.0))
            .expect("verified closure");

        assert_eq!(closure.answer_trace_event_id(), answer.0);
        assert_eq!(closure.turn_proof_event_id(), proof_event.0);
        assert_eq!(closure.proof_hash(), proof_hash);
        assert_eq!(closure.evidence_graph_hash(), graph.graph_hash);
        assert_eq!(closure.receipt_proof_join_event_id(), Some(join.0.as_str()));

        let wrong_key = ZaionKeypair::generate().public_key_bytes();
        let invalid_signature = ProofClosureVerifier::new(&ledger, &wrong_key)
            .verify(&answer.0, &proof_event.0, Some(&join.0))
            .expect_err("wrong public key must fail closed");
        assert!(matches!(
            invalid_signature,
            ProofClosureError::InvalidSignature { .. }
        ));
    }

    #[test]
    fn verifier_requires_signed_namespace_transition_for_compression_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ledger = EventLedger::new(dir.path().join("namespace-transition.db"));
        let keypair = ZaionKeypair::generate();
        let parent_namespace = NamespaceKey("parent-session".to_string());
        let child_namespace = NamespaceKey("child-session".to_string());

        let received = ledger
            .append_signed_typed_event(
                &keypair,
                &parent_namespace,
                EventType::ChannelReceived,
                serde_json::json!({"message": "compress"}),
                None,
            )
            .expect("received");
        let route = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &parent_namespace,
                EventType::OmniRoute,
                serde_json::json!({"route": "terminal"}),
                None,
                Some(&received),
            )
            .expect("route");
        let transition = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &child_namespace,
                EventType::ChannelReceived,
                serde_json::json!({
                    "source": "compression.active_child_continuation",
                    "source_parent_namespace_key": parent_namespace.0,
                    "source_parent_received_event_id": received.0,
                    "copy_policy": "active_child_turn_materialization",
                }),
                None,
                Some(&received),
            )
            .expect("namespace transition");
        let sent = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &child_namespace,
                EventType::ChannelSent,
                serde_json::json!({"message": "compressed response"}),
                None,
                Some(&route),
            )
            .expect("sent");

        let append_closure_events = |transition_event_id: &str| {
            let graph = build_answer_evidence_subgraph(AnswerEvidenceInput {
                response_hash: "response-hash".to_string(),
                source_ledger_event_ids: vec![
                    received.0.clone(),
                    route.0.clone(),
                    transition_event_id.to_string(),
                ],
                output_ledger_event_id: sent.0.clone(),
                ..Default::default()
            });
            let graph_hash = graph.graph_hash.clone();
            let answer = ledger
                .append_signed_typed_event_with_parent(
                    &keypair,
                    &child_namespace,
                    EventType::AnswerTrace,
                    serde_json::json!({
                        "response_hash": "response-hash",
                        "user_event_id": received.0,
                        "output_event_id": sent.0,
                        "namespace_transition_event_id": transition_event_id,
                        "evidence_graph_hash": graph_hash,
                        "evidence_graph": graph,
                    }),
                    None,
                    Some(&sent),
                )
                .expect("answer trace");
            let proof = build_turn_proof(TurnProofInput {
                principal_id: keypair.principal_id().as_str().to_string(),
                workspace_id: "workspace".to_string(),
                project_id: "project".to_string(),
                channel_id: "terminal".to_string(),
                thread_id: "main".to_string(),
                namespace_key: child_namespace.0.clone(),
                user_event_id: received.0.clone(),
                output_event_id: sent.0.clone(),
                omni_route_event_id: Some(route.0.clone()),
                omni_route_authority_hash: Some("route-authority".to_string()),
                namespace_transition_event_id: Some(transition_event_id.to_string()),
                identity_contract: "identity".to_string(),
                capability_manifest: TurnCapabilityManifest {
                    provider: "test".to_string(),
                    model: "test".to_string(),
                    max_tokens: None,
                    temperature: None,
                    memory_enabled: false,
                    mcp_enabled: false,
                    cache_enabled: false,
                    smart_route_enabled: false,
                    compression_requested: true,
                    tools_requested: Vec::new(),
                    boundaries: Vec::new(),
                },
                context_pack_id: None,
                context_layers: Vec::new(),
                memory_atom_ids: Vec::new(),
                compression_evidence: None,
                cost_evidence: None,
                runtime_memory_evidence: None,
                evidence_graph_hash: Some(graph_hash),
                tokens_in: 1,
                tokens_out: 1,
                tool_call_count: 0,
                tool_receipt_ids: Vec::new(),
            });
            let mut proof_payload = serde_json::to_value(proof).expect("proof payload");
            proof_payload["answer_trace_event_id"] = serde_json::json!(answer.0);
            let proof_event = ledger
                .append_signed_typed_event_with_parent(
                    &keypair,
                    &child_namespace,
                    EventType::TurnProof,
                    proof_payload,
                    None,
                    Some(&answer),
                )
                .expect("turn proof");
            (answer, proof_event)
        };

        let (answer, proof) = append_closure_events(&transition.0);
        let public_key = keypair.public_key_bytes();
        ProofClosureVerifier::new(&ledger, &public_key)
            .verify(&answer.0, &proof.0, None)
            .expect("signed compression namespace transition should close");

        let invalid_transition = ledger
            .append_signed_typed_event_with_parent(
                &keypair,
                &child_namespace,
                EventType::ChannelReceived,
                serde_json::json!({
                    "source": "compression.active_child_continuation",
                    "source_parent_namespace_key": "unrelated-session",
                    "source_parent_received_event_id": received.0,
                    "copy_policy": "active_child_turn_materialization",
                }),
                None,
                Some(&received),
            )
            .expect("invalid namespace transition fixture");
        let (invalid_answer, invalid_proof) = append_closure_events(&invalid_transition.0);
        let error = ProofClosureVerifier::new(&ledger, &public_key)
            .verify(&invalid_answer.0, &invalid_proof.0, None)
            .expect_err("mismatched parent namespace must fail closed");
        assert!(matches!(
            error,
            ProofClosureError::InvalidNamespaceTransition(_)
        ));
    }
}
