//! zaion mcp — MCP server configuration and control plane.
//!
//! Implements the `zaion mcp` command family: serve/add/remove/list/test/configure.
//!
//! ## Subcommands
//!
//! | Command              | Description                                          |
//! |----------------------|------------------------------------------------------|
//! | `add`                | Register a new MCP server entry                      |
//! | `remove <name>`      | Remove a registered MCP server                       |
//! | `list`               | List all registered MCP servers                      |
//! | `configure <name>`   | Update fields of an existing entry                   |
//! | `test [<name>]`      | Probe server health (HTTP) or validate config (stdio)|
//! | `serve`              | Start a local MCP-over-HTTP server                   |
//!
//! Configuration is persisted in `ZAION_HOME/mcp.toml`.
use crate::commands::process::{
    cmd_wake_with_request, structured_wake_request, StreamCallback, StreamEvent, WakeRequest,
};
use crate::commands::{data_dir, CliError};
use crate::config::{McpServerConfig, McpStore, McpTransport, ZaionConfig};
use sha2::{Digest, Sha256};
use zaion_mcp::{register_builtin_tools, McpSandbox, McpSandboxPolicy, McpToolRegistry};
use zaion_runtime::operation_stream::{OperationEvent, OperationStreamCursor};
use zaion_runtime::TurnProof;
use zaion_types::envelope::{ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::policy::{CapabilityClass, PolicyDecision};
use zaion_types::session::{ChannelId, NamespaceKey, RunId, ThreadId};
use zaion_watchdog::toxic::{ToxicHashRegistry, ToxicReason};

// ─── Top-level dispatcher ──────────────────────────────────────────────────

pub fn cmd_mcp(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "add" => cmd_mcp_add(args),
        "remove" | "rm" => cmd_mcp_remove(args),
        "list" | "ls" => cmd_mcp_list(),
        "configure" | "config" => cmd_mcp_configure(args),
        "test" => cmd_mcp_test(args),
        "sandbox" => cmd_mcp_sandbox(args),
        "serve" => cmd_mcp_serve(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown mcp subcommand: '{other}'.\n\
             Use: add, remove/rm, list/ls, configure/config, test, sandbox, serve"
        ))),
    }
}

// ─── list ─────────────────────────────────────────────────────────────────

fn cmd_mcp_list() -> Result<(), CliError> {
    let store = McpStore::load();
    if store.servers.is_empty() {
        println!("No MCP servers configured.");
        println!("Add one with:  zaion mcp add --name <name> --url <url>");
        return Ok(());
    }
    println!(
        "{:<20} {:<8} {:<40} {:<10}",
        "NAME", "TRANSPORT", "URL / COMMAND", "STATUS"
    );
    println!("{}", "-".repeat(82));
    for s in &store.servers {
        let target = match s.transport {
            McpTransport::Http => s.url.as_deref().unwrap_or("(no url)"),
            McpTransport::Stdio => s.command.as_deref().unwrap_or("(no command)"),
        };
        let status = if s.enabled { "enabled" } else { "disabled" };
        println!(
            "{:<20} {:<8} {:<40} {:<10}",
            s.name,
            s.transport.to_string(),
            truncate(target, 40),
            status,
        );
        if !s.args.is_empty() {
            println!("  args: {}", s.args.join(" "));
        }
        if let Some(auth) = &s.auth {
            println!("  auth: {}", auth);
        }
        if let Some(desc) = &s.description {
            println!("  description: {}", desc);
        }
    }
    Ok(())
}

// ─── add ──────────────────────────────────────────────────────────────────

fn cmd_mcp_add(args: &[String]) -> Result<(), CliError> {
    // zaion mcp add --name <n> --url <u>  [--transport http|stdio]
    //               [--command <cmd>] [--description <desc>] [--disabled]
    let name = flag_value(args, "--name")
        .or_else(|| positional_arg(args, 3))
        .ok_or_else(|| CliError::Usage("mcp add requires <name> or --name <name>".to_string()))?;

    let positional_target = positional_arg(args, 4);
    let explicit_transport = flag_value(args, "--transport");
    let flag_url = flag_value(args, "--url");
    let flag_command = flag_value(args, "--command");
    let inferred_transport = if let Some(value) = explicit_transport.as_deref() {
        value.to_string()
    } else if flag_command.is_some() {
        "stdio".to_string()
    } else {
        "http".to_string()
    };
    let transport = parse_transport(&inferred_transport)?;
    let url = flag_url.or_else(|| {
        (transport == McpTransport::Http)
            .then(|| positional_target.clone())
            .flatten()
    });
    let command = flag_command.or_else(|| {
        (transport == McpTransport::Stdio)
            .then(|| positional_target.clone())
            .flatten()
    });
    let mcp_args = flag_values_until_next_flag(args, "--args");
    let auth = flag_value(args, "--auth");
    let description = flag_value(args, "--description");
    let enabled = !args.iter().any(|a| a == "--disabled");

    let entry = McpServerConfig {
        name,
        transport: transport.clone(),
        url,
        command,
        args: mcp_args,
        auth,
        description,
        enabled,
    };
    entry.validate().map_err(CliError::Usage)?;

    let entry_name = entry.name.clone();
    let mut store = McpStore::load();
    if store.exists(&entry_name)
        && args
            .iter()
            .any(|a| a == "--force" || a == "--yes" || a == "-y")
    {
        store.remove(&entry_name).map_err(CliError::Usage)?;
    }
    store.add(entry).map_err(CliError::Usage)?;
    store.save().map_err(CliError::Usage)?;

    println!("MCP server '{}' registered.", entry_name);
    println!("  transport: {}", transport);
    Ok(())
}

// ─── remove ───────────────────────────────────────────────────────────────

fn cmd_mcp_remove(args: &[String]) -> Result<(), CliError> {
    // zaion mcp remove <name>
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("mcp remove requires a server name".to_string()))?;
    let mut store = McpStore::load();
    store.remove(name).map_err(CliError::Usage)?;
    store.save().map_err(CliError::Usage)?;
    println!("MCP server '{}' removed.", name);
    Ok(())
}

// ─── configure ────────────────────────────────────────────────────────────

fn cmd_mcp_configure(args: &[String]) -> Result<(), CliError> {
    // zaion mcp configure <name> [--url <u>] [--command <cmd>]
    //                            [--description <d>] [--enable] [--disable]
    //                            [--transport http|stdio]
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("mcp configure requires a server name".to_string()))?;

    let new_url = flag_value(args, "--url");
    let new_command = flag_value(args, "--command");
    let new_args = args
        .iter()
        .any(|arg| arg == "--args")
        .then(|| flag_values_until_next_flag(args, "--args"));
    let new_auth = flag_value(args, "--auth");
    let new_description = flag_value(args, "--description");
    let new_transport = flag_value(args, "--transport");
    let do_enable = args.iter().any(|a| a == "--enable");
    let do_disable = args.iter().any(|a| a == "--disable");

    if new_url.is_none()
        && new_command.is_none()
        && new_args.is_none()
        && new_auth.is_none()
        && new_description.is_none()
        && new_transport.is_none()
        && !do_enable
        && !do_disable
    {
        return Err(CliError::Usage(
            "mcp configure: provide at least one field to update \
             (--url, --command, --args, --auth, --description, --transport, --enable, --disable)"
                .to_string(),
        ));
    }

    let mut store = McpStore::load();
    store
        .update(name, |entry| {
            if let Some(u) = &new_url {
                entry.url = Some(u.clone());
            }
            if let Some(c) = &new_command {
                entry.command = Some(c.clone());
            }
            if let Some(values) = &new_args {
                entry.args = values.clone();
            }
            if let Some(auth) = &new_auth {
                entry.auth = Some(auth.clone());
            }
            if let Some(d) = &new_description {
                entry.description = Some(d.clone());
            }
            if let Some(t) = &new_transport {
                // Propagate parse error instead of silently ignoring
                match parse_transport(t) {
                    Ok(tr) => entry.transport = tr,
                    Err(e) => return Err(format!("invalid transport: {}", e)),
                }
            }
            if do_enable {
                entry.enabled = true;
            }
            if do_disable {
                entry.enabled = false;
            }
            Ok(())
        })
        .map_err(CliError::Usage)?;

    // Re-validate after mutation.
    let entry = store.find(name).ok_or_else(|| {
        CliError::Usage(format!(
            "MCP server '{}' missing after mutation (race?)",
            name
        ))
    })?;
    entry.validate().map_err(CliError::Usage)?;

    store.save().map_err(CliError::Usage)?;
    println!("MCP server '{}' updated.", name);
    Ok(())
}

