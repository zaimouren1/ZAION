pub mod audit;
pub mod auth;
pub mod store;

#[cfg(test)]
mod tests;

pub use audit::*;
pub use auth::*;
pub use store::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SecretsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
}
