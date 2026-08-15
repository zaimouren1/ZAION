//! Core filesystem tool handlers (read/list/search/write/edit/delete/copy/move/mkdir).
//!
//! Advanced traversal/metadata tools (fs_stat/read_lines/append/glob/find/tree)
//! live in the sibling `fs_advanced` module to keep this file within budget.

use std::collections::VecDeque;
use std::io::Read;

use serde_json::json;

use super::{check_drift, record_file_state, resolve_under_workspace, workspace_root, DriftCheck};
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

/// Directory names skipped by recursive walks (build/cache/VCS dirs).
pub(super) const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".next", ".cache"];

// ── fs_read ───────────────────────────────────────────────────────────────────

pub(super) fn fs_read_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_BYTES: u64 = 50 * 1024; // 50 KB

    let path = input["path"]
        .as_str()
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let path = resolve_under_workspace(path, true)?;

    let metadata = std::fs::metadata(&path)
        .map_err(|e| format!("cannot access '{}': {}", path.display(), e))?;

    if !metadata.is_file() {
        return Err(format!("'{}' is not a regular file", path.display()));
    }

    if metadata.len() > MAX_BYTES {
        return Err(format!(
            "file '{}' is {} bytes, exceeds 50 KB limit",
            path.display(),
            metadata.len()
        ));
    }

    let mut file = std::fs::File::open(&path)
        .map_err(|e| format!("cannot open '{}': {}", path.display(), e))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;

    let lines = content.lines().count();

    // Gate 0 of the edit-safety triad: record observed state so a later
    // fs_edit / fs_write can verify the file hasn't drifted underneath us.
    record_file_state(&path, &content);

    Ok(json!({
        "content": content,
        "lines": lines,
    }))
}

// ── fs_list ───────────────────────────────────────────────────────────────────

pub(super) fn fs_list_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_ENTRIES: usize = 200;

    let path = input["path"]
        .as_str()
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let path = resolve_under_workspace(path, true)?;

    let read_dir =
        std::fs::read_dir(&path).map_err(|e| format!("cannot list '{}': {}", path.display(), e))?;

    let mut entries = Vec::new();
    for entry_result in read_dir.take(MAX_ENTRIES) {
        let entry = entry_result.map_err(|e| format!("error reading directory entry: {}", e))?;

        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().into_owned();

        let meta = entry
            .metadata()
            .map_err(|e| format!("cannot stat '{}': {}", name, e))?;

        let is_dir = meta.is_dir();
        let size: Option<u64> = if meta.is_file() {
            Some(meta.len())
        } else {
            None
        };

        entries.push(json!({
            "name": name,
            "is_dir": is_dir,
            "size": size,
        }));
    }

    Ok(json!({ "entries": entries }))
}

pub(super) fn fs_search_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_RESULTS: usize = 100;
    const MAX_FILE_BYTES: u64 = 256 * 1024;
    const MAX_VISITED_FILES: usize = 2_000;

    let query = input["query"]
        .as_str()
        .ok_or_else(|| "missing 'query' parameter".to_string())?;
    if query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }

    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let max_results = input
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .clamp(1, MAX_RESULTS as u64) as usize;
    let case_sensitive = input
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = workspace_root()?;
    let start = resolve_under_workspace(path, true)?;
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut queue = VecDeque::from([start]);
    let mut results = Vec::new();
    let mut visited_files = 0usize;

    while let Some(path) = queue.pop_front() {
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };

        if meta.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(
                name,
                ".git" | "target" | "node_modules" | ".next" | ".cache"
            ) {
                continue;
            }
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                queue.push_back(entry.path());
            }
            continue;
        }

        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        visited_files += 1;
        if visited_files > MAX_VISITED_FILES {
            break;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        for (idx, line) in content.lines().enumerate() {
            let matched = if case_sensitive {
                line.contains(&needle)
            } else {
                line.to_lowercase().contains(&needle)
            };
            if matched {
                let rel = path.strip_prefix(&root).unwrap_or(&path);
                results.push(json!({
                    "path": rel.to_string_lossy(),
                    "line": idx + 1,
                    "preview": line.trim(),
                }));
                if results.len() >= max_results {
                    return Ok(json!({
                        "query": query,
                        "results": results,
                        "truncated": true,
                    }));
                }
            }
        }
    }

    Ok(json!({
        "query": query,
        "results": results,
        "truncated": false,
    }))
}

// ── File Operations Tools ─────────────────────────────────────────────────────

