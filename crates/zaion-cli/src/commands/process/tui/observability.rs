use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObservabilityTruth {
    Observed,
    Estimated,
    #[default]
    Unavailable,
    Simulated,
}

impl ObservabilityTruth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Estimated => "estimated",
            Self::Unavailable => "unavailable",
            Self::Simulated => "simulated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Layer,
    Head,
    Mlp,
    Feature,
    Memory,
    Tool,
    Agent,
    Retrieval,
    Token,
    Controller,
    Planner,
    Executor,
    Critic,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
    pub label: String,
    pub activation: f32,
    pub confidence: f32,
    pub risk: f32,
    pub health: f32,
    pub participates_current_output: bool,
    pub last_updated: u64,
    pub truth: ObservabilityTruth,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub flow: f32,
    pub attribution: f32,
    pub risk: f32,
    pub last_updated: u64,
    pub truth: ObservabilityTruth,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct EvidencePacket {
    pub statement: String,
    pub prompt_spans: Vec<String>,
    pub memory_refs: Vec<String>,
    pub retrieval_refs: Vec<String>,
    pub tool_refs: Vec<String>,
    pub neural_refs: Vec<String>,
    pub attribution_scores: Vec<f32>,
    pub confidence: f32,
    pub unsupported: bool,
    pub contradictions: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenTrace {
    pub token: String,
    pub token_id: Option<u32>,
    pub position: usize,
    pub top_k_logits: Vec<(String, f32)>,
    pub probability: Option<f32>,
    pub entropy: Option<f32>,
    pub attention_summary: Option<String>,
    pub attribution_summary: Option<String>,
    pub evidence_refs: Vec<String>,
    pub risk_flags: Vec<String>,
    pub truth: ObservabilityTruth,
}

#[derive(Debug, Clone)]
pub enum ObservabilityEventKind {
    SessionStarted,
    SessionEnded,
    UserInputReceived,
    PromptBuilt,
    ContextCompacted,
    ModelGenerationStarted,
    ModelTokenGenerated(TokenTrace),
    ModelGenerationDone,
    Error,
    AgentPlanCreated,
    AgentStepStarted,
    AgentStepDone,
    AgentDecisionMade,
    AgentConfidenceUpdated(f32),
    AgentRiskDetected(String),
    AgentSelfCheckStarted,
    AgentSelfCheckDone,
    MemoryRead {
        memory_id: String,
        score: f32,
        content_preview: String,
    },
    MemoryWrite {
        memory_id: String,
        content_preview: String,
    },
    MemoryUpdated,
    MemoryDecayed,
    MemoryConflictDetected(String),
    MemoryInfluenceScored {
        memory_id: String,
        score: f32,
    },
    RetrievalQuery(String),
    RetrievalChunkSelected(String),
    RetrievalChunkRejected(String),
    RetrievalReranked,
    RetrievalInfluenceScored {
        chunk_id: String,
        score: f32,
    },
    ToolCallProposed(String),
    ToolCallStarted(String),
    ToolCallDelta(String),
    ToolCallDone(String),
    ToolCallFailed(String),
    ToolOutputUsed(String),
    ToolOutputIgnored(String),
    NeuralLayerEntered(String),
    NeuralLayerExited(String),
    NeuralNodeActivated {
        node_id: String,
        node_type: NodeType,
        activation: f32,
        confidence: f32,
        risk: f32,
        participates_current_output: bool,
        truth: ObservabilityTruth,
    },
    NeuralNodeDecayed(String),
    NeuralEdgeUpdated {
        source: String,
        target: String,
        weight: f32,
        flow: f32,
        attribution: f32,
        risk: f32,
        truth: ObservabilityTruth,
    },
    NeuralAttentionUpdated,
    NeuralFeatureActivated(String),
    NeuralLogitsUpdated(TokenTrace),
    NeuralEntropyUpdated(f32),
    NeuralKvCacheUpdated(String),
    NeuralWeightDeltaDetected(String),
    NeuralTopologyChanged,
    AttributionComputed(EvidencePacket),
    CircuitDetected(String),
    CausalPathEstimated(String),
    ActivationPatchStarted,
    ActivationPatchDone,
    CounterfactualRunStarted,
    CounterfactualRunDone,
    ExplanationReportGenerated(String),
    HallucinationRiskDetected(String),
    UnsupportedClaimDetected(String),
    PromptInjectionDetected(String),
    ToolMisuseDetected(String),
    MemoryPoisoningDetected(String),
    ReasoningFaithfulnessWarning(String),
    ConfidenceMismatchDetected(String),
}

#[derive(Debug, Clone)]
pub struct ObservabilityEvent {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub kind: ObservabilityEventKind,
    pub truth: ObservabilityTruth,
    pub summary: String,
}

pub struct ObservabilityRingBuffer {
    events: VecDeque<ObservabilityEvent>,
    capacity: usize,
    dropped: u64,
}

impl ObservabilityRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
            dropped: 0,
        }
    }

    pub fn push(&mut self, event: ObservabilityEvent) {
        while self.events.len() >= self.capacity {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(event);
    }

    pub fn events(&self) -> impl Iterator<Item = &ObservabilityEvent> {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Live,
    Paused,
    Replay,
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    OneSecond,
    FiveSeconds,
    ThirtySeconds,
    CurrentAnswer,
    CurrentSession,
}

#[derive(Debug, Clone)]
pub struct TuiObservabilityState {
    pub nodes: BTreeMap<String, Node>,
    pub edges: BTreeMap<(String, String), Edge>,
    pub events: Vec<String>,
    pub tokens: Vec<TokenTrace>,
    pub evidence_packets: Vec<EvidencePacket>,
    pub risks: Vec<String>,
    pub selected_id: Option<String>,
    pub playback_mode: PlaybackMode,
    pub time_window: TimeWindow,
    pub event_rate: f32,
    pub context_length: usize,
    pub sample_rate: f32,
    pub intervention_sandbox: bool,
    pub interpretability_mode: ObservabilityTruth,
    pub dropped_events: u64,
}

impl Default for TuiObservabilityState {
    fn default() -> Self {
        let mut state = Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            events: Vec::new(),
            tokens: Vec::new(),
            evidence_packets: Vec::new(),
            risks: Vec::new(),
            selected_id: Some("controller".to_string()),
            playback_mode: PlaybackMode::Live,
            time_window: TimeWindow::CurrentAnswer,
            event_rate: 0.0,
            context_length: 0,
            sample_rate: 1.0,
            intervention_sandbox: true,
            interpretability_mode: ObservabilityTruth::Estimated,
            dropped_events: 0,
        };
        state.seed_default_nodes();
        state
    }
}

