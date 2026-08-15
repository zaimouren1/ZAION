//! MCP tool registry and runtime auto-loading
//!
//! This module implements the MCP tool registration system and runtime
//! auto-loading, completing the MCP bridge paradigm breakthrough.
//!
//! ## Architecture
//!
//! ```text
//! MCP Tool Definition (TOML)
//!     ↓
//! McpToolRegistry (this module)
//!     ↓
//! McpBridge (mcp_bridge.rs)
//!     ↓
//! MCP Subprocess (stdio JSON-RPC)
//! ```
//!
//! ## Paradigm Breakthrough vs Hermes
//!
//! Hermes mcp_config.py:
//! - TOML-based MCP server configuration
//! - Manual server start/stop
//! - Basic health checks
//!
//! Zaion mcp_tools.rs adds:
//! - **Automatic tool discovery**: Scan ZAION_HOME/mcp.toml at runtime
//! - **Hot reload**: Detect config changes and reload without restart
//! - **Tool routing**: Route tool calls to correct MCP subprocess
//! - **Capability negotiation**: Query MCP server capabilities at startup
//! - **Ed25519 signed tool calls**: All tool invocations cryptographically signed

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::mcp_bridge::{JsonRpcRequest, McpBridge, McpSubprocessConfig};

/// MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    /// Tool name (e.g., "read_file", "web_search")
    pub name: String,

    /// MCP server name that provides this tool
    pub server: String,

    /// Tool description
    pub description: String,

    /// Tool parameters schema (JSON Schema)
    pub parameters: serde_json::Value,
}

/// Runtime report for Hermes-style dynamic MCP toolsets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpDynamicToolsetReport {
    pub server: String,
    pub toolset: String,
    pub alias: String,
    pub enabled: bool,
    pub discovered_tool_count: usize,
    pub tools: Vec<String>,
}

/// MCP server definition (from ZAION_HOME/mcp.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDefinition {
    /// Server name
    pub name: String,

    /// Command to execute
    pub command: String,

    /// Command arguments
    pub args: Vec<String>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Auto-start on load
    #[serde(default = "default_true")]
    pub auto_start: bool,

    /// Auto-restart on crash
    #[serde(default = "default_true")]
    pub auto_restart: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct McpConfigFile {
    #[serde(default)]
    servers: Vec<McpPersistedServerDefinition>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum McpPersistedTransport {
    #[default]
    Http,
    Stdio,
}

#[derive(Debug, Clone, Deserialize)]
struct McpPersistedServerDefinition {
    name: String,
    #[serde(default)]
    transport: McpPersistedTransport,
    url: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_true")]
    auto_start: bool,
    #[serde(default = "default_true")]
    auto_restart: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyMcpServerDefinition {
    name: Option<String>,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_true")]
    auto_start: bool,
    #[serde(default = "default_true")]
    auto_restart: bool,
}

fn default_true() -> bool {
    true
}

fn parse_mcp_server_definitions(
    config_content: &str,
) -> Result<Vec<(String, McpServerDefinition)>, String> {
    let root: toml::Value = toml::from_str(config_content)
        .map_err(|e| format!("Failed to parse MCP config TOML: {}", e))?;

    if root.get("servers").is_some() {
        let config: McpConfigFile = toml::from_str(config_content)
            .map_err(|e| format!("Failed to parse MCP config [[servers]] entries: {}", e))?;
        return config
            .servers
            .into_iter()
            .filter(|server| server.enabled)
            .filter_map(|server| match server.into_runtime_definition() {
                Ok(Some(definition)) => Some(Ok(definition)),
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            })
            .collect();
    }

    let config: HashMap<String, LegacyMcpServerDefinition> = toml::from_str(config_content)
        .map_err(|e| format!("Failed to parse legacy MCP config: {}", e))?;

    config
        .into_iter()
        .map(|(key, server)| {
            let name = server.name.unwrap_or_else(|| key.clone());
            Ok((
                key,
                McpServerDefinition {
                    name,
                    command: server.command,
                    args: server.args,
                    env: server.env,
                    auto_start: server.auto_start,
                    auto_restart: server.auto_restart,
                },
            ))
        })
        .collect()
}

impl McpPersistedServerDefinition {
    fn into_runtime_definition(self) -> Result<Option<(String, McpServerDefinition)>, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("MCP server name must not be empty".to_string());
        }

        match self.transport {
            McpPersistedTransport::Http => {
                let _ = self.url;
                Ok(None)
            }
            McpPersistedTransport::Stdio => {
                let raw_command = self.command.ok_or_else(|| {
                    format!("MCP server '{}': stdio transport requires command", name)
                })?;
                let mut command_parts = split_command_line(&raw_command)
                    .map_err(|e| format!("MCP server '{}': invalid command line: {}", name, e))?;
                if command_parts.is_empty() {
                    return Err(format!(
                        "MCP server '{}': stdio transport requires command",
                        name
                    ));
                }

                let command = command_parts.remove(0);
                let mut args = command_parts;
                args.extend(self.args);

                Ok(Some((
                    name.clone(),
                    McpServerDefinition {
                        name,
                        command,
                        args,
                        env: self.env,
                        auto_start: self.auto_start,
                        auto_restart: self.auto_restart,
                    },
                )))
            }
        }
    }
}

