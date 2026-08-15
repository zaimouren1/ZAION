//! Diagnostic tool handlers: capability/surface status, ledger trace, receipt
//! trace and the turn-proof hash verification helpers.

use std::collections::VecDeque;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::shell::ALLOWED_COMMANDS;
use super::{read_toml_file_safe, sha256_hex, zaion_data_dir_path, zaion_home_path};
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn capability_status_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let home = zaion_home_path();
    let mcp_path = home.as_ref().map(|path| path.join("mcp.toml"));
    Ok(json!({
        "schema": "zaion.capability_status.v1",
        "callable_tools": [
            {
                "name": "fs_read",
                "class": "read",
                "scope": "workspace",
                "limit": "single file <= 50KB"
            },
            {
                "name": "fs_list",
                "class": "read",
                "scope": "workspace",
                "limit": "directory <= 200 entries"
            },
            {
                "name": "fs_search",
                "class": "read",
                "scope": "workspace",
                "limit": "recursive text search, max_results <= 100"
            },
            {
                "name": "memory_search",
                "class": "memory",
                "scope": "ZAION_HOME / ZAION_DATA_DIR",
                "limit": "MemoryAtom evidence first, raw state fallback labelled"
            },
            {
                "name": "shell_exec",
                "class": "execute",
                "scope": "workspace",
                "limit": format!("allow_list={}", ALLOWED_COMMANDS.join(","))
            },
            {
                "name": "capability_status",
                "class": "diagnostic",
                "scope": "safe local metadata",
                "limit": "no secrets"
            },
            {
                "name": "surface_status",
                "class": "diagnostic",
                "scope": "safe local config and runtime surface status",
                "limit": "no secrets"
            },
            {
                "name": "ledger_recent",
                "class": "diagnostic",
                "scope": "recent local ledger metadata",
                "limit": "event metadata only by default"
            },
            {
                "name": "tool_receipt_trace",
                "class": "diagnostic",
                "scope": "local process ledger receipt/proof trace",
                "limit": "compact receipt, join, and proof hash status only"
            }
        ],
        "surfaces_are_not_tools": {
            "terminal_cli": "entry surface for commands such as zaion chat/wake/start",
            "tui": "interactive terminal surface backed by wake stream events",
            "telegram": "channel adapter surface backed by the same wake runtime",
            "http": "gateway/API/WebUI surface",
            "mcp": "external tool extension surface; configured servers add callable tools",
            "memory": "state and evidence subsystem exposed through memory_search and runtime context",
            "context": "prompt/context construction subsystem, not a direct unconstrained tool",
            "ledger": "signed provenance store exposed through ledger_recent and proof events"
        },
        "mcp_config": mcp_path.as_ref().map(|path| path.display().to_string()),
        "mcp_config_exists": mcp_path.as_ref().is_some_and(|path| path.exists()),
    }))
}

pub(super) fn surface_status_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let home = zaion_home_path();
    let data_dir = zaion_data_dir_path();
    let config = home
        .as_ref()
        .map(|path| read_toml_file_safe(&path.join("config.toml")))
        .unwrap_or_else(|| json!({"exists": false}));
    let channels = home
        .as_ref()
        .map(|path| read_toml_file_safe(&path.join("channels.toml")))
        .unwrap_or_else(|| json!({"exists": false}));
    let mcp = home
        .as_ref()
        .map(|path| read_toml_file_safe(&path.join("mcp.toml")))
        .unwrap_or_else(|| json!({"exists": false}));
    let daemon_pid_file = data_dir.as_ref().map(|path| path.join("daemon.pid"));
    let daemon_pid = daemon_pid_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());

    Ok(json!({
        "schema": "zaion.surface_status.v1",
        "zaion_home": home.as_ref().map(|path| path.display().to_string()),
        "zaion_data_dir": data_dir.as_ref().map(|path| path.display().to_string()),
        "surfaces": {
            "terminal_cli": {
                "state": "enabled",
                "entry": "zaion chat / zaion wake / zaion start"
            },
            "tui": {
                "state": "enabled when identity and provider are ready",
                "entry": "zaion or zaion tui",
                "reply_source": "StreamEvent::Token plus final-text fallback"
            },
            "telegram": {
                "state": "configured when token/channel secret exists",
                "entry": "zaion start or zaion tg start",
                "reply_source": "same wake runtime as TUI"
            },
            "http": {
                "state": "gateway surface",
                "entry": "zaion dashboard / zaion gateway start"
            },
            "mcp": {
                "state": "extends callable tools when enabled stdio servers are configured",
                "config": mcp
            },
            "memory": {
                "state": "queried through memory_search and runtime memory context"
            },
            "context": {
                "state": "compiled into prompt; audited through operation/context events"
            },
            "ledger": {
                "state": "signed provenance; inspect with ledger_recent"
            }
        },
        "config": config,
        "channels": channels,
        "daemon": {
            "pid_file": daemon_pid_file.as_ref().map(|path| path.display().to_string()),
            "pid": daemon_pid,
        }
    }))
}