impl TuiObservabilityState {
    pub fn apply(&mut self, event: &ObservabilityEvent) {
        self.events.push(format!(
            "#{:04} [{}] {}",
            event.seq,
            event.truth.label(),
            event.summary
        ));
        if self.events.len() > 200 {
            self.events.remove(0);
        }

        match &event.kind {
            ObservabilityEventKind::ModelTokenGenerated(trace)
            | ObservabilityEventKind::NeuralLogitsUpdated(trace) => {
                self.tokens.push(trace.clone());
                self.context_length = self.context_length.max(trace.position + 1);
            }
            ObservabilityEventKind::NeuralNodeActivated {
                node_id,
                node_type,
                activation,
                confidence,
                risk,
                participates_current_output,
                truth,
            } => {
                let node = self.nodes.entry(node_id.clone()).or_insert_with(|| Node {
                    id: node_id.clone(),
                    node_type: *node_type,
                    label: node_id.clone(),
                    activation: 0.0,
                    confidence: 0.0,
                    risk: 0.0,
                    health: 1.0,
                    participates_current_output: false,
                    last_updated: event.timestamp_ms,
                    truth: *truth,
                    metadata: BTreeMap::new(),
                });
                node.activation = *activation;
                node.confidence = *confidence;
                node.risk = *risk;
                node.health = (1.0 - risk).clamp(0.0, 1.0);
                node.participates_current_output = *participates_current_output;
                node.last_updated = event.timestamp_ms;
                node.truth = *truth;
            }
            ObservabilityEventKind::NeuralEdgeUpdated {
                source,
                target,
                weight,
                flow,
                attribution,
                risk,
                truth,
            } => {
                self.edges.insert(
                    (source.clone(), target.clone()),
                    Edge {
                        source: source.clone(),
                        target: target.clone(),
                        weight: *weight,
                        flow: *flow,
                        attribution: *attribution,
                        risk: *risk,
                        last_updated: event.timestamp_ms,
                        truth: *truth,
                        metadata: BTreeMap::new(),
                    },
                );
            }
            ObservabilityEventKind::AttributionComputed(packet) => {
                self.evidence_packets.push(packet.clone());
            }
            ObservabilityEventKind::HallucinationRiskDetected(reason)
            | ObservabilityEventKind::UnsupportedClaimDetected(reason)
            | ObservabilityEventKind::PromptInjectionDetected(reason)
            | ObservabilityEventKind::ToolMisuseDetected(reason)
            | ObservabilityEventKind::MemoryPoisoningDetected(reason)
            | ObservabilityEventKind::ReasoningFaithfulnessWarning(reason)
            | ObservabilityEventKind::ConfidenceMismatchDetected(reason)
            | ObservabilityEventKind::AgentRiskDetected(reason)
            | ObservabilityEventKind::MemoryConflictDetected(reason) => {
                self.risks.push(reason.clone());
            }
            _ => {}
        }
    }