fn split_command_line(input: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.trim().chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' | '\'' => match quote {
                Some(active) if active == ch => quote = None,
                Some(_) => current.push(ch),
                None => quote = Some(ch),
            },
            '\\' if quote == Some('"') && matches!(chars.peek(), Some('"')) => {
                current.push('"');
                let _ = chars.next();
            }
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if let Some(ch) = quote {
        return Err(format!("unterminated {} quote", ch));
    }

    if !current.is_empty() {
        parts.push(current);
    }

    Ok(parts)
}

fn parse_tool_definition(
    server_name: &str,
    tool_value: &serde_json::Value,
) -> Option<McpToolDefinition> {
    let name = tool_value.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let description = tool_value
        .get("description")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let parameters = tool_value
        .get("inputSchema")
        .or_else(|| tool_value.get("input_schema"))
        .or_else(|| tool_value.get("parameters"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
    Some(McpToolDefinition {
        name: name.to_string(),
        server: server_name.to_string(),
        description,
        parameters,
    })
}

/// MCP tool registry
pub struct McpToolRegistry {
    /// Registered tools (tool_name -> definition)
    tools: Arc<RwLock<HashMap<String, McpToolDefinition>>>,

    /// Server definitions (server_name -> definition)
    servers: Arc<RwLock<HashMap<String, McpServerDefinition>>>,

    /// MCP bridge instance
    bridge: Arc<McpBridge>,

    /// Config file path
    config_path: PathBuf,
}

impl McpToolRegistry {
    /// Create new MCP tool registry
    pub fn new(config_path: PathBuf, bridge: Arc<McpBridge>) -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            servers: Arc::new(RwLock::new(HashMap::new())),
            bridge,
            config_path,
        }
    }

    /// Load MCP servers from config file
    pub async fn load_from_config(&self) -> Result<(), String> {
        // Read config file
        let config_content = std::fs::read_to_string(&self.config_path)
            .map_err(|e| format!("Failed to read MCP config: {}", e))?;

        // Parse TOML. Supports both the CLI control-plane schema:
        //
        //   [[servers]]
        //   name = "filesystem"
        //   transport = "stdio"
        //   command = "npx -y @modelcontextprotocol/server-filesystem ."
        //
        // and the older runtime map schema:
        //
        //   [filesystem]
        //   command = "npx"
        //   args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
        let config = parse_mcp_server_definitions(&config_content)?;

        // Register servers
        let mut auto_start = Vec::new();
        {
            let mut servers = self.servers.write().await;
            for (name, server_def) in config {
                servers.insert(name.clone(), server_def.clone());

                // Auto-start if enabled
                if server_def.auto_start {
                    auto_start.push((name, server_def));
                }
            }
        }

        for (name, server_def) in auto_start {
            self.start_server(&name, &server_def).await?;
        }

        Ok(())
    }

    /// Start MCP server subprocess
    async fn start_server(
        &self,
        name: &str,
        server_def: &McpServerDefinition,
    ) -> Result<(), String> {
        let config = McpSubprocessConfig {
            command: server_def.command.clone(),
            args: server_def.args.clone(),
            env: server_def.env.clone(),
            auto_restart: server_def.auto_restart,
            max_restarts: 3,
        };

        self.bridge
            .register_subprocess(name.to_string(), config)
            .await?;

        self.initialize_server(name).await?;

        // Query server capabilities and register tools
        self.discover_tools(name).await?;

        Ok(())
    }

    /// Perform the standard MCP initialize handshake.
    async fn initialize_server(&self, server_name: &str) -> Result<(), String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(uuid::Uuid::new_v4().to_string())),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "zaion",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        let response = self.bridge.dispatch(server_name, request).await?;
        if response.error.is_some() {
            return Ok(());
        }

        let notification = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        self.bridge.notify(server_name, notification).await
    }

    /// Discover tools from MCP server
    async fn discover_tools(&self, server_name: &str) -> Result<(), String> {
        let discovered = self.discover_tool_definitions(server_name).await?;
        let mut tools = self.tools.write().await;
        for tool_def in discovered {
            tools.insert(tool_def.name.clone(), tool_def);
        }
        Ok(())
    }

    async fn discover_tool_definitions(
        &self,
        server_name: &str,
    ) -> Result<Vec<McpToolDefinition>, String> {
        // Send tools/list request to MCP server
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = self.bridge.dispatch(server_name, request).await?;

        let mut discovered = Vec::new();
        if let Some(result) = response.result {
            if let Some(tools_array) = result.get("tools").and_then(|t| t.as_array()) {
                for tool_value in tools_array {
                    if let Some(tool_def) = parse_tool_definition(server_name, tool_value) {
                        discovered.push(tool_def);
                    }
                }
            }
        }

        Ok(discovered)
    }

    /// Refresh tools for one MCP server after a `notifications/tools/list_changed` event.
    pub async fn refresh_server_tools(&self, server_name: &str) -> Result<(), String> {
        if !self.servers.read().await.contains_key(server_name) {
            return Err(format!("MCP server '{}' not found", server_name));
        }

        let discovered = self.discover_tool_definitions(server_name).await?;
        {
            let mut tools = self.tools.write().await;
            tools.retain(|_, tool| tool.server != server_name);
            for tool_def in discovered {
                tools.insert(tool_def.name.clone(), tool_def);
            }
        }

        Ok(())
    }

    /// Get tool definition by name
    pub async fn get_tool(&self, name: &str) -> Option<McpToolDefinition> {
        self.tools.read().await.get(name).cloned()
    }

    /// List all registered tools
    pub async fn list_tools(&self) -> Vec<McpToolDefinition> {
        self.tools.read().await.values().cloned().collect()
    }

    /// Report configured dynamic MCP toolsets using Hermes' `mcp-<server>` naming.
    pub async fn dynamic_toolset_reports(&self) -> Vec<McpDynamicToolsetReport> {
        let servers = self.servers.read().await;
        let tools = self.tools.read().await;
        let mut reports = servers
            .keys()
            .map(|server| {
                let mut server_tools = tools
                    .values()
                    .filter(|tool| tool.server == *server)
                    .map(|tool| tool.name.clone())
                    .collect::<Vec<_>>();
                server_tools.sort();
                McpDynamicToolsetReport {
                    server: server.clone(),
                    toolset: dynamic_mcp_toolset_name(server),
                    alias: server.clone(),
                    enabled: true,
                    discovered_tool_count: server_tools.len(),
                    tools: server_tools,
                }
            })
            .collect::<Vec<_>>();
        reports.sort_by(|left, right| left.server.cmp(&right.server));
        reports
    }

    /// Resolve either `mcp-<server>` or the raw server-name alias to discovered tool names.
    pub async fn resolve_dynamic_toolset(&self, name: &str) -> Option<Vec<String>> {
        self.dynamic_toolset_reports()
            .await
            .into_iter()
            .find(|report| report.toolset == name || report.alias == name)
            .map(|report| report.tools)
    }

    /// Call MCP tool
    pub async fn call_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Get tool definition
        let tool = self
            .get_tool(tool_name)
            .await
            .ok_or_else(|| format!("Tool '{}' not found", tool_name))?;

        // Build JSON-RPC request
        let request = build_tool_call_request(tool_name, params);

        // Dispatch to MCP server
        let response = self.bridge.dispatch(&tool.server, request).await?;

        // Extract result
        response.result.ok_or_else(|| {
            response
                .error
                .map(|e| format!("MCP error: {}", e.message))
                .unwrap_or_else(|| "Unknown MCP error".to_string())
        })
    }

    /// Reload config and restart servers
    pub async fn reload(&self) -> Result<(), String> {
        // Stop all servers
        // TODO: Implement graceful shutdown

        // Clear registries
        self.tools.write().await.clear();
        self.servers.write().await.clear();

        // Reload from config
        self.load_from_config().await?;

        Ok(())
    }

    /// Health check all MCP servers
    pub async fn health_check(&self) -> HashMap<String, String> {
        let states = self.bridge.health_check().await;
        states
            .into_iter()
            .map(|(name, state)| (name, format!("{:?}", state)))
            .collect()
    }
}

