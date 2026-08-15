//! `zaion agent` — ACP federation CLI for managing remote agent bindings
//! and spawning/monitoring remote runs.

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use sha2::{Digest, Sha256};
use zaion_types::identity::SignatureBytes;

/// `zaion agent` dispatcher.
pub fn cmd_agent(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();
    let pid = match args.get(3).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let registry_path = store.process_dir(&pid).join("agents.json");
    let mut registry = zaion_a2a::AgentRegistry::load(&registry_path);
    match sub {
        "proof" => return cmd_delegation_proof(args, &pid, &store),
        "receipts" => return cmd_delegation_receipts(&pid, &store),
        "receipt-trace" | "receipt_trace" => {
            return cmd_delegation_receipt_trace(args, &pid, &store)
        }
        "list" => {
            if registry.agents.is_empty() {
                println!(
                    "no bound agents for {}. use: zaion agent bind <pid> <name> <acp_url>",
                    pid
                );
            } else {
                println!("{:<20} ACP_URL", "NAME");
                println!("{}", "-".repeat(60));
                for a in &registry.agents {
                    println!("{:<20} {}", a.name, a.acp_url);
                }
            }
        }
        "bind" => {
            let name = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion agent bind <pid> <name> <acp_url>".into()))?;
            let acp_url = args
                .get(5)
                .ok_or_else(|| CliError::Usage("zaion agent bind <pid> <name> <acp_url>".into()))?;
            registry.bind(name, acp_url);
            registry
                .save(&registry_path)
                .map_err(|e: zaion_a2a::A2AError| CliError::Usage(e.to_string()))?;
            println!("agent '{}' bound to {}", name, acp_url);
        }
        "remove" => {
            let name = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion agent remove <pid> <name>".into()))?;
            if !registry.remove(name) {
                return Err(CliError::Usage(format!("agent '{}' not found", name)));
            }
            registry
                .save(&registry_path)
                .map_err(|e: zaion_a2a::A2AError| CliError::Usage(e.to_string()))?;
            println!("agent '{}' removed", name);
        }
        "spawn" => {
            // zaion agent spawn <pid> <name|url> <task>
            let name_or_url = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion agent spawn <pid> <name|url> <task>".into())
            })?;
            let task = args.get(5).ok_or_else(|| {
                CliError::Usage("zaion agent spawn <pid> <name|url> <task>".into())
            })?;
            let acp_url = if name_or_url.starts_with("http") {
                name_or_url.clone()
            } else {
                registry
                    .get(name_or_url)
                    .ok_or_else(|| CliError::Usage(format!("agent '{}' not bound", name_or_url)))?
                    .acp_url
                    .clone()
            };
            let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
            let client = zaion_a2a::AcpClient::new(&acp_url);
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| CliError::Usage(format!("tokio runtime init failed: {}", e)))?;
            let run = rt
                .block_on(client.spawn(task, Some(kp.principal_id().as_str())))
                .map_err(|e: zaion_a2a::A2AError| CliError::Usage(e.to_string()))?;
            println!("run created: {}", run.run_id);
            println!("  status : {}", run.status);
            println!("  task   : {}", run.task);
            println!("  url    : {}/v1/runs/{}", acp_url, run.run_id);
        }
        "status" => {
            let name_or_url = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion agent status <pid> <name|url> <run_id>".into())
            })?;
            let run_id = args.get(5).ok_or_else(|| {
                CliError::Usage("zaion agent status <pid> <name|url> <run_id>".into())
            })?;
            let acp_url = if name_or_url.starts_with("http") {
                name_or_url.clone()
            } else {
                registry
                    .get(name_or_url)
                    .ok_or_else(|| CliError::Usage(format!("agent '{}' not bound", name_or_url)))?
                    .acp_url
                    .clone()
            };
            let client = zaion_a2a::AcpClient::new(&acp_url);
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| CliError::Usage(format!("tokio runtime init failed: {}", e)))?;
            let run = rt
                .block_on(client.status(run_id))
                .map_err(|e: zaion_a2a::A2AError| CliError::Usage(e.to_string()))?;
            println!("run_id  : {}", run.run_id);
            println!("status  : {}", run.status);
            println!("task    : {}", run.task);
            if let Some(ref r) = run.result {
                println!("result  : {}", r);
            }
            if let Some(ref e) = run.error {
                println!("error   : {}", e);
            }
        }
        other => {
            return Err(CliError::Usage(format!(
            "unknown agent subcommand: {}. Use: list, bind, remove, spawn, status, proof, receipts, receipt-trace",
            other
        )))
        }
    }
    Ok(())
}

