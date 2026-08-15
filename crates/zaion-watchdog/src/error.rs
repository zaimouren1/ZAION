use thiserror::Error;

#[derive(Error, Debug)]
pub enum WatchdogError {
    #[error("process not found: pid={0}")]
    ProcessNotFound(u32),
    #[error("crash detected: {0}")]
    CrashDetected(String),
    #[error("heal failed: {0}")]
    HealFailed(String),
    #[error("resurrect failed: {0}")]
    ResurrectFailed(String),
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Other(String),
}
