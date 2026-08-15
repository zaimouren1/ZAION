pub mod agent_fsm;
pub mod agent_loop;
pub mod architecture_graph;
pub mod cancel;
pub mod session_actor;
pub mod authenticated_ingress;
pub mod batch_runner;
pub mod compressor;
pub mod context;
pub mod context_strategy;
pub mod cron;
pub mod ego_integration;
pub mod evidence_graph;
pub mod execute_code;
pub mod execute_code_js;
pub mod execute_code_uds;
pub mod genesis;
pub mod hooks;
pub mod mcp_bridge;
pub mod mcp_tools;
pub mod mcts;
pub mod meta;
pub mod moa;
pub mod policy;
pub mod reference;
pub mod sandbox;
pub mod session_branch;
pub mod session_store_adapter;
pub mod shadow_agent;
pub mod slash_commands;
pub mod storage_boundary;
pub mod streaming;
pub mod task;
pub mod task_async;
pub mod todo_tool;
pub mod tool_result_storage;
pub mod ttc;
pub mod tutorial;

pub mod approval_chain;
pub mod circuit_breaker;
pub mod compression_split;
pub mod display_config;
pub mod integrated_agent_loop;
pub mod lifecycle_graph;
pub mod omni_session;
pub mod operation_stream;
pub mod panel_sink;
pub mod platform_lifecycle;
pub mod sandbox_tools;
pub mod task_scheduler;
pub mod tool_broker;
pub mod trinity;
pub mod turn_kernel;
pub mod turn_outcome;
pub mod turn_proof;
pub mod turn_state;
pub mod turn_store;
pub mod unified_agent_runtime;
pub mod wake_request;
pub mod wake_stream;
#[cfg(test)]
pub mod webhook_e2e_test;
pub mod webhook_runtime;

#[cfg(test)]
mod tests;