fn cmd_delegation_proof(
    args: &[String],
    pid: &str,
    store: &zaion_core::process::ProcessStore,
) -> Result<(), CliError> {
    let delegate_principal = args.get(4).ok_or_else(|| {
        CliError::Usage("zaion agent proof <pid> <delegate_principal> <task>".into())
    })?;
    let task = args.get(5).ok_or_else(|| {
        CliError::Usage("zaion agent proof <pid> <delegate_principal> <task>".into())
    })?;
    let scope = arg_value(args, "--scope").unwrap_or("read-only");
    let input = parse_json_or_string(arg_value(args, "--input").unwrap_or("{}"));
    let output = parse_json_or_string(arg_value(args, "--output").unwrap_or("{}"));
    let input_hash = value_hash(&input);
    let output_hash = value_hash(&output);
    let merge_receipt = merge_receipt_hash(
        pid,
        delegate_principal,
        task,
        scope,
        &input_hash,
        &output_hash,
    );
    let (_, kp) = store.load(pid).map_err(CliError::Core)?;
    let message = zaion_a2a::A2AMessage::new(
        &kp,
        delegate_principal,
        zaion_a2a::MessageType::Delegate,
        serde_json::json!({
            "task": task,
            "scope": scope,
            "input_hash": input_hash.clone(),
            "output_hash": output_hash.clone(),
            "merge_receipt": merge_receipt.clone(),
        }),
    );
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
    let ns = zaion_types::session::NamespaceKey(pid.to_string());
    let event_id = ledger.append_signed_event(
        &kp,
        &ns,
        "delegation.proof",
        serde_json::json!({
            "delegate_principal": delegate_principal,
            "task": task,
            "scope": scope,
            "input": input,
            "output": output,
            "input_hash": input_hash.clone(),
            "output_hash": output_hash.clone(),
            "message_id": message.message_id,
            "message_signature": message.signature_hex,
            "merge_receipt": merge_receipt.clone(),
        }),
        None,
    )?;

    println!("delegation proof");
    println!("  event_id          : {}", event_id.0);
    println!("  from_principal    : {}", kp.principal_id().as_str());
    println!("  delegate          : {}", delegate_principal);
    println!("  task              : {}", task);
    println!("  scope             : {}", scope);
    println!("  input_hash        : {}", input_hash);
    println!("  output_hash       : {}", output_hash);
    println!("  merge_receipt     : {}", merge_receipt);
    println!("  breakthrough      : delegated principal + scope + IO hashes + merge receipt");
    Ok(())
}

fn cmd_delegation_receipts(
    pid: &str,
    store: &zaion_core::process::ProcessStore,
) -> Result<(), CliError> {
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
    let events = ledger.list_global_events(100)?;
    let receipts = events
        .iter()
        .filter(|event| event.event_type == "delegation.proof")
        .collect::<Vec<_>>();
    println!("delegation receipts");
    println!("  principal : {}", pid);
    println!("  receipts  : {}", receipts.len());
    for event in receipts {
        let delegate = event
            .payload
            .get("delegate_principal")
            .and_then(|value| value.as_str())
            .unwrap_or("-");
        let scope = event
            .payload
            .get("scope")
            .and_then(|value| value.as_str())
            .unwrap_or("-");
        let merge = event
            .payload
            .get("merge_receipt")
            .and_then(|value| value.as_str())
            .unwrap_or("-");
        println!(
            "  {} delegate={} scope={} merge_receipt={}",
            event.event_id.0, delegate, scope, merge
        );
    }
    Ok(())
}

