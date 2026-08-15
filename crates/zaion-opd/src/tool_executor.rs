//! Tool Executor - Real tool call parsing and execution
//!
//! This module implements:
//! 1. Tool call parsing from LLM responses (OpenAI format)
//! 2. Tool execution with real side effects
//! 3. Tool result formatting
//! 4. Session isolation via task_id
//!
//! # Security
//!
//! `execute_terminal` no longer uses `sh -c`.  Instead it:
//!   1. Parses the user-supplied command string with `shell_words::split` to
//!      produce a proper argv without shell expansion.
//!   2. Validates the program against the `ToolExecutor`'s `allowed_programs`
//!      allow-list (default empty — fail-closed).
//!   3. Spawns `Command::new(program).args(rest)` — no shell is involved.

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use tracing::{debug, warn};

use crate::toolset_distribution::{canonical_tool_name, Toolset};
use crate::trajectory::{ToolCall, ToolResult};

/// Tool executor function type.
///
/// Third parameter is the executor-level allow-list; individual executors that
/// do not spawn processes may ignore it via `_allowed`.
pub type ToolExecutorFn = fn(&str, &Value, &HashSet<String>, &Path) -> Result<String>;

/// Tool definition
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub executor: ToolExecutorFn,
}

/// Tool executor with session isolation
pub struct ToolExecutor {
    /// Task ID for session isolation
    task_id: String,

    /// Available tools registry
    tools: HashMap<String, ToolDefinition>,

    /// Allow-list of programs that `execute_terminal` may spawn.
    /// Default: empty → fail-closed (no program is permitted unless listed).
    allowed_programs: HashSet<String>,

    /// Optional per-trajectory allow-list of sampled OPD tool names.
    /// `None` means every registered tool is available.
    allowed_tools: Option<HashSet<String>>,
    toolset_name: Option<String>,

    workspace_root: PathBuf,
}

impl ToolExecutor {
    /// Create a new tool executor with an empty allow-list (fail-closed).
    pub fn new(task_id: String) -> Self {
        let mut tools = HashMap::new();

        // Register built-in tools
        tools.insert(
            "terminal".to_string(),
            ToolDefinition {
                name: "terminal".to_string(),
                description: "Execute shell commands".to_string(),
                executor: execute_terminal,
            },
        );

        tools.insert(
            "read_file".to_string(),
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read file contents".to_string(),
                executor: execute_read_file,
            },
        );

