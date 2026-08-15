//! tool_parsers — Multi-format tool call parsers for 11 LLM providers.
//!
//! Hermes equivalent: `environments/tool_call_parsers/` directory (11 files).
//!
//! Different LLM providers embed tool calls in different ways:
//!   - OpenAI/Anthropic: native `tool_calls` JSON arrays (handled by provider.rs)
//!   - DeepSeek V3, GLM, Kimi, Qwen, Llama, Mistral: text-embedded JSON
//!
//! The `ToolCallParser` trait normalizes all formats into `Vec<ToolCall>`.

use crate::provider::ToolCall;
use serde_json::Value;

/// Trait for parsing tool calls from raw LLM text output.
///
/// Implementations handle provider-specific encoding. The `parse` method
/// always returns a (possibly empty) Vec — callers should fall back to
/// treating the raw text as content if the vec is empty.
pub trait ToolCallParser: Send + Sync {
    fn name(&self) -> &'static str;
    fn parse(&self, raw: &str) -> Vec<ToolCall>;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Try to parse a JSON object as a ToolCall (name + input/arguments).
fn json_to_tool_call(id: &str, obj: &Value) -> Option<ToolCall> {
    let name = obj
        .get("name")
        .or_else(|| obj.get("function").and_then(|f| f.get("name")))
        .and_then(|v| v.as_str())?
        .to_string();
    let arguments = obj
        .get("arguments")
        .or_else(|| obj.get("input"))
        .or_else(|| obj.get("parameters"))
        .or_else(|| obj.get("function").and_then(|f| f.get("arguments")))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    // arguments may be a JSON string (needs extra parse) or already an object
    let arguments = match arguments {
        Value::String(s) => {
            serde_json::from_str(&s).unwrap_or(Value::Object(serde_json::Map::new()))
        }
        other => other,
    };

    Some(ToolCall {
        id: id.to_string(),
        name,
        arguments,
    })
}

/// Extract all complete JSON objects from text that start with `{`.
/// Returns them in order of appearance.
fn extract_json_objects(text: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Try to find balanced closing brace
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape_next = false;
            let mut j = i;
            while j < bytes.len() {
                let c = bytes[j];
                if escape_next {
                    escape_next = false;
                } else if in_string {
                    if c == b'\\' {
                        escape_next = true;
                    } else if c == b'"' {
                        in_string = false;
                    }
                } else {
                    match c {
                        b'"' => in_string = true,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                let slice = &text[i..=j];
                                if let Ok(v) = serde_json::from_str::<Value>(slice) {
                                    results.push(v);
                                }
                                i = j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }
            if depth != 0 {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    results
}

/// Extract text between the first occurrence of `open` and the matching `close`.
fn extract_between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)?;
    let inner_start = start + open.len();
    let end = text[inner_start..].find(close)?;
    Some(&text[inner_start..inner_start + end])
}

/// Extract content from markdown code blocks (```json ... ``` or ``` ... ```).
fn extract_code_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("```") {
        remaining = &remaining[start + 3..];
        // Skip language tag if present (e.g., "json\n")
        if let Some(newline) = remaining.find('\n') {
            remaining = &remaining[newline + 1..];
        }
        if let Some(end) = remaining.find("```") {
            blocks.push(&remaining[..end]);
            remaining = &remaining[end + 3..];
        } else {
            break;
        }
    }
    blocks
}

// ── 1. Hermes / OpenAI native ─────────────────────────────────────────────────

/// Native OpenAI/Hermes format: tool_calls is a JSON array.
/// Usually handled by the HTTP layer, but included for completeness.
pub struct HermesParser;
impl ToolCallParser for HermesParser {
    fn name(&self) -> &'static str {
        "hermes"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        // Try top-level tool_calls array
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            if let Some(calls) = v.get("tool_calls").and_then(|t| t.as_array()) {
                return calls
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| json_to_tool_call(&format!("call_{i}"), c))
                    .collect();
            }
        }
        vec![]
    }
}

