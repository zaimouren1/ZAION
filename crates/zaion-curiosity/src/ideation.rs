//! Spontaneous Ideation Loop
//!
//! Generates exploratory prompts during idle periods
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeationPrompt {
    pub prompt: String,
    pub category: IdeationCategory,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum IdeationCategory {
    Exploration,   // Explore unknown parts of the system
    Optimization,  // Find performance improvements
    Refactoring,   // Identify code quality issues
    Documentation, // Suggest documentation improvements
    Testing,       // Propose new test cases
    Security,      // Look for security concerns
}

impl IdeationCategory {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Exploration,
            Self::Optimization,
            Self::Refactoring,
            Self::Documentation,
            Self::Testing,
            Self::Security,
        ]
    }

    pub fn random() -> Self {
        let categories = Self::all();
        let mut rng = rand::thread_rng();
        categories[rng.gen_range(0..categories.len())]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeationConfig {
    pub enabled: bool,
    pub min_idle_seconds: u64,
    pub categories: Vec<IdeationCategory>,
}

impl Default for IdeationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_idle_seconds: 300, // 5 minutes
            categories: IdeationCategory::all(),
        }
    }
}

pub struct IdeationLoop {
    config: IdeationConfig,
    last_ideation: Option<chrono::DateTime<chrono::Utc>>,
}

impl IdeationLoop {
    pub fn new(config: IdeationConfig) -> Self {
        Self {
            config,
            last_ideation: None,
        }
    }

    pub fn should_ideate(&self, idle_seconds: u64) -> bool {
        if !self.config.enabled {
            return false;
        }

        if idle_seconds < self.config.min_idle_seconds {
            return false;
        }

        // Check if enough time has passed since last ideation
        if let Some(last) = self.last_ideation {
            let elapsed = (chrono::Utc::now() - last).num_seconds() as u64;
            elapsed >= self.config.min_idle_seconds
        } else {
            true
        }
    }

    pub fn generate_prompt(&mut self) -> Option<IdeationPrompt> {
        if self.config.categories.is_empty() {
            return None;
        }

        let mut rng = rand::thread_rng();
        let category = self.config.categories[rng.gen_range(0..self.config.categories.len())];

        let prompt = match category {
            IdeationCategory::Exploration => {
                "What parts of this codebase haven't been explored recently?".to_string()
            }
            IdeationCategory::Optimization => {
                "Are there any performance bottlenecks that could be optimized?".to_string()
            }
            IdeationCategory::Refactoring => {
                "What code could be simplified or better structured?".to_string()
            }
            IdeationCategory::Documentation => {
                "What functionality lacks clear documentation?".to_string()
            }
            IdeationCategory::Testing => {
                "What edge cases or scenarios need better test coverage?".to_string()
            }
            IdeationCategory::Security => {
                "Are there any potential security vulnerabilities to address?".to_string()
            }
        };

        self.last_ideation = Some(chrono::Utc::now());

        Some(IdeationPrompt {
            prompt,
            category,
            generated_at: chrono::Utc::now(),
        })
    }

    pub fn reset(&mut self) {
        self.last_ideation = None;
    }
}

impl Default for IdeationLoop {
    fn default() -> Self {
        Self::new(IdeationConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ideation_config_defaults() {
        let config = IdeationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.min_idle_seconds, 300);
        assert_eq!(config.categories.len(), 6);
    }

    #[test]
    fn should_not_ideate_when_disabled() {
        let config = IdeationConfig {
            enabled: false,
            ..IdeationConfig::default()
        };

        let loop_instance = IdeationLoop::new(config);
        assert!(!loop_instance.should_ideate(1000));
    }

    #[test]
    fn should_not_ideate_below_threshold() {
        let loop_instance = IdeationLoop::default();
        assert!(!loop_instance.should_ideate(100));
    }

    #[test]
    fn should_ideate_above_threshold() {
        let loop_instance = IdeationLoop::default();
        assert!(loop_instance.should_ideate(400));
    }

    #[test]
    fn generates_prompt_with_category() {
        let mut loop_instance = IdeationLoop::default();
        let prompt = loop_instance.generate_prompt();

        assert!(prompt.is_some());
        let prompt = prompt.unwrap();
        assert!(!prompt.prompt.is_empty());
    }

    #[test]
    fn respects_min_time_between_ideations() {
        let mut loop_instance = IdeationLoop::default();

        // First ideation should work
        assert!(loop_instance.generate_prompt().is_some());

        // Second ideation immediately after should not trigger
        assert!(!loop_instance.should_ideate(400));
    }

    #[test]
    fn reset_clears_last_ideation() {
        let mut loop_instance = IdeationLoop::default();
        loop_instance.generate_prompt();

        assert!(loop_instance.last_ideation.is_some());

        loop_instance.reset();
        assert!(loop_instance.last_ideation.is_none());
    }

    #[test]
    fn all_categories_available() {
        let categories = IdeationCategory::all();
        assert_eq!(categories.len(), 6);
    }

    #[test]
    fn random_category_selection() {
        let cat1 = IdeationCategory::random();
        let cat2 = IdeationCategory::random();

        // Just verify they're valid categories
        assert!(IdeationCategory::all().contains(&cat1));
        assert!(IdeationCategory::all().contains(&cat2));
    }
}
