use crate::McpSchema;
use serde::{Deserialize, Serialize};
/// MCP Tool Registry — register, lookup, and enumerate MCP tools.
///
/// Tools are registered by name+version. The registry is the single source
/// of truth for what capabilities Zaion exposes to the LLM.
///
/// Tool handlers are synchronous closures:
///   `Fn(serde_json::Value) → Result<serde_json::Value, String>`
///
/// For async tools, wrap in `tokio::task::block_in_place`.
use std::collections::HashMap;
use std::sync::Arc;

// ── McpToolMeta ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub schema: McpSchema,
    /// "read" | "write" | "execute" — for policy enforcement
    pub capability_class: String,
}

impl McpToolMeta {
    pub fn new(
        name: &str,
        version: &str,
        description: &str,
        schema: McpSchema,
        capability_class: &str,
    ) -> Self {
        McpToolMeta {
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            schema,
            capability_class: capability_class.to_string(),
        }
    }
}

// ── McpTool ───────────────────────────────────────────────────────────────────

pub struct McpTool {
    pub meta: McpToolMeta,
    handler: Arc<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>,
}

impl McpTool {
    pub fn new<F>(meta: McpToolMeta, handler: F) -> Self
    where
        F: Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
    {
        McpTool {
            meta,
            handler: Arc::new(handler),
        }
    }

    pub fn call(&self, input: serde_json::Value) -> Result<serde_json::Value, String> {
        (self.handler)(input)
    }
}

// ── McpToolRegistry ───────────────────────────────────────────────────────────

pub struct McpToolRegistry {
    /// key: "name@version"
    tools: HashMap<String, McpTool>,
}

impl McpToolRegistry {
    pub fn new() -> Self {
        McpToolRegistry {
            tools: HashMap::new(),
        }
    }

    fn key(name: &str, version: &str) -> String {
        format!("{name}@{version}")
    }

    /// Register a tool. Overwrites if name+version already registered.
    pub fn register(&mut self, tool: McpTool) {
        let k = Self::key(&tool.meta.name, &tool.meta.version);
        self.tools.insert(k, tool);
    }

    /// Look up by name (returns latest if multiple versions, else exact match).
    pub fn get(&self, name: &str) -> Option<&McpTool> {
        // Prefer exact "name@latest" or pick first match by name
        self.tools
            .values()
            .filter(|t| t.meta.name == name)
            .max_by_key(|t| t.meta.version.as_str())
    }

    pub fn get_versioned(&self, name: &str, version: &str) -> Option<&McpTool> {
        self.tools.get(&Self::key(name, version))
    }

    pub fn list_meta(&self) -> Vec<&McpToolMeta> {
        let mut metas: Vec<&McpToolMeta> = self.tools.values().map(|t| &t.meta).collect();
        metas.sort_by_key(|m| m.name.as_str());
        metas
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Generate a combined prompt listing all available tools (for LLM system prompt injection).
    pub fn to_tools_prompt(&self) -> String {
        let metas = self.list_meta();
        if metas.is_empty() {
            return "No tools available.".to_string();
        }
        let mut lines = vec!["Available tools:".to_string()];
        for m in metas {
            lines.push(format!(
                "\n## {} (v{}) [{}]\n{}\n\nParameters:\n{}",
                m.name,
                m.version,
                m.capability_class,
                m.description,
                m.schema.to_prompt_description(),
            ));
        }
        lines.join("\n")
    }
}

impl Default for McpToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{McpParam, McpParamType, McpSchema};
    use serde_json::json;

    fn echo_tool() -> McpTool {
        let meta = McpToolMeta::new(
            "echo",
            "1.0",
            "Echoes the input back",
            McpSchema::new(vec![McpParam::required(
                "message",
                McpParamType::String,
                "message to echo",
            )]),
            "read",
        );
        McpTool::new(meta, |input| Ok(json!({ "echo": input["message"] })))
    }

    fn calc_tool() -> McpTool {
        let meta = McpToolMeta::new(
            "calc",
            "1.0",
            "Simple calculator",
            McpSchema::new(vec![
                McpParam::required("a", McpParamType::Number, "first operand"),
                McpParam::required("b", McpParamType::Number, "second operand"),
            ]),
            "read",
        );
        McpTool::new(meta, |input| {
            let a = input["a"].as_f64().unwrap_or(0.0);
            let b = input["b"].as_f64().unwrap_or(0.0);
            Ok(json!({ "result": a + b }))
        })
    }

    #[test]
    fn register_and_get_by_name() {
        let mut reg = McpToolRegistry::new();
        reg.register(echo_tool());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn call_echo_tool() {
        let mut reg = McpToolRegistry::new();
        reg.register(echo_tool());
        let tool = reg.get("echo").unwrap();
        let result = tool.call(json!({"message": "hello"})).unwrap();
        assert_eq!(result["echo"], json!("hello"));
    }

    #[test]
    fn list_meta_sorted() {
        let mut reg = McpToolRegistry::new();
        reg.register(calc_tool());
        reg.register(echo_tool());
        let names: Vec<&str> = reg.list_meta().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["calc", "echo"]);
    }

    #[test]
    fn tools_prompt_contains_tool_names() {
        let mut reg = McpToolRegistry::new();
        reg.register(echo_tool());
        let prompt = reg.to_tools_prompt();
        assert!(prompt.contains("echo"));
    }

    #[test]
    fn registry_len() {
        let mut reg = McpToolRegistry::new();
        assert_eq!(reg.len(), 0);
        reg.register(echo_tool());
        reg.register(calc_tool());
        assert_eq!(reg.len(), 2);
    }
}
