//! Advanced filesystem tool handlers: fs_stat / fs_read_lines / fs_append /
//! fs_glob / fs_find / fs_tree.
//!
//! Split out of `fs.rs` to keep each module under the file-size budget. Shares
//! the recursive-walk skip list (`SKIP_DIRS`) with the core fs module.

use std::collections::VecDeque;

use serde_json::json;

use super::fs::SKIP_DIRS;
use super::{record_file_state, resolve_under_workspace, workspace_root};
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

/// Convert a glob pattern into an anchored regex.
fn glob_to_regex(glob: &str) -> Result<regex::Regex, String> {
    let mut pattern = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    pattern.push_str(".*");
                } else {
                    pattern.push_str("[^/]*");
                }
            }
            '?' => pattern.push('.'),
            // Escape regex metacharacters.
            '.' | '+' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' => {
                pattern.push('\\');
                pattern.push(c);
            }
            _ => pattern.push(c),
        }
    }
    pattern.push('$');
    regex::Regex::new(&pattern).map_err(|e| format!("invalid glob pattern: {}", e))
}

// ── fs_stat ─────────────────────────────────────────────────────────────────

pub(super) fn fs_stat_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let resolved = resolve_under_workspace(path, true)?;

    let meta =
        std::fs::metadata(&resolved).map_err(|e| format!("cannot stat '{}': {}", path, e))?;

    let modified_unix: Option<u64> = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    Ok(json!({
        "path": path,
        "is_dir": meta.is_dir(),
        "is_file": meta.is_file(),
        "size_bytes": meta.len(),
        "readonly": meta.permissions().readonly(),
        "modified_unix": modified_unix,
    }))
}

// ── fs_read_lines ─────────────────────────────────────────────────────────────

pub(super) fn fs_read_lines_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_BYTES: u64 = 50 * 1024;

    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let resolved = resolve_under_workspace(path, true)?;

    let meta =
        std::fs::metadata(&resolved).map_err(|e| format!("cannot access '{}': {}", path, e))?;
    if !meta.is_file() {
        return Err(format!("'{}' is not a regular file", path));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "file '{}' is {} bytes, exceeds 50 KB limit",
            path,
            meta.len()
        ));
    }

    let full_content =
        std::fs::read_to_string(&resolved).map_err(|e| format!("cannot read '{}': {}", path, e))?;

    // Record the FULL content so edit-safety stays valid.
    record_file_state(&resolved, &full_content);

    let lines: Vec<&str> = full_content.lines().collect();
    let total_lines = lines.len();

    let start = input
        .get("start")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1) as usize;
    let end = input
        .get("end")
        .and_then(|v| v.as_u64())
        .map(|e| e as usize)
        .unwrap_or(total_lines)
        .min(total_lines);

    let content = if start > total_lines || start > end {
        String::new()
    } else {
        lines[start - 1..end].join("\n")
    };

    Ok(json!({
        "path": path,
        "start": start,
        "end": end,
        "total_lines": total_lines,
        "content": content,
    }))
}

// ── fs_append ─────────────────────────────────────────────────────────────────

pub(super) fn fs_append_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'content' parameter".to_string())?;

    let resolved = resolve_under_workspace(path, false)?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&resolved)
        .map_err(|e| format!("cannot open '{}' for append: {}", path, e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("failed to append: {}", e))?;

    if let Ok(full) = std::fs::read_to_string(&resolved) {
        record_file_state(&resolved, &full);
    }

    Ok(json!({
        "status": "success",
        "path": path,
        "bytes_appended": content.len(),
    }))
}

// ── fs_glob ─────────────────────────────────────────────────────────────────

pub(super) fn fs_glob_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_RESULTS: usize = 500;

    let pattern = input
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'pattern' parameter".to_string())?;
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let re = glob_to_regex(pattern)?;
    let root = workspace_root()?;
    let start = resolve_under_workspace(path, true)?;

    let mut queue = VecDeque::from([start]);
    let mut matches: Vec<String> = Vec::new();

    while let Some(p) = queue.pop_front() {
        let meta = match std::fs::metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    queue.push_back(entry.path());
                }
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let rel = p.strip_prefix(&root).unwrap_or(&p);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if re.is_match(&rel_str) || re.is_match(file_name) {
            matches.push(rel_str);
            if matches.len() >= MAX_RESULTS {
                break;
            }
        }
    }

    let count = matches.len();
    Ok(json!({
        "pattern": pattern,
        "matches": matches,
        "count": count,
    }))
}

// ── fs_find ─────────────────────────────────────────────────────────────────

