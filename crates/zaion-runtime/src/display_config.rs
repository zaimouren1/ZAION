//! Display configuration persistence for slash commands
//!
//! Hermes architecture: /verbose, /statusbar, /skin, /reasoning commands
//! persist display preferences across sessions.
//!
//! Integration points:
//! - SlashCommand execution updates display config
//! - Config stored in ZAION_HOME/display.toml
//! - Runtime loads config on startup

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Display configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisplayConfig {
    /// Verbose mode: off, new, all, verbose
    pub verbose_mode: VerboseMode,
    /// Statusbar visibility
    pub statusbar_enabled: bool,
    /// UI skin/theme
    pub skin: String,
    /// Reasoning display mode
    pub reasoning_mode: ReasoningMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerboseMode {
    Off,
    New,
    All,
    Verbose,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMode {
    Show,
    Hide,
    Effort,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            verbose_mode: VerboseMode::New,
            statusbar_enabled: true,
            skin: "default".to_string(),
            reasoning_mode: ReasoningMode::Show,
        }
    }
}

impl DisplayConfig {
    /// Load display config from file
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read display config: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Failed to parse display config: {}", e))
    }

    /// Save display config to file
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize display config: {}", e))?;

        fs::write(path, content).map_err(|e| format!("Failed to write display config: {}", e))
    }

    /// Get default config path
    pub fn default_path() -> Result<PathBuf, String> {
        Ok(zaion_paths::display_config_path())
    }

    /// Toggle verbose mode (cycles through: off → new → all → verbose → off)
    pub fn toggle_verbose(&mut self) {
        self.verbose_mode = match self.verbose_mode {
            VerboseMode::Off => VerboseMode::New,
            VerboseMode::New => VerboseMode::All,
            VerboseMode::All => VerboseMode::Verbose,
            VerboseMode::Verbose => VerboseMode::Off,
        };
    }

    /// Toggle statusbar
    pub fn toggle_statusbar(&mut self) {
        self.statusbar_enabled = !self.statusbar_enabled;
    }

    /// Set skin
    pub fn set_skin(&mut self, skin: String) {
        self.skin = skin;
    }

    /// Set reasoning mode
    pub fn set_reasoning(&mut self, mode: ReasoningMode) {
        self.reasoning_mode = mode;
    }

    /// Parse reasoning action string
    pub fn parse_reasoning_action(action: &str) -> Option<ReasoningMode> {
        match action.to_lowercase().as_str() {
            "show" => Some(ReasoningMode::Show),
            "hide" => Some(ReasoningMode::Hide),
            "effort" => Some(ReasoningMode::Effort),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = DisplayConfig::default();
        assert_eq!(config.verbose_mode, VerboseMode::New);
        assert!(config.statusbar_enabled);
        assert_eq!(config.skin, "default");
        assert_eq!(config.reasoning_mode, ReasoningMode::Show);
    }

    #[test]
    fn test_toggle_verbose() {
        let mut config = DisplayConfig::default();
        assert_eq!(config.verbose_mode, VerboseMode::New);

        config.toggle_verbose();
        assert_eq!(config.verbose_mode, VerboseMode::All);

        config.toggle_verbose();
        assert_eq!(config.verbose_mode, VerboseMode::Verbose);

        config.toggle_verbose();
        assert_eq!(config.verbose_mode, VerboseMode::Off);

        config.toggle_verbose();
        assert_eq!(config.verbose_mode, VerboseMode::New);
    }

    #[test]
    fn test_toggle_statusbar() {
        let mut config = DisplayConfig::default();
        assert!(config.statusbar_enabled);

        config.toggle_statusbar();
        assert!(!config.statusbar_enabled);

        config.toggle_statusbar();
        assert!(config.statusbar_enabled);
    }

    #[test]
    fn test_set_skin() {
        let mut config = DisplayConfig::default();
        config.set_skin("dark".to_string());
        assert_eq!(config.skin, "dark");
    }

    #[test]
    fn test_set_reasoning() {
        let mut config = DisplayConfig::default();
        config.set_reasoning(ReasoningMode::Hide);
        assert_eq!(config.reasoning_mode, ReasoningMode::Hide);
    }

    #[test]
    fn test_parse_reasoning_action() {
        assert_eq!(
            DisplayConfig::parse_reasoning_action("show"),
            Some(ReasoningMode::Show)
        );
        assert_eq!(
            DisplayConfig::parse_reasoning_action("hide"),
            Some(ReasoningMode::Hide)
        );
        assert_eq!(
            DisplayConfig::parse_reasoning_action("effort"),
            Some(ReasoningMode::Effort)
        );
        assert_eq!(DisplayConfig::parse_reasoning_action("invalid"), None);
    }

    #[test]
    fn test_save_and_load() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let config = DisplayConfig {
            verbose_mode: VerboseMode::All,
            statusbar_enabled: false,
            skin: "dark".to_string(),
            reasoning_mode: ReasoningMode::Hide,
        };

        config.save(path).unwrap();

        let loaded = DisplayConfig::load(path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let path = Path::new("/nonexistent/display.toml");
        let config = DisplayConfig::load(path).unwrap();
        assert_eq!(config, DisplayConfig::default());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = DisplayConfig {
            verbose_mode: VerboseMode::Verbose,
            statusbar_enabled: false,
            skin: "light".to_string(),
            reasoning_mode: ReasoningMode::Effort,
        };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: DisplayConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed, config);
    }
}
