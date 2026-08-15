pub mod acp;
pub mod agent_card;
pub mod federation;
pub mod federation_message;
pub mod protocol;
pub mod stdio_service;

#[cfg(test)]
mod tests;

pub use acp::*;
pub use agent_card::*;
pub use federation::*;
pub use protocol::*;
pub use stdio_service::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum A2AError {
    #[error("core error: {0}")]
    Core(#[from] zaion_core::CoreError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("authorization failed: {0}")]
    AuthFailed(String),
}
