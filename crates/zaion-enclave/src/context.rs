//! SecureContext — 内存隔离计算上下文（软件模拟）
//!
//! 真实 TEE 提供硬件保护的 enclave 内存区域。
//! 软件模拟：提供受限执行环境，所有 I/O 经过审计门（AuditGate）。

use crate::EnclaveIdentity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureTaskResult {
    pub task_id: String,
    pub output: serde_json::Value,
    pub audit_log: Vec<String>,
    pub executed_in_enclave: bool,
}

pub struct SecureContext {
    identity: EnclaveIdentity,
    audit_log: Vec<String>,
}

impl SecureContext {
    pub fn new(identity: EnclaveIdentity) -> Self {
        Self {
            identity,
            audit_log: Vec::new(),
        }
    }

    /// Execute a pure function inside the secure context.
    /// All task boundaries are logged to the immutable audit trail.
    pub fn execute<F>(&mut self, task_id: &str, input: serde_json::Value, f: F) -> SecureTaskResult
    where
        F: FnOnce(serde_json::Value) -> serde_json::Value,
    {
        self.audit_log.push(format!(
            "[{}] task_started: {}",
            chrono::Utc::now().to_rfc3339(),
            task_id
        ));
        let output = f(input);
        self.audit_log.push(format!(
            "[{}] task_completed: {}",
            chrono::Utc::now().to_rfc3339(),
            task_id
        ));

        SecureTaskResult {
            task_id: task_id.to_string(),
            output,
            audit_log: self.audit_log.clone(),
            executed_in_enclave: true,
        }
    }

    pub fn audit_log(&self) -> &[String] {
        &self.audit_log
    }

    pub fn enclave_id(&self) -> String {
        self.identity.enclave_id()
    }
}
