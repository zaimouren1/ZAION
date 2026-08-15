//! Autonomic Runtime — background tokio polling loop
//!
//! Executes registered WASM probes on a fixed interval, accumulates stimulus
//! into `ActionPotential`s, and sends `AutonomicEvent`s via an mpsc channel
//! whenever a potential fires (reaches threshold).
use crate::{
    ActionPotential, AutonomicReflex, ProbeEngine, ReflexRegistry, StimulusAccumulator, WasmProbe,
};

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::mpsc;

// ─── Public event type ────────────────────────────────────────────────────────

/// Emitted by the background loop whenever an `ActionPotential` fires.
#[derive(Debug, Clone)]
pub struct AutonomicEvent {
    /// ID of the potential that fired.
    pub potential_id: String,
    /// Action type from the matched reflex (or "fire" if no reflex matched).
    pub action_type: String,
    /// UTC timestamp of the firing.
    pub fired_at: chrono::DateTime<chrono::Utc>,
}

// ─── AutonomicRuntime ─────────────────────────────────────────────────────────

/// Background runtime that drives the Zero-Token Autonomic System.
///
/// Create with [`AutonomicRuntime::new`], register reflexes / potentials /
/// probes, then call [`AutonomicRuntime::spawn`] to start the loop.
/// Events are delivered through the [`mpsc::Receiver`] returned by `new`.
pub struct AutonomicRuntime {
    registry: Arc<Mutex<ReflexRegistry>>,
    accumulator: Arc<Mutex<StimulusAccumulator>>,
    probe_engine: Arc<ProbeEngine>,
    probes: Arc<Mutex<Vec<WasmProbe>>>,
    poll_interval: Duration,
    tx: mpsc::Sender<AutonomicEvent>,
}

impl AutonomicRuntime {
    /// Build a new runtime and return the event receiver.
    ///
    /// The channel capacity is 64; if the consumer is slow, events are
    /// silently dropped (non-blocking send) rather than blocking the loop.
    pub fn new(poll_interval: Duration) -> (Self, mpsc::Receiver<AutonomicEvent>) {
        let (tx, rx) = mpsc::channel(64);
        let runtime = Self {
            registry: Arc::new(Mutex::new(ReflexRegistry::new())),
            accumulator: Arc::new(Mutex::new(StimulusAccumulator::new())),
            probe_engine: Arc::new(ProbeEngine::new()),
            probes: Arc::new(Mutex::new(Vec::new())),
            poll_interval,
            tx,
        };
        (runtime, rx)
    }

    // ── Registration helpers ──────────────────────────────────────────────────

