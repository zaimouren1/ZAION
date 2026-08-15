//! Text tool handlers: text_diff / text_regex_replace / base64_* / url_* / uuid_generate / json_query.

use serde_json::json;

use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn text_diff_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let left = input
        .get("left")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'left' parameter".to_string())?;
    let right = input
        .get("right")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'right' parameter".to_string())?;

    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();

    // Simple line-level diff: report lines only in left (removed) and only in
    // right (added), preserving order. This is not a minimal-edit diff but is
    // sufficient for surfacing what changed between two text blobs.
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let max = left_lines.len().max(right_lines.len());
    for i in 0..max {
        let l = left_lines.get(i);
        let r = right_lines.get(i);
        if l != r {
            if let Some(rl) = r {
                added.push(json!({ "line": i + 1, "text": rl }));
            }
            if let Some(ll) = l {
                removed.push(json!({ "line": i + 1, "text": ll }));
            }
        }
    }

    Ok(json!({
        "left_lines": left_lines.len(),
        "right_lines": right_lines.len(),
        "added": added,
        "removed": removed,
        "changed": !added.is_empty() || !removed.is_empty()
    }))
}

pub(super) fn text_regex_replace_handler(
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;
    let pattern = input
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'pattern' parameter".to_string())?;
    let replacement = input
        .get("replacement")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'replacement' parameter".to_string())?;

    let re = regex::Regex::new(pattern).map_err(|e| format!("invalid regex: {}", e))?;
    let result = re.replace_all(text, replacement).into_owned();
    let match_count = re.find_iter(text).count();

    Ok(json!({
        "result": result,
        "match_count": match_count
    }))
}

pub(super) fn base64_encode_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());

    Ok(json!({ "encoded": encoded }))
}

pub(super) fn base64_decode_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let encoded = input
        .get("encoded")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'encoded' parameter".to_string())?;

    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("base64 decode failed: {}", e))?;
    let text = String::from_utf8(bytes)
        .map_err(|e| format!("decoded bytes are not valid UTF-8: {}", e))?;

    Ok(json!({ "text": text }))
}

/// Percent-encode all bytes that are not RFC3986 unreserved characters.
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("truncated percent-escape".to_string());
                }
                let hi = (bytes[i + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| "invalid percent-escape".to_string())?;
                let lo = (bytes[i + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| "invalid percent-escape".to_string())?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|e| format!("decoded bytes are not valid UTF-8: {}", e))
}

pub(super) fn url_encode_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    Ok(json!({ "encoded": percent_encode(text) }))
}

pub(super) fn url_decode_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let encoded = input
        .get("encoded")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'encoded' parameter".to_string())?;

    Ok(json!({ "text": percent_decode(encoded)? }))
}

pub(super) fn uuid_generate_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = uuid::Uuid::new_v4();
    Ok(json!({ "uuid": id.to_string() }))
}

pub(super) fn json_query_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;

    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid JSON: {}", e))?;

    // Dot-path query with optional [index] array access, e.g. "items[0].name".
    let mut current = &root;
    for raw_segment in path.split('.').filter(|s| !s.is_empty()) {
        let mut segment = raw_segment;
        // Handle a leading object key before any bracket indices.
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            if !key.is_empty() {
                current = current
                    .get(key)
                    .ok_or_else(|| format!("key '{}' not found", key))?;
            }
            segment = &segment[bracket..];
            // Process one or more [index] groups.
            while segment.starts_with('[') {
                let end = segment
                    .find(']')
                    .ok_or_else(|| "unterminated '[' in path".to_string())?;
                let idx: usize = segment[1..end]
                    .parse()
                    .map_err(|_| format!("invalid array index in '{}'", raw_segment))?;
                current = current
                    .get(idx)
                    .ok_or_else(|| format!("index {} out of range", idx))?;
                segment = &segment[end + 1..];
            }
        } else {
            current = current
                .get(segment)
                .ok_or_else(|| format!("key '{}' not found", segment))?;
        }
    }

    Ok(json!({
        "path": path,
        "value": current.clone()
    }))
}

/// Register the text tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "text_diff",
            "1.0",
            "Compute a line-level diff between two text blobs.",
            McpSchema::new(vec![
                McpParam::required("left", McpParamType::String, "left/original text"),
                McpParam::required("right", McpParamType::String, "right/new text"),
            ]),
            "utility",
        ),
        text_diff_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "text_regex_replace",
            "1.0",
            "Replace all regex matches in text with a replacement string.",
            McpSchema::new(vec![
                McpParam::required("text", McpParamType::String, "input text"),
                McpParam::required(
                    "pattern",
                    McpParamType::String,
                    "regular expression pattern",
                ),
                McpParam::required(
                    "replacement",
                    McpParamType::String,
                    "replacement string (supports $1, $name capture refs)",
                ),
            ]),
            "utility",
        ),
        text_regex_replace_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "base64_encode",
            "1.0",
            "Base64-encode UTF-8 text.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "text to encode",
            )]),
            "utility",
        ),
        base64_encode_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "base64_decode",
            "1.0",
            "Decode base64 text back to UTF-8.",
            McpSchema::new(vec![McpParam::required(
                "encoded",
                McpParamType::String,
                "base64-encoded text",
            )]),
            "utility",
        ),
        base64_decode_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "url_encode",
            "1.0",
            "Percent-encode text for safe use in URLs.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "text to percent-encode",
            )]),
            "utility",
        ),
        url_encode_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "url_decode",
            "1.0",
            "Decode percent-encoded URL text.",
            McpSchema::new(vec![McpParam::required(
                "encoded",
                McpParamType::String,
                "percent-encoded text",
            )]),
            "utility",
        ),
        url_decode_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "uuid_generate",
            "1.0",
            "Generate a random v4 UUID.",
            McpSchema::new(vec![]),
            "utility",
        ),
        uuid_generate_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "json_query",
            "1.0",
            "Extract a value from JSON using a dot/bracket path (e.g. items[0].name).",
            McpSchema::new(vec![
                McpParam::required("text", McpParamType::String, "JSON text to query"),
                McpParam::required(
                    "path",
                    McpParamType::String,
                    "dot/bracket access path, e.g. 'a.b[0].c'",
                ),
            ]),
            "utility",
        ),
        json_query_handler,
    ));
}
