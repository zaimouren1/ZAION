//! zaion-aci — ACI 2.0 Agent Computer Interface
//!
//! 架构层次：
//!
//!   AciAction        — Agent 发起的操作请求（枚举）
//!   AciResult        — 操作执行结果
//!   SyntaxGate       — 语法校验熔断器（Rust/TOML/JSON/TS/Python/Shell）
//!   AstPatcher       — AST 级别 replace_node 实现（tree-sitter lite 文本替换 + 语法验证）
//!   FileOpsGate      — Reality-Sync 校验 + Toxic 拦截 + 安全写文件
//!   AciDispatcher    — 统一入口，路由所有 AciAction
//!   AciLedger        — 每次 ACI 操作签名写入 Event Ledger
pub mod action;
pub mod ast_patcher;
pub mod dispatcher;
pub mod error;
pub mod file_ops;
pub mod ledger;
pub mod merge;
pub mod syntax_gate;

pub use action::{AciAction, AciResult, AciStatus};
pub use ast_patcher::AstPatcher;
pub use dispatcher::AciDispatcher;
pub use error::AciError;
pub use file_ops::FileOpsGate;
pub use merge::{AstChange, AstChunk, AstDiff, AstMergeResolver, ConflictBlock, MergeResult};
pub use syntax_gate::{SyntaxCheckResult, SyntaxGate, SyntaxLanguage};
