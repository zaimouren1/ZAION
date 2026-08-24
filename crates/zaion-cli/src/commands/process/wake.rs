//! `zaion wake` — single-shot LLM turn against a process.
//!
//! Provides two entrypoints:
//!   * [`cmd_wake`]                — CLI, argv-driven.
//!   * [`cmd_wake_with_request`]   — structured entrypoint (preferred).
//!
//! Pipeline (in order):
//!   1. Load the long-lived process identity and ledger.
//!   2. Build or validate the [`CanonicalEnvelope`].
//!   3. Append the signed `channel.received` ledger event.
//!   4. Resolve provider / model / flags from [`WakeRequest`].
//!   5. Expand `@file:` / `@url:` / `@git:` references in the received body.
//!   6. Run the injection scanner and emit a warning (never block).
//!   7. Dispatch slash commands inline.
//!   8. Optionally hand off to `process_unified` with the received parent.
//!   9. Spin up optional Memory / MCP / Lifecycle subsystems (shared runtime).
//!  10. Build system / context / history messages.
//!  11. Compress history when it exceeds the compressor threshold.
//!  12. Pick the final provider via `SmartRouter` when smart-route is set.
//!  13. Call the LLM (streaming or blocking) through `RetryProvider`.
//!  14. Parse tool calls from the response and forward to callback.
//!  15. Append `channel.sent`, `tool.receipt`, and `turn.proof` ledger events.

use std::{
    collections::BTreeSet,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use zaion_adapters::provider::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCall,
    ToolDefinition,
};
use zaion_adapters::tool_parsers::{get_parser, try_all_parsers};
use zaion_adapters::{RetryConfig, RetryProvider};
use zaion_memory::runtime_integration::{
    BuiltinMemoryProvider, MemoryManager, MemoryRuntimeConfig,
};
use zaion_pricing::{estimate_usage_cost, CanonicalUsage};
use zaion_runtime::mcp_bridge::McpBridge;
use zaion_runtime::mcp_tools::McpToolRegistry;
use zaion_runtime::operation_stream::{
    OperationContext, OperationEventKind, OperationLevel, OperationStage, RedactionClass,
    VisibleToolCall,
};
use zaion_runtime::{
    build_answer_evidence_subgraph, build_turn_proof, expand_references, AnswerEvidenceInput,
    CompressedContext, CompressionSplitRequest, CompressionSplitter, CompressorConfig,
    ContextCompressor, LifecycleHookExecutor, OmniSessionManager, PartialLedgerTail,
    PlatformLifecycleManager, ProofClosureVerifier, RuntimeOutput, StreamCallback, TaskMode,
    TodoStore, ToolCallEvent, Turn, TurnCanonicalUsageEvidence, TurnCapabilityManifest,
    TurnCompressionEvidence, TurnContextLayer, TurnCostEvidence, TurnError, TurnExecution,
    TurnKernelEntry, TurnProofInput, TurnRuntimeMemoryEvidence, TurnState, WakeFeatureDefaults,
    WakeFeaturePolicy, WakeOperationRecorder, WakeRequest,
};
use zaion_safety::{InjectionScanner, SecretRedactor};
use zaion_types::envelope::{compute_source_hash, ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::event::EventType;
use zaion_types::identity::PrincipalId;
use zaion_types::policy::{CapabilityClass, PolicyDecision};
use zaion_types::session::{ChannelId, NamespaceKey, ThreadId};

use crate::commands::provider::{
    build_provider, provider_supports_prompt_cache, resolve_provider_selection,
    resolve_smart_provider_model,
};
use crate::commands::slash_integration::SlashCommandProcessor;
use crate::commands::{data_dir, CliError};
use crate::config::{McpStore, McpTransport, ZaionConfig};

use super::helpers::load_chat_history;
#[cfg(test)]
use super::wake_contract_v2::local_cli_ingress;
use super::wake_contract_v2::{
    active_profile_id, duplicate_execution, turn_contract_v2_enabled, TurnContractAdmission,
    TurnContractV2, V2ToolGateDecision,
};
use super::wake_shared::runtime as shared_rt;

const MAX_NATIVE_TOOL_TURNS: usize = 24;
const MAX_TOOL_RESULT_CONTEXT_CHARS: usize = 16_000;
const MAX_TODO_STATE_TITLE_CHARS: usize = 512;
const MAX_TODO_STATE_NOTES_CHARS: usize = 2_048;
const TODO_STATE_EVENT_TYPE: &str = "zaion.session_todo.state.v1";

/// Soft ceiling on cumulative tokens (input+output) consumed within a single
/// native tool loop. Exceeding it stops the loop early to bound token spend.
const TOOL_LOOP_TOKEN_BUDGET: usize = 120_000;
/// Number of most-recent follow-up turns inspected for diminishing returns.
const DIMINISHING_RETURNS_WINDOW: usize = 3;
/// A follow-up turn producing fewer than this many output tokens counts as
/// negligible progress for the diminishing-returns heuristic.
const DIMINISHING_RETURNS_MIN_OUTPUT: usize = 16;

/// Reason the native tool loop should stop before exhausting all turns.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolLoopStop {
    /// Cumulative token spend crossed the soft budget ceiling.
    TokenBudgetExceeded { used: usize, budget: usize },
    /// The last `DIMINISHING_RETURNS_WINDOW` follow-ups each both requested
    /// no new tool calls' worth of progress and produced negligible output.
    DiminishingReturns { window: usize },
}

impl ToolLoopStop {
    fn into_notice(self) -> String {
        match self {
            ToolLoopStop::TokenBudgetExceeded { used, budget } => format!(
                "Zaion stopped the tool loop early: token budget reached ({} of {} tokens). \
                 Tool results were recorded; ask me to continue for another pass.",
                used, budget
            ),
            ToolLoopStop::DiminishingReturns { window } => format!(
                "Zaion stopped the tool loop early: the last {} follow-up turns produced \
                 negligible new output (diminishing returns). Ask me to continue if needed.",
                window
            ),
        }
    }
}

/// Decide whether to stop the native tool loop early.
///
/// Two guards (ported from Claude Code's loop-control heuristics):
/// 1. **Token budget** — if cumulative input+output tokens exceed
///    [`TOOL_LOOP_TOKEN_BUDGET`], stop to bound spend.
/// 2. **Diminishing returns** — if the most recent
///    [`DIMINISHING_RETURNS_WINDOW`] follow-up turns each produced fewer than
///    [`DIMINISHING_RETURNS_MIN_OUTPUT`] output tokens, the model is spinning
///    without making progress, so stop.
fn evaluate_tool_loop_stop(
    total_tokens_used: usize,
    recent_followup_outputs: &[usize],
) -> Option<ToolLoopStop> {
    if total_tokens_used >= TOOL_LOOP_TOKEN_BUDGET {
        return Some(ToolLoopStop::TokenBudgetExceeded {
            used: total_tokens_used,
            budget: TOOL_LOOP_TOKEN_BUDGET,
        });
    }
    if recent_followup_outputs.len() >= DIMINISHING_RETURNS_WINDOW {
        let window =
            &recent_followup_outputs[recent_followup_outputs.len() - DIMINISHING_RETURNS_WINDOW..];
        if window
            .iter()
            .all(|&out| out < DIMINISHING_RETURNS_MIN_OUTPUT)
        {
            return Some(ToolLoopStop::DiminishingReturns {
                window: DIMINISHING_RETURNS_WINDOW,
            });
        }
    }
    None
}

// ─── Structured entrypoint ──────────────────────────────────────────────────

// ─── Entry points ────────────────────────────────────────────────────────────

pub(crate) fn structured_wake_request(
    pid: impl Into<String>,
    message: impl Into<String>,
    envelope: CanonicalEnvelope,
) -> WakeRequest {
    let tool_result_environment = tool_result_environment_from_envelope(&envelope);
    let mut req = WakeRequest::new(pid, message)
        .with_envelope(envelope)
        .with_tool_result_storage_root(workspace_tool_result_storage_root())
        // M2 migration (all channels adapted): main wake entry defaults to the durable turn contract.
        .with_turn_contract_v2(true);
    if let Some((environment_id, environment_kind)) = tool_result_environment {
        req.tool_result_environment_id = Some(environment_id);
        req.tool_result_environment_kind = Some(environment_kind);
    }
    req
}

fn tool_result_environment_from_envelope(envelope: &CanonicalEnvelope) -> Option<(String, String)> {
    let environment = envelope.metadata.get("tool_result_environment")?;
    let environment_id = environment.get("environment_id")?.as_str()?.trim();
    let environment_kind = environment.get("environment_kind")?.as_str()?.trim();
    if environment_id.is_empty() || environment_kind.is_empty() {
        return None;
    }
    Some((environment_id.to_string(), environment_kind.to_string()))
}

/// CLI entrypoint: parse argv, execute, print to stdout/stderr.
/// Hero mode: run a wake mission with the core tool subset pre-loaded
/// (keeps tool-use tendency high for the configured model).
pub fn cmd_wake_hero(args: &[String]) -> Result<(), CliError> {
    if std::env::var("ZAION_TOOL_SUBSET").is_err() {
        std::env::set_var(
            "ZAION_TOOL_SUBSET",
            "fs_read,fs_write,fs_list,fs_search,shell_exec",
        );
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("zaion hero <pid> <message> - run a mission with the core tool subset");
        println!(
            "  (auto-sets ZAION_TOOL_SUBSET to fs_read,fs_write,fs_list,fs_search,shell_exec)"
        );
        return Ok(());
    }
    cmd_wake(args)
}

pub fn cmd_wake(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_wake_help();
        return Ok(());
    }
    let req = parse_argv(args)?;
    // Envelope attachment happens inside cmd_wake_with_request (idempotent).
    cmd_wake_with_request(req, None)
}

/// Structured entrypoint. The heart of the wake pipeline.
pub fn cmd_wake_with_request(
    req: WakeRequest,
    callback: Option<StreamCallback>,
) -> Result<(), CliError> {
    execute_wake_with_request(req, callback).map(|_| ())
}

pub fn execute_wake_with_request(
    req: WakeRequest,
    callback: Option<StreamCallback>,
) -> Result<TurnExecution, CliError> {
    // Ensure the request carries a validated CanonicalEnvelope. CLI callers
    // (cmd_wake) already attach one; direct callers such as the inline TUI build
    // a bare WakeRequest, so synthesize the envelope here. This is idempotent:
    // adapter-supplied envelopes are returned untouched.
    let mut req = req;
    req.turn_contract_v2 = turn_contract_v2_enabled(req.turn_contract_v2);
    let req = attach_cli_envelope(req)?;
    WakeTurnKernelEntry {
        callback,
        cancel: None,
    }
    .execute(req)
}

struct WakeTurnKernelEntry {
    callback: Option<StreamCallback>,
    cancel: Option<zaion_runtime::cancel::CancelToken>,
}

impl TurnKernelEntry for WakeTurnKernelEntry {
    type Request = WakeRequest;
    type Output = TurnExecution;
    type Error = CliError;

    fn runtime_owner(&self) -> &'static str {
        "TurnKernelEntry:wake"
    }

    fn execute(&self, req: Self::Request) -> Result<Self::Output, Self::Error> {
        self.execute_wake(req)
    }
}

