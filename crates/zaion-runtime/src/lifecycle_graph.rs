use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleEventKind {
    SystemAwake,
    SystemIdle,
    SystemQuiescent,
    SystemResume,
    SystemResourceRebuilt,
}

impl LifecycleEventKind {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SystemAwake => "system.awake",
            Self::SystemIdle => "system.idle",
            Self::SystemQuiescent => "system.quiescent",
            Self::SystemResume => "system.resume",
            Self::SystemResourceRebuilt => "system.resource_rebuilt",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_event_types_cover_cold_start_and_quiescent_edges() {
        assert_eq!(LifecycleEventKind::SystemAwake.event_type(), "system.awake");
        assert_eq!(LifecycleEventKind::SystemIdle.event_type(), "system.idle");
        assert_eq!(
            LifecycleEventKind::SystemQuiescent.event_type(),
            "system.quiescent"
        );
        assert_eq!(
            LifecycleEventKind::SystemResume.event_type(),
            "system.resume"
        );
        assert_eq!(
            LifecycleEventKind::SystemResourceRebuilt.event_type(),
            "system.resource_rebuilt"
        );
    }
}
