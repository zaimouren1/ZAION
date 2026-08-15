//! Unified agent runtime integration for cmd_wake
//!
//! This module implements the --unified flag for cmd_wake, integrating:
//! - UnifiedAgentRuntime (webhook + memory + MCP + compression)
//! - Ed25519 signed execution
//! - Provenance tracking
//! - Automatic context compression

use crate::commands::provider::{
    build_provider, provider_supports_prompt_cache, resolve_provider_selection_from_args,
    resolve_smart_provider_model,
};
use crate::commands::{data_dir, CliError};
use crate::config::{McpStore, ZaionConfig};
use std::sync::Arc;
use zaion_adapters::provider::{ChatMessage, CompletionRequest};
use zaion_federation::HonchoClient;
use zaion_memory::runtime_integration::{
    BuiltinMemoryProvider, MemoryManager, MemoryRuntimeConfig,
};
use zaion_runtime::{
    build_answer_evidence_subgraph, build_turn_proof, stable_hash_bytes, AnswerEvidenceInput,
    McpBridge, McpToolRegistry, ProofClosureVerifier, RuntimeOutput, TurnCapabilityManifest,
    TurnExecution, TurnProofInput, UnifiedAgentConfig, UnifiedAgentRuntime, WakeFeaturePolicy,
    WakeOperationRecorder, WebhookRuntimeManager,
};
use zaion_types::event::{EventId, EventType};

pub(crate) struct UnifiedWakeHandoff<'a> {
    pub received_event_id: Option<EventId>,
    pub inherited_omni_route_event_id: Option<EventId>,
    pub inherited_omni_route_authority_hash: Option<String>,
    pub runtime_owner: &'static str,
    pub runtime_topology: Vec<String>,
    pub operation_recorder: &'a WakeOperationRecorder,
    pub parent_sequence: Option<u64>,
}

