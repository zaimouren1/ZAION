pub mod honcho;
pub mod session;

pub use honcho::{ApiKeySource, HonchoClient, HonchoConfig};
pub use session::{FederatedSession, SessionNamingStrategy, SessionStrategy};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FederationError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("federation error: {0}")]
    Other(String),
}
