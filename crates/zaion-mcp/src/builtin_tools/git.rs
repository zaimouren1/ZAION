//! Git tool handlers (read-only): git_status / git_log / git_diff_stat / git_branch / git_remote.
//!
//! These tools shell out to `git` but are constrained to a fixed set of
//! read-only subcommands. They never mutate the repository: no commit, push,
//! checkout, reset, clean, or config writes are reachable through this surface.
//! Authorization for the "execute"/"read" capability class is enforced by the
//! upper-layer capability gate, not by these handlers.

use std::process::Command;

use serde_json::json;

use super::workspace_root;
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

/// Run a read-only git invocation in the workspace root and capture stdout.
fn run_git(args: &[&str]) -> Result<String, String> {
    let root = workspace_root()?;
    let output = Command::new("git")
        .args(args)
        .current_dir(&root)
        .output()
        .map_err(|e| format!("failed to spawn git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {:?} failed: {}", args, stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn git_status_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let out = run_git(&["status", "--porcelain=v1", "--branch"])?;

    let mut branch_line = String::new();
    let mut entries = Vec::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            branch_line = rest.to_string();
        } else if !line.is_empty() {
            let (status, path) = line.split_at(2.min(line.len()));
            entries.push(json!({
                "status": status.trim(),
                "path": path.trim()
            }));
        }
    }

    Ok(json!({
        "branch": branch_line,
        "changed_count": entries.len(),
        "entries": entries
    }))
}

pub(super) fn git_log_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = input
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(200);

    let limit_arg = format!("-n{}", limit);
    // Use a unit-separator-delimited format so commit subjects with spaces parse cleanly.
    let out = run_git(&["log", &limit_arg, "--pretty=format:%H%x1f%an%x1f%aI%x1f%s"])?;

    let commits: Vec<_> = out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.split('\u{1f}');
            json!({
                "hash": parts.next().unwrap_or(""),
                "author": parts.next().unwrap_or(""),
                "date": parts.next().unwrap_or(""),
                "subject": parts.next().unwrap_or("")
            })
        })
        .collect();

    Ok(json!({
        "count": commits.len(),
        "commits": commits
    }))
}

pub(super) fn git_diff_stat_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let out = run_git(&["diff", "--numstat"])?;

    let mut files = Vec::new();
    let mut total_added: u64 = 0;
    let mut total_deleted: u64 = 0;
    for line in out.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.split('\t');
        let added = parts.next().unwrap_or("0");
        let deleted = parts.next().unwrap_or("0");
        let path = parts.next().unwrap_or("");
        let added_n = added.parse::<u64>().ok();
        let deleted_n = deleted.parse::<u64>().ok();
        total_added += added_n.unwrap_or(0);
        total_deleted += deleted_n.unwrap_or(0);
        files.push(json!({
            "path": path,
            "added": added_n,
            "deleted": deleted_n,
            "binary": added == "-" || deleted == "-"
        }));
    }

    Ok(json!({
        "files_changed": files.len(),
        "total_added": total_added,
        "total_deleted": total_deleted,
        "files": files
    }))
}

pub(super) fn git_branch_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let out = run_git(&["branch", "--list", "--format=%(refname:short)%09%(HEAD)"])?;

    let mut branches = Vec::new();
    let mut current = String::new();
    for line in out.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.split('\t');
        let name = parts.next().unwrap_or("").trim().to_string();
        let is_head = parts.next().map(|m| m.trim() == "*").unwrap_or(false);
        if is_head {
            current = name.clone();
        }
        branches.push(json!({
            "name": name,
            "current": is_head
        }));
    }

    Ok(json!({
        "current": current,
        "count": branches.len(),
        "branches": branches
    }))
}

pub(super) fn git_remote_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    // Read-only listing of configured remotes. No fetch/push is performed.
    let out = run_git(&["remote", "-v"])?;

    let mut remotes = Vec::new();
    for line in out.lines().filter(|l| !l.is_empty()) {
        // Format: "<name>\t<url> (fetch|push)"
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or("").to_string();
        let url = parts.next().unwrap_or("").to_string();
        let direction = parts
            .next()
            .map(|d| d.trim_matches(['(', ')']).to_string())
            .unwrap_or_default();
        remotes.push(json!({
            "name": name,
            "url": url,
            "direction": direction
        }));
    }

    Ok(json!({
        "count": remotes.len(),
        "remotes": remotes
    }))
}

/// Register the read-only git tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "git_status",
            "1.0",
            "Show the working-tree status (porcelain) for the workspace repository.",
            McpSchema::new(vec![]),
            "read",
        ),
        git_status_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "git_log",
            "1.0",
            "List recent commits (hash, author, date, subject).",
            McpSchema::new(vec![McpParam::optional(
                "limit",
                McpParamType::Number,
                "maximum number of commits to return (default 20, max 200)",
                json!(20),
            )]),
            "read",
        ),
        git_log_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "git_diff_stat",
            "1.0",
            "Show added/deleted line counts per file for the unstaged diff.",
            McpSchema::new(vec![]),
            "read",
        ),
        git_diff_stat_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "git_branch",
            "1.0",
            "List local branches and identify the current branch.",
            McpSchema::new(vec![]),
            "read",
        ),
        git_branch_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "git_remote",
            "1.0",
            "List configured git remotes (read-only; no fetch or push).",
            McpSchema::new(vec![]),
            "read",
        ),
        git_remote_handler,
    ));
}
