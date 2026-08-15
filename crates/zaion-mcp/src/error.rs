use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpError {
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("schema validation failed: {0}")]
    SchemaValidation(String),
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("internal error: {0}")]
    Internal(String),
}
