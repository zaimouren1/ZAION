use crate::CodexError;
/// Codegen helper: insert or replace named symbols in files.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CodegenEdit {
    pub file_path: PathBuf,
    pub symbol_name: String,
    pub new_content: String,
    pub kind: CodegenKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenKind {
    /// Replace the entire function/struct/impl block
    Replace,
    /// Insert after the named symbol
    InsertAfter,
    /// Insert before the named symbol
    InsertBefore,
}

impl CodegenEdit {
    /// Apply this edit to a file.
    pub fn apply(&self) -> Result<(), CodexError> {
        let content = std::fs::read_to_string(&self.file_path).map_err(CodexError::Io)?;

        let lines: Vec<&str> = content.lines().collect();
        let mut result = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            // Check if this line contains the symbol declaration
            if line_contains_symbol(trimmed, &self.symbol_name) {
                match self.kind {
                    CodegenKind::Replace => {
                        // Find end of block and replace
                        let end = find_block_end(&lines, i);
                        result.push(self.new_content.clone());
                        i = end + 1;
                    }
                    CodegenKind::InsertAfter => {
                        result.push(line.to_string());
                        let end = find_block_end(&lines, i);
                        i = end + 1;
                        result.push(self.new_content.clone());
                    }
                    CodegenKind::InsertBefore => {
                        result.push(self.new_content.clone());
                        result.push(line.to_string());
                        i += 1;
                    }
                }
            } else {
                result.push(line.to_string());
                i += 1;
            }
        }

        let new_content = result.join("\n");
        std::fs::write(&self.file_path, new_content).map_err(CodexError::Io)?;

        Ok(())
    }
}

fn line_contains_symbol(line: &str, symbol: &str) -> bool {
    line.contains(&format!("fn {}", symbol))
        || line.contains(&format!("struct {}", symbol))
        || line.contains(&format!("enum {}", symbol))
        || line.contains(&format!("impl {}", symbol))
}

fn find_block_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0;
    let mut i = start;

    while i < lines.len() {
        let line = lines[i];
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;

        if depth == 0 && i > start {
            return i;
        }
        i += 1;
    }

    lines.len() - 1
}

/// Builder for codegen edits.
pub struct CodegenBuilder {
    edit: CodegenEdit,
}

impl CodegenBuilder {
    pub fn new(file_path: impl AsRef<Path>, symbol_name: &str) -> Self {
        CodegenBuilder {
            edit: CodegenEdit {
                file_path: file_path.as_ref().to_path_buf(),
                symbol_name: symbol_name.to_string(),
                new_content: String::new(),
                kind: CodegenKind::Replace,
            },
        }
    }

    pub fn content(mut self, c: &str) -> Self {
        self.edit.new_content = c.to_string();
        self
    }

    pub fn kind(mut self, k: CodegenKind) -> Self {
        self.edit.kind = k;
        self
    }

    pub fn build(self) -> CodegenEdit {
        self.edit
    }
}
