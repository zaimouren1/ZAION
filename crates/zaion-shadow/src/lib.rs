pub mod command_spec;
pub mod error;
pub mod executor;
pub mod lifecycle;
pub mod queue;
pub mod task;

#[cfg(test)]
mod tests;

pub use command_spec::{AllowList, CommandSpec, ProgramNotAllowed};
pub use error::ShadowError;
pub use executor::{
    ExecutorCommand, ExecutorConfig, ShadowEvent, ShadowEventRx, ShadowEventTx, ShadowExecutor,
};
pub use lifecycle::{LifecycleEvent, LifecycleState, ShadowLifecycle};
pub use queue::{QueueStats, TaskQueue};
pub use task::{ShadowTask, TaskId, TaskResult, TaskStatus};

/// Convenience: create a new broadcast channel pair for ShadowEvents.
pub fn make_event_channel() -> (ShadowEventTx, ShadowEventRx) {
    tokio::sync::broadcast::channel(64)
}