impl WakeTurnKernelEntry {
    fn execute_wake(&self, req: WakeRequest) -> Result<TurnExecution, CliError> {
        let callback = self.callback.clone();
        let cfg = ZaionConfig::load();
        let feature_policy: WakeFeaturePolicy =
            req.effective_features(wake_feature_defaults(&req, &cfg));
        // M2c entry chain: per-turn cancel token (injectable for tests; a cancel
        // marker file written by the command surface is checked alongside it).
        let cancel_token = self.cancel.clone().unwrap_or_default();
        // Clean any stale cancel marker from a previous run; the command
        // surface writes a marker to cancel this turn (marker exists = cancel).
        let cancel_marker = CancelMarker::cleanup(&req.pid);

        let envelope = req.envelope.as_ref().ok_or_else(|| {
        CliError::Usage(
            "wake runtime requires a pre-validated CanonicalEnvelope; use cmd_wake for CLI input or WakeRequest::with_envelope for adapters"
                .to_string(),
        )
    })?;
        let envelope = ingest_envelope(envelope)
            .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?;

        // Resolve pid from the envelope. A caller-supplied pid must match it.
        let pid = if req.pid.is_empty() {
            envelope.principal.as_str().to_string()
        } else if req.pid != envelope.principal.as_str() {
            return Err(CliError::Usage(format!(
                "wake request pid {} does not match canonical envelope principal {}",
                req.pid,
                envelope.principal.as_str()
            )));
        } else {
            req.pid.clone()
        };

        // Build a log sink that routes messages to the callback in TUI mode or
        // to stderr in CLI mode. Never `eprintln!` directly — that would shred
        // the TUI's raw-mode screen.
        let log = Logger::new(callback.clone());

        // ── --unified handoff ───────────────────────────────────────────────────
        // ── @reference expansion ────────────────────────────────────────────────
        // External references, provider selection, memory, MCP, and model access
        // are intentionally delayed until after the canonical envelope is signed.

        // ── Injection scan ──────────────────────────────────────────────────────
        let store = zaion_core::process::ProcessStore::new(data_dir());
        let (process, kp) = store.load(&pid).map_err(CliError::Core)?;
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
        let ns_key = NamespaceKey(pid.clone());
        let mut active_ns_key = ns_key.clone();

        // ── Slash command ───────────────────────────────────────────────────────
        let signing_principal = kp.principal_id();
        if envelope.principal.as_str() != signing_principal.as_str() {
            return Err(CliError::Usage(format!(
                "canonical envelope principal {} does not match loaded identity {}",
                envelope.principal.as_str(),
                signing_principal.as_str()
            )));
        }
        let recovered_durable_turns = if req.turn_contract_v2 {
            TurnContractV2::recover_local_cli(store.ledger_path(&pid), chrono::Utc::now())
                .map_err(CliError::Runtime)?
        } else {
            0
        };
        let profile_id = active_profile_id();
        let mut turn_contract_v2 = if req.turn_contract_v2 {
            match TurnContractV2::begin_local_cli(
                &process,
                &envelope,
                &profile_id,
                store.ledger_path(&pid),
                chrono::Utc::now(),
            )
            .map_err(CliError::Runtime)?
            {
                TurnContractAdmission::Created(contract) => Some(contract),
                TurnContractAdmission::Duplicate(record) => {
                    return duplicate_execution(&record).map_err(CliError::Runtime);
                }
            }
        } else {
            None
        };
        let channel_id = envelope.channel.0.as_str();
        let thread_id = envelope.thread.0.as_str();
        let thread_filter = Some(thread_id);
        let operation_recorder = WakeOperationRecorder::new(
            OperationContext {
                stream_id: format!("wake-{}", uuid::Uuid::new_v4()),
                turn_id: thread_id.to_string(),
                principal_id: pid.clone(),
                channel_id: channel_id.to_string(),
                thread_id: thread_id.to_string(),
            },
            callback.clone(),
            512,
        );
        let mut last_operation_sequence = Some(operation_recorder.emit_turn_started().sequence);
        last_operation_sequence = Some(
            operation_recorder
                .emit(
                    OperationStage::Identity,
                    OperationEventKind::IdentityVerified,
                    OperationLevel::Info,
                    "identity verified",
                    serde_json::json!({
                        "principal_id": signing_principal.as_str(),
                    }),
                    RedactionClass::Public,
                    last_operation_sequence,
                )
                .sequence,
        );
        let execution_result = (|| -> Result<TurnExecution, CliError> {
            let received_event_id = ledger.append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelReceived,
                envelope.to_channel_received_payload(),
                None,
            )?;
            last_operation_sequence = Some(
                operation_recorder
                    .emit(
                        OperationStage::Ingress,
                        OperationEventKind::IngressAccepted,
                        OperationLevel::Info,
                        "canonical ingress accepted",
                        serde_json::json!({
                            "source": envelope.source,
                            "ledger_event_id": received_event_id.0,
                        "turn_contract_v2": turn_contract_v2.is_some(),
                        "recovered_durable_turns": recovered_durable_turns,
                            "durable_turn_id": turn_contract_v2
                                .as_ref()
                                .and_then(TurnContractV2::turn_id),
                            "turn_state": turn_contract_v2
                                .as_ref()
                                .map(|contract| contract.state().state()),
                            "turn_revision": turn_contract_v2
                                .as_ref()
                                .map(|contract| contract.state().revision()),
                        }),
                        RedactionClass::Public,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            let mut omni_session_manager = OmniSessionManager::new(128_000);
            let omni_graph_replay_before = omni_session_manager
                .replay_from_ledger(&ledger, Some(pid.as_str()), 512)
                .ok();
            let omni_route_authority =
                omni_session_manager
                    .route_envelope(&envelope)
                    .map_err(|e| {
                        CliError::Runtime(format!(
                            "omni session authority rejected envelope: {}",
                            e
                        ))
                    })?;
            let omni_route_authority_hash = omni_route_authority.authority_hash();
            let omni_route_payload =
                omni_route_authority.to_ledger_payload(received_event_id.0.clone());
            let omni_route_event_id = ledger.append_signed_typed_event_with_parent(
                &kp,
                &ns_key,
                EventType::OmniRoute,
                omni_route_payload,
                None,
                Some(&received_event_id),
            )?;
            last_operation_sequence = Some(
                operation_recorder
                    .emit_ledger_appended(
                        "omni.route",
                        &omni_route_event_id.0,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            if let Some(contract) = turn_contract_v2.as_mut() {
                contract
                    .transition(TurnState::Routed)
                    .and_then(|_| contract.transition(TurnState::Running))
                    .map_err(CliError::Runtime)?;
                last_operation_sequence = Some(
                    operation_recorder
                        .emit(
                            OperationStage::Policy,
                            OperationEventKind::PolicyChecked,
                            OperationLevel::Info,
                            "turn contract v2 admitted local CLI ingress",
                            serde_json::json!({
                                "schema": "zaion.turn_contract_v2.transition.v1",
                                "tenant_id": contract.ingress().tenant_id().as_str(),
                                "subject_id": contract.ingress().subject_id().as_str(),
                                "workspace_id": contract.ingress().workspace_id().0.as_str(),
                                "state": contract.state().state(),
                                "revision": contract.state().revision(),
                            }),
                            RedactionClass::Public,
                            last_operation_sequence,
                        )
                        .sequence,
                );
            }
            let omni_graph_route_snapshot = serde_json::json!({
                "schema": "zaion.omni_session_graph_route_snapshot.v1",
                "principal_id": pid,
                "active_omni_session_id": omni_route_authority.omni_session_id,
                "message_count": omni_route_authority.message_count,
                "attachment_count": omni_route_authority.attachment_count,
                "session_graph_hash": omni_route_authority.session_graph_hash,
            });

            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let (expanded_message_after_envelope, ref_errors) =
                expand_references(&envelope.body, &cwd);
            for e in &ref_errors {
                log.warn(format!("@ref: {}", e));
            }
            let message: &str = &expanded_message_after_envelope;

            let provider_selection =
                resolve_provider_selection(req.provider.as_deref(), req.model.as_deref(), &cfg)?;
            let provider_type = provider_selection.provider;
            let model_opt = provider_selection.model;

            if req.unified {
                let argv = request_to_argv(&req, &pid);
                let runtime_topology = self
                    .stable_topology()
                    .stage_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                let execution = crate::commands::process_unified::cmd_wake_unified(
                    &argv,
                    &pid,
                    message,
                    &cfg,
                    feature_policy,
                    crate::commands::process_unified::UnifiedWakeHandoff {
                        received_event_id: Some(received_event_id),
                        inherited_omni_route_event_id: Some(omni_route_event_id),
                        inherited_omni_route_authority_hash: Some(omni_route_authority_hash),
                        runtime_owner: self.runtime_owner(),
                        runtime_topology,
                        operation_recorder: &operation_recorder,
                        parent_sequence: last_operation_sequence,
                    },
                )?;
                if let Some(contract) = turn_contract_v2.as_mut() {
                    contract
                        .finish_execution(&execution)
                        .map_err(CliError::Runtime)?;
                }
                return Ok(execution);
            }

            {
                let scan = InjectionScanner::scan(message);
                if !scan.clean {
                    let categories: Vec<&str> =
                        scan.findings.iter().map(|f| f.category.as_str()).collect();
                    log.warn(format!(
                        "potential prompt injection ({})",
                        categories.join(", ")
                    ));
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit(
                                OperationStage::Safety,
                                OperationEventKind::PolicyChecked,
                                OperationLevel::Warning,
                                "prompt injection warning",
                                serde_json::json!({"categories": categories}),
                                RedactionClass::PanelSafe,
                                last_operation_sequence,
                            )
                            .sequence,
                    );
                }
            }

            let session_db_path = data_dir().join("sessions.db");
            let session_store = zaion_ledger::SessionStore::new(&session_db_path);
            let existing_session_entry = session_store.get_session(&ns_key.0).map_err(|error| {
                CliError::Runtime(format!("session store load failed: {}", error))
            })?;
            let session_entry = zaion_ledger::SessionEntry {
                session_id: ns_key.0.clone(),
                principal_id: pid.clone(),
                platform: channel_id.to_string(),
                chat_id: thread_id.to_string(),
                user_id: None,
                thread_id: Some(thread_id.to_string()),
                session_key: format!("wake:{}:{}", channel_id, thread_id),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
                message_count: load_chat_history(&ledger, &ns_key, 50, thread_filter).len() as i64,
                tool_call_count: 0,
                estimated_cost_usd: existing_session_entry
                    .as_ref()
                    .map(|session| session.estimated_cost_usd)
                    .unwrap_or(0.0),
                memory_flushed: false,
                was_auto_reset: false,
                auto_reset_reason: None,
                parent_session_id: None,
                end_reason: None,
            };
            session_store
                .upsert_session(&session_entry)
                .map_err(|error| {
                    CliError::Runtime(format!("session store upsert failed: {}", error))
                })?;
            if let Some(resolved) =
                resolve_active_compression_session(&session_store, &ns_key.0, thread_id).map_err(
                    |error| {
                        CliError::Runtime(format!(
                            "active compression session resolve failed: {}",
                            error
                        ))
                    },
                )?
            {
                active_ns_key = NamespaceKey(resolved.session_id.clone());
                last_operation_sequence = Some(
                operation_recorder
                    .emit(
                        OperationStage::Context,
                        OperationEventKind::ContextCompiling,
                        OperationLevel::Info,
                        "resolved active compression child session",
                        serde_json::json!({
                            "schema": "zaion.active_compression_session_resolution.v1",
                            "parent_session_id": ns_key.0.clone(),
                            "active_session_id": resolved.session_id.clone(),
                            "parent_session_end_reason": resolved.parent_session_end_reason.clone(),
                            "lineage_depth": resolved.lineage_depth,
                            "thread_id": thread_id,
                        }),
                        RedactionClass::Public,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            }
            let session_store_adapter = zaion_runtime::SessionStoreAdapter::new_with_ledger(
                session_store,
                zaion_ledger::EventLedger::new(store.ledger_path(&pid)),
                kp.clone(),
            )
            .map_err(|e| CliError::Runtime(format!("runtime init failed: {}", e)))?;
            let session_brancher = Arc::new(zaion_runtime::SessionBrancher::new(Box::new(
                session_store_adapter,
            )));
            let slash_processor = SlashCommandProcessor::new(active_ns_key.0.clone())
                .with_session_brancher(session_brancher);

            if SlashCommandProcessor::is_slash_command(message) {
                let history = load_chat_history(&ledger, &active_ns_key, 6, thread_filter);
                match slash_processor.process_command(message, &history, None) {
                    Ok(result) => {
                        log.notice(result.message);
                        if !result.should_continue {
                            if let Some(task) = result.scheduled_task {
                                let parent_execution = operation_recorder.finish_handled_turn(
                                    "slash.command.scheduled",
                                    0,
                                    0,
                                    last_operation_sequence,
                                );
                                if let Some(contract) = turn_contract_v2.as_mut() {
                                    contract
                                        .finish_execution(&parent_execution)
                                        .map_err(CliError::Runtime)?;
                                }
                                let execution = dispatch_scheduled_wake_task(
                                    &task, &req, &kp, channel_id, thread_id, &ledger, &ns_key,
                                    callback,
                                )?;
                                return Ok(execution);
                            }
                            let execution = operation_recorder.finish_handled_turn(
                                "slash.command",
                                0,
                                0,
                                last_operation_sequence,
                            );
                            if let Some(contract) = turn_contract_v2.as_mut() {
                                contract
                                    .finish_execution(&execution)
                                    .map_err(CliError::Runtime)?;
                            }
                            return Ok(execution);
                        }
                    }
                    Err(e) => {
                        log.error(format!("slash command: {}", e));
                        return Err(CliError::Runtime(format!("slash command failed: {}", e)));
                    }
                }
            }

            // ── Memory runtime (shared tokio) ───────────────────────────────────────
            let memory_manager = if feature_policy.memory_enabled {
                log.status("Initialising memory runtime");
                Some(build_wake_memory_manager(
                    &cfg,
                    &pid,
                    kp.principal_id().as_str(),
                    &store.process_dir(&pid),
                )?)
            } else {
                None
            };

            // ── MCP tool registry + tool definitions ────────────────────────────────
            let mut builtin_tool_registry = zaion_mcp::McpToolRegistry::new();
            zaion_mcp::register_builtin_tools(&mut builtin_tool_registry);
            let mut tool_defs = collect_builtin_tool_defs(&builtin_tool_registry);
            tool_defs.push(todo_tool_definition());
            // M3 hero mode: optional tool subset via env (smaller context keeps
            // tool-use tendency high for the configured model).
            if let Ok(subset) = std::env::var("ZAION_TOOL_SUBSET") {
                let allowed: Vec<String> = subset
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if !allowed.is_empty() {
                    let before = tool_defs.len();
                    tool_defs.retain(|t| allowed.contains(&t.name));
                    log.status(format!(
                        "tool subset applied: {} -> {} tools",
                        before,
                        tool_defs.len()
                    ));
                }
            }
            let mut mcp_tool_defs: Vec<ToolDefinition> = Vec::new();
            log.status(format!("Native tools loaded ({} tools)", tool_defs.len()));
            let mcp_registry = if feature_policy.mcp_enabled {
                let mcp_config_path = McpStore::path();
                if mcp_config_path.exists() {
                    let http_server_count = McpStore::load()
                        .servers
                        .into_iter()
                        .filter(|server| server.enabled && server.transport == McpTransport::Http)
                        .count();
                    let bridge = Arc::new(McpBridge::new_with_key(Arc::new(kp.clone())));
                    let registry = Arc::new(McpToolRegistry::new(mcp_config_path, bridge));
                    let registry_clone = registry.clone();
                    let load_result = shared_rt().block_on(registry_clone.load_from_config());
                    if let Err(e) = load_result {
                        log.warn(format!("mcp: config load failed: {}", e));
                        None
                    } else {
                        let defs = collect_mcp_tool_defs(&registry);
                        if defs.is_empty() {
                            if http_server_count > 0 {
                                log.warn(format!(
                            "mcp: {} HTTP server(s) configured; wake currently auto-loads stdio MCP tools only",
                            http_server_count
                        ));
                            }
                            log.status("MCP loaded (0 tools)");
                        } else {
                            for def in defs {
                                if tool_defs.iter().any(|existing| existing.name == def.name) {
                                    log.warn(format!(
                                        "mcp: tool '{}' shadowed by Zaion native tool",
                                        def.name
                                    ));
                                } else {
                                    mcp_tool_defs.push(def.clone());
                                    tool_defs.push(def);
                                }
                            }
                            log.status(format!("MCP loaded ({} tools)", mcp_tool_defs.len()));
                        }
                        Some(registry)
                    }
                } else {
                    log.warn(format!("mcp: no config at {:?}", mcp_config_path));
                    None
                }
            } else {
                None
            };

            // ── Lifecycle manager ───────────────────────────────────────────────────
            let lifecycle_manager = Arc::new(PlatformLifecycleManager::new());
            let _lifecycle_executor = LifecycleHookExecutor::new(lifecycle_manager.clone());

            // ── Context engine ──────────────────────────────────────────────────────
            last_operation_sequence = Some(
                operation_recorder
                    .emit(
                        OperationStage::Context,
                        OperationEventKind::ContextCompiling,
                        OperationLevel::Info,
                        "context compiling",
                        serde_json::json!({"budget": 6000}),
                        RedactionClass::Public,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            let ctx_engine = zaion_runtime::ContextEngine::new(
                store.process_dir(&pid),
                kp.principal_id().as_str(),
            );
            let mut ctx = match ctx_engine.build(message, 6000, &ledger) {
                Ok(c) => {
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit(
                                OperationStage::Context,
                                OperationEventKind::ContextCompiled,
                                OperationLevel::Info,
                                "context compiled",
                                serde_json::json!({
                                    "total_tokens": c.total_tokens,
                                    "chunk_count": c.chunks.len(),
                                }),
                                RedactionClass::Public,
                                last_operation_sequence,
                            )
                            .sequence,
                    );
                    Some(c)
                }
                Err(e) => {
                    log.warn(format!("ctx build failed: {}", e));
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit(
                                OperationStage::Context,
                                OperationEventKind::TurnDegraded,
                                OperationLevel::Warning,
                                "context build failed",
                                serde_json::json!({"error": e.to_string()}),
                                RedactionClass::PanelSafe,
                                last_operation_sequence,
                            )
                            .sequence,
                    );
                    None
                }
            };

            let traceable_memory_atoms = if feature_policy.memory_enabled {
                collect_traceable_memory_atoms(&pid, 8)
            } else {
                Vec::new()
            };
            if !traceable_memory_atoms.is_empty() {
                let atom_lines = traceable_memory_atoms
                    .iter()
                    .map(|(id, content)| {
                        format!("- [{}] {}", id, crate::commands::truncate_str(content, 240))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(ref mut ctx) = ctx {
                    ctx.chunks.push(zaion_runtime::ContextChunk {
                        layer: 5,
                        label: "memory_atoms".to_string(),
                        token_estimate: atom_lines.len() / 4,
                        content: atom_lines,
                        lineage: traceable_memory_atoms
                            .iter()
                            .map(|(id, _)| format!("memory:atom:{}", id))
                            .collect(),
                    });
                    ctx.chunks.sort_by_key(|chunk| chunk.layer);
                    ctx.total_tokens = ctx.chunks.iter().map(|chunk| chunk.token_estimate).sum();
                    ctx.budget_used = ctx.total_tokens.min(6000);
                    ctx.budget_remaining = 6000usize.saturating_sub(ctx.budget_used);
                    ctx.system_prompt = ctx
                        .chunks
                        .iter()
                        .map(|chunk| format!("## {}\n{}", chunk.label, chunk.content))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                }
            }

            let identity_contract = crate::commands::identity::startup_contract_for_prompt(
                &cfg,
                Some(&pid),
                Some(&process.workspace_id),
                Some(&process.project_id),
            );
            let mut messages = vec![ChatMessage::text("system", identity_contract.clone())];
            if !tool_defs.is_empty() {
                messages.push(ChatMessage::text("system", build_tool_guidance(&tool_defs)));
            }
            if let Some(ref ctx) = ctx {
                if !ctx.system_prompt.is_empty() {
                    messages.push(ChatMessage::text("system", ctx.system_prompt.clone()));
                }
            }

            // ── Memory prefetch ─────────────────────────────────────────────────────
            let mut runtime_memory_evidence = None;
            if let Some(ref mem_mgr) = memory_manager {
                let session_id = format!("{}:{}", pid, ns_key.0);
                let memory_context =
                    shared_rt().block_on(mem_mgr.prefetch_all(message, &session_id));
                if !memory_context.is_empty() {
                    log.status(format!(
                        "Prefetched {} chars of memory",
                        memory_context.len()
                    ));
                    runtime_memory_evidence = TurnRuntimeMemoryEvidence::from_context(
                        feature_policy.memory_enabled,
                        &memory_context,
                    );
                    messages.push(ChatMessage::text(
                        "system",
                        format!("# Relevant Memories\n\n{}", memory_context),
                    ));
                }
            }
            let context_pack_id = ctx.as_ref().and_then(|ctx| {
                match crate::commands::context_packs::save_runtime_context_pack(
                    &pid,
                    kp.principal_id().as_str(),
                    message,
                    6000,
                    ctx,
                    crate::commands::context_packs::EmbeddingTrace::from_config(&cfg),
                ) {
                    Ok(manifest) => Some(manifest.pack_id),
                    Err(e) => {
                        log.warn(format!("ctx proof save failed: {}", e));
                        None
                    }
                }
            });

            // ── History + compression ───────────────────────────────────────────────
            let raw_history = load_chat_history(&ledger, &active_ns_key, 6, thread_filter);
            let mut todo_store = TodoStore::new();
            let mut todo_state_snapshot_required = false;
            if hydrate_todo_store_from_latest_state_event(
                &ledger,
                &active_ns_key,
                thread_id,
                &mut todo_store,
            ) != Some(true)
            {
                hydrate_todo_store_from_history(&mut todo_store, &raw_history);
            }
            let (mut compressor, token_budget) = wake_context_compressor(&cfg);
            if let Some(previous_summary) =
                latest_persisted_compression_summary(&ledger, &active_ns_key, thread_id)
            {
                compressor.restore_previous_summary(previous_summary);
            } else if let Some(previous_summary) = latest_summary_turn_from_history(&raw_history) {
                compressor.restore_previous_summary(previous_summary);
            }
            let turns: Vec<Turn> = raw_history
                .iter()
                .map(|m| Turn::new(m.role.clone(), m.content.clone()))
                .collect();

            let total_tokens: usize = turns.iter().map(|t| t.token_estimate()).sum();
            let threshold = (token_budget as f64 * compressor.config.threshold_ratio) as usize;
            let mut compression_evidence = build_compression_evidence(
                feature_policy.compression_requested,
                false,
                turns.len(),
                turns.len(),
                0,
                total_tokens,
                total_tokens,
                token_budget,
                threshold,
                "",
                None,
            );

            let history: Vec<ChatMessage> = if feature_policy.compression_enabled
                && (feature_policy.compression_requested || total_tokens > threshold)
                && !raw_history.is_empty()
            {
                let compression_summary_prompt = if feature_policy.compression_requested {
                    compressor.build_compression_summary_prompt_forced(&turns, token_budget)
                } else {
                    compressor.build_compression_summary_prompt(&turns, token_budget)
                };
                let llm_summary = compression_summary_prompt.and_then(|prompt| {
                    generate_provider_backed_compression_summary(
                        &compressor,
                        prompt,
                        &cfg,
                        &provider_type,
                        model_opt.clone(),
                        &log,
                    )
                });
                let compression_parent_ns_key = active_ns_key.clone();
                let split_session_store =
                    zaion_ledger::SessionStore::new(data_dir().join("sessions.db"));
                let split_adapter = zaion_runtime::SessionStoreAdapter::new_with_ledger(
                    split_session_store,
                    zaion_ledger::EventLedger::new(store.ledger_path(&pid)),
                    kp.clone(),
                )
                .map_err(|e| CliError::Runtime(format!("runtime init failed: {}", e)))?;
                let mut splitter = CompressionSplitter::new(compressor, Box::new(split_adapter));
                let split_result = splitter
                    .compress_and_split_with_todo_reinjection(
                        CompressionSplitRequest {
                            current_session_id: active_ns_key.0.clone(),
                            history: turns.clone(),
                            token_budget,
                            llm_summary,
                            force_compression: feature_policy.compression_requested,
                        },
                        &todo_store,
                    )
                    .map_err(|error| {
                        CliError::Runtime(format!("compression split failed: {}", error))
                    })?;
                let result = split_result.compressed;
                compression_evidence = build_compression_evidence(
                    feature_policy.compression_requested,
                    result.was_compressed,
                    turns.len(),
                    result.turns.len(),
                    result.turns_pruned,
                    total_tokens,
                    result.total_tokens,
                    token_budget,
                    threshold,
                    &result.summary_text,
                    Some(&result),
                );
                if result.was_compressed {
                    log.status(format!(
                        "Compressed history {} -> {} turns ({} tokens -> {})",
                        turns.len(),
                        result.turns.len(),
                        total_tokens,
                        result.total_tokens,
                    ));
                }
                if split_result.split_performed {
                    log.status(format!(
                        "Archived compressed parent session {} and opened {}",
                        active_ns_key.0,
                        split_result
                            .new_session_id
                            .as_deref()
                            .unwrap_or("(unknown child session)")
                    ));
                    if let Some(new_session_id) = &split_result.new_session_id {
                        let compressed_turns = result
                            .turns
                            .iter()
                            .map(|turn| ChatMessage::text(turn.role.clone(), turn.content.clone()))
                            .collect::<Vec<_>>();
                        let child_ns_key = NamespaceKey(new_session_id.clone());
                        materialize_compressed_history_for_active_child(
                            &ledger,
                            &kp,
                            &child_ns_key,
                            &compression_parent_ns_key,
                            channel_id,
                            thread_id,
                            &compressed_turns,
                        )?;
                        persist_compression_summary_state(
                            &ledger,
                            &kp,
                            &child_ns_key,
                            &compression_parent_ns_key,
                            channel_id,
                            thread_id,
                            &result,
                        )?;
                        active_ns_key = child_ns_key;
                        todo_state_snapshot_required = todo_store.has_items();
                    }
                } else if result.was_compressed {
                    persist_compression_summary_state(
                        &ledger,
                        &kp,
                        &active_ns_key,
                        &compression_parent_ns_key,
                        channel_id,
                        thread_id,
                        &result,
                    )?;
                }
                // autoCompact circuit breaker: if the pass ran but stalled (failed
                // to remove min_reduction_ratio of tokens) or the result is still
                // over threshold, hard-truncate to the budget instead of shipping
                // oversized context (which would loop on the next wake).
                let final_turns = if result.was_compressed
                    && (!result.compaction_effective || result.still_over_threshold)
                {
                    let breaker = ContextCompressor::with_defaults();
                    let (truncated, dropped) =
                        breaker.hard_truncate_to_budget(&result.turns, token_budget);
                    if dropped > 0 {
                        log.notice(format!(
                            "autoCompact breaker tripped (effective={}, over_threshold={}): \
                         hard-truncated {} stale turns to fit {} token budget",
                            result.compaction_effective,
                            result.still_over_threshold,
                            dropped,
                            token_budget,
                        ));
                    }
                    truncated
                } else {
                    result.turns
                };
                final_turns
                    .into_iter()
                    .map(|t| ChatMessage::text(t.role, t.content))
                    .collect()
            } else {
                raw_history
            };

            messages.extend(history);
            for extra_context in &req.extra_model_context {
                if !extra_context.trim().is_empty() {
                    messages.push(ChatMessage::text("system", extra_context.clone()));
                }
            }
            messages.push(ChatMessage::text("user", message));

            // ── Smart routing ───────────────────────────────────────────────────────
            let (final_provider_type, final_model) = resolve_smart_provider_model(
                message,
                &provider_type,
                model_opt.as_deref(),
                feature_policy.smart_route_enabled,
                feature_policy.mcp_enabled,
            );
            if feature_policy.smart_route_enabled
                && (final_provider_type != provider_type || final_model != model_opt)
            {
                log.status(format!(
                    "smart-route → {}/{} (cheap)",
                    final_provider_type,
                    final_model.as_deref().unwrap_or("(not set)")
                ));
            }
            let cache_enabled = feature_policy.cache_enabled
                && provider_supports_prompt_cache(
                    &final_provider_type,
                    final_model.as_deref(),
                    &cfg,
                )?;

            let (provider, actual_model) = build_provider(&final_provider_type, final_model, &cfg)?;

            // ── Build CompletionRequest ─────────────────────────────────────────────
            let mut completion_req = CompletionRequest {
                model: actual_model,
                messages,
                max_tokens: req.max_tokens.or(Some(4096)),
                temperature: req.temperature.or(Some(0.7)),
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs.clone())
                },
                tool_choice: None,
                enable_cache: cache_enabled,
            };

            // ── Cancellation check ──────────────────────────────────────────────────
            if let Some(ref cb) = callback {
                if cb.is_cancelled() {
                    let execution = operation_recorder.finish_aborted_turn(
                        TurnError {
                            reason_code: "user_cancelled".to_string(),
                            message: "turn cancelled before provider execution".to_string(),
                        },
                        PartialLedgerTail {
                            appended_event_ids: vec![
                                received_event_id.0.clone(),
                                omni_route_event_id.0.clone(),
                            ],
                            last_safe_parent_event_id: Some(omni_route_event_id.0.clone()),
                        },
                        last_operation_sequence,
                    );
                    if let Some(contract) = turn_contract_v2.as_mut() {
                        contract
                            .finish_execution(&execution)
                            .map_err(CliError::Runtime)?;
                    }
                    return Ok(execution);
                }
            }

            // ── LLM call ────────────────────────────────────────────────────────────
            let log_for_retry = log.clone();
            let cb_for_retry = callback.clone();
            let retry_provider = RetryProvider::new(provider, RetryConfig::default())
                .with_on_retry(move |attempt, delay_ms, error| {
                    let msg = format!("retry {}/3: {} ({}ms)", attempt, error, delay_ms);
                    if cb_for_retry.is_some() {
                        log_for_retry.status(msg);
                    } else {
                        eprintln!("[retry] {}", msg);
                    }
                });
            let streamed_visible_token = Arc::new(AtomicBool::new(false));

            let resp_result: Result<_, _> = if req.stream {
                if let Some(ref cb) = callback {
                    let cb_clone = cb.clone();
                    let recorder = operation_recorder.clone();
                    let streamed_visible_token = streamed_visible_token.clone();
                    let parent_sequence = last_operation_sequence;
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit(
                                OperationStage::Reasoning,
                                OperationEventKind::ProviderCalling,
                                OperationLevel::Info,
                                "provider calling",
                                serde_json::json!({
                                    "provider": final_provider_type,
                                    "model": completion_req.model,
                                    "stream": true,
                                }),
                                RedactionClass::Public,
                                parent_sequence,
                            )
                            .sequence,
                    );
                    retry_provider.complete_stream(&completion_req, &mut move |token| {
                        if !token.is_empty() {
                            streamed_visible_token.store(true, Ordering::Relaxed);
                        }
                        cb_clone.send_token(token.to_string());
                        recorder.emit_token_delta(token, parent_sequence);
                    })
                } else {
                    use std::io::Write;
                    let mut out = std::io::stdout();
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit(
                                OperationStage::Reasoning,
                                OperationEventKind::ProviderCalling,
                                OperationLevel::Info,
                                "provider calling",
                                serde_json::json!({
                                    "provider": final_provider_type,
                                    "model": completion_req.model,
                                    "stream": true,
                                }),
                                RedactionClass::Public,
                                last_operation_sequence,
                            )
                            .sequence,
                    );
                    retry_provider.complete_stream(&completion_req, &mut |token| {
                        out.write_all(token.as_bytes()).ok();
                        out.flush().ok();
                    })
                }
            } else {
                last_operation_sequence = Some(
                    operation_recorder
                        .emit(
                            OperationStage::Reasoning,
                            OperationEventKind::ProviderCalling,
                            OperationLevel::Info,
                            "provider calling",
                            serde_json::json!({
                                "provider": final_provider_type,
                                "model": completion_req.model,
                                "stream": false,
                            }),
                            RedactionClass::Public,
                            last_operation_sequence,
                        )
                        .sequence,
                );
                retry_provider.complete(&completion_req)
            };

            let mut resp = match resp_result {
                Ok(r) => r,
                Err(e) => {
                    log.error(e.to_string());
                    return Err(CliError::Runtime(e.to_string()));
                }
            };
            if let Err(e) = ensure_visible_provider_response(&resp) {
                log.error(e.to_string());
                return Err(e);
            }
            let mut total_input_tokens = resp.input_tokens;
            let mut total_output_tokens = resp.output_tokens;
            let mut total_cache_read_tokens = resp.cache_read_tokens;
            let mut total_cache_write_tokens = resp.cache_write_tokens;
            let mut native_tool_calls = Vec::new();
            let mut tool_execution_records = Vec::new();
            let mut used_native_tool_loop = false;
            let mut recent_followup_outputs: Vec<usize> = Vec::new();
            let tool_result_budget_config = wake_tool_result_budget_config(&req);
            let tool_result_storage_target =
                wake_tool_result_storage_target(&req, &tool_result_budget_config);

            // ── Tool-lifecycle hook runner (Claude Code PreToolUse/PostToolUse) ──────
            // Built once per wake from the same identity/ledger as the rest of the
            // turn so hook executions are signed and auditable. Hook firing itself
            // short-circuits when no matching hook is installed, so this is cheap
            // when no lifecycle hooks exist.
            let tool_hook_runner = zaion_runtime::HookRunner::new(
                store.process_dir(&pid).join("hooks.db"),
                zaion_ledger::EventLedger::new(store.ledger_path(&pid)),
                kp.clone(),
                active_ns_key.clone(),
            );

            // ── Forward tool calls from response to callback ────────────────────────
            for turn in 0..MAX_NATIVE_TOOL_TURNS {
                if resp.tool_calls.is_empty() {
                    break;
                }
                // M2c entry chain: a cancelled turn stops before executing more tools.
                if turn_cancelled(&cancel_token, &cancel_marker) {
                    break;
                }
                used_native_tool_loop = true;

                native_tool_calls.extend(resp.tool_calls.clone());
                completion_req
                    .messages
                    .push(assistant_message_from_response(&resp));
                for call in &resp.tool_calls {
                    let visible = visible_tool_call_event(
                        call,
                        "detected tool call before execution",
                        "runtime_tool",
                        "pending",
                        None,
                    );
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit_tool_visible(&visible, last_operation_sequence)
                            .sequence,
                    );
                }

                if let Some(contract) = turn_contract_v2.as_mut() {
                    contract
                        .transition(TurnState::ToolRunning)
                        .map_err(CliError::Runtime)?;
                }
                let mut records = execute_native_tool_calls(
                    &builtin_tool_registry,
                    mcp_registry.as_ref(),
                    &mut todo_store,
                    &resp.tool_calls,
                    &log,
                    callback.as_ref(),
                    &tool_result_budget_config,
                    &tool_result_storage_target,
                    turn_contract_v2.as_ref(),
                    Some(&tool_hook_runner),
                );
                if let Some(contract) = turn_contract_v2.as_mut() {
                    contract
                        .transition(TurnState::Running)
                        .map_err(CliError::Runtime)?;
                }
                enforce_tool_context_turn_budget_with_storage_target(
                    &mut records,
                    &log,
                    &tool_result_budget_config,
                    &tool_result_storage_target,
                );
                for record in &records {
                    completion_req.messages.push(ChatMessage::tool_result(
                        record.call_id.clone(),
                        record.context_output.clone(),
                    ));
                }
                tool_execution_records.extend(records);

                if turn + 1 >= MAX_NATIVE_TOOL_TURNS {
                    resp = CompletionResponse {
                content: format!(
                    "Zaion stopped the tool loop after {} turns. Tool results were recorded; ask me to continue if you want another pass.",
                    MAX_NATIVE_TOOL_TURNS
                ),
                model: completion_req.model.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                reasoning_content: String::new(),
    reasoning_signature: String::new(),
            };
                    break;
                }

                // ── Early-stop guards: bounded token spend + diminishing returns ────
                let tokens_used_so_far =
                    total_input_tokens.saturating_add(total_output_tokens) as usize;
                if let Some(stop) =
                    evaluate_tool_loop_stop(tokens_used_so_far, &recent_followup_outputs)
                {
                    let notice = stop.into_notice();
                    log.notice(notice.clone());
                    resp = CompletionResponse {
                        content: notice,
                        model: completion_req.model.clone(),
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                        tool_calls: Vec::new(),
                        finish_reason: FinishReason::Stop,

                        reasoning_content: String::new(),
    reasoning_signature: String::new(),
                    };
                    break;
                }

                let followup = match retry_provider.complete(&completion_req) {
                    Ok(r) => r,
                    Err(e) => {
                        log.error(e.to_string());
                        return Err(CliError::Runtime(e.to_string()));
                    }
                };
                if let Err(e) = ensure_visible_provider_response(&followup) {
                    log.error(e.to_string());
                    return Err(e);
                }
                recent_followup_outputs.push(followup.output_tokens as usize);
                total_input_tokens = total_input_tokens.saturating_add(followup.input_tokens);
                total_output_tokens = total_output_tokens.saturating_add(followup.output_tokens);
                total_cache_read_tokens =
                    total_cache_read_tokens.saturating_add(followup.cache_read_tokens);
                total_cache_write_tokens =
                    total_cache_write_tokens.saturating_add(followup.cache_write_tokens);
                resp = followup;
            }

            if callback.is_none() {
                if !req.stream || used_native_tool_loop {
                    println!("{}", resp.content);
                }
                if req.stream && !used_native_tool_loop {
                    println!();
                }
                println!(
                    "[tokens: in={} out={}]",
                    total_input_tokens, total_output_tokens
                );
            } else if let Some(ref cb) = callback {
                if should_forward_final_response_to_callback(
                    &req,
                    used_native_tool_loop,
                    streamed_visible_token.load(Ordering::Relaxed),
                    &resp,
                ) {
                    cb.send_token(resp.content.clone());
                    last_operation_sequence = Some(
                        operation_recorder
                            .emit_token_delta(&resp.content, last_operation_sequence)
                            .sequence,
                    );
                }
            }

            // ── Parser-based tool-call extraction from text ─────────────────────────
            let parsed_tool_calls =
                report_parsed_tool_calls(req.parser.as_deref(), &resp.content, callback.as_ref());
            if let Some(ref mem_mgr) = memory_manager {
                let session_id = format!("{}:{}", pid, active_ns_key.0);
                shared_rt().block_on(async {
                    mem_mgr.sync_all(message, &resp.content, &session_id).await;
                    mem_mgr.queue_prefetch_all(message, &session_id).await;
                });
            }
            let session_rollup_store = zaion_ledger::SessionStore::new(&session_db_path);
            let canonical_usage = CanonicalUsage {
                input_tokens: total_input_tokens as u64,
                output_tokens: total_output_tokens as u64,
                cache_read_tokens: total_cache_read_tokens as u64,
                cache_write_tokens: total_cache_write_tokens as u64,
                reasoning_tokens: 0,
            };
            let session_before_cost = session_rollup_store
                .get_session(&active_ns_key.0)
                .map_err(|error| {
                    CliError::Runtime(format!("session store cost load failed: {}", error))
                })?
                .or_else(|| {
                    if active_ns_key.0 != ns_key.0 {
                        session_rollup_store.get_session(&ns_key.0).ok().flatten()
                    } else {
                        None
                    }
                });
            let prior_estimated_cost_usd = session_before_cost
                .as_ref()
                .map(|session| session.estimated_cost_usd)
                .unwrap_or(0.0);
            let turn_cost = estimate_usage_cost(&canonical_usage, &completion_req.model);
            let estimated_cost_usd = turn_cost.as_ref().map(|cost| cost.total_cost_usd);
            let session_estimated_cost_usd =
                prior_estimated_cost_usd + estimated_cost_usd.unwrap_or(0.0);
            let mut cost_evidence = build_cost_evidence(
                &final_provider_type,
                &completion_req.model,
                canonical_usage,
                estimated_cost_usd,
                session_estimated_cost_usd,
                turn_cost.as_ref().map(|cost| cost.provider.as_str()),
            );

            let namespace_transition_event_id = if active_ns_key.0 != ns_key.0 {
                Some(ledger.append_signed_typed_event_with_parent(
                    &kp,
                    &active_ns_key,
                    EventType::ChannelReceived,
                    serde_json::json!({
                        "principal_id": pid,
                        "channel_id": channel_id,
                        "thread_id": thread_id,
                        "message": message,
                        "source": "compression.active_child_continuation",
                        "source_parent_namespace_key": ns_key.0,
                        "source_parent_received_event_id": received_event_id.0,
                        "copy_policy": "active_child_turn_materialization",
                    }),
                    None,
                    Some(&received_event_id),
                )?)
            } else {
                None
            };

            // Canonical sent payload: both `content` and `response` keys, plus tool_calls.
            let response_hash = hash_text(&resp.content);
            let sent_payload = serde_json::json!({
                "principal_id": pid,
                "channel_id": channel_id,
                "thread_id": thread_id,
                "to": thread_id,
                "content": resp.content,
                "response": resp.content,
                "tool_calls": &native_tool_calls,
                "tokens_in": total_input_tokens,
                "tokens_out": total_output_tokens,
                "usage": &cost_evidence.usage,
                "cost_evidence_hash": cost_evidence.evidence_hash,
            });
            let sent_event_id = ledger.append_signed_typed_event_with_parent(
                &kp,
                &active_ns_key,
                EventType::ChannelSent,
                sent_payload,
                None,
                Some(&omni_route_event_id),
            )?;
            last_operation_sequence = Some(
                operation_recorder
                    .emit_ledger_appended("channel.sent", &sent_event_id.0, last_operation_sequence)
                    .sequence,
            );
            let cost_rollup_event_id = persist_usage_cost_rollup(
                &ledger,
                &kp,
                &active_ns_key,
                &pid,
                channel_id,
                thread_id,
                &sent_event_id,
                &cost_evidence,
            )?;
            cost_evidence.rollup_event_id = Some(cost_rollup_event_id.0.clone());
            refresh_session_rollup(
                &session_rollup_store,
                session_before_cost.as_ref(),
                &active_ns_key,
                &ns_key,
                &pid,
                channel_id,
                thread_id,
                session_estimated_cost_usd,
                load_chat_history(&ledger, &active_ns_key, 50, thread_filter).len() as i64,
                (native_tool_calls.len() + parsed_tool_calls.len()) as i64,
            )?;

            let receipt_ids = append_tool_receipts(
                ToolReceiptContext {
                    ledger: &ledger,
                    kp: &kp,
                    ns_key: &active_ns_key,
                    pid: &pid,
                    channel_id,
                    thread_id,
                    user_event_id: Some(&received_event_id),
                    sent_event_id: &sent_event_id,
                },
                &native_tool_calls,
                &parsed_tool_calls,
                &tool_execution_records,
            )?;
            append_latest_todo_state_event(
                &ledger,
                &kp,
                &active_ns_key,
                &pid,
                channel_id,
                thread_id,
                &sent_event_id,
                &tool_execution_records,
                &todo_store,
                todo_state_snapshot_required,
            )?;

            let context_layers = ctx
                .as_ref()
                .map(|built| {
                    built
                        .chunks
                        .iter()
                        .map(|chunk| TurnContextLayer {
                            layer: chunk.layer,
                            label: chunk.label.clone(),
                            token_estimate: chunk.token_estimate,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let memory_atom_ids = traceable_memory_atoms
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let mut context_layers = context_layers;
            let has_memory_atom_layer = context_layers
                .iter()
                .any(|layer| layer.label == "memory_atoms");
            if !memory_atom_ids.is_empty() && !has_memory_atom_layer {
                context_layers.push(TurnContextLayer {
                    layer: 5,
                    label: "memory_atoms".to_string(),
                    token_estimate: traceable_memory_atoms
                        .iter()
                        .map(|(_, content)| content.len() / 4)
                        .sum(),
                });
            }
            let answer_trace_spans = build_answer_trace_spans(
                &resp.content,
                &response_hash,
                context_pack_id.as_deref(),
                &context_layers,
                &traceable_memory_atoms,
            );
            let mut source_ledger_event_ids =
                vec![received_event_id.0.clone(), omni_route_event_id.0.clone()];
            if let Some(event_id) = &namespace_transition_event_id {
                source_ledger_event_ids.push(event_id.0.clone());
            }
            let evidence_graph = build_answer_evidence_subgraph(AnswerEvidenceInput {
                response_hash: response_hash.clone(),
                context_pack_id: context_pack_id.clone(),
                memory_atom_ids: memory_atom_ids.clone(),
                tool_receipt_ids: receipt_ids.clone(),
                source_ledger_event_ids,
                output_ledger_event_id: sent_event_id.0.clone(),
                answer_trace_span_hashes: answer_trace_spans
                    .iter()
                    .filter_map(|span| span.get("evidence_hash"))
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect(),
            });
            let evidence_graph_hash = evidence_graph.graph_hash.clone();
            let compression_evidence_hash = compression_evidence.evidence_hash.clone();
            let cost_evidence_hash = cost_evidence.evidence_hash.clone();
            let runtime_memory_evidence_hash = runtime_memory_evidence
                .as_ref()
                .map(|evidence| evidence.evidence_hash.clone());
            let tools_requested = tool_defs
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>();
            let turn_proof = build_turn_proof(TurnProofInput {
                principal_id: pid.clone(),
                workspace_id: process.workspace_id.clone(),
                project_id: process.project_id.clone(),
                channel_id: channel_id.to_string(),
                thread_id: thread_id.to_string(),
                namespace_key: active_ns_key.0.clone(),
                user_event_id: received_event_id.0.clone(),
                output_event_id: sent_event_id.0.clone(),
                omni_route_event_id: Some(omni_route_event_id.0.clone()),
                omni_route_authority_hash: Some(omni_route_authority_hash.clone()),
                namespace_transition_event_id: namespace_transition_event_id
                    .as_ref()
                    .map(|event_id| event_id.0.clone()),
                identity_contract,
                capability_manifest: TurnCapabilityManifest {
                    provider: final_provider_type,
                    model: completion_req.model.clone(),
                    max_tokens: completion_req.max_tokens,
                    temperature: completion_req.temperature,
                    memory_enabled: feature_policy.memory_enabled,
                    mcp_enabled: feature_policy.mcp_enabled,
                    cache_enabled,
                    smart_route_enabled: feature_policy.smart_route_enabled,
                    compression_requested: feature_policy.compression_requested,
                    tools_requested,
                    boundaries: vec![
                        "identity_contract_required".to_string(),
                        "capability_manifest_required".to_string(),
                        "ledger_event_lineage_required".to_string(),
                        "channel_envelope_required".to_string(),
                        "missing_evidence_must_be_reported_as_unknown".to_string(),
                    ],
                },
                context_pack_id: context_pack_id.clone(),
                context_layers: context_layers.clone(),
                memory_atom_ids: memory_atom_ids.clone(),
                compression_evidence: Some(compression_evidence.clone()),
                cost_evidence: Some(cost_evidence.clone()),
                runtime_memory_evidence: runtime_memory_evidence.clone(),
                evidence_graph_hash: Some(evidence_graph_hash.clone()),
                tokens_in: total_input_tokens,
                tokens_out: total_output_tokens,
                tool_call_count: native_tool_calls.len(),
                tool_receipt_ids: receipt_ids.clone(),
            });
            last_operation_sequence = Some(
                operation_recorder
                    .emit(
                        OperationStage::Proof,
                        OperationEventKind::ProofClosing,
                        OperationLevel::Info,
                        "proof closing",
                        serde_json::json!({
                            "tool_call_count": native_tool_calls.len(),
                            "tokens_in": total_input_tokens,
                            "tokens_out": total_output_tokens,
                            "cost_evidence_hash": cost_evidence_hash,
                            "evidence_graph_hash": evidence_graph_hash,
                        }),
                        RedactionClass::Public,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            let answer_trace_event_id = ledger.append_signed_typed_event_with_parent(
                &kp,
                &active_ns_key,
                EventType::AnswerTrace,
                serde_json::json!({
                    "schema": "zaion.answer_trace.v1",
                    "principal_id": pid,
                    "channel_id": channel_id,
                    "thread_id": thread_id,
                    "user_event_id": received_event_id.0,
                    "output_event_id": sent_event_id.0,
                    "omni_route_event_id": omni_route_event_id.0,
                    "omni_route_authority_hash": omni_route_authority_hash,
                    "namespace_transition_event_id": namespace_transition_event_id
                        .as_ref()
                        .map(|event_id| event_id.0.clone()),
                    "context_pack_id": context_pack_id,
                    "context_layers": context_layers,
                    "memory_atom_ids": memory_atom_ids,
                    "compression_evidence": compression_evidence,
                    "compression_evidence_hash": compression_evidence_hash,
                    "cost_evidence": cost_evidence,
                    "cost_evidence_hash": cost_evidence_hash,
                    "runtime_memory_evidence": runtime_memory_evidence,
                    "runtime_memory_evidence_hash": runtime_memory_evidence_hash,
                    "answer_trace_spans": answer_trace_spans,
                    "evidence_graph_hash": evidence_graph_hash,
                    "evidence_graph": evidence_graph,
                    "tool_call_count": native_tool_calls.len(),
                    "tokens_in": total_input_tokens,
                    "tokens_out": total_output_tokens,
                    "response_hash": response_hash,
                    "omni_graph_replay_before": omni_graph_replay_before,
                    "omni_graph_route_snapshot": omni_graph_route_snapshot,
                    "lineage": [
                        received_event_id.0.as_str(),
                        omni_route_event_id.0.as_str(),
                        sent_event_id.0.as_str()
                    ],
                }),
                None,
                Some(&sent_event_id),
            )?;
            last_operation_sequence = Some(
                operation_recorder
                    .emit_ledger_appended(
                        "answer.trace",
                        &answer_trace_event_id.0,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            let mut proof_payload =
                serde_json::to_value(&turn_proof).map_err(|e| CliError::Runtime(e.to_string()))?;
            let runtime_topology = self
                .stable_topology()
                .stage_names()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if let serde_json::Value::Object(ref mut object) = proof_payload {
                object.insert(
                    "answer_trace_event_id".to_string(),
                    serde_json::Value::String(answer_trace_event_id.0.clone()),
                );
                object.insert(
                    "runtime_owner".to_string(),
                    serde_json::Value::String(self.runtime_owner().to_string()),
                );
                object.insert(
                    "runtime_topology".to_string(),
                    serde_json::Value::Array(
                        runtime_topology
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
            let turn_proof_event_id = ledger.append_signed_typed_event_with_parent(
                &kp,
                &active_ns_key,
                EventType::TurnProof,
                proof_payload,
                None,
                Some(&answer_trace_event_id),
            )?;
            last_operation_sequence = Some(
                operation_recorder
                    .emit_ledger_appended(
                        "turn.proof",
                        &turn_proof_event_id.0,
                        last_operation_sequence,
                    )
                    .sequence,
            );
            let receipt_proof_join_event_id = append_tool_receipt_proof_join_event(
                &ledger,
                &kp,
                &active_ns_key,
                &pid,
                channel_id,
                thread_id,
                &received_event_id,
                &sent_event_id,
                &answer_trace_event_id,
                &turn_proof_event_id,
                &turn_proof.proof_hash,
                &receipt_ids,
            )?;
            let public_key = kp.public_key_bytes();
            let proof_closure = ProofClosureVerifier::new(&ledger, &public_key)
                .verify(
                    &answer_trace_event_id.0,
                    &turn_proof_event_id.0,
                    receipt_proof_join_event_id
                        .as_ref()
                        .map(|event_id| event_id.0.as_str()),
                )
                .map_err(|error| {
                    CliError::Runtime(format!("turn proof closure verification failed: {error}"))
                })?;

            let runtime_output = RuntimeOutput {
                runtime_owner: self.runtime_owner().to_string(),
                runtime_topology,
                provider_response_hash: response_hash,
                context_pack_id: context_pack_id.unwrap_or_default(),
                memory_atom_ids,
                tool_receipt_ids: receipt_ids,
                stream_hash: String::new(),
            };
            let runtime_execution = operation_recorder.finish_completed_turn(
                runtime_output,
                proof_closure,
                total_input_tokens as usize,
                total_output_tokens as usize,
                last_operation_sequence,
            );
            if let Some(contract) = turn_contract_v2.as_mut() {
                contract
                    .finish_execution(&runtime_execution)
                    .map_err(CliError::Runtime)?;
            }

            // ── Queued task chain ───────────────────────────────────────────────────
            if let Some(next_task) = slash_processor.get_next_queue_task() {
                log.status(format!("queued task: {}", next_task.prompt));
                let mut next = req.clone();
                next.message = next_task.prompt;
                let queued_message_id = format!("queued-{}", uuid::Uuid::new_v4());
                let queued_envelope = CanonicalEnvelope::new(
                    "internal-queue",
                    PrincipalId(kp.principal_id().as_str().to_string()),
                    ChannelId(channel_id.to_string()),
                    ThreadId(thread_id.to_string()),
                    queued_message_id,
                    next.message.clone(),
                    None,
                )
                .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?
                .with_metadata("queued_by", serde_json::json!("slash_command_queue"));
                let queued_envelope = ingest_envelope(&queued_envelope).map_err(|e| {
                    CliError::Runtime(format!("canonical envelope rejected: {}", e))
                })?;
                next = next.with_envelope(queued_envelope);
                next.source_message_id = None;
                next.source_hash = None;
                return WakeTurnKernelEntry {
                    callback,
                    cancel: None,
                }
                .execute(next);
            }

            Ok(runtime_execution)
        })();
        match execution_result {
            Ok(execution) => Ok(execution),
            Err(error) => {
                if let Some(contract) = turn_contract_v2.as_mut() {
                    if let Err(commit_error) = contract.fail_execution(&error.to_string()) {
                        return Err(CliError::Runtime(format!(
                            "wake pipeline failed ({error}); durable terminal commit also failed: {commit_error}"
                        )));
                    }
                }
                Err(error)
            }
        }
    }
}

struct ActiveCompressionSession {
    session_id: String,
    parent_session_end_reason: String,
    lineage_depth: usize,
}

fn resolve_active_compression_session(
    session_store: &zaion_ledger::SessionStore,
    root_session_id: &str,
    thread_id: &str,
) -> Result<Option<ActiveCompressionSession>, String> {
    let root = session_store
        .get_session(root_session_id)
        .map_err(|error| error.to_string())?;
    let Some(root) = root else {
        return Ok(None);
    };
    if root.end_reason.as_deref() != Some("compression") {
        return Ok(None);
    }

    let sessions = session_store
        .list_by_principal(&root.principal_id, 512)
        .map_err(|error| error.to_string())?;
    let thread_matches = |entry: &zaion_ledger::SessionEntry| {
        entry.thread_id.as_deref() == Some(thread_id) || entry.chat_id == thread_id
    };

    let mut active = None;
    let mut current_parent = root_session_id.to_string();
    let mut depth = 0usize;
    while let Some(child) = sessions
        .iter()
        .filter(|entry| {
            entry.parent_session_id.as_deref() == Some(current_parent.as_str())
                && entry.end_reason.as_deref() != Some("compression")
                && thread_matches(entry)
        })
        .max_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.session_id.cmp(&right.session_id))
        })
    {
        depth += 1;
        current_parent = child.session_id.clone();
        active = Some(ActiveCompressionSession {
            session_id: child.session_id.clone(),
            parent_session_end_reason: root
                .end_reason
                .clone()
                .unwrap_or_else(|| "compression".to_string()),
            lineage_depth: depth,
        });
    }

    Ok(active)
}

fn materialize_compressed_history_for_active_child(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    child_ns_key: &NamespaceKey,
    parent_ns_key: &NamespaceKey,
    channel_id: &str,
    thread_id: &str,
    compressed_history: &[ChatMessage],
) -> Result<(), CliError> {
    if compressed_history.is_empty() {
        return Ok(());
    }

    let mut copied = 0usize;
    for pair in compressed_history.chunks(2) {
        let Some(user) = pair.first() else {
            continue;
        };
        if user.role != "user" {
            continue;
        }
        let Some(assistant) = pair.get(1) else {
            continue;
        };
        if assistant.role != "assistant" {
            continue;
        }

        let received_event_id = ledger.append_signed_typed_event(
            kp,
            child_ns_key,
            EventType::ChannelReceived,
            serde_json::json!({
                "principal_id": kp.principal_id().as_str(),
                "channel_id": channel_id,
                "thread_id": thread_id,
                "message": user.content,
                "source": "compression.materialized_history",
                "source_parent_namespace_key": parent_ns_key.0,
                "copy_policy": "compressed_history_materialization",
            }),
            None,
        )?;
        ledger.append_signed_typed_event_with_parent(
            kp,
            child_ns_key,
            EventType::ChannelSent,
            serde_json::json!({
                "principal_id": kp.principal_id().as_str(),
                "channel_id": channel_id,
                "thread_id": thread_id,
                "to": thread_id,
                "content": assistant.content,
                "response": assistant.content,
                "source": "compression.materialized_history",
                "source_parent_namespace_key": parent_ns_key.0,
                "copy_policy": "compressed_history_materialization",
            }),
            None,
            Some(&received_event_id),
        )?;
        copied += 1;
    }

    if copied > 0 {
        ledger.append_signed_event(
            kp,
            child_ns_key,
            "session.compressed_history.materialized",
            serde_json::json!({
                "schema": "zaion.compressed_history_materialized.v1",
                "parent_session_id": parent_ns_key.0,
                "child_session_id": child_ns_key.0,
                "channel_id": channel_id,
                "thread_id": thread_id,
                "turn_pairs_materialized": copied,
            }),
            None,
        )?;
    }

    Ok(())
}

fn latest_persisted_compression_summary(
    ledger: &zaion_ledger::EventLedger,
    ns_key: &NamespaceKey,
    thread_id: &str,
) -> Option<String> {
    use zaion_types::session::SessionKey;
    let events = ledger
        .list_events(
            &SessionKey(ns_key.0.clone()),
            Some("zaion.context_summary.persisted.v1"),
            16,
        )
        .ok()?;

    events
        .into_iter()
        .filter(|event| event.payload["thread_id"].as_str() == Some(thread_id))
        .filter_map(|event| {
            event
                .payload
                .get("summary_text")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|summary| !summary.is_empty())
                .map(str::to_string)
        })
        .next()
}

fn latest_summary_turn_from_history(history: &[ChatMessage]) -> Option<String> {
    history.iter().rev().find_map(|message| {
        let content = message.content.trim();
        if message.role == "assistant" && content.starts_with("[CONTEXT COMPACTION]") {
            Some(content.to_string())
        } else {
            None
        }
    })
}

fn persist_compression_summary_state(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    parent_ns_key: &NamespaceKey,
    channel_id: &str,
    thread_id: &str,
    compressed: &CompressedContext,
) -> Result<(), CliError> {
    if !compressed.was_compressed || compressed.summary_text.trim().is_empty() {
        return Ok(());
    }

    ledger.append_signed_event(
        kp,
        ns_key,
        "zaion.context_summary.persisted.v1",
        serde_json::json!({
            "schema": "zaion.context_summary.persisted.v1",
            "parent_session_id": parent_ns_key.0,
            "session_id": ns_key.0,
            "channel_id": channel_id,
            "thread_id": thread_id,
            "summary_text": compressed.summary_text,
            "summary_hash": hash_text(&compressed.summary_text),
            "summary_strategy": compressed.summary_strategy,
            "turns_pruned": compressed.turns_pruned,
            "pruned_tool_outputs": compressed.pruned_tool_outputs,
            "protected_head_turns": compressed.protected_head_turns,
            "protected_tail_turns": compressed.protected_tail_turns,
            "protected_tail_tokens": compressed.protected_tail_tokens,
            "summary_budget_tokens": compressed.summary_budget_tokens,
            "copy_policy": "iterative_compression_summary_state",
        }),
        None,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn dispatch_scheduled_wake_task(
    task: &zaion_runtime::ScheduledTask,
    req: &WakeRequest,
    kp: &zaion_crypto::ZaionKeypair,
    channel_id: &str,
    thread_id: &str,
    ledger: &zaion_ledger::EventLedger,
    ns_key: &zaion_types::session::NamespaceKey,
    callback: Option<StreamCallback>,
) -> Result<TurnExecution, CliError> {
    match task.mode {
        TaskMode::Queue => {
            let next = build_internal_task_wake_request(
                req,
                kp,
                channel_id,
                thread_id,
                task,
                "internal-queue",
                "slash_command_queue",
            )?;
            WakeTurnKernelEntry {
                callback,
                cancel: None,
            }
            .execute(next)
        }
        TaskMode::Background => {
            let background_req = build_internal_task_wake_request(
                req,
                kp,
                channel_id,
                thread_id,
                task,
                "internal-background",
                "slash_command_background",
            )?;
            let background_event_id = ledger.append_signed_event(
                kp,
                ns_key,
                "task.background.started",
                serde_json::json!({
                    "schema": "zaion.background_task_start.v1",
                    "task_id": task.task_id,
                    "session_key": task.session_key,
                    "prompt_hash": hash_text(&task.prompt),
                    "source": "slash_command_background",
                    "message_id": background_req.source_message_id,
                    "source_hash": background_req.source_hash,
                }),
                None,
            )?;
            spawn_background_wake(&background_req, kp.principal_id().as_str(), &task.task_id)?;
            Ok(TurnExecution::scheduled(
                task.task_id.clone(),
                background_event_id.0,
            ))
        }
    }
}

fn build_internal_task_wake_request(
    req: &WakeRequest,
    kp: &zaion_crypto::ZaionKeypair,
    channel_id: &str,
    thread_id: &str,
    task: &zaion_runtime::ScheduledTask,
    envelope_source: &str,
    queued_by: &str,
) -> Result<WakeRequest, CliError> {
    let mut next = req.clone();
    next.message = task.prompt.clone();
    let task_message_id = format!("{}-{}", task.task_id, uuid::Uuid::new_v4());
    let task_envelope = CanonicalEnvelope::new(
        envelope_source,
        PrincipalId(kp.principal_id().as_str().to_string()),
        ChannelId(channel_id.to_string()),
        ThreadId(thread_id.to_string()),
        task_message_id,
        next.message.clone(),
        None,
    )
    .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?
    .with_metadata("queued_by", serde_json::json!(queued_by))
    .with_metadata("scheduled_task_id", serde_json::json!(task.task_id))
    .with_metadata("scheduled_task_mode", serde_json::json!(task.mode));
    let task_envelope = ingest_envelope(&task_envelope)
        .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?;
    next = next.with_envelope(task_envelope);
    next.source_message_id = next
        .envelope
        .as_ref()
        .map(|envelope| envelope.message_id.clone());
    next.source_hash = next
        .envelope
        .as_ref()
        .map(|envelope| envelope.source_hash.clone());
    Ok(next)
}

fn spawn_background_wake(req: &WakeRequest, pid: &str, task_id: &str) -> Result<(), CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::Runtime(e.to_string()))?;
    let mut argv = request_to_argv(req, pid);
    if !argv.is_empty() {
        argv.remove(0);
    }
    std::process::Command::new(exe)
        .args(argv)
        .env("ZAION_BACKGROUND_TASK_ID", task_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CliError::Runtime(format!("background wake spawn failed: {}", e)))?;
    Ok(())
}

fn attach_cli_envelope(req: WakeRequest) -> Result<WakeRequest, CliError> {
    if req.envelope.is_some() {
        return Ok(req);
    }
    let cfg = ZaionConfig::load();
    let pid = if req.pid.is_empty() {
        super::helpers::resolve_default_pid(&cfg)?
    } else {
        req.pid.clone()
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_process, kp) = store.load(&pid).map_err(CliError::Core)?;
    let channel_id = req.channel_id.as_deref().unwrap_or("terminal");
    let thread_id = req.thread_id.as_deref().unwrap_or("default");
    let source_message_id = req.source_message_id.clone().unwrap_or_else(|| {
        if req.turn_contract_v2 && req.source_hash.is_none() {
            return format!("cli-{}", uuid::Uuid::new_v4());
        }
        format!(
            "{}-{}",
            channel_id,
            &compute_source_hash(
                request_source(channel_id),
                kp.principal_id().as_str(),
                channel_id,
                thread_id,
                "auto-message-id",
                &req.message,
            )[..16]
        )
    });
    let source = req
        .source
        .clone()
        .unwrap_or_else(|| request_source(channel_id).to_string());
    let envelope = CanonicalEnvelope::new(
        source,
        PrincipalId(kp.principal_id().as_str().to_string()),
        ChannelId(channel_id.to_string()),
        ThreadId(thread_id.to_string()),
        source_message_id,
        req.message.clone(),
        req.source_hash.clone(),
    )
    .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?;
    let envelope = ingest_envelope(&envelope)
        .map_err(|e| CliError::Runtime(format!("canonical envelope rejected: {}", e)))?;
    Ok(req.with_envelope(envelope))
}

// ─── Provider factory ────────────────────────────────────────────────────────

/// Extract every loaded MCP tool as a [`ToolDefinition`] suitable for passing
/// in `CompletionRequest::tools`. If the registry doesn't expose a listing
/// API, returns an empty vector (caller treats `None` vs `Some(empty)`
/// identically by never setting tools when empty).
fn collect_mcp_tool_defs(registry: &Arc<McpToolRegistry>) -> Vec<ToolDefinition> {
    let defs = shared_rt().block_on(async { registry.list_tools().await });
    defs.into_iter()
        .map(|d| ToolDefinition {
            name: d.name,
            description: d.description,
            parameters: d.parameters,
        })
        .collect()
}

fn collect_builtin_tool_defs(registry: &zaion_mcp::McpToolRegistry) -> Vec<ToolDefinition> {
    registry
        .list_meta()
        .into_iter()
        .map(|meta| ToolDefinition {
            name: meta.name.clone(),
            description: meta.description.clone(),
            parameters: mcp_schema_to_json_schema(&meta.schema),
        })
        .collect()
}

fn todo_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "todo".to_string(),
        description:
            "Manage the current session todo list; returns the full list after every update."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "update", "complete", "remove", "list", "replace"]
                },
                "id": {
                    "type": "string",
                    "description": "Stable task id for add/update/complete/remove"
                },
                "title": {
                    "type": "string",
                    "description": "Task title for add or update"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "cancelled"]
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high", "urgent"],
                    "default": "normal"
                },
                "notes": {
                    "type": "string"
                },
                "active_only": {
                    "type": "boolean",
                    "default": false
                },
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object"
                    }
                }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    }
}

fn hydrate_todo_store_from_history(todo_store: &mut TodoStore, history: &[ChatMessage]) -> bool {
    history
        .iter()
        .rev()
        .filter(|message| message.role == "tool")
        .filter_map(|message| {
            todo_store
                .hydrate_from_tool_response_json(&message.content)
                .ok()
        })
        .find(|hydrated| *hydrated)
        .unwrap_or(false)
}

fn hydrate_todo_store_from_latest_state_event(
    ledger: &zaion_ledger::EventLedger,
    ns_key: &NamespaceKey,
    thread_id: &str,
    todo_store: &mut TodoStore,
) -> Option<bool> {
    use zaion_types::session::SessionKey;
    ledger
        .list_events_by_payload_string(
            &SessionKey(ns_key.0.clone()),
            TODO_STATE_EVENT_TYPE,
            "thread_id",
            thread_id,
            1,
        )
        .ok()?
        .into_iter()
        .filter_map(|event| {
            event
                .payload
                .get("state_json")
                .and_then(|value| value.as_str())
                .and_then(|state_json| todo_store.hydrate_from_tool_response_json(state_json).ok())
        })
        .find(|hydrated| *hydrated)
}

fn mcp_schema_to_json_schema(schema: &zaion_mcp::McpSchema) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for param in &schema.params {
        let mut spec = serde_json::Map::new();
        let json_type = match &param.param_type {
            zaion_mcp::McpParamType::String => "string",
            zaion_mcp::McpParamType::Number => "number",
            zaion_mcp::McpParamType::Boolean => "boolean",
            zaion_mcp::McpParamType::Array => "array",
            zaion_mcp::McpParamType::Object => "object",
        };
        spec.insert(
            "type".to_string(),
            serde_json::Value::String(json_type.to_string()),
        );
        spec.insert(
            "description".to_string(),
            serde_json::Value::String(param.description.clone()),
        );
        if let Some(default) = &param.default {
            spec.insert("default".to_string(), default.clone());
        }
        if param.required {
            required.push(serde_json::Value::String(param.name.clone()));
        }
        properties.insert(param.name.clone(), serde_json::Value::Object(spec));
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn build_tool_guidance(tools: &[ToolDefinition]) -> String {
    let names = tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Zaion tool execution contract:\n\
         - Available tools are provided in the native tool schema for this request: {names}.\n\
         - If filesystem, shell, memory, or external capability evidence is needed, call the matching tool instead of describing a pretend action.\n\
         - Read-only tools may be used proactively for evidence. shell_exec is restricted to the allow-listed commands and workspace scope.\n\
         - Never claim a tool ran unless a tool result message for that call id is present.\n\
         - If no listed tool can do the job, say so plainly and continue with available evidence."
    )
}

// ─── Parser fallback ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ToolExecutionRecord {
    call_id: String,
    name: String,
    arguments_hash: String,
    output_hash: Option<String>,
    policy_decision: PolicyDecision,
    permission_decision: String,
    sandbox_scope: String,
    receipt_status: String,
    context_output: String,
    tool_result_metadata: Option<zaion_runtime::ToolResultMetadata>,
    error: Option<String>,
    todo_state_json: Option<String>,
}

