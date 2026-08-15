//! Utility tool handlers: hash_file / compress / decompress / json_validate / yaml_parse.

use serde_json::json;

use super::{resolve_under_workspace, sha256_hex};
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn hash_file_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;

    let resolved = resolve_under_workspace(path, true)?;
    let bytes = std::fs::read(&resolved).map_err(|e| format!("failed to read file: {}", e))?;

    let hash = sha256_hex(&bytes);

    Ok(json!({
        "path": path,
        "sha256": hash,
        "bytes": bytes.len()
    }))
}

pub(super) fn compress_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(text.as_bytes())
        .map_err(|e| format!("compression failed: {}", e))?;
    let compressed = encoder
        .finish()
        .map_err(|e| format!("compression failed: {}", e))?;

    use base64::Engine;
    let base64 = base64::engine::general_purpose::STANDARD.encode(&compressed);

    Ok(json!({
        "original_bytes": text.len(),
        "compressed_bytes": compressed.len(),
        "compression_ratio": text.len() as f64 / compressed.len() as f64,
        "compressed_base64": base64
    }))
}

pub(super) fn decompress_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let compressed_base64 = input
        .get("compressed_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'compressed_base64' parameter".to_string())?;

    use base64::Engine;
    use flate2::read::GzDecoder;
    use std::io::Read;

    let compressed = base64::engine::general_purpose::STANDARD
        .decode(compressed_base64)
        .map_err(|e| format!("base64 decode failed: {}", e))?;

    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder
        .read_to_string(&mut decompressed)
        .map_err(|e| format!("decompression failed: {}", e))?;

    Ok(json!({
        "text": decompressed,
        "bytes": decompressed.len()
    }))
}

pub(super) fn json_validate_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => Ok(json!({
            "valid": true
        })),
        Err(e) => Ok(json!({
            "valid": false,
            "error": e.to_string()
        })),
    }
}

pub(super) fn yaml_parse_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    let parsed: serde_json::Value =
        serde_yaml::from_str(text).map_err(|e| format!("yaml parse failed: {}", e))?;

    Ok(json!({
        "valid": true,
        "json": parsed
    }))
}

// ── hash_text ─────────────────────────────────────────────────────────────────

pub(super) fn hash_text_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    Ok(json!({
        "sha256": sha256_hex(text.as_bytes()),
        "bytes": text.len(),
    }))
}

// ── csv_parse ─────────────────────────────────────────────────────────────────

fn trim_quotes(cell: &str) -> String {
    let c = cell.trim();
    if c.len() >= 2 && c.starts_with('"') && c.ends_with('"') {
        c[1..c.len() - 1].to_string()
    } else {
        c.to_string()
    }
}

pub(super) fn csv_parse_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;
    let has_header = input
        .get("has_header")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let delimiter = input
        .get("delimiter")
        .and_then(|v| v.as_str())
        .and_then(|s| s.chars().next())
        .unwrap_or(',');

    let mut lines = text.lines().filter(|l| !l.is_empty());

    let headers: Option<Vec<String>> = if has_header {
        lines
            .next()
            .map(|h| h.split(delimiter).map(trim_quotes).collect())
    } else {
        None
    };

    let rows: serde_json::Value = if let Some(hdrs) = &headers {
        let objs: Vec<serde_json::Value> = lines
            .map(|line| {
                let cells: Vec<String> = line.split(delimiter).map(trim_quotes).collect();
                let mut obj = serde_json::Map::new();
                for (i, h) in hdrs.iter().enumerate() {
                    obj.insert(h.clone(), json!(cells.get(i).cloned().unwrap_or_default()));
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        json!(objs)
    } else {
        let arrs: Vec<Vec<String>> = lines
            .map(|line| line.split(delimiter).map(trim_quotes).collect())
            .collect();
        json!(arrs)
    };

    let row_count = rows.as_array().map(|a| a.len()).unwrap_or(0);

    Ok(json!({
        "headers": headers,
        "rows": rows,
        "row_count": row_count,
    }))
}

// ── json_format ─────────────────────────────────────────────────────────────

pub(super) fn json_format_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;
    let indent = input.get("indent").and_then(|v| v.as_u64()).unwrap_or(2);

    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid json: {}", e))?;
    let formatted =
        serde_json::to_string_pretty(&value).map_err(|e| format!("serialize failed: {}", e))?;

    Ok(json!({
        "formatted": formatted,
        "valid": true,
        "indent": indent,
    }))
}

// ── random_hex ─────────────────────────────────────────────────────────────

pub(super) fn random_hex_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    use rand::RngCore;

    let n = input
        .get("bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(16)
        .clamp(1, 256) as usize;

    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);

    Ok(json!({
        "hex": hex::encode(&buf),
        "bytes": n,
    }))
}

/// Register the utility tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "hash_file",
            "1.0",
            "Calculate SHA-256 hash of a file.",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to the file to hash",
            )]),
            "utility",
        ),
        hash_file_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "compress",
            "1.0",
            "Compress text using gzip and return base64-encoded result.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "text to compress",
            )]),
            "utility",
        ),
        compress_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "decompress",
            "1.0",
            "Decompress base64-encoded gzip data back to text.",
            McpSchema::new(vec![McpParam::required(
                "compressed_base64",
                McpParamType::String,
                "base64-encoded compressed data",
            )]),
            "utility",
        ),
        decompress_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "json_validate",
            "1.0",
            "Validate JSON syntax and return validity status.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "JSON text to validate",
            )]),
            "utility",
        ),
        json_validate_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "yaml_parse",
            "1.0",
            "Parse YAML text and convert to JSON.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "YAML text to parse",
            )]),
            "utility",
        ),
        yaml_parse_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "hash_text",
            "1.0",
            "Calculate SHA-256 hash of a text string.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "text to hash",
            )]),
            "utility",
        ),
        hash_text_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "csv_parse",
            "1.0",
            "Parse CSV text into rows. With a header, rows are objects; otherwise arrays.",
            McpSchema::new(vec![
                McpParam::required("text", McpParamType::String, "CSV text to parse"),
                McpParam::optional(
                    "has_header",
                    McpParamType::Boolean,
                    "treat the first row as field names (default true)",
                    json!(true),
                ),
                McpParam::optional(
                    "delimiter",
                    McpParamType::String,
                    "single-character field delimiter (default ',')",
                    json!(","),
                ),
            ]),
            "utility",
        ),
        csv_parse_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "json_format",
            "1.0",
            "Pretty-print a JSON string. Returns an error if the JSON is invalid.",
            McpSchema::new(vec![
                McpParam::required("text", McpParamType::String, "JSON text to format"),
                McpParam::optional(
                    "indent",
                    McpParamType::Number,
                    "informational indent width (default 2)",
                    json!(2),
                ),
            ]),
            "utility",
        ),
        json_format_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "random_hex",
            "1.0",
            "Generate cryptographically random bytes as a hex string.",
            McpSchema::new(vec![McpParam::optional(
                "bytes",
                McpParamType::Number,
                "number of random bytes, 1..=256 (default 16)",
                json!(16),
            )]),
            "utility",
        ),
        random_hex_handler,
    ));
}