/// Execute cmd_wake using UnifiedAgentRuntime
pub(crate) fn cmd_wake_unified(
    args: &[String],
    pid: &str,
    message: &str,
    cfg: &ZaionConfig,
    feature_policy: WakeFeaturePolicy,
    handoff: UnifiedWakeHandoff<'_>,
) -> Result<TurnExecution, CliError> {
    let UnifiedWakeHandoff {
        received_event_id,
        inherited_omni_route_event_id,
        inherited_omni_route_authority_hash,
        runtime_owner,
        runtime_topology,
        operation_recorder,
        parent_sequence,
    } = handoff;
    // Parse provider/model through the same resolver used by chat and wake.
    let provider_selection = resolve_provider_selection_from_args(args, cfg)?;
    let (provider_type, model) = resolve_smart_provider_model(
        message,
        &provider_selection.provider,
        provider_selection.model.as_deref(),
        feature_policy.smart_route_enabled,
        false,
    );
    let proof_model = model
        .clone()
        .unwrap_or_else(|| cfg.model.clone().unwrap_or_else(|| "(not set)".to_string()));

    let enable_memory = feature_policy.memory_enabled;
    let enable_compression = feature_policy.compression_enabled;
    let enable_mcp = feature_policy.mcp_enabled;
    let enable_webhooks = feature_policy.webhooks_enabled;
    let enable_cache = feature_policy.cache_enabled
        && provider_supports_prompt_cache(&provider_type, model.as_deref(), cfg)?;
    let enable_smart_route = feature_policy.smart_route_enabled;
    let agent_settings = cfg.agent.clamped();
    let enable_honcho = args.iter().any(|a| a == "--honcho");

    // Load process
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (process, kp) = store.load(pid).map_err(CliError::Core)?;

    // Create unified runtime configuration.
    // 压缩阈值与 Token 预算来自校验后的 `[agent]` 配置，而非硬编码常量。
    let unified_config = UnifiedAgentConfig {
        enable_memory,
        enable_compression,
        force_compression: feature_policy.compression_requested,
        enable_mcp,
        enable_webhooks,
        compression_threshold: agent_settings.compression_threshold,
        token_budget: agent_settings.token_budget,
        session_id: format!("{}:wake", pid),
        principal_id: pid.to_string(),
    };

    // Initialize runtime components
    let webhook_manager = Arc::new(WebhookRuntimeManager::new());
    let memory_manager = if enable_memory {
        build_unified_wake_memory_manager(
            cfg,
            pid,
            kp.principal_id().as_str(),
            &store.process_dir(pid),
        )?
    } else {
        Arc::new(MemoryManager::new())
    };

    // Initialize MCP tool registry if enabled
    let mcp_registry = if enable_mcp {
        let mcp_config_path = McpStore::path();
        if mcp_config_path.exists() {
            let bridge = Arc::new(McpBridge::new_with_key(Arc::new(kp.clone())));
            let registry = Arc::new(McpToolRegistry::new(mcp_config_path, bridge));
            match tokio::runtime::Runtime::new() {
                Ok(rt) => {
                    if let Err(e) = rt.block_on(registry.load_from_config()) {
                        eprintln!("[unified] Failed to load MCP config: {}", e);
                        None
                    } else {
                        eprintln!("[unified] Loaded MCP tool registry");
                        Some(registry)
                    }
                }
                Err(e) => {
                    eprintln!("[unified] Failed to create tokio runtime for MCP: {}", e);
                    None
                }
            }
        } else {
            eprintln!("[unified] No MCP config found at {:?}", mcp_config_path);
            None
        }
    } else {
        None
    };

    // Create unified runtime
    let mut runtime = if enable_honcho {
        // Load Honcho config if available
        let honcho_config_path = zaion_paths::honcho_path();
        if honcho_config_path.exists() {
            match std::fs::read_to_string(&honcho_config_path) {
                Ok(content) => match toml::from_str::<zaion_federation::HonchoConfig>(&content) {
                    Ok(honcho_config) => {
                        let honcho_client = Arc::new(HonchoClient::new(honcho_config));
                        eprintln!("[unified] Honcho federation enabled");
                        UnifiedAgentRuntime::new_with_honcho_key(
                            unified_config,
                            webhook_manager,
                            memory_manager,
                            honcho_client,
                            Arc::new(kp.clone()),
                        )
                        .map_err(CliError::Usage)?
                    }
                    Err(e) => {
                        eprintln!("[unified] Failed to parse Honcho config: {}", e);
                        UnifiedAgentRuntime::new_with_key(
                            unified_config,
                            webhook_manager,
                            memory_manager,
                            Arc::new(kp.clone()),
                        )
                        .map_err(CliError::Usage)?
                    }
                },
                Err(e) => {
                    eprintln!("[unified] Failed to read Honcho config: {}", e);
                    UnifiedAgentRuntime::new_with_key(
                        unified_config,
                        webhook_manager,
                        memory_manager,
                        Arc::new(kp.clone()),
                    )
                    .map_err(CliError::Usage)?
                }
            }
        } else {
            eprintln!(
                "[unified] No Honcho config found at {:?}, run 'zaion honcho setup'",
                honcho_config_path
            );
            UnifiedAgentRuntime::new_with_key(
                unified_config,
                webhook_manager,
                memory_manager,
                Arc::new(kp.clone()),
            )
            .map_err(CliError::Usage)?
        }
    } else {
        UnifiedAgentRuntime::new_with_key(
            unified_config,
            webhook_manager,
            memory_manager,
            Arc::new(kp.clone()),
        )
        .map_err(CliError::Usage)?
    };
    if let Some(registry) = mcp_registry.clone() {
        runtime = runtime.with_mcp_registry(registry);
    }

    eprintln!(
        "[unified] memory={} compression={} mcp={} webhooks={} cache={} smart_route={} honcho={} mcp_loaded={}",
        enable_memory,
        enable_compression,
        enable_mcp,
        enable_webhooks,
        enable_cache,
        enable_smart_route,
        enable_honcho,
        mcp_registry.is_some()
    );

    // Create agent executor closure
    let provider_type_clone = provider_type.clone();
    let model_clone = model.clone();
    let cfg_clone = cfg.clone();
    let identity_contract_for_model = crate::commands::identity::startup_contract_for_prompt(
        cfg,
        Some(pid),
        Some(&process.workspace_id),
        Some(&process.project_id),
    );

    let agent_executor = move |prompt: &str| -> Result<String, String> {
        let prompt = prompt.to_string();
        let (provider, actual_model) =
            build_provider(&provider_type_clone, model_clone.clone(), &cfg_clone)
                .map_err(|e| e.to_string())?;

        // Build request
        let messages = vec![
            ChatMessage::text("system", identity_contract_for_model.clone()),
            ChatMessage::text("user", prompt),
        ];
        let req = CompletionRequest {
            model: actual_model,
            messages,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            tools: None,
            tool_choice: None,
            enable_cache,
        };

        // Execute
        tokio::task::block_in_place(|| provider.complete(&req).map(|resp| resp.content))
            .map_err(|e| e.to_string())
    };

    // Execute turn via unified runtime
    let rt = tokio::runtime::Runtime::new().map_err(|e| CliError::Usage(e.to_string()))?;
    let result = rt
        .block_on(runtime.execute_turn(message, agent_executor))
        .map_err(CliError::Usage)?;

    // Display result
    println!("{}", result.response);
    println!();
    eprintln!(
        "[unified] execution_time={}ms compressed={} turns_pruned={} memory_context_bytes={} mcp_tools_loaded={}",
        result.execution_time_ms,
        result.was_compressed,
        result.turns_compressed,
        result.memory_context_size,
        result.mcp_tools_loaded
    );
    eprintln!("[unified] provenance={}", &result.provenance_hash[..16]);
    eprintln!(
        "[unified] sig_scheme={} signer={}",
        result.ed25519_signature.scheme, result.ed25519_signature.signing_key_id
    );
    eprintln!(
        "[unified] signer_prefix={}",
        result
            .ed25519_signature
            .signing_key_id
            .chars()
            .take(16)
            .collect::<String>()
    );

    // Log to ledger
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.to_string());
    let received_event = if let Some(event_id) = received_event_id.as_ref() {
        ledger.get_event(&event_id.0).map_err(CliError::Ledger)?
    } else {
        None
    };
    let (omni_route_event_id, omni_route_authority_hash) =
        inherit_omni_route_proof_from_wake_handoff(
            &ledger,
            pid,
            received_event_id.as_ref(),
            inherited_omni_route_event_id.as_ref(),
            inherited_omni_route_authority_hash.as_deref(),
        )?;
    let channel_id = received_event
        .as_ref()
        .and_then(|event| event.payload.get("channel_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("unified");
    let thread_id = received_event
        .as_ref()
        .and_then(|event| event.payload.get("thread_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("wake");

    let sent_payload = serde_json::json!({
        "principal_id": pid,
        "response": result.response,
        "content": result.response,
        "channel_id": channel_id,
        "thread_id": thread_id,
        "unified": true,
        "was_compressed": result.was_compressed,
        "turns_compressed": result.turns_compressed,
        "execution_time_ms": result.execution_time_ms,
        "provenance_hash": result.provenance_hash,
        "ed25519_signature": result.ed25519_signature,
    });
    let sent_event_id = if received_event_id.is_some() {
        ledger
            .append_signed_typed_event_with_parent(
                &kp,
                &ns_key,
                EventType::ChannelSent,
                sent_payload,
                None,
                Some(&omni_route_event_id),
            )
            .map_err(CliError::Ledger)?
    } else {
        ledger
            .append_signed_typed_event(&kp, &ns_key, EventType::ChannelSent, sent_payload, None)
            .map_err(CliError::Ledger)?
    };

    let user_event_id = received_event_id
        .as_ref()
        .map(|event_id| event_id.0.clone())
        .unwrap_or_else(|| sent_event_id.0.clone());
    let response_hash = stable_hash_bytes(result.response.as_bytes());
    let runtime_memory_evidence = result.runtime_memory_evidence.clone();
    let runtime_memory_evidence_hash = runtime_memory_evidence
        .as_ref()
        .map(|evidence| evidence.evidence_hash.clone());
    let compression_evidence = result.compression_evidence.clone();
    let compression_evidence_hash = compression_evidence.evidence_hash.clone();
    let mut source_ledger_event_ids = vec![omni_route_event_id.0.clone()];
    if let Some(received_event_id) = &received_event_id {
        source_ledger_event_ids.push(received_event_id.0.clone());
    }
    let evidence_graph = build_answer_evidence_subgraph(AnswerEvidenceInput {
        response_hash: response_hash.clone(),
        source_ledger_event_ids,
        output_ledger_event_id: sent_event_id.0.clone(),
        ..Default::default()
    });
    let evidence_graph_hash = evidence_graph.graph_hash.clone();
    let answer_trace_event_id = ledger
        .append_signed_typed_event_with_parent(
            &kp,
            &ns_key,
            EventType::AnswerTrace,
            serde_json::json!({
                "schema": "zaion.answer_trace.v1",
                "principal_id": pid,
                "channel_id": channel_id,
                "thread_id": thread_id,
                "user_event_id": user_event_id,
                "output_event_id": sent_event_id.0,
                "omni_route_event_id": omni_route_event_id.0,
                "omni_route_authority_hash": omni_route_authority_hash,
                "context_pack_id": serde_json::Value::Null,
                "context_layers": [],
                "memory_atom_ids": [],
                "runtime_memory_evidence": runtime_memory_evidence,
                "runtime_memory_evidence_hash": runtime_memory_evidence_hash,
                "compression_evidence": compression_evidence,
                "compression_evidence_hash": compression_evidence_hash,
                "evidence_graph_hash": evidence_graph_hash,
                "evidence_graph": evidence_graph,
                "tool_call_count": 0,
                "tokens_in": 0,
                "tokens_out": 0,
                "response_hash": response_hash,
                "runtime": "unified",
                "provenance_hash": result.provenance_hash,
                "lineage": [
                    user_event_id.as_str(),
                    omni_route_event_id.0.as_str(),
                    sent_event_id.0.as_str()
                ],
            }),
            None,
            Some(&sent_event_id),
        )
        .map_err(CliError::Ledger)?;

    let identity_contract = crate::commands::identity::startup_contract_for_prompt(
        cfg,
        Some(pid),
        Some(&process.workspace_id),
        Some(&process.project_id),
    );
    let turn_proof = build_turn_proof(TurnProofInput {
        principal_id: pid.to_string(),
        workspace_id: process.workspace_id.clone(),
        project_id: process.project_id.clone(),
        channel_id: channel_id.to_string(),
        thread_id: thread_id.to_string(),
        namespace_key: ns_key.0.clone(),
        user_event_id,
        output_event_id: sent_event_id.0.clone(),
        omni_route_event_id: Some(omni_route_event_id.0.clone()),
        omni_route_authority_hash: Some(omni_route_authority_hash),
        namespace_transition_event_id: None,
        identity_contract,
        capability_manifest: TurnCapabilityManifest {
            provider: provider_type,
            model: proof_model,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            memory_enabled: enable_memory,
            mcp_enabled: enable_mcp,
            cache_enabled: enable_cache,
            smart_route_enabled: enable_smart_route,
            compression_requested: feature_policy.compression_requested,
            tools_requested: Vec::new(),
            boundaries: vec![
                "identity_contract_required".to_string(),
                "capability_manifest_required".to_string(),
                "ledger_event_lineage_required".to_string(),
                "channel_envelope_required".to_string(),
            ],
        },
        context_pack_id: None,
        context_layers: Vec::new(),
        memory_atom_ids: Vec::new(),
        compression_evidence: Some(result.compression_evidence.clone()),
        cost_evidence: None,
        runtime_memory_evidence: result.runtime_memory_evidence.clone(),
        evidence_graph_hash: Some(evidence_graph_hash.clone()),
        tokens_in: 0,
        tokens_out: 0,
        tool_call_count: 0,
        tool_receipt_ids: Vec::new(),
    });
    let mut proof_payload =
        serde_json::to_value(&turn_proof).map_err(|error| CliError::Usage(error.to_string()))?;
    if let serde_json::Value::Object(ref mut object) = proof_payload {
        object.insert(
            "answer_trace_event_id".to_string(),
            serde_json::Value::String(answer_trace_event_id.0.clone()),
        );
        object.insert(
            "runtime".to_string(),
            serde_json::Value::String("unified".to_string()),
        );
        object.insert(
            "runtime_owner".to_string(),
            serde_json::Value::String(runtime_owner.to_string()),
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
    let turn_proof_event_id = ledger
        .append_signed_typed_event_with_parent(
            &kp,
            &ns_key,
            EventType::TurnProof,
            proof_payload,
            None,
            Some(&answer_trace_event_id),
        )
        .map_err(CliError::Ledger)?;
    let public_key = kp.public_key_bytes();
    let proof_closure = ProofClosureVerifier::new(&ledger, &public_key)
        .verify(&answer_trace_event_id.0, &turn_proof_event_id.0, None)
        .map_err(|error| {
            CliError::Runtime(format!(
                "unified turn proof closure verification failed: {error}"
            ))
        })?;
    let runtime_output = RuntimeOutput {
        runtime_owner: runtime_owner.to_string(),
        runtime_topology,
        provider_response_hash: response_hash,
        context_pack_id: String::new(),
        memory_atom_ids: Vec::new(),
        tool_receipt_ids: Vec::new(),
        stream_hash: String::new(),
    };

    Ok(operation_recorder.finish_completed_turn(
        runtime_output,
        proof_closure,
        0,
        0,
        parent_sequence,
    ))
}

fn build_unified_wake_memory_manager(
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
    let rt = tokio::runtime::Runtime::new().map_err(|e| CliError::Usage(e.to_string()))?;
    rt.block_on(manager.add_provider(Box::new(provider)));
    let tool_count = rt.block_on(manager.get_all_tool_schemas()).len();
    let _ = (pid, tool_count);
    Ok(manager)
}

fn inherit_omni_route_proof_from_wake_handoff(
    ledger: &zaion_ledger::EventLedger,
    pid: &str,
    received_event_id: Option<&EventId>,
    inherited_omni_route_event_id: Option<&EventId>,
    inherited_omni_route_authority_hash: Option<&str>,
) -> Result<(EventId, String), CliError> {
    let received_event_id = received_event_id.ok_or_else(|| {
        CliError::Usage(
            "unified wake must inherit channel.received and omni.route from wake handoff"
                .to_string(),
        )
    })?;
    let inherited_omni_route_event_id = inherited_omni_route_event_id.ok_or_else(|| {
        CliError::Usage("unified wake must inherit omni.route event from wake handoff".to_string())
    })?;
    let inherited_omni_route_authority_hash =
        inherited_omni_route_authority_hash.ok_or_else(|| {
            CliError::Usage(
                "unified wake must inherit omni route authority hash from wake handoff".to_string(),
            )
        })?;

    let route_event = ledger
        .get_event(&inherited_omni_route_event_id.0)
        .map_err(CliError::Ledger)?
        .ok_or_else(|| {
            CliError::Usage(
                "unified wake must fail closed if inherited omni.route is missing".to_string(),
            )
        })?;

    if route_event.event_type != "omni.route" {
        return Err(CliError::Usage(format!(
            "unified wake inherited event {} is {}, expected omni.route",
            inherited_omni_route_event_id.0, route_event.event_type
        )));
    }
    if route_event
        .parent_event_id
        .as_ref()
        .map(|event_id| event_id.0.as_str())
        != Some(received_event_id.0.as_str())
    {
        return Err(CliError::Usage(format!(
            "unified wake inherited omni.route {} is not parented to channel.received {}",
            inherited_omni_route_event_id.0, received_event_id.0
        )));
    }
    if route_event
        .payload
        .get("principal_id")
        .and_then(|value| value.as_str())
        != Some(pid)
    {
        return Err(CliError::Usage(
            "unified wake inherited omni.route principal does not match wake principal".to_string(),
        ));
    }
    if route_event
        .payload
        .get("authority")
        .and_then(|value| value.as_str())
        != Some("OmniSessionManager")
    {
        return Err(CliError::Usage(
            "unified wake inherited omni.route must be issued by OmniSessionManager".to_string(),
        ));
    }
    let route_authority_hash = route_event
        .payload
        .get("authority_hash")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            CliError::Usage("unified wake inherited omni.route missing authority_hash".to_string())
        })?;
    if route_authority_hash != inherited_omni_route_authority_hash {
        return Err(CliError::Usage(
            "unified wake inherited omni authority hash does not match route event".to_string(),
        ));
    }

    Ok((
        EventId(inherited_omni_route_event_id.0.clone()),
        inherited_omni_route_authority_hash.to_string(),
    ))
}