// ── 2. DeepSeek V3 ────────────────────────────────────────────────────────────

/// DeepSeek V3 embeds tool calls as a JSON object directly in text output.
/// Format: `{"name": "tool", "arguments": {...}}`
pub struct DeepSeekV3Parser;
impl ToolCallParser for DeepSeekV3Parser {
    fn name(&self) -> &'static str {
        "deepseek_v3"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        extract_json_objects(raw)
            .into_iter()
            .enumerate()
            .filter_map(|(i, obj)| json_to_tool_call(&format!("ds_{i}"), &obj))
            .collect()
    }
}

// ── 3. DeepSeek V3.1 ──────────────────────────────────────────────────────────

/// DeepSeek V3.1 wraps tool calls in `<tool_call>...</tool_call>` XML tags.
pub struct DeepSeekV3_1Parser;
impl ToolCallParser for DeepSeekV3_1Parser {
    fn name(&self) -> &'static str {
        "deepseek_v3_1"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        let mut results = Vec::new();
        let mut remaining = raw;
        while let Some(content) = extract_between(remaining, "<tool_call>", "</tool_call>") {
            if let Ok(v) = serde_json::from_str::<Value>(content.trim()) {
                let id = format!("ds1_{}", results.len());
                if let Some(tc) = json_to_tool_call(&id, &v) {
                    results.push(tc);
                }
            }
            // Advance past this tag
            let skip =
                remaining.find("<tool_call>").unwrap_or(0) + "<tool_call>".len() + content.len();
            remaining = &remaining[skip.min(remaining.len())..];
        }
        results
    }
}

// ── 4. Mistral ────────────────────────────────────────────────────────────────

/// Mistral format: function_call field with `{"function": {"name": ..., "arguments": ...}}`
pub struct MistralParser;
impl ToolCallParser for MistralParser {
    fn name(&self) -> &'static str {
        "mistral"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            if let Some(calls) = v.get("tool_calls").and_then(|t| t.as_array()) {
                return calls
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        let func = c.get("function")?;
                        let name = func.get("name")?.as_str()?.to_string();
                        let args_raw = func.get("arguments")?;
                        let arguments = match args_raw {
                            Value::String(s) => {
                                serde_json::from_str(s).unwrap_or(Value::Object(Default::default()))
                            }
                            other => other.clone(),
                        };
                        Some(ToolCall {
                            id: format!("mist_{i}"),
                            name,
                            arguments,
                        })
                    })
                    .collect();
            }
        }
        // Fallback: bare JSON objects
        extract_json_objects(raw)
            .into_iter()
            .enumerate()
            .filter_map(|(i, obj)| {
                if obj.get("function").is_some() {
                    json_to_tool_call(&format!("mist_{i}"), &obj)
                } else {
                    None
                }
            })
            .collect()
    }
}

// ── 5. Llama 3 JSON ───────────────────────────────────────────────────────────

/// Llama 3 encodes tool calls in ```json code blocks.
pub struct Llama3JsonParser;
impl ToolCallParser for Llama3JsonParser {
    fn name(&self) -> &'static str {
        "llama3_json"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        let mut results = Vec::new();
        for block in extract_code_blocks(raw) {
            if let Ok(v) = serde_json::from_str::<Value>(block.trim()) {
                match &v {
                    Value::Array(arr) => {
                        for (i, item) in arr.iter().enumerate() {
                            if let Some(tc) = json_to_tool_call(&format!("ll_{i}"), item) {
                                results.push(tc);
                            }
                        }
                    }
                    Value::Object(_) => {
                        if let Some(tc) = json_to_tool_call(&format!("ll_{}", results.len()), &v) {
                            results.push(tc);
                        }
                    }
                    _ => {}
                }
            }
        }
        results
    }
}

