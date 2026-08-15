//! Network tool handlers: http_get / http_post / dns_lookup / ping / port_check.

use std::time::Duration;

use serde_json::json;

use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn http_get_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;

    let timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let response = ureq::get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .call()
        .map_err(|e| format!("http request failed: {}", e))?;

    let status = response.status();
    let body = response
        .into_string()
        .map_err(|e| format!("failed to read response body: {}", e))?;

    Ok(json!({
        "status": status,
        "body": body,
        "url": url
    }))
}

pub(super) fn http_post_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;
    let body = input
        .get("body")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'body' parameter".to_string())?;

    let timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let response = ureq::post(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send_string(body)
        .map_err(|e| format!("http request failed: {}", e))?;

    let status = response.status();
    let response_body = response
        .into_string()
        .map_err(|e| format!("failed to read response body: {}", e))?;

    Ok(json!({
        "status": status,
        "body": response_body,
        "url": url
    }))
}

pub(super) fn dns_lookup_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let hostname = input
        .get("hostname")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'hostname' parameter".to_string())?;

    let addrs = std::net::ToSocketAddrs::to_socket_addrs(&format!("{}:0", hostname))
        .map_err(|e| format!("dns lookup failed: {}", e))?
        .map(|addr| addr.ip().to_string())
        .collect::<Vec<_>>();

    Ok(json!({
        "hostname": hostname,
        "addresses": addrs
    }))
}

pub(super) fn ping_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let host = input
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'host' parameter".to_string())?;

    let start = std::time::Instant::now();
    let result = std::net::TcpStream::connect_timeout(
        &format!("{}:80", host)
            .parse()
            .map_err(|e| format!("invalid host: {}", e))?,
        Duration::from_secs(5),
    );
    let latency_ms = start.elapsed().as_millis();

    let reachable = result.is_ok();

    Ok(json!({
        "host": host,
        "reachable": reachable,
        "latency_ms": latency_ms
    }))
}

pub(super) fn port_check_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let host = input
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'host' parameter".to_string())?;
    let port = input
        .get("port")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing 'port' parameter".to_string())?;

    let addr = format!("{}:{}", host, port);
    let result = std::net::TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e| format!("invalid address: {}", e))?,
        Duration::from_secs(5),
    );

    let open = result.is_ok();

    Ok(json!({
        "host": host,
        "port": port,
        "open": open
    }))
}

pub(super) fn http_head_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;

    let timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let response = ureq::head(url)
        .timeout(Duration::from_secs(timeout_secs))
        .call()
        .map_err(|e| format!("http request failed: {}", e))?;

    let status = response.status();
    let mut headers = serde_json::Map::new();
    for name in response.headers_names() {
        if let Some(value) = response.header(&name) {
            headers.insert(name, json!(value));
        }
    }

    Ok(json!({
        "status": status,
        "url": url,
        "headers": headers
    }))
}

pub(super) fn http_request_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let method = input
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'method' parameter".to_string())?
        .to_uppercase();
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;

    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD"
    ) {
        return Err(format!("unsupported method: {}", method));
    }

    let timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let body = input.get("body").and_then(|v| v.as_str());

    let request = ureq::request(&method, url).timeout(Duration::from_secs(timeout_secs));

    let response = match body {
        Some(b) if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") => {
            request.send_string(b)
        }
        _ => request.call(),
    }
    .map_err(|e| format!("http request failed: {}", e))?;

    let status = response.status();
    let response_body = response
        .into_string()
        .map_err(|e| format!("failed to read response body: {}", e))?;

    Ok(json!({
        "method": method,
        "url": url,
        "status": status,
        "body": response_body
    }))
}

pub(super) fn url_parse_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;

    let scheme_split = url
        .find("://")
        .ok_or_else(|| "invalid url: missing scheme".to_string())?;
    let scheme = &url[..scheme_split];
    let rest = &url[scheme_split + 3..];

    // Split off the query first.
    let (without_query, query) = match rest.find('?') {
        Some(q) => (&rest[..q], Some(rest[q + 1..].to_string())),
        None => (rest, None),
    };

    // Authority is everything up to the first '/'.
    let (authority, path) = match without_query.find('/') {
        Some(p) => (&without_query[..p], without_query[p..].to_string()),
        None => (without_query, "/".to_string()),
    };

    let (host, port) = match authority.rfind(':') {
        Some(c) => {
            let port_str = &authority[c + 1..];
            match port_str.parse::<u64>() {
                Ok(p) => (authority[..c].to_string(), Some(p)),
                Err(_) => (authority.to_string(), None),
            }
        }
        None => (authority.to_string(), None),
    };

    Ok(json!({
        "url": url,
        "scheme": scheme,
        "host": host,
        "port": port,
        "path": path,
        "query": query
    }))
}

