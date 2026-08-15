use crate::commands::identity::model_window_estimate;
use crate::commands::provider::provider_health;
use crate::commands::{data_dir, experimental_command_help_lines, CliError};
use crate::config::{
    effective_telegram_token, secret_is_set, zaion_state_paths, ChannelStore, McpServerConfig,
    McpStore, WebhookStore, ZaionConfig,
};
use zaion_types::policy::{CapabilityClass, PolicyDecision};

pub fn cmd_capability(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => capability_show(args),
        other => Err(CliError::Usage(format!(
            "unknown capability subcommand: {}. Use: show",
            other
        ))),
    }
}

fn capability_show(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let provider = provider_health(&cfg);
    let paths = zaion_state_paths();
    let mcp = McpStore::load();
    let channels = ChannelStore::load().with_config_fallback(&cfg);
    let webhooks = WebhookStore::load();
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let process_count = store.list_all().map(|p| p.len()).unwrap_or(0);
    let telegram = effective_telegram_token(&cfg, &channels);

    if output_json(args) {
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "capability_manifest",
            "identity_contract": "zaion identity show",
            "provider": cfg.provider.as_deref().unwrap_or("(not set)"),
            "model": provider.model,
            "model_window": model_window_estimate(&provider.model),
            "api_key": provider.api_key_status,
            "base_url": provider.base_url,
            "environment": {
                "zaion_home": paths.home.path,
                "zaion_home_source": paths.home.source,
                "data_dir": paths.data_dir.path,
                "data_source": paths.data_dir.source,
                "process_count": process_count,
            },
            "channels": {
                "profiles": channels.channels.len(),
                "telegram": if secret_is_set(telegram.as_deref()) { "configured" } else { "not configured" },
                "webhooks": webhooks.subscriptions.len(),
                "terminal_cli": "enabled",
                "tui": "enabled when provider and process are ready",
            },
            "tools": {
                "mcp_config": McpStore::path(),
                "mcp_servers": mcp.servers.len(),
                "mcp_enabled": mcp.servers.iter().filter(|server| server.enabled).count(),
                "dynamic_mcp_toolsets": dynamic_mcp_toolset_manifest(&mcp.servers),
                "native_runtime_tools": native_runtime_tool_manifest(),
                "control_plane_commands": ["chat", "wake", "events", "sync", "memory", "context", "doctor"],
            },
            "permissions": {
                "filesystem_scope": "configured Zaion home/data plus user-approved paths",
                "network_scope": "provider endpoints, configured MCP/webhooks, explicit activity domains",
                "memory_scope": "signed ledger, principal memory, semantic memory, traceable memory atoms",
                "autonomy_scope": "off by default; activity continuity requires explicit enablement",
                "forbidden_auto": ["destructive actions", "credential access", "purchases", "code modification"],
            },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }

    println!("capability manifest");
    println!("  identity_contract : zaion identity show");
    println!(
        "  provider          : {}",
        cfg.provider.as_deref().unwrap_or("(not set)")
    );
    println!("  model             : {}", provider.model);
    println!(
        "  model_window      : {}",
        model_window_estimate(&provider.model)
    );
    println!("  api_key           : {}", provider.api_key_status);
    println!("  base_url          : {}", provider.base_url);
    println!();
    println!("environment");
    println!("  zaion_home        : {}", paths.home.path.display());
    println!("  zaion_home_source : {}", paths.home.source);
    println!("  data_dir          : {}", paths.data_dir.path.display());
    println!("  data_source       : {}", paths.data_dir.source);
    println!("  process_count     : {}", process_count);
    println!();
    println!("channels");
    println!("  profiles          : {}", channels.channels.len());
    println!(
        "  telegram          : {}",
        if secret_is_set(telegram.as_deref()) {
            "configured"
        } else {
            "not configured"
        }
    );
    println!("  webhooks          : {}", webhooks.subscriptions.len());
    println!("  terminal_cli      : enabled");
    println!("  tui               : enabled when provider and process are ready");
    println!();
    println!("tools");
    println!("  mcp_config        : {}", McpStore::path().display());
    println!("  mcp_servers       : {}", mcp.servers.len());
    println!(
        "  mcp_enabled       : {}",
        mcp.servers.iter().filter(|server| server.enabled).count()
    );
    let dynamic_mcp = dynamic_mcp_toolset_manifest(&mcp.servers);
    if !dynamic_mcp.is_empty() {
        println!("  dynamic_mcp_toolsets:");
        for report in &dynamic_mcp {
            println!(
                "    {} alias={} enabled={} discovered={} tools={}",
                report["toolset"].as_str().unwrap_or("(unknown)"),
                report["alias"].as_str().unwrap_or("(unknown)"),
                report["enabled"].as_bool().unwrap_or(false),
                report["discovered_tool_count"].as_u64().unwrap_or(0),
                report["tools"]
                    .as_array()
                    .map(|tools| tools.len())
                    .unwrap_or(0)
            );
        }
    }
    println!("  native_runtime    : fs_read, fs_list, fs_search, memory_search");
    println!("  native_diagnostic : capability_status, surface_status, ledger_recent");
    println!("  native_execute    : shell_exec(disabled-by-default; allow-listed)");
    println!("  permission_proof  : tool.receipt ledger event for runtime tool execution");
    println!("  enforcement       : crates/zaion-mcp/src/builtin_tools/mod.rs");
    println!("  control_plane     : chat, wake, events, sync, memory, context, doctor");
    println!("  surface_note      : terminal_cli/tui/telegram/http/mcp/memory/context/ledger are product surfaces; callable tools are listed above and can expand through configured MCP servers");
    println!();
    println!("permissions");
    println!("  filesystem_scope  : configured Zaion home/data plus user-approved paths");
    println!("  network_scope     : provider endpoints, configured MCP/webhooks, explicit activity domains");
    println!("  memory_scope      : signed ledger, principal memory, semantic memory, traceable memory atoms");
    println!(
        "  autonomy_scope    : off by default; activity continuity requires explicit enablement"
    );
    println!("  forbidden_auto    : destructive actions, credential access, purchases, code modification");
    println!();
    println!("experimental surfaces");
    for line in experimental_command_help_lines() {
        println!("{}", line);
    }
    Ok(())
}