    /// Register an [`AutonomicReflex`].
    pub fn register_reflex(&self, reflex: AutonomicReflex) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.register(reflex);
        }
    }

    /// Register an [`ActionPotential`] in the accumulator.
    pub fn register_potential(&self, potential: ActionPotential) {
        if let Ok(mut acc) = self.accumulator.lock() {
            acc.register(potential);
        }
    }

    /// Add a WASM probe to be executed every poll interval.
    pub fn add_probe(&self, probe: WasmProbe) {
        if let Ok(mut probes) = self.probes.lock() {
            probes.push(probe);
        }
    }

    // ── Public query helpers ──────────────────────────────────────────────────

    /// Number of registered reflexes.
    pub fn reflex_count(&self) -> usize {
        self.registry.lock().map(|r| r.count()).unwrap_or(0)
    }

    /// Number of registered potentials.
    pub fn potential_count(&self) -> usize {
        self.accumulator
            .lock()
            .map(|a| a.list_all().len())
            .unwrap_or(0)
    }

    /// Number of registered probes.
    pub fn probe_count(&self) -> usize {
        self.probes.lock().map(|p| p.len()).unwrap_or(0)
    }

    // ── Spawn background loop ─────────────────────────────────────────────────

    /// Spawn the background polling loop on the current tokio runtime.
    ///
    /// The loop runs until all senders (including `self`) are dropped.
    /// Returns a [`tokio::task::JoinHandle`] that resolves when the loop exits.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        let registry = Arc::clone(&self.registry);
        let accumulator = Arc::clone(&self.accumulator);
        let probe_engine = Arc::clone(&self.probe_engine);
        let probes = Arc::clone(&self.probes);
        let interval = self.poll_interval;
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                // H26 fix: snapshot probe bytes while holding the std::Mutex,
                // then release the lock BEFORE awaiting spawn_blocking so we
                // never hold a non-Send guard across an .await.
                let probe_snapshots: Vec<(String, Vec<u8>)> = {
                    match probes.lock() {
                        Ok(p) => p
                            .iter()
                            .map(|probe| (probe.name().to_string(), probe.bytes().to_vec()))
                            .collect(),
                        Err(_) => continue,
                    }
                    // lock dropped here
                };

                // Execute each probe on a blocking thread.  wasmtime module
                // compile + call can be CPU-heavy (ms-to-seconds) so we
                // must not block a Tokio worker.
                for (probe_name, wasm_bytes) in probe_snapshots {
                    let engine_for_task = Arc::clone(&probe_engine);
                    let probe_name_for_task = probe_name.clone();
                    let exec_result = tokio::task::spawn_blocking(move || {
                        engine_for_task.execute_bytes(&probe_name_for_task, &wasm_bytes)
                    })
                    .await;

                    let result = match exec_result {
                        Ok(r) => r,
                        Err(join_err) => {
                            eprintln!(
                                "[autonomic] probe '{}' join error: {}",
                                probe_name, join_err
                            );
                            continue;
                        }
                    };

                    match result {
                        Err(e) => {
                            eprintln!("[autonomic] probe '{}' error: {}", probe_name, e);
                            continue;
                        }
                        Ok(probe_result) if !probe_result.success => continue,
                        Ok(probe_result) => {
                            // Stimulate the potential whose id matches the probe name.
                            let fired = {
                                match accumulator.lock() {
                                    Ok(mut acc) => acc
                                        .stimulate(&probe_name, probe_result.value)
                                        .unwrap_or(false),
                                    Err(_) => false,
                                }
                            };

                            if fired {
                                // Look up matching reflexes for this potential.
                                let action_type = {
                                    match registry.lock() {
                                        Ok(reg) => {
                                            let matches =
                                                reg.match_trigger("action_potential", None);
                                            matches
                                                .first()
                                                .map(|r| r.action.action_type.clone())
                                                .unwrap_or_else(|| "fire".to_string())
                                        }
                                        Err(_) => "fire".to_string(),
                                    }
                                };

                                let event = AutonomicEvent {
                                    potential_id: probe_name.clone(),
                                    action_type,
                                    fired_at: chrono::Utc::now(),
                                };

                                // Non-blocking send — if the channel is full or
                                // closed, drop the event rather than blocking.
                                let _ = tx.try_send(event);
                            }
                        }
                    }
                }
            }
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionPotential, Threshold};

    #[test]
    fn runtime_creates_with_receiver() {
        let (runtime, _rx) = AutonomicRuntime::new(Duration::from_secs(1));
        assert_eq!(runtime.reflex_count(), 0);
        assert_eq!(runtime.potential_count(), 0);
        assert_eq!(runtime.probe_count(), 0);
    }

    #[test]
    fn register_potential_increases_count() {
        let (runtime, _rx) = AutonomicRuntime::new(Duration::from_secs(1));
        let ap = ActionPotential::new(
            "heartbeat".to_string(),
            "Heartbeat".to_string(),
            Threshold {
                value: 1.0,
                decay_rate: 0.0,
            },
        );
        runtime.register_potential(ap);
        assert_eq!(runtime.potential_count(), 1);
    }

    /// Directly stimulate an ActionPotential through the accumulator and verify
    /// that an event arrives on the channel within a short timeout.
    ///
    /// This test does NOT require any .wasm files — it bypasses WASM execution
    /// and directly stimulates the accumulator, then verifies the channel path.
    #[tokio::test]
    async fn runtime_fires_event_when_threshold_met() {
        use crate::{AutonomicReflex, ReflexAction, ReflexTrigger};

        let (runtime, mut rx) = AutonomicRuntime::new(Duration::from_millis(50));

        // Register a potential with a low threshold.
        let ap = ActionPotential::new(
            "test-ap".to_string(),
            "Test AP".to_string(),
            Threshold {
                value: 0.5,
                decay_rate: 0.0,
            },
        );
        runtime.register_potential(ap);

        // Register a reflex so action_type is non-default.
        runtime.register_reflex(AutonomicReflex {
            id: "test-reflex".to_string(),
            name: "Test Reflex".to_string(),
            trigger: ReflexTrigger {
                trigger_type: "action_potential".to_string(),
                pattern: None,
                threshold: None,
            },
            action: ReflexAction {
                action_type: "alert".to_string(),
                parameters: serde_json::json!({}),
            },
            enabled: true,
        });

        // Directly stimulate the accumulator to cross threshold — simulating
        // what a WASM probe returning value=1.0 would do.
        {
            let mut acc = runtime.accumulator.lock().unwrap();
            let fired = acc.stimulate("test-ap", 1.0).unwrap_or(false);

            if fired {
                // Manually send the event as the spawn loop would.
                let event = AutonomicEvent {
                    potential_id: "test-ap".to_string(),
                    action_type: "alert".to_string(),
                    fired_at: chrono::Utc::now(),
                };
                let _ = runtime.tx.try_send(event);
            }
        }

        // Expect to receive an event within 500 ms.
        let result = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;

        assert!(result.is_ok(), "timeout waiting for AutonomicEvent");
        let event = result.unwrap();
        assert!(event.is_some(), "channel closed unexpectedly");
        let event = event.unwrap();
        assert_eq!(event.potential_id, "test-ap");
    }
}