// ── 6. Longcat ────────────────────────────────────────────────────────────────

/// Longcat format: `<functioncall>{"name": ..., "arguments": {...}}</functioncall>`
pub struct LongcatParser;
impl ToolCallParser for LongcatParser {
    fn name(&self) -> &'static str {
        "longcat"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        let mut results = Vec::new();
        let mut remaining = raw;
        while let Some(content) = extract_between(remaining, "<functioncall>", "</functioncall>") {
            if let Ok(v) = serde_json::from_str::<Value>(content.trim()) {
                let id = format!("lc_{}", results.len());
                if let Some(tc) = json_to_tool_call(&id, &v) {
                    results.push(tc);
                }
            }
            let skip = remaining.find("<functioncall>").unwrap_or(0)
                + "<functioncall>".len()
                + content.len();
            remaining = &remaining[skip.min(remaining.len())..];
        }
        results
    }
}

// ── 7. GLM-4.5 ────────────────────────────────────────────────────────────────

/// GLM-4.5 embeds tool calls as JSON objects in plain text output.
/// Format: `{"name": "tool_name", "arguments": {...}}`
pub struct Glm45Parser;
impl ToolCallParser for Glm45Parser {
    fn name(&self) -> &'static str {
        "glm45"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        extract_json_objects(raw)
            .into_iter()
            .enumerate()
            .filter_map(|(i, obj)| {
                if obj.get("name").is_some()
                    && (obj.get("arguments").is_some() || obj.get("input").is_some())
                {
                    json_to_tool_call(&format!("glm45_{i}"), &obj)
                } else {
                    None
                }
            })
            .collect()
    }
}

// ── 8. GLM-4.7 ────────────────────────────────────────────────────────────────

/// GLM-4.7 uses `tool_calls` array with `type: "function"` structure.
pub struct Glm47Parser;
impl ToolCallParser for Glm47Parser {
    fn name(&self) -> &'static str {
        "glm47"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            if let Some(calls) = v.get("tool_calls").and_then(|t| t.as_array()) {
                return calls
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        if c.get("type").and_then(|t| t.as_str()) == Some("function") {
                            json_to_tool_call(&format!("glm47_{i}"), c)
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        }
        // Fallback to GLM-4.5 style
        Glm45Parser.parse(raw)
    }
}

// ── 9. Kimi-K2 ────────────────────────────────────────────────────────────────

/// Kimi-K2 (MoonShot) tool call format.
/// Uses `<|tool_calls|>` delimiter followed by JSON array.
pub struct KimiK2Parser;
impl ToolCallParser for KimiK2Parser {
    fn name(&self) -> &'static str {
        "kimi_k2"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        // Try <|tool_calls|> delimiter
        if let Some(content) = extract_between(raw, "<|tool_calls|>", "<|") {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content.trim()) {
                return arr
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| json_to_tool_call(&format!("kimi_{i}"), c))
                    .collect();
            }
        }
        // Fallback: JSON array anywhere in text
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(raw.trim()) {
            return arr
                .iter()
                .enumerate()
                .filter_map(|(i, c)| json_to_tool_call(&format!("kimi_{i}"), c))
                .collect();
        }
        // Fallback: bare JSON objects
        extract_json_objects(raw)
            .into_iter()
            .enumerate()
            .filter_map(|(i, obj)| json_to_tool_call(&format!("kimi_{i}"), &obj))
            .collect()
    }
}

// ── 10. Qwen3-Coder ───────────────────────────────────────────────────────────

