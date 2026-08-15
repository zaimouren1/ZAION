use crate::commands::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use zaion_memory::AutoMemoryExtractor;

/// Minimum extractor confidence required before an automatically observed
/// preference is allowed to enter the traceable store. Persona-derived
/// preference signals are emitted at 0.8 by `AutoMemoryExtractor`; this
/// threshold keeps low-signal noise out of the long-lived ledger.
const MIN_AUTO_LEARN_CONFIDENCE: f32 = 0.7;

/// Source tag stamped on entries written by the automatic learning loop.
const AUTO_LEARN_SOURCE: &str = "auto-learned";

/// Source tag stamped on entries written explicitly via the CLI. Manual
/// intent always wins: the learning loop never overwrites these.
const MANUAL_SOURCE: &str = "manual-cli";

/// Key prefix the extractor uses for preference signals. Only candidates under
/// this namespace are eligible for the automatic learning loop.
const PREFERENCE_KEY_PREFIX: &str = "preference";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceEntry {
    pub key: String,
    pub value: String,
    pub source: String,
    pub updated_at: String,
    pub evidence_hash: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PreferenceStore {
    pub learning_enabled: bool,
    pub entries: Vec<PreferenceEntry>,
}

impl PreferenceStore {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("preferences.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn set(&mut self, key: &str, value: &str, source: &str) {
        let entry = PreferenceEntry {
            key: key.to_string(),
            value: value.to_string(),
            source: source.to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            evidence_hash: preference_hash(key, value, source),
        };
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.key == key) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
        self.entries.sort_by(|a, b| a.key.cmp(&b.key));
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.key != key);
        before != self.entries.len()
    }

    /// Look up an entry by key without cloning.
    pub fn get(&self, key: &str) -> Option<&PreferenceEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    /// Absorb preference signals automatically observed from a conversation
    /// turn. This is the runtime half of the self-evolution loop: the wizard
    /// only flips `learning_enabled`; this method is what actually turns that
    /// switch into traceable, low-noise ledger growth.
    ///
    /// Contract (paradigm: control plane, not chat-only memory):
    ///   * No-op unless `learning_enabled` is true — the switch genuinely gates
    ///     the path instead of being a dead flag.
    ///   * Only `preference.*` candidates above [`MIN_AUTO_LEARN_CONFIDENCE`]
    ///     are eligible; everything else is left to the typed-memory layer.
    ///   * Manual intent is sacred: an entry whose source is [`MANUAL_SOURCE`]
    ///     is never overwritten by the learner.
    ///   * Idempotent: re-observing an identical (key,value) pair already in the
    ///     ledger does not churn the entry or its evidence hash.
    ///
    /// Returns the keys that were created or updated, so callers can surface a
    /// transparent "learned: …" notice to the user.
    pub fn absorb_turn(&mut self, user_content: &str, assistant_content: &str) -> Vec<String> {
        if !self.learning_enabled {
            return Vec::new();
        }

        let extraction =
            AutoMemoryExtractor::extract_from_turn(user_content, assistant_content, "preference");

        let mut changed = Vec::new();
        for candidate in extraction.candidates {
            if !candidate.key.starts_with(PREFERENCE_KEY_PREFIX) {
                continue;
            }
            if candidate.confidence < MIN_AUTO_LEARN_CONFIDENCE {
                continue;
            }
            let value = candidate.content.trim();
            if candidate.key.trim().is_empty() || value.is_empty() {
                continue;
            }

            if let Some(existing) = self.get(&candidate.key) {
                // Manual entries win and identical observations are no-ops.
                if existing.source == MANUAL_SOURCE || existing.value == value {
                    continue;
                }
            }

            self.set(candidate.key.trim(), value, AUTO_LEARN_SOURCE);
            changed.push(candidate.key.trim().to_string());
        }
        changed
    }
}

fn preference_hash(key: &str, value: &str, source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn cmd_preference(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => preference_show(),
        "set" => {
            let key = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion preference set <key> <value>".into()))?;
            let value = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion preference set <key> <value>".into()))?;
            preference_set(key, value)
        }
        "unset" => {
            let key = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion preference unset <key>".into()))?;
            preference_unset(key)
        }
        other => Err(CliError::Usage(format!(
            "unknown preference subcommand: {}. Use: show, set, unset",
            other
        ))),
    }
}

fn preference_show() -> Result<(), CliError> {
    let store = PreferenceStore::load();
    println!("preferences");
    println!("  path             : {}", PreferenceStore::path().display());
    println!("  learning_enabled : {}", store.learning_enabled);
    println!("  entries          : {}", store.entries.len());
    for entry in &store.entries {
        println!(
            "  {} = {} (source={}, hash={})",
            entry.key, entry.value, entry.source, entry.evidence_hash
        );
    }
    Ok(())
}

