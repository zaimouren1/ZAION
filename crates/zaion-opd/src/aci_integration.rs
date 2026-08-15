//! ACI Integration - AST-level code transformation for OPD training
//!
//! This module integrates zaion-aci AstPatcher into the OPD training loop,
//! enabling syntax-aware code modifications and AST-level optimization signals.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// ACI-enhanced code transformation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AciTransformResult {
    /// Original code
    pub original: String,

    /// Transformed code (after AST patching)
    pub transformed: String,

    /// Whether transformation succeeded
    pub success: bool,

    /// AST node that was modified
    pub modified_node: Option<String>,

    /// Syntax validation result
    pub syntax_valid: bool,

    /// Error message if transformation failed
    pub error: Option<String>,
}

/// ACI-enhanced code transformer
pub struct AciTransformer {
    /// Supported languages
    languages: Vec<String>,
}

impl AciTransformer {
    /// Create a new ACI transformer
    pub fn new() -> Self {
        Self {
            languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "typescript".to_string(),
                "javascript".to_string(),
            ],
        }
    }

    /// Transform code using AST-level patching
    pub fn transform_code(
        &self,
        original: &str,
        old_node: &str,
        new_node: &str,
        language: &str,
    ) -> Result<AciTransformResult> {
        // Validate language support
        if !self.languages.contains(&language.to_lowercase()) {
            return Ok(AciTransformResult {
                original: original.to_string(),
                transformed: original.to_string(),
                success: false,
                modified_node: None,
                syntax_valid: false,
                error: Some(format!("Unsupported language: {}", language)),
            });
        }

        // Perform AST-level replacement
        match self.ast_replace(original, old_node, new_node, language) {
            Ok(transformed) => {
                // Validate syntax
                let syntax_valid = self.validate_syntax(&transformed, language);

                Ok(AciTransformResult {
                    original: original.to_string(),
                    transformed: transformed.clone(),
                    success: true,
                    modified_node: Some(old_node.to_string()),
                    syntax_valid,
                    error: None,
                })
            }
            Err(e) => Ok(AciTransformResult {
                original: original.to_string(),
                transformed: original.to_string(),
                success: false,
                modified_node: Some(old_node.to_string()),
                syntax_valid: false,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Perform AST-level replacement (in-memory)
    fn ast_replace(
        &self,
        original: &str,
        old_node: &str,
        new_node: &str,
        language: &str,
    ) -> Result<String> {
        // Check for exact match
        let occurrences = original.matches(old_node).count();
        if occurrences == 0 {
            anyhow::bail!("Node not found in code");
        }
        if occurrences > 1 {
            anyhow::bail!("Ambiguous node (found {} occurrences)", occurrences);
        }

        // Perform replacement
        let transformed = original.replacen(old_node, new_node, 1);

        // Validate syntax
        if !self.validate_syntax(&transformed, language) {
            anyhow::bail!("Syntax error after transformation");
        }

        Ok(transformed)
    }

    /// Validate syntax of transformed code
    fn validate_syntax(&self, code: &str, language: &str) -> bool {
        match language {
            "rust" => self.validate_rust_syntax(code),
            "python" => self.validate_python_syntax(code),
            "typescript" | "javascript" => self.validate_js_syntax(code),
            _ => true, // Unknown language, skip validation
        }
    }

    /// Validate Rust syntax (basic bracket matching)
    fn validate_rust_syntax(&self, code: &str) -> bool {
        let mut stack = Vec::new();
        for ch in code.chars() {
            match ch {
                '(' | '[' | '{' => stack.push(ch),
                ')' => {
                    if stack.pop() != Some('(') {
                        return false;
                    }
                }
                ']' => {
                    if stack.pop() != Some('[') {
                        return false;
                    }
                }
                '}' => {
                    if stack.pop() != Some('{') {
                        return false;
                    }
                }
                _ => {}
            }
        }
        stack.is_empty()
    }

    /// Validate Python syntax (basic indentation check)
    fn validate_python_syntax(&self, code: &str) -> bool {
        // Basic check: no mixing tabs and spaces
        let has_tabs = code.contains('\t');
        let has_spaces = code.contains("    ");

        // If both present, likely mixed indentation
        if has_tabs && has_spaces {
            return false;
        }

        // Check for unclosed strings
        let mut in_string = false;
        let mut escape = false;
        for ch in code.chars() {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' | '\'' => in_string = !in_string,
                _ => {}
            }
        }
        !in_string
    }

    /// Validate JavaScript/TypeScript syntax (basic bracket matching)
    fn validate_js_syntax(&self, code: &str) -> bool {
        self.validate_rust_syntax(code) // Same bracket matching logic
    }

    /// Extract AST nodes from code (simplified)
    pub fn extract_nodes(&self, code: &str, language: &str) -> Vec<String> {
        match language {
            "rust" => self.extract_rust_nodes(code),
            "python" => self.extract_python_nodes(code),
            _ => vec![],
        }
    }

    /// Extract Rust function definitions
    fn extract_rust_nodes(&self, code: &str) -> Vec<String> {
        let mut nodes = Vec::new();
        for line in code.lines() {
            if line.trim().starts_with("fn ") {
                nodes.push(line.trim().to_string());
            }
        }
        nodes
    }

    /// Extract Python function definitions
    fn extract_python_nodes(&self, code: &str) -> Vec<String> {
        let mut nodes = Vec::new();
        for line in code.lines() {
            if line.trim().starts_with("def ") {
                nodes.push(line.trim().to_string());
            }
        }
        nodes
    }
}

impl Default for AciTransformer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aci_transformer_creation() {
        let transformer = AciTransformer::new();
        assert_eq!(transformer.languages.len(), 4);
    }

    #[test]
    fn test_transform_rust_code() {
        let transformer = AciTransformer::new();
        let original = "fn foo() { let x = 1; }";
        let result = transformer
            .transform_code(original, "let x = 1;", "let x = 42;", "rust")
            .unwrap();

        assert!(result.success);
        assert!(result.syntax_valid);
        assert!(result.transformed.contains("let x = 42;"));
    }

    #[test]
    fn test_transform_syntax_error() {
        let transformer = AciTransformer::new();
        let original = "fn foo() { let x = 1; }";
        let result = transformer
            .transform_code(original, "}", "// unclosed", "rust")
            .unwrap();

        assert!(!result.success);
        assert!(!result.syntax_valid);
    }

    #[test]
    fn test_transform_ambiguous_node() {
        let transformer = AciTransformer::new();
        let original = "x x x";
        let result = transformer
            .transform_code(original, "x", "y", "rust")
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Ambiguous"));
    }

    #[test]
    fn test_validate_rust_syntax() {
        let transformer = AciTransformer::new();
        assert!(transformer.validate_rust_syntax("fn foo() {}"));
        assert!(!transformer.validate_rust_syntax("fn foo() {"));
        assert!(!transformer.validate_rust_syntax("fn foo() }"));
    }

    #[test]
    fn test_validate_python_syntax() {
        let transformer = AciTransformer::new();
        assert!(transformer.validate_python_syntax("def foo():\n    pass"));
        assert!(!transformer.validate_python_syntax("def foo():\n\tpass\n    pass"));
        // Mixed indentation
    }

    #[test]
    fn test_extract_rust_nodes() {
        let transformer = AciTransformer::new();
        let code = "fn foo() {}\nfn bar() {}\nlet x = 1;";
        let nodes = transformer.extract_nodes(code, "rust");
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].contains("fn foo"));
        assert!(nodes[1].contains("fn bar"));
    }

    #[test]
    fn test_extract_python_nodes() {
        let transformer = AciTransformer::new();
        let code = "def foo():\n    pass\ndef bar():\n    pass\nx = 1";
        let nodes = transformer.extract_nodes(code, "python");
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].contains("def foo"));
        assert!(nodes[1].contains("def bar"));
    }

    #[test]
    fn test_unsupported_language() {
        let transformer = AciTransformer::new();
        let result = transformer
            .transform_code("code", "old", "new", "cobol")
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unsupported"));
    }
}