fn cmd_delegation_receipt_trace(
    args: &[String],
    pid: &str,
    store: &zaion_core::process::ProcessStore,
) -> Result<(), CliError> {
    let event_id = args.get(4).ok_or_else(|| {
        CliError::Usage("zaion agent receipt-trace <pid> <delegation-proof-event-id>".into())
    })?;
    let (_, kp) = store.load(pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
    let event = ledger
        .get_event(event_id)?
        .filter(|event| event.event_type == "delegation.proof")
        .ok_or_else(|| {
            CliError::Usage(format!("no delegation.proof event found for {}", event_id))
        })?;
    let delegate = event
        .payload
        .get("delegate_principal")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let task = event
        .payload
        .get("task")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let scope = event
        .payload
        .get("scope")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let input_hash = event
        .payload
        .get("input_hash")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let output_hash = event
        .payload
        .get("output_hash")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let stored_merge_receipt = event
        .payload
        .get("merge_receipt")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let expected_merge_receipt =
        merge_receipt_hash(pid, delegate, task, scope, input_hash, output_hash);
    let merge_receipt_verified = stored_merge_receipt == expected_merge_receipt;
    let message_signature_valid =
        verify_delegation_message_signature(&kp, &event.payload).unwrap_or(false);

    println!("delegation receipt trace");
    println!("  principal              : {}", pid);
    println!("  event_id               : {}", event.event_id.0);
    println!("  delegate               : {}", delegate);
    println!("  task                   : {}", task);
    println!("  scope                  : {}", scope);
    println!("  runtime_scope          : delegation_proof");
    println!(
        "  merge_receipt_verified : {}",
        bool_text(merge_receipt_verified)
    );
    println!(
        "  message_signature_valid: {}",
        bool_text(message_signature_valid)
    );
    println!("  merge_receipt          : {}", stored_merge_receipt);
    println!("  expected_merge_receipt : {}", expected_merge_receipt);
    Ok(())
}

fn parse_json_or_string(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::json!({ "text": value }))
}

fn value_hash(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(encoded.as_bytes());
    hex::encode(hasher.finalize())
}

fn merge_receipt_hash(
    pid: &str,
    delegate: &str,
    task: &str,
    scope: &str,
    input_hash: &str,
    output_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    for part in [pid, delegate, task, scope, input_hash, output_hash] {
        hasher.update(part.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn verify_delegation_message_signature(
    keypair: &zaion_crypto::keypair::ZaionKeypair,
    payload: &serde_json::Value,
) -> Result<bool, CliError> {
    let Some(message_id) = payload.get("message_id").and_then(|value| value.as_str()) else {
        return Ok(false);
    };
    let Some(to_principal) = payload
        .get("delegate_principal")
        .and_then(|value| value.as_str())
    else {
        return Ok(false);
    };
    let Some(signature_hex) = payload
        .get("message_signature")
        .and_then(|value| value.as_str())
    else {
        return Ok(false);
    };
    let signed_payload = serde_json::json!({
        "task": payload.get("task").cloned().unwrap_or(serde_json::Value::Null),
        "scope": payload.get("scope").cloned().unwrap_or(serde_json::Value::Null),
        "input_hash": payload.get("input_hash").cloned().unwrap_or(serde_json::Value::Null),
        "output_hash": payload.get("output_hash").cloned().unwrap_or(serde_json::Value::Null),
        "merge_receipt": payload.get("merge_receipt").cloned().unwrap_or(serde_json::Value::Null),
    });
    let content = format!("{}:{}:{}", message_id, to_principal, signed_payload);
    let signature = hex::decode(signature_hex)
        .map(SignatureBytes)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    Ok(zaion_crypto::verify::verify_signature(
        &keypair.public_key_bytes(),
        content.as_bytes(),
        &signature,
    )
    .is_ok())
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
