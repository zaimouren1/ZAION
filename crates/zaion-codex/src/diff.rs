use crate::CodexError;
/// Git diff summary: parse line-level changes without git2 dependency.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub file_path: PathBuf,
    pub additions: usize,
    pub deletions: usize,
    pub changes: Vec<LineChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone)]
pub struct LineChange {
    pub line_number: usize,
    pub kind: ChangeKind,
    pub content: String,
}

impl DiffSummary {
    /// Parse a unified diff format string.
    pub fn parse(file_path: &Path, diff_text: &str) -> Result<Self, CodexError> {
        let mut summary = DiffSummary {
            file_path: file_path.to_path_buf(),
            additions: 0,
            deletions: 0,
            changes: Vec::new(),
        };

        let mut current_line = 0;
        for line in diff_text.lines() {
            if line.starts_with("@@") {
                // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
                if let Some(new_part) = line.split('+').nth(1) {
                    if let Some(num_str) = new_part.split(',').next() {
                        if let Ok(num) = num_str.parse::<usize>() {
                            current_line = num;
                        }
                    }
                }
            } else if line.starts_with('+') && !line.starts_with("+++") {
                summary.additions += 1;
                let content = line[1..].to_string();
                summary.changes.push(LineChange {
                    line_number: current_line,
                    kind: ChangeKind::Added,
                    content,
                });
                current_line += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                summary.deletions += 1;
                let content = line[1..].to_string();
                summary.changes.push(LineChange {
                    line_number: current_line,
                    kind: ChangeKind::Deleted,
                    content,
                });
            } else if !line.starts_with('-') && !line.starts_with('+') && !line.starts_with('@') {
                current_line += 1;
            }
        }

        Ok(summary)
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "{}: +{} -{}",
            self.file_path.display(),
            self.additions,
            self.deletions
        )
    }
}

/// Compare two file contents and produce a diff summary.
pub fn diff_files(old_path: &Path, new_path: &Path) -> Result<DiffSummary, CodexError> {
    let old_content = std::fs::read_to_string(old_path).unwrap_or_default();
    let new_content = std::fs::read_to_string(new_path).map_err(CodexError::Io)?;

    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let mut summary = DiffSummary {
        file_path: new_path.to_path_buf(),
        additions: 0,
        deletions: 0,
        changes: Vec::new(),
    };

    // Simple line-by-line diff (not optimal, but fast)
    let max_len = old_lines.len().max(new_lines.len());
    for i in 0..max_len {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        if old_line != new_line {
            if let Some(line) = old_line {
                summary.deletions += 1;
                summary.changes.push(LineChange {
                    line_number: i + 1,
                    kind: ChangeKind::Deleted,
                    content: line.to_string(),
                });
            }
            if let Some(line) = new_line {
                summary.additions += 1;
                summary.changes.push(LineChange {
                    line_number: i + 1,
                    kind: ChangeKind::Added,
                    content: line.to_string(),
                });
            }
        }
    }

    Ok(summary)
}