pub(super) fn http_download_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'url' parameter".to_string())?;
    let dest = input
        .get("dest")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'dest' parameter".to_string())?;

    let timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);

    let resolved = super::resolve_under_workspace(dest, false)?;

    let response = ureq::get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .call()
        .map_err(|e| format!("http request failed: {}", e))?;

    const CAP: u64 = 10 * 1024 * 1024;
    // Read one extra byte to detect overflow beyond the cap.
    use std::io::Read;
    let mut reader = response.into_reader().take(CAP + 1);
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("failed to read response body: {}", e))?;

    if buf.len() as u64 > CAP {
        return Err("download exceeds 10 MB cap".to_string());
    }

    let mut file = std::fs::File::create(&resolved)
        .map_err(|e| format!("failed to create dest file: {}", e))?;
    let bytes_written = std::io::copy(&mut buf.as_slice(), &mut file)
        .map_err(|e| format!("failed to write dest file: {}", e))?;

    Ok(json!({
        "url": url,
        "dest": dest,
        "bytes_written": bytes_written
    }))
}

pub(super) fn net_interfaces_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let networks = sysinfo::Networks::new_with_refreshed_list();
    let interfaces: Vec<_> = networks
        .iter()
        .map(|(name, data)| {
            json!({
                "name": name,
                "received_bytes": data.total_received(),
                "transmitted_bytes": data.total_transmitted()
            })
        })
        .collect();

    Ok(json!({
        "count": interfaces.len(),
        "interfaces": interfaces
    }))
}

/// Register the network tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "http_get",
            "1.0",
            "Perform an HTTP GET request and return the response.",
            McpSchema::new(vec![
                McpParam::required("url", McpParamType::String, "URL to send GET request to"),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "request timeout in seconds (default 30)",
                    json!(30),
                ),
            ]),
            "network",
        ),
        http_get_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "http_post",
            "1.0",
            "Perform an HTTP POST request and return the response.",
            McpSchema::new(vec![
                McpParam::required("url", McpParamType::String, "URL to send POST request to"),
                McpParam::required("body", McpParamType::String, "request body content"),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "request timeout in seconds (default 30)",
                    json!(30),
                ),
            ]),
            "network",
        ),
        http_post_handler,
    ));
    registry.register(McpTool::new(
        McpToolMeta::new(
            "dns_lookup",
            "1.0",
            "Resolve a hostname to IP addresses via DNS lookup.",
            McpSchema::new(vec![McpParam::required(
                "hostname",
                McpParamType::String,
                "hostname to resolve",
            )]),
            "network",
        ),
        dns_lookup_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "ping",
            "1.0",
            "Check if a host is reachable and measure latency.",
            McpSchema::new(vec![McpParam::required(
                "host",
                McpParamType::String,
                "hostname or IP address to ping",
            )]),
            "network",
        ),
        ping_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "port_check",
            "1.0",
            "Check if a specific port is open on a host.",
            McpSchema::new(vec![
                McpParam::required("host", McpParamType::String, "hostname or IP address"),
                McpParam::required("port", McpParamType::Number, "port number to check"),
            ]),
            "network",
        ),
        port_check_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "http_head",
            "1.0",
            "Perform an HTTP HEAD request and return status and headers.",
            McpSchema::new(vec![
                McpParam::required("url", McpParamType::String, "URL to send HEAD request to"),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "request timeout in seconds (default 30)",
                    json!(30),
                ),
            ]),
            "network",
        ),
        http_head_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "http_request",
            "1.0",
            "Perform an HTTP request with a custom method (GET/POST/PUT/DELETE/PATCH/HEAD).",
            McpSchema::new(vec![
                McpParam::required(
                    "method",
                    McpParamType::String,
                    "HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD)",
                ),
                McpParam::required("url", McpParamType::String, "URL to send the request to"),
                McpParam::optional(
                    "body",
                    McpParamType::String,
                    "optional request body (for write methods)",
                    json!(""),
                ),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "request timeout in seconds (default 30)",
                    json!(30),
                ),
            ]),
            "network",
        ),
        http_request_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "url_parse",
            "1.0",
            "Parse a URL into scheme, host, port, path, and query components.",
            McpSchema::new(vec![McpParam::required(
                "url",
                McpParamType::String,
                "URL to parse",
            )]),
            "network",
        ),
        url_parse_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "http_download",
            "1.0",
            "Download a URL to a workspace-relative file (max 10 MB).",
            McpSchema::new(vec![
                McpParam::required("url", McpParamType::String, "URL to download"),
                McpParam::required(
                    "dest",
                    McpParamType::String,
                    "workspace-relative destination file path",
                ),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "request timeout in seconds (default 60)",
                    json!(60),
                ),
            ]),
            "write",
        ),
        http_download_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "net_interfaces",
            "1.0",
            "List local network interfaces with received/transmitted byte counts.",
            McpSchema::new(vec![]),
            "network",
        ),
        net_interfaces_handler,
    ));
}
