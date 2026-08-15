//! Event bus for component communication
//!
//! Provides a broadcast channel for system events that all components can subscribe to.

use crate::components::SystemEvent;
use parking_lot::RwLock;
use std::sync::Arc;

/// Event bus for broadcasting system events to all components
pub struct EventBus {
    subscribers: Arc<RwLock<Vec<Subscriber>>>,
}

/// Subscriber handle for receiving events
pub struct EventSubscriber {
    id: usize,
    rx: tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
}

struct Subscriber {
    tx: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Emit an event to all subscribers
    pub fn emit(&self, event: SystemEvent) {
        let subscribers = self.subscribers.read();
        for subscriber in subscribers.iter() {
            let _ = subscriber.tx.send(event.clone());
        }
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> EventSubscriber {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut subscribers = self.subscribers.write();
        let id = subscribers.len();
        subscribers.push(Subscriber { tx });
        EventSubscriber { id, rx }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSubscriber {
    /// Try to receive an event (non-blocking)
    pub fn try_recv(&mut self) -> Option<SystemEvent> {
        self.rx.try_recv().ok()
    }

    /// Get subscriber ID
    pub fn id(&self) -> usize {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{SystemEvent, TimerEvent};

    #[test]
    fn test_event_bus_broadcast() {
        let bus = EventBus::new();
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();

        bus.emit(SystemEvent::Timer(TimerEvent::PeriodicRefresh));

        assert!(sub1.try_recv().is_some());
        assert!(sub2.try_recv().is_some());
    }

    #[test]
    fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let mut subs: Vec<_> = (0..5).map(|_| bus.subscribe()).collect();

        bus.emit(SystemEvent::Timer(TimerEvent::AutoScroll));

        for sub in &mut subs {
            assert!(sub.try_recv().is_some());
        }
    }
}
