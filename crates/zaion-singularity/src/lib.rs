//! System Integration: Singularity Runtime
//!
//! Orchestrates all 5 v5.0 systems into a unified runtime
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use zaion_autonomic::{AutonomicReflex, ReflexRegistry, StimulusAccumulator};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_curiosity::{IdeationCategory, IdeationConfig, IdeationLoop, IdleTimer};
use zaion_ego::{DynamicLexicalBaffle, EgoCompiler, EgoManifest, SoulHash};
use zaion_ledger::EventLedger;
use zaion_metabolic::{BudgetTracker, HungerState, MetabolicAction, MetabolicPolicy, PainReceptor};
use zaion_proprioception::{global_lockdown, FingerprintCollector, ShockDetector, ShockSeverity};
use zaion_types::NamespaceKey;

#[derive(Error, Debug)]
pub enum SingularityError {
    #[error("ego error: {0}")]
    Ego(String),
    #[error("autonomic error: {0}")]
    Autonomic(String),
    #[error("proprioception error: {0}")]
    Proprioception(String),
    #[error("metabolic error: {0}")]
    Metabolic(String),
    #[error("curiosity error: {0}")]
    Curiosity(String),
    #[error("ledger error: {0}")]
    Ledger(String),
}

pub struct SingularityRuntime {
    // System I: Ego
    ego_manifest: EgoManifest,
    baffle: DynamicLexicalBaffle,
    soul_hash: SoulHash,

    // System II: Autonomic
    reflex_registry: ReflexRegistry,
    stimulus_accumulator: StimulusAccumulator,

    // System III: Proprioception
    shock_detector: ShockDetector,

    // System IV: Metabolic
    budget_tracker: BudgetTracker,
    pain_receptor: PainReceptor,
    hunger_state: HungerState,

    // System V: Curiosity
    idle_timer: IdleTimer,
    ideation_loop: IdeationLoop,

    // Core infrastructure
    ledger: Arc<EventLedger>,
    keypair: Arc<ZaionKeypair>,
    namespace_key: NamespaceKey,
}

impl SingularityRuntime {
    pub fn new(
        zaion_dir: &std::path::Path,
        ledger: Arc<EventLedger>,
        keypair: Arc<ZaionKeypair>,
        namespace_key: NamespaceKey,
    ) -> Result<Self, SingularityError> {
        // System I: Load or create ego manifest
        let ego_store = zaion_ego::EgoStore::new(zaion_dir);
        let ego_manifest = if ego_store.exists() {
            ego_store
                .load()
                .map_err(|e| SingularityError::Ego(e.to_string()))?
        } else {
            EgoManifest::default()
        };

        let baffle = DynamicLexicalBaffle::new(&ego_manifest)
            .map_err(|e| SingularityError::Ego(e.to_string()))?;
        let soul_hash = SoulHash::compute(&ego_manifest, &keypair)
            .map_err(|e| SingularityError::Ego(e.to_string()))?;

        // System II: Autonomic
        let reflex_registry = ReflexRegistry::new();
        let stimulus_accumulator = StimulusAccumulator::new();

        // System III: Proprioception
        let current_fingerprint = FingerprintCollector::new()
            .collect()
            .map_err(|e| SingularityError::Proprioception(e.to_string()))?;
        let shock_detector = ShockDetector::with_baseline(current_fingerprint);

        // System IV: Metabolic
        let budget_tracker = BudgetTracker::new(100000);
        let pain_receptor = PainReceptor::new();
        let hunger_state = HungerState::new();

        // System V: Curiosity
        let idle_timer = IdleTimer::with_thresholds(
            Duration::from_secs(300),  // 5 min idle threshold
            Duration::from_secs(1800), // 30 min deep idle threshold
        );
        let ideation_config = IdeationConfig {
            enabled: true,
            min_idle_seconds: 7200, // 2 hours
            categories: IdeationCategory::all(),
        };
        let ideation_loop = IdeationLoop::new(ideation_config);

        Ok(Self {
            ego_manifest,
            baffle,
            soul_hash,
            reflex_registry,
            stimulus_accumulator,
            shock_detector,
            budget_tracker,
            pain_receptor,
            hunger_state,
            idle_timer,
            ideation_loop,
            ledger,
            keypair,
            namespace_key,
        })
    }

    /// System I: Get compiled system prompt
    pub fn system_prompt(&self) -> String {
        EgoCompiler::compile(&self.ego_manifest)
    }

    /// System I: Check if token is allowed by baffle
    pub fn is_token_allowed(&self, token: &str) -> bool {
        self.baffle.is_allowed(token)
    }