pub(super) fn fs_write_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'content' parameter".to_string())?;

    let resolved = resolve_under_workspace(path, false)?;

    // Edit-safety triad for EXISTING files: a blind overwrite must be preceded
    // by an fs_read, and the on-disk content must not have drifted since then.
    // Brand-new files are exempt (nothing to clobber).
    if resolved.is_file() {
        let current = std::fs::read_to_string(&resolved)
            .map_err(|e| format!("cannot read '{}' for safety check: {}", path, e))?;
        match check_drift(&resolved, &current) {
            DriftCheck::Fresh => {}
            DriftCheck::NeverRead => {
                return Err(format!(
                    "refusing to overwrite '{}': read it with fs_read first \
                     (read-before-edit safety)",
                    path
                ));
            }
            DriftCheck::Drifted => {
                return Err(format!(
                    "refusing to overwrite '{}': file changed on disk since it was \
                     read — re-read it before writing (stale-edit safety)",
                    path
                ));
            }
        }
    }

    std::fs::write(&resolved, content).map_err(|e| format!("failed to write file: {}", e))?;

    // Update observed state so subsequent edits in the same session stay fresh.
    record_file_state(&resolved, content);

    Ok(json!({
        "status": "success",
        "path": path,
        "bytes_written": content.len()
    }))
}

// ── fs_edit: surgical string replacement with unique-match guarantee ────────────

pub(super) fn fs_edit_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;
    let old_str = input
        .get("old_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'old_str' parameter".to_string())?;
    let new_str = input
        .get("new_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'new_str' parameter".to_string())?;
    let replace_all = input
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if old_str == new_str {
        return Err("old_str and new_str are identical — nothing to change".to_string());
    }

    let resolved = resolve_under_workspace(path, true)?;
    if !resolved.is_file() {
        return Err(format!("'{}' is not a regular file", path));
    }

    let current =
        std::fs::read_to_string(&resolved).map_err(|e| format!("cannot read '{}': {}", path, e))?;

    // Gate 1 + Gate 2: read-before-edit and drift detection.
    match check_drift(&resolved, &current) {
        DriftCheck::Fresh => {}
        DriftCheck::NeverRead => {
            return Err(format!(
                "refusing to edit '{}': read it with fs_read first (read-before-edit safety)",
                path
            ));
        }
        DriftCheck::Drifted => {
            return Err(format!(
                "refusing to edit '{}': file changed on disk since it was read — \
                 re-read it before editing (stale-edit safety)",
                path
            ));
        }
    }

    // Gate 3: unique-match. Ambiguous edits are rejected unless replace_all.
    let occurrences = current.matches(old_str).count();
    if occurrences == 0 {
        return Err(format!(
            "old_str not found in '{}' — no replacement made",
            path
        ));
    }
    if occurrences > 1 && !replace_all {
        return Err(format!(
            "old_str occurs {} times in '{}' — ambiguous. Provide more surrounding \
             context to make it unique, or set replace_all=true",
            occurrences, path
        ));
    }

    let updated = if replace_all {
        current.replace(old_str, new_str)
    } else {
        current.replacen(old_str, new_str, 1)
    };

    std::fs::write(&resolved, &updated).map_err(|e| format!("failed to write file: {}", e))?;
    record_file_state(&resolved, &updated);

    Ok(json!({
        "status": "success",
        "path": path,
        "replacements": if replace_all { occurrences } else { 1 },
        "bytes_written": updated.len(),
    }))
}

pub(super) fn fs_delete_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;

    let resolved = resolve_under_workspace(path, true)?;

    if resolved.is_dir() {
        std::fs::remove_dir_all(&resolved)
            .map_err(|e| format!("failed to delete directory: {}", e))?;
    } else {
        std::fs::remove_file(&resolved).map_err(|e| format!("failed to delete file: {}", e))?;
    }

    Ok(json!({
        "status": "success",
        "path": path,
        "deleted": true
    }))
}

pub(super) fn fs_copy_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let source = input
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'source' parameter".to_string())?;
    let destination = input
        .get("destination")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'destination' parameter".to_string())?;

    let src = resolve_under_workspace(source, true)?;
    let dst = resolve_under_workspace(destination, false)?;

    std::fs::copy(&src, &dst).map_err(|e| format!("failed to copy file: {}", e))?;

    Ok(json!({
        "status": "success",
        "source": source,
        "destination": destination
    }))
}