fn assistant_message_from_response(resp: &CompletionResponse) -> ChatMessage {
    ChatMessage {
        role: "assistant".to_string(),
        content: resp.content.clone(),
        tool_calls: resp.tool_calls.clone(),
        tool_call_id: None,
        reasoning_content: (!resp.reasoning_content.is_empty())
            .then(|| resp.reasoning_content.clone()),
        reasoning_signature: (!resp.reasoning_signature.is_empty())
            .then(|| resp.reasoning_signature.clone()),
    }
}

fn ensure_visible_provider_response(resp: &CompletionResponse) -> Result<(), CliError> {
    if resp.content.trim().is_empty() && resp.tool_calls.is_empty() {
        return Err(CliError::Runtime(
            "provider returned no visible assistant content".to_string(),
        ));
    }
    Ok(())
}

/// Cross-process cancel marker: a file the command surface writes to cancel
/// an in-flight wake turn (zero-IPC; removed when the wake process exits).
struct CancelMarker {
    path: std::path::PathBuf,
}

impl CancelMarker {
    /// Remove any stale marker (wake start); the command surface writes it on cancel.
    fn cleanup(pid: &str) -> Self {
        let path = crate::commands::data_dir()
            .join("turns")
            .join(format!("{pid}.cancel"));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    fn exists(&self) -> bool {
        self.path.exists()
    }
}

impl Drop for CancelMarker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// True when the turn should stop: either the injected token was cancelled
/// or the command surface wrote the cross-process cancel marker.
fn turn_cancelled(token: &zaion_runtime::cancel::CancelToken, marker: &CancelMarker) -> bool {
    token.is_cancelled() || marker.exists()
}

fn should_forward_final_response_to_callback(
    req: &WakeRequest,
    used_native_tool_loop: bool,
    streamed_visible_token: bool,
    resp: &CompletionResponse,
) -> bool {
    !resp.content.is_empty() && (!req.stream || used_native_tool_loop || !streamed_visible_token)
}

// Orchestrates one turn's worth of tool calls: PreToolUse gate → ordered
// (parallel-safe / serial) execution → PostToolUse observation. The argument
// list threads the registries, stores, budget, and hook runner the loop needs;
// bundling them into a struct would obscure the data flow rather than clarify it.
#[allow(clippy::too_many_arguments)]
fn execute_native_tool_calls(
    builtin_registry: &zaion_mcp::McpToolRegistry,
    registry: Option<&Arc<McpToolRegistry>>,
    todo_store: &mut TodoStore,
    calls: &[ToolCall],
    log: &Logger,
    callback: Option<&StreamCallback>,
    budget_config: &zaion_runtime::ToolResultBudgetConfig,
    storage_target: &dyn zaion_runtime::ToolResultStorageTarget,
    turn_contract_v2: Option<&TurnContractV2>,
    hook_runner: Option<&zaion_runtime::HookRunner>,
) -> Vec<ToolExecutionRecord> {
    let n = calls.len();
    if n == 0 {
        return Vec::new();
    }

    // ── PreToolUse lifecycle gate (Claude Code hook contract) ───────────────
    // Each call is offered to `tool.pre_use` hooks *before* execution. A hook
    // may veto (fail-closed): a blocked call never runs and yields a "blocked"
    // receipt instead. The gate is a no-op (zero SQLite cost) when no pre hook
    // is installed — the common case.
    let mut records: Vec<Option<ToolExecutionRecord>> = (0..n).map(|_| None).collect();
    let mut blocked: Vec<bool> = vec![false; n];
    if let Some(runner) = hook_runner {
        if runner.has_hooks_for(zaion_runtime::HookRunner::EVENT_PRE_TOOL_USE) {
            for (i, call) in calls.iter().enumerate() {
                if let zaion_runtime::HookGate::Block { hook_name, reason } =
                    runner.fire_pre_tool_use(&call.name, &call.arguments)
                {
                    log.notice(format!(
                        "tool {}: blocked by pre_tool_use hook '{}': {}",
                        call.name, hook_name, reason
                    ));
                    let record = blocked_tool_record(call, &hook_name, &reason);
                    send_visible_tool_call_decision(
                        callback,
                        call,
                        "blocked by pre_tool_use hook",
                        &record.policy_decision,
                    );
                    records[i] = Some(record);
                    blocked[i] = true;
                }
            }
        }
    }

    let mut broker_policies: Vec<Option<PolicyDecision>> = (0..n).map(|_| None).collect();
    if let Some(contract) = turn_contract_v2 {
        for (i, call) in calls.iter().enumerate() {
            if blocked[i] {
                continue;
            }
            let gate = authorize_v2_tool_call(contract, builtin_registry, call);
            if gate.allowed {
                broker_policies[i] = Some(gate.policy);
                continue;
            }

            log.notice(format!(
                "tool {}: blocked by turn contract v2: {}",
                call.name, gate.reason
            ));
            send_visible_tool_call_decision(
                callback,
                call,
                "blocked by turn contract v2 tool broker",
                &gate.policy,
            );
            records[i] = Some(broker_blocked_tool_record(call, gate));
            blocked[i] = true;
        }
    }

    // Classify each call: concurrency-safe builtin (pure read/memory/diagnostic)
    // tools can run in parallel; write/execute/network builtins, the `todo`
    // tool (shared mutable state), and MCP tools must run serially to preserve
    // causal ordering of side effects (Claude Code's streaming-tool pattern).
    // Blocked calls are never safe-grouped — they are already finalized.
    let safe: Vec<bool> = calls
        .iter()
        .enumerate()
        .map(|(i, call)| !blocked[i] && is_concurrency_safe_builtin(builtin_registry, call))
        .collect();

    let mut i = 0;
    while i < n {
        if blocked[i] {
            i += 1;
            continue;
        }
        if safe[i] {
            // Greedily group consecutive concurrency-safe calls so we never
            // reorder a safe read across a serial write that precedes/follows.
            let start = i;
            while i < n && safe[i] {
                i += 1;
            }
            let group = &calls[start..i];
            if group.len() == 1 {
                // Single safe call: avoid thread overhead, run inline.
                records[start] = Some(execute_native_tool_call(
                    builtin_registry,
                    registry,
                    todo_store,
                    &group[0],
                    log,
                    callback,
                    budget_config,
                    storage_target,
                ));
            } else {
                let batch = execute_concurrency_safe_batch(builtin_registry, group, log, callback);
                for (offset, (capability_class, result)) in batch.into_iter().enumerate() {
                    let call = &group[offset];
                    let arguments_json =
                        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
                    let arguments_hash = hash_text(&arguments_json);
                    records[start + offset] = Some(build_builtin_tool_record(
                        call,
                        arguments_hash,
                        result,
                        capability_class,
                        budget_config,
                        storage_target,
                    ));
                }
            }
        } else {
            records[i] = Some(execute_native_tool_call(
                builtin_registry,
                registry,
                todo_store,
                &calls[i],
                log,
                callback,
                budget_config,
                storage_target,
            ));
            i += 1;
        }
    }

    let mut records: Vec<ToolExecutionRecord> = records
        .into_iter()
        .map(|record| record.expect("every tool call produces a record"))
        .collect();
    for (record, broker_policy) in records.iter_mut().zip(broker_policies.into_iter()) {
        if let Some(policy) = broker_policy {
            record.permission_decision = policy.reason_code.clone();
            record.sandbox_scope = policy.sandbox_scope.clone();
            record.policy_decision = policy;
        }
    }

    // ── PostToolUse lifecycle observation (Claude Code hook contract) ───────
    // Observation only: post hooks see the outcome but never alter control
    // flow. Fired on the main thread, in call order, so ledger/log ordering is
    // deterministic even when the executed calls ran in a parallel batch.
    if let Some(runner) = hook_runner {
        if runner.has_hooks_for(zaion_runtime::HookRunner::EVENT_POST_TOOL_USE) {
            for record in &records {
                let success = record.error.is_none();
                let preview = crate::commands::truncate_str(&record.context_output, 256);
                let results = runner.fire_post_tool_use(&record.name, success, &preview);
                for r in &results {
                    if !r.success {
                        log.notice(format!(
                            "tool {}: post_use hook '{}' failed: {}",
                            record.name,
                            r.hook_name,
                            r.error.as_deref().unwrap_or("unknown error")
                        ));
                    }
                }
            }
        }
    }

    records
}

fn authorize_v2_tool_call(
    contract: &TurnContractV2,
    builtin_registry: &zaion_mcp::McpToolRegistry,
    call: &ToolCall,
) -> V2ToolGateDecision {
    if call.name == "todo" {
        return contract.authorize_builtin(
            "todo",
            "1.0",
            CapabilityClass::Write,
            &call.arguments,
            chrono::Utc::now(),
        );
    }

    let Some(tool) = builtin_registry.get(&call.name) else {
        return contract.deny_unmanifested(
            &call.name,
            CapabilityClass::External,
            "tool has no native v2 manifest; MCP and unknown tools fail closed",
        );
    };
    let Some(capability_class) =
        CapabilityClass::try_from_tool_meta(tool.meta.capability_class.as_str())
    else {
        return contract.deny_unmanifested(
            &call.name,
            CapabilityClass::Execute,
            format!(
                "unrecognized native capability class: {}",
                tool.meta.capability_class
            ),
        );
    };
    let arguments = match tool.meta.schema.validate_and_fill(&call.arguments) {
        Ok(arguments) => arguments,
        Err(error) => {
            return contract.deny_unmanifested(
                &call.name,
                capability_class,
                format!("tool arguments rejected before policy evaluation: {error}"),
            )
        }
    };
    contract.authorize_builtin(
        &tool.meta.name,
        &tool.meta.version,
        capability_class,
        &arguments,
        chrono::Utc::now(),
    )
}

fn broker_blocked_tool_record(call: &ToolCall, gate: V2ToolGateDecision) -> ToolExecutionRecord {
    let arguments_json =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
    let arguments_hash = hash_text(&arguments_json);
    let output = serde_json::json!({
        "error": gate.reason,
        "tool": call.name,
        "source": "zaion_runtime::ToolBroker",
        "executed": false,
    })
    .to_string();
    ToolExecutionRecord {
        call_id: call.id.clone(),
        name: call.name.clone(),
        arguments_hash,
        output_hash: Some(hash_text(&output)),
        permission_decision: gate.policy.reason_code.clone(),
        sandbox_scope: gate.policy.sandbox_scope.clone(),
        policy_decision: gate.policy,
        receipt_status: "blocked".to_string(),
        context_output: output.clone(),
        tool_result_metadata: None,
        error: Some(output),
        todo_state_json: None,
    }
}

/// Build a "blocked" receipt for a tool call a PreToolUse hook vetoed.
///
/// The tool never ran. The deny `PolicyDecision` records which hook blocked it,
/// and the surfaced output lets the model see why so it can adapt.
fn blocked_tool_record(call: &ToolCall, hook_name: &str, reason: &str) -> ToolExecutionRecord {
    let arguments_json =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
    let arguments_hash = hash_text(&arguments_json);
    let output = serde_json::json!({
        "error": reason,
        "tool": call.name.clone(),
        "source": "pre_tool_use_hook",
        "blocked_by": hook_name,
    })
    .to_string();
    let policy_decision = PolicyDecision::denied_by_hook(&call.name, hook_name);
    ToolExecutionRecord {
        call_id: call.id.clone(),
        name: call.name.clone(),
        arguments_hash,
        output_hash: Some(hash_text(&output)),
        permission_decision: policy_decision.reason_code.clone(),
        sandbox_scope: policy_decision.sandbox_scope.clone(),
        policy_decision,
        receipt_status: "blocked".to_string(),
        context_output: output,
        tool_result_metadata: None,
        error: Some(format!("blocked by hook '{}': {}", hook_name, reason)),
        todo_state_json: None,
    }
}

/// True if `call` resolves to a builtin tool whose effects are pure and
/// idempotent (read / memory / diagnostic), making it safe to run in parallel
/// with sibling safe calls. The `todo` tool (mutates the session store), MCP
/// tools (async, shared runtime), write/execute/network builtins, and unknown
/// tools are all treated as serial.
fn is_concurrency_safe_builtin(
    builtin_registry: &zaion_mcp::McpToolRegistry,
    call: &ToolCall,
) -> bool {
    if call.name == "todo" {
        return false;
    }
    match builtin_registry.get(&call.name) {
        // Fail closed: only a strictly-recognized, observation-class capability
        // qualifies for parallel execution. Unknown/malformed metadata → serial.
        Some(tool) => CapabilityClass::try_from_tool_meta(tool.meta.capability_class.as_str())
            .is_some_and(CapabilityClass::is_concurrency_safe),
        None => false,
    }
}

/// Run a batch of concurrency-safe builtin tools in parallel.
///
/// Visible decisions and status logging happen on the calling thread (the
/// `StreamCallback` sender is `!Sync`), then the pure handlers execute on
/// scoped worker threads. Results are returned in input order so the caller
/// can rebuild records deterministically.
fn execute_concurrency_safe_batch(
    builtin_registry: &zaion_mcp::McpToolRegistry,
    calls: &[ToolCall],
    log: &Logger,
    callback: Option<&StreamCallback>,
) -> Vec<(CapabilityClass, Result<serde_json::Value, String>)> {
    let mut capability_classes = Vec::with_capacity(calls.len());
    for call in calls {
        let capability_class = builtin_registry
            .get(&call.name)
            .and_then(|tool| {
                CapabilityClass::try_from_tool_meta(tool.meta.capability_class.as_str())
            })
            // Fail closed: a registry miss or unrecognized class is sandboxed as
            // Execute (most restrictive) rather than the permissive Read.
            .unwrap_or(CapabilityClass::Execute);
        let execution_policy = PolicyDecision::allow_builtin(&call.name, capability_class);
        send_visible_tool_call_decision(
            callback,
            call,
            "execute Zaion native tool (parallel)",
            &execution_policy,
        );
        log.status(format!(
            "tool {}: executing via Zaion native (parallel batch of {})",
            call.name,
            calls.len()
        ));
        capability_classes.push(capability_class);
    }

    let results: Vec<Result<serde_json::Value, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = calls
            .iter()
            .map(|call| {
                scope.spawn(move || {
                    let tool = builtin_registry
                        .get(&call.name)
                        .ok_or_else(|| "concurrency-safe call missing from registry".to_string())?;
                    tool.meta
                        .schema
                        .validate_and_fill(&call.arguments)
                        .map_err(|e| e.to_string())
                        .and_then(|validated| tool.call(validated))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("tool worker thread panicked".to_string()))
            })
            .collect()
    });