// ─── test ─────────────────────────────────────────────────────────────────

fn cmd_mcp_test(args: &[String]) -> Result<(), CliError> {
    // zaion mcp test [<name>]
    // If name omitted, test all enabled servers.
    let store = McpStore::load();
    if store.servers.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    let filter = args.get(3).map(|s| s.as_str());
    let targets: Vec<&McpServerConfig> = store
        .servers
        .iter()
        .filter(|s| filter.map_or(s.enabled, |n| s.name == n))
        .collect();

    if targets.is_empty() {
        return Err(CliError::Usage(format!(
            "No matching MCP server{}",
            filter.map_or("s found".to_string(), |n| format!(" '{}' found", n))
        )));
    }

    let mut any_fail = false;
    for srv in targets {
        print!("  Testing '{}' ({})  ... ", srv.name, srv.transport);
        match probe_server(srv) {
            Ok(msg) => println!("OK  {}", msg),
            Err(msg) => {
                println!("FAIL  {}", msg);
                any_fail = true;
            }
        }
    }

    if any_fail {
        return Err(CliError::Usage(
            "One or more MCP servers failed the probe.".to_string(),
        ));
    }
    Ok(())
}

/// Probe a single server entry. Returns a human-readable status or error.
pub fn probe_server(srv: &McpServerConfig) -> Result<String, String> {
    match srv.transport {
        McpTransport::Stdio => {
            // For stdio we can only validate config — we don't actually spawn here.
            srv.validate()?;
            let args = if srv.args.is_empty() {
                String::new()
            } else {
                format!(" args={}", srv.args.join(" "))
            };
            let auth = srv
                .auth
                .as_deref()
                .map(|auth| format!(" auth={}", auth))
                .unwrap_or_default();
            Ok(format!(
                "config valid (command={}{}{})",
                srv.command.as_deref().unwrap_or("?"),
                args,
                auth
            ))
        }
        McpTransport::Http => {
            let health_url = srv
                .health_url()
                .ok_or_else(|| "no health URL derivable".to_string())?;
            http_probe(&health_url)
        }
    }
}

/// Perform an HTTP GET probe with a 5-second timeout.
fn http_probe(url: &str) -> Result<String, String> {
    use std::time::Duration;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("http client build error: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("request failed: {}", e))?;

    let status = resp.status();
    if status.is_success() {
        // Read at most 256 bytes of body for a summary.
        let body = resp.text().unwrap_or_default();
        let snippet = body.chars().take(80).collect::<String>();
        Ok(format!("HTTP {} — {}", status.as_u16(), snippet))
    } else {
        Err(format!("HTTP {} (non-2xx)", status.as_u16()))
    }
}

// ─── serve ────────────────────────────────────────────────────────────────

fn cmd_mcp_sandbox(args: &[String]) -> Result<(), CliError> {
    let plugin_path = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion mcp sandbox <plugin-file>".to_string()))?;
    let max_source_bytes = flag_value(args, "--max-bytes")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(64 * 1024);
    let max_runtime_ms = flag_value(args, "--max-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(50);
    let policy = McpSandboxPolicy {
        max_source_bytes,
        max_runtime_ms,
        allow_network: args.iter().any(|arg| arg == "--allow-network"),
        allow_filesystem_write: args.iter().any(|arg| arg == "--allow-filesystem-write"),
    };
    let source = std::fs::read_to_string(plugin_path)
        .map_err(|e| CliError::Usage(format!("read plugin failed: {}", e)))?;
    let mut receipt = McpSandbox::inspect_source(&source, &policy);
    let sandbox_dir = data_dir().join("mcp-sandbox");
    std::fs::create_dir_all(&sandbox_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    let toxic = ToxicHashRegistry::new(sandbox_dir.join("toxic.db"));
    toxic.ensure().map_err(|e| CliError::Usage(e.to_string()))?;

    let already_toxic = toxic
        .is_toxic(&receipt.plugin_hash)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    if already_toxic {
        receipt.status = "refused_by_toxic_registry".to_string();
        receipt.cellular_apoptosis = true;
        receipt.reason = Some("toxic_hash_registry_hit".to_string());
    } else if receipt.cellular_apoptosis {
        let reason = match receipt.reason.as_deref() {
            Some("infinite_loop_signature") => ToxicReason::InfiniteLoop,
            Some("memory_budget_exceeded") => ToxicReason::MemoryLeak,
            Some("filesystem_write_capability_blocked") | Some("network_capability_blocked") => {
                ToxicReason::SecurityViolation
            }
            _ => ToxicReason::Manual,
        };
        toxic
            .mark_toxic(
                &receipt.plugin_hash,
                Some(plugin_path),
                reason,
                receipt.reason.as_deref().unwrap_or("sandbox apoptosis"),
            )
            .map_err(|e| CliError::Usage(e.to_string()))?;
    }

    let receipt_path = sandbox_dir.join(format!("{}.json", &receipt.plugin_hash[..16]));
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("mcp sandbox receipt");
    println!("  plugin          : {}", plugin_path);
    println!("  plugin_hash     : {}", receipt.plugin_hash);
    println!("  runtime         : {}", receipt.runtime);
    println!("  external_runtime: {}", receipt.external_runtime);
    println!("  max_runtime_ms  : {}", receipt.max_runtime_ms);
    println!("  source_bytes    : {}", receipt.source_bytes);
    println!("  status          : {}", receipt.status);
    println!("  cellular_apoptosis: {}", receipt.cellular_apoptosis);
    println!(
        "  reason          : {}",
        receipt.reason.as_deref().unwrap_or("none")
    );
    println!(
        "  toxic_registry  : {}",
        sandbox_dir.join("toxic.db").display()
    );
    println!("  receipt_path    : {}", receipt_path.display());

    if receipt.status == "refused_by_toxic_registry" {
        return Err(CliError::Usage(
            "plugin refused by toxic hash registry".to_string(),
        ));
    }
    Ok(())
}

fn cmd_mcp_serve(args: &[String]) -> Result<(), CliError> {
    // zaion mcp serve [-v|--verbose] [--port <p>] [--host <h>]
    let port: u16 = flag_value(args, "--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let host = flag_value(args, "--host").unwrap_or_else(|| "127.0.0.1".to_string());
    let verbose = args.iter().any(|arg| arg == "-v" || arg == "--verbose");

    // Warn if binding to all interfaces
    if host == "0.0.0.0" || host == "::" {
        eprintln!(
            "WARNING: Binding to {} exposes the server to all network interfaces",
            host
        );
    }

    let addr = format!("{}:{}", host, port);
    println!("Starting MCP server on http://{} ...", addr);
    println!("Verbose logging: {}", if verbose { "on" } else { "off" });
    println!("Endpoints:");
    println!("  GET  http://{}/mcp/v1/health", addr);
    println!("  GET  http://{}/mcp/v1/tools", addr);
    println!(
        "  POST http://{}/mcp/v1/call  (requires onboarded identity; writes signed receipt)",
        addr
    );
    println!();
    println!("NOTE: This is a development server. For production use 'zaion gateway serve'.");
    println!("Press Ctrl-C to stop.");

    let listener = std::net::TcpListener::bind(&addr)
        .map_err(|e| CliError::Usage(format!("cannot bind {}: {}", addr, e)))?;

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                if let Err(e) = handle_mcp_request(&mut s) {
                    eprintln!("[mcp serve] request error: {}", e);
                }
            }
            Err(e) => eprintln!("[mcp serve] accept error: {}", e),
        }
    }
    Ok(())
}

/// Minimal HTTP/1.1 handler for the MCP serve loop.
fn handle_mcp_request(stream: &mut std::net::TcpStream) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};

    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;

    // Read remaining headers, then body for POST /mcp/v1/call.
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            break;
        }
        headers.push(trimmed);
    }
    let content_length = headers
        .iter()
        .filter_map(|header| header.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        use std::io::Read;
        reader
            .read_exact(&mut body_bytes)
            .map_err(|e| e.to_string())?;
    }
    let request_body = String::from_utf8_lossy(&body_bytes).into_owned();

    let mut parts = request_line.trim().splitn(3, ' ');
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    let (status, body) = mcp_route_with_body(method, path, &request_body);
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body,
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Route an MCP request and return (status_line, json_body).
pub fn mcp_route(method: &str, path: &str) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/mcp/v1/health") | ("GET", "/mcp/v1/health/") => (
            "200 OK",
            r#"{"status":"ok","service":"zaion-mcp"}"#.to_string(),
        ),
        ("GET", "/mcp/v1/tools") | ("GET", "/mcp/v1/tools/") => {
            // Return currently registered MCP servers (config-level listing).
            let store = McpStore::load();
            let names: Vec<serde_json::Value> = store
                .servers
                .iter()
                .filter(|s| s.enabled)
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "transport": s.transport.to_string(),
                        "url": s.url,
                        "command": s.command,
                        "args": &s.args,
                        "auth": &s.auth,
                        "description": s.description,
                    })
                })
                .collect();
            let body = serde_json::to_string(&serde_json::json!({ "servers": names }))
                .unwrap_or_else(|_| "{}".to_string());
            ("200 OK", body)
        }
        ("POST", "/mcp/v1/call") | ("POST", "/mcp/v1/call/") => {
            let body = serde_json::json!({
                "error": "direct MCP call requires the body-aware architecture route",
                "status": "experimental",
                "stable_path": "send POST bodies through mcp_route_with_body via zaion mcp serve; the legacy no-body route stays disabled",
            })
            .to_string();
            ("501 Not Implemented", body)
        }
        _ => {
            // Use proper JSON serialization to prevent XSS in error responses
            let body = serde_json::json!({
                "error": "not found",
                "path": path
            })
            .to_string();
            ("404 Not Found", body)
        }
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────

