pub mod binding;
pub mod blob;
pub mod ledger;
pub mod schema;
pub mod session_reset;
pub mod session_store;

#[cfg(test)]
mod tests;

pub use binding::{
    validated_database_path, verify_existing_idempotent_event_in_connection,
    IdempotentEventBinding, VerifiedEventCommit,
};
pub use blob::*;
pub use ledger::ChainVerifyResult;
pub use ledger::*;
pub use session_reset::{
    resolve_reset_policy, should_reset_for_idle, should_reset_for_new_day,
    should_reset_for_trigger, ResetPolicyConfig, SessionResetPolicy,
};
pub use session_store::{SessionEntry, SessionKeyStrategy, SessionStore};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("event not found: {0}")]
    NotFound(String),
    #[error("corrupt payload: {0}")]
    CorruptPayload(String),
    #[error("invalid ledger append idempotency key")]
    InvalidIdempotencyKey,
    #[error("event id {event_id} is already bound to different content")]
    EventIdConflict { event_id: String },
    #[error("event binding principal does not match the supplied key: expected {expected}, derived {derived}")]
    EventBindingPrincipalMismatch { expected: String, derived: String },
    #[error("event binding mismatch in {field}")]
    EventBindingMismatch { field: &'static str },
    #[error("event binding signature is missing or invalid")]
    EventBindingSignatureInvalid,
    #[error("event binding requires a canonical-envelope signature")]
    EventBindingNonCanonicalSignature,
    #[error("event binding requires a file-backed ledger path: {0}")]
    EventBindingUnsupportedLedgerPath(String),
    #[error("ledger database instance identity is invalid: {0}")]
    InvalidDatabaseInstanceIdentity(String),
    #[error("ledger database instance identity changed from {expected} to {actual}")]
    DatabaseInstanceIdentityDrift { expected: String, actual: String },
    #[error("ledger chain metadata is corrupt and requires explicit repair: {0}")]
    CorruptChain(String),
}