fn output_json(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
}

fn native_runtime_tool_manifest() -> Vec<serde_json::Value> {
    [
        ("fs_read", "proven", CapabilityClass::Read),
        ("fs_list", "proven", CapabilityClass::Read),
        ("fs_search", "proven", CapabilityClass::Read),
        ("todo", "proven", CapabilityClass::External),
        (
            "shell_exec",
            "experimental_disabled_by_default",
            CapabilityClass::Execute,
        ),
        ("memory_search", "experimental", CapabilityClass::Memory),
        ("capability_status", "proven", CapabilityClass::External),
        ("surface_status", "proven", CapabilityClass::External),
        ("ledger_recent", "proven", CapabilityClass::External),
    ]
    .into_iter()
    .map(|(name, status, class)| {
        let decision = PolicyDecision::allow_builtin(name, class);
        serde_json::json!({
            "name": name,
            "status": status,
            "permission_id": decision.permission_id,
            "capability_class": decision.capability_class,
            "sandbox_scope": decision.sandbox_scope,
            "policy_effect": decision.effect,
            "permission_proof": decision.permission_proof(),
            "enforced_at": decision.enforced_at,
            "receipt_event": "tool.receipt",
        })
    })
    .collect()
}

pub(crate) fn dynamic_mcp_toolset_manifest(servers: &[McpServerConfig]) -> Vec<serde_json::Value> {
    crate::commands::tool::dynamic_mcp_toolset_reports_from_servers(servers)
        .into_iter()
        .map(|report| {
            serde_json::json!({
                "server": report.server,
                "toolset": report.toolset,
                "alias": report.alias,
                "enabled": report.enabled,
                "discovered_tool_count": report.discovered_tool_count,
                "configured_tool_count": report.configured_tool_count,
                "pending_tool_count": report.pending_tool_count,
                "tools": report.tools,
            })
        })
        .collect()
}

pub fn doctor_summary() -> Vec<String> {
    let cfg = ZaionConfig::load();
    let provider = provider_health(&cfg);
    let mcp = McpStore::load();
    let channels = ChannelStore::load().with_config_fallback(&cfg);
    vec![
        format!(
            "provider    : {}",
            cfg.provider.as_deref().unwrap_or("(not set)")
        ),
        format!("model       : {}", provider.model),
        format!("model_window: {}", model_window_estimate(&provider.model)),
        format!(
            "mcp_enabled : {}",
            mcp.servers.iter().filter(|s| s.enabled).count()
        ),
        format!("channels    : {}", channels.channels.len()),
        "tools       : native=fs_read,fs_list,fs_search,memory_search,capability_status,surface_status,ledger_recent; execute=shell_exec(disabled-by-default)".to_string(),
        "proof       : runtime tool calls must emit signed tool.receipt".to_string(),
        "autonomy     : off unless activity continuity is enabled".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{McpServerConfig, McpTransport};

    #[test]
    fn capability_manifest_includes_dynamic_mcp_toolsets() {
        let servers = vec![McpServerConfig {
            name: "docs".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("docs-mcp".to_string()),
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        }];

        let manifest = dynamic_mcp_toolset_manifest(&servers);
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0]["server"], "docs");
        assert_eq!(manifest[0]["toolset"], "mcp-docs");
        assert_eq!(manifest[0]["alias"], "docs");
        assert_eq!(manifest[0]["enabled"], true);
        assert_eq!(manifest[0]["discovered_tool_count"], 0);
        assert_eq!(manifest[0]["configured_tool_count"], 0);
        assert_eq!(manifest[0]["pending_tool_count"], 1);
        assert_eq!(manifest[0]["tools"], serde_json::json!([]));
    }

    #[test]
    fn capability_manifest_lists_todo_as_callable_native_tool() {
        let tools = native_runtime_tool_manifest();
        let todo = tools
            .iter()
            .find(|tool| tool["name"] == "todo")
            .expect("todo native tool manifest entry");

        assert_eq!(todo["status"], "proven");
        assert_eq!(todo["permission_id"], "builtin.todo.external");
        assert_eq!(todo["receipt_event"], "tool.receipt");
    }
}