/// Route an MCP HTTP request with access to its body.
///
/// Direct tool calls are accepted only through this body-aware route because
/// architecture alignment requires canonical ingress, persisted identity,
/// typed policy proof, and signed parented ledger receipts.
pub fn mcp_route_with_body(method: &str, path: &str, body: &str) -> (&'static str, String) {
    match (method, path) {
        ("POST", "/mcp/v1/call") | ("POST", "/mcp/v1/call/") => execute_mcp_http_call(body),
        _ => mcp_route(method, path),
    }
}

fn execute_mcp_http_call(body: &str) -> (&'static str, String) {
    let request: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                "400 Bad Request",
                serde_json::json!({ "error": format!("invalid JSON body: {}", error) }),
            );
        }
    };
    if request
        .get("runtime_route")
        .and_then(|value| value.as_str())
        == Some("wake")
    {
        return execute_mcp_http_wake_route(&request);
    }
    let Some(tool_name) = request.get("tool_name").and_then(|value| value.as_str()) else {
        return json_response(
            "400 Bad Request",
            serde_json::json!({ "error": "missing tool_name" }),
        );
    };
    let input = request
        .get("input")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let context = request
        .get("context")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let thread_id = context
        .get("thread_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("mcp-http");

    let cfg = ZaionConfig::load();
    let pid = match crate::commands::process::verify_configured_default_pid(&cfg) {
        Ok(Some(pid)) => pid,
        Ok(None) => {
            return json_response(
                "401 Unauthorized",
                serde_json::json!({
                    "error": "mcp HTTP call requires an onboarded default principal",
                    "required_identity": "onboarded long-lived principal",
                }),
            );
        }
        Err(error) => {
            return json_response(
                "401 Unauthorized",
                serde_json::json!({
                    "error": error.to_string(),
                    "required_identity": "onboarded long-lived principal",
                }),
            );
        }
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_process, keypair) = match store.load(&pid) {
        Ok(loaded) => loaded,
        Err(error) => {
            return json_response(
                "401 Unauthorized",
                serde_json::json!({
                    "error": format!("principal identity unavailable: {}", error),
                    "required_identity": "onboarded long-lived principal",
                }),
            );
        }
    };

    let body_for_hash = serde_json::json!({
        "tool_name": tool_name,
        "input": input,
        "context": context,
    })
    .to_string();
    let envelope = match CanonicalEnvelope::new(
        "mcp-http",
        PrincipalId(pid.clone()),
        ChannelId("mcp-http".to_string()),
        ThreadId(thread_id.to_string()),
        format!("mcp-http-{}", uuid::Uuid::new_v4()),
        body_for_hash,
        None,
    )
    .and_then(|envelope| ingest_envelope(&envelope))
    {
        Ok(envelope) => envelope,
        Err(error) => {
            return json_response(
                "400 Bad Request",
                serde_json::json!({
                    "error": format!("canonical envelope rejected: {}", error),
                }),
            );
        }
    };

    let mut registry = McpToolRegistry::new();
    register_builtin_tools(&mut registry);
    let Some(tool) = registry.get(tool_name) else {
        return json_response(
            "404 Not Found",
            serde_json::json!({ "error": format!("unknown MCP tool: {}", tool_name) }),
        );
    };
    let capability_class = CapabilityClass::from_tool_meta(tool.meta.capability_class.as_str());
    let policy_decision = PolicyDecision {
        enforced_at: "zaion_cli::commands::mcp::mcp_route_with_body".to_string(),
        ..PolicyDecision::allow_builtin(tool_name, capability_class)
    };

    let started = std::time::Instant::now();
    let validated_input = match tool.meta.schema.validate_and_fill(&input) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                "400 Bad Request",
                serde_json::json!({
                    "error": error.to_string(),
                    "permission_proof": policy_decision.permission_proof(),
                }),
            );
        }
    };
    let result = match tool.call(validated_input.clone()) {
        Ok(output) => serde_json::json!({
            "success": true,
            "tool_name": tool_name,
            "output": output,
            "error": null,
            "duration_ms": started.elapsed().as_millis() as u64,
        }),
        Err(error) => serde_json::json!({
            "success": false,
            "tool_name": tool_name,
            "output": null,
            "error": error,
            "duration_ms": started.elapsed().as_millis() as u64,
        }),
    };

    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let namespace = NamespaceKey(pid.clone());
    let run_id = RunId(format!("mcp-http-{}", envelope.envelope_id()));
    let mut ingress_payload = envelope.to_channel_received_payload();
    annotate_mcp_receipt_scope(&mut ingress_payload);
    let ingress_event_id = match ledger.append_signed_event(
        &keypair,
        &namespace,
        "channel.received",
        ingress_payload.clone(),
        Some(&run_id),
    ) {
        Ok(event_id) => event_id,
        Err(error) => {
            return json_response(
                "500 Internal Server Error",
                serde_json::json!({
                    "error": format!("failed to append MCP ingress event: {}", error),
                }),
            );
        }
    };
    let input_hash = hash_json(&validated_input);
    let output_hash = hash_json(&result["output"]);
    let call_payload = serde_json::json!({
        "schema": "zaion.mcp_tool_called.v1",
        "source": "mcp-http",
        "principal_id": pid,
        "tool_name": tool_name,
        "capability_class": policy_decision.capability_class,
        "input_hash": input_hash,
        "envelope_id": envelope.envelope_id(),
        "ingress_event_id": ingress_event_id.0,
    });
    let call_event_id = match ledger.append_signed_event_with_parent(
        &keypair,
        &namespace,
        "mcp.tool_called",
        call_payload,
        Some(&run_id),
        Some(&ingress_event_id),
    ) {
        Ok(event_id) => event_id,
        Err(error) => {
            return json_response(
                "500 Internal Server Error",
                serde_json::json!({
                    "error": format!("failed to append MCP tool call event: {}", error),
                }),
            );
        }
    };
    let receipt_payload = serde_json::json!({
        "schema": "zaion.tool_receipt.v1",
        "source": "mcp-http",
        "runtime_scope": "receipt_only",
        "runtime_scope_reason": "MCP direct call executes a tool receipt; route through wake for turn proofs",
        "principal_id": keypair.principal_id().as_str(),
        "tool_name": tool_name,
        "capability_class": policy_decision.capability_class,
        "permission_id": policy_decision.permission_id,
        "policy_effect": policy_decision.effect,
        "sandbox_scope": policy_decision.sandbox_scope,
        "permission_decision": policy_decision.reason_code,
        "permission_proof": policy_decision.permission_proof(),
        "input_hash": input_hash,
        "output_hash": output_hash,
        "success": result["success"],
        "duration_ms": result["duration_ms"],
        "error": result["error"],
        "receipt_status": if result["success"].as_bool().unwrap_or(false) { "executed" } else { "failed" },
        "parent_call_event_id": call_event_id.0,
        "ingress_event_id": ingress_event_id.0,
    });
    let receipt_event_id = match ledger.append_signed_event_with_parent(
        &keypair,
        &namespace,
        "tool.receipt",
        receipt_payload,
        Some(&run_id),
        Some(&call_event_id),
    ) {
        Ok(event_id) => event_id,
        Err(error) => {
            return json_response(
                "500 Internal Server Error",
                serde_json::json!({
                    "error": format!("failed to append MCP tool receipt: {}", error),
                }),
            );
        }
    };

    json_response(
        "200 OK",
        serde_json::json!({
            "schema": "zaion.mcp_http_call.v1",
            "runtime_scope": "receipt_only",
            "proof_chain": null,
            "ingress": ingress_payload,
            "ingress_event_id": ingress_event_id.0,
            "ingress_event_type": "channel.received",
            "call_event_id": call_event_id.0,
            "receipt_event_id": receipt_event_id.0,
            "permission_proof": policy_decision.permission_proof(),
            "result": result,
        }),
    )
}

