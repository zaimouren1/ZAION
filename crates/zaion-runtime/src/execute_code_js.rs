//! JavaScript/Node.js code execution runtime with local JSONL RPC bridge
//!
//! Architecture mirrors Python implementation:
//! 1. Parent process (Zaion) creates a local RPC endpoint
//! 2. Parent spawns Node.js child process with endpoint details in env
//! 3. Child loads generated stub module (zaion_tools.js)
//! 4. Child executes user code, tool calls travel over UDS back to parent
//! 5. Parent dispatches tool calls and returns results over UDS
//!
//! Experimental: this API is hidden from the stable CLI path and should not be
//! treated as stable sandbox/security infrastructure yet.

use std::io::{BufRead, BufReader, Write};
#[cfg(not(unix))]
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::execute_code_uds::{
    new_rpc_token, validate_rpc_token, DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES,
    DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES, DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS,
    DEFAULT_EXECUTE_CODE_TIMEOUT_SECS,
};
use super::execute_code_uds::{
    ExecuteCodeRequest, ExecuteCodeResult, ToolCallRecord, UdsRpcRequest, UdsRpcResponse,
    SANDBOX_ALLOWED_TOOLS,
};

/// JavaScript code executor using Node.js subprocess + UDS RPC
pub struct JsCodeExecutor {
    /// Tool dispatcher callback
    tool_dispatcher: super::execute_code_uds::ToolDispatcher,
}

impl JsCodeExecutor {
    /// Create new JavaScript code executor with tool dispatcher
    pub fn new(tool_dispatcher: super::execute_code_uds::ToolDispatcher) -> Self {
        Self { tool_dispatcher }
    }

