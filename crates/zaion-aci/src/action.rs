//! AciAction — Agent 可以发起的所有操作（ACI 2.0 动作组）
//!
//! 设计原则：Agent 不直接操作 bash 或裸文件，而是通过
//! 高维度的 ACI 动作，由 Rust 核心负责校验、熔断、落盘。
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── AciAction（Agent 发起的操作） ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action_type", rename_all = "snake_case")]
pub enum AciAction {
    /// 读取文件内容（经 RealitySync 校验）
    ReadFile { path: PathBuf },

    /// 覆写文件（经 SyntaxGate + RealitySync + Toxic 三重校验）
    WriteFile {
        path: PathBuf,
        content: String,
        /// 写入后自动更新 RealityAnchor
        update_anchor: bool,
    },

    /// AST 节点替换：在文件中定位 old_text，替换为 new_text，
    /// 替换后经 SyntaxGate 语法校验，少一个括号直接打回重写
    ReplaceAstNode {
        path: PathBuf,
        /// 要被替换的精确文本片段（唯一匹配）
        old_text: String,
        /// 替换后的新文本
        new_text: String,
        /// 语言类型（用于选择语法校验器）
        language: String,
    },

    /// 在文件指定行后插入代码块（经 SyntaxGate 校验）
    InsertAfterLine {
        path: PathBuf,
        line_number: usize,
        content: String,
        language: String,
    },

    /// 删除文件中精确匹配的文本块（经 SyntaxGate 校验）
    DeleteTextBlock {
        path: PathBuf,
        target_text: String,
        language: String,
    },

    /// 语法检查（只读，不修改文件）
    SyntaxCheck { path: PathBuf, language: String },

    /// 校验文件与 RealityAnchor 是否一致
    RealityCheck { path: PathBuf },

    /// 将文件标记为有毒（拦截后续操作）
    MarkToxic { path: PathBuf, reason: String },

    /// 查询文件是否被标记为有毒
    IsToxic { path: PathBuf },
}

// ── AciResult ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AciResult {
    pub status: AciStatus,
    /// 操作成功时的返回数据
    pub data: Option<serde_json::Value>,
    /// 错误时的说明
    pub error: Option<String>,
    /// 操作 ID（与 Ledger 事件关联）
    pub op_id: String,
    /// 操作耗时（微秒）
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AciStatus {
    Ok,
    SyntaxError,
    RealityDiverged,
    ToxicBlocked,
    NotFound,
    Error,
}

impl AciResult {
    pub fn ok(data: serde_json::Value, op_id: &str, elapsed_us: u64) -> Self {
        AciResult {
            status: AciStatus::Ok,
            data: Some(data),
            error: None,
            op_id: op_id.to_string(),
            elapsed_us,
        }
    }

    pub fn err(status: AciStatus, msg: impl Into<String>, op_id: &str, elapsed_us: u64) -> Self {
        AciResult {
            status,
            data: None,
            error: Some(msg.into()),
            op_id: op_id.to_string(),
            elapsed_us,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == AciStatus::Ok
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aci_result_ok_is_ok() {
        let r = AciResult::ok(serde_json::json!({"written": true}), "op-1", 42);
        assert!(r.is_ok());
        assert!(r.error.is_none());
    }

    #[test]
    fn aci_result_err_is_not_ok() {
        let r = AciResult::err(AciStatus::SyntaxError, "missing bracket", "op-2", 10);
        assert!(!r.is_ok());
        assert!(r.error.is_some());
    }

    #[test]
    fn aci_action_serializes() {
        let action = AciAction::ReplaceAstNode {
            path: std::path::PathBuf::from("/src/main.rs"),
            old_text: "fn foo() {}".to_string(),
            new_text: "fn foo() { println!(\"hello\"); }".to_string(),
            language: "rust".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("replace_ast_node"));
        assert!(json.contains("main.rs"));
    }
}