    capability_classes.into_iter().zip(results).collect()
}

fn default_tool_result_budget_config() -> zaion_runtime::ToolResultBudgetConfig {
    zaion_runtime::ToolResultBudgetConfig {
        result_budget_bytes: MAX_TOOL_RESULT_CONTEXT_CHARS,
        storage_dir: active_wake_tool_result_storage_root(),
        ..zaion_runtime::ToolResultBudgetConfig::default()
    }
}

fn active_wake_tool_result_storage_root() -> std::path::PathBuf {
    workspace_tool_result_storage_root()
}

pub(crate) fn workspace_tool_result_storage_root() -> std::path::PathBuf {
    std::env::current_dir()
        .map(|cwd| cwd.join(".zaion").join("tool-results"))
        .unwrap_or_else(|_| data_dir().join("tool-results"))
}

fn wake_tool_result_budget_config(req: &WakeRequest) -> zaion_runtime::ToolResultBudgetConfig {
    let mut config = default_tool_result_budget_config();
    if let Some(root) = &req.tool_result_storage_root {
        config.storage_dir = root.clone();
    }
    config
}

fn wake_tool_result_storage_target(
    req: &WakeRequest,
    config: &zaion_runtime::ToolResultBudgetConfig,
) -> zaion_runtime::HostToolResultStorageTarget {
    match (
        req.tool_result_environment_id.as_deref(),
        req.tool_result_environment_kind.as_deref(),
    ) {
        (Some(environment_id), Some(environment_kind)) => {
            zaion_runtime::HostToolResultStorageTarget::with_environment(
                config.storage_dir.clone(),
                environment_id,
                environment_kind,
            )
        }
        _ => zaion_runtime::HostToolResultStorageTarget::new(config.storage_dir.clone()),
    }
}

#[cfg(test)]
fn budget_tool_context_output_with_config(
    tool_name: &str,
    tool_call_id: &str,
    raw: &str,
    config: &zaion_runtime::ToolResultBudgetConfig,
) -> zaion_runtime::ToolResultStorageResult<String> {
    zaion_runtime::maybe_store_tool_result(raw, tool_name, tool_call_id, config)
        .map(|stored| stored.injectable_content)
}

fn budget_tool_context_result_with_target(
    tool_name: &str,
    tool_call_id: &str,
    raw: &str,
    config: &zaion_runtime::ToolResultBudgetConfig,
    target: &dyn zaion_runtime::ToolResultStorageTarget,
) -> zaion_runtime::ToolResultStorageResult<zaion_runtime::StoredToolResult> {
    zaion_runtime::maybe_store_tool_result_with_target(
        raw,
        tool_name,
        tool_call_id,
        config.result_budget_bytes,
        config,
        target,
    )
}

#[cfg(test)]
fn budget_tool_context_output_with_target(
    tool_name: &str,
    tool_call_id: &str,
    raw: &str,
    config: &zaion_runtime::ToolResultBudgetConfig,
    target: &dyn zaion_runtime::ToolResultStorageTarget,
) -> zaion_runtime::ToolResultStorageResult<String> {
    budget_tool_context_result_with_target(tool_name, tool_call_id, raw, config, target)
        .map(|stored| stored.injectable_content)
}

fn enforce_tool_context_turn_budget_with_storage_target(
    records: &mut [ToolExecutionRecord],
    log: &Logger,
    config: &zaion_runtime::ToolResultBudgetConfig,
    target: &dyn zaion_runtime::ToolResultStorageTarget,
) {
    if let Err(error) = enforce_tool_context_turn_budget_with_target(records, config, target) {
        log.warn(format!(
            "tool result turn budget enforcement failed; preserving existing bounded outputs: {}",
            error
        ));
    }
}

#[cfg(test)]
fn enforce_tool_context_turn_budget_with_config(
    records: &mut [ToolExecutionRecord],
    config: &zaion_runtime::ToolResultBudgetConfig,
) -> zaion_runtime::ToolResultStorageResult<()> {
    let target = zaion_runtime::HostToolResultStorageTarget::new(config.storage_dir.clone());
    enforce_tool_context_turn_budget_with_target(records, config, &target)
}

