//! System V: Entropic Curiosity
//!
//! Idle timer and spontaneous ideation loop for autonomous exploration
mod ideation;
mod idle;
pub mod llm_ideation;

pub use ideation::{IdeationCategory, IdeationConfig, IdeationLoop, IdeationPrompt};
pub use idle::{IdleState, IdleTimer};
pub use llm_ideation::{
    build_system_prompt, gather_context, generate_llm_prompt, CodebaseContext, LlmIdeationResult,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CuriosityError {
    #[error("ideation failed: {0}")]
    IdeationFailed(String),
    #[error("idle timer error: {0}")]
    IdleTimerError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curiosity_system_loads() {
        let timer = IdleTimer::new(std::time::Duration::from_secs(60));
        assert!(!timer.is_idle());
    }
}
