//! Error types for telemetry operations

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TelemetryError {
    #[error("span not found: {0}")]
    SpanNotFound(String),

    #[error("trace not found: {0}")]
    TraceNotFound(String),

    #[error("invalid span: {0}")]
    InvalidSpan(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type TelemetryResult<T> = Result<T, TelemetryError>;
