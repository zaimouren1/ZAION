pub mod controller;
pub mod daemon;
pub mod pairing;
pub mod process;

#[cfg(test)]
mod tests;

pub use controller::*;
pub use daemon::{
    detect_crash, run_with_watchdog, DaemonConfig, DaemonError, DaemonHandle, HeartbeatWriter,
    WatchdogEvent, WatchdogOutcome,
};
pub use pairing::*;
pub use process::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("memory error: {0}")]
    Memory(#[from] zaion_memory::MemoryError),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("store error: {0}")]
    Store(String),
    #[error("process not found: {0}")]
    NotFound(String),
    #[error("process already exists: {0}")]
    AlreadyExists(String),
}
