use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use zaion_runtime::{stable_hash_json, OmniSessionManager, TurnProof};

const COST_RECONCILIATION_EVENT: &str = "zaion.usage_cost.reconciled.v1";

pub fn cmd_turn(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("latest");
    match sub {
        "latest" => cmd_turn_latest(args),
        "trace" => cmd_turn_trace(args),
        "reconcile-cost" => cmd_turn_reconcile_cost(args),
        "approve" => cmd_turn_approve(args),
        "cancel" => cmd_turn_cancel(args),
        other => Err(CliError::Usage(format!(
            "unknown turn subcommand: {}. Use: latest, trace, approve, reconcile-cost",
            other
        ))),
    }
}

/// Cancel an in-flight wake turn by writing the cross-process cancel marker.
fn cmd_turn_cancel(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = arg_value(args, "--pid")
        .map(|pid| pid.to_string())
        .map(Ok)
        .unwrap_or_else(|| crate::commands::process::resolve_existing_pid(&cfg))?;
    let path = data_dir().join("turns").join(format!("{pid}.cancel"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::Usage(format!("failed to create turns dir: {e}")))?;
    }
    std::fs::write(&path, "")
        .map_err(|e| CliError::Usage(format!("failed to write cancel marker: {e}")))?;
    println!("cancel requested for {}", pid);
    Ok(())
}

/// Approve a turn awaiting approval (WaitingApproval -> ToolRunning).
fn cmd_turn_approve(args: &[String]) -> Result<(), CliError> {
    let turn_id = args.get(3).ok_or_else(|| {
        CliError::Usage("zaion turn approve <turn-id> [--tenant <tenant>] [--pid <pid>]".into())
    })?;
    let tenant = args
        .iter()
        .position(|a| a == "--tenant")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "local".to_string());
    let cfg = ZaionConfig::load();
    let pid = arg_value(args, "--pid")
        .map(|pid| pid.to_string())
        .map(Ok)
        .unwrap_or_else(|| crate::commands::process::resolve_existing_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let actor =
        zaion_runtime::session_actor::SessionActor::open(store.ledger_path(&pid), None)
            .map_err(|error| CliError::Usage(format!("failed to open turn store: {error}")))?;
    let approved = actor
        .approve_turn(&tenant, turn_id, chrono::Utc::now())
        .map_err(|error| CliError::Usage(format!("approval failed: {error}")))?;
    println!("turn approved");
    println!("  turn_id       : {}", approved.turn_id);
    println!("  tenant_id     : {}", approved.tenant_id);
    println!("  state         : {:?}", approved.state.state());
    Ok(())
}

fn cmd_turn_latest(args: &[String]) -> Result<(), CliError> {
    let (ledger, _) = load_turn_ledger(args)?;
    let event = latest_turn_proof_event(&ledger)?;
    let proof = decode_turn_proof(&event)?;
    println!("turn proof latest");
    println!("  proof_event_id : {}", event.event_id.0);
    println!("  proof_id       : {}", proof.proof_id);
    println!("  principal_id   : {}", proof.principal_id);
    println!("  output_event_id: {}", proof.output_event_id);
    println!("  provider       : {}", proof.capability_manifest.provider);
    println!("  model          : {}", proof.capability_manifest.model);
    println!("  proof_hash     : {}", proof.proof_hash);
    Ok(())
}

