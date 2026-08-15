use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LifecycleState {
    Idle,
    Starting,
    Running,
    Pausing,
    Paused,
    Resuming,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LifecycleEvent {
    Start,
    Pause,
    Resume,
    Stop,
    Fail {
        reason: String,
    },
    TaskQueued {
        task_id: crate::TaskId,
    },
    TaskStarted {
        task_id: crate::TaskId,
    },
    TaskCompleted {
        task_id: crate::TaskId,
        success: bool,
    },
    TaskCancelled {
        task_id: crate::TaskId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowLifecycle {
    pub state: LifecycleState,
    pub started_at: Option<DateTime<Utc>>,
    pub last_event: Option<LifecycleEvent>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub tasks_processed: u64,
    pub tasks_failed: u64,
    pub uptime_seconds: u64,
}

impl Default for ShadowLifecycle {
    fn default() -> Self {
        Self {
            state: LifecycleState::Idle,
            started_at: None,
            last_event: None,
            last_event_at: None,
            tasks_processed: 0,
            tasks_failed: 0,
            uptime_seconds: 0,
        }
    }
}

impl ShadowLifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn transition(&mut self, event: LifecycleEvent) -> Result<(), crate::ShadowError> {
        let new_state = match (&self.state, &event) {
            (LifecycleState::Idle, LifecycleEvent::Start) => LifecycleState::Starting,
            (LifecycleState::Starting, LifecycleEvent::TaskQueued { .. }) => {
                LifecycleState::Running
            }
            (LifecycleState::Running, LifecycleEvent::Pause) => LifecycleState::Pausing,
            (LifecycleState::Pausing, _) => LifecycleState::Paused,
            (LifecycleState::Paused, LifecycleEvent::Resume) => LifecycleState::Resuming,
            (LifecycleState::Resuming, _) => LifecycleState::Running,
            (LifecycleState::Running, LifecycleEvent::Stop) => LifecycleState::Stopping,
            (LifecycleState::Stopping, _) => LifecycleState::Stopped,
            (_, LifecycleEvent::Fail { .. }) => LifecycleState::Failed,
            (LifecycleState::Running, LifecycleEvent::TaskStarted { .. }) => {
                LifecycleState::Running
            }
            (LifecycleState::Running, LifecycleEvent::TaskCompleted { success, .. }) => {
                if *success {
                    self.tasks_processed += 1;
                } else {
                    self.tasks_failed += 1;
                }
                LifecycleState::Running
            }
            (LifecycleState::Running, LifecycleEvent::TaskCancelled { .. }) => {
                LifecycleState::Running
            }
            _ => {
                return Err(crate::ShadowError::InvalidStateTransition {
                    from: format!("{:?}", self.state),
                    to: format!("{:?}", event),
                });
            }
        };

        self.state = new_state;
        self.last_event = Some(event);
        self.last_event_at = Some(Utc::now());

        if matches!(self.state, LifecycleState::Starting) && self.started_at.is_none() {
            self.started_at = Some(Utc::now());
        }

        Ok(())
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            LifecycleState::Starting
                | LifecycleState::Running
                | LifecycleState::Pausing
                | LifecycleState::Resuming
        )
    }

    pub fn can_accept_tasks(&self) -> bool {
        matches!(self.state, LifecycleState::Running)
    }

    pub fn update_uptime(&mut self) {
        if let Some(started) = self.started_at {
            self.uptime_seconds = (Utc::now() - started).num_seconds() as u64;
        }
    }
}
