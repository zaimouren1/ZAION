use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShadowError {
    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("task already running: {0}")]
    TaskAlreadyRunning(String),

    #[error("executor not running")]
    ExecutorNotRunning,

    #[error("queue full: max {max} tasks")]
    QueueFull { max: usize },

    #[error("invalid task state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("invalid identity: {0}")]
    InvalidIdentity(String),

    #[error("ACI error: {0}")]
    Aci(#[from] zaion_aci::AciError),

    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),

    #[error("crypto error: {0}")]
    Crypto(#[from] zaion_crypto::CryptoError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("task execution failed: {0}")]
    TaskFailed(String),
}