fn cmd_turn_trace(args: &[String]) -> Result<(), CliError> {
    let event_id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion turn trace <event-id> [--pid <pid>]".into()))?;
    let (ledger, _) = load_turn_ledger(args)?;
    let proof_event = find_turn_proof_event(&ledger, event_id)?;
    let proof = decode_turn_proof(&proof_event)?;
    let cost_reconciliation = latest_cost_reconciliation_event(&ledger, &proof)?;
    let proof_hash_ok = verify_proof_hash(&proof);
    let tool_receipt_join_trace = trace_tool_receipt_joins(&ledger, &proof_event, &proof)?;
    let received = ledger.get_event(&proof.user_event_id)?;
    let sent = ledger.get_event(&proof.output_event_id)?;
    let omni_route = find_omni_route_event(&ledger, &proof)?;
    let omni_graph_replay = replay_omni_session_graph(&ledger, &proof);
    let received_ok = received
        .as_ref()
        .map(|event| event.event_type == "channel.received")
        .unwrap_or(false);
    let route_parent_ok = omni_route
        .as_ref()
        .map(|event| {
            event
                .parent_event_id
                .as_ref()
                .map(|parent| parent.0.as_str())
                == Some(proof.user_event_id.as_str())
        })
        .unwrap_or(false);
    let sent_ok = sent
        .as_ref()
        .map(|event| {
            event.event_type == "channel.sent"
                && event
                    .parent_event_id
                    .as_ref()
                    .map(|parent| parent.0.as_str())
                    == proof
                        .omni_route_event_id
                        .as_deref()
                        .or(Some(proof.user_event_id.as_str()))
        })
        .unwrap_or(false);
    let answer_trace_parent = proof_event
        .parent_event_id
        .as_ref()
        .and_then(|parent| ledger.get_event(&parent.0).ok().flatten());
    let runtime_memory_trace_match =
        runtime_memory_trace_matches(answer_trace_parent.as_ref(), &proof);
    let proof_parent_ok = proof_event
        .parent_event_id
        .as_ref()
        .map(|parent| parent.0.as_str())
        == Some(proof.output_event_id.as_str())
        || answer_trace_parent
            .as_ref()
            .map(|event| {
                event.event_type == "answer.trace"
                    && event
                        .parent_event_id
                        .as_ref()
                        .map(|parent| parent.0.as_str())
                        == Some(proof.output_event_id.as_str())
            })
            .unwrap_or(false);

    println!("turn proof trace");
    println!("  proof_event_id          : {}", proof_event.event_id.0);
    println!("  proof_id                : {}", proof.proof_id);
    println!("  principal_id            : {}", proof.principal_id);
    println!("  workspace_id            : {}", proof.workspace_id);
    println!("  project_id              : {}", proof.project_id);
    println!("  channel_id              : {}", proof.channel_id);
    println!("  thread_id               : {}", proof.thread_id);
    println!("  namespace_key           : {}", proof.namespace_key);
    println!("  user_event_id           : {}", proof.user_event_id);
    println!("  output_event_id         : {}", proof.output_event_id);
    println!(
        "  proof_omni_route_event  : {}",
        proof.omni_route_event_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  omni_authority_hash     : {}",
        proof
            .omni_route_authority_hash
            .as_deref()
            .unwrap_or("(none)")
    );
    println!("  lineage_received        : {}", bool_text(received_ok));
    println!("  lineage_route_parent    : {}", bool_text(route_parent_ok));
    println!("  lineage_sent_parent     : {}", bool_text(sent_ok));
    println!("  lineage_proof_parent    : {}", bool_text(proof_parent_ok));
    println!(
        "  omni_route_event_id     : {}",
        omni_route
            .as_ref()
            .map(|event| event.event_id.0.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  omni_session_id         : {}",
        omni_route
            .as_ref()
            .and_then(|event| event.payload.get("session_id"))
            .and_then(|value| value.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  omni_authority          : {}",
        omni_route
            .as_ref()
            .and_then(|event| event.payload.get("authority"))
            .and_then(|value| value.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  omni_authority_verified : {}",
        bool_text(omni_authority_matches(&proof, omni_route.as_ref()))
    );
    println!(
        "  omni_graph_replay_schema: {}",
        omni_graph_replay
            .as_ref()
            .map(|replay| replay.schema.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  omni_graph_replay_hash  : {}",
        omni_graph_replay
            .as_ref()
            .map(|replay| replay.replay_hash.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  omni_graph_replay_events: {}",
        omni_graph_replay
            .as_ref()
            .map(|replay| replay.route_event_count)
            .unwrap_or(0)
    );
    println!(
        "  omni_graph_replay_ok    : {}",
        bool_text(omni_graph_replay_matches(
            &proof,
            omni_route.as_ref(),
            omni_graph_replay.as_ref()
        ))
    );
    println!(
        "  omni_channel_attached   : {}",
        bool_text(
            omni_route
                .as_ref()
                .and_then(|event| event.payload.get("channel_attached"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        )
    );
    println!(
        "  identity_contract_hash  : {}",
        proof.identity_contract_hash
    );
    println!(
        "  capability_manifest_hash: {}",
        proof.capability_manifest_hash
    );
    println!("  context_digest          : {}", proof.context_digest);
    println!(
        "  context_pack_id         : {}",
        proof.context_pack_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  provider                : {}",
        proof.capability_manifest.provider
    );
    println!(
        "  model                   : {}",
        proof.capability_manifest.model
    );
    println!(
        "  memory_enabled          : {}",
        bool_text(proof.capability_manifest.memory_enabled)
    );
    println!(
        "  mcp_enabled             : {}",
        bool_text(proof.capability_manifest.mcp_enabled)
    );
    println!(
        "  tools_requested         : {}",
        proof.capability_manifest.tools_requested.len()
    );
    println!("  context_layers          : {}", proof.context_layers.len());
    println!(
        "  memory_atom_ids         : {}",
        proof.memory_atom_ids.join(",")
    );
    println!(
        "  memory_atoms_active     : {}",
        memory_atoms_active_status(&proof.principal_id, &proof.memory_atom_ids)
    );
    if let Some(evidence) = proof.compression_evidence.as_ref() {
        println!("  compression_schema      : {}", evidence.schema);
        println!(
            "  compression_requested   : {}",
            bool_text(evidence.compression_requested)
        );
        println!(
            "  compression_applied     : {}",
            bool_text(evidence.was_compressed)
        );
        println!("  compression_turns_pruned: {}", evidence.turns_pruned);
        println!("  compression_summary_hash: {}", evidence.summary_hash);
        println!(
            "  compression_summary_strategy: {}",
            evidence.summary_strategy
        );
        println!(
            "  compression_pruned_tool_outputs: {}",
            evidence.pruned_tool_outputs
        );
        println!(
            "  compression_protected_head_turns: {}",
            evidence.protected_head_turns
        );
        println!(
            "  compression_protected_tail_turns: {}",
            evidence.protected_tail_turns
        );
        println!(
            "  compression_protected_tail_tokens: {}",
            evidence.protected_tail_tokens
        );
        println!(
            "  compression_summary_budget_tokens: {}",
            evidence.summary_budget_tokens
        );
        println!(
            "  compression_evidence_hash: {}",
            proof
                .compression_evidence_hash
                .as_deref()
                .unwrap_or(evidence.evidence_hash.as_str())
        );
    } else {
        println!("  compression_evidence    : (none)");
    }
    if let Some(evidence) = proof.cost_evidence.as_ref() {
        let reconciled_status = cost_reconciliation
            .as_ref()
            .and_then(|event| event.payload.get("cost_status"))
            .and_then(|value| value.as_str())
            .unwrap_or(evidence.cost_status.as_str());
        let reconciled_source = cost_reconciliation
            .as_ref()
            .and_then(|event| event.payload.get("cost_source"))
            .and_then(|value| value.as_str())
            .unwrap_or(evidence.cost_source.as_str());
        println!("  cost_schema             : {}", evidence.schema);
        println!("  cost_provider           : {}", evidence.provider);
        println!("  cost_model              : {}", evidence.model);
        println!("  cost_status             : {}", reconciled_status);
        println!("  cost_source             : {}", reconciled_source);
        println!("  cost_billing_mode       : {}", evidence.billing_mode);
        println!(
            "  cost_input_tokens       : {}",
            evidence.usage.input_tokens
        );
        println!(
            "  cost_output_tokens      : {}",
            evidence.usage.output_tokens
        );
        println!(
            "  cost_cache_read_tokens  : {}",
            evidence.usage.cache_read_tokens
        );
        println!(
            "  cost_estimated_usd      : {}",
            evidence
                .estimated_cost_usd
                .map(|amount| format!("{amount:.8}"))
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!(
            "  cost_actual_usd         : {}",
            cost_reconciliation
                .as_ref()
                .and_then(|event| event.payload.get("actual_cost_usd"))
                .and_then(|value| value.as_f64())
                .map(|amount| format!("{amount:.8}"))
                .or_else(|| evidence
                    .actual_cost_usd
                    .map(|amount| format!("{amount:.8}")))
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!(
            "  cost_session_estimated_usd: {:.8}",
            evidence.session_estimated_cost_usd
        );
        println!(
            "  cost_reconciliation_event_id: {}",
            cost_reconciliation
                .as_ref()
                .map(|event| event.event_id.0.as_str())
                .unwrap_or("(none)")
        );
        println!(
            "  cost_reconciliation_hash: {}",
            cost_reconciliation
                .as_ref()
                .and_then(|event| event.payload.get("reconciliation_hash"))
                .and_then(|value| value.as_str())
                .unwrap_or("(none)")
        );
        println!(
            "  cost_rollup_event_id    : {}",
            evidence.rollup_event_id.as_deref().unwrap_or("(none)")
        );
        println!(
            "  cost_evidence_hash      : {}",
            proof
                .cost_evidence_hash
                .as_deref()
                .unwrap_or(evidence.evidence_hash.as_str())
        );
    } else {
        println!("  cost_evidence           : (none)");
    }
    if let Some(evidence) = proof.runtime_memory_evidence.as_ref() {
        println!("  runtime_memory_schema   : {}", evidence.schema);
        println!(
            "  runtime_memory_bytes    : {}",
            evidence.memory_context_bytes
        );
        println!(
            "  runtime_memory_context_hash: {}",
            evidence.memory_context_hash
        );
        println!(
            "  runtime_memory_fenced   : {}",
            bool_text(evidence.fenced_context)
        );
        println!(
            "  runtime_memory_evidence_hash: {}",
            proof
                .runtime_memory_evidence_hash
                .as_deref()
                .unwrap_or(evidence.evidence_hash.as_str())
        );
        println!(
            "  runtime_memory_trace_match: {}",
            bool_text(runtime_memory_trace_match)
        );
    } else {
        println!("  runtime_memory_evidence : (none)");
        println!("  runtime_memory_trace_match: no");
    }
    println!("  tokens_in               : {}", proof.tokens_in);
    println!("  tokens_out              : {}", proof.tokens_out);
    println!("  tool_call_count         : {}", proof.tool_call_count);
    println!("  tool_receipt_count     : {}", proof.tool_receipt_count);
    println!(
        "  tool_receipt_join_found: {}",
        bool_text(tool_receipt_join_trace.all_found)
    );
    println!(
        "  tool_receipt_join_proof: {}",
        bool_text(tool_receipt_join_trace.all_point_to_proof)
    );
    println!(
        "  tool_receipt_join_hash : {}",
        bool_text(tool_receipt_join_trace.all_hash_match)
    );
    println!("  proof_hash              : {}", proof.proof_hash);
    println!("  proof_hash_verified     : {}", bool_text(proof_hash_ok));
    Ok(())
}

fn cmd_turn_reconcile_cost(args: &[String]) -> Result<(), CliError> {
    let event_id = args.get(3).ok_or_else(|| {
        CliError::Usage(
            "zaion turn reconcile-cost <event-id> [--actual-cost USD | --provider-generation-id ID] [--source provider_generation_api|provider_cost_api] [--pid <pid>]".into(),
        )
    })?;
    let cfg = ZaionConfig::load();
    let pid = arg_value(args, "--pid")
        .map(|pid| pid.to_string())
        .map(Ok)
        .unwrap_or_else(|| crate::commands::process::resolve_existing_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let proof_event = find_turn_proof_event(&ledger, event_id)?;
    let proof = decode_turn_proof(&proof_event)?;
    if proof.principal_id != pid {
        return Err(CliError::Usage(format!(
            "turn proof {} belongs to principal {}, not {}",
            proof_event.event_id.0, proof.principal_id, pid
        )));
    }
    let evidence = proof
        .cost_evidence
        .as_ref()
        .ok_or_else(|| CliError::Usage("turn proof has no cost_evidence to reconcile".into()))?;
    let original_cost_evidence_hash = proof
        .cost_evidence_hash
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(evidence.evidence_hash.as_str());
    let rollup_event = find_cost_rollup_event(&ledger, evidence, original_cost_evidence_hash)?;

    let provider_generation_id = arg_value(args, "--provider-generation-id").map(str::to_string);
    let provider_cost_id = arg_value(args, "--provider-cost-id").map(str::to_string);
    let source = arg_value(args, "--source").unwrap_or(if provider_generation_id.is_some() {
        "provider_generation_api"
    } else {
        "provider_cost_api"
    });
    validate_cost_reconciliation_source(source)?;
    let actual_cost_usd = match arg_value(args, "--actual-cost") {
        Some(_) => parse_f64_flag(args, "--actual-cost")?,
        None => fetch_reconciled_actual_cost(
            args,
            &cfg,
            evidence,
            provider_generation_id.as_deref(),
            source,
        )?,
    };
    if !actual_cost_usd.is_finite() || actual_cost_usd < 0.0 {
        return Err(CliError::Usage(
            "actual cost must be a non-negative finite number".into(),
        ));
    }
    let pricing_version = arg_value(args, "--pricing-version")
        .map(str::to_string)
        .or_else(|| evidence.pricing_version.clone());
    let session_actual_cost_usd = Some(actual_cost_usd);
    let mut payload = serde_json::json!({
        "schema": COST_RECONCILIATION_EVENT,
        "principal_id": proof.principal_id,
        "channel_id": proof.channel_id,
        "thread_id": proof.thread_id,
        "provider": evidence.provider,
        "model": evidence.model,
        "billing_provider": evidence.billing_provider,
        "billing_mode": "official_actual_reconciled",
        "usage": evidence.usage,
        "cost_status": "actual",
        "cost_source": source,
        "estimated_cost_usd": evidence.estimated_cost_usd,
        "actual_cost_usd": actual_cost_usd,
        "session_estimated_cost_usd": evidence.session_estimated_cost_usd,
        "session_actual_cost_usd": session_actual_cost_usd,
        "pricing_version": pricing_version,
        "original_cost_evidence_hash": original_cost_evidence_hash,
        "original_rollup_event_id": rollup_event.event_id.0,
        "proof_event_id": proof_event.event_id.0,
        "output_event_id": proof.output_event_id,
        "provider_generation_id": provider_generation_id,
        "provider_cost_id": provider_cost_id,
    });
    let reconciliation_hash = stable_hash_json(&payload);
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "reconciliation_hash".to_string(),
            serde_json::Value::String(reconciliation_hash.clone()),
        );
    }

    let reconciliation_event_id = ledger.append_signed_event_with_parent(
        &kp,
        &rollup_event.namespace_key,
        COST_RECONCILIATION_EVENT,
        payload,
        None,
        Some(&rollup_event.event_id),
    )?;

    println!("cost reconciliation");
    println!("  event_id          : {}", reconciliation_event_id.0);
    println!("  proof_event_id    : {}", proof_event.event_id.0);
    println!("  rollup_event_id   : {}", rollup_event.event_id.0);
    println!("  cost_status       : actual");
    println!("  cost_source       : {}", source);
    println!("  actual_cost_usd   : {:.8}", actual_cost_usd);
    println!("  reconciliation_hash: {}", reconciliation_hash);
    Ok(())
}

fn load_turn_ledger(args: &[String]) -> Result<(zaion_ledger::EventLedger, String), CliError> {
    let cfg = ZaionConfig::load();
    let pid = arg_value(args, "--pid")
        .map(|pid| pid.to_string())
        .map(Ok)
        .unwrap_or_else(|| crate::commands::process::resolve_existing_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    Ok((zaion_ledger::EventLedger::new(store.ledger_path(&pid)), pid))
}

fn latest_turn_proof_event(
    ledger: &zaion_ledger::EventLedger,
) -> Result<zaion_types::event::LedgerEvent, CliError> {
    ledger
        .list_global_events(256)?
        .into_iter()
        .find(|event| event.event_type == "turn.proof")
        .ok_or_else(|| CliError::Usage("no turn.proof event found for this process".into()))
}

fn find_turn_proof_event(
    ledger: &zaion_ledger::EventLedger,
    event_id: &str,
) -> Result<zaion_types::event::LedgerEvent, CliError> {
    if let Some(event) = ledger.get_event(event_id)? {
        if event.event_type == "turn.proof" {
            return Ok(event);
        }
    }

    ledger
        .list_global_events(512)?
        .into_iter()
        .find(|event| {
            if event.event_type != "turn.proof" {
                return false;
            }
            event.payload.get("user_event_id").and_then(|v| v.as_str()) == Some(event_id)
                || event
                    .payload
                    .get("output_event_id")
                    .and_then(|v| v.as_str())
                    == Some(event_id)
        })
        .ok_or_else(|| {
            CliError::Usage(format!(
                "no turn.proof event found for event id {}",
                event_id
            ))
        })
}

fn find_omni_route_event(
    ledger: &zaion_ledger::EventLedger,
    proof: &TurnProof,
) -> Result<Option<zaion_types::event::LedgerEvent>, CliError> {
    Ok(ledger.list_global_events(512)?.into_iter().find(|event| {
        event.event_type == "omni.route"
            && event
                .parent_event_id
                .as_ref()
                .map(|parent| parent.0.as_str())
                == Some(proof.user_event_id.as_str())
            && event
                .payload
                .get("principal_id")
                .and_then(|value| value.as_str())
                == Some(proof.principal_id.as_str())
            && event
                .payload
                .get("channel_id")
                .and_then(|value| value.as_str())
                == Some(proof.channel_id.as_str())
            && event
                .payload
                .get("thread_id")
                .and_then(|value| value.as_str())
                == Some(proof.thread_id.as_str())
            && proof
                .omni_route_event_id
                .as_deref()
                .map(|expected| expected == event.event_id.0)
                .unwrap_or(true)
    }))
}

fn decode_turn_proof(event: &zaion_types::event::LedgerEvent) -> Result<TurnProof, CliError> {
    serde_json::from_value(event.payload.clone()).map_err(|e| {
        CliError::Usage(format!(
            "event {} is not a valid turn.proof payload: {}",
            event.event_id.0, e
        ))
    })
}

struct ToolReceiptJoinTrace {
    all_found: bool,
    all_point_to_proof: bool,
    all_hash_match: bool,
}

fn trace_tool_receipt_joins(
    ledger: &zaion_ledger::EventLedger,
    proof_event: &zaion_types::event::LedgerEvent,
    proof: &TurnProof,
) -> Result<ToolReceiptJoinTrace, CliError> {
    if proof.tool_receipt_ids.is_empty() {
        return Ok(ToolReceiptJoinTrace {
            all_found: false,
            all_point_to_proof: false,
            all_hash_match: false,
        });
    }

    let session_key = zaion_types::session::SessionKey(proof.namespace_key.clone());
    let mut all_found = true;
    let mut all_point_to_proof = true;
    let mut all_hash_match = true;
    for receipt_id in &proof.tool_receipt_ids {
        let join = ledger
            .list_events_by_payload_string_array_contains(
                &session_key,
                "tool.receipt.proof_join",
                "tool_receipt_ids",
                receipt_id,
                1,
            )?
            .into_iter()
            .next();
        let Some(join) = join else {
            all_found = false;
            all_point_to_proof = false;
            all_hash_match = false;
            continue;
        };
        if join
            .payload
            .get("turn_proof_event_id")
            .and_then(|value| value.as_str())
            != Some(proof_event.event_id.0.as_str())
        {
            all_point_to_proof = false;
        }
        if join
            .payload
            .get("turn_proof_hash")
            .and_then(|value| value.as_str())
            != Some(proof.proof_hash.as_str())
        {
            all_hash_match = false;
        }
    }
    Ok(ToolReceiptJoinTrace {
        all_found,
        all_point_to_proof,
        all_hash_match,
    })
}

fn parse_f64_flag(args: &[String], flag: &str) -> Result<f64, CliError> {
    let raw = arg_value(args, flag)
        .ok_or_else(|| CliError::Usage(format!("missing required flag {}", flag)))?;
    raw.parse::<f64>()
        .map_err(|_| CliError::Usage(format!("{} must be a number, got {}", flag, raw)))
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn validate_cost_reconciliation_source(source: &str) -> Result<(), CliError> {
    match source {
        "provider_cost_api" | "provider_generation_api" | "user_override" | "custom_contract" => {
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unsupported cost reconciliation source '{}'. Use provider_cost_api, provider_generation_api, user_override, or custom_contract",
            other
        ))),
    }
}

fn fetch_reconciled_actual_cost(
    args: &[String],
    cfg: &ZaionConfig,
    evidence: &zaion_runtime::TurnCostEvidence,
    provider_generation_id: Option<&str>,
    source: &str,
) -> Result<f64, CliError> {
    match source {
        "provider_generation_api" => {
            let generation_id = provider_generation_id.ok_or_else(|| {
                CliError::Usage(
                    "missing --actual-cost; pass --provider-generation-id for provider_generation_api reconciliation".into(),
                )
            })?;
            fetch_provider_generation_cost(args, cfg, evidence, generation_id)
        }
        "provider_cost_api" => Err(CliError::Usage(
            "provider_cost_api reconciliation requires --actual-cost until a provider cost API adapter is configured".into(),
        )),
        "user_override" | "custom_contract" => Err(CliError::Usage(format!(
            "{} reconciliation requires explicit --actual-cost",
            source
        ))),
        other => Err(CliError::Usage(format!(
            "unsupported cost reconciliation source '{}'",
            other
        ))),
    }
}

fn fetch_provider_generation_cost(
    args: &[String],
    cfg: &ZaionConfig,
    evidence: &zaion_runtime::TurnCostEvidence,
    generation_id: &str,
) -> Result<f64, CliError> {
    let provider = crate::commands::provider::normalize_provider_name(&evidence.billing_provider);
    let base_url = arg_value(args, "--base-url")
        .map(str::to_string)
        .unwrap_or_else(|| crate::commands::provider::resolved_base_url(&provider, cfg));
    let api_key = arg_value(args, "--api-key")
        .map(str::to_string)
        .unwrap_or_else(|| crate::commands::provider::resolved_api_key(&provider, cfg));
    let url = provider_generation_url(&provider, &base_url, generation_id)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|error| CliError::Usage(format!("provider generation client failed: {error}")))?;
    let mut request = client.get(&url);
    if !api_key.trim().is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request
        .send()
        .map_err(|error| CliError::Usage(format!("provider generation fetch failed: {error}")))?;
    let status = response.status();
    let body = response.text().map_err(|error| {
        CliError::Usage(format!("provider generation response read failed: {error}"))
    })?;
    if !status.is_success() {
        return Err(CliError::Usage(format!(
            "provider generation fetch failed with HTTP {} {}",
            status.as_u16(),
            crate::commands::truncate_str(body.trim(), 120)
        )));
    }
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
        CliError::Usage(format!(
            "provider generation response was not JSON: {error}"
        ))
    })?;
    extract_generation_total_cost(&json).ok_or_else(|| {
        CliError::Usage(
            "provider generation response did not include total_cost or data.total_cost".into(),
        )
    })
}

fn provider_generation_url(
    provider: &str,
    base_url: &str,
    generation_id: &str,
) -> Result<String, CliError> {
    let generation_id = generation_id.trim();
    if generation_id.is_empty() {
        return Err(CliError::Usage(
            "--provider-generation-id must not be empty".into(),
        ));
    }
    let base = base_url.trim_end_matches('/');
    match provider {
        "openrouter" => Ok(format!("{}/generation?id={}", base, generation_id)),
        _ => Ok(format!("{}/generation?id={}", base, generation_id)),
    }
}

fn extract_generation_total_cost(json: &serde_json::Value) -> Option<f64> {
    for path in [
        &["total_cost"][..],
        &["data", "total_cost"][..],
        &["generation", "total_cost"][..],
        &["cost", "total_cost"][..],
    ] {
        let mut value = json;
        let mut found = true;
        for key in path {
            match value.get(*key) {
                Some(next) => value = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        if let Some(amount) = value.as_f64() {
            return Some(amount);
        }
        if let Some(text) = value.as_str() {
            if let Ok(amount) = text.parse::<f64>() {
                return Some(amount);
            }
        }
    }
    None
}

fn find_cost_rollup_event(
    ledger: &zaion_ledger::EventLedger,
    evidence: &zaion_runtime::TurnCostEvidence,
    cost_evidence_hash: &str,
) -> Result<zaion_types::event::LedgerEvent, CliError> {
    if let Some(event_id) = evidence.rollup_event_id.as_deref() {
        if let Some(event) = ledger.get_event(event_id)? {
            if event.event_type == "zaion.usage_cost.rollup.v1" {
                return Ok(event);
            }
        }
    }

    ledger
        .list_global_events(512)?
        .into_iter()
        .find(|event| {
            event.event_type == "zaion.usage_cost.rollup.v1"
                && event
                    .payload
                    .get("cost_evidence_hash")
                    .and_then(|value| value.as_str())
                    == Some(cost_evidence_hash)
        })
        .ok_or_else(|| {
            CliError::Usage(format!(
                "no zaion.usage_cost.rollup.v1 event found for cost evidence hash {}",
                cost_evidence_hash
            ))
        })
}

fn latest_cost_reconciliation_event(
    ledger: &zaion_ledger::EventLedger,
    proof: &TurnProof,
) -> Result<Option<zaion_types::event::LedgerEvent>, CliError> {
    let Some(evidence) = proof.cost_evidence.as_ref() else {
        return Ok(None);
    };
    let cost_evidence_hash = proof
        .cost_evidence_hash
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(evidence.evidence_hash.as_str());

    Ok(ledger.list_global_events(512)?.into_iter().find(|event| {
        event.event_type == COST_RECONCILIATION_EVENT
            && event
                .payload
                .get("original_cost_evidence_hash")
                .and_then(|value| value.as_str())
                == Some(cost_evidence_hash)
            && event
                .payload
                .get("output_event_id")
                .and_then(|value| value.as_str())
                == Some(proof.output_event_id.as_str())
    }))
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn runtime_memory_trace_matches(
    answer_trace: Option<&zaion_types::event::LedgerEvent>,
    proof: &TurnProof,
) -> bool {
    let Some(evidence) = proof.runtime_memory_evidence.as_ref() else {
        return false;
    };
    let expected_hash = proof
        .runtime_memory_evidence_hash
        .as_deref()
        .unwrap_or(evidence.evidence_hash.as_str());
    let Some(answer_trace) = answer_trace.filter(|event| event.event_type == "answer.trace") else {
        return false;
    };
    if answer_trace
        .payload
        .get("runtime_memory_evidence_hash")
        .and_then(|value| value.as_str())
        != Some(expected_hash)
    {
        return false;
    }
    let Ok(proof_evidence) = serde_json::to_value(evidence) else {
        return false;
    };
    answer_trace.payload.get("runtime_memory_evidence") == Some(&proof_evidence)
}

fn omni_authority_matches(
    proof: &TurnProof,
    omni_route: Option<&zaion_types::event::LedgerEvent>,
) -> bool {
    let Some(expected) = proof.omni_route_authority_hash.as_deref() else {
        return false;
    };
    let Some(route) = omni_route else {
        return false;
    };
    route
        .payload
        .get("authority_hash")
        .and_then(|value| value.as_str())
        == Some(expected)
        && route
            .payload
            .get("authority")
            .and_then(|value| value.as_str())
            == Some("OmniSessionManager")
}

fn replay_omni_session_graph(
    ledger: &zaion_ledger::EventLedger,
    proof: &TurnProof,
) -> Option<zaion_runtime::OmniSessionGraphReplay> {
    let target_route_id = proof.omni_route_event_id.as_deref()?;
    let events = ledger.list_global_events(512).ok()?;
    let mut route_events = events
        .iter()
        .filter(|event| {
            event.event_type == "omni.route"
                && event
                    .payload
                    .get("principal_id")
                    .and_then(|value| value.as_str())
                    == Some(proof.principal_id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    route_events.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let target_index = route_events
        .iter()
        .position(|event| event.event_id.0 == target_route_id)?;
    route_events.truncate(target_index + 1);

    let mut manager = OmniSessionManager::new(128_000);
    manager
        .replay_signed_route_events(&route_events, Some(proof.principal_id.as_str()))
        .ok()
}

fn omni_graph_replay_matches(
    proof: &TurnProof,
    omni_route: Option<&zaion_types::event::LedgerEvent>,
    replay: Option<&zaion_runtime::OmniSessionGraphReplay>,
) -> bool {
    let (Some(route), Some(replay)) = (omni_route, replay) else {
        return false;
    };
    if replay.principal_id != proof.principal_id {
        return false;
    }
    if replay.last_route_event_id.as_deref() != proof.omni_route_event_id.as_deref() {
        return false;
    }
    let Some(route_graph_hash) = route
        .payload
        .get("session_graph_hash")
        .and_then(|value| value.as_str())
    else {
        return false;
    };
    replay.replay_hash == route_graph_hash
}

fn memory_atoms_active_status(pid: &str, ids: &[String]) -> String {
    if ids.is_empty() {
        return "(none)".to_string();
    }

    let store = crate::commands::memory_atoms::MemoryAtomStore::load_for_pid(pid);
    let mut missing = Vec::new();
    let mut inactive = Vec::new();
    for id in ids {
        match store.find(id) {
            Some(atom) if atom.valid_until.is_none() => {}
            Some(_) => inactive.push(id.clone()),
            None => missing.push(id.clone()),
        }
    }

    if missing.is_empty() && inactive.is_empty() {
        "yes".to_string()
    } else {
        format!(
            "no (missing={}, inactive={})",
            missing.len(),
            inactive.len()
        )
    }
}

fn verify_proof_hash(proof: &TurnProof) -> bool {
    let mut normalized = proof.clone();
    let expected = normalized.proof_hash.clone();
    normalized.proof_hash.clear();
    stable_hash_json(&normalized) == expected
}