    pub fn selected_node(&self) -> Option<&Node> {
        let id = self.selected_id.as_deref()?;
        self.nodes.get(id)
    }

    pub fn audit_summary(&self) -> AuditSummary {
        let unsupported = self
            .evidence_packets
            .iter()
            .filter(|packet| packet.unsupported)
            .count();
        AuditSummary {
            context_used: self.context_length,
            memory_used: self
                .nodes
                .values()
                .filter(|node| {
                    node.node_type == NodeType::Memory && node.participates_current_output
                })
                .count(),
            tools_used: self
                .nodes
                .values()
                .filter(|node| node.node_type == NodeType::Tool && node.participates_current_output)
                .count(),
            supported_claims: self.evidence_packets.len().saturating_sub(unsupported),
            unsupported_claims: unsupported,
            risk_count: self.risks.len(),
            confidence: average_confidence(self.nodes.values()),
        }
    }

    fn seed_default_nodes(&mut self) {
        for (id, node_type, label, truth) in [
            (
                "controller",
                NodeType::Controller,
                "controller",
                ObservabilityTruth::Observed,
            ),
            (
                "planner",
                NodeType::Planner,
                "planner",
                ObservabilityTruth::Observed,
            ),
            (
                "executor",
                NodeType::Executor,
                "executor",
                ObservabilityTruth::Observed,
            ),
            (
                "critic",
                NodeType::Critic,
                "critic",
                ObservabilityTruth::Observed,
            ),
            (
                "memory",
                NodeType::Memory,
                "memory modules",
                ObservabilityTruth::Observed,
            ),
            (
                "retrieval",
                NodeType::Retrieval,
                "retrieval nodes",
                ObservabilityTruth::Observed,
            ),
            (
                "tools",
                NodeType::Tool,
                "tool nodes",
                ObservabilityTruth::Observed,
            ),
            (
                "layer",
                NodeType::Layer,
                "model layers",
                ObservabilityTruth::Unavailable,
            ),
            (
                "heads",
                NodeType::Head,
                "attention heads",
                ObservabilityTruth::Unavailable,
            ),
            (
                "mlp",
                NodeType::Mlp,
                "MLP blocks",
                ObservabilityTruth::Unavailable,
            ),
            (
                "features",
                NodeType::Feature,
                "SAE features",
                ObservabilityTruth::Unavailable,
            ),
        ] {
            self.nodes.insert(
                id.to_string(),
                Node {
                    id: id.to_string(),
                    node_type,
                    label: label.to_string(),
                    activation: if truth == ObservabilityTruth::Unavailable {
                        0.0
                    } else {
                        0.35
                    },
                    confidence: if truth == ObservabilityTruth::Unavailable {
                        0.0
                    } else {
                        0.7
                    },
                    risk: 0.0,
                    health: 1.0,
                    participates_current_output: matches!(
                        id,
                        "controller" | "planner" | "executor" | "critic"
                    ),
                    last_updated: 0,
                    truth,
                    metadata: BTreeMap::new(),
                },
            );
        }
    }
}

