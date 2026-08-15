//! Tree-sitter-based AST scanner for Rust source files.
//! Provides accurate function-level analysis replacing heuristic line counting.

use tree_sitter::{Node, Parser};

use crate::scanner::{Finding, FindingKind};

/// Metadata about a single function extracted from the AST.
pub struct AstFunction {
    pub name: String,
    pub start_line: usize, // 1-indexed
    pub end_line: usize,   // 1-indexed
    pub line_count: usize,
    pub is_public: bool,
    pub has_doc_comment: bool,
}

/// AST-based scanner backed by tree-sitter-rust.
pub struct AstScanner {
    parser: Parser,
}

impl AstScanner {
    /// Create a new `AstScanner`.
    ///
    /// Returns `None` if tree-sitter initialisation fails, allowing the caller
    /// to fall back gracefully to heuristic scanning.
    pub fn new() -> Option<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).ok()?;
        Some(Self { parser })
    }

    /// Parse Rust source code and extract all function definitions.
    pub fn extract_functions(&mut self, source: &str) -> Vec<AstFunction> {
        let tree = match self.parser.parse(source, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let source_bytes = source.as_bytes();
        let mut functions = Vec::new();

        Self::collect_functions(root, source_bytes, &mut functions);
        functions
    }

    /// Recursively walk the tree and collect `function_item` nodes.
    fn collect_functions(node: Node<'_>, source: &[u8], out: &mut Vec<AstFunction>) {
        if node.kind() == "function_item" {
            if let Some(func) = Self::extract_function_info(node, source) {
                out.push(func);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_functions(child, source, out);
        }
    }

    /// Extract structured info from a `function_item` node.
    fn extract_function_info(node: Node<'_>, source: &[u8]) -> Option<AstFunction> {
        // Name is the `identifier` child under field "name"
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?.to_string();

        let start_line = node.start_position().row + 1; // convert 0-indexed to 1-indexed
        let end_line = node.end_position().row + 1;
        let line_count = end_line.saturating_sub(start_line) + 1;

        // Public = has a `visibility_modifier` child containing "pub"
        let is_public = Self::has_pub_visibility(node, source);

        // Doc comment = preceding sibling is `line_comment` (///) or `block_comment` (/**)
        let has_doc_comment = Self::has_preceding_doc_comment(node, source);

        Some(AstFunction {
            name,
            start_line,
            end_line,
            line_count,
            is_public,
            has_doc_comment,
        })
    }

    /// Return `true` if this `function_item` has a `pub` visibility modifier.
    fn has_pub_visibility(node: Node<'_>, source: &[u8]) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                if let Ok(text) = child.utf8_text(source) {
                    // Matches "pub", "pub(crate)", "pub(super)", etc.
                    if text.starts_with("pub") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Return `true` if the node is immediately preceded by a doc comment.
    ///
    /// A doc comment is a `line_comment` starting with `///` or a
    /// `block_comment` starting with `/**`.  Attribute items (`#[...]`) between
    /// the doc comment and the function are skipped.
    fn has_preceding_doc_comment(node: Node<'_>, source: &[u8]) -> bool {
        let mut current = node.prev_sibling();
        while let Some(sib) = current {
            match sib.kind() {
                "attribute_item" => {
                    // Skip attribute annotations (#[inline], #[must_use], etc.)
                    current = sib.prev_sibling();
                }
                "line_comment" => {
                    if let Ok(text) = sib.utf8_text(source) {
                        return text.starts_with("///");
                    }
                    return false;
                }
                "block_comment" => {
                    if let Ok(text) = sib.utf8_text(source) {
                        return text.starts_with("/**");
                    }
                    return false;
                }
                _ => break,
            }
        }
        false
    }

    /// Scan source for oversized functions (more than `max_lines` lines).
    pub fn scan_oversized_functions(
        &mut self,
        source: &str,
        file: &str,
        max_lines: usize,
    ) -> Vec<Finding> {
        self.extract_functions(source)
            .into_iter()
            .filter(|f| f.line_count > max_lines)
            .map(|f| Finding {
                kind: FindingKind::OversizedFunction,
                file: file.to_string(),
                line: f.start_line,
                snippet: format!(
                    "fn {} — {} lines (limit {})",
                    f.name, f.line_count, max_lines
                ),
                priority: 1,
            })
            .collect()
    }

    /// Scan source for public functions that lack a doc comment.
    pub fn scan_undocumented_pub_fns(&mut self, source: &str, file: &str) -> Vec<Finding> {
        self.extract_functions(source)
            .into_iter()
            .filter(|f| f.is_public && !f.has_doc_comment)
            .map(|f| Finding {
                kind: FindingKind::UndocumentedPubFn,
                file: file.to_string(),
                line: f.start_line,
                snippet: format!("pub fn {} — missing doc comment", f.name),
                priority: 0,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanner() -> AstScanner {
        AstScanner::new().expect("tree-sitter-rust should be available in test environment")
    }

    // Test 1: basic construction
    #[test]
    fn ast_scanner_new_succeeds() {
        assert!(
            AstScanner::new().is_some(),
            "AstScanner::new() must return Some"
        );
    }

    // Test 2: finds a simple function
    #[test]
    fn extract_functions_finds_simple_fn() {
        let mut s = scanner();
        let src = r#"
fn hello() {
    println!("hi");
}
"#;
        let fns = s.extract_functions(src);
        assert_eq!(fns.len(), 1, "expected exactly one function");
        assert_eq!(fns[0].name, "hello");
    }

    // Test 3: public vs private detection
    #[test]
    fn extract_functions_detects_pub() {
        let mut s = scanner();
        let src = r#"
pub fn visible() {}
fn hidden() {}
pub(crate) fn crate_visible() {}
"#;
        let fns = s.extract_functions(src);
        assert_eq!(fns.len(), 3);

        let visible = fns.iter().find(|f| f.name == "visible").unwrap();
        assert!(visible.is_public, "'visible' should be public");

        let hidden = fns.iter().find(|f| f.name == "hidden").unwrap();
        assert!(!hidden.is_public, "'hidden' should not be public");

        let crate_vis = fns.iter().find(|f| f.name == "crate_visible").unwrap();
        assert!(
            crate_vis.is_public,
            "'crate_visible' should be public (pub(crate))"
        );
    }

    // Test 4: accurate line counting
    #[test]
    fn extract_functions_counts_lines_accurately() {
        let mut s = scanner();
        // fn line + 8 body lines + closing `}` = 10 lines
        let body: String = (0..8)
            .map(|i| format!("    let _x{} = {};\n", i, i))
            .collect();
        let src = format!("fn sized() {{\n{}}}\n", body);

        let fns = s.extract_functions(&src);
        assert_eq!(fns.len(), 1);
        assert_eq!(
            fns[0].line_count, 10,
            "expected 10 lines, got {} (start={}, end={})",
            fns[0].line_count, fns[0].start_line, fns[0].end_line
        );
    }

    // Test 5: oversized function is detected
    #[test]
    fn scan_oversized_detects_large_fn() {
        let mut s = scanner();
        // fn line + 60 body lines + closing `}` = 62 lines, well over limit of 50
        let body: String = (0..60)
            .map(|i| format!("    let _v{} = {};\n", i, i))
            .collect();
        let src = format!("fn big_fn() {{\n{}}}\n", body);

        let findings = s.scan_oversized_functions(&src, "big_fn.rs", 50);
        assert!(
            !findings.is_empty(),
            "expected OversizedFunction finding for 62-line function"
        );
        assert_eq!(findings[0].kind, FindingKind::OversizedFunction);
        assert!(
            findings[0].snippet.contains("big_fn"),
            "snippet should mention the function name"
        );
    }

    // Test 6: undocumented public function is flagged
    #[test]
    fn scan_undocumented_pub_fns_detects_missing_doc() {
        let mut s = scanner();
        let src = r#"
pub fn no_doc() -> u32 {
    42
}
"#;
        let findings = s.scan_undocumented_pub_fns(src, "lib.rs");
        assert!(!findings.is_empty(), "expected UndocumentedPubFn finding");
        assert_eq!(findings[0].kind, FindingKind::UndocumentedPubFn);
        assert!(
            findings[0].snippet.contains("no_doc"),
            "snippet should mention the function name"
        );
    }

    // Test 7: documented public function is NOT flagged
    #[test]
    fn scan_documented_pub_fn_not_flagged() {
        let mut s = scanner();
        let src = r#"
/// Returns the answer.
pub fn documented() -> u32 {
    42
}
"#;
        let findings = s.scan_undocumented_pub_fns(src, "lib.rs");
        assert!(
            findings.is_empty(),
            "documented pub fn should NOT produce a finding, got: {:?}",
            findings.iter().map(|f| &f.snippet).collect::<Vec<_>>()
        );
    }
}
