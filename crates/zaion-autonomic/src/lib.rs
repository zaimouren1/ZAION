//! System II: Zero-Token Autonomic System
//!
//! Implements reflexive, zero-token responses to environmental stimuli
//! without consuming LLM tokens. Uses WASM probes for extensibility.
mod action_potential;
mod probe;
mod reflex;
pub mod runtime;

pub use action_potential::{ActionPotential, StimulusAccumulator, Threshold};
pub use probe::{ProbeEngine, ProbeResult, WasmProbe};
pub use reflex::{AutonomicReflex, ReflexAction, ReflexRegistry, ReflexTrigger};
pub use runtime::{AutonomicEvent, AutonomicRuntime};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AutonomicError {
    #[error("reflex not found: {0}")]
    ReflexNotFound(String),
    #[error("probe execution failed: {0}")]
    ProbeExecutionFailed(String),
    #[error("wasm error: {0}")]
    WasmError(String),
    #[error("threshold not met: current={0}, required={1}")]
    ThresholdNotMet(f64, f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomic_system_loads() {
        let registry = ReflexRegistry::new();
        assert_eq!(registry.count(), 0);
    }
}