fn average_confidence<'a>(nodes: impl Iterator<Item = &'a Node>) -> f32 {
    let mut sum = 0.0;
    let mut count = 0usize;
    for node in nodes {
        if node.confidence > 0.0 {
            sum += node.confidence;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

#[derive(Debug, Clone)]
pub struct AuditSummary {
    pub context_used: usize,
    pub memory_used: usize,
    pub tools_used: usize,
    pub supported_claims: usize,
    pub unsupported_claims: usize,
    pub risk_count: usize,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditCommand {
    Why(String),
    TraceToken(usize),
    TraceClaim(String),
    Topology,
    Freeze,
    Resume,
    Replay,
    DiffState,
    Counterfactual(String),
    AblateNode(String),
    InspectNode(String),
    InspectEdge(String),
    Evidence,
    Risk,
    Status,
    Model,
    Sessions,
    Usage,
    Agents,
    ExportTrace(String),
    Help,
    Unknown(String),
}

pub fn parse_audit_command(input: &str) -> Option<AuditCommand> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim().to_string();
    Some(match command {
        "/why" => AuditCommand::Why(rest),
        "/trace-token" => AuditCommand::TraceToken(rest.parse().unwrap_or(0)),
        "/trace-claim" => AuditCommand::TraceClaim(rest),
        "/topology" => AuditCommand::Topology,
        "/freeze" => AuditCommand::Freeze,
        "/resume" => AuditCommand::Resume,
        "/replay" => AuditCommand::Replay,
        "/diff-state" => AuditCommand::DiffState,
        "/counterfactual" => AuditCommand::Counterfactual(rest),
        "/ablate-node" => AuditCommand::AblateNode(rest),
        "/inspect-node" => AuditCommand::InspectNode(rest),
        "/inspect-edge" => AuditCommand::InspectEdge(rest),
        "/evidence" => AuditCommand::Evidence,
        "/risk" => AuditCommand::Risk,
        "/status" => AuditCommand::Status,
        "/model" => AuditCommand::Model,
        "/sessions" => AuditCommand::Sessions,
        "/usage" => AuditCommand::Usage,
        "/agents" => AuditCommand::Agents,
        "/export-trace" => AuditCommand::ExportTrace(rest),
        "/help" => AuditCommand::Help,
        other => AuditCommand::Unknown(other.to_string()),
    })
}

pub struct RuntimeProbe;

impl RuntimeProbe {
    pub fn start_session(seq: u64, timestamp_ms: u64, session_id: &str) -> ObservabilityEvent {
        ObservabilityEvent {
            seq,
            timestamp_ms,
            kind: ObservabilityEventKind::SessionStarted,
            truth: ObservabilityTruth::Observed,
            summary: format!("session.started {session_id}"),
        }
    }

    pub fn capture_token(
        seq: u64,
        timestamp_ms: u64,
        token: &str,
        position: usize,
        truth: ObservabilityTruth,
    ) -> ObservabilityEvent {
        ObservabilityEvent {
            seq,
            timestamp_ms,
            kind: ObservabilityEventKind::ModelTokenGenerated(TokenTrace {
                token: token.to_string(),
                position,
                truth,
                ..TokenTrace::default()
            }),
            truth,
            summary: format!("model.token.generated {position}:{token}"),
        }
    }

    pub fn capture_memory_read(
        seq: u64,
        timestamp_ms: u64,
        memory_id: &str,
        content: &str,
        score: f32,
    ) -> ObservabilityEvent {
        ObservabilityEvent {
            seq,
            timestamp_ms,
            kind: ObservabilityEventKind::MemoryRead {
                memory_id: memory_id.to_string(),
                score,
                content_preview: content.chars().take(80).collect(),
            },
            truth: ObservabilityTruth::Observed,
            summary: format!("memory.read {memory_id} score={score:.2}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_applies_backpressure_and_tracks_drops() {
        let mut buffer = ObservabilityRingBuffer::new(2);
        for seq in 0..4 {
            buffer.push(RuntimeProbe::start_session(seq, seq * 10, "s"));
        }
        let seqs: Vec<u64> = buffer.events().map(|event| event.seq).collect();
        assert_eq!(seqs, vec![2, 3]);
        assert_eq!(buffer.dropped(), 2);
    }

    #[test]
    fn reducer_tracks_nodes_edges_tokens_risks_and_truth() {
        let mut state = TuiObservabilityState::default();
        state.apply(&ObservabilityEvent {
            seq: 1,
            timestamp_ms: 10,
            kind: ObservabilityEventKind::NeuralNodeActivated {
                node_id: "planner".to_string(),
                node_type: NodeType::Planner,
                activation: 0.82,
                confidence: 0.91,
                risk: 0.07,
                participates_current_output: true,
                truth: ObservabilityTruth::Observed,
            },
            truth: ObservabilityTruth::Observed,
            summary: "planner activated".to_string(),
        });
        state.apply(&ObservabilityEvent {
            seq: 2,
            timestamp_ms: 20,
            kind: ObservabilityEventKind::NeuralEdgeUpdated {
                source: "planner".to_string(),
                target: "executor".to_string(),
                weight: 0.7,
                flow: 0.6,
                attribution: 0.5,
                risk: 0.1,
                truth: ObservabilityTruth::Estimated,
            },
            truth: ObservabilityTruth::Estimated,
            summary: "planner -> executor".to_string(),
        });
        state.apply(&RuntimeProbe::capture_token(
            3,
            30,
            "Zaion",
            7,
            ObservabilityTruth::Estimated,
        ));
        state.apply(&ObservabilityEvent {
            seq: 4,
            timestamp_ms: 40,
            kind: ObservabilityEventKind::UnsupportedClaimDetected("UNSUPPORTED CLAIM".to_string()),
            truth: ObservabilityTruth::Observed,
            summary: "risk".to_string(),
        });

        let planner = state.nodes.get("planner").unwrap();
        assert_eq!(planner.truth, ObservabilityTruth::Observed);
        assert!(planner.participates_current_output);
        assert_eq!(
            state
                .edges
                .get(&("planner".to_string(), "executor".to_string()))
                .unwrap()
                .truth,
            ObservabilityTruth::Estimated
        );
        assert_eq!(state.tokens[0].token, "Zaion");
        assert_eq!(state.tokens[0].position, 7);
        assert_eq!(state.risks, vec!["UNSUPPORTED CLAIM"]);
    }

    #[test]
    fn audit_commands_cover_required_counterfactual_surface() {
        assert_eq!(
            parse_audit_command("/why this answer"),
            Some(AuditCommand::Why("this answer".to_string()))
        );
        assert_eq!(
            parse_audit_command("/trace-token 42"),
            Some(AuditCommand::TraceToken(42))
        );
        assert_eq!(
            parse_audit_command("/counterfactual remove memory:abc"),
            Some(AuditCommand::Counterfactual(
                "remove memory:abc".to_string()
            ))
        );
        assert_eq!(
            parse_audit_command("/ablate-node heads.3.7"),
            Some(AuditCommand::AblateNode("heads.3.7".to_string()))
        );
        assert_eq!(parse_audit_command("/risk"), Some(AuditCommand::Risk));
        assert_eq!(parse_audit_command("/model"), Some(AuditCommand::Model));
        assert_eq!(
            parse_audit_command("/sessions"),
            Some(AuditCommand::Sessions)
        );
        assert_eq!(parse_audit_command("/usage"), Some(AuditCommand::Usage));
        assert_eq!(parse_audit_command("/agents"), Some(AuditCommand::Agents));
    }
}
