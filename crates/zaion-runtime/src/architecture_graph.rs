use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArchitectureNodeStatus {
    Passing,
    Experimental,
    NotPromoted,
    InvalidChain,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureNode {
    pub id: String,
    pub owner: String,
    pub status: ArchitectureNodeStatus,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureGraph {
    pub nodes: Vec<ArchitectureNode>,
}

impl ArchitectureGraph {
    pub fn stable_default() -> Self {
        Self {
            nodes: vec![
                node("TurnKernelEntry:wake", "zaion-runtime", "turn_kernel"),
                node(
                    "OperationStreamGraph:runtime",
                    "zaion-runtime",
                    "operation_stream",
                ),
                node("PanelSink:tui", "zaion-cli", "tui stream consumer"),
                node("PanelSink:telegram", "zaion-cli", "telegram_panel"),
                node(
                    "TelegramCommandGraph:stable",
                    "zaion-cli",
                    "telegram_commands",
                ),
                node(
                    "StorageBoundary:event-knowledge-session",
                    "zaion-runtime",
                    "storage_boundary",
                ),
                node(
                    "ContextStrategy:minimal",
                    "zaion-runtime",
                    "context_strategy",
                ),
                node("ContextStrategy:full", "zaion-runtime", "context_strategy"),
                node_with_status(
                    "TurnOutcome:stable",
                    "zaion-runtime",
                    ArchitectureNodeStatus::NotPromoted,
                    "completed/aborted paths are typed; degraded/quarantined signed production paths remain open",
                ),
                node(
                    "FederationMessage:remote-ingress",
                    "zaion-a2a",
                    "federation_message",
                ),
                node("SyncProtocol:append-only", "zaion-sync", "protocol"),
                node("LifecycleGraph:stable", "zaion-runtime", "lifecycle_graph"),
                node(
                    "CircuitBreakerGraph:stable",
                    "zaion-runtime",
                    "circuit_breaker",
                ),
                node("NeverManifest:stable", "zaion-safety", "never_manifest"),
                node(
                    "CompileTimeGate:must_produce",
                    "zaion-contract-macros",
                    "must_produce",
                ),
            ],
        }
    }

    pub fn has_node(&self, id: &str) -> bool {
        self.nodes.iter().any(|node| node.id == id)
    }
}

fn node(id: &'static str, owner: &'static str, evidence: &'static str) -> ArchitectureNode {
    node_with_status(id, owner, ArchitectureNodeStatus::Passing, evidence)
}

fn node_with_status(
    id: &'static str,
    owner: &'static str,
    status: ArchitectureNodeStatus,
    evidence: &'static str,
) -> ArchitectureNode {
    ArchitectureNode {
        id: id.to_string(),
        owner: owner.to_string(),
        status,
        evidence: evidence.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_graph_contains_user_trust_and_runtime_nodes() {
        let graph = ArchitectureGraph::stable_default();
        for required in [
            "TurnKernelEntry:wake",
            "OperationStreamGraph:runtime",
            "PanelSink:tui",
            "PanelSink:telegram",
            "TelegramCommandGraph:stable",
            "StorageBoundary:event-knowledge-session",
            "ContextStrategy:minimal",
            "ContextStrategy:full",
            "TurnOutcome:stable",
            "FederationMessage:remote-ingress",
            "SyncProtocol:append-only",
            "LifecycleGraph:stable",
            "CircuitBreakerGraph:stable",
            "NeverManifest:stable",
            "CompileTimeGate:must_produce",
        ] {
            assert!(graph.has_node(required), "missing {required}");
        }
        let turn_outcome = graph
            .nodes
            .iter()
            .find(|node| node.id == "TurnOutcome:stable")
            .expect("turn outcome node");
        assert_eq!(turn_outcome.status, ArchitectureNodeStatus::NotPromoted);
    }
}
