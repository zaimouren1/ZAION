//! `shell_exec` handler plus the command allow-list and risk classification.

use std::path::Path;
use std::time::Duration;

use serde_json::json;

use super::{shell_arg_stays_in_workspace, truncate_output, workspace_root};
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

// ── Security allow-list ───────────────────────────────────────────────────────

/// Commands that `shell_exec` is permitted to run.
pub(super) const ALLOWED_COMMANDS: &[&str] =
    &["git", "cargo", "echo", "ls", "dir", "cat", "type", "python", "python3"];

fn is_allowed_command(cmd: &str) -> bool {
    let base = std::path::Path::new(cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);
    ALLOWED_COMMANDS
        .iter()
        .any(|&allowed| allowed.eq_ignore_ascii_case(base))
}

// ── Sub-command risk classification (#2 deny-rules + #9 read-only) ──────────────
//
// Ported from Claude Code's bash permission model. The base allow-list gates
// which executable may run, but a permitted executable can still reach dangerous
// sub-commands (`git push`, `cargo publish`, …). We layer two checks that run
// AFTER the allow-list:
//
//   #2 deny-rules:    (executable, sub-command) pairs that are ALWAYS rejected,
//                     regardless of context. Destructive or network-reaching
//                     operations live here.
//   #9 read-only:     when the caller requests `read_only: true`, only commands
//                     classified `ReadOnly` pass; anything `Mutating` is rejected.
//
// Rationale: shell_exec runs autonomously inside the tool loop with no per-call
// human confirmation, so destructive sub-commands are denied by default and the
// user can still run them manually.

/// Risk class of a concrete (executable + sub-command) invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    /// Pure observation; safe under a read-only contract.
    ReadOnly,
    /// Has side effects but is permitted outside a read-only contract.
    Mutating,
    /// Never permitted, even with explicit allow-listing of the executable.
    Denied,
}

/// (executable, sub-command) pairs that are denied even when the executable is
/// allow-listed. Destructive history rewrites, force operations, and anything
/// that publishes/installs from a network registry.
const DENIED_SUBCOMMANDS: &[(&str, &str)] = &[
    ("git", "push"),
    ("git", "clean"),
    ("git", "filter-branch"),
    ("cargo", "publish"),
    ("cargo", "install"),
    ("cargo", "yank"),
    ("cargo", "login"),
];

/// (executable, sub-command) pairs that are read-only observations. Anything on
/// an allow-listed executable that is NOT here is treated as `Mutating`.
const READ_ONLY_SUBCOMMANDS: &[(&str, &str)] = &[
    ("git", "status"),
    ("git", "log"),
    ("git", "diff"),
    ("git", "show"),
    ("git", "branch"),
    ("git", "remote"),
    ("git", "tag"),
    ("git", "blame"),
    ("git", "describe"),
    ("git", "rev-parse"),
    ("git", "ls-files"),
    ("git", "config"), // read form; `--global` writes are still scoped to args
    ("cargo", "check"),
    ("cargo", "tree"),
    ("cargo", "metadata"),
    ("cargo", "search"),
    ("cargo", "version"),
    ("cargo", "--version"),
];

/// Executables that are themselves wholly read-only regardless of sub-command.
const READ_ONLY_COMMANDS: &[&str] = &["echo", "ls", "dir", "cat", "type"];

/// Normalize an executable path to its lowercase base name (no extension).
fn command_base(cmd: &str) -> String {
    Path::new(cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
        .to_ascii_lowercase()
}

/// Extract the first non-flag argument as the sub-command token (lowercased).
fn first_subcommand(args: &[String]) -> Option<String> {
    args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|a| a.to_ascii_lowercase())
}

/// Classify a concrete invocation into a [`CommandRisk`]. Deny-rules win over
/// everything; otherwise the (command, sub-command) is looked up in the
/// read-only tables, defaulting to `Mutating` for allow-listed executables that
/// reach an unrecognized sub-command.
pub(super) fn classify_command(cmd: &str, args: &[String]) -> CommandRisk {
    let base = command_base(cmd);
    let sub = first_subcommand(args);

    // #2 — deny-rules take precedence.
    if let Some(ref s) = sub {
        if DENIED_SUBCOMMANDS
            .iter()
            .any(|(c, d)| *c == base && *d == s.as_str())
        {
            return CommandRisk::Denied;
        }
    }

    // Wholly read-only executables (echo/ls/cat/…): always read-only.
    if READ_ONLY_COMMANDS.iter().any(|&c| c == base) {
        return CommandRisk::ReadOnly;
    }

    // Sub-command-gated executables (git/cargo): match the read-only table.
    match sub {
        Some(ref s)
            if READ_ONLY_SUBCOMMANDS
                .iter()
                .any(|(c, d)| *c == base && *d == s.as_str()) =>
        {
            CommandRisk::ReadOnly
        }
        // Bare `git`/`cargo` (no sub-command) just prints help → read-only.
        None => CommandRisk::ReadOnly,
        // Recognized executable, unrecognized sub-command → assume mutating.
        _ => CommandRisk::Mutating,
    }
}

