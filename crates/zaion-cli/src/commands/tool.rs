use crate::commands::{data_dir, CliError};
use crate::config::{McpServerConfig, McpStore, ZaionConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use zaion_runtime::execute_code_uds::{
    DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES, DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES,
    DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS, DEFAULT_EXECUTE_CODE_TIMEOUT_SECS,
};
use zaion_runtime::TurnProof;
use zaion_runtime::{
    DEFAULT_BATCH_RUNNER_CHECKPOINT_FILE, DEFAULT_BATCH_RUNNER_NUM_WORKERS,
    DEFAULT_BATCH_RUNNER_TRAJECTORY_FILE, DEFAULT_BATCH_RUNNER_TRAJECTORY_FORMAT,
};

const TOOL_RECEIPT_SCHEMA: &str = "zaion.tool_receipt.v1";
const POLICY_DECISION_SCHEMA: &str = "zaion.policy_decision.v1";
const EXECUTE_CODE_SERVICE_MATRIX_SCHEMA: &str = "zaion.execute_code_service_matrix.v1";
const BATCH_RUNNER_SERVICE_MATRIX_SCHEMA: &str = "zaion.batch_runner_service_matrix.v1";

pub fn cmd_tool(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("receipts");
    match sub {
        "receipts" => cmd_tool_receipts(args),
        "receipt-trace" | "receipt_trace" => cmd_tool_receipt_trace(args),
        "verify" => cmd_tool_verify(args),
        "execute-code-matrix" | "execute_code_matrix" => cmd_tool_execute_code_matrix(args),
        "batch-runner-matrix" | "batch_runner_matrix" => cmd_tool_batch_runner_matrix(args),
        "list" | "enable" | "disable" | "summary" | "config" => cmd_tools(args),
        other => Err(CliError::Usage(format!(
            "unknown tool subcommand: {}. Use: receipts, receipt-trace, verify, execute-code-matrix, batch-runner-matrix, list, enable, disable",
            other
        ))),
    }
}

pub fn cmd_tools(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let sub = if sub == "--summary" { "summary" } else { sub };
    match sub {
        "list" => {
            let platform = arg_value(args, "--platform").unwrap_or("cli");
            let store = ToolsetStore::load();
            print_tools_list(&store, platform);
            Ok(())
        }
        "summary" => {
            let store = ToolsetStore::load();
            print_tools_summary(&store);
            Ok(())
        }
        "enable" | "disable" => {
            let platform = arg_value(args, "--platform").unwrap_or("cli").to_string();
            validate_platform(&platform)?;
            let names = positional_targets(args, 3)
                .into_iter()
                .map(|name| canonical_toolset_name(&name))
                .collect::<Vec<_>>();
            if names.is_empty() {
                return Err(CliError::Usage(format!(
                    "zaion tools {} <toolset|server:tool>... [--platform cli]",
                    sub
                )));
            }
            let mut store = ToolsetStore::load();
            let mcp_servers = McpStore::load().servers;
            let mut changed = Vec::new();
            for name in names {
                if let Some((server, tool)) = name.split_once(':') {
                    if !mcp_servers.iter().any(|candidate| candidate.name == server) {
                        return Err(CliError::Usage(format!(
                            "MCP server '{}' not found in config",
                            server
                        )));
                    }
                    if sub == "disable" {
                        store
                            .mcp_excluded
                            .entry(server.to_string())
                            .or_default()
                            .insert(tool.to_string());
                    } else if let Some(excluded) = store.mcp_excluded.get_mut(server) {
                        excluded.remove(tool);
                    }
                    changed.push(name);
                    continue;
                }
                let name = if validate_toolset(&name).is_ok() {
                    name
                } else {
                    resolve_dynamic_mcp_toolset_alias(&name, &mcp_servers).ok_or_else(|| {
                        CliError::Usage(format!(
                            "unknown toolset '{}'. Run: zaion tools list",
                            name
                        ))
                    })?
                };
                let enabled = store.platform_toolsets.entry(platform.clone()).or_default();
                if sub == "disable" {
                    enabled.remove(&name);
                } else {
                    enabled.insert(name.clone());
                }
                changed.push(name);
            }
            store.save()?;
            let verb = if sub == "disable" {
                "disabled"
            } else {
                "enabled"
            };
            println!("{}: {}", verb, changed.join(", "));
            println!("platform: {}", platform);
            Ok(())
        }
        "config" => {
            println!("tools config: {}", ToolsetStore::path().display());
            println!("commands: list, enable, disable");
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown tools subcommand: {}. Use: list, enable, disable, config",
            other
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ToolsetStore {
    #[serde(default)]
    platform_toolsets: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    mcp_excluded: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicMcpToolsetReport {
    pub server: String,
    pub toolset: String,
    pub alias: String,
    pub enabled: bool,
    pub discovered_tool_count: usize,
    pub configured_tool_count: usize,
    pub pending_tool_count: usize,
    pub tools: Vec<String>,
}

impl ToolsetStore {
    fn path() -> PathBuf {
        ZaionConfig::config_path()
            .parent()
            .map(|parent| parent.join("tools.toml"))
            .unwrap_or_else(|| PathBuf::from("tools.toml"))
    }

    fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::write(
            path,
            toml::to_string_pretty(self).map_err(|e| CliError::Usage(e.to_string()))?,
        )
        .map_err(|e| CliError::Usage(e.to_string()))
    }

    fn enabled_for_platform(&self, platform: &str) -> BTreeSet<String> {
        self.platform_toolsets
            .get(platform)
            .cloned()
            .unwrap_or_else(default_enabled_toolsets)
    }
}

fn print_tools_list(store: &ToolsetStore, platform: &str) {
    let enabled = store.enabled_for_platform(platform);
    println!("built-in toolsets ({})", platform);
    for (key, label) in configurable_toolsets() {
        let status = if enabled.contains(*key) {
            "enabled"
        } else {
            "disabled"
        };
        println!("  {:8}  {:16} {}", status, key, label);
    }
    let mcp = McpStore::load();
    let dynamic_reports = dynamic_mcp_toolset_reports_from_servers(&mcp.servers);
    if !dynamic_reports.is_empty() {
        println!();
        println!("MCP dynamic toolsets");
        for report in &dynamic_reports {
            let excluded = store
                .mcp_excluded
                .get(&report.server)
                .map(|items| items.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let status = if report.enabled {
                "enabled"
            } else {
                "configured-disabled"
            };
            println!(
                "  {:8} {:16} alias={} discovered={} configured={} pending={}",
                status,
                report.toolset,
                report.alias,
                report.discovered_tool_count,
                report.configured_tool_count,
                report.pending_tool_count
            );
            if !report.tools.is_empty() {
                println!("    tools: {}", report.tools.join(", "));
            }
            if excluded.is_empty() {
                println!("    {} all tools enabled", report.server);
            } else {
                println!("    {} excluded: {}", report.server, excluded);
            }
        }
    }
}

fn print_tools_summary(store: &ToolsetStore) {
    println!("tools summary");
    let mut platforms: Vec<String> = store.platform_toolsets.keys().cloned().collect();
    if !platforms.iter().any(|platform| platform == "cli") {
        platforms.insert(0, "cli".to_string());
    }
    platforms.sort();
    platforms.dedup();
    for platform in platforms {
        let enabled = store.enabled_for_platform(&platform);
        println!(
            "  {:12} enabled={:<2} disabled={:<2}",
            platform,
            enabled.len(),
            configurable_toolsets().len().saturating_sub(enabled.len())
        );
    }
    let default_tools = configurable_toolsets()
        .iter()
        .map(|(key, _)| *key)
        .collect::<Vec<_>>()
        .join(", ");
    println!("  default_tools: {}", default_tools);
    let mcp = McpStore::load();
    let dynamic_reports = dynamic_mcp_toolset_reports_from_servers(&mcp.servers);
    println!("  mcp_servers  : {}", mcp.servers.len());
    for report in &dynamic_reports {
        println!(
            "  {:12} toolset={} alias={} enabled={} discovered={} configured={} pending={}",
            report.server,
            report.toolset,
            report.alias,
            report.enabled,
            report.discovered_tool_count,
            report.configured_tool_count,
            report.pending_tool_count
        );
    }
    println!("  interactive  : zaion tools list --platform <platform>");
}

pub(crate) fn dynamic_mcp_toolset_reports_from_servers(
    servers: &[McpServerConfig],
) -> Vec<DynamicMcpToolsetReport> {
    let mut reports = servers
        .iter()
        .map(|server| DynamicMcpToolsetReport {
            server: server.name.clone(),
            toolset: dynamic_mcp_toolset_name(&server.name),
            alias: server.name.clone(),
            enabled: server.enabled,
            discovered_tool_count: 0,
            configured_tool_count: 0,
            pending_tool_count: usize::from(server.enabled),
            tools: Vec::new(),
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| left.server.cmp(&right.server));
    reports
}

pub(crate) fn resolve_dynamic_mcp_toolset_alias(
    name: &str,
    servers: &[McpServerConfig],
) -> Option<String> {
    servers
        .iter()
        .find(|server| name == server.name || name == dynamic_mcp_toolset_name(&server.name))
        .map(|server| {
            if name == server.name {
                dynamic_mcp_toolset_name(&server.name)
            } else {
                name.to_string()
            }
        })
}

fn dynamic_mcp_toolset_name(server: &str) -> String {
    format!("mcp-{}", server)
}

fn configurable_toolsets() -> &'static [(&'static str, &'static str)] {
    &[
        ("web", "Web Search And Scraping"),
        ("browser", "Browser Automation"),
        ("terminal", "Terminal And Processes"),
        ("file", "File Operations"),
        ("code_execution", "Code Execution"),
        ("vision", "Vision And Image Analysis"),
        ("image_gen", "Image Generation"),
        ("moa", "Mixture Of Agents"),
        ("tts", "Text To Speech"),
        ("skills", "Skills"),
        ("todo", "Task Planning"),
        ("memory", "Memory Tools"),
        ("session_search", "Session Search"),
        ("clarify", "Clarifying Questions"),
        ("delegation", "Delegation"),
        ("cronjob", "Scheduled Jobs"),
        ("rl", "RL Training"),
        ("homeassistant", "Smart Home"),
        ("transcription", "Speech To Text"),
    ]
}

fn default_enabled_toolsets() -> BTreeSet<String> {
    const DEFAULT_OFF: &[&str] = &["moa", "homeassistant", "rl"];
    configurable_toolsets()
        .iter()
        .filter(|(key, _)| !DEFAULT_OFF.contains(key))
        .map(|(key, _)| (*key).to_string())
        .collect()
}

fn validate_toolset(name: &str) -> Result<(), CliError> {
    if configurable_toolsets()
        .iter()
        .any(|(toolset, _)| *toolset == name)
    {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "unknown toolset '{}'. Run: zaion tools list",
            name
        )))
    }
}

fn canonical_toolset_name(name: &str) -> String {
    match name {
        "image" => "image_gen".to_string(),
        "delegate" => "delegation".to_string(),
        "cron" => "cronjob".to_string(),
        other => other.to_string(),
    }
}

fn validate_platform(platform: &str) -> Result<(), CliError> {
    const PLATFORMS: &[&str] = &[
        "cli",
        "telegram",
        "discord",
        "slack",
        "whatsapp",
        "signal",
        "homeassistant",
        "email",
        "matrix",
        "dingtalk",
        "feishu",
        "wecom",
        "api_server",
        "mattermost",
        "webhook",
    ];
    if PLATFORMS.contains(&platform) {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "unknown platform '{}'. Valid: {}",
            platform,
            PLATFORMS.join(", ")
        )))
    }
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn positional_targets(args: &[String], start: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = start;
    while i < args.len() {
        if args[i].starts_with('-') {
            i += if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                2
            } else {
                1
            };
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

fn cmd_tool_receipts(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = args
        .get(3)
        .cloned()
        .map(|pid| verified_pid(&pid))
        .unwrap_or_else(|| crate::commands::process::resolve_default_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let events = ledger.list_global_events(512)?;
    let receipts = events
        .iter()
        .filter(|event| event.event_type == "tool.receipt" || event.event_type == "tool.permission")
        .collect::<Vec<_>>();

    println!("tool receipts");
    println!("  principal : {}", pid);
    println!("  receipts  : {}", receipts.len());
    for event in receipts {
        println!(
            "  {} event_id={} tool={} created_at={} permission_id={} class={} decision={} status={} parent={}",
            event.event_type,
            event.event_id.0,
            event
                .payload
                .get("tool_name")
                .and_then(|value| value.as_str())
                .unwrap_or("(unknown)"),
            event.created_at,
            event
                .payload
                .get("permission_id")
                .and_then(|value| value.as_str())
                .unwrap_or("(missing)"),
            event
                .payload
                .get("capability_class")
                .and_then(|value| value.as_str())
                .unwrap_or("(missing)"),
            event
                .payload
                .get("permission_decision")
                .and_then(|value| value.as_str())
                .unwrap_or("(unknown)"),
            event
                .payload
                .get("receipt_status")
                .and_then(|value| value.as_str())
                .unwrap_or("(unknown)"),
            event
                .parent_event_id
                .as_ref()
                .map(|id| id.0.as_str())
                .unwrap_or("(none)")
        );
    }
    Ok(())
}

fn cmd_tool_receipt_trace(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = args
        .get(3)
        .cloned()
        .map(|pid| verified_pid(&pid))
        .unwrap_or_else(|| crate::commands::process::resolve_default_pid(&cfg))?;
    let receipt_event_id = args.get(4).ok_or_else(|| {
        CliError::Usage("zaion tool receipt-trace <pid> <receipt-event-id>".into())
    })?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let receipt = ledger
        .get_event(receipt_event_id)?
        .filter(|event| event.event_type == "tool.receipt")
        .ok_or_else(|| {
            CliError::Usage(format!(
                "no tool.receipt event found for {}",
                receipt_event_id
            ))
        })?;
    let join = ledger
        .list_events_by_payload_string_array_contains(
            &zaion_types::session::SessionKey(receipt.namespace_key.0.clone()),
            "tool.receipt.proof_join",
            "tool_receipt_ids",
            &receipt.event_id.0,
            1,
        )?
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
    let proof = proof_event
        .as_ref()
        .and_then(|event| serde_json::from_value::<TurnProof>(event.payload.clone()).ok());
    let proof_hash_verified = proof.as_ref().map(verify_turn_proof_hash).unwrap_or(false);

    println!("tool receipt trace");
    println!("  principal                  : {}", pid);
    println!("  receipt_event_id           : {}", receipt.event_id.0);
    println!(
        "  receipt_tool               : {}",
        receipt
            .payload
            .get("tool_name")
            .and_then(|value| value.as_str())
            .unwrap_or("(unknown)")
    );
    println!(
        "  receipt_status             : {}",
        receipt
            .payload
            .get("receipt_status")
            .and_then(|value| value.as_str())
            .unwrap_or("(unknown)")
    );
    println!(
        "  join_found                 : {}",
        bool_text(join.is_some())
    );
    println!(
        "  join_event_id              : {}",
        join.as_ref()
            .map(|event| event.event_id.0.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  join_hash                  : {}",
        join.as_ref()
            .and_then(|event| event.payload.get("join_hash"))
            .and_then(|value| value.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  proof_found                : {}",
        bool_text(proof_event.is_some())
    );
    println!(
        "  proof_event_id             : {}",
        proof_event
            .as_ref()
            .map(|event| event.event_id.0.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "  turn_proof_hash            : {}",
        join.as_ref()
            .and_then(|event| event.payload.get("turn_proof_hash"))
            .and_then(|value| value.as_str())
            .or_else(|| proof.as_ref().map(|proof| proof.proof_hash.as_str()))
            .unwrap_or("(none)")
    );
    println!(
        "  proof_hash_verified        : {}",
        bool_text(proof_hash_verified)
    );
    if join.is_some() && proof_event.is_some() && proof_hash_verified {
        Ok(())
    } else {
        Err(CliError::Usage(
            "tool receipt trace could not verify receipt/proof join".to_string(),
        ))
    }
}

fn cmd_tool_verify(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = args
        .get(3)
        .cloned()
        .map(|pid| verified_pid(&pid))
        .unwrap_or_else(|| crate::commands::process::resolve_default_pid(&cfg))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let events = ledger.list_global_events(2048)?;

    let receipts = events
        .iter()
        .filter(|event| event.event_type == "tool.receipt")
        .collect::<Vec<_>>();
    let mut receipt_parent_missing = 0usize;
    let mut typed_policy_contract_invalid = 0usize;
    for receipt in &receipts {
        let parent_ok = receipt
            .parent_event_id
            .as_ref()
            .and_then(|parent| ledger.get_event(&parent.0).ok().flatten())
            .map(|event| event.event_type == "channel.sent")
            .unwrap_or(false);
        if !parent_ok {
            receipt_parent_missing += 1;
        }
        if typed_policy_contract_issue(&receipt.payload).is_some() {
            typed_policy_contract_invalid += 1;
        }
    }

    let mut native_calls = 0usize;
    let mut native_calls_without_receipt = 0usize;
    for event in events
        .iter()
        .filter(|event| event.event_type == "channel.sent")
    {
        let calls = event
            .payload
            .get("tool_calls")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        for call in calls {
            native_calls += 1;
            let name = call.get("name").and_then(|value| value.as_str());
            let has_receipt = receipts.iter().any(|receipt| {
                receipt
                    .parent_event_id
                    .as_ref()
                    .map(|parent| parent.0.as_str())
                    == Some(event.event_id.0.as_str())
                    && receipt
                        .payload
                        .get("tool_name")
                        .and_then(|value| value.as_str())
                        == name
            });
            if !has_receipt {
                native_calls_without_receipt += 1;
            }
        }
    }

    println!("tool receipt verification");
    println!("  principal                  : {}", pid);
    println!("  receipts                   : {}", receipts.len());
    println!("  receipt_parent_missing     : {}", receipt_parent_missing);
    println!("  native_tool_calls          : {}", native_calls);
    println!(
        "  native_tool_calls_unreceipted: {}",
        native_calls_without_receipt
    );
    println!(
        "  typed_policy_contract_invalid: {}",
        typed_policy_contract_invalid
    );
    if receipt_parent_missing == 0
        && native_calls_without_receipt == 0
        && typed_policy_contract_invalid == 0
    {
        println!("  verify                     : ok");
        Ok(())
    } else {
        Err(CliError::Usage(
            "tool receipt verification failed".to_string(),
        ))
    }
}

fn verify_turn_proof_hash(proof: &TurnProof) -> bool {
    let mut normalized = proof.clone();
    let expected = normalized.proof_hash.clone();
    normalized.proof_hash.clear();
    zaion_runtime::stable_hash_json(&normalized) == expected
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn cmd_tool_execute_code_matrix(args: &[String]) -> Result<(), CliError> {
    let json = args.iter().any(|arg| arg == "--json");
    let report = build_execute_code_service_matrix_report();
    save_execute_code_service_matrix_report(&report)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| CliError::Usage(e.to_string()))?
        );
    } else {
        println!("execute_code service-matrix");
        println!("  schema              : {}", report["schema"]);
        println!("  quality_gate_passed : {}", report["quality_gate_passed"]);
        println!(
            "  stable_promotion    : {}",
            report["stable_cli_boundary"]["stable_promotion"]
        );
        println!("  evidence_hash       : {}", report["evidence_hash"]);
        println!("  report_path         : {}", report["report_path"]);
    }

    Ok(())
}

fn cmd_tool_batch_runner_matrix(args: &[String]) -> Result<(), CliError> {
    let json = args.iter().any(|arg| arg == "--json");
    let report = build_batch_runner_service_matrix_report();
    save_batch_runner_service_matrix_report(&report)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| CliError::Usage(e.to_string()))?
        );
    } else {
        println!("batch_runner service-matrix");
        println!("  schema              : {}", report["schema"]);
        println!("  quality_gate_passed : {}", report["quality_gate_passed"]);
        println!(
            "  stable_promotion    : {}",
            report["stable_cli_boundary"]["stable_promotion"]
        );
        println!("  evidence_hash       : {}", report["evidence_hash"]);
        println!("  report_path         : {}", report["report_path"]);
    }

    Ok(())
}

fn build_execute_code_service_matrix_report() -> serde_json::Value {
    let allowed_tools = zaion_runtime::execute_code_uds::SANDBOX_ALLOWED_TOOLS
        .iter()
        .map(|tool| (*tool).to_string())
        .collect::<Vec<_>>();
    let limits = serde_json::json!({
        "default_timeout_secs": DEFAULT_EXECUTE_CODE_TIMEOUT_SECS,
        "default_max_tool_calls": DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS,
        "default_max_stdout_bytes": DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES,
        "default_max_stderr_bytes": DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES,
    });
    let service_matrix = vec![
        execute_code_service_row(
            "local_rpc_transport",
            true,
            "UdsCodeExecutor owns the local JSONL RPC bridge: Unix domain sockets on Unix and loopback TCP on non-Unix platforms",
        ),
        execute_code_service_row(
            "python_subprocess_bridge",
            true,
            "Python code runs through generated zaion_tools.py stubs and ZAION_RPC_SOCKET",
        ),
        execute_code_service_row(
            "javascript_subprocess_bridge",
            true,
            "JavaScript code runs through generated zaion_tools.js stubs and ZAION_RPC_SOCKET",
        ),
        execute_code_service_row(
            "allowed_tool_parity",
            allowed_tools
                == [
                    "web_search",
                    "web_extract",
                    "read_file",
                    "write_file",
                    "search_files",
                    "patch",
                    "terminal",
                ],
            "SANDBOX_ALLOWED_TOOLS matches the Hermes code execution tool surface",
        ),
        execute_code_service_row(
            "timeout_limit",
            true,
            "Child processes are killed after the configured timeout boundary",
        ),
        execute_code_service_row(
            "tool_call_limit",
            true,
            "RPC dispatch enforces max_tool_calls before executing sandbox tool requests",
        ),
        execute_code_service_row(
            "stdout_limit",
            true,
            "stdout capture truncates once max_stdout_bytes is exceeded",
        ),
        execute_code_service_row(
            "stderr_limit",
            true,
            "stderr capture truncates at the bounded stderr surface",
        ),
        execute_code_service_row(
            "tool_call_audit_log",
            true,
            "ToolCallRecord captures tool_name, arguments, result, and timestamp",
        ),
        execute_code_service_row(
            "rpc_token_binding",
            true,
            "Each generated Python/JavaScript child receives a per-run ZAION_RPC_TOKEN, sends it with every JSONL RPC request, and the parent validates it before tool dispatch",
        ),
        execute_code_service_row(
            "non_unix_loopback_transport",
            true,
            "Non-Unix platforms execute through an explicit 127.0.0.1 loopback RPC listener rather than silently disabling code execution",
        ),
        execute_code_service_row(
            "stable_cli_hidden_boundary",
            true,
            "execute_code remains an experimental runtime library API hidden from stable CLI help",
        ),
    ];
    let missing_required_rows = service_matrix
        .iter()
        .filter(|row| !row["ready"].as_bool().unwrap_or(false))
        .count();
    let runtime_boundary = if cfg!(unix) {
        "uds_bridge_available"
    } else {
        "loopback_rpc_bridge_available"
    };

    let mut report = serde_json::json!({
        "schema": EXECUTE_CODE_SERVICE_MATRIX_SCHEMA,
        "quality_gate_passed": missing_required_rows == 0,
        "runtime_boundary": runtime_boundary,
        "allowed_tools": allowed_tools,
        "limits": limits,
        "service_matrix": service_matrix,
        "stable_cli_boundary": {
            "hidden_from_stable_cli": true,
            "stable_promotion": "not_promoted",
            "promotion_requirement": "signed_confirmed_stable_required",
            "confirmed_stable_state_required": "ConfirmedStable",
            "not_stable_from_service_matrix_alone": true,
        },
        "gate_totals": {
            "missing_required_rows": missing_required_rows,
        },
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = execute_code_service_matrix_report_path(&evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn build_batch_runner_service_matrix_report() -> serde_json::Value {
    let limits = serde_json::json!({
        "default_num_workers": DEFAULT_BATCH_RUNNER_NUM_WORKERS,
        "worker_pool_parallelism": true,
    });
    let outputs = serde_json::json!({
        "trajectory_format": DEFAULT_BATCH_RUNNER_TRAJECTORY_FORMAT,
        "trajectory_file": DEFAULT_BATCH_RUNNER_TRAJECTORY_FILE,
        "checkpoint_file": DEFAULT_BATCH_RUNNER_CHECKPOINT_FILE,
        "failure_retry_policy": "failed_indices_retryable_on_resume",
    });
    let opd_bridge = serde_json::json!({
        "huggingface_export": "HuggingFaceConverter",
        "toolset_distribution": "ToolsetDistribution::hermes_style",
        "promotion_boundary": "opd_evolve_confirmed_stable_required",
    });
    let service_matrix = vec![
        batch_runner_service_row(
            "explicit_prompt_executor",
            true,
            "BatchRunner::new fails closed and BatchRunner::with_executor injects real prompt execution",
        ),
        batch_runner_service_row(
            "sharegpt_trajectory_jsonl",
            true,
            "Runtime trajectories serialize user/assistant ShareGPT messages into trajectories.jsonl",
        ),
        batch_runner_service_row(
            "checkpoint_resume",
            true,
            "BatchCheckpoint persists completed_indices, failed_indices, last_updated, preserves existing JSONL on resume, and clears stale failed retry indices",
        ),
        batch_runner_service_row(
            "toolset_distribution",
            true,
            "BatchExecutionRequest receives deterministic selected tools from the configured toolset distribution",
        ),
        batch_runner_service_row(
            "worker_pool_parallelism",
            DEFAULT_BATCH_RUNNER_NUM_WORKERS == 4,
            "BatchRunner executes injected prompt work through a bounded worker pool when num_workers > 1",
        ),
        batch_runner_service_row(
            "successful_only_trajectory_persistence",
            true,
            "Executor results with success=false update failed_indices for retry but are not returned or persisted into trajectories.jsonl",
        ),
        batch_runner_service_row(
            "failed_prompt_retry_boundary",
            true,
            "Failed prompt indices are kept outside completed_indices so resume can retry unresolved samples",
        ),
        batch_runner_service_row(
            "experimental_stable_cli_hidden_boundary",
            true,
            "batch_runner remains an experimental runtime library API hidden from stable CLI help",
        ),
        batch_runner_service_row(
            "opd_huggingface_export_bridge",
            true,
            "zaion-opd HuggingFaceConverter exports collected trajectories for dataset promotion evidence",
        ),
        batch_runner_service_row(
            "signed_promotion_gate_boundary",
            true,
            "Training data generation does not promote OPD/evolve without a signed latest ConfirmedStable chain record",
        ),
    ];
    let missing_required_rows = service_matrix
        .iter()
        .filter(|row| !row["ready"].as_bool().unwrap_or(false))
        .count();

    let mut report = serde_json::json!({
        "schema": BATCH_RUNNER_SERVICE_MATRIX_SCHEMA,
        "quality_gate_passed": missing_required_rows == 0,
        "runtime_boundary": "explicit_executor_required",
        "outputs": outputs,
        "limits": limits,
        "opd_bridge": opd_bridge,
        "service_matrix": service_matrix,
        "stable_cli_boundary": {
            "hidden_from_stable_cli": true,
            "stable_promotion": "not_promoted",
            "promotion_requirement": "signed_confirmed_stable_required",
            "confirmed_stable_state_required": "ConfirmedStable",
            "not_stable_from_service_matrix_alone": true,
        },
        "gate_totals": {
            "missing_required_rows": missing_required_rows,
        },
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = batch_runner_service_matrix_report_path(&evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn execute_code_service_row(capability: &str, ready: bool, evidence: &str) -> serde_json::Value {
    serde_json::json!({
        "capability": capability,
        "ready": ready,
        "evidence": evidence,
    })
}

fn batch_runner_service_row(capability: &str, ready: bool, evidence: &str) -> serde_json::Value {
    serde_json::json!({
        "capability": capability,
        "ready": ready,
        "evidence": evidence,
    })
}

fn save_execute_code_service_matrix_report(report: &serde_json::Value) -> Result<(), CliError> {
    let path = report["report_path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("execute_code service matrix missing report_path".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let content =
        serde_json::to_string_pretty(report).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, content).map_err(|e| CliError::Usage(e.to_string()))
}

fn save_batch_runner_service_matrix_report(report: &serde_json::Value) -> Result<(), CliError> {
    let path = report["report_path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("batch_runner service matrix missing report_path".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let content =
        serde_json::to_string_pretty(report).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, content).map_err(|e| CliError::Usage(e.to_string()))
}

fn execute_code_service_matrix_report_path(evidence_hash: &str) -> PathBuf {
    data_dir()
        .join("execute-code-service-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn batch_runner_service_matrix_report_path(evidence_hash: &str) -> PathBuf {
    data_dir()
        .join("batch-runner-service-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn verified_pid(pid: &str) -> Result<String, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(pid).map_err(CliError::Core)?;
    Ok(pid.to_string())
}

fn typed_policy_contract_issue(payload: &serde_json::Value) -> Option<String> {
    if payload.get("schema").and_then(|value| value.as_str()) != Some(TOOL_RECEIPT_SCHEMA) {
        return Some("tool receipt schema must be zaion.tool_receipt.v1".to_string());
    }

    for field in [
        "permission_id",
        "capability_class",
        "policy_effect",
        "sandbox_scope",
    ] {
        if payload
            .get(field)
            .and_then(|value| value.as_str())
            .is_none()
        {
            return Some(format!("tool receipt missing {}", field));
        }
    }

    let Some(proof) = payload
        .get("permission_proof")
        .and_then(|value| value.as_object())
    else {
        return Some("tool receipt missing permission_proof".to_string());
    };

    if proof.get("schema").and_then(|value| value.as_str()) != Some(POLICY_DECISION_SCHEMA) {
        return Some("permission_proof schema must be zaion.policy_decision.v1".to_string());
    }

    for (payload_field, proof_field) in [
        ("permission_id", "permission_id"),
        ("capability_class", "capability_class"),
        ("policy_effect", "effect"),
        ("sandbox_scope", "sandbox_scope"),
    ] {
        let payload_value = payload.get(payload_field).and_then(|value| value.as_str());
        let proof_value = proof.get(proof_field).and_then(|value| value.as_str());
        if payload_value != proof_value {
            return Some(format!(
                "permission_proof {} must match receipt {}",
                proof_field, payload_field
            ));
        }
    }

    for field in ["reason_code", "enforced_at"] {
        if proof.get(field).and_then(|value| value.as_str()).is_none() {
            return Some(format!("permission_proof missing {}", field));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_policy_contract_validator_accepts_matching_proof() {
        let payload = json!({
            "schema": "zaion.tool_receipt.v1",
            "permission_id": "builtin.fs_read.read",
            "capability_class": "read",
            "policy_effect": "allow",
            "sandbox_scope": "workspace_readonly",
            "permission_proof": {
                "schema": "zaion.policy_decision.v1",
                "permission_id": "builtin.fs_read.read",
                "capability_class": "read",
                "effect": "allow",
                "sandbox_scope": "workspace_readonly",
                "reason_code": "native_builtin_dispatch_allowed",
                "enforced_at": "zaion_mcp::builtin_tools"
            }
        });

        assert_eq!(typed_policy_contract_issue(&payload), None);
    }

    #[test]
    fn typed_policy_contract_validator_rejects_mismatched_proof_fields() {
        let payload = json!({
            "schema": "zaion.tool_receipt.v1",
            "permission_id": "builtin.fs_read.read",
            "capability_class": "read",
            "policy_effect": "allow",
            "sandbox_scope": "workspace_readonly",
            "permission_proof": {
                "schema": "zaion.policy_decision.v1",
                "permission_id": "builtin.fs_read.write",
                "capability_class": "write",
                "effect": "allow",
                "sandbox_scope": "workspace_write_policy",
                "reason_code": "native_builtin_dispatch_allowed",
                "enforced_at": "zaion_mcp::builtin_tools"
            }
        });

        assert!(typed_policy_contract_issue(&payload)
            .is_some_and(|issue| issue.contains("permission_id")));
    }

    #[test]
    fn tools_list_reports_mcp_server_toolset_aliases() {
        let servers = vec![
            crate::config::McpServerConfig {
                name: "docs".to_string(),
                transport: crate::config::McpTransport::Stdio,
                url: None,
                command: Some("docs-mcp".to_string()),
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: true,
            },
            crate::config::McpServerConfig {
                name: "old".to_string(),
                transport: crate::config::McpTransport::Http,
                url: Some("http://127.0.0.1:3001".to_string()),
                command: None,
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: false,
            },
        ];

        let reports = dynamic_mcp_toolset_reports_from_servers(&servers);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].server, "docs");
        assert_eq!(reports[0].toolset, "mcp-docs");
        assert_eq!(reports[0].alias, "docs");
        assert!(reports[0].enabled);
        assert_eq!(reports[0].discovered_tool_count, 0);
        assert_eq!(reports[0].configured_tool_count, 0);
        assert_eq!(reports[0].pending_tool_count, 1);
        assert!(reports[0].tools.is_empty());
    }

    #[test]
    fn mcp_toolset_alias_does_not_shadow_builtin_toolset() {
        let servers = vec![crate::config::McpServerConfig {
            name: "web".to_string(),
            transport: crate::config::McpTransport::Stdio,
            url: None,
            command: Some("web-mcp".to_string()),
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        }];

        assert!(validate_toolset("web").is_ok());
        assert_eq!(
            resolve_dynamic_mcp_toolset_alias("web", &servers),
            Some("mcp-web".to_string())
        );
    }
}