fn execute_mcp_http_wake_route(request: &serde_json::Value) -> (&'static str, String) {
    let message = request
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| request.get("task").and_then(|value| value.as_str()))
        .or_else(|| {
            request
                .get("input")
                .and_then(|input| input.get("message"))
                .and_then(|value| value.as_str())
        });
    let Some(message) = message.filter(|value| !value.trim().is_empty()) else {
        return json_response(
            "400 Bad Request",
            serde_json::json!({
                "error": "runtime_route wake requires message",
                "runtime_route": "wake",
            }),
        );
    };
    let context = request
        .get("context")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let thread_id = context
        .get("thread_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("mcp-http");

    let cfg = ZaionConfig::load();
    let pid = match crate::commands::process::verify_configured_default_pid(&cfg) {
        Ok(Some(pid)) => pid,
        Ok(None) => {
            return json_response(
                "401 Unauthorized",
                serde_json::json!({
                    "error": "mcp HTTP wake route requires an onboarded default principal",
                    "required_identity": "onboarded long-lived principal",
                }),
            );
        }
        Err(error) => {
            return json_response(
                "401 Unauthorized",
                serde_json::json!({
                    "error": error.to_string(),
                    "required_identity": "onboarded long-lived principal",
                }),
            );
        }
    };
    let process_store = zaion_core::process::ProcessStore::new(data_dir());
    if let Err(error) = process_store.load(&pid) {
        return json_response(
            "401 Unauthorized",
            serde_json::json!({
                "error": format!("principal identity unavailable: {}", error),
                "required_identity": "onboarded long-lived principal",
            }),
        );
    }

    let message_id = format!("mcp-http-wake-{}", uuid::Uuid::new_v4());
    let envelope = match CanonicalEnvelope::new(
        "mcp-http",
        PrincipalId(pid.clone()),
        ChannelId("mcp-http".to_string()),
        ThreadId(thread_id.to_string()),
        message_id,
        message.to_string(),
        None,
    )
    .and_then(|envelope| ingest_envelope(&envelope))
    {
        Ok(envelope) => envelope,
        Err(error) => {
            return json_response(
                "400 Bad Request",
                serde_json::json!({
                    "error": format!("canonical envelope rejected: {}", error),
                }),
            );
        }
    };

    let wake_request = mcp_http_wake_request(
        pid.clone(),
        envelope.clone(),
        cfg.provider.clone(),
        cfg.model.clone(),
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let callback = StreamCallback::new(tx);
    let runtime_result = cmd_wake_with_request(wake_request, Some(callback));
    let transcript = collect_mcp_runtime_stream(rx);
    if let Err(error) = runtime_result {
        return json_response(
            "500 Internal Server Error",
            serde_json::json!({
                "error": format!("wake runtime failed: {}", error),
                "runtime_route": "wake",
                "runtime_warnings": transcript.warnings,
                "stream_contract": mcp_transcript_stream_contract_value(&transcript.operation_events),
            }),
        );
    }
    if let Some(error) = transcript.errors.first() {
        return json_response(
            "500 Internal Server Error",
            serde_json::json!({
                "error": format!("wake runtime emitted error: {}", error),
                "runtime_route": "wake",
                "runtime_warnings": transcript.warnings,
                "stream_contract": mcp_transcript_stream_contract_value(&transcript.operation_events),
            }),
        );
    }

    let ledger = zaion_ledger::EventLedger::new(process_store.ledger_path(&pid));
    let Some(proof) = runtime_proof_for_mcp_http_run(&ledger, "mcp-http", thread_id) else {
        return json_response(
            "500 Internal Server Error",
            serde_json::json!({
                "error": "wake runtime completed without MCP HTTP turn proof",
                "runtime_route": "wake",
                "required_ledger_chain": "channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof",
                "stream_contract": mcp_transcript_stream_contract_value(&transcript.operation_events),
            }),
        );
    };

    let operation_events = crate::commands::operation_backlog::append_shared_operation_backlog(
        &transcript.operation_events,
    );
    let stream_contract = mcp_transcript_stream_contract_value(&operation_events);

    json_response(
        "200 OK",
        serde_json::json!({
            "schema": "zaion.mcp_http_call.v1",
            "runtime_scope": "turn_runtime",
            "runtime_route": "wake",
            "proof_chain": {
                "events": [
                    "channel.received",
                    "omni.route",
                    "channel.sent",
                    "answer.trace",
                    "turn.proof",
                ],
            },
            "ingress": envelope.to_channel_received_payload(),
            "ingress_event_id": proof.ingress_event_id,
            "ingress_event_type": "channel.received",
            "output_event_id": proof.output_event_id,
            "answer_trace_event_id": proof.answer_trace_event_id,
            "turn_proof_event_id": proof.turn_proof_event_id,
            "tool_receipt_ids": proof.tool_receipt_ids,
            "tool_receipt_count": proof.tool_receipt_count,
            "tool_result_storage_receipts": proof.tool_result_storage_receipts,
            "tool_result_storage_receipt_count": proof.tool_result_storage_receipt_count,
            "tool_receipt_proof_join_event_id": proof.tool_receipt_proof_join_event_id,
            "tool_receipt_proof_join": proof.tool_receipt_proof_join,
            "tool_receipt_join_found": proof.tool_receipt_join_found,
            "tool_receipt_proof_hash_verified": proof.tool_receipt_proof_hash_verified,
            "response_text": transcript.response_text,
            "runtime_warnings": transcript.warnings,
            "stream_contract": stream_contract,
        }),
    )
}

fn mcp_http_wake_request(
    pid: String,
    envelope: CanonicalEnvelope,
    provider: Option<String>,
    model: Option<String>,
) -> WakeRequest {
    let mut request = structured_wake_request(pid, envelope.body.clone(), envelope);
    request.provider = provider;
    request.model = model;
    request.stream = false;
    request
}

#[derive(Debug, Default)]
struct McpRuntimeTranscript {
    response_text: String,
    warnings: Vec<String>,
    errors: Vec<String>,
    operation_events: Vec<OperationEvent>,
}

fn collect_mcp_runtime_stream(rx: std::sync::mpsc::Receiver<StreamEvent>) -> McpRuntimeTranscript {
    let mut transcript = McpRuntimeTranscript::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            StreamEvent::Token(token) | StreamEvent::SystemNotice(token) => {
                transcript.response_text.push_str(&token);
            }
            StreamEvent::Warning(warning) | StreamEvent::Status(warning) => {
                transcript.warnings.push(warning);
            }
            StreamEvent::Error(error) => transcript.errors.push(error),
            StreamEvent::Operation(event) => transcript.operation_events.push(event),
            StreamEvent::ToolCall(_) | StreamEvent::Complete { .. } | StreamEvent::Cancelled => {}
        }
    }
    transcript
}

