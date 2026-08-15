use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use std::collections::BTreeSet;
use zaion_runtime::TurnProof;

const COST_RECONCILIATION_EVENT: &str = "zaion.usage_cost.reconciled.v1";

pub fn cmd_answer(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("trace");
    match sub {
        "trace" => cmd_answer_trace(args),
        other => Err(CliError::Usage(format!(
            "unknown answer subcommand: {}. Use: trace",
            other
        ))),
    }
}

fn cmd_answer_trace(args: &[String]) -> Result<(), CliError> {
    let event_id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion answer trace <event-id> [--pid <pid>]".into()))?;
    let (ledger, _) = load_ledger(args)?;
    let proof_event = find_turn_proof_event(&ledger, event_id)?;
    let proof: TurnProof = serde_json::from_value(proof_event.payload.clone()).map_err(|e| {
        CliError::Usage(format!(
            "event {} is not a valid turn.proof payload: {}",
            proof_event.event_id.0, e
        ))
    })?;
    let output_event = ledger.get_event(&proof.output_event_id)?.ok_or_else(|| {
        CliError::Usage(format!("output event not found: {}", proof.output_event_id))
    })?;
    let answer = output_event
        .payload
        .get("content")
        .or_else(|| output_event.payload.get("response"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let runtime_memory_trace_match = runtime_memory_trace_matches(&ledger, &proof_event, &proof);

    println!("answer trace");
    println!("  proof_event_id       : {}", proof_event.event_id.0);
    println!("  proof_id             : {}", proof.proof_id);
    println!("  principal_id         : {}", proof.principal_id);
    println!("  output_event_id      : {}", proof.output_event_id);
    println!(
        "  context_pack_id      : {}",
        proof.context_pack_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  memory_atom_ids      : {}",
        if proof.memory_atom_ids.is_empty() {
            "(none)".to_string()
        } else {
            proof.memory_atom_ids.join(",")
        }
    );
    if let Some(evidence) = proof.compression_evidence.as_ref() {
        println!("  compression_schema  : {}", evidence.schema);
        println!("  compression_applied : {}", evidence.was_compressed);
        println!("  compression_turns_pruned: {}", evidence.turns_pruned);
        println!(
            "  compression_summary_strategy: {}",
            evidence.summary_strategy
        );
        println!(
            "  compression_pruned_tool_outputs: {}",
            evidence.pruned_tool_outputs
        );
        println!(
            "  compression_protected_tail_turns: {}",
            evidence.protected_tail_turns
        );
        println!(
            "  compression_evidence_hash: {}",
            proof
                .compression_evidence_hash
                .as_deref()
                .unwrap_or(evidence.evidence_hash.as_str())
        );
    } else {
        println!("  compression_evidence: (none)");
    }
    if let Some(evidence) = proof.cost_evidence.as_ref() {
        let cost_reconciliation = latest_cost_reconciliation_event(&ledger, &proof)?;
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
        println!("  cost_schema         : {}", evidence.schema);
        println!("  cost_status         : {}", reconciled_status);
        println!("  cost_source         : {}", reconciled_source);
        println!("  cost_provider       : {}", evidence.provider);
        println!("  cost_model          : {}", evidence.model);
        println!(
            "  cost_estimated_usd  : {}",
            evidence
                .estimated_cost_usd
                .map(|amount| format!("{amount:.8}"))
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!(
            "  cost_actual_usd     : {}",
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
            "  cost_evidence_hash  : {}",
            proof
                .cost_evidence_hash
                .as_deref()
                .unwrap_or(evidence.evidence_hash.as_str())
        );
    } else {
        println!("  cost_evidence      : (none)");
    }
    if let Some(evidence) = proof.runtime_memory_evidence.as_ref() {
        println!("  runtime_memory_schema: {}", evidence.schema);
        println!("  runtime_memory_bytes: {}", evidence.memory_context_bytes);
        println!(
            "  runtime_memory_context_hash: {}",
            evidence.memory_context_hash
        );
        println!("  runtime_memory_fenced: {}", evidence.fenced_context);
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
        println!("  runtime_memory_evidence: (none)");
        println!("  runtime_memory_trace_match: no");
    }
    let ledger_spans = proof_event
        .parent_event_id
        .as_ref()
        .and_then(|parent| ledger.get_event(&parent.0).ok().flatten())
        .and_then(|event| event.payload.get("answer_trace_spans").cloned())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    let context = proof
        .context_pack_id
        .as_deref()
        .and_then(crate::commands::context_packs::find_context_pack_manifest);
    let memory_store =
        crate::commands::memory_atoms::MemoryAtomStore::load_for_pid(&proof.principal_id);
    let spans = answer_spans(answer);
    println!("  spans               : {}", spans.len());

    for (idx, span) in spans.iter().enumerate() {
        let matched_memory = proof
            .memory_atom_ids
            .iter()
            .filter_map(|id| {
                memory_store
                    .find(id)
                    .filter(|atom| evidence_overlap(span, &atom.content) > 0)
                    .map(|_| id.clone())
            })
            .collect::<Vec<_>>();

        let matched_chunks = context
            .as_ref()
            .map(|(_, manifest)| {
                let mut scored = manifest
                    .chunks
                    .iter()
                    .filter_map(|chunk| {
                        let score = evidence_overlap(span, &chunk.content);
                        (score > 0).then_some((score, chunk))
                    })
                    .collect::<Vec<_>>();
                scored.sort_by(|a, b| b.0.cmp(&a.0));
                scored
                    .into_iter()
                    .take(3)
                    .map(|(_, chunk)| {
                        format!(
                            "L{}:{} [{}]",
                            chunk.layer,
                            chunk.label,
                            chunk.lineage.join(",")
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        println!("  span {}              : {}", idx + 1, span);
        println!(
            "    context_evidence  : {}",
            if matched_chunks.is_empty() {
                "(none)".to_string()
            } else {
                matched_chunks.join(" | ")
            }
        );
        println!(
            "    memory_evidence   : {}",
            if matched_memory.is_empty() {
                "(none)".to_string()
            } else {
                matched_memory.join(",")
            }
        );
        if let Some(ledger_span) = ledger_spans.get(idx) {
            println!(
                "    evidence_kind     : {}",
                ledger_span["evidence_kind"].as_str().unwrap_or("(unknown)")
            );
            println!(
                "    evidence_hash     : {}",
                ledger_span["evidence_hash"].as_str().unwrap_or("(missing)")
            );
        }
    }

    if let Some((pid, manifest)) = context {
        println!("  context_process     : {}", pid);
        println!("  context_chunks      : {}", manifest.chunks.len());
        println!("  context_tokens_used : {}", manifest.tokens_used);
    }
    Ok(())
}

fn runtime_memory_trace_matches(
    ledger: &zaion_ledger::EventLedger,
    proof_event: &zaion_types::event::LedgerEvent,
    proof: &TurnProof,
) -> bool {
    let Some(evidence) = proof.runtime_memory_evidence.as_ref() else {
        return false;
    };
    let expected_hash = proof
        .runtime_memory_evidence_hash
        .as_deref()
        .unwrap_or(evidence.evidence_hash.as_str());
    let Some(answer_trace) = proof_event
        .parent_event_id
        .as_ref()
        .and_then(|parent| ledger.get_event(&parent.0).ok().flatten())
        .filter(|event| event.event_type == "answer.trace")
    else {
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

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn load_ledger(args: &[String]) -> Result<(zaion_ledger::EventLedger, String), CliError> {
    let cfg = ZaionConfig::load();
    let pid = arg_value(args, "--pid")
        .map(|pid| pid.to_string())
        .map(Ok)
        .unwrap_or_else(|| crate::commands::process::resolve_existing_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    Ok((zaion_ledger::EventLedger::new(store.ledger_path(&pid)), pid))
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

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}