// ── shell_exec ────────────────────────────────────────────────────────────────

pub(super) fn shell_exec_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let command = input["command"]
        .as_str()
        .ok_or_else(|| "missing 'command' parameter".to_string())?;

    // SECURITY GATE — reject anything not on the allow-list.
    if !is_allowed_command(command) {
        return Err(format!(
            "command '{}' is not in the allow-list {:?}",
            command, ALLOWED_COMMANDS
        ));
    }

    let args: Vec<String> = match input.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    };

    for arg in &args {
        if !shell_arg_stays_in_workspace(arg) {
            return Err(format!(
                "argument '{}' is outside the workspace policy",
                arg
            ));
        }
    }

    // SECURITY GATE #2/#9 — sub-command risk classification.
    //  • Denied pairs are rejected unconditionally.
    //  • Under a read-only contract, only ReadOnly invocations pass.
    let read_only = input
        .get("read_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match classify_command(command, &args) {
        CommandRisk::Denied => {
            return Err(format!(
                "command '{}{}' is denied by policy (destructive or network-reaching)",
                command,
                first_subcommand(&args)
                    .map(|s| format!(" {}", s))
                    .unwrap_or_default()
            ));
        }
        CommandRisk::Mutating if read_only => {
            return Err(format!(
                "command '{}{}' is mutating and the call requested read_only=true",
                command,
                first_subcommand(&args)
                    .map(|s| format!(" {}", s))
                    .unwrap_or_default()
            ));
        }
        _ => {}
    }

    if command.eq_ignore_ascii_case("echo") {
        return Ok(json!({
            "stdout": format!("{}\n", args.join(" ")),
            "stderr": "",
            "exit_code": 0,
        }));
    }

    let timeout_secs: u64 = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(10);

    let mut cmd = std::process::Command::new(command);
    cmd.args(&args);
    cmd.current_dir(workspace_root()?);

    // Capture stdout and stderr.
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn '{}': {}", command, e))?;

    // Manual timeout: poll until the process exits or time runs out.
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(format!(
                        "command '{}' timed out after {} seconds",
                        command, timeout_secs
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!("error waiting for process: {}", e));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to collect output of '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);

    // Route potentially-unbounded output through the truncation gate so a
    // noisy command can't blow up the model's context. Full output spills to
    // disk; the model gets a head+tail preview plus the spill path.
    let out = truncate_output("shell_exec_stdout", &stdout);
    let err = truncate_output("shell_exec_stderr", &stderr);

    Ok(json!({
        "stdout": out.text,
        "stderr": err.text,
        "exit_code": exit_code,
        "stdout_truncated": out.truncated,
        "stderr_truncated": err.truncated,
        "stdout_total_lines": out.total_lines,
        "stderr_total_lines": err.total_lines,
        "stdout_total_bytes": out.total_bytes,
        "stderr_total_bytes": err.total_bytes,
        "stdout_full_path": out.spill_path,
        "stderr_full_path": err.spill_path,
    }))
}

/// Register the `shell_exec` tool into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    // shell_exec
    registry.register(McpTool::new(
        McpToolMeta::new(
            "shell_exec",
            "1.0",
            "Execute an allow-listed shell command (git, cargo, echo, ls, dir, cat, type).",
            McpSchema::new(vec![
                McpParam::required(
                    "command",
                    McpParamType::String,
                    "command name (must be on the allow-list)",
                ),
                McpParam::optional("args", McpParamType::Array, "list of arguments", json!([])),
                McpParam::optional(
                    "timeout_secs",
                    McpParamType::Number,
                    "maximum execution time in seconds (default 10)",
                    json!(10),
                ),
                McpParam::optional(
                    "read_only",
                    McpParamType::Boolean,
                    "when true, reject any mutating sub-command (e.g. git commit, cargo build)",
                    json!(false),
                ),
            ]),
            "execute",
        ),
        shell_exec_handler,
    ));
}
