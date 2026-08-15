use crate::identity::{PrincipalId, SignatureBytes};
use crate::session::{NamespaceKey, RunId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventId(pub String);

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEvent {
    pub event_id: EventId,
    pub principal_id: PrincipalId,
    pub namespace_key: NamespaceKey,
    pub run_id: Option<RunId>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub signature: Option<SignatureBytes>,
    pub created_at: String,
    /// Optional parent event for DAG lineage (event-level branching).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EventType {
    ProcessCreated,
    ProcessMigrated,
    ChannelReceived,
    ChannelSent,
    TaskStarted,
    TaskCompleted,
    TaskFailed,
    ToolCalled,
    ToolResult,
    ProviderInvoked,
    ProviderResponded,
    SkillDistilled,
    RuleApplied,
    CheckpointWritten,
    CheckpointRestored,
    IdentityVerified,
    OmniRoute,
    AnswerTrace,
    TurnProof,
    ToolReceipt,
    ToolReceiptProofJoin,
    OperationEvent,
    Custom(String),
}

impl EventType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ProcessCreated => "process.created",
            Self::ProcessMigrated => "process.migrated",
            Self::ChannelReceived => "channel.received",
            Self::ChannelSent => "channel.sent",
            Self::TaskStarted => "task.started",
            Self::TaskCompleted => "task.completed",
            Self::TaskFailed => "task.failed",
            Self::ToolCalled => "tool.called",
            Self::ToolResult => "tool.result",
            Self::ProviderInvoked => "provider.invoked",
            Self::ProviderResponded => "provider.responded",
            Self::SkillDistilled => "skill.distilled",
            Self::RuleApplied => "rule.applied",
            Self::CheckpointWritten => "checkpoint.written",
            Self::CheckpointRestored => "checkpoint.restored",
            Self::IdentityVerified => "identity.verified",
            Self::OmniRoute => "omni.route",
            Self::AnswerTrace => "answer.trace",
            Self::TurnProof => "turn.proof",
            Self::ToolReceipt => "tool.receipt",
            Self::ToolReceiptProofJoin => "tool.receipt.proof_join",
            Self::OperationEvent => "operation.event",
            Self::Custom(s) => s.as_str(),
        }
    }
}
