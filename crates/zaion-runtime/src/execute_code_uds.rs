//! Local-RPC code execution runtime for Python/JavaScript
//!
//! Architecture:
//! 1. Parent process (Zaion) creates a local RPC endpoint
//! 2. Parent spawns child process (Python/Node) with endpoint details in env
//! 3. Child loads generated stub module (zaion_tools.py/zaion_tools.js)
//! 4. Child executes user code, tool calls travel over UDS back to parent
//! 5. Parent dispatches tool calls and returns results over UDS
//!
//! This is Zaion's implementation of Hermes code_execution_tool.py architecture.
//!
//! Unix platforms use Unix domain sockets. Non-Unix platforms use an explicit
//! loopback TCP listener so the runtime remains usable without pretending to be
//! a stable sandbox/security boundary.
//! Experimental: this API is hidden from the stable CLI path and should not be
//! treated as stable sandbox/security infrastructure yet.

use serde::{Deserialize, Serialize};
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
use uuid::Uuid;

/// Tool dispatcher callback signature shared between the Python/Node sandbox
/// executors. Receives `(tool_name, args_json)` and returns `Result<json, err>`.
pub type ToolDispatcher =
    Arc<dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;

/// UDS RPC request from child process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdsRpcRequest {
    pub tool: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub token: Option<String>,
}

/// UDS RPC response to child process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdsRpcResponse {
    pub result: serde_json::Value,
}

/// Tool call record for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: serde_json::Value,
    pub timestamp: String,
}

/// Code execution request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCodeRequest {
    pub language: CodeLanguage,
    pub code: String,
    pub timeout_secs: u64,
    pub allowed_tools: Vec<String>,
    /// Maximum tool calls allowed (default: 50)
    pub max_tool_calls: Option<usize>,
    /// Maximum stdout bytes (default: 50KB)
    pub max_stdout_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodeLanguage {
    Python,
    JavaScript,
}

/// Code execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCodeResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub exit_code: Option<i32>,
}

/// Sandbox-allowed tools (matching Hermes SANDBOX_ALLOWED_TOOLS)
pub const SANDBOX_ALLOWED_TOOLS: &[&str] = &[
    "web_search",
    "web_extract",
    "read_file",
    "write_file",
    "search_files",
    "patch",
    "terminal",
];

pub const DEFAULT_EXECUTE_CODE_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS: usize = 50;
pub const DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES: usize = 50_000;
pub const DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES: usize = 10_000;

pub(crate) fn new_rpc_token() -> String {
    Uuid::new_v4().simple().to_string()
}

pub(crate) fn validate_rpc_token(
    request: &UdsRpcRequest,
    expected_token: &str,
) -> Option<UdsRpcResponse> {
    match request.token.as_deref() {
        Some(token) if token_matches(token, expected_token) => None,
        _ => Some(UdsRpcResponse {
            result: serde_json::json!({"error": "RPC authentication failed"}),
        }),
    }
}

fn token_matches(token: &str, expected_token: &str) -> bool {
    let token_bytes = token.as_bytes();
    let expected_bytes = expected_token.as_bytes();
    let max_len = token_bytes.len().max(expected_bytes.len());
    let mut diff = token_bytes.len() ^ expected_bytes.len();
    for idx in 0..max_len {
        diff |= usize::from(
            token_bytes.get(idx).copied().unwrap_or(0)
                ^ expected_bytes.get(idx).copied().unwrap_or(0),
        );
    }
    diff == 0
}

/// UDS code executor
pub struct UdsCodeExecutor {
    /// Tool dispatcher callback
    tool_dispatcher: ToolDispatcher,
    /// Optional cancellation token (kill spawned subprocess on cancel)
    cancel: Option<crate::cancel::CancelToken>,
}

impl UdsCodeExecutor {
    /// Create new UDS code executor with tool dispatcher
    pub fn new(tool_dispatcher: ToolDispatcher) -> Self {
        Self {
            tool_dispatcher,
            cancel: None,
        }
    }

