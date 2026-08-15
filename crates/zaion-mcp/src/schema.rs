use crate::McpError;
/// MCP Schema — parameter type definitions and input validation.
///
/// Implements a subset of JSON Schema sufficient for tool parameter validation:
///   - required/optional fields
///   - basic types: string, number, boolean, array, object
///   - description for each param (used in prompt injection)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Parameter Types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpParamType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

impl McpParamType {
    /// Validate that a JSON value matches this type.
    pub fn validate(&self, value: &serde_json::Value) -> bool {
        match self {
            McpParamType::String => value.is_string(),
            McpParamType::Number => value.is_number(),
            McpParamType::Boolean => value.is_boolean(),
            McpParamType::Array => value.is_array(),
            McpParamType::Object => value.is_object(),
        }
    }
}

// ── McpParam ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpParam {
    pub name: String,
    pub param_type: McpParamType,
    pub description: String,
    pub required: bool,
    /// Optional default value
    pub default: Option<serde_json::Value>,
}

impl McpParam {
    pub fn required(name: &str, param_type: McpParamType, description: &str) -> Self {
        McpParam {
            name: name.to_string(),
            param_type,
            description: description.to_string(),
            required: true,
            default: None,
        }
    }

    pub fn optional(
        name: &str,
        param_type: McpParamType,
        description: &str,
        default: serde_json::Value,
    ) -> Self {
        McpParam {
            name: name.to_string(),
            param_type,
            description: description.to_string(),
            required: false,
            default: Some(default),
        }
    }
}

// ── McpSchema ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSchema {
    pub params: Vec<McpParam>,
}

impl McpSchema {
    pub fn new(params: Vec<McpParam>) -> Self {
        McpSchema { params }
    }

    pub fn empty() -> Self {
        McpSchema { params: vec![] }
    }

    /// Validate and normalise an input object.
    /// Fills in defaults for optional params, errors on missing required params.
    pub fn validate_and_fill(
        &self,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let obj = input
            .as_object()
            .ok_or_else(|| McpError::SchemaValidation("input must be a JSON object".to_string()))?;

        let mut filled: HashMap<String, serde_json::Value> = HashMap::new();

        for param in &self.params {
            match obj.get(&param.name) {
                Some(val) => {
                    if !param.param_type.validate(val) {
                        return Err(McpError::SchemaValidation(format!(
                            "param '{}': expected {:?}, got {}",
                            param.name, param.param_type, val
                        )));
                    }
                    filled.insert(param.name.clone(), val.clone());
                }
                None if param.required => {
                    return Err(McpError::SchemaValidation(format!(
                        "missing required param '{}'",
                        param.name
                    )));
                }
                None => {
                    if let Some(default) = &param.default {
                        filled.insert(param.name.clone(), default.clone());
                    }
                }
            }
        }

        Ok(serde_json::Value::Object(filled.into_iter().collect()))
    }

    /// Generate a human-readable prompt description of this schema.
    pub fn to_prompt_description(&self) -> String {
        if self.params.is_empty() {
            return "No parameters required.".to_string();
        }
        self.params
            .iter()
            .map(|p| {
                let req = if p.required {
                    "(required)"
                } else {
                    "(optional)"
                };
                format!("- {} {:?} {}: {}", p.name, p.param_type, req, p.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn example_schema() -> McpSchema {
        McpSchema::new(vec![
            McpParam::required("query", McpParamType::String, "search query"),
            McpParam::optional("limit", McpParamType::Number, "max results", json!(10)),
        ])
    }

    #[test]
    fn validates_correct_input() {
        let schema = example_schema();
        let input = json!({"query": "test search"});
        let filled = schema.validate_and_fill(&input).unwrap();
        assert_eq!(filled["query"], json!("test search"));
        assert_eq!(filled["limit"], json!(10)); // default filled
    }

    #[test]
    fn rejects_missing_required() {
        let schema = example_schema();
        let input = json!({"limit": 5});
        assert!(schema.validate_and_fill(&input).is_err());
    }

    #[test]
    fn rejects_wrong_type() {
        let schema = example_schema();
        let input = json!({"query": 42}); // should be string
        assert!(schema.validate_and_fill(&input).is_err());
    }

    #[test]
    fn rejects_non_object_input() {
        let schema = example_schema();
        assert!(schema.validate_and_fill(&json!("not an object")).is_err());
    }

    #[test]
    fn prompt_description_not_empty() {
        let schema = example_schema();
        let desc = schema.to_prompt_description();
        assert!(desc.contains("query"));
        assert!(desc.contains("limit"));
    }

    #[test]
    fn empty_schema_accepts_empty_object() {
        let schema = McpSchema::empty();
        let result = schema.validate_and_fill(&json!({})).unwrap();
        assert!(result.as_object().unwrap().is_empty());
    }
}