    /// Execute JavaScript code with UDS RPC bridge
    #[cfg(unix)]
    pub fn execute(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
        // Resource limits
        let max_tool_calls = request
            .max_tool_calls
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS);
        let max_stdout_bytes = request
            .max_stdout_bytes
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES);

        // Create temp directory for sandbox
        let tmpdir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let tmpdir_path = tmpdir.path();

        // Create socket path
        let sock_path = tmpdir_path.join("zaion_rpc.sock");

        // Generate zaion_tools.js stub module
        let tools_src = self.generate_js_tools_module(&request.allowed_tools);
        std::fs::write(tmpdir_path.join("zaion_tools.js"), tools_src)
            .map_err(|e| format!("Failed to write zaion_tools.js: {}", e))?;

        // Write user script
        std::fs::write(tmpdir_path.join("script.js"), &request.code)
            .map_err(|e| format!("Failed to write script.js: {}", e))?;

        // Create UDS listener
        let listener = UnixListener::bind(&sock_path)
            .map_err(|e| format!("Failed to bind UDS socket: {}", e))?;

        // Shared state for tool call log and counter
        let tool_call_log = Arc::new(Mutex::new(Vec::new()));
        let tool_call_log_clone = Arc::clone(&tool_call_log);
        let tool_call_counter = Arc::new(Mutex::new(0usize));
        let tool_call_counter_clone = Arc::clone(&tool_call_counter);

        // Shutdown signal — set after the child exits so the RPC accept loop
        // wakes up and returns. Without this the listener.accept() blocks
        // forever if the child never connected (e.g. in the timeout path)
        // and the background thread leaks.
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        // Non-blocking accept so the worker loop can poll the shutdown flag.
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {}", e))?;

        // Start RPC server thread
        let dispatcher = Arc::clone(&self.tool_dispatcher);
        let allowed_tools: Vec<String> = request.allowed_tools.clone();
        let rpc_token = new_rpc_token();
        let rpc_token_for_thread = rpc_token.clone();
        let rpc_thread = thread::spawn(move || {
            Self::rpc_server_loop(
                listener,
                dispatcher,
                tool_call_log_clone,
                tool_call_counter_clone,
                max_tool_calls,
                allowed_tools,
                rpc_token_for_thread,
                shutdown_clone,
            )
        });

        // Spawn Node.js subprocess
        let sock_path_str = sock_path.to_str().ok_or_else(|| {
            format!(
                "UDS socket path is not valid UTF-8: {}",
                sock_path.display()
            )
        })?;
        let mut child = Command::new("node")
            .arg("script.js")
            .current_dir(tmpdir_path)
            .env("ZAION_RPC_SOCKET", sock_path_str)
            .env("ZAION_RPC_TOKEN", rpc_token)
            .env("NODE_NO_WARNINGS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn Node.js: {}", e))?;

        // Collect stdout/stderr with timeout and size limits
        let timeout_secs = if request.timeout_secs == 0 {
            DEFAULT_EXECUTE_CODE_TIMEOUT_SECS
        } else {
            request.timeout_secs
        };
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        let stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout was not piped".to_string())?;
        let stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| "child stderr was not piped".to_string())?;

        let stdout_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stdout_handle);
            let mut output = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                output.push_str(&line);
                if output.len() > max_stdout_bytes {
                    output.truncate(max_stdout_bytes);
                    output.push_str("\n[OUTPUT TRUNCATED: exceeded max_stdout_bytes limit]");
                    break;
                }
                line.clear();
            }
            output
        });

        let stderr_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stderr_handle);
            let mut output = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                output.push_str(&line);
                if output.len() > DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES {
                    output.truncate(DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES);
                    output.push_str("\n[STDERR TRUNCATED]");
                    break;
                }
                line.clear();
            }
            output
        });

        // Wait for process with timeout
        let exit_code = loop {
            if start.elapsed() > timeout {
                // Timeout - kill process
                let _ = child.kill();
                break None;
            }

            match child.try_wait() {
                Ok(Some(status)) => break Some(status.code().unwrap_or(-1)),
                Ok(None) => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break Some(-1),
            }
        };

        // Collect output
        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        // Signal the RPC server loop to exit and wait for it to finish.
        // This prevents leaking the background accept()-blocked thread when
        // the child never connected (e.g. timeout path) and guarantees that
        // `tool_call_log` below sees a stable snapshot.
        shutdown.store(true, Ordering::Release);
        if let Err(e) = rpc_thread.join() {
            eprintln!("RPC thread panicked during shutdown: {:?}", e);
        }

        // Extract tool call log (now safe: RPC thread has joined).
        let tool_calls = tool_call_log.lock().unwrap().clone();

        let success = exit_code == Some(0);

        Ok(ExecuteCodeResult {
            success,
            stdout,
            stderr,
            tool_calls,
            exit_code,
        })
    }

    /// Execute JavaScript code through a loopback RPC bridge on non-Unix platforms.
    #[cfg(not(unix))]
    pub fn execute(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
        let max_tool_calls = request
            .max_tool_calls
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS);
        let max_stdout_bytes = request
            .max_stdout_bytes
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES);

        let tmpdir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let tmpdir_path = tmpdir.path();

        let tools_src = self.generate_js_tools_module(&request.allowed_tools);
        std::fs::write(tmpdir_path.join("zaion_tools.js"), tools_src)
            .map_err(|e| format!("Failed to write zaion_tools.js: {}", e))?;
        std::fs::write(tmpdir_path.join("script.js"), &request.code)
            .map_err(|e| format!("Failed to write script.js: {}", e))?;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("Failed to bind loopback RPC listener: {}", e))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read loopback RPC address: {}", e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {}", e))?;

        let tool_call_log = Arc::new(Mutex::new(Vec::new()));
        let tool_call_log_clone = Arc::clone(&tool_call_log);
        let tool_call_counter = Arc::new(Mutex::new(0usize));
        let tool_call_counter_clone = Arc::clone(&tool_call_counter);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let dispatcher = Arc::clone(&self.tool_dispatcher);
        let allowed_tools = request.allowed_tools.clone();
        let rpc_token = new_rpc_token();
        let rpc_token_for_thread = rpc_token.clone();

        let rpc_thread = thread::spawn(move || {
            Self::tcp_rpc_server_loop(
                listener,
                dispatcher,
                tool_call_log_clone,
                tool_call_counter_clone,
                max_tool_calls,
                allowed_tools,
                rpc_token_for_thread,
                shutdown_clone,
            )
        });

        let mut child = Command::new("node")
            .arg("script.js")
            .current_dir(tmpdir_path)
            .env("ZAION_RPC_HOST", "127.0.0.1")
            .env("ZAION_RPC_PORT", addr.port().to_string())
            .env("ZAION_RPC_TOKEN", rpc_token)
            .env("NODE_NO_WARNINGS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn Node.js: {}", e))?;

        let timeout_secs = if request.timeout_secs == 0 {
            DEFAULT_EXECUTE_CODE_TIMEOUT_SECS
        } else {
            request.timeout_secs
        };
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        let stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout was not piped".to_string())?;
        let stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| "child stderr was not piped".to_string())?;

        let stdout_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stdout_handle);
            let mut output = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                output.push_str(&line);
                if output.len() > max_stdout_bytes {
                    output.truncate(max_stdout_bytes);
                    output.push_str("\n[OUTPUT TRUNCATED: exceeded max_stdout_bytes limit]");
                    break;
                }
                line.clear();
            }
            output
        });

        let stderr_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stderr_handle);
            let mut output = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                output.push_str(&line);
                if output.len() > DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES {
                    output.truncate(DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES);
                    output.push_str("\n[STDERR TRUNCATED]");
                    break;
                }
                line.clear();
            }
            output
        });

        let exit_code = loop {
            if start.elapsed() > timeout {
                let _ = child.kill();
                break None;
            }

            match child.try_wait() {
                Ok(Some(status)) => break Some(status.code().unwrap_or(-1)),
                Ok(None) => thread::sleep(Duration::from_millis(100)),
                Err(_) => break Some(-1),
            }
        };

        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        shutdown.store(true, Ordering::Release);
        if let Err(e) = rpc_thread.join() {
            eprintln!("RPC thread panicked during shutdown: {:?}", e);
        }

        let tool_calls = tool_call_log.lock().unwrap().clone();
        let success = exit_code == Some(0);

        Ok(ExecuteCodeResult {
            success,
            stdout,
            stderr,
            tool_calls,
            exit_code,
        })
    }

    /// RPC server loop (runs in background thread)
    #[cfg(unix)]
    fn rpc_server_loop(
        listener: UnixListener,
        dispatcher: super::execute_code_uds::ToolDispatcher,
        tool_call_log: Arc<Mutex<Vec<ToolCallRecord>>>,
        tool_call_counter: Arc<Mutex<usize>>,
        max_tool_calls: usize,
        allowed_tools: Vec<String>,
        expected_rpc_token: String,
        shutdown: Arc<AtomicBool>,
    ) {
        // Poll for an incoming connection in non-blocking mode so we can
        // observe the shutdown flag and exit cleanly when the parent signals.
        while !shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(e) = Self::handle_rpc_connection(
                        stream,
                        Arc::clone(&dispatcher),
                        Arc::clone(&tool_call_log),
                        Arc::clone(&tool_call_counter),
                        max_tool_calls,
                        allowed_tools.clone(),
                        &expected_rpc_token,
                        Arc::clone(&shutdown),
                    ) {
                        eprintln!("RPC connection error: {}", e);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    eprintln!("RPC accept error: {}", e);
                    return;
                }
            }
        }
    }

    /// Handle RPC connection from child process
    #[cfg(unix)]
    fn handle_rpc_connection(
        mut stream: UnixStream,
        dispatcher: super::execute_code_uds::ToolDispatcher,
        tool_call_log: Arc<Mutex<Vec<ToolCallRecord>>>,
        tool_call_counter: Arc<Mutex<usize>>,
        max_tool_calls: usize,
        allowed_tools: Vec<String>,
        expected_rpc_token: &str,
        shutdown: Arc<AtomicBool>,
    ) -> Result<(), String> {
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    continue;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e.to_string()),
            };
            if line.trim().is_empty() {
                continue;
            }

            // Parse request
            let request: UdsRpcRequest = serde_json::from_str(&line)
                .map_err(|e| format!("Failed to parse RPC request: {}", e))?;

            if let Some(error_response) = validate_rpc_token(&request, expected_rpc_token) {
                let response_json = serde_json::to_string(&error_response).unwrap();
                writeln!(stream, "{}", response_json).map_err(|e: std::io::Error| e.to_string())?;
                continue;
            }

            // Check tool call limit
            {
                let mut counter = tool_call_counter.lock().unwrap();
                if *counter >= max_tool_calls {
                    let error_response = UdsRpcResponse {
                        result: serde_json::json!({
                            "error": format!("Tool call limit exceeded (max: {})", max_tool_calls)
                        }),
                    };
                    let response_json = serde_json::to_string(&error_response).unwrap();
                    writeln!(stream, "{}", response_json)
                        .map_err(|e: std::io::Error| e.to_string())?;
                    continue;
                }
                *counter += 1;
            }

            // Check if tool is allowed
            if !allowed_tools.contains(&request.tool) {
                let error_response = UdsRpcResponse {
                    result: serde_json::json!({
                        "error": format!("Tool '{}' not allowed", request.tool)
                    }),
                };
                let response_json = serde_json::to_string(&error_response).unwrap();
                writeln!(stream, "{}", response_json).map_err(|e: std::io::Error| e.to_string())?;
                continue;
            }

            // Dispatch tool call
            let result = dispatcher(&request.tool, &request.args);

            // Record tool call
            let timestamp = chrono::Utc::now().to_rfc3339();
            let record = ToolCallRecord {
                tool_name: request.tool.clone(),
                arguments: request.args.clone(),
                result: result
                    .clone()
                    .unwrap_or_else(|e| serde_json::json!({"error": e})),
                timestamp,
            };
            tool_call_log.lock().unwrap().push(record);

            // Send response
            let response = UdsRpcResponse {
                result: result.unwrap_or_else(|e| serde_json::json!({"error": e})),
            };
            let response_json = serde_json::to_string(&response).unwrap();
            writeln!(stream, "{}", response_json).map_err(|e: std::io::Error| e.to_string())?;
        }

        Ok(())
    }

    #[cfg(not(unix))]
    #[allow(clippy::too_many_arguments)]
    fn tcp_rpc_server_loop(
        listener: TcpListener,
        dispatcher: super::execute_code_uds::ToolDispatcher,
        tool_call_log: Arc<Mutex<Vec<ToolCallRecord>>>,
        tool_call_counter: Arc<Mutex<usize>>,
        max_tool_calls: usize,
        allowed_tools: Vec<String>,
        expected_rpc_token: String,
        shutdown: Arc<AtomicBool>,
    ) {
        while !shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(e) = stream.set_nonblocking(false) {
                        eprintln!("RPC stream blocking-mode error: {}", e);
                        continue;
                    }
                    if let Err(e) = Self::handle_tcp_rpc_connection(
                        stream,
                        Arc::clone(&dispatcher),
                        Arc::clone(&tool_call_log),
                        Arc::clone(&tool_call_counter),
                        max_tool_calls,
                        allowed_tools.clone(),
                        &expected_rpc_token,
                        Arc::clone(&shutdown),
                    ) {
                        eprintln!("RPC connection error: {}", e);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    eprintln!("RPC accept error: {}", e);
                    return;
                }
            }
        }
    }

    #[cfg(not(unix))]
    #[allow(clippy::too_many_arguments)]
    fn handle_tcp_rpc_connection(
        mut stream: TcpStream,
        dispatcher: super::execute_code_uds::ToolDispatcher,
        tool_call_log: Arc<Mutex<Vec<ToolCallRecord>>>,
        tool_call_counter: Arc<Mutex<usize>>,
        max_tool_calls: usize,
        allowed_tools: Vec<String>,
        expected_rpc_token: &str,
        shutdown: Arc<AtomicBool>,
    ) -> Result<(), String> {
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    continue;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e.to_string()),
            };
            if line.trim().is_empty() {
                continue;
            }

            let request: UdsRpcRequest = serde_json::from_str(&line)
                .map_err(|e| format!("Failed to parse RPC request: {}", e))?;

            if let Some(response) = validate_rpc_token(&request, expected_rpc_token) {
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap())
                    .map_err(|e: std::io::Error| e.to_string())?;
                continue;
            }

            {
                let mut counter = tool_call_counter.lock().unwrap();
                if *counter >= max_tool_calls {
                    let response = UdsRpcResponse {
                        result: serde_json::json!({
                            "error": format!("Tool call limit exceeded (max: {})", max_tool_calls)
                        }),
                    };
                    writeln!(stream, "{}", serde_json::to_string(&response).unwrap())
                        .map_err(|e: std::io::Error| e.to_string())?;
                    continue;
                }
                *counter += 1;
            }

            if !allowed_tools.contains(&request.tool) {
                let response = UdsRpcResponse {
                    result: serde_json::json!({
                        "error": format!("Tool '{}' not allowed", request.tool)
                    }),
                };
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap())
                    .map_err(|e: std::io::Error| e.to_string())?;
                continue;
            }

            let result = dispatcher(&request.tool, &request.args);
            let record = ToolCallRecord {
                tool_name: request.tool.clone(),
                arguments: request.args.clone(),
                result: result
                    .clone()
                    .unwrap_or_else(|e| serde_json::json!({"error": e})),
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            tool_call_log.lock().unwrap().push(record);

            let response = UdsRpcResponse {
                result: result.unwrap_or_else(|e| serde_json::json!({"error": e})),
            };
            writeln!(stream, "{}", serde_json::to_string(&response).unwrap())
                .map_err(|e: std::io::Error| e.to_string())?;
        }

        Ok(())
    }

    /// Generate JavaScript zaion_tools.js stub module
    fn generate_js_tools_module(&self, enabled_tools: &[String]) -> String {
        let allowed: Vec<&str> = SANDBOX_ALLOWED_TOOLS
            .iter()
            .filter(|t| enabled_tools.contains(&t.to_string()))
            .copied()
            .collect();

        let mut stubs = Vec::new();
        let mut exports = Vec::new();
        for tool in &allowed {
            let (export_name, stub) = match *tool {
                "read_file" => (
                    "readFile",
                    r#"function readFile(path, offset = 1, limit = 500) {
  // Read a file (1-indexed lines). Returns object with content and total_lines.
  return _call("read_file", { path, offset, limit });
}
"#,
                ),
                "write_file" => (
                    "writeFile",
                    r#"function writeFile(path, content) {
  // Write content to a file (always overwrites). Returns object with status.
  return _call("write_file", { path, content });
}
"#,
                ),
                "terminal" => (
                    "terminal",
                    r#"function terminal(command, timeout = null, workdir = null) {
  // Run a shell command (foreground only). Returns object with output and exit_code.
  return _call("terminal", { command, timeout, workdir });
}
"#,
                ),
                "web_search" => (
                    "webSearch",
                    r#"function webSearch(query, maxResults = 10) {
  // Search the web. Returns object with results list.
  return _call("web_search", { query, max_results: maxResults });
}
"#,
                ),
                "web_extract" => (
                    "webExtract",
                    r#"function webExtract(url) {
  // Extract content from a URL. Returns object with text content.
  return _call("web_extract", { url });
}
"#,
                ),
                "search_files" => (
                    "searchFiles",
                    r#"function searchFiles(pattern, path = ".") {
  // Search for files matching pattern. Returns object with file paths.
  return _call("search_files", { pattern, path });
}
"#,
                ),
                "patch" => (
                    "patch",
                    r#"function patch(filePath, oldText, newText) {
  // Apply a patch to a file. Returns object with status.
  return _call("patch", { file_path: filePath, old_text: oldText, new_text: newText });
}
"#,
                ),
                _ => continue,
            };
            stubs.push(stub);
            exports.push(export_name);
        }
        let module_exports = exports
            .iter()
            .map(|name| format!("  {name},"))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"// Auto-generated Zaion tools RPC stubs