fn preference_set(key: &str, value: &str) -> Result<(), CliError> {
    if key.trim().is_empty() || value.trim().is_empty() {
        return Err(CliError::Usage(
            "preference key and value must not be empty".into(),
        ));
    }
    let mut store = PreferenceStore::load();
    store.set(key.trim(), value.trim(), MANUAL_SOURCE);
    store.save().map_err(CliError::Usage)?;
    println!("preference set");
    println!("  {} = {}", key.trim(), value.trim());
    println!("  source = {}", MANUAL_SOURCE);
    Ok(())
}

fn preference_unset(key: &str) -> Result<(), CliError> {
    let mut store = PreferenceStore::load();
    let removed = store.remove(key.trim());
    store.save().map_err(CliError::Usage)?;
    if removed {
        println!("preference unset: {}", key.trim());
    } else {
        println!("preference not found: {}", key.trim());
    }
    Ok(())
}

pub fn enable_learning_from_suggestion() -> Result<(), String> {
    let mut store = PreferenceStore::load();
    store.learning_enabled = true;
    store.save()
}

/// Runtime entry point for the self-evolution loop. Called once per completed
/// conversation turn: loads the ledger, absorbs any high-confidence preference
/// signals observed in the exchange, and persists only when something changed.
///
/// Returns the keys that were learned this turn (empty when learning is off or
/// nothing crossed the confidence/novelty bar) so the caller can show a
/// transparent notice. Persistence failures are surfaced to the caller rather
/// than silently swallowed.
pub fn learn_from_turn(user_content: &str, assistant_content: &str) -> Result<Vec<String>, String> {
    let mut store = PreferenceStore::load();
    let learned = store.absorb_turn(user_content, assistant_content);
    if !learned.is_empty() {
        store.save()?;
    }
    Ok(learned)
}

#[cfg(test)]
mod preference_tests {
    use super::*;

    fn store_with_learning(enabled: bool) -> PreferenceStore {
        PreferenceStore {
            learning_enabled: enabled,
            entries: Vec::new(),
        }
    }

    #[test]
    fn evidence_hash_is_stable_and_distinct() {
        let h1 = preference_hash("k", "v", "manual-cli");
        let h2 = preference_hash("k", "v", "manual-cli");
        let h3 = preference_hash("k", "v2", "manual-cli");
        assert_eq!(h1, h2, "same inputs must hash identically");
        assert_ne!(h1, h3, "different value must change the evidence hash");
    }

    #[test]
    fn set_inserts_then_updates_in_place() {
        let mut store = store_with_learning(false);
        store.set("preference.theme", "dark", MANUAL_SOURCE);
        assert_eq!(store.entries.len(), 1);
        // Same key updates the existing slot rather than appending a duplicate.
        store.set("preference.theme", "light", MANUAL_SOURCE);
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.get("preference.theme").unwrap().value, "light");
    }

    #[test]
    fn absorb_is_noop_when_learning_disabled() {
        let mut store = store_with_learning(false);
        let learned = store.absorb_turn("I prefer dark mode in my editor", "Noted.");
        assert!(learned.is_empty(), "disabled switch must gate the loop");
        assert!(store.entries.is_empty());
    }

    #[test]
    fn absorb_learns_preference_signal_when_enabled() {
        let mut store = store_with_learning(true);
        let learned = store.absorb_turn("I prefer dark mode in my editor", "Noted.");
        assert_eq!(learned.len(), 1, "one preference signal should be absorbed");
        let key = &learned[0];
        assert!(key.starts_with(PREFERENCE_KEY_PREFIX));
        let entry = store.get(key).unwrap();
        assert_eq!(entry.source, AUTO_LEARN_SOURCE);
        assert!(
            !entry.evidence_hash.is_empty(),
            "auto entry must carry provenance"
        );
    }

    #[test]
    fn absorb_never_overwrites_manual_entry() {
        let mut store = store_with_learning(true);
        // First learn the key automatically to discover its exact form.
        let learned = store.absorb_turn("I prefer dark mode in my editor", "ok");
        let key = learned[0].clone();
        // User pins it manually to a different value.
        store.set(&key, "user-pinned-value", MANUAL_SOURCE);
        // A fresh observation must NOT clobber the manual choice.
        let again = store.absorb_turn("I prefer dark mode in my editor", "ok");
        assert!(again.is_empty(), "manual intent must win over the learner");
        assert_eq!(store.get(&key).unwrap().value, "user-pinned-value");
        assert_eq!(store.get(&key).unwrap().source, MANUAL_SOURCE);
    }

    #[test]
    fn absorb_is_idempotent_for_identical_observation() {
        let mut store = store_with_learning(true);
        let first = store.absorb_turn("I prefer dark mode in my editor", "ok");
        assert_eq!(first.len(), 1);
        // Same exact signal again -> no churn.
        let second = store.absorb_turn("I prefer dark mode in my editor", "ok");
        assert!(second.is_empty(), "re-observing the same value is a no-op");
        assert_eq!(store.entries.len(), 1);
    }

    #[test]
    fn remove_reports_presence() {
        let mut store = store_with_learning(false);
        store.set("preference.theme", "dark", MANUAL_SOURCE);
        assert!(store.remove("preference.theme"), "existing key removed");
        assert!(
            !store.remove("preference.theme"),
            "absent key reports false"
        );
    }
}
