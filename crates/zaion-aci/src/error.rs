use thiserror::Error;

#[derive(Error, Debug)]
pub enum AciError {
    #[error("syntax error in {language}: {message}")]
    SyntaxError { language: String, message: String },

    #[error("reality diverged: file '{path}' was modified externally (expected={expected}, actual={actual})")]
    RealityDiverged {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("toxic plugin blocked: hash={hash}, reason={reason}")]
    ToxicBlocked { hash: String, reason: String },

    #[error("ast patch failed: {0}")]
    PatchFailed(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ledger error: {0}")]
    Ledger(String),

    #[error("internal error: {0}")]
    Internal(String),
}