const net = require('net');
const fs = require('fs');

function _connect() {{
  let sock;
  if (process.env.ZAION_RPC_PORT) {{
    sock = net.createConnection({{
      host: process.env.ZAION_RPC_HOST || '127.0.0.1',
      port: Number(process.env.ZAION_RPC_PORT),
    }});
  }} else {{
    sock = net.createConnection(process.env.ZAION_RPC_SOCKET);
  }}
  sock.setEncoding('utf8');
  return sock;
}}

function _call(toolName, args) {{
  return new Promise((resolve, reject) => {{
    const conn = _connect();
    const request = JSON.stringify({{
      tool: toolName,
      args,
      token: process.env.ZAION_RPC_TOKEN,
    }}) + '\n';

    let buffer = '';
    const onData = (chunk) => {{
      buffer += chunk;
      if (buffer.endsWith('\n')) {{
        conn.removeListener('data', onData);
        try {{
          const result = JSON.parse(buffer.trim());
          if (result.result !== undefined) {{
            resolve(result.result);
          }} else {{
            resolve(result);
          }}
        }} catch (e) {{
          reject(new Error('Failed to parse response: ' + e.message));
        }} finally {{
          conn.end();
        }}
      }}
    }};

    conn.on('data', onData);
    conn.on('error', reject);
    conn.write(request);
  }});
}}

