use crate::{ChunkKind, CodexError, CodexIndex};
/// LSP (Language Server Protocol) skeleton for zaion-codex.
///
/// Implements the LSP 3.17 subset needed by Campaign VI:
///   - textDocument/definition  — jump to symbol definition
///   - textDocument/hover       — type/doc info at cursor
///   - textDocument/completion  — symbol completion
///
/// Transport: stdio (compatible with VS Code, Neovim, helix, etc.)
/// The server reads JSON-RPC 2.0 messages from stdin and writes to stdout.
///
/// Usage:
///   zaion codex lsp --index ZAION_DATA_DIR/codex.db
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};

// ─── JSON-RPC 2.0 types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcMessage {
    id: Option<Value>,
    method: Option<String>,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ─── Server ───────────────────────────────────────────────────────────────

/// Run the LSP server over stdio until the client sends `exit`.
pub fn run_lsp_server(index: &CodexIndex) -> Result<(), CodexError> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut writer = std::io::BufWriter::new(stdout.lock());

    // Send initialize capabilities.
    eprintln!("[zaion-lsp] started, waiting for initialize...");

    loop {
        // Read Content-Length header.
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            break;
        }
        let header = header.trim();
        if !header.starts_with("Content-Length:") {
            continue;
        }
        let length: usize = header["Content-Length:".len()..]
            .trim()
            .parse()
            .unwrap_or(0);
        if length == 0 {
            continue;
        }

        // Skip blank line.
        let mut blank = String::new();
        let _ = reader.read_line(&mut blank);

        // Read body.
        let mut body = vec![0u8; length];
        use std::io::Read;
        let _ = reader.read_exact(&mut body);
        let body_str = String::from_utf8_lossy(&body);

        let msg: RpcMessage = match serde_json::from_str(&body_str) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let id = msg.id.clone().unwrap_or(Value::Null);
        let method = msg.method.as_deref().unwrap_or("");

        let response = match method {
            "initialize" => handle_initialize(id),
            "initialized" => {
                continue;
            } // notification, no response
            "shutdown" => {
                send_response(&mut writer, RpcResponse::ok(id, Value::Null));
                break;
            }
            "exit" => break,
            "textDocument/definition" => handle_definition(id, &msg.params, index),
            "textDocument/hover" => handle_hover(id, &msg.params, index),
            "textDocument/completion" => handle_completion(id, &msg.params, index),
            _ => RpcResponse::err(id, -32601, format!("method not found: {}", method)),
        };

        send_response(&mut writer, response);
    }

    Ok(())
}

fn send_response(writer: &mut impl Write, resp: RpcResponse) {
    let body = serde_json::to_string(&resp).unwrap_or_default();
    let _ = write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = writer.flush();
}

// ─── Handlers ─────────────────────────────────────────────────────────────

fn handle_initialize(id: Value) -> RpcResponse {
    RpcResponse::ok(
        id,
        serde_json::json!({
            "capabilities": {
                "definitionProvider": true,
                "hoverProvider": true,
                "completionProvider": {
                    "triggerCharacters": [":", "."]
                }
            },
            "serverInfo": {
                "name": "zaion-codex-lsp",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_definition(id: Value, params: &Option<Value>, index: &CodexIndex) -> RpcResponse {
    let symbol = extract_word_at_cursor(params);
    let chunks = match index.lookup_exact(&symbol) {
        Ok(c) if !c.is_empty() => c,
        _ => match index.search_by_name(&symbol) {
            Ok(c) => c,
            Err(_) => return RpcResponse::ok(id, Value::Null),
        },
    };

    let locations: Vec<Value> = chunks
        .iter()
        .take(5)
        .map(|c| {
            serde_json::json!({
                "uri": file_to_uri(&c.file_path),
                "range": {
                    "start": { "line": c.start_line.saturating_sub(1), "character": 0 },
                    "end":   { "line": c.end_line.saturating_sub(1),   "character": 0 }
                }
            })
        })
        .collect();

    RpcResponse::ok(id, serde_json::json!(locations))
}

fn handle_hover(id: Value, params: &Option<Value>, index: &CodexIndex) -> RpcResponse {
    let symbol = extract_word_at_cursor(params);
    let chunks = match index.lookup_exact(&symbol) {
        Ok(c) if !c.is_empty() => c,
        _ => return RpcResponse::ok(id, Value::Null),
    };

    let chunk = &chunks[0];
    let mut md = format!(
        "```rust\n{}\n```\n",
        chunk.content.lines().next().unwrap_or("")
    );
    if let Some(ref doc) = chunk.doc_comment {
        md.push_str("\n---\n");
        md.push_str(doc);
    }
    md.push_str(&format!("\n\n*{}:{}*", chunk.file_path, chunk.start_line));

    RpcResponse::ok(
        id,
        serde_json::json!({
            "contents": { "kind": "markdown", "value": md }
        }),
    )
}

fn handle_completion(id: Value, params: &Option<Value>, index: &CodexIndex) -> RpcResponse {
    let prefix = extract_word_at_cursor(params);
    let chunks = match index.search_by_name(&prefix) {
        Ok(c) => c,
        Err(_) => return RpcResponse::ok(id, serde_json::json!({ "items": [] })),
    };

    let items: Vec<Value> = chunks
        .iter()
        .take(20)
        .map(|c| {
            // LSP completion kinds: 1=Text, 2=Method, 3=Function, 5=Field, 6=Variable
            let kind: u32 = match c.kind {
                ChunkKind::Function => 3,
                ChunkKind::Struct => 7, // Class
                ChunkKind::Enum => 13,  // Enum
                ChunkKind::Trait => 8,  // Interface
                ChunkKind::Const => 21, // Constant
                _ => 1,                 // Text
            };
            serde_json::json!({
                "label":         c.name,
                "kind":          kind,
                "detail":        format!("{} ({}:{})", c.kind.as_str(), c.file_path, c.start_line),
                "documentation": c.doc_comment
            })
        })
        .collect();

    RpcResponse::ok(id, serde_json::json!({ "items": items }))
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn extract_word_at_cursor(params: &Option<Value>) -> String {
    // In a real LSP client, we'd read the document text and extract the word.
    // Here we read the symbol from the position context if provided as metadata,
    // or return empty string for graceful fallback.
    params
        .as_ref()
        .and_then(|p| p.get("symbol"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}

fn file_to_uri(path: &str) -> String {
    if path.starts_with('/') || path.starts_with("file://") {
        format!("file://{}", path)
    } else {
        // Windows path: C:\foo\bar → file:///C:/foo/bar
        let normalized = path.replace('\\', "/");
        format!("file:///{}", normalized)
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_unix() {
        let uri = file_to_uri("/home/user/src/lib.rs");
        assert_eq!(uri, "file:///home/user/src/lib.rs");
    }

    #[test]
    fn test_uri_windows() {
        let uri = file_to_uri("D:\\zaion\\zaion\\src\\lib.rs");
        assert_eq!(uri, "file:///D:/zaion/zaion/src/lib.rs");
    }

    #[test]
    fn test_initialize_response() {
        let resp = handle_initialize(serde_json::json!(1));
        let result = resp.result.unwrap();
        assert!(result["capabilities"]["definitionProvider"]
            .as_bool()
            .unwrap());
        assert!(result["capabilities"]["hoverProvider"].as_bool().unwrap());
    }
}