    /// System I: Filter response text through baffle
    pub fn filter_response(&self, text: &str) -> String {
        self.baffle.filter_response(text)
    }

    /// System I: Get current soul hash
    pub fn soul_hash(&self) -> &SoulHash {
        &self.soul_hash
    }

    /// System II: Register an autonomic reflex.
    ///
    /// Reflexes registered here are matched by [`Self::check_reflexes`]; each
    /// fire is audited to the signed ledger as an `autonomic.reflex_triggered`
    /// event. The runtime starts with no reflexes, so this is the entry point
    /// for wiring System II behaviour.
    pub fn register_reflex(&mut self, reflex: AutonomicReflex) {
        self.reflex_registry.register(reflex);
    }

    /// Number of reflexes currently registered in System II.
    pub fn reflex_count(&self) -> usize {
        self.reflex_registry.count()
    }

    /// System II: Check reflexes and execute if triggered
    pub async fn check_reflexes(
        &mut self,
        trigger_type: &str,
        value: f64,
    ) -> Result<Vec<String>, SingularityError> {
        let mut actions = Vec::new();

        let reflexes: Vec<&AutonomicReflex> = self
            .reflex_registry
            .match_trigger(trigger_type, Some(value));
        for reflex in reflexes {
            // Log reflex trigger
            self.ledger
                .append_signed_event(
                    self.keypair.as_ref(),
                    &self.namespace_key,
                    "autonomic.reflex_triggered",
                    serde_json::json!({
                        "trigger_type": trigger_type,
                        "value": value,
                        "action": reflex.action.action_type,
                    }),
                    None,
                )
                .map_err(|e| SingularityError::Ledger(e.to_string()))?;

            actions.push(reflex.action.action_type.clone());
        }

        Ok(actions)
    }

    /// System II: Stimulate action potential (must be registered first)
    pub fn stimulate(&mut self, potential_id: &str, amount: f64) -> Result<bool, SingularityError> {
        self.stimulus_accumulator
            .stimulate(potential_id, amount)
            .map_err(|e| SingularityError::Autonomic(e.to_string()))
    }

    /// System III: Check for transplantation shock.
    ///
    /// When severity is Moderate or Severe the global lockdown is engaged and
    /// a `proprioception.lockdown_engaged` event is written to the ledger.
    pub fn check_shock(&mut self) -> Result<ShockSeverity, SingularityError> {
        let current = FingerprintCollector::new()
            .collect()
            .map_err(|e| SingularityError::Proprioception(e.to_string()))?;

        let shock = self
            .shock_detector
            .detect(&current)
            .map_err(|e| SingularityError::Proprioception(e.to_string()))?;

        if shock.severity != ShockSeverity::None {
            self.ledger
                .append_signed_event(
                    self.keypair.as_ref(),
                    &self.namespace_key,
                    "proprioception.shock_detected",
                    serde_json::json!({
                        "severity": format!("{:?}", shock.severity),
                        "similarity_score": shock.similarity_score,
                        "differences": shock.differences,
                        "timestamp": Utc::now().to_rfc3339(),
                    }),
                    None,
                )
                .map_err(|e| SingularityError::Ledger(e.to_string()))?;
        }

        // Enforce lockdown for Moderate or Severe shock.
        let needs_lockdown = matches!(
            shock.severity,
            ShockSeverity::Moderate | ShockSeverity::Severe
        );

        if needs_lockdown {
            let reason = format!(
                "{:?} transplantation shock detected — differences: {}",
                shock.severity,
                shock.differences.join("; ")
            );

            // Engage the global lockdown flag.
            global_lockdown()
                .lock()
                .map_err(|e| {
                    SingularityError::Proprioception(format!("lockdown mutex poisoned: {}", e))
                })?
                .engage(shock.severity, reason.clone());

            // Write a dedicated ledger event.
            self.ledger
                .append_signed_event(
                    self.keypair.as_ref(),
                    &self.namespace_key,
                    "proprioception.lockdown_engaged",
                    serde_json::json!({
                        "severity": format!("{:?}", shock.severity),
                        "reason": reason,
                        "timestamp": Utc::now().to_rfc3339(),
                    }),
                    None,
                )
                .map_err(|e| SingularityError::Ledger(e.to_string()))?;
        }

        Ok(shock.severity)
    }