pub use agent_fsm::*;
pub use agent_loop::*;
pub use approval_chain::{
    ApprovalChain, ApprovalDecision, ApprovalRequest, ApprovalResponse, ApprovalScope,
};
pub use authenticated_ingress::{
    AuthenticatedIngress, AuthenticatedIngressInput, AuthenticatedSource, AuthenticatedSourceInput,
    IngressAttachment, IngressAttachmentInput, IngressValidationError, ProfileId, SubjectId,
    TenantId,
};
pub use batch_runner::{
    BatchCheckpoint, BatchConfig, BatchExecutionRequest, BatchExecutionResult, BatchRunner,
    ShareGptMessage, ToolsetSample, Trajectory, DEFAULT_BATCH_RUNNER_CHECKPOINT_FILE,
    DEFAULT_BATCH_RUNNER_NUM_WORKERS, DEFAULT_BATCH_RUNNER_TRAJECTORY_FILE,
    DEFAULT_BATCH_RUNNER_TRAJECTORY_FORMAT,
};
pub use compression_split::{CompressionSplitRequest, CompressionSplitResult, CompressionSplitter};
pub use compressor::{CompressedContext, CompressorConfig, ContextCompressor, Turn};
pub use context::*;
pub use cron::*;
pub use display_config::{DisplayConfig, ReasoningMode, VerboseMode};
pub use ego_integration::*;
pub use evidence_graph::{
    build_answer_evidence_subgraph, AnswerEvidenceInput, EvidenceEdge, EvidenceEdgeKind,
    EvidenceNode, EvidenceNodeKind, EvidenceSubgraph,
};
pub use execute_code::{
    CodeExecutor, CodeLanguage, ExecuteCodeRequest, ExecuteCodeResult, RpcRequest, RpcResponse,
    ToolCallRecord,
};
pub use execute_code_js::JsCodeExecutor;
pub use execute_code_uds::{UdsCodeExecutor, UdsRpcRequest, UdsRpcResponse};
pub use hooks::*;
pub use integrated_agent_loop::{
    IntegratedAgentConfig, IntegratedAgentExecutionReport, IntegratedAgentLoop,
};
pub use mcp_bridge::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpBridge, McpProvenance, McpSubprocess,
    McpSubprocessConfig, McpSubprocessState,
};
pub use mcp_tools::{McpServerDefinition, McpToolDefinition, McpToolRegistry};
pub use mcts::{Candidate, MctsPlanner};
pub use meta::*;
pub use moa::{
    build_aggregator_prompt, format_moa_output, run_moa_sync, MoaConfig, MoaProposer, MoaResult,
    ProposerResult,
};
pub use omni_session::{
    ChannelAttachment, ChannelKey, ChannelType, ContextLayer, DisplayCapabilities,
    MediaCapabilities, OmniRouteAuthority, OmniSession, OmniSessionGraphReplay, OmniSessionManager,
    UnifiedMessage,
};
pub use platform_lifecycle::{
    LifecycleEvent, LifecycleEventType, LifecycleHookExecutor, PlatformAdapter,
    PlatformLifecycleManager,
};
pub use policy::*;
pub use reference::{expand_references, parse_references, RefError, Reference};
pub use sandbox::*;
pub use sandbox_tools::{
    PatchRequest, PatchResult, SandboxTools, SearchFilesRequest, SearchFilesResult,
    WebExtractRequest, WebExtractResult, WebSearchRequest, WebSearchResult,
};
pub use session_branch::{
    BranchRequest, BranchResult, BranchTurn, SessionBrancher, SessionMetadata, SessionStore,
};
pub use session_store_adapter::SessionStoreAdapter;
pub use shadow_agent::{ExecutionStrategy, ShadowAgent, ShadowResult};
pub use slash_commands::{
    execute_slash_command, parse_slash_command, slash_command_help, slash_command_registry,
    SlashCommand, SlashCommandContext, SlashCommandDef, SlashCommandResult,
};
pub use streaming::*;
pub use task::*;
pub use task_async::{AsyncTask, AsyncTaskEngine, AsyncTaskHandler, TaskStatus};
pub use task_scheduler::{
    ScheduledTask, TaskMode, TaskPriority, TaskScheduler, TaskStatus as ScheduledTaskStatus,
};
pub use todo_tool::{
    TodoItem, TodoPriority as TodoItemPriority, TodoStatus, TodoStore, TodoSummary,
    TodoToolRequest, TodoToolResponse, TodoUpdate,
};
pub use tool_broker::{
    EnvironmentPolicy, FilesystemPolicy, NetworkPolicy, ToolApprovalGrant, ToolApprovalRequirement,
    ToolAuthorization, ToolAuthorizationError, ToolAuthorizationInput, ToolBroker, ToolDenyReason,
    ToolEffect, ToolIdempotency, ToolInvocation, ToolInvocationInput, ToolManifest,
    ToolManifestError, ToolManifestInput, ToolPolicyDecision, ToolRisk,
};
pub use tool_result_storage::{
    enforce_turn_budget, enforce_turn_budget_with_target, generate_preview,
    maybe_store_tool_result, maybe_store_tool_result_with_target,
    maybe_store_tool_result_with_threshold, stable_result_path, HostToolResultStorageTarget,
    StoredToolResult, ToolResultBudgetConfig, ToolResultMessage, ToolResultMetadata,
    ToolResultStorageError, ToolResultStorageResult, ToolResultStorageTarget,
    DEFAULT_PREVIEW_BYTES, DEFAULT_RESULT_BUDGET_BYTES, DEFAULT_TURN_BUDGET_BYTES,
    PERSISTED_OUTPUT_CLOSING_TAG, PERSISTED_OUTPUT_TAG,
};
pub use trinity::{
    MockTrinityEngine, TrinityConfig, TrinityEngine, TrinityPlan, TrinityRole, TrinityVerdict,
};
pub use ttc::{
    ComplexityEstimator, ComplexityScore, DynamicComputeAllocator, ThinkingMode, TtcResult,
};
pub use turn_kernel::{
    HandledTurn, RuntimeOutput, ScheduledTurn, TurnExecution, TurnKernelEntry, TurnKernelTopology,
};
pub use turn_outcome::{
    DegradationReport, PartialLedgerTail, ProofClosure, ProofClosureError, ProofClosureRef,
    ProofClosureVerifier, QuarantineEvent, TurnError, TurnOutcome,
};
pub use turn_proof::{
    build_turn_proof, stable_hash_bytes, stable_hash_json, verify_turn_proof_hash,
    TurnCanonicalUsageEvidence, TurnCapabilityManifest, TurnCompressionEvidence, TurnContextLayer,
    TurnCostEvidence, TurnProof, TurnProofInput, TurnRuntimeMemoryEvidence,
};
pub use turn_state::{TurnState, TurnTransitionError, VersionedTurnState};
pub use turn_store::{
    BeginTurnResult, DurableTurnAdmission, DurableTurnRecord, DurableTurnStore,
    InMemoryOutboxSignerResolver, OutboxCompletion, OutboxDispatchFailure,
    OutboxDispatchFailureClass, OutboxDispatchFailureCode, OutboxDispatchPhase, OutboxDispatcher,
    OutboxDispatcherConfig, OutboxDispatcherError, OutboxDispatcherHealth,
    OutboxDispatcherLastError, OutboxDispatcherLifecycle, OutboxQuarantineRecord,
    OutboxSignerResolveError, OutboxSignerResolver, SigningValidatedOutbox, TurnActorIdentity,
    TurnActorRecord, TurnOutboxRecord, TurnOutboxStatus, TurnStoreError, TURN_OUTBOX_SCHEMA,
    TURN_STORE_SCHEMA,
};
pub use tutorial::TutorialManager;
pub use unified_agent_runtime::{UnifiedAgentConfig, UnifiedAgentResult, UnifiedAgentRuntime};
pub use wake_request::{WakeFeatureDefaults, WakeFeaturePolicy, WakeRequest};
pub use wake_stream::{StreamCallback, StreamEvent, ToolCallEvent, WakeOperationRecorder};
pub use webhook_runtime::{
    AgentTriggerConfig, AgentTriggerResult, PreparedAgentTrigger, WebhookAgentEvent,
    WebhookRuntimeManager,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("memory error: {0}")]
    Memory(#[from] zaion_memory::MemoryError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("task failed: {0}")]
    TaskFailed(String),
    #[error("task error: {0}")]
    Task(String),
    #[error("policy violation: {0}")]
    PolicyViolation(String),
    #[error("internal error: {0}")]
    Internal(String),
}