{}

module.exports = {{
{}
}};
"#,
            stubs.join("\n"),
            module_exports
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_generate_js_tools_module() {
        let dispatcher = Arc::new(|_tool: &str, _args: &serde_json::Value| {
            Ok(serde_json::json!({"status": "ok"}))
        });
        let executor = JsCodeExecutor::new(dispatcher);

        let tools = vec!["read_file".to_string(), "write_file".to_string()];
        let module = executor.generate_js_tools_module(&tools);

        assert!(module.contains("function readFile"));
        assert!(module.contains("function writeFile"));
        assert!(module.contains("function _call"));
        assert!(module.contains("ZAION_RPC_SOCKET"));
        assert!(module.contains("ZAION_RPC_TOKEN"));
        assert!(module.contains("token: process.env.ZAION_RPC_TOKEN"));
    }

    #[test]
    fn test_js_executor_creation() {
        let dispatcher = Arc::new(|_tool: &str, _args: &serde_json::Value| {
            Ok(serde_json::json!({"status": "ok"}))
        });
        let executor = JsCodeExecutor::new(dispatcher);

        // Just verify it compiles and creates
        let tools = vec!["terminal".to_string()];
        let module = executor.generate_js_tools_module(&tools);
        assert!(module.contains("function terminal"));
    }

    #[test]
    fn test_js_module_exports_all_tools() {
        let dispatcher = Arc::new(|_tool: &str, _args: &serde_json::Value| {
            Ok(serde_json::json!({"status": "ok"}))
        });
        let executor = JsCodeExecutor::new(dispatcher);

        let tools = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "terminal".to_string(),
            "web_search".to_string(),
        ];
        let module = executor.generate_js_tools_module(&tools);

        assert!(module.contains("module.exports"));
        assert!(module.contains("readFile"));
        assert!(module.contains("writeFile"));
        assert!(module.contains("terminal"));
        assert!(module.contains("webSearch"));
    }
}