pub(super) fn fs_find_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_RESULTS: usize = 500;

    let name_pattern = input
        .get("name_pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'name_pattern' parameter".to_string())?;
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let re = regex::Regex::new(name_pattern).map_err(|e| format!("invalid regex: {}", e))?;
    let root = workspace_root()?;
    let start = resolve_under_workspace(path, true)?;

    let mut queue = VecDeque::from([start]);
    let mut matches: Vec<String> = Vec::new();

    while let Some(p) = queue.pop_front() {
        let meta = match std::fs::metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    queue.push_back(entry.path());
                }
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if re.is_match(file_name) {
            let rel = p.strip_prefix(&root).unwrap_or(&p);
            matches.push(rel.to_string_lossy().replace('\\', "/"));
            if matches.len() >= MAX_RESULTS {
                break;
            }
        }
    }

    let count = matches.len();
    Ok(json!({
        "name_pattern": name_pattern,
        "matches": matches,
        "count": count,
    }))
}

// ── fs_tree ─────────────────────────────────────────────────────────────────

pub(super) fn fs_tree_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_NODES: usize = 1000;

    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let max_depth = input.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

    let start = resolve_under_workspace(path, true)?;

    fn build(
        p: &std::path::Path,
        depth: usize,
        max_depth: usize,
        count: &mut usize,
    ) -> serde_json::Value {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(".")
            .to_string();
        let is_dir = p.is_dir();
        if !is_dir {
            return json!({ "name": name, "type": "file" });
        }

        let mut children: Vec<serde_json::Value> = Vec::new();
        if depth < max_depth && *count < MAX_NODES {
            if let Ok(entries) = std::fs::read_dir(p) {
                for entry in entries.flatten() {
                    if *count >= MAX_NODES {
                        break;
                    }
                    let child = entry.path();
                    let child_name = child.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if child.is_dir() && SKIP_DIRS.contains(&child_name) {
                        continue;
                    }
                    *count += 1;
                    children.push(build(&child, depth + 1, max_depth, count));
                }
            }
        }
        json!({ "name": name, "type": "dir", "children": children })
    }

    let mut count = 0usize;
    let tree = build(&start, 0, max_depth, &mut count);

    Ok(json!({
        "tree": tree,
        "truncated": count >= MAX_NODES,
    }))
}

/// Register the advanced filesystem tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_stat",
            "1.0",
            "Get metadata of a file or directory (size, type, readonly, mtime).",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to stat",
            )]),
            "read",
        ),
        fs_stat_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_read_lines",
            "1.0",
            "Read a 1-indexed inclusive line range from a file (max 50 KB).",
            McpSchema::new(vec![
                McpParam::required(
                    "path",
                    McpParamType::String,
                    "workspace-relative path to the file to read",
                ),
                McpParam::optional(
                    "start",
                    McpParamType::Number,
                    "1-indexed start line (default 1)",
                    json!(1),
                ),
                McpParam::optional(
                    "end",
                    McpParamType::Number,
                    "1-indexed inclusive end line (default last line)",
                    json!(0),
                ),
            ]),
            "read",
        ),
        fs_read_lines_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_append",
            "1.0",
            "Append content to a file, creating it if it does not exist.",
            McpSchema::new(vec![
                McpParam::required(
                    "path",
                    McpParamType::String,
                    "workspace-relative path to the file to append to",
                ),
                McpParam::required(
                    "content",
                    McpParamType::String,
                    "content to append to the file",
                ),
            ]),
            "write",
        ),
        fs_append_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_glob",
            "1.0",
            "Recursively find files matching a glob pattern (*, **, ?). Skips build/cache dirs. Max 500 results.",
            McpSchema::new(vec![
                McpParam::required(
                    "pattern",
                    McpParamType::String,
                    "glob pattern, e.g. '**/*.rs' or 'src/*.toml'",
                ),
                McpParam::optional(
                    "path",
                    McpParamType::String,
                    "workspace-relative directory to search",
                    json!("."),
                ),
            ]),
            "read",
        ),
        fs_glob_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_find",
            "1.0",
            "Recursively find files whose name matches a regex. Skips build/cache dirs. Max 500 results.",
            McpSchema::new(vec![
                McpParam::required(
                    "name_pattern",
                    McpParamType::String,
                    "regex matched against each file's name",
                ),
                McpParam::optional(
                    "path",
                    McpParamType::String,
                    "workspace-relative directory to search",
                    json!("."),
                ),
            ]),
            "read",
        ),
        fs_find_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_tree",
            "1.0",
            "Build a nested directory tree up to max_depth. Skips build/cache dirs. Max 1000 nodes.",
            McpSchema::new(vec![
                McpParam::optional(
                    "path",
                    McpParamType::String,
                    "workspace-relative directory root",
                    json!("."),
                ),
                McpParam::optional(
                    "max_depth",
                    McpParamType::Number,
                    "maximum recursion depth (default 3)",
                    json!(3),
                ),
            ]),
            "read",
        ),
        fs_tree_handler,
    ));
}