        tools.insert(
            "write_file".to_string(),
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write file contents".to_string(),
                executor: execute_write_file,
            },
        );

        tools.insert(
            "list_directory".to_string(),
            ToolDefinition {
                name: "list_directory".to_string(),
                description: "List directory entries".to_string(),
                executor: execute_list_directory,
            },
        );

        tools.insert(
            "search_files".to_string(),
            ToolDefinition {
                name: "search_files".to_string(),
                description: "Search files by simple name pattern".to_string(),
                executor: execute_search_files,
            },
        );

        let workspace_root = std::env::current_dir()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            task_id,
            tools,
            // Default empty ⇒ fail-closed: every program is denied until
            // explicitly added via `with_allowed_programs`.
            allowed_programs: HashSet::new(),
            allowed_tools: None,
            toolset_name: None,
            workspace_root,
        }
    }

    /// Builder: set the set of programs that `execute_terminal` may spawn.
    pub fn with_allowed_programs(
        mut self,
        programs: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_programs = programs.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Result<Self> {
        self.workspace_root = root
            .into()
            .canonicalize()
            .context("failed to canonicalize workspace root")?;
        Ok(self)
    }

    /// Create a tool executor whose available tools are restricted by a toolset.
    pub fn with_toolset(mut self, toolset: &Toolset) -> Self {
        self.allowed_tools = Some(toolset.allowed_tool_set());
        self.toolset_name = Some(toolset.name.clone());
        self
    }

    /// Parse tool calls from LLM response
    pub fn parse_tool_calls(&self, response: &str) -> Result<Vec<ToolCall>> {
        // Try to parse as JSON (OpenAI tool call format)
        if let Ok(json) = serde_json::from_str::<Value>(response) {
            if let Some(tool_calls) = json.get("tool_calls").and_then(|v| v.as_array()) {
                return self.parse_openai_tool_calls(tool_calls);
            }
        }

        // Try to extract tool calls from text (function call format)
        self.parse_text_tool_calls(response)
    }

    /// Parse OpenAI format tool calls
    fn parse_openai_tool_calls(&self, tool_calls: &[Value]) -> Result<Vec<ToolCall>> {
        let mut calls = Vec::new();

        for tc in tool_calls {
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let function = tc.get("function").context("Missing function field")?;

            let name = function
                .get("name")
                .and_then(|v| v.as_str())
                .context("Missing function name")?
                .to_string();
            let name = canonical_tool_name(&name).to_string();

            let arguments = function
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}")
                .to_string();

            calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }

        Ok(calls)
    }

    /// Parse text format tool calls (e.g., "terminal(command='ls -la')")
    fn parse_text_tool_calls(&self, text: &str) -> Result<Vec<ToolCall>> {
        let mut calls = Vec::new();

        // Simple regex-based parsing for function call format.
        // Format: tool_name(arg1=value1, arg2=value2).
        // The pattern is const, so Regex::new is effectively infallible;
        // we still propagate via `?` for defence-in-depth.
        let re = regex::Regex::new(r"(\w+)\((.*?)\)")
            .expect("tool-call regex must compile (const pattern)");

        for cap in re.captures_iter(text) {
            // Skip malformed matches instead of panicking on user/LLM output.
            let Some(name_m) = cap.get(1) else { continue };
            let Some(args_m) = cap.get(2) else { continue };
            let name = canonical_tool_name(name_m.as_str()).to_string();
            let args_str = args_m.as_str();

            // Only parse if it's a known tool
            if !self.tools.contains_key(&name) {
                continue;
            }

            // Parse arguments (simple key=value format)
            let mut args = HashMap::new();
            for pair in args_str.split(',') {
                let parts: Vec<&str> = pair.split('=').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    let value = parts[1].trim().trim_matches(|c| c == '\'' || c == '"');
                    args.insert(key.to_string(), value.to_string());
                }
            }

            let id = format!("call_{}", uuid::Uuid::new_v4());
            let arguments = serde_json::to_string(&args)?;

            calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }

        Ok(calls)
    }

    /// Execute a tool call
    pub fn execute(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        debug!(
            "Executing tool: {} (task_id: {})",
            tool_call.name, self.task_id
        );

        let tool_name = canonical_tool_name(&tool_call.name);

        if let Some(allowed_tools) = &self.allowed_tools {
            if !allowed_tools.contains(tool_name) {
                let toolset_name = self.toolset_name.as_deref().unwrap_or("unknown");
                return Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    content: format!(
                        "Error: tool '{}' is not available in selected toolset '{}'",
                        tool_name, toolset_name
                    ),
                    success: false,
                });
            }
        }

        let tool = self
            .tools
            .get(tool_name)
            .context(format!("Unknown tool: {}", tool_call.name))?;

        let args: Value =
            serde_json::from_str(&tool_call.arguments).context("Failed to parse tool arguments")?;

        match (tool.executor)(
            &self.task_id,
            &args,
            &self.allowed_programs,
            &self.workspace_root,
        ) {
            Ok(content) => Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                content,
                success: true,
            }),
            Err(e) => {
                warn!("Tool execution failed: {}", e);
                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    content: format!("Error: {}", e),
                    success: false,
                })
            }
        }
    }
}

// ─── Built-in tool executors ────────────────────────────────────────────────