fn enforce_tool_context_turn_budget_with_target(
    records: &mut [ToolExecutionRecord],
    config: &zaion_runtime::ToolResultBudgetConfig,
    target: &dyn zaion_runtime::ToolResultStorageTarget,
) -> zaion_runtime::ToolResultStorageResult<()> {
    let mut messages = records
        .iter()
        .map(|record| {
            let mut message = zaion_runtime::ToolResultMessage::new(
                record.name.clone(),
                record.call_id.clone(),
                record.context_output.clone(),
            );
            message.metadata = record.tool_result_metadata.clone();
            message
        })
        .collect::<Vec<_>>();
    zaion_runtime::enforce_turn_budget_with_target(&mut messages, config, target)?;
    for (record, message) in records.iter_mut().zip(messages.into_iter()) {
        record.context_output = message.content;
        if message.metadata.is_some() {
            record.tool_result_metadata = message.metadata;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_native_tool_call(
    builtin_registry: &zaion_mcp::McpToolRegistry,
    registry: Option<&Arc<McpToolRegistry>>,
    todo_store: &mut TodoStore,
    call: &ToolCall,
    log: &Logger,
    callback: Option<&StreamCallback>,
    budget_config: &zaion_runtime::ToolResultBudgetConfig,
    storage_target: &dyn zaion_runtime::ToolResultStorageTarget,
) -> ToolExecutionRecord {
    let arguments_json =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
    let arguments_hash = hash_text(&arguments_json);
    if call.name == "todo" {
        let capability_class = CapabilityClass::External;
        let execution_policy = PolicyDecision::allow_builtin(&call.name, capability_class);
        send_visible_tool_call_decision(
            callback,
            call,
            "update session todo store",
            &execution_policy,
        );
        log.status("tool todo: updating session todo store");
        return match todo_store.handle_json(&call.arguments) {
            Ok(value) => {
                let raw = serde_json::to_string(&value).unwrap_or_else(|_| value.to_string());
                let full_state_json =
                    serde_json::to_string(&todo_store.response()).unwrap_or_else(|_| raw.clone());
                let stored = budget_tool_context_result_with_target(
                    &call.name,
                    &call.id,
                    &raw,
                    budget_config,
                    storage_target,
                );
                let (context_output, tool_result_metadata) = match stored {
                    Ok(stored) => (stored.injectable_content, Some(stored.metadata)),
                    Err(_) => (
                        crate::commands::truncate_str(&raw, MAX_TOOL_RESULT_CONTEXT_CHARS),
                        None,
                    ),
                };
                ToolExecutionRecord {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_hash,
                    output_hash: Some(hash_text(&raw)),
                    permission_decision: execution_policy.reason_code.clone(),
                    sandbox_scope: execution_policy.sandbox_scope.clone(),
                    policy_decision: execution_policy,
                    receipt_status: "executed".to_string(),
                    context_output,
                    tool_result_metadata,
                    error: None,
                    todo_state_json: Some(full_state_json),
                }
            }
            Err(error) => {
                let policy_decision = PolicyDecision::failed_builtin(&call.name, capability_class);
                let output = serde_json::json!({
                    "error": error.clone(),
                    "tool": call.name.clone(),
                    "source": "zaion_session_todo"
                })
                .to_string();
                ToolExecutionRecord {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_hash,
                    output_hash: Some(hash_text(&output)),
                    permission_decision: policy_decision.reason_code.clone(),
                    sandbox_scope: policy_decision.sandbox_scope.clone(),
                    policy_decision,
                    receipt_status: "failed".to_string(),
                    context_output: output.clone(),
                    tool_result_metadata: None,
                    error: Some(error),
                    todo_state_json: None,
                }
            }
        };
    }
    if let Some(tool) = builtin_registry.get(&call.name) {
        let capability_class = CapabilityClass::from_tool_meta(tool.meta.capability_class.as_str());
        let execution_policy = PolicyDecision::allow_builtin(&call.name, capability_class);
        send_visible_tool_call_decision(
            callback,
            call,
            "execute Zaion native tool",
            &execution_policy,
        );
        log.status(format!("tool {}: executing via Zaion native", call.name));
        let result = tool
            .meta
            .schema
            .validate_and_fill(&call.arguments)
            .map_err(|e| e.to_string())
            .and_then(|validated| tool.call(validated));
        return build_builtin_tool_record(
            call,
            arguments_hash,
            result,
            capability_class,
            budget_config,
            storage_target,
        );
    }

    let Some(registry) = registry else {
        let output = serde_json::json!({
            "error": "unknown tool; not found in Zaion native tools and MCP registry is not loaded",
            "tool": call.name.clone(),
            "hint": "use one of the listed tools or enable/configure MCP for external tools"
        })
        .to_string();
        let policy_decision = PolicyDecision::deny_unknown_tool(&call.name);
        send_visible_tool_call_decision(
            callback,
            call,
            "deny unknown tool before execution",
            &policy_decision,
        );
        return ToolExecutionRecord {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments_hash,
            output_hash: Some(hash_text(&output)),
            permission_decision: policy_decision.reason_code.clone(),
            sandbox_scope: policy_decision.sandbox_scope.clone(),
            policy_decision,
            receipt_status: "failed".to_string(),
            context_output: output,
            tool_result_metadata: None,
            error: Some("unknown tool and MCP registry not loaded".to_string()),
            todo_state_json: None,
        };
    };

    let execution_policy = PolicyDecision::allow_mcp(&call.name);
    send_visible_tool_call_decision(callback, call, "execute MCP tool", &execution_policy);
    log.status(format!("tool {}: executing via MCP", call.name));
    match shared_rt().block_on(registry.call_tool(&call.name, call.arguments.clone())) {
        Ok(value) => {
            let raw = serde_json::to_string(&value).unwrap_or_else(|_| value.to_string());
            let stored = budget_tool_context_result_with_target(
                &call.name,
                &call.id,
                &raw,
                budget_config,
                storage_target,
            );
            let (context_output, tool_result_metadata) = match stored {
                Ok(stored) => (stored.injectable_content, Some(stored.metadata)),
                Err(_) => (
                    crate::commands::truncate_str(&raw, MAX_TOOL_RESULT_CONTEXT_CHARS),
                    None,
                ),
            };
            ToolExecutionRecord {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_hash,
                output_hash: Some(hash_text(&raw)),
                permission_decision: execution_policy.reason_code.clone(),
                sandbox_scope: execution_policy.sandbox_scope.clone(),
                policy_decision: execution_policy,
                receipt_status: "executed".to_string(),
                context_output,
                tool_result_metadata,
                error: None,
                todo_state_json: None,
            }
        }
        Err(error) => {
            let output = serde_json::json!({
                "error": error.clone(),
                "tool": call.name.clone(),
            })
            .to_string();
            let policy_decision = PolicyDecision::failed_mcp(&call.name);
            ToolExecutionRecord {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_hash,
                output_hash: Some(hash_text(&output)),
                permission_decision: policy_decision.reason_code.clone(),
                sandbox_scope: policy_decision.sandbox_scope.clone(),
                policy_decision,
                receipt_status: "failed".to_string(),
                context_output: output.clone(),
                tool_result_metadata: None,
                error: Some(error),
                todo_state_json: None,
            }
        }
    }
}

/// Build a `ToolExecutionRecord` from a native builtin tool's result.
///
/// Extracted so the orchestrator can run concurrency-safe builtin tools on a
/// worker thread and still produce an identical record off the main thread.
fn build_builtin_tool_record(
    call: &ToolCall,
    arguments_hash: String,
    result: Result<serde_json::Value, String>,
    capability_class: CapabilityClass,
    budget_config: &zaion_runtime::ToolResultBudgetConfig,
    storage_target: &dyn zaion_runtime::ToolResultStorageTarget,
) -> ToolExecutionRecord {
    match result {
        Ok(value) => {
            let raw = serde_json::to_string(&value).unwrap_or_else(|_| value.to_string());
            let stored = budget_tool_context_result_with_target(
                &call.name,
                &call.id,
                &raw,
                budget_config,
                storage_target,
            );
            let (context_output, tool_result_metadata) = match stored {
                Ok(stored) => (stored.injectable_content, Some(stored.metadata)),
                Err(_) => (
                    crate::commands::truncate_str(&raw, MAX_TOOL_RESULT_CONTEXT_CHARS),
                    None,
                ),
            };
            let execution_policy = PolicyDecision::allow_builtin(&call.name, capability_class);
            ToolExecutionRecord {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_hash,
                output_hash: Some(hash_text(&raw)),
                permission_decision: execution_policy.reason_code.clone(),
                sandbox_scope: execution_policy.sandbox_scope.clone(),
                policy_decision: execution_policy,
                receipt_status: "executed".to_string(),
                context_output,
                tool_result_metadata,
                error: None,
                todo_state_json: None,
            }
        }
        Err(error) => {
            let policy_decision = PolicyDecision::failed_builtin(&call.name, capability_class);
            let output = serde_json::json!({
                "error": error.clone(),
                "tool": call.name.clone(),
                "source": "zaion_native"
            })
            .to_string();
            ToolExecutionRecord {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_hash,
                output_hash: Some(hash_text(&output)),
                permission_decision: policy_decision.reason_code.clone(),
                sandbox_scope: policy_decision.sandbox_scope.clone(),
                policy_decision,
                receipt_status: "failed".to_string(),
                context_output: output.clone(),
                tool_result_metadata: None,
                error: Some(error),
                todo_state_json: None,
            }
        }
    }
}

fn send_visible_tool_call_decision(
    callback: Option<&StreamCallback>,
    call: &ToolCall,
    purpose: &str,
    policy_decision: &PolicyDecision,
) {
    if let Some(cb) = callback {
        cb.send_tool_call(visible_tool_call_event(
            call,
            purpose,
            &policy_decision.sandbox_scope,
            &policy_decision.effect,
            Some(policy_decision.permission_id.clone()),
        ));
    }
}

fn collect_traceable_memory_atoms(pid: &str, limit: usize) -> Vec<(String, String)> {
    let store = crate::commands::memory_atoms::MemoryAtomStore::load_for_pid(pid);
    store
        .atoms
        .into_iter()
        .filter(|atom| atom.valid_until.is_none())
        .take(limit)
        .map(|atom| (atom.id, atom.content))
        .collect()
}

fn build_wake_memory_manager(
    cfg: &ZaionConfig,
    pid: &str,
    principal_id: &str,
    process_dir: &std::path::Path,
) -> Result<Arc<MemoryManager>, CliError> {
    let manager = Arc::new(MemoryManager::new());
    let runtime_config = MemoryRuntimeConfig {
        enabled: cfg.memory.enabled,
        semantic_enabled: cfg.memory.semantic_enabled,
        principal_enabled: cfg.memory.principal_enabled,
        default_top_k: cfg.memory.default_top_k,
        context_max_tokens: cfg.memory.default_query_budget,
        auto_prefetch: cfg.memory.enabled,
        auto_sync: cfg.memory.enabled,
    };
    let provider = BuiltinMemoryProvider::new(
        principal_id.to_string(),
        Arc::new(zaion_memory::SemanticStore::new(process_dir)),
        Arc::new(zaion_memory::PrincipalMemoryStore::new(process_dir)),
        Arc::new(zaion_memory::TypedMemoryStore::new(process_dir)),
        runtime_config,
    );
    shared_rt().block_on(manager.add_provider(Box::new(provider)));
    let tool_count = shared_rt().block_on(manager.get_all_tool_schemas()).len();
    let _ = (pid, tool_count);
    Ok(manager)
}

fn build_answer_trace_spans(
    answer: &str,
    response_hash: &str,
    context_pack_id: Option<&str>,
    context_layers: &[TurnContextLayer],
    memory_atoms: &[(String, String)],
) -> Vec<serde_json::Value> {
    answer_spans(answer)
        .into_iter()
        .enumerate()
        .map(|(idx, span)| {
            let matched_memory_atom_ids = memory_atoms
                .iter()
                .filter(|(_, content)| evidence_overlap(&span, content) > 0)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let matched_context_layers = if matched_memory_atom_ids.is_empty() {
                Vec::new()
            } else {
                context_layers
                    .iter()
                    .filter(|layer| layer.label == "memory_atoms")
                    .map(|layer| {
                        serde_json::json!({
                            "layer": layer.layer,
                            "label": layer.label,
                            "token_estimate": layer.token_estimate,
                        })
                    })
                    .collect::<Vec<_>>()
            };
            let evidence_kind =
                if matched_memory_atom_ids.is_empty() || matched_context_layers.is_empty() {
                    "response_only"
                } else {
                    "memory_context_overlap"
                };
            let mut span_payload = serde_json::json!({
                "schema": "zaion.answer_trace_span.v1",
                "span_index": idx + 1,
                "span_hash": hash_text(&span),
                "response_hash": response_hash,
                "context_pack_id": context_pack_id,
                "context_layers": matched_context_layers,
                "memory_atom_ids": matched_memory_atom_ids,
                "evidence_kind": evidence_kind,
            });
            let evidence_hash = hash_text(&span_payload.to_string());
            if let Some(object) = span_payload.as_object_mut() {
                object.insert(
                    "evidence_hash".to_string(),
                    serde_json::Value::String(evidence_hash),
                );
            }
            span_payload
        })
        .collect()
}

fn answer_spans(answer: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut current = String::new();
    for ch in answer.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') {
            let span = current.trim();
            if !span.is_empty() {
                spans.push(span.to_string());
            }
            current.clear();
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        spans.push(tail.to_string());
    }
    if spans.is_empty() && !answer.trim().is_empty() {
        spans.push(answer.trim().to_string());
    }
    spans
}

fn evidence_overlap(span: &str, evidence: &str) -> usize {
    let evidence_words = normalized_words(evidence);
    normalized_words(span)
        .into_iter()
        .filter(|word| evidence_words.contains(word))
        .count()
}

fn normalized_words(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|word| {
            let word = word.trim().to_ascii_lowercase();
            (word.len() >= 4).then_some(word)
        })
        .collect()
}

fn report_parsed_tool_calls(
    parser_name: Option<&str>,
    content: &str,
    callback: Option<&StreamCallback>,
) -> Vec<ToolCall> {
    let Some(pname) = parser_name else {
        return Vec::new();
    };
    let tool_calls = if pname == "auto" {
        try_all_parsers(content)
    } else if let Some(parser) = get_parser(pname) {
        parser.parse(content)
    } else {
        if callback.is_none() {
            eprintln!("[parser] unknown parser '{}', using auto", pname);
        }
        try_all_parsers(content)
    };
    for tc in &tool_calls {
        if let Some(cb) = callback {
            cb.send_tool_call(visible_tool_call_event(
                tc,
                "record parsed tool request without execution",
                "recorded_only",
                "not_executed",
                None,
            ));
        } else {
            eprintln!("[parser] {}({:?})", tc.name, tc.arguments);
        }
    }
    tool_calls
}

fn visible_tool_call_event(
    call: &ToolCall,
    purpose: &str,
    safety_class: &str,
    permission_state: &str,
    policy_decision_id: Option<String>,
) -> ToolCallEvent {
    let visible = VisibleToolCall::new(
        call.id.clone(),
        call.name.clone(),
        "runtime_tool",
        purpose,
        call.arguments.clone(),
        safety_class,
        permission_state,
        policy_decision_id,
    )
    .redacted_for_panel();
    ToolCallEvent::from_visible_tool_call(&visible)
}

struct ToolReceiptContext<'a> {
    ledger: &'a zaion_ledger::EventLedger,
    kp: &'a zaion_crypto::keypair::ZaionKeypair,
    ns_key: &'a zaion_types::session::NamespaceKey,
    pid: &'a str,
    channel_id: &'a str,
    thread_id: &'a str,
    user_event_id: Option<&'a zaion_types::event::EventId>,
    sent_event_id: &'a zaion_types::event::EventId,
}

fn append_tool_receipts(
    ctx: ToolReceiptContext<'_>,
    native_tool_calls: &[ToolCall],
    parsed_tool_calls: &[ToolCall],
    execution_records: &[ToolExecutionRecord],
) -> Result<Vec<String>, CliError> {
    let mut receipt_ids = Vec::new();
    for record in execution_records {
        let tool_result_storage = tool_result_storage_receipt_payload(&record.tool_result_metadata);
        let tool_result_storage_binding =
            tool_result_storage_binding_receipt_payload(&ctx, record, tool_result_storage.as_ref());
        let payload = serde_json::json!({
            "schema": "zaion.tool_receipt.v1",
            "principal_id": ctx.pid,
            "channel_id": ctx.channel_id,
            "thread_id": ctx.thread_id,
            "tool_name": record.name,
            "tool_call_id": record.call_id,
            "source": "native-provider",
            "arguments_hash": record.arguments_hash,
            "output_hash": record.output_hash,
            "permission_id": record.policy_decision.permission_id,
            "capability_class": record.policy_decision.capability_class,
            "policy_effect": record.policy_decision.effect,
            "permission_decision": record.permission_decision,
            "sandbox_scope": record.sandbox_scope,
            "permission_proof": record.policy_decision.permission_proof(),
            "receipt_status": record.receipt_status,
            "error": record.error,
            "tool_result_storage": tool_result_storage,
            "tool_result_storage_binding": tool_result_storage_binding,
            "parent_output_event_id": ctx.sent_event_id.0,
        });
        let receipt_id = ctx.ledger.append_signed_typed_event_with_parent(
            ctx.kp,
            ctx.ns_key,
            EventType::ToolReceipt,
            payload,
            None,
            Some(ctx.sent_event_id),
        )?;
        receipt_ids.push(receipt_id.0);
    }

    for call in native_tool_calls {
        if execution_records
            .iter()
            .any(|record| record.call_id == call.id)
        {
            continue;
        }
        let receipt_id = append_recorded_only_tool_receipt(&ctx, "native-provider", call)?;
        receipt_ids.push(receipt_id.0);
    }

    for call in parsed_tool_calls {
        let receipt_id = append_recorded_only_tool_receipt(&ctx, "parsed-response", call)?;
        receipt_ids.push(receipt_id.0);
    }
    Ok(receipt_ids)
}

fn tool_result_storage_receipt_payload(
    metadata: &Option<zaion_runtime::ToolResultMetadata>,
) -> Option<serde_json::Value> {
    let metadata = metadata.as_ref()?;
    if !metadata.stored {
        return None;
    }
    let storage_root = metadata.path.as_ref().and_then(|path| path.parent());
    Some(serde_json::json!({
        "schema": "zaion.tool_result_storage.v1",
        "tool_name": metadata.tool_name,
        "tool_call_id": metadata.tool_call_id,
        "stored": metadata.stored,
        "truncated": metadata.truncated,
        "bytes": metadata.bytes,
        "preview_bytes": metadata.preview_bytes,
        "path": metadata.path.as_ref().map(|path| path.to_string_lossy().to_string()),
        "storage_root": storage_root.map(|path| path.to_string_lossy().to_string()),
        "environment_id": metadata.environment_id,
        "environment_kind": metadata.environment_kind,
    }))
}

