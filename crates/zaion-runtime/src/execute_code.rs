//! C5: Programmatic tool execution — Python/JS code → UDS RPC → tool dispatch
//!
//! Allows LLM to generate Python/JavaScript code that calls tools via Unix Domain Socket RPC.
//! Architecture: zaion spawns a code executor subprocess, establishes UDS connection,
//! executes user code in sandboxed environment, and routes tool calls back to main process.
//!
//! Experimental: this is not part of the stable CLI path.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Request to execute code in a sandboxed environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCodeRequest {
    pub language: CodeLanguage,
    pub code: String,
    pub timeout_secs: u64,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodeLanguage {
    Python,
    JavaScript,
}

/// Result of code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCodeResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub exit_code: Option<i32>,
}

/// Record of a tool call made during code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: serde_json::Value,
    pub timestamp: String,
}

/// Code executor that manages subprocess and UDS communication.
pub struct CodeExecutor {
    tool_dispatcher: crate::execute_code_uds::ToolDispatcher,
    cancel: Option<crate::cancel::CancelToken>,
}

impl CodeExecutor {
    pub fn new() -> Self {
        let tool_dispatcher: crate::execute_code_uds::ToolDispatcher =
            Arc::new(|tool: &str, _args: &serde_json::Value| {
                Err(format!(
                    "Tool '{}' requires an explicit CodeExecutor dispatcher",
                    tool
                ))
            });
        Self::with_dispatcher(tool_dispatcher)
    }

    pub fn with_dispatcher(tool_dispatcher: crate::execute_code_uds::ToolDispatcher) -> Self {
        Self {
            tool_dispatcher,
            cancel: None,
        }
    }

    /// Attach a cancel token so in-flight code execution can be cancelled
    /// (kills the sandbox subprocess tree). M2c entry chain.
    pub fn with_cancel(mut self, cancel: Option<crate::cancel::CancelToken>) -> Self {
        self.cancel = cancel;
        self
    }

    /// Execute code in a sandboxed subprocess with UDS RPC bridge.
    pub fn execute(&self, request: &ExecuteCodeRequest) -> Result<ExecuteCodeResult, String> {
        let uds_request = crate::execute_code_uds::ExecuteCodeRequest {
            language: request.language.clone().into(),
            code: request.code.clone(),
            timeout_secs: if request.timeout_secs == 0 {
                crate::execute_code_uds::DEFAULT_EXECUTE_CODE_TIMEOUT_SECS
            } else {
                request.timeout_secs
            },
            allowed_tools: request.allowed_tools.clone(),
            max_tool_calls: None,
            max_stdout_bytes: None,
        };
        let mut executor =
            crate::execute_code_uds::UdsCodeExecutor::new(Arc::clone(&self.tool_dispatcher));
        if let Some(token) = &self.cancel {
            executor = executor.with_cancel(token.clone());
        }
        executor.execute(&uds_request).map(ExecuteCodeResult::from)
    }
}

impl Default for CodeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl From<CodeLanguage> for crate::execute_code_uds::CodeLanguage {
    fn from(value: CodeLanguage) -> Self {
        match value {
            CodeLanguage::Python => crate::execute_code_uds::CodeLanguage::Python,
            CodeLanguage::JavaScript => crate::execute_code_uds::CodeLanguage::JavaScript,
        }
    }
}

impl From<crate::execute_code_uds::ToolCallRecord> for ToolCallRecord {
    fn from(value: crate::execute_code_uds::ToolCallRecord) -> Self {
        Self {
            tool_name: value.tool_name,
            arguments: value.arguments,
            result: value.result,
            timestamp: value.timestamp,
        }
    }
}

impl From<crate::execute_code_uds::ExecuteCodeResult> for ExecuteCodeResult {
    fn from(value: crate::execute_code_uds::ExecuteCodeResult) -> Self {
        Self {
            success: value.success,
            stdout: value.stdout,
            stderr: value.stderr,
            tool_calls: value
                .tool_calls
                .into_iter()
                .map(ToolCallRecord::from)
                .collect(),
            exit_code: value.exit_code,
        }
    }
}