/// Execute a terminal command.
///
/// # Security
///
/// 1. `command` is split into argv by `shell_words::split` — no shell expansion.
/// 2. The program name is checked against `allowed_programs` before execution;
///    if not present the call is rejected with an error (fail-closed).
/// 3. `Command::new(program).args(rest)` is used — never `sh -c`.
fn execute_terminal(
    _task_id: &str,
    args: &Value,
    allowed_programs: &HashSet<String>,
    workspace_root: &Path,
) -> Result<String> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .context("Missing 'command' argument")?;

    debug!("Parsing terminal command: {}", command);

    // Step 1 — safe argv splitting (no shell interpretation)
    let argv = shell_words::split(command).context("Failed to parse command into argv")?;

    if argv.is_empty() {
        anyhow::bail!("Empty command");
    }

    let (program, rest) = argv.split_first().expect("non-empty checked above");

    // Step 2 — allow-list check (fail-closed)
    if !allowed_programs.contains(program.as_str()) {
        anyhow::bail!(
            "Program '{}' is not in the allow-list; execution denied (allow-list: {:?})",
            program,
            allowed_programs,
        );
    }

    for arg in rest {
        if path_arg_escapes_workspace(arg) {
            anyhow::bail!("argument '{}' is outside the workspace policy", arg);
        }
    }

    debug!("Executing: {} {:?}", program, rest);

    // Step 3 — direct exec, never sh -c
    let output = Command::new(program)
        .args(rest)
        .current_dir(workspace_root)
        .output()
        .context(format!("Failed to execute '{}'", program))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let result = serde_json::json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": stdout,
        "stderr": stderr,
    });

    Ok(serde_json::to_string(&result)?)
}

fn execute_read_file(
    _task_id: &str,
    args: &Value,
    _allowed: &HashSet<String>,
    workspace_root: &Path,
) -> Result<String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .context("Missing 'path' argument")?;

    let resolved = resolve_under_workspace(workspace_root, path, true)?;
    debug!("Reading file: {}", resolved.display());

    let content = std::fs::read_to_string(&resolved)
        .context(format!("Failed to read file: {}", resolved.display()))?;

    let result = serde_json::json!({
        "path": resolved.display().to_string(),
        "content": content,
        "size": content.len(),
    });

    Ok(serde_json::to_string(&result)?)
}

fn execute_write_file(
    _task_id: &str,
    args: &Value,
    _allowed: &HashSet<String>,
    workspace_root: &Path,
) -> Result<String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .context("Missing 'path' argument")?;

    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .context("Missing 'content' argument")?;

    let resolved = resolve_under_workspace(workspace_root, path, false)?;
    debug!("Writing file: {}", resolved.display());

    std::fs::write(&resolved, content)
        .context(format!("Failed to write file: {}", resolved.display()))?;

    let result = serde_json::json!({
        "path": resolved.display().to_string(),
        "size": content.len(),
        "success": true,
    });

    Ok(serde_json::to_string(&result)?)
}

fn execute_list_directory(
    _task_id: &str,
    args: &Value,
    _allowed: &HashSet<String>,
    workspace_root: &Path,
) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let resolved = resolve_under_workspace(workspace_root, path, true)?;
    if !resolved.is_dir() {
        anyhow::bail!("path '{}' is not a directory", path);
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&resolved)
        .context(format!("Failed to list directory: {}", resolved.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        entries.push(serde_json::json!({
            "name": entry.file_name().to_string_lossy(),
            "kind": if file_type.is_dir() { "directory" } else { "file" },
        }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["name"].as_str().unwrap_or_default())
    });

    let result = serde_json::json!({
        "path": resolved.display().to_string(),
        "entries": entries,
    });

    Ok(serde_json::to_string(&result)?)
}

fn execute_search_files(
    _task_id: &str,
    args: &Value,
    _allowed: &HashSet<String>,
    workspace_root: &Path,
) -> Result<String> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .context("Missing 'pattern' argument")?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let resolved = resolve_under_workspace(workspace_root, path, true)?;
    if !resolved.is_dir() {
        anyhow::bail!("path '{}' is not a directory", path);
    }

    let mut matches = Vec::new();
    search_files_recursive(&resolved, pattern, workspace_root, &mut matches)?;
    matches.sort();
    matches.truncate(1000);

    let result = serde_json::json!({
        "path": resolved.display().to_string(),
        "pattern": pattern,
        "matches": matches,
    });

    Ok(serde_json::to_string(&result)?)
}

fn resolve_under_workspace(workspace_root: &Path, path: &str, must_exist: bool) -> Result<PathBuf> {
    let input = Path::new(path);
    if input.is_absolute() || path_arg_escapes_workspace(path) {
        anyhow::bail!("path '{}' is outside the workspace policy", path);
    }

    let root = workspace_root
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let joined = root.join(input);
    let resolved = if must_exist {
        joined
            .canonicalize()
            .context(format!("failed to canonicalize '{}'", path))?
    } else {
        let parent = joined
            .parent()
            .context(format!("path '{}' has no parent", path))?;
        parent
            .canonicalize()
            .context(format!("failed to canonicalize parent for '{}'", path))?
            .join(
                joined
                    .file_name()
                    .context(format!("invalid path '{}'", path))?,
            )
    };

    if !resolved.starts_with(&root) {
        anyhow::bail!("path '{}' escapes workspace root", path);
    }

    Ok(resolved)
}