fn tool_result_storage_binding_receipt_payload(
    ctx: &ToolReceiptContext<'_>,
    record: &ToolExecutionRecord,
    storage: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let storage = storage?;
    let storage_root = storage
        .get("storage_root")
        .and_then(serde_json::Value::as_str)?;
    let path = storage
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let environment_id = storage
        .get("environment_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("storage-root:{}", hash_text(storage_root)));
    let environment_kind = storage
        .get("environment_kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("storage_target");
    let user_event_id = ctx.user_event_id.map(|event_id| event_id.0.clone());
    let mut event_lineage = Vec::new();
    if let Some(user_event_id) = &user_event_id {
        event_lineage.push(user_event_id.clone());
    }
    event_lineage.push(ctx.sent_event_id.0.clone());

    let binding_without_hash = serde_json::json!({
        "schema": "zaion.tool_result_storage_binding.v1",
        "environment": {
            "environment_id": environment_id,
            "environment_kind": environment_kind,
            "storage_root": storage_root,
            "path": path,
        },
        "permission_scope": {
            "permission_id": record.policy_decision.permission_id,
            "capability_class": record.policy_decision.capability_class,
            "policy_effect": record.policy_decision.effect,
            "permission_decision": record.permission_decision,
            "sandbox_scope": record.sandbox_scope,
            "permission_proof_hash": hash_text(&record.policy_decision.permission_proof().to_string()),
        },
        "provenance_chain": {
            "principal_id": ctx.pid,
            "namespace_key": ctx.ns_key.0,
            "channel_id": ctx.channel_id,
            "thread_id": ctx.thread_id,
            "parent_output_event_id": ctx.sent_event_id.0,
            "tool_name": record.name,
            "tool_call_id": record.call_id,
            "arguments_hash": record.arguments_hash,
            "output_hash": record.output_hash,
        },
        "turn_proof_material": {
            "user_event_id": user_event_id,
            "output_event_id": ctx.sent_event_id.0,
            "event_lineage": event_lineage,
            "turn_proof_event_id": serde_json::Value::Null,
            "turn_proof_hash": serde_json::Value::Null,
        },
    });
    let binding_hash = hash_text(&binding_without_hash.to_string());
    let mut binding = binding_without_hash;
    if let serde_json::Value::Object(ref mut object) = binding {
        object.insert(
            "binding_hash".to_string(),
            serde_json::Value::String(binding_hash),
        );
    }
    Some(binding)
}

#[allow(clippy::too_many_arguments)]
fn append_tool_receipt_proof_join_event(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::keypair::ZaionKeypair,
    ns_key: &zaion_types::session::NamespaceKey,
    pid: &str,
    channel_id: &str,
    thread_id: &str,
    received_event_id: &zaion_types::event::EventId,
    sent_event_id: &zaion_types::event::EventId,
    answer_trace_event_id: &zaion_types::event::EventId,
    turn_proof_event_id: &zaion_types::event::EventId,
    turn_proof_hash: &str,
    receipt_ids: &[String],
) -> Result<Option<zaion_types::event::EventId>, CliError> {
    if receipt_ids.is_empty() {
        return Ok(None);
    }

    let join_without_hash = serde_json::json!({
        "schema": "zaion.tool_receipt_proof_join.v1",
        "principal_id": pid,
        "namespace_key": ns_key.0,
        "channel_id": channel_id,
        "thread_id": thread_id,
        "tool_receipt_ids": receipt_ids,
        "tool_receipt_count": receipt_ids.len(),
        "turn_proof_event_id": turn_proof_event_id.0,
        "turn_proof_hash": turn_proof_hash,
        "answer_trace_event_id": answer_trace_event_id.0,
        "output_event_id": sent_event_id.0,
        "user_event_id": received_event_id.0,
        "lineage": [
            received_event_id.0.as_str(),
            sent_event_id.0.as_str(),
            answer_trace_event_id.0.as_str(),
            turn_proof_event_id.0.as_str()
        ],
    });
    let join_hash = hash_text(&join_without_hash.to_string());
    let mut payload = join_without_hash;
    if let serde_json::Value::Object(ref mut object) = payload {
        object.insert(
            "join_hash".to_string(),
            serde_json::Value::String(join_hash),
        );
    }
    ledger
        .append_signed_typed_event_with_parent(
            kp,
            ns_key,
            EventType::ToolReceiptProofJoin,
            payload,
            None,
            Some(turn_proof_event_id),
        )
        .map(Some)
        .map_err(CliError::Ledger)
}

fn sanitize_todo_state_json(state_json: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(state_json) else {
        return cap_todo_state_string(
            &SecretRedactor::redact(state_json),
            MAX_TODO_STATE_NOTES_CHARS,
        );
    };
    if let Some(todos) = value
        .get_mut("todos")
        .and_then(serde_json::Value::as_array_mut)
    {
        for todo in todos {
            sanitize_todo_state_field(todo, "title", MAX_TODO_STATE_TITLE_CHARS);
            sanitize_todo_state_field(todo, "content", MAX_TODO_STATE_TITLE_CHARS);
            sanitize_todo_state_field(todo, "notes", MAX_TODO_STATE_NOTES_CHARS);
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| {
        cap_todo_state_string(
            &SecretRedactor::redact(state_json),
            MAX_TODO_STATE_NOTES_CHARS,
        )
    })
}

fn sanitize_todo_state_field(todo: &mut serde_json::Value, key: &str, max_chars: usize) {
    let Some(text) = todo.get(key).and_then(serde_json::Value::as_str) else {
        return;
    };
    todo[key] = serde_json::Value::String(cap_todo_state_string(
        &SecretRedactor::redact(text),
        max_chars,
    ));
}

fn cap_todo_state_string(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut capped = text.chars().take(keep).collect::<String>();
    capped.push_str("...");
    capped
}

#[allow(clippy::too_many_arguments)]
fn append_latest_todo_state_event(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::keypair::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    channel_id: &str,
    thread_id: &str,
    sent_event_id: &zaion_types::event::EventId,
    execution_records: &[ToolExecutionRecord],
    todo_store: &TodoStore,
    snapshot_required: bool,
) -> Result<Option<zaion_types::event::EventId>, CliError> {
    let latest_executed_record = execution_records
        .iter()
        .rev()
        .find(|record| record.name == "todo" && record.receipt_status == "executed");
    let state_json = if let Some(record) = latest_executed_record {
        record.todo_state_json.clone()
    } else if snapshot_required && todo_store.has_items() {
        serde_json::to_string(&todo_store.response()).ok()
    } else {
        None
    };
    let Some(state_json) = state_json else {
        return Ok(None);
    };
    let state_json = sanitize_todo_state_json(&state_json);
    let state_value = serde_json::from_str::<serde_json::Value>(&state_json)
        .unwrap_or_else(|_| serde_json::Value::String(state_json.clone()));
    ledger
        .append_signed_event_with_parent(
            kp,
            ns_key,
            TODO_STATE_EVENT_TYPE,
            serde_json::json!({
                "schema": TODO_STATE_EVENT_TYPE,
                "principal_id": pid,
                "channel_id": channel_id,
                "thread_id": thread_id,
                "tool_call_id": latest_executed_record.map(|record| record.call_id.clone()),
                "source": if latest_executed_record.is_some() { "todo_tool_result" } else { "compression_session_snapshot" },
                "state_json": state_json,
                "state": state_value,
                "state_hash": hash_text(&state_json),
                "parent_output_event_id": sent_event_id.0,
            }),
            None,
            Some(sent_event_id),
        )
        .map(Some)
        .map_err(CliError::Ledger)
}

fn append_recorded_only_tool_receipt(
    ctx: &ToolReceiptContext<'_>,
    source: &str,
    call: &ToolCall,
) -> Result<zaion_types::event::EventId, CliError> {
    let arguments_json =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
    let policy_decision = PolicyDecision::recorded_not_executed(&call.name);
    let payload = serde_json::json!({
        "schema": "zaion.tool_receipt.v1",
        "principal_id": ctx.pid,
        "channel_id": ctx.channel_id,
        "thread_id": ctx.thread_id,
        "tool_name": call.name,
        "tool_call_id": call.id,
        "source": source,
        "arguments_hash": hash_text(&arguments_json),
        "output_hash": null,
        "permission_id": policy_decision.permission_id,
        "capability_class": policy_decision.capability_class,
        "policy_effect": policy_decision.effect,
        "permission_decision": policy_decision.reason_code,
        "sandbox_scope": policy_decision.sandbox_scope,
        "permission_proof": policy_decision.permission_proof(),
        "receipt_status": "recorded_not_executed",
        "parent_output_event_id": ctx.sent_event_id.0,
    });
    ctx.ledger
        .append_signed_typed_event_with_parent(
            ctx.kp,
            ctx.ns_key,
            EventType::ToolReceipt,
            payload,
            None,
            Some(ctx.sent_event_id),
        )
        .map_err(CliError::Ledger)
}

fn hash_text(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_provider_backed_compression_summary(
    compressor: &ContextCompressor,
    prompt: String,
    cfg: &ZaionConfig,
    provider_type: &str,
    model: Option<String>,
    log: &Logger,
) -> Option<String> {
    let (provider, actual_model) = match build_provider(provider_type, model, cfg) {
        Ok(provider) => provider,
        Err(error) => {
            log.warn(format!(
                "compression summary provider unavailable; using structured fallback: {}",
                error
            ));
            return None;
        }
    };
    let request = CompletionRequest {
        model: actual_model,
        messages: vec![ChatMessage::text("user", prompt)],
        max_tokens: Some(compressor.config.max_summary_tokens.min(u32::MAX as usize) as u32),
        temperature: Some(0.2),
        tools: None,
        tool_choice: None,
        enable_cache: false,
    };
    match provider.complete(&request) {
        Ok(response) if !response.content.trim().is_empty() => Some(response.content),
        Ok(_) => {
            log.warn(
                "compression summary provider returned empty content; using structured fallback",
            );
            None
        }
        Err(error) => {
            log.warn(format!(
                "compression summary provider failed; using structured fallback: {}",
                error
            ));
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_compression_evidence(
    compression_requested: bool,
    was_compressed: bool,
    original_turns: usize,
    compressed_turns: usize,
    turns_pruned: usize,
    original_tokens: usize,
    compressed_tokens: usize,
    token_budget: usize,
    trigger_threshold: usize,
    summary_text: &str,
    compressed_context: Option<&CompressedContext>,
) -> TurnCompressionEvidence {
    let mut evidence = TurnCompressionEvidence {
        schema: "zaion.context_compression_evidence.v1".to_string(),
        compression_requested,
        was_compressed,
        original_turns,
        compressed_turns,
        turns_pruned,
        original_tokens,
        compressed_tokens,
        token_budget,
        trigger_threshold,
        summary_hash: hash_text(summary_text),
        summary_strategy: compressed_context
            .map(|context| context.summary_strategy.clone())
            .unwrap_or_else(|| "none".to_string()),
        pruned_tool_outputs: compressed_context
            .map(|context| context.pruned_tool_outputs)
            .unwrap_or(0),
        protected_head_turns: compressed_context
            .map(|context| context.protected_head_turns)
            .unwrap_or(0),
        protected_tail_turns: compressed_context
            .map(|context| context.protected_tail_turns)
            .unwrap_or(0),
        protected_tail_tokens: compressed_context
            .map(|context| context.protected_tail_tokens)
            .unwrap_or(0),
        summary_budget_tokens: compressed_context
            .map(|context| context.summary_budget_tokens)
            .unwrap_or(0),
        evidence_hash: String::new(),
    };
    let evidence_json =
        serde_json::to_string(&evidence).unwrap_or_else(|_| "compression_evidence".to_string());
    evidence.evidence_hash = hash_text(&evidence_json);
    evidence
}

fn build_cost_evidence(
    provider: &str,
    model: &str,
    usage: CanonicalUsage,
    estimated_cost_usd: Option<f64>,
    session_estimated_cost_usd: f64,
    billing_provider: Option<&str>,
) -> TurnCostEvidence {
    let cost_status = match estimated_cost_usd {
        Some(0.0) => "included",
        Some(_) => "estimated",
        None => "unknown",
    }
    .to_string();
    let cost_source = if estimated_cost_usd.is_some() {
        "official_docs_snapshot"
    } else {
        "none"
    }
    .to_string();
    let billing_provider = billing_provider
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(provider)
        .to_string();
    let billing_mode = match cost_status.as_str() {
        "included" => "subscription_or_local_included",
        "estimated" => "official_docs_snapshot",
        _ => "unknown",
    }
    .to_string();
    let mut evidence = TurnCostEvidence {
        schema: "zaion.usage_cost_evidence.v1".to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        billing_provider,
        billing_mode,
        usage: TurnCanonicalUsageEvidence {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            request_count: 1,
        },
        cost_status,
        cost_source,
        estimated_cost_usd,
        actual_cost_usd: None,
        session_estimated_cost_usd,
        session_actual_cost_usd: None,
        pricing_version: Some("zaion-pricing-static-snapshot".to_string()),
        rollup_event_id: None,
        notes: Vec::new(),
        evidence_hash: String::new(),
    };
    evidence.evidence_hash = cost_evidence_hash(&evidence);
    evidence
}

fn cost_evidence_hash(evidence: &TurnCostEvidence) -> String {
    let mut stable = evidence.clone();
    stable.rollup_event_id = None;
    stable.evidence_hash.clear();
    let evidence_json =
        serde_json::to_string(&stable).unwrap_or_else(|_| "cost_evidence".to_string());
    hash_text(&evidence_json)
}

#[allow(clippy::too_many_arguments)]
fn persist_usage_cost_rollup(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::keypair::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    channel_id: &str,
    thread_id: &str,
    sent_event_id: &zaion_types::event::EventId,
    evidence: &TurnCostEvidence,
) -> Result<zaion_types::event::EventId, CliError> {
    ledger
        .append_signed_event_with_parent(
            kp,
            ns_key,
            "zaion.usage_cost.rollup.v1",
            serde_json::json!({
                "schema": "zaion.usage_cost.rollup.v1",
                "principal_id": pid,
                "channel_id": channel_id,
                "thread_id": thread_id,
                "provider": evidence.provider,
                "model": evidence.model,
                "billing_provider": evidence.billing_provider,
                "billing_mode": evidence.billing_mode,
                "usage": evidence.usage,
                "cost_status": evidence.cost_status,
                "cost_source": evidence.cost_source,
                "estimated_cost_usd": evidence.estimated_cost_usd,
                "actual_cost_usd": evidence.actual_cost_usd,
                "session_estimated_cost_usd": evidence.session_estimated_cost_usd,
                "session_actual_cost_usd": evidence.session_actual_cost_usd,
                "pricing_version": evidence.pricing_version,
                "cost_evidence_hash": evidence.evidence_hash,
                "parent_output_event_id": sent_event_id.0,
            }),
            None,
            Some(sent_event_id),
        )
        .map_err(CliError::Ledger)
}

#[allow(clippy::too_many_arguments)]
fn refresh_session_rollup(
    session_store: &zaion_ledger::SessionStore,
    existing: Option<&zaion_ledger::SessionEntry>,
    active_ns_key: &NamespaceKey,
    root_ns_key: &NamespaceKey,
    pid: &str,
    channel_id: &str,
    thread_id: &str,
    session_estimated_cost_usd: f64,
    message_count: i64,
    tool_call_count: i64,
) -> Result<(), CliError> {
    let now = chrono::Utc::now().to_rfc3339();
    let entry = zaion_ledger::SessionEntry {
        session_id: active_ns_key.0.clone(),
        principal_id: existing
            .map(|session| session.principal_id.clone())
            .unwrap_or_else(|| pid.to_string()),
        platform: existing
            .map(|session| session.platform.clone())
            .unwrap_or_else(|| channel_id.to_string()),
        chat_id: existing
            .map(|session| session.chat_id.clone())
            .unwrap_or_else(|| thread_id.to_string()),
        user_id: existing.and_then(|session| session.user_id.clone()),
        thread_id: existing
            .and_then(|session| session.thread_id.clone())
            .or_else(|| Some(thread_id.to_string())),
        session_key: existing
            .map(|session| session.session_key.clone())
            .unwrap_or_else(|| {
                if active_ns_key.0 == root_ns_key.0 {
                    format!("wake:{}:{}", channel_id, thread_id)
                } else {
                    format!("compression:{}:{}", root_ns_key.0, active_ns_key.0)
                }
            }),
        created_at: existing
            .map(|session| session.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        message_count,
        tool_call_count,
        estimated_cost_usd: session_estimated_cost_usd,
        memory_flushed: existing
            .map(|session| session.memory_flushed)
            .unwrap_or(false),
        was_auto_reset: existing
            .map(|session| session.was_auto_reset)
            .unwrap_or(false),
        auto_reset_reason: existing.and_then(|session| session.auto_reset_reason.clone()),
        parent_session_id: existing.and_then(|session| session.parent_session_id.clone()),
        end_reason: existing.and_then(|session| session.end_reason.clone()),
    };
    session_store
        .upsert_session(&entry)
        .map_err(|error| CliError::Runtime(format!("session store cost rollup failed: {}", error)))
}

fn request_source(channel_id: &str) -> &'static str {
    match channel_id {
        "telegram" => "telegram",
        "http-webhook" | "webhook" => "http",
        "mcp" => "mcp",
        "tui" => "tui",
        "terminal" | "cli" => "cli",
        _ => "adapter",
    }
}

fn wake_feature_defaults(req: &WakeRequest, cfg: &ZaionConfig) -> WakeFeatureDefaults {
    let unified_defaults = req.unified;
    WakeFeatureDefaults {
        memory_enabled: unified_defaults,
        mcp_enabled: unified_defaults,
        compression_enabled: cfg.agent.clamped().compression_enabled,
        webhooks_enabled: unified_defaults,
        cache_enabled: false,
        smart_route_enabled: false,
    }
}

fn wake_context_compressor(cfg: &ZaionConfig) -> (ContextCompressor, usize) {
    let settings = cfg.agent.clamped();
    let compressor = ContextCompressor::new(CompressorConfig {
        threshold_ratio: settings.compression_threshold,
        ..CompressorConfig::default()
    });
    (compressor, settings.token_budget)
}

// ─── argv <-> WakeRequest ───────────────────────────────────────────────────

const WAKE_USAGE: &str = "zaion wake <pid> <message> [--provider X] [--model X] [--stream] \
    [--mcp|--no-mcp] [--memory|--no-memory] [--cache] [--smart-route] \
    [--compress|--no-compress] [--no-webhooks] [--unified] \
    [--parser X] [--channel X] [--thread X] [--message-id X] [--source-hash X]";

fn print_wake_help() {
    println!("zaion wake - run one canonical process turn");
    println!();
    println!("USAGE:");
    println!("  {WAKE_USAGE}");
}

fn parse_argv(args: &[String]) -> Result<WakeRequest, CliError> {
    let pid = args.get(2).cloned().unwrap_or_default();
    let message = args
        .get(3)
        .cloned()
        .ok_or_else(|| CliError::Usage(WAKE_USAGE.to_string()))?;

    let mut req = WakeRequest::new(pid, message);
    req.provider = args
        .windows(2)
        .find(|w| w[0] == "--provider")
        .map(|w| w[1].clone());
    req.model = args
        .windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].clone());
    req.parser = args
        .windows(2)
        .find(|w| w[0] == "--parser")
        .map(|w| w[1].clone());
    req.channel_id = args
        .windows(2)
        .find(|w| w[0] == "--channel")
        .map(|w| w[1].clone());
    req.thread_id = args
        .windows(2)
        .find(|w| w[0] == "--thread")
        .map(|w| w[1].clone());
    req.source = args
        .windows(2)
        .find(|w| w[0] == "--source")
        .map(|w| w[1].clone());
    req.source_message_id = args
        .windows(2)
        .find(|w| w[0] == "--message-id")
        .map(|w| w[1].clone());
    req.source_hash = args
        .windows(2)
        .find(|w| w[0] == "--source-hash")
        .map(|w| w[1].clone());
    req.stream = args.iter().any(|a| a == "--stream" || a == "-s");
    req.enable_cache = args.iter().any(|a| a == "--cache");
    req.enable_memory = args.iter().any(|a| a == "--memory");
    req.enable_mcp = args.iter().any(|a| a == "--mcp");
    req.smart_route = args.iter().any(|a| a == "--smart-route");
    req.compress = args.iter().any(|a| a == "--compress");
    req.unified = args.iter().any(|a| a == "--unified");
    req.disable_memory = args.iter().any(|a| a == "--no-memory");
    req.disable_mcp = args.iter().any(|a| a == "--no-mcp");
    req.disable_compression = args.iter().any(|a| a == "--no-compress");
    req.disable_webhooks = args.iter().any(|a| a == "--no-webhooks");
    req.turn_contract_v2 = args.iter().any(|a| a == "--turn-contract-v2");
    Ok(req)
}

fn request_to_argv(req: &WakeRequest, pid: &str) -> Vec<String> {
    let mut argv = vec![
        "zaion".into(),
        "wake".into(),
        pid.to_string(),
        req.message.clone(),
    ];
    if let Some(ref p) = req.provider {
        argv.push("--provider".into());
        argv.push(p.clone());
    }
    if let Some(ref m) = req.model {
        argv.push("--model".into());
        argv.push(m.clone());
    }
    if req.stream {
        argv.push("--stream".into());
    }
    if req.enable_cache {
        argv.push("--cache".into());
    }
    if req.enable_memory && !req.disable_memory {
        argv.push("--memory".into());
    }
    if req.enable_mcp && !req.disable_mcp {
        argv.push("--mcp".into());
    }
    if req.smart_route {
        argv.push("--smart-route".into());
    }
    if req.compress && !req.disable_compression {
        argv.push("--compress".into());
    }
    if req.unified {
        argv.push("--unified".into());
    }
    if req.disable_memory {
        argv.push("--no-memory".into());
    }
    if req.disable_mcp {
        argv.push("--no-mcp".into());
    }
    if req.disable_compression {
        argv.push("--no-compress".into());
    }
    if req.disable_webhooks {
        argv.push("--no-webhooks".into());
    }
    if req.turn_contract_v2 {
        argv.push("--turn-contract-v2".into());
    }
    if let Some(ref channel_id) = req.channel_id {
        argv.push("--channel".into());
        argv.push(channel_id.clone());
    }
    if let Some(ref thread_id) = req.thread_id {
        argv.push("--thread".into());
        argv.push(thread_id.clone());
    }
    if let Some(ref source) = req.source {
        argv.push("--source".into());
        argv.push(source.clone());
    }
    if let Some(ref source_message_id) = req.source_message_id {
        argv.push("--message-id".into());
        argv.push(source_message_id.clone());
    }
    if let Some(ref source_hash) = req.source_hash {
        argv.push("--source-hash".into());
        argv.push(source_hash.clone());
    }
    argv
}

// ─── Logger: route to callback or stderr, never stdout ───────────────────────

#[derive(Clone)]
struct Logger {
    callback: Option<StreamCallback>,
}

impl Logger {
    fn new(callback: Option<StreamCallback>) -> Self {
        Self { callback }
    }

    fn status(&self, msg: impl Into<String>) {
        let m = msg.into();
        match &self.callback {
            Some(cb) => cb.send_status(m),
            None => eprintln!("[status] {}", m),
        }
    }

    fn warn(&self, msg: impl Into<String>) {
        let m = msg.into();
        match &self.callback {
            Some(cb) => cb.send_warning(m),
            None => eprintln!("⚠ {}", m),
        }
    }

    fn notice(&self, msg: impl Into<String>) {
        let m = msg.into();
        match &self.callback {
            Some(cb) => cb.send_notice(m),
            None => println!("{}", m),
        }
    }

    fn error(&self, msg: impl Into<String>) {
        let m = msg.into();
        match &self.callback {
            Some(cb) => cb.send_error(m),
            None => eprintln!("✗ {}", m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use zaion_runtime::operation_stream::OperationEventKind;
    use zaion_runtime::{ToolResultStorageTarget, TurnOutcome};

    struct EnvRestoreGuard {
        home: Option<std::ffi::OsString>,
        data: Option<std::ffi::OsString>,
        profile: Option<std::ffi::OsString>,
    }

    impl EnvRestoreGuard {
        fn capture() -> Self {
            Self {
                home: std::env::var_os("ZAION_HOME"),
                data: std::env::var_os("ZAION_DATA_DIR"),
                profile: std::env::var_os("ZAION_ACTIVE_PROFILE"),
            }
        }
    }

    impl Drop for EnvRestoreGuard {
        fn drop(&mut self) {
            match self.home.take() {
                Some(value) => std::env::set_var("ZAION_HOME", value),
                None => std::env::remove_var("ZAION_HOME"),
            }
            match self.data.take() {
                Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
                None => std::env::remove_var("ZAION_DATA_DIR"),
            }
            match self.profile.take() {
                Some(value) => std::env::set_var("ZAION_ACTIVE_PROFILE", value),
                None => std::env::remove_var("ZAION_ACTIVE_PROFILE"),
            }
        }
    }

    struct TempRootGuard(std::path::PathBuf);

    impl Drop for TempRootGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct TempFileGuard(std::path::PathBuf);

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir(parent);
                if let Some(grandparent) = parent.parent() {
                    let _ = std::fs::remove_dir(grandparent);
                }
            }
        }
    }

    #[test]
    fn conflicting_cli_feature_flags_resolve_with_disable_precedence() {
        let args = [
            "zaion",
            "wake",
            "did:key:test",
            "hello",
            "--memory",
            "--no-memory",
            "--mcp",
            "--no-mcp",
            "--compress",
            "--no-compress",
            "--no-webhooks",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let request = parse_argv(&args).expect("wake request");
        let policy = request.effective_features(WakeFeatureDefaults {
            memory_enabled: true,
            mcp_enabled: true,
            compression_enabled: true,
            webhooks_enabled: true,
            cache_enabled: false,
            smart_route_enabled: false,
        });

        assert_eq!(policy, WakeFeaturePolicy::default());

        let canonical_argv = request_to_argv(&request, "did:key:test");
        for positive in ["--memory", "--mcp", "--compress"] {
            assert!(!canonical_argv.iter().any(|arg| arg == positive));
        }
        for negative in ["--no-memory", "--no-mcp", "--no-compress", "--no-webhooks"] {
            assert!(canonical_argv.iter().any(|arg| arg == negative));
        }
    }

    #[test]
    fn wake_usage_lists_supported_positive_and_negative_feature_flags() {
        for flag in [
            "--memory",
            "--no-memory",
            "--mcp",
            "--no-mcp",
            "--cache",
            "--smart-route",
            "--compress",
            "--no-compress",
            "--no-webhooks",
        ] {
            assert!(WAKE_USAGE.contains(flag), "missing {flag}: {WAKE_USAGE}");
        }
    }

    #[test]
    fn wake_compressor_consumes_clamped_agent_threshold_and_token_budget() {
        let mut cfg = ZaionConfig::default();
        cfg.agent.compression_threshold = 0.75;
        cfg.agent.token_budget = 123_456;

        let (compressor, token_budget) = wake_context_compressor(&cfg);

        assert_eq!(compressor.config.threshold_ratio, 0.75);
        assert_eq!(token_budget, 123_456);
    }

    #[test]
    fn internal_scheduled_task_request_preserves_canonical_source_and_metadata() {
        let keypair = zaion_crypto::ZaionKeypair::generate();
        let task = zaion_runtime::ScheduledTask::new(
            "session-1".to_string(),
            "queued work".to_string(),
            TaskMode::Queue,
        );
        let mut req = WakeRequest::new(keypair.principal_id().as_str(), "/queue queued work");
        req.provider = Some("openai".to_string());
        req.model = Some("gpt-test".to_string());
        req.channel_id = Some("terminal".to_string());
        req.thread_id = Some("main".to_string());

        let next = build_internal_task_wake_request(
            &req,
            &keypair,
            "terminal",
            "main",
            &task,
            "internal-queue",
            "slash_command_queue",
        )
        .unwrap();
        let envelope = next.envelope.as_ref().unwrap();

        assert_eq!(next.message, "queued work");
        assert_eq!(next.provider.as_deref(), Some("openai"));
        assert_eq!(next.model.as_deref(), Some("gpt-test"));
        assert_eq!(envelope.source, "internal-queue");
        assert_eq!(
            envelope
                .metadata
                .get("queued_by")
                .and_then(|value| value.as_str()),
            Some("slash_command_queue")
        );
        assert_eq!(
            envelope
                .metadata
                .get("scheduled_task_id")
                .and_then(|value| value.as_str()),
            Some(task.task_id.as_str())
        );
        assert_eq!(
            next.source_message_id.as_deref(),
            Some(envelope.message_id.as_str())
        );
        assert_eq!(
            next.source_hash.as_deref(),
            Some(envelope.source_hash.as_str())
        );

        let argv = request_to_argv(&next, keypair.principal_id().as_str());
        assert!(argv
            .windows(2)
            .any(|window| window == ["--source", "internal-queue"]));
        assert!(argv
            .windows(2)
            .any(|window| window[0] == "--source-hash" && window[1] == envelope.source_hash));
    }

    #[test]
    fn queue_slash_handoff_completes_current_stream_before_dispatching_next_task() {
        let _guard = crate::config::env_test_lock();
        let root = std::env::temp_dir().join(format!(
            "zaion-queue-slash-handoff-{}",
            uuid::Uuid::new_v4()
        ));
        let _root_cleanup = TempRootGuard(root.clone());
        let home = root.join("home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let _env_restore = EnvRestoreGuard::capture();
        std::env::set_var("ZAION_HOME", &home);
        std::env::set_var("ZAION_DATA_DIR", &data);

        crate::config::ZaionConfig {
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ..Default::default()
        }
        .save()
        .unwrap();

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(kp.principal_id().as_str().to_string()),
            ChannelId("terminal".to_string()),
            ThreadId("main".to_string()),
            "msg-queue".to_string(),
            "/queue /stop".to_string(),
            None,
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let callback = StreamCallback::new(tx);
        let req = WakeRequest::new(process.principal_id, "/queue /stop")
            .with_envelope(envelope)
            .with_turn_contract_v2(true);

        cmd_wake_with_request(req, Some(callback)).unwrap();

        let events: Vec<_> = rx.try_iter().collect();
        let completed_operation_count = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    zaion_runtime::StreamEvent::Operation(op)
                        if op.kind == OperationEventKind::TurnCompleted
                )
            })
            .count();
        let complete_event_count = events
            .iter()
            .filter(|event| matches!(event, zaion_runtime::StreamEvent::Complete { .. }))
            .count();

        assert_eq!(
            completed_operation_count, 2,
            "slash /queue handoff should close both the scheduling turn and the dispatched slash turn"
        );
        assert_eq!(
            complete_event_count, 2,
            "legacy stream consumers should receive a completion event for both slash turns"
        );
    }

    #[test]
    fn local_cli_wake_executes_through_authenticated_turn_contract_v2() {
        let _guard = crate::config::env_test_lock();
        let root = std::env::temp_dir().join(format!(
            "zaion-local-cli-turn-contract-v2-{}",
            uuid::Uuid::new_v4()
        ));
        let _root_cleanup = TempRootGuard(root.clone());
        let home = root.join("home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let _env_restore = EnvRestoreGuard::capture();
        std::env::set_var("ZAION_HOME", &home);
        std::env::set_var("ZAION_DATA_DIR", &data);
        std::env::set_var("ZAION_ACTIVE_PROFILE", "default");
        crate::config::ZaionConfig {
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ..Default::default()
        }
        .save()
        .unwrap();

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let mut envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(kp.principal_id().as_str().to_string()),
            ChannelId("terminal".to_string()),
            ThreadId("main".to_string()),
            "msg-v2".to_string(),
            "/stop".to_string(),
            None,
        )
        .unwrap();
        envelope.received_at = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        let (tx, rx) = mpsc::channel();
        let execution = execute_wake_with_request(
            WakeRequest::new(process.principal_id.clone(), "/stop")
                .with_envelope(envelope.clone())
                .with_turn_contract_v2(true),
            Some(StreamCallback::new(tx)),
        )
        .unwrap();

        assert!(matches!(execution, TurnExecution::Handled(_)));
        let events = rx.try_iter().collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            zaion_runtime::StreamEvent::Operation(operation)
                if operation.payload["schema"]
                    == "zaion.turn_contract_v2.transition.v1"
                    && operation.payload["state"] == "running"
                    && operation.payload["revision"] == 2
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            zaion_runtime::StreamEvent::Operation(operation)
                if operation.kind == OperationEventKind::TurnCompleted
        )));

        let mut retry =
            WakeRequest::new(process.principal_id.clone(), "/stop").with_turn_contract_v2(true);
        retry.source = Some("cli".to_string());
        retry.channel_id = Some("terminal".to_string());
        retry.thread_id = Some("main".to_string());
        retry.source_message_id = Some("msg-v2".to_string());
        let replay = execute_wake_with_request(retry, None).unwrap();
        assert_eq!(replay, execution);
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let received = ledger
            .list_events(
                &zaion_types::session::SessionKey(process.principal_id),
                Some(EventType::ChannelReceived.as_str()),
                10,
            )
            .unwrap();
        assert_eq!(
            received.len(),
            1,
            "duplicate durable ingress must not append a second channel.received event"
        );
    }

    #[test]
    fn durable_turn_explicitly_quarantines_pipeline_errors_after_running() {
        let _guard = crate::config::env_test_lock();
        let root = std::env::temp_dir().join(format!(
            "zaion-durable-turn-error-v2-{}",
            uuid::Uuid::new_v4()
        ));
        let _root_cleanup = TempRootGuard(root.clone());
        let home = root.join("home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let _env_restore = EnvRestoreGuard::capture();
        std::env::set_var("ZAION_HOME", &home);
        std::env::set_var("ZAION_DATA_DIR", &data);
        std::env::set_var("ZAION_ACTIVE_PROFILE", "default");
        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(kp.principal_id().as_str().to_string()),
            ChannelId("terminal".to_string()),
            ThreadId("main".to_string()),
            "msg-v2-error".to_string(),
            "inspect this workspace".to_string(),
            None,
        )
        .unwrap();
        let mut request = WakeRequest::new(process.principal_id.clone(), envelope.body.clone())
            .with_envelope(envelope.clone())
            .with_turn_contract_v2(true);
        request.provider = Some("definitely-not-a-provider".to_string());

        let error = execute_wake_with_request(request, None).unwrap_err();
        assert!(error.to_string().contains("provider"));

        let ingress =
            local_cli_ingress(&process, &envelope, active_profile_id(), chrono::Utc::now())
                .unwrap();
        let duplicate = TurnContractV2::begin_durable(
            ingress,
            &envelope,
            store.ledger_path(&process.principal_id),
            chrono::Utc::now(),
        )
        .unwrap();
        let TurnContractAdmission::Duplicate(record) = duplicate else {
            panic!("failed wake must remain durably deduplicated");
        };
        assert_eq!(record.state.state(), TurnState::Quarantined);
        let terminal = duplicate_execution(&record).unwrap();
        assert!(matches!(
            terminal,
            TurnExecution::Finished { outcome, .. }
                if matches!(outcome.as_ref(), TurnOutcome::Quarantined(_))
        ));
    }

    #[test]
    fn legacy_complete_is_emitted_after_runtime_turn_completed() {
        let (tx, rx) = mpsc::channel();
        let callback = StreamCallback::new(tx);
        let operation_recorder = WakeOperationRecorder::new(
            OperationContext {
                stream_id: "stream-order".to_string(),
                turn_id: "turn-order".to_string(),
                principal_id: "did:key:order".to_string(),
                channel_id: "terminal".to_string(),
                thread_id: "main".to_string(),
            },
            Some(callback.clone()),
            8,
        );

        let _execution = operation_recorder.finish_handled_turn("test.turn", 3, 5, None);

        let events = rx.try_iter().collect::<Vec<_>>();
        assert!(matches!(
            events.first(),
            Some(zaion_runtime::StreamEvent::Operation(operation))
                if operation.kind == OperationEventKind::TurnCompleted
        ));
        assert!(matches!(
            events.get(1),
            Some(zaion_runtime::StreamEvent::Complete {
                input_tokens: 3,
                output_tokens: 5,
            })
        ));
    }

    #[test]
    fn provider_success_without_visible_answer_or_tool_call_is_rejected() {
        let response = CompletionResponse {
            content: " \n\t ".to_string(),
            model: "gpt-5.5".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,

            reasoning_content: String::new(),
    reasoning_signature: String::new(),
        };

        let err = ensure_visible_provider_response(&response).unwrap_err();

        assert!(
            err.to_string()
                .contains("provider returned no visible assistant content"),
            "wake must fail closed before appending channel.sent/turn.proof for empty provider success: {err}"
        );
    }

    #[test]
    fn streaming_callback_forwards_final_text_when_provider_did_not_emit_token_deltas() {
        let req = WakeRequest::new("did:key:test".to_string(), "say hello".to_string()).streaming();
        let response = CompletionResponse {
            content: "zaion alive".to_string(),
            model: "gpt-5.5".to_string(),
            input_tokens: 10,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,

            reasoning_content: String::new(),
    reasoning_signature: String::new(),
        };

        assert!(
            should_forward_final_response_to_callback(&req, false, false, &response),
            "stream consumers need a final-text fallback when the provider returns content without streaming token deltas"
        );
    }

    #[test]
    fn streaming_callback_does_not_duplicate_normal_token_delta_responses() {
        let req = WakeRequest::new("did:key:test".to_string(), "say hello".to_string()).streaming();
        let response = CompletionResponse {
            content: "zaion alive".to_string(),
            model: "gpt-5.5".to_string(),
            input_tokens: 10,
            output_tokens: 11,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,

            reasoning_content: String::new(),
    reasoning_signature: String::new(),
        };

        assert!(
            !should_forward_final_response_to_callback(&req, false, true, &response),
            "normal streaming providers already delivered token deltas and must not duplicate the final response"
        );
    }

    #[test]
    fn wake_tool_definitions_include_session_todo_tool() {
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        zaion_mcp::register_builtin_tools(&mut builtin);
        let mut defs = collect_builtin_tool_defs(&builtin);
        defs.push(todo_tool_definition());

        let todo = defs
            .iter()
            .find(|definition| definition.name == "todo")
            .expect("todo tool definition");

        assert_eq!(todo.parameters["required"], serde_json::json!(["action"]));
        assert!(todo.parameters["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("replace")));
    }

    #[test]
    fn wake_todo_tool_call_updates_session_store_and_returns_full_state() {
        let builtin = zaion_mcp::McpToolRegistry::new();
        let mut todos = TodoStore::new();
        let call = ToolCall {
            id: "todo_call".to_string(),
            name: "todo".to_string(),
            arguments: serde_json::json!({
                "action": "add",
                "id": "hermes-gap",
                "title": "Wire session todo into wake",
                "priority": "high"
            }),
        };
        let log = Logger::new(None);
        let budget_config = default_tool_result_budget_config();
        let storage_target =
            zaion_runtime::HostToolResultStorageTarget::new(budget_config.storage_dir.clone());

        let record = execute_native_tool_call(
            &builtin,
            None,
            &mut todos,
            &call,
            &log,
            None,
            &budget_config,
            &storage_target,
        );

        assert_eq!(record.receipt_status, "executed");
        assert_eq!(todos.summary().total, 1);
        assert!(record.todo_state_json.is_some());
        let response: serde_json::Value = serde_json::from_str(&record.context_output).unwrap();
        assert_eq!(response["summary"]["total"], 1);
        assert_eq!(response["todos"][0]["id"], "hermes-gap");
        assert_eq!(response["todos"][0]["priority"], "high");
    }

    #[test]
    fn wake_todo_list_filtered_response_does_not_truncate_durable_state() {
        let builtin = zaion_mcp::McpToolRegistry::new();
        let mut todos = TodoStore::new();
        todos
            .handle_json(&serde_json::json!({
                "action": "add",
                "id": "active",
                "title": "active item"
            }))
            .unwrap();
        todos
            .handle_json(&serde_json::json!({
                "action": "add",
                "id": "done",
                "title": "completed item"
            }))
            .unwrap();
        todos
            .handle_json(&serde_json::json!({
                "action": "complete",
                "id": "done"
            }))
            .unwrap();
        let call = ToolCall {
            id: "todo_list".to_string(),
            name: "todo".to_string(),
            arguments: serde_json::json!({
                "action": "list",
                "active_only": true
            }),
        };
        let log = Logger::new(None);
        let budget_config = default_tool_result_budget_config();
        let storage_target =
            zaion_runtime::HostToolResultStorageTarget::new(budget_config.storage_dir.clone());

        let record = execute_native_tool_call(
            &builtin,
            None,
            &mut todos,
            &call,
            &log,
            None,
            &budget_config,
            &storage_target,
        );

        let visible_response: serde_json::Value =
            serde_json::from_str(&record.context_output).unwrap();
        assert_eq!(visible_response["todos"].as_array().unwrap().len(), 1);
        assert_eq!(visible_response["todos"][0]["id"], "active");
        let durable_state: serde_json::Value =
            serde_json::from_str(record.todo_state_json.as_ref().unwrap()).unwrap();
        assert_eq!(durable_state["todos"].as_array().unwrap().len(), 2);
        assert!(durable_state["todos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|todo| todo["id"] == "done" && todo["status"] == "completed"));
    }

    #[test]
    fn wake_todo_state_event_persists_and_hydrates_latest_matching_thread() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-todo-state-ledger-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = zaion_ledger::EventLedger::new(dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.todo.test".to_string());
        let sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "thread-a",
                    "content": "todo updated",
                }),
                None,
            )
            .unwrap();
        let other_sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "thread-b",
                    "content": "todo updated elsewhere",
                }),
                None,
            )
            .unwrap();
        let matching_state = serde_json::json!({
            "todos": [
                {"id": "durable", "content": "restore me", "status": "in_progress"}
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let other_state = serde_json::json!({
            "todos": [
                {"id": "other", "content": "wrong thread", "status": "pending"}
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let records = vec![ToolExecutionRecord {
            call_id: "todo_matching".to_string(),
            name: "todo".to_string(),
            arguments_hash: "args".to_string(),
            output_hash: Some(hash_text(&matching_state)),
            policy_decision: PolicyDecision::allow_builtin("todo", CapabilityClass::External),
            permission_decision: "allowed_builtin".to_string(),
            sandbox_scope: "builtin".to_string(),
            receipt_status: "executed".to_string(),
            context_output: matching_state.clone(),
            tool_result_metadata: None,
            error: None,
            todo_state_json: Some(matching_state),
        }];
        append_latest_todo_state_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "thread-a",
            &sent_event_id,
            &records,
            &TodoStore::new(),
            false,
        )
        .unwrap();
        let other_records = vec![ToolExecutionRecord {
            call_id: "todo_other".to_string(),
            name: "todo".to_string(),
            arguments_hash: "args".to_string(),
            output_hash: Some(hash_text(&other_state)),
            policy_decision: PolicyDecision::allow_builtin("todo", CapabilityClass::External),
            permission_decision: "allowed_builtin".to_string(),
            sandbox_scope: "builtin".to_string(),
            receipt_status: "executed".to_string(),
            context_output: other_state.clone(),
            tool_result_metadata: None,
            error: None,
            todo_state_json: Some(other_state),
        }];
        append_latest_todo_state_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "thread-b",
            &other_sent_event_id,
            &other_records,
            &TodoStore::new(),
            false,
        )
        .unwrap();

        let mut hydrated = TodoStore::new();
        assert_eq!(
            hydrate_todo_store_from_latest_state_event(&ledger, &ns_key, "thread-a", &mut hydrated),
            Some(true)
        );
        assert_eq!(hydrated.summary().total, 1);
        assert_eq!(hydrated.list()[0].id, "durable");
        assert_eq!(hydrated.list()[0].status.as_str(), "in_progress");
    }

    #[test]
    fn wake_todo_state_hydration_is_not_shadowed_by_newer_other_threads() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-todo-state-thread-index-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = zaion_ledger::EventLedger::new(dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.todo.thread.index.test".to_string());
        let target_sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "thread-target",
                    "content": "target todo state",
                }),
                None,
            )
            .unwrap();
        let target_state = serde_json::json!({
            "todos": [
                {"id": "target", "content": "do not lose me", "status": "pending"}
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let target_records = vec![ToolExecutionRecord {
            call_id: "todo_target".to_string(),
            name: "todo".to_string(),
            arguments_hash: "args".to_string(),
            output_hash: Some(hash_text(&target_state)),
            policy_decision: PolicyDecision::allow_builtin("todo", CapabilityClass::External),
            permission_decision: "allowed_builtin".to_string(),
            sandbox_scope: "builtin".to_string(),
            receipt_status: "executed".to_string(),
            context_output: target_state.clone(),
            tool_result_metadata: None,
            error: None,
            todo_state_json: Some(target_state),
        }];
        append_latest_todo_state_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "thread-target",
            &target_sent_event_id,
            &target_records,
            &TodoStore::new(),
            false,
        )
        .unwrap();

        for index in 0..600 {
            let thread_id = format!("thread-other-{index}");
            let sent_event_id = ledger
                .append_signed_typed_event(
                    &kp,
                    &ns_key,
                    EventType::ChannelSent,
                    serde_json::json!({
                        "principal_id": kp.principal_id().as_str(),
                        "channel_id": "terminal",
                        "thread_id": thread_id,
                        "content": "other todo state",
                    }),
                    None,
                )
                .unwrap();
            let other_state = serde_json::json!({
                "todos": [
                    {"id": format!("other-{index}"), "content": "wrong thread", "status": "pending"}
                ],
                "summary": {"total": 1}
            })
            .to_string();
            let other_records = vec![ToolExecutionRecord {
                call_id: format!("todo_other_{index}"),
                name: "todo".to_string(),
                arguments_hash: "args".to_string(),
                output_hash: Some(hash_text(&other_state)),
                policy_decision: PolicyDecision::allow_builtin("todo", CapabilityClass::External),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: other_state.clone(),
                tool_result_metadata: None,
                error: None,
                todo_state_json: Some(other_state),
            }];
            append_latest_todo_state_event(
                &ledger,
                &kp,
                &ns_key,
                kp.principal_id().as_str(),
                "terminal",
                &thread_id,
                &sent_event_id,
                &other_records,
                &TodoStore::new(),
                false,
            )
            .unwrap();
        }

        let mut hydrated = TodoStore::new();
        assert_eq!(
            hydrate_todo_store_from_latest_state_event(
                &ledger,
                &ns_key,
                "thread-target",
                &mut hydrated
            ),
            Some(true)
        );
        assert_eq!(hydrated.summary().total, 1);
        assert_eq!(hydrated.list()[0].id, "target");
    }

    #[test]
    fn wake_todo_state_event_redacts_and_caps_durable_strings_before_ledger_write() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-todo-state-sanitize-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = zaion_ledger::EventLedger::new(dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.todo.sanitize.test".to_string());
        let sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "thread-a",
                    "content": "todo updated",
                }),
                None,
            )
            .unwrap();
        let long_title = format!(
            "ship secret sk-proj-{} {}",
            "a".repeat(64),
            "title-overflow".repeat(80)
        );
        let long_notes = format!(
            "db postgres://admin:s3cr3t@localhost:5432/app {}",
            "notes-overflow".repeat(220)
        );
        let state = serde_json::json!({
            "todos": [
                {
                    "id": "sanitize",
                    "title": long_title,
                    "status": "pending",
                    "priority": "urgent",
                    "notes": long_notes
                }
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let records = vec![ToolExecutionRecord {
            call_id: "todo_sanitize".to_string(),
            name: "todo".to_string(),
            arguments_hash: "args".to_string(),
            output_hash: Some(hash_text(&state)),
            policy_decision: PolicyDecision::allow_builtin("todo", CapabilityClass::External),
            permission_decision: "allowed_builtin".to_string(),
            sandbox_scope: "builtin".to_string(),
            receipt_status: "executed".to_string(),
            context_output: state.clone(),
            tool_result_metadata: None,
            error: None,
            todo_state_json: Some(state),
        }];

        append_latest_todo_state_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "thread-a",
            &sent_event_id,
            &records,
            &TodoStore::new(),
            false,
        )
        .unwrap();

        let events = ledger
            .list_events(
                &zaion_types::session::SessionKey(ns_key.0.clone()),
                Some(TODO_STATE_EVENT_TYPE),
                1,
            )
            .unwrap();
        let payload = &events[0].payload;
        let state_json = payload["state_json"].as_str().unwrap();
        assert!(!state_json.contains("sk-proj-"), "{state_json}");
        assert!(!state_json.contains("s3cr3t"), "{state_json}");
        assert_eq!(
            payload["state_hash"],
            serde_json::json!(hash_text(state_json))
        );

        let stored_title = payload["state"]["todos"][0]["title"].as_str().unwrap();
        let stored_notes = payload["state"]["todos"][0]["notes"].as_str().unwrap();
        assert!(stored_title.contains("[REDACTED]"), "{stored_title}");
        assert!(stored_notes.contains("***@"), "{stored_notes}");
        assert!(
            stored_title.chars().count() <= 512,
            "title chars: {}",
            stored_title.chars().count()
        );
        assert!(
            stored_notes.chars().count() <= 2048,
            "notes chars: {}",
            stored_notes.chars().count()
        );

        let mut hydrated = TodoStore::new();
        assert_eq!(
            hydrate_todo_store_from_latest_state_event(&ledger, &ns_key, "thread-a", &mut hydrated),
            Some(true)
        );
        let hydrated_item = hydrated.list().into_iter().next().unwrap();
        assert!(!hydrated_item.title.contains("sk-proj-"));
        assert!(!hydrated_item.notes.unwrap().contains("s3cr3t"));
    }

    #[test]
    fn wake_todo_state_snapshot_preserves_store_when_compression_splits_session() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-todo-state-snapshot-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = zaion_ledger::EventLedger::new(dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("child.session.todo.test".to_string());
        let sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "thread-a",
                    "content": "compressed into child",
                }),
                None,
            )
            .unwrap();
        let mut todos = TodoStore::new();
        todos
            .handle_json(&serde_json::json!({
                "action": "add",
                "id": "carry-forward",
                "title": "keep active todo through split"
            }))
            .unwrap();

        let event_id = append_latest_todo_state_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "thread-a",
            &sent_event_id,
            &[],
            &todos,
            true,
        )
        .unwrap()
        .expect("snapshot todo state event");
        let events = ledger
            .list_events(
                &zaion_types::session::SessionKey(ns_key.0.clone()),
                Some(TODO_STATE_EVENT_TYPE),
                1,
            )
            .unwrap();
        assert_eq!(events[0].event_id.0, event_id.0);
        assert_eq!(
            events[0].payload["source"],
            serde_json::json!("compression_session_snapshot")
        );

        let mut hydrated = TodoStore::new();
        assert_eq!(
            hydrate_todo_store_from_latest_state_event(&ledger, &ns_key, "thread-a", &mut hydrated),
            Some(true)
        );
        assert_eq!(hydrated.list()[0].id, "carry-forward");
    }

    #[test]
    fn wake_todo_store_hydrates_from_latest_tool_history() {
        let mut todos = TodoStore::new();
        let old_response = serde_json::json!({
            "todos": [
                {"id": "old", "content": "old item", "status": "pending"}
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let latest_response = serde_json::json!({
            "todos": [
                {"id": "latest", "content": "latest item", "status": "in_progress"}
            ],
            "summary": {"total": 1}
        })
        .to_string();
        let history = vec![
            ChatMessage::tool_result("todo_old", old_response),
            ChatMessage::text("assistant", "continuing"),
            ChatMessage::tool_result("todo_latest", latest_response),
        ];

        assert!(hydrate_todo_store_from_history(&mut todos, &history));
        assert_eq!(todos.summary().total, 1);
        assert_eq!(todos.list()[0].id, "latest");
    }

    #[test]
    fn wake_tool_context_output_spills_large_results_before_model_reentry() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-result-budget-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 80,
            turn_budget_bytes: 10_000,
            preview_bytes: 32,
            storage_dir: dir.clone(),
        };
        let raw = serde_json::json!({
            "result": "tool-output-line\n".repeat(40)
        })
        .to_string();

        let context_output =
            budget_tool_context_output_with_config("fs_read", "call_large", &raw, &config).unwrap();

        assert!(context_output.contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(context_output.contains("Full output saved to:"));
        assert!(context_output.len() < raw.len());
        assert_eq!(
            std::fs::read_to_string(dir.join("call_large.txt")).unwrap(),
            raw
        );
    }

    #[test]
    fn wake_live_tool_result_budget_defaults_to_workspace_visible_dir() {
        let cwd = std::env::current_dir().unwrap();
        let config = default_tool_result_budget_config();

        assert_eq!(config.storage_dir, cwd.join(".zaion").join("tool-results"));
        assert_ne!(config.storage_dir, data_dir().join("tool-results"));
    }

    #[test]
    fn wake_live_tool_result_budget_spills_under_workspace_visible_dir() {
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 80,
            turn_budget_bytes: 10_000,
            preview_bytes: 32,
            ..default_tool_result_budget_config()
        };
        let raw = "workspace-visible wake output\n".repeat(40);
        let call_id = format!("workspace_live_{}", uuid::Uuid::new_v4());

        let context_output =
            budget_tool_context_output_with_config("shell_exec", &call_id, &raw, &config).unwrap();

        let stored_path = config.storage_dir.join(format!("{}.txt", call_id));
        let _cleanup_file = TempFileGuard(stored_path.clone());

        assert!(context_output.contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(context_output.contains(".zaion"));
        assert!(context_output.contains("tool-results"));
        assert_eq!(std::fs::read_to_string(&stored_path).unwrap(), raw);
        assert!(!data_dir()
            .join("tool-results")
            .join(format!("{}.txt", call_id))
            .exists());
    }

    #[test]
    fn wake_request_tool_result_storage_root_overrides_default_budget_root() {
        let root = std::env::temp_dir().join(format!(
            "zaion-wake-request-tool-results-{}",
            uuid::Uuid::new_v4()
        ));
        let req = WakeRequest::new("pid", "message").with_tool_result_storage_root(root.clone());

        let config = wake_tool_result_budget_config(&req);

        assert_eq!(config.storage_dir, root);
    }

    #[test]
    fn wake_request_tool_result_environment_identity_reaches_host_storage_target() {
        let root = std::env::temp_dir().join(format!(
            "zaion-wake-request-tool-results-env-{}",
            uuid::Uuid::new_v4()
        ));
        let mut req =
            WakeRequest::new("pid", "message").with_tool_result_storage_root(root.clone());
        req.tool_result_environment_id = Some("daytona:workspace:zaion-main:sandbox-9".to_string());
        req.tool_result_environment_kind = Some("daytona".to_string());

        let config = wake_tool_result_budget_config(&req);
        let target = wake_tool_result_storage_target(&req, &config);

        assert_eq!(target.storage_root(), root.as_path());
        assert_eq!(
            target.environment_id(),
            Some("daytona:workspace:zaion-main:sandbox-9")
        );
        assert_eq!(target.environment_kind(), Some("daytona"));
    }

    #[test]
    fn structured_wake_request_workspace_tool_result_root_matches_live_default() {
        let cwd = std::env::current_dir().unwrap();
        let req = WakeRequest::new("pid", "message")
            .with_tool_result_storage_root(workspace_tool_result_storage_root());

        let config = wake_tool_result_budget_config(&req);

        assert_eq!(config.storage_dir, cwd.join(".zaion").join("tool-results"));
    }

    #[test]
    fn structured_wake_request_from_envelope_defaults_to_workspace_tool_result_root() {
        let envelope = CanonicalEnvelope::new(
            "api",
            PrincipalId("did:key:test".to_string()),
            ChannelId("api".to_string()),
            ThreadId("thread-a".to_string()),
            "message-a".to_string(),
            "hello".to_string(),
            None,
        )
        .unwrap();
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = structured_wake_request("did:key:test", "hello", envelope);

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(
                std::env::current_dir()
                    .unwrap()
                    .join(".zaion")
                    .join("tool-results")
                    .as_path()
            )
        );
    }

    #[test]
    fn structured_wake_request_from_envelope_preserves_tool_result_environment_identity() {
        let envelope = CanonicalEnvelope::new(
            "api",
            PrincipalId("did:key:test".to_string()),
            ChannelId("api".to_string()),
            ThreadId("thread-a".to_string()),
            "message-a".to_string(),
            "hello".to_string(),
            None,
        )
        .unwrap()
        .with_metadata(
            "tool_result_environment",
            serde_json::json!({
                "environment_id": "docker:workspace:zaion-main:container-42",
                "environment_kind": "docker",
            }),
        );
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = structured_wake_request("did:key:test", "hello", envelope);

        assert_eq!(
            req.tool_result_environment_id.as_deref(),
            Some("docker:workspace:zaion-main:container-42")
        );
        assert_eq!(req.tool_result_environment_kind.as_deref(), Some("docker"));
    }

    struct RecordingToolResultTarget {
        root: std::path::PathBuf,
        environment_id: Option<String>,
        environment_kind: Option<String>,
        writes: std::sync::Mutex<Vec<(std::path::PathBuf, String)>>,
    }

    impl RecordingToolResultTarget {
        fn new(root: std::path::PathBuf) -> Self {
            Self {
                root,
                environment_id: None,
                environment_kind: None,
                writes: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn with_environment(
            root: std::path::PathBuf,
            environment_id: impl Into<String>,
            environment_kind: impl Into<String>,
        ) -> Self {
            Self {
                root,
                environment_id: Some(environment_id.into()),
                environment_kind: Some(environment_kind.into()),
                writes: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl zaion_runtime::ToolResultStorageTarget for RecordingToolResultTarget {
        fn storage_root(&self) -> &std::path::Path {
            &self.root
        }

        fn environment_id(&self) -> Option<&str> {
            self.environment_id.as_deref()
        }

        fn environment_kind(&self) -> Option<&str> {
            self.environment_kind.as_deref()
        }

        fn write_tool_result(
            &self,
            path: &std::path::Path,
            content: &str,
        ) -> zaion_runtime::ToolResultStorageResult<()> {
            self.writes
                .lock()
                .unwrap()
                .push((path.to_path_buf(), content.to_string()));
            Ok(())
        }
    }

    #[test]
    fn wake_tool_context_output_can_spill_to_active_environment_target() {
        let host_dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-result-host-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(host_dir.clone());
        std::fs::create_dir_all(&host_dir).unwrap();
        let env_root = host_dir.join("active-env").join("zaion-tool-results");
        let target = RecordingToolResultTarget::new(env_root.clone());
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 80,
            turn_budget_bytes: 10_000,
            preview_bytes: 32,
            storage_dir: host_dir.clone(),
        };
        let raw = "sandbox-visible wake output\n".repeat(40);

        let context_output = budget_tool_context_output_with_target(
            "shell_exec",
            "wake_env_call",
            &raw,
            &config,
            &target,
        )
        .unwrap();

        assert!(context_output.contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(context_output.contains(env_root.to_string_lossy().as_ref()));
        assert!(!host_dir.join("wake_env_call.txt").exists());
        let writes = target.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, env_root.join("wake_env_call.txt"));
        assert_eq!(writes[0].1, raw);
    }

    #[test]
    fn wake_native_tool_calls_use_active_environment_target_for_per_result_spill() {
        let host_dir = std::env::temp_dir().join(format!(
            "zaion-wake-native-tool-env-target-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(host_dir.clone());
        std::fs::create_dir_all(&host_dir).unwrap();
        let env_root = host_dir.join("active-env").join("zaion-tool-results");
        let target = RecordingToolResultTarget::new(env_root.clone());
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 120,
            turn_budget_bytes: 10_000,
            preview_bytes: 40,
            storage_dir: host_dir.clone(),
        };
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "large_native",
                "1.0",
                "returns output large enough to spill",
                zaion_mcp::McpSchema::empty(),
                "read",
            ),
            |_| Ok(serde_json::json!({ "content": "z".repeat(800) })),
        ));
        let call = ToolCall {
            id: "native_env_call".to_string(),
            name: "large_native".to_string(),
            arguments: serde_json::json!({}),
        };
        let mut todos = TodoStore::new();
        let log = Logger::new(None);

        let records = execute_native_tool_calls(
            &builtin,
            None,
            &mut todos,
            &[call],
            &log,
            None,
            &config,
            &target,
            None,
            None,
        );

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].receipt_status, "executed");
        assert!(records[0]
            .context_output
            .contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(records[0]
            .context_output
            .contains(env_root.to_string_lossy().as_ref()));
        assert!(!host_dir.join("native_env_call.txt").exists());
        let writes = target.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, env_root.join("native_env_call.txt"));
        assert!(writes[0].1.contains("\"content\""));
        assert!(writes[0].1.contains(&"z".repeat(800)));
    }

    #[test]
    fn concurrency_safe_classification_matches_capability_class() {
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        zaion_mcp::register_builtin_tools(&mut builtin);

        let safe = |name: &str| {
            is_concurrency_safe_builtin(
                &builtin,
                &ToolCall {
                    id: "c".to_string(),
                    name: name.to_string(),
                    arguments: serde_json::json!({}),
                },
            )
        };

        // read / memory / diagnostic builtins are parallel-safe
        assert!(safe("fs_read"));
        assert!(safe("fs_list"));
        assert!(safe("memory_search"));
        assert!(safe("capability_status"));
        // write / execute / network builtins are serial
        assert!(!safe("fs_write"));
        assert!(!safe("fs_edit"));
        assert!(!safe("shell_exec"));
        assert!(!safe("http_get"));
        // the session todo store and unknown tools are serial
        assert!(!safe("todo"));
        assert!(!safe("does_not_exist"));
    }

    #[test]
    fn parallel_batch_preserves_input_order_and_results() {
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        // Three pure read-class tools that echo their own name back.
        for name in ["echo_a", "echo_b", "echo_c"] {
            let tool_name = name.to_string();
            builtin.register(zaion_mcp::McpTool::new(
                zaion_mcp::McpToolMeta::new(
                    name,
                    "1.0",
                    "echo tool name",
                    zaion_mcp::McpSchema::empty(),
                    "read",
                ),
                move |_| Ok(serde_json::json!({ "who": tool_name.clone() })),
            ));
        }

        let calls: Vec<ToolCall> = ["echo_a", "echo_b", "echo_c"]
            .iter()
            .enumerate()
            .map(|(i, name)| ToolCall {
                id: format!("call_{i}"),
                name: name.to_string(),
                arguments: serde_json::json!({}),
            })
            .collect();

        let config = default_tool_result_budget_config();
        let target = zaion_runtime::HostToolResultStorageTarget::new(config.storage_dir.clone());
        let mut todos = TodoStore::new();
        let log = Logger::new(None);

        let records = execute_native_tool_calls(
            &builtin, None, &mut todos, &calls, &log, None, &config, &target, None, None,
        );

        assert_eq!(records.len(), 3);
        // Results must come back in the same order as the calls, despite
        // running on separate worker threads.
        assert_eq!(records[0].call_id, "call_0");
        assert_eq!(records[1].call_id, "call_1");
        assert_eq!(records[2].call_id, "call_2");
        assert!(records[0].context_output.contains("echo_a"));
        assert!(records[1].context_output.contains("echo_b"));
        assert!(records[2].context_output.contains("echo_c"));
        for record in &records {
            assert_eq!(record.receipt_status, "executed");
        }
    }

    #[test]
    fn turn_contract_v2_allows_once_and_denies_with_zero_execution() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let read_executions = Arc::new(AtomicUsize::new(0));
        let write_executions = Arc::new(AtomicUsize::new(0));
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        let read_counter = Arc::clone(&read_executions);
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "read_probe",
                "1.0",
                "must execute exactly once with v2 read authority",
                zaion_mcp::McpSchema::empty(),
                "read",
            ),
            move |_| {
                read_counter.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"executed": true}))
            },
        ));
        let write_counter = Arc::clone(&write_executions);
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "write_probe",
                "1.0",
                "must not execute without v2 write authority",
                zaion_mcp::McpSchema::empty(),
                "write",
            ),
            move |_| {
                write_counter.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"executed": true}))
            },
        ));

        let process = zaion_core::process::AgenticProcess {
            principal_id: "did:key:tool-broker-test".to_string(),
            public_key_hex: "00".to_string(),
            state: zaion_core::process::ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-1",
            "run write probe",
            None,
        )
        .unwrap();
        let ingress = local_cli_ingress(
            &process,
            &envelope,
            "default".to_string(),
            chrono::Utc::now(),
        )
        .unwrap();
        let mut contract = TurnContractV2::new(ingress);
        contract.transition(TurnState::Routed).unwrap();
        contract.transition(TurnState::Running).unwrap();

        let calls = [
            ToolCall {
                id: "call-read-probe".to_string(),
                name: "read_probe".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "call-write-probe".to_string(),
                name: "write_probe".to_string(),
                arguments: serde_json::json!({}),
            },
        ];
        let config = default_tool_result_budget_config();
        let target = zaion_runtime::HostToolResultStorageTarget::new(config.storage_dir.clone());
        let mut todos = TodoStore::new();
        let records = execute_native_tool_calls(
            &builtin,
            None,
            &mut todos,
            &calls,
            &Logger::new(None),
            None,
            &config,
            &target,
            Some(&contract),
            None,
        );

        assert_eq!(read_executions.load(Ordering::SeqCst), 1);
        assert_eq!(write_executions.load(Ordering::SeqCst), 0);
        assert_eq!(records[0].receipt_status, "executed");
        assert_eq!(records[0].policy_decision.effect, "allow");
        assert_eq!(
            records[0].policy_decision.reason_code,
            "tool_broker_v2_allowed"
        );
        assert_eq!(records[1].receipt_status, "blocked");
        assert_eq!(records[1].policy_decision.effect, "deny");
        assert_eq!(
            records[1].policy_decision.reason_code,
            "tool_broker_v2_denied"
        );
    }

    #[test]
    fn mixed_safe_and_serial_calls_preserve_order() {
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "pure_read",
                "1.0",
                "pure read",
                zaion_mcp::McpSchema::empty(),
                "read",
            ),
            |_| Ok(serde_json::json!({ "kind": "read" })),
        ));
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "serial_write",
                "1.0",
                "serial write",
                zaion_mcp::McpSchema::empty(),
                "write",
            ),
            |_| Ok(serde_json::json!({ "kind": "write" })),
        ));

        // Pattern: read, read, write, read — the writes break the safe groups.
        let calls: Vec<ToolCall> = [
            ("pure_read", "r0"),
            ("pure_read", "r1"),
            ("serial_write", "w0"),
            ("pure_read", "r2"),
        ]
        .iter()
        .map(|(name, id)| ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({}),
        })
        .collect();

        let config = default_tool_result_budget_config();
        let target = zaion_runtime::HostToolResultStorageTarget::new(config.storage_dir.clone());
        let mut todos = TodoStore::new();
        let log = Logger::new(None);

        let records = execute_native_tool_calls(
            &builtin, None, &mut todos, &calls, &log, None, &config, &target, None, None,
        );

        assert_eq!(records.len(), 4);
        assert_eq!(records[0].call_id, "r0");
        assert_eq!(records[1].call_id, "r1");
        assert_eq!(records[2].call_id, "w0");
        assert_eq!(records[3].call_id, "r2");
        assert!(records[2].context_output.contains("write"));
        for record in &records {
            assert_eq!(record.receipt_status, "executed");
        }
    }

    #[test]
    fn tool_loop_stops_when_token_budget_exceeded() {
        let stop = evaluate_tool_loop_stop(TOOL_LOOP_TOKEN_BUDGET + 1, &[]);
        match stop {
            Some(ToolLoopStop::TokenBudgetExceeded { used, budget }) => {
                assert_eq!(used, TOOL_LOOP_TOKEN_BUDGET + 1);
                assert_eq!(budget, TOOL_LOOP_TOKEN_BUDGET);
            }
            other => panic!("expected TokenBudgetExceeded, got {:?}", other),
        }
    }

    #[test]
    fn tool_loop_stops_on_diminishing_returns() {
        // Three consecutive negligible-output follow-ups → stop.
        let outputs = vec![5, 2, 0];
        let stop = evaluate_tool_loop_stop(1_000, &outputs);
        assert_eq!(
            stop,
            Some(ToolLoopStop::DiminishingReturns {
                window: DIMINISHING_RETURNS_WINDOW
            })
        );
    }

    #[test]
    fn tool_loop_continues_with_productive_output() {
        // Recent window includes a productive turn → keep going.
        let outputs = vec![1, 1, 500];
        assert_eq!(evaluate_tool_loop_stop(1_000, &outputs), None);
    }

    #[test]
    fn tool_loop_needs_full_window_for_diminishing_returns() {
        // Only two negligible turns recorded; window not yet full.
        let outputs = vec![1, 1];
        assert_eq!(evaluate_tool_loop_stop(1_000, &outputs), None);
    }

    #[test]
    fn wake_tool_receipt_records_persisted_output_storage_metadata() {
        let host_dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-receipt-storage-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(host_dir.clone());
        std::fs::create_dir_all(&host_dir).unwrap();
        let env_root = host_dir.join("active-env").join("zaion-tool-results");
        let target = RecordingToolResultTarget::new(env_root.clone());
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 120,
            turn_budget_bytes: 10_000,
            preview_bytes: 40,
            storage_dir: host_dir.clone(),
        };
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "large_receipt_native",
                "1.0",
                "returns output large enough to persist",
                zaion_mcp::McpSchema::empty(),
                "read",
            ),
            |_| Ok(serde_json::json!({ "content": "r".repeat(800) })),
        ));
        let call = ToolCall {
            id: "receipt_env_call".to_string(),
            name: "large_receipt_native".to_string(),
            arguments: serde_json::json!({}),
        };
        let mut todos = TodoStore::new();
        let log = Logger::new(None);
        let records = execute_native_tool_calls(
            &builtin,
            None,
            &mut todos,
            std::slice::from_ref(&call),
            &log,
            None,
            &config,
            &target,
            None,
            None,
        );
        let ledger = zaion_ledger::EventLedger::new(host_dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.tool.receipt.storage.test".to_string());
        let sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "tool result persisted",
                }),
                None,
            )
            .unwrap();
        let received_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelReceived,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "run large tool",
                }),
                None,
            )
            .unwrap();

        let receipt_ids = append_tool_receipts(
            ToolReceiptContext {
                ledger: &ledger,
                kp: &kp,
                ns_key: &ns_key,
                pid: kp.principal_id().as_str(),
                channel_id: "terminal",
                thread_id: "main",
                user_event_id: Some(&received_event_id),
                sent_event_id: &sent_event_id,
            },
            std::slice::from_ref(&call),
            &[],
            &records,
        )
        .unwrap();
        assert_eq!(receipt_ids.len(), 1);
        assert!(
            receipt_ids[0].starts_with("evt-"),
            "wake should expose signed tool receipt event ids"
        );

        let events = ledger
            .list_typed_events(
                &zaion_types::session::SessionKey(ns_key.0.clone()),
                Some(&EventType::ToolReceipt),
                1,
            )
            .unwrap();
        let payload = &events[0].payload;
        let storage = payload
            .get("tool_result_storage")
            .expect("tool receipt should carry persisted output storage metadata");

        assert_eq!(storage["stored"], serde_json::json!(true));
        assert_eq!(storage["truncated"], serde_json::json!(true));
        assert_eq!(storage["bytes"], serde_json::json!(814));
        assert_eq!(storage["preview_bytes"], serde_json::json!(40));
        assert_eq!(
            storage["path"],
            serde_json::json!(env_root.join("receipt_env_call.txt").to_string_lossy())
        );
        assert_eq!(
            storage["storage_root"],
            serde_json::json!(env_root.to_string_lossy())
        );
        assert_eq!(
            storage["tool_call_id"],
            serde_json::json!("receipt_env_call")
        );
        assert_eq!(
            storage["tool_name"],
            serde_json::json!("large_receipt_native")
        );
        assert!(
            payload["permission_proof"].is_object(),
            "receipt should preserve permission proof alongside storage metadata"
        );
        let binding = payload.get("tool_result_storage_binding").expect(
            "tool receipt should bind persisted storage to provenance and turn proof material",
        );
        assert_eq!(
            binding["schema"],
            serde_json::json!("zaion.tool_result_storage_binding.v1")
        );
        assert_eq!(
            binding["environment"]["environment_id"],
            serde_json::json!(format!(
                "storage-root:{}",
                hash_text(env_root.to_string_lossy().as_ref())
            ))
        );
        assert_eq!(
            binding["environment"]["storage_root"],
            serde_json::json!(env_root.to_string_lossy())
        );
        assert_eq!(
            binding["permission_scope"]["permission_id"],
            payload["permission_id"]
        );
        assert_eq!(
            binding["permission_scope"]["sandbox_scope"],
            payload["sandbox_scope"]
        );
        assert_eq!(
            binding["provenance_chain"]["principal_id"],
            serde_json::json!(kp.principal_id().as_str())
        );
        assert_eq!(
            binding["provenance_chain"]["namespace_key"],
            serde_json::json!(ns_key.0.clone())
        );
        assert_eq!(
            binding["provenance_chain"]["parent_output_event_id"],
            serde_json::json!(sent_event_id.0.clone())
        );
        assert_eq!(
            binding["turn_proof_material"]["user_event_id"],
            serde_json::json!(received_event_id.0.clone())
        );
        assert_eq!(
            binding["turn_proof_material"]["output_event_id"],
            serde_json::json!(sent_event_id.0.clone())
        );
        assert_eq!(
            binding["turn_proof_material"]["event_lineage"],
            serde_json::json!([received_event_id.0.clone(), sent_event_id.0.clone()])
        );
        assert!(binding["binding_hash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64));
    }

    #[test]
    fn wake_tool_receipt_binding_prefers_explicit_environment_identity() {
        let host_dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-receipt-env-id-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(host_dir.clone());
        std::fs::create_dir_all(&host_dir).unwrap();
        let env_root = host_dir.join("remote-modal").join("zaion-tool-results");
        let target = RecordingToolResultTarget::with_environment(
            env_root.clone(),
            "modal:workspace:zaion-main:runner-17",
            "modal",
        );
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 120,
            turn_budget_bytes: 10_000,
            preview_bytes: 40,
            storage_dir: host_dir.clone(),
        };
        let mut builtin = zaion_mcp::McpToolRegistry::new();
        builtin.register(zaion_mcp::McpTool::new(
            zaion_mcp::McpToolMeta::new(
                "remote_large_receipt_native",
                "1.0",
                "returns output large enough to persist through a named backend",
                zaion_mcp::McpSchema::empty(),
                "read",
            ),
            |_| Ok(serde_json::json!({ "content": "e".repeat(800) })),
        ));
        let call = ToolCall {
            id: "receipt_remote_env_call".to_string(),
            name: "remote_large_receipt_native".to_string(),
            arguments: serde_json::json!({}),
        };
        let mut todos = TodoStore::new();
        let log = Logger::new(None);
        let records = execute_native_tool_calls(
            &builtin,
            None,
            &mut todos,
            std::slice::from_ref(&call),
            &log,
            None,
            &config,
            &target,
            None,
            None,
        );
        let ledger = zaion_ledger::EventLedger::new(host_dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.tool.receipt.env.identity.test".to_string());
        let received_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelReceived,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "run remote large tool",
                }),
                None,
            )
            .unwrap();
        let sent_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "remote tool result persisted",
                }),
                None,
            )
            .unwrap();

        let receipt_ids = append_tool_receipts(
            ToolReceiptContext {
                ledger: &ledger,
                kp: &kp,
                ns_key: &ns_key,
                pid: kp.principal_id().as_str(),
                channel_id: "terminal",
                thread_id: "main",
                user_event_id: Some(&received_event_id),
                sent_event_id: &sent_event_id,
            },
            std::slice::from_ref(&call),
            &[],
            &records,
        )
        .unwrap();
        assert_eq!(receipt_ids.len(), 1);

        let events = ledger
            .list_typed_events(
                &zaion_types::session::SessionKey(ns_key.0.clone()),
                Some(&EventType::ToolReceipt),
                1,
            )
            .unwrap();
        let binding = events[0]
            .payload
            .get("tool_result_storage_binding")
            .expect("persisted receipt should carry storage binding");

        assert_eq!(
            binding["environment"]["environment_id"],
            serde_json::json!("modal:workspace:zaion-main:runner-17")
        );
        assert_eq!(
            binding["environment"]["environment_kind"],
            serde_json::json!("modal")
        );
        assert_eq!(
            binding["environment"]["storage_root"],
            serde_json::json!(env_root.to_string_lossy())
        );
        assert_ne!(
            binding["environment"]["environment_id"],
            serde_json::json!(format!(
                "storage-root:{}",
                hash_text(env_root.to_string_lossy().as_ref())
            )),
            "named backend identity must not collapse back to a storage-root hash"
        );
    }

    #[test]
    fn wake_tool_receipt_proof_join_event_links_receipts_to_turn_proof() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-receipt-proof-join-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = zaion_ledger::EventLedger::new(dir.join("events.db"));
        let kp = zaion_crypto::ZaionKeypair::generate();
        let ns_key = NamespaceKey("session.tool.receipt.proof.join.test".to_string());
        let received_event_id = ledger
            .append_signed_typed_event(
                &kp,
                &ns_key,
                EventType::ChannelReceived,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "run tool",
                }),
                None,
            )
            .unwrap();
        let sent_event_id = ledger
            .append_signed_typed_event_with_parent(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                serde_json::json!({
                    "principal_id": kp.principal_id().as_str(),
                    "channel_id": "terminal",
                    "thread_id": "main",
                    "content": "tool done",
                }),
                None,
                Some(&received_event_id),
            )
            .unwrap();
        let receipt_ids = vec!["evt-receipt-a".to_string(), "evt-receipt-b".to_string()];
        let answer_trace_event_id = ledger
            .append_signed_typed_event_with_parent(
                &kp,
                &ns_key,
                EventType::AnswerTrace,
                serde_json::json!({
                    "schema": "zaion.answer_trace.v1",
                    "lineage": [
                        received_event_id.0.as_str(),
                        sent_event_id.0.as_str()
                    ],
                }),
                None,
                Some(&sent_event_id),
            )
            .unwrap();
        let turn_proof_event_id = ledger
            .append_signed_typed_event_with_parent(
                &kp,
                &ns_key,
                EventType::TurnProof,
                serde_json::json!({
                    "schema": "zaion.turn_proof.v1",
                    "proof_hash": "proof-hash-123",
                    "tool_receipt_ids": receipt_ids,
                }),
                None,
                Some(&answer_trace_event_id),
            )
            .unwrap();

        let join_event_id = append_tool_receipt_proof_join_event(
            &ledger,
            &kp,
            &ns_key,
            kp.principal_id().as_str(),
            "terminal",
            "main",
            &received_event_id,
            &sent_event_id,
            &answer_trace_event_id,
            &turn_proof_event_id,
            "proof-hash-123",
            &receipt_ids,
        )
        .unwrap()
        .expect("tool receipts should produce a proof join event");

        let event = ledger
            .get_event(&join_event_id.0)
            .unwrap()
            .expect("join event should be queryable");
        assert_eq!(event.event_type, "tool.receipt.proof_join");
        assert_eq!(
            event
                .parent_event_id
                .as_ref()
                .map(|event_id| event_id.0.as_str()),
            Some(turn_proof_event_id.0.as_str())
        );
        assert_eq!(
            event.payload["schema"],
            serde_json::json!("zaion.tool_receipt_proof_join.v1")
        );
        assert_eq!(
            event.payload["tool_receipt_ids"],
            serde_json::json!(["evt-receipt-a", "evt-receipt-b"])
        );
        assert_eq!(
            event.payload["turn_proof_event_id"],
            serde_json::json!(turn_proof_event_id.0.clone())
        );
        assert_eq!(
            event.payload["turn_proof_hash"],
            serde_json::json!("proof-hash-123")
        );
        assert_eq!(
            event.payload["lineage"],
            serde_json::json!([
                received_event_id.0.clone(),
                sent_event_id.0.clone(),
                answer_trace_event_id.0.clone(),
                turn_proof_event_id.0.clone()
            ])
        );
        assert!(event.payload["join_hash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64));
    }

    #[test]
    fn wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry() {
        let dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-result-turn-budget-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 10_000,
            turn_budget_bytes: 520,
            preview_bytes: 40,
            storage_dir: dir.clone(),
        };
        let mut records = vec![
            ToolExecutionRecord {
                call_id: "small_call".to_string(),
                name: "fs_read".to_string(),
                arguments_hash: "small_args".to_string(),
                output_hash: Some("small_output".to_string()),
                policy_decision: PolicyDecision::allow_builtin(
                    "fs_read",
                    CapabilityClass::External,
                ),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: "a".repeat(80),
                tool_result_metadata: None,
                error: None,
                todo_state_json: None,
            },
            ToolExecutionRecord {
                call_id: "large_call".to_string(),
                name: "shell_exec".to_string(),
                arguments_hash: "large_args".to_string(),
                output_hash: Some("large_output".to_string()),
                policy_decision: PolicyDecision::allow_builtin(
                    "shell_exec",
                    CapabilityClass::External,
                ),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: "b".repeat(900),
                tool_result_metadata: None,
                error: None,
                todo_state_json: None,
            },
        ];

        enforce_tool_context_turn_budget_with_config(&mut records, &config).unwrap();

        assert_eq!(records[0].context_output, "a".repeat(80));
        assert!(records[1]
            .context_output
            .contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(records[1].context_output.contains("Full output saved to:"));
        assert_eq!(
            std::fs::read_to_string(dir.join("large_call.txt")).unwrap(),
            "b".repeat(900)
        );
        let total: usize = records
            .iter()
            .map(|record| record.context_output.len())
            .sum();
        assert!(
            total <= config.turn_budget_bytes,
            "aggregate tool context should fit the turn budget after spill"
        );
    }

    #[test]
    fn wake_tool_context_batch_can_enforce_turn_budget_with_active_environment_target() {
        let host_dir = std::env::temp_dir().join(format!(
            "zaion-wake-tool-result-turn-env-{}",
            uuid::Uuid::new_v4()
        ));
        let _cleanup = TempRootGuard(host_dir.clone());
        std::fs::create_dir_all(&host_dir).unwrap();
        let env_root = host_dir.join("active-env").join("zaion-tool-results");
        let target = RecordingToolResultTarget::new(env_root.clone());
        let config = zaion_runtime::ToolResultBudgetConfig {
            result_budget_bytes: 10_000,
            turn_budget_bytes: 1_350,
            preview_bytes: 40,
            storage_dir: host_dir.clone(),
        };
        let mut records = vec![
            ToolExecutionRecord {
                call_id: "small_env_call".to_string(),
                name: "fs_read".to_string(),
                arguments_hash: "small_args".to_string(),
                output_hash: Some("small_output".to_string()),
                policy_decision: PolicyDecision::allow_builtin(
                    "fs_read",
                    CapabilityClass::External,
                ),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: "a".repeat(80),
                tool_result_metadata: None,
                error: None,
                todo_state_json: None,
            },
            ToolExecutionRecord {
                call_id: "large_env_call".to_string(),
                name: "shell_exec".to_string(),
                arguments_hash: "large_args".to_string(),
                output_hash: Some("large_output".to_string()),
                policy_decision: PolicyDecision::allow_builtin(
                    "shell_exec",
                    CapabilityClass::External,
                ),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: "b".repeat(1_500),
                tool_result_metadata: None,
                error: None,
                todo_state_json: None,
            },
            ToolExecutionRecord {
                call_id: "medium_env_call".to_string(),
                name: "web_extract".to_string(),
                arguments_hash: "medium_args".to_string(),
                output_hash: Some("medium_output".to_string()),
                policy_decision: PolicyDecision::allow_builtin(
                    "web_extract",
                    CapabilityClass::External,
                ),
                permission_decision: "allowed_builtin".to_string(),
                sandbox_scope: "builtin".to_string(),
                receipt_status: "executed".to_string(),
                context_output: "c".repeat(600),
                tool_result_metadata: None,
                error: None,
                todo_state_json: None,
            },
        ];

        enforce_tool_context_turn_budget_with_target(&mut records, &config, &target).unwrap();

        assert_eq!(records[0].context_output, "a".repeat(80));
        assert!(records[1]
            .context_output
            .contains(zaion_runtime::PERSISTED_OUTPUT_TAG));
        assert!(records[1]
            .context_output
            .contains(env_root.to_string_lossy().as_ref()));
        assert_eq!(records[2].context_output, "c".repeat(600));
        assert!(!host_dir.join("large_env_call.txt").exists());
        let writes = target.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, env_root.join("large_env_call.txt"));
        assert_eq!(writes[0].1, "b".repeat(1_500));
    }

    #[test]
    fn blocked_tool_record_produces_deny_receipt() {
        let call = ToolCall {
            id: "call-blocked".to_string(),
            name: "shell_exec".to_string(),
            arguments: serde_json::json!({"command": "rm", "args": ["-rf", "/"]}),
        };
        let record = blocked_tool_record(&call, "workspace-guard", "destructive command denied");

        // The tool never ran: receipt is "blocked", a deny decision is recorded,
        // and the surfaced output names the blocking hook so the model can adapt.
        assert_eq!(record.receipt_status, "blocked");
        assert_eq!(record.policy_decision.effect, "deny");
        assert_eq!(
            record.policy_decision.reason_code,
            "denied_by_pre_tool_use_hook"
        );
        assert_eq!(record.name, "shell_exec");
        assert_eq!(record.call_id, "call-blocked");
        assert!(record.error.as_deref().unwrap().contains("workspace-guard"));
        let output: serde_json::Value =
            serde_json::from_str(&record.context_output).expect("output is JSON");
        assert_eq!(output["blocked_by"], "workspace-guard");
        assert_eq!(output["source"], "pre_tool_use_hook");
        assert_eq!(output["error"], "destructive command denied");
    }

    #[test]
    fn wake_entry_injected_cancel_token_is_triggerable() {
        let token = zaion_runtime::cancel::CancelToken::new();
        let _entry = WakeTurnKernelEntry {
            callback: None,
            cancel: Some(token.clone()),
        };
        assert!(!token.is_cancelled(), "token starts live");
        token.cancel();
        assert!(
            token.is_cancelled(),
            "external cancel triggers the entry token"
        );
    }

    #[test]
    fn turn_cancelled_responds_to_token_and_marker() {
        let token = zaion_runtime::cancel::CancelToken::new();
        assert!(!token.is_cancelled(), "token starts live");
        let marker = CancelMarker::cleanup("pid-cancel-test-0001");
        // the command surface writes the marker to cancel a turn
        let marker_path = marker.path.clone();
        if let Some(parent) = marker_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&marker_path, "").unwrap();
        assert!(turn_cancelled(&token, &marker), "marker triggers stop");
        token.cancel();
        assert!(
            turn_cancelled(&token, &marker),
            "token cancel also triggers"
        );
        drop(token);
        let token2 = zaion_runtime::cancel::CancelToken::new();
        assert!(turn_cancelled(&token2, &marker), "marker alone triggers");
        let marker_path = marker.path.clone();
        drop(marker);
        assert!(!marker_path.exists(), "marker cleaned on drop");
    }

    #[test]
    fn wake_hero_preloads_core_tool_subset() {
        // M3: hero mode pre-loads the core tool subset so missions keep
        // high tool-use tendency without manual env configuration.
        let prev = std::env::var("ZAION_TOOL_SUBSET").ok();
        std::env::remove_var("ZAION_TOOL_SUBSET");
        let args = vec!["hero".to_string(), "--help".to_string()];
        cmd_wake_hero(&args).expect("hero --help is a no-op");
        let subset = std::env::var("ZAION_TOOL_SUBSET").unwrap_or_default();
        assert!(
            subset.contains("fs_read") && subset.contains("shell_exec"),
            "hero preloads core tools, got: {subset}"
        );
        match prev {
            Some(v) => std::env::set_var("ZAION_TOOL_SUBSET", v),
            None => std::env::remove_var("ZAION_TOOL_SUBSET"),
        }
    }
}