    /// Register a cancellation token; on cancel the spawned subprocess is killed.
    pub fn with_cancel(mut self, cancel: crate::cancel::CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Execute code with UDS RPC bridge
    #[cfg(unix)]
    pub fn execute(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
        match request.language {
            CodeLanguage::Python => self.execute_python(request),
            CodeLanguage::JavaScript => {
                // Delegate to JsCodeExecutor
                let js_executor =
                    crate::execute_code_js::JsCodeExecutor::new(Arc::clone(&self.tool_dispatcher));
                js_executor.execute(request)
            }
        }
    }

    /// Execute code with a loopback RPC bridge on non-Unix platforms.
    #[cfg(not(unix))]
    pub fn execute(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
        match request.language {
            CodeLanguage::Python => self.execute_python_loopback(request),
            CodeLanguage::JavaScript => {
                let js_executor =
                    crate::execute_code_js::JsCodeExecutor::new(Arc::clone(&self.tool_dispatcher));
                js_executor.execute(request)
            }
        }
    }

    #[cfg(unix)]
    fn execute_python(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
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

        // Generate zaion_tools.py stub module
        let tools_src = self.generate_python_tools_module(&request.allowed_tools);
        std::fs::write(tmpdir_path.join("zaion_tools.py"), tools_src)
            .map_err(|e| format!("Failed to write zaion_tools.py: {}", e))?;

        // Write user script
        std::fs::write(tmpdir_path.join("script.py"), &request.code)
            .map_err(|e| format!("Failed to write script.py: {}", e))?;

        // Create UDS listener
        let listener = UnixListener::bind(&sock_path)
            .map_err(|e| format!("Failed to bind UDS socket: {}", e))?;

        // Shared state for tool call log and counter
        let tool_call_log = Arc::new(Mutex::new(Vec::new()));
        let tool_call_log_clone = Arc::clone(&tool_call_log);
        let tool_call_counter = Arc::new(Mutex::new(0usize));
        let tool_call_counter_clone = Arc::clone(&tool_call_counter);

        // Shutdown signal — set after the child exits so the accept loop in
        // rpc_server_loop wakes up and exits. Without this the RPC thread
        // leaks (blocked on listener.accept()) whenever the child never
        // connected, most notably in the timeout path.
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
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

        // Spawn Python subprocess
        let sock_path_str = sock_path.to_str().ok_or_else(|| {
            format!(
                "UDS socket path is not valid UTF-8: {}",
                sock_path.display()
            )
        })?;
        let mut child = Command::new(Self::python_program())
            .arg("script.py")
            .current_dir(tmpdir_path)
            .env("ZAION_RPC_SOCKET", sock_path_str)
            .env("ZAION_RPC_TOKEN", rpc_token)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn Python: {}", e))?;

        if let Some(cancel) = &self.cancel {
            cancel.register_child(&mut child);
        }

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

        // Signal the RPC loop to exit and join it before reading tool_call_log.
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

    #[cfg(not(unix))]
    fn execute_python_loopback(
        &self,
        request: &ExecuteCodeRequest,
    ) -> Result<ExecuteCodeResult, String> {
        let max_tool_calls = request
            .max_tool_calls
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS);
        let max_stdout_bytes = request
            .max_stdout_bytes
            .unwrap_or(DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES);

        let tmpdir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let tmpdir_path = tmpdir.path();

        let tools_src = self.generate_python_tools_module(&request.allowed_tools);
        std::fs::write(tmpdir_path.join("zaion_tools.py"), tools_src)
            .map_err(|e| format!("Failed to write zaion_tools.py: {}", e))?;
        std::fs::write(tmpdir_path.join("script.py"), &request.code)
            .map_err(|e| format!("Failed to write script.py: {}", e))?;

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

        let mut child = Command::new(Self::python_program())
            .arg("script.py")
            .current_dir(tmpdir_path)
            .env("ZAION_RPC_HOST", "127.0.0.1")
            .env("ZAION_RPC_PORT", addr.port().to_string())
            .env("ZAION_RPC_TOKEN", rpc_token)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn Python: {}", e))?;

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
        dispatcher: ToolDispatcher,
        tool_call_log: Arc<Mutex<Vec<ToolCallRecord>>>,
        tool_call_counter: Arc<Mutex<usize>>,
        max_tool_calls: usize,
        allowed_tools: Vec<String>,
        expected_rpc_token: String,
        shutdown: Arc<AtomicBool>,
    ) {
        // Poll listener in non-blocking mode so we observe the shutdown flag
        // and exit cleanly when the parent signals. Prevents leaking this
        // background thread on timeout paths where the child never connected.
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
        dispatcher: ToolDispatcher,
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
            let result = match request.tool.as_str() {
                "read_file" | "write_file" | "terminal" => {
                    // Delegate to main dispatcher for these tools
                    dispatcher(&request.tool, &request.args)
                }
                "web_search" | "web_extract" | "search_files" | "patch" => {
                    // Use sandbox tools implementation
                    crate::sandbox_tools::SandboxTools::dispatch(&request.tool, &request.args)
                }
                _ => Err(format!("Unknown tool: {}", request.tool)),
            };

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
        dispatcher: ToolDispatcher,
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
        dispatcher: ToolDispatcher,
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

            let result = match request.tool.as_str() {
                "read_file" | "write_file" | "terminal" => dispatcher(&request.tool, &request.args),
                "web_search" | "web_extract" | "search_files" | "patch" => {
                    crate::sandbox_tools::SandboxTools::dispatch(&request.tool, &request.args)
                }
                _ => Err(format!("Unknown tool: {}", request.tool)),
            };

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

    /// Generate Python zaion_tools.py stub module
    fn generate_python_tools_module(&self, enabled_tools: &[String]) -> String {
        let allowed: Vec<&str> = SANDBOX_ALLOWED_TOOLS
            .iter()
            .filter(|t| enabled_tools.contains(&t.to_string()))
            .copied()
            .collect();

        let mut stubs = Vec::new();
        for tool in &allowed {
            let stub = match *tool {
                "read_file" => {
                    r#"def read_file(path: str, offset: int = 1, limit: int = 500):
    """Read a file (1-indexed lines). Returns dict with content and total_lines."""
    return _call("read_file", {"path": path, "offset": offset, "limit": limit})
"#
                }
                "write_file" => {
                    r#"def write_file(path: str, content: str):
    """Write content to a file (always overwrites). Returns dict with status."""
    return _call("write_file", {"path": path, "content": content})
"#
                }
                "terminal" => {
                    r#"def terminal(command: str, timeout: int = None, workdir: str = None):
    """Run a shell command (foreground only). Returns dict with output and exit_code."""
    return _call("terminal", {"command": command, "timeout": timeout, "workdir": workdir})
"#
                }
                "web_search" => {
                    r#"def web_search(query: str, max_results: int = 10):
    """Search the web. Returns dict with results list."""
    return _call("web_search", {"query": query, "max_results": max_results})
"#
                }
                "web_extract" => {
                    r#"def web_extract(url: str):
    """Extract content from a URL. Returns dict with text content."""
    return _call("web_extract", {"url": url})
"#
                }
                "search_files" => {
                    r#"def search_files(pattern: str, path: str = "."):
    """Search for files matching pattern. Returns dict with file paths."""
    return _call("search_files", {"pattern": pattern, "path": path})
"#
                }
                "patch" => {
                    r#"def patch(file_path: str, old_text: str, new_text: str):
    """Apply a patch to a file. Returns dict with status."""
    return _call("patch", {"file_path": file_path, "old_text": old_text, "new_text": new_text})
"#
                }
                _ => continue,
            };
            stubs.push(stub);
        }

        format!(
            r#"# Auto-generated Zaion tools RPC stubs.
import json, os, socket

_sock = None

def _connect():
    global _sock
    if _sock is None:
        if os.environ.get("ZAION_RPC_PORT"):
            _sock = socket.create_connection((
                os.environ.get("ZAION_RPC_HOST", "127.0.0.1"),
                int(os.environ["ZAION_RPC_PORT"]),
            ), timeout=300)
        else:
            _sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            _sock.connect(os.environ["ZAION_RPC_SOCKET"])
        _sock.settimeout(300)
    return _sock

def _call(tool_name, args):
    """Send a tool call to the parent process and return the parsed result."""
    conn = _connect()
    request = json.dumps({{
        "tool": tool_name,
        "args": args,
        "token": os.environ.get("ZAION_RPC_TOKEN"),
    }}) + "\n"
    conn.sendall(request.encode())
    buf = b""
    while True:
        chunk = conn.recv(65536)
        if not chunk:
            raise RuntimeError("Parent process disconnected")
        buf += chunk
        if buf.endswith(b"\n"):
            break
    raw = buf.decode().strip()
    result = json.loads(raw)
    if isinstance(result, dict) and "result" in result:
        return result["result"]
    return result

{}
"#,
            stubs.join("\n")
        )
    }

    fn python_program() -> &'static str {
        let candidates: &[&str] = if cfg!(windows) {
            &["python", "py", "python3"]
        } else {
            &["python3", "python"]
        };
        candidates
            .iter()
            .copied()
            .find(|program| command_available(program))
            .unwrap_or(candidates[0])
    }
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_python_tools_module() {
        let dispatcher = Arc::new(|_tool: &str, _args: &serde_json::Value| {
            Ok(serde_json::json!({"status": "ok"}))
        });
        let executor = UdsCodeExecutor::new(dispatcher);

        let tools = vec!["read_file".to_string(), "write_file".to_string()];
        let module = executor.generate_python_tools_module(&tools);

        assert!(module.contains("def read_file"));
        assert!(module.contains("def write_file"));
        assert!(module.contains("def _call"));
        assert!(module.contains("ZAION_RPC_SOCKET"));
        assert!(module.contains("ZAION_RPC_TOKEN"));
        assert!(module.contains("\"token\": os.environ.get(\"ZAION_RPC_TOKEN\")"));
    }

    #[test]
    fn test_execute_code_request_serialization() {
        let request = ExecuteCodeRequest {
            language: CodeLanguage::Python,
            code: "print('hello')".to_string(),
            timeout_secs: 30,
            allowed_tools: vec!["read_file".to_string()],
            max_tool_calls: Some(50),
            max_stdout_bytes: Some(50_000),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: ExecuteCodeRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.language, CodeLanguage::Python);
        assert_eq!(parsed.code, "print('hello')");
    }

    #[test]
    fn test_tool_call_record_creation() {
        let record = ToolCallRecord {
            tool_name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            result: serde_json::json!({"content": "hello"}),
            timestamp: "2026-04-16T00:00:00Z".to_string(),
        };

        assert_eq!(record.tool_name, "read_file");
        assert!(record.arguments.is_object());
    }

    #[test]
    fn rpc_token_validation_rejects_missing_or_wrong_token() {
        let request = UdsRpcRequest {
            tool: "read_file".to_string(),
            args: serde_json::json!({}),
            token: None,
        };
        assert!(validate_rpc_token(&request, "expected").is_some());

        let request = UdsRpcRequest {
            token: Some("wrong".to_string()),
            ..request
        };
        assert!(validate_rpc_token(&request, "expected").is_some());

        let request = UdsRpcRequest {
            token: Some("expected".to_string()),
            ..request
        };
        assert!(validate_rpc_token(&request, "expected").is_none());
    }
}