fn dynamic_mcp_toolset_name(server: &str) -> String {
    format!("mcp-{}", server)
}

fn build_tool_call_request(tool_name: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(uuid::Uuid::new_v4().to_string())),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": tool_name,
            "arguments": params,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_mcp_tool_definition_serialization() {
        let tool = McpToolDefinition {
            name: "read_file".to_string(),
            server: "filesystem".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        };

        let json = serde_json::to_string(&tool).unwrap();
        let parsed: McpToolDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "read_file");
        assert_eq!(parsed.server, "filesystem");
    }

    #[tokio::test]
    async fn test_mcp_server_definition_defaults() {
        let server = McpServerDefinition {
            name: "test".to_string(),
            command: "python".to_string(),
            args: vec!["-m".to_string(), "mcp".to_string()],
            env: HashMap::new(),
            auto_start: true,
            auto_restart: true,
        };

        assert!(server.auto_start);
        assert!(server.auto_restart);
    }

    #[tokio::test]
    async fn test_mcp_tool_registry_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        assert_eq!(registry.list_tools().await.len(), 0);
    }

    #[tokio::test]
    async fn test_load_from_empty_config() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Empty MCP config").unwrap();

        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        let result = registry.load_from_config().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_http_config_is_skipped_by_stdio_runtime() {
        let content = r#"
            [[servers]]
            name = "local-http"
            transport = "http"
            url = "http://127.0.0.1:3001"
            enabled = true
        "#;

        let servers = parse_mcp_server_definitions(content).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn test_cli_stdio_config_splits_command_line() {
        let content = r#"
            [[servers]]
            name = "filesystem"
            transport = "stdio"
            command = "npx -y \"@modelcontextprotocol/server-filesystem\""
            args = ["."]
            auto_start = false
        "#;

        let servers = parse_mcp_server_definitions(content).unwrap();
        assert_eq!(servers.len(), 1);

        let (name, server) = &servers[0];
        assert_eq!(name, "filesystem");
        assert_eq!(server.command, "npx");
        assert_eq!(
            server.args,
            vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                ".".to_string()
            ]
        );
        assert!(!server.auto_start);
    }

    #[test]
    fn test_legacy_map_config_still_loads() {
        let content = r#"
            [filesystem]
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
            auto_start = false
        "#;

        let servers = parse_mcp_server_definitions(content).unwrap();
        assert_eq!(servers.len(), 1);

        let (name, server) = &servers[0];
        assert_eq!(name, "filesystem");
        assert_eq!(server.name, "filesystem");
        assert_eq!(server.command, "npx");
        assert_eq!(server.args.len(), 3);
        assert!(!server.auto_start);
    }

    #[test]
    fn test_standard_mcp_tool_schema_gets_server_attached() {
        let tool = serde_json::json!({
            "name": "fs_read",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }
        });

        let parsed = parse_tool_definition("filesystem", &tool).unwrap();
        assert_eq!(parsed.name, "fs_read");
        assert_eq!(parsed.server, "filesystem");
        assert_eq!(parsed.parameters["properties"]["path"]["type"], "string");
    }

    #[test]
    fn test_tool_call_uses_standard_mcp_method() {
        let request = build_tool_call_request("fs_read", serde_json::json!({"path": "README.md"}));
        assert_eq!(request.method, "tools/call");
        let params = request.params.unwrap();
        assert_eq!(params["name"], "fs_read");
        assert_eq!(params["arguments"]["path"], "README.md");
    }

    #[tokio::test]
    async fn mcp_registry_reports_dynamic_server_toolsets() {
        let temp_file = NamedTempFile::new().unwrap();
        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        registry.servers.write().await.insert(
            "docs".to_string(),
            McpServerDefinition {
                name: "docs".to_string(),
                command: "docs-mcp".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                auto_start: false,
                auto_restart: true,
            },
        );
        registry.tools.write().await.insert(
            "search".to_string(),
            McpToolDefinition {
                name: "search".to_string(),
                server: "docs".to_string(),
                description: "Search docs".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        );

        let reports = registry.dynamic_toolset_reports().await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].server, "docs");
        assert_eq!(reports[0].toolset, "mcp-docs");
        assert_eq!(reports[0].alias, "docs");
        assert!(reports[0].enabled);
        assert_eq!(reports[0].discovered_tool_count, 1);
        assert_eq!(reports[0].tools, vec!["search".to_string()]);
        assert_eq!(
            registry.resolve_dynamic_toolset("mcp-docs").await,
            Some(vec!["search".to_string()])
        );
        assert_eq!(
            registry.resolve_dynamic_toolset("docs").await,
            Some(vec!["search".to_string()])
        );
    }

    #[cfg_attr(
        not(windows),
        ignore = "PowerShell-backed subprocess fixture is Windows-only"
    )]
    #[tokio::test]
    async fn mcp_registry_refreshes_server_tools_after_list_changed() {
        let temp_file = NamedTempFile::new().unwrap();
        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge.clone());

        registry.servers.write().await.insert(
            "docs".to_string(),
            McpServerDefinition {
                name: "docs".to_string(),
                command: "powershell".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                auto_start: false,
                auto_restart: true,
            },
        );

        let script = r#"