    /// System IV: Consume tokens from budget
    pub fn consume_tokens(&mut self, amount: u64) -> Result<(), SingularityError> {
        self.budget_tracker
            .consume(amount)
            .map_err(|e| SingularityError::Metabolic(e.to_string()))?;

        // Update hunger state
        let hunger_ratio = amount as f64 / 100000.0;
        self.hunger_state.starve(hunger_ratio);

        Ok(())
    }

    /// System IV: Feed tokens to restore budget
    pub fn feed_tokens(&mut self, amount: u64) {
        let feed_ratio = amount as f64 / 100000.0;
        self.hunger_state.feed(feed_ratio);
    }

    /// System IV: Check pain signals
    pub fn check_pain(&self) -> Vec<String> {
        self.pain_receptor
            .active_signals()
            .into_iter()
            .map(|t| format!("{:?}", t.signal_type))
            .collect()
    }

    /// System IV: Get current hunger degradation level
    pub fn hunger_degradation(&self) -> &zaion_metabolic::DegradationLevel {
        &self.hunger_state.degradation
    }

    /// System IV: Evaluate the metabolic policy given current token budget.
    ///
    /// Returns the action the runtime should enforce right now:
    /// - `Normal`             — below 80% utilization
    /// - `ReduceConcurrency`  — 80–95% utilization
    /// - `EmergencyThrottle`  — ≥ 95% utilization
    pub fn evaluate_metabolic_policy(&self) -> MetabolicAction {
        MetabolicPolicy::evaluate(&self.budget_tracker)
    }

    /// System V: Mark activity (resets idle timer)
    pub fn mark_activity(&mut self) {
        self.idle_timer.reset();
    }

    /// System V: Check if should ideate
    pub fn should_ideate(&mut self) -> Option<zaion_curiosity::IdeationPrompt> {
        let idle_seconds = self.idle_timer.time_since_activity().as_secs();

        if self.ideation_loop.should_ideate(idle_seconds) {
            if let Some(prompt) = self.ideation_loop.generate_prompt() {
                // Log ideation event
                let _ = self.ledger.append_signed_event(
                    self.keypair.as_ref(),
                    &self.namespace_key,
                    "curiosity.ideation_triggered",
                    serde_json::json!({
                        "category": format!("{:?}", prompt.category),
                        "idle_seconds": idle_seconds,
                    }),
                    None,
                );

                return Some(prompt);
            }
        }
        None
    }

    /// Get remaining token budget
    pub fn remaining_budget(&self) -> u64 {
        self.budget_tracker.remaining()
    }

    /// Get idle state
    pub fn idle_state(&self) -> zaion_curiosity::IdleState {
        self.idle_timer.state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_runtime() -> (SingularityRuntime, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let keypair = ZaionKeypair::generate();
        let namespace_key = NamespaceKey("test-namespace".to_string());
        let ledger_path = temp_dir.path().join("ledger.db");
        let ledger = Arc::new(EventLedger::new(&ledger_path));
        ledger.ensure().unwrap();

        let runtime =
            SingularityRuntime::new(temp_dir.path(), ledger, Arc::new(keypair), namespace_key)
                .unwrap();

        (runtime, temp_dir)
    }

    #[test]
    fn runtime_initializes() {
        let (runtime, _temp) = setup_runtime();
        assert_eq!(runtime.remaining_budget(), 100000);
    }

    #[test]
    fn system_prompt_generation() {
        let (runtime, _temp) = setup_runtime();
        let prompt = runtime.system_prompt();
        assert!(prompt.contains("<Zaion_Protocol>"));
    }

    #[test]
    fn token_consumption() {
        let (mut runtime, _temp) = setup_runtime();
        runtime.consume_tokens(1000).unwrap();
        assert_eq!(runtime.remaining_budget(), 99000);
    }

    #[test]
    fn activity_tracking() {
        let (mut runtime, _temp) = setup_runtime();
        runtime.mark_activity();
        let state = runtime.idle_state();
        assert!(matches!(state, zaion_curiosity::IdleState::Active));
    }

    #[test]
    fn baffle_filtering() {
        let (runtime, _temp) = setup_runtime();
        let filtered = runtime.filter_response("test response");
        assert_eq!(filtered, "test response");
    }

    #[test]
    fn shock_detection() {
        let (mut runtime, _temp) = setup_runtime();
        let severity = runtime.check_shock().unwrap();
        // First check should be None (same environment)
        assert_eq!(severity, ShockSeverity::None);
    }

    #[test]
    fn hunger_degradation() {
        let (runtime, _temp) = setup_runtime();
        assert!(matches!(
            runtime.hunger_degradation(),
            zaion_metabolic::DegradationLevel::None
        ));
    }
}
