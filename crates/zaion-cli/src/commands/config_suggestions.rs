use crate::commands::activity::ActivityConfig;
use crate::commands::identity::{IdentityContinuityStore, IdentityProfile};
use crate::commands::preference::{enable_learning_from_suggestion, PreferenceStore};
use crate::commands::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSuggestion {
    pub id: String,
    pub title: String,
    pub reason: String,
    pub command_preview: String,
    pub requires_value: bool,
    pub requires_cost_ack: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedSuggestionEvent {
    pub sequence: u64,
    pub suggestion_id: String,
    pub applied_at: String,
    pub value_hash: Option<String>,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConfigSuggestionLog {
    pub applied: Vec<AppliedSuggestionEvent>,
}

impl ConfigSuggestionLog {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("config-suggestions.toml")
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

    pub fn append(&mut self, suggestion_id: &str, value: Option<&str>) -> Result<(), String> {
        let sequence = self.applied.len() as u64 + 1;
        let previous_hash = self
            .applied
            .last()
            .map(|event| event.hash.clone())
            .unwrap_or_else(|| "GENESIS".to_string());
        let value_hash = value.map(hash_text);
        let mut event = AppliedSuggestionEvent {
            sequence,
            suggestion_id: suggestion_id.to_string(),
            applied_at: chrono::Utc::now().to_rfc3339(),
            value_hash,
            previous_hash,
            hash: String::new(),
        };
        event.hash = hash_event(&event);
        self.applied.push(event);
        self.save()
    }
}

fn hash_event(event: &AppliedSuggestionEvent) -> String {
    hash_text(&format!(
        "{}|{}|{}|{}|{}",
        event.sequence,
        event.suggestion_id,
        event.applied_at,
        event.value_hash.as_deref().unwrap_or("-"),
        event.previous_hash
    ))
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn cmd_config_suggest(_args: &[String]) -> Result<(), CliError> {
    let suggestions = default_suggestions();
    println!("config suggestions");
    println!("  path : {}", ConfigSuggestionLog::path().display());
    println!("  note : optional settings stay out of onboard and require explicit apply");
    for suggestion in &suggestions {
        println!();
        println!("  id       : {}", suggestion.id);
        println!("  title    : {}", suggestion.title);
        println!("  reason   : {}", suggestion.reason);
        println!("  preview  : {}", suggestion.command_preview);
        println!("  value    : {}", suggestion.requires_value);
        println!("  cost_ack : {}", suggestion.requires_cost_ack);
    }
    Ok(())
}

pub fn cmd_config_apply_suggestion(args: &[String]) -> Result<(), CliError> {
    let id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion config apply-suggestion <id>".into()))?;
    let value = arg_value(args, "--value");
    let ack_cost = args.iter().any(|arg| arg == "--ack-cost");
    let suggestion = default_suggestions()
        .into_iter()
        .find(|suggestion| suggestion.id == *id)
        .ok_or_else(|| CliError::Usage(format!("unknown config suggestion: {}", id)))?;

    if suggestion.requires_value && value.unwrap_or("").trim().is_empty() {
        return Err(CliError::Usage(format!(
            "suggestion {} requires --value <value>",
            suggestion.id
        )));
    }
    if suggestion.requires_cost_ack && !ack_cost {
        println!("WARNING: this suggestion may consume many tokens or network calls when enabled.");
        return Err(CliError::Usage(format!(
            "suggestion {} requires --ack-cost",
            suggestion.id
        )));
    }

    match suggestion.id.as_str() {
        "identity.rename" => apply_rename(value.unwrap())?,
        "preference.learning" => enable_learning_from_suggestion().map_err(CliError::Usage)?,
        "activity.suggest_only" => apply_activity_suggest_only()?,
        other => {
            return Err(CliError::Usage(format!(
                "suggestion {} has no apply handler",
                other
            )))
        }
    }

    let mut log = ConfigSuggestionLog::load();
    log.append(&suggestion.id, value).map_err(CliError::Usage)?;
    println!("config suggestion applied");
    println!("  id     : {}", suggestion.id);
    println!("  trace  : {}", ConfigSuggestionLog::path().display());
    Ok(())
}

fn default_suggestions() -> Vec<ConfigSuggestion> {
    vec![
        ConfigSuggestion {
            id: "identity.rename".to_string(),
            title: "Give Zaion a user-facing name".to_string(),
            reason: "Display name can be changed conversationally without changing cryptographic identity"
                .to_string(),
            command_preview: "zaion config apply-suggestion identity.rename --value <name>"
                .to_string(),
            requires_value: true,
            requires_cost_ack: false,
        },
        ConfigSuggestion {
            id: "preference.learning".to_string(),
            title: "Allow long-term preference learning".to_string(),
            reason: "Preference learning helps activity continuity choose non-hardcoded topics later"
                .to_string(),
            command_preview: "zaion config apply-suggestion preference.learning".to_string(),
            requires_value: false,
            requires_cost_ack: false,
        },
        ConfigSuggestion {
            id: "activity.suggest_only".to_string(),
            title: "Prepare activity continuity in suggest-only mode".to_string(),
            reason: "Zaion may birth bounded local thought drafts while avoiding network/tool autonomy"
                .to_string(),
            command_preview:
                "zaion config apply-suggestion activity.suggest_only --ack-cost".to_string(),
            requires_value: false,
            requires_cost_ack: true,
        },
    ]
}

fn apply_rename(name: &str) -> Result<(), CliError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage("identity name must not be empty".into()));
    }
    let mut profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    profile.display_name = trimmed.to_string();
    profile.updated_at = chrono::Utc::now().to_rfc3339();
    profile.save().map_err(CliError::Usage)?;
    let mut continuity = IdentityContinuityStore::load();
    continuity
        .append_event("identity.renamed", &profile, "config suggestion applied")
        .map_err(CliError::Usage)?;
    Ok(())
}

fn apply_activity_suggest_only() -> Result<(), CliError> {
    let mut activity = ActivityConfig::load();
    activity.enabled = true;
    activity.paused = false;
    activity.mode = "suggest-only".to_string();
    activity.warning_acknowledged = true;
    if activity.daily_token_budget == 0 {
        activity.daily_token_budget = 2000;
    }
    activity.update_timestamp();
    activity.save().map_err(CliError::Usage)?;

    let mut prefs = PreferenceStore::load();
    if prefs
        .entries
        .iter()
        .all(|entry| entry.key != "activity_mode_preference")
    {
        prefs.set(
            "activity_mode_preference",
            "suggest-only local drafts",
            "config-suggestion",
        );
        prefs.save().map_err(CliError::Usage)?;
    }
    Ok(())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}