/// UDS RPC protocol for tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_code_request_serialization() {
        let req = ExecuteCodeRequest {
            language: CodeLanguage::Python,
            code: "print('hello')".into(),
            timeout_secs: 10,
            allowed_tools: vec!["read_file".into()],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("python"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn code_language_serde() {
        let py = CodeLanguage::Python;
        let json = serde_json::to_string(&py).unwrap();
        assert_eq!(json, "\"python\"");

        let js = CodeLanguage::JavaScript;
        let json = serde_json::to_string(&js).unwrap();
        assert_eq!(json, "\"javascript\"");
    }

    #[test]
    fn execute_code_result_structure() {
        let result = ExecuteCodeResult {
            success: true,
            stdout: "output".into(),
            stderr: "".into(),
            tool_calls: vec![],
            exit_code: Some(0),
        };
        assert!(
            result.success,
            "stdout={}, stderr={}, exit_code={:?}",
            result.stdout, result.stderr, result.exit_code
        );
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn tool_call_record_captures_metadata() {
        let record = ToolCallRecord {
            tool_name: "read_file".into(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            result: serde_json::json!({"content": "hello"}),
            timestamp: "2026-04-12T15:00:00Z".into(),
        };
        assert_eq!(record.tool_name, "read_file");
    }

    #[test]
    fn rpc_request_response_roundtrip() {
        let req = RpcRequest {
            id: "req-1".into(),
            method: "call_tool".into(),
            params: serde_json::json!({"tool": "read_file"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "req-1");
        assert_eq!(parsed.method, "call_tool");
    }

    #[test]
    fn python_execution_delegates_to_real_bridge_boundary() {
        let executor = CodeExecutor::new();
        let req = ExecuteCodeRequest {
            language: CodeLanguage::Python,
            code: "print('test')".into(),
            timeout_secs: 5,
            allowed_tools: vec![],
        };
        let result = executor
            .execute(&req)
            .expect("python bridge should execute on every supported platform");
        assert!(result.success);
        assert!(result.stdout.contains("test"));
    }

    #[test]
    fn javascript_execution_delegates_to_real_bridge_boundary() {
        let executor = CodeExecutor::new();
        let req = ExecuteCodeRequest {
            language: CodeLanguage::JavaScript,
            code: "console.log('test')".into(),
            timeout_secs: 5,
            allowed_tools: vec![],
        };
        let result = executor
            .execute(&req)
            .expect("javascript bridge should execute on every supported platform");
        assert!(result.success);
        assert!(result.stdout.contains("test"));
    }

    #[test]
    fn explicit_dispatcher_is_available_for_top_level_executor() {
        let dispatcher: crate::execute_code_uds::ToolDispatcher =
            Arc::new(|tool: &str, args: &serde_json::Value| {
                Ok(serde_json::json!({
                    "tool": tool,
                    "echo": args,
                }))
            });
        let executor = CodeExecutor::with_dispatcher(dispatcher);

        let req = ExecuteCodeRequest {
            language: CodeLanguage::Python,
            code: "from zaion_tools import read_file\nprint(read_file('/tmp/example')['tool'])"
                .into(),
            timeout_secs: 5,
            allowed_tools: vec!["read_file".into()],
        };
        let result = executor.execute(&req);

        let result = result.expect("python tool bridge should execute on every supported platform");
        assert!(
            result.success,
            "stdout={}, stderr={}, exit_code={:?}",
            result.stdout, result.stderr, result.exit_code
        );
        assert!(result.stdout.contains("read_file"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "read_file");
    }

    #[test]
    fn javascript_dispatcher_is_available_for_top_level_executor() {
        let dispatcher: crate::execute_code_uds::ToolDispatcher =
            Arc::new(|tool: &str, args: &serde_json::Value| {
                Ok(serde_json::json!({
                    "tool": tool,
                    "echo": args,
                }))
            });
        let executor = CodeExecutor::with_dispatcher(dispatcher);

        let req = ExecuteCodeRequest {
            language: CodeLanguage::JavaScript,
            code: "const { readFile } = require('./zaion_tools'); readFile('/tmp/example').then(result => console.log(result.tool));".into(),
            timeout_secs: 5,
            allowed_tools: vec!["read_file".into()],
        };
        let result = executor
            .execute(&req)
            .expect("javascript tool bridge should execute on every supported platform");
        assert!(
            result.success,
            "stdout={}, stderr={}, exit_code={:?}",
            result.stdout, result.stderr, result.exit_code
        );
        assert!(result.stdout.contains("read_file"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "read_file");
    }

    #[test]
    fn javascript_dispatcher_keeps_rpc_available_for_multiple_tool_calls() {
        let dispatcher: crate::execute_code_uds::ToolDispatcher =
            Arc::new(|tool: &str, args: &serde_json::Value| {
                Ok(serde_json::json!({
                    "tool": tool,
                    "path": args.get("path").and_then(|value| value.as_str()).unwrap_or(""),
                }))
            });
        let executor = CodeExecutor::with_dispatcher(dispatcher);

        let req = ExecuteCodeRequest {
            language: CodeLanguage::JavaScript,
            code: "const { readFile } = require('./zaion_tools'); Promise.all([readFile('/tmp/a'), readFile('/tmp/b')]).then(results => console.log(results.map(r => r.path).join(',')));".into(),
            timeout_secs: 5,
            allowed_tools: vec!["read_file".into()],
        };
        let result = executor
            .execute(&req)
            .expect("javascript tool bridge should execute on every supported platform");
        assert!(
            result.success,
            "stdout={}, stderr={}, exit_code={:?}",
            result.stdout, result.stderr, result.exit_code
        );
        assert!(result.stdout.contains("/tmp/a,/tmp/b"));
        assert_eq!(result.tool_calls.len(), 2);
    }
}