fn mcp_transcript_stream_contract_value(operation_events: &[OperationEvent]) -> serde_json::Value {
    let operation_event_cursor = operation_events
        .last()
        .map(mcp_operation_event_cursor)
        .unwrap_or_default();
    let operation_event_values = operation_events
        .iter()
        .map(mcp_operation_event_payload)
        .collect::<Vec<_>>();

    serde_json::json!({
        "sink": "TranscriptSink",
        "live": false,
        "schema": "zaion.operation_stream.transcript.v1",
        "operation_backlog": "shared_process_local",
        "operation_event_count": operation_events.len(),
        "operation_event_cursor": operation_event_cursor,
        "operation_events": operation_event_values,
    })
}

fn mcp_operation_event_cursor(event: &OperationEvent) -> String {
    OperationStreamCursor::new(event.stream_id.clone(), event.sequence).to_sse_id()
}

fn mcp_operation_event_payload(event: &OperationEvent) -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.operation_event.v1",
        "stream_id": event.stream_id,
        "turn_id": event.turn_id,
        "sequence": event.sequence,
        "timestamp": event.timestamp,
        "principal_id": event.principal_id,
        "channel_id": event.channel_id,
        "thread_id": event.thread_id,
        "stage": event.stage,
        "kind": event.kind,
        "level": event.level,
        "display_text": event.display_text,
        "payload": event.payload,
        "redaction_class": event.redaction_class,
        "ledger_event_id": event.ledger_event_id,
        "proof_hash": event.proof_hash,
        "parent_sequence": event.parent_sequence,
        "cursor": mcp_operation_event_cursor(event),
    })
}

struct McpWakeProof {
    ingress_event_id: String,
    output_event_id: String,
    answer_trace_event_id: String,
    turn_proof_event_id: String,
    tool_receipt_ids: Vec<String>,
    tool_receipt_count: usize,
    tool_result_storage_receipts: Vec<serde_json::Value>,
    tool_result_storage_receipt_count: usize,
    tool_receipt_proof_join_event_id: Option<String>,
    tool_receipt_proof_join: Option<serde_json::Value>,
    tool_receipt_join_found: bool,
    tool_receipt_proof_hash_verified: bool,
}

