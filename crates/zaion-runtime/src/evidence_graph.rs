//! Deterministic answer-local evidence graphs used to close turn proofs.

use serde::{Deserialize, Serialize};

use crate::turn_proof::stable_hash_json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceNodeKind {
    ProviderTrace,
    ContextPack,
    MemoryAtom,
    ToolReceipt,
    LedgerEvent,
    AnswerTraceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EvidenceNode {
    pub id: String,
    pub kind: EvidenceNodeKind,
    pub evidence_hash: String,
}

impl EvidenceNode {
    pub fn with_hash(
        kind: EvidenceNodeKind,
        id: impl Into<String>,
        evidence_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            evidence_hash: evidence_hash.into(),
        }
    }

    pub fn reference(kind: EvidenceNodeKind, id: impl Into<String>) -> Self {
        let id = id.into();
        let evidence_hash = stable_hash_json(&serde_json::json!({
            "kind": &kind,
            "id": &id,
        }));
        Self::with_hash(kind, id, evidence_hash)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceEdgeKind {
    UsedBy,
    DerivedFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EvidenceEdge {
    pub from: String,
    pub to: String,
    pub kind: EvidenceEdgeKind,
}

impl EvidenceEdge {
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: EvidenceEdgeKind) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceSubgraph {
    pub schema: String,
    pub answer_id: String,
    pub nodes: Vec<EvidenceNode>,
    pub edges: Vec<EvidenceEdge>,
    pub graph_hash: String,
}

impl EvidenceSubgraph {
    pub fn new(
        answer_id: impl Into<String>,
        mut nodes: Vec<EvidenceNode>,
        mut edges: Vec<EvidenceEdge>,
    ) -> Self {
        nodes.sort();
        nodes.dedup();
        edges.sort();
        edges.dedup();
        let mut graph = Self {
            schema: "zaion.evidence_subgraph.v1".to_string(),
            answer_id: answer_id.into(),
            nodes,
            edges,
            graph_hash: String::new(),
        };
        graph.graph_hash = graph.expected_hash();
        graph
    }

    pub fn verify_hash(&self) -> bool {
        !self.graph_hash.is_empty() && self.graph_hash == self.expected_hash()
    }

    fn expected_hash(&self) -> String {
        stable_hash_json(&serde_json::json!({
            "schema": self.schema,
            "answer_id": self.answer_id,
            "nodes": self.nodes,
            "edges": self.edges,
        }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnswerEvidenceInput {
    pub response_hash: String,
    pub context_pack_id: Option<String>,
    pub memory_atom_ids: Vec<String>,
    pub tool_receipt_ids: Vec<String>,
    pub source_ledger_event_ids: Vec<String>,
    pub output_ledger_event_id: String,
    pub answer_trace_span_hashes: Vec<String>,
}

pub fn build_answer_evidence_subgraph(input: AnswerEvidenceInput) -> EvidenceSubgraph {
    let response_node_id = format!("provider-response:{}", input.response_hash);
    let mut nodes = vec![EvidenceNode::with_hash(
        EvidenceNodeKind::ProviderTrace,
        response_node_id.clone(),
        input.response_hash,
    )];
    let mut edges = Vec::new();

    {
        let mut add_input_reference = |kind: EvidenceNodeKind, prefix: &str, id: String| {
            let node_id = format!("{prefix}:{id}");
            nodes.push(EvidenceNode::reference(kind, node_id.clone()));
            edges.push(EvidenceEdge::new(
                node_id,
                response_node_id.clone(),
                EvidenceEdgeKind::UsedBy,
            ));
        };

        if let Some(context_pack_id) = input.context_pack_id {
            add_input_reference(
                EvidenceNodeKind::ContextPack,
                "context-pack",
                context_pack_id,
            );
        }
        for memory_atom_id in input.memory_atom_ids {
            add_input_reference(EvidenceNodeKind::MemoryAtom, "memory-atom", memory_atom_id);
        }
        for tool_receipt_id in input.tool_receipt_ids {
            add_input_reference(
                EvidenceNodeKind::ToolReceipt,
                "tool-receipt",
                tool_receipt_id,
            );
        }
        for ledger_event_id in input.source_ledger_event_ids {
            add_input_reference(
                EvidenceNodeKind::LedgerEvent,
                "ledger-event",
                ledger_event_id,
            );
        }
    }

    if !input.output_ledger_event_id.is_empty() {
        let output_node_id = format!("ledger-event:{}", input.output_ledger_event_id);
        nodes.push(EvidenceNode::reference(
            EvidenceNodeKind::LedgerEvent,
            output_node_id.clone(),
        ));
        edges.push(EvidenceEdge::new(
            output_node_id,
            response_node_id.clone(),
            EvidenceEdgeKind::DerivedFrom,
        ));
    }

    for span_hash in input.answer_trace_span_hashes {
        let span_node_id = format!("answer-span:{span_hash}");
        nodes.push(EvidenceNode::with_hash(
            EvidenceNodeKind::AnswerTraceSpan,
            span_node_id.clone(),
            span_hash,
        ));
        edges.push(EvidenceEdge::new(
            span_node_id,
            response_node_id.clone(),
            EvidenceEdgeKind::DerivedFrom,
        ));
    }

    EvidenceSubgraph::new(response_node_id, nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> AnswerEvidenceInput {
        AnswerEvidenceInput {
            response_hash: "response-hash".to_string(),
            context_pack_id: Some("ctx-1".to_string()),
            memory_atom_ids: vec!["mem-2".to_string(), "mem-1".to_string()],
            tool_receipt_ids: vec!["receipt-1".to_string()],
            source_ledger_event_ids: vec!["route-1".to_string(), "received-1".to_string()],
            output_ledger_event_id: "sent-1".to_string(),
            answer_trace_span_hashes: vec!["span-1".to_string()],
        }
    }

    #[test]
    fn answer_evidence_graph_is_deterministic_and_hash_verified() {
        let first = build_answer_evidence_subgraph(sample_input());
        let mut reordered = sample_input();
        reordered.memory_atom_ids.reverse();
        reordered.source_ledger_event_ids.reverse();
        let second = build_answer_evidence_subgraph(reordered);

        assert!(first.verify_hash());
        assert_eq!(first, second);
        assert!(first
            .nodes
            .iter()
            .any(|node| node.kind == EvidenceNodeKind::ToolReceipt));
        assert!(first
            .nodes
            .iter()
            .any(|node| node.kind == EvidenceNodeKind::AnswerTraceSpan));
    }

    #[test]
    fn evidence_graph_hash_detects_tampering() {
        let mut graph = build_answer_evidence_subgraph(sample_input());
        graph.nodes[0].evidence_hash.push_str("-tampered");
        assert!(!graph.verify_hash());
    }
}
