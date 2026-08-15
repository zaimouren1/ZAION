pub mod diff;
pub mod rollback;
/// zaion-gitledger — Git-Native Spacetime Ledger (Campaign VII)
///
/// Architecture:
///   C7.1 Shadow-branch engine — every Agent code change auto-commits to
///         `zaion-shadow/<pid>` with a message embedding the ledger event_id.
///   C7.2 Time-travel rollback — `zaion undo --to <event_id>` resolves the
///         shadow commit that corresponds to that ledger event and hard-resets.
///   C7.3 Self-verifying rollback — run tests after each shadow commit;
///         auto-revert on failure and log the fail record to the ledger.
///   C7.4 CLI verbs — `zaion git status/diff/log/merge`
pub mod shadow;

pub use diff::*;
pub use rollback::*;
pub use shadow::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitLedgerError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("ledger error: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("internal: {0}")]
    Internal(String),
}