fn path_arg_escapes_workspace(arg: &str) -> bool {
    let path = Path::new(arg);
    path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
}

fn search_files_recursive(
    dir: &Path,
    pattern: &str,
    workspace_root: &Path,
    matches: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            search_files_recursive(&path, pattern, workspace_root, matches)?;
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if simple_pattern_matches(&file_name, pattern) {
            let relative = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .display()
                .to_string();
            matches.push(relative);
        }
    }
    Ok(())
}

fn simple_pattern_matches(file_name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return file_name.ends_with(&format!(".{}", suffix));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return file_name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return file_name.ends_with(suffix);
    }
    file_name == pattern || file_name.contains(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_executor_creation() {
        let executor = ToolExecutor::new("test-task".to_string());
        assert_eq!(executor.task_id, "test-task");
        assert!(executor.tools.contains_key("terminal"));
        assert!(executor.tools.contains_key("read_file"));
        assert!(executor.tools.contains_key("write_file"));
        assert!(executor.tools.contains_key("list_directory"));
        assert!(executor.tools.contains_key("search_files"));
        // Default allow-list must be empty (fail-closed)
        assert!(
            executor.allowed_programs.is_empty(),
            "default allow-list must be empty"
        );
    }

    #[test]
    fn test_parse_openai_tool_calls() {
        let executor = ToolExecutor::new("test-task".to_string());

        let response = r#"{
            "tool_calls": [
                {
                    "id": "call_123",
                    "function": {
                        "name": "terminal",
                        "arguments": "{\"command\":\"ls -la\"}"
                    }
                }
            ]
        }"#;

        let calls = executor.parse_tool_calls(response).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "terminal");
        assert_eq!(calls[0].id, "call_123");
    }

    #[test]
    fn test_parse_text_tool_calls() {
        let executor = ToolExecutor::new("test-task".to_string());

        let response = "Let me check the file: terminal(command='cat file.txt')";

        let calls = executor.parse_tool_calls(response).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "terminal");
    }

    #[test]
    fn parse_tool_calls_canonicalizes_execute_terminal_alias() {
        let executor = ToolExecutor::new("test-task".to_string());

        let calls = executor
            .parse_tool_calls("Need shell: execute_terminal(command='echo hi')")
            .unwrap();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "terminal");
    }

    #[test]
    fn test_execute_read_file() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_opd_read.txt");
        let mut file = std::fs::File::create(&temp_path).unwrap();
        file.write_all(b"test content").unwrap();

        let args = serde_json::json!({
            "path": "test_opd_read.txt"
        });

        let allowed: HashSet<String> = HashSet::new();
        let result = execute_read_file("test-task", &args, &allowed, dir.path()).unwrap();
        assert!(result.contains("test content"));
    }

    #[test]
    fn test_execute_write_file() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_opd_write.txt");

        let args = serde_json::json!({
            "path": "test_opd_write.txt",
            "content": "new content"
        });

        let allowed: HashSet<String> = HashSet::new();
        let result = execute_write_file("test-task", &args, &allowed, dir.path()).unwrap();
        assert!(result.contains("success"));

        // Verify file was written
        let content = std::fs::read_to_string(&temp_path).unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn test_execute_list_directory_and_search_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("beta.txt"), "text").unwrap();

        let allowed: HashSet<String> = HashSet::new();
        let list_args = serde_json::json!({ "path": "." });
        let list_result =
            execute_list_directory("test-task", &list_args, &allowed, dir.path()).unwrap();
        assert!(list_result.contains("alpha.rs"));
        assert!(list_result.contains("beta.txt"));

        let search_args = serde_json::json!({ "path": ".", "pattern": "*.rs" });
        let search_result =
            execute_search_files("test-task", &search_args, &allowed, dir.path()).unwrap();
        assert!(search_result.contains("alpha.rs"));
        assert!(!search_result.contains("beta.txt"));
    }

    #[test]
    fn test_execute_terminal_with_allowed_program() {
        let mut allowed = HashSet::new();
        allowed.insert("echo".to_string());

        let args = serde_json::json!({
            "command": "echo hello world"
        });

        let cwd = std::env::current_dir().unwrap();
        let result = execute_terminal("test-task", &args, &allowed, &cwd).unwrap();
        assert!(result.contains("hello world"));
        assert!(result.contains("exit_code"));
    }

    /// SECURITY: Programs not in the allow-list must be rejected (fail-closed).
    #[test]
    fn execute_terminal_blocks_unlisted_program() {
        let allowed: HashSet<String> = HashSet::new(); // empty → deny all

        let args = serde_json::json!({
            "command": "echo should-be-blocked"
        });

        let cwd = std::env::current_dir().unwrap();
        let result = execute_terminal("test-task", &args, &allowed, &cwd);
        assert!(result.is_err(), "unlisted program must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not in the allow-list") || msg.contains("allow-list"),
            "error must mention allow-list, got: {}",
            msg
        );
    }

    /// SECURITY: Shell metacharacters in args must NOT be interpreted as shell commands.
    /// The arg `"hello; rm -rf /"` must be passed literally to the program, not to sh.
    #[test]
    fn execute_terminal_shell_metacharacters_are_literal() {
        let mut allowed = HashSet::new();
        allowed.insert("echo".to_string());

        // The semicolon and rm are inside a quoted arg — shell_words must keep them literal.
        let args = serde_json::json!({
            "command": "echo 'hello; rm -rf /'"
        });

        let cwd = std::env::current_dir().unwrap();
        let result = execute_terminal("test-task", &args, &allowed, &cwd);
        assert!(
            result.is_ok(),
            "echo with literal arg must succeed: {:?}",
            result.err()
        );
        let output = result.unwrap();
        // The full string (including the semicolon) must appear in stdout
        assert!(
            output.contains("hello; rm -rf /"),
            "literal shell metacharacters must reach stdout, got: {}",
            output
        );
    }

    /// SECURITY: `with_allowed_programs` builder correctly populates the allow-list.
    #[test]
    fn with_allowed_programs_builder_sets_list() {
        let executor = ToolExecutor::new("t".to_string()).with_allowed_programs(["echo", "cat"]);

        assert!(executor.allowed_programs.contains("echo"));
        assert!(executor.allowed_programs.contains("cat"));
        assert!(!executor.allowed_programs.contains("sh"));
        assert!(!executor.allowed_programs.contains("bash"));
    }

    #[test]
    fn toolset_allowed_tools_include_file_tools_for_read_only_set() {
        use crate::toolset_distribution::ToolsetDistribution;

        let dist = ToolsetDistribution::hermes_style();
        let read_only = dist
            .toolsets
            .iter()
            .find(|toolset| toolset.name == "read_only")
            .unwrap();

        let allowed = read_only.allowed_tools();
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(allowed.contains(&"list_directory".to_string()));
        assert!(allowed.contains(&"search_files".to_string()));
        assert!(!allowed.contains(&"write_file".to_string()));
        assert!(!allowed.contains(&"terminal".to_string()));
    }

    #[test]
    fn toolset_restriction_keeps_disallowed_tool_visible_and_fails_execution() {
        use crate::toolset_distribution::ToolsetDistribution;

        let dir = tempfile::tempdir().unwrap();
        let dist = ToolsetDistribution::hermes_style();
        let read_only = dist
            .toolsets
            .iter()
            .find(|toolset| toolset.name == "read_only")
            .unwrap();
        let executor = ToolExecutor::new("test-task".to_string())
            .with_workspace_root(dir.path())
            .unwrap()
            .with_toolset(read_only);

        let calls = executor
            .parse_tool_calls("write_file(path='blocked.txt', content='must-not-write')")
            .unwrap();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");

        let result = executor.execute(&calls[0]).unwrap();
        assert!(!result.success);
        assert!(
            result.content.contains("not available in selected toolset"),
            "unexpected denial message: {}",
            result.content
        );
        assert!(!dir.path().join("blocked.txt").exists());
    }
}
