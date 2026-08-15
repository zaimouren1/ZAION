pub mod diff;
pub mod export;
pub mod import;
pub mod protocol;
pub mod relay;

pub use diff::SyncDiff;
pub use export::{SyncBundle, SyncProofArtifact};
pub use import::ImportResult;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ledger error: {0}")]
    Ledger(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("no events to export")]
    NoEvents,
    #[error("invalid sync bundle: {0}")]
    InvalidBundle(String),
    #[error("relay error: {0}")]
    Relay(String),
}
