pub mod auto_extraction;
pub mod hnsw_index;
pub mod memory_consolidator;
pub mod principal;
pub mod projection;
pub mod route;
pub mod runtime_integration;
pub mod semantic;
pub mod skill;
pub mod slimmer;
pub mod typed_memory;

#[cfg(test)]
mod tests;

pub use auto_extraction::{AutoMemoryExtractor, ExtractionResult, MemoryCandidate};
pub use memory_consolidator::{
    ConsolidatorConfig, ConsolidatorError, MemoryConsolidator, RollupCommitment,
};
pub use principal::{PrincipalMemoryEntry, PrincipalMemoryStore};
pub use projection::*;
pub use route::{AccountRouter, RouteRule};
pub use runtime_integration::{
    BuiltinMemoryProvider, MemoryManager, MemoryProvider, MemoryRuntimeConfig,
};
pub use semantic::*;
pub use skill::*;
pub use slimmer::*;
pub use typed_memory::{MemoryStats, MemoryType, TypedMemoryEntry, TypedMemoryStore};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}