fn runtime_proof_for_mcp_http_run(
    ledger: &zaion_ledger::EventLedger,
    channel_id: &str,
    thread_id: &str,
) -> Option<McpWakeProof> {
    let events = ledger.list_global_events(100).ok()?;
    let proof = events.iter().find(|event| {
        event.event_type == "turn.proof"
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let ingress_event_id = proof.payload["user_event_id"].as_str()?.to_string();
    let output_event_id = proof.payload["output_event_id"].as_str()?.to_string();
    let answer_trace_event_id = proof.payload["answer_trace_event_id"]
        .as_str()
        .or_else(|| proof.parent_event_id.as_ref().map(|id| id.0.as_str()))?
        .to_string();
    let omni_route_event_id = proof.payload["omni_route_event_id"].as_str()?.to_string();

    let received = events.iter().find(|event| {
        event.event_type == "channel.received"
            && event.event_id.0 == ingress_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let route = events.iter().find(|event| {
        event.event_type == "omni.route"
            && event.event_id.0 == omni_route_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let sent = events.iter().find(|event| {
        event.event_type == "channel.sent"
            && event.event_id.0 == output_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let answer_trace = events.iter().find(|event| {
        event.event_type == "answer.trace"
            && event.event_id.0 == answer_trace_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;

    if [received, route, sent, answer_trace, proof]
        .iter()
        .any(|event| event.signature.is_none())
    {
        return None;
    }
    if route.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(received.event_id.0.as_str())
    {
        return None;
    }
    if route.payload["parent_received_event_id"].as_str() != Some(received.event_id.0.as_str()) {
        return None;
    }
    if sent.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace
        .parent_event_id
        .as_ref()
        .map(|id| id.0.as_str())
        != Some(sent.event_id.0.as_str())
    {
        return None;
    }
    if proof.parent_event_id.as_ref().map(|id| id.0.as_str())
        != Some(answer_trace.event_id.0.as_str())
    {
        return None;
    }
    let route_authority_hash = route.payload["authority_hash"].as_str()?;
    if proof.payload["answer_trace_event_id"].as_str() != Some(answer_trace.event_id.0.as_str()) {
        return None;
    }
    if proof.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }
    if answer_trace.payload["omni_route_event_id"].as_str() != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }

    let decoded_proof = serde_json::from_value::<TurnProof>(proof.payload.clone()).ok()?;
    let receipt_join = crate::commands::receipt_join::tool_receipt_proof_join_for_turn_proof(
        ledger,
        proof,
        &decoded_proof,
    )
    .unwrap_or_default();
    let storage_receipts = crate::commands::receipt_join::tool_result_storage_receipts(
        ledger,
        &decoded_proof.tool_receipt_ids,
    )
    .unwrap_or_default();

    Some(McpWakeProof {
        ingress_event_id,
        output_event_id,
        answer_trace_event_id,
        turn_proof_event_id: proof.event_id.0.clone(),
        tool_receipt_ids: decoded_proof.tool_receipt_ids.clone(),
        tool_receipt_count: decoded_proof.tool_receipt_count,
        tool_result_storage_receipt_count: storage_receipts.receipts.len(),
        tool_result_storage_receipts: storage_receipts.receipts,
        tool_receipt_proof_join_event_id: receipt_join.event_id,
        tool_receipt_proof_join: receipt_join.summary,
        tool_receipt_join_found: receipt_join.found,
        tool_receipt_proof_hash_verified: receipt_join.proof_hash_verified,
    })
}

fn annotate_mcp_receipt_scope(payload: &mut serde_json::Value) {
    if let serde_json::Value::Object(object) = payload {
        object.insert("runtime_scope".to_string(), "receipt_only".into());
        object.insert(
            "runtime_scope_reason".to_string(),
            "MCP direct call executes a tool receipt; route through wake for turn proofs".into(),
        );
    }
}

fn json_response(status: &'static str, value: serde_json::Value) -> (&'static str, String) {
    (
        status,
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn hash_json(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Extract the value of a `--flag value` pair from args.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn flag_values_until_next_flag(args: &[String], flag: &str) -> Vec<String> {
    let Some(start) = args.iter().position(|arg| arg == flag) else {
        return Vec::new();
    };
    args.iter()
        .skip(start + 1)
        .take_while(|arg| !arg.starts_with('-'))
        .cloned()
        .collect()
}

fn positional_arg(args: &[String], index: usize) -> Option<String> {
    args.get(index)
        .filter(|value| !value.starts_with('-'))
        .cloned()
}

/// Parse `"http"` or `"stdio"` into `McpTransport`.
fn parse_transport(s: &str) -> Result<McpTransport, CliError> {
    match s.to_lowercase().as_str() {
        "http" => Ok(McpTransport::Http),
        "stdio" => Ok(McpTransport::Stdio),
        other => Err(CliError::Usage(format!(
            "unknown transport '{}'; use 'http' or 'stdio'",
            other
        ))),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", head)
}

fn print_help() {
    println!("zaion mcp — MCP server control plane");
    println!();
    println!("USAGE:");
    println!("  zaion mcp <subcommand> [args]");
    println!();
    println!("SUBCOMMANDS:");
    println!("  list                              List all registered MCP servers");
    println!("  add  --name <n> --url <u>         Register an HTTP MCP server");
    println!("       --name <n> --transport stdio --command <cmd>    Register a stdio server");
    println!("       [--args <a> ...] [--auth oauth|header] [--description <d>] [--disabled]");
    println!("  remove <name>                     Remove a registered server");
    println!("  configure <name> [--url <u>] [--command <cmd>]");
    println!("                   [--args <a> ...] [--auth oauth|header]");
    println!("                   [--description <d>] [--transport http|stdio]");
    println!("                   [--enable] [--disable]   Update server config");
    println!("  test [<name>]                     Probe health (all enabled, or named)");
    println!("  sandbox <plugin-file>             Inspect plugin in Rust inline sandbox");
    println!("  serve [--port <p>] [--host <h>]   Start local MCP-over-HTTP server");
    println!();
    println!("EXAMPLES:");
    println!("  zaion mcp add --name local --url http://127.0.0.1:3001");
    println!("  zaion mcp add node-server --transport stdio --command npx --args @modelcontextprotocol/server-filesystem .");
    println!("  zaion mcp list");
    println!("  zaion mcp test local");
    println!("  zaion mcp sandbox ./plugin.js --max-ms 50");
    println!("  zaion mcp configure local --disable");
    println!("  zaion mcp remove local");
    println!("  zaion mcp serve --port 3001");
}

// ─── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    fn spawn_openai_compatible_mock(
        expected_requests: usize,
        content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
        let addr = listener.local_addr().expect("mock addr");
        let handle = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut handled = 0;
            while handled < expected_requests && Instant::now() < deadline {
                match listener.accept() {
                    Ok((stream, _)) => {
                        handle_mock_completion_request(stream, content);
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            handled
        });
        (addr, handle)
    }

    fn spawn_openai_tool_call_mock(
        final_content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        spawn_openai_named_tool_call_mock(
            final_content,
            "call_mcp_http_fs_list",
            "fs_list",
            "{\"path\":\".\"}",
        )
    }

    fn spawn_openai_named_tool_call_mock(
        final_content: &'static str,
        call_id: &'static str,
        tool_name: &'static str,
        arguments: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
        let addr = listener.local_addr().expect("mock addr");
        let handle = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut handled = 0;
            while handled < 2 && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _body = read_mock_request_body(&mut stream);
                        if handled == 0 {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": null,
                                            "tool_calls": [{
                                                "id": call_id,
                                                "type": "function",
                                                "function": {
                                                    "name": tool_name,
                                                    "arguments": arguments
                                                }
                                            }]
                                        },
                                        "finish_reason": "tool_calls"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 13,
                                        "completion_tokens": 1
                                    }
                                }),
                            );
                        } else {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": final_content
                                        },
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 19,
                                        "completion_tokens": 5
                                    }
                                }),
                            );
                        }
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            handled
        });
        (addr, handle)
    }

    fn read_mock_request_body(stream: &mut TcpStream) -> String {
        stream
            .set_nonblocking(false)
            .expect("blocking request stream");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        let mut content_length = 0usize;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if line == "\r\n" || line == "\n" {
                break;
            }
            let trimmed = line.trim_end();
            if let Some((name, value)) = trimmed.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }

        let mut request_body = vec![0u8; content_length];
        if content_length > 0 {
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
        }
        String::from_utf8_lossy(&request_body).into_owned()
    }

    fn write_mock_json_response(stream: &mut TcpStream, body: serde_json::Value) {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    }

    fn handle_mock_completion_request(mut stream: TcpStream, content: &str) {
        stream
            .set_nonblocking(false)
            .expect("blocking request stream");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        let mut content_length = 0usize;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if line == "\r\n" || line == "\n" {
                break;
            }
            let trimmed = line.trim_end();
            if let Some((name, value)) = trimmed.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }

        let mut request_body = vec![0u8; content_length];
        if content_length > 0 {
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
        }

        let body = serde_json::json!({
            "model": "llama3.2",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": content,
                },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 7,
            },
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    }

    // ── helper to build args vec ──────────────────────────────────────────
    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    // ── flag_value ────────────────────────────────────────────────────────
    #[test]
    fn test_flag_value_found() {
        let a = args(&[
            "zaion", "mcp", "add", "--name", "local", "--url", "http://x",
        ]);
        assert_eq!(flag_value(&a, "--name"), Some("local".to_string()));
        assert_eq!(flag_value(&a, "--url"), Some("http://x".to_string()));
    }

    #[test]
    fn test_flag_value_missing() {
        let a = args(&["zaion", "mcp", "add"]);
        assert_eq!(flag_value(&a, "--name"), None);
    }

    // ── parse_transport ───────────────────────────────────────────────────
    #[test]
    fn test_parse_transport_http() {
        assert_eq!(parse_transport("http").unwrap(), McpTransport::Http);
    }

    #[test]
    fn test_parse_transport_stdio() {
        assert_eq!(parse_transport("stdio").unwrap(), McpTransport::Stdio);
    }

    #[test]
    fn test_parse_transport_invalid() {
        assert!(parse_transport("grpc").is_err());
    }

    // ── mcp_route ─────────────────────────────────────────────────────────
    #[test]
    fn test_route_health() {
        let (status, body) = mcp_route("GET", "/mcp/v1/health");
        assert_eq!(status, "200 OK");
        assert!(body.contains("ok"));
    }

    #[test]
    fn test_route_tools() {
        let (status, _body) = mcp_route("GET", "/mcp/v1/tools");
        assert_eq!(status, "200 OK");
    }

    #[test]
    fn test_route_call_requires_body_aware_dispatch() {
        let (status, body) = mcp_route("POST", "/mcp/v1/call");
        assert_eq!(status, "501 Not Implemented");
        assert!(body.contains("error"));
        assert!(body.contains("experimental"));
        assert!(body.contains("body-aware"));
        assert!(body.contains("stable_path"));
    }

    #[test]
    fn mcp_http_runtime_route_wake_request_uses_workspace_tool_result_root() {
        let envelope = CanonicalEnvelope::new(
            "mcp-http",
            PrincipalId("did:key:mcp".to_string()),
            ChannelId("mcp-http".to_string()),
            ThreadId("mcp-thread".to_string()),
            "mcp-message".to_string(),
            "mcp wake task".to_string(),
            None,
        )
        .unwrap();
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = mcp_http_wake_request("did:key:mcp".to_string(), envelope, None, None);

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
    fn mcp_http_wake_request_inherits_automatic_compression_without_forcing_it() {
        let envelope = CanonicalEnvelope::new(
            "mcp-http",
            PrincipalId("did:key:mcp".to_string()),
            ChannelId("mcp-http".to_string()),
            ThreadId("mcp-thread".to_string()),
            "mcp-message".to_string(),
            "mcp wake task".to_string(),
            None,
        )
        .unwrap();
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = mcp_http_wake_request(
            "did:key:mcp".to_string(),
            envelope,
            Some("openai".to_string()),
            Some("gpt-5.5".to_string()),
        );
        let disabled = req.effective_features(zaion_runtime::WakeFeatureDefaults::default());
        let enabled = req.effective_features(zaion_runtime::WakeFeatureDefaults {
            compression_enabled: true,
            ..zaion_runtime::WakeFeatureDefaults::default()
        });

        assert_eq!(req.provider.as_deref(), Some("openai"));
        assert_eq!(req.model.as_deref(), Some("gpt-5.5"));
        assert!(!req.stream);
        assert!(!req.compress);
        assert!(!disabled.compression_enabled);
        assert!(!disabled.compression_requested);
        assert!(enabled.compression_enabled);
        assert!(!enabled.compression_requested);
    }

    #[test]
    fn mcp_http_wake_request_preserves_environment_identity_from_envelope_metadata() {
        let envelope = CanonicalEnvelope::new(
            "mcp-http",
            PrincipalId("did:key:mcp".to_string()),
            ChannelId("mcp-http".to_string()),
            ThreadId("mcp-thread".to_string()),
            "mcp-message".to_string(),
            "mcp wake task".to_string(),
            None,
        )
        .unwrap()
        .with_metadata(
            "tool_result_environment",
            serde_json::json!({
                "environment_id": "modal:workspace:mcp:runner-8",
                "environment_kind": "modal",
            }),
        );
        let envelope = ingest_envelope(&envelope).unwrap();

        let req = mcp_http_wake_request("did:key:mcp".to_string(), envelope, None, None);

        assert_eq!(
            req.tool_result_environment_id.as_deref(),
            Some("modal:workspace:mcp:runner-8")
        );
        assert_eq!(req.tool_result_environment_kind.as_deref(), Some("modal"));
    }

    #[test]
    fn direct_mcp_http_call_executes_builtin_tool_with_signed_receipt() {
        let _guard = crate::config::env_test_lock();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-mcp-http-call-{nonce}"));
        let home = root.join("home");
        let zaion_home = root.join("zaion-home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&zaion_home).expect("zaion_home");
        std::fs::create_dir_all(&data).expect("data");
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("ZAION_HOME", &zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &data);

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, _keypair) = store
            .create("mcp-http-call", "test")
            .expect("create process");
        let cfg = crate::config::ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let request = serde_json::json!({
            "tool_name": "shell_exec",
            "input": {
                "command": "echo",
                "args": ["mcp-http-call-proof"]
            },
            "context": {
                "thread_id": "mcp-http-test"
            }
        });
        let (status, body) = mcp_route_with_body("POST", "/mcp/v1/call", &request.to_string());
        assert_eq!(status, "200 OK", "body: {body}");
        let response: serde_json::Value = serde_json::from_str(&body).expect("json body");

        assert_eq!(response["schema"], "zaion.mcp_http_call.v1");
        assert_eq!(response["runtime_scope"], "receipt_only");
        assert_eq!(response["proof_chain"], serde_json::Value::Null);
        assert_eq!(response["ingress"]["runtime_scope"], "receipt_only");
        assert_eq!(
            response["ingress"]["runtime_scope_reason"],
            "MCP direct call executes a tool receipt; route through wake for turn proofs"
        );
        assert_eq!(response["result"]["success"], true);
        assert!(response["result"]["output"]["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("mcp-http-call-proof")));
        assert_eq!(
            response["permission_proof"]["schema"],
            "zaion.policy_decision.v1"
        );
        assert_eq!(
            response["permission_proof"]["permission_id"],
            "builtin.shell_exec.execute"
        );
        assert!(response["call_event_id"]
            .as_str()
            .is_some_and(|event_id| event_id.starts_with("evt-")));
        assert!(response["receipt_event_id"]
            .as_str()
            .is_some_and(|event_id| event_id.starts_with("evt-")));

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let events = ledger
            .list_events(
                &zaion_types::session::SessionKey(process.principal_id.clone()),
                Some("tool.receipt"),
                10,
            )
            .expect("receipt events");
        let receipt = events
            .iter()
            .find(|event| event.event_id.0 == response["receipt_event_id"].as_str().unwrap())
            .expect("matching receipt event");

        assert!(receipt.signature.is_some(), "receipt must be signed");
        assert_eq!(receipt.payload["schema"], "zaion.tool_receipt.v1");
        assert_eq!(receipt.payload["source"], "mcp-http");
        assert_eq!(receipt.payload["runtime_scope"], "receipt_only");
        assert_eq!(
            receipt.payload["permission_id"],
            "builtin.shell_exec.execute"
        );
        assert_eq!(receipt.payload["policy_effect"], "allow");
        assert_eq!(
            receipt.payload["permission_proof"]["enforced_at"],
            "zaion_cli::commands::mcp::mcp_route_with_body"
        );
        assert_eq!(
            receipt
                .parent_event_id
                .as_ref()
                .map(|event_id| event_id.0.as_str()),
            response["call_event_id"].as_str()
        );

        let all_events = ledger.list_global_events(20).expect("global events");
        assert!(all_events
            .iter()
            .all(|event| event.event_type != "turn.proof"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_http_runtime_route_wake_joins_stable_turn_proof_chain() {
        let _guard = crate::config::env_test_lock();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-mcp-http-wake-{nonce}"));
        let home = root.join("home");
        let zaion_home = root.join("zaion-home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&zaion_home).expect("zaion_home");
        std::fs::create_dir_all(&data).expect("data");
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("ZAION_HOME", &zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &data);

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, _keypair) = store
            .create("mcp-http-wake", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_compatible_mock(1, "mcp http wake proof ok");
        let cfg = crate::config::ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let request = serde_json::json!({
            "runtime_route": "wake",
            "message": "prove MCP HTTP can enter wake runtime",
            "context": {
                "thread_id": "mcp-http-wake-test"
            }
        });
        let (status, body) = mcp_route_with_body("POST", "/mcp/v1/call", &request.to_string());
        assert_eq!(status, "200 OK", "body: {body}");
        let response: serde_json::Value = serde_json::from_str(&body).expect("json body");

        assert_eq!(response["schema"], "zaion.mcp_http_call.v1");
        assert_eq!(response["runtime_scope"], "turn_runtime");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(response["response_text"], "mcp http wake proof ok");
        assert_eq!(
            response["proof_chain"]["events"],
            serde_json::json!([
                "channel.received",
                "omni.route",
                "channel.sent",
                "answer.trace",
                "turn.proof"
            ])
        );
        assert_eq!(response["ingress"]["channel_id"], "mcp-http");
        assert_eq!(response["ingress"]["thread_id"], "mcp-http-wake-test");
        assert_eq!(response["ingress_event_type"], "channel.received");
        for field in [
            "ingress_event_id",
            "output_event_id",
            "answer_trace_event_id",
            "turn_proof_event_id",
        ] {
            assert!(
                response[field]
                    .as_str()
                    .is_some_and(|value| value.starts_with("evt-")),
                "missing event id field {field}: {response:#?}"
            );
        }

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let events = ledger.list_global_events(100).expect("global events");
        for (event_type, field) in [
            ("channel.received", "ingress_event_id"),
            ("channel.sent", "output_event_id"),
            ("answer.trace", "answer_trace_event_id"),
            ("turn.proof", "turn_proof_event_id"),
        ] {
            let event_id = response[field].as_str().expect(field);
            let event = events
                .iter()
                .find(|event| event.event_id.0 == event_id && event.event_type == event_type)
                .unwrap_or_else(|| panic!("missing {event_type} {event_id}: {events:#?}"));
            assert!(event.signature.is_some(), "{event_type} must be signed");
            assert_eq!(event.payload["channel_id"], "mcp-http");
            assert_eq!(event.payload["thread_id"], "mcp-http-wake-test");
        }
        let proof = events
            .iter()
            .find(|event| event.event_id.0 == response["turn_proof_event_id"].as_str().unwrap())
            .expect("proof event");
        let answer_trace = events
            .iter()
            .find(|event| event.event_id.0 == response["answer_trace_event_id"].as_str().unwrap())
            .expect("answer trace");
        assert_eq!(
            proof.parent_event_id.as_ref().map(|id| id.0.as_str()),
            Some(answer_trace.event_id.0.as_str())
        );

        assert_eq!(server.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_http_wake_tool_call_exposes_receipt_proof_trace() {
        let _guard = crate::config::env_test_lock();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-mcp-http-wake-tool-{nonce}"));
        let home = root.join("home");
        let zaion_home = root.join("zaion-home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&zaion_home).expect("zaion_home");
        std::fs::create_dir_all(&data).expect("data");
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("ZAION_HOME", &zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &data);

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, _keypair) = store
            .create("mcp-http-wake-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_tool_call_mock("mcp http wake tool proof ok");
        let cfg = crate::config::ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let request = serde_json::json!({
            "runtime_route": "wake",
            "message": "prove MCP HTTP wake tool receipts join turn proof",
            "context": {
                "thread_id": "mcp-http-wake-tool-test"
            }
        });
        let (status, body) = mcp_route_with_body("POST", "/mcp/v1/call", &request.to_string());
        assert_eq!(status, "200 OK", "body: {body}");
        let response: serde_json::Value = serde_json::from_str(&body).expect("json body");

        assert_eq!(response["runtime_scope"], "turn_runtime");
        assert_eq!(response["response_text"], "mcp http wake tool proof ok");
        assert_eq!(
            response["tool_receipt_count"],
            serde_json::json!(1),
            "response should expose wake tool receipt count: {response:#?}"
        );
        let receipt_ids = response["tool_receipt_ids"]
            .as_array()
            .expect("tool receipt ids");
        assert_eq!(receipt_ids.len(), 1, "response: {response:#?}");
        let receipt_id = receipt_ids[0].as_str().expect("receipt id");
        assert!(receipt_id.starts_with("evt-"));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(0),
            "MCP wake response should expose default storage receipt count: {response:#?}"
        );
        assert_eq!(
            response["tool_result_storage_receipts"],
            serde_json::json!([])
        );
        assert_eq!(response["tool_receipt_join_found"], serde_json::json!(true));
        assert_eq!(
            response["tool_receipt_proof_hash_verified"],
            serde_json::json!(true)
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let receipt = ledger
            .get_event(receipt_id)
            .expect("read receipt")
            .expect("receipt event");
        assert_eq!(receipt.event_type, "tool.receipt");
        assert_eq!(receipt.payload["source"], "native-provider");
        assert_eq!(receipt.payload["tool_name"], "fs_list");
        let join = ledger
            .list_events_by_payload_string_array_contains(
                &zaion_types::session::SessionKey(process.principal_id.clone()),
                "tool.receipt.proof_join",
                "tool_receipt_ids",
                receipt_id,
                1,
            )
            .expect("receipt join")
            .into_iter()
            .next()
            .expect("join event");
        assert_eq!(
            join.payload["turn_proof_event_id"],
            response["turn_proof_event_id"]
        );

        assert_eq!(server.join().unwrap(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_http_wake_tool_call_exposes_persisted_storage_receipt_summary() {
        let _guard = crate::config::env_test_lock();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-mcp-http-storage-tool-{nonce}"));
        let home = root.join("home");
        let zaion_home = root.join("zaion-home");
        let data = root.join("data");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&zaion_home).expect("zaion_home");
        std::fs::create_dir_all(&data).expect("data");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let large_file = workspace.join("large-search-source.txt");
        let mut large_content = String::new();
        let long_preview = "x".repeat(1_600);
        for idx in 0..120 {
            large_content.push_str(&format!(
                "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
            ));
        }
        std::fs::write(&large_file, large_content).expect("large search source");

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        let old_cwd = std::env::current_dir().expect("cwd");
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("ZAION_HOME", &zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &data);
        std::env::set_current_dir(&workspace).expect("switch workspace");

        let store = zaion_core::process::ProcessStore::new(&data);
        let (process, _keypair) = store
            .create("mcp-http-storage-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_named_tool_call_mock(
            "mcp http storage tool proof ok",
            "call_mcp_http_fs_search_large",
            "fs_search",
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}",
        );
        let cfg = crate::config::ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let request = serde_json::json!({
            "runtime_route": "wake",
            "message": "prove MCP HTTP wake tool storage receipt summary",
            "context": {
                "thread_id": "mcp-http-storage-tool-test"
            }
        });
        let (status, body) = mcp_route_with_body("POST", "/mcp/v1/call", &request.to_string());
        assert_eq!(status, "200 OK", "body: {body}");
        let response: serde_json::Value = serde_json::from_str(&body).expect("json body");

        assert_eq!(response["runtime_scope"], "turn_runtime");
        assert_eq!(response["response_text"], "mcp http storage tool proof ok");
        assert_eq!(response["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(1),
            "MCP wake response should expose persisted storage receipt summary: {response:#?}"
        );
        let storage_receipts = response["tool_result_storage_receipts"]
            .as_array()
            .expect("storage receipt summaries");
        assert_eq!(storage_receipts.len(), 1, "response: {response:#?}");
        let storage_summary = &storage_receipts[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_mcp_http_fs_search_large")
        );
        assert_eq!(
            storage_summary["tool_result_storage"]["stored"],
            serde_json::json!(true)
        );
        assert_eq!(
            storage_summary["tool_result_storage_binding"]["environment"]["environment_kind"],
            serde_json::json!("storage_target")
        );
        let stored_path = storage_summary["tool_result_storage"]["path"]
            .as_str()
            .expect("stored path");
        assert!(
            stored_path.contains(".zaion") && stored_path.contains("tool-results"),
            "stored path should be workspace-visible: {stored_path}"
        );
        assert!(
            std::path::Path::new(stored_path).exists(),
            "stored output file should exist: {stored_path}"
        );

        assert_eq!(server.join().unwrap(), 2);
        std::env::set_current_dir(old_cwd).expect("restore cwd");
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_route_unknown() {
        let (status, _) = mcp_route("GET", "/not-found");
        assert_eq!(status, "404 Not Found");
    }

    // ── cmd_mcp dispatcher ────────────────────────────────────────────────
    #[test]
    fn test_cmd_mcp_unknown_subcommand() {
        let a = args(&["zaion", "mcp", "frobnicate"]);
        let res = cmd_mcp(&a);
        assert!(res.is_err());
    }

    #[test]
    fn test_cmd_mcp_help() {
        let a = args(&["zaion", "mcp", "help"]);
        assert!(cmd_mcp(&a).is_ok());
    }

    #[test]
    fn test_cmd_mcp_list_default_is_ok() {
        // List on an empty (or real) store should never panic.
        let a = args(&["zaion", "mcp", "list"]);
        let res = cmd_mcp(&a);
        assert!(res.is_ok());
    }

    // ── probe_server (stdio path) ─────────────────────────────────────────
    #[test]
    fn test_probe_stdio_valid_config() {
        let srv = McpServerConfig {
            name: "node".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("node server.js".to_string()),
            args: vec!["--stdio".to_string()],
            auth: Some("header".to_string()),
            description: None,
            enabled: true,
        };
        let result = probe_server(&srv);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let result = result.unwrap();
        assert!(result.contains("config valid"));
        assert!(result.contains("args=--stdio"));
        assert!(result.contains("auth=header"));
    }

    #[test]
    fn test_probe_stdio_invalid_config() {
        let srv = McpServerConfig {
            name: "bad".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: None, // missing command → should fail validation
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(probe_server(&srv).is_err());
    }

    // ── cmd_mcp_add missing --name returns error ──────────────────────────
    #[test]
    fn test_add_missing_name_error() {
        let a = args(&["zaion", "mcp", "add", "--url", "http://x"]);
        assert!(cmd_mcp(&a).is_err());
    }

    // ── cmd_mcp_add missing --url for http returns error ─────────────────
    #[test]
    fn test_add_http_missing_url_error() {
        let a = args(&["zaion", "mcp", "add", "--name", "srv"]);
        assert!(cmd_mcp(&a).is_err());
    }

    // ── cmd_mcp_remove missing name returns error ─────────────────────────
    #[test]
    fn test_remove_missing_name_error() {
        let a = args(&["zaion", "mcp", "remove"]);
        assert!(cmd_mcp(&a).is_err());
    }

    // ── cmd_mcp_configure missing name returns error ──────────────────────
    #[test]
    fn test_configure_missing_name_error() {
        let a = args(&["zaion", "mcp", "configure"]);
        assert!(cmd_mcp(&a).is_err());
    }

    // ── truncate ──────────────────────────────────────────────────────────
    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("hello world this is long", 10);
        assert!(result.len() <= 12); // 9 chars + ellipsis char
        assert!(result.contains('…'));
    }
}