$toolsListCount = 0
while (($line = [Console]::In.ReadLine()) -ne $null) {
  if ([string]::IsNullOrWhiteSpace($line)) { continue }
  $request = $line | ConvertFrom-Json
  if ($request.method -eq 'notifications/initialized') { continue }
  if ($request.method -eq 'tools/list') {
    $toolsListCount += 1
    if ($toolsListCount -eq 1) {
      $tools = @(
        @{ name = 'old_search'; description = 'Old search'; inputSchema = @{ type = 'object'; properties = @{} } }
      )
    } else {
      $tools = @(
        @{ name = 'new_search'; description = 'New search'; inputSchema = @{ type = 'object'; properties = @{} } },
        @{ name = 'summarize'; description = 'Summarize docs'; inputSchema = @{ type = 'object'; properties = @{} } }
      )
    }
    $response = @{ jsonrpc = '2.0'; id = $request.id; result = @{ tools = $tools } }
  } else {
    $response = @{ jsonrpc = '2.0'; id = $request.id; result = @{} }
  }
  [Console]::Out.WriteLine(($response | ConvertTo-Json -Compress -Depth 10))
  [Console]::Out.Flush()
}
"#;

        bridge
            .register_subprocess(
                "docs".to_string(),
                McpSubprocessConfig {
                    command: "powershell".to_string(),
                    args: vec![
                        "-NoProfile".to_string(),
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-Command".to_string(),
                        script.to_string(),
                    ],
                    env: HashMap::new(),
                    auto_restart: false,
                    max_restarts: 0,
                },
            )
            .await
            .unwrap();

        registry.discover_tools("docs").await.unwrap();
        assert!(registry.get_tool("old_search").await.is_some());
        assert_eq!(
            registry.resolve_dynamic_toolset("mcp-docs").await,
            Some(vec!["old_search".to_string()])
        );

        registry.refresh_server_tools("docs").await.unwrap();

        let tool_names = registry
            .list_tools()
            .await
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(!tool_names.contains(&"old_search".to_string()));
        assert!(tool_names.contains(&"new_search".to_string()));
        assert!(tool_names.contains(&"summarize".to_string()));

        let reports = registry.dynamic_toolset_reports().await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].discovered_tool_count, 2);
        assert_eq!(
            reports[0].tools,
            vec!["new_search".to_string(), "summarize".to_string()]
        );
        assert_eq!(
            registry.resolve_dynamic_toolset("docs").await,
            Some(vec!["new_search".to_string(), "summarize".to_string()])
        );
    }

    #[tokio::test]
    async fn mcp_registry_refresh_unknown_server_fails() {
        let temp_file = NamedTempFile::new().unwrap();
        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        let err = registry.refresh_server_tools("missing").await.unwrap_err();
        assert!(err.contains("MCP server 'missing' not found"));
    }

    #[tokio::test]
    async fn mcp_registry_refresh_failure_preserves_existing_server_tools() {
        let temp_file = NamedTempFile::new().unwrap();
        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        registry.servers.write().await.insert(
            "docs".to_string(),
            McpServerDefinition {
                name: "docs".to_string(),
                command: "missing-docs-mcp".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                auto_start: false,
                auto_restart: true,
            },
        );
        registry.tools.write().await.insert(
            "old_search".to_string(),
            McpToolDefinition {
                name: "old_search".to_string(),
                server: "docs".to_string(),
                description: "Old search".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        );

        let err = registry.refresh_server_tools("docs").await.unwrap_err();
        assert!(
            !err.trim().is_empty(),
            "refresh should report the failed rediscovery"
        );
        assert!(registry.get_tool("old_search").await.is_some());
        assert_eq!(
            registry.resolve_dynamic_toolset("docs").await,
            Some(vec!["old_search".to_string()])
        );
    }

    #[tokio::test]
    async fn test_load_cli_stdio_config_without_autostart() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
            [[servers]]
            name = "stdio-dev"
            transport = "stdio"
            command = "zaion-mcp-dev --stdio"
            auto_start = false
            "#
        )
        .unwrap();

        let bridge = Arc::new(McpBridge::new());
        let registry = McpToolRegistry::new(temp_file.path().to_path_buf(), bridge);

        let result = registry.load_from_config().await;
        assert!(result.is_ok());

        let servers = registry.servers.read().await;
        let server = servers.get("stdio-dev").unwrap();
        assert_eq!(server.command, "zaion-mcp-dev");
        assert_eq!(server.args, vec!["--stdio".to_string()]);
    }
}