/// Qwen3-Coder format: `<tool_call>\n{...}\n</tool_call>` blocks.
pub struct Qwen3CoderParser;
impl ToolCallParser for Qwen3CoderParser {
    fn name(&self) -> &'static str {
        "qwen3_coder"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        let mut results = Vec::new();
        let mut remaining = raw;
        while let Some(content) = extract_between(remaining, "<tool_call>", "</tool_call>") {
            if let Ok(v) = serde_json::from_str::<Value>(content.trim()) {
                let id = format!("qwen3_{}", results.len());
                if let Some(tc) = json_to_tool_call(&id, &v) {
                    results.push(tc);
                }
            }
            let skip =
                remaining.find("<tool_call>").unwrap_or(0) + "<tool_call>".len() + content.len();
            remaining = &remaining[skip.min(remaining.len())..];
        }
        results
    }
}

// ── 11. Qwen (general) ────────────────────────────────────────────────────────

/// General Qwen format: `✿FUNCTION✿: tool_name\n✿ARGS✿: {...}` or JSON objects.
pub struct QwenParser;
impl ToolCallParser for QwenParser {
    fn name(&self) -> &'static str {
        "qwen"
    }
    fn parse(&self, raw: &str) -> Vec<ToolCall> {
        let mut results = Vec::new();
        // Try ✿FUNCTION✿ / ✿ARGS✿ format
        if raw.contains("✿FUNCTION✿") {
            let mut remaining = raw;
            while let Some(fn_pos) = remaining.find("✿FUNCTION✿:") {
                remaining = &remaining[fn_pos + "✿FUNCTION✿:".len()..];
                let name = remaining
                    .trim_start()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let arguments =
                    if let Some(args_content) = extract_between(remaining, "✿ARGS✿:", "\n✿") {
                        serde_json::from_str(args_content.trim())
                            .unwrap_or(Value::Object(Default::default()))
                    } else if let Some(args_content) = remaining
                        .find("✿ARGS✿:")
                        .map(|p| &remaining[p + "✿ARGS✿:".len()..])
                    {
                        serde_json::from_str(args_content.trim())
                            .unwrap_or(Value::Object(Default::default()))
                    } else {
                        Value::Object(Default::default())
                    };
                results.push(ToolCall {
                    id: format!("qwen_{}", results.len()),
                    name,
                    arguments,
                });
            }
            if !results.is_empty() {
                return results;
            }
        }
        // Fallback: JSON objects in text
        extract_json_objects(raw)
            .into_iter()
            .enumerate()
            .filter_map(|(i, obj)| json_to_tool_call(&format!("qwen_{i}"), &obj))
            .collect()
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Create a parser by name.
pub fn get_parser(name: &str) -> Option<Box<dyn ToolCallParser>> {
    match name {
        "hermes" => Some(Box::new(HermesParser)),
        "deepseek_v3" => Some(Box::new(DeepSeekV3Parser)),
        "deepseek_v3_1" => Some(Box::new(DeepSeekV3_1Parser)),
        "mistral" => Some(Box::new(MistralParser)),
        "llama3_json" => Some(Box::new(Llama3JsonParser)),
        "longcat" => Some(Box::new(LongcatParser)),
        "glm45" => Some(Box::new(Glm45Parser)),
        "glm47" => Some(Box::new(Glm47Parser)),
        "kimi_k2" => Some(Box::new(KimiK2Parser)),
        "qwen3_coder" => Some(Box::new(Qwen3CoderParser)),
        "qwen" => Some(Box::new(QwenParser)),
        _ => None,
    }
}

/// Try all parsers in priority order; return the first non-empty result.
/// Used as a fallback when the provider format is unknown.
pub fn try_all_parsers(raw: &str) -> Vec<ToolCall> {
    // Priority order: XML-tagged formats first (most specific), then JSON-object formats
    let priority = [
        "deepseek_v3_1",
        "qwen3_coder",
        "longcat", // XML-tagged (most specific)
        "kimi_k2", // delimiter-based
        "hermes",
        "mistral",
        "llama3_json", // wrapper JSON
        "glm47",
        "glm45",
        "deepseek_v3",
        "qwen", // bare JSON
    ];
    for name in &priority {
        if let Some(parser) = get_parser(name) {
            let calls = parser.parse(raw);
            if !calls.is_empty() {
                return calls;
            }
        }
    }
    vec![]
}

pub fn all_parser_names() -> &'static [&'static str] {
    &[
        "hermes",
        "deepseek_v3",
        "deepseek_v3_1",
        "mistral",
        "llama3_json",
        "longcat",
        "glm45",
        "glm47",
        "kimi_k2",
        "qwen3_coder",
        "qwen",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_parser_tool_calls_array() {
        let raw = r#"{"tool_calls": [{"name": "read_file", "arguments": {"path": "/tmp/x"}}]}"#;
        let calls = HermesParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn deepseek_v3_bare_json() {
        let raw = r#"I'll use a tool. {"name": "bash", "arguments": {"cmd": "ls"}}"#;
        let calls = DeepSeekV3Parser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "bash");
    }

    #[test]
    fn deepseek_v3_1_xml_tags() {
        let raw = r#"<tool_call>{"name": "write_file", "arguments": {"path": "/a", "content": "hi"}}</tool_call>"#;
        let calls = DeepSeekV3_1Parser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
    }

    #[test]
    fn mistral_function_field() {
        let raw = r#"{"tool_calls": [{"type": "function", "function": {"name": "search", "arguments": "{\"q\": \"rust\"}"}}]}"#;
        let calls = MistralParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search");
    }

    #[test]
    fn llama3_json_code_block() {
        let raw = "Sure!\n```json\n{\"name\": \"grep\", \"arguments\": {\"pattern\": \"fn\"}}\n```";
        let calls = Llama3JsonParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grep");
    }

    #[test]
    fn longcat_functioncall_tags() {
        let raw =
            r#"<functioncall>{"name": "glob", "arguments": {"pattern": "**/*.rs"}}</functioncall>"#;
        let calls = LongcatParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "glob");
    }

    #[test]
    fn glm45_bare_json_with_name_and_arguments() {
        let raw = r#"{"name": "memory_search", "arguments": {"query": "rust async"}}"#;
        let calls = Glm45Parser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_search");
    }

    #[test]
    fn glm47_tool_calls_array() {
        let raw = r#"{"tool_calls": [{"type": "function", "name": "list_dir", "arguments": {"path": "."}}]}"#;
        let calls = Glm47Parser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_dir");
    }

    #[test]
    fn kimi_k2_json_array() {
        let raw = r#"[{"name": "web_search", "arguments": {"query": "Kimi K2"}}]"#;
        let calls = KimiK2Parser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
    }

    #[test]
    fn qwen3_coder_xml_tags() {
        let raw =
            r#"<tool_call>{"name": "read_file", "arguments": {"path": "main.rs"}}</tool_call>"#;
        let calls = Qwen3CoderParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn qwen_star_format() {
        let raw = "✿FUNCTION✿: bash\n✿ARGS✿: {\"cmd\": \"pwd\"}\n✿RESULT✿:";
        let calls = QwenParser.parse(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "bash");
    }

    #[test]
    fn get_parser_returns_correct_impl() {
        assert_eq!(get_parser("glm45").unwrap().name(), "glm45");
        assert_eq!(get_parser("kimi_k2").unwrap().name(), "kimi_k2");
        assert!(get_parser("unknown_xyz").is_none());
    }

    #[test]
    fn try_all_parsers_xml_format_wins() {
        let raw = r#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#;
        let calls = try_all_parsers(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ls");
    }

    #[test]
    fn try_all_parsers_returns_empty_for_plain_text() {
        let raw = "Hello world! No tool calls here.";
        let calls = try_all_parsers(raw);
        assert!(calls.is_empty());
    }

    #[test]
    fn all_11_parsers_registered() {
        assert_eq!(all_parser_names().len(), 11);
        for name in all_parser_names() {
            assert!(
                get_parser(name).is_some(),
                "parser '{}' not in registry",
                name
            );
        }
    }
}