pub(super) fn ledger_recent_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = input["limit"].as_u64().unwrap_or(10).clamp(1, 50) as usize;
    let data_dir =
        zaion_data_dir_path().ok_or_else(|| "ZAION_DATA_DIR is not available".to_string())?;
    let mut files = Vec::new();
    let mut queue = VecDeque::from([data_dir.clone()]);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if queue.len() < 128 {
                    queue.push_back(path);
                }
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower.contains("ledger") || lower.ends_with(".jsonl") || lower.ends_with(".sqlite") {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                files.push((modified, path, metadata.len()));
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let results = files
        .into_iter()
        .take(limit)
        .map(|(modified_unix, path, bytes)| {
            json!({
                "path": path.display().to_string(),
                "bytes": bytes,
                "modified_unix": modified_unix,
                "sha256": std::fs::read(&path).ok().map(|bytes| sha256_hex(&bytes)),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "schema": "zaion.ledger_recent.v1",
        "data_dir": data_dir.display().to_string(),
        "limit": limit,
        "files": results,
        "note": "This diagnostic returns ledger-like file metadata and hashes, not secret payload expansion."
    }))
}

pub(super) fn tool_receipt_trace_handler(
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let pid = input["pid"]
        .as_str()
        .ok_or_else(|| "missing 'pid' parameter".to_string())?;
    validate_process_id_segment(pid)?;
    let receipt_event_id = input["receipt_event_id"]
        .as_str()
        .ok_or_else(|| "missing 'receipt_event_id' parameter".to_string())?;
    let data_dir =
        zaion_data_dir_path().ok_or_else(|| "ZAION_DATA_DIR is not available".to_string())?;
    tool_receipt_trace_from_data_dir(&data_dir, pid, receipt_event_id)
}

pub(super) fn tool_receipt_trace_from_data_dir(
    data_dir: &Path,
    pid: &str,
    receipt_event_id: &str,
) -> Result<serde_json::Value, String> {
    validate_process_id_segment(pid)?;
    let ledger = zaion_ledger::EventLedger::new(data_dir.join(pid).join("ledger.db"));
    let receipt = ledger
        .get_event(receipt_event_id)
        .map_err(|error| error.to_string())?
        .filter(|event| event.event_type == "tool.receipt")
        .ok_or_else(|| format!("no tool.receipt event found for {}", receipt_event_id))?;
    let join = ledger
        .list_events_by_payload_string_array_contains(
            &zaion_types::session::SessionKey(receipt.namespace_key.0.clone()),
            "tool.receipt.proof_join",
            "tool_receipt_ids",
            &receipt.event_id.0,
            1,
        )
        .map_err(|error| error.to_string())?
        .into_iter()
        .next();
    let proof_event = join
        .as_ref()
        .and_then(|event| {
            event
                .payload
                .get("turn_proof_event_id")
                .and_then(|value| value.as_str())
        })
        .and_then(|event_id| ledger.get_event(event_id).ok().flatten())
        .filter(|event| event.event_type == "turn.proof");
    let proof_hash_verified = proof_event
        .as_ref()
        .map(|event| verify_turn_proof_hash_value(&event.payload))
        .unwrap_or(false);
    let proof_hash_matches_join = join
        .as_ref()
        .and_then(|event| event.payload.get("turn_proof_hash"))
        .and_then(|value| value.as_str())
        == proof_event
            .as_ref()
            .and_then(|event| event.payload.get("proof_hash"))
            .and_then(|value| value.as_str());

    Ok(json!({
        "schema": "zaion.tool_receipt_trace.v1",
        "runtime_scope": if proof_event.is_some() { "turn_runtime" } else { "receipt_only_or_unjoined" },
        "pid": pid,
        "receipt_event_id": receipt.event_id.0,
        "receipt_tool": receipt.payload.get("tool_name").and_then(|value| value.as_str()).unwrap_or("(unknown)"),
        "receipt_status": receipt.payload.get("receipt_status").and_then(|value| value.as_str()).unwrap_or("(unknown)"),
        "join_found": join.is_some(),
        "join_event_id": join.as_ref().map(|event| event.event_id.0.as_str()),
        "join_hash": join.as_ref().and_then(|event| event.payload.get("join_hash")).and_then(|value| value.as_str()),
        "proof_found": proof_event.is_some(),
        "proof_event_id": proof_event.as_ref().map(|event| event.event_id.0.as_str()),
        "turn_proof_hash": join
            .as_ref()
            .and_then(|event| event.payload.get("turn_proof_hash"))
            .and_then(|value| value.as_str())
            .or_else(|| proof_event.as_ref().and_then(|event| event.payload.get("proof_hash")).and_then(|value| value.as_str())),
        "proof_hash_matches_join": proof_hash_matches_join,
        "proof_hash_verified": proof_hash_verified,
    }))
}

fn validate_process_id_segment(pid: &str) -> Result<(), String> {
    if pid.trim().is_empty() {
        return Err("invalid pid: must not be empty".to_string());
    }
    let path = Path::new(pid);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("invalid pid: must be a single process directory name".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnContextLayerForHash {
    layer: u8,
    label: String,
    token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnCanonicalUsageEvidenceForHash {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
    request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnCompressionEvidenceForHash {
    schema: String,
    compression_requested: bool,
    was_compressed: bool,
    original_turns: usize,
    compressed_turns: usize,
    turns_pruned: usize,
    original_tokens: usize,
    compressed_tokens: usize,
    token_budget: usize,
    trigger_threshold: usize,
    summary_hash: String,
    #[serde(default)]
    summary_strategy: String,
    #[serde(default)]
    pruned_tool_outputs: usize,
    #[serde(default)]
    protected_head_turns: usize,
    #[serde(default)]
    protected_tail_turns: usize,
    #[serde(default)]
    protected_tail_tokens: usize,
    #[serde(default)]
    summary_budget_tokens: usize,
    evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnCostEvidenceForHash {
    schema: String,
    provider: String,
    model: String,
    billing_provider: String,
    billing_mode: String,
    usage: TurnCanonicalUsageEvidenceForHash,
    cost_status: String,
    cost_source: String,
    estimated_cost_usd: Option<f64>,
    #[serde(default)]
    actual_cost_usd: Option<f64>,
    session_estimated_cost_usd: f64,
    #[serde(default)]
    session_actual_cost_usd: Option<f64>,
    #[serde(default)]
    pricing_version: Option<String>,
    #[serde(default)]
    rollup_event_id: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
    evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnRuntimeMemoryEvidenceForHash {
    schema: String,
    memory_enabled: bool,
    memory_context_bytes: usize,
    memory_context_hash: String,
    fenced_context: bool,
    evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnCapabilityManifestForHash {
    provider: String,
    model: String,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    memory_enabled: bool,
    mcp_enabled: bool,
    cache_enabled: bool,
    smart_route_enabled: bool,
    compression_requested: bool,
    tools_requested: Vec<String>,
    boundaries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TurnProofForHash {
    schema_version: u8,
    proof_id: String,
    principal_id: String,
    workspace_id: String,
    project_id: String,
    channel_id: String,
    thread_id: String,
    namespace_key: String,
    user_event_id: String,
    output_event_id: String,
    omni_route_event_id: Option<String>,
    omni_route_authority_hash: Option<String>,
    event_lineage: Vec<String>,
    identity_contract_hash: String,
    capability_manifest_hash: String,
    context_pack_id: Option<String>,
    context_digest: String,
    context_layers: Vec<TurnContextLayerForHash>,
    memory_atom_ids: Vec<String>,
    #[serde(default)]
    compression_evidence: Option<TurnCompressionEvidenceForHash>,
    #[serde(default)]
    compression_evidence_hash: Option<String>,
    #[serde(default)]
    cost_evidence: Option<TurnCostEvidenceForHash>,
    #[serde(default)]
    cost_evidence_hash: Option<String>,
    #[serde(default)]
    runtime_memory_evidence: Option<TurnRuntimeMemoryEvidenceForHash>,
    #[serde(default)]
    runtime_memory_evidence_hash: Option<String>,
    capability_manifest: TurnCapabilityManifestForHash,
    tokens_in: u32,
    tokens_out: u32,
    tool_call_count: usize,
    #[serde(default)]
    tool_receipt_ids: Vec<String>,
    #[serde(default)]
    tool_receipt_count: usize,
    proof_hash: String,
}

fn verify_turn_proof_hash_value(value: &serde_json::Value) -> bool {
    let Ok(mut proof) = serde_json::from_value::<TurnProofForHash>(value.clone()) else {
        return false;
    };
    let expected = proof.proof_hash.clone();
    proof.proof_hash.clear();
    stable_hash_json_value(&proof) == expected
}

pub(super) fn stable_hash_json_value<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    sha256_hex(&bytes)
}

/// Register the diagnostic tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "capability_status",
            "1.0",
            "Explain Zaion callable tools, configured MCP extension state, and the distinction between product surfaces and model tools.",
            McpSchema::new(vec![]),
            "diagnostic",
        ),
        capability_status_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "surface_status",
            "1.0",
            "Report safe local status for Zaion terminal, TUI, Telegram, HTTP, MCP, memory, context, and ledger surfaces.",
            McpSchema::new(vec![]),
            "diagnostic",
        ),
        surface_status_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "ledger_recent",
            "1.0",
            "List recent ledger-like local files with metadata and hashes for provenance diagnostics.",
            McpSchema::new(vec![McpParam::optional(
                "limit",
                McpParamType::Number,
                "maximum files to return (default 10, max 50)",
                json!(10),
            )]),
            "diagnostic",
        ),
        ledger_recent_handler,
    ));
    registry.register(McpTool::new(
        McpToolMeta::new(
            "tool_receipt_trace",
            "1.0",
            "Trace a local tool.receipt event to its signed receipt/proof join and verify the linked turn proof hash.",
            McpSchema::new(vec![
                McpParam::required(
                    "pid",
                    McpParamType::String,
                    "principal id whose local ledger contains the receipt",
                ),
                McpParam::required(
                    "receipt_event_id",
                    McpParamType::String,
                    "tool.receipt event id to trace",
                ),
            ]),
            "diagnostic",
        ),
        tool_receipt_trace_handler,
    ));
}