pub(super) fn fs_move_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let source = input
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'source' parameter".to_string())?;
    let destination = input
        .get("destination")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'destination' parameter".to_string())?;

    let src = resolve_under_workspace(source, true)?;
    let dst = resolve_under_workspace(destination, false)?;

    std::fs::rename(&src, &dst).map_err(|e| format!("failed to move file: {}", e))?;

    Ok(json!({
        "status": "success",
        "source": source,
        "destination": destination
    }))
}

pub(super) fn fs_mkdir_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'path' parameter".to_string())?;

    let resolved = resolve_under_workspace(path, false)?;
    std::fs::create_dir_all(&resolved).map_err(|e| format!("failed to create directory: {}", e))?;

    Ok(json!({
        "status": "success",
        "path": path,
        "created": true
    }))
}

/// Register all filesystem tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    // fs_read
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_read",
            "1.0",
            "Read a file (max 50 KB). Returns content and line count.",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to the file to read",
            )]),
            "read",
        ),
        fs_read_handler,
    ));

    // fs_list
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_list",
            "1.0",
            "List directory contents (max 200 entries).",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to the directory to list",
            )]),
            "read",
        ),
        fs_list_handler,
    ));
    // fs_search
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_search",
            "1.0",
            "Search text recursively under a workspace-relative directory. Skips large files and build/cache directories.",
            McpSchema::new(vec![
                McpParam::required("query", McpParamType::String, "text to search for"),
                McpParam::optional(
                    "path",
                    McpParamType::String,
                    "workspace-relative directory to search",
                    json!("."),
                ),
                McpParam::optional(
                    "max_results",
                    McpParamType::Number,
                    "maximum matches to return (default 50, max 100)",
                    json!(50),
                ),
                McpParam::optional(
                    "case_sensitive",
                    McpParamType::Boolean,
                    "whether the search is case-sensitive",
                    json!(false),
                ),
            ]),
            "read",
        ),
        fs_search_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_write",
            "1.0",
            "Write content to a file. Creates a new file, or overwrites an \
             existing one. Overwriting an existing file requires that it was \
             read with fs_read first and has not changed on disk since \
             (read-before-edit + stale-edit safety).",
            McpSchema::new(vec![
                McpParam::required(
                    "path",
                    McpParamType::String,
                    "workspace-relative path to the file to write",
                ),
                McpParam::required(
                    "content",
                    McpParamType::String,
                    "content to write to the file",
                ),
            ]),
            "write",
        ),
        fs_write_handler,
    ));
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_edit",
            "1.0",
            "Surgically replace an exact string in an existing file. The file \
             must be read with fs_read first and unchanged on disk. old_str \
             must match exactly once (unless replace_all=true), otherwise the \
             edit is rejected as ambiguous. Prefer this over fs_write for \
             targeted changes.",
            McpSchema::new(vec![
                McpParam::required(
                    "path",
                    McpParamType::String,
                    "workspace-relative path to the file to edit",
                ),
                McpParam::required(
                    "old_str",
                    McpParamType::String,
                    "exact substring to replace; include surrounding context to make it unique",
                ),
                McpParam::required(
                    "new_str",
                    McpParamType::String,
                    "replacement text (may be empty to delete the matched region)",
                ),
                McpParam::optional(
                    "replace_all",
                    McpParamType::Boolean,
                    "replace every occurrence instead of requiring a unique match",
                    json!(false),
                ),
            ]),
            "write",
        ),
        fs_edit_handler,
    ));
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_delete",
            "1.0",
            "Delete a file or directory. Recursively deletes directories.",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to the file or directory to delete",
            )]),
            "write",
        ),
        fs_delete_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_copy",
            "1.0",
            "Copy a file from source to destination.",
            McpSchema::new(vec![
                McpParam::required(
                    "source",
                    McpParamType::String,
                    "workspace-relative path to the source file",
                ),
                McpParam::required(
                    "destination",
                    McpParamType::String,
                    "workspace-relative path to the destination file",
                ),
            ]),
            "write",
        ),
        fs_copy_handler,
    ));
    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_move",
            "1.0",
            "Move or rename a file from source to destination.",
            McpSchema::new(vec![
                McpParam::required(
                    "source",
                    McpParamType::String,
                    "workspace-relative path to the source file",
                ),
                McpParam::required(
                    "destination",
                    McpParamType::String,
                    "workspace-relative path to the destination file",
                ),
            ]),
            "write",
        ),
        fs_move_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "fs_mkdir",
            "1.0",
            "Create a directory and all parent directories if needed.",
            McpSchema::new(vec![McpParam::required(
                "path",
                McpParamType::String,
                "workspace-relative path to the directory to create",
            )]),
            "write",
        ),
        fs_mkdir_handler,
    ));
}